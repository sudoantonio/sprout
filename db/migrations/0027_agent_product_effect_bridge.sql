-- Authoritative bridge from R5 governance/kernel certificates to concrete
-- product effects. These rows certify effects already applied atomically by
-- the server; they are not a second permission or governance state machine.

ALTER TABLE agent_user_governance_audit_log
    DROP CONSTRAINT agent_user_governance_audit_log_event_kind_check,
    ADD CONSTRAINT agent_user_governance_audit_log_event_kind_check CHECK (event_kind IN (
        'responsibility_drafted', 'responsibility_activated',
        'local_goal_drafted', 'local_goal_activated',
        'cross_owner_routed', 'cross_owner_decided', 'cross_owner_ready',
        'cross_owner_materialized'
    ));

-- TaskObligationProvenance is a general causal certificate in R5.37G.  A
-- cross-owner TaskIntent may be one of its sources, but is not its identity and
-- is not required for ordinary task work.  Existing 0026 rows retain their
-- stable identity by adopting the former task-intent id.
ALTER TABLE agent_task_obligation_provenance
    ADD COLUMN id uuid;
UPDATE agent_task_obligation_provenance
SET id = task_intent_id;
ALTER TABLE agent_task_obligation_provenance
    ALTER COLUMN id SET DEFAULT gen_random_uuid(),
    ALTER COLUMN id SET NOT NULL,
    DROP CONSTRAINT agent_task_obligation_provenance_pkey;
ALTER TABLE agent_task_obligation_provenance
    ALTER COLUMN task_intent_id DROP NOT NULL,
    ADD CONSTRAINT agent_task_obligation_provenance_pkey
        PRIMARY KEY (project_id, id),
    ADD CONSTRAINT agent_task_obligation_provenance_intent_unique
        UNIQUE (project_id, task_intent_id),
    ADD CONSTRAINT agent_task_obligation_provenance_project_id_unique
        UNIQUE (project_id, id);

-- Concrete witness for ControllerApprovalMatchesDraft (R5.35): the server
-- assigns a stable draft identity and persists the exact controller/agent,
-- ciphertext hash and LocalGoal revision approved at activation. The
-- certificate intentionally has no destructive FK to agent-scoped rows so
-- governance history survives authorized retention.
ALTER TABLE agent_prompt_revisions
    ADD COLUMN draft_id uuid NOT NULL DEFAULT gen_random_uuid(),
    ADD CONSTRAINT agent_prompt_revisions_draft_unique
        UNIQUE (project_id, draft_id);

CREATE TABLE agent_prompt_final_approvals (
    project_id uuid NOT NULL,
    draft_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    controller_identity_id uuid NOT NULL,
    local_goal_id uuid NOT NULL,
    local_goal_revision bigint NOT NULL CHECK (local_goal_revision > 0),
    prompt_hash bytea NOT NULL CHECK (octet_length(prompt_hash) = 32),
    approved_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, draft_id)
);

CREATE FUNCTION sprout_private.validate_prompt_final_approval()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM agent_prompt_revisions prompt
        JOIN governed_agents agent
          ON agent.project_id = prompt.project_id
         AND agent.id = prompt.agent_id
         AND agent.controller_identity_id = NEW.controller_identity_id
        WHERE prompt.project_id = NEW.project_id
          AND prompt.draft_id = NEW.draft_id
          AND prompt.agent_id = NEW.agent_id
          AND prompt.local_goal_id = NEW.local_goal_id
          AND prompt.local_goal_revision = NEW.local_goal_revision
          AND prompt.prompt_hash = NEW.prompt_hash
          AND prompt.approved_by_identity_id = NEW.controller_identity_id
          AND prompt.state = 'active'
    ) THEN
        RAISE EXCEPTION 'final prompt approval does not match the exact active draft'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_prompt_final_approval_exact_draft
BEFORE INSERT ON agent_prompt_final_approvals
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_prompt_final_approval();

INSERT INTO agent_prompt_final_approvals (
    project_id, draft_id, agent_id, controller_identity_id,
    local_goal_id, local_goal_revision, prompt_hash, approved_at
)
SELECT prompt.project_id, prompt.draft_id, prompt.agent_id,
       prompt.approved_by_identity_id, prompt.local_goal_id,
       prompt.local_goal_revision, prompt.prompt_hash, prompt.activated_at
FROM agent_prompt_revisions prompt
WHERE prompt.state = 'active';

CREATE FUNCTION sprout_private.require_active_prompt_final_approval()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state = 'active' AND NOT EXISTS (
        SELECT 1
        FROM agent_prompt_final_approvals approval
        WHERE approval.project_id = NEW.project_id
          AND approval.draft_id = NEW.draft_id
          AND approval.agent_id = NEW.agent_id
          AND approval.controller_identity_id = NEW.approved_by_identity_id
          AND approval.local_goal_id = NEW.local_goal_id
          AND approval.local_goal_revision = NEW.local_goal_revision
          AND approval.prompt_hash = NEW.prompt_hash
    ) THEN
        RAISE EXCEPTION 'active prompt requires its exact final approval certificate'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE CONSTRAINT TRIGGER agent_prompt_active_approval_certificate
