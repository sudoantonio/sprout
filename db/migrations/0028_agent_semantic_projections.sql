-- Concrete R5 projections for exact Work -> Task causality and prefix-stable
-- operational histories.  The relational projections below are authoritative;
-- runtime snapshots are hydrated from them and retained product rows never
-- become a second semantic graph or list.

-- ---------------------------------------------------------------------------
-- Exact Work -> Task causal links
-- ---------------------------------------------------------------------------

-- 0025 already protects this table with an append-only UPDATE/DELETE trigger.
-- Suspend that guard only for this migration's transactional projection
-- backfill: existing normative fields remain unchanged, while the new stable
-- position and exact 0027 task-effect witness are populated.  PostgreSQL rolls
-- the trigger state back together with the migration on any failure.
ALTER TABLE agent_run_causal_links
    DISABLE TRIGGER agent_run_causal_links_append_only;

ALTER TABLE agent_run_causal_links
    ADD COLUMN causal_position bigint,
    ADD COLUMN task_effect_id uuid;

WITH ordered AS (
    SELECT id, row_number() OVER (
        ORDER BY recorded_at, project_id, run_id, id
    ) AS position
    FROM agent_run_causal_links
)
UPDATE agent_run_causal_links link
SET causal_position = ordered.position
FROM ordered
WHERE ordered.id = link.id;

CREATE SEQUENCE agent_run_causal_position_seq;
SELECT setval(
    'agent_run_causal_position_seq',
    GREATEST(COALESCE((SELECT max(causal_position) FROM agent_run_causal_links), 0), 1),
    EXISTS (SELECT 1 FROM agent_run_causal_links)
);

ALTER TABLE agent_run_causal_links
    ALTER COLUMN causal_position SET DEFAULT nextval('agent_run_causal_position_seq'),
    ALTER COLUMN causal_position SET NOT NULL,
    ADD CONSTRAINT agent_run_causal_position_unique UNIQUE (causal_position),
    ADD CONSTRAINT agent_run_task_effect_id_unique UNIQUE (project_id, task_effect_id);

CREATE TABLE agent_run_causal_link_retained_history (
    causal_position bigint PRIMARY KEY CHECK (causal_position > 0),
    id uuid NOT NULL,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    predecessor jsonb NOT NULL CHECK (jsonb_typeof(predecessor) = 'object'),
    successor jsonb NOT NULL CHECK (jsonb_typeof(successor) = 'object'),
    observed_tick bigint NOT NULL CHECK (observed_tick >= 0),
    transition_id uuid NOT NULL,
    task_effect_id uuid,
    recorded_at timestamptz NOT NULL,
    retained_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_run_causal_retained_identity_unique
        UNIQUE (project_id, run_id, predecessor, successor),
    CONSTRAINT agent_run_causal_retained_id_unique UNIQUE (id)
);

-- A 0027 task effect already contains the exact run/work/claim/attempt/task
-- certificate.  Backfill only when the separately certified work outcome is
-- present; temporal or same-agent correlation is intentionally insufficient.
INSERT INTO agent_run_causal_links (
    id, project_id, run_id, goal_id, predecessor, successor,
    observed_tick, transition_id, task_effect_id, recorded_at
)
SELECT effect.id,
       effect.project_id,
       effect.run_id,
       run.goal_id,
       jsonb_build_object('kind', 'work', 'work', effect.work_item_id),
       jsonb_build_object('kind', 'task', 'task', effect.task_resource_node_id),
       floor(extract(epoch FROM effect.applied_at))::bigint,
       outcome.transition_id,
       effect.id,
       effect.recorded_at
FROM agent_run_task_effects effect
JOIN agent_collaborative_runs run
  ON run.project_id = effect.project_id AND run.id = effect.run_id
JOIN agent_run_work_outcomes outcome
  ON outcome.project_id = effect.project_id
 AND outcome.run_id = effect.run_id
 AND outcome.work_item_id = effect.work_item_id
 AND outcome.claim_id = effect.claim_id
 AND outcome.attempt = effect.attempt
 AND outcome.outcome_kind = 'task_completion'
 AND outcome.product_event_id = effect.task_completion_id
 AND outcome.observed_at = effect.applied_at
 AND outcome.provenance_hash = effect.provenance_hash
