-- Endpoint-TCB compilation certificates for R5.35/R5.37. The controller or
-- administrator device sees the E2EE plaintext and signs a closed compiler
-- artifact. The server remains unable to decrypt and retains authority over
-- schema, scope, action catalog, permission and governance validation.

CREATE TABLE agent_compiler_builds (
    task_kind text NOT NULL CHECK (task_kind IN ('responsibility', 'local_goal')),
    compiler_name text NOT NULL CHECK (compiler_name ~ '^[a-z0-9._-]{3,128}$'),
    compiler_version integer NOT NULL CHECK (compiler_version > 0),
    build_digest bytea NOT NULL CHECK (octet_length(build_digest) = 32),
    artifact_kind text NOT NULL CHECK (artifact_kind IN ('protocol_manifest', 'executable')),
    enabled boolean NOT NULL DEFAULT true,
    registered_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (task_kind, compiler_name, compiler_version),
    UNIQUE (task_kind, compiler_name, compiler_version, build_digest)
);

INSERT INTO agent_compiler_builds (
    task_kind, compiler_name, compiler_version, build_digest, artifact_kind
) VALUES
    ('responsibility', 'sprout.responsibility.compiler', 1,
     decode('78bd83db79112191f81aa118512092f7ea54a87733a82e823fa83cf107e3eb73', 'hex'),
     'protocol_manifest'),
    ('local_goal', 'sprout.local-goal.compiler', 1,
     decode('0c675e853701375c7ba5d396f4e1f9b55592339a3a4e45859b9f2c2e8fdbbfc2', 'hex'),
     'protocol_manifest');

CREATE FUNCTION sprout_private.reject_compiler_build_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'pinned compiler registry is migration-managed'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_compiler_builds_immutable
BEFORE UPDATE OR DELETE ON agent_compiler_builds
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_compiler_build_mutation();

