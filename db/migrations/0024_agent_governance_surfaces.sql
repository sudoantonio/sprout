-- Persist the remaining governance surfaces without introducing principals,
-- permissions, key stores or model memory outside the existing Sprout model.

-- The retention pipeline deletes epochs before their resource container. Info
-- documents are removed later by the container's explicit retention trigger,
-- so this existing FK must participate in the transaction-wide deferred check.
ALTER TABLE info_documents
    DROP CONSTRAINT info_documents_epoch_fk;
ALTER TABLE info_documents
    ADD CONSTRAINT info_documents_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE NO ACTION
        DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE agent_audit_log
    DROP CONSTRAINT agent_audit_log_event_kind_check;
ALTER TABLE agent_audit_log
    ADD CONSTRAINT agent_audit_log_event_kind_check CHECK (event_kind IN (
        'agent_provisioned', 'runner_activated', 'responsibility_recorded',
        'local_goal_recorded', 'global_contract_recorded',
        'interrogation_recorded', 'invocation_queued', 'invocation_leased',
        'invocation_succeeded', 'invocation_failed', 'effect_rejected',
        'effect_applied', 'runner_revoked'
    ));

-- Provenance joins must identify the same concrete agent record, rather than
-- accepting independently valid agent and identity UUIDs.
ALTER TABLE governed_agents
    ADD CONSTRAINT governed_agents_project_id_principal_unique
        UNIQUE (project_id, id, principal_identity_id);
ALTER TABLE agent_local_goal_contracts
    ADD CONSTRAINT agent_local_goals_project_id_revision_agent_unique
        UNIQUE (project_id, id, revision, agent_id);
ALTER TABLE agent_responsibility_contracts
    ADD CONSTRAINT agent_responsibilities_user_revision_unique
        UNIQUE (project_id, user_identity_id, revision);

CREATE UNIQUE INDEX agent_local_goals_one_active_per_agent_idx
    ON agent_local_goal_contracts (project_id, agent_id)
    WHERE state = 'active';

