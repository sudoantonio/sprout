CREATE TABLE retention_policies (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    data_class text NOT NULL
        CHECK (data_class IN (
            'sync_event', 'snapshot', 'notification', 'export',
            'file_blob', 'audit', 'idempotency', 'domain_snapshot'
        )),
    retain_for interval NOT NULL CHECK (retain_for > interval '0 seconds'),
    legal_hold boolean NOT NULL DEFAULT false,
    encrypted_policy_metadata bytea,
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT retention_policies_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_policies_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_policies_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT retention_policies_class_unique UNIQUE (project_id, data_class),
    CONSTRAINT retention_policies_metadata_nonempty
        CHECK (encrypted_policy_metadata IS NULL OR octet_length(encrypted_policy_metadata) > 0)
);

CREATE TRIGGER retention_policies_touch_updated_at
BEFORE UPDATE ON retention_policies
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE retention_leases (
    project_id uuid NOT NULL,
    lease_scope text NOT NULL CHECK (length(lease_scope) BETWEEN 1 AND 128),
    partition_key text NOT NULL CHECK (length(partition_key) BETWEEN 1 AND 256),
    lease_owner uuid NOT NULL,
    lease_token uuid NOT NULL,
    acquired_at timestamptz NOT NULL DEFAULT now(),
    heartbeat_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    PRIMARY KEY (project_id, lease_scope, partition_key),
    CONSTRAINT retention_leases_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_leases_expiry_after_acquisition CHECK (expires_at > acquired_at),
    CONSTRAINT retention_leases_heartbeat_after_acquisition CHECK (heartbeat_at >= acquired_at)
);

CREATE INDEX retention_leases_expiry_idx ON retention_leases (expires_at);

CREATE TABLE notifications (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    recipient_identity_id uuid NOT NULL,
    notification_kind text NOT NULL CHECK (length(notification_kind) BETWEEN 1 AND 128),
    delivery_channel text NOT NULL
        CHECK (delivery_channel IN ('in_app', 'push', 'email_bridge', 'webhook')),
    encrypted_payload bytea NOT NULL,
    deduplication_key bytea,
    state text NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'delivered', 'read', 'dismissed', 'failed')),
    scheduled_at timestamptz NOT NULL DEFAULT now(),
    created_at timestamptz NOT NULL DEFAULT now(),
    delivered_at timestamptz,
    read_at timestamptz,
    dismissed_at timestamptz,
    CONSTRAINT notifications_recipient_fk
        FOREIGN KEY (project_id, recipient_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT notifications_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT notifications_payload_nonempty CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT notifications_dedupe_nonempty
        CHECK (deduplication_key IS NULL OR octet_length(deduplication_key) >= 16),
    CONSTRAINT notifications_delivered_state
        CHECK (delivered_at IS NULL OR state IN ('delivered', 'read', 'dismissed')),
    CONSTRAINT notifications_read_state CHECK (read_at IS NULL OR state = 'read'),
    CONSTRAINT notifications_dismissed_state CHECK (dismissed_at IS NULL OR state = 'dismissed')
);

CREATE UNIQUE INDEX notifications_deduplication_unique
    ON notifications (project_id, recipient_identity_id, deduplication_key)
    WHERE deduplication_key IS NOT NULL;
CREATE INDEX notifications_delivery_work_idx
    ON notifications (scheduled_at, created_at)
    WHERE state = 'pending';
CREATE INDEX notifications_recipient_idx
    ON notifications (project_id, recipient_identity_id, created_at DESC);