ORDER BY effect.applied_at, effect.project_id, effect.run_id,
         effect.work_item_id, effect.id
ON CONFLICT (project_id, run_id, predecessor, successor) DO UPDATE
SET task_effect_id = EXCLUDED.task_effect_id
WHERE agent_run_causal_links.task_effect_id IS NULL;

-- Retention 0027 preserves the complete structural effect row.  It is an exact
-- legacy witness only when the authoritative run outcome still matches it.
INSERT INTO agent_run_causal_links (
    id, project_id, run_id, goal_id, predecessor, successor,
    observed_tick, transition_id, task_effect_id, recorded_at
)
SELECT retained.effect_id,
       retained.project_id,
       (retained.structural_record ->> 'run_id')::uuid,
       run.goal_id,
       jsonb_build_object(
           'kind', 'work',
           'work', (retained.structural_record ->> 'work_item_id')::uuid
       ),
       jsonb_build_object('kind', 'task', 'task', retained.task_resource_node_id),
       floor(extract(epoch FROM (retained.structural_record ->> 'applied_at')::timestamptz))::bigint,
       outcome.transition_id,
       retained.effect_id,
       (retained.structural_record ->> 'recorded_at')::timestamptz
FROM agent_product_effect_retained_history retained
JOIN agent_collaborative_runs run
  ON run.project_id = retained.project_id
 AND run.id = (retained.structural_record ->> 'run_id')::uuid
JOIN agent_run_work_outcomes outcome
  ON outcome.project_id = retained.project_id
 AND outcome.run_id = (retained.structural_record ->> 'run_id')::uuid
 AND outcome.work_item_id = (retained.structural_record ->> 'work_item_id')::uuid
 AND outcome.claim_id = (retained.structural_record ->> 'claim_id')::uuid
 AND outcome.attempt = (retained.structural_record ->> 'attempt')::integer
 AND outcome.outcome_kind = 'task_completion'
 AND outcome.product_event_id =
       (retained.structural_record ->> 'task_completion_id')::uuid
 AND outcome.observed_at =
       (retained.structural_record ->> 'applied_at')::timestamptz
 AND outcome.provenance_hash = decode(
       substring(retained.structural_record ->> 'provenance_hash' FROM 3), 'hex'
 )
WHERE retained.effect_kind = 'task_completion'
ORDER BY (retained.structural_record ->> 'applied_at')::timestamptz,
         retained.project_id,
         (retained.structural_record ->> 'run_id')::uuid,
         (retained.structural_record ->> 'work_item_id')::uuid,
         retained.effect_id
ON CONFLICT (project_id, run_id, predecessor, successor) DO UPDATE
SET task_effect_id = EXCLUDED.task_effect_id
WHERE agent_run_causal_links.task_effect_id IS NULL;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM agent_run_causal_links link
        WHERE link.predecessor ->> 'kind' = 'work'
          AND link.successor ->> 'kind' = 'task'
          AND link.task_effect_id IS NULL
    ) THEN
        RAISE EXCEPTION 'legacy Work -> Task link lacks an exact 0027 task-effect witness'
            USING ERRCODE = '55000';
    END IF;
END
$$;

ALTER TABLE agent_run_causal_links
    ENABLE TRIGGER agent_run_causal_links_append_only;

