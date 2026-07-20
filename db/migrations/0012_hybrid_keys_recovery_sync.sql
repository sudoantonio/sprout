-- HLR-06/07: experimental independent PQ/classical device packages,
-- append-only key transparency, n-of-n project recovery, and resource-bound sync.
-- Suite 32769 is not a claim of standardized hybrid encryption.

ALTER TABLE device_keys
    ADD COLUMN suite_version integer NOT NULL DEFAULT 0,
    ADD COLUMN generation bigint NOT NULL DEFAULT 0,
    ADD COLUMN previous_package_hash bytea,
    ADD COLUMN package_hash bytea,
    ADD COLUMN package_json bytea,
    ADD COLUMN x25519_key_id uuid,
    ADD COLUMN ml_kem_768_key_id uuid,
    ADD COLUMN ed25519_key_id uuid,
    ADD COLUMN ml_dsa_65_key_id uuid,
    ADD COLUMN x25519_public_key bytea,
    ADD COLUMN ml_kem_768_public_key bytea,
    ADD COLUMN ed25519_public_key bytea,
    ADD COLUMN ml_dsa_65_public_key bytea;

ALTER TABLE devices
    ADD COLUMN key_epoch bigint NOT NULL DEFAULT 1 CHECK (key_epoch > 0);

UPDATE device_keys
SET
    previous_package_hash = decode(repeat('00', 32), 'hex'),
    package_hash = digest(
        identity_id::text || ':' || device_id::text || ':' || key_version::text,
        'sha256'
    ),
    x25519_public_key = encryption_public_key,
    ed25519_public_key = signing_public_key;

ALTER TABLE device_keys
    ALTER COLUMN previous_package_hash SET NOT NULL,
    ALTER COLUMN package_hash SET NOT NULL,
    ALTER COLUMN x25519_public_key SET NOT NULL,
    ALTER COLUMN ed25519_public_key SET NOT NULL,
    ADD CONSTRAINT device_keys_suite_shape CHECK (
        (
            suite_version = 0
            AND ml_kem_768_public_key IS NULL
            AND ml_dsa_65_public_key IS NULL
        )
        OR (
            suite_version = 32769
            AND generation >= 0
            AND octet_length(previous_package_hash) = 32
            AND octet_length(package_hash) = 32
            AND package_json IS NOT NULL
            AND octet_length(package_json) > 0
            AND x25519_key_id IS NOT NULL
            AND ml_kem_768_key_id IS NOT NULL
            AND ed25519_key_id IS NOT NULL
            AND ml_dsa_65_key_id IS NOT NULL
            AND octet_length(x25519_public_key) = 32
            AND octet_length(ml_kem_768_public_key) = 1184
            AND octet_length(ed25519_public_key) = 32
            AND octet_length(ml_dsa_65_public_key) = 1952
        )
    ),
    ADD CONSTRAINT device_keys_package_hash_length
        CHECK (octet_length(package_hash) = 32),
    ADD CONSTRAINT device_keys_previous_package_hash_length
        CHECK (octet_length(previous_package_hash) = 32),
    ADD CONSTRAINT device_keys_device_generation_unique UNIQUE (device_id, generation),
    ADD CONSTRAINT device_keys_package_hash_unique UNIQUE (package_hash);

-- Public package lookup intentionally crosses identity RLS after validating
-- that both requester and candidate are active members of the project.
ALTER TABLE device_keys NO FORCE ROW LEVEL SECURITY;

CREATE FUNCTION sprout_private.validate_device_key_generation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    latest_generation bigint;
    latest_package_hash bytea;
BEGIN
    IF NEW.suite_version <> 32769 THEN
        RETURN NEW;
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended(NEW.device_id::text, 12));
    SELECT generation, package_hash
    INTO latest_generation, latest_package_hash
    FROM device_keys
    WHERE device_id = NEW.device_id AND suite_version = 32769
    ORDER BY generation DESC
    LIMIT 1;
    IF FOUND THEN
        IF NEW.generation <> latest_generation + 1
           OR NEW.previous_package_hash <> latest_package_hash
        THEN
            RAISE EXCEPTION 'device key package generation rollback or fork'
                USING ERRCODE = '40001';
        END IF;
    ELSIF NEW.generation <> 0
          OR NEW.previous_package_hash <> decode(repeat('00', 32), 'hex')
    THEN
        RAISE EXCEPTION 'initial device key package generation is invalid'
            USING ERRCODE = '40001';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER device_keys_validate_generation