AFTER INSERT OR UPDATE ON agent_prompt_revisions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION sprout_private.require_active_prompt_final_approval();

ALTER TABLE agent_prompt_final_approvals ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_prompt_final_approvals FORCE ROW LEVEL SECURITY;
CREATE POLICY agent_prompt_final_approval_parties ON agent_prompt_final_approvals
    USING (
        controller_identity_id = sprout_private.current_identity_id()
        OR EXISTS (
            SELECT 1 FROM governed_agents agent
            WHERE agent.project_id = agent_prompt_final_approvals.project_id
              AND agent.id = agent_prompt_final_approvals.agent_id
              AND agent.principal_identity_id = sprout_private.current_identity_id()
        )
        OR EXISTS (
            SELECT 1 FROM project_memberships membership
            WHERE membership.project_id = agent_prompt_final_approvals.project_id
              AND membership.identity_id = sprout_private.current_identity_id()
              AND membership.state = 'active'
              AND membership.role IN ('owner', 'admin')
        )
    )
    WITH CHECK (controller_identity_id = sprout_private.current_identity_id());

CREATE TRIGGER agent_prompt_final_approval_append_only
BEFORE UPDATE OR DELETE ON agent_prompt_final_approvals
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

REVOKE ALL ON FUNCTION sprout_private.validate_prompt_final_approval() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.require_active_prompt_final_approval() FROM PUBLIC;