CREATE TABLE agent_compilation_certificates (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    task_kind text NOT NULL CHECK (task_kind IN ('responsibility', 'local_goal')),
    compiler_name text NOT NULL,
    compiler_version integer NOT NULL CHECK (compiler_version > 0),
    compiler_build_digest bytea NOT NULL CHECK (octet_length(compiler_build_digest) = 32),
    signer_identity_id uuid NOT NULL,
    signer_device_id uuid NOT NULL,
    signer_device_key_version integer NOT NULL CHECK (signer_device_key_version > 0),
    subject_id uuid NOT NULL,
    subject_revision bigint NOT NULL CHECK (subject_revision > 0),
    draft_id uuid NOT NULL,
    agent_principal_identity_id uuid,
    controller_identity_id uuid,
    administrator_identity_id uuid,
    user_identity_id uuid,
    input_commitment bytea NOT NULL CHECK (octet_length(input_commitment) = 32),
    ciphertext_commitment bytea NOT NULL CHECK (octet_length(ciphertext_commitment) = 32),
    canonical_output jsonb NOT NULL CHECK (jsonb_typeof(canonical_output) = 'object'),
    output_hash bytea NOT NULL CHECK (octet_length(output_hash) = 32),
    compilation_envelope jsonb NOT NULL CHECK (jsonb_typeof(compilation_envelope) = 'object'),
    envelope_hash bytea NOT NULL CHECK (octet_length(envelope_hash) = 32),
    certificate_hash bytea NOT NULL CHECK (octet_length(certificate_hash) = 32),
    idempotency_key uuid NOT NULL,
    classical_signature bytea NOT NULL CHECK (octet_length(classical_signature) = 64),
    post_quantum_signature bytea NOT NULL CHECK (octet_length(post_quantum_signature) > 0),
    classifier_version integer,
    classifier_output_hash bytea,
    authorization_kind text NOT NULL CHECK (authorization_kind IN (
        'responsibility_compilation', 'responsibility',
        'administrator_exception', 'global_mandate', 'administrator_creation'
    )),
    authorization_id uuid,
    authorization_revision bigint,
    verification_state text NOT NULL CHECK (verification_state IN (
        'verified', 'legacy_unverified'
    )),
    verified_at timestamptz,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_compilation_build_fk
        FOREIGN KEY (task_kind, compiler_name, compiler_version, compiler_build_digest)
        REFERENCES agent_compiler_builds (
            task_kind, compiler_name, compiler_version, build_digest
        ) ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_compilation_signer_key_fk
        FOREIGN KEY (signer_identity_id, signer_device_id, signer_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_compilation_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_compilation_subject_shape CHECK (
        (task_kind = 'responsibility'
            AND administrator_identity_id IS NOT NULL
            AND user_identity_id IS NOT NULL
            AND agent_principal_identity_id IS NULL
            AND controller_identity_id IS NULL
            AND classifier_version IS NULL
            AND classifier_output_hash IS NULL)
        OR
        (task_kind = 'local_goal'
            AND agent_principal_identity_id IS NOT NULL
            AND controller_identity_id IS NOT NULL
            AND administrator_identity_id IS NULL
            AND user_identity_id IS NULL
            AND classifier_version > 0
            AND octet_length(classifier_output_hash) = 32)
    ),
    CONSTRAINT agent_compilation_authorization_shape CHECK (
        (authorization_kind IN ('responsibility', 'administrator_exception', 'global_mandate')
            AND authorization_id IS NOT NULL AND authorization_revision IS NOT NULL)
        OR
        (authorization_kind = 'administrator_creation'
            AND authorization_id IS NOT NULL AND authorization_revision IS NULL)
        OR
        (authorization_kind = 'responsibility_compilation'
            AND authorization_id IS NULL AND authorization_revision IS NULL)
    ),
    CONSTRAINT agent_compilation_verification_shape CHECK (
        (verification_state = 'verified' AND verified_at IS NOT NULL)
        OR (verification_state = 'legacy_unverified' AND verified_at IS NULL)
    ),
    CONSTRAINT agent_compilation_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT agent_compilation_draft_unique UNIQUE (project_id, draft_id),
    CONSTRAINT agent_compilation_idempotency_unique
        UNIQUE (project_id, signer_identity_id, idempotency_key),
    -- Fail closed on observable compiler equivocation. A retry for the same
    -- compiler/input/subject can only recover this same row and output hash.
    CONSTRAINT agent_compilation_subject_revision_unique UNIQUE (
        project_id, task_kind, subject_id, subject_revision
    )
);

ALTER TABLE agent_responsibility_contracts
    ADD COLUMN compilation_certificate_id uuid,
    ADD CONSTRAINT agent_responsibility_compilation_fk
        FOREIGN KEY (project_id, compilation_certificate_id)
        REFERENCES agent_compilation_certificates (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

ALTER TABLE agent_local_goal_contracts
    ADD COLUMN compilation_certificate_id uuid,
    ADD COLUMN classifier_version integer,
    ADD COLUMN classifier_output_hash bytea,
    ADD COLUMN administrator_creation_approval_id uuid,
    ADD CONSTRAINT agent_local_goal_compilation_fk
        FOREIGN KEY (project_id, compilation_certificate_id)
        REFERENCES agent_compilation_certificates (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT agent_local_goal_classifier_shape CHECK (
        (compilation_certificate_id IS NULL
            AND classifier_version IS NULL AND classifier_output_hash IS NULL)
        OR
        (compilation_certificate_id IS NOT NULL
            AND classifier_version > 0
            AND octet_length(classifier_output_hash) = 32)
    );

-- Existing 0027 approvals lack an independent signed witness and are not
-- retroactively upgraded into valid certificates.
ALTER TABLE agent_prompt_final_approvals
    ADD COLUMN approval_id uuid NOT NULL DEFAULT gen_random_uuid(),
    ADD COLUMN idempotency_key uuid NOT NULL DEFAULT gen_random_uuid(),
    ADD COLUMN agent_principal_identity_id uuid,
    ADD COLUMN signer_device_id uuid,
    ADD COLUMN signer_device_key_version integer,
    ADD COLUMN prompt_input_commitment bytea,
    ADD COLUMN ciphertext_commitment bytea,
    ADD COLUMN compilation_certificate_id uuid,
    ADD COLUMN structured_output_hash bytea,
    ADD COLUMN approval_identity_hash bytea,
    ADD COLUMN approval_hash bytea,
    ADD COLUMN classical_signature bytea,
    ADD COLUMN post_quantum_signature bytea,
    ADD COLUMN verification_state text NOT NULL DEFAULT 'legacy_unverified'
        CHECK (verification_state IN ('verified', 'legacy_unverified')),
    ADD CONSTRAINT agent_prompt_approval_verified_shape CHECK (
        (verification_state = 'legacy_unverified'
            AND agent_principal_identity_id IS NULL
            AND signer_device_id IS NULL
            AND signer_device_key_version IS NULL
            AND prompt_input_commitment IS NULL
            AND ciphertext_commitment IS NULL
            AND compilation_certificate_id IS NULL
            AND structured_output_hash IS NULL
            AND approval_identity_hash IS NULL
            AND approval_hash IS NULL
            AND classical_signature IS NULL
            AND post_quantum_signature IS NULL)
        OR
        (verification_state = 'verified'
            AND agent_principal_identity_id IS NOT NULL
            AND signer_device_id IS NOT NULL
            AND signer_device_key_version > 0
            AND octet_length(prompt_input_commitment) = 32
            AND octet_length(ciphertext_commitment) = 32
            AND compilation_certificate_id IS NOT NULL
            AND octet_length(structured_output_hash) = 32
            AND octet_length(approval_identity_hash) = 32
            AND octet_length(approval_hash) = 32
            AND octet_length(classical_signature) = 64
            AND octet_length(post_quantum_signature) > 0)
    ),
    ADD CONSTRAINT agent_prompt_approval_signer_key_fk
        FOREIGN KEY (
            controller_identity_id, signer_device_id, signer_device_key_version
        ) REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT agent_prompt_approval_compilation_fk
        FOREIGN KEY (project_id, compilation_certificate_id)
        REFERENCES agent_compilation_certificates (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT agent_prompt_approval_id_unique UNIQUE (project_id, approval_id),
    ADD CONSTRAINT agent_prompt_approval_idempotency_unique
        UNIQUE (project_id, controller_identity_id, idempotency_key);

CREATE TABLE agent_administrator_creation_approvals (
    project_id uuid NOT NULL,
    approval_id uuid NOT NULL,
    administrator_identity_id uuid NOT NULL,
    signer_device_id uuid NOT NULL,
    signer_device_key_version integer NOT NULL CHECK (signer_device_key_version > 0),
    proposed_agent_identity_id uuid NOT NULL,
    governed_agent_id uuid NOT NULL,
    proposal_draft_id uuid NOT NULL,
    local_goal_id uuid NOT NULL,
    local_goal_revision bigint NOT NULL CHECK (local_goal_revision = 1),
    contract_hash bytea NOT NULL CHECK (octet_length(contract_hash) = 32),
    compilation_certificate_id uuid NOT NULL,
    prompt_input_commitment bytea NOT NULL CHECK (octet_length(prompt_input_commitment) = 32),
    ciphertext_commitment bytea NOT NULL CHECK (octet_length(ciphertext_commitment) = 32),
    availability text NOT NULL CHECK (availability IN ('controller_private', 'project_delegable')),
    scope_resource_node_id uuid NOT NULL,
    canonical_proposal_hash bytea NOT NULL CHECK (octet_length(canonical_proposal_hash) = 32),
    idempotency_key uuid NOT NULL,
    approval_hash bytea NOT NULL CHECK (octet_length(approval_hash) = 32),
    classical_signature bytea NOT NULL CHECK (octet_length(classical_signature) = 64),
    post_quantum_signature bytea NOT NULL CHECK (octet_length(post_quantum_signature) > 0),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, approval_id),
    UNIQUE (project_id, administrator_identity_id, idempotency_key),
    UNIQUE (project_id, governed_agent_id),
    UNIQUE (project_id, proposal_draft_id),
    CONSTRAINT administrator_creation_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT administrator_creation_compilation_fk
        FOREIGN KEY (project_id, compilation_certificate_id)
        REFERENCES agent_compilation_certificates (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT administrator_creation_signer_key_fk
        FOREIGN KEY (administrator_identity_id, signer_device_id, signer_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

ALTER TABLE agent_local_goal_contracts
    ADD CONSTRAINT agent_local_goal_administrator_creation_fk
        FOREIGN KEY (project_id, administrator_creation_approval_id)
        REFERENCES agent_administrator_creation_approvals (project_id, approval_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

CREATE SEQUENCE agent_governance_ledger_position_seq AS bigint;

CREATE TABLE agent_governance_ledger (
    position bigint PRIMARY KEY DEFAULT nextval('agent_governance_ledger_position_seq'),
    project_id uuid NOT NULL,
    entry_kind text NOT NULL CHECK (entry_kind IN (
        'compilation', 'final_prompt_approval', 'administrator_creation_approval',
        'responsibility_revision', 'local_goal_revision'
    )),
    entry_id uuid NOT NULL,
    subject_id uuid NOT NULL,
    subject_revision bigint NOT NULL CHECK (subject_revision > 0),
    entry_hash bytea NOT NULL CHECK (octet_length(entry_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (project_id, entry_kind, entry_id),
    UNIQUE (project_id, position),
    CONSTRAINT agent_governance_ledger_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE FUNCTION sprout_private.insert_verified_compilation_certificate(
    candidate_id uuid, candidate_project_id uuid, candidate_task_kind text,
    candidate_compiler_id text, candidate_compiler_version integer,
    candidate_build_digest bytea, candidate_signer_identity_id uuid,
    candidate_signer_device_id uuid, candidate_signer_key_version integer,
    candidate_subject_id uuid, candidate_subject_revision bigint,
    candidate_draft_id uuid, candidate_agent_identity_id uuid,
    candidate_controller_identity_id uuid, candidate_administrator_identity_id uuid,
    candidate_user_identity_id uuid, candidate_input_commitment bytea,
    candidate_ciphertext_commitment bytea, candidate_output jsonb,
    candidate_output_hash bytea, candidate_envelope jsonb,
    candidate_envelope_hash bytea, candidate_certificate_hash bytea,
    candidate_idempotency_key uuid, candidate_classical_signature bytea,
    candidate_post_quantum_signature bytea, candidate_classifier_version integer,
    candidate_classifier_output_hash bytea, candidate_authorization_kind text,
    candidate_authorization_id uuid, candidate_authorization_revision bigint
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    inserted_rows bigint;
BEGIN
    IF candidate_signer_identity_id <> sprout_private.current_identity_id()
       OR NOT EXISTS (
           SELECT 1 FROM agent_compiler_builds build
           WHERE build.task_kind = candidate_task_kind
             AND build.compiler_name = candidate_compiler_id
             AND build.compiler_version = candidate_compiler_version
             AND build.build_digest = candidate_build_digest
             AND build.enabled
       )
    THEN
        RAISE EXCEPTION 'untrusted compiler certificate writer'
            USING ERRCODE = '42501';
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended(candidate_project_id::text, 40));
    INSERT INTO agent_compilation_certificates (
        id, project_id, task_kind, compiler_name, compiler_version,
        compiler_build_digest, signer_identity_id, signer_device_id,
        signer_device_key_version, subject_id, subject_revision, draft_id,
        agent_principal_identity_id, controller_identity_id,
        administrator_identity_id, user_identity_id, input_commitment,
        ciphertext_commitment, canonical_output, output_hash,
        compilation_envelope, envelope_hash, certificate_hash,
        idempotency_key, classical_signature, post_quantum_signature,
        classifier_version, classifier_output_hash, authorization_kind,
        authorization_id, authorization_revision, verification_state, verified_at
    ) VALUES (
        candidate_id, candidate_project_id, candidate_task_kind,
        candidate_compiler_id, candidate_compiler_version, candidate_build_digest,
        candidate_signer_identity_id, candidate_signer_device_id,
        candidate_signer_key_version, candidate_subject_id,
        candidate_subject_revision, candidate_draft_id,
        candidate_agent_identity_id, candidate_controller_identity_id,
        candidate_administrator_identity_id, candidate_user_identity_id,
        candidate_input_commitment, candidate_ciphertext_commitment,
        candidate_output, candidate_output_hash, candidate_envelope,
        candidate_envelope_hash, candidate_certificate_hash,
        candidate_idempotency_key, candidate_classical_signature,
        candidate_post_quantum_signature, candidate_classifier_version,
        candidate_classifier_output_hash, candidate_authorization_kind,
        candidate_authorization_id, candidate_authorization_revision,
        'verified', clock_timestamp()
    ) ON CONFLICT (project_id, id) DO NOTHING;
    GET DIAGNOSTICS inserted_rows = ROW_COUNT;
    IF inserted_rows = 0 THEN
        IF NOT EXISTS (
            SELECT 1 FROM agent_compilation_certificates certificate
            WHERE certificate.project_id = candidate_project_id
              AND certificate.id = candidate_id
              AND certificate.task_kind = candidate_task_kind
              AND certificate.compiler_name = candidate_compiler_id
              AND certificate.compiler_version = candidate_compiler_version
              AND certificate.compiler_build_digest = candidate_build_digest
              AND certificate.signer_identity_id = candidate_signer_identity_id
              AND certificate.signer_device_id = candidate_signer_device_id
              AND certificate.signer_device_key_version = candidate_signer_key_version
              AND certificate.subject_id = candidate_subject_id
              AND certificate.subject_revision = candidate_subject_revision
              AND certificate.draft_id = candidate_draft_id
              AND certificate.agent_principal_identity_id IS NOT DISTINCT FROM candidate_agent_identity_id
              AND certificate.controller_identity_id IS NOT DISTINCT FROM candidate_controller_identity_id
              AND certificate.administrator_identity_id IS NOT DISTINCT FROM candidate_administrator_identity_id
              AND certificate.user_identity_id IS NOT DISTINCT FROM candidate_user_identity_id
              AND certificate.input_commitment = candidate_input_commitment
              AND certificate.ciphertext_commitment = candidate_ciphertext_commitment
              AND certificate.canonical_output = candidate_output
              AND certificate.output_hash = candidate_output_hash
              AND certificate.compilation_envelope = candidate_envelope
              AND certificate.envelope_hash = candidate_envelope_hash
              AND certificate.certificate_hash = candidate_certificate_hash
              AND certificate.idempotency_key = candidate_idempotency_key
              AND certificate.classical_signature = candidate_classical_signature
              AND certificate.post_quantum_signature = candidate_post_quantum_signature
              AND certificate.classifier_version IS NOT DISTINCT FROM candidate_classifier_version
              AND certificate.classifier_output_hash IS NOT DISTINCT FROM candidate_classifier_output_hash
              AND certificate.authorization_kind = candidate_authorization_kind
              AND certificate.authorization_id IS NOT DISTINCT FROM candidate_authorization_id
              AND certificate.authorization_revision IS NOT DISTINCT FROM candidate_authorization_revision
              AND certificate.verification_state = 'verified'
        ) THEN
            RAISE EXCEPTION 'compiler certificate replay conflict'
                USING ERRCODE = '23505';
        END IF;
        RETURN;
    END IF;
    INSERT INTO agent_governance_ledger (
        project_id, entry_kind, entry_id, subject_id, subject_revision, entry_hash
    ) VALUES (
        candidate_project_id, 'compilation', candidate_id,
        candidate_subject_id, candidate_subject_revision, candidate_certificate_hash
    );
END;
$$;

CREATE FUNCTION sprout_private.insert_verified_administrator_creation_approval(
    candidate_project_id uuid, candidate_approval_id uuid,
    candidate_administrator_identity_id uuid, candidate_signer_device_id uuid,
    candidate_signer_key_version integer, candidate_agent_identity_id uuid,
    candidate_governed_agent_id uuid, candidate_draft_id uuid,
    candidate_local_goal_id uuid, candidate_local_revision bigint,
    candidate_contract_hash bytea, candidate_compilation_id uuid,
    candidate_prompt_commitment bytea, candidate_ciphertext_commitment bytea,
    candidate_availability text, candidate_scope_id uuid,
    candidate_proposal_hash bytea, candidate_idempotency_key uuid,
    candidate_approval_hash bytea, candidate_classical_signature bytea,
    candidate_post_quantum_signature bytea
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    inserted_rows bigint;
BEGIN
    IF candidate_administrator_identity_id <> sprout_private.current_identity_id()
       OR NOT EXISTS (
           SELECT 1 FROM project_memberships membership
           WHERE membership.project_id = candidate_project_id
             AND membership.identity_id = candidate_administrator_identity_id
             AND membership.state = 'active'
             AND membership.role IN ('owner', 'admin')
       )
       OR NOT EXISTS (
           SELECT 1 FROM agent_compilation_certificates certificate
           WHERE certificate.project_id = candidate_project_id
             AND certificate.id = candidate_compilation_id
             AND certificate.task_kind = 'local_goal'
             AND certificate.subject_id = candidate_local_goal_id
             AND certificate.subject_revision = candidate_local_revision
             AND certificate.draft_id = candidate_draft_id
             AND certificate.agent_principal_identity_id = candidate_agent_identity_id
             AND certificate.controller_identity_id = candidate_administrator_identity_id
             AND certificate.input_commitment = candidate_prompt_commitment
             AND certificate.ciphertext_commitment = candidate_ciphertext_commitment
             AND certificate.authorization_kind = 'administrator_creation'
             AND certificate.authorization_id = candidate_approval_id
             AND certificate.authorization_revision IS NULL
             AND certificate.verification_state = 'verified'
       )
    THEN
        RAISE EXCEPTION 'exact administrator creation certificate required'
            USING ERRCODE = '42501';
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended(candidate_project_id::text, 40));
    INSERT INTO agent_administrator_creation_approvals (
        project_id, approval_id, administrator_identity_id, signer_device_id,
        signer_device_key_version, proposed_agent_identity_id, governed_agent_id,
        proposal_draft_id, local_goal_id, local_goal_revision, contract_hash,
        compilation_certificate_id, prompt_input_commitment,
        ciphertext_commitment, availability, scope_resource_node_id,
        canonical_proposal_hash, idempotency_key, approval_hash,
        classical_signature, post_quantum_signature
    ) VALUES (
        candidate_project_id, candidate_approval_id,
        candidate_administrator_identity_id, candidate_signer_device_id,
        candidate_signer_key_version, candidate_agent_identity_id,
        candidate_governed_agent_id, candidate_draft_id,
        candidate_local_goal_id, candidate_local_revision,
        candidate_contract_hash, candidate_compilation_id,
        candidate_prompt_commitment, candidate_ciphertext_commitment,
        candidate_availability, candidate_scope_id, candidate_proposal_hash,
        candidate_idempotency_key, candidate_approval_hash,
        candidate_classical_signature, candidate_post_quantum_signature
    ) ON CONFLICT (project_id, approval_id) DO NOTHING;
    GET DIAGNOSTICS inserted_rows = ROW_COUNT;
    IF inserted_rows = 0 THEN
        IF NOT EXISTS (
            SELECT 1 FROM agent_administrator_creation_approvals approval
            WHERE approval.project_id = candidate_project_id
              AND approval.approval_id = candidate_approval_id
              AND approval.administrator_identity_id = candidate_administrator_identity_id
              AND approval.signer_device_id = candidate_signer_device_id
              AND approval.signer_device_key_version = candidate_signer_key_version
              AND approval.proposed_agent_identity_id = candidate_agent_identity_id
              AND approval.governed_agent_id = candidate_governed_agent_id
              AND approval.proposal_draft_id = candidate_draft_id
              AND approval.local_goal_id = candidate_local_goal_id
              AND approval.local_goal_revision = candidate_local_revision
              AND approval.contract_hash = candidate_contract_hash
              AND approval.compilation_certificate_id = candidate_compilation_id
              AND approval.prompt_input_commitment = candidate_prompt_commitment
              AND approval.ciphertext_commitment = candidate_ciphertext_commitment
              AND approval.availability = candidate_availability
              AND approval.scope_resource_node_id = candidate_scope_id
              AND approval.canonical_proposal_hash = candidate_proposal_hash
              AND approval.idempotency_key = candidate_idempotency_key
              AND approval.approval_hash = candidate_approval_hash
              AND approval.classical_signature = candidate_classical_signature
              AND approval.post_quantum_signature = candidate_post_quantum_signature
        ) THEN
            RAISE EXCEPTION 'administrator creation approval replay conflict'
                USING ERRCODE = '23505';
        END IF;
        RETURN;
    END IF;
    INSERT INTO agent_governance_ledger (
        project_id, entry_kind, entry_id, subject_id, subject_revision, entry_hash
    ) VALUES (
        candidate_project_id, 'administrator_creation_approval',
        candidate_approval_id, candidate_local_goal_id,
        candidate_local_revision, candidate_approval_hash
    );
END;
$$;

CREATE FUNCTION sprout_private.insert_verified_final_prompt_approval(
    candidate_project_id uuid, candidate_draft_id uuid, candidate_agent_id uuid,
    candidate_controller_identity_id uuid, candidate_local_goal_id uuid,
    candidate_local_revision bigint, candidate_prompt_hash bytea,
    candidate_approval_id uuid, candidate_idempotency_key uuid,
    candidate_agent_identity_id uuid, candidate_signer_device_id uuid,
    candidate_signer_key_version integer, candidate_prompt_commitment bytea,
    candidate_ciphertext_commitment bytea, candidate_compilation_id uuid,
    candidate_output_hash bytea, candidate_approval_identity_hash bytea,
    candidate_approval_hash bytea, candidate_classical_signature bytea,
    candidate_post_quantum_signature bytea
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    inserted_rows bigint;
BEGIN
    IF candidate_controller_identity_id <> sprout_private.current_identity_id()
       OR NOT EXISTS (
           SELECT 1 FROM agent_compilation_certificates certificate
           WHERE certificate.project_id = candidate_project_id
             AND certificate.id = candidate_compilation_id
             AND certificate.task_kind = 'local_goal'
             AND certificate.subject_id = candidate_local_goal_id
             AND certificate.subject_revision = candidate_local_revision
             AND certificate.draft_id = candidate_draft_id
             AND certificate.agent_principal_identity_id = candidate_agent_identity_id
             AND certificate.controller_identity_id = candidate_controller_identity_id
             AND certificate.input_commitment = candidate_prompt_commitment
             AND certificate.ciphertext_commitment = candidate_ciphertext_commitment
             AND certificate.output_hash = candidate_output_hash
             AND certificate.verification_state = 'verified'
       )
    THEN
        RAISE EXCEPTION 'exact compilation certificate required for prompt approval'
            USING ERRCODE = '42501';
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended(candidate_project_id::text, 40));
    INSERT INTO agent_prompt_final_approvals (
        project_id, draft_id, agent_id, controller_identity_id,
        local_goal_id, local_goal_revision, prompt_hash, approval_id,
        idempotency_key, agent_principal_identity_id, signer_device_id,
        signer_device_key_version, prompt_input_commitment,
        ciphertext_commitment, compilation_certificate_id,
        structured_output_hash, approval_identity_hash, approval_hash,
        classical_signature, post_quantum_signature, verification_state
    ) VALUES (
        candidate_project_id, candidate_draft_id, candidate_agent_id,
        candidate_controller_identity_id, candidate_local_goal_id,
        candidate_local_revision, candidate_prompt_hash, candidate_approval_id,
        candidate_idempotency_key, candidate_agent_identity_id,
        candidate_signer_device_id, candidate_signer_key_version,
        candidate_prompt_commitment, candidate_ciphertext_commitment,
        candidate_compilation_id, candidate_output_hash,
        candidate_approval_identity_hash, candidate_approval_hash,
        candidate_classical_signature, candidate_post_quantum_signature, 'verified'
    ) ON CONFLICT (project_id, draft_id) DO NOTHING;
    GET DIAGNOSTICS inserted_rows = ROW_COUNT;
    IF inserted_rows = 0 THEN
        IF NOT EXISTS (
            SELECT 1 FROM agent_prompt_final_approvals approval
            WHERE approval.project_id = candidate_project_id
              AND approval.draft_id = candidate_draft_id
              AND approval.agent_id = candidate_agent_id
              AND approval.controller_identity_id = candidate_controller_identity_id
              AND approval.local_goal_id = candidate_local_goal_id
              AND approval.local_goal_revision = candidate_local_revision
              AND approval.prompt_hash = candidate_prompt_hash
              AND approval.approval_id = candidate_approval_id
              AND approval.idempotency_key = candidate_idempotency_key
              AND approval.agent_principal_identity_id = candidate_agent_identity_id
              AND approval.signer_device_id = candidate_signer_device_id
              AND approval.signer_device_key_version = candidate_signer_key_version
              AND approval.prompt_input_commitment = candidate_prompt_commitment
              AND approval.ciphertext_commitment = candidate_ciphertext_commitment
              AND approval.compilation_certificate_id = candidate_compilation_id
              AND approval.structured_output_hash = candidate_output_hash
              AND approval.approval_identity_hash = candidate_approval_identity_hash
              AND approval.approval_hash = candidate_approval_hash
              AND approval.classical_signature = candidate_classical_signature
              AND approval.post_quantum_signature = candidate_post_quantum_signature
              AND approval.verification_state = 'verified'
        ) THEN
            RAISE EXCEPTION 'final prompt approval replay conflict'
                USING ERRCODE = '23505';
        END IF;
        RETURN;
    END IF;
    INSERT INTO agent_governance_ledger (
        project_id, entry_kind, entry_id, subject_id, subject_revision, entry_hash
    ) VALUES (
        candidate_project_id, 'final_prompt_approval', candidate_approval_id,
        candidate_local_goal_id, candidate_local_revision, candidate_approval_hash
    );
END;
$$;

CREATE FUNCTION sprout_private.append_verified_governance_revision(
    candidate_project_id uuid, candidate_entry_kind text,
    candidate_subject_id uuid, candidate_subject_revision bigint,
    candidate_compilation_id uuid, candidate_contract_hash bytea
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    inserted_rows bigint;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(candidate_project_id::text, 40));
    IF candidate_entry_kind = 'responsibility_revision' THEN
        IF NOT EXISTS (
            SELECT 1 FROM agent_responsibility_contracts responsibility
            WHERE responsibility.project_id = candidate_project_id
              AND responsibility.id = candidate_subject_id
              AND responsibility.revision = candidate_subject_revision
              AND responsibility.compilation_certificate_id = candidate_compilation_id
              AND responsibility.contract_hash = candidate_contract_hash
              AND responsibility.administrator_identity_id = sprout_private.current_identity_id()
        ) THEN
            RAISE EXCEPTION 'exact responsibility revision required'
                USING ERRCODE = '42501';
        END IF;
    ELSIF candidate_entry_kind = 'local_goal_revision' THEN
        IF NOT EXISTS (
            SELECT 1 FROM agent_local_goal_contracts local
            WHERE local.project_id = candidate_project_id
              AND local.id = candidate_subject_id
              AND local.revision = candidate_subject_revision
              AND local.compilation_certificate_id = candidate_compilation_id
              AND local.contract_hash = candidate_contract_hash
              AND local.controller_identity_id = sprout_private.current_identity_id()
        ) THEN
            RAISE EXCEPTION 'exact LocalGoal revision required'
                USING ERRCODE = '42501';
        END IF;
    ELSE
        RAISE EXCEPTION 'unsupported governance ledger entry'
            USING ERRCODE = '22023';
    END IF;
    INSERT INTO agent_governance_ledger (
        project_id, entry_kind, entry_id, subject_id, subject_revision, entry_hash
    ) VALUES (
        candidate_project_id, candidate_entry_kind, candidate_compilation_id,
        candidate_subject_id, candidate_subject_revision, candidate_contract_hash
    ) ON CONFLICT (project_id, entry_kind, entry_id) DO NOTHING;
    GET DIAGNOSTICS inserted_rows = ROW_COUNT;
    IF inserted_rows = 0 AND NOT EXISTS (
        SELECT 1 FROM agent_governance_ledger ledger
        WHERE ledger.project_id = candidate_project_id
          AND ledger.entry_kind = candidate_entry_kind
          AND ledger.entry_id = candidate_compilation_id
          AND ledger.subject_id = candidate_subject_id
          AND ledger.subject_revision = candidate_subject_revision
          AND ledger.entry_hash = candidate_contract_hash
    ) THEN
        RAISE EXCEPTION 'governance revision replay conflict'
            USING ERRCODE = '23505';
    END IF;
END;
$$;

CREATE FUNCTION sprout_private.require_verified_governance_activation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state = 'active' AND (
        NEW.compilation_certificate_id IS NULL
        OR NOT EXISTS (
            SELECT 1 FROM agent_compilation_certificates certificate
            WHERE certificate.project_id = NEW.project_id
              AND certificate.id = NEW.compilation_certificate_id
              AND certificate.subject_id = NEW.id
              AND certificate.subject_revision = NEW.revision
              AND certificate.verification_state = 'verified'
              AND certificate.task_kind = TG_ARGV[0]
        )
    ) THEN
        RAISE EXCEPTION 'active governance revision requires exact verified compilation certificate'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_responsibility_verified_compilation
BEFORE INSERT OR UPDATE OF state ON agent_responsibility_contracts
FOR EACH ROW WHEN (NEW.state = 'active')
EXECUTE FUNCTION sprout_private.require_verified_governance_activation('responsibility');

CREATE TRIGGER agent_local_goal_verified_compilation
BEFORE INSERT OR UPDATE OF state ON agent_local_goal_contracts
FOR EACH ROW WHEN (NEW.state = 'active')
EXECUTE FUNCTION sprout_private.require_verified_governance_activation('local_goal');

CREATE OR REPLACE FUNCTION sprout_private.require_active_prompt_final_approval()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state = 'active' AND NOT EXISTS (
        SELECT 1
        FROM agent_prompt_final_approvals approval
        JOIN governed_agents agent
          ON agent.project_id = approval.project_id
         AND agent.id = approval.agent_id
         AND agent.principal_identity_id = approval.agent_principal_identity_id
        JOIN agent_local_goal_contracts local
          ON local.project_id = approval.project_id
         AND local.agent_id = approval.agent_id
         AND local.id = approval.local_goal_id
         AND local.revision = approval.local_goal_revision
        JOIN agent_compilation_certificates certificate
          ON certificate.project_id = local.project_id
         AND certificate.id = local.compilation_certificate_id
         AND certificate.output_hash = approval.structured_output_hash
        WHERE approval.project_id = NEW.project_id
          AND approval.draft_id = NEW.draft_id
          AND approval.agent_id = NEW.agent_id
          AND approval.controller_identity_id = NEW.approved_by_identity_id
          AND approval.local_goal_id = NEW.local_goal_id
          AND approval.local_goal_revision = NEW.local_goal_revision
          AND approval.prompt_hash = NEW.prompt_hash
          AND approval.ciphertext_commitment = NEW.prompt_hash
          AND approval.verification_state = 'verified'
    ) THEN
        RAISE EXCEPTION 'active prompt requires exact verified final approval certificate'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

ALTER TABLE agent_compilation_certificates ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_compilation_certificates FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_administrator_creation_approvals ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_administrator_creation_approvals FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_governance_ledger ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_governance_ledger FORCE ROW LEVEL SECURITY;

CREATE POLICY agent_compilation_certificate_parties ON agent_compilation_certificates
    USING (
        signer_identity_id = sprout_private.current_identity_id()
        OR controller_identity_id = sprout_private.current_identity_id()
        OR administrator_identity_id = sprout_private.current_identity_id()
        OR EXISTS (
            SELECT 1 FROM project_memberships membership
            WHERE membership.project_id = agent_compilation_certificates.project_id
              AND membership.identity_id = sprout_private.current_identity_id()
              AND membership.state = 'active'
              AND membership.role IN ('owner', 'admin')
        )
    )
    WITH CHECK (signer_identity_id = sprout_private.current_identity_id());

CREATE TRIGGER agent_compilation_certificates_append_only
BEFORE UPDATE OR DELETE ON agent_compilation_certificates
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE POLICY agent_administrator_creation_approval_parties
ON agent_administrator_creation_approvals
FOR SELECT
USING (
    administrator_identity_id = sprout_private.current_identity_id()
    OR EXISTS (
        SELECT 1 FROM project_memberships membership
        WHERE membership.project_id = agent_administrator_creation_approvals.project_id
          AND membership.identity_id = sprout_private.current_identity_id()
          AND membership.state = 'active'
          AND membership.role IN ('owner', 'admin')
    )
);

CREATE POLICY agent_governance_ledger_project_members
ON agent_governance_ledger
FOR SELECT
USING (
    EXISTS (
        SELECT 1 FROM project_memberships membership
        WHERE membership.project_id = agent_governance_ledger.project_id
          AND membership.identity_id = sprout_private.current_identity_id()
          AND membership.state = 'active'
    )
);

CREATE TRIGGER agent_administrator_creation_approvals_append_only
BEFORE UPDATE OR DELETE ON agent_administrator_creation_approvals
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TRIGGER agent_governance_ledger_append_only
BEFORE UPDATE OR DELETE ON agent_governance_ledger
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

REVOKE ALL ON TABLE agent_compiler_builds FROM PUBLIC;
GRANT SELECT ON TABLE agent_compiler_builds TO PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_compilation_certificates FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_prompt_final_approvals FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_administrator_creation_approvals FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_governance_ledger FROM PUBLIC;
REVOKE ALL ON SEQUENCE agent_governance_ledger_position_seq FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_compiler_build_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.require_verified_governance_activation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.insert_verified_compilation_certificate(
    uuid, uuid, text, text, integer, bytea, uuid, uuid, integer, uuid, bigint,
    uuid, uuid, uuid, uuid, uuid, bytea, bytea, jsonb, bytea, jsonb, bytea,
    bytea, uuid, bytea, bytea, integer, bytea, text, uuid, bigint
) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.insert_verified_administrator_creation_approval(
    uuid, uuid, uuid, uuid, integer, uuid, uuid, uuid, uuid, bigint, bytea,
    uuid, bytea, bytea, text, uuid, bytea, uuid, bytea, bytea, bytea
) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.insert_verified_final_prompt_approval(
    uuid, uuid, uuid, uuid, uuid, bigint, bytea, uuid, uuid, uuid, uuid,
    integer, bytea, bytea, uuid, bytea, bytea, bytea, bytea, bytea
) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.append_verified_governance_revision(
    uuid, text, uuid, bigint, uuid, bytea
) FROM PUBLIC;

COMMENT ON TABLE agent_compilation_certificates IS
    'Endpoint-TCB compiler evidence. It authenticates a pinned compiler artifact but does not grant permission or expose E2EE plaintext.';
COMMENT ON COLUMN agent_prompt_final_approvals.verification_state IS
    '0027 backfill rows remain legacy_unverified; only independently signed exact-draft approvals are verified.';

-- Initial creation is one transaction. A normal controller may enter this
-- narrow SECURITY DEFINER path only after the same transaction has persisted a
-- verified local compiler artifact bound to an active Responsibility. Project
-- administrators use project governance directly and never receive a fake
-- Responsibility.
DROP FUNCTION sprout_private.provision_edge_agent(
    uuid, uuid, uuid, uuid, text, bytea, uuid, bytea, integer, text,
    uuid, uuid, bytea, uuid, bytea, timestamptz
);

CREATE FUNCTION sprout_private.provision_edge_agent(
    candidate_project_id uuid,
    candidate_agent_id uuid,
    candidate_principal_identity_id uuid,
    candidate_controller_identity_id uuid,
    candidate_identity_handle text,
    candidate_encrypted_profile bytea,
    candidate_profile_resource_node_id uuid,
    candidate_encrypted_system_prompt bytea,
    candidate_key_epoch integer,
    candidate_availability text,
    candidate_runner_id uuid,
    candidate_device_id uuid,
    candidate_encrypted_device_label bytea,
    candidate_session_id uuid,
    candidate_token_hash bytea,
    candidate_session_expires_at timestamptz,
    candidate_local_goal_id uuid,
    candidate_local_revision bigint,
    candidate_draft_id uuid,
    candidate_compilation_id uuid,
    candidate_authorization_kind text,
    candidate_authorization_id uuid
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    caller_identity_id uuid := sprout_private.current_identity_id();
BEGIN
    IF candidate_controller_identity_id <> caller_identity_id
       OR NOT EXISTS (
            SELECT 1 FROM project_memberships membership
            JOIN identities identity ON identity.id = membership.identity_id
            WHERE membership.project_id = candidate_project_id
              AND membership.identity_id = caller_identity_id
              AND membership.state = 'active'
              AND identity.status = 'active'
              AND identity.principal_kind = 'user'
       )
    THEN
        RAISE EXCEPTION 'active human controller required'
            USING ERRCODE = '42501';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM agent_compilation_certificates certificate
        WHERE certificate.project_id = candidate_project_id
          AND certificate.id = candidate_compilation_id
          AND certificate.task_kind = 'local_goal'
          AND certificate.subject_id = candidate_local_goal_id
          AND certificate.subject_revision = candidate_local_revision
          AND certificate.draft_id = candidate_draft_id
          AND certificate.agent_principal_identity_id = candidate_principal_identity_id
          AND certificate.controller_identity_id = caller_identity_id
          AND certificate.authorization_kind = candidate_authorization_kind
          AND certificate.authorization_id = candidate_authorization_id
          AND certificate.verification_state = 'verified'
    ) THEN
        RAISE EXCEPTION 'exact verified local compilation required'
            USING ERRCODE = '42501';
    END IF;
    IF candidate_authorization_kind = 'responsibility' AND NOT EXISTS (
        SELECT 1
        FROM agent_compilation_certificates certificate
        JOIN agent_responsibility_contracts responsibility
          ON responsibility.project_id = certificate.project_id
         AND responsibility.id = certificate.authorization_id
         AND responsibility.revision = certificate.authorization_revision
         AND responsibility.user_identity_id = caller_identity_id
         AND responsibility.state = 'active'
        WHERE certificate.project_id = candidate_project_id
          AND certificate.task_kind = 'local_goal'
          AND certificate.id = candidate_compilation_id
          AND certificate.agent_principal_identity_id = candidate_principal_identity_id
          AND certificate.controller_identity_id = caller_identity_id
          AND certificate.authorization_kind = 'responsibility'
          AND certificate.verification_state = 'verified'
    ) THEN
        RAISE EXCEPTION 'verified active responsibility certificate required'
            USING ERRCODE = '42501';
    ELSIF candidate_authorization_kind = 'administrator_creation' AND NOT EXISTS (
        SELECT 1
        FROM agent_administrator_creation_approvals approval
        JOIN project_memberships membership
          ON membership.project_id = approval.project_id
         AND membership.identity_id = approval.administrator_identity_id
         AND membership.state = 'active'
         AND membership.role IN ('owner', 'admin')
        WHERE approval.project_id = candidate_project_id
          AND approval.approval_id = candidate_authorization_id
          AND approval.administrator_identity_id = caller_identity_id
          AND approval.proposed_agent_identity_id = candidate_principal_identity_id
          AND approval.governed_agent_id = candidate_agent_id
          AND approval.proposal_draft_id = candidate_draft_id
          AND approval.local_goal_id = candidate_local_goal_id
          AND approval.local_goal_revision = candidate_local_revision
          AND approval.compilation_certificate_id = candidate_compilation_id
          AND approval.availability = candidate_availability
    ) THEN
        RAISE EXCEPTION 'exact administrator creation approval required'
            USING ERRCODE = '42501';
    ELSIF candidate_authorization_kind NOT IN ('responsibility', 'administrator_creation') THEN
        RAISE EXCEPTION 'initial creation authorization adapter is unavailable'
            USING ERRCODE = '42501';
    END IF;
    IF candidate_session_expires_at <= clock_timestamp()
       OR octet_length(candidate_token_hash) < 32
       OR octet_length(candidate_encrypted_system_prompt) = 0
    THEN
        RAISE EXCEPTION 'invalid runner bootstrap session or prompt'
            USING ERRCODE = '23514';
    END IF;

    INSERT INTO identities (
        id, identity_handle, encrypted_profile, principal_kind
    ) VALUES (
        candidate_principal_identity_id, candidate_identity_handle,
        candidate_encrypted_profile, 'agent'
    );
    INSERT INTO project_memberships (project_id, identity_id, role)
    VALUES (candidate_project_id, candidate_principal_identity_id, 'member');
    INSERT INTO devices (
        id, identity_id, device_kind, encrypted_label, trust_state
    ) VALUES (
        candidate_device_id, candidate_principal_identity_id,
        'service', candidate_encrypted_device_label, 'trusted'
    );
    INSERT INTO sessions (
        id, identity_id, device_id, token_hash, expires_at
    ) VALUES (
        candidate_session_id, candidate_principal_identity_id,
        candidate_device_id, candidate_token_hash, candidate_session_expires_at
    );
    INSERT INTO governed_agents (
        id, project_id, principal_identity_id, controller_identity_id,
        profile_resource_node_id, encrypted_system_prompt, key_epoch,
        availability
    ) VALUES (
        candidate_agent_id, candidate_project_id, candidate_principal_identity_id,
        candidate_controller_identity_id, candidate_profile_resource_node_id,
        candidate_encrypted_system_prompt, candidate_key_epoch,
        candidate_availability
    );
    INSERT INTO agent_runners (
        id, project_id, agent_id, principal_identity_id, device_id
    ) VALUES (
        candidate_runner_id, candidate_project_id, candidate_agent_id,
        candidate_principal_identity_id, candidate_device_id
    );
END;
$$;

REVOKE ALL ON FUNCTION sprout_private.provision_edge_agent(
    uuid, uuid, uuid, uuid, text, bytea, uuid, bytea, integer, text,
    uuid, uuid, bytea, uuid, bytea, timestamptz,
    uuid, bigint, uuid, uuid, text, uuid
) FROM PUBLIC;