CREATE TABLE exports (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    requested_by_identity_id uuid NOT NULL,
    idempotency_key uuid NOT NULL,
    export_kind text NOT NULL
        CHECK (export_kind IN ('project_archive', 'audit', 'tasks', 'questionnaire', 'user_data')),
    encrypted_request bytea NOT NULL,
    encrypted_result_metadata bytea,
    result_blob_id uuid,
    state text NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'running', 'succeeded', 'failed', 'expired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    expires_at timestamptz,
    CONSTRAINT exports_requester_fk
        FOREIGN KEY (project_id, requested_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT exports_result_blob_fk
        FOREIGN KEY (project_id, result_blob_id) REFERENCES file_blobs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT exports_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT exports_idempotency_unique
        UNIQUE (project_id, requested_by_identity_id, idempotency_key),
    CONSTRAINT exports_request_nonempty CHECK (octet_length(encrypted_request) > 0),
    CONSTRAINT exports_result_nonempty
        CHECK (encrypted_result_metadata IS NULL OR octet_length(encrypted_result_metadata) > 0),
    CONSTRAINT exports_completion_state
        CHECK ((state IN ('succeeded', 'failed')) = (completed_at IS NOT NULL)),
    CONSTRAINT exports_success_result
        CHECK (state <> 'succeeded' OR result_blob_id IS NOT NULL),
    CONSTRAINT exports_expiry_after_creation
        CHECK (expires_at IS NULL OR expires_at > created_at)
);

CREATE INDEX exports_work_idx ON exports (state, created_at)
    WHERE state IN ('pending', 'running');
CREATE INDEX exports_requester_idx
    ON exports (project_id, requested_by_identity_id, created_at DESC);
CREATE TRIGGER exports_touch_updated_at
BEFORE UPDATE ON exports
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE audit_log (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    audit_sequence bigint GENERATED ALWAYS AS IDENTITY,
    project_id uuid NOT NULL,
    actor_identity_id uuid,
    actor_device_id uuid,
    actor_device_key_version integer,
    action text NOT NULL CHECK (length(action) BETWEEN 1 AND 128),
    target_kind text NOT NULL CHECK (length(target_kind) BETWEEN 1 AND 128),
    target_id uuid,
    encrypted_detail bytea NOT NULL,
    previous_hash bytea,
    entry_hash bytea NOT NULL,
    signature bytea NOT NULL,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT audit_log_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT audit_log_actor_membership_fk
        FOREIGN KEY (project_id, actor_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT audit_log_actor_device_key_fk
        FOREIGN KEY (actor_identity_id, actor_device_id, actor_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT audit_log_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT audit_log_project_sequence_unique UNIQUE (project_id, audit_sequence),
    CONSTRAINT audit_log_actor_shape CHECK (
        (actor_device_id IS NULL AND actor_device_key_version IS NULL)
        OR (actor_identity_id IS NOT NULL
            AND actor_device_id IS NOT NULL
            AND actor_device_key_version IS NOT NULL)
    ),
    CONSTRAINT audit_log_detail_nonempty CHECK (octet_length(encrypted_detail) > 0),
    CONSTRAINT audit_log_previous_hash_nonempty
        CHECK (previous_hash IS NULL OR octet_length(previous_hash) >= 16),
    CONSTRAINT audit_log_hash_nonempty CHECK (octet_length(entry_hash) >= 16),
    CONSTRAINT audit_log_signature_nonempty CHECK (octet_length(signature) > 0)
);

CREATE UNIQUE INDEX audit_log_hash_unique ON audit_log (project_id, entry_hash);
CREATE INDEX audit_log_cursor_idx ON audit_log (project_id, audit_sequence);

CREATE OR REPLACE FUNCTION sprout_private.validate_audit_chain()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    expected_previous_hash bytea;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(NEW.project_id::text, 1));

    SELECT entry_hash INTO expected_previous_hash
    FROM audit_log
    WHERE project_id = NEW.project_id
    ORDER BY audit_sequence DESC
    LIMIT 1;

    IF NEW.previous_hash IS DISTINCT FROM expected_previous_hash THEN
        RAISE EXCEPTION 'audit entry previous hash does not match project audit head'
            USING ERRCODE = '40001';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER audit_log_validate_chain
BEFORE INSERT ON audit_log
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_audit_chain();
CREATE TRIGGER audit_log_immutable
BEFORE UPDATE OR DELETE ON audit_log
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE outbox (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    aggregate_kind text NOT NULL CHECK (length(aggregate_kind) BETWEEN 1 AND 128),
    aggregate_id uuid NOT NULL,
    event_kind text NOT NULL CHECK (length(event_kind) BETWEEN 1 AND 128),
    deduplication_key uuid NOT NULL,
    encrypted_payload bytea NOT NULL,
    available_at timestamptz NOT NULL DEFAULT now(),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    lock_owner uuid,
    lock_token uuid,
    locked_until timestamptz,
    delivered_at timestamptz,
    dead_lettered_at timestamptz,
    last_error_code text,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT outbox_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT outbox_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT outbox_deduplication_unique UNIQUE (project_id, deduplication_key),
    CONSTRAINT outbox_payload_nonempty CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT outbox_lock_shape CHECK (
        (lock_owner IS NULL AND lock_token IS NULL AND locked_until IS NULL)
        OR (lock_owner IS NOT NULL AND lock_token IS NOT NULL AND locked_until IS NOT NULL)
    ),
    CONSTRAINT outbox_terminal_shape
        CHECK (delivered_at IS NULL OR dead_lettered_at IS NULL)
);

CREATE INDEX outbox_delivery_work_idx
    ON outbox (available_at, created_at)
    WHERE delivered_at IS NULL AND dead_lettered_at IS NULL;
CREATE INDEX outbox_expired_locks_idx
    ON outbox (locked_until)
    WHERE locked_until IS NOT NULL AND delivered_at IS NULL AND dead_lettered_at IS NULL;
