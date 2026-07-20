CREATE TABLE identities (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    identity_handle text NOT NULL,
    login_lookup_hash bytea,
    encrypted_profile bytea NOT NULL,
    profile_key_epoch integer NOT NULL DEFAULT 1 CHECK (profile_key_epoch > 0),
    status text NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'suspended', 'deleted')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    CONSTRAINT identities_handle_format
        CHECK (identity_handle = lower(identity_handle) AND length(identity_handle) BETWEEN 3 AND 128),
    CONSTRAINT identities_login_lookup_hash_nonempty
        CHECK (login_lookup_hash IS NULL OR octet_length(login_lookup_hash) >= 16),
    CONSTRAINT identities_encrypted_profile_nonempty
        CHECK (octet_length(encrypted_profile) > 0),
    CONSTRAINT identities_deleted_state
        CHECK ((status = 'deleted') = (deleted_at IS NOT NULL))
);

CREATE UNIQUE INDEX identities_handle_unique
    ON identities (identity_handle);
CREATE UNIQUE INDEX identities_login_lookup_hash_unique
    ON identities (login_lookup_hash)
    WHERE login_lookup_hash IS NOT NULL AND deleted_at IS NULL;

CREATE TRIGGER identities_touch_updated_at
BEFORE UPDATE ON identities
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE passkeys (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    identity_id uuid NOT NULL,
    credential_id bytea NOT NULL,
    public_key_cose bytea NOT NULL,
    sign_count bigint NOT NULL DEFAULT 0 CHECK (sign_count >= 0),
    transports text[] NOT NULL DEFAULT '{}',
    backup_eligible boolean NOT NULL DEFAULT false,
    backup_state boolean NOT NULL DEFAULT false,
    encrypted_label bytea,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz,
    revoked_at timestamptz,
    CONSTRAINT passkeys_identity_fk
        FOREIGN KEY (identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT passkeys_credential_nonempty CHECK (octet_length(credential_id) > 0),
    CONSTRAINT passkeys_public_key_nonempty CHECK (octet_length(public_key_cose) > 0),
    CONSTRAINT passkeys_label_nonempty
        CHECK (encrypted_label IS NULL OR octet_length(encrypted_label) > 0)
);

CREATE UNIQUE INDEX passkeys_credential_id_unique ON passkeys (credential_id);
CREATE INDEX passkeys_active_identity_idx
    ON passkeys (identity_id, created_at)
    WHERE revoked_at IS NULL;

CREATE TABLE devices (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    identity_id uuid NOT NULL,
    device_kind text NOT NULL
        CHECK (device_kind IN ('web', 'ios', 'android', 'desktop', 'service', 'other')),
    encrypted_label bytea NOT NULL,
    trust_state text NOT NULL DEFAULT 'pending'
        CHECK (trust_state IN ('pending', 'trusted', 'blocked', 'retired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz,
    retired_at timestamptz,
    CONSTRAINT devices_identity_fk
        FOREIGN KEY (identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT devices_identity_id_unique UNIQUE (identity_id, id),
    CONSTRAINT devices_label_nonempty CHECK (octet_length(encrypted_label) > 0),
    CONSTRAINT devices_retired_state
        CHECK ((trust_state = 'retired') = (retired_at IS NOT NULL))
);

CREATE INDEX devices_active_identity_idx
    ON devices (identity_id, last_seen_at DESC NULLS LAST)
    WHERE retired_at IS NULL;

CREATE TRIGGER devices_touch_updated_at
BEFORE UPDATE ON devices
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE sessions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    identity_id uuid NOT NULL,
    device_id uuid NOT NULL,
    token_hash bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    revoke_reason text,
    CONSTRAINT sessions_device_fk
        FOREIGN KEY (identity_id, device_id) REFERENCES devices (identity_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT sessions_token_hash_nonempty CHECK (octet_length(token_hash) >= 32),
    CONSTRAINT sessions_expiry_after_creation CHECK (expires_at > created_at),
    CONSTRAINT sessions_revoke_reason_state
        CHECK ((revoked_at IS NULL AND revoke_reason IS NULL)
            OR (revoked_at IS NOT NULL AND revoke_reason IS NOT NULL))
);

CREATE UNIQUE INDEX sessions_token_hash_unique ON sessions (token_hash);
CREATE INDEX sessions_active_identity_idx
    ON sessions (identity_id, expires_at)
    WHERE revoked_at IS NULL;
CREATE INDEX sessions_active_device_idx
    ON sessions (device_id, expires_at)
    WHERE revoked_at IS NULL;
