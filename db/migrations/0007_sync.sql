CREATE TABLE sync_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    event_sequence bigint GENERATED ALWAYS AS IDENTITY,
    project_id uuid NOT NULL,
    stream_id uuid NOT NULL,
    actor_identity_id uuid NOT NULL,
    actor_device_id uuid NOT NULL,
    actor_device_key_version integer NOT NULL,
    device_sequence bigint NOT NULL CHECK (device_sequence > 0),
    client_event_id uuid NOT NULL,
    event_kind text NOT NULL CHECK (length(event_kind) BETWEEN 1 AND 128),
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    encrypted_payload bytea NOT NULL,
    previous_hash bytea,
    event_hash bytea NOT NULL,
    signature bytea NOT NULL,
    client_created_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT sync_events_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT sync_events_actor_membership_fk
        FOREIGN KEY (project_id, actor_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT sync_events_actor_device_key_fk
        FOREIGN KEY (actor_identity_id, actor_device_id, actor_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT sync_events_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT sync_events_project_sequence_unique UNIQUE (project_id, event_sequence),
    CONSTRAINT sync_events_project_id_sequence_unique UNIQUE (project_id, id, event_sequence),
    CONSTRAINT sync_events_device_sequence_unique
        UNIQUE (project_id, actor_device_id, device_sequence),
    CONSTRAINT sync_events_client_event_unique
        UNIQUE (project_id, actor_device_id, client_event_id),
    CONSTRAINT sync_events_payload_nonempty CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT sync_events_previous_hash_nonempty
        CHECK (previous_hash IS NULL OR octet_length(previous_hash) >= 16),
    CONSTRAINT sync_events_hash_nonempty CHECK (octet_length(event_hash) >= 16),
    CONSTRAINT sync_events_signature_nonempty CHECK (octet_length(signature) > 0),
    CONSTRAINT sync_events_hash_progress
        CHECK (previous_hash IS NULL OR previous_hash <> event_hash)
);

CREATE UNIQUE INDEX sync_events_stream_hash_unique
    ON sync_events (project_id, stream_id, event_hash);
CREATE INDEX sync_events_project_cursor_idx
    ON sync_events (project_id, event_sequence);
CREATE INDEX sync_events_stream_cursor_idx
    ON sync_events (project_id, stream_id, event_sequence);

CREATE OR REPLACE FUNCTION sprout_private.validate_sync_event_chain()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    expected_previous_hash bytea;
BEGIN
    PERFORM pg_advisory_xact_lock(
        hashtextextended(NEW.project_id::text || ':' || NEW.stream_id::text, 0)
    );

    SELECT event_hash INTO expected_previous_hash
    FROM sync_events
    WHERE project_id = NEW.project_id
      AND stream_id = NEW.stream_id
    ORDER BY event_sequence DESC
    LIMIT 1;

    IF NEW.previous_hash IS DISTINCT FROM expected_previous_hash THEN
        RAISE EXCEPTION 'sync event previous hash does not match stream head'
            USING ERRCODE = '40001';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER sync_events_validate_chain
BEFORE INSERT ON sync_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_sync_event_chain();

CREATE TRIGGER sync_events_immutable
BEFORE UPDATE OR DELETE ON sync_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE sync_idempotency (
    project_id uuid NOT NULL,
    actor_device_id uuid NOT NULL,
    idempotency_key uuid NOT NULL,
    request_hash bytea NOT NULL,
    sync_event_id uuid NOT NULL,
    event_sequence bigint NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    PRIMARY KEY (project_id, actor_device_id, idempotency_key),
    CONSTRAINT sync_idempotency_event_fk
        FOREIGN KEY (project_id, sync_event_id, event_sequence)
        REFERENCES sync_events (project_id, id, event_sequence)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT sync_idempotency_request_hash_nonempty CHECK (octet_length(request_hash) >= 16),
    CONSTRAINT sync_idempotency_expiry_after_creation CHECK (expires_at > created_at)
);

CREATE INDEX sync_idempotency_expiry_idx ON sync_idempotency (expires_at);
CREATE TRIGGER sync_idempotency_no_update
BEFORE UPDATE ON sync_idempotency
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE sync_snapshots (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    stream_id uuid NOT NULL,
    through_event_id uuid NOT NULL,
    through_event_sequence bigint NOT NULL,
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    encrypted_payload bytea NOT NULL,
    snapshot_hash bytea NOT NULL,
    signature bytea NOT NULL,
    created_by_identity_id uuid NOT NULL,
    created_by_device_id uuid NOT NULL,
    created_by_device_key_version integer NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT sync_snapshots_event_fk
        FOREIGN KEY (project_id, through_event_id, through_event_sequence)
        REFERENCES sync_events (project_id, id, event_sequence)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT sync_snapshots_creator_membership_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT sync_snapshots_creator_device_key_fk
        FOREIGN KEY (created_by_identity_id, created_by_device_id, created_by_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT sync_snapshots_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT sync_snapshots_stream_cursor_unique
        UNIQUE (project_id, stream_id, through_event_sequence),
    CONSTRAINT sync_snapshots_payload_nonempty CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT sync_snapshots_hash_nonempty CHECK (octet_length(snapshot_hash) >= 16),
    CONSTRAINT sync_snapshots_signature_nonempty CHECK (octet_length(signature) > 0)
);

CREATE INDEX sync_snapshots_latest_idx
    ON sync_snapshots (project_id, stream_id, through_event_sequence DESC);
CREATE TRIGGER sync_snapshots_immutable
BEFORE UPDATE OR DELETE ON sync_snapshots
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();