CREATE FUNCTION sprout_private.validate_agent_run_causal_link()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM agent_collaborative_runs run
        JOIN agent_run_transitions transition
          ON transition.project_id = run.project_id
         AND transition.run_id = run.id
         AND transition.id = NEW.transition_id
        WHERE run.project_id = NEW.project_id
          AND run.id = NEW.run_id
          AND run.goal_id = NEW.goal_id
          AND EXISTS (
              SELECT 1
              FROM jsonb_array_elements(
                  transition.state_snapshot -> 'causal_links'
              ) certificate
              WHERE certificate -> 'predecessor' = NEW.predecessor
                AND certificate -> 'successor' = NEW.successor
                AND (certificate ->> 'observed_at')::bigint = NEW.observed_tick
          )
    ) THEN
        RAISE EXCEPTION 'causal link is not backed by its exact run transition'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.task_effect_id IS NOT NULL AND (
        NEW.predecessor ->> 'kind' IS DISTINCT FROM 'work'
        OR NEW.successor ->> 'kind' IS DISTINCT FROM 'task'
        OR NOT (
            EXISTS (
                SELECT 1 FROM agent_run_task_effects effect
                WHERE effect.project_id = NEW.project_id
                  AND effect.id = NEW.task_effect_id
                  AND effect.run_id = NEW.run_id
                  AND effect.work_item_id = (NEW.predecessor ->> 'work')::uuid
                  AND effect.task_resource_node_id = (NEW.successor ->> 'task')::uuid
            )
            OR EXISTS (
                SELECT 1 FROM agent_product_effect_retained_history retained
                WHERE retained.project_id = NEW.project_id
                  AND retained.effect_kind = 'task_completion'
                  AND retained.effect_id = NEW.task_effect_id
                  AND (retained.structural_record ->> 'run_id')::uuid = NEW.run_id
                  AND (retained.structural_record ->> 'work_item_id')::uuid =
                      (NEW.predecessor ->> 'work')::uuid
                  AND retained.task_resource_node_id =
                      (NEW.successor ->> 'task')::uuid
            )
        )
    ) THEN
        RAISE EXCEPTION 'task causal link lacks exact Work -> Task effect provenance'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.predecessor ->> 'kind' = 'work'
       AND NEW.successor ->> 'kind' = 'task'
       AND NEW.task_effect_id IS NULL
    THEN
        RAISE EXCEPTION 'Work -> Task causal link requires an exact task effect'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
EXCEPTION WHEN invalid_text_representation THEN
    RAISE EXCEPTION 'invalid task causal-link identity'
        USING ERRCODE = '55000';
END;
$$;

CREATE FUNCTION sprout_private.reject_agent_run_causal_link_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
BEGIN
    IF TG_OP = 'DELETE'
       AND sprout_private.agent_kernel_retention_marked(OLD.run_id, OLD.project_id)
    THEN
        INSERT INTO agent_run_causal_link_retained_history (
            causal_position, id, project_id, run_id, goal_id,
            predecessor, successor, observed_tick, transition_id,
            task_effect_id, recorded_at
        ) VALUES (
            OLD.causal_position, OLD.id, OLD.project_id, OLD.run_id, OLD.goal_id,
            OLD.predecessor, OLD.successor, OLD.observed_tick, OLD.transition_id,
            OLD.task_effect_id, OLD.recorded_at
        )
        ON CONFLICT (causal_position) DO NOTHING;
        IF NOT EXISTS (
            SELECT 1
            FROM agent_run_causal_link_retained_history retained
            WHERE retained.causal_position = OLD.causal_position
              AND retained.id = OLD.id
              AND retained.project_id = OLD.project_id
              AND retained.run_id = OLD.run_id
              AND retained.goal_id = OLD.goal_id
              AND retained.predecessor = OLD.predecessor
              AND retained.successor = OLD.successor
              AND retained.observed_tick = OLD.observed_tick
              AND retained.transition_id = OLD.transition_id
              AND retained.task_effect_id IS NOT DISTINCT FROM OLD.task_effect_id
              AND retained.recorded_at = OLD.recorded_at
        ) THEN
            RAISE EXCEPTION 'retained causal position conflicts with live certificate'
                USING ERRCODE = '55000';
        END IF;
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'agent causal links are append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_run_causal_link_certificate
BEFORE INSERT ON agent_run_causal_links
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_run_causal_link();

CREATE TRIGGER agent_run_causal_link_append_only
BEFORE UPDATE OR DELETE ON agent_run_causal_links
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_run_causal_link_mutation();

