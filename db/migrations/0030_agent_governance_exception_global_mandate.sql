-- Certified local-goal exception and existing-agent global-mandate governance.
-- Cryptographic verification remains in the Rust endpoint TCB.  This migration
-- supplies an append-only, exact-binding persistence boundary; it does not
-- create permissions, tool grants, memberships, Responsibilities, or keys.

ALTER TABLE agent_governance_ledger
    DROP CONSTRAINT agent_governance_ledger_entry_kind_check;

ALTER TABLE agent_governance_ledger
    ADD CONSTRAINT agent_governance_ledger_entry_kind_check CHECK (entry_kind IN (
        'compilation', 'final_prompt_approval', 'administrator_creation_approval',
        'responsibility_revision', 'local_goal_revision',
        'local_draft_disposition', 'exception_consent', 'exception_review',
        'exception_admin_draft', 'exception_decision', 'approved_local_exception',
        'global_coverage_need', 'global_mandate_assignment', 'global_agent_proposal'
    ));

CREATE TABLE agent_governance_authorization_events (
    project_id uuid NOT NULL,
    event_id uuid NOT NULL,
    event_kind text NOT NULL CHECK (event_kind IN (
        'local_draft_disposition', 'exception_consent', 'exception_review',
        'exception_admin_draft', 'exception_decision', 'approved_local_exception',
        'global_coverage_need', 'global_mandate_assignment', 'global_agent_proposal'
    )),
    workflow_id uuid NOT NULL,
    workflow_revision bigint NOT NULL DEFAULT 0 CHECK (workflow_revision >= 0),
    actor_identity_id uuid NOT NULL,
    user_identity_id uuid,
    administrator_identity_id uuid,
    agent_id uuid,
    source_draft_id uuid,
    review_task_id uuid,
    local_goal_id uuid,
    local_goal_revision bigint CHECK (local_goal_revision IS NULL OR local_goal_revision > 0),
    global_contract_id uuid,
    global_revision bigint CHECK (global_revision IS NULL OR global_revision > 0),
    obligation_id uuid,
    compilation_certificate_id uuid,
    responsibility_compilation_id uuid,
    idempotency_key uuid NOT NULL,
    event_hash bytea NOT NULL CHECK (octet_length(event_hash) = 32),
    payload jsonb NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
    ledger_position bigint NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, event_id),
    UNIQUE (project_id, event_kind, actor_identity_id, idempotency_key),
    UNIQUE (project_id, event_kind, workflow_id, workflow_revision),
    UNIQUE (project_id, ledger_position),
    CONSTRAINT agent_governance_0030_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_governance_0030_actor_fk
        FOREIGN KEY (project_id, actor_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_governance_0030_user_fk
        FOREIGN KEY (project_id, user_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_governance_0030_administrator_fk
        FOREIGN KEY (project_id, administrator_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_governance_0030_agent_fk
        FOREIGN KEY (project_id, agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_governance_0030_review_task_fk
        FOREIGN KEY (project_id, review_task_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_governance_0030_compilation_fk
        FOREIGN KEY (project_id, compilation_certificate_id)
        REFERENCES agent_compilation_certificates (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT agent_governance_0030_responsibility_compilation_fk
        FOREIGN KEY (project_id, responsibility_compilation_id)
        REFERENCES agent_compilation_certificates (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_governance_0030_ledger_fk
        FOREIGN KEY (project_id, ledger_position)
        REFERENCES agent_governance_ledger (project_id, position)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_governance_0030_shape CHECK (
        (event_kind = 'local_draft_disposition'
            AND source_draft_id IS NOT NULL AND agent_id IS NOT NULL)
        OR (event_kind = 'exception_consent'
            AND source_draft_id IS NOT NULL AND user_identity_id IS NOT NULL)
        OR (event_kind = 'exception_review'
            AND source_draft_id IS NOT NULL AND review_task_id IS NOT NULL
            AND user_identity_id IS NOT NULL AND administrator_identity_id IS NOT NULL
            AND agent_id IS NOT NULL)
        OR (event_kind = 'exception_admin_draft'
            AND administrator_identity_id IS NOT NULL AND agent_id IS NOT NULL
            AND local_goal_id IS NOT NULL AND local_goal_revision IS NOT NULL)
        OR (event_kind = 'exception_decision'
            AND administrator_identity_id IS NOT NULL AND agent_id IS NOT NULL)
        OR (event_kind = 'approved_local_exception'
            AND administrator_identity_id IS NOT NULL AND user_identity_id IS NOT NULL
            AND agent_id IS NOT NULL AND local_goal_id IS NOT NULL
            AND local_goal_revision IS NOT NULL AND compilation_certificate_id IS NOT NULL)
        OR (event_kind = 'global_coverage_need'
            AND administrator_identity_id IS NOT NULL AND global_contract_id IS NOT NULL
            AND global_revision IS NOT NULL AND obligation_id IS NOT NULL)
        OR (event_kind = 'global_mandate_assignment'
            AND administrator_identity_id IS NOT NULL AND agent_id IS NOT NULL
            AND local_goal_id IS NOT NULL AND local_goal_revision IS NOT NULL
            AND global_contract_id IS NOT NULL AND global_revision IS NOT NULL
            AND obligation_id IS NOT NULL AND compilation_certificate_id IS NOT NULL)
        OR (event_kind = 'global_agent_proposal'
            AND administrator_identity_id IS NOT NULL AND global_contract_id IS NOT NULL
            AND global_revision IS NOT NULL AND obligation_id IS NOT NULL)
    )
);

CREATE UNIQUE INDEX agent_governance_0030_one_terminal_decision
    ON agent_governance_authorization_events (project_id, workflow_id)
    WHERE event_kind = 'exception_decision';
CREATE UNIQUE INDEX agent_governance_0030_one_approved_exception
    ON agent_governance_authorization_events (project_id, workflow_id)
    WHERE event_kind = 'approved_local_exception';

CREATE FUNCTION sprout_private.insert_agent_governance_authorization_event(
    candidate_project_id uuid, candidate_event_id uuid, candidate_event_kind text,
    candidate_workflow_id uuid, candidate_workflow_revision bigint,
    candidate_actor_identity_id uuid, candidate_user_identity_id uuid,
    candidate_administrator_identity_id uuid, candidate_agent_id uuid,
    candidate_source_draft_id uuid, candidate_review_task_id uuid,
    candidate_local_goal_id uuid, candidate_local_goal_revision bigint,
    candidate_global_contract_id uuid, candidate_global_revision bigint,
    candidate_obligation_id uuid, candidate_compilation_certificate_id uuid,
    candidate_responsibility_compilation_id uuid, candidate_idempotency_key uuid,
    candidate_event_hash bytea, candidate_payload jsonb
)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    existing_event_id uuid;
    existing_hash bytea;
    inserted_position bigint;
BEGIN
    IF candidate_event_kind NOT IN (
        'local_draft_disposition', 'exception_consent', 'exception_review',
        'exception_admin_draft', 'exception_decision', 'approved_local_exception',
        'global_coverage_need', 'global_mandate_assignment', 'global_agent_proposal'
    ) OR octet_length(candidate_event_hash) <> 32
       OR jsonb_typeof(candidate_payload) <> 'object'
    THEN
        RAISE EXCEPTION 'invalid governance authorization event'
            USING ERRCODE = '23514';
    END IF;

    PERFORM pg_advisory_xact_lock(hashtextextended(
        candidate_project_id::text || ':' || candidate_event_kind || ':' ||
        candidate_workflow_id::text || ':' || candidate_workflow_revision::text, 43));

    SELECT event_id, event_hash INTO existing_event_id, existing_hash
    FROM agent_governance_authorization_events
    WHERE project_id = candidate_project_id
      AND event_kind = candidate_event_kind
      AND workflow_id = candidate_workflow_id
      AND workflow_revision = candidate_workflow_revision;
    IF FOUND THEN
        IF existing_event_id = candidate_event_id
           AND existing_hash = candidate_event_hash
        THEN
            RETURN (
                SELECT ledger_position FROM agent_governance_authorization_events
                WHERE project_id = candidate_project_id AND event_id = existing_event_id
            );
        END IF;
        RAISE EXCEPTION 'governance event equivocation'
            USING ERRCODE = '40001';
    END IF;

    SELECT event_id, event_hash INTO existing_event_id, existing_hash
    FROM agent_governance_authorization_events
    WHERE project_id = candidate_project_id
      AND event_kind = candidate_event_kind
      AND actor_identity_id = candidate_actor_identity_id
      AND idempotency_key = candidate_idempotency_key;
    IF FOUND THEN
        IF existing_event_id = candidate_event_id
           AND existing_hash = candidate_event_hash
        THEN
            RETURN (
                SELECT ledger_position FROM agent_governance_authorization_events
                WHERE project_id = candidate_project_id AND event_id = existing_event_id
            );
        END IF;
        RAISE EXCEPTION 'governance event idempotency conflict'
            USING ERRCODE = '40001';
    END IF;

    INSERT INTO agent_governance_ledger (
        project_id, entry_kind, entry_id, subject_id, subject_revision, entry_hash
    ) VALUES (
        candidate_project_id, candidate_event_kind, candidate_event_id,
        candidate_workflow_id, GREATEST(candidate_workflow_revision, 1),
        candidate_event_hash
    ) RETURNING position INTO inserted_position;

    INSERT INTO agent_governance_authorization_events (
        project_id, event_id, event_kind, workflow_id, workflow_revision,
        actor_identity_id, user_identity_id, administrator_identity_id, agent_id,
        source_draft_id, review_task_id, local_goal_id, local_goal_revision,
        global_contract_id, global_revision, obligation_id,
        compilation_certificate_id, responsibility_compilation_id,
        idempotency_key, event_hash, payload, ledger_position
    ) VALUES (
        candidate_project_id, candidate_event_id, candidate_event_kind,
        candidate_workflow_id, candidate_workflow_revision,
        candidate_actor_identity_id, candidate_user_identity_id,
        candidate_administrator_identity_id, candidate_agent_id,
        candidate_source_draft_id, candidate_review_task_id,
        candidate_local_goal_id, candidate_local_goal_revision,
        candidate_global_contract_id, candidate_global_revision,
        candidate_obligation_id, candidate_compilation_certificate_id,
        candidate_responsibility_compilation_id, candidate_idempotency_key,
        candidate_event_hash, candidate_payload, inserted_position
    );
    RETURN inserted_position;
END;
$$;

-- Permit an administrator to relay a controller-signed GlobalMandate
-- compilation without pretending that the administrator signed it.  The
-- endpoint Rust TCB verifies both hybrid signatures and the exact mandate
-- before calling this private function.  All other compilation writers retain
-- the 0029 signer=current-caller requirement.
CREATE OR REPLACE FUNCTION sprout_private.insert_verified_compilation_certificate(
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
    caller_identity_id uuid := sprout_private.current_identity_id();
BEGIN
    IF NOT EXISTS (
           SELECT 1 FROM agent_compiler_builds build
           WHERE build.task_kind = candidate_task_kind
             AND build.compiler_name = candidate_compiler_id
             AND build.compiler_version = candidate_compiler_version
             AND build.build_digest = candidate_build_digest
             AND build.enabled
       ) OR NOT (
           candidate_signer_identity_id = caller_identity_id
           OR (
               candidate_task_kind = 'local_goal'
               AND candidate_authorization_kind = 'global_mandate'
               AND candidate_signer_identity_id = candidate_controller_identity_id
               AND EXISTS (
                   SELECT 1
                   FROM governed_agents agent
                   JOIN project_memberships administrator
                     ON administrator.project_id = agent.project_id
                    AND administrator.identity_id = caller_identity_id
                    AND administrator.state = 'active'
                    AND administrator.role IN ('owner', 'admin')
                   WHERE agent.project_id = candidate_project_id
                     AND agent.principal_identity_id = candidate_agent_identity_id
                     AND agent.controller_identity_id = candidate_signer_identity_id
                     AND agent.state = 'active'
                     AND agent.availability = 'project_delegable'
               )
           )
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

-- 0029 required the current caller to be the LocalGoal controller.  The
-- certified exception workflow is the one revision path where the exact
-- administrator review draft is compiled by its administrator.  Preserve the
-- controller rule and add only the causal, exact exception-draft alternative;
-- project administration alone is deliberately insufficient.
CREATE OR REPLACE FUNCTION sprout_private.append_verified_governance_revision(
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
            SELECT 1
            FROM agent_local_goal_contracts local
            JOIN agent_compilation_certificates certificate
              ON certificate.project_id = local.project_id
             AND certificate.id = local.compilation_certificate_id
            WHERE local.project_id = candidate_project_id
              AND local.id = candidate_subject_id
              AND local.revision = candidate_subject_revision
              AND local.compilation_certificate_id = candidate_compilation_id
              AND local.contract_hash = candidate_contract_hash
              AND (
                  local.controller_identity_id = sprout_private.current_identity_id()
                  OR (
                      certificate.authorization_kind = 'administrator_exception'
                      AND certificate.signer_identity_id = sprout_private.current_identity_id()
                      AND EXISTS (
                          SELECT 1
                          FROM agent_governance_authorization_events review_draft
                          WHERE review_draft.project_id = local.project_id
                            AND review_draft.event_kind = 'exception_admin_draft'
                            AND review_draft.workflow_id = certificate.authorization_id
                            AND review_draft.workflow_revision = certificate.authorization_revision
                            AND review_draft.administrator_identity_id = certificate.signer_identity_id
                            AND review_draft.local_goal_id = local.id
                            AND review_draft.local_goal_revision = local.revision
                            AND review_draft.payload #>> '{local_compilation,statement,certificate_id}'
                                = certificate.id::text
                      )
                  )
                  OR (
                      certificate.authorization_kind = 'global_mandate'
                      AND certificate.signer_identity_id = local.controller_identity_id
                      AND EXISTS (
                          SELECT 1
                          FROM agent_governance_authorization_events assignment
                          JOIN agent_governance_authorization_events need
                            ON need.project_id = assignment.project_id
                           AND need.event_id = (assignment.payload->>'need_id')::uuid
                           AND need.event_kind = 'global_coverage_need'
                           AND need.global_contract_id = assignment.global_contract_id
                           AND need.global_revision = assignment.global_revision
                           AND need.obligation_id = assignment.obligation_id
                           AND need.payload->'need' = assignment.payload #> '{assignment,need}'
                          JOIN governed_agents agent
                            ON agent.project_id = assignment.project_id
                           AND agent.id = assignment.agent_id
                           AND agent.principal_identity_id = local.agent_identity_id
                           AND agent.controller_identity_id = certificate.signer_identity_id
                          JOIN project_memberships administrator
                            ON administrator.project_id = agent.project_id
                           AND administrator.identity_id = sprout_private.current_identity_id()
                           AND administrator.state = 'active'
                           AND administrator.role IN ('owner', 'admin')
                          WHERE assignment.project_id = local.project_id
                            AND assignment.event_kind = 'global_mandate_assignment'
                            AND assignment.event_id = certificate.authorization_id
                            AND assignment.workflow_id = certificate.authorization_id
                            AND assignment.workflow_revision = certificate.authorization_revision
                            AND assignment.global_revision = certificate.authorization_revision
                            AND assignment.agent_id = agent.id
                            AND assignment.local_goal_id = local.id
                            AND assignment.local_goal_revision = local.revision
                            AND assignment.compilation_certificate_id = certificate.id
                            AND assignment.source_draft_id = certificate.draft_id
                            AND assignment.administrator_identity_id = administrator.identity_id
                            AND assignment.payload #>> '{assignment,assigned_by}'
                                = administrator.identity_id::text
                            AND assignment.payload #> '{assignment,local}' = local.contract
                            AND local.contract #>> '{origin,kind}' = 'global_mandate'
                            AND (local.contract #>> '{origin,global_revision}')::bigint
                                = certificate.authorization_revision
                            AND agent.state = 'active'
                            AND agent.availability = 'project_delegable'
                      )
                  )
              )
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

ALTER TABLE agent_governance_authorization_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_governance_authorization_events FORCE ROW LEVEL SECURITY;

CREATE POLICY agent_governance_0030_parties
ON agent_governance_authorization_events
FOR SELECT
USING (
    actor_identity_id = sprout_private.current_identity_id()
    OR user_identity_id = sprout_private.current_identity_id()
    OR administrator_identity_id = sprout_private.current_identity_id()
    OR EXISTS (
        SELECT 1 FROM governed_agents agent
        WHERE agent.project_id = agent_governance_authorization_events.project_id
          AND agent.id = agent_governance_authorization_events.agent_id
          AND (agent.controller_identity_id = sprout_private.current_identity_id()
               OR agent.principal_identity_id = sprout_private.current_identity_id())
    )
);

CREATE TRIGGER agent_governance_authorization_events_append_only
BEFORE UPDATE OR DELETE ON agent_governance_authorization_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

REVOKE INSERT, UPDATE, DELETE ON agent_governance_authorization_events FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.insert_agent_governance_authorization_event(
    uuid, uuid, text, uuid, bigint, uuid, uuid, uuid, uuid, uuid, uuid,
    uuid, bigint, uuid, bigint, uuid, uuid, uuid, uuid, bytea, jsonb
) FROM PUBLIC;

COMMENT ON TABLE agent_governance_authorization_events IS
    'Append-only 0030 governance evidence. Rows are verified by the Rust TCB before the private writer is invoked; they do not grant product authority.';