CREATE TABLE agent_global_contracts (
    id uuid NOT NULL,
    project_id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision > 0),
    synthesis_envelope jsonb NOT NULL
        CHECK (jsonb_typeof(synthesis_envelope) = 'object'),
    candidate jsonb NOT NULL CHECK (jsonb_typeof(candidate) = 'object'),
    groundings jsonb NOT NULL CHECK (jsonb_typeof(groundings) = 'array'),
    synthesis_invocation_id uuid,
    synthesized_by_agent_id uuid,
    contract_hash bytea NOT NULL CHECK (octet_length(contract_hash) = 32),
    recorded_by_identity_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, id, revision),
    CONSTRAINT agent_global_contracts_recorder_fk
        FOREIGN KEY (project_id, recorded_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_global_contracts_invocation_fk
        FOREIGN KEY (project_id, synthesis_invocation_id)
        REFERENCES agent_invocations (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_global_contracts_synthesizer_fk
        FOREIGN KEY (project_id, synthesized_by_agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_global_contracts_synthesizer_shape CHECK (
        (synthesis_invocation_id IS NULL) = (synthesized_by_agent_id IS NULL)
    ),
    CONSTRAINT agent_global_contracts_hash_unique UNIQUE (project_id, contract_hash)
);

CREATE TABLE agent_global_contract_sources (
    project_id uuid NOT NULL,
    global_contract_id uuid NOT NULL,
    global_revision bigint NOT NULL,
    agent_id uuid NOT NULL,
    local_goal_id uuid NOT NULL,
    local_revision bigint NOT NULL,
    PRIMARY KEY (
        project_id, global_contract_id, global_revision, agent_id,
        local_goal_id, local_revision
    ),
    CONSTRAINT agent_global_sources_contract_fk
        FOREIGN KEY (project_id, global_contract_id, global_revision)
        REFERENCES agent_global_contracts (project_id, id, revision)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_global_sources_agent_fk
        FOREIGN KEY (project_id, agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_global_sources_local_goal_fk
        FOREIGN KEY (project_id, local_goal_id, local_revision, agent_id)
        REFERENCES agent_local_goal_contracts (project_id, id, revision, agent_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE TABLE user_proxies (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    user_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT user_proxies_user_fk
        FOREIGN KEY (project_id, user_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT user_proxies_project_user_unique UNIQUE (project_id, user_identity_id),
    CONSTRAINT user_proxies_project_id_unique UNIQUE (project_id, id)
);

CREATE TABLE user_proxy_threads (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    proxy_id uuid NOT NULL,
    creator_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    closed_at timestamptz,
    CONSTRAINT user_proxy_threads_proxy_fk
        FOREIGN KEY (project_id, proxy_id)
        REFERENCES user_proxies (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT user_proxy_threads_creator_fk
        FOREIGN KEY (project_id, creator_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT user_proxy_threads_project_id_unique UNIQUE (project_id, id)
);

CREATE TABLE user_proxy_requests (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    user_identity_id uuid NOT NULL,
    encrypted_payload bytea NOT NULL CHECK (octet_length(encrypted_payload) > 0),
    submitted_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT user_proxy_requests_thread_fk
        FOREIGN KEY (project_id, thread_id)
        REFERENCES user_proxy_threads (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT user_proxy_requests_user_fk
        FOREIGN KEY (project_id, user_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT user_proxy_requests_project_id_unique UNIQUE (project_id, id)
);

CREATE TABLE user_proxy_plans (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    request_id uuid NOT NULL,
    invocation_id uuid,
    planning_envelope jsonb NOT NULL
        CHECK (jsonb_typeof(planning_envelope) = 'object'),
    action_plan jsonb NOT NULL CHECK (jsonb_typeof(action_plan) = 'object'),
    action_classification jsonb NOT NULL
        CHECK (jsonb_typeof(action_classification) = 'array'),
    responsibility_id uuid,
    responsibility_revision bigint,
    confirmation jsonb,
    plan_hash bytea NOT NULL CHECK (octet_length(plan_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT user_proxy_plans_request_fk
        FOREIGN KEY (project_id, request_id)
        REFERENCES user_proxy_requests (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT user_proxy_plans_invocation_fk
        FOREIGN KEY (project_id, invocation_id)
        REFERENCES agent_invocations (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT user_proxy_plans_responsibility_fk
        FOREIGN KEY (project_id, responsibility_id, responsibility_revision)
        REFERENCES agent_responsibility_contracts (project_id, id, revision)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT user_proxy_plans_responsibility_shape CHECK (
        (responsibility_id IS NULL) = (responsibility_revision IS NULL)
    ),
    CONSTRAINT user_proxy_plans_confirmation_shape CHECK (
        confirmation IS NULL OR jsonb_typeof(confirmation) = 'object'
    ),
    CONSTRAINT user_proxy_plans_request_unique UNIQUE (project_id, request_id),
    CONSTRAINT user_proxy_plans_hash_unique UNIQUE (project_id, plan_hash)
);

CREATE TABLE agent_interrogations (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    creator_identity_id uuid NOT NULL,
    target_agent_id uuid NOT NULL,
    target_agent_identity_id uuid NOT NULL,
    transcript_resource_node_id uuid NOT NULL,
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    encrypted_transcript bytea NOT NULL CHECK (octet_length(encrypted_transcript) > 0),
    causal_delta jsonb NOT NULL CHECK (jsonb_typeof(causal_delta) = 'object'),
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT agent_interrogations_creator_fk
        FOREIGN KEY (project_id, creator_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_interrogations_agent_fk
        FOREIGN KEY (project_id, target_agent_id, target_agent_identity_id)
        REFERENCES governed_agents (project_id, id, principal_identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_interrogations_resource_fk
        FOREIGN KEY (project_id, transcript_resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_interrogations_epoch_fk
        FOREIGN KEY (project_id, transcript_resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE NO ACTION
        DEFERRABLE INITIALLY DEFERRED
);

CREATE FUNCTION sprout_private.agent_retention_row_marked(
    setting_name text,
    candidate_id uuid,
    candidate_project_id uuid
)
RETURNS boolean
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    marked jsonb;
BEGIN
    marked := COALESCE(
        NULLIF(current_setting(setting_name, true), '')::jsonb,
        '[]'::jsonb
    );
    RETURN marked ? candidate_id::text
       AND sprout_private.retention_purge_row_allowed(jsonb_build_object(
           'project_id', candidate_project_id,
           'resource_node_id', NULLIF(current_setting(
               'app.agent_retention_resource_id', true
           ), '')::uuid
       ));
EXCEPTION
    WHEN invalid_text_representation THEN RETURN false;
END;
$$;

CREATE OR REPLACE FUNCTION sprout_private.reject_agent_audit_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' AND sprout_private.agent_retention_row_marked(
        'app.agent_retention_audit_ids', OLD.id, OLD.project_id
    )
    THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'agent audit records are append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE FUNCTION sprout_private.reject_agent_governance_history_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    row_data jsonb := to_jsonb(OLD);
    row_project_id uuid := NULLIF(row_data ->> 'project_id', '')::uuid;
BEGIN
    IF TG_OP = 'DELETE' AND (
        (TG_TABLE_NAME = 'agent_global_contracts'
         AND sprout_private.agent_retention_row_marked(
             'app.agent_retention_global_contract_ids',
             NULLIF(row_data ->> 'id', '')::uuid,
             row_project_id
         ))
        OR (TG_TABLE_NAME = 'agent_global_contract_sources'
            AND sprout_private.agent_retention_row_marked(
                'app.agent_retention_global_contract_ids',
                NULLIF(row_data ->> 'global_contract_id', '')::uuid,
                row_project_id
            ))
        OR (TG_TABLE_NAME = 'user_proxy_plans'
            AND sprout_private.agent_retention_row_marked(
                'app.agent_retention_proxy_plan_ids',
                NULLIF(row_data ->> 'id', '')::uuid,
                row_project_id
            ))
        OR (TG_TABLE_NAME = 'user_proxy_requests'
            AND sprout_private.agent_retention_row_marked(
                'app.agent_retention_proxy_request_ids',
                NULLIF(row_data ->> 'id', '')::uuid,
                row_project_id
            ))
        OR (TG_TABLE_NAME = 'agent_interrogations'
            AND sprout_private.agent_retention_row_marked(
                'app.agent_retention_interrogation_ids',
                NULLIF(row_data ->> 'id', '')::uuid,
                row_project_id
            ))
       )
    THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'agent governance history is append-only'
        USING ERRCODE = '55000';
END;
$$;

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'agent_global_contracts', 'agent_global_contract_sources',
        'user_proxies', 'user_proxy_threads', 'user_proxy_requests',
        'user_proxy_plans', 'agent_interrogations'
    ]
    LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
    END LOOP;
    FOREACH table_name IN ARRAY ARRAY[
        'agent_global_contracts', 'agent_global_contract_sources',
        'user_proxy_requests', 'user_proxy_plans', 'agent_interrogations'
    ]
    LOOP
        EXECUTE format(
            'CREATE TRIGGER %I_append_only BEFORE UPDATE OR DELETE ON %I
             FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_governance_history_mutation()',
            table_name, table_name
        );
    END LOOP;
END;
$$;

CREATE POLICY global_contract_admin_access ON agent_global_contracts
    USING (
        EXISTS (
            SELECT 1 FROM project_memberships membership
            WHERE membership.project_id = agent_global_contracts.project_id
              AND membership.identity_id = sprout_private.current_identity_id()
              AND membership.state = 'active'
              AND membership.role IN ('owner', 'admin')
        )
        OR (
            recorded_by_identity_id = sprout_private.current_identity_id()
            AND EXISTS (
                SELECT 1
                FROM agent_invocations invocation
                JOIN governed_agents agent
                  ON agent.project_id = invocation.project_id
                 AND agent.id = invocation.agent_id
                JOIN agent_runners runner
                  ON runner.project_id = invocation.project_id
                 AND runner.id = invocation.runner_id
                 AND runner.agent_id = invocation.agent_id
                JOIN device_keys key
                  ON key.identity_id = runner.principal_identity_id
                 AND key.device_id = runner.device_id
                 AND key.key_version = runner.activated_key_version
                WHERE invocation.project_id = agent_global_contracts.project_id
                  AND invocation.id = agent_global_contracts.synthesis_invocation_id
                  AND invocation.agent_id = agent_global_contracts.synthesized_by_agent_id
                  AND invocation.status = 'succeeded'
                  AND agent.principal_identity_id = sprout_private.current_identity_id()
                  AND agent.state = 'active'
                  AND runner.device_id = sprout_private.current_device_id()
                  AND runner.state = 'active'
                  AND key.revoked_at IS NULL
            )
        )
    )
    WITH CHECK (
        EXISTS (
            SELECT 1 FROM project_memberships membership
            WHERE membership.project_id = agent_global_contracts.project_id
              AND membership.identity_id = sprout_private.current_identity_id()
              AND membership.state = 'active'
              AND membership.role IN ('owner', 'admin')
        )
        OR (
            recorded_by_identity_id = sprout_private.current_identity_id()
            AND EXISTS (
                SELECT 1
                FROM agent_invocations invocation
                JOIN governed_agents agent
                  ON agent.project_id = invocation.project_id
                 AND agent.id = invocation.agent_id
                JOIN agent_runners runner
                  ON runner.project_id = invocation.project_id
                 AND runner.id = invocation.runner_id
                 AND runner.agent_id = invocation.agent_id
                JOIN device_keys key
                  ON key.identity_id = runner.principal_identity_id
                 AND key.device_id = runner.device_id
                 AND key.key_version = runner.activated_key_version
                WHERE invocation.project_id = agent_global_contracts.project_id
                  AND invocation.id = agent_global_contracts.synthesis_invocation_id
                  AND invocation.agent_id = agent_global_contracts.synthesized_by_agent_id
                  AND invocation.status = 'succeeded'
                  AND agent.principal_identity_id = sprout_private.current_identity_id()
                  AND agent.state = 'active'
                  AND runner.device_id = sprout_private.current_device_id()
                  AND runner.state = 'active'
                  AND key.revoked_at IS NULL
            )
        )
    );
CREATE POLICY global_source_admin_access ON agent_global_contract_sources
    USING (
        EXISTS (
            SELECT 1 FROM project_memberships membership
            WHERE membership.project_id = agent_global_contract_sources.project_id
              AND membership.identity_id = sprout_private.current_identity_id()
              AND membership.state = 'active'
              AND membership.role IN ('owner', 'admin')
        )
        OR EXISTS (
            SELECT 1 FROM agent_global_contracts global_contract
            WHERE global_contract.project_id = agent_global_contract_sources.project_id
              AND global_contract.id = agent_global_contract_sources.global_contract_id
              AND global_contract.revision = agent_global_contract_sources.global_revision
              AND global_contract.recorded_by_identity_id = sprout_private.current_identity_id()
        )
    )
    WITH CHECK (
        EXISTS (
            SELECT 1 FROM project_memberships membership
            WHERE membership.project_id = agent_global_contract_sources.project_id
              AND membership.identity_id = sprout_private.current_identity_id()
              AND membership.state = 'active'
              AND membership.role IN ('owner', 'admin')
        )
        OR EXISTS (
            SELECT 1 FROM agent_global_contracts global_contract
            WHERE global_contract.project_id = agent_global_contract_sources.project_id
              AND global_contract.id = agent_global_contract_sources.global_contract_id
              AND global_contract.revision = agent_global_contract_sources.global_revision
              AND global_contract.recorded_by_identity_id = sprout_private.current_identity_id()
        )
    );

CREATE POLICY user_proxy_self_access ON user_proxies
    USING (user_identity_id = sprout_private.current_identity_id())
    WITH CHECK (user_identity_id = sprout_private.current_identity_id());
CREATE POLICY user_proxy_thread_self_access ON user_proxy_threads
    USING (creator_identity_id = sprout_private.current_identity_id())
    WITH CHECK (creator_identity_id = sprout_private.current_identity_id());
CREATE POLICY user_proxy_request_self_access ON user_proxy_requests
    USING (user_identity_id = sprout_private.current_identity_id())
    WITH CHECK (user_identity_id = sprout_private.current_identity_id());
CREATE POLICY user_proxy_plan_self_access ON user_proxy_plans
    USING (EXISTS (
        SELECT 1 FROM user_proxy_requests request
        WHERE request.project_id = user_proxy_plans.project_id
          AND request.id = user_proxy_plans.request_id
          AND request.user_identity_id = sprout_private.current_identity_id()
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM user_proxy_requests request
        WHERE request.project_id = user_proxy_plans.project_id
          AND request.id = user_proxy_plans.request_id
          AND request.user_identity_id = sprout_private.current_identity_id()
    ));

CREATE POLICY interrogation_creator_only ON agent_interrogations
    USING (creator_identity_id = sprout_private.current_identity_id())
    WITH CHECK (creator_identity_id = sprout_private.current_identity_id());

REVOKE ALL ON FUNCTION sprout_private.reject_agent_governance_history_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.agent_retention_row_marked(text, uuid, uuid) FROM PUBLIC;

-- Resource retention remains the only path that may physically remove
-- append-only agent records. No FK performs an implicit cascade: this trigger
-- computes the closed dependent set first, marks exactly those rows for the
-- append-only guards, and deletes them in dependency order.
CREATE FUNCTION sprout_private.purge_agent_records_for_resource()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    agent_ids uuid[] := ARRAY[]::uuid[];
    agent_identity_ids uuid[] := ARRAY[]::uuid[];
    runner_device_ids uuid[] := ARRAY[]::uuid[];
    local_goal_ids uuid[] := ARRAY[]::uuid[];
    responsibility_ids uuid[] := ARRAY[]::uuid[];
    invocation_ids uuid[] := ARRAY[]::uuid[];
    global_contract_ids uuid[] := ARRAY[]::uuid[];
    proxy_plan_ids uuid[] := ARRAY[]::uuid[];
    proxy_request_ids uuid[] := ARRAY[]::uuid[];
    proxy_thread_ids uuid[] := ARRAY[]::uuid[];
    proxy_ids uuid[] := ARRAY[]::uuid[];
    interrogation_ids uuid[] := ARRAY[]::uuid[];
    audit_ids uuid[] := ARRAY[]::uuid[];
BEGIN
    IF TG_OP <> 'DELETE' OR NOT sprout_private.retention_purge_row_allowed(
        jsonb_build_object(
            'project_id', OLD.project_id,
            'resource_node_id', OLD.id
        )
    ) THEN
        RETURN OLD;
    END IF;

    SELECT COALESCE(array_agg(agent.id), ARRAY[]::uuid[])
    INTO agent_ids
    FROM governed_agents agent
    WHERE agent.project_id = OLD.project_id
      AND agent.profile_resource_node_id = OLD.id;

    SELECT COALESCE(array_agg(agent.principal_identity_id), ARRAY[]::uuid[])
    INTO agent_identity_ids
    FROM governed_agents agent
    WHERE agent.project_id = OLD.project_id AND agent.id = ANY(agent_ids);

    SELECT COALESCE(array_agg(runner.device_id), ARRAY[]::uuid[])
    INTO runner_device_ids
    FROM agent_runners runner
    WHERE runner.project_id = OLD.project_id AND runner.agent_id = ANY(agent_ids);

    SELECT COALESCE(array_agg(DISTINCT local.id), ARRAY[]::uuid[])
    INTO local_goal_ids
    FROM agent_local_goal_contracts local
    WHERE local.project_id = OLD.project_id
      AND (
          local.agent_id = ANY(agent_ids)
          OR local.contract #>> '{contract,scope}' = OLD.id::text
          OR EXISTS (
              SELECT 1
              FROM jsonb_array_elements(local.contract -> 'clauses') clause
              WHERE clause ->> 'scope' = OLD.id::text
          )
      );

    SELECT COALESCE(array_agg(DISTINCT responsibility.id), ARRAY[]::uuid[])
    INTO responsibility_ids
    FROM agent_responsibility_contracts responsibility
    WHERE responsibility.project_id = OLD.project_id
      AND EXISTS (
          SELECT 1
          FROM jsonb_array_elements(responsibility.contract -> 'rules') rule
          WHERE rule ->> 'scope' = OLD.id::text
      );

    SELECT COALESCE(array_agg(DISTINCT invocation.id), ARRAY[]::uuid[])
    INTO invocation_ids
    FROM agent_invocations invocation
    WHERE invocation.project_id = OLD.project_id
      AND (
          invocation.agent_id = ANY(agent_ids)
          OR invocation.local_goal_id = ANY(local_goal_ids)
          OR EXISTS (
              SELECT 1
              FROM agent_invocation_sources source
              WHERE source.project_id = invocation.project_id
                AND source.invocation_id = invocation.id
                AND source.resource_node_id = OLD.id
          )
          OR EXISTS (
              SELECT 1
              FROM agent_effect_proposals effect
              WHERE effect.project_id = invocation.project_id
                AND effect.invocation_id = invocation.id
                AND effect.effect ->> 'resource_id' = OLD.id::text
          )
      );

    SELECT COALESCE(array_agg(DISTINCT global_contract.id), ARRAY[]::uuid[])
    INTO global_contract_ids
    FROM agent_global_contracts global_contract
    WHERE global_contract.project_id = OLD.project_id
      AND (
          global_contract.candidate #>> '{contract,scope}' = OLD.id::text
          OR global_contract.synthesis_invocation_id = ANY(invocation_ids)
          OR EXISTS (
              SELECT 1
              FROM agent_global_contract_sources source
              WHERE source.project_id = global_contract.project_id
                AND source.global_contract_id = global_contract.id
                AND source.global_revision = global_contract.revision
                AND (
                    source.agent_id = ANY(agent_ids)
                    OR source.local_goal_id = ANY(local_goal_ids)
                )
          )
      );

    SELECT COALESCE(array_agg(DISTINCT plan.id), ARRAY[]::uuid[])
    INTO proxy_plan_ids
    FROM user_proxy_plans plan
    WHERE plan.project_id = OLD.project_id
      AND (
          plan.invocation_id = ANY(invocation_ids)
          OR plan.responsibility_id = ANY(responsibility_ids)
          OR EXISTS (
              SELECT 1
              FROM jsonb_array_elements(plan.action_classification) action
              WHERE action ->> 'resource_id' = OLD.id::text
          )
      );

    SELECT COALESCE(array_agg(DISTINCT plan.request_id), ARRAY[]::uuid[])
    INTO proxy_request_ids
    FROM user_proxy_plans plan
    WHERE plan.project_id = OLD.project_id
      AND plan.id = ANY(proxy_plan_ids);

    SELECT COALESCE(array_agg(DISTINCT request.thread_id), ARRAY[]::uuid[])
    INTO proxy_thread_ids
    FROM user_proxy_requests request
    WHERE request.project_id = OLD.project_id
      AND request.id = ANY(proxy_request_ids);

    SELECT COALESCE(array_agg(DISTINCT thread.proxy_id), ARRAY[]::uuid[])
    INTO proxy_ids
    FROM user_proxy_threads thread
    WHERE thread.project_id = OLD.project_id
      AND thread.id = ANY(proxy_thread_ids);

    SELECT COALESCE(array_agg(DISTINCT interrogation.id), ARRAY[]::uuid[])
    INTO interrogation_ids
    FROM agent_interrogations interrogation
    WHERE interrogation.project_id = OLD.project_id
      AND (
          interrogation.target_agent_id = ANY(agent_ids)
          OR interrogation.transcript_resource_node_id = OLD.id
      );

    SELECT COALESCE(array_agg(DISTINCT audit.id), ARRAY[]::uuid[])
    INTO audit_ids
    FROM agent_audit_log audit
    WHERE audit.project_id = OLD.project_id
      AND (
          audit.agent_id = ANY(agent_ids)
          OR audit.invocation_id = ANY(invocation_ids)
          OR (audit.facts ->> 'local_goal_id')::uuid = ANY(local_goal_ids)
          OR (audit.facts ->> 'responsibility_id')::uuid = ANY(responsibility_ids)
          OR (audit.facts ->> 'global_contract_id')::uuid = ANY(global_contract_ids)
          OR (audit.facts ->> 'interrogation_id')::uuid = ANY(interrogation_ids)
      );

    PERFORM set_config('app.agent_retention_resource_id', OLD.id::text, true);
    PERFORM set_config(
        'app.agent_retention_audit_ids', to_jsonb(audit_ids)::text, true
    );
    PERFORM set_config(
        'app.agent_retention_global_contract_ids',
        to_jsonb(global_contract_ids)::text,
        true
    );
    PERFORM set_config(
        'app.agent_retention_proxy_plan_ids', to_jsonb(proxy_plan_ids)::text, true
    );
    PERFORM set_config(
        'app.agent_retention_proxy_request_ids',
        to_jsonb(proxy_request_ids)::text,
        true
    );
    PERFORM set_config(
        'app.agent_retention_interrogation_ids',
        to_jsonb(interrogation_ids)::text,
        true
    );

    DELETE FROM agent_audit_log
    WHERE project_id = OLD.project_id AND id = ANY(audit_ids);
    DELETE FROM agent_global_contract_sources
    WHERE project_id = OLD.project_id
      AND global_contract_id = ANY(global_contract_ids);
    DELETE FROM agent_global_contracts
    WHERE project_id = OLD.project_id AND id = ANY(global_contract_ids);
    DELETE FROM user_proxy_plans
    WHERE project_id = OLD.project_id AND id = ANY(proxy_plan_ids);
    DELETE FROM user_proxy_requests
    WHERE project_id = OLD.project_id AND id = ANY(proxy_request_ids);
    DELETE FROM user_proxy_threads thread
    WHERE thread.project_id = OLD.project_id
      AND thread.id = ANY(proxy_thread_ids)
      AND NOT EXISTS (
          SELECT 1 FROM user_proxy_requests request
          WHERE request.project_id = thread.project_id
            AND request.thread_id = thread.id
      );
    DELETE FROM user_proxies proxy
    WHERE proxy.project_id = OLD.project_id
      AND proxy.id = ANY(proxy_ids)
      AND NOT EXISTS (
          SELECT 1 FROM user_proxy_threads thread
          WHERE thread.project_id = proxy.project_id
            AND thread.proxy_id = proxy.id
      );
    DELETE FROM agent_interrogations
    WHERE project_id = OLD.project_id AND id = ANY(interrogation_ids);
    DELETE FROM agent_effect_proposals
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM agent_invocation_sources
    WHERE project_id = OLD.project_id AND invocation_id = ANY(invocation_ids);
    DELETE FROM agent_invocations
    WHERE project_id = OLD.project_id AND id = ANY(invocation_ids);
    DELETE FROM agent_local_goal_contracts
    WHERE project_id = OLD.project_id AND id = ANY(local_goal_ids);
    DELETE FROM agent_responsibility_contracts
    WHERE project_id = OLD.project_id AND id = ANY(responsibility_ids);
    UPDATE sessions
    SET revoked_at = COALESCE(revoked_at, clock_timestamp()),
        revoke_reason = COALESCE(revoke_reason, 'agent_profile_retention')
    WHERE identity_id = ANY(agent_identity_ids) AND revoked_at IS NULL;
    UPDATE device_keys
    SET revoked_at = COALESCE(revoked_at, clock_timestamp())
    WHERE identity_id = ANY(agent_identity_ids)
      AND device_id = ANY(runner_device_ids)
      AND revoked_at IS NULL;
    UPDATE devices
    SET trust_state = 'retired',
        retired_at = COALESCE(retired_at, clock_timestamp())
    WHERE identity_id = ANY(agent_identity_ids)
      AND id = ANY(runner_device_ids);
    DELETE FROM agent_runners
    WHERE project_id = OLD.project_id AND agent_id = ANY(agent_ids);
    DELETE FROM governed_agents
    WHERE project_id = OLD.project_id AND id = ANY(agent_ids);
    RETURN OLD;
EXCEPTION
    WHEN invalid_text_representation THEN
        RAISE EXCEPTION 'invalid agent retention provenance'
            USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER resource_nodes_agent_retention_purge
BEFORE DELETE ON resource_nodes
FOR EACH ROW EXECUTE FUNCTION sprout_private.purge_agent_records_for_resource();

REVOKE ALL ON FUNCTION sprout_private.purge_agent_records_for_resource() FROM PUBLIC;
