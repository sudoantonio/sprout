\set ON_ERROR_STOP on

BEGIN;

INSERT INTO identities (id, identity_handle, encrypted_profile)
VALUES (
    '10000000-0000-0000-0000-000000000001',
    'schema-verifier',
    decode('01', 'hex')
);

INSERT INTO devices (id, identity_id, device_kind, encrypted_label, trust_state)
VALUES (
    '20000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    'service',
    decode('01', 'hex'),
    'trusted'
);

INSERT INTO device_keys (
    id,
    identity_id,
    device_id,
    key_version,
    encryption_public_key,
    signing_public_key,
    previous_package_hash,
    package_hash,
    x25519_public_key,
    ed25519_public_key
)
VALUES (
    '21000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    1,
    decode('01', 'hex'),
    decode('02', 'hex'),
    decode(repeat('00', 32), 'hex'),
    decode(repeat('01', 32), 'hex'),
    decode('01', 'hex'),
    decode('02', 'hex')
);

INSERT INTO projects (id, owner_identity_id, encrypted_metadata)
VALUES
    (
        '30000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001',
        decode('01', 'hex')
    ),
    (
        '30000000-0000-0000-0000-000000000002',
        '10000000-0000-0000-0000-000000000001',
        decode('02', 'hex')
    );

INSERT INTO project_memberships (
    id,
    project_id,
    identity_id,
    role
)
VALUES
    (
        '31000000-0000-0000-0000-000000000001',
        '30000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001',
        'owner'
    ),
    (
        '31000000-0000-0000-0000-000000000002',
        '30000000-0000-0000-0000-000000000002',
        '10000000-0000-0000-0000-000000000001',
        'owner'
    );

INSERT INTO resource_nodes (
    id,
    project_id,
    parent_id,
    node_kind,
    encrypted_metadata,
    created_by_identity_id
)
VALUES
    (
        '40000000-0000-0000-0000-000000000001',
        '30000000-0000-0000-0000-000000000001',
        NULL,
        'root',
        decode('01', 'hex'),
        '10000000-0000-0000-0000-000000000001'
    ),
    (
        '40000000-0000-0000-0000-000000000002',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000001',
        'topic',
        decode('02', 'hex'),
        '10000000-0000-0000-0000-000000000001'
    ),
    (
        '40000000-0000-0000-0000-000000000003',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000002',
        'task_list',
        decode('03', 'hex'),
        '10000000-0000-0000-0000-000000000001'
    ),
    (
        '40000000-0000-0000-0000-000000000004',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000003',
        'task',
        decode('04', 'hex'),
        '10000000-0000-0000-0000-000000000001'
    ),
    (
        '40000000-0000-0000-0000-000000000005',
        '30000000-0000-0000-0000-000000000002',
        NULL,
        'root',
        decode('05', 'hex'),
        '10000000-0000-0000-0000-000000000001'
    );

-- Behavioral fixtures insert many domain rows directly. Keep their payload
-- epoch FK valid without duplicating epoch boilerplate in every scenario.
CREATE FUNCTION pg_temp.ensure_fixture_payload_epoch()
RETURNS trigger
LANGUAGE plpgsql
AS $fixture$
BEGIN
    INSERT INTO resource_epochs (
        id, project_id, resource_node_id, epoch,
        created_by_identity_id, created_by_device_id,
        created_by_device_key_version, key_commitment, reason
    )
    VALUES (
        gen_random_uuid(), NEW.project_id, NEW.resource_node_id, NEW.key_epoch,
        '10000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000001',
        1, decode(repeat('ee', 16), 'hex'), 'created'
    )
    ON CONFLICT (project_id, resource_node_id, epoch) DO NOTHING;
    RETURN NEW;
END;
$fixture$;

CREATE TRIGGER fixture_topics_payload_epoch
BEFORE INSERT ON topics
FOR EACH ROW EXECUTE FUNCTION pg_temp.ensure_fixture_payload_epoch();

CREATE TRIGGER fixture_task_lists_payload_epoch
BEFORE INSERT ON task_lists
FOR EACH ROW EXECUTE FUNCTION pg_temp.ensure_fixture_payload_epoch();

CREATE TRIGGER fixture_tasks_payload_epoch
BEFORE INSERT ON tasks
FOR EACH ROW EXECUTE FUNCTION pg_temp.ensure_fixture_payload_epoch();

DO $test$
BEGIN
    BEGIN
        UPDATE resource_nodes
        SET parent_id = '40000000-0000-0000-0000-000000000004'
        WHERE id = '40000000-0000-0000-0000-000000000002';
        RAISE EXCEPTION 'cycle update unexpectedly succeeded';
    EXCEPTION
        WHEN check_violation THEN NULL;
    END;

    BEGIN
        INSERT INTO resource_nodes (
            id,
            project_id,
            parent_id,
            node_kind,
            encrypted_metadata,
            created_by_identity_id
        )
        VALUES (
            '40000000-0000-0000-0000-000000000006',
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000005',
            'task',
            decode('06', 'hex'),
            '10000000-0000-0000-0000-000000000001'
        );
        RAISE EXCEPTION 'cross-project parent unexpectedly succeeded';
    EXCEPTION
        WHEN foreign_key_violation THEN NULL;
    END;
END;
$test$;

INSERT INTO resource_epochs (
    id, project_id, resource_node_id, epoch,
    created_by_identity_id, created_by_device_id,
    created_by_device_key_version, key_commitment, reason
)
VALUES
    (
        '60000000-0000-0000-0000-000000000021',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000002', 1,
        '10000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000001', 1,
        decode(repeat('a2', 16), 'hex'), 'created'
    ),
    (
        '60000000-0000-0000-0000-000000000022',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000003', 1,
        '10000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000001', 1,
        decode(repeat('a3', 16), 'hex'), 'created'
    ),
    (
        '60000000-0000-0000-0000-000000000001',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000004', 1,
        '10000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000001', 1,
        decode(repeat('aa', 16), 'hex'), 'created'
    );

INSERT INTO topics (id, project_id, resource_node_id, encrypted_payload)
VALUES (
    '50000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000002',
    decode('01', 'hex')
);

INSERT INTO task_lists (id, project_id, topic_id, resource_node_id, encrypted_payload)
VALUES (
    '51000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '50000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    decode('01', 'hex')
);

INSERT INTO info_documents (
    id, project_id, task_list_id, resource_node_id,
    encrypted_payload, key_epoch, created_by_identity_id
)
VALUES (
    '51500000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '51000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    decode('01', 'hex'), 1,
    '10000000-0000-0000-0000-000000000001'
);

INSERT INTO info_documents (
    id, project_id, task_list_id, parent_document_id, resource_node_id,
    encrypted_payload, key_epoch, created_by_identity_id
)
VALUES (
    '51500000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000001',
    '51000000-0000-0000-0000-000000000001',
    '51500000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    decode('02', 'hex'), 1,
    '10000000-0000-0000-0000-000000000001'
);

DO $test$
BEGIN
    BEGIN
        INSERT INTO info_documents (
            id, project_id, task_list_id, resource_node_id,
            encrypted_payload, key_epoch, created_by_identity_id
        )
        VALUES (
            '51500000-0000-0000-0000-000000000003',
            '30000000-0000-0000-0000-000000000001',
            '51000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000003',
            decode('03', 'hex'), 1,
            '10000000-0000-0000-0000-000000000001'
        );
        RAISE EXCEPTION 'second active task-list info root unexpectedly succeeded';
    EXCEPTION
        WHEN unique_violation THEN NULL;
    END;

    BEGIN
        UPDATE info_documents
        SET resource_node_id = '40000000-0000-0000-0000-000000000002'
        WHERE id = '51500000-0000-0000-0000-000000000002';
        RAISE EXCEPTION 'info document escaped its encrypted container';
    EXCEPTION
        WHEN check_violation THEN NULL;
    END;
END;
$test$;

INSERT INTO tasks (id, project_id, task_list_id, resource_node_id, encrypted_payload)
VALUES (
    '52000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '51000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000004',
    decode('01', 'hex')
);

INSERT INTO task_recurrences (
    id,
    project_id,
    task_id,
    client_recurrence_id,
    encrypted_rule,
    rule_hash,
    starts_at
)
VALUES (
    '53000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '52000000-0000-0000-0000-000000000001',
    '53000000-0000-0000-0000-000000000011',
    decode('01', 'hex'),
    decode(repeat('aa', 16), 'hex'),
    now()
);

DO $test$
BEGIN
    BEGIN
        INSERT INTO task_recurrences (
            id,
            project_id,
            task_id,
            client_recurrence_id,
            encrypted_rule,
            rule_hash,
            starts_at
        )
        VALUES (
            '53000000-0000-0000-0000-000000000002',
            '30000000-0000-0000-0000-000000000001',
            '52000000-0000-0000-0000-000000000001',
            '53000000-0000-0000-0000-000000000012',
            decode('02', 'hex'),
            decode(repeat('bb', 16), 'hex'),
            now()
        );
        RAISE EXCEPTION 'second active recurrence unexpectedly succeeded';
    EXCEPTION
        WHEN unique_violation THEN NULL;
    END;
END;
$test$;

INSERT INTO resource_key_envelopes (
    id, project_id, resource_node_id, epoch, envelope_version,
    recipient_identity_id, recipient_device_id,
    recipient_device_key_version, encrypted_key, sender_signature,
    sender_post_quantum_signature,
    created_by_identity_id, created_by_device_id,
    created_by_device_key_version
)
VALUES (
    '60000000-0000-0000-0000-000000000011',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000004',
    1,
    1,
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    1,
    decode(repeat('ab', 16), 'hex'),
    decode(repeat('cd', 64), 'hex'),
    decode('01', 'hex'),
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    1
);

DO $test$
BEGIN
    BEGIN
        INSERT INTO resource_key_envelopes (
            project_id, resource_node_id, epoch, envelope_version,
            recipient_identity_id, recipient_device_id,
            recipient_device_key_version, encrypted_key, sender_signature,
            sender_post_quantum_signature,
            created_by_identity_id, created_by_device_id,
            created_by_device_key_version
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000004',
            1,
            1,
            '10000000-0000-0000-0000-000000000001',
            '20000000-0000-0000-0000-000000000001',
            1,
            decode(repeat('ef', 16), 'hex'),
            decode(repeat('01', 63), 'hex'),
            decode('01', 'hex'),
            '10000000-0000-0000-0000-000000000001',
            '20000000-0000-0000-0000-000000000001',
            1
        );
        RAISE EXCEPTION 'invalid envelope signature length unexpectedly succeeded';
    EXCEPTION
        WHEN check_violation THEN NULL;
    END;
END;
$test$;

-- T-LLR-06.6: purpose-separated header envelopes coexist with body envelopes;
-- duplicate body purpose for the same recipient device remains rejected.
UPDATE resource_epochs
SET header_key_commitment = decode(repeat('bb', 16), 'hex')
WHERE id = '60000000-0000-0000-0000-000000000001';

INSERT INTO resource_key_envelopes (
    id, project_id, resource_node_id, epoch, envelope_version,
    key_purpose,
    recipient_identity_id, recipient_device_id,
    recipient_device_key_version, encrypted_key, sender_signature,
    sender_post_quantum_signature,
    created_by_identity_id, created_by_device_id,
    created_by_device_key_version
)
VALUES (
    '60000000-0000-0000-0000-000000000012',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000004',
    1,
    1,
    'header',
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    1,
    decode(repeat('ac', 16), 'hex'),
    decode(repeat('ce', 64), 'hex'),
    decode('02', 'hex'),
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    1
);

DO $test$
DECLARE
    body_count integer;
    header_count integer;
BEGIN
    SELECT count(*) INTO body_count
    FROM resource_key_envelopes
    WHERE id = '60000000-0000-0000-0000-000000000011'
      AND key_purpose = 'body';
    SELECT count(*) INTO header_count
    FROM resource_key_envelopes
    WHERE id = '60000000-0000-0000-0000-000000000012'
      AND key_purpose = 'header';
    IF body_count <> 1 OR header_count <> 1 THEN
        RAISE EXCEPTION 'T-LLR-06.6 purpose-separated envelopes were not stored';
    END IF;

    BEGIN
        INSERT INTO resource_key_envelopes (
            project_id, resource_node_id, epoch, envelope_version,
            key_purpose,
            recipient_identity_id, recipient_device_id,
            recipient_device_key_version, encrypted_key, sender_signature,
            sender_post_quantum_signature,
            created_by_identity_id, created_by_device_id,
            created_by_device_key_version
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000004',
            1,
            1,
            'body',
            '10000000-0000-0000-0000-000000000001',
            '20000000-0000-0000-0000-000000000001',
            1,
            decode(repeat('ad', 16), 'hex'),
            decode(repeat('cf', 64), 'hex'),
            decode('03', 'hex'),
            '10000000-0000-0000-0000-000000000001',
            '20000000-0000-0000-0000-000000000001',
            1
        );
        RAISE EXCEPTION 'duplicate body envelope for the same device unexpectedly succeeded';
    EXCEPTION
        WHEN unique_violation THEN NULL;
    END;
END;
$test$;

INSERT INTO recovery_sets (
    id,
    project_id,
    resource_node_id,
    epoch,
    created_by_identity_id,
    share_count,
    threshold,
    commitment
)
VALUES (
    '61000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000004',
    1,
    '10000000-0000-0000-0000-000000000001',
    1,
    1,
    decode(repeat('bb', 16), 'hex')
);

DO $test$
BEGIN
    BEGIN
        UPDATE recovery_sets
        SET state = 'active', activated_at = now()
        WHERE id = '61000000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'incomplete recovery set unexpectedly activated';
    EXCEPTION
        WHEN check_violation THEN NULL;
    END;
END;
$test$;

INSERT INTO recovery_shares (
    id,
    project_id,
    recovery_set_id,
    share_index,
    holder_identity_id,
    holder_device_id,
    holder_device_key_version,
    encrypted_share,
    share_commitment
)
VALUES (
    '62000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '61000000-0000-0000-0000-000000000001',
    1,
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    1,
    decode('01', 'hex'),
    decode(repeat('cc', 16), 'hex')
);

UPDATE recovery_sets
SET state = 'active', activated_at = now()
WHERE id = '61000000-0000-0000-0000-000000000001';

DO $test$
BEGIN
    BEGIN
        DELETE FROM recovery_shares
        WHERE id = '62000000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'active recovery share unexpectedly deleted';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
END;
$test$;

-- T-LLR-01.1 / T-LLR-01.2 / T-LLR-01.3 / T-LLR-01.4 /
-- T-LLR-01.5: identity lifecycle, recovery boundaries, suggestions,
-- invitations, and one-time ceremony records. T-LLR-01.2 remains partial
-- because browser RP/origin/UV enforcement is not exercised here.
INSERT INTO identities (id, identity_handle, encrypted_profile, status)
VALUES (
    '10000000-0000-0000-0000-000000000010',
    'pending-verifier',
    decode('10', 'hex'),
    'pending'
);

INSERT INTO identity_emails (identity_id, normalized_email)
VALUES (
    '10000000-0000-0000-0000-000000000010',
    'pending@example.test'
);

DO $test$
BEGIN
    BEGIN
        INSERT INTO identities (id, identity_handle, encrypted_profile, status)
        VALUES (
            '10000000-0000-0000-0000-000000000011',
            'duplicate-email',
            decode('11', 'hex'),
            'pending'
        );
        INSERT INTO identity_emails (identity_id, normalized_email)
        VALUES (
            '10000000-0000-0000-0000-000000000011',
            'pending@example.test'
        );
        RAISE EXCEPTION 'duplicate normalized email unexpectedly succeeded';
    EXCEPTION
        WHEN unique_violation THEN NULL;
    END;
END;
$test$;

INSERT INTO email_verification_tokens (
    id, identity_id, token_hash, created_at, expires_at
)
VALUES (
    '70000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000010',
    digest('verification-token', 'sha256'),
    now(),
    now() + interval '10 minutes'
);

DO $test$
DECLARE
    affected integer;
BEGIN
    UPDATE email_verification_tokens
    SET consumed_at = clock_timestamp()
    WHERE id = '70000000-0000-0000-0000-000000000001'
      AND consumed_at IS NULL
      AND expires_at > clock_timestamp();
    GET DIAGNOSTICS affected = ROW_COUNT;
    IF affected <> 1 THEN
        RAISE EXCEPTION 'valid verification token was not consumed';
    END IF;

    UPDATE email_verification_tokens
    SET consumed_at = clock_timestamp()
    WHERE id = '70000000-0000-0000-0000-000000000001'
      AND consumed_at IS NULL
      AND expires_at > clock_timestamp();
    GET DIAGNOSTICS affected = ROW_COUNT;
    IF affected <> 0 THEN
        RAISE EXCEPTION 'verification token replay unexpectedly succeeded';
    END IF;
END;
$test$;

UPDATE identities
SET status = 'active'
WHERE id = '10000000-0000-0000-0000-000000000010'
  AND status = 'pending';
UPDATE identity_emails
SET verified_at = now()
WHERE identity_id = '10000000-0000-0000-0000-000000000010';

DO $test$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM identities identity
        JOIN identity_emails email ON email.identity_id = identity.id
        WHERE identity.id = '10000000-0000-0000-0000-000000000010'
          AND identity.status = 'active'
          AND email.verified_at IS NOT NULL
    ) THEN
        RAISE EXCEPTION 'verified pending identity did not become active';
    END IF;
END;
$test$;

INSERT INTO email_verification_tokens (
    id, identity_id, token_hash, created_at, expires_at
)
VALUES (
    '70000000-0000-0000-0000-000000000002',
    '10000000-0000-0000-0000-000000000010',
    digest('expired-verification-token', 'sha256'),
    now() - interval '2 hours',
    now() - interval '1 hour'
);

DO $test$
DECLARE
    affected integer;
BEGIN
    UPDATE email_verification_tokens
    SET consumed_at = clock_timestamp()
    WHERE id = '70000000-0000-0000-0000-000000000002'
      AND consumed_at IS NULL
      AND expires_at > clock_timestamp();
    GET DIAGNOSTICS affected = ROW_COUNT;
    IF affected <> 0 THEN
        RAISE EXCEPTION 'expired verification token unexpectedly succeeded';
    END IF;
END;
$test$;

INSERT INTO webauthn_ceremonies (
    id, identity_id, ceremony_kind, serialized_state, created_at, expires_at
)
VALUES
    (
        '70500000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000010',
        'registration',
        convert_to('{"state":"opaque"}', 'UTF8'),
        now(),
        now() + interval '5 minutes'
    ),
    (
        '70500000-0000-0000-0000-000000000002',
        '10000000-0000-0000-0000-000000000010',
        'authentication',
        convert_to('{"state":"expired"}', 'UTF8'),
        now() - interval '10 minutes',
        now() - interval '5 minutes'
    );

