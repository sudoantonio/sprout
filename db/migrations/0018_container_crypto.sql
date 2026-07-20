-- Split navigation headers from resource bodies. Existing ciphertext and
-- envelopes remain body-only; resources opt in by publishing a header
-- commitment, encrypted header, and purpose-bound header envelopes.

ALTER TABLE resource_epochs
    ADD COLUMN header_key_commitment bytea,
    ADD CONSTRAINT resource_epochs_header_commitment_length
        CHECK (
            header_key_commitment IS NULL
            OR octet_length(header_key_commitment) >= 16
        );

ALTER TABLE resource_key_envelopes
    ADD COLUMN key_purpose text NOT NULL DEFAULT 'body'
        CHECK (key_purpose IN ('body', 'header'));

ALTER TABLE resource_key_envelopes
    DROP CONSTRAINT resource_key_envelopes_recipient_unique;

ALTER TABLE resource_key_envelopes
    ADD CONSTRAINT resource_key_envelopes_recipient_purpose_unique
        UNIQUE (
            project_id,
            resource_node_id,
            epoch,
            key_purpose,
            recipient_device_id,
            recipient_device_key_version
        );

ALTER TABLE topics
    ADD COLUMN encrypted_header bytea,
    ADD CONSTRAINT topics_encrypted_header_nonempty
        CHECK (
            encrypted_header IS NULL
            OR octet_length(encrypted_header) > 0
        );

ALTER TABLE task_lists
    ADD COLUMN encrypted_header bytea,
    ADD CONSTRAINT task_lists_encrypted_header_nonempty
        CHECK (
            encrypted_header IS NULL
            OR octet_length(encrypted_header) > 0
        );

ALTER TABLE tasks
    ADD COLUMN encrypted_header bytea,
    ADD CONSTRAINT tasks_encrypted_header_nonempty
        CHECK (
            encrypted_header IS NULL
            OR octet_length(encrypted_header) > 0
        );

CREATE INDEX resource_key_envelopes_recipient_purpose_active_idx
    ON resource_key_envelopes (
        project_id,
        recipient_device_id,
        key_purpose,
        created_at
    )
    WHERE revoked_at IS NULL;

COMMENT ON COLUMN resource_key_envelopes.key_purpose IS
    'body opens full resource content; header opens only minimal navigation metadata';
COMMENT ON COLUMN resource_epochs.header_key_commitment IS
    'NULL for legacy body-only resources; non-NULL requires purpose-separated envelopes';