CREATE TABLE agent_cross_owner_assignment_effects (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    cross_owner_assignment_id uuid NOT NULL,
    task_intent_id uuid NOT NULL,
    task_resource_node_id uuid NOT NULL,
    task_id uuid NOT NULL,
    task_assignment_id uuid NOT NULL,
    target_agent_id uuid NOT NULL,
    assignee_identity_id uuid NOT NULL,
    materialized_by_identity_id uuid NOT NULL,
    materialized_by_device_id uuid NOT NULL,
    idempotency_key uuid NOT NULL,
    request_hash bytea NOT NULL CHECK (octet_length(request_hash) = 32),
    provenance_hash bytea NOT NULL CHECK (octet_length(provenance_hash) = 32),
    applied_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_cross_effect_route_fk
        FOREIGN KEY (project_id, cross_owner_assignment_id)
        REFERENCES agent_cross_owner_assignments (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_effect_intent_fk
        FOREIGN KEY (project_id, task_intent_id)
        REFERENCES agent_task_intents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_effect_task_fk
        FOREIGN KEY (project_id, task_id, task_assignment_id)
        REFERENCES task_assignments (project_id, task_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_effect_resource_fk
        FOREIGN KEY (project_id, task_resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_effect_agent_fk
        FOREIGN KEY (project_id, target_agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_effect_actor_device_fk
        FOREIGN KEY (materialized_by_identity_id, materialized_by_device_id)
        REFERENCES devices (identity_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_cross_effect_route_unique
        UNIQUE (project_id, cross_owner_assignment_id),
    CONSTRAINT agent_cross_effect_project_id_unique
        UNIQUE (project_id, id),
    CONSTRAINT agent_cross_effect_task_assignment_unique
        UNIQUE (project_id, task_assignment_id),
    CONSTRAINT agent_cross_effect_idempotency_unique
        UNIQUE (project_id, materialized_by_identity_id, idempotency_key)
);

CREATE TABLE agent_run_task_effects (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    claim_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    task_provenance_id uuid NOT NULL,
    task_intent_id uuid,
    task_resource_node_id uuid NOT NULL,
    task_id uuid NOT NULL,
    task_assignment_id uuid NOT NULL,
    task_completion_id uuid NOT NULL,
    target_agent_id uuid NOT NULL,
    cross_owner_effect_id uuid,
    actor_identity_id uuid NOT NULL,
    actor_device_id uuid NOT NULL,
    idempotency_key uuid NOT NULL,
    request_hash bytea NOT NULL CHECK (octet_length(request_hash) = 32),
    provenance_hash bytea NOT NULL CHECK (octet_length(provenance_hash) = 32),
    applied_at timestamptz NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_task_effect_work_fk
        FOREIGN KEY (project_id, run_id, work_item_id)
        REFERENCES agent_run_work_slots (project_id, run_id, work_item_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_effect_claim_fk
        FOREIGN KEY (project_id, claim_id)
        REFERENCES agent_run_claim_leases (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_effect_intent_fk
        FOREIGN KEY (project_id, task_intent_id)
        REFERENCES agent_task_intents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_effect_provenance_fk
        FOREIGN KEY (project_id, task_provenance_id)
        REFERENCES agent_task_obligation_provenance (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_effect_assignment_fk
        FOREIGN KEY (project_id, task_id, task_assignment_id)
        REFERENCES task_assignments (project_id, task_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_effect_completion_fk
        FOREIGN KEY (project_id, task_completion_id)
        REFERENCES task_completions (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_effect_agent_fk
        FOREIGN KEY (project_id, target_agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_effect_optional_cross_origin_fk
        FOREIGN KEY (project_id, cross_owner_effect_id)
        REFERENCES agent_cross_owner_assignment_effects (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_effect_actor_device_fk
        FOREIGN KEY (actor_identity_id, actor_device_id)
        REFERENCES devices (identity_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_task_effect_attempt_unique
        UNIQUE (project_id, run_id, work_item_id, attempt),
    CONSTRAINT agent_task_effect_completion_unique
        UNIQUE (project_id, task_completion_id),
    CONSTRAINT agent_task_effect_idempotency_unique
        UNIQUE (project_id, actor_identity_id, idempotency_key)
);

ALTER TABLE agent_cross_owner_assignment_effects ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_cross_owner_assignment_effects FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_run_task_effects ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_run_task_effects FORCE ROW LEVEL SECURITY;

CREATE POLICY agent_cross_effect_parties ON agent_cross_owner_assignment_effects
    USING (EXISTS (
        SELECT 1 FROM agent_cross_owner_assignments assignment
        WHERE assignment.project_id = agent_cross_owner_assignment_effects.project_id
          AND assignment.id = agent_cross_owner_assignment_effects.cross_owner_assignment_id
          AND (
              assignment.requester_identity_id = sprout_private.current_identity_id()
              OR assignment.target_controller_identity_id = sprout_private.current_identity_id()
              OR EXISTS (
                  SELECT 1 FROM governed_agents agent
                  WHERE agent.project_id = assignment.project_id
                    AND agent.id = assignment.target_agent_id
                    AND agent.principal_identity_id = sprout_private.current_identity_id()
              )
          )
    ))
    WITH CHECK (materialized_by_identity_id = sprout_private.current_identity_id());

CREATE POLICY agent_task_effect_run_access ON agent_run_task_effects
    USING (sprout_private.agent_run_access(project_id, run_id))
    WITH CHECK (
        actor_identity_id = sprout_private.current_identity_id()
        AND sprout_private.agent_run_access(project_id, run_id)
    );

-- Ordinary assignment creation owns a permission lineage. Cross-owner agent
-- governance must not create or amplify that lineage. Readability and E2EE
-- material are checked later by the execution/effect boundary that consumes
-- task plaintext; the structural assignment itself never manufactures them.
ALTER TABLE task_assignments
    ADD COLUMN permission_managed_by_assignment boolean NOT NULL DEFAULT true;

-- Narrow structural snapshot for the requester materializing an already-ready
-- route. It rechecks current Manage permission and exposes no plaintext.
CREATE FUNCTION sprout_private.cross_owner_materialization_snapshot(
    candidate_project_id uuid,
    candidate_assignment_id uuid
)
RETURNS TABLE (
    task_resource_node_id uuid,
    task_id uuid,
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
    task_resource uuid;
    permission_level text;
    permission_scope text;
BEGIN
    SELECT assignment.task_resource_node_id
    INTO task_resource
    FROM agent_cross_owner_assignments assignment
    WHERE assignment.project_id = candidate_project_id
      AND assignment.id = candidate_assignment_id
      AND assignment.requester_identity_id = caller
      AND assignment.state = 'ready';
    SELECT permission.access_level, permission.access_scope
    INTO permission_level, permission_scope
    FROM sprout_private.effective_domain_permission(
        candidate_project_id, task_resource, caller
    ) permission;
    IF task_resource IS NULL
       OR permission_level IS DISTINCT FROM 'manage'
       OR permission_scope IS DISTINCT FROM 'full'
    THEN
        RAISE EXCEPTION 'current ready-route manage permission required'
            USING ERRCODE = '42501';
    END IF;
    RETURN QUERY
    SELECT assignment.task_resource_node_id,
           task.id,
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
    JOIN agent_prompt_final_approvals approval
      ON approval.project_id = prompt.project_id
     AND approval.draft_id = prompt.draft_id
     AND approval.agent_id = prompt.agent_id
     AND approval.controller_identity_id = assignment.target_controller_identity_id
     AND approval.local_goal_id = prompt.local_goal_id
     AND approval.local_goal_revision = prompt.local_goal_revision
     AND approval.prompt_hash = prompt.prompt_hash
    JOIN tasks task
      ON task.project_id = assignment.project_id
     AND task.resource_node_id = assignment.task_resource_node_id
     AND task.deleted_at IS NULL
    JOIN task_lists task_list
      ON task_list.project_id = task.project_id
     AND task_list.id = task.task_list_id
     AND task_list.deleted_at IS NULL
    WHERE assignment.project_id = candidate_project_id
      AND assignment.id = candidate_assignment_id
      AND assignment.requester_identity_id = caller
      AND assignment.state = 'ready';
END;
$$;

CREATE FUNCTION sprout_private.validate_cross_owner_assignment_effect()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM sprout_private.effective_domain_permission(
            NEW.project_id,
            NEW.task_resource_node_id,
            NEW.materialized_by_identity_id
        ) permission
        WHERE permission.access_level = 'manage'
          AND permission.access_scope = 'full'
    ) OR NOT EXISTS (
        SELECT 1
        FROM agent_cross_owner_assignments route
        JOIN agent_task_intents intent
          ON intent.project_id = route.project_id
         AND intent.id = route.task_intent_id
         AND intent.task_resource_node_id = route.task_resource_node_id
         AND intent.scope_resource_node_id = route.task_resource_node_id
         AND intent.required_actions = '["assign_own_task"]'::jsonb
        JOIN agent_task_obligation_provenance provenance
          ON provenance.project_id = route.project_id
         AND provenance.task_intent_id = route.task_intent_id
         AND provenance.task_resource_node_id = route.task_resource_node_id
         AND provenance.target_agent_id = route.target_agent_id
        JOIN agent_local_goal_contracts local
          ON local.project_id = provenance.project_id
         AND local.id = provenance.local_goal_id
         AND local.revision = provenance.local_goal_revision
         AND local.agent_id = provenance.target_agent_id
         AND local.state = 'active'
        JOIN agent_prompt_revisions prompt
          ON prompt.project_id = local.project_id
         AND prompt.agent_id = local.agent_id
         AND prompt.local_goal_id = local.id
         AND prompt.local_goal_revision = local.revision
         AND prompt.state = 'active'
        JOIN agent_prompt_final_approvals approval
          ON approval.project_id = prompt.project_id
         AND approval.draft_id = prompt.draft_id
         AND approval.agent_id = prompt.agent_id
         AND approval.controller_identity_id = route.target_controller_identity_id
         AND approval.local_goal_id = prompt.local_goal_id
         AND approval.local_goal_revision = prompt.local_goal_revision
         AND approval.prompt_hash = prompt.prompt_hash
        JOIN governed_agents agent
          ON agent.project_id = route.project_id
         AND agent.id = route.target_agent_id
         AND agent.controller_identity_id = route.target_controller_identity_id
         AND agent.state = 'active'
         AND agent.encrypted_system_prompt = prompt.encrypted_prompt
        JOIN tasks task
          ON task.project_id = route.project_id
         AND task.resource_node_id = route.task_resource_node_id
         AND task.deleted_at IS NULL
        JOIN task_assignments task_assignment
          ON task_assignment.project_id = task.project_id
         AND task_assignment.task_id = task.id
         AND task_assignment.id = NEW.task_assignment_id
         AND task_assignment.assignee_identity_id = agent.principal_identity_id
         AND task_assignment.assigned_by_identity_id = route.requester_identity_id
         AND NOT task_assignment.permission_managed_by_assignment
         AND task_assignment.revoked_at IS NULL
        WHERE route.project_id = NEW.project_id
          AND route.id = NEW.cross_owner_assignment_id
          AND route.state = 'ready'
          AND route.requester_identity_id = NEW.materialized_by_identity_id
          AND route.task_intent_id = NEW.task_intent_id
          AND route.task_resource_node_id = NEW.task_resource_node_id
          AND route.target_agent_id = NEW.target_agent_id
          AND task.id = NEW.task_id
          AND agent.principal_identity_id = NEW.assignee_identity_id
    ) THEN
        RAISE EXCEPTION 'cross-owner effect lacks current authoritative governance or permission'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_run_task_effect()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM agent_collaborative_runs run
        JOIN agent_run_work_slots slot
          ON slot.project_id = run.project_id
         AND slot.run_id = run.id
         AND slot.work_item_id = NEW.work_item_id
        JOIN agent_run_claim_leases claim
          ON claim.project_id = slot.project_id
         AND claim.run_id = slot.run_id
         AND claim.work_item_id = slot.work_item_id
         AND claim.id = NEW.claim_id
         AND claim.attempt = NEW.attempt
         AND claim.claimant_identity_id = NEW.actor_identity_id
         AND claim.status = 'active'
         AND claim.acquired_at <= NEW.applied_at
         AND claim.expires_at > NEW.applied_at
        JOIN agent_task_obligation_provenance provenance
          ON provenance.project_id = run.project_id
         AND provenance.id = NEW.task_provenance_id
         AND provenance.task_intent_id IS NOT DISTINCT FROM NEW.task_intent_id
         AND provenance.local_goal_id = run.local_goal_id
         AND provenance.local_goal_revision = run.local_goal_revision
         AND provenance.work_spec_ordinal = slot.work_spec_ordinal
         AND provenance.task_resource_node_id = NEW.task_resource_node_id
         AND provenance.target_agent_id = NEW.target_agent_id
        JOIN governed_agents agent
          ON agent.project_id = provenance.project_id
         AND agent.id = provenance.target_agent_id
         AND agent.principal_identity_id = NEW.actor_identity_id
         AND agent.state = 'active'
        JOIN agent_local_goal_contracts local
          ON local.project_id = provenance.project_id
         AND local.id = provenance.local_goal_id
         AND local.revision = provenance.local_goal_revision
         AND local.agent_id = provenance.target_agent_id
         AND local.state = 'active'
        JOIN tasks task
          ON task.project_id = provenance.project_id
         AND task.resource_node_id = provenance.task_resource_node_id
         AND task.id = NEW.task_id
         AND task.state = 'completed'
         AND task.completed_by_identity_id = NEW.actor_identity_id
         AND task.completed_at = NEW.applied_at
        JOIN task_assignments task_assignment
          ON task_assignment.project_id = task.project_id
         AND task_assignment.task_id = task.id
         AND task_assignment.id = NEW.task_assignment_id
         AND task_assignment.assignee_identity_id = NEW.actor_identity_id
         AND task_assignment.revoked_at IS NULL
        JOIN task_completions completion
          ON completion.project_id = task.project_id
         AND completion.task_id = task.id
         AND completion.id = NEW.task_completion_id
         AND completion.assignment_id = task_assignment.id
         AND completion.assignee_identity_id = NEW.actor_identity_id
         AND completion.recorded_by_identity_id = NEW.actor_identity_id
         AND completion.completed_at = NEW.applied_at
        WHERE run.project_id = NEW.project_id
          AND run.id = NEW.run_id
          AND run.local_goal_id IS NOT NULL
          AND (
              NEW.cross_owner_effect_id IS NULL
              OR EXISTS (
                  SELECT 1
                  FROM agent_cross_owner_assignment_effects cross_effect
                  WHERE cross_effect.project_id = NEW.project_id
                    AND cross_effect.id = NEW.cross_owner_effect_id
                    AND cross_effect.task_intent_id = NEW.task_intent_id
                    AND cross_effect.task_resource_node_id = NEW.task_resource_node_id
                    AND cross_effect.task_id = NEW.task_id
                    AND cross_effect.task_assignment_id = NEW.task_assignment_id
                    AND cross_effect.target_agent_id = NEW.target_agent_id
                    AND cross_effect.assignee_identity_id = NEW.actor_identity_id
              )
          )
    ) THEN
        RAISE EXCEPTION 'task effect lacks exact run/work/claim/intent product provenance'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_cross_owner_assignment_effect_certificate
BEFORE INSERT ON agent_cross_owner_assignment_effects
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_cross_owner_assignment_effect();

CREATE TRIGGER agent_run_task_effect_certificate
BEFORE INSERT ON agent_run_task_effects
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_run_task_effect();

CREATE FUNCTION sprout_private.reject_agent_product_effect_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'agent product-effect provenance is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_cross_owner_assignment_effect_append_only
BEFORE UPDATE OR DELETE ON agent_cross_owner_assignment_effects
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_product_effect_mutation();

CREATE TRIGGER agent_run_task_effect_append_only
BEFORE UPDATE OR DELETE ON agent_run_task_effects
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_product_effect_mutation();

REVOKE ALL ON FUNCTION sprout_private.validate_cross_owner_assignment_effect() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_run_task_effect() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_product_effect_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.cross_owner_materialization_snapshot(uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.cross_owner_materialization_snapshot(uuid, uuid) TO PUBLIC;

-- Retention may remove live product rows, but the R5.37K semantic projection
-- remains append-only. These retained records deliberately have no FK back to
-- the purged subject; they contain only structural provenance, never E2EE
-- plaintext or key material.
CREATE TABLE agent_task_intent_retained_history (
    project_id uuid NOT NULL,
    id uuid NOT NULL,
    task_resource_node_id uuid NOT NULL,
    scope_resource_node_id uuid NOT NULL,
    required_actions jsonb NOT NULL,
    derived_by_identity_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL,
    retained_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, id)
);

CREATE TABLE agent_task_obligation_retained_history (
    project_id uuid NOT NULL,
    id uuid NOT NULL,
    task_intent_id uuid,
    task_resource_node_id uuid NOT NULL,
    target_agent_id uuid NOT NULL,
    local_goal_id uuid NOT NULL,
    local_goal_revision bigint NOT NULL,
    obligation_id uuid NOT NULL,
    work_spec_ordinal bigint NOT NULL,
    recorded_at timestamptz NOT NULL,
    retained_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, id)
);

CREATE TABLE agent_product_effect_retained_history (
    project_id uuid NOT NULL,
    effect_kind text NOT NULL CHECK (effect_kind IN ('cross_owner_assignment', 'task_completion')),
    effect_id uuid NOT NULL,
    task_resource_node_id uuid NOT NULL,
    structural_record jsonb NOT NULL,
    retained_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, effect_kind, effect_id)
);

ALTER TABLE agent_task_intent_retained_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_task_intent_retained_history FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_task_obligation_retained_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_task_obligation_retained_history FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_product_effect_retained_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_product_effect_retained_history FORCE ROW LEVEL SECURITY;

CREATE TRIGGER agent_task_intent_retained_history_append_only
BEFORE UPDATE OR DELETE ON agent_task_intent_retained_history
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();
CREATE TRIGGER agent_task_obligation_retained_history_append_only
BEFORE UPDATE OR DELETE ON agent_task_obligation_retained_history
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();
CREATE TRIGGER agent_product_effect_retained_history_append_only
BEFORE UPDATE OR DELETE ON agent_product_effect_retained_history
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE VIEW sprout_private.semantic_task_intent_projection AS
SELECT project_id, id, task_resource_node_id, scope_resource_node_id,
       required_actions, derived_by_identity_id, recorded_at
FROM agent_task_intents
UNION
SELECT project_id, id, task_resource_node_id, scope_resource_node_id,
       required_actions, derived_by_identity_id, recorded_at
FROM agent_task_intent_retained_history;

CREATE VIEW sprout_private.semantic_task_obligation_provenance_projection AS
SELECT project_id, id, task_intent_id, task_resource_node_id, target_agent_id,
       local_goal_id, local_goal_revision, obligation_id, work_spec_ordinal, recorded_at
FROM agent_task_obligation_provenance
UNION
SELECT project_id, id, task_intent_id, task_resource_node_id, target_agent_id,
       local_goal_id, local_goal_revision, obligation_id, work_spec_ordinal, recorded_at
FROM agent_task_obligation_retained_history;

-- Core retention deletes task assignments before their resource nodes. Archive
-- and detach only product-effect certificates causally bound to that exact
-- assignment while the authenticated retention lease is current.
CREATE FUNCTION sprout_private.retain_agent_product_effects_for_task_assignment()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    task_resource uuid;
BEGIN
    SELECT task.resource_node_id
    INTO task_resource
    FROM tasks task
    WHERE task.project_id = OLD.project_id AND task.id = OLD.task_id;
    IF task_resource IS NULL OR NOT sprout_private.retention_purge_row_allowed(
        jsonb_build_object(
            'project_id', OLD.project_id,
            'resource_node_id', task_resource
        )
    ) THEN
        RETURN OLD;
    END IF;

    INSERT INTO agent_product_effect_retained_history (
        project_id, effect_kind, effect_id, task_resource_node_id, structural_record
    )
    SELECT effect.project_id, 'task_completion', effect.id,
           effect.task_resource_node_id, to_jsonb(effect)
    FROM agent_run_task_effects effect
    WHERE effect.project_id = OLD.project_id
      AND effect.task_assignment_id = OLD.id
    ON CONFLICT DO NOTHING;
    DELETE FROM agent_run_task_effects effect
    WHERE effect.project_id = OLD.project_id
      AND effect.task_assignment_id = OLD.id;

    INSERT INTO agent_product_effect_retained_history (
        project_id, effect_kind, effect_id, task_resource_node_id, structural_record
    )
    SELECT effect.project_id, 'cross_owner_assignment', effect.id,
           effect.task_resource_node_id, to_jsonb(effect)
    FROM agent_cross_owner_assignment_effects effect
    WHERE effect.project_id = OLD.project_id
      AND effect.task_assignment_id = OLD.id
    ON CONFLICT DO NOTHING;
    DELETE FROM agent_cross_owner_assignment_effects effect
    WHERE effect.project_id = OLD.project_id
      AND effect.task_assignment_id = OLD.id;
    RETURN OLD;
END;
$$;

CREATE TRIGGER aa_task_assignment_agent_effect_retention
BEFORE DELETE ON task_assignments
FOR EACH ROW EXECUTE FUNCTION sprout_private.retain_agent_product_effects_for_task_assignment();

REVOKE ALL ON FUNCTION sprout_private.retain_agent_product_effects_for_task_assignment()
FROM PUBLIC;

-- Generic causal snapshot for a claimed task WorkItem. Cross-owner provenance
-- is optional and is returned only when the exact assignment originated there.
CREATE FUNCTION sprout_private.agent_task_effect_snapshot(
    candidate_project_id uuid,
    candidate_run_id uuid,
    candidate_claim_id uuid
)
RETURNS TABLE (
    work_item_id uuid,
    attempt integer,
    work_spec_ordinal bigint,
    task_provenance_id uuid,
    task_intent_id uuid,
    task_resource_node_id uuid,
    task_id uuid,
    task_payload_version bigint,
    task_assignment_id uuid,
    target_agent_id uuid,
    local_goal_id uuid,
    local_goal_revision bigint,
    obligation_id uuid,
    provenance_recorded_at timestamptz,
    local_contract jsonb,
    target_controller_identity_id uuid,
    target_availability text,
    cross_owner_effect_id uuid
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    caller uuid := sprout_private.current_identity_id();
    caller_device uuid := sprout_private.current_device_id();
BEGIN
    IF caller IS NULL OR caller_device IS NULL THEN
        RAISE EXCEPTION 'authenticated runner required' USING ERRCODE = '42501';
    END IF;
    RETURN QUERY
    SELECT slot.work_item_id, claim.attempt, slot.work_spec_ordinal,
           provenance.id, provenance.task_intent_id, provenance.task_resource_node_id,
           task.id, task.payload_version, assignment.id,
           provenance.target_agent_id, provenance.local_goal_id,
           provenance.local_goal_revision, provenance.obligation_id,
           provenance.recorded_at, local.contract,
           agent.controller_identity_id, agent.availability,
           cross_effect.id
    FROM agent_collaborative_runs run
    JOIN agent_run_participants participant
      ON participant.project_id = run.project_id
     AND participant.run_id = run.id
     AND participant.identity_id = caller
    JOIN agent_run_claim_leases claim
      ON claim.project_id = run.project_id
     AND claim.run_id = run.id
     AND claim.id = candidate_claim_id
     AND claim.claimant_identity_id = caller
     AND claim.status = 'active'
     AND claim.expires_at > clock_timestamp()
    JOIN agent_run_work_slots slot
      ON slot.project_id = claim.project_id
     AND slot.run_id = claim.run_id
     AND slot.work_item_id = claim.work_item_id
    JOIN agent_task_obligation_provenance provenance
      ON provenance.project_id = run.project_id
     AND provenance.local_goal_id = run.local_goal_id
     AND provenance.local_goal_revision = run.local_goal_revision
     AND provenance.work_spec_ordinal = slot.work_spec_ordinal
    JOIN governed_agents agent
      ON agent.project_id = provenance.project_id
     AND agent.id = provenance.target_agent_id
     AND agent.principal_identity_id = caller
     AND agent.state = 'active'
    JOIN agent_runners runner
      ON runner.project_id = agent.project_id
     AND runner.agent_id = agent.id
     AND runner.principal_identity_id = caller
     AND runner.device_id = caller_device
     AND runner.state = 'active'
    JOIN device_keys runner_key
      ON runner_key.identity_id = runner.principal_identity_id
     AND runner_key.device_id = runner.device_id
     AND runner_key.key_version = runner.activated_key_version
     AND runner_key.revoked_at IS NULL
    JOIN agent_local_goal_contracts local
      ON local.project_id = provenance.project_id
     AND local.id = provenance.local_goal_id
     AND local.revision = provenance.local_goal_revision
     AND local.agent_id = provenance.target_agent_id
     AND local.state = 'active'
    JOIN tasks task
      ON task.project_id = provenance.project_id
     AND task.resource_node_id = provenance.task_resource_node_id
     AND task.state = 'open'
     AND task.deleted_at IS NULL
    JOIN resource_closure scope
      ON scope.project_id = task.project_id
     AND scope.ancestor_id = run.scope_resource_node_id
     AND scope.descendant_id = task.resource_node_id
    JOIN task_assignments assignment
      ON assignment.project_id = task.project_id
     AND assignment.task_id = task.id
     AND assignment.assignee_identity_id = caller
     AND assignment.revoked_at IS NULL
    LEFT JOIN agent_cross_owner_assignment_effects cross_effect
      ON cross_effect.project_id = assignment.project_id
     AND cross_effect.task_assignment_id = assignment.id
     AND cross_effect.task_intent_id = provenance.task_intent_id
     AND cross_effect.target_agent_id = provenance.target_agent_id
    WHERE run.project_id = candidate_project_id
      AND run.id = candidate_run_id
      AND run.local_goal_id IS NOT NULL
      AND sprout_private.can_access_resource(
          candidate_project_id, run.scope_resource_node_id, 'read'
      )
    FOR UPDATE OF claim, task, assignment;
END;
$$;

REVOKE ALL ON FUNCTION sprout_private.agent_task_effect_snapshot(uuid, uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.agent_task_effect_snapshot(uuid, uuid, uuid) TO PUBLIC;

CREATE OR REPLACE FUNCTION sprout_private.reject_agent_product_effect_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' AND sprout_private.retention_purge_row_allowed(
        jsonb_build_object(
            'project_id', OLD.project_id,
            'resource_node_id', OLD.task_resource_node_id
        )
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'agent product-effect provenance is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE OR REPLACE FUNCTION sprout_private.validate_agent_run_work_outcome()
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
          AND (
              EXISTS (
                  SELECT 1
                  FROM agent_run_work_product_bindings binding
                  WHERE binding.project_id = NEW.project_id
                    AND binding.run_id = NEW.run_id
                    AND binding.work_item_id = NEW.work_item_id
                    AND binding.claim_id = NEW.claim_id
                    AND binding.attempt = NEW.attempt
                    AND binding.resource_node_id = task.resource_node_id
              )
              OR EXISTS (
                  SELECT 1
                  FROM agent_run_task_effects effect
                  WHERE effect.project_id = NEW.project_id
                    AND effect.run_id = NEW.run_id
                    AND effect.work_item_id = NEW.work_item_id
                    AND effect.claim_id = NEW.claim_id
                    AND effect.attempt = NEW.attempt
                    AND effect.task_resource_node_id = task.resource_node_id
                    AND effect.task_id = task.id
                    AND effect.task_completion_id = NEW.product_event_id
                    AND effect.actor_identity_id = claim.claimant_identity_id
                    AND effect.applied_at = NEW.observed_at
              )
          )
    ) THEN
        RAISE EXCEPTION 'work outcome is not backed by its authoritative product event and domain transition'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

-- Rust's DateTime::timestamp() projects an observation to its containing
-- second. PostgreSQL's direct double-to-bigint cast rounds instead, which made
-- otherwise exact evidence nondeterministically fail for fractional seconds
-- at or above .500. Keep the certificate strict while using the same temporal
-- projection as the authoritative domain transition.
CREATE OR REPLACE FUNCTION sprout_private.validate_agent_run_evidence_certificate()
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
                    floor(extract(epoch FROM NEW.observed_at))::bigint
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

CREATE OR REPLACE FUNCTION sprout_private.purge_agent_user_governance_for_resource()
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

    INSERT INTO agent_product_effect_retained_history (
        project_id, effect_kind, effect_id, task_resource_node_id, structural_record
    )
    SELECT effect.project_id, 'task_completion', effect.id,
           effect.task_resource_node_id, to_jsonb(effect)
    FROM agent_run_task_effects effect
    WHERE effect.project_id = OLD.project_id
      AND (effect.task_resource_node_id = OLD.id
           OR effect.target_agent_id = ANY(purged_agent_ids))
    ON CONFLICT DO NOTHING;
    DELETE FROM agent_run_task_effects effect
    WHERE effect.project_id = OLD.project_id
      AND (effect.task_resource_node_id = OLD.id
           OR effect.target_agent_id = ANY(purged_agent_ids));

    INSERT INTO agent_product_effect_retained_history (
        project_id, effect_kind, effect_id, task_resource_node_id, structural_record
    )
    SELECT effect.project_id, 'cross_owner_assignment', effect.id,
           effect.task_resource_node_id, to_jsonb(effect)
    FROM agent_cross_owner_assignment_effects effect
    WHERE effect.project_id = OLD.project_id
      AND (effect.task_resource_node_id = OLD.id
           OR effect.target_agent_id = ANY(purged_agent_ids))
    ON CONFLICT DO NOTHING;
    DELETE FROM agent_cross_owner_assignment_effects effect
    WHERE effect.project_id = OLD.project_id
      AND (effect.task_resource_node_id = OLD.id
           OR effect.target_agent_id = ANY(purged_agent_ids));

    INSERT INTO agent_task_obligation_retained_history (
        project_id, id, task_intent_id, task_resource_node_id, target_agent_id,
        local_goal_id, local_goal_revision, obligation_id, work_spec_ordinal, recorded_at
    )
    SELECT provenance.project_id, provenance.id, provenance.task_intent_id,
           provenance.task_resource_node_id, provenance.target_agent_id,
           provenance.local_goal_id, provenance.local_goal_revision,
           provenance.obligation_id, provenance.work_spec_ordinal, provenance.recorded_at
    FROM agent_task_obligation_provenance provenance
    WHERE provenance.project_id = OLD.project_id
      AND (
          provenance.target_agent_id = ANY(purged_agent_ids)
          OR provenance.task_resource_node_id = OLD.id
          OR provenance.local_goal_id = ANY(purged_local_goal_ids)
      )
    ON CONFLICT DO NOTHING;

    INSERT INTO agent_task_intent_retained_history (
        project_id, id, task_resource_node_id, scope_resource_node_id,
        required_actions, derived_by_identity_id, recorded_at
    )
    SELECT intent.project_id, intent.id, intent.task_resource_node_id,
           intent.scope_resource_node_id, intent.required_actions,
           intent.derived_by_identity_id, intent.recorded_at
    FROM agent_task_intents intent
    WHERE intent.project_id = OLD.project_id
      AND (intent.task_resource_node_id = OLD.id OR intent.scope_resource_node_id = OLD.id)
    ON CONFLICT DO NOTHING;

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