REVOKE ALL ON FUNCTION sprout_private.validate_agent_run_causal_link() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_run_causal_link_mutation() FROM PUBLIC;

CREATE FUNCTION sprout_private.reject_agent_run_causal_retained_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'retained agent causal links are append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_run_causal_retained_append_only
BEFORE UPDATE OR DELETE ON agent_run_causal_link_retained_history
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_run_causal_retained_mutation();

REVOKE ALL ON FUNCTION sprout_private.reject_agent_run_causal_retained_mutation() FROM PUBLIC;

ALTER TABLE agent_run_causal_link_retained_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_run_causal_link_retained_history FORCE ROW LEVEL SECURITY;

CREATE POLICY agent_run_causal_retained_project_read
ON agent_run_causal_link_retained_history FOR SELECT
USING (EXISTS (
    SELECT 1 FROM project_memberships membership
    WHERE membership.project_id = agent_run_causal_link_retained_history.project_id
      AND membership.identity_id = sprout_private.current_identity_id()
      AND membership.state = 'active'
));

CREATE FUNCTION sprout_private.semantic_run_causal_link_list(
    candidate_project_id uuid,
    candidate_run_id uuid
)
RETURNS TABLE (
    causal_position bigint,
    id uuid,
    goal_id uuid,
    predecessor jsonb,
    successor jsonb,
    observed_tick bigint,
    transition_id uuid,
    task_effect_id uuid,
    recorded_at timestamptz
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
    SELECT projection.causal_position, projection.id, projection.goal_id,
           projection.predecessor, projection.successor,
           projection.observed_tick, projection.transition_id,
           projection.task_effect_id, projection.recorded_at
    FROM (
        SELECT link.causal_position, link.id, link.project_id, link.run_id,
               link.goal_id, link.predecessor, link.successor,
               link.observed_tick, link.transition_id,
               link.task_effect_id, link.recorded_at
        FROM agent_run_causal_links link
        UNION ALL
        SELECT retained.causal_position, retained.id, retained.project_id,
               retained.run_id, retained.goal_id, retained.predecessor,
               retained.successor, retained.observed_tick,
               retained.transition_id, retained.task_effect_id,
               retained.recorded_at
        FROM agent_run_causal_link_retained_history retained
    ) projection
    WHERE projection.project_id = candidate_project_id
      AND projection.run_id = candidate_run_id
      AND EXISTS (
          SELECT 1 FROM project_memberships membership
          WHERE membership.project_id = candidate_project_id
            AND membership.identity_id = sprout_private.current_identity_id()
            AND membership.state = 'active'
      )
    ORDER BY projection.causal_position
$$;

REVOKE ALL ON FUNCTION sprout_private.semantic_run_causal_link_list(uuid, uuid)
FROM PUBLIC;

-- Evidence consumes the same authoritative causal relation hydrated into the
-- runtime SemanticState.  Blocker/evidence terminality remains a later,
-- separate transition from task materialization.
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
        JOIN task_completions completion
          ON completion.project_id = outcome.project_id
         AND completion.id = outcome.product_event_id
         AND completion.completed_at = outcome.observed_at
        JOIN tasks task
          ON task.project_id = completion.project_id
         AND task.id = completion.task_id
        JOIN agent_run_causal_links causal
          ON causal.project_id = outcome.project_id
         AND causal.run_id = outcome.run_id
         AND causal.predecessor = jsonb_build_object(
             'kind', 'work', 'work', outcome.work_item_id
         )
         AND causal.successor = jsonb_build_object(
             'kind', 'task', 'task', task.resource_node_id
         )
         AND causal.task_effect_id IS NOT NULL
        JOIN agent_run_task_effects effect
          ON effect.project_id = causal.project_id
         AND effect.id = causal.task_effect_id
         AND effect.run_id = causal.run_id
         AND effect.work_item_id = outcome.work_item_id
         AND effect.task_resource_node_id = task.resource_node_id
         AND effect.task_completion_id = outcome.product_event_id
         AND effect.applied_at = outcome.observed_at
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
        RAISE EXCEPTION 'evidence certificate lacks exact Work -> Task causal provenance'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

-- A TaskCompleted work result is certified exclusively by the exact product
-- effect that also backs the Work -> Task edge.  The older generic
-- invocation/effect binding remains historical provenance, but correlation
-- through it alone can no longer produce a semantic work result.
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
        JOIN agent_run_task_effects effect
          ON effect.project_id = NEW.project_id
         AND effect.run_id = NEW.run_id
         AND effect.work_item_id = NEW.work_item_id
         AND effect.claim_id = NEW.claim_id
         AND effect.attempt = NEW.attempt
         AND effect.task_resource_node_id = task.resource_node_id
         AND effect.task_id = task.id
         AND effect.task_completion_id = NEW.product_event_id
         AND effect.actor_identity_id = claim.claimant_identity_id
         AND effect.applied_at = NEW.observed_at
         AND effect.provenance_hash = NEW.provenance_hash
        JOIN agent_run_causal_links causal
          ON causal.project_id = effect.project_id
         AND causal.run_id = effect.run_id
         AND causal.task_effect_id = effect.id
         AND causal.predecessor = jsonb_build_object(
             'kind', 'work', 'work', effect.work_item_id
         )
         AND causal.successor = jsonb_build_object(
             'kind', 'task', 'task', effect.task_resource_node_id
         )
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
        RAISE EXCEPTION 'work outcome lacks its exact task effect and Work -> Task link'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

-- ---------------------------------------------------------------------------
-- Prefix-append-only SemanticOperationalState projection
-- ---------------------------------------------------------------------------

CREATE TABLE sprout_private.semantic_operational_cursor (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    last_position bigint NOT NULL CHECK (last_position >= 0)
);
INSERT INTO sprout_private.semantic_operational_cursor (singleton, last_position)
VALUES (true, 0);

CREATE TABLE agent_semantic_operational_ledger (
    semantic_position bigint PRIMARY KEY CHECK (semantic_position > 0),
    entry_kind text NOT NULL CHECK (entry_kind IN ('task_intent', 'task_provenance')),
    project_id uuid NOT NULL,
    record_id uuid NOT NULL,
    task_resource_node_id uuid NOT NULL,
    scope_resource_node_id uuid,
    required_actions jsonb,
    principal_identity_id uuid NOT NULL,
    target_agent_id uuid,
    task_intent_id uuid,
    local_goal_id uuid,
    local_goal_revision bigint,
    obligation_id uuid,
    work_spec_ordinal bigint,
    recorded_at timestamptz NOT NULL,
    CONSTRAINT agent_semantic_operational_identity_unique
        UNIQUE (entry_kind, project_id, record_id),
    CONSTRAINT agent_semantic_operational_shape CHECK (
        (entry_kind = 'task_intent'
         AND scope_resource_node_id IS NOT NULL
         AND jsonb_typeof(required_actions) = 'array'
         AND target_agent_id IS NULL
         AND task_intent_id IS NULL
         AND local_goal_id IS NULL
         AND local_goal_revision IS NULL
         AND obligation_id IS NULL
         AND work_spec_ordinal IS NULL)
        OR
        (entry_kind = 'task_provenance'
         AND scope_resource_node_id IS NULL
         AND required_actions IS NULL
         AND target_agent_id IS NOT NULL
         AND local_goal_id IS NOT NULL
         AND local_goal_revision > 0
         AND obligation_id IS NOT NULL
         AND work_spec_ordinal >= 0)
    )
);

-- Fail closed if an already-retained 0027 provenance no longer contains enough
-- structural data to recover the exact agent principal.  Do not turn AgentId,
-- timestamps or nearby effects into a weak PrincipalId correlation.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM agent_task_obligation_retained_history retained
        WHERE NOT EXISTS (
            SELECT 1 FROM governed_agents agent
            WHERE agent.project_id = retained.project_id
              AND agent.id = retained.target_agent_id
        )
        AND NOT EXISTS (
            SELECT 1 FROM agent_product_effect_retained_history effect
            WHERE effect.project_id = retained.project_id
              AND effect.task_resource_node_id = retained.task_resource_node_id
              AND effect.structural_record ->> 'target_agent_id' =
                  retained.target_agent_id::text
              AND (
                  effect.structural_record ? 'actor_identity_id'
                  OR effect.structural_record ? 'assignee_identity_id'
              )
        )
    ) THEN
        RAISE EXCEPTION 'legacy retained provenance lacks exact agent-principal witness'
            USING ERRCODE = '55000';
    END IF;
