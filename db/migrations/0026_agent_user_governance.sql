-- User-level governance and atomically bound prompt/LocalGoal revisions.
-- These records refine the existing permission/RLS/E2EE model; they do not
-- create principals, ACL entries, resource grants, or key material.

ALTER TABLE agent_responsibility_contracts
    ADD COLUMN state text NOT NULL DEFAULT 'active'
        CHECK (state IN ('draft', 'active', 'superseded')),
    ADD COLUMN activated_at timestamptz,
    ADD COLUMN superseded_at timestamptz;

UPDATE agent_responsibility_contracts
SET activated_at = recorded_at
WHERE state = 'active';

ALTER TABLE agent_responsibility_contracts
    ADD CONSTRAINT agent_responsibilities_lifecycle_shape CHECK (
        (state = 'draft' AND activated_at IS NULL AND superseded_at IS NULL)
        OR (state = 'active' AND activated_at IS NOT NULL AND superseded_at IS NULL)
        OR (state = 'superseded' AND activated_at IS NOT NULL AND superseded_at IS NOT NULL)
    );

CREATE UNIQUE INDEX agent_responsibilities_one_active_per_user_idx
    ON agent_responsibility_contracts (project_id, user_identity_id)
    WHERE state = 'active';
CREATE UNIQUE INDEX agent_responsibilities_one_draft_per_user_idx
    ON agent_responsibility_contracts (project_id, user_identity_id)
    WHERE state = 'draft';

ALTER TABLE agent_local_goal_contracts
    DROP CONSTRAINT agent_local_goals_terminal_shape;
ALTER TABLE agent_local_goal_contracts
    DROP CONSTRAINT agent_local_goal_contracts_state_check;
ALTER TABLE agent_local_goal_contracts
    ADD CONSTRAINT agent_local_goal_contracts_state_check
        CHECK (state IN ('draft', 'active', 'completed', 'failed', 'superseded')),
    ADD CONSTRAINT agent_local_goals_terminal_shape CHECK (
        (state IN ('draft', 'active') AND terminal_at IS NULL)
        OR (state IN ('completed', 'failed', 'superseded') AND terminal_at IS NOT NULL)
    );

CREATE UNIQUE INDEX agent_local_goals_one_draft_per_agent_idx
    ON agent_local_goal_contracts (project_id, agent_id)
    WHERE state = 'draft';