BEFORE INSERT ON device_keys
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_device_key_generation();

CREATE FUNCTION sprout_private.validate_device_key_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.revoked_at IS NOT NULL
       OR NEW.revoked_at IS NULL
       OR NEW.id IS DISTINCT FROM OLD.id
       OR NEW.identity_id IS DISTINCT FROM OLD.identity_id
       OR NEW.device_id IS DISTINCT FROM OLD.device_id
       OR NEW.key_version IS DISTINCT FROM OLD.key_version
       OR NEW.encryption_public_key IS DISTINCT FROM OLD.encryption_public_key
       OR NEW.signing_public_key IS DISTINCT FROM OLD.signing_public_key
       OR NEW.key_attestation IS DISTINCT FROM OLD.key_attestation
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
       OR NEW.suite_version IS DISTINCT FROM OLD.suite_version
       OR NEW.generation IS DISTINCT FROM OLD.generation
       OR NEW.previous_package_hash IS DISTINCT FROM OLD.previous_package_hash
       OR NEW.package_hash IS DISTINCT FROM OLD.package_hash
       OR NEW.package_json IS DISTINCT FROM OLD.package_json
       OR NEW.x25519_key_id IS DISTINCT FROM OLD.x25519_key_id
       OR NEW.ml_kem_768_key_id IS DISTINCT FROM OLD.ml_kem_768_key_id
       OR NEW.ed25519_key_id IS DISTINCT FROM OLD.ed25519_key_id
       OR NEW.ml_dsa_65_key_id IS DISTINCT FROM OLD.ml_dsa_65_key_id
       OR NEW.x25519_public_key IS DISTINCT FROM OLD.x25519_public_key
       OR NEW.ml_kem_768_public_key IS DISTINCT FROM OLD.ml_kem_768_public_key
       OR NEW.ed25519_public_key IS DISTINCT FROM OLD.ed25519_public_key
       OR NEW.ml_dsa_65_public_key IS DISTINCT FROM OLD.ml_dsa_65_public_key
    THEN
        RAISE EXCEPTION 'device key packages are immutable except one-time revocation'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER device_keys_validate_update
BEFORE UPDATE ON device_keys
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_device_key_update();