END
$$;

WITH intent_rows AS (
    SELECT project_id, id, task_resource_node_id, scope_resource_node_id,
           required_actions, derived_by_identity_id, recorded_at
    FROM agent_task_intents
    UNION
    SELECT project_id, id, task_resource_node_id, scope_resource_node_id,
           required_actions, derived_by_identity_id, recorded_at
    FROM agent_task_intent_retained_history
),
provenance_rows AS (
    SELECT provenance.project_id, provenance.id, provenance.task_intent_id,
           provenance.task_resource_node_id, provenance.target_agent_id,
           provenance.local_goal_id, provenance.local_goal_revision,
           provenance.obligation_id, provenance.work_spec_ordinal,
           provenance.recorded_at,
           agent.principal_identity_id
    FROM agent_task_obligation_provenance provenance
    JOIN governed_agents agent
      ON agent.project_id = provenance.project_id
     AND agent.id = provenance.target_agent_id
    UNION
    SELECT retained.project_id, retained.id, retained.task_intent_id,
           retained.task_resource_node_id, retained.target_agent_id,
           retained.local_goal_id, retained.local_goal_revision,
           retained.obligation_id, retained.work_spec_ordinal,
           retained.recorded_at,
           COALESCE(
               agent.principal_identity_id,
               (
                   SELECT COALESCE(
                       NULLIF(effect.structural_record ->> 'actor_identity_id', '')::uuid,
                       NULLIF(effect.structural_record ->> 'assignee_identity_id', '')::uuid
                   )
                   FROM agent_product_effect_retained_history effect
                   WHERE effect.project_id = retained.project_id
                     AND effect.task_resource_node_id = retained.task_resource_node_id
                     AND effect.structural_record ->> 'target_agent_id' =
                         retained.target_agent_id::text
                   ORDER BY effect.retained_at, effect.effect_id
                   LIMIT 1
               )
           ) AS principal_identity_id
    FROM agent_task_obligation_retained_history retained
    LEFT JOIN governed_agents agent
      ON agent.project_id = retained.project_id
     AND agent.id = retained.target_agent_id
),
all_rows AS (
    SELECT 'task_intent'::text AS entry_kind, project_id, id AS record_id,
           task_resource_node_id, scope_resource_node_id, required_actions,
           derived_by_identity_id AS principal_identity_id,
           NULL::uuid AS target_agent_id, NULL::uuid AS task_intent_id,
           NULL::uuid AS local_goal_id,
           NULL::bigint AS local_goal_revision, NULL::uuid AS obligation_id,
           NULL::bigint AS work_spec_ordinal, recorded_at
    FROM intent_rows
    UNION ALL
    SELECT 'task_provenance', project_id, id, task_resource_node_id,
           NULL, NULL, principal_identity_id, target_agent_id,
           task_intent_id, local_goal_id,
           local_goal_revision, obligation_id, work_spec_ordinal, recorded_at
    FROM provenance_rows
),
ordered AS (
    SELECT row_number() OVER (
               ORDER BY recorded_at, entry_kind, project_id, record_id
           ) AS semantic_position,
           *
    FROM all_rows
)
INSERT INTO agent_semantic_operational_ledger (
    semantic_position, entry_kind, project_id, record_id,
    task_resource_node_id, scope_resource_node_id, required_actions,
    principal_identity_id, target_agent_id, task_intent_id, local_goal_id,
    local_goal_revision, obligation_id, work_spec_ordinal, recorded_at
)
SELECT semantic_position, entry_kind, project_id, record_id,
       task_resource_node_id, scope_resource_node_id, required_actions,
       principal_identity_id, target_agent_id, task_intent_id, local_goal_id,
       local_goal_revision, obligation_id, work_spec_ordinal, recorded_at