DO $test$
DECLARE
    affected integer;
BEGIN
    UPDATE webauthn_ceremonies
    SET consumed_at = clock_timestamp()
    WHERE id = '70500000-0000-0000-0000-000000000001'
      AND identity_id = '10000000-0000-0000-0000-000000000010'
      AND ceremony_kind = 'registration'
      AND consumed_at IS NULL
      AND expires_at > clock_timestamp();
    GET DIAGNOSTICS affected = ROW_COUNT;
    IF affected <> 1 THEN
        RAISE EXCEPTION 'valid WebAuthn ceremony was not consumed';
    END IF;

    UPDATE webauthn_ceremonies
    SET consumed_at = clock_timestamp()
    WHERE id = '70500000-0000-0000-0000-000000000001'
      AND consumed_at IS NULL
      AND expires_at > clock_timestamp();
    GET DIAGNOSTICS affected = ROW_COUNT;
    IF affected <> 0 THEN
        RAISE EXCEPTION 'WebAuthn ceremony replay unexpectedly succeeded';
    END IF;

    UPDATE webauthn_ceremonies
    SET consumed_at = clock_timestamp()
    WHERE id = '70500000-0000-0000-0000-000000000002'
      AND consumed_at IS NULL
      AND expires_at > clock_timestamp();
    GET DIAGNOSTICS affected = ROW_COUNT;
    IF affected <> 0 THEN
        RAISE EXCEPTION 'expired WebAuthn ceremony unexpectedly succeeded';
    END IF;
END;
$test$;

-- Invitation acceptance verifies both token possession and normalized email,
-- then inserts membership and consumes the invitation atomically.
INSERT INTO identities (id, identity_handle, encrypted_profile, status)
VALUES (
    '10000000-0000-0000-0000-000000000020',
    'invited-member',
    decode('20', 'hex'),
    'active'
);

INSERT INTO identity_emails (identity_id, normalized_email, verified_at)
VALUES (
    '10000000-0000-0000-0000-000000000020',
    'invited@example.test',
    now()
);

INSERT INTO project_invitations (
    id, project_id, invited_by_identity_id, invitee_lookup_hash,
    token_hash, encrypted_payload, role, expires_at
)
VALUES (
    '71000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    digest(convert_to('invited@example.test', 'UTF8'), 'sha256'),
    digest('invitation-token', 'sha256'),
    decode('71', 'hex'),
    'member',
    now() + interval '1 hour'
);

SELECT set_config(
    'app.identity_id',
    '10000000-0000-0000-0000-000000000020',
    true
);
SELECT set_config(
    'app.device_id',
    '20000000-0000-0000-0000-000000000020',
    true
);

DO $test$
BEGIN
    IF NOT sprout_private.accept_project_invitation(
        '30000000-0000-0000-0000-000000000001',
        '71000000-0000-0000-0000-000000000001',
        digest('invitation-token', 'sha256'),
        '10000000-0000-0000-0000-000000000020'
    ) THEN
        RAISE EXCEPTION 'valid invitation was not accepted';
    END IF;

    IF sprout_private.accept_project_invitation(
        '30000000-0000-0000-0000-000000000001',
        '71000000-0000-0000-0000-000000000001',
        digest('invitation-token', 'sha256'),
        '10000000-0000-0000-0000-000000000020'
    ) THEN
        RAISE EXCEPTION 'invitation replay unexpectedly succeeded';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM project_memberships
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND identity_id = '10000000-0000-0000-0000-000000000020'
          AND role = 'member'
          AND state = 'active'
    ) THEN
        RAISE EXCEPTION 'invitation did not create active membership';
    END IF;
END;
$test$;

-- Suggestions include prior collaborators who are not already in the target
-- project and rank broader collaboration first.
INSERT INTO identities (id, identity_handle, encrypted_profile, status)
VALUES
    (
        '10000000-0000-0000-0000-000000000030',
        'candidate-one',
        decode('30', 'hex'),
        'active'
    ),
    (
        '10000000-0000-0000-0000-000000000031',
        'candidate-two',
        decode('31', 'hex'),
        'active'
    ),
    (
        '10000000-0000-0000-0000-000000000032',
        'candidate-three',
        decode('32', 'hex'),
        'active'
    );

INSERT INTO projects (
    id, owner_identity_id, encrypted_metadata, updated_at
)
VALUES
    (
        '30000000-0000-0000-0000-000000000003',
        '10000000-0000-0000-0000-000000000001',
        decode('03', 'hex'),
        now()
    ),
    (
        '30000000-0000-0000-0000-000000000004',
        '10000000-0000-0000-0000-000000000001',
        decode('04', 'hex'),
        now() - interval '1 hour'
    );

INSERT INTO project_memberships (project_id, identity_id, role, state, joined_at)
VALUES
    (
        '30000000-0000-0000-0000-000000000003',
        '10000000-0000-0000-0000-000000000001',
        'owner',
        'active',
        now() - interval '3 days'
    ),
    (
        '30000000-0000-0000-0000-000000000002',
        '10000000-0000-0000-0000-000000000030',
        'member',
        'active',
        now() - interval '2 days'
    ),
    (
        '30000000-0000-0000-0000-000000000002',
        '10000000-0000-0000-0000-000000000031',
        'member',
        'active',
        now() - interval '2 days'
    ),
    (
        '30000000-0000-0000-0000-000000000003',
        '10000000-0000-0000-0000-000000000031',
        'member',
        'active',
        now() - interval '1 day'
    ),
    (
        '30000000-0000-0000-0000-000000000004',
        '10000000-0000-0000-0000-000000000001',
        'owner',
        'active',
        now() - interval '4 days'
    ),
    (
        '30000000-0000-0000-0000-000000000004',
        '10000000-0000-0000-0000-000000000032',
        'member',
        'active',
        now() - interval '3 days'
    );

SELECT set_config(
    'app.identity_id',
    '10000000-0000-0000-0000-000000000001',
    true
);

DO $test$
DECLARE
    top_identity uuid;
    top_shared_count bigint;
    ranked_identities uuid[];
    ranked_recency timestamptz[];
BEGIN
    SELECT identity_id, shared_project_count
    INTO top_identity, top_shared_count
    FROM sprout_private.suggest_project_participants(
        '30000000-0000-0000-0000-000000000001',
        'candidate-',
        10
    )
    LIMIT 1;

    IF top_identity <> '10000000-0000-0000-0000-000000000031'
       OR top_shared_count <> 2
    THEN
        RAISE EXCEPTION 'participant suggestions were not ranked by shared projects';
    END IF;

    SELECT
        array_agg(
            identity_id
            ORDER BY
                shared_project_count DESC,
                most_recent_shared_project_at DESC,
                identity_handle,
                identity_id
        ),
        array_agg(
            most_recent_shared_project_at
            ORDER BY
                shared_project_count DESC,
                most_recent_shared_project_at DESC,
                identity_handle,
                identity_id
        )
    INTO ranked_identities, ranked_recency
    FROM sprout_private.suggest_project_participants(
        '30000000-0000-0000-0000-000000000001',
        'candidate-',
        10
    );

    IF ranked_identities[1] <> '10000000-0000-0000-0000-000000000031'
       OR ranked_identities[2] <> '10000000-0000-0000-0000-000000000032'
       OR ranked_identities[3] <> '10000000-0000-0000-0000-000000000030'
       OR ranked_recency[2] <= ranked_recency[3]
    THEN
        RAISE EXCEPTION
            'participant suggestion tie was not ranked by project modification time: ids=%, recency=%',
            ranked_identities,
            ranked_recency;
    END IF;
END;
$test$;

-- HLT-02 / T-LLR-02.1 / T-LLR-02.2 / T-LLR-02.3 / T-LLR-02.4 /
-- T-LLR-02.5 / T-LLR-02.7 / T-LLR-02.8 / T-LLR-10.2:
-- materialized hierarchy,
-- independent origins, assignment preservation, and fail-closed project
-- boundaries. T-LLR-02.1 and T-LLR-02.5 have API matrices in
-- apps/server/tests/requirements.rs.
SELECT sprout_private.grant_hierarchical_permission(
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000004',
    '10000000-0000-0000-0000-000000000020',
    'edit',
    'full',
    'restricted',
    '80000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001'
);

DO $test$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM task_permissions
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND task_id = '52000000-0000-0000-0000-000000000001'
          AND member_identity_id = '10000000-0000-0000-0000-000000000020'
          AND root_grant_id = '80000000-0000-0000-0000-000000000001'
          AND id = root_grant_id
          AND access_scope = 'full'
          AND grant_origin = 'explicit'
          AND revoked_at IS NULL
    ) OR NOT EXISTS (
        SELECT 1
        FROM task_list_permissions
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND task_list_id = '51000000-0000-0000-0000-000000000001'
          AND root_grant_id = '80000000-0000-0000-0000-000000000001'
          AND access_scope = 'container_only'
          AND grant_origin = 'materialized'
          AND revoked_at IS NULL
    ) OR NOT EXISTS (
        SELECT 1
        FROM topic_permissions
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND topic_id = '50000000-0000-0000-0000-000000000001'
          AND root_grant_id = '80000000-0000-0000-0000-000000000001'
          AND access_scope = 'container_only'
          AND grant_origin = 'materialized'
          AND revoked_at IS NULL
    ) THEN
        RAISE EXCEPTION 'task grant did not materialize full task and container-only ancestors';
    END IF;
END;
$test$;

INSERT INTO resource_nodes (
    id, project_id, parent_id, node_kind,
    encrypted_metadata, created_by_identity_id
)
VALUES (
    '40000000-0000-0000-0000-000000000041',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    'task',
    decode('41', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO tasks (
    id, project_id, task_list_id, resource_node_id, encrypted_payload
)
VALUES (
    '52000000-0000-0000-0000-000000000041',
    '30000000-0000-0000-0000-000000000001',
    '51000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000041',
    decode('41', 'hex')
);

DO $test$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM task_permissions
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND task_id = '52000000-0000-0000-0000-000000000041'
          AND root_grant_id = '80000000-0000-0000-0000-000000000001'
          AND revoked_at IS NULL
    ) THEN
        RAISE EXCEPTION 'child grant leaked to a sibling task';
    END IF;
END;
$test$;

SELECT sprout_private.grant_hierarchical_permission(
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    '10000000-0000-0000-0000-000000000020',
    'view',
    'full',
    'restricted',
    '80000000-0000-0000-0000-000000000002',
    '10000000-0000-0000-0000-000000000001'
);

INSERT INTO task_assignments (
    id, project_id, task_id, assignee_identity_id,
    assigned_by_identity_id, encrypted_payload, permission_root_grant_id
)
VALUES (
    '54000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '52000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000020',
    '10000000-0000-0000-0000-000000000001',
    decode('54', 'hex'),
    '80000000-0000-0000-0000-000000000003'
);
SELECT sprout_private.grant_hierarchical_permission(
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000004',
    '10000000-0000-0000-0000-000000000020',
    'edit',
    'full',
    'restricted',
    '80000000-0000-0000-0000-000000000003',
    '10000000-0000-0000-0000-000000000001',
    'assignment',
    '54000000-0000-0000-0000-000000000001'
);

INSERT INTO resource_nodes (
    id, project_id, parent_id, node_kind,
    encrypted_metadata, created_by_identity_id
)
VALUES (
    '40000000-0000-0000-0000-000000000042',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    'task',
    decode('42', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO tasks (
    id, project_id, task_list_id, resource_node_id, encrypted_payload
)
VALUES (
    '52000000-0000-0000-0000-000000000042',
    '30000000-0000-0000-0000-000000000001',
    '51000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000042',
    decode('42', 'hex')
);

DO $test$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM task_permissions
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND task_id = '52000000-0000-0000-0000-000000000042'
          AND root_grant_id = '80000000-0000-0000-0000-000000000002'
          AND access_scope = 'full'
          AND revoked_at IS NULL
    ) OR EXISTS (
        SELECT 1
        FROM task_permissions
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND task_id = '52000000-0000-0000-0000-000000000042'
          AND root_grant_id IN (
              '80000000-0000-0000-0000-000000000001',
              '80000000-0000-0000-0000-000000000003'
          )
          AND revoked_at IS NULL
    ) THEN
        RAISE EXCEPTION 'new descendant did not inherit only active ancestor full grants';
    END IF;
END;
$test$;

SELECT sprout_private.revoke_hierarchical_permission(
    '30000000-0000-0000-0000-000000000001',
    '80000000-0000-0000-0000-000000000002',
    '10000000-0000-0000-0000-000000000020',
    '10000000-0000-0000-0000-000000000001',
    decode('aabb', 'hex')
);

DO $test$
DECLARE
    list_scope text;
BEGIN
    IF EXISTS (
        SELECT 1
        FROM sprout_private.domain_permission_rows
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND root_grant_id = '80000000-0000-0000-0000-000000000002'
          AND revoked_at IS NULL
    ) OR NOT EXISTS (
        SELECT 1
        FROM task_permissions
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND task_id = '52000000-0000-0000-0000-000000000001'
          AND root_grant_id IN (
              '80000000-0000-0000-0000-000000000001',
              '80000000-0000-0000-0000-000000000003'
          )
          AND revoked_at IS NULL
    ) THEN
        RAISE EXCEPTION 'revocation did not isolate the selected root lineage';
    END IF;

    SELECT access_scope INTO list_scope
    FROM sprout_private.effective_domain_permission(
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000003',
        '10000000-0000-0000-0000-000000000020'
    );
    IF list_scope <> 'container_only' THEN
        RAISE EXCEPTION 'container-only ancestor header permission was not preserved';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM sprout_private.effective_domain_permission(
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000041',
            '10000000-0000-0000-0000-000000000020'
        )
    ) THEN
        RAISE EXCEPTION 'container-only permission exposed a sibling task body';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM notifications
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND notification_kind = 'assigned_task_list_access_removed'
          AND state = 'pending'
    ) THEN
        RAISE EXCEPTION 'list access removal did not queue an admin notification';
    END IF;
END;
$test$;

-- Complete HLT-02 with an independent third participant whose ancestor grant
-- is materialized through the populated tree and revoked without affecting
-- the second participant's unrelated direct and assignment origins.
INSERT INTO project_memberships (
    project_id, identity_id, role, state, joined_at
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000030',
    'member',
    'active',
    now()
);

SELECT sprout_private.grant_hierarchical_permission(
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000002',
    '10000000-0000-0000-0000-000000000030',
    'view',
    'full',
    'restricted',
    '80000000-0000-0000-0000-000000000005',
    '10000000-0000-0000-0000-000000000001'
);

DO $test$
DECLARE
    visible_resources integer;
BEGIN
    SELECT count(DISTINCT resource_node_id)
    INTO visible_resources
    FROM sprout_private.domain_permission_rows
    WHERE project_id = '30000000-0000-0000-0000-000000000001'
      AND member_identity_id = '10000000-0000-0000-0000-000000000030'
      AND root_grant_id = '80000000-0000-0000-0000-000000000005'
      AND access_scope = 'full'
      AND revoked_at IS NULL;

    IF visible_resources <> 5 THEN
        RAISE EXCEPTION
            'third-participant ancestor grant did not cover the five-node subtree: %',
            visible_resources;
    END IF;
END;
$test$;

SELECT sprout_private.revoke_hierarchical_permission(
    '30000000-0000-0000-0000-000000000001',
    '80000000-0000-0000-0000-000000000005',
    '10000000-0000-0000-0000-000000000030',
    '10000000-0000-0000-0000-000000000001',
    decode('ccdd', 'hex')
);

DO $test$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM sprout_private.domain_permission_rows
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND member_identity_id = '10000000-0000-0000-0000-000000000030'
          AND root_grant_id = '80000000-0000-0000-0000-000000000005'
          AND revoked_at IS NULL
    ) OR NOT EXISTS (
        SELECT 1
        FROM task_permissions
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND task_id = '52000000-0000-0000-0000-000000000001'
          AND member_identity_id = '10000000-0000-0000-0000-000000000020'
          AND root_grant_id IN (
              '80000000-0000-0000-0000-000000000001',
              '80000000-0000-0000-0000-000000000003'
          )
          AND revoked_at IS NULL
    ) THEN
        RAISE EXCEPTION 'third-participant revocation affected another origin';
    END IF;
END;
$test$;

DO $test$
BEGIN
    BEGIN
        PERFORM sprout_private.grant_hierarchical_permission(
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000005',
            '10000000-0000-0000-0000-000000000020',
            'view',
            'full',
            'restricted',
            '80000000-0000-0000-0000-000000000004',
            '10000000-0000-0000-0000-000000000001'
        );
        RAISE EXCEPTION 'cross-project permission grant unexpectedly succeeded';
    EXCEPTION
        WHEN foreign_key_violation THEN NULL;
    END;
END;
$test$;

