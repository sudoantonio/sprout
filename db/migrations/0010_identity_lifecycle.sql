-- HLR-01 identity lifecycle, durable ceremonies, and invitation acceptance.
-- Email addresses are intentionally normalized plaintext routing metadata.

ALTER TABLE identities
    DROP CONSTRAINT identities_status_check;
ALTER TABLE identities
    ADD CONSTRAINT identities_status_check
    CHECK (status IN ('pending', 'active', 'suspended', 'deleted'));

-- Invitation acceptance must cross the pre-membership boundary. Application
-- roles remain subject to RLS and must not own tables.
ALTER TABLE project_invitations NO FORCE ROW LEVEL SECURITY;

CREATE TABLE identity_directory (
    identity_id uuid PRIMARY KEY,
    identity_handle text NOT NULL UNIQUE,
    identity_status text NOT NULL
        CHECK (identity_status IN ('pending', 'active', 'suspended', 'deleted')),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT identity_directory_identity_fk
        FOREIGN KEY (identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

INSERT INTO identity_directory (identity_id, identity_handle, identity_status, updated_at)
SELECT id, identity_handle, status, updated_at
FROM identities;

CREATE FUNCTION sprout_private.sync_identity_directory()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
BEGIN
    INSERT INTO identity_directory (
        identity_id, identity_handle, identity_status, updated_at
    )
    VALUES (NEW.id, NEW.identity_handle, NEW.status, clock_timestamp())
    ON CONFLICT (identity_id) DO UPDATE
    SET identity_handle = EXCLUDED.identity_handle,
        identity_status = EXCLUDED.identity_status,
        updated_at = EXCLUDED.updated_at;
    RETURN NEW;
END;
$$;

REVOKE ALL ON FUNCTION sprout_private.sync_identity_directory() FROM PUBLIC;

CREATE TRIGGER identities_sync_directory
AFTER INSERT OR UPDATE OF identity_handle, status ON identities
FOR EACH ROW EXECUTE FUNCTION sprout_private.sync_identity_directory();

CREATE TABLE identity_emails (
    identity_id uuid PRIMARY KEY,
    normalized_email text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    verified_at timestamptz,
    CONSTRAINT identity_emails_identity_fk
        FOREIGN KEY (identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT identity_emails_normalized
        CHECK (
            normalized_email = lower(btrim(normalized_email))
            AND length(normalized_email) BETWEEN 3 AND 320
            AND position('@' IN normalized_email) BETWEEN 2 AND length(normalized_email) - 1
            AND normalized_email !~ '[[:space:][:cntrl:]]'
        ),
    CONSTRAINT identity_emails_verified_after_creation
        CHECK (verified_at IS NULL OR verified_at >= created_at)
);

CREATE UNIQUE INDEX identity_emails_normalized_unique
    ON identity_emails (normalized_email);

CREATE TABLE email_verification_tokens (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    identity_id uuid NOT NULL,
    token_hash bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    CONSTRAINT email_verification_tokens_identity_fk
        FOREIGN KEY (identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT email_verification_tokens_hash_unique UNIQUE (token_hash),
    CONSTRAINT email_verification_tokens_hash_length CHECK (octet_length(token_hash) = 32),
    CONSTRAINT email_verification_tokens_expiry CHECK (expires_at > created_at),
    CONSTRAINT email_verification_tokens_consumption
        CHECK (consumed_at IS NULL OR consumed_at >= created_at)
);

CREATE UNIQUE INDEX email_verification_tokens_one_active_idx
    ON email_verification_tokens (identity_id)
    WHERE consumed_at IS NULL;
CREATE INDEX email_verification_tokens_cleanup_idx
    ON email_verification_tokens (expires_at)
    WHERE consumed_at IS NULL;

CREATE TABLE account_recovery_tokens (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    identity_id uuid NOT NULL,
    token_hash bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    CONSTRAINT account_recovery_tokens_identity_fk
        FOREIGN KEY (identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT account_recovery_tokens_hash_unique UNIQUE (token_hash),
    CONSTRAINT account_recovery_tokens_hash_length CHECK (octet_length(token_hash) = 32),
    CONSTRAINT account_recovery_tokens_expiry CHECK (expires_at > created_at),
    CONSTRAINT account_recovery_tokens_consumption
        CHECK (consumed_at IS NULL OR consumed_at >= created_at)
);

CREATE UNIQUE INDEX account_recovery_tokens_one_active_idx
    ON account_recovery_tokens (identity_id)
    WHERE consumed_at IS NULL;
CREATE INDEX account_recovery_tokens_cleanup_idx
    ON account_recovery_tokens (expires_at)
    WHERE consumed_at IS NULL;

CREATE TABLE webauthn_ceremonies (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    identity_id uuid NOT NULL,
    ceremony_kind text NOT NULL
        CHECK (ceremony_kind IN ('registration', 'authentication')),
    serialized_state bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    CONSTRAINT webauthn_ceremonies_identity_fk
        FOREIGN KEY (identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT webauthn_ceremonies_state_nonempty
        CHECK (octet_length(serialized_state) > 0),
    CONSTRAINT webauthn_ceremonies_expiry CHECK (expires_at > created_at),
    CONSTRAINT webauthn_ceremonies_consumption
        CHECK (consumed_at IS NULL OR consumed_at >= created_at)
);

CREATE UNIQUE INDEX webauthn_ceremonies_one_active_kind_idx
    ON webauthn_ceremonies (identity_id, ceremony_kind)
    WHERE consumed_at IS NULL;
CREATE INDEX webauthn_ceremonies_cleanup_idx
    ON webauthn_ceremonies (expires_at)
    WHERE consumed_at IS NULL;

CREATE TABLE email_outbox (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    identity_id uuid NOT NULL,
    message_kind text NOT NULL
        CHECK (message_kind IN ('signup_verification', 'account_recovery', 'project_invitation')),
    recipient_email text NOT NULL,
    token_hash bytea NOT NULL,
    payload_nonce bytea NOT NULL,
    encrypted_payload bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    available_at timestamptz NOT NULL DEFAULT now(),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    lock_owner uuid,
    lock_token uuid,
    locked_until timestamptz,
    delivered_at timestamptz,
    last_error_code text,
    CONSTRAINT email_outbox_identity_fk
        FOREIGN KEY (identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT email_outbox_token_hash_unique UNIQUE (token_hash),
    CONSTRAINT email_outbox_token_hash_length CHECK (octet_length(token_hash) = 32),
    CONSTRAINT email_outbox_nonce_length CHECK (octet_length(payload_nonce) = 12),
    CONSTRAINT email_outbox_payload_nonempty CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT email_outbox_recipient_normalized
        CHECK (
            recipient_email = lower(btrim(recipient_email))
            AND length(recipient_email) BETWEEN 3 AND 320
            AND position('@' IN recipient_email) BETWEEN 2 AND length(recipient_email) - 1
            AND recipient_email !~ '[[:space:][:cntrl:]]'
        ),
    CONSTRAINT email_outbox_lock_shape CHECK (
        (lock_owner IS NULL AND lock_token IS NULL AND locked_until IS NULL)
        OR (lock_owner IS NOT NULL AND lock_token IS NOT NULL AND locked_until IS NOT NULL)
    )
);

CREATE INDEX email_outbox_delivery_idx
    ON email_outbox (available_at, created_at)
    WHERE delivered_at IS NULL;
CREATE INDEX email_outbox_cleanup_idx
    ON email_outbox (delivered_at)
    WHERE delivered_at IS NOT NULL;

ALTER TABLE identity_emails ENABLE ROW LEVEL SECURITY;
CREATE POLICY identity_emails_self_access ON identity_emails
    USING (identity_id = sprout_private.current_identity_id())
    WITH CHECK (identity_id = sprout_private.current_identity_id());

ALTER TABLE identity_directory ENABLE ROW LEVEL SECURITY;
CREATE POLICY identity_directory_self_access ON identity_directory
    USING (identity_id = sprout_private.current_identity_id())
    WITH CHECK (identity_id = sprout_private.current_identity_id());

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'email_verification_tokens',
        'account_recovery_tokens',
        'webauthn_ceremonies',
        'email_outbox'
    ]
    LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
        EXECUTE format(
            'CREATE POLICY identity_isolation ON %I
             USING (identity_id = sprout_private.current_identity_id())
             WITH CHECK (identity_id = sprout_private.current_identity_id())',
            table_name
        );
    END LOOP;
END;
$$;

CREATE OR REPLACE FUNCTION sprout_private.active_identity_for_email(candidate_email text)
RETURNS uuid
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
    SELECT identity.identity_id
    FROM identity_emails email
    JOIN identity_directory identity ON identity.identity_id = email.identity_id
    WHERE email.normalized_email = candidate_email
      AND email.verified_at IS NOT NULL
      AND identity.identity_status = 'active'
    LIMIT 1
$$;

REVOKE ALL ON FUNCTION sprout_private.active_identity_for_email(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.active_identity_for_email(text) TO PUBLIC;

CREATE OR REPLACE FUNCTION sprout_private.accept_project_invitation(
    candidate_project_id uuid,
    candidate_invitation_id uuid,
    candidate_token_hash bytea,
    candidate_identity_id uuid
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    invitation project_invitations%ROWTYPE;
    candidate_email text;
BEGIN
    IF sprout_private.current_identity_id() IS DISTINCT FROM candidate_identity_id
       OR octet_length(candidate_token_hash) <> 32
    THEN
        RETURN false;
    END IF;

    SELECT *
    INTO invitation
    FROM project_invitations
    WHERE project_id = candidate_project_id
      AND id = candidate_invitation_id
    FOR UPDATE;

    IF NOT FOUND
       OR invitation.state <> 'pending'
       OR invitation.expires_at <= clock_timestamp()
       OR invitation.token_hash <> candidate_token_hash
    THEN
        RETURN false;
    END IF;

    SELECT normalized_email
    INTO candidate_email
    FROM identity_emails
    WHERE identity_id = candidate_identity_id
      AND verified_at IS NOT NULL;

    IF candidate_email IS NULL
       OR digest(convert_to(candidate_email, 'UTF8'), 'sha256')
            <> invitation.invitee_lookup_hash
       OR EXISTS (
            SELECT 1
            FROM project_memberships
            WHERE project_id = candidate_project_id
              AND identity_id = candidate_identity_id
       )
    THEN
        RETURN false;
    END IF;

    INSERT INTO project_memberships (project_id, identity_id, role, state)
    VALUES (
        candidate_project_id,
        candidate_identity_id,
        invitation.role,
        'active'
    );

    UPDATE project_invitations
    SET state = 'accepted',
        accepted_by_identity_id = candidate_identity_id,
        accepted_at = clock_timestamp()
    WHERE project_id = candidate_project_id
      AND id = candidate_invitation_id;

    RETURN true;
EXCEPTION
    WHEN unique_violation THEN
        RETURN false;
END;
$$;

REVOKE ALL ON FUNCTION sprout_private.accept_project_invitation(uuid, uuid, bytea, uuid)
    FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.accept_project_invitation(uuid, uuid, bytea, uuid)
    TO PUBLIC;

CREATE OR REPLACE FUNCTION sprout_private.suggest_project_participants(
    target_project_id uuid,
    handle_prefix text,
    result_limit integer
)
RETURNS TABLE (
    identity_id uuid,
    identity_handle text,
    shared_project_count bigint,
    most_recent_shared_project_at timestamptz
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
    WITH requester AS (
        SELECT membership.project_id, membership.joined_at
        FROM project_memberships membership
        WHERE membership.identity_id = sprout_private.current_identity_id()
          AND membership.state = 'active'
    ),
    guard AS (
        SELECT 1
        FROM requester
        WHERE requester.project_id = target_project_id
    )
    SELECT
        candidate_identity.identity_id,
        candidate_identity.identity_handle,
        count(DISTINCT candidate_membership.project_id),
        max(GREATEST(requester.joined_at, candidate_membership.joined_at))
    FROM guard
    CROSS JOIN requester
    JOIN project_memberships candidate_membership
      ON candidate_membership.project_id = requester.project_id
     AND candidate_membership.state = 'active'
     AND candidate_membership.identity_id <> sprout_private.current_identity_id()
    JOIN identity_directory candidate_identity
      ON candidate_identity.identity_id = candidate_membership.identity_id
     AND candidate_identity.identity_status = 'active'
    WHERE result_limit BETWEEN 1 AND 50
      AND length(handle_prefix) <= 128
      AND left(candidate_identity.identity_handle, char_length(handle_prefix)) = handle_prefix
      AND NOT EXISTS (
          SELECT 1
          FROM project_memberships target_membership
          WHERE target_membership.project_id = target_project_id
            AND target_membership.identity_id = candidate_identity.identity_id
            AND target_membership.state = 'active'
      )
    GROUP BY candidate_identity.identity_id, candidate_identity.identity_handle
    ORDER BY
        count(DISTINCT candidate_membership.project_id) DESC,
        max(GREATEST(requester.joined_at, candidate_membership.joined_at)) DESC,
        candidate_identity.identity_handle,
        candidate_identity.identity_id
    LIMIT result_limit
$$;

REVOKE ALL ON FUNCTION sprout_private.suggest_project_participants(uuid, text, integer)
    FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.suggest_project_participants(uuid, text, integer)
    TO PUBLIC;