FROM ordered;

UPDATE sprout_private.semantic_operational_cursor
SET last_position = COALESCE(
    (SELECT max(semantic_position) FROM agent_semantic_operational_ledger), 0
);

CREATE FUNCTION sprout_private.append_semantic_operational_entry()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    next_position bigint;
    agent_principal uuid;
BEGIN
    UPDATE sprout_private.semantic_operational_cursor
    SET last_position = last_position + 1
    WHERE singleton
    RETURNING last_position INTO STRICT next_position;

    IF TG_TABLE_NAME = 'agent_task_intents' THEN
        INSERT INTO agent_semantic_operational_ledger (
            semantic_position, entry_kind, project_id, record_id,
            task_resource_node_id, scope_resource_node_id, required_actions,
            principal_identity_id, recorded_at
        ) VALUES (
            next_position, 'task_intent', NEW.project_id, NEW.id,
            NEW.task_resource_node_id, NEW.scope_resource_node_id,
            NEW.required_actions, NEW.derived_by_identity_id, NEW.recorded_at
        );
    ELSIF TG_TABLE_NAME = 'agent_task_obligation_provenance' THEN
        SELECT agent.principal_identity_id INTO STRICT agent_principal
        FROM governed_agents agent
        WHERE agent.project_id = NEW.project_id AND agent.id = NEW.target_agent_id;
        INSERT INTO agent_semantic_operational_ledger (
            semantic_position, entry_kind, project_id, record_id,
            task_resource_node_id, principal_identity_id, target_agent_id,
            task_intent_id,
            local_goal_id, local_goal_revision, obligation_id,
            work_spec_ordinal, recorded_at
        ) VALUES (
            next_position, 'task_provenance', NEW.project_id, NEW.id,
            NEW.task_resource_node_id, agent_principal, NEW.target_agent_id,
            NEW.task_intent_id,
            NEW.local_goal_id, NEW.local_goal_revision, NEW.obligation_id,
            NEW.work_spec_ordinal, NEW.recorded_at
        );
    ELSE
        RAISE EXCEPTION 'unsupported semantic operational source table'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.reject_task_intent_mutation()
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
           OR OLD.scope_resource_node_id = retention_resource
       )
    THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'task intents are append-only'
        USING ERRCODE = '55000';
