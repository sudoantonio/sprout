-- Provider-neutral, state-grounded language runtime witnesses. Plaintext and
-- provider state remain inside the authorized endpoint TCB. These tables keep
-- only structural coordinates, canonical commitments and signed observations;
-- they are not a model-memory store.

ALTER TABLE agent_invocations
    ADD COLUMN context_principal_identity_id uuid,
    ADD COLUMN invocation_surface text NOT NULL DEFAULT 'generic'
        CHECK (invocation_surface IN (
            'generic', 'user_proxy', 'interrogation', 'governance_summary'
        )),
    ADD COLUMN proxy_request_id uuid,
    ADD COLUMN interrogation_id uuid,
    ADD COLUMN trace_id uuid,
    ADD COLUMN run_id uuid,
    ADD COLUMN goal_id uuid,
    ADD COLUMN work_item_id uuid,
    ADD COLUMN work_claim_id uuid,
    ADD COLUMN work_attempt integer CHECK (work_attempt IS NULL OR work_attempt > 0),
    ADD COLUMN context_hash bytea CHECK (
        context_hash IS NULL OR octet_length(context_hash) = 32
    ),
    ADD CONSTRAINT agent_invocations_context_principal_fk
        FOREIGN KEY (project_id, context_principal_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT agent_invocations_proxy_request_fk
        FOREIGN KEY (project_id, proxy_request_id)
        REFERENCES user_proxy_requests (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT agent_invocations_interrogation_fk
        FOREIGN KEY (interrogation_id) REFERENCES agent_interrogations (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT agent_invocations_work_shape CHECK (
        (trace_id IS NULL AND run_id IS NULL AND goal_id IS NULL
         AND work_item_id IS NULL AND work_claim_id IS NULL AND work_attempt IS NULL)
        OR
        (trace_id IS NOT NULL AND run_id IS NOT NULL AND goal_id IS NOT NULL
         AND work_item_id IS NOT NULL AND work_claim_id IS NOT NULL AND work_attempt IS NOT NULL)
    ),
    ADD CONSTRAINT agent_invocations_surface_shape CHECK (
        (invocation_surface = 'generic'
         AND proxy_request_id IS NULL AND interrogation_id IS NULL)
        OR (invocation_surface = 'user_proxy'
            AND proxy_request_id IS NOT NULL AND interrogation_id IS NULL)
        OR (invocation_surface = 'interrogation'
            AND proxy_request_id IS NULL AND interrogation_id IS NOT NULL)
        OR (invocation_surface = 'governance_summary'
            AND proxy_request_id IS NULL AND interrogation_id IS NULL)
    );

UPDATE agent_invocations
SET context_principal_identity_id = agent_identity_id
WHERE context_principal_identity_id IS NULL;

-- Legacy/runtime fixtures created before 0031 have no independent context
-- principal witness. They remain readable but cannot enter an exact 0031
-- projection. Every new API writer supplies this column explicitly.

CREATE TABLE agent_model_attempt_dispatches (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    lease_id uuid NOT NULL,
    runner_id uuid NOT NULL,
    runner_identity_id uuid NOT NULL,
    runner_device_id uuid NOT NULL,
    runner_key_version integer NOT NULL CHECK (runner_key_version > 0),
    context_principal_identity_id uuid NOT NULL,
    request_commitment bytea NOT NULL CHECK (octet_length(request_commitment) = 32),
    context_commitment bytea NOT NULL CHECK (octet_length(context_commitment) = 32),
    exposure_commitment bytea NOT NULL CHECK (octet_length(exposure_commitment) = 32),
    transport_commitment bytea NOT NULL CHECK (octet_length(transport_commitment) = 32),
    source_descriptors jsonb NOT NULL CHECK (jsonb_typeof(source_descriptors) = 'array'),
    dispatched_at timestamptz NOT NULL,
    lease_expires_at timestamptz NOT NULL,
    CONSTRAINT agent_model_dispatch_time_shape CHECK (lease_expires_at > dispatched_at),
    CONSTRAINT agent_model_dispatch_attempt_unique
        UNIQUE (project_id, invocation_id, attempt),
    CONSTRAINT agent_model_dispatch_lease_unique
        UNIQUE (project_id, lease_id),
    CONSTRAINT agent_model_dispatch_transport_unique
        UNIQUE (project_id, transport_commitment)
);

CREATE TABLE agent_model_attempt_observations (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    dispatch_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    lease_id uuid NOT NULL,
    principal_identity_id uuid NOT NULL,
    status text NOT NULL CHECK (status IN ('succeeded', 'explicit_failure')),
    provider_status text NOT NULL CHECK (char_length(provider_status) BETWEEN 1 AND 128),
    request_commitment bytea NOT NULL CHECK (octet_length(request_commitment) = 32),
    context_commitment bytea NOT NULL CHECK (octet_length(context_commitment) = 32),
    exposure_commitment bytea NOT NULL CHECK (octet_length(exposure_commitment) = 32),
    output_commitment bytea CHECK (
        output_commitment IS NULL OR octet_length(output_commitment) = 32
    ),
    artifact_commitment bytea CHECK (
        artifact_commitment IS NULL OR octet_length(artifact_commitment) = 32
    ),
    structured_artifact jsonb CHECK (
        structured_artifact IS NULL OR jsonb_typeof(structured_artifact) = 'object'
    ),
    transport_commitment bytea NOT NULL CHECK (octet_length(transport_commitment) = 32),
    exposed_source_descriptors jsonb NOT NULL
        CHECK (jsonb_typeof(exposed_source_descriptors) = 'array'),
    hidden_persistent_model_memory_available boolean NOT NULL DEFAULT false
        CHECK (NOT hidden_persistent_model_memory_available),
    signer_identity_id uuid NOT NULL,
    signer_device_id uuid NOT NULL,
    signer_key_version integer NOT NULL CHECK (signer_key_version > 0),
    classical_signature bytea NOT NULL CHECK (octet_length(classical_signature) = 64),
    post_quantum_signature bytea NOT NULL CHECK (octet_length(post_quantum_signature) > 0),
    observation_hash bytea NOT NULL CHECK (octet_length(observation_hash) = 32),
    idempotency_key uuid NOT NULL,
    observed_at timestamptz NOT NULL,
    CONSTRAINT agent_model_observation_terminal_shape CHECK (
        (status = 'succeeded' AND output_commitment IS NOT NULL
         AND artifact_commitment IS NOT NULL AND structured_artifact IS NOT NULL)
        OR (status = 'explicit_failure' AND output_commitment IS NULL
            AND artifact_commitment IS NULL AND structured_artifact IS NULL)
    ),
    CONSTRAINT agent_model_observation_dispatch_unique UNIQUE (project_id, dispatch_id),
    CONSTRAINT agent_model_observation_attempt_unique
        UNIQUE (project_id, invocation_id, attempt),
    CONSTRAINT agent_model_observation_idempotency_unique
        UNIQUE (project_id, invocation_id, idempotency_key),
    CONSTRAINT agent_model_observation_hash_unique UNIQUE (project_id, observation_hash)
);

CREATE TABLE agent_model_invocation_projections (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    observation_id uuid NOT NULL,
    provider_attempt integer NOT NULL CHECK (provider_attempt > 0),
    trace_id uuid,
    run_id uuid,
    goal_id uuid,
    work_item_id uuid,
    work_claim_id uuid,
    work_attempt integer CHECK (work_attempt IS NULL OR work_attempt > 0),
    principal_identity_id uuid NOT NULL,
    status text NOT NULL CHECK (status IN ('succeeded', 'explicit_failure')),
    invocation_surface text NOT NULL CHECK (invocation_surface IN (
        'generic', 'user_proxy', 'interrogation', 'governance_summary'
    )),
    language_task jsonb NOT NULL CHECK (jsonb_typeof(language_task) = 'object'),
    context_source_descriptors jsonb NOT NULL
        CHECK (jsonb_typeof(context_source_descriptors) = 'array'),
    request_commitment bytea NOT NULL CHECK (octet_length(request_commitment) = 32),
    context_commitment bytea NOT NULL CHECK (octet_length(context_commitment) = 32),
    output_commitment bytea CHECK (
        output_commitment IS NULL OR octet_length(output_commitment) = 32
    ),
    artifact_commitment bytea CHECK (
        artifact_commitment IS NULL OR octet_length(artifact_commitment) = 32
    ),
    structured_artifact jsonb CHECK (
        structured_artifact IS NULL OR jsonb_typeof(structured_artifact) = 'object'
    ),
    invoked_at timestamptz NOT NULL,
    projected_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_model_projection_work_shape CHECK (
        (trace_id IS NULL AND run_id IS NULL AND goal_id IS NULL
         AND work_item_id IS NULL AND work_claim_id IS NULL AND work_attempt IS NULL)
        OR
        (trace_id IS NOT NULL AND run_id IS NOT NULL AND goal_id IS NOT NULL
         AND work_item_id IS NOT NULL AND work_claim_id IS NOT NULL AND work_attempt IS NOT NULL)
    ),
    CONSTRAINT agent_model_projection_terminal_shape CHECK (
        (status = 'succeeded' AND output_commitment IS NOT NULL
         AND artifact_commitment IS NOT NULL AND structured_artifact IS NOT NULL)
        OR (status = 'explicit_failure' AND output_commitment IS NULL
            AND artifact_commitment IS NULL AND structured_artifact IS NULL)
    ),
    CONSTRAINT agent_model_projection_invocation_attempt_unique
        UNIQUE (project_id, invocation_id, provider_attempt),
    CONSTRAINT agent_model_projection_observation_unique UNIQUE (project_id, observation_id)
);

CREATE TABLE agent_interrogation_answers (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    interrogation_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    encrypted_answer bytea NOT NULL CHECK (octet_length(encrypted_answer) > 0),
    context_source_descriptors jsonb NOT NULL
        CHECK (jsonb_typeof(context_source_descriptors) = 'array'),
    question_state_fingerprint bytea NOT NULL
        CHECK (octet_length(question_state_fingerprint) = 32),
    answer_state_fingerprint bytea NOT NULL
        CHECK (octet_length(answer_state_fingerprint) = 32),
    answered_at timestamptz NOT NULL,
    CONSTRAINT agent_interrogation_answers_session_fk
        FOREIGN KEY (interrogation_id) REFERENCES agent_interrogations (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_interrogation_answers_session_unique
        UNIQUE (project_id, interrogation_id),
    CONSTRAINT agent_interrogation_answers_invocation_unique
        UNIQUE (project_id, invocation_id)
);

ALTER TABLE agent_effect_proposals
    ADD CONSTRAINT agent_effects_project_invocation_id_unique
    UNIQUE (project_id, invocation_id, id);

-- Canonical cross-subsystem causal ledger for authoritative mutations that a
-- language invocation actually causes. Product tables remain authoritative
-- for the mutation itself; this ledger is the exact invocation edge. A
-- category absent from all runtime writers cannot be inferred from time,
-- actor, or project proximity.
CREATE TABLE agent_language_causal_mutations (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    category text NOT NULL CHECK (category IN (
        'resource_effect', 'tool_invocation', 'prompt_revision',
        'local_goal_revision', 'created_work', 'activated_obligation',
        'assigned_task'
    )),
    record_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_language_causal_mutation_invocation_fk
        FOREIGN KEY (project_id, invocation_id)
        REFERENCES agent_invocations (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_language_causal_mutation_effect_fk
        FOREIGN KEY (project_id, invocation_id, record_id)
        REFERENCES agent_effect_proposals (project_id, invocation_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_language_causal_mutation_exact_unique
        UNIQUE (project_id, invocation_id, category, record_id)
);

CREATE FUNCTION sprout_private.agent_language_retention_delete_allowed(
    candidate_project_id uuid,
    candidate_invocation_id uuid,
    candidate_interrogation_id uuid,
    candidate_proxy_request_id uuid
)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
    SELECT CASE
        WHEN candidate_interrogation_id IS NOT NULL THEN
            sprout_private.agent_retention_row_marked(
                'app.agent_retention_interrogation_ids',
                candidate_interrogation_id,
                candidate_project_id
            )
        WHEN candidate_proxy_request_id IS NOT NULL THEN
            sprout_private.agent_retention_row_marked(
                'app.agent_retention_proxy_request_ids',
                candidate_proxy_request_id,
                candidate_project_id
            )
        WHEN candidate_invocation_id IS NOT NULL THEN EXISTS (
            SELECT 1
            FROM public.agent_invocations invocation
            JOIN public.governed_agents agent
              ON agent.project_id = invocation.project_id
             AND agent.id = invocation.agent_id
            WHERE invocation.project_id = candidate_project_id
              AND invocation.id = candidate_invocation_id
              AND (
                  (invocation.interrogation_id IS NOT NULL
                   AND sprout_private.agent_retention_row_marked(
                       'app.agent_retention_interrogation_ids',
                       invocation.interrogation_id,
                       candidate_project_id
                   ))
                  OR
                  (invocation.proxy_request_id IS NOT NULL
                   AND sprout_private.agent_retention_row_marked(
                       'app.agent_retention_proxy_request_ids',
                       invocation.proxy_request_id,
                       candidate_project_id
                   ))
                  OR
                  (sprout_private.retention_purge_row_allowed(jsonb_build_object(
                       'project_id', candidate_project_id,
                       'resource_node_id', NULLIF(current_setting(
                           'app.agent_retention_resource_id', true
                       ), '')::uuid
                   ))
                   AND (
                       agent.profile_resource_node_id = NULLIF(current_setting(
                           'app.agent_retention_resource_id', true
                       ), '')::uuid
                       OR EXISTS (
                           SELECT 1 FROM public.agent_invocation_sources source
                           WHERE source.project_id = invocation.project_id
                             AND source.invocation_id = invocation.id
                             AND source.resource_node_id = NULLIF(current_setting(
                                 'app.agent_retention_resource_id', true
                             ), '')::uuid
                       )
                       OR EXISTS (
                           SELECT 1 FROM public.agent_effect_proposals effect
                           WHERE effect.project_id = invocation.project_id
                             AND effect.invocation_id = invocation.id
                             AND effect.effect ->> 'resource_id' = current_setting(
                                 'app.agent_retention_resource_id', true
                             )
                       )
                   ))
              )
        )
        ELSE false
    END
$$;

CREATE FUNCTION sprout_private.purge_agent_language_for_invocation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF TG_OP <> 'DELETE'
       OR NOT sprout_private.agent_language_retention_delete_allowed(
           OLD.project_id, OLD.id, OLD.interrogation_id, OLD.proxy_request_id
       )
    THEN
        RETURN OLD;
    END IF;
    DELETE FROM public.agent_model_invocation_projections
    WHERE project_id = OLD.project_id AND invocation_id = OLD.id;
    DELETE FROM public.agent_model_attempt_observations
    WHERE project_id = OLD.project_id AND invocation_id = OLD.id;
    DELETE FROM public.agent_model_attempt_dispatches
    WHERE project_id = OLD.project_id AND invocation_id = OLD.id;
    DELETE FROM public.agent_language_causal_mutations
    WHERE project_id = OLD.project_id AND invocation_id = OLD.id;
    RETURN OLD;
END;
$$;

CREATE FUNCTION sprout_private.purge_agent_language_for_effect()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF TG_OP <> 'DELETE'
       OR NOT sprout_private.agent_language_retention_delete_allowed(
           OLD.project_id, OLD.invocation_id, NULL, NULL
       )
    THEN
        RETURN OLD;
    END IF;
    DELETE FROM public.agent_language_causal_mutations
    WHERE project_id = OLD.project_id
      AND invocation_id = OLD.invocation_id
      AND category = 'resource_effect'
      AND record_id = OLD.id;
    RETURN OLD;
END;
$$;

CREATE FUNCTION sprout_private.reject_agent_language_history_mutation()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    row_data jsonb := to_jsonb(OLD);
BEGIN
    IF TG_OP = 'DELETE'
       AND sprout_private.agent_language_retention_delete_allowed(
           NULLIF(row_data ->> 'project_id', '')::uuid,
           NULLIF(row_data ->> 'invocation_id', '')::uuid,
           NULLIF(row_data ->> 'interrogation_id', '')::uuid,
           NULLIF(row_data ->> 'proxy_request_id', '')::uuid
       )
    THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'agent language runtime history is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_language_causal_mutation()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF NEW.category <> 'resource_effect' THEN
        RAISE EXCEPTION 'language runtime has no grounded writer for causal category %',
            NEW.category USING ERRCODE = '0A000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.purge_agent_language_for_proxy_request()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
DECLARE
    invocation_ids uuid[];
BEGIN
    IF TG_OP <> 'DELETE'
       OR NOT sprout_private.agent_retention_row_marked(
           'app.agent_retention_proxy_request_ids', OLD.id, OLD.project_id
       )
    THEN
        RETURN OLD;
    END IF;

    SELECT COALESCE(array_agg(invocation.id), ARRAY[]::uuid[])
    INTO invocation_ids
    FROM public.agent_invocations invocation
    WHERE invocation.project_id = OLD.project_id
      AND invocation.proxy_request_id = OLD.id;

    DELETE FROM public.agent_model_invocation_projections
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_model_attempt_observations
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_model_attempt_dispatches
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_language_causal_mutations
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_effect_proposals
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_invocation_sources
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_invocations
    WHERE project_id = OLD.project_id AND id = ANY(invocation_ids);
    RETURN OLD;
END;
$$;

-- The 0024 retention workflow deletes an interrogation before its agent
-- invocations. Clear only the exact language-runtime descendants while the
-- authenticated retention lease and exact interrogation marker are live.
CREATE FUNCTION sprout_private.purge_agent_language_for_interrogation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
DECLARE
    invocation_ids uuid[];
BEGIN
    IF TG_OP <> 'DELETE'
       OR NOT sprout_private.agent_retention_row_marked(
           'app.agent_retention_interrogation_ids', OLD.id, OLD.project_id
       )
    THEN
        RETURN OLD;
    END IF;

    SELECT COALESCE(array_agg(invocation.id), ARRAY[]::uuid[])
    INTO invocation_ids
    FROM public.agent_invocations invocation
    WHERE invocation.project_id = OLD.project_id
      AND invocation.interrogation_id = OLD.id;

    DELETE FROM public.agent_interrogation_answers
    WHERE project_id = OLD.project_id AND interrogation_id = OLD.id;
    DELETE FROM public.agent_model_invocation_projections
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_model_attempt_observations
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_model_attempt_dispatches
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_language_causal_mutations
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_effect_proposals
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_invocation_sources
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM public.agent_invocations
    WHERE project_id = OLD.project_id AND id = ANY(invocation_ids);
    RETURN OLD;
END;
$$;

CREATE TRIGGER agent_model_dispatches_append_only
BEFORE UPDATE OR DELETE ON agent_model_attempt_dispatches
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_language_history_mutation();
CREATE TRIGGER agent_model_observations_append_only
BEFORE UPDATE OR DELETE ON agent_model_attempt_observations
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_language_history_mutation();
CREATE TRIGGER agent_model_projections_append_only
BEFORE UPDATE OR DELETE ON agent_model_invocation_projections
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_language_history_mutation();
CREATE TRIGGER agent_interrogation_answers_append_only
BEFORE UPDATE OR DELETE ON agent_interrogation_answers
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_language_history_mutation();
CREATE TRIGGER agent_language_causal_mutations_append_only
BEFORE UPDATE OR DELETE ON agent_language_causal_mutations
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_language_history_mutation();
CREATE TRIGGER agent_language_causal_mutations_grounded
BEFORE INSERT ON agent_language_causal_mutations
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_language_causal_mutation();
CREATE TRIGGER aa_agent_interrogation_language_retention
BEFORE DELETE ON agent_interrogations
FOR EACH ROW EXECUTE FUNCTION sprout_private.purge_agent_language_for_interrogation();
CREATE TRIGGER aa_agent_proxy_language_retention
BEFORE DELETE ON user_proxy_requests
FOR EACH ROW EXECUTE FUNCTION sprout_private.purge_agent_language_for_proxy_request();
CREATE TRIGGER aa_agent_invocation_language_retention
BEFORE DELETE ON agent_invocations
FOR EACH ROW EXECUTE FUNCTION sprout_private.purge_agent_language_for_invocation();
CREATE TRIGGER aa_agent_effect_language_retention
BEFORE DELETE ON agent_effect_proposals
FOR EACH ROW EXECUTE FUNCTION sprout_private.purge_agent_language_for_effect();

ALTER TABLE agent_model_attempt_dispatches ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_model_attempt_dispatches FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_model_attempt_observations ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_model_attempt_observations FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_model_invocation_projections ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_model_invocation_projections FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_interrogation_answers ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_interrogation_answers FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_language_causal_mutations ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_language_causal_mutations FORCE ROW LEVEL SECURITY;

CREATE POLICY agent_model_dispatch_party_isolation ON agent_model_attempt_dispatches
    USING (EXISTS (
        SELECT 1 FROM agent_invocations invocation
        WHERE invocation.project_id = agent_model_attempt_dispatches.project_id
          AND invocation.id = agent_model_attempt_dispatches.invocation_id
          AND sprout_private.agent_party_access(invocation.project_id, invocation.agent_id)
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM agent_invocations invocation
        WHERE invocation.project_id = agent_model_attempt_dispatches.project_id
          AND invocation.id = agent_model_attempt_dispatches.invocation_id
          AND sprout_private.agent_party_access(invocation.project_id, invocation.agent_id)
    ));
CREATE POLICY agent_model_observation_party_isolation ON agent_model_attempt_observations
    USING (EXISTS (
        SELECT 1 FROM agent_invocations invocation
        WHERE invocation.project_id = agent_model_attempt_observations.project_id
          AND invocation.id = agent_model_attempt_observations.invocation_id
          AND sprout_private.agent_party_access(invocation.project_id, invocation.agent_id)
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM agent_invocations invocation
        WHERE invocation.project_id = agent_model_attempt_observations.project_id
          AND invocation.id = agent_model_attempt_observations.invocation_id
          AND sprout_private.agent_party_access(invocation.project_id, invocation.agent_id)
    ));
CREATE POLICY agent_model_projection_party_isolation ON agent_model_invocation_projections
    USING (EXISTS (
        SELECT 1 FROM agent_invocations invocation
        WHERE invocation.project_id = agent_model_invocation_projections.project_id
          AND invocation.id = agent_model_invocation_projections.invocation_id
          AND sprout_private.agent_party_access(invocation.project_id, invocation.agent_id)
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM agent_invocations invocation
        WHERE invocation.project_id = agent_model_invocation_projections.project_id
          AND invocation.id = agent_model_invocation_projections.invocation_id
          AND sprout_private.agent_party_access(invocation.project_id, invocation.agent_id)
    ));
CREATE POLICY agent_interrogation_answer_creator_only ON agent_interrogation_answers
    FOR SELECT
    USING (EXISTS (
        SELECT 1 FROM agent_interrogations interrogation
        WHERE interrogation.project_id = agent_interrogation_answers.project_id
          AND interrogation.id = agent_interrogation_answers.interrogation_id
          AND interrogation.creator_identity_id = sprout_private.current_identity_id()
    ));
CREATE POLICY agent_interrogation_answer_target_insert ON agent_interrogation_answers
    FOR INSERT
    WITH CHECK (EXISTS (
        SELECT 1
        FROM agent_interrogations interrogation
        JOIN agent_invocations invocation
          ON invocation.project_id = interrogation.project_id
         AND invocation.id = agent_interrogation_answers.invocation_id
         AND invocation.interrogation_id = interrogation.id
        WHERE interrogation.project_id = agent_interrogation_answers.project_id
          AND interrogation.id = agent_interrogation_answers.interrogation_id
          AND interrogation.target_agent_identity_id = sprout_private.current_identity_id()
          AND invocation.status = 'leased'
    ));
CREATE POLICY agent_language_causal_mutation_party_isolation
    ON agent_language_causal_mutations
    USING (EXISTS (
        SELECT 1 FROM agent_invocations invocation
        WHERE invocation.project_id = agent_language_causal_mutations.project_id
          AND invocation.id = agent_language_causal_mutations.invocation_id
          AND sprout_private.agent_party_access(invocation.project_id, invocation.agent_id)
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM agent_invocations invocation
        WHERE invocation.project_id = agent_language_causal_mutations.project_id
          AND invocation.id = agent_language_causal_mutations.invocation_id
          AND sprout_private.agent_party_access(invocation.project_id, invocation.agent_id)
    ));

CREATE FUNCTION sprout_private.interrogation_invocation_is_read_only(
    candidate_project_id uuid,
    candidate_invocation_id uuid
)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM public.agent_invocations invocation
        JOIN public.governed_agents agent
          ON agent.project_id = invocation.project_id
         AND agent.id = invocation.agent_id
        JOIN public.project_memberships requester
          ON requester.project_id = invocation.project_id
         AND requester.identity_id = sprout_private.current_identity_id()
         AND requester.state = 'active'
        WHERE invocation.project_id = candidate_project_id
          AND invocation.id = candidate_invocation_id
          AND invocation.invocation_surface = 'interrogation'
          AND (
              agent.principal_identity_id = requester.identity_id
              OR agent.controller_identity_id = requester.identity_id
              OR requester.role IN ('owner', 'admin')
          )
          AND NOT EXISTS (
              SELECT 1 FROM public.agent_effect_proposals effect
              WHERE effect.project_id = invocation.project_id
                AND effect.invocation_id = invocation.id
          )
          AND NOT EXISTS (
              SELECT 1 FROM public.agent_language_causal_mutations mutation
              WHERE mutation.project_id = invocation.project_id
                AND mutation.invocation_id = invocation.id
                AND mutation.category = 'resource_effect'
          )
    )
$$;

-- Certified surface records are stricter than runtime history. Explicit
-- failures remain immutable history but do not witness a successful R5.40
-- model event. Each view also retains the current-party predicate explicitly;
-- view-owner execution must not expose cross-project identifiers.
CREATE VIEW agent_r541_model_surface_records AS
SELECT projection.project_id, projection.invocation_id, projection.observation_id,
       projection.trace_id, projection.run_id, projection.goal_id,
       projection.work_item_id, projection.work_attempt
FROM agent_model_invocation_projections projection
JOIN agent_model_attempt_observations observation
  ON observation.project_id = projection.project_id
 AND observation.id = projection.observation_id
 AND observation.invocation_id = projection.invocation_id
 AND observation.attempt = projection.provider_attempt
WHERE projection.status = 'succeeded' AND observation.status = 'succeeded'
  AND projection.trace_id IS NOT NULL
  AND projection.request_commitment = observation.request_commitment
  AND projection.context_commitment = observation.context_commitment
  AND projection.output_commitment = observation.output_commitment
  AND projection.artifact_commitment = observation.artifact_commitment
  AND EXISTS (
      SELECT 1 FROM agent_invocations invocation
      WHERE invocation.project_id = projection.project_id
        AND invocation.id = projection.invocation_id
        AND sprout_private.agent_party_access(invocation.project_id, invocation.agent_id)
  );

CREATE VIEW agent_r541_interrogation_surface_records AS
SELECT interrogation.project_id, interrogation.id AS interrogation_id,
       projection.invocation_id, projection.observation_id
FROM agent_interrogations interrogation
JOIN agent_interrogation_answers answer
  ON answer.project_id = interrogation.project_id
 AND answer.interrogation_id = interrogation.id
JOIN agent_model_invocation_projections projection
  ON projection.project_id = interrogation.project_id
 AND projection.invocation_id = answer.invocation_id
JOIN agent_invocations invocation
  ON invocation.project_id = projection.project_id
 AND invocation.id = projection.invocation_id
 AND invocation.interrogation_id = interrogation.id
WHERE projection.status = 'succeeded'
  AND projection.invocation_surface = 'interrogation'
  AND projection.structured_artifact ->> 'kind' = 'interrogation_answer'
  AND interrogation.creator_identity_id = sprout_private.current_identity_id();

CREATE VIEW agent_r541_proxy_surface_records AS
SELECT plan.project_id, plan.id AS plan_id, plan.request_id,
       projection.invocation_id, projection.observation_id
FROM user_proxy_plans plan
JOIN agent_model_invocation_projections projection
  ON projection.project_id = plan.project_id
 AND projection.invocation_id = plan.invocation_id
JOIN agent_invocations invocation
  ON invocation.project_id = projection.project_id
 AND invocation.id = projection.invocation_id
 AND invocation.proxy_request_id = plan.request_id
WHERE projection.status = 'succeeded'
  AND projection.invocation_surface = 'user_proxy'
  AND projection.structured_artifact ->> 'kind' = 'user_proxy_plan'
  AND invocation.context_principal_identity_id = sprout_private.current_identity_id();

-- The inventory is derived from the exact certified-record views. There is no
-- caller supplied mode and therefore no possible enabled/empty combination.
CREATE VIEW agent_r541_language_surface_inventory AS
WITH surfaces(surface) AS (
    VALUES ('model'), ('interrogation'), ('proxy'), ('comment'), ('disclosure')
), counts AS (
    SELECT 'model'::text AS surface, count(*)::bigint AS record_count
    FROM agent_r541_model_surface_records
    UNION ALL
    SELECT 'interrogation', count(*)::bigint
    FROM agent_r541_interrogation_surface_records
    UNION ALL
    SELECT 'proxy', count(*)::bigint FROM agent_r541_proxy_surface_records
    UNION ALL SELECT 'comment', 0::bigint
    UNION ALL SELECT 'disclosure', 0::bigint
)
SELECT surfaces.surface,
       CASE WHEN counts.record_count > 0 THEN 'enabled' ELSE 'disabled_fail_closed' END AS mode,
       counts.record_count
FROM surfaces JOIN counts USING (surface);

REVOKE ALL ON TABLE agent_model_attempt_dispatches FROM PUBLIC;
REVOKE ALL ON TABLE agent_model_attempt_observations FROM PUBLIC;
REVOKE ALL ON TABLE agent_model_invocation_projections FROM PUBLIC;
REVOKE ALL ON TABLE agent_interrogation_answers FROM PUBLIC;
REVOKE ALL ON TABLE agent_language_causal_mutations FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_model_surface_records FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_interrogation_surface_records FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_proxy_surface_records FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_language_surface_inventory FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.agent_language_retention_delete_allowed(uuid, uuid, uuid, uuid)
    FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_language_history_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_language_causal_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.purge_agent_language_for_interrogation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.purge_agent_language_for_proxy_request() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.purge_agent_language_for_invocation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.purge_agent_language_for_effect() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.interrogation_invocation_is_read_only(uuid, uuid)
    FROM PUBLIC;