-- HLT-03 / T-LLR-03.2 / T-LLR-03.3 / T-LLR-03.4 / T-LLR-03.5 /
-- T-LLR-03.6 / T-LLR-07.5: exact per-pretask values, immutable snapshots,
-- assignee-only completion, copy provenance, and recurrence
-- atomicity/idempotency.
INSERT INTO presets (
    id, project_id, encrypted_metadata, created_by_identity_id
)
VALUES (
    '56000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    decode('5601', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);

INSERT INTO preset_versions (
    id, project_id, preset_id, version_number,
    encrypted_payload, content_hash, created_by_identity_id
)
VALUES (
    '56100000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '56000000-0000-0000-0000-000000000001',
    1,
    decode('5611', 'hex'),
    decode(repeat('56', 16), 'hex'),
    '10000000-0000-0000-0000-000000000001'
);

INSERT INTO preset_pretasks (
    id, project_id, preset_version_id, client_key,
    ordinal, task_kind, encrypted_payload
)
VALUES
    (
        '56200000-0000-0000-0000-000000000001',
        '30000000-0000-0000-0000-000000000001',
        '56100000-0000-0000-0000-000000000001',
        '56200000-0000-0000-0000-000000000011',
        0, 'priority', decode('5621', 'hex')
    ),
    (
        '56200000-0000-0000-0000-000000000002',
        '30000000-0000-0000-0000-000000000001',
        '56100000-0000-0000-0000-000000000001',
        '56200000-0000-0000-0000-000000000012',
        1, 'deadline', decode('5622', 'hex')
    ),
    (
        '56200000-0000-0000-0000-000000000003',
        '30000000-0000-0000-0000-000000000001',
        '56100000-0000-0000-0000-000000000001',
        '56200000-0000-0000-0000-000000000013',
        2, 'recurring', decode('5623', 'hex')
    );

INSERT INTO recurrence_series (
    id, project_id, task_list_id, encrypted_rule, created_by_identity_id
)
VALUES (
    '55000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '51000000-0000-0000-0000-000000000001',
    decode('5501', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);

INSERT INTO preset_assignments (
    id, project_id, preset_version_id, destination_task_list_id,
    assigned_to_identity_id, assigned_by_identity_id, encrypted_payload
)
VALUES
    (
        '56300000-0000-0000-0000-000000000001',
        '30000000-0000-0000-0000-000000000001',
        '56100000-0000-0000-0000-000000000001',
        '51000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001',
        decode('5631', 'hex')
    ),
    (
        '56300000-0000-0000-0000-000000000002',
        '30000000-0000-0000-0000-000000000001',
        '56100000-0000-0000-0000-000000000001',
        '51000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001',
        decode('5632', 'hex')
    );

INSERT INTO preset_assignment_values (
    project_id, preset_assignment_id, preset_version_id,
    pretask_id, task_kind, encrypted_selected_value
)
VALUES
    (
        '30000000-0000-0000-0000-000000000001',
        '56300000-0000-0000-0000-000000000001',
        '56100000-0000-0000-0000-000000000001',
        '56200000-0000-0000-0000-000000000001',
        'priority', decode('5701', 'hex')
    ),
    (
        '30000000-0000-0000-0000-000000000001',
        '56300000-0000-0000-0000-000000000001',
        '56100000-0000-0000-0000-000000000001',
        '56200000-0000-0000-0000-000000000002',
        'deadline', decode('5702', 'hex')
    ),
    (
        '30000000-0000-0000-0000-000000000001',
        '56300000-0000-0000-0000-000000000001',
        '56100000-0000-0000-0000-000000000001',
        '56200000-0000-0000-0000-000000000003',
        'recurring', decode('5703', 'hex')
    ),
    (
        '30000000-0000-0000-0000-000000000001',
        '56300000-0000-0000-0000-000000000002',
        '56100000-0000-0000-0000-000000000001',
        '56200000-0000-0000-0000-000000000001',
        'deadline', decode('5704', 'hex')
    );

DO $test$
BEGIN
    BEGIN
        UPDATE preset_assignments
        SET state = 'materialized', materialized_at = clock_timestamp()
        WHERE id = '56300000-0000-0000-0000-000000000002';
        RAISE EXCEPTION 'missing/incompatible pretask values unexpectedly materialized';
    EXCEPTION
        WHEN check_violation THEN NULL;
    END;
END;
$test$;

INSERT INTO resource_nodes (
    id, project_id, parent_id, node_kind,
    encrypted_metadata, created_by_identity_id
)
VALUES
    (
        '40000000-0000-0000-0000-000000000061',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000003',
        'task', decode('6101', 'hex'),
        '10000000-0000-0000-0000-000000000001'
    ),
    (
        '40000000-0000-0000-0000-000000000062',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000003',
        'task', decode('6201', 'hex'),
        '10000000-0000-0000-0000-000000000001'
    ),
    (
        '40000000-0000-0000-0000-000000000063',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000003',
        'task', decode('6301', 'hex'),
        '10000000-0000-0000-0000-000000000001'
    );

INSERT INTO resource_epochs (
    id, project_id, resource_node_id, epoch,
    created_by_identity_id, created_by_device_id,
    created_by_device_key_version, key_commitment, reason
)
VALUES
    (
        '60000000-0000-0000-0000-000000000061',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000061', 1,
        '10000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000001', 1,
        decode(repeat('61', 16), 'hex'), 'created'
    ),
    (
        '60000000-0000-0000-0000-000000000062',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000062', 1,
        '10000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000001', 1,
        decode(repeat('62', 16), 'hex'), 'created'
    ),
    (
        '60000000-0000-0000-0000-000000000063',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000063', 1,
        '10000000-0000-0000-0000-000000000001',
        '20000000-0000-0000-0000-000000000001', 1,
        decode(repeat('63', 16), 'hex'), 'created'
    );

INSERT INTO resource_key_envelopes (
    project_id, resource_node_id, epoch, envelope_version,
    recipient_identity_id, recipient_device_id,
    recipient_device_key_version, encrypted_key, sender_signature,
    sender_post_quantum_signature, created_by_identity_id,
    created_by_device_id, created_by_device_key_version
)
SELECT
    '30000000-0000-0000-0000-000000000001',
    resource_id,
    1, 1,
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    1,
    decode(repeat('64', 16), 'hex'),
    decode(repeat('65', 64), 'hex'),
    decode('66', 'hex'),
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    1
FROM unnest(ARRAY[
    '40000000-0000-0000-0000-000000000061'::uuid,
    '40000000-0000-0000-0000-000000000062'::uuid,
    '40000000-0000-0000-0000-000000000063'::uuid
]) resource_id;

INSERT INTO tasks (
    id, project_id, task_list_id, resource_node_id,
    task_kind, encrypted_payload, encrypted_value_snapshot,
    source_pretask_id, preset_assignment_id, created_by_identity_id,
    recurrence_series_id, occurrence_number
)
VALUES
    (
        '56400000-0000-0000-0000-000000000001',
        '30000000-0000-0000-0000-000000000001',
        '51000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000061',
        'priority', decode('6101', 'hex'), decode('5701', 'hex'),
        '56200000-0000-0000-0000-000000000001',
        '56300000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001', NULL, NULL
    ),
    (
        '56400000-0000-0000-0000-000000000002',
        '30000000-0000-0000-0000-000000000001',
        '51000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000062',
        'deadline', decode('6201', 'hex'), decode('5702', 'hex'),
        '56200000-0000-0000-0000-000000000002',
        '56300000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001', NULL, NULL
    ),
    (
        '56400000-0000-0000-0000-000000000003',
        '30000000-0000-0000-0000-000000000001',
        '51000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000063',
        'recurring', decode('6301', 'hex'), decode('5703', 'hex'),
        '56200000-0000-0000-0000-000000000003',
        '56300000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001',
        '55000000-0000-0000-0000-000000000001', 1
    );

INSERT INTO preset_assignment_materialized_tasks (
    project_id, preset_assignment_id, preset_version_id,
    pretask_id, task_id, task_kind,
    encrypted_selected_value_snapshot, encrypted_task_snapshot
)
VALUES
    (
        '30000000-0000-0000-0000-000000000001',
        '56300000-0000-0000-0000-000000000001',
        '56100000-0000-0000-0000-000000000001',
        '56200000-0000-0000-0000-000000000001',
        '56400000-0000-0000-0000-000000000001',
        'priority', decode('5701', 'hex'), decode('6101', 'hex')
    ),
    (
        '30000000-0000-0000-0000-000000000001',
        '56300000-0000-0000-0000-000000000001',
        '56100000-0000-0000-0000-000000000001',
        '56200000-0000-0000-0000-000000000002',
        '56400000-0000-0000-0000-000000000002',
        'deadline', decode('5702', 'hex'), decode('6201', 'hex')
    ),
    (
        '30000000-0000-0000-0000-000000000001',
        '56300000-0000-0000-0000-000000000001',
        '56100000-0000-0000-0000-000000000001',
        '56200000-0000-0000-0000-000000000003',
        '56400000-0000-0000-0000-000000000003',
        'recurring', decode('5703', 'hex'), decode('6301', 'hex')
    );

INSERT INTO task_assignments (
    id, project_id, task_id, assignee_identity_id,
    assigned_by_identity_id, encrypted_payload, permission_root_grant_id
)
VALUES
    (
        '56500000-0000-0000-0000-000000000001',
        '30000000-0000-0000-0000-000000000001',
        '56400000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001',
        decode('6501', 'hex'),
        '56500000-0000-0000-0000-000000000011'
    ),
    (
        '56500000-0000-0000-0000-000000000002',
        '30000000-0000-0000-0000-000000000001',
        '56400000-0000-0000-0000-000000000002',
        '10000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001',
        decode('6502', 'hex'),
        '56500000-0000-0000-0000-000000000012'
    ),
    (
        '56500000-0000-0000-0000-000000000003',
        '30000000-0000-0000-0000-000000000001',
        '56400000-0000-0000-0000-000000000003',
        '10000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001',
        decode('6503', 'hex'),
        '56500000-0000-0000-0000-000000000013'
    );

UPDATE preset_assignments
SET
    state = 'materialized',
    materialized_at = clock_timestamp(),
    payload_version = payload_version + 1
WHERE id = '56300000-0000-0000-0000-000000000001';

DO $test$
DECLARE
    mixed_count bigint;
    original_snapshot bytea;
BEGIN
    SELECT count(DISTINCT task_kind) INTO mixed_count
    FROM tasks
    WHERE task_list_id = '51000000-0000-0000-0000-000000000001'
      AND id IN (
          '56400000-0000-0000-0000-000000000001',
          '56400000-0000-0000-0000-000000000002',
          '56400000-0000-0000-0000-000000000003'
      );
    IF mixed_count <> 3 THEN
        RAISE EXCEPTION 'mixed task kinds were not stored in one list';
    END IF;

    SELECT encrypted_payload INTO original_snapshot
    FROM tasks
    WHERE id = '56400000-0000-0000-0000-000000000001';
    BEGIN
        UPDATE preset_pretasks
        SET encrypted_payload = decode('ffff', 'hex')
        WHERE id = '56200000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'historical pretask edit unexpectedly succeeded';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
    UPDATE presets
    SET state = 'deleted', deleted_at = clock_timestamp()
    WHERE id = '56000000-0000-0000-0000-000000000001';
    IF (SELECT encrypted_payload FROM tasks
        WHERE id = '56400000-0000-0000-0000-000000000001')
       <> original_snapshot
    THEN
        RAISE EXCEPTION 'template deletion changed a concrete task snapshot';
    END IF;
END;
$test$;

DO $test$
BEGIN
    BEGIN
        INSERT INTO task_completions (
            project_id, task_id, assignment_id,
            assignee_identity_id, recorded_by_identity_id,
            occurrence_key, encrypted_payload, completed_at
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '56400000-0000-0000-0000-000000000001',
            '56500000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000020',
            '10000000-0000-0000-0000-000000000020',
            '56500000-0000-0000-0000-000000000021',
            decode('6502', 'hex'), now()
        );
        RAISE EXCEPTION 'non-assignee completion unexpectedly succeeded';
    EXCEPTION
        WHEN insufficient_privilege THEN NULL;
    END;

    -- The project owner is deliberately not the assignee of this task.
    BEGIN
        INSERT INTO task_completions (
            project_id, task_id, assignment_id,
            assignee_identity_id, recorded_by_identity_id,
            occurrence_key, encrypted_payload, completed_at
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '52000000-0000-0000-0000-000000000001',
            '54000000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000001',
            '56500000-0000-0000-0000-000000000022',
            decode('6502', 'hex'), now()
        );
        RAISE EXCEPTION 'non-assigned owner completion unexpectedly succeeded';
    EXCEPTION
        WHEN insufficient_privilege THEN NULL;
    END;
END;
$test$;

INSERT INTO task_completions (
    id, project_id, task_id, assignment_id,
    assignee_identity_id, recorded_by_identity_id,
    occurrence_key, encrypted_payload, completed_at,
    idempotency_key, request_hash
)
VALUES (
    '56500000-0000-0000-0000-000000000031',
    '30000000-0000-0000-0000-000000000001',
    '56400000-0000-0000-0000-000000000001',
    '56500000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    '56500000-0000-0000-0000-000000000031',
    decode('6503', 'hex'), now(),
    '56500000-0000-0000-0000-000000000032',
    decode(repeat('65', 16), 'hex')
);
UPDATE tasks
SET
    state = 'completed',
    completed_by_identity_id = '10000000-0000-0000-0000-000000000001',
    completed_at = now(),
    payload_version = payload_version + 1
WHERE id = '56400000-0000-0000-0000-000000000001';

DO $test$
BEGIN
    BEGIN
        UPDATE tasks
        SET state = 'open', completed_by_identity_id = NULL, completed_at = NULL,
            payload_version = payload_version + 1
        WHERE id = '56400000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'completed task reopened';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
    BEGIN
        INSERT INTO task_assignments (
            project_id, task_id, assignee_identity_id,
            assigned_by_identity_id, encrypted_payload, permission_root_grant_id
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '56400000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000001',
            decode('6504', 'hex'),
            '56500000-0000-0000-0000-000000000012'
        );
        RAISE EXCEPTION 'completed task reassigned';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
END;
$test$;

INSERT INTO resource_nodes (
    id, project_id, parent_id, node_kind,
    encrypted_metadata, created_by_identity_id
)
VALUES (
    '40000000-0000-0000-0000-000000000064',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    'task', decode('6401', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO tasks (
    id, project_id, task_list_id, resource_node_id,
    task_kind, encrypted_payload, encrypted_value_snapshot,
    copied_from_task_id, created_by_identity_id
)
VALUES (
    '56400000-0000-0000-0000-000000000004',
    '30000000-0000-0000-0000-000000000001',
    '51000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000064',
    'priority', decode('6401', 'hex'), decode('5701', 'hex'),
    '56400000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001'
);

INSERT INTO resource_nodes (
    id, project_id, parent_id, node_kind,
    encrypted_metadata, created_by_identity_id
)
VALUES (
    '40000000-0000-0000-0000-000000000065',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    'task', decode('6501', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO tasks (
    id, project_id, task_list_id, resource_node_id,
    task_kind, encrypted_payload, encrypted_value_snapshot,
    recurrence_series_id, occurrence_number, created_by_identity_id
)
VALUES (
    '56400000-0000-0000-0000-000000000005',
    '30000000-0000-0000-0000-000000000001',
    '51000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000065',
    'recurring', decode('6501', 'hex'), decode('6502', 'hex'),
    '55000000-0000-0000-0000-000000000001', 2,
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO task_completions (
    id, project_id, task_id, assignment_id,
    assignee_identity_id, recorded_by_identity_id,
    occurrence_key, encrypted_payload, completed_at,
    idempotency_key, request_hash,
    recurrence_series_id, occurrence_number, next_task_id
)
VALUES (
    '56500000-0000-0000-0000-000000000041',
    '30000000-0000-0000-0000-000000000001',
    '56400000-0000-0000-0000-000000000003',
    '56500000-0000-0000-0000-000000000003',
    '10000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    '56500000-0000-0000-0000-000000000041',
    decode('6506', 'hex'), now(),
    '56500000-0000-0000-0000-000000000042',
    decode(repeat('66', 16), 'hex'),
    '55000000-0000-0000-0000-000000000001', 2,
    '56400000-0000-0000-0000-000000000005'
);
UPDATE tasks
SET
    state = 'completed',
    completed_by_identity_id = '10000000-0000-0000-0000-000000000001',
    completed_at = now(),
    payload_version = payload_version + 1
WHERE id = '56400000-0000-0000-0000-000000000003';

DO $test$
BEGIN
    BEGIN
        INSERT INTO resource_nodes (
            id, project_id, parent_id, node_kind,
            encrypted_metadata, created_by_identity_id
        )
        VALUES (
            '40000000-0000-0000-0000-000000000066',
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000003',
            'task', decode('6601', 'hex'),
            '10000000-0000-0000-0000-000000000001'
        );
        INSERT INTO tasks (
            id, project_id, task_list_id, resource_node_id,
            task_kind, encrypted_payload, encrypted_value_snapshot,
            recurrence_series_id, occurrence_number, created_by_identity_id
        )
        VALUES (
            '56400000-0000-0000-0000-000000000006',
            '30000000-0000-0000-0000-000000000001',
            '51000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000066',
            'recurring', decode('6601', 'hex'), decode('6602', 'hex'),
            '55000000-0000-0000-0000-000000000001', 3,
            '10000000-0000-0000-0000-000000000001'
        );
        RAISE EXCEPTION 'injected recurrence crash';
    EXCEPTION
        WHEN raise_exception THEN NULL;
    END;
    IF EXISTS (
        SELECT 1 FROM tasks
        WHERE id = '56400000-0000-0000-0000-000000000006'
    ) THEN
        RAISE EXCEPTION 'crashed recurrence exposed a partial next task';
    END IF;

    BEGIN
        INSERT INTO resource_nodes (
            id, project_id, parent_id, node_kind,
            encrypted_metadata, created_by_identity_id
        )
        VALUES (
            '40000000-0000-0000-0000-000000000067',
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000003',
            'task', decode('6701', 'hex'),
            '10000000-0000-0000-0000-000000000001'
        );
        INSERT INTO tasks (
            id, project_id, task_list_id, resource_node_id,
            task_kind, encrypted_payload, encrypted_value_snapshot,
            recurrence_series_id, occurrence_number, created_by_identity_id
        )
        VALUES (
            '56400000-0000-0000-0000-000000000007',
            '30000000-0000-0000-0000-000000000001',
            '51000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000067',
            'recurring', decode('6701', 'hex'), decode('6702', 'hex'),
            '55000000-0000-0000-0000-000000000001', 2,
            '10000000-0000-0000-0000-000000000001'
        );
        RAISE EXCEPTION 'duplicate occurrence unexpectedly succeeded';
    EXCEPTION
        WHEN unique_violation THEN NULL;
    END;

    IF (SELECT count(*) FROM tasks
        WHERE recurrence_series_id = '55000000-0000-0000-0000-000000000001'
          AND occurrence_number = 2) <> 1
    THEN
        RAISE EXCEPTION 'duplicate recurrence occurrence was persisted';
    END IF;
END;
$test$;

-- R5.30 persistent-kernel RLS fixture. The member used below can read the
-- scope through normal permissions but is deliberately not a run participant.
SELECT set_config(
    'app.identity_id',
    '10000000-0000-0000-0000-000000000001',
    true
);
SELECT set_config(
    'app.device_id',
    '20000000-0000-0000-0000-000000000001',
    true
);

INSERT INTO identities (
    id, identity_handle, encrypted_profile, principal_kind
) VALUES (
    '91000000-0000-0000-0000-000000000001',
    'completion-kernel-agent', decode('91', 'hex'), 'agent'
);
INSERT INTO project_memberships (
    project_id, identity_id, role, state, suspended_at
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '91000000-0000-0000-0000-000000000001', 'member',
    'suspended', clock_timestamp()
);
INSERT INTO governed_agents (
    id, project_id, principal_identity_id, controller_identity_id,
    profile_resource_node_id, encrypted_system_prompt, key_epoch,
    availability
) VALUES (
    '92000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '91000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    decode('92', 'hex'), 1, 'controller_private'
);
-- This fixture exercises the completion bridge, not the endpoint signature
-- verifier. The migration-owner test harness supplies the minimal structural
-- witness that 0029 now requires for an active LocalGoal; application roles
-- cannot perform this direct verified insert.
INSERT INTO agent_compilation_certificates (
    id, project_id, task_kind, compiler_name, compiler_version,
    compiler_build_digest, signer_identity_id, signer_device_id,
    signer_device_key_version, subject_id, subject_revision, draft_id,
    agent_principal_identity_id, controller_identity_id,
    input_commitment, ciphertext_commitment, canonical_output, output_hash,
    compilation_envelope, envelope_hash, certificate_hash, idempotency_key,
    classical_signature, post_quantum_signature, classifier_version,
    classifier_output_hash, authorization_kind, authorization_id,
    verification_state, verified_at
) VALUES (
    '92900000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    'local_goal', 'sprout.local-goal.compiler', 1,
    decode('0c675e853701375c7ba5d396f4e1f9b55592339a3a4e45859b9f2c2e8fdbbfc2', 'hex'),
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001', 1,
    '93000000-0000-0000-0000-000000000001', 1,
    '92900000-0000-0000-0000-000000000002',
    '91000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    decode(repeat('90', 32), 'hex'), decode(repeat('91', 32), 'hex'),
    '{}'::jsonb, decode(repeat('92', 32), 'hex'),
    '{}'::jsonb, decode(repeat('93', 32), 'hex'),
    decode(repeat('94', 32), 'hex'),
    '92900000-0000-0000-0000-000000000003',
    decode(repeat('95', 64), 'hex'), decode('96', 'hex'),
    1, decode(repeat('97', 32), 'hex'),
    'administrator_creation',
    '92900000-0000-0000-0000-000000000004',
    'verified', clock_timestamp()
);
INSERT INTO agent_local_goal_contracts (
    id, project_id, agent_id, agent_identity_id,
    controller_identity_id, revision, contract, contract_hash, state,
    compilation_certificate_id, classifier_version, classifier_output_hash
) VALUES (
    '93000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '92000000-0000-0000-0000-000000000001',
    '91000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    1, '{}'::jsonb, decode(repeat('93', 32), 'hex'), 'active',
    '92900000-0000-0000-0000-000000000001', 1,
    decode(repeat('97', 32), 'hex')
);
INSERT INTO agent_collaborative_runs (
    id, project_id, goal_id, scope_resource_node_id,
    local_goal_id, local_goal_revision,
    contract, contract_hash, state, state_hash,
    state_version, goal_status, run_status, created_by_identity_id
) VALUES (
    '94000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '94100000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    '93000000-0000-0000-0000-000000000001', 1,
    '{}'::jsonb, decode(repeat('94', 32), 'hex'),
    '{"blockers":{"95000000-0000-0000-0000-000000000001":{"status":"waiting","created_at":1,"terminal_at":null}}}'::jsonb,
    decode(repeat('95', 32), 'hex'),
    1, 'active', 'running',
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO agent_run_participants (
    project_id, run_id, identity_id, participant_role
) VALUES (
    '30000000-0000-0000-0000-000000000001',
    '94000000-0000-0000-0000-000000000001',
    '91000000-0000-0000-0000-000000000001', 'agent'
);
INSERT INTO agent_run_transitions (
    id, project_id, run_id, state_version, transition_kind,
    runtime_actor_kind, actor_identity_id,
    next_state_hash, facts_hash, state_snapshot, fact_references
) VALUES (
    '96000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '94000000-0000-0000-0000-000000000001', 1, 'initialized',
    'principal', '10000000-0000-0000-0000-000000000001',
    decode(repeat('95', 32), 'hex'), decode(repeat('96', 32), 'hex'),
    '{"blockers":{"95000000-0000-0000-0000-000000000001":{"status":"waiting","created_at":1,"terminal_at":null}},"blocker_resolutions":[]}'::jsonb,
    '{}'::jsonb
);
INSERT INTO agent_run_blockers (
    id, project_id, run_id, obligation_id, waiting_rule_ordinal,
    scope, waiting_condition, current_status, created_tick
) VALUES (
    '95000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '94000000-0000-0000-0000-000000000001',
    '95100000-0000-0000-0000-000000000001', 1,
    '{"kind":"goal","goal":"94100000-0000-0000-0000-000000000001"}'::jsonb,
    '{"kind":"external_outcome","condition":"95200000-0000-0000-0000-000000000001"}'::jsonb,
    'waiting', 1
);
INSERT INTO agent_run_work_slots (
    project_id, run_id, work_spec_ordinal, slot, work_item_id
) VALUES (
    '30000000-0000-0000-0000-000000000001',
    '94000000-0000-0000-0000-000000000001', 1, 0,
    '97000000-0000-0000-0000-000000000001'
);
INSERT INTO agent_run_claim_leases (
    id, project_id, run_id, work_item_id, attempt,
    claimant_identity_id, acquired_at, expires_at, status
) VALUES (
    '98000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '94000000-0000-0000-0000-000000000001',
    '97000000-0000-0000-0000-000000000001', 1,
    '91000000-0000-0000-0000-000000000001',
    clock_timestamp(), clock_timestamp() + interval '5 minutes', 'active'
);

DO $test$
BEGIN
    BEGIN
        UPDATE agent_run_blockers
        SET current_status = 'cancelled', terminal_tick = 2
        WHERE id = '95000000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'direct blocker status mutation bypassed domain transition';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
    BEGIN
        INSERT INTO agent_run_blocker_resolutions (
            project_id, run_id, blocker_id, observation_kind,
            observation_id, terminal_status, observed_at,
            provenance_hash, transition_id
        ) VALUES (
            '30000000-0000-0000-0000-000000000001',
            '94000000-0000-0000-0000-000000000001',
            '95000000-0000-0000-0000-000000000001',
            'external_outcome',
            '95200000-0000-0000-0000-000000000001',
            'resolved', clock_timestamp(), decode(repeat('99', 32), 'hex'),
            '96000000-0000-0000-0000-000000000001'
        );
        RAISE EXCEPTION 'forged blocker resolution bypassed domain transition';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
END;
$test$;

-- R5.30 evidence provenance must not infer work causality from an event chosen
-- by the runner. Two task completions are deliberately equivalent by actor,
-- scope and time; only the one named by a preexisting invocation/effect
-- binding may become a work outcome. The outer subtransaction rolls the
-- synthetic product adapter fixture back after proving both branches.
DO $test$
DECLARE
    claim_time timestamptz := clock_timestamp() - interval '3 seconds';
    bound_time timestamptz := clock_timestamp() - interval '2 seconds';
    completion_time timestamptz := clock_timestamp() - interval '1 second';
BEGIN
    BEGIN
        UPDATE project_memberships
        SET state = 'active', suspended_at = NULL
        WHERE project_id = '30000000-0000-0000-0000-000000000001'
          AND identity_id = '91000000-0000-0000-0000-000000000001';

        INSERT INTO devices (
            id, identity_id, device_kind, encrypted_label, trust_state
        ) VALUES (
            '99600000-0000-0000-0000-000000000001',
            '91000000-0000-0000-0000-000000000001',
            'service', decode('9961', 'hex'), 'trusted'
        );

        INSERT INTO resource_nodes (
            id, project_id, parent_id, node_kind,
            encrypted_metadata, created_by_identity_id
        ) VALUES
        (
            '99000000-0000-0000-0000-000000000001',
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000003',
            'task', decode('9901', 'hex'),
            '10000000-0000-0000-0000-000000000001'
        ),
        (
            '99000000-0000-0000-0000-000000000002',
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000003',
            'task', decode('9902', 'hex'),
            '10000000-0000-0000-0000-000000000001'
        );
        INSERT INTO tasks (
            id, project_id, task_list_id, resource_node_id, task_kind,
            encrypted_payload, encrypted_value_snapshot, created_by_identity_id
        ) VALUES
        (
            '99100000-0000-0000-0000-000000000001',
            '30000000-0000-0000-0000-000000000001',
            '51000000-0000-0000-0000-000000000001',
            '99000000-0000-0000-0000-000000000001', 'priority',
            decode('9911', 'hex'), decode('9912', 'hex'),
            '10000000-0000-0000-0000-000000000001'
        ),
        (
            '99100000-0000-0000-0000-000000000002',
            '30000000-0000-0000-0000-000000000001',
            '51000000-0000-0000-0000-000000000001',
            '99000000-0000-0000-0000-000000000002', 'priority',
            decode('9921', 'hex'), decode('9922', 'hex'),
            '10000000-0000-0000-0000-000000000001'
        );
        INSERT INTO task_assignments (
            id, project_id, task_id, assignee_identity_id,
            assigned_by_identity_id, encrypted_payload, permission_root_grant_id
        ) VALUES
        (
            '99200000-0000-0000-0000-000000000001',
            '30000000-0000-0000-0000-000000000001',
            '99100000-0000-0000-0000-000000000001',
            '91000000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000001', decode('9921', 'hex'),
            '99200000-0000-0000-0000-000000000011'
        ),
        (
            '99200000-0000-0000-0000-000000000002',
            '30000000-0000-0000-0000-000000000001',
            '99100000-0000-0000-0000-000000000002',
            '91000000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000001', decode('9922', 'hex'),
            '99200000-0000-0000-0000-000000000012'
        );
        INSERT INTO task_completions (
            id, project_id, task_id, assignment_id,
            assignee_identity_id, recorded_by_identity_id,
            occurrence_key, encrypted_payload, completed_at
        ) VALUES
        (
            '99300000-0000-0000-0000-000000000001',
            '30000000-0000-0000-0000-000000000001',
            '99100000-0000-0000-0000-000000000001',
            '99200000-0000-0000-0000-000000000001',
            '91000000-0000-0000-0000-000000000001',
            '91000000-0000-0000-0000-000000000001',
            '99300000-0000-0000-0000-000000000011', decode('9931', 'hex'),
            completion_time
        ),
        (
            '99300000-0000-0000-0000-000000000002',
            '30000000-0000-0000-0000-000000000001',
            '99100000-0000-0000-0000-000000000002',
            '99200000-0000-0000-0000-000000000002',
            '91000000-0000-0000-0000-000000000001',
            '91000000-0000-0000-0000-000000000001',
            '99300000-0000-0000-0000-000000000012', decode('9932', 'hex'),
            completion_time
        );
        UPDATE tasks
        SET state = 'completed',
            completed_by_identity_id = '91000000-0000-0000-0000-000000000001',
            completed_at = completion_time,
            payload_version = payload_version + 1
        WHERE id IN (
            '99100000-0000-0000-0000-000000000001',
            '99100000-0000-0000-0000-000000000002'
        );

        INSERT INTO agent_task_obligation_provenance (
            id, project_id, task_intent_id, task_resource_node_id,
            target_agent_id, local_goal_id, local_goal_revision,
            obligation_id, work_spec_ordinal
        ) VALUES (
            '99600000-0000-0000-0000-000000000002',
            '30000000-0000-0000-0000-000000000001', NULL,
            '99000000-0000-0000-0000-000000000001',
            '92000000-0000-0000-0000-000000000001',
            '93000000-0000-0000-0000-000000000001', 1,
            '95100000-0000-0000-0000-000000000001', 1
        );

        UPDATE agent_run_claim_leases
        SET acquired_at = claim_time, expires_at = completion_time + interval '5 minutes'
        WHERE id = '98000000-0000-0000-0000-000000000001';
        UPDATE agent_collaborative_runs
        SET state_version = 2,
            state = jsonb_build_object(
                'claims', jsonb_build_object(
                    '98000000-0000-0000-0000-000000000001',
                    jsonb_build_object('status', 'active')
                ),
                'work_items', jsonb_build_object(
                    '97000000-0000-0000-0000-000000000001',
                    jsonb_build_object('status', 'claimed')
                )
            ),
            state_hash = decode(repeat('a2', 32), 'hex')
        WHERE id = '94000000-0000-0000-0000-000000000001';
        INSERT INTO agent_run_transitions (
            id, project_id, run_id, state_version, transition_kind,
            runtime_actor_kind, actor_identity_id, previous_state_hash,
            next_state_hash, facts_hash, state_snapshot, fact_references
        ) VALUES (
            '99700000-0000-0000-0000-000000000001',
            '30000000-0000-0000-0000-000000000001',
            '94000000-0000-0000-0000-000000000001', 2, 'work_claimed',
            'principal', '91000000-0000-0000-0000-000000000001',
            decode(repeat('95', 32), 'hex'), decode(repeat('a2', 32), 'hex'),
            decode(repeat('97', 32), 'hex'),
            jsonb_build_object(
                'claims', jsonb_build_object(
                    '98000000-0000-0000-0000-000000000001',
                    jsonb_build_object('status', 'active')
                )
            ), '{}'::jsonb
        );
        INSERT INTO agent_invocations (
            id, project_id, agent_id, agent_identity_id,
            language_task, authority_envelope, encrypted_input, request_hash,
            status, attempt, max_attempts, completed_at,
            encrypted_output, output_hash, created_by_identity_id
        ) VALUES (
            '99400000-0000-0000-0000-000000000001',
            '30000000-0000-0000-0000-000000000001',
            '92000000-0000-0000-0000-000000000001',
            '91000000-0000-0000-0000-000000000001',
            '{}'::jsonb, '{}'::jsonb, decode('9941', 'hex'),
            decode(repeat('94', 32), 'hex'), 'succeeded', 1, 1, bound_time,
            decode('9942', 'hex'), decode(repeat('95', 32), 'hex'),
            '10000000-0000-0000-0000-000000000001'
        );
        INSERT INTO agent_effect_proposals (
            id, project_id, invocation_id, agent_id, ordinal, effect,
            proposal_hash, status, decided_at, applied_at
        ) VALUES (
            '99500000-0000-0000-0000-000000000001',
            '30000000-0000-0000-0000-000000000001',
            '99400000-0000-0000-0000-000000000001',
            '92000000-0000-0000-0000-000000000001', 0,
            '{"effect":{"resource_id":"99000000-0000-0000-0000-000000000001","operation":"complete_assigned_task"},"materialization":{"kind":"complete_assigned_task"}}'::jsonb,
            decode(repeat('96', 32), 'hex'), 'applied', bound_time, bound_time
        );
        INSERT INTO agent_run_work_product_bindings (
            project_id, run_id, work_item_id, claim_id, attempt,
            invocation_id, effect_id, resource_node_id, bound_at,
            claim_transition_id
        ) VALUES (
            '30000000-0000-0000-0000-000000000001',
            '94000000-0000-0000-0000-000000000001',
            '97000000-0000-0000-0000-000000000001',
            '98000000-0000-0000-0000-000000000001', 1,
            '99400000-0000-0000-0000-000000000001',
            '99500000-0000-0000-0000-000000000001',
            '99000000-0000-0000-0000-000000000001', bound_time,
            '99700000-0000-0000-0000-000000000001'
        );

        INSERT INTO agent_run_task_effects (
            id, project_id, run_id, work_item_id, claim_id, attempt,
            task_provenance_id, task_intent_id, task_resource_node_id,
            task_id, task_assignment_id, task_completion_id,
            target_agent_id, cross_owner_effect_id,
            actor_identity_id, actor_device_id, idempotency_key,
            request_hash, provenance_hash, applied_at
        ) VALUES (
            '99600000-0000-0000-0000-000000000003',
            '30000000-0000-0000-0000-000000000001',
            '94000000-0000-0000-0000-000000000001',
            '97000000-0000-0000-0000-000000000001',
            '98000000-0000-0000-0000-000000000001', 1,
            '99600000-0000-0000-0000-000000000002', NULL,
            '99000000-0000-0000-0000-000000000001',
            '99100000-0000-0000-0000-000000000001',
            '99200000-0000-0000-0000-000000000001',
            '99300000-0000-0000-0000-000000000001',
            '92000000-0000-0000-0000-000000000001', NULL,
            '91000000-0000-0000-0000-000000000001',
            '99600000-0000-0000-0000-000000000001',
            '99600000-0000-0000-0000-000000000004',
            decode(repeat('98', 32), 'hex'),
            decode(repeat('99', 32), 'hex'), completion_time
        );

        UPDATE agent_run_claim_leases
        SET status = 'released', terminal_at = completion_time
        WHERE id = '98000000-0000-0000-0000-000000000001';
        UPDATE agent_collaborative_runs
        SET state_version = 3,
            state = jsonb_build_object(
                'claims', jsonb_build_object(
                    '98000000-0000-0000-0000-000000000001',
                    jsonb_build_object('status', 'released')
                ),
                'work_items', jsonb_build_object(
                    '97000000-0000-0000-0000-000000000001',
                    jsonb_build_object('status', 'succeeded')
                )
            ),
            state_hash = decode(repeat('a3', 32), 'hex')
        WHERE id = '94000000-0000-0000-0000-000000000001';
        INSERT INTO agent_run_transitions (
            id, project_id, run_id, state_version, transition_kind,
            runtime_actor_kind, actor_identity_id, observation_kind, observation_id,
            previous_state_hash, next_state_hash, facts_hash,
            state_snapshot, fact_references
        ) VALUES (
            '99700000-0000-0000-0000-000000000002',
            '30000000-0000-0000-0000-000000000001',
            '94000000-0000-0000-0000-000000000001', 3, 'work_succeeded',
            'principal', '91000000-0000-0000-0000-000000000001',
            'task_completion', '99300000-0000-0000-0000-000000000002',
            decode(repeat('a2', 32), 'hex'), decode(repeat('a3', 32), 'hex'),
            decode(repeat('97', 32), 'hex'),
            jsonb_build_object(
                'claims', jsonb_build_object(
                    '98000000-0000-0000-0000-000000000001',
                    jsonb_build_object('status', 'released')
                ),
                'work_items', jsonb_build_object(
                    '97000000-0000-0000-0000-000000000001',
                    jsonb_build_object('status', 'succeeded')
                )
            ), '{}'::jsonb
        );
        BEGIN
            INSERT INTO agent_run_work_outcomes (
                project_id, run_id, work_item_id, claim_id, attempt,
                outcome_kind, product_event_id, observed_at,
                provenance_hash, transition_id
            ) VALUES (
                '30000000-0000-0000-0000-000000000001',
                '94000000-0000-0000-0000-000000000001',
                '97000000-0000-0000-0000-000000000001',
                '98000000-0000-0000-0000-000000000001', 1,
                'task_completion', '99300000-0000-0000-0000-000000000002',
                completion_time, decode(repeat('98', 32), 'hex'),
                '99700000-0000-0000-0000-000000000002'
            );
            RAISE EXCEPTION 'unbound same-agent task completion became a work outcome';
        EXCEPTION
            WHEN SQLSTATE '55000' THEN NULL;
        END;

        UPDATE agent_collaborative_runs
        SET state_version = 4, state_hash = decode(repeat('a4', 32), 'hex')
        WHERE id = '94000000-0000-0000-0000-000000000001';
        INSERT INTO agent_run_transitions (
            id, project_id, run_id, state_version, transition_kind,
            runtime_actor_kind, actor_identity_id, observation_kind, observation_id,
            previous_state_hash, next_state_hash, facts_hash,
            state_snapshot, fact_references
        ) SELECT
            '99700000-0000-0000-0000-000000000003', project_id, run_id,
            4, transition_kind, runtime_actor_kind, actor_identity_id,
            'task_completion', '99300000-0000-0000-0000-000000000001',
            decode(repeat('a3', 32), 'hex'), decode(repeat('a4', 32), 'hex'),
            facts_hash,
            state_snapshot || jsonb_build_object(
                'causal_links', jsonb_build_array(jsonb_build_object(
                    'predecessor', jsonb_build_object(
                        'kind', 'work',
                        'work', '97000000-0000-0000-0000-000000000001'
                    ),
                    'successor', jsonb_build_object(
                        'kind', 'task',
                        'task', '99000000-0000-0000-0000-000000000001'
                    ),
                    'observed_at', floor(extract(epoch FROM completion_time))::bigint
                ))
            ),
            fact_references
        FROM agent_run_transitions
        WHERE id = '99700000-0000-0000-0000-000000000002';
        INSERT INTO agent_run_causal_links (
            project_id, run_id, goal_id, predecessor, successor,
            observed_tick, transition_id, task_effect_id
        ) VALUES (
            '30000000-0000-0000-0000-000000000001',
            '94000000-0000-0000-0000-000000000001',
            '94100000-0000-0000-0000-000000000001',
            jsonb_build_object(
                'kind', 'work',
                'work', '97000000-0000-0000-0000-000000000001'
            ),
            jsonb_build_object(
                'kind', 'task',
                'task', '99000000-0000-0000-0000-000000000001'
            ),
            floor(extract(epoch FROM completion_time))::bigint,
            '99700000-0000-0000-0000-000000000003',
            '99600000-0000-0000-0000-000000000003'
        );
        INSERT INTO agent_run_work_outcomes (
            project_id, run_id, work_item_id, claim_id, attempt,
            outcome_kind, product_event_id, observed_at,
            provenance_hash, transition_id
        ) VALUES (
            '30000000-0000-0000-0000-000000000001',
            '94000000-0000-0000-0000-000000000001',
            '97000000-0000-0000-0000-000000000001',
            '98000000-0000-0000-0000-000000000001', 1,
            'task_completion', '99300000-0000-0000-0000-000000000001',
            completion_time, decode(repeat('99', 32), 'hex'),
            '99700000-0000-0000-0000-000000000003'
        );
        IF NOT EXISTS (
            SELECT 1 FROM agent_run_work_outcomes
            WHERE product_event_id = '99300000-0000-0000-0000-000000000001'
        ) THEN
            RAISE EXCEPTION 'causally bound task completion was rejected';
        END IF;
        RAISE EXCEPTION USING
            ERRCODE = 'ZX001', MESSAGE = 'rollback causal outcome fixture';
    EXCEPTION
        WHEN SQLSTATE 'ZX001' THEN NULL;
    END;
END;
$test$;

SELECT set_config(
    'app.identity_id',
    '10000000-0000-0000-0000-000000000020',
    true
);

-- Recovery establishes only authentication state. It must not create device
-- keys, resource epochs, or key envelopes that would grant content access.
INSERT INTO account_recovery_tokens (
    id, identity_id, token_hash, expires_at
)
VALUES (
    '72000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000020',
    digest('recovery-token', 'sha256'),
    now() + interval '10 minutes'
);

DO $test$
DECLARE
    envelopes_before bigint;
    envelopes_after bigint;
BEGIN
    SELECT count(*) INTO envelopes_before FROM resource_key_envelopes;

    UPDATE account_recovery_tokens
    SET consumed_at = clock_timestamp()
    WHERE id = '72000000-0000-0000-0000-000000000001'
      AND consumed_at IS NULL
      AND expires_at > clock_timestamp();

    INSERT INTO devices (
        id, identity_id, device_kind, encrypted_label, trust_state
    )
    VALUES (
        '20000000-0000-0000-0000-000000000020',
        '10000000-0000-0000-0000-000000000020',
        'web',
        decode('20', 'hex'),
        'trusted'
    );

    INSERT INTO sessions (
        id, identity_id, device_id, token_hash, expires_at
    )
    VALUES (
        '73000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000020',
        '20000000-0000-0000-0000-000000000020',
        digest('session-token', 'sha256'),
        now() + interval '1 hour'
    );

    SELECT count(*) INTO envelopes_after FROM resource_key_envelopes;
    IF envelopes_after <> envelopes_before THEN
        RAISE EXCEPTION 'recovery unexpectedly created a key envelope';
    END IF;
END;
$test$;

-- T-LLR-06.5: append-only device key transparency and future-revision
-- revocation filtering.
INSERT INTO device_keys (
    identity_id, device_id, key_version,
    encryption_public_key, signing_public_key,
    suite_version, generation, previous_package_hash, package_hash, package_json,
    x25519_key_id, ml_kem_768_key_id, ed25519_key_id, ml_dsa_65_key_id,
    x25519_public_key, ml_kem_768_public_key,
    ed25519_public_key, ml_dsa_65_public_key
)
VALUES (
    '10000000-0000-0000-0000-000000000020',
    '20000000-0000-0000-0000-000000000020',
    1,
    decode(repeat('21', 32), 'hex'),
    decode(repeat('22', 32), 'hex'),
    32769,
    0,
    decode(repeat('00', 32), 'hex'),
    decode(repeat('23', 32), 'hex'),
    convert_to('{"suite":"experimental_independent_keys_v1"}', 'UTF8'),
    '74000000-0000-0000-0000-000000000001',
    '74000000-0000-0000-0000-000000000002',
    '74000000-0000-0000-0000-000000000003',
    '74000000-0000-0000-0000-000000000004',
    decode(repeat('21', 32), 'hex'),
    decode(repeat('24', 1184), 'hex'),
    decode(repeat('22', 32), 'hex'),
    decode(repeat('25', 1952), 'hex')
);

INSERT INTO device_key_transparency_log (
    identity_id, device_id, key_version, generation, event_kind,
    package_hash, previous_entry_hash, entry_hash
)
VALUES (
    '10000000-0000-0000-0000-000000000020',
    '20000000-0000-0000-0000-000000000020',
    1,
    0,
    'registered',
    decode(repeat('23', 32), 'hex'),
    NULL,
    digest(
        convert_to('sprout-device-key-transparency-v1', 'UTF8')
        || uuid_send('10000000-0000-0000-0000-000000000020'::uuid)
        || uuid_send('20000000-0000-0000-0000-000000000020'::uuid)
        || int4send(1::integer)
        || int8send(0::bigint)
        || convert_to('registered', 'UTF8')
        || decode(repeat('23', 32), 'hex'),
        'sha256'
    )
);

DO $test$
BEGIN
    BEGIN
        INSERT INTO device_key_transparency_log (
            identity_id, device_id, key_version, generation, event_kind,
            package_hash, previous_entry_hash, entry_hash,
            classical_signature, post_quantum_signature
        )
        VALUES (
            '10000000-0000-0000-0000-000000000020',
            '20000000-0000-0000-0000-000000000020',
            1,
            0,
            'revoked',
            decode(repeat('23', 32), 'hex'),
            decode(repeat('ff', 32), 'hex'),
            decode(repeat('27', 32), 'hex'),
            decode(repeat('28', 64), 'hex'),
            decode('29', 'hex')
        );
        RAISE EXCEPTION 'invalid transparency predecessor unexpectedly succeeded';
    EXCEPTION
        WHEN serialization_failure THEN NULL;
    END;
END;
$test$;

INSERT INTO device_key_transparency_log (
    identity_id, device_id, key_version, generation, event_kind,
    package_hash, previous_entry_hash, entry_hash,
    classical_signature, post_quantum_signature
)
SELECT
    '10000000-0000-0000-0000-000000000020',
    '20000000-0000-0000-0000-000000000020',
    1,
    0,
    'revoked',
    decode(repeat('23', 32), 'hex'),
    previous.entry_hash,
    digest(
        convert_to('sprout-device-key-transparency-v1', 'UTF8')
        || uuid_send('10000000-0000-0000-0000-000000000020'::uuid)
        || uuid_send('20000000-0000-0000-0000-000000000020'::uuid)
        || int4send(1::integer)
        || int8send(0::bigint)
        || convert_to('revoked', 'UTF8')
        || decode(repeat('23', 32), 'hex')
        || previous.entry_hash,
        'sha256'
    ),
    decode(repeat('28', 64), 'hex'),
    decode('29', 'hex')
FROM device_key_transparency_log previous
WHERE previous.device_id = '20000000-0000-0000-0000-000000000020'
  AND previous.event_kind = 'registered';

DO $test$
BEGIN
    BEGIN
        UPDATE device_key_transparency_log
        SET entry_hash = decode(repeat('30', 32), 'hex')
        WHERE device_id = '20000000-0000-0000-0000-000000000020'
          AND event_kind = 'revoked';
        RAISE EXCEPTION 'transparency history mutation unexpectedly succeeded';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
END;
$test$;

UPDATE device_keys
SET revoked_at = clock_timestamp()
WHERE identity_id = '10000000-0000-0000-0000-000000000020'
  AND device_id = '20000000-0000-0000-0000-000000000020'
  AND key_version = 1;

DO $test$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM sprout_private.active_project_device_keys(
            '30000000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000020'
        )
    ) THEN
        RAISE EXCEPTION 'revoked device key retained future project access';
    END IF;
END;
$test$;

-- T-LLR-06.8: recovery freezes an n-of-n electorate at a membership epoch,
-- rejects incomplete/expired/replayed approval sets, and finalizes once.
INSERT INTO project_memberships (project_id, identity_id, role, state)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000030',
    'member',
    'active'
)
ON CONFLICT (project_id, identity_id) DO NOTHING;

INSERT INTO devices (id, identity_id, device_kind, encrypted_label, trust_state)
VALUES (
    '20000000-0000-0000-0000-000000000030',
    '10000000-0000-0000-0000-000000000030',
    'web',
    decode('30', 'hex'),
    'trusted'
);

INSERT INTO device_keys (
    identity_id, device_id, key_version,
    encryption_public_key, signing_public_key,
    previous_package_hash, package_hash,
    x25519_public_key, ed25519_public_key
)
VALUES (
    '10000000-0000-0000-0000-000000000030',
    '20000000-0000-0000-0000-000000000030',
    1,
    decode('30', 'hex'),
    decode('31', 'hex'),
    decode(repeat('00', 32), 'hex'),
    decode(repeat('39', 32), 'hex'),
    decode('30', 'hex'),
    decode('31', 'hex')
);

INSERT INTO project_recovery_requests (
    id, project_id, requester_identity_id, request_kind,
    challenge, context_hash, membership_epoch, expires_at
)
SELECT
    '76000000-0000-0000-0000-000000000001',
    id,
    '10000000-0000-0000-0000-000000000001',
    'lost_owner',
    decode(repeat('40', 32), 'hex'),
    decode(repeat('41', 32), 'hex'),
    membership_epoch,
    now() + interval '1 hour'
FROM projects
WHERE id = '30000000-0000-0000-0000-000000000001';

INSERT INTO project_recovery_electorate (
    project_id, recovery_request_id, approver_identity_id,
    snapshot_role, membership_epoch
)
SELECT
    request.project_id,
    request.id,
    membership.identity_id,
    membership.role,
    request.membership_epoch
FROM project_recovery_requests request
JOIN project_memberships membership
  ON membership.project_id = request.project_id
 AND membership.role <> 'owner'
 AND membership.state = 'active'
WHERE request.id = '76000000-0000-0000-0000-000000000001';

INSERT INTO project_memberships (project_id, identity_id, role, state)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000031',
    'member',
    'active'
);

DO $test$
BEGIN
    IF EXISTS (
        SELECT 1 FROM project_recovery_electorate
        WHERE recovery_request_id = '76000000-0000-0000-0000-000000000001'
          AND approver_identity_id = '10000000-0000-0000-0000-000000000031'
    ) OR NOT EXISTS (
        SELECT 1
        FROM project_recovery_requests request
        JOIN projects project ON project.id = request.project_id
        WHERE request.id = '76000000-0000-0000-0000-000000000001'
          AND project.membership_epoch > request.membership_epoch
    ) THEN
        RAISE EXCEPTION 'recovery electorate was not frozen at its membership epoch';
    END IF;
END;
$test$;

INSERT INTO project_recovery_approvals (
    project_id, recovery_request_id, approver_identity_id,
    approver_device_id, approver_device_key_version,
    encrypted_share, classical_signature, post_quantum_signature
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '76000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000020',
    '20000000-0000-0000-0000-000000000020',
    1,
    decode('42', 'hex'),
    decode(repeat('43', 64), 'hex'),
    decode('44', 'hex')
);

DO $test$
BEGIN
    BEGIN
        UPDATE project_recovery_requests
        SET status = 'finalized', finalized_at = clock_timestamp()
        WHERE id = '76000000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'n-1 recovery unexpectedly finalized';
    EXCEPTION
        WHEN check_violation THEN NULL;
    END;
END;
$test$;

INSERT INTO project_recovery_approvals (
    project_id, recovery_request_id, approver_identity_id,
    approver_device_id, approver_device_key_version,
    encrypted_share, classical_signature, post_quantum_signature
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '76000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000030',
    '20000000-0000-0000-0000-000000000030',
    1,
    decode('45', 'hex'),
    decode(repeat('46', 64), 'hex'),
    decode('47', 'hex')
);

UPDATE project_recovery_requests
SET status = 'finalized', finalized_at = clock_timestamp()
WHERE id = '76000000-0000-0000-0000-000000000001';

DO $test$
BEGIN
    BEGIN
        UPDATE project_recovery_requests
        SET finalized_at = clock_timestamp() + interval '1 second'
        WHERE id = '76000000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'recovery finalization replay unexpectedly succeeded';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
END;
$test$;

INSERT INTO project_recovery_requests (
    id, project_id, requester_identity_id, request_kind,
    challenge, context_hash, membership_epoch,
    created_at, expires_at
)
SELECT
    '76000000-0000-0000-0000-000000000002',
    id,
    '10000000-0000-0000-0000-000000000031',
    'participant_device',
    decode(repeat('48', 32), 'hex'),
    decode(repeat('49', 32), 'hex'),
    membership_epoch,
    now() - interval '2 hours',
    now() - interval '1 hour'
FROM projects
WHERE id = '30000000-0000-0000-0000-000000000001';

INSERT INTO project_recovery_electorate (
    project_id, recovery_request_id, approver_identity_id,
    snapshot_role, membership_epoch
)
SELECT
    project_id,
    id,
    '10000000-0000-0000-0000-000000000001',
    'owner',
    membership_epoch
FROM project_recovery_requests
WHERE id = '76000000-0000-0000-0000-000000000002';

INSERT INTO project_recovery_approvals (
    project_id, recovery_request_id, approver_identity_id,
    approver_device_id, approver_device_key_version,
    encrypted_share, classical_signature, post_quantum_signature
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '76000000-0000-0000-0000-000000000002',
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    1,
    decode('50', 'hex'),
    decode(repeat('51', 64), 'hex'),
    decode('52', 'hex')
);

DO $test$
BEGIN
    BEGIN
        UPDATE project_recovery_requests
        SET status = 'finalized', finalized_at = clock_timestamp()
        WHERE id = '76000000-0000-0000-0000-000000000002';
        RAISE EXCEPTION 'expired recovery unexpectedly finalized';
    EXCEPTION
        WHEN check_violation THEN NULL;
    END;

    BEGIN
        INSERT INTO project_recovery_approvals (
            project_id, recovery_request_id, approver_identity_id,
            approver_device_id, approver_device_key_version,
            encrypted_share, classical_signature, post_quantum_signature
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '76000000-0000-0000-0000-000000000002',
            '10000000-0000-0000-0000-000000000001',
            '20000000-0000-0000-0000-000000000001',
            1,
            decode('53', 'hex'),
            decode(repeat('54', 64), 'hex'),
            decode('55', 'hex')
        );
        RAISE EXCEPTION 'recovery approval replay unexpectedly succeeded';
    EXCEPTION
        WHEN unique_violation THEN NULL;
    END;
END;
$test$;

-- Project recovery provision: draft→active, holder-only share SELECT.
INSERT INTO project_recovery_sets (
    id, project_id, recovery_epoch, membership_epoch,
    created_by_identity_id, share_count, threshold,
    secret_commitment, context_hash, encrypted_owner_key_escrow, state
)
SELECT
    '77000000-0000-0000-0000-000000000001',
    id,
    recovery_epoch,
    membership_epoch,
    '10000000-0000-0000-0000-000000000001',
    2,
    2,
    decode(repeat('61', 32), 'hex'),
    decode(repeat('62', 32), 'hex'),
    decode('63', 'hex'),
    'draft'
FROM projects
WHERE id = '30000000-0000-0000-0000-000000000001';

INSERT INTO project_recovery_shares (
    id, project_id, recovery_set_id, share_index,
    holder_identity_id, holder_device_id, holder_device_key_version,
    encrypted_share, share_commitment
)
VALUES
(
    '78000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '77000000-0000-0000-0000-000000000001',
    1,
    '10000000-0000-0000-0000-000000000020',
    '20000000-0000-0000-0000-000000000020',
    1,
    decode('64', 'hex'),
    decode(repeat('65', 32), 'hex')
),
(
    '78000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000001',
    '77000000-0000-0000-0000-000000000001',
    2,
    '10000000-0000-0000-0000-000000000030',
    '20000000-0000-0000-0000-000000000030',
    1,
    decode('66', 'hex'),
    decode(repeat('67', 32), 'hex')
);

UPDATE project_recovery_sets
SET state = 'active', activated_at = clock_timestamp()
WHERE id = '77000000-0000-0000-0000-000000000001';

DO $test$
BEGIN
    BEGIN
        UPDATE project_recovery_shares
        SET encrypted_share = decode('68', 'hex')
        WHERE id = '78000000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'active project recovery shares unexpectedly mutated';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
END;
$test$;

SELECT set_config(
    'app.identity_id',
    '10000000-0000-0000-0000-000000000020',
    true
);
SELECT set_config(
    'app.device_id',
    '20000000-0000-0000-0000-000000000020',
    true
);

CREATE ROLE sprout_behavior_rls NOSUPERUSER NOBYPASSRLS NOLOGIN;
GRANT USAGE ON SCHEMA public, sprout_private TO sprout_behavior_rls;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO sprout_behavior_rls;
GRANT INSERT, UPDATE, DELETE ON resource_nodes TO sprout_behavior_rls;
GRANT INSERT, DELETE ON resource_closure TO sprout_behavior_rls;
GRANT SELECT, UPDATE ON agent_collaborative_runs TO sprout_behavior_rls;
GRANT SELECT, UPDATE ON agent_run_blockers TO sprout_behavior_rls;
GRANT SELECT, UPDATE ON agent_run_claim_leases TO sprout_behavior_rls;
SET LOCAL ROLE sprout_behavior_rls;

DO $test$
DECLARE
    visible_count integer;
BEGIN
    SELECT count(*) INTO visible_count
    FROM project_recovery_shares
    WHERE recovery_set_id = '77000000-0000-0000-0000-000000000001';
    IF visible_count <> 1 THEN
        RAISE EXCEPTION 'holder-only recovery share RLS failed: saw % rows', visible_count;
    END IF;
END;
$test$;

DO $test$
DECLARE
    visible_runs integer;
    visible_blockers integer;
    visible_claims integer;
    changed_runs integer;
    changed_blockers integer;
    changed_claims integer;
BEGIN
    IF NOT sprout_private.can_access_resource(
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000003',
        'read'
    ) THEN
        RAISE EXCEPTION 'R5.30 RLS fixture foreign member lacks intended scope read';
    END IF;
    IF sprout_private.agent_run_access(
        '30000000-0000-0000-0000-000000000001',
        '94000000-0000-0000-0000-000000000001'
    ) THEN
        RAISE EXCEPTION 'R5.30 foreign non-participant gained agent run access';
    END IF;

    SELECT count(*) INTO visible_runs FROM agent_collaborative_runs
    WHERE id = '94000000-0000-0000-0000-000000000001';
    SELECT count(*) INTO visible_blockers FROM agent_run_blockers
    WHERE id = '95000000-0000-0000-0000-000000000001';
    SELECT count(*) INTO visible_claims FROM agent_run_claim_leases
    WHERE id = '98000000-0000-0000-0000-000000000001';
    IF visible_runs <> 0 OR visible_blockers <> 0 OR visible_claims <> 0 THEN
        RAISE EXCEPTION
            'R5.30 RLS exposed run/blocker/claim to a foreign non-participant';
    END IF;

    UPDATE agent_collaborative_runs SET updated_at = clock_timestamp()
    WHERE id = '94000000-0000-0000-0000-000000000001';
    GET DIAGNOSTICS changed_runs = ROW_COUNT;
    UPDATE agent_run_blockers SET current_status = 'cancelled', terminal_tick = 2
    WHERE id = '95000000-0000-0000-0000-000000000001';
    GET DIAGNOSTICS changed_blockers = ROW_COUNT;
    UPDATE agent_run_claim_leases SET status = 'released', terminal_at = clock_timestamp()
    WHERE id = '98000000-0000-0000-0000-000000000001';
    GET DIAGNOSTICS changed_claims = ROW_COUNT;
    IF changed_runs <> 0 OR changed_blockers <> 0 OR changed_claims <> 0 THEN
        RAISE EXCEPTION
            'R5.30 RLS allowed foreign mutation of run/blocker/claim';
    END IF;
END;
$test$;

DO $test$
DECLARE
    visible_count integer;
    updated_count integer;
    inserted_count integer;
    deleted_count integer;
    unauthorized_inserted boolean := false;
    direct_delete_succeeded boolean := false;
BEGIN
    SELECT count(*) INTO visible_count
    FROM resource_nodes
    WHERE project_id = '30000000-0000-0000-0000-000000000002';
    IF visible_count <> 0 THEN
        RAISE EXCEPTION
            'T-LLR-10.2 direct SQL read crossed project RLS: saw % rows',
            visible_count;
    END IF;

    UPDATE resource_nodes
    SET encrypted_metadata = encrypted_metadata
    WHERE project_id = '30000000-0000-0000-0000-000000000002';
    GET DIAGNOSTICS updated_count = ROW_COUNT;
    IF updated_count <> 0 THEN
        RAISE EXCEPTION
            'T-LLR-10.2 direct SQL update crossed project RLS: changed % rows',
            updated_count;
    END IF;

    INSERT INTO resource_nodes (
        id, project_id, parent_id, node_kind,
        encrypted_metadata, created_by_identity_id
    ) VALUES (
        '40000000-0000-0000-0000-000000000090',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000003',
        'task', decode('90', 'hex'),
        '10000000-0000-0000-0000-000000000020'
    );
    GET DIAGNOSTICS inserted_count = ROW_COUNT;
    IF inserted_count <> 1 THEN
        RAISE EXCEPTION
            'T-LLR-10.2 authorized direct SQL insert was denied';
    END IF;

    BEGIN
        INSERT INTO resource_nodes (
            id, project_id, parent_id, node_kind,
            encrypted_metadata, created_by_identity_id
        ) VALUES (
            '40000000-0000-0000-0000-000000000091',
            '30000000-0000-0000-0000-000000000002',
            '40000000-0000-0000-0000-000000000005',
            'task', decode('91', 'hex'),
            '10000000-0000-0000-0000-000000000001'
        );
        unauthorized_inserted := true;
    EXCEPTION
        WHEN insufficient_privilege OR foreign_key_violation OR check_violation OR raise_exception
            THEN NULL;
    END;
    IF unauthorized_inserted THEN
        RAISE EXCEPTION
            'T-LLR-10.2 direct SQL insert crossed project RLS';
    END IF;

    DELETE FROM resource_nodes
    WHERE project_id = '30000000-0000-0000-0000-000000000002';
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    IF deleted_count <> 0 THEN
        RAISE EXCEPTION
            'T-LLR-10.2 direct SQL delete crossed project RLS: removed % rows',
            deleted_count;
    END IF;

    BEGIN
        DELETE FROM resource_nodes
        WHERE id = '40000000-0000-0000-0000-000000000090';
        direct_delete_succeeded := true;
    EXCEPTION
        WHEN insufficient_privilege THEN NULL;
    END;
    IF direct_delete_succeeded THEN
        RAISE EXCEPTION
            'T-LLR-10.2 direct SQL delete bypassed retention-only purge';
    END IF;
END;
$test$;

RESET ROLE;

CREATE ROLE sprout_behavior_service NOLOGIN BYPASSRLS;
GRANT USAGE ON SCHEMA public TO sprout_behavior_service;
GRANT SELECT ON resource_nodes TO sprout_behavior_service;
SET LOCAL ROLE sprout_behavior_service;

DO $test$
DECLARE
    cross_project_count integer;
BEGIN
    SELECT count(*) INTO cross_project_count
    FROM resource_nodes
    WHERE project_id = '30000000-0000-0000-0000-000000000002';
    IF cross_project_count = 0 THEN
        RAISE EXCEPTION
            'T-LLR-10.2 provisioned BYPASSRLS service role could not read worker data';
    END IF;
END;
$test$;

RESET ROLE;

SELECT set_config(
    'app.identity_id',
    '10000000-0000-0000-0000-000000000001',
    true
);
SELECT set_config(
    'app.device_id',
    '20000000-0000-0000-0000-000000000001',
    true
);

-- T-LLR-07.6 / T-LLR-10.1: optimistic sync versions, device chains,
-- transactional projection rollback, and tombstone finality.
SELECT set_config(
    'app.identity_id',
    '10000000-0000-0000-0000-000000000001',
    true
);

INSERT INTO sync_events (
    project_id, stream_id, resource_node_id, base_version, aggregate_version,
    mutation_kind, actor_identity_id, actor_device_id,
    actor_device_key_version, device_sequence, client_event_id, event_kind,
    key_epoch, encrypted_payload, previous_hash, event_hash, signature,
    client_created_at
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000004',
    '40000000-0000-0000-0000-000000000004',
    0, 1, 'upsert',
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    1, 1,
    '75000000-0000-0000-0000-000000000001',
    'updated', 1, decode('01', 'hex'), NULL,
    decode(repeat('31', 32), 'hex'),
    decode(repeat('32', 64), 'hex'),
    now()
);

DO $test$
BEGIN
    BEGIN
        INSERT INTO sync_events (
            project_id, stream_id, resource_node_id, base_version, aggregate_version,
            mutation_kind, actor_identity_id, actor_device_id,
            actor_device_key_version, device_sequence, client_event_id, event_kind,
            key_epoch, encrypted_payload, previous_hash, event_hash, signature,
            client_created_at
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000004',
            '40000000-0000-0000-0000-000000000004',
            0, 1, 'upsert',
            '10000000-0000-0000-0000-000000000001',
            '20000000-0000-0000-0000-000000000001',
            1, 2,
            '75000000-0000-0000-0000-000000000002',
            'updated', 1, decode('02', 'hex'),
            decode(repeat('31', 32), 'hex'),
            decode(repeat('33', 32), 'hex'),
            decode(repeat('34', 64), 'hex'),
            now()
        );
        RAISE EXCEPTION 'stale aggregate version unexpectedly succeeded';
    EXCEPTION
        WHEN serialization_failure THEN NULL;
    END;
END;
$test$;

INSERT INTO sync_events (
    project_id, stream_id, resource_node_id, base_version, aggregate_version,
    mutation_kind, actor_identity_id, actor_device_id,
    actor_device_key_version, device_sequence, client_event_id, event_kind,
    key_epoch, encrypted_payload, previous_hash, event_hash, signature,
    client_created_at
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000004',
    '40000000-0000-0000-0000-000000000004',
    1, 2, 'tombstone',
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    1, 2,
    '75000000-0000-0000-0000-000000000003',
    'deleted', 1, decode('03', 'hex'),
    decode(repeat('31', 32), 'hex'),
    decode(repeat('35', 32), 'hex'),
    decode(repeat('36', 64), 'hex'),
    now()
);

DO $test$
DECLARE
    projection_version bigint;
    projection_mutation text;
    projection_payload bytea;
BEGIN
    SELECT aggregate_version, mutation_kind, encrypted_payload
    INTO projection_version, projection_mutation, projection_payload
    FROM sync_current_projections
    WHERE project_id = '30000000-0000-0000-0000-000000000001'
      AND resource_node_id = '40000000-0000-0000-0000-000000000004';
    IF projection_version <> 2
       OR projection_mutation <> 'tombstone'
       OR projection_payload <> decode('03', 'hex')
    THEN
        RAISE EXCEPTION 'signed event and current projection did not advance atomically';
    END IF;

    BEGIN
        INSERT INTO sync_events (
            project_id, stream_id, resource_node_id, base_version, aggregate_version,
            mutation_kind, actor_identity_id, actor_device_id,
            actor_device_key_version, device_sequence, client_event_id, event_kind,
            key_epoch, encrypted_payload, previous_hash, event_hash, signature,
            client_created_at
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000004',
            '40000000-0000-0000-0000-000000000004',
            2, 3, 'tombstone',
            '10000000-0000-0000-0000-000000000001',
            '20000000-0000-0000-0000-000000000001',
            1, 3,
            '75000000-0000-0000-0000-000000000005',
            'deleted', 1, decode('05', 'hex'),
            decode(repeat('35', 32), 'hex'),
            decode(repeat('39', 32), 'hex'),
            decode(repeat('40', 64), 'hex'),
            now()
        );
        RAISE EXCEPTION 'injected failure after signed mutation';
    EXCEPTION
        WHEN raise_exception THEN NULL;
    END;

    SELECT aggregate_version, mutation_kind, encrypted_payload
    INTO projection_version, projection_mutation, projection_payload
    FROM sync_current_projections
    WHERE project_id = '30000000-0000-0000-0000-000000000001'
      AND resource_node_id = '40000000-0000-0000-0000-000000000004';
    IF projection_version <> 2
       OR projection_mutation <> 'tombstone'
       OR projection_payload <> decode('03', 'hex')
    THEN
        RAISE EXCEPTION 'rollback exposed a partial sync projection';
    END IF;
END;
$test$;

DO $test$
BEGIN
    BEGIN
        INSERT INTO sync_events (
            project_id, stream_id, resource_node_id, base_version, aggregate_version,
            mutation_kind, actor_identity_id, actor_device_id,
            actor_device_key_version, device_sequence, client_event_id, event_kind,
            key_epoch, encrypted_payload, previous_hash, event_hash, signature,
            client_created_at
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000004',
            '40000000-0000-0000-0000-000000000004',
            2, 3, 'upsert',
            '10000000-0000-0000-0000-000000000001',
            '20000000-0000-0000-0000-000000000001',
            1, 3,
            '75000000-0000-0000-0000-000000000004',
            'updated', 1, decode('04', 'hex'),
            decode(repeat('35', 32), 'hex'),
            decode(repeat('37', 32), 'hex'),
            decode(repeat('38', 64), 'hex'),
            now()
        );
        RAISE EXCEPTION 'signed tombstone resurrection unexpectedly succeeded';
    EXCEPTION
        WHEN serialization_failure THEN NULL;
    END;
END;
$test$;

-- T-LLR-07.3 / T-LLR-07.6: idempotency replay with identical digest succeeds;
-- same key with a different digest is rejected; reconstruction uses events.
INSERT INTO sync_idempotency (
    project_id, actor_device_id, idempotency_key, request_hash,
    sync_event_id, event_sequence, expires_at
)
SELECT
    event.project_id,
    event.actor_device_id,
    '76000000-0000-0000-0000-000000000001',
    decode(repeat('41', 32), 'hex'),
    event.id,
    event.event_sequence,
    now() + interval '1 day'
FROM sync_events event
WHERE event.client_event_id = '75000000-0000-0000-0000-000000000001';

DO $test$
DECLARE
    stored_hash bytea;
BEGIN
    SELECT request_hash INTO stored_hash
    FROM sync_idempotency
    WHERE project_id = '30000000-0000-0000-0000-000000000001'
      AND actor_device_id = '20000000-0000-0000-0000-000000000001'
      AND idempotency_key = '76000000-0000-0000-0000-000000000001';
    IF stored_hash IS DISTINCT FROM decode(repeat('41', 32), 'hex') THEN
        RAISE EXCEPTION 'T-LLR-07.3 identical idempotency digest was not retained';
    END IF;

    BEGIN
        INSERT INTO sync_idempotency (
            project_id, actor_device_id, idempotency_key, request_hash,
            sync_event_id, event_sequence, expires_at
        )
        SELECT
            event.project_id,
            event.actor_device_id,
            '76000000-0000-0000-0000-000000000001',
            decode(repeat('42', 32), 'hex'),
            event.id,
            event.event_sequence,
            now() + interval '1 day'
        FROM sync_events event
        WHERE event.client_event_id = '75000000-0000-0000-0000-000000000003';
        RAISE EXCEPTION 'T-LLR-07.3 colliding idempotency digest unexpectedly succeeded';
    EXCEPTION
        WHEN unique_violation THEN NULL;
    END;

    IF NOT EXISTS (
        SELECT 1
        FROM sync_events event
        JOIN sync_current_projections projection
          ON projection.project_id = event.project_id
         AND projection.resource_node_id = event.resource_node_id
         AND projection.aggregate_version = event.aggregate_version
        WHERE event.client_event_id = '75000000-0000-0000-0000-000000000003'
          AND projection.mutation_kind = 'tombstone'
    ) THEN
        RAISE EXCEPTION 'T-LLR-07.6 event/projection reconstruction failed';
    END IF;
END;
$test$;

UPDATE tasks
SET visibility = 'restricted'
WHERE project_id = '30000000-0000-0000-0000-000000000001'
  AND resource_node_id = '40000000-0000-0000-0000-000000000004';

SELECT set_config(
    'app.identity_id',
    '10000000-0000-0000-0000-000000000031',
    true
);

DO $test$
DECLARE
    visible_events bigint;
BEGIN
    SELECT count(*)
    INTO visible_events
    FROM sync_events event
    JOIN resource_nodes node
      ON node.project_id = event.project_id
     AND node.id = event.resource_node_id
     AND node.deleted_at IS NULL
    JOIN projects project ON project.id = event.project_id
    JOIN project_memberships membership
      ON membership.project_id = event.project_id
     AND membership.identity_id = '10000000-0000-0000-0000-000000000031'
     AND membership.state = 'active'
    WHERE event.project_id = '30000000-0000-0000-0000-000000000001'
      AND (
          project.owner_identity_id = membership.identity_id
          OR membership.role = 'admin'
          OR node.created_by_identity_id = membership.identity_id
          OR EXISTS (
              SELECT 1
              FROM sprout_private.effective_domain_permission(
                  event.project_id,
                  event.resource_node_id,
                  membership.identity_id
              )
          )
      );
    IF visible_events <> 0 THEN
        RAISE EXCEPTION 'unrelated restricted sync ciphertext was visible';
    END IF;
END;
$test$;

-- HLT-04 / T-LLR-04.1 / T-LLR-04.2 / T-LLR-04.3 / T-LLR-04.4 /
-- T-LLR-04.5: editable drafts, immutable published versions, exact task
-- pins, assignee-only drafts, dual-signed finalization, and retained
-- historical question/option rows.
SELECT set_config(
    'app.identity_id',
    '10000000-0000-0000-0000-000000000001',
    true
);
SELECT set_config(
    'app.device_id',
    '20000000-0000-0000-0000-000000000001',
    true
);

INSERT INTO questionnaires (
    id, project_id, encrypted_metadata, created_by_identity_id
)
VALUES (
    '58000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    decode('5801', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);

INSERT INTO questionnaire_versions (
    id, project_id, questionnaire_id, version_number,
    encrypted_payload, content_hash, created_by_identity_id
)
VALUES (
    '58100000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '58000000-0000-0000-0000-000000000001',
    1,
    decode('5811', 'hex'),
    decode(repeat('58', 16), 'hex'),
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO questionnaire_questions (
    id, project_id, questionnaire_version_id, client_key,
    question_kind, ordinal, required, encrypted_payload
)
VALUES (
    '58200000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '58100000-0000-0000-0000-000000000001',
    '58200000-0000-0000-0000-000000000011',
    'single_choice', 0, true, decode('5821', 'hex')
);
INSERT INTO questionnaire_options (
    id, project_id, question_id, client_key, ordinal, encrypted_payload
)
VALUES (
    '58300000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '58200000-0000-0000-0000-000000000001',
    '58300000-0000-0000-0000-000000000011',
    0, decode('5831', 'hex')
);
UPDATE questionnaire_versions
SET published_at = clock_timestamp(), revision = revision + 1
WHERE id = '58100000-0000-0000-0000-000000000001';

DO $test$
BEGIN
    BEGIN
        UPDATE questionnaire_questions
        SET required = false
        WHERE id = '58200000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'published question mutation unexpectedly succeeded';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
    BEGIN
        UPDATE questionnaire_versions
        SET encrypted_payload = decode('ffff', 'hex'), revision = revision + 1
        WHERE id = '58100000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'published version mutation unexpectedly succeeded';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
END;
$test$;

INSERT INTO questionnaire_versions (
    id, project_id, questionnaire_id, version_number, source_version_id,
    encrypted_payload, content_hash, created_by_identity_id
)
VALUES (
    '58100000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000001',
    '58000000-0000-0000-0000-000000000001',
    2,
    '58100000-0000-0000-0000-000000000001',
    decode('5812', 'hex'),
    decode(repeat('59', 16), 'hex'),
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO questionnaire_questions (
    id, project_id, questionnaire_version_id, client_key,
    question_kind, ordinal, required, encrypted_payload
)
VALUES (
    '58200000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000001',
    '58100000-0000-0000-0000-000000000002',
    '58200000-0000-0000-0000-000000000012',
    'single_choice', 0, false, decode('5822', 'hex')
);
INSERT INTO questionnaire_options (
    id, project_id, question_id, client_key, ordinal, encrypted_payload
)
VALUES (
    '58300000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000001',
    '58200000-0000-0000-0000-000000000002',
    '58300000-0000-0000-0000-000000000012',
    0, decode('5832', 'hex')
);

INSERT INTO resource_nodes (
    id, project_id, parent_id, node_kind,
    encrypted_metadata, created_by_identity_id
)
VALUES (
    '40000000-0000-0000-0000-000000000068',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    'task', decode('6801', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);

DO $test$
BEGIN
    BEGIN
        INSERT INTO tasks (
            id, project_id, task_list_id, resource_node_id,
            task_kind, encrypted_payload, encrypted_value_snapshot,
            questionnaire_version_id, created_by_identity_id
        )
        VALUES (
            '56400000-0000-0000-0000-000000000009',
            '30000000-0000-0000-0000-000000000001',
            '51000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000068',
            'priority', decode('6801', 'hex'), decode('6802', 'hex'),
            '58100000-0000-0000-0000-000000000002',
            '10000000-0000-0000-0000-000000000001'
        );
        RAISE EXCEPTION 'task pinned an unpublished questionnaire version';
    EXCEPTION
        WHEN check_violation THEN NULL;
    END;
END;
$test$;

UPDATE questionnaire_versions
SET published_at = clock_timestamp(), revision = revision + 1
WHERE id = '58100000-0000-0000-0000-000000000002';

INSERT INTO tasks (
    id, project_id, task_list_id, resource_node_id,
    task_kind, encrypted_payload, encrypted_value_snapshot,
    questionnaire_version_id, created_by_identity_id
)
VALUES (
    '56400000-0000-0000-0000-000000000008',
    '30000000-0000-0000-0000-000000000001',
    '51000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000068',
    'priority', decode('6801', 'hex'), decode('6802', 'hex'),
    '58100000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO task_assignments (
    id, project_id, task_id, assignee_identity_id,
    assigned_by_identity_id, encrypted_payload, permission_root_grant_id
)
VALUES (
    '56500000-0000-0000-0000-000000000008',
    '30000000-0000-0000-0000-000000000001',
    '56400000-0000-0000-0000-000000000008',
    '10000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    decode('6803', 'hex'),
    '56500000-0000-0000-0000-000000000018'
);
INSERT INTO questionnaire_submissions (
    id, project_id, questionnaire_version_id,
    submitted_by_identity_id, client_submission_id,
    encrypted_payload, state, task_id, assignment_id
)
VALUES (
    '58400000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '58100000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    '58400000-0000-0000-0000-000000000011',
    decode('5841', 'hex'), 'draft',
    '56400000-0000-0000-0000-000000000008',
    '56500000-0000-0000-0000-000000000008'
);

DO $test$
BEGIN
    BEGIN
        INSERT INTO questionnaire_answers (
            project_id, questionnaire_version_id,
            submission_id, question_id, encrypted_payload
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '58100000-0000-0000-0000-000000000001',
            '58400000-0000-0000-0000-000000000001',
            '58200000-0000-0000-0000-000000000002',
            decode('5842', 'hex')
        );
        RAISE EXCEPTION 'cross-version question unexpectedly entered draft';
    EXCEPTION
        WHEN foreign_key_violation THEN NULL;
    END;
END;
$test$;

INSERT INTO questionnaire_answers (
    id, project_id, questionnaire_version_id,
    submission_id, question_id, encrypted_payload
)
VALUES (
    '58500000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '58100000-0000-0000-0000-000000000001',
    '58400000-0000-0000-0000-000000000001',
    '58200000-0000-0000-0000-000000000001',
    decode('5851', 'hex')
);

DO $test$
BEGIN
    BEGIN
        INSERT INTO questionnaire_answer_options (
            project_id, questionnaire_version_id, submission_id,
            answer_id, question_id, option_id
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '58100000-0000-0000-0000-000000000001',
            '58400000-0000-0000-0000-000000000001',
            '58500000-0000-0000-0000-000000000001',
            '58200000-0000-0000-0000-000000000001',
            '58300000-0000-0000-0000-000000000002'
        );
        RAISE EXCEPTION 'cross-version option unexpectedly entered draft';
    EXCEPTION
        WHEN foreign_key_violation THEN NULL;
    END;
END;
$test$;

INSERT INTO questionnaire_answer_options (
    project_id, questionnaire_version_id, submission_id,
    answer_id, question_id, option_id
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '58100000-0000-0000-0000-000000000001',
    '58400000-0000-0000-0000-000000000001',
    '58500000-0000-0000-0000-000000000001',
    '58200000-0000-0000-0000-000000000001',
    '58300000-0000-0000-0000-000000000001'
);

SELECT set_config(
    'app.identity_id',
    '10000000-0000-0000-0000-000000000020',
    true
);
DO $test$
BEGIN
    BEGIN
        UPDATE questionnaire_submissions
        SET encrypted_payload = decode('ffff', 'hex'), revision = revision + 1
        WHERE id = '58400000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'non-assignee draft edit unexpectedly succeeded';
    EXCEPTION
        WHEN insufficient_privilege THEN NULL;
    END;
END;
$test$;

SELECT set_config(
    'app.identity_id',
    '10000000-0000-0000-0000-000000000001',
    true
);

-- T-LLR-02.2 explicitly denies a non-assigned owner from creating a
-- questionnaire draft for a task assigned to another member.
INSERT INTO resource_nodes (
    id, project_id, parent_id, node_kind,
    encrypted_metadata, created_by_identity_id
)
VALUES (
    '40000000-0000-0000-0000-000000000069',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000003',
    'task', decode('6901', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO tasks (
    id, project_id, task_list_id, resource_node_id,
    task_kind, encrypted_payload, encrypted_value_snapshot,
    questionnaire_version_id, created_by_identity_id
)
VALUES (
    '56400000-0000-0000-0000-000000000009',
    '30000000-0000-0000-0000-000000000001',
    '51000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000069',
    'priority', decode('6901', 'hex'), decode('6902', 'hex'),
    '58100000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO task_assignments (
    id, project_id, task_id, assignee_identity_id,
    assigned_by_identity_id, encrypted_payload, permission_root_grant_id
)
VALUES (
    '56500000-0000-0000-0000-000000000009',
    '30000000-0000-0000-0000-000000000001',
    '56400000-0000-0000-0000-000000000009',
    '10000000-0000-0000-0000-000000000020',
    '10000000-0000-0000-0000-000000000001',
    decode('6903', 'hex'),
    '56500000-0000-0000-0000-000000000019'
);
DO $test$
BEGIN
    BEGIN
        INSERT INTO questionnaire_submissions (
            project_id, questionnaire_version_id,
            submitted_by_identity_id, client_submission_id,
            encrypted_payload, state, task_id, assignment_id
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '58100000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000001',
            '58400000-0000-0000-0000-000000000019',
            decode('6904', 'hex'), 'draft',
            '56400000-0000-0000-0000-000000000009',
            '56500000-0000-0000-0000-000000000009'
        );
        RAISE EXCEPTION 'non-assigned owner questionnaire draft unexpectedly succeeded';
    EXCEPTION
        WHEN insufficient_privilege THEN NULL;
    END;
END;
$test$;

UPDATE questionnaire_submissions
SET
    state = 'submitted',
    signer_device_id = '20000000-0000-0000-0000-000000000001',
    signer_device_key_version = 1,
    classical_signature = decode(repeat('58', 64), 'hex'),
    post_quantum_signature = decode('59', 'hex'),
    idempotency_key = '58400000-0000-0000-0000-000000000021',
    request_hash = decode(repeat('5a', 32), 'hex'),
    submitted_at = clock_timestamp(),
    revision = revision + 1
WHERE id = '58400000-0000-0000-0000-000000000001';

DO $test$
BEGIN
    BEGIN
        UPDATE questionnaire_submissions
        SET encrypted_payload = decode('eeee', 'hex'), revision = revision + 1
        WHERE id = '58400000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'final submission mutation unexpectedly succeeded';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
END;
$test$;

UPDATE questionnaires
SET state = 'archived'
WHERE id = '58000000-0000-0000-0000-000000000001';
DO $test$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM questionnaire_submissions submission
        JOIN questionnaire_answers answer
          ON answer.project_id = submission.project_id
         AND answer.submission_id = submission.id
        JOIN questionnaire_questions question
          ON question.project_id = answer.project_id
         AND question.id = answer.question_id
        JOIN questionnaire_answer_options selected
          ON selected.project_id = answer.project_id
         AND selected.answer_id = answer.id
        JOIN questionnaire_options option
          ON option.project_id = selected.project_id
         AND option.id = selected.option_id
        WHERE submission.id = '58400000-0000-0000-0000-000000000001'
          AND question.encrypted_payload = decode('5821', 'hex')
          AND option.encrypted_payload = decode('5831', 'hex')
    ) THEN
        RAISE EXCEPTION 'historical questionnaire fidelity was not retained';
    END IF;
END;
$test$;

-- T-LLR-05.3: three attachment identities, immutable provenance,
-- resource-key binding, ciphertext-only declarations, and assignee-only
-- completion uploads. Browser OPFS behavior is out of scope.
INSERT INTO file_blobs (
    id, project_id, storage_provider, storage_key, ciphertext_size,
    ciphertext_hash, key_epoch, encrypted_metadata,
    created_by_identity_id, resource_node_id
)
VALUES
    (
        '59000000-0000-0000-0000-000000000001',
        '30000000-0000-0000-0000-000000000001',
        'filesystem', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.blob', 16,
        decode(repeat('aa', 32), 'hex'), 1, decode('5901', 'hex'),
        '10000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000062'
    ),
    (
        '59000000-0000-0000-0000-000000000002',
        '30000000-0000-0000-0000-000000000001',
        'filesystem', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.blob', 17,
        decode(repeat('bb', 32), 'hex'), 1, decode('5902', 'hex'),
        '10000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000062'
    ),
    (
        '59000000-0000-0000-0000-000000000003',
        '30000000-0000-0000-0000-000000000001',
        'filesystem', 'cccccccccccccccccccccccccccccccc.blob', 18,
        decode(repeat('cc', 32), 'hex'), 1, decode('5903', 'hex'),
        '10000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000062'
    ),
    (
        '59000000-0000-0000-0000-000000000004',
        '30000000-0000-0000-0000-000000000001',
        'filesystem', 'dddddddddddddddddddddddddddddddd.blob', 19,
        decode(repeat('dd', 32), 'hex'), 1, decode('5904', 'hex'),
        '10000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000062'
    );

INSERT INTO file_links (
    project_id, blob_id, resource_node_id, link_kind,
    encrypted_metadata, created_by_identity_id
)
SELECT
    '30000000-0000-0000-0000-000000000001',
    blob_id,
    '40000000-0000-0000-0000-000000000062',
    'attachment',
    decode('59aa', 'hex'),
    '10000000-0000-0000-0000-000000000001'
FROM unnest(ARRAY[
    '59000000-0000-0000-0000-000000000001'::uuid,
    '59000000-0000-0000-0000-000000000002'::uuid,
    '59000000-0000-0000-0000-000000000003'::uuid,
    '59000000-0000-0000-0000-000000000004'::uuid
]) blob_id;

INSERT INTO pretask_template_attachments (
    id, project_id, preset_version_id, pretask_id,
    blob_id, resource_node_id, key_epoch,
    encrypted_metadata, created_by_identity_id
)
VALUES (
    '59100000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '56100000-0000-0000-0000-000000000001',
    '56200000-0000-0000-0000-000000000002',
    '59000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000062',
    1, decode('5911', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO task_required_attachments (
    id, project_id, task_id, source_template_attachment_id,
    blob_id, resource_node_id, key_epoch,
    encrypted_snapshot, materialized_by_identity_id
)
VALUES (
    '59200000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '56400000-0000-0000-0000-000000000002',
    '59100000-0000-0000-0000-000000000001',
    '59000000-0000-0000-0000-000000000002',
    '40000000-0000-0000-0000-000000000062',
    1, decode('5921', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);
INSERT INTO task_completed_attachments (
    id, project_id, task_id, assignment_id,
    required_attachment_id, blob_id, resource_node_id,
    key_epoch, encrypted_metadata, uploaded_by_identity_id
)
VALUES (
    '59300000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '56400000-0000-0000-0000-000000000002',
    '56500000-0000-0000-0000-000000000002',
    '59200000-0000-0000-0000-000000000001',
    '59000000-0000-0000-0000-000000000003',
    '40000000-0000-0000-0000-000000000062',
    1, decode('5931', 'hex'),
    '10000000-0000-0000-0000-000000000001'
);

INSERT INTO file_blobs (
    id, project_id, storage_provider, storage_key, ciphertext_size,
    ciphertext_hash, key_epoch, encrypted_metadata,
    created_by_identity_id, resource_node_id
)
VALUES (
    '59000000-0000-0000-0000-000000000005',
    '30000000-0000-0000-0000-000000000001',
    'filesystem', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee.blob', 20,
    decode(repeat('ee', 32), 'hex'), 1, decode('5905', 'hex'),
    '10000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000004'
);

DO $test$
BEGIN
    IF (
        SELECT count(DISTINCT blob_id)
        FROM (
            SELECT blob_id FROM pretask_template_attachments
            WHERE id = '59100000-0000-0000-0000-000000000001'
            UNION ALL
            SELECT blob_id FROM task_required_attachments
            WHERE id = '59200000-0000-0000-0000-000000000001'
            UNION ALL
            SELECT blob_id FROM task_completed_attachments
            WHERE id = '59300000-0000-0000-0000-000000000001'
        ) attachment_blobs
    ) <> 3 THEN
        RAISE EXCEPTION 'attachment entities aliased one blob identity';
    END IF;

    BEGIN
        INSERT INTO file_blobs (
            project_id, storage_provider, storage_key, ciphertext_size,
            ciphertext_hash, key_epoch, encrypted_metadata,
            created_by_identity_id, resource_node_id
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            'filesystem', '../../hostile.html', 1,
            decode(repeat('dd', 32), 'hex'), 1, decode('59dd', 'hex'),
            '10000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000062'
        );
        RAISE EXCEPTION 'hostile filesystem key unexpectedly succeeded';
    EXCEPTION
        WHEN check_violation THEN NULL;
    END;

    BEGIN
        INSERT INTO task_completed_attachments (
            project_id, task_id, assignment_id,
            required_attachment_id, blob_id, resource_node_id,
            key_epoch, encrypted_metadata, uploaded_by_identity_id
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '56400000-0000-0000-0000-000000000002',
            '56500000-0000-0000-0000-000000000002',
            '59200000-0000-0000-0000-000000000001',
            '59000000-0000-0000-0000-000000000004',
            '40000000-0000-0000-0000-000000000062',
            1, decode('59ee', 'hex'),
            '10000000-0000-0000-0000-000000000020'
        );
        RAISE EXCEPTION 'non-assignee completed attachment unexpectedly succeeded';
    EXCEPTION
        WHEN insufficient_privilege THEN NULL;
    END;

    BEGIN
        INSERT INTO task_completed_attachments (
            project_id, task_id, assignment_id,
            required_attachment_id, blob_id, resource_node_id,
            key_epoch, encrypted_metadata, uploaded_by_identity_id
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '52000000-0000-0000-0000-000000000001',
            '54000000-0000-0000-0000-000000000001',
            NULL,
            '59000000-0000-0000-0000-000000000005',
            '40000000-0000-0000-0000-000000000004',
            1, decode('59ef', 'hex'),
            '10000000-0000-0000-0000-000000000001'
        );
        RAISE EXCEPTION 'non-assigned owner completed attachment unexpectedly succeeded';
    EXCEPTION
        WHEN insufficient_privilege THEN NULL;
    END;

    BEGIN
        UPDATE task_required_attachments
        SET encrypted_snapshot = decode('ffff', 'hex')
        WHERE id = '59200000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'required attachment provenance mutation unexpectedly succeeded';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
END;
$test$;

UPDATE file_blobs
SET upload_state = 'available', available_at = clock_timestamp()
WHERE id = '59000000-0000-0000-0000-000000000003';

-- T-LLR-08.1 / T-LLR-08.2 / T-LLR-08.3 / T-LLR-08.4 /
-- T-LLR-10.4: exact UTC windows, maximum dependency retention,
-- exactly-once warning rows, per-user export preference, fixed post-purge
-- archive expiry, leases, and anti-resurrection markers.
DO $test$
DECLARE
    source_time timestamptz :=
        '2024-01-31 23:30:00+00'::timestamptz;
BEGIN
    IF sprout_private.add_utc_calendar_months(source_time, 6)
       <> '2024-07-31 23:30:00+00'::timestamptz
       OR sprout_private.add_utc_calendar_months(source_time, 12)
       <> '2025-01-31 23:30:00+00'::timestamptz
       OR sprout_private.add_utc_calendar_months(
           '2024-02-29 23:30:00+00'::timestamptz,
           12
       )
       <> '2025-02-28 23:30:00+00'::timestamptz
    THEN
        RAISE EXCEPTION 'calendar-month UTC retention arithmetic is incorrect';
    END IF;
END;
$test$;

INSERT INTO retention_subjects (
    id, project_id, source_kind, source_id, resource_node_id,
    owner_identity_id, retention_class,
    source_at, warning_at, purge_at
)
VALUES
    (
        '77000000-0000-0000-0000-000000000001',
        '30000000-0000-0000-0000-000000000001',
        'resource_deleted',
        '77000000-0000-0000-0000-000000000011',
        '40000000-0000-0000-0000-000000000041',
        '10000000-0000-0000-0000-000000000001',
        'deleted_or_obsolete',
        '2024-01-01 00:00:00+00',
        '2024-01-16 00:00:00+00',
        '2024-01-31 00:00:00+00'
    ),
    (
        '77000000-0000-0000-0000-000000000002',
        '30000000-0000-0000-0000-000000000001',
        'resource_deleted',
        '77000000-0000-0000-0000-000000000012',
        '40000000-0000-0000-0000-000000000042',
        '10000000-0000-0000-0000-000000000001',
        'deleted_or_obsolete',
        '2024-02-01 00:00:00+00',
        '2024-02-16 00:00:00+00',
        '2024-03-02 00:00:00+00'
    );

INSERT INTO retention_dependencies (
    project_id, subject_id, depends_on_subject_id, reason
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '77000000-0000-0000-0000-000000000002',
    '77000000-0000-0000-0000-000000000001',
    'historical_test_dependency'
);

DO $test$
BEGIN
    IF sprout_private.retention_effective_purge_at(
        '77000000-0000-0000-0000-000000000001'
    ) <> '2024-03-02 00:00:00+00'::timestamptz THEN
        RAISE EXCEPTION 'historical dependency did not extend to maximum deadline';
    END IF;
END;
$test$;

INSERT INTO identity_retention_preferences (
    identity_id, auto_export_enabled
)
VALUES (
    '10000000-0000-0000-0000-000000000001',
    true
);

INSERT INTO retention_warning_deliveries (
    project_id, subject_id, recipient_identity_id, warning_at,
    in_app_enqueued_at, email_enqueued_at
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '77000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    '2024-01-16 00:00:00+00',
    '2024-01-16 00:00:00+00',
    '2024-01-16 00:00:00+00'
)
ON CONFLICT (subject_id, recipient_identity_id, warning_at) DO NOTHING;
INSERT INTO retention_warning_deliveries (
    project_id, subject_id, recipient_identity_id, warning_at,
    in_app_enqueued_at, email_enqueued_at
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    '77000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    '2024-01-16 00:00:00+00',
    '2024-01-16 00:00:00+00',
    '2024-01-16 00:00:00+00'
)
ON CONFLICT (subject_id, recipient_identity_id, warning_at) DO NOTHING;

DO $test$
BEGIN
    IF (
        SELECT count(*)
        FROM retention_warning_deliveries
        WHERE subject_id = '77000000-0000-0000-0000-000000000001'
          AND recipient_identity_id =
              '10000000-0000-0000-0000-000000000001'
    ) <> 1 THEN
        RAISE EXCEPTION 'retention warning was not exactly once per window';
    END IF;
END;
$test$;

INSERT INTO retention_archives (
    id, project_id, subject_id, recipient_identity_id
)
VALUES (
    '78000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '77000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001'
);
UPDATE retention_archives
SET
    source_purged_at = '2024-03-05 10:00:00+00',
    expires_at = '2024-04-04 10:00:00+00'
WHERE id = '78000000-0000-0000-0000-000000000001';

DO $test$
DECLARE
    first_token uuid;
    second_token uuid;
BEGIN
    INSERT INTO retention_leases (
        project_id, lease_scope, partition_key,
        lease_owner, lease_token, acquired_at, heartbeat_at, expires_at
    )
    VALUES (
        '30000000-0000-0000-0000-000000000001',
        'hlt08', 'concurrency',
        '79000000-0000-0000-0000-000000000001',
        '79000000-0000-0000-0000-000000000011',
        '2024-01-01 00:00:00+00',
        '2024-01-01 00:00:00+00',
        '2024-01-01 00:01:00+00'
    )
    RETURNING lease_token INTO first_token;

    INSERT INTO retention_leases (
        project_id, lease_scope, partition_key,
        lease_owner, lease_token, acquired_at, heartbeat_at, expires_at
    )
    VALUES (
        '30000000-0000-0000-0000-000000000001',
        'hlt08', 'concurrency',
        '79000000-0000-0000-0000-000000000002',
        '79000000-0000-0000-0000-000000000012',
        '2024-01-01 00:00:30+00',
        '2024-01-01 00:00:30+00',
        '2024-01-01 00:01:30+00'
    )
    ON CONFLICT (project_id, lease_scope, partition_key)
    DO UPDATE SET lease_token = EXCLUDED.lease_token
    WHERE retention_leases.expires_at <= '2024-01-01 00:00:30+00'
    RETURNING lease_token INTO second_token;
    IF second_token IS NOT NULL OR first_token IS NULL THEN
        RAISE EXCEPTION 'unexpired lease was stolen by a concurrent worker';
    END IF;
END;
$test$;

UPDATE tasks
SET deleted_at = '2024-01-01 00:00:00+00'
WHERE id = '52000000-0000-0000-0000-000000000041';
UPDATE resource_nodes
SET deleted_at = '2024-01-01 00:00:00+00'
WHERE id = '40000000-0000-0000-0000-000000000041';
SELECT sprout_private.materialize_retention_subjects(
    '2025-01-01 00:00:00+00'
);
UPDATE retention_subjects
SET
    state = 'purging',
    lease_owner = '79000000-0000-0000-0000-000000000031',
    lease_token = '79000000-0000-0000-0000-000000000032',
    leased_until = '2025-01-01 01:00:00+00'
WHERE source_kind = 'resource_deleted'
  AND source_id = '40000000-0000-0000-0000-000000000041';

DO $test$
DECLARE
    subject_id uuid;
BEGIN
    SELECT id INTO subject_id
    FROM retention_subjects
    WHERE source_kind = 'resource_deleted'
      AND source_id = '40000000-0000-0000-0000-000000000041';
    IF NOT sprout_private.purge_retention_subject(
        subject_id,
        '79000000-0000-0000-0000-000000000032',
        '2025-01-01 00:00:00+00'
    ) OR NOT sprout_private.purge_retention_subject(
        subject_id,
        '79000000-0000-0000-0000-000000000032',
        '2025-01-01 00:00:00+00'
    ) THEN
        RAISE EXCEPTION 'leased purge was not idempotent';
    END IF;
    IF EXISTS (
        SELECT 1 FROM tasks
        WHERE id = '52000000-0000-0000-0000-000000000041'
    ) OR EXISTS (
        SELECT 1 FROM resource_nodes
        WHERE id = '40000000-0000-0000-0000-000000000041'
    ) OR NOT EXISTS (
        SELECT 1 FROM purge_markers
        WHERE resource_node_id =
            '40000000-0000-0000-0000-000000000041'
    ) THEN
        RAISE EXCEPTION 'controlled purge did not remove source and preserve marker';
    END IF;
END;
$test$;

INSERT INTO purge_markers (
    project_id, source_kind, source_id, resource_node_id,
    final_aggregate_version, final_event_hash, purged_at
)
VALUES (
    '30000000-0000-0000-0000-000000000001',
    'resource_deleted',
    '79000000-0000-0000-0000-000000000021',
    '40000000-0000-0000-0000-000000000004',
    2,
    decode(repeat('35', 32), 'hex'),
    clock_timestamp()
);

DO $test$
BEGIN
    BEGIN
        INSERT INTO sync_events (
            project_id, stream_id, resource_node_id,
            base_version, aggregate_version, mutation_kind,
            actor_identity_id, actor_device_id,
            actor_device_key_version, device_sequence,
            client_event_id, event_kind, key_epoch,
            encrypted_payload, previous_hash, event_hash,
            signature, post_quantum_signature, client_created_at
        )
        VALUES (
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000004',
            '40000000-0000-0000-0000-000000000004',
            2, 3, 'upsert',
            '10000000-0000-0000-0000-000000000001',
            '20000000-0000-0000-0000-000000000001',
            1, 99,
            '79000000-0000-0000-0000-000000000022',
            'stale_resurrection', 1,
            decode('79', 'hex'),
            decode(repeat('35', 32), 'hex'),
            decode(repeat('79', 32), 'hex'),
            decode(repeat('78', 64), 'hex'),
            decode('77', 'hex'),
            clock_timestamp()
        );
        RAISE EXCEPTION 'purge marker allowed stale-client resurrection';
    EXCEPTION
        WHEN SQLSTATE '55000' THEN NULL;
    END;
END;
$test$;

-- R5.0033: the runtime route uses a narrow SECURITY DEFINER writer. A
-- NOBYPASSRLS application role cannot write the permission ledger directly or
-- invoke the private writer, even after forging identity/project GUCs.
INSERT INTO agent_external_tool_catalog (
    tool_name, version, adapter_protocol, operation, risk_tier, availability,
    effect_class, max_attempts, max_timeout_seconds, max_input_bytes,
    max_output_bytes, input_schema, input_schema_hash, output_schema,
    output_schema_hash, required_effects, output_audience_kind,
    terminal_status_mapping, manifest_hash
)
SELECT tool_name, 2, adapter_protocol || '-test-v2', operation, risk_tier,
       availability, effect_class, max_attempts, max_timeout_seconds,
       max_input_bytes, max_output_bytes, input_schema, input_schema_hash,
       output_schema, output_schema_hash, required_effects,
       output_audience_kind, terminal_status_mapping,
       digest('verify-behavior-web-read-v2', 'sha256')
FROM agent_external_tool_catalog
WHERE tool_name = 'web.read' AND version = 1;

SELECT set_config('app.identity_id',
    '10000000-0000-0000-0000-000000000001', true);
SELECT set_config('app.project_id',
    '30000000-0000-0000-0000-000000000001', true);

SELECT permission_id
FROM sprout_private.grant_agent_tool_permission(
    'a3300000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001', NULL,
    'web.read', 1,
    '10000000-0000-0000-0000-000000000001',
    'a3300000-0000-0000-0000-000000000011',
    digest('grant-web-read-v1', 'sha256')
);
SELECT permission_id
FROM sprout_private.grant_agent_tool_permission(
    'a3300000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001', NULL,
    'web.read', 2,
    '10000000-0000-0000-0000-000000000001',
    'a3300000-0000-0000-0000-000000000012',
    digest('grant-web-read-v2', 'sha256')
);

CREATE ROLE sprout_tool_permission_app NOSUPERUSER NOBYPASSRLS NOLOGIN;
GRANT sprout_tool_permission_app TO CURRENT_USER;
GRANT USAGE ON SCHEMA public, sprout_private TO sprout_tool_permission_app;
GRANT SELECT, INSERT, UPDATE ON agent_tool_permissions TO sprout_tool_permission_app;
SET LOCAL ROLE sprout_tool_permission_app;

DO $tool_permission_rls$
BEGIN
    BEGIN
        INSERT INTO agent_tool_permissions (
            id, project_id, principal_identity_id, tool_name, tool_version,
            granted_by_identity_id, idempotency_key, grant_hash
        ) VALUES (
            'a3300000-0000-0000-0000-000000000003',
            current_setting('app.project_id')::uuid,
            current_setting('app.identity_id')::uuid,
            'web.read', 1, current_setting('app.identity_id')::uuid,
            'a3300000-0000-0000-0000-000000000013',
            digest('forged-direct-grant', 'sha256')
        );
        RAISE EXCEPTION 'application role directly inserted tool permission';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    BEGIN
        UPDATE agent_tool_permissions SET revoked_at = clock_timestamp(),
            revoked_by_identity_id = current_setting('app.identity_id')::uuid
        WHERE id = 'a3300000-0000-0000-0000-000000000001';
        RAISE EXCEPTION 'application role directly revoked tool permission';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    BEGIN
        PERFORM sprout_private.grant_agent_tool_permission(
            'a3300000-0000-0000-0000-000000000004',
            current_setting('app.project_id')::uuid,
            '40000000-0000-0000-0000-000000000001',
            current_setting('app.identity_id')::uuid, NULL,
            'web.read', 1, current_setting('app.identity_id')::uuid,
            'a3300000-0000-0000-0000-000000000014',
            digest('forged-private-writer', 'sha256')
        );
        RAISE EXCEPTION 'application role executed private permission writer';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END;
$tool_permission_rls$;
RESET ROLE;

-- The server obtains its database identity exclusively from DATABASE_URL.  The
-- deployed route role needs EXECUTE on the narrow writer, not table DML.  This
-- non-bypass role models that split explicitly instead of relying on the
-- disposable database owner's superuser behavior.
CREATE ROLE sprout_tool_permission_route_app NOSUPERUSER NOBYPASSRLS NOLOGIN;
GRANT sprout_tool_permission_route_app TO CURRENT_USER;
GRANT USAGE ON SCHEMA public, sprout_private TO sprout_tool_permission_route_app;
GRANT EXECUTE ON FUNCTION sprout_private.grant_agent_tool_permission(
    uuid, uuid, uuid, uuid, uuid, text, integer, uuid, uuid, bytea
) TO sprout_tool_permission_route_app;
SET LOCAL ROLE sprout_tool_permission_route_app;
SELECT set_config('app.identity_id',
    '10000000-0000-0000-0000-000000000001', true);
SELECT set_config('app.project_id',
    '30000000-0000-0000-0000-000000000001', true);
DO $tool_permission_route_app$
DECLARE
    written uuid;
BEGIN
    SELECT permission_id INTO written
    FROM sprout_private.grant_agent_tool_permission(
        'a3300000-0000-0000-0000-000000000008',
        '30000000-0000-0000-0000-000000000001',
        '40000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001', NULL,
        'document.local.read', 1,
        '10000000-0000-0000-0000-000000000001',
        'a3300000-0000-0000-0000-000000000018',
        digest('route-app-document-read-v1', 'sha256')
    );
    IF written IS DISTINCT FROM 'a3300000-0000-0000-0000-000000000008'::uuid THEN
        RAISE EXCEPTION 'authorized route role did not use trusted writer';
    END IF;
END;
$tool_permission_route_app$;

SELECT set_config('app.identity_id',
    '10000000-0000-0000-0000-000000000020', true);
DO $tool_permission_route_forged_identity$
BEGIN
    BEGIN
        PERFORM sprout_private.grant_agent_tool_permission(
            'a3300000-0000-0000-0000-000000000009',
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000020', NULL,
            'document.local.read', 1,
            '10000000-0000-0000-0000-000000000020',
            'a3300000-0000-0000-0000-000000000019',
            digest('forged-route-identity', 'sha256')
        );
        RAISE EXCEPTION 'forged route identity granted tool permission';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END;
$tool_permission_route_forged_identity$;

SELECT set_config('app.identity_id',
    '10000000-0000-0000-0000-000000000001', true);
SELECT set_config('app.project_id',
    '30000000-0000-0000-0000-000000000002', true);
DO $tool_permission_route_forged_project$
BEGIN
    BEGIN
        PERFORM sprout_private.grant_agent_tool_permission(
            'a3300000-0000-0000-0000-000000000010',
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000001', NULL,
            'document.local.read', 1,
            '10000000-0000-0000-0000-000000000001',
            'a3300000-0000-0000-0000-00000000001a',
            digest('forged-route-project', 'sha256')
        );
        RAISE EXCEPTION 'forged route project granted tool permission';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END;
$tool_permission_route_forged_project$;
RESET ROLE;
SELECT set_config('app.project_id',
    '30000000-0000-0000-0000-000000000001', true);

DO $tool_permission_authority$
BEGIN
    -- A cross-project argument cannot be smuggled under project-1 context.
    BEGIN
        PERFORM sprout_private.grant_agent_tool_permission(
            'a3300000-0000-0000-0000-000000000005',
            '30000000-0000-0000-0000-000000000002',
            '40000000-0000-0000-0000-000000000005',
            '10000000-0000-0000-0000-000000000001', NULL,
            'web.read', 1,
            '10000000-0000-0000-0000-000000000001',
            'a3300000-0000-0000-0000-000000000015',
            digest('cross-project-grant', 'sha256')
        );
        RAISE EXCEPTION 'cross-project permission writer succeeded';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END;
$tool_permission_authority$;

SELECT set_config('app.identity_id',
    '10000000-0000-0000-0000-000000000020', true);
DO $tool_permission_unauthorized$
BEGIN
    BEGIN
        PERFORM sprout_private.grant_agent_tool_permission(
            'a3300000-0000-0000-0000-000000000006',
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000001',
            '10000000-0000-0000-0000-000000000020', NULL,
            'web.read', 1,
            '10000000-0000-0000-0000-000000000020',
            'a3300000-0000-0000-0000-000000000016',
            digest('unauthorized-grant', 'sha256')
        );
        RAISE EXCEPTION 'member without Manage granted tool permission';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END;
$tool_permission_unauthorized$;

SELECT set_config('app.identity_id',
    '91000000-0000-0000-0000-000000000001', true);
DO $tool_permission_agent$
BEGIN
    BEGIN
        PERFORM sprout_private.grant_agent_tool_permission(
            'a3300000-0000-0000-0000-000000000007',
            '30000000-0000-0000-0000-000000000001',
            '40000000-0000-0000-0000-000000000003',
            '91000000-0000-0000-0000-000000000001',
            '92000000-0000-0000-0000-000000000001',
            'web.read', 1,
            '91000000-0000-0000-0000-000000000001',
            'a3300000-0000-0000-0000-000000000017',
            digest('agent-self-grant', 'sha256')
        );
        RAISE EXCEPTION 'agent self-granted tool permission';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END;
$tool_permission_agent$;

SELECT set_config('app.identity_id',
    '10000000-0000-0000-0000-000000000001', true);
SELECT permission_id
FROM sprout_private.revoke_agent_tool_permission(
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001', NULL,
    'web.read', 1,
    '10000000-0000-0000-0000-000000000001'
);

DO $tool_permission_versions$
BEGIN
    IF EXISTS (
        SELECT 1 FROM agent_tool_permissions
        WHERE id = 'a3300000-0000-0000-0000-000000000001'
          AND revoked_at IS NULL
    ) OR NOT EXISTS (
        SELECT 1 FROM agent_tool_permissions
        WHERE id = 'a3300000-0000-0000-0000-000000000002'
          AND tool_version = 2 AND revoked_at IS NULL
    ) THEN
        RAISE EXCEPTION 'version-exact tool permission revoke failed';
    END IF;
END;
$tool_permission_versions$;

ROLLBACK;

SELECT 'sprout behavioral verification passed' AS result;