EXCEPTION WHEN invalid_text_representation THEN
    RAISE EXCEPTION 'task intents are append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE FUNCTION sprout_private.reject_semantic_operational_ledger_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'semantic operational projection is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_task_intent_semantic_append
AFTER INSERT ON agent_task_intents
FOR EACH ROW EXECUTE FUNCTION sprout_private.append_semantic_operational_entry();

CREATE TRIGGER agent_task_provenance_semantic_append
AFTER INSERT ON agent_task_obligation_provenance
FOR EACH ROW EXECUTE FUNCTION sprout_private.append_semantic_operational_entry();

CREATE TRIGGER agent_task_intent_append_only
BEFORE UPDATE OR DELETE ON agent_task_intents
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_task_intent_mutation();

CREATE TRIGGER agent_semantic_operational_ledger_append_only
BEFORE UPDATE OR DELETE ON agent_semantic_operational_ledger
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_semantic_operational_ledger_mutation();

REVOKE ALL ON FUNCTION sprout_private.append_semantic_operational_entry() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_task_intent_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_semantic_operational_ledger_mutation() FROM PUBLIC;

ALTER TABLE agent_semantic_operational_ledger ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_semantic_operational_ledger FORCE ROW LEVEL SECURITY;

CREATE POLICY agent_semantic_operational_project_read
ON agent_semantic_operational_ledger FOR SELECT
USING (EXISTS (
    SELECT 1 FROM project_memberships membership
    WHERE membership.project_id = agent_semantic_operational_ledger.project_id
      AND membership.identity_id = sprout_private.current_identity_id()
      AND membership.state = 'active'
));

