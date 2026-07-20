-- Project-scoped pre-provisioned unanimous recovery material.
-- Distinct from per-resource recovery_sets / recovery_shares (0006).

CREATE TABLE project_recovery_sets (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    recovery_epoch bigint NOT NULL CHECK (recovery_epoch > 0),
    membership_epoch bigint NOT NULL CHECK (membership_epoch > 0),
    created_by_identity_id uuid NOT NULL,
    share_count smallint NOT NULL CHECK (share_count BETWEEN 1 AND 16),
    threshold smallint NOT NULL,
    secret_commitment bytea NOT NULL CHECK (octet_length(secret_commitment) = 32),
    context_hash bytea NOT NULL CHECK (octet_length(context_hash) = 32),
    encrypted_owner_key_escrow bytea NOT NULL
        CHECK (octet_length(encrypted_owner_key_escrow) > 0),
    state text NOT NULL DEFAULT 'draft'
        CHECK (state IN ('draft', 'active', 'retired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    activated_at timestamptz,
    retired_at timestamptz,
    CONSTRAINT project_recovery_sets_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_recovery_sets_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_recovery_sets_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT project_recovery_sets_n_of_n CHECK (threshold = share_count),
    CONSTRAINT project_recovery_sets_activation_state CHECK (
        (state = 'active') = (activated_at IS NOT NULL AND retired_at IS NULL)
        OR state <> 'active'
    ),
    CONSTRAINT project_recovery_sets_retired_state CHECK (
        (state = 'retired') = (retired_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX project_recovery_sets_one_active_idx
    ON project_recovery_sets (project_id)
    WHERE state = 'active';

-- Retired history may retain an epoch; only one live (draft/active) set per epoch.
CREATE UNIQUE INDEX project_recovery_sets_epoch_live_unique
    ON project_recovery_sets (project_id, recovery_epoch)
    WHERE state IN ('draft', 'active');

CREATE TABLE project_recovery_shares (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    recovery_set_id uuid NOT NULL,
    share_index smallint NOT NULL CHECK (share_index BETWEEN 1 AND 16),
    holder_identity_id uuid NOT NULL,
    holder_device_id uuid NOT NULL,
    holder_device_key_version integer NOT NULL,
    encrypted_share bytea NOT NULL CHECK (octet_length(encrypted_share) > 0),
    share_commitment bytea NOT NULL CHECK (octet_length(share_commitment) = 32),
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT project_recovery_shares_set_fk
        FOREIGN KEY (project_id, recovery_set_id)
        REFERENCES project_recovery_sets (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_recovery_shares_holder_membership_fk
        FOREIGN KEY (project_id, holder_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_recovery_shares_holder_device_key_fk
        FOREIGN KEY (
            holder_identity_id, holder_device_id, holder_device_key_version
        ) REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_recovery_shares_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT project_recovery_shares_device_unique
        UNIQUE (
            project_id, recovery_set_id,
            holder_device_id, holder_device_key_version
        )
);

CREATE INDEX project_recovery_shares_holder_idx
    ON project_recovery_shares (project_id, holder_identity_id, recovery_set_id);

CREATE OR REPLACE FUNCTION sprout_private.validate_project_recovery_share_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    set_state text;
    expected_count smallint;
    candidate_index smallint;
    candidate_project_id uuid;
    candidate_recovery_set_id uuid;
BEGIN
    IF TG_OP = 'DELETE' THEN
        candidate_project_id := OLD.project_id;
        candidate_recovery_set_id := OLD.recovery_set_id;
        candidate_index := OLD.share_index;
    ELSE
        candidate_project_id := NEW.project_id;
        candidate_recovery_set_id := NEW.recovery_set_id;
        candidate_index := NEW.share_index;
    END IF;

    SELECT state, share_count INTO set_state, expected_count
    FROM project_recovery_sets
    WHERE project_id = candidate_project_id
      AND id = candidate_recovery_set_id
    FOR UPDATE;

    IF set_state IS NULL THEN
        RAISE EXCEPTION 'project recovery set not found' USING ERRCODE = '23503';
    END IF;

    IF set_state <> 'draft' THEN
        RAISE EXCEPTION 'project recovery shares are immutable after activation'
            USING ERRCODE = '55000';
    END IF;

    IF candidate_index > expected_count THEN
        RAISE EXCEPTION 'project recovery share index exceeds declared n-of-n share count'
            USING ERRCODE = '23514';
    END IF;

    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION sprout_private.validate_project_recovery_activation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    actual_indexes integer;
    actual_holders integer;
    activation_requested boolean := false;
BEGIN
    IF NEW.state = 'active' THEN
        IF TG_OP = 'INSERT' THEN
            activation_requested := true;
        ELSE
            activation_requested := OLD.state IS DISTINCT FROM 'active';
        END IF;
    END IF;

    IF activation_requested THEN
        SELECT count(DISTINCT share_index), count(DISTINCT holder_identity_id)
        INTO actual_indexes, actual_holders
        FROM project_recovery_shares
        WHERE project_id = NEW.project_id
          AND recovery_set_id = NEW.id;

        IF actual_indexes <> NEW.share_count OR actual_holders <> NEW.share_count THEN
            RAISE EXCEPTION
                'active project recovery set requires exactly % distinct participant shares; found indexes=% holders=%',
                NEW.share_count, actual_indexes, actual_holders
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER project_recovery_shares_validate_mutation
BEFORE INSERT OR UPDATE OR DELETE ON project_recovery_shares
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_project_recovery_share_mutation();

CREATE TRIGGER project_recovery_sets_validate_activation
BEFORE INSERT OR UPDATE OF state ON project_recovery_sets
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_project_recovery_activation();

ALTER TABLE project_recovery_requests
    ADD COLUMN recovery_epoch bigint,
    ADD COLUMN recovery_set_id uuid,
    ADD COLUMN requester_device_id uuid,
    ADD COLUMN requester_device_key_version integer;

UPDATE project_recovery_requests
SET recovery_epoch = 1
WHERE recovery_epoch IS NULL;

ALTER TABLE project_recovery_requests
    ALTER COLUMN recovery_epoch SET NOT NULL,
    ALTER COLUMN recovery_epoch SET DEFAULT 1,
    ADD CONSTRAINT project_recovery_requests_recovery_epoch_positive
        CHECK (recovery_epoch > 0),
    ADD CONSTRAINT project_recovery_requests_set_fk
        FOREIGN KEY (project_id, recovery_set_id)
        REFERENCES project_recovery_sets (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

-- Pending ceremonies created before provisioning cannot bind an active set.
UPDATE project_recovery_requests
SET status = 'cancelled'
WHERE status = 'pending';

ALTER TABLE project_recovery_sets ENABLE ROW LEVEL SECURITY;
ALTER TABLE project_recovery_sets FORCE ROW LEVEL SECURITY;
ALTER TABLE project_recovery_shares ENABLE ROW LEVEL SECURITY;
ALTER TABLE project_recovery_shares FORCE ROW LEVEL SECURITY;

CREATE POLICY project_recovery_sets_member ON project_recovery_sets
    USING (sprout_private.is_project_member(project_id))
    WITH CHECK (sprout_private.is_project_member(project_id));

-- Holders may read only their own share ciphertext.
CREATE POLICY project_recovery_shares_holder_select ON project_recovery_shares
    FOR SELECT
    USING (
        holder_identity_id = sprout_private.current_identity_id()
        AND sprout_private.is_project_member(project_id)
    );

-- Draft share writes are membership-scoped; activation immutability is trigger-enforced.
CREATE POLICY project_recovery_shares_member_insert ON project_recovery_shares
    FOR INSERT
    WITH CHECK (sprout_private.is_project_member(project_id));

CREATE POLICY project_recovery_shares_member_update ON project_recovery_shares
    FOR UPDATE
    USING (sprout_private.is_project_member(project_id))
    WITH CHECK (sprout_private.is_project_member(project_id));

CREATE POLICY project_recovery_shares_member_delete ON project_recovery_shares
    FOR DELETE
    USING (sprout_private.is_project_member(project_id));

DROP POLICY IF EXISTS project_recovery_approvals_project ON project_recovery_approvals;

CREATE POLICY project_recovery_approvals_insert ON project_recovery_approvals
    FOR INSERT
    WITH CHECK (
        sprout_private.is_project_member(project_id)
        AND approver_identity_id = sprout_private.current_identity_id()
    );

-- Approver sees own delivery row; requester sees all deliveries for their request.
CREATE POLICY project_recovery_approvals_select ON project_recovery_approvals
    FOR SELECT
    USING (
        sprout_private.is_project_member(project_id)
        AND (
            approver_identity_id = sprout_private.current_identity_id()
            OR EXISTS (
                SELECT 1
                FROM project_recovery_requests request
                WHERE request.project_id = project_recovery_approvals.project_id
                  AND request.id = project_recovery_approvals.recovery_request_id
                  AND request.requester_identity_id = sprout_private.current_identity_id()
            )
        )
    );

COMMENT ON TABLE project_recovery_sets IS
    'Project-scoped n-of-n recovery epochs; secret commitment and opaque owner-key escrow only';
COMMENT ON COLUMN project_recovery_sets.encrypted_owner_key_escrow IS
    'Opaque ciphertext opened only by the combined recovery secret; server never holds plaintext';
COMMENT ON TABLE project_recovery_shares IS
    'Per-participant/device encrypted share envelopes for an active or draft recovery set';
