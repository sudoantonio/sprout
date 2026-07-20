CREATE TABLE device_keys (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    identity_id uuid NOT NULL,
    device_id uuid NOT NULL,
    key_version integer NOT NULL CHECK (key_version > 0),
    encryption_public_key bytea NOT NULL,
    signing_public_key bytea NOT NULL,
    key_attestation bytea,
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    CONSTRAINT device_keys_device_fk
        FOREIGN KEY (identity_id, device_id) REFERENCES devices (identity_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT device_keys_identity_device_version_unique
        UNIQUE (identity_id, device_id, key_version),
    CONSTRAINT device_keys_device_version_unique UNIQUE (device_id, key_version),
    CONSTRAINT device_keys_encryption_key_nonempty CHECK (octet_length(encryption_public_key) > 0),
    CONSTRAINT device_keys_signing_key_nonempty CHECK (octet_length(signing_public_key) > 0),
    CONSTRAINT device_keys_attestation_nonempty
        CHECK (key_attestation IS NULL OR octet_length(key_attestation) > 0)
);

CREATE UNIQUE INDEX device_keys_one_active_version_idx
    ON device_keys (device_id)
    WHERE revoked_at IS NULL;

CREATE TABLE resource_epochs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    epoch integer NOT NULL CHECK (epoch > 0),
    previous_epoch_id uuid,
    created_by_identity_id uuid NOT NULL,
    created_by_device_id uuid NOT NULL,
    created_by_device_key_version integer NOT NULL,
    key_commitment bytea NOT NULL,
    reason text NOT NULL
        CHECK (reason IN ('created', 'membership_change', 'device_revocation', 'manual', 'recovery')),
    created_at timestamptz NOT NULL DEFAULT now(),
    retired_at timestamptz,
    CONSTRAINT resource_epochs_resource_fk
        FOREIGN KEY (project_id, resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_epochs_creator_membership_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_epochs_creator_device_key_fk
        FOREIGN KEY (
            created_by_identity_id,
            created_by_device_id,
            created_by_device_key_version
        ) REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_epochs_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT resource_epochs_resource_epoch_unique
        UNIQUE (project_id, resource_node_id, epoch),
    CONSTRAINT resource_epochs_previous_fk
        FOREIGN KEY (project_id, previous_epoch_id) REFERENCES resource_epochs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_epochs_commitment_nonempty CHECK (octet_length(key_commitment) >= 16),
    CONSTRAINT resource_epochs_not_self_previous
        CHECK (previous_epoch_id IS NULL OR previous_epoch_id <> id)
);

CREATE UNIQUE INDEX resource_epochs_one_active_idx
    ON resource_epochs (project_id, resource_node_id)
    WHERE retired_at IS NULL;
CREATE INDEX resource_epochs_latest_idx
    ON resource_epochs (project_id, resource_node_id, epoch DESC);

CREATE TABLE resource_key_envelopes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    epoch integer NOT NULL,
    recipient_identity_id uuid NOT NULL,
    recipient_device_id uuid NOT NULL,
    recipient_device_key_version integer NOT NULL,
    encrypted_key bytea NOT NULL,
    sender_signature bytea NOT NULL,
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    CONSTRAINT resource_key_envelopes_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_key_envelopes_recipient_membership_fk
        FOREIGN KEY (project_id, recipient_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_key_envelopes_recipient_device_key_fk
        FOREIGN KEY (
            recipient_identity_id,
            recipient_device_id,
            recipient_device_key_version
        ) REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_key_envelopes_creator_membership_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_key_envelopes_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT resource_key_envelopes_recipient_unique
        UNIQUE (
            project_id,
            resource_node_id,
            epoch,
            recipient_device_id,
            recipient_device_key_version
        ),
    CONSTRAINT resource_key_envelopes_key_nonempty CHECK (octet_length(encrypted_key) > 0),
    CONSTRAINT resource_key_envelopes_signature_nonempty CHECK (octet_length(sender_signature) > 0)
);

CREATE INDEX resource_key_envelopes_recipient_active_idx
    ON resource_key_envelopes (project_id, recipient_device_id, created_at)
    WHERE revoked_at IS NULL;

CREATE TABLE recovery_sets (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    epoch integer NOT NULL,
    created_by_identity_id uuid NOT NULL,
    share_count smallint NOT NULL CHECK (share_count > 0),
    threshold smallint NOT NULL,
    commitment bytea NOT NULL,
    state text NOT NULL DEFAULT 'draft'
        CHECK (state IN ('draft', 'active', 'retired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    activated_at timestamptz,
    retired_at timestamptz,
    CONSTRAINT recovery_sets_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT recovery_sets_creator_membership_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT recovery_sets_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT recovery_sets_n_of_n CHECK (threshold = share_count),
    CONSTRAINT recovery_sets_commitment_nonempty CHECK (octet_length(commitment) >= 16),
    CONSTRAINT recovery_sets_activation_state
        CHECK ((state = 'active') = (activated_at IS NOT NULL AND retired_at IS NULL)
            OR state <> 'active'),
    CONSTRAINT recovery_sets_retired_state CHECK ((state = 'retired') = (retired_at IS NOT NULL))
);

CREATE UNIQUE INDEX recovery_sets_one_active_idx
    ON recovery_sets (project_id, resource_node_id, epoch)
    WHERE state = 'active';

CREATE TABLE recovery_shares (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    recovery_set_id uuid NOT NULL,
    share_index smallint NOT NULL CHECK (share_index > 0),
    holder_identity_id uuid NOT NULL,
    holder_device_id uuid NOT NULL,
    holder_device_key_version integer NOT NULL,
    encrypted_share bytea NOT NULL,
    share_commitment bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT recovery_shares_set_fk
        FOREIGN KEY (project_id, recovery_set_id) REFERENCES recovery_sets (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT recovery_shares_holder_membership_fk
        FOREIGN KEY (project_id, holder_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT recovery_shares_holder_device_key_fk
        FOREIGN KEY (holder_identity_id, holder_device_id, holder_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT recovery_shares_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT recovery_shares_index_unique
        UNIQUE (project_id, recovery_set_id, share_index),
    CONSTRAINT recovery_shares_holder_unique
        UNIQUE (project_id, recovery_set_id, holder_device_id, holder_device_key_version),
    CONSTRAINT recovery_shares_ciphertext_nonempty CHECK (octet_length(encrypted_share) > 0),
    CONSTRAINT recovery_shares_commitment_nonempty CHECK (octet_length(share_commitment) >= 16)
);

CREATE OR REPLACE FUNCTION sprout_private.validate_recovery_share_mutation()
RETURNS trigger
LANGUAGE plpgsql
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
    FROM recovery_sets
    WHERE project_id = candidate_project_id
      AND id = candidate_recovery_set_id
    FOR UPDATE;

    IF set_state <> 'draft' THEN
        RAISE EXCEPTION 'recovery shares are immutable after activation'
            USING ERRCODE = '55000';
    END IF;

    IF candidate_index > expected_count THEN
        RAISE EXCEPTION 'recovery share index exceeds declared n-of-n share count'
            USING ERRCODE = '23514';
    END IF;

    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION sprout_private.validate_recovery_activation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    actual_count integer;
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
        SELECT count(*) INTO actual_count
        FROM recovery_shares
        WHERE project_id = NEW.project_id
          AND recovery_set_id = NEW.id;

        IF actual_count <> NEW.share_count THEN
            RAISE EXCEPTION 'active n-of-n recovery set requires exactly % shares; found %',
                NEW.share_count, actual_count
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER recovery_shares_validate_mutation
BEFORE INSERT OR UPDATE OR DELETE ON recovery_shares
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_recovery_share_mutation();

CREATE TRIGGER recovery_sets_validate_activation
BEFORE INSERT OR UPDATE OF state ON recovery_sets
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_recovery_activation();