CREATE TABLE agent_prompt_revisions (
    project_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    local_goal_id uuid NOT NULL,
    local_goal_revision bigint NOT NULL CHECK (local_goal_revision > 0),
    encrypted_prompt bytea NOT NULL CHECK (octet_length(encrypted_prompt) > 0),
    prompt_hash bytea NOT NULL CHECK (octet_length(prompt_hash) = 32),
    state text NOT NULL CHECK (state IN ('draft', 'active', 'superseded')),
    approved_by_identity_id uuid,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    activated_at timestamptz,
    superseded_at timestamptz,
    PRIMARY KEY (project_id, agent_id, local_goal_id, local_goal_revision),
    CONSTRAINT agent_prompt_revisions_agent_fk
        FOREIGN KEY (project_id, agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_prompt_revisions_local_goal_fk
        FOREIGN KEY (project_id, local_goal_id, local_goal_revision, agent_id)
        REFERENCES agent_local_goal_contracts (project_id, id, revision, agent_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_prompt_revisions_approver_fk
        FOREIGN KEY (project_id, approved_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_prompt_revisions_lifecycle_shape CHECK (
        (state = 'draft' AND approved_by_identity_id IS NULL
            AND activated_at IS NULL AND superseded_at IS NULL)
        OR (state = 'active' AND approved_by_identity_id IS NOT NULL
            AND activated_at IS NOT NULL AND superseded_at IS NULL)
        OR (state = 'superseded' AND approved_by_identity_id IS NOT NULL
            AND activated_at IS NOT NULL AND superseded_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX agent_prompt_revisions_one_active_per_agent_idx
    ON agent_prompt_revisions (project_id, agent_id)
    WHERE state = 'active';
CREATE UNIQUE INDEX agent_prompt_revisions_one_draft_per_agent_idx
    ON agent_prompt_revisions (project_id, agent_id)
    WHERE state = 'draft';

CREATE TABLE agent_user_governance_audit_log (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    sequence bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    project_id uuid NOT NULL,
    subject_user_identity_id uuid NOT NULL,
    agent_id uuid,
    actor_identity_id uuid NOT NULL,
    actor_device_id uuid NOT NULL,
    event_kind text NOT NULL CHECK (event_kind IN (
        'responsibility_drafted', 'responsibility_activated',
        'local_goal_drafted', 'local_goal_activated',
        'cross_owner_routed', 'cross_owner_decided', 'cross_owner_ready'
    )),
    facts jsonb NOT NULL CHECK (jsonb_typeof(facts) = 'object'),
    previous_hash bytea CHECK (previous_hash IS NULL OR octet_length(previous_hash) = 32),
    entry_hash bytea NOT NULL CHECK (octet_length(entry_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_user_governance_audit_subject_fk
        FOREIGN KEY (project_id, subject_user_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_user_governance_audit_actor_device_fk
        FOREIGN KEY (actor_identity_id, actor_device_id)
        REFERENCES devices (identity_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE INDEX agent_user_governance_audit_subject_idx
    ON agent_user_governance_audit_log (
        project_id, subject_user_identity_id, sequence
    );

CREATE TABLE agent_task_intents (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    task_resource_node_id uuid NOT NULL,
    scope_resource_node_id uuid NOT NULL,
    required_actions jsonb NOT NULL CHECK (jsonb_typeof(required_actions) = 'array'),
    derived_by_identity_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_task_intents_task_fk
        FOREIGN KEY (project_id, task_resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_intents_scope_fk
        FOREIGN KEY (project_id, scope_resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_intents_deriver_fk
        FOREIGN KEY (project_id, derived_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_intents_project_id_unique UNIQUE (project_id, id)
);

CREATE TABLE agent_task_obligation_provenance (
    project_id uuid NOT NULL,
    task_intent_id uuid NOT NULL,
    task_resource_node_id uuid NOT NULL,
    target_agent_id uuid NOT NULL,
    local_goal_id uuid NOT NULL,
    local_goal_revision bigint NOT NULL CHECK (local_goal_revision > 0),
    obligation_id uuid NOT NULL,
    work_spec_ordinal bigint NOT NULL CHECK (work_spec_ordinal >= 0),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, task_intent_id),
    CONSTRAINT agent_task_obligation_intent_fk
        FOREIGN KEY (project_id, task_intent_id)
        REFERENCES agent_task_intents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_obligation_task_fk
        FOREIGN KEY (project_id, task_resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_obligation_agent_fk
        FOREIGN KEY (project_id, target_agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_obligation_local_fk
        FOREIGN KEY (project_id, local_goal_id, local_goal_revision, target_agent_id)
        REFERENCES agent_local_goal_contracts (project_id, id, revision, agent_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE TABLE agent_cross_owner_assignments (
    id uuid NOT NULL,
    project_id uuid NOT NULL,
    task_resource_node_id uuid NOT NULL,
    requester_identity_id uuid NOT NULL,
    target_agent_id uuid NOT NULL,
    target_controller_identity_id uuid NOT NULL,
    task_intent_id uuid NOT NULL,
    route text NOT NULL CHECK (route IN (
        'automatic_existing_obligation', 'controller_review', 'rejected'
    )),
    review_task_resource_node_id uuid,
    responsibility_id uuid,
    responsibility_revision bigint,
    decision text CHECK (decision IN ('approved', 'rejected')),
    state text NOT NULL CHECK (state IN (
        'ready', 'pending_review', 'approved_pending_mandate', 'rejected'
    )),
    requested_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    decided_at timestamptz,
    PRIMARY KEY (project_id, id),
    CONSTRAINT agent_cross_owner_task_fk
        FOREIGN KEY (project_id, task_resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_owner_requester_fk
        FOREIGN KEY (project_id, requester_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_owner_target_fk
        FOREIGN KEY (project_id, target_agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_owner_controller_fk
        FOREIGN KEY (project_id, target_controller_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_owner_intent_fk
        FOREIGN KEY (project_id, task_intent_id)
        REFERENCES agent_task_intents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_owner_review_task_fk
        FOREIGN KEY (project_id, review_task_resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_owner_responsibility_fk
        FOREIGN KEY (project_id, responsibility_id, responsibility_revision)
        REFERENCES agent_responsibility_contracts (project_id, id, revision)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_owner_responsibility_shape CHECK (
        (responsibility_id IS NULL) = (responsibility_revision IS NULL)
    ),
    CONSTRAINT agent_cross_owner_review_shape CHECK (
        (route = 'controller_review' AND review_task_resource_node_id IS NOT NULL)
        OR (route <> 'controller_review' AND review_task_resource_node_id IS NULL)
    ),
    CONSTRAINT agent_cross_owner_decision_shape CHECK (
        (decision IS NULL AND decided_at IS NULL
            AND state IN ('ready', 'pending_review', 'rejected'))
        OR (decision = 'approved' AND decided_at IS NOT NULL
            AND state IN ('approved_pending_mandate', 'ready'))
        OR (decision = 'rejected' AND decided_at IS NOT NULL AND state = 'rejected')
    )
);

ALTER TABLE agent_prompt_revisions ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_prompt_revisions FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_task_intents ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_task_intents FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_task_obligation_provenance ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_task_obligation_provenance FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_cross_owner_assignments ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_cross_owner_assignments FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_user_governance_audit_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_user_governance_audit_log FORCE ROW LEVEL SECURITY;

CREATE POLICY agent_prompt_revision_parties ON agent_prompt_revisions
    USING (EXISTS (
        SELECT 1 FROM governed_agents agent
        WHERE agent.project_id = agent_prompt_revisions.project_id
          AND agent.id = agent_prompt_revisions.agent_id
          AND (
              agent.principal_identity_id = sprout_private.current_identity_id()
              OR agent.controller_identity_id = sprout_private.current_identity_id()
              OR EXISTS (
                  SELECT 1 FROM project_memberships membership
                  WHERE membership.project_id = agent.project_id
                    AND membership.identity_id = sprout_private.current_identity_id()
                    AND membership.state = 'active'
                    AND membership.role IN ('owner', 'admin')
              )
          )
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM governed_agents agent
        WHERE agent.project_id = agent_prompt_revisions.project_id
          AND agent.id = agent_prompt_revisions.agent_id
          AND agent.controller_identity_id = sprout_private.current_identity_id()
    ));

CREATE POLICY agent_task_intent_requester_access ON agent_task_intents
    USING (derived_by_identity_id = sprout_private.current_identity_id())
    WITH CHECK (derived_by_identity_id = sprout_private.current_identity_id());

CREATE POLICY agent_task_obligation_parties ON agent_task_obligation_provenance
    USING (EXISTS (
        SELECT 1 FROM agent_cross_owner_assignments assignment
        WHERE assignment.project_id = agent_task_obligation_provenance.project_id
          AND assignment.task_intent_id = agent_task_obligation_provenance.task_intent_id
          AND (
              assignment.requester_identity_id = sprout_private.current_identity_id()
              OR assignment.target_controller_identity_id = sprout_private.current_identity_id()
          )
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM governed_agents agent
        WHERE agent.project_id = agent_task_obligation_provenance.project_id
          AND agent.id = agent_task_obligation_provenance.target_agent_id
          AND agent.controller_identity_id = sprout_private.current_identity_id()
    ));

CREATE FUNCTION sprout_private.reject_task_obligation_provenance_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    retention_resource uuid := NULLIF(
        current_setting('app.agent_retention_resource_id', true), ''
    )::uuid;
BEGIN
    IF TG_OP = 'DELETE'
       AND retention_resource IS NOT NULL
       AND sprout_private.retention_purge_row_allowed(jsonb_build_object(
           'project_id', OLD.project_id,
           'resource_node_id', retention_resource
       ))
       AND (
           OLD.task_resource_node_id = retention_resource
           OR EXISTS (
               SELECT 1 FROM governed_agents agent
               WHERE agent.project_id = OLD.project_id
                 AND agent.id = OLD.target_agent_id
                 AND agent.profile_resource_node_id = retention_resource
           )
       )
    THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'task obligation provenance is append-only'
        USING ERRCODE = '55000';
EXCEPTION WHEN invalid_text_representation THEN
    RAISE EXCEPTION 'task obligation provenance is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_task_obligation_provenance_append_only
BEFORE UPDATE OR DELETE ON agent_task_obligation_provenance
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_task_obligation_provenance_mutation();

CREATE POLICY agent_cross_owner_parties ON agent_cross_owner_assignments
    USING (
        requester_identity_id = sprout_private.current_identity_id()
        OR target_controller_identity_id = sprout_private.current_identity_id()
    )
    WITH CHECK (
        requester_identity_id = sprout_private.current_identity_id()
        OR target_controller_identity_id = sprout_private.current_identity_id()
    );

CREATE POLICY agent_user_governance_audit_parties
    ON agent_user_governance_audit_log
    USING (
        subject_user_identity_id = sprout_private.current_identity_id()
        OR EXISTS (
            SELECT 1 FROM project_memberships membership
            WHERE membership.project_id = agent_user_governance_audit_log.project_id
              AND membership.identity_id = sprout_private.current_identity_id()
              AND membership.state = 'active'
              AND membership.role IN ('owner', 'admin')
        )
    )
    WITH CHECK (
        subject_user_identity_id = sprout_private.current_identity_id()
        OR EXISTS (
            SELECT 1 FROM project_memberships membership
            WHERE membership.project_id = agent_user_governance_audit_log.project_id
              AND membership.identity_id = sprout_private.current_identity_id()
              AND membership.state = 'active'
              AND membership.role IN ('owner', 'admin')
        )
    );

CREATE FUNCTION sprout_private.reject_user_governance_audit_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'user governance audit records are append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_user_governance_audit_append_only
BEFORE UPDATE OR DELETE ON agent_user_governance_audit_log
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_user_governance_audit_mutation();

-- A Responsibility belongs to the human controller rather than to any resource
-- mentioned by one of its rules. The historical retention trigger selects
-- responsibility rows by JSON scope, so suppress that derived DELETE only when
-- it is part of an already-authorized purge of the exact current resource.
CREATE FUNCTION sprout_private.preserve_user_responsibility_on_agent_purge()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    retention_resource uuid := NULLIF(
        current_setting('app.agent_retention_resource_id', true), ''
    )::uuid;
BEGIN
    IF TG_OP = 'DELETE'
       AND retention_resource IS NOT NULL
       AND sprout_private.retention_purge_row_allowed(jsonb_build_object(
           'project_id', OLD.project_id,
           'resource_node_id', retention_resource
       ))
    THEN
        RETURN NULL;
    END IF;
    RETURN OLD;
EXCEPTION WHEN invalid_text_representation THEN
    RETURN OLD;
END;
$$;

CREATE TRIGGER aa_responsibility_preserve_agent_purge
BEFORE DELETE ON agent_responsibility_contracts
FOR EACH ROW EXECUTE FUNCTION sprout_private.preserve_user_responsibility_on_agent_purge();

-- New agent-scoped governance children are removed explicitly before the
-- historical 0024 retention trigger deletes LocalGoal/agent rows. User-level
-- Responsibility and its audit chain deliberately remain outside this set.
CREATE FUNCTION sprout_private.purge_agent_user_governance_for_resource()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    purged_agent_ids uuid[] := ARRAY[]::uuid[];
    purged_local_goal_ids uuid[] := ARRAY[]::uuid[];
BEGIN
    IF TG_OP <> 'DELETE' OR NOT sprout_private.retention_purge_row_allowed(
        jsonb_build_object('project_id', OLD.project_id, 'resource_node_id', OLD.id)
    ) THEN
        RETURN OLD;
    END IF;
    PERFORM set_config('app.agent_retention_resource_id', OLD.id::text, true);

    SELECT COALESCE(array_agg(agent.id), ARRAY[]::uuid[])
    INTO purged_agent_ids
    FROM governed_agents agent
    WHERE agent.project_id = OLD.project_id
      AND agent.profile_resource_node_id = OLD.id;

    SELECT COALESCE(array_agg(DISTINCT local.id), ARRAY[]::uuid[])
    INTO purged_local_goal_ids
    FROM agent_local_goal_contracts local
    WHERE local.project_id = OLD.project_id
      AND (
          local.agent_id = ANY(purged_agent_ids)
          OR local.contract #>> '{contract,scope}' = OLD.id::text
          OR EXISTS (
              SELECT 1 FROM jsonb_array_elements(local.contract -> 'clauses') clause
              WHERE clause ->> 'scope' = OLD.id::text
          )
      );

    DELETE FROM agent_cross_owner_assignments assignment
    WHERE assignment.project_id = OLD.project_id
      AND (
          assignment.target_agent_id = ANY(purged_agent_ids)
          OR assignment.task_resource_node_id = OLD.id
          OR assignment.review_task_resource_node_id = OLD.id
      );
    DELETE FROM agent_task_obligation_provenance provenance
    WHERE provenance.project_id = OLD.project_id
      AND (
          provenance.target_agent_id = ANY(purged_agent_ids)
          OR provenance.task_resource_node_id = OLD.id
          OR provenance.local_goal_id = ANY(purged_local_goal_ids)
      );
    DELETE FROM agent_task_intents intent
    WHERE intent.project_id = OLD.project_id
      AND (intent.task_resource_node_id = OLD.id OR intent.scope_resource_node_id = OLD.id)
      AND NOT EXISTS (
          SELECT 1 FROM agent_cross_owner_assignments assignment
          WHERE assignment.project_id = intent.project_id
            AND assignment.task_intent_id = intent.id
      );
    DELETE FROM agent_prompt_revisions prompt
    WHERE prompt.project_id = OLD.project_id
      AND (
          prompt.agent_id = ANY(purged_agent_ids)
          OR prompt.local_goal_id = ANY(purged_local_goal_ids)
      );
    RETURN OLD;
END;
$$;

CREATE TRIGGER resource_nodes_agent_governance_purge
BEFORE DELETE ON resource_nodes
FOR EACH ROW EXECUTE FUNCTION sprout_private.purge_agent_user_governance_for_resource();

-- Narrow RLS-safe projection for a foreign requester. This is not an ACL: it
-- returns only structural governance metadata after rechecking the existing
-- permission engine and never grants access to agent plaintext or keys.
CREATE FUNCTION sprout_private.cross_owner_routing_snapshot(
    candidate_project_id uuid,
    candidate_task_resource_id uuid,
    candidate_target_agent_id uuid
)
RETURNS TABLE (
    principal_identity_id uuid,
    controller_identity_id uuid,
    availability text,
    controller_is_administrator boolean,
    responsibility_contract jsonb,
    automatic_contract jsonb,
    automatic_state jsonb,
    automatic_local_contract jsonb,
    automatic_work_item_id uuid,
    automatic_bound_at timestamptz
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    caller uuid := sprout_private.current_identity_id();
    permission_level text;
    permission_scope text;
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM project_memberships membership
        JOIN identities identity ON identity.id = membership.identity_id
        WHERE membership.project_id = candidate_project_id
          AND membership.identity_id = caller
          AND membership.state = 'active' AND identity.status = 'active'
          AND identity.principal_kind = 'user'
          AND membership.role NOT IN ('owner', 'admin')
    ) THEN
        RAISE EXCEPTION 'normal project user required' USING ERRCODE = '42501';
    END IF;
    SELECT permission.access_level, permission.access_scope
    INTO permission_level, permission_scope
    FROM sprout_private.effective_domain_permission(
        candidate_project_id, candidate_task_resource_id, caller
    ) permission;
    IF permission_level IS DISTINCT FROM 'manage'
       OR permission_scope IS DISTINCT FROM 'full'
       OR NOT EXISTS (
           SELECT 1 FROM tasks task
           WHERE task.project_id = candidate_project_id
             AND task.resource_node_id = candidate_task_resource_id
             AND task.deleted_at IS NULL
             AND task.state IN ('open', 'completed')
       )
    THEN
        RAISE EXCEPTION 'current task manage permission required'
            USING ERRCODE = '42501';
    END IF;
    RETURN QUERY
    SELECT agent.principal_identity_id,
           agent.controller_identity_id,
           agent.availability,
           controller.role IN ('owner', 'admin'),
           responsibility.contract,
           automatic.contract,
           automatic.state,
           automatic.local_contract,
           automatic.work_item_id,
           automatic.bound_at
    FROM governed_agents agent
    JOIN project_memberships controller
      ON controller.project_id = agent.project_id
     AND controller.identity_id = agent.controller_identity_id
     AND controller.state = 'active'
    LEFT JOIN agent_responsibility_contracts responsibility
      ON responsibility.project_id = agent.project_id
     AND responsibility.user_identity_id = agent.controller_identity_id
     AND responsibility.state = 'active'
    LEFT JOIN LATERAL (
        SELECT run.contract, run.state,
               local.contract AS local_contract,
               binding.work_item_id, binding.bound_at
        FROM agent_run_work_product_bindings binding
        JOIN agent_collaborative_runs run
          ON run.project_id = binding.project_id AND run.id = binding.run_id
        JOIN agent_local_goal_contracts local
          ON local.project_id = run.project_id
         AND local.id = run.local_goal_id
         AND local.revision = run.local_goal_revision
        WHERE binding.project_id = candidate_project_id
          AND binding.resource_node_id = candidate_task_resource_id
          AND local.agent_id = candidate_target_agent_id
          AND local.state = 'active'
          AND run.goal_status = 'active' AND run.run_status = 'running'
        ORDER BY binding.bound_at DESC
        LIMIT 1
    ) automatic ON true
    WHERE agent.project_id = candidate_project_id
      AND agent.id = candidate_target_agent_id
      AND agent.state = 'active'
      AND agent.controller_identity_id <> caller;
END;
$$;

CREATE FUNCTION sprout_private.cross_owner_active_mandate_snapshot(
    candidate_project_id uuid,
    candidate_assignment_id uuid
)
RETURNS TABLE (
    task_resource_node_id uuid,
    task_intent_id uuid,
    intent_required_actions jsonb,
    intent_recorded_at timestamptz,
    target_agent_id uuid,
    target_controller_identity_id uuid,
    target_principal_identity_id uuid,
    target_availability text,
    local_contract jsonb,
    obligation_id uuid,
    work_spec_ordinal bigint,
    provenance_recorded_at timestamptz,
    exact_prompt boolean
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    caller uuid := sprout_private.current_identity_id();
    permission_level text;
    permission_scope text;
    task_resource uuid;
BEGIN
    SELECT assignment.task_resource_node_id
    INTO task_resource
    FROM agent_cross_owner_assignments assignment
    WHERE assignment.project_id = candidate_project_id
      AND assignment.id = candidate_assignment_id
      AND assignment.requester_identity_id = caller
      AND assignment.route = 'controller_review'
      AND assignment.decision = 'approved'
      AND assignment.state = 'approved_pending_mandate';
    IF task_resource IS NULL THEN
        RAISE EXCEPTION 'approved cross-owner request required'
            USING ERRCODE = '40001';
    END IF;
    SELECT permission.access_level, permission.access_scope
    INTO permission_level, permission_scope
    FROM sprout_private.effective_domain_permission(
        candidate_project_id, task_resource, caller
    ) permission;
    IF permission_level IS DISTINCT FROM 'manage'
       OR permission_scope IS DISTINCT FROM 'full'
    THEN
        RAISE EXCEPTION 'current task manage permission required'
            USING ERRCODE = '42501';
    END IF;
    RETURN QUERY
    SELECT assignment.task_resource_node_id,
           assignment.task_intent_id,
           intent.required_actions,
           intent.recorded_at,
           assignment.target_agent_id,
           assignment.target_controller_identity_id,
           agent.principal_identity_id,
           agent.availability,
           local.contract,
           provenance.obligation_id,
           provenance.work_spec_ordinal,
           provenance.recorded_at,
           prompt.encrypted_prompt = agent.encrypted_system_prompt
    FROM agent_cross_owner_assignments assignment
    JOIN agent_task_intents intent
      ON intent.project_id = assignment.project_id
     AND intent.id = assignment.task_intent_id
     AND intent.task_resource_node_id = assignment.task_resource_node_id
    JOIN agent_task_obligation_provenance provenance
      ON provenance.project_id = assignment.project_id
     AND provenance.task_intent_id = assignment.task_intent_id
     AND provenance.task_resource_node_id = assignment.task_resource_node_id
     AND provenance.target_agent_id = assignment.target_agent_id
    JOIN governed_agents agent
      ON agent.project_id = assignment.project_id
     AND agent.id = assignment.target_agent_id
     AND agent.controller_identity_id = assignment.target_controller_identity_id
     AND agent.state = 'active'
    JOIN agent_local_goal_contracts local
      ON local.project_id = agent.project_id AND local.agent_id = agent.id
     AND local.id = provenance.local_goal_id
     AND local.revision = provenance.local_goal_revision
     AND local.state = 'active'
    JOIN agent_prompt_revisions prompt
      ON prompt.project_id = local.project_id
     AND prompt.agent_id = local.agent_id
     AND prompt.local_goal_id = local.id
     AND prompt.local_goal_revision = local.revision
     AND prompt.state = 'active'
    WHERE assignment.project_id = candidate_project_id
      AND assignment.id = candidate_assignment_id
      AND assignment.requester_identity_id = caller
      AND assignment.decision = 'approved'
      AND assignment.state = 'approved_pending_mandate';
END;
$$;

REVOKE ALL ON FUNCTION sprout_private.preserve_user_responsibility_on_agent_purge() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_user_governance_audit_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_task_obligation_provenance_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.purge_agent_user_governance_for_resource() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.cross_owner_routing_snapshot(uuid, uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.cross_owner_routing_snapshot(uuid, uuid, uuid) TO PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.cross_owner_active_mandate_snapshot(uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.cross_owner_active_mandate_snapshot(uuid, uuid) TO PUBLIC;