CREATE FUNCTION sprout_private.semantic_task_intent_list(candidate_project_id uuid)
RETURNS TABLE (
    semantic_position bigint,
    id uuid,
    task_resource_node_id uuid,
    scope_resource_node_id uuid,
    required_actions jsonb,
    derived_by_identity_id uuid,
    recorded_at timestamptz
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
    SELECT ledger.semantic_position, ledger.record_id,
           ledger.task_resource_node_id, ledger.scope_resource_node_id,
           ledger.required_actions, ledger.principal_identity_id,
           ledger.recorded_at
    FROM agent_semantic_operational_ledger ledger
    WHERE ledger.project_id = candidate_project_id
      AND ledger.entry_kind = 'task_intent'
      AND EXISTS (
          SELECT 1 FROM project_memberships membership
          WHERE membership.project_id = candidate_project_id
            AND membership.identity_id = sprout_private.current_identity_id()
            AND membership.state = 'active'
      )
    ORDER BY ledger.semantic_position
$$;

CREATE FUNCTION sprout_private.semantic_task_provenance_list(candidate_project_id uuid)
RETURNS TABLE (
    semantic_position bigint,
    id uuid,
    task_intent_id uuid,
    task_resource_node_id uuid,
    target_agent_id uuid,
    agent_identity_id uuid,
    local_goal_id uuid,
    local_goal_revision bigint,
    obligation_id uuid,
    work_spec_ordinal bigint,
    recorded_at timestamptz
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
    SELECT ledger.semantic_position, ledger.record_id, ledger.task_intent_id,
           ledger.task_resource_node_id, ledger.target_agent_id,
           ledger.principal_identity_id,
           ledger.local_goal_id, ledger.local_goal_revision,
           ledger.obligation_id, ledger.work_spec_ordinal, ledger.recorded_at
    FROM agent_semantic_operational_ledger ledger
    WHERE ledger.project_id = candidate_project_id
      AND ledger.entry_kind = 'task_provenance'
      AND EXISTS (
          SELECT 1 FROM project_memberships membership
          WHERE membership.project_id = candidate_project_id
            AND membership.identity_id = sprout_private.current_identity_id()
            AND membership.state = 'active'
      )
    ORDER BY ledger.semantic_position
$$;

REVOKE ALL ON FUNCTION sprout_private.semantic_task_intent_list(uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.semantic_task_provenance_list(uuid) FROM PUBLIC;

-- These list projections are internal runtime adapters.  An arbitrary SQL
-- role can set custom GUCs, so PUBLIC EXECUTE would turn app.identity_id into
-- spoofable authority at this SECURITY DEFINER boundary.  The function owner
-- (the server/migration role) retains implicit EXECUTE; no unprivileged role
-- receives it transitively.

-- Keep the old relational view names for compatibility, but source them from
-- the immutable ledger.  The list-valued functions above are the normative
-- ordered projection consumed by runtime and tests.
DROP VIEW sprout_private.semantic_task_intent_projection;
CREATE VIEW sprout_private.semantic_task_intent_projection AS
SELECT project_id, record_id AS id, task_resource_node_id,
       scope_resource_node_id, required_actions,
       principal_identity_id AS derived_by_identity_id, recorded_at
FROM agent_semantic_operational_ledger
WHERE entry_kind = 'task_intent';

DROP VIEW sprout_private.semantic_task_obligation_provenance_projection;
CREATE VIEW sprout_private.semantic_task_obligation_provenance_projection AS
SELECT project_id, record_id AS id, task_intent_id, task_resource_node_id,
       target_agent_id,
       local_goal_id, local_goal_revision, obligation_id,
       work_spec_ordinal, recorded_at
FROM agent_semantic_operational_ledger
WHERE entry_kind = 'task_provenance';
