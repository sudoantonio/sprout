-- Persistent concrete refinement of the R5.30 collaborative completion
-- kernel. The canonical semantic state is the schema-closed Rust snapshot in
-- agent_collaborative_runs.state. Relational child tables are immutable
-- certificates and concurrency guards, never an alternate state machine.

CREATE TABLE agent_collaborative_runs (
    id uuid NOT NULL,
    project_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    scope_resource_node_id uuid NOT NULL,
    local_goal_id uuid,
    local_goal_revision bigint,
    global_contract_id uuid,
    global_contract_revision bigint,
    contract jsonb NOT NULL CHECK (jsonb_typeof(contract) = 'object'),
    contract_hash bytea NOT NULL CHECK (octet_length(contract_hash) = 32),
    state jsonb NOT NULL CHECK (jsonb_typeof(state) = 'object'),
    state_hash bytea NOT NULL CHECK (octet_length(state_hash) = 32),
    state_version bigint NOT NULL DEFAULT 1 CHECK (state_version > 0),
    goal_status text NOT NULL
        CHECK (goal_status IN ('active', 'completed', 'failed', 'cancelled', 'superseded')),
    run_status text NOT NULL
        CHECK (run_status IN ('running', 'completed', 'cancelled')),
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, id),
    CONSTRAINT agent_runs_scope_fk
        FOREIGN KEY (project_id, scope_resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_runs_local_contract_fk
        FOREIGN KEY (project_id, local_goal_id, local_goal_revision)
        REFERENCES agent_local_goal_contracts (project_id, id, revision)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_runs_global_contract_fk
        FOREIGN KEY (project_id, global_contract_id, global_contract_revision)
        REFERENCES agent_global_contracts (project_id, id, revision)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_runs_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_runs_contract_source_shape CHECK (
        (
            local_goal_id IS NOT NULL AND local_goal_revision IS NOT NULL
            AND global_contract_id IS NULL AND global_contract_revision IS NULL
        ) OR (
            local_goal_id IS NULL AND local_goal_revision IS NULL
            AND global_contract_id IS NOT NULL AND global_contract_revision IS NOT NULL
        )
    ),
    CONSTRAINT agent_runs_completed_shape CHECK (
        run_status <> 'completed' OR goal_status = 'completed'
    )
);

CREATE INDEX agent_runs_scope_status_idx
    ON agent_collaborative_runs (project_id, scope_resource_node_id, goal_status, updated_at);

CREATE TABLE agent_run_participants (
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    identity_id uuid NOT NULL,
    participant_role text NOT NULL
        CHECK (participant_role IN ('agent', 'controller', 'sponsor')),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, run_id, identity_id, participant_role),
    CONSTRAINT agent_run_participants_run_fk
        FOREIGN KEY (project_id, run_id)
        REFERENCES agent_collaborative_runs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_participants_member_fk
        FOREIGN KEY (project_id, identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE TABLE agent_run_work_slots (
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    work_spec_ordinal bigint NOT NULL CHECK (work_spec_ordinal >= 0),
    slot integer NOT NULL CHECK (slot >= 0),
    work_item_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, run_id, work_spec_ordinal, slot),
    CONSTRAINT agent_run_work_slots_run_fk
        FOREIGN KEY (project_id, run_id)
        REFERENCES agent_collaborative_runs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_work_slots_work_unique
        UNIQUE (project_id, run_id, work_item_id)
);