CREATE FUNCTION sprout_private.active_project_device_keys(
    candidate_project_id uuid,
    candidate_identity_id uuid
)
RETURNS TABLE (
    identity_id uuid,
    device_id uuid,
    key_version integer,
    generation bigint,
    package_hash bytea,
    package_json bytea,
    x25519_public_key bytea,
    ml_kem_768_public_key bytea,
    ed25519_public_key bytea,
    ml_dsa_65_public_key bytea
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
    SELECT
        key.identity_id,
        key.device_id,
        key.key_version,
        key.generation,
        key.package_hash,
        key.package_json,
        key.x25519_public_key,
        key.ml_kem_768_public_key,
        key.ed25519_public_key,
        key.ml_dsa_65_public_key
    FROM device_keys key
    JOIN devices device
      ON device.identity_id = key.identity_id
     AND device.id = key.device_id
    WHERE key.identity_id = candidate_identity_id
      AND key.suite_version = 32769
      AND key.revoked_at IS NULL
      AND device.trust_state = 'trusted'
      AND device.retired_at IS NULL
      AND EXISTS (
          SELECT 1 FROM project_memberships requester
          WHERE requester.project_id = candidate_project_id
            AND requester.identity_id = sprout_private.current_identity_id()
            AND requester.state = 'active'
      )
      AND EXISTS (
          SELECT 1 FROM project_memberships candidate
          WHERE candidate.project_id = candidate_project_id
            AND candidate.identity_id = candidate_identity_id
            AND candidate.state = 'active'
      )
$$;

REVOKE ALL ON FUNCTION sprout_private.active_project_device_keys(uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.active_project_device_keys(uuid, uuid) TO PUBLIC;

CREATE TABLE device_key_transparency_log (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    log_sequence bigint GENERATED ALWAYS AS IDENTITY,
    identity_id uuid NOT NULL,
    device_id uuid NOT NULL,
    key_version integer NOT NULL,
    generation bigint NOT NULL,
    event_kind text NOT NULL
        CHECK (event_kind IN ('registered', 'rotated', 'revoked', 'recovery_revoked')),
    package_hash bytea NOT NULL CHECK (octet_length(package_hash) = 32),
    previous_entry_hash bytea CHECK (
        previous_entry_hash IS NULL OR octet_length(previous_entry_hash) = 32
    ),
    entry_hash bytea NOT NULL UNIQUE CHECK (octet_length(entry_hash) = 32),
    classical_signature bytea,
    post_quantum_signature bytea,
    authorization_reference uuid,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT device_key_transparency_key_fk
        FOREIGN KEY (identity_id, device_id, key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT device_key_transparency_signature_shape CHECK (
        (event_kind = 'registered' AND generation = 0
            AND classical_signature IS NULL AND post_quantum_signature IS NULL
            AND authorization_reference IS NULL)
        OR
        (event_kind IN ('rotated', 'revoked')
            AND octet_length(classical_signature) = 64
            AND post_quantum_signature IS NOT NULL
            AND octet_length(post_quantum_signature) > 0
            AND authorization_reference IS NULL)
        OR
        (event_kind = 'recovery_revoked'
            AND classical_signature IS NULL
            AND post_quantum_signature IS NULL
            AND authorization_reference IS NOT NULL)
    )
);

CREATE UNIQUE INDEX device_key_transparency_device_generation_event_idx
    ON device_key_transparency_log (device_id, generation, event_kind);
CREATE INDEX device_key_transparency_device_sequence_idx
    ON device_key_transparency_log (device_id, log_sequence);

CREATE FUNCTION sprout_private.validate_device_key_transparency_chain()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    expected_previous bytea;
    expected_entry bytea;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(NEW.device_id::text, 12));
    SELECT entry_hash INTO expected_previous
    FROM device_key_transparency_log
    WHERE device_id = NEW.device_id
    ORDER BY log_sequence DESC
    LIMIT 1;
    IF NEW.previous_entry_hash IS DISTINCT FROM expected_previous THEN
        RAISE EXCEPTION 'device key transparency chain mismatch'
            USING ERRCODE = '40001';
    END IF;
    expected_entry := digest(
        convert_to('sprout-device-key-transparency-v1', 'UTF8')
        || uuid_send(NEW.identity_id)
        || uuid_send(NEW.device_id)
        || int4send(NEW.key_version)
        || int8send(NEW.generation)
        || convert_to(NEW.event_kind, 'UTF8')
        || NEW.package_hash
        || COALESCE(NEW.previous_entry_hash, ''::bytea),
        'sha256'
    );
    IF NEW.entry_hash <> expected_entry THEN
        RAISE EXCEPTION 'device key transparency entry hash mismatch'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER device_key_transparency_validate_chain
BEFORE INSERT ON device_key_transparency_log
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_device_key_transparency_chain();
CREATE TRIGGER device_key_transparency_immutable
BEFORE UPDATE OR DELETE ON device_key_transparency_log
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

ALTER TABLE projects
    ADD COLUMN membership_epoch bigint NOT NULL DEFAULT 1 CHECK (membership_epoch > 0),
    ADD COLUMN owner_epoch bigint NOT NULL DEFAULT 1 CHECK (owner_epoch > 0),
    ADD COLUMN recovery_epoch bigint NOT NULL DEFAULT 1 CHECK (recovery_epoch > 0);

CREATE FUNCTION sprout_private.advance_project_membership_epoch()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    candidate_project_id uuid;
BEGIN
    candidate_project_id := CASE WHEN TG_OP = 'DELETE' THEN OLD.project_id ELSE NEW.project_id END;
    UPDATE projects
    SET membership_epoch = membership_epoch + 1
    WHERE id = candidate_project_id;
    RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END;
$$;

CREATE TRIGGER project_memberships_advance_epoch
AFTER INSERT OR DELETE OR UPDATE OF identity_id, role, state ON project_memberships
FOR EACH ROW EXECUTE FUNCTION sprout_private.advance_project_membership_epoch();

CREATE TABLE project_recovery_requests (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    requester_identity_id uuid NOT NULL,
    request_kind text NOT NULL
        CHECK (request_kind IN ('participant_device', 'lost_owner')),
    challenge bytea NOT NULL CHECK (octet_length(challenge) = 32),
    context_hash bytea NOT NULL CHECK (octet_length(context_hash) = 32),
    membership_epoch bigint NOT NULL CHECK (membership_epoch > 0),
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'finalized', 'expired', 'cancelled')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    finalized_at timestamptz,
    CONSTRAINT project_recovery_requests_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_recovery_requests_requester_fk
        FOREIGN KEY (project_id, requester_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_recovery_requests_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT project_recovery_requests_expiry CHECK (expires_at > created_at),
    CONSTRAINT project_recovery_requests_finalized_state CHECK (
        (status = 'finalized' AND finalized_at IS NOT NULL)
        OR (status <> 'finalized' AND finalized_at IS NULL)
    )
);

CREATE UNIQUE INDEX project_recovery_requests_one_pending_idx
    ON project_recovery_requests (project_id, requester_identity_id)
    WHERE status = 'pending';
CREATE INDEX project_recovery_requests_expiry_idx
    ON project_recovery_requests (expires_at)
    WHERE status = 'pending';

CREATE TABLE project_recovery_electorate (
    project_id uuid NOT NULL,
    recovery_request_id uuid NOT NULL,
    approver_identity_id uuid NOT NULL,
    snapshot_role text NOT NULL CHECK (snapshot_role IN ('owner', 'admin', 'member', 'guest')),
    membership_epoch bigint NOT NULL CHECK (membership_epoch > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, recovery_request_id, approver_identity_id),
    CONSTRAINT project_recovery_electorate_request_fk
        FOREIGN KEY (project_id, recovery_request_id)
        REFERENCES project_recovery_requests (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_recovery_electorate_identity_fk
        FOREIGN KEY (project_id, approver_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE TABLE project_recovery_approvals (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    recovery_request_id uuid NOT NULL,
    approver_identity_id uuid NOT NULL,
    approver_device_id uuid NOT NULL,
    approver_device_key_version integer NOT NULL,
    encrypted_share bytea NOT NULL CHECK (octet_length(encrypted_share) > 0),
    classical_signature bytea NOT NULL CHECK (octet_length(classical_signature) = 64),
    post_quantum_signature bytea NOT NULL CHECK (octet_length(post_quantum_signature) > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT project_recovery_approvals_electorate_fk
        FOREIGN KEY (project_id, recovery_request_id, approver_identity_id)
        REFERENCES project_recovery_electorate (
            project_id, recovery_request_id, approver_identity_id
        )
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_recovery_approvals_device_key_fk
        FOREIGN KEY (
            approver_identity_id, approver_device_id, approver_device_key_version
        ) REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_recovery_approvals_unique_approver
        UNIQUE (project_id, recovery_request_id, approver_identity_id)
);

CREATE TRIGGER project_recovery_electorate_immutable
BEFORE UPDATE OR DELETE ON project_recovery_electorate
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();
CREATE TRIGGER project_recovery_approvals_immutable
BEFORE UPDATE OR DELETE ON project_recovery_approvals
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE FUNCTION sprout_private.validate_project_recovery_finalization()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    expected_count integer;
    approval_count integer;
BEGIN
    IF OLD.status = 'finalized'
       AND (
           NEW.status IS DISTINCT FROM OLD.status
           OR NEW.finalized_at IS DISTINCT FROM OLD.finalized_at
       )
    THEN
        RAISE EXCEPTION 'finalized recovery request is immutable'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.status = 'finalized' AND OLD.status <> 'finalized' THEN
        IF OLD.status <> 'pending' OR OLD.expires_at <= clock_timestamp() THEN
            RAISE EXCEPTION 'recovery request is not active' USING ERRCODE = '23514';
        END IF;
        SELECT count(*) INTO expected_count
        FROM project_recovery_electorate
        WHERE project_id = NEW.project_id AND recovery_request_id = NEW.id;
        SELECT count(*) INTO approval_count
        FROM project_recovery_approvals
        WHERE project_id = NEW.project_id AND recovery_request_id = NEW.id;
        IF expected_count = 0 OR approval_count <> expected_count THEN
            RAISE EXCEPTION 'project recovery requires every frozen electorate approval'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER project_recovery_validate_finalization
BEFORE UPDATE OF status, finalized_at ON project_recovery_requests
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_project_recovery_finalization();

ALTER TABLE resource_key_envelopes
    ADD COLUMN sender_post_quantum_signature bytea;
UPDATE resource_key_envelopes
SET sender_post_quantum_signature = decode('00', 'hex');
ALTER TABLE resource_key_envelopes
    ALTER COLUMN sender_post_quantum_signature SET NOT NULL,
    ADD CONSTRAINT resource_key_envelopes_pq_signature_nonempty
        CHECK (octet_length(sender_post_quantum_signature) > 0);

ALTER TABLE sync_events
    ADD COLUMN resource_node_id uuid,
    ADD COLUMN base_version bigint,
    ADD COLUMN aggregate_version bigint,
    ADD COLUMN mutation_kind text,
    ADD COLUMN signature_suite integer NOT NULL DEFAULT 0,
    ADD COLUMN post_quantum_signature bytea;

UPDATE sync_events
SET resource_node_id = stream_id,
    mutation_kind = CASE WHEN event_kind = 'deleted' THEN 'tombstone' ELSE 'upsert' END;

WITH versions AS (
    SELECT
        id,
        row_number() OVER (
            PARTITION BY project_id, resource_node_id
            ORDER BY event_sequence
        ) AS aggregate_version
    FROM sync_events
)
UPDATE sync_events event
SET
    aggregate_version = versions.aggregate_version,
    base_version = versions.aggregate_version - 1
FROM versions
WHERE versions.id = event.id;

ALTER TABLE sync_events
    ALTER COLUMN resource_node_id SET NOT NULL,
    ALTER COLUMN base_version SET NOT NULL,
    ALTER COLUMN aggregate_version SET NOT NULL,
    ALTER COLUMN mutation_kind SET NOT NULL,
    ADD CONSTRAINT sync_events_resource_fk
        FOREIGN KEY (project_id, resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT sync_events_version_progress
        CHECK (base_version >= 0 AND aggregate_version = base_version + 1),
    ADD CONSTRAINT sync_events_mutation_kind
        CHECK (mutation_kind IN ('upsert', 'tombstone')),
    ADD CONSTRAINT sync_events_dual_signature_shape CHECK (
        (signature_suite = 0)
        OR (
            signature_suite = 32769
            AND octet_length(signature) = 64
            AND post_quantum_signature IS NOT NULL
            AND octet_length(post_quantum_signature) > 0
        )
    ),
    ADD CONSTRAINT sync_events_resource_version_unique
        UNIQUE (project_id, resource_node_id, aggregate_version);

CREATE INDEX sync_events_resource_cursor_idx
    ON sync_events (project_id, resource_node_id, event_sequence);

CREATE TABLE sync_aggregates (
    project_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    current_version bigint NOT NULL CHECK (current_version >= 0),
    tombstoned boolean NOT NULL DEFAULT false,
    last_event_hash bytea NOT NULL CHECK (octet_length(last_event_hash) = 32),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, resource_node_id),
    CONSTRAINT sync_aggregates_resource_fk
        FOREIGN KEY (project_id, resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE TABLE sync_device_heads (
    project_id uuid NOT NULL,
    actor_identity_id uuid NOT NULL,
    actor_device_id uuid NOT NULL,
    device_sequence bigint NOT NULL CHECK (device_sequence > 0),
    last_event_hash bytea NOT NULL CHECK (octet_length(last_event_hash) = 32),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, actor_device_id),
    CONSTRAINT sync_device_heads_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT sync_device_heads_device_fk
        FOREIGN KEY (actor_identity_id, actor_device_id)
        REFERENCES devices (identity_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

INSERT INTO sync_aggregates (
    project_id, resource_node_id, current_version, tombstoned, last_event_hash
)
SELECT DISTINCT ON (project_id, resource_node_id)
    project_id,
    resource_node_id,
    aggregate_version,
    mutation_kind = 'tombstone',
    event_hash
FROM sync_events
ORDER BY project_id, resource_node_id, aggregate_version DESC;

INSERT INTO sync_device_heads (
    project_id, actor_identity_id, actor_device_id,
    device_sequence, last_event_hash
)
SELECT DISTINCT ON (project_id, actor_device_id)
    project_id,
    actor_identity_id,
    actor_device_id,
    device_sequence,
    event_hash
FROM sync_events
ORDER BY project_id, actor_device_id, device_sequence DESC;

DROP TRIGGER sync_events_validate_chain ON sync_events;
DROP FUNCTION sprout_private.validate_sync_event_chain();

CREATE FUNCTION sprout_private.validate_sync_event_chain()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    expected_previous_hash bytea;
    expected_device_sequence bigint;
    aggregate_state sync_aggregates%ROWTYPE;
BEGIN
    PERFORM pg_advisory_xact_lock(
        hashtextextended(NEW.project_id::text || ':' || NEW.actor_device_id::text, 0)
    );
    SELECT last_event_hash, device_sequence + 1
    INTO expected_previous_hash, expected_device_sequence
    FROM sync_device_heads
    WHERE project_id = NEW.project_id
      AND actor_device_id = NEW.actor_device_id
    FOR UPDATE;
    IF NEW.previous_hash IS DISTINCT FROM expected_previous_hash
       OR NEW.device_sequence IS DISTINCT FROM COALESCE(expected_device_sequence, 1)
    THEN
        RAISE EXCEPTION 'sync device hash chain mismatch' USING ERRCODE = '40001';
    END IF;

    PERFORM pg_advisory_xact_lock(
        hashtextextended(NEW.project_id::text || ':' || NEW.resource_node_id::text, 7)
    );
    SELECT * INTO aggregate_state
    FROM sync_aggregates
    WHERE project_id = NEW.project_id
      AND resource_node_id = NEW.resource_node_id
    FOR UPDATE;
    IF FOUND THEN
        IF NEW.base_version <> aggregate_state.current_version THEN
            RAISE EXCEPTION 'stale sync base version' USING ERRCODE = '40001';
        END IF;
        IF aggregate_state.tombstoned AND NEW.mutation_kind <> 'tombstone' THEN
            RAISE EXCEPTION 'signed tombstone prevents resurrection'
                USING ERRCODE = '40001';
        END IF;
    ELSIF NEW.base_version <> 0 THEN
        RAISE EXCEPTION 'stale sync base version' USING ERRCODE = '40001';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER sync_events_validate_chain
BEFORE INSERT ON sync_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_sync_event_chain();

CREATE FUNCTION sprout_private.advance_sync_aggregate()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO sync_aggregates (
        project_id, resource_node_id, current_version, tombstoned,
        last_event_hash, updated_at
    )
    VALUES (
        NEW.project_id, NEW.resource_node_id, NEW.aggregate_version,
        NEW.mutation_kind = 'tombstone', NEW.event_hash, clock_timestamp()
    )
    ON CONFLICT (project_id, resource_node_id) DO UPDATE
    SET current_version = EXCLUDED.current_version,
        tombstoned = EXCLUDED.tombstoned,
        last_event_hash = EXCLUDED.last_event_hash,
        updated_at = EXCLUDED.updated_at;
    INSERT INTO sync_device_heads (
        project_id, actor_identity_id, actor_device_id,
        device_sequence, last_event_hash, updated_at
    )
    VALUES (
        NEW.project_id, NEW.actor_identity_id, NEW.actor_device_id,
        NEW.device_sequence, NEW.event_hash, clock_timestamp()
    )
    ON CONFLICT (project_id, actor_device_id) DO UPDATE
    SET actor_identity_id = EXCLUDED.actor_identity_id,
        device_sequence = EXCLUDED.device_sequence,
        last_event_hash = EXCLUDED.last_event_hash,
        updated_at = EXCLUDED.updated_at;
    RETURN NEW;
END;
$$;

CREATE TRIGGER sync_events_advance_aggregate
AFTER INSERT ON sync_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.advance_sync_aggregate();

CREATE FUNCTION sprout_private.can_access_resource(
    candidate_project_id uuid,
    candidate_resource_node_id uuid,
    required_access text
)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM resource_nodes node
        JOIN projects project ON project.id = node.project_id
        JOIN project_memberships membership
          ON membership.project_id = node.project_id
         AND membership.identity_id = sprout_private.current_identity_id()
         AND membership.state = 'active'
        LEFT JOIN LATERAL sprout_private.effective_domain_permission(
            node.project_id,
            node.id,
            sprout_private.current_identity_id()
        ) permission ON true
        WHERE node.project_id = candidate_project_id
          AND node.id = candidate_resource_node_id
          AND node.deleted_at IS NULL
          AND (
              project.owner_identity_id = membership.identity_id
              OR membership.role = 'admin'
              OR node.created_by_identity_id = membership.identity_id
              OR (
                  required_access = 'read'
                  AND permission.access_level IS NOT NULL
              )
              OR (
                  required_access = 'write'
                  AND permission.access_level IN ('edit', 'manage')
              )
          )
    )
$$;

REVOKE ALL ON FUNCTION sprout_private.can_access_resource(uuid, uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.can_access_resource(uuid, uuid, text) TO PUBLIC;

DROP POLICY project_isolation ON sync_events;
CREATE POLICY sync_events_resource_read ON sync_events
    FOR SELECT
    USING (
        sprout_private.can_access_resource(project_id, resource_node_id, 'read')
    );
CREATE POLICY sync_events_resource_insert ON sync_events
    FOR INSERT
    WITH CHECK (
        actor_identity_id = sprout_private.current_identity_id()
        AND actor_device_id = sprout_private.current_device_id()
        AND sprout_private.can_access_resource(project_id, resource_node_id, 'write')
    );

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'device_key_transparency_log',
        'project_recovery_requests',
        'project_recovery_electorate',
        'project_recovery_approvals',
        'sync_aggregates',
        'sync_device_heads'
    ]
    LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
    END LOOP;
END;
$$;

CREATE POLICY device_key_transparency_identity ON device_key_transparency_log
    USING (identity_id = sprout_private.current_identity_id())
    WITH CHECK (identity_id = sprout_private.current_identity_id());
CREATE POLICY project_recovery_requests_project ON project_recovery_requests
    USING (sprout_private.is_project_member(project_id))
    WITH CHECK (sprout_private.is_project_member(project_id));
CREATE POLICY project_recovery_electorate_project ON project_recovery_electorate
    USING (sprout_private.is_project_member(project_id))
    WITH CHECK (sprout_private.is_project_member(project_id));
CREATE POLICY project_recovery_approvals_project ON project_recovery_approvals
    USING (sprout_private.is_project_member(project_id))
    WITH CHECK (sprout_private.is_project_member(project_id));
CREATE POLICY sync_aggregates_resource_read ON sync_aggregates
    FOR SELECT
    USING (
        sprout_private.can_access_resource(project_id, resource_node_id, 'read')
    );
CREATE POLICY sync_aggregates_resource_insert ON sync_aggregates
    FOR INSERT
    WITH CHECK (
        sprout_private.can_access_resource(project_id, resource_node_id, 'write')
    );
CREATE POLICY sync_aggregates_resource_update ON sync_aggregates
    FOR UPDATE
    USING (
        sprout_private.can_access_resource(project_id, resource_node_id, 'write')
    )
    WITH CHECK (
        sprout_private.can_access_resource(project_id, resource_node_id, 'write')
    );
CREATE POLICY sync_device_heads_identity ON sync_device_heads
    USING (actor_identity_id = sprout_private.current_identity_id())
    WITH CHECK (
        actor_identity_id = sprout_private.current_identity_id()
        AND actor_device_id = sprout_private.current_device_id()
    );