CREATE TABLE agent_run_claim_leases (
    id uuid NOT NULL,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    claimant_identity_id uuid NOT NULL,
    acquired_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    status text NOT NULL CHECK (status IN ('active', 'expired', 'released')),
    terminal_at timestamptz,
    PRIMARY KEY (project_id, id),
    CONSTRAINT agent_run_claims_work_fk
        FOREIGN KEY (project_id, run_id, work_item_id)
        REFERENCES agent_run_work_slots (project_id, run_id, work_item_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_claims_claimant_fk
        FOREIGN KEY (project_id, claimant_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_claims_time_shape CHECK (expires_at > acquired_at),
    CONSTRAINT agent_run_claims_terminal_shape CHECK (
        (status = 'active' AND terminal_at IS NULL)
        OR (status <> 'active' AND terminal_at IS NOT NULL)
    ),
    CONSTRAINT agent_run_claims_work_attempt_unique
        UNIQUE (project_id, run_id, work_item_id, attempt)
);

CREATE UNIQUE INDEX agent_run_claims_one_active_per_work_idx
    ON agent_run_claim_leases (project_id, run_id, work_item_id)
    WHERE status = 'active';

CREATE INDEX agent_run_claims_expiry_idx
    ON agent_run_claim_leases (expires_at, project_id, run_id)
    WHERE status = 'active';

CREATE TABLE agent_run_blockers (
    id uuid NOT NULL,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    obligation_id uuid NOT NULL,
    waiting_rule_ordinal bigint NOT NULL CHECK (waiting_rule_ordinal >= 0),
    scope jsonb NOT NULL CHECK (jsonb_typeof(scope) = 'object'),
    waiting_condition jsonb NOT NULL CHECK (jsonb_typeof(waiting_condition) = 'object'),
    current_status text NOT NULL
        CHECK (current_status IN ('waiting', 'resolved', 'failed', 'cancelled')),
    created_tick bigint NOT NULL CHECK (created_tick >= 0),
    terminal_tick bigint CHECK (terminal_tick IS NULL OR terminal_tick >= created_tick),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, id),
    CONSTRAINT agent_run_blockers_run_fk
        FOREIGN KEY (project_id, run_id)
        REFERENCES agent_collaborative_runs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_blockers_status_shape CHECK (
        (current_status = 'waiting' AND terminal_tick IS NULL)
        OR (current_status <> 'waiting' AND terminal_tick IS NOT NULL)
    ),
    CONSTRAINT agent_run_blockers_run_id_unique
        UNIQUE (project_id, run_id, id)
);

CREATE TABLE agent_run_transitions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    sequence bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    state_version bigint NOT NULL CHECK (state_version > 0),
    transition_kind text NOT NULL CHECK (transition_kind IN (
        'initialized', 'frontier_refreshed', 'work_claimed',
        'claim_recovered', 'work_succeeded', 'work_failed',
        'blocker_created', 'blocker_resolved', 'evidence_accepted',
        'goal_completed', 'run_completed'
    )),
    runtime_actor_kind text NOT NULL CHECK (runtime_actor_kind IN ('principal', 'scheduler')),
    actor_identity_id uuid,
    actor_device_id uuid,
    observation_kind text,
    observation_id uuid,
    previous_state_hash bytea,
    next_state_hash bytea NOT NULL CHECK (octet_length(next_state_hash) = 32),
    facts_hash bytea NOT NULL CHECK (octet_length(facts_hash) = 32),
    state_snapshot jsonb NOT NULL CHECK (jsonb_typeof(state_snapshot) = 'object'),
    fact_references jsonb NOT NULL CHECK (jsonb_typeof(fact_references) = 'object'),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_run_transitions_run_fk
        FOREIGN KEY (project_id, run_id)
        REFERENCES agent_collaborative_runs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_transitions_actor_fk
        FOREIGN KEY (project_id, actor_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_transitions_device_fk
        FOREIGN KEY (actor_identity_id, actor_device_id)
        REFERENCES devices (identity_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_transitions_version_unique
        UNIQUE (project_id, run_id, state_version),
    CONSTRAINT agent_run_transitions_previous_hash_shape CHECK (
        (state_version = 1 AND previous_state_hash IS NULL)
        OR (state_version > 1 AND octet_length(previous_state_hash) = 32)
    ),
    CONSTRAINT agent_run_transitions_observation_shape CHECK (
        (observation_kind IS NULL) = (observation_id IS NULL)
    ),
    CONSTRAINT agent_run_transitions_actor_shape CHECK (
        (runtime_actor_kind = 'principal' AND actor_identity_id IS NOT NULL)
        OR (
            runtime_actor_kind = 'scheduler'
            AND actor_identity_id IS NULL AND actor_device_id IS NULL
        )
    )
);

CREATE TABLE agent_run_work_product_bindings (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    claim_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    invocation_id uuid NOT NULL,
    effect_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    bound_at timestamptz NOT NULL,
    claim_transition_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_run_work_bindings_work_fk
        FOREIGN KEY (project_id, run_id, work_item_id)
        REFERENCES agent_run_work_slots (project_id, run_id, work_item_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_work_bindings_claim_fk
        FOREIGN KEY (project_id, claim_id)
        REFERENCES agent_run_claim_leases (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_work_bindings_invocation_fk
        FOREIGN KEY (project_id, invocation_id)
        REFERENCES agent_invocations (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_work_bindings_effect_fk
        FOREIGN KEY (effect_id) REFERENCES agent_effect_proposals (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_work_bindings_resource_fk
        FOREIGN KEY (project_id, resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_work_bindings_transition_fk
        FOREIGN KEY (claim_transition_id) REFERENCES agent_run_transitions (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_work_bindings_attempt_unique
        UNIQUE (project_id, run_id, work_item_id, attempt),
    CONSTRAINT agent_run_work_bindings_effect_unique
        UNIQUE (project_id, effect_id)
);

CREATE TABLE agent_run_work_outcomes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    claim_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    outcome_kind text NOT NULL CHECK (outcome_kind IN ('task_completion')),
    product_event_id uuid NOT NULL,
    observed_at timestamptz NOT NULL,
    provenance_hash bytea NOT NULL CHECK (octet_length(provenance_hash) = 32),
    transition_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_run_work_outcomes_work_fk
        FOREIGN KEY (project_id, run_id, work_item_id)
        REFERENCES agent_run_work_slots (project_id, run_id, work_item_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_work_outcomes_claim_fk
        FOREIGN KEY (project_id, claim_id)
        REFERENCES agent_run_claim_leases (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_work_outcomes_transition_fk
        FOREIGN KEY (transition_id) REFERENCES agent_run_transitions (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_work_outcomes_attempt_unique
        UNIQUE (project_id, run_id, work_item_id, attempt),
    CONSTRAINT agent_run_work_outcomes_product_unique
        UNIQUE (project_id, outcome_kind, product_event_id)
);

CREATE TABLE agent_run_blocker_resolutions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    blocker_id uuid NOT NULL,
    observation_kind text NOT NULL CHECK (observation_kind IN (
        'human_task_terminal', 'administrator_decision',
        'principal_response', 'external_outcome'
    )),
    observation_id uuid NOT NULL,
    terminal_status text NOT NULL CHECK (terminal_status IN ('resolved', 'failed', 'cancelled')),
    observed_at timestamptz NOT NULL,
    provenance_hash bytea NOT NULL CHECK (octet_length(provenance_hash) = 32),
    transition_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_run_blocker_resolutions_run_fk
        FOREIGN KEY (project_id, run_id)
        REFERENCES agent_collaborative_runs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_blocker_resolutions_transition_fk
        FOREIGN KEY (transition_id) REFERENCES agent_run_transitions (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_blocker_resolution_unique
        UNIQUE (project_id, run_id, blocker_id)
);

CREATE TABLE agent_run_evidence_provenance (
    evidence_id uuid NOT NULL,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    obligation_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    evidence_rule_ordinal bigint NOT NULL CHECK (evidence_rule_ordinal >= 0),
    evidence_kind text NOT NULL CHECK (evidence_kind IN (
        'tool_completed', 'task_completed', 'comment_observed',
        'principal_response', 'human_approval', 'administrator_approval',
        'external_outcome', 'derived_fact'
    )),
    verification_mode text NOT NULL
        CHECK (verification_mode IN ('mechanical', 'semantic_judgment')),
    product_event_kind text NOT NULL,
    product_event_id uuid NOT NULL,
    observed_at timestamptz NOT NULL,
    provenance_hash bytea NOT NULL CHECK (octet_length(provenance_hash) = 32),
    transition_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, evidence_id),
    CONSTRAINT agent_run_evidence_run_fk
        FOREIGN KEY (project_id, run_id)
        REFERENCES agent_collaborative_runs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_evidence_work_fk
        FOREIGN KEY (project_id, run_id, work_item_id)
        REFERENCES agent_run_work_slots (project_id, run_id, work_item_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_evidence_transition_fk
        FOREIGN KEY (transition_id) REFERENCES agent_run_transitions (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_evidence_product_unique
        UNIQUE (
            project_id, run_id, evidence_rule_ordinal,
            product_event_kind, product_event_id
        )
);

CREATE TABLE agent_run_causal_links (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    predecessor jsonb NOT NULL CHECK (jsonb_typeof(predecessor) = 'object'),
    successor jsonb NOT NULL CHECK (jsonb_typeof(successor) = 'object'),
    observed_tick bigint NOT NULL CHECK (observed_tick >= 0),
    transition_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_run_causal_links_run_fk
        FOREIGN KEY (project_id, run_id)
        REFERENCES agent_collaborative_runs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_causal_links_transition_fk
        FOREIGN KEY (transition_id) REFERENCES agent_run_transitions (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_causal_link_unique
        UNIQUE (project_id, run_id, predecessor, successor)
);

CREATE FUNCTION sprout_private.agent_run_access(
    candidate_project_id uuid,
    candidate_run_id uuid
)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM agent_collaborative_runs run
        JOIN project_memberships membership
          ON membership.project_id = run.project_id
         AND membership.identity_id = sprout_private.current_identity_id()
         AND membership.state = 'active'
        WHERE run.project_id = candidate_project_id
          AND run.id = candidate_run_id
          AND sprout_private.can_access_resource(
              run.project_id, run.scope_resource_node_id, 'read'
          )
          AND (
              run.created_by_identity_id = membership.identity_id
              OR membership.role IN ('owner', 'admin')
              OR EXISTS (
                  SELECT 1 FROM agent_run_participants participant
                  WHERE participant.project_id = run.project_id
                    AND participant.run_id = run.id
                    AND participant.identity_id = membership.identity_id
              )
              OR EXISTS (
                  SELECT 1
                  FROM agent_run_participants participant
                  JOIN governed_agents agent
                    ON agent.project_id = participant.project_id
                   AND agent.principal_identity_id = participant.identity_id
                  WHERE participant.project_id = run.project_id
                    AND participant.run_id = run.id
                    AND agent.controller_identity_id = membership.identity_id
              )
          )
    )
$$;

REVOKE ALL ON FUNCTION sprout_private.agent_run_access(uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.agent_run_access(uuid, uuid) TO PUBLIC;

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'agent_collaborative_runs', 'agent_run_participants',
        'agent_run_work_slots', 'agent_run_claim_leases', 'agent_run_blockers',
        'agent_run_transitions', 'agent_run_work_product_bindings',
        'agent_run_work_outcomes',
        'agent_run_blocker_resolutions',
        'agent_run_evidence_provenance', 'agent_run_causal_links'
    ]
    LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
    END LOOP;
END;
$$;

CREATE POLICY agent_runs_access ON agent_collaborative_runs
    USING (sprout_private.agent_run_access(project_id, id))
    WITH CHECK (
        sprout_private.can_access_resource(project_id, scope_resource_node_id, 'write')
        AND sprout_private.is_project_member(project_id)
    );

CREATE POLICY agent_run_participants_access ON agent_run_participants
    USING (sprout_private.agent_run_access(project_id, run_id))
    WITH CHECK (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_run_slots_access ON agent_run_work_slots
    USING (sprout_private.agent_run_access(project_id, run_id))
    WITH CHECK (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_run_claims_access ON agent_run_claim_leases
    USING (sprout_private.agent_run_access(project_id, run_id))
    WITH CHECK (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_run_blockers_access ON agent_run_blockers
    USING (sprout_private.agent_run_access(project_id, run_id))
    WITH CHECK (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_run_transitions_access ON agent_run_transitions
    USING (sprout_private.agent_run_access(project_id, run_id))
    WITH CHECK (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_run_work_outcomes_access ON agent_run_work_outcomes
    USING (sprout_private.agent_run_access(project_id, run_id))
    WITH CHECK (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_run_work_bindings_access ON agent_run_work_product_bindings
    USING (sprout_private.agent_run_access(project_id, run_id))
    WITH CHECK (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_run_blocker_resolutions_access ON agent_run_blocker_resolutions
    USING (sprout_private.agent_run_access(project_id, run_id))
    WITH CHECK (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_run_evidence_access ON agent_run_evidence_provenance
    USING (sprout_private.agent_run_access(project_id, run_id))
    WITH CHECK (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_run_causal_access ON agent_run_causal_links
    USING (sprout_private.agent_run_access(project_id, run_id))
    WITH CHECK (sprout_private.agent_run_access(project_id, run_id));

CREATE FUNCTION sprout_private.agent_kernel_retention_marked(
    candidate_run_id uuid,
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
        NULLIF(current_setting('app.agent_kernel_retention_run_ids', true), '')::jsonb,
        '[]'::jsonb
    );
    RETURN marked ? candidate_run_id::text
       AND sprout_private.retention_purge_row_allowed(jsonb_build_object(
           'project_id', candidate_project_id,
           'resource_node_id', NULLIF(current_setting(
               'app.agent_kernel_retention_resource_id', true
           ), '')::uuid
       ));
EXCEPTION
    WHEN invalid_text_representation THEN RETURN false;
END;
$$;

CREATE FUNCTION sprout_private.reject_agent_kernel_history_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    row_data jsonb := to_jsonb(OLD);
BEGIN
    IF TG_OP = 'DELETE' AND sprout_private.agent_kernel_retention_marked(
        NULLIF(row_data ->> 'run_id', '')::uuid,
        NULLIF(row_data ->> 'project_id', '')::uuid
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'agent kernel certificate/history is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE FUNCTION sprout_private.reject_agent_run_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' AND sprout_private.agent_kernel_retention_marked(
        OLD.id, OLD.project_id
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'agent run deletion requires authorized retention'
        USING ERRCODE = '55000';
END;
$$;

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'agent_run_participants', 'agent_run_work_slots',
        'agent_run_transitions', 'agent_run_work_product_bindings',
        'agent_run_work_outcomes',
        'agent_run_blocker_resolutions',
        'agent_run_evidence_provenance', 'agent_run_causal_links'
    ]
    LOOP
        EXECUTE format(
            'CREATE TRIGGER %I_append_only BEFORE UPDATE OR DELETE ON %I
             FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_kernel_history_mutation()',
            table_name, table_name
        );
    END LOOP;
END;
$$;

CREATE TRIGGER agent_collaborative_runs_delete_guard
BEFORE DELETE ON agent_collaborative_runs
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_run_delete();

CREATE FUNCTION sprout_private.validate_agent_run_blocker_projection()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND (
        NEW.id, NEW.project_id, NEW.run_id, NEW.obligation_id,
        NEW.waiting_rule_ordinal, NEW.scope, NEW.waiting_condition,
        NEW.created_tick, NEW.recorded_at
    ) IS DISTINCT FROM (
        OLD.id, OLD.project_id, OLD.run_id, OLD.obligation_id,
        OLD.waiting_rule_ordinal, OLD.scope, OLD.waiting_condition,
        OLD.created_tick, OLD.recorded_at
    ) THEN
        RAISE EXCEPTION 'agent blocker identity and provenance are immutable'
            USING ERRCODE = '55000';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM agent_collaborative_runs run
        JOIN agent_run_transitions transition
          ON transition.project_id = run.project_id
         AND transition.run_id = run.id
         AND transition.state_version = run.state_version
        WHERE run.project_id = NEW.project_id
          AND run.id = NEW.run_id
          AND transition.state_snapshot #>> ARRAY[
              'blockers', NEW.id::text, 'status'
          ] = NEW.current_status
          AND (transition.state_snapshot #>> ARRAY[
              'blockers', NEW.id::text, 'created_at'
          ])::bigint = NEW.created_tick
          AND (
              (NEW.terminal_tick IS NULL AND transition.state_snapshot #> ARRAY[
                  'blockers', NEW.id::text, 'terminal_at'
              ] = 'null'::jsonb)
              OR (transition.state_snapshot #>> ARRAY[
                  'blockers', NEW.id::text, 'terminal_at'
              ])::bigint = NEW.terminal_tick
          )
          AND (
              NEW.current_status = 'waiting'
              OR EXISTS (
                  SELECT 1 FROM agent_run_blocker_resolutions resolution
                  WHERE resolution.project_id = NEW.project_id
                    AND resolution.run_id = NEW.run_id
                    AND resolution.blocker_id = NEW.id
                    AND resolution.terminal_status = NEW.current_status
                    AND resolution.transition_id = transition.id
              )
          )
    ) THEN
        RAISE EXCEPTION 'agent blocker status is not backed by the current domain transition'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_blocker_resolution_certificate()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM agent_run_transitions transition
        WHERE transition.id = NEW.transition_id
          AND transition.project_id = NEW.project_id
          AND transition.run_id = NEW.run_id
          AND transition.observation_kind = NEW.observation_kind
          AND transition.observation_id = NEW.observation_id
          AND transition.state_snapshot #>> ARRAY[
              'blockers', NEW.blocker_id::text, 'status'
          ] = NEW.terminal_status
          AND EXISTS (
              SELECT 1
              FROM jsonb_array_elements(
                  transition.state_snapshot -> 'blocker_resolutions'
              ) certificate
              WHERE certificate ->> 'blocker' = NEW.blocker_id::text
                AND certificate ->> 'terminal_status' = NEW.terminal_status
          )
    ) THEN
        RAISE EXCEPTION 'blocker resolution is not backed by its domain transition'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_run_blockers_projection_guard
BEFORE INSERT OR UPDATE ON agent_run_blockers
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_run_blocker_projection();

CREATE TRIGGER agent_run_blocker_resolution_certificate_guard
BEFORE INSERT ON agent_run_blocker_resolutions
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_blocker_resolution_certificate();

CREATE FUNCTION sprout_private.validate_agent_run_work_product_binding()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM agent_run_claim_leases claim
        JOIN agent_run_transitions transition
          ON transition.id = NEW.claim_transition_id
         AND transition.project_id = NEW.project_id
         AND transition.run_id = NEW.run_id
         AND transition.transition_kind = 'work_claimed'
         AND transition.state_snapshot #>> ARRAY[
             'claims', NEW.claim_id::text, 'status'
         ] = 'active'
        JOIN agent_invocations invocation
          ON invocation.project_id = NEW.project_id
         AND invocation.id = NEW.invocation_id
         AND invocation.agent_identity_id = claim.claimant_identity_id
         AND invocation.status = 'succeeded'
        JOIN agent_effect_proposals effect
          ON effect.id = NEW.effect_id
         AND effect.project_id = NEW.project_id
         AND effect.invocation_id = invocation.id
         AND effect.status = 'applied'
         AND effect.applied_at = NEW.bound_at
         AND effect.effect #>> '{effect,resource_id}' = NEW.resource_node_id::text
         AND effect.effect #>> '{effect,operation}' = 'complete_assigned_task'
        JOIN tasks task
          ON task.project_id = NEW.project_id
         AND task.resource_node_id = NEW.resource_node_id
        JOIN agent_collaborative_runs run
          ON run.project_id = NEW.project_id
         AND run.id = NEW.run_id
        JOIN resource_closure scope
          ON scope.project_id = run.project_id
         AND scope.ancestor_id = run.scope_resource_node_id
         AND scope.descendant_id = NEW.resource_node_id
        WHERE claim.project_id = NEW.project_id
          AND claim.id = NEW.claim_id
          AND claim.run_id = NEW.run_id
          AND claim.work_item_id = NEW.work_item_id
          AND claim.attempt = NEW.attempt
          AND claim.acquired_at <= NEW.bound_at
    ) THEN
        RAISE EXCEPTION 'work product binding lacks preexisting invocation/effect provenance'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_run_work_outcome()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.outcome_kind <> 'task_completion' OR NOT EXISTS (
        SELECT 1
        FROM agent_run_transitions transition
        JOIN agent_collaborative_runs run
          ON run.project_id = transition.project_id
         AND run.id = transition.run_id
        JOIN agent_run_claim_leases claim
          ON claim.project_id = NEW.project_id
         AND claim.id = NEW.claim_id
         AND claim.run_id = NEW.run_id
         AND claim.work_item_id = NEW.work_item_id
         AND claim.attempt = NEW.attempt
        JOIN agent_run_work_product_bindings binding
          ON binding.project_id = NEW.project_id
         AND binding.run_id = NEW.run_id
         AND binding.work_item_id = NEW.work_item_id
         AND binding.claim_id = NEW.claim_id
         AND binding.attempt = NEW.attempt
        JOIN task_completions completion
          ON completion.project_id = NEW.project_id
         AND completion.id = NEW.product_event_id
         AND completion.assignee_identity_id = claim.claimant_identity_id
         AND completion.completed_at = NEW.observed_at
         AND completion.completed_at >= claim.acquired_at
        JOIN tasks task
          ON task.project_id = completion.project_id
         AND task.id = completion.task_id
         AND task.state = 'completed'
         AND task.resource_node_id = binding.resource_node_id
        JOIN resource_closure scope
          ON scope.project_id = run.project_id
         AND scope.ancestor_id = run.scope_resource_node_id
         AND scope.descendant_id = task.resource_node_id
        WHERE transition.id = NEW.transition_id
          AND transition.project_id = NEW.project_id
          AND transition.run_id = NEW.run_id
          AND transition.transition_kind = 'work_succeeded'
          AND transition.observation_kind = NEW.outcome_kind
          AND transition.observation_id = NEW.product_event_id
          AND transition.state_snapshot #>> ARRAY[
              'work_items', NEW.work_item_id::text, 'status'
          ] = 'succeeded'
          AND transition.state_snapshot #>> ARRAY[
              'claims', NEW.claim_id::text, 'status'
          ] = 'released'
    ) THEN
        RAISE EXCEPTION 'work outcome is not backed by its authoritative product event and domain transition'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_run_evidence_certificate()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.evidence_kind <> 'task_completed'
       OR NEW.verification_mode <> 'mechanical'
       OR NEW.product_event_kind <> 'task_completion'
       OR NOT EXISTS (
        SELECT 1
        FROM agent_run_transitions transition
        JOIN agent_collaborative_runs run
          ON run.project_id = transition.project_id
         AND run.id = transition.run_id
        JOIN agent_run_work_outcomes outcome
          ON outcome.project_id = NEW.project_id
         AND outcome.run_id = NEW.run_id
         AND outcome.work_item_id = NEW.work_item_id
         AND outcome.outcome_kind = NEW.product_event_kind
         AND outcome.product_event_id = NEW.product_event_id
         AND outcome.observed_at = NEW.observed_at
        WHERE transition.id = NEW.transition_id
          AND transition.project_id = NEW.project_id
          AND transition.run_id = NEW.run_id
          AND transition.transition_kind = 'evidence_accepted'
          AND transition.observation_kind = NEW.product_event_kind
          AND transition.observation_id = NEW.product_event_id
          AND transition.state_snapshot #>> ARRAY[
              'obligations', NEW.obligation_id::text, 'status'
          ] = 'discharged'
          AND EXISTS (
              SELECT 1
              FROM jsonb_array_elements(transition.state_snapshot -> 'evidence') certificate
              WHERE certificate ->> 'id' = NEW.evidence_id::text
                AND certificate ->> 'obligation' = NEW.obligation_id::text
                AND (certificate ->> 'rule_id')::bigint = NEW.evidence_rule_ordinal
                AND certificate ->> 'kind' = NEW.evidence_kind
                AND certificate ->> 'work' = NEW.work_item_id::text
                AND (certificate ->> 'observed_at')::bigint =
                    extract(epoch FROM NEW.observed_at)::bigint
          )
          AND EXISTS (
              SELECT 1
              FROM jsonb_array_elements(run.contract -> 'evidence_rules') rule
              WHERE (rule ->> 'id')::bigint = NEW.evidence_rule_ordinal
                AND rule ->> 'obligation' = NEW.obligation_id::text
                AND rule ->> 'kind' = NEW.evidence_kind
                AND rule ->> 'verification' = NEW.verification_mode
                AND rule #>> '{subject,kind}' = 'work_result'
          )
    ) THEN
        RAISE EXCEPTION 'evidence certificate is not backed by the authoritative outcome and domain transition'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_run_work_outcomes_certificate_guard
BEFORE INSERT ON agent_run_work_outcomes
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_run_work_outcome();

CREATE TRIGGER agent_run_work_product_bindings_certificate_guard
BEFORE INSERT ON agent_run_work_product_bindings
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_run_work_product_binding();

CREATE TRIGGER agent_run_evidence_certificate_guard
BEFORE INSERT ON agent_run_evidence_provenance
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_run_evidence_certificate();

CREATE FUNCTION sprout_private.purge_agent_kernel_for_resource()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    run_ids uuid[] := ARRAY[]::uuid[];
BEGIN
    IF TG_OP <> 'DELETE' OR NOT sprout_private.retention_purge_row_allowed(
        jsonb_build_object('project_id', OLD.project_id, 'resource_node_id', OLD.id)
    ) THEN
        RAISE EXCEPTION 'invalid agent kernel retention provenance'
            USING ERRCODE = '55000';
    END IF;

    SELECT COALESCE(array_agg(DISTINCT run.id), ARRAY[]::uuid[])
    INTO run_ids
    FROM agent_collaborative_runs run
    LEFT JOIN agent_local_goal_contracts local
      ON local.project_id = run.project_id
     AND local.id = run.local_goal_id
     AND local.revision = run.local_goal_revision
    LEFT JOIN governed_agents agent
      ON agent.project_id = local.project_id
     AND agent.id = local.agent_id
    LEFT JOIN agent_global_contract_sources global_source
      ON global_source.project_id = run.project_id
     AND global_source.global_contract_id = run.global_contract_id
     AND global_source.global_revision = run.global_contract_revision
    LEFT JOIN agent_local_goal_contracts global_local
      ON global_local.project_id = global_source.project_id
     AND global_local.id = global_source.local_goal_id
     AND global_local.revision = global_source.local_revision
     AND global_local.agent_id = global_source.agent_id
    LEFT JOIN governed_agents global_agent
      ON global_agent.project_id = global_source.project_id
     AND global_agent.id = global_source.agent_id
    WHERE run.project_id = OLD.project_id
      AND (
          run.scope_resource_node_id = OLD.id
          OR agent.profile_resource_node_id = OLD.id
          OR global_agent.profile_resource_node_id = OLD.id
          OR global_local.contract #>> '{contract,scope}' = OLD.id::text
          OR EXISTS (
              SELECT 1
              FROM jsonb_array_elements(global_local.contract -> 'clauses') clause
              WHERE clause ->> 'scope' = OLD.id::text
          )
      );

    IF cardinality(run_ids) = 0 THEN
        RETURN OLD;
    END IF;

    PERFORM set_config('app.agent_kernel_retention_resource_id', OLD.id::text, true);
    PERFORM set_config(
        'app.agent_kernel_retention_run_ids', to_jsonb(run_ids)::text, true
    );

    DELETE FROM agent_run_blocker_resolutions
    WHERE project_id = OLD.project_id AND run_id = ANY(run_ids);
    DELETE FROM agent_run_evidence_provenance
    WHERE project_id = OLD.project_id AND run_id = ANY(run_ids);
    DELETE FROM agent_run_causal_links
    WHERE project_id = OLD.project_id AND run_id = ANY(run_ids);
    DELETE FROM agent_run_work_outcomes
    WHERE project_id = OLD.project_id AND run_id = ANY(run_ids);
    DELETE FROM agent_run_work_product_bindings
    WHERE project_id = OLD.project_id AND run_id = ANY(run_ids);
    DELETE FROM agent_run_transitions
    WHERE project_id = OLD.project_id AND run_id = ANY(run_ids);
    DELETE FROM agent_run_claim_leases
    WHERE project_id = OLD.project_id AND run_id = ANY(run_ids);
    DELETE FROM agent_run_blockers
    WHERE project_id = OLD.project_id AND run_id = ANY(run_ids);
    DELETE FROM agent_run_work_slots
    WHERE project_id = OLD.project_id AND run_id = ANY(run_ids);
    DELETE FROM agent_run_participants
    WHERE project_id = OLD.project_id AND run_id = ANY(run_ids);
    DELETE FROM agent_collaborative_runs
    WHERE project_id = OLD.project_id AND id = ANY(run_ids);

    RETURN OLD;
END;
$$;

CREATE TRIGGER resource_nodes_agent_kernel_purge
BEFORE DELETE ON resource_nodes
FOR EACH ROW EXECUTE FUNCTION sprout_private.purge_agent_kernel_for_resource();

REVOKE ALL ON FUNCTION sprout_private.agent_kernel_retention_marked(uuid, uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_kernel_history_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_run_delete() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_run_blocker_projection() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_blocker_resolution_certificate() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_run_work_product_binding() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_run_work_outcome() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_run_evidence_certificate() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.purge_agent_kernel_for_resource() FROM PUBLIC;
