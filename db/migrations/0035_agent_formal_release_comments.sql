-- Full R5.40/R5.41 release projection and the native governed Comment surface.
--
-- This migration is additive.  A run becomes 0035-native only when the server
-- initializes an exact release root in the same transaction as its canonical
-- run initialization.  Existing 0034 roots and histories are never promoted.

CREATE TABLE agent_coordination_policy_versions (
    version integer PRIMARY KEY CHECK (version > 0),
    max_agent_comment_depth integer NOT NULL CHECK (max_agent_comment_depth > 0),
    policy_hash bytea NOT NULL CHECK (octet_length(policy_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

INSERT INTO agent_coordination_policy_versions (
    version, max_agent_comment_depth, policy_hash
) VALUES (
    1, 4,
    digest(convert_to('sprout-coordination-policy-v1|max-agent-comment-depth=4', 'UTF8'), 'sha256')
);

-- Operational timestamptz values are not R5.40 Nat ticks.  These nullable,
-- server-written fields bind only 0035-native records to the same semantic
-- tick domain as `agent_run_transitions.semantic_tick`; NULL keeps legacy
-- records unprojected and fail-closed.
ALTER TABLE agent_model_attempt_dispatches
    ADD COLUMN semantic_tick bigint CHECK (semantic_tick IS NULL OR semantic_tick >= 0);
ALTER TABLE agent_model_invocation_projections
    ADD COLUMN semantic_tick bigint CHECK (semantic_tick IS NULL OR semantic_tick >= 0);
ALTER TABLE agent_interrogations
    ADD COLUMN semantic_tick bigint CHECK (semantic_tick IS NULL OR semantic_tick >= 0);
ALTER TABLE agent_interrogation_answers
    ADD COLUMN semantic_tick bigint CHECK (semantic_tick IS NULL OR semantic_tick >= 0);
ALTER TABLE agent_effect_proposals
    ADD COLUMN applied_semantic_tick bigint
      CHECK (applied_semantic_tick IS NULL OR applied_semantic_tick >= 0);

-- Claim authority has always carried Nat ticks in the canonical run state,
-- while the operational lease ledger uses timestamptz for recovery.  Preserve
-- both clocks explicitly for 0035-native claims; legacy rows remain NULL and
-- are never promoted into the formal release projection.
ALTER TABLE agent_run_claim_leases
    ADD COLUMN acquired_semantic_tick bigint
      CHECK (acquired_semantic_tick IS NULL OR acquired_semantic_tick >= 0),
    ADD COLUMN expires_semantic_tick bigint
      CHECK (expires_semantic_tick IS NULL OR expires_semantic_tick > acquired_semantic_tick),
    ADD CONSTRAINT agent_run_claim_semantic_time_shape CHECK (
      (acquired_semantic_tick IS NULL AND expires_semantic_tick IS NULL)
      OR (acquired_semantic_tick IS NOT NULL AND expires_semantic_tick IS NOT NULL)
    );

CREATE TABLE agent_r541_release_roots (
    trace_number bigint PRIMARY KEY CHECK (trace_number > 0),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    start_tick bigint NOT NULL CHECK (start_tick >= 0),
    initialization_transition_id uuid NOT NULL,
    coordination_policy_version integer NOT NULL,
    governance_start_snapshot jsonb NOT NULL CHECK (jsonb_typeof(governance_start_snapshot)='object'),
    operational_start_snapshot jsonb NOT NULL CHECK (
      jsonb_typeof(operational_start_snapshot)='object'
      AND jsonb_typeof(operational_start_snapshot->'proxy_transcripts')='array'
      AND jsonb_typeof(operational_start_snapshot->'proxy_audit')='array'
      AND jsonb_typeof(operational_start_snapshot->'task_provenance')='array'
      AND jsonb_typeof(operational_start_snapshot->'task_intents')='array'
    ),
    proxy_directory_start_snapshot jsonb NOT NULL CHECK (jsonb_typeof(proxy_directory_start_snapshot)='array'),
    comment_start_snapshot jsonb NOT NULL CHECK (jsonb_typeof(comment_start_snapshot)='array'),
    root_hash bytea NOT NULL CHECK (octet_length(root_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number, project_id)
        REFERENCES agent_r540_tool_trace_roots (trace_number, project_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (coordination_policy_version)
        REFERENCES agent_coordination_policy_versions (version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (project_id, run_id),
    UNIQUE (trace_number, project_id),
    UNIQUE (trace_number, project_id, run_id),
    UNIQUE (trace_number, project_id, run_id, goal_id)
);

CREATE TABLE agent_run_resource_authority_snapshots (
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    sponsor_identity_id uuid NOT NULL,
    resource_authority jsonb NOT NULL CHECK (jsonb_typeof(resource_authority)='array'),
    authority_statement text NOT NULL CHECK (octet_length(authority_statement)>0),
    authority_hash bytea NOT NULL CHECK (octet_length(authority_hash)=32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id,run_id),
    -- The operational run is retention-purgeable.  This immutable authority
    -- snapshot preserves structural provenance and exact served views still
    -- require a live matching run; retaining it cannot recreate permission.
    FOREIGN KEY (project_id,sponsor_identity_id)
      REFERENCES project_memberships(project_id,identity_id)
      ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (project_id,run_id,authority_hash),
    CHECK (resource_authority=authority_statement::jsonb),
    CHECK (authority_hash=digest(convert_to(authority_statement,'UTF8'),'sha256'))
);
CREATE FUNCTION sprout_private.reject_agent_run_resource_authority_mutation()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
BEGIN RAISE EXCEPTION 'run resource authority snapshot is immutable' USING ERRCODE='55000'; END $$;
CREATE TRIGGER agent_run_resource_authority_immutable BEFORE UPDATE OR DELETE
 ON agent_run_resource_authority_snapshots FOR EACH ROW
 EXECUTE FUNCTION sprout_private.reject_agent_run_resource_authority_mutation();
ALTER TABLE agent_run_resource_authority_snapshots ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_run_resource_authority_snapshots FORCE ROW LEVEL SECURITY;
CREATE POLICY agent_run_resource_authority_read ON agent_run_resource_authority_snapshots
 FOR SELECT USING (sprout_private.agent_run_access(project_id,run_id));
REVOKE INSERT,UPDATE,DELETE ON agent_run_resource_authority_snapshots FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_run_resource_authority_mutation() FROM PUBLIC;

-- The pre-0035 evidence writer projected the formal Nat `observedAt` from the
-- task-completion wall clock.  Preserve that legacy branch byte-for-byte in
-- meaning, but require 0035-native evidence to use the canonical run semantic
-- tick carried by the exact `evidence_accepted` transition.  `NEW.observed_at`
-- remains the real operational task-completion timestamp in both branches.
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
          AND transition.transition_kind IN ('evidence_accepted','work_succeeded')
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
                AND (
                  (EXISTS (
                     SELECT 1 FROM agent_r541_release_roots root
                     WHERE root.project_id=NEW.project_id AND root.run_id=NEW.run_id
                       AND root.goal_id=run.goal_id
                   )
                   AND transition.semantic_tick IS NOT NULL
                   AND (certificate ->> 'observed_at')::bigint=transition.semantic_tick)
                  OR
                  (NOT EXISTS (
                     SELECT 1 FROM agent_r541_release_roots root
                     WHERE root.project_id=NEW.project_id AND root.run_id=NEW.run_id
                   )
                   AND (certificate ->> 'observed_at')::bigint =
                       floor(extract(epoch FROM NEW.observed_at))::bigint)
                )
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

-- One canonical logical clock backs every formal Event of a 0035-native run.
-- Wall-clock timestamps remain operational metadata only: collision freedom
-- and total order come from the row lock on this cursor and the immutable
-- allocation ledger below.
CREATE TABLE agent_run_semantic_tick_cursors (
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    start_tick bigint NOT NULL CHECK (start_tick >= 0),
    last_tick bigint NOT NULL CHECK (last_tick >= start_tick),
    allocation_count bigint NOT NULL DEFAULT 0 CHECK (allocation_count >= 0),
    cursor_hash bytea NOT NULL CHECK (octet_length(cursor_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, run_id),
    UNIQUE (trace_number),
    FOREIGN KEY (trace_number, project_id, run_id)
        REFERENCES agent_r541_release_roots (trace_number, project_id, run_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE TABLE agent_run_semantic_tick_allocations (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    ordinal bigint NOT NULL CHECK (ordinal > 0),
    semantic_tick bigint NOT NULL CHECK (semantic_tick >= 0),
    event_kind text NOT NULL CHECK (event_kind <> ''),
    event_key uuid NOT NULL,
    request_hash bytea NOT NULL CHECK (octet_length(request_hash) = 32),
    previous_allocation_hash bytea NOT NULL CHECK (octet_length(previous_allocation_hash) = 32),
    allocation_hash bytea NOT NULL CHECK (octet_length(allocation_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number, project_id, run_id)
        REFERENCES agent_r541_release_roots (trace_number, project_id, run_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (project_id, run_id, ordinal),
    UNIQUE (project_id, run_id, semantic_tick),
    UNIQUE (project_id, run_id, event_key),
    UNIQUE (trace_number, ordinal),
    UNIQUE (trace_number, semantic_tick),
    UNIQUE (trace_number, allocation_hash)
);

-- The 0033 call row contains both formal Nat ticks and operational timestamps.
-- This immutable per-attempt ledger proves their deliberately separate
-- meanings for 0035-native runs.  It also lets the unchanged 0033 trigger
-- validate its legacy epoch projection in the middle of the BEFORE chain,
-- after which the exact semantic tick is restored into the persisted row.
CREATE TABLE agent_tool_attempt_clock_bindings (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    call_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    allocation_id uuid NOT NULL,
    requested_tick bigint NOT NULL CHECK (requested_tick >= 0),
    tool_deadline_tick bigint NOT NULL CHECK (tool_deadline_tick > requested_tick),
    requested_at timestamptz NOT NULL,
    tool_deadline_at timestamptz NOT NULL CHECK (tool_deadline_at > requested_at),
    binding_hash bytea NOT NULL CHECK (octet_length(binding_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (allocation_id) REFERENCES agent_run_semantic_tick_allocations (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (trace_number, project_id, run_id)
        REFERENCES agent_r541_release_roots (trace_number, project_id, run_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (project_id, call_id, attempt),
    UNIQUE (trace_number, call_id, attempt),
    UNIQUE (allocation_id),
    UNIQUE (binding_hash)
);

CREATE TABLE native_comments (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    author_identity_id uuid NOT NULL,
    author_kind text NOT NULL CHECK (author_kind IN ('administrator', 'user', 'agent')),
    recipient_identity_id uuid NOT NULL,
    target_resource_node_id uuid NOT NULL,
    parent_comment_id uuid,
    agent_depth integer NOT NULL CHECK (agent_depth >= 0),
    encrypted_payload bytea,
    payload_commitment bytea NOT NULL CHECK (octet_length(payload_commitment) = 32),
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    semantic_tick bigint NOT NULL CHECK (semantic_tick >= 0),
    idempotency_key uuid NOT NULL,
    request_hash bytea NOT NULL CHECK (octet_length(request_hash) = 32),
    run_id uuid,
    goal_id uuid,
    work_item_id uuid,
    claim_id uuid,
    attempt integer CHECK (attempt IS NULL OR attempt > 0),
    trace_number bigint CHECK (trace_number IS NULL OR trace_number > 0),
    event_hash bytea NOT NULL CHECK (octet_length(event_hash) = 32),
    payload_purged_at timestamptz,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT native_comments_distinct_parties CHECK (author_identity_id <> recipient_identity_id),
    CONSTRAINT native_comments_payload_shape CHECK (
        (encrypted_payload IS NOT NULL AND octet_length(encrypted_payload) > 0 AND payload_purged_at IS NULL)
        OR (encrypted_payload IS NULL AND payload_purged_at IS NOT NULL)
    ),
    CONSTRAINT native_comments_author_depth_shape CHECK (
        (author_kind IN ('administrator', 'user') AND agent_depth = 0)
        OR (author_kind = 'agent' AND agent_depth > 0)
    ),
    CONSTRAINT native_comments_agent_work_shape CHECK (
        (author_kind = 'agent' AND run_id IS NOT NULL AND goal_id IS NOT NULL
          AND work_item_id IS NOT NULL AND claim_id IS NOT NULL AND attempt IS NOT NULL
          AND trace_number IS NOT NULL)
        OR (author_kind <> 'agent' AND work_item_id IS NULL AND claim_id IS NULL AND attempt IS NULL
          AND ((run_id IS NULL AND goal_id IS NULL AND trace_number IS NULL)
            OR (run_id IS NOT NULL AND goal_id IS NOT NULL AND trace_number IS NOT NULL)))
    ),
    -- The resource is exact-validated by the trusted writer.  It is not an FK
    -- parent because authorized retention may purge the live resource while
    -- the immutable logical Comment descriptor/history must survive.
    FOREIGN KEY (project_id, author_identity_id)
        REFERENCES project_memberships (project_id, identity_id) ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (project_id, recipient_identity_id)
        REFERENCES project_memberships (project_id, identity_id) ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (project_id, parent_comment_id)
        REFERENCES native_comments (project_id, id) ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (trace_number, project_id, run_id, goal_id)
        REFERENCES agent_r541_release_roots (trace_number, project_id, run_id, goal_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (project_id, id),
    UNIQUE (project_id, author_identity_id, idempotency_key),
    UNIQUE (project_id, event_hash)
);

CREATE UNIQUE INDEX native_comments_agent_root_fresh_idx
    ON native_comments (project_id, author_identity_id, recipient_identity_id, target_resource_node_id)
    WHERE author_kind = 'agent' AND parent_comment_id IS NULL;
CREATE INDEX native_comments_target_tick_idx
    ON native_comments (project_id, target_resource_node_id, semantic_tick, id);
CREATE INDEX native_comments_recipient_tick_idx
    ON native_comments (project_id, recipient_identity_id, semantic_tick, id);

CREATE TABLE native_comment_events (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    comment_id uuid NOT NULL,
    project_ordinal bigint NOT NULL CHECK (project_ordinal > 0),
    run_id uuid,
    trace_number bigint CHECK (trace_number IS NULL OR trace_number > 0),
    semantic_tick bigint NOT NULL CHECK (semantic_tick >= 0),
    event_kind text NOT NULL CHECK (event_kind = 'comment_posted'),
    action_path text NOT NULL CHECK (action_path IN ('human_native','agent_action')),
    comment_snapshot jsonb NOT NULL CHECK (jsonb_typeof(comment_snapshot) = 'object'),
    event_hash bytea NOT NULL CHECK (octet_length(event_hash) = 32),
    previous_state_hash bytea CHECK (
        previous_state_hash IS NULL OR octet_length(previous_state_hash) = 32
    ),
    semantic_state_hash bytea NOT NULL CHECK (octet_length(semantic_state_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (project_id, comment_id) REFERENCES native_comments (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (project_id, comment_id),
    UNIQUE (project_id, project_ordinal),
    UNIQUE (project_id, event_hash),
    UNIQUE (project_id, semantic_state_hash),
    CONSTRAINT native_comment_events_trace_shape CHECK (
        (run_id IS NULL AND trace_number IS NULL) OR (run_id IS NOT NULL AND trace_number IS NOT NULL)
    )
);

CREATE TABLE native_comment_notifications (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    comment_id uuid NOT NULL,
    recipient_identity_id uuid NOT NULL,
    semantic_tick bigint NOT NULL CHECK (semantic_tick >= 0),
    notification_hash bytea NOT NULL CHECK (octet_length(notification_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (project_id, comment_id) REFERENCES native_comments (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (project_id, run_id, comment_id),
    UNIQUE (project_id, notification_hash)
);

CREATE TABLE native_comment_responses (
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    comment_id uuid NOT NULL,
    response_comment_id uuid NOT NULL,
    recipient_identity_id uuid NOT NULL,
    response_tick bigint NOT NULL CHECK (response_tick >= 0),
    response_hash bytea NOT NULL CHECK (octet_length(response_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, run_id, comment_id),
    FOREIGN KEY (project_id, comment_id) REFERENCES native_comments (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (project_id, response_comment_id) REFERENCES native_comments (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (project_id, run_id, response_comment_id)
);

CREATE TABLE agent_native_comment_security_effects (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    claim_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    actor_identity_id uuid NOT NULL,
    comment_id uuid NOT NULL,
    target_resource_node_id uuid NOT NULL,
    context_sources jsonb NOT NULL CHECK (jsonb_typeof(context_sources)='array'),
    payload_commitment bytea NOT NULL CHECK (octet_length(payload_commitment)=32),
    observed_tick bigint NOT NULL CHECK (observed_tick >= 0),
    effect_hash bytea NOT NULL CHECK (octet_length(effect_hash)=32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (project_id,comment_id) REFERENCES native_comments(project_id,id)
      ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE(project_id,run_id,comment_id),
    UNIQUE(project_id,effect_hash)
);

CREATE TABLE agent_r541_comment_records (
    id uuid PRIMARY KEY,
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    semantic_tick bigint NOT NULL CHECK (semantic_tick >= 0),
    comment_id uuid NOT NULL,
    comment_event_id uuid NOT NULL,
    comment_snapshot jsonb NOT NULL CHECK (jsonb_typeof(comment_snapshot) = 'object'),
    record_hash bytea NOT NULL CHECK (octet_length(record_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number, project_id, run_id, goal_id)
        REFERENCES agent_r541_release_roots (trace_number, project_id, run_id, goal_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (project_id, comment_id) REFERENCES native_comments (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (comment_event_id) REFERENCES native_comment_events (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (trace_number, comment_id),
    UNIQUE (trace_number, record_hash)
);

CREATE TABLE agent_r541_comment_inventory (
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    project_id uuid NOT NULL,
    ordinal bigint NOT NULL CHECK (ordinal > 0),
    comment_record_id uuid NOT NULL,
    record_hash bytea NOT NULL CHECK (octet_length(record_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (trace_number, ordinal),
    FOREIGN KEY (trace_number, project_id)
        REFERENCES agent_r541_release_roots (trace_number, project_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (comment_record_id) REFERENCES agent_r541_comment_records (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (trace_number, comment_record_id)
);

CREATE TABLE agent_r541_comment_certificates (
    id uuid PRIMARY KEY,
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    end_tick bigint NOT NULL CHECK (end_tick >= 0),
    last_ordinal bigint NOT NULL CHECK (last_ordinal > 0),
    comment_inventory jsonb NOT NULL CHECK (
        jsonb_typeof(comment_inventory) = 'array' AND jsonb_array_length(comment_inventory) > 0
    ),
    inventory_commitment bytea NOT NULL CHECK (octet_length(inventory_commitment) = 32),
    comment_gate_mode text NOT NULL CHECK (comment_gate_mode = 'enabled'),
    previous_certificate_hash bytea CHECK (
        previous_certificate_hash IS NULL OR octet_length(previous_certificate_hash) = 32
    ),
    certificate_hash bytea NOT NULL CHECK (octet_length(certificate_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number, project_id, run_id, goal_id)
        REFERENCES agent_r541_release_roots (trace_number, project_id, run_id, goal_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (trace_number, version),
    UNIQUE (trace_number, certificate_hash)
);

-- Full run-level R5.40 inventory.  The 0034 tool-cluster ledger remains its
-- authority; these rows bind each exact source event into the single release
-- trace rather than reconstructing old attempts from mutable current rows.
CREATE TABLE agent_r540_release_events (
    id uuid PRIMARY KEY,
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    event_kind text NOT NULL CHECK (event_kind IN (
      'work_attempt','work_outcome','blocker_resolution','causal_link','tool_event',
      'evidence','disclosure','model_invocation','interrogation'
    )),
    semantic_tick bigint NOT NULL CHECK (semantic_tick >= 0),
    source_relation text NOT NULL,
    source_record_id uuid NOT NULL,
    event_snapshot jsonb NOT NULL CHECK (jsonb_typeof(event_snapshot)='object'),
    event_hash bytea NOT NULL CHECK (octet_length(event_hash)=32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number,project_id,run_id,goal_id)
      REFERENCES agent_r541_release_roots (trace_number,project_id,run_id,goal_id)
      ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (trace_number,source_relation,source_record_id),
    UNIQUE (trace_number,event_hash)
);

CREATE TABLE agent_r540_release_inventory (
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    project_id uuid NOT NULL,
    ordinal bigint NOT NULL CHECK (ordinal > 0),
    event_id uuid NOT NULL,
    event_kind text NOT NULL CHECK (event_kind IN (
      'work_attempt','work_outcome','blocker_resolution','causal_link','tool_event',
      'evidence','disclosure','model_invocation','interrogation'
    )),
    event_hash bytea NOT NULL CHECK (octet_length(event_hash)=32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (trace_number,ordinal),
    FOREIGN KEY (trace_number,project_id)
      REFERENCES agent_r541_release_roots (trace_number,project_id)
      ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (event_id) REFERENCES agent_r540_release_events(id)
      ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (trace_number,event_id)
);

CREATE TABLE agent_r540_release_certificates (
    id uuid PRIMARY KEY,
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    end_tick bigint NOT NULL CHECK (end_tick >= 0),
    last_ordinal bigint NOT NULL CHECK (last_ordinal >= 0),
    work_attempt_inventory jsonb NOT NULL CHECK (jsonb_typeof(work_attempt_inventory)='array'),
    work_outcome_inventory jsonb NOT NULL CHECK (jsonb_typeof(work_outcome_inventory)='array'),
    blocker_inventory jsonb NOT NULL CHECK (jsonb_typeof(blocker_inventory)='array'),
    causal_inventory jsonb NOT NULL CHECK (jsonb_typeof(causal_inventory)='array'),
    tool_inventory jsonb NOT NULL CHECK (jsonb_typeof(tool_inventory)='array'),
    evidence_inventory jsonb NOT NULL CHECK (jsonb_typeof(evidence_inventory)='array'),
    disclosure_inventory jsonb NOT NULL CHECK (jsonb_typeof(disclosure_inventory)='array'),
    model_inventory jsonb NOT NULL CHECK (jsonb_typeof(model_inventory)='array'),
    interrogation_inventory jsonb NOT NULL CHECK (jsonb_typeof(interrogation_inventory)='array'),
    inventory_commitment bytea NOT NULL CHECK (octet_length(inventory_commitment)=32),
    outcome_gate_mode text NOT NULL CHECK (outcome_gate_mode IN ('enabled','disabled_fail_closed')),
    blocker_gate_mode text NOT NULL CHECK (blocker_gate_mode IN ('enabled','disabled_fail_closed')),
    causal_gate_mode text NOT NULL CHECK (causal_gate_mode IN ('enabled','disabled_fail_closed')),
    tool_gate_mode text NOT NULL CHECK (tool_gate_mode IN ('enabled','disabled_fail_closed')),
    evidence_gate_mode text NOT NULL CHECK (evidence_gate_mode IN ('enabled','disabled_fail_closed')),
    disclosure_gate_mode text NOT NULL CHECK (disclosure_gate_mode IN ('enabled','disabled_fail_closed')),
    model_gate_mode text NOT NULL CHECK (model_gate_mode IN ('enabled','disabled_fail_closed')),
    interrogation_gate_mode text NOT NULL CHECK (interrogation_gate_mode IN ('enabled','disabled_fail_closed')),
    previous_certificate_hash bytea CHECK (previous_certificate_hash IS NULL OR octet_length(previous_certificate_hash)=32),
    certificate_hash bytea NOT NULL CHECK (octet_length(certificate_hash)=32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number,project_id,run_id,goal_id)
      REFERENCES agent_r541_release_roots(trace_number,project_id,run_id,goal_id)
      ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (trace_number,version),
    UNIQUE (trace_number,certificate_hash)
);

CREATE FUNCTION sprout_private.append_agent_r540_release_certificate(
  candidate_trace_number bigint,candidate_end_tick bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE root_row public.agent_r541_release_roots%ROWTYPE;
 previous_row public.agent_r540_release_certificates%ROWTYPE;
 all_inventory jsonb; work_list jsonb; outcome_list jsonb; blocker_list jsonb;
 causal_list jsonb; tool_list jsonb; evidence_list jsonb; disclosure_list jsonb;
 model_list jsonb; interrogation_list jsonb; max_ordinal bigint; next_version integer;
 inventory_hash bytea; candidate_hash bytea;
BEGIN
 SELECT * INTO STRICT root_row FROM public.agent_r541_release_roots
   WHERE trace_number=candidate_trace_number FOR UPDATE;
 SELECT COALESCE(jsonb_agg(jsonb_build_object('ordinal',ordinal,'event_id',event_id,
      'event_hash',encode(event_hash,'hex')) ORDER BY ordinal),'[]'::jsonb),COALESCE(max(ordinal),0),
   COALESCE(jsonb_agg(jsonb_build_object('ordinal',ordinal,'event_id',event_id,'event_hash',encode(event_hash,'hex')) ORDER BY ordinal) FILTER(WHERE event_kind='work_attempt'),'[]'::jsonb),
   COALESCE(jsonb_agg(jsonb_build_object('ordinal',ordinal,'event_id',event_id,'event_hash',encode(event_hash,'hex')) ORDER BY ordinal) FILTER(WHERE event_kind='work_outcome'),'[]'::jsonb),
   COALESCE(jsonb_agg(jsonb_build_object('ordinal',ordinal,'event_id',event_id,'event_hash',encode(event_hash,'hex')) ORDER BY ordinal) FILTER(WHERE event_kind='blocker_resolution'),'[]'::jsonb),
   COALESCE(jsonb_agg(jsonb_build_object('ordinal',ordinal,'event_id',event_id,'event_hash',encode(event_hash,'hex')) ORDER BY ordinal) FILTER(WHERE event_kind='causal_link'),'[]'::jsonb),
   COALESCE(jsonb_agg(jsonb_build_object('ordinal',ordinal,'event_id',event_id,'event_hash',encode(event_hash,'hex')) ORDER BY ordinal) FILTER(WHERE event_kind='tool_event'),'[]'::jsonb),
   COALESCE(jsonb_agg(jsonb_build_object('ordinal',ordinal,'event_id',event_id,'event_hash',encode(event_hash,'hex')) ORDER BY ordinal) FILTER(WHERE event_kind='evidence'),'[]'::jsonb),
   COALESCE(jsonb_agg(jsonb_build_object('ordinal',ordinal,'event_id',event_id,'event_hash',encode(event_hash,'hex')) ORDER BY ordinal) FILTER(WHERE event_kind='disclosure'),'[]'::jsonb),
   COALESCE(jsonb_agg(jsonb_build_object('ordinal',ordinal,'event_id',event_id,'event_hash',encode(event_hash,'hex')) ORDER BY ordinal) FILTER(WHERE event_kind='model_invocation'),'[]'::jsonb),
   COALESCE(jsonb_agg(jsonb_build_object('ordinal',ordinal,'event_id',event_id,'event_hash',encode(event_hash,'hex')) ORDER BY ordinal) FILTER(WHERE event_kind='interrogation'),'[]'::jsonb)
 INTO all_inventory,max_ordinal,work_list,outcome_list,blocker_list,causal_list,tool_list,
   evidence_list,disclosure_list,model_list,interrogation_list
 FROM public.agent_r540_release_inventory WHERE trace_number=candidate_trace_number;
 SELECT * INTO previous_row FROM public.agent_r540_release_certificates
   WHERE trace_number=candidate_trace_number ORDER BY version DESC LIMIT 1;
 IF FOUND AND previous_row.last_ordinal=max_ordinal AND previous_row.end_tick>=candidate_end_tick THEN RETURN; END IF;
 IF FOUND AND previous_row.last_ordinal>max_ordinal THEN
   RAISE EXCEPTION 'R540 release prefix regression' USING ERRCODE='40001'; END IF;
 next_version:=COALESCE(previous_row.version,0)+1;
 inventory_hash:=public.digest(convert_to(all_inventory::text,'UTF8'),'sha256');
 candidate_hash:=public.digest(convert_to(concat_ws(E'\n','sprout-r540-release-certificate-v1',
   candidate_trace_number::text,root_row.project_id::text,root_row.run_id::text,root_row.goal_id::text,
   next_version::text,candidate_end_tick::text,max_ordinal::text,encode(inventory_hash,'hex'),
   COALESCE(encode(previous_row.certificate_hash,'hex'),'')),'UTF8'),'sha256');
 INSERT INTO public.agent_r540_release_certificates (
   id,trace_number,project_id,run_id,goal_id,version,end_tick,last_ordinal,
   work_attempt_inventory,work_outcome_inventory,blocker_inventory,causal_inventory,
   tool_inventory,evidence_inventory,disclosure_inventory,model_inventory,interrogation_inventory,
   inventory_commitment,outcome_gate_mode,blocker_gate_mode,causal_gate_mode,tool_gate_mode,
   evidence_gate_mode,disclosure_gate_mode,model_gate_mode,interrogation_gate_mode,
   previous_certificate_hash,certificate_hash
 ) VALUES (gen_random_uuid(),candidate_trace_number,root_row.project_id,root_row.run_id,
   root_row.goal_id,next_version,candidate_end_tick,max_ordinal,work_list,outcome_list,blocker_list,
   causal_list,tool_list,evidence_list,disclosure_list,model_list,interrogation_list,inventory_hash,
   CASE WHEN jsonb_array_length(outcome_list)>0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
   CASE WHEN jsonb_array_length(blocker_list)>0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
   CASE WHEN jsonb_array_length(causal_list)>0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
   CASE WHEN jsonb_array_length(tool_list)>0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
   CASE WHEN jsonb_array_length(evidence_list)>0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
   CASE WHEN jsonb_array_length(disclosure_list)>0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
   CASE WHEN jsonb_array_length(model_list)>0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
   CASE WHEN jsonb_array_length(interrogation_list)>0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
   previous_row.certificate_hash,candidate_hash);
END $$;

CREATE FUNCTION sprout_private.append_agent_r540_release_event(
  candidate_trace_number bigint,candidate_event_kind text,candidate_semantic_tick bigint,
  candidate_source_relation text,candidate_source_record_id uuid,candidate_snapshot jsonb
) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE root_row public.agent_r541_release_roots%ROWTYPE; candidate_id uuid:=gen_random_uuid();
 candidate_hash bytea; existing_id uuid; next_ordinal bigint;
BEGIN
 SELECT * INTO STRICT root_row FROM public.agent_r541_release_roots
   WHERE trace_number=candidate_trace_number FOR UPDATE;
 IF candidate_event_kind NOT IN ('work_attempt','work_outcome','blocker_resolution','causal_link',
   'tool_event','evidence','disclosure','model_invocation','interrogation')
   OR candidate_semantic_tick<root_row.start_tick OR jsonb_typeof(candidate_snapshot)<>'object' THEN
   RAISE EXCEPTION 'invalid R540 event projection' USING ERRCODE='23514'; END IF;
 candidate_hash:=public.digest(convert_to(concat_ws(E'\n','sprout-r540-release-event-v1',
   candidate_trace_number::text,candidate_event_kind,candidate_semantic_tick::text,
   candidate_source_relation,candidate_source_record_id::text,candidate_snapshot::text),'UTF8'),'sha256');
 INSERT INTO public.agent_r540_release_events(id,trace_number,project_id,run_id,goal_id,event_kind,
   semantic_tick,source_relation,source_record_id,event_snapshot,event_hash)
 VALUES(candidate_id,candidate_trace_number,root_row.project_id,root_row.run_id,root_row.goal_id,
   candidate_event_kind,candidate_semantic_tick,candidate_source_relation,candidate_source_record_id,
   candidate_snapshot,candidate_hash)
 ON CONFLICT(trace_number,source_relation,source_record_id) DO NOTHING RETURNING id INTO existing_id;
 IF existing_id IS NULL THEN
   SELECT id INTO STRICT existing_id FROM public.agent_r540_release_events
    WHERE trace_number=candidate_trace_number AND source_relation=candidate_source_relation
      AND source_record_id=candidate_source_record_id AND event_kind=candidate_event_kind
      AND semantic_tick=candidate_semantic_tick AND event_hash=candidate_hash;
   RETURN existing_id;
 END IF;
 SELECT COALESCE(max(ordinal),0)+1 INTO next_ordinal FROM public.agent_r540_release_inventory
   WHERE trace_number=candidate_trace_number;
 INSERT INTO public.agent_r540_release_inventory(trace_number,project_id,ordinal,event_id,event_kind,event_hash)
 VALUES(candidate_trace_number,root_row.project_id,next_ordinal,existing_id,candidate_event_kind,candidate_hash);
 PERFORM sprout_private.append_agent_r540_release_certificate(candidate_trace_number,candidate_semantic_tick);
 RETURN existing_id;
END $$;

CREATE FUNCTION sprout_private.project_agent_r540_transition_event()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE root_row public.agent_r541_release_roots%ROWTYPE; claim_entry record;
 work_snapshot jsonb; claim_snapshot jsonb; prior_transition public.agent_run_transitions%ROWTYPE;
 changed_work record;
BEGIN
 SELECT * INTO root_row FROM public.agent_r541_release_roots
   WHERE project_id=NEW.project_id AND run_id=NEW.run_id;
 IF NOT FOUND OR NEW.semantic_tick IS NULL THEN RETURN NEW; END IF;
 IF NEW.transition_kind='work_claimed' THEN
   FOR claim_entry IN SELECT key,value FROM jsonb_each(NEW.state_snapshot->'claims')
     WHERE value->>'status'='active' AND (value->>'acquired_at')::bigint=NEW.semantic_tick
   LOOP
     work_snapshot:=NEW.state_snapshot->'work_items'->(claim_entry.value->>'work');
     IF work_snapshot IS NULL OR work_snapshot->>'kind' IN ('tool_invocation','tool_retry') THEN CONTINUE; END IF;
     PERFORM sprout_private.append_agent_r540_release_event(root_row.trace_number,'work_attempt',
       NEW.semantic_tick,'agent_run_transitions',NEW.id,jsonb_build_object(
         'trace_id',root_row.trace_number,'run',NEW.run_id,'goal',root_row.goal_id,
         'work',(claim_entry.value->>'work')::uuid,'claim',claim_entry.key::uuid,
         'attempt',(claim_entry.value->>'attempt')::integer,
         'actor',(claim_entry.value->>'claimant')::uuid,'tick',NEW.semantic_tick,
         'work_snapshot',work_snapshot,'claim_snapshot',claim_entry.value,
         'transition_id',NEW.id));
   END LOOP;
 ELSIF NEW.transition_kind IN ('work_succeeded','work_failed') THEN
   SELECT * INTO prior_transition FROM public.agent_run_transitions prior
     WHERE prior.project_id=NEW.project_id AND prior.run_id=NEW.run_id
       AND prior.next_state_hash=NEW.previous_state_hash ORDER BY prior.state_version DESC LIMIT 1;
   IF NOT FOUND THEN RETURN NEW; END IF;
   FOR changed_work IN
     SELECT current.key,current.value FROM jsonb_each(NEW.state_snapshot->'work_items') current
     JOIN jsonb_each(prior_transition.state_snapshot->'work_items') prior ON prior.key=current.key
     WHERE current.value->>'status' IN ('succeeded','failed')
       AND prior.value->>'status' IS DISTINCT FROM current.value->>'status'
       -- Tool work coordinates already have the immutable, attempt-exact 0034
       -- WorkOutcome projector.  Projecting the generic transition as well
       -- would duplicate one formal WorkOutcome in the run-level inventory.
       AND current.value->>'kind' NOT IN ('tool_invocation','tool_retry')
   LOOP
     SELECT value INTO claim_snapshot FROM jsonb_each(NEW.state_snapshot->'claims')
       WHERE value->>'work'=changed_work.key
         AND (value->>'attempt')::integer=(changed_work.value->>'attempt')::integer
       ORDER BY key LIMIT 1;
     IF claim_snapshot IS NULL THEN CONTINUE; END IF;
     PERFORM sprout_private.append_agent_r540_release_event(root_row.trace_number,'work_outcome',
       NEW.semantic_tick,'agent_run_transitions',NEW.id,jsonb_build_object(
         'trace_id',root_row.trace_number,'run',NEW.run_id,'goal',root_row.goal_id,
         'work',changed_work.key::uuid,'claim',(claim_snapshot->>'id')::uuid,
         'attempt',(changed_work.value->>'attempt')::integer,
         'status',changed_work.value->>'status','observed_at',NEW.semantic_tick,
         'work_snapshot',changed_work.value,'claim_snapshot',claim_snapshot,
         'transition_id',NEW.id));
   END LOOP;
 END IF;
 RETURN NEW;
END $$;

CREATE TRIGGER agent_r540_project_transition
AFTER INSERT ON agent_run_transitions FOR EACH ROW
EXECUTE FUNCTION sprout_private.project_agent_r540_transition_event();

CREATE FUNCTION sprout_private.project_agent_r540_blocker_resolution()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE root_row public.agent_r541_release_roots%ROWTYPE; transition_row public.agent_run_transitions%ROWTYPE;
BEGIN
 SELECT * INTO root_row FROM public.agent_r541_release_roots WHERE project_id=NEW.project_id AND run_id=NEW.run_id;
 IF NOT FOUND THEN RETURN NEW; END IF;
 SELECT * INTO STRICT transition_row FROM public.agent_run_transitions WHERE id=NEW.transition_id
   AND transition_kind='blocker_resolved' AND project_id=NEW.project_id AND run_id=NEW.run_id;
 PERFORM sprout_private.append_agent_r540_release_event(root_row.trace_number,'blocker_resolution',
   transition_row.semantic_tick,'agent_run_blocker_resolutions',NEW.id,jsonb_build_object(
    'trace_id',root_row.trace_number,'run',NEW.run_id,'goal',root_row.goal_id,
    'blocker',NEW.blocker_id,'resolution',jsonb_build_object('blocker',NEW.blocker_id,
      'observation_kind',NEW.observation_kind,'observation_id',NEW.observation_id,
      'terminal_status',NEW.terminal_status,'observed_at',transition_row.semantic_tick,
      'provenance_hash',encode(NEW.provenance_hash,'hex')),
    'observed_at',transition_row.semantic_tick,'transition_id',NEW.transition_id));
 RETURN NEW;
END $$;
CREATE TRIGGER agent_r540_project_blocker_resolution AFTER INSERT ON agent_run_blocker_resolutions
FOR EACH ROW EXECUTE FUNCTION sprout_private.project_agent_r540_blocker_resolution();

CREATE FUNCTION sprout_private.project_agent_r540_causal_link()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE root_row public.agent_r541_release_roots%ROWTYPE; transition_tick bigint;
BEGIN
 SELECT * INTO root_row FROM public.agent_r541_release_roots WHERE project_id=NEW.project_id AND run_id=NEW.run_id;
 IF NOT FOUND THEN RETURN NEW; END IF;
 SELECT semantic_tick INTO STRICT transition_tick FROM public.agent_run_transitions
   WHERE id=NEW.transition_id AND project_id=NEW.project_id AND run_id=NEW.run_id;
 PERFORM sprout_private.append_agent_r540_release_event(root_row.trace_number,'causal_link',
   transition_tick,'agent_run_causal_links',NEW.id,jsonb_build_object(
    'trace_id',root_row.trace_number,'run',NEW.run_id,'goal',NEW.goal_id,
    'link',jsonb_build_object('run',NEW.run_id,'goal',NEW.goal_id,'predecessor',NEW.predecessor,
      'successor',NEW.successor,'observed_at',NEW.observed_tick),
    'recorded_at',transition_tick,'transition_id',NEW.transition_id));
 RETURN NEW;
END $$;
CREATE TRIGGER agent_r540_project_causal_link AFTER INSERT ON agent_run_causal_links
FOR EACH ROW EXECUTE FUNCTION sprout_private.project_agent_r540_causal_link();

CREATE FUNCTION sprout_private.project_agent_r540_evidence()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE root_row public.agent_r541_release_roots%ROWTYPE; transition_row public.agent_run_transitions%ROWTYPE;
 outcome_row public.agent_run_work_outcomes%ROWTYPE;
 claim_row public.agent_run_claim_leases%ROWTYPE;
 work_snapshot jsonb;
 evidence_subject jsonb;
 exact_rule jsonb;
 evidence_observed_tick bigint;
 exact_claim_count bigint;
 exact_work_event_count bigint;
BEGIN
 SELECT * INTO root_row FROM public.agent_r541_release_roots WHERE project_id=NEW.project_id AND run_id=NEW.run_id;
 IF NOT FOUND THEN RETURN NEW; END IF;
 SELECT * INTO STRICT transition_row FROM public.agent_run_transitions WHERE id=NEW.transition_id
   AND transition_kind IN ('evidence_accepted','work_succeeded')
   AND project_id=NEW.project_id AND run_id=NEW.run_id;
 work_snapshot:=transition_row.state_snapshot->'work_items'->NEW.work_item_id::text;
 IF work_snapshot IS NULL
    OR work_snapshot->>'run' IS DISTINCT FROM NEW.run_id::text
    OR work_snapshot->>'goal' IS DISTINCT FROM root_row.goal_id::text
    OR work_snapshot->>'id' IS DISTINCT FROM NEW.work_item_id::text
    OR work_snapshot->>'serves' IS DISTINCT FROM NEW.obligation_id::text
    OR work_snapshot->>'attempt' IS NULL THEN
   RAISE EXCEPTION 'evidence WorkItem snapshot or obligation mismatch' USING ERRCODE='23514';
 END IF;

 -- The product-event outcome is the authoritative bridge from accepted evidence
 -- to the historical WorkAttempt.  Never infer this coordinate from the latest
 -- lease for the WorkItem: a retry may already have materialized attempt N+1.
 SELECT * INTO STRICT outcome_row FROM public.agent_run_work_outcomes
  WHERE project_id=NEW.project_id AND run_id=NEW.run_id
    AND work_item_id=NEW.work_item_id
    AND outcome_kind=NEW.product_event_kind AND product_event_id=NEW.product_event_id
    AND observed_at=NEW.observed_at;
 IF (work_snapshot->>'attempt')::integer IS DISTINCT FROM outcome_row.attempt THEN
   RAISE EXCEPTION 'evidence accepted WorkItem attempt mismatch' USING ERRCODE='23514';
 END IF;

 SELECT count(*) INTO exact_claim_count
 FROM public.agent_run_claim_leases
 WHERE project_id=NEW.project_id AND run_id=NEW.run_id
   AND work_item_id=NEW.work_item_id AND attempt=outcome_row.attempt;
 IF exact_claim_count<>1 THEN
   RAISE EXCEPTION 'evidence historical claim coordinate is missing or ambiguous' USING ERRCODE='23514';
 END IF;
 SELECT * INTO STRICT claim_row FROM public.agent_run_claim_leases
  WHERE project_id=NEW.project_id AND id=outcome_row.claim_id
    AND run_id=NEW.run_id AND work_item_id=NEW.work_item_id
    AND attempt=outcome_row.attempt;

 SELECT transition.semantic_tick INTO STRICT evidence_observed_tick
 FROM public.agent_run_transitions transition
 WHERE transition.id=outcome_row.transition_id
   AND transition.project_id=NEW.project_id AND transition.run_id=NEW.run_id
   AND transition.transition_kind='work_succeeded'
   AND transition.semantic_tick<=transition_row.semantic_tick;

 IF NEW.product_event_kind<>'task_completion' OR NEW.evidence_kind<>'task_completed'
    OR NEW.verification_mode<>'mechanical' THEN
   RAISE EXCEPTION 'evidence family lacks a persisted exact verification witness'
     USING ERRCODE='23514';
 END IF;
 SELECT jsonb_build_object('kind','task','task',task.resource_node_id)
   INTO STRICT evidence_subject
 FROM public.task_completions completion
 JOIN public.tasks task ON task.project_id=completion.project_id AND task.id=completion.task_id
 WHERE completion.project_id=NEW.project_id AND completion.id=NEW.product_event_id
   AND completion.completed_at=NEW.observed_at AND task.state='completed';
 SELECT rule INTO STRICT exact_rule
 FROM public.agent_collaborative_runs run,
      LATERAL jsonb_array_elements(run.contract->'evidence_rules') rule
 WHERE run.project_id=NEW.project_id AND run.id=NEW.run_id
   AND (rule->>'id')::bigint=NEW.evidence_rule_ordinal
   AND rule->>'obligation' IS NOT DISTINCT FROM NEW.obligation_id::text
   AND rule->>'kind' IS NOT DISTINCT FROM NEW.evidence_kind
   AND rule->>'verification' IS NOT DISTINCT FROM NEW.verification_mode
   AND rule#>>'{subject,kind}' IS NOT DISTINCT FROM 'work_result'
   AND (rule#>>'{subject,work_spec_id}')::bigint=
       (work_snapshot->>'work_spec_id')::bigint;

 -- Require the already-projected exact WorkAttempt, whether it came from the
 -- native transition projector or the 0034 tool-attempt projector.
 SELECT count(*) INTO exact_work_event_count
 FROM public.agent_r540_exact_release_events projected
 WHERE projected.trace_number=root_row.trace_number
   AND projected.event_kind='work_attempt'
   AND COALESCE(projected.event_snapshot->>'work',
                projected.event_snapshot->>'work_item_id')=NEW.work_item_id::text
   AND COALESCE(projected.event_snapshot->>'claim',
                projected.event_snapshot->>'claim_id')=outcome_row.claim_id::text
   AND (projected.event_snapshot->>'attempt')::integer=outcome_row.attempt;
 IF exact_work_event_count<>1 THEN
   RAISE EXCEPTION 'evidence has no unique exact projected WorkAttempt' USING ERRCODE='23514';
 END IF;
 PERFORM sprout_private.append_agent_r540_release_event(root_row.trace_number,'evidence',
   transition_row.semantic_tick,'agent_run_evidence_provenance',NEW.evidence_id,jsonb_build_object(
    'trace_id',root_row.trace_number,'run',NEW.run_id,'goal',root_row.goal_id,
    'work',NEW.work_item_id,'claim',outcome_row.claim_id,'attempt',outcome_row.attempt,
    'evidence',jsonb_build_object('id',NEW.evidence_id,'run',NEW.run_id,'obligation',NEW.obligation_id,
      'rule_id',NEW.evidence_rule_ordinal,'kind',NEW.evidence_kind,
      'subject',evidence_subject,'observed_at',evidence_observed_tick,
      'verification',NEW.verification_mode,'rule',exact_rule,
      'provenance_hash',encode(NEW.provenance_hash,'hex')),
    'accepted_at',transition_row.semantic_tick,'transition_id',NEW.transition_id,
    'work_snapshot',work_snapshot));
 RETURN NEW;
END $$;
CREATE TRIGGER agent_r540_project_evidence AFTER INSERT ON agent_run_evidence_provenance
FOR EACH ROW EXECUTE FUNCTION sprout_private.project_agent_r540_evidence();

CREATE FUNCTION sprout_private.project_agent_r540_tool_cluster_event()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE root_row public.agent_r541_release_roots%ROWTYPE;
BEGIN
 SELECT * INTO root_row FROM public.agent_r541_release_roots
   WHERE trace_number=NEW.trace_number AND project_id=NEW.project_id AND run_id=NEW.run_id;
 IF NOT FOUND THEN RETURN NEW; END IF;
 -- `NEW` is a polymorphic trigger record.  Referencing `observed_tick` in a
 -- SQL CASE still attempts to resolve that field for WorkAttempt rows, whose
 -- formal tick column is named `tick`.  Keep the three typed projections in
 -- separate PL/pgSQL branches so no non-existent coordinate is inspected.
 IF TG_TABLE_NAME='agent_r540_work_attempt_events' THEN
   PERFORM sprout_private.append_agent_r540_release_event(
     NEW.trace_number,'work_attempt',NEW.tick,TG_TABLE_NAME,NEW.id,to_jsonb(NEW));
 ELSIF TG_TABLE_NAME='agent_r540_work_outcome_events' THEN
   PERFORM sprout_private.append_agent_r540_release_event(
     NEW.trace_number,'work_outcome',NEW.observed_tick,TG_TABLE_NAME,NEW.id,to_jsonb(NEW));
 ELSIF TG_TABLE_NAME='agent_r540_tool_attempt_events' THEN
   PERFORM sprout_private.append_agent_r540_release_event(
     NEW.trace_number,'tool_event',NEW.observed_tick,TG_TABLE_NAME,NEW.id,to_jsonb(NEW));
 ELSE
   RAISE EXCEPTION 'unsupported R540 tool-cluster projection source'
     USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER agent_r540_release_project_tool_work AFTER INSERT ON agent_r540_work_attempt_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.project_agent_r540_tool_cluster_event();
CREATE TRIGGER agent_r540_release_project_tool_event AFTER INSERT ON agent_r540_tool_attempt_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.project_agent_r540_tool_cluster_event();
CREATE TRIGGER agent_r540_release_project_tool_outcome AFTER INSERT ON agent_r540_work_outcome_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.project_agent_r540_tool_cluster_event();

CREATE FUNCTION sprout_private.project_agent_r540_model_invocation()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE root_row public.agent_r541_release_roots%ROWTYPE; invocation_row public.agent_invocations%ROWTYPE;
 dispatch_row public.agent_model_attempt_dispatches%ROWTYPE; observation_row public.agent_model_attempt_observations%ROWTYPE;
BEGIN
 IF NEW.status<>'succeeded' OR NEW.run_id IS NULL OR NEW.goal_id IS NULL
    OR NEW.work_item_id IS NULL OR NEW.work_claim_id IS NULL OR NEW.work_attempt IS NULL THEN RETURN NEW; END IF;
 SELECT * INTO root_row FROM public.agent_r541_release_roots
   WHERE project_id=NEW.project_id AND run_id=NEW.run_id AND goal_id=NEW.goal_id;
 IF NOT FOUND THEN RETURN NEW; END IF;
 SELECT * INTO invocation_row FROM public.agent_invocations
   WHERE project_id=NEW.project_id AND id=NEW.invocation_id AND status='succeeded';
 IF NOT FOUND THEN RETURN NEW; END IF;
 SELECT * INTO STRICT dispatch_row FROM public.agent_model_attempt_dispatches
   WHERE project_id=NEW.project_id AND invocation_id=NEW.invocation_id AND attempt=NEW.provider_attempt;
 SELECT * INTO STRICT observation_row FROM public.agent_model_attempt_observations
   WHERE project_id=NEW.project_id AND id=NEW.observation_id AND invocation_id=NEW.invocation_id
     AND attempt=NEW.provider_attempt AND status='succeeded';
 IF NEW.semantic_tick IS NULL OR dispatch_row.semantic_tick IS NULL
    OR NEW.semantic_tick IS DISTINCT FROM dispatch_row.semantic_tick THEN RETURN NEW; END IF;
 PERFORM sprout_private.append_agent_r540_release_event(root_row.trace_number,'model_invocation',
   NEW.semantic_tick,'agent_model_invocation_projections',NEW.id,
   jsonb_build_object('trace_id',root_row.trace_number,'run',NEW.run_id,'goal',NEW.goal_id,
     'work',NEW.work_item_id,'claim',NEW.work_claim_id,'attempt',NEW.work_attempt,
     'principal',NEW.principal_identity_id,
     'context',jsonb_build_object('direct_sources',NEW.context_source_descriptors),
     'projection',jsonb_build_object('request_commitment',encode(NEW.request_commitment,'hex'),
       'context_commitment',encode(NEW.context_commitment,'hex'),
       'direct_sources_exposed',NEW.context_source_descriptors,
       'hidden_persistent_model_memory_available',false),
     'input_payload_pointer',jsonb_build_object('relation','agent_invocations','id',invocation_row.id,
       'commitment',encode(public.digest(invocation_row.encrypted_input,'sha256'),'hex')),
     'output_payload_pointer',jsonb_build_object('relation','agent_invocations','id',invocation_row.id,
       'commitment',encode(public.digest(invocation_row.encrypted_output,'sha256'),'hex')),
     'invoked_at',NEW.semantic_tick,
     'dispatch_id',dispatch_row.id,'observation_id',observation_row.id));
 RETURN NEW;
END $$;
CREATE TRIGGER agent_r540_project_model_invocation AFTER INSERT ON agent_model_invocation_projections
FOR EACH ROW EXECUTE FUNCTION sprout_private.project_agent_r540_model_invocation();

CREATE FUNCTION sprout_private.project_agent_r540_completed_model_invocation()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE projection_row public.agent_model_invocation_projections%ROWTYPE;
 root_row public.agent_r541_release_roots%ROWTYPE;
 dispatch_row public.agent_model_attempt_dispatches%ROWTYPE;
 observation_row public.agent_model_attempt_observations%ROWTYPE;
BEGIN
 IF NEW.status<>'succeeded' OR OLD.status='succeeded' THEN RETURN NEW; END IF;
 FOR projection_row IN SELECT * FROM public.agent_model_invocation_projections
   WHERE project_id=NEW.project_id AND invocation_id=NEW.id AND status='succeeded'
     AND run_id IS NOT NULL AND goal_id IS NOT NULL AND work_item_id IS NOT NULL
     AND work_claim_id IS NOT NULL AND work_attempt IS NOT NULL
 LOOP
   SELECT * INTO root_row FROM public.agent_r541_release_roots
     WHERE project_id=NEW.project_id AND run_id=projection_row.run_id AND goal_id=projection_row.goal_id;
   IF NOT FOUND THEN CONTINUE; END IF;
   SELECT * INTO STRICT dispatch_row FROM public.agent_model_attempt_dispatches
     WHERE project_id=NEW.project_id AND invocation_id=NEW.id AND attempt=projection_row.provider_attempt;
   SELECT * INTO STRICT observation_row FROM public.agent_model_attempt_observations
     WHERE project_id=NEW.project_id AND id=projection_row.observation_id
       AND invocation_id=NEW.id AND attempt=projection_row.provider_attempt AND status='succeeded';
   IF projection_row.semantic_tick IS NULL OR dispatch_row.semantic_tick IS NULL
      OR projection_row.semantic_tick IS DISTINCT FROM dispatch_row.semantic_tick THEN CONTINUE; END IF;
   PERFORM sprout_private.append_agent_r540_release_event(root_row.trace_number,'model_invocation',
     projection_row.semantic_tick,
     'agent_model_invocation_projections',projection_row.id,
     jsonb_build_object('trace_id',root_row.trace_number,'run',projection_row.run_id,
       'goal',projection_row.goal_id,'work',projection_row.work_item_id,
       'claim',projection_row.work_claim_id,'attempt',projection_row.work_attempt,
       'principal',projection_row.principal_identity_id,
       'context',jsonb_build_object('direct_sources',projection_row.context_source_descriptors),
       'projection',jsonb_build_object('request_commitment',encode(projection_row.request_commitment,'hex'),
         'context_commitment',encode(projection_row.context_commitment,'hex'),
         'direct_sources_exposed',projection_row.context_source_descriptors,
         'hidden_persistent_model_memory_available',false),
       'input_payload_pointer',jsonb_build_object('relation','agent_invocations','id',NEW.id,
         'commitment',encode(public.digest(NEW.encrypted_input,'sha256'),'hex')),
       'output_payload_pointer',jsonb_build_object('relation','agent_invocations','id',NEW.id,
         'commitment',encode(public.digest(NEW.encrypted_output,'sha256'),'hex')),
       'invoked_at',projection_row.semantic_tick,
       'dispatch_id',dispatch_row.id,'observation_id',observation_row.id));
 END LOOP;
 RETURN NEW;
END $$;
CREATE TRIGGER agent_r540_project_completed_model_invocation
AFTER UPDATE OF status ON agent_invocations FOR EACH ROW
EXECUTE FUNCTION sprout_private.project_agent_r540_completed_model_invocation();

CREATE FUNCTION sprout_private.project_agent_r540_interrogation()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE projection_row public.agent_model_invocation_projections%ROWTYPE;
 root_row public.agent_r541_release_roots%ROWTYPE; interrogation_row public.agent_interrogations%ROWTYPE;
BEGIN
 SELECT * INTO projection_row FROM public.agent_model_invocation_projections
   WHERE project_id=NEW.project_id AND invocation_id=NEW.invocation_id AND status='succeeded';
 IF NOT FOUND OR projection_row.run_id IS NULL THEN RETURN NEW; END IF;
 SELECT * INTO root_row FROM public.agent_r541_release_roots
   WHERE project_id=NEW.project_id AND run_id=projection_row.run_id AND goal_id=projection_row.goal_id;
 IF NOT FOUND THEN RETURN NEW; END IF;
 SELECT * INTO STRICT interrogation_row FROM public.agent_interrogations
   WHERE project_id=NEW.project_id AND id=NEW.interrogation_id;
 IF NEW.semantic_tick IS NULL OR interrogation_row.semantic_tick IS NULL
    OR interrogation_row.semantic_tick>NEW.semantic_tick THEN RETURN NEW; END IF;
 PERFORM sprout_private.append_agent_r540_release_event(root_row.trace_number,'interrogation',
   NEW.semantic_tick,'agent_interrogation_answers',NEW.id,
   jsonb_build_object('trace_id',root_row.trace_number,'session_id',NEW.interrogation_id,
     'session',jsonb_build_object('id',NEW.interrogation_id,'creator',interrogation_row.creator_identity_id,
       'target_agent',interrogation_row.target_agent_identity_id,
       'created_at',interrogation_row.semantic_tick,'via_tool_call',NULL),
     'question_payload_pointer',jsonb_build_object('relation','agent_interrogations',
       'id',interrogation_row.id,'commitment',encode(public.digest(interrogation_row.encrypted_transcript,'sha256'),'hex')),
     'question_asked_at',interrogation_row.semantic_tick,
     'question_state_fingerprint',encode(NEW.question_state_fingerprint,'hex'),
     'answer_payload_pointer',jsonb_build_object('relation','agent_interrogation_answers',
       'id',NEW.id,'commitment',encode(public.digest(NEW.encrypted_answer,'sha256'),'hex')),
     'answer_responder',interrogation_row.target_agent_identity_id,
     'answer_answered_at',NEW.semantic_tick,
     'answer_state_fingerprint',encode(NEW.answer_state_fingerprint,'hex'),
     'delta',interrogation_row.causal_delta,'context',NEW.context_source_descriptors,
     'projection_id',projection_row.id,'observed_at',NEW.semantic_tick));
 RETURN NEW;
END $$;
CREATE TRIGGER agent_r540_project_interrogation AFTER INSERT ON agent_interrogation_answers
FOR EACH ROW EXECUTE FUNCTION sprout_private.project_agent_r540_interrogation();

CREATE FUNCTION sprout_private.project_agent_r540_comment_disclosure()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE root_row public.agent_r541_release_roots%ROWTYPE;
BEGIN
 SELECT * INTO STRICT root_row FROM public.agent_r541_release_roots
   WHERE project_id=NEW.project_id AND run_id=NEW.run_id AND goal_id=NEW.goal_id;
 PERFORM sprout_private.append_agent_r540_release_event(root_row.trace_number,'disclosure',
   NEW.observed_tick,'agent_native_comment_security_effects',NEW.id,
   jsonb_build_object('trace_id',root_row.trace_number,'run',NEW.run_id,'goal',NEW.goal_id,
    'work',NEW.work_item_id,'claim',NEW.claim_id,'attempt',NEW.attempt,'actor',NEW.actor_identity_id,
    'sink',jsonb_build_object('kind','comment_on','target',NEW.target_resource_node_id),
    'sources',NEW.context_sources,
    'payload_pointer',jsonb_build_object('relation','native_comments','id',NEW.comment_id,
      'commitment',encode(NEW.payload_commitment,'hex')),
    'observed_at',NEW.observed_tick,'comment_id',NEW.comment_id));
 RETURN NEW;
END $$;
CREATE TRIGGER agent_r540_project_comment_disclosure AFTER INSERT ON agent_native_comment_security_effects
FOR EACH ROW EXECUTE FUNCTION sprout_private.project_agent_r540_comment_disclosure();

CREATE FUNCTION sprout_private.project_agent_r540_applied_disclosure()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE projection_row public.agent_model_invocation_projections%ROWTYPE;
 root_row public.agent_r541_release_roots%ROWTYPE; applied_tick bigint;
BEGIN
 IF NEW.status<>'applied' OR OLD.status='applied' OR NEW.encrypted_materialization IS NULL THEN RETURN NEW; END IF;
 SELECT * INTO projection_row FROM public.agent_model_invocation_projections
   WHERE project_id=NEW.project_id AND invocation_id=NEW.invocation_id AND status='succeeded';
 IF NOT FOUND OR projection_row.run_id IS NULL THEN RETURN NEW; END IF;
 SELECT * INTO root_row FROM public.agent_r541_release_roots WHERE project_id=NEW.project_id
   AND run_id=projection_row.run_id AND goal_id=projection_row.goal_id;
 IF NOT FOUND THEN RETURN NEW; END IF;
 applied_tick:=NEW.applied_semantic_tick;
 IF applied_tick IS NULL THEN RETURN NEW; END IF;
 PERFORM sprout_private.append_agent_r540_release_event(root_row.trace_number,'disclosure',applied_tick,
   'agent_effect_proposals',NEW.id,jsonb_build_object('trace_id',root_row.trace_number,
     'run',projection_row.run_id,'goal',projection_row.goal_id,'work',projection_row.work_item_id,
     'claim',projection_row.work_claim_id,'attempt',projection_row.work_attempt,
     'actor',projection_row.principal_identity_id,'effect',NEW.effect,
     'sink',CASE WHEN NEW.effect#>>'{effect,operation}'='edit_info'
       THEN jsonb_build_object('kind','info_document','container',NEW.effect#>>'{effect,resource_id}')
       ELSE NULL END,
     'sources',projection_row.context_source_descriptors,
     'payload_pointer',jsonb_build_object('relation','agent_effect_proposals','id',NEW.id,
       'commitment',encode(public.digest(NEW.encrypted_materialization,'sha256'),'hex')),
     'observed_at',applied_tick,'invocation_id',NEW.invocation_id));
 RETURN NEW;
END $$;
CREATE TRIGGER agent_r540_project_applied_disclosure AFTER UPDATE OF status ON agent_effect_proposals
FOR EACH ROW EXECUTE FUNCTION sprout_private.project_agent_r540_applied_disclosure();

CREATE FUNCTION sprout_private.reject_agent_formal_release_history_mutation()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
BEGIN
    IF TG_TABLE_NAME = 'native_comments' AND TG_OP = 'UPDATE'
       AND OLD.payload_purged_at IS NULL AND NEW.payload_purged_at IS NOT NULL
       AND NEW.encrypted_payload IS NULL
       AND (to_jsonb(OLD) - ARRAY['encrypted_payload','payload_purged_at'])
           = (to_jsonb(NEW) - ARRAY['encrypted_payload','payload_purged_at'])
       AND NULLIF(current_setting('app.agent_comment_retention', true), '') = 'authorized'
    THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'agent formal release history is append-only' USING ERRCODE = '55000';
END
$$;

CREATE TRIGGER agent_coordination_policy_immutable BEFORE UPDATE OR DELETE ON agent_coordination_policy_versions
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER agent_release_roots_immutable BEFORE UPDATE OR DELETE ON agent_r541_release_roots
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER agent_run_semantic_tick_allocations_immutable BEFORE UPDATE OR DELETE ON agent_run_semantic_tick_allocations
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER agent_tool_attempt_clock_bindings_immutable BEFORE UPDATE OR DELETE ON agent_tool_attempt_clock_bindings
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER native_comments_immutable BEFORE UPDATE OR DELETE ON native_comments
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER native_comment_events_immutable BEFORE UPDATE OR DELETE ON native_comment_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER native_comment_notifications_immutable BEFORE UPDATE OR DELETE ON native_comment_notifications
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER native_comment_responses_immutable BEFORE UPDATE OR DELETE ON native_comment_responses
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER agent_native_comment_effects_immutable BEFORE UPDATE OR DELETE ON agent_native_comment_security_effects
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER agent_r541_comment_records_immutable BEFORE UPDATE OR DELETE ON agent_r541_comment_records
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER agent_r541_comment_inventory_immutable BEFORE UPDATE OR DELETE ON agent_r541_comment_inventory
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER agent_r541_comment_certificates_immutable BEFORE UPDATE OR DELETE ON agent_r541_comment_certificates
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER agent_r540_release_events_immutable BEFORE UPDATE OR DELETE ON agent_r540_release_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER agent_r540_release_inventory_immutable BEFORE UPDATE OR DELETE ON agent_r540_release_inventory
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
CREATE TRIGGER agent_r540_release_certificates_immutable BEFORE UPDATE OR DELETE ON agent_r540_release_certificates
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();

CREATE FUNCTION sprout_private.comment_permission_allowed(
    candidate_project_id uuid,
    candidate_resource_node_id uuid,
    candidate_identity_id uuid,
    candidate_operation text
) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
    SELECT candidate_operation IN ('read_comment', 'post_comment')
      AND EXISTS (
        SELECT 1
        FROM public.resource_nodes node
        JOIN public.project_memberships membership
          ON membership.project_id = node.project_id
         AND membership.identity_id = candidate_identity_id
         AND membership.state = 'active'
        JOIN LATERAL sprout_private.effective_domain_permission(
          candidate_project_id, candidate_resource_node_id, candidate_identity_id
        ) permission ON true
        WHERE node.project_id = candidate_project_id
          AND node.id = candidate_resource_node_id
          AND node.deleted_at IS NULL
          AND permission.access_scope = 'full'
          AND permission.access_level IN ('comment', 'edit', 'manage')
      )
$$;

-- Safe typed-payload resolver used by every independent exact projection.
-- It is declared before operational snapshots so SQL function validation
-- cannot accidentally bind to a later or caller-controlled resolver.
CREATE FUNCTION sprout_private.try_parse_encrypted_payload(candidate bytea)
RETURNS jsonb LANGUAGE plpgsql IMMUTABLE STRICT
SET search_path=pg_catalog AS $$
DECLARE parsed jsonb;
BEGIN
 parsed:=pg_catalog.convert_from(candidate,'UTF8')::jsonb;
 IF jsonb_typeof(parsed)<>'object'
    OR jsonb_typeof(parsed->'version')<>'number'
    OR jsonb_typeof(parsed->'algorithm')<>'string'
    OR jsonb_typeof(parsed->'key_id')<>'string'
    OR jsonb_typeof(parsed->'nonce_b64')<>'string'
    OR jsonb_typeof(parsed->'ciphertext_b64')<>'string' THEN
   RETURN NULL;
 END IF;
 RETURN parsed;
EXCEPTION WHEN OTHERS THEN RETURN NULL;
END $$;

CREATE TABLE agent_r541_task_operational_bindings (
    id uuid PRIMARY KEY,
    trace_number bigint NOT NULL CHECK (trace_number>0),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    task_effect_id uuid NOT NULL,
    intent_record jsonb,
    provenance_record jsonb NOT NULL CHECK (jsonb_typeof(provenance_record)='object'),
    binding_hash bytea NOT NULL CHECK (octet_length(binding_hash)=32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number,project_id,run_id)
      REFERENCES agent_r541_release_roots(trace_number,project_id,run_id)
      ON UPDATE RESTRICT ON DELETE RESTRICT,
    -- The operational effect can be retention-purged.  Exact served views
    -- rejoin it; the immutable binding must not block that lifecycle.
    UNIQUE (trace_number,task_effect_id),
    UNIQUE (trace_number,binding_hash),
    CHECK (intent_record IS NULL OR jsonb_typeof(intent_record)='object')
);
CREATE TRIGGER agent_r541_task_operational_bindings_immutable BEFORE UPDATE OR DELETE
ON agent_r541_task_operational_bindings FOR EACH ROW
EXECUTE FUNCTION sprout_private.reject_agent_formal_release_history_mutation();
ALTER TABLE agent_r541_task_operational_bindings ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_task_operational_bindings FORCE ROW LEVEL SECURITY;
CREATE POLICY agent_r541_task_operational_binding_read ON agent_r541_task_operational_bindings
FOR SELECT USING (sprout_private.agent_run_access(project_id,run_id));
REVOKE INSERT,UPDATE,DELETE ON agent_r541_task_operational_bindings FROM PUBLIC;

CREATE VIEW agent_r541_exact_task_operational_bindings AS
SELECT binding.*
FROM agent_r541_task_operational_bindings binding
JOIN agent_run_task_effects effect ON effect.id=binding.task_effect_id
 AND effect.project_id=binding.project_id AND effect.run_id=binding.run_id
JOIN agent_semantic_operational_ledger provenance
 ON provenance.project_id=effect.project_id AND provenance.entry_kind='task_provenance'
 AND provenance.record_id=effect.task_provenance_id
LEFT JOIN agent_semantic_operational_ledger intent
 ON intent.project_id=effect.project_id AND intent.entry_kind='task_intent'
 AND intent.record_id=effect.task_intent_id
WHERE provenance.task_resource_node_id=effect.task_resource_node_id
 AND provenance.target_agent_id=effect.target_agent_id
 AND binding.provenance_record=jsonb_build_object('trace_number',binding.trace_number,
   'provenance',jsonb_build_object('task',provenance.task_resource_node_id,
     'agent',provenance.principal_identity_id,'local_revision',provenance.local_goal_revision,
     'obligation',provenance.obligation_id,'work_spec_id',provenance.work_spec_ordinal,
     'recorded_at',provenance.semantic_position))
 AND ((effect.task_intent_id IS NULL AND binding.intent_record IS NULL)
   OR (intent.record_id IS NOT NULL AND intent.task_resource_node_id=effect.task_resource_node_id
     AND binding.intent_record#>>'{trace_number}'=binding.trace_number::text
     AND binding.intent_record#>>'{envelope,task,kind}'='derive_task_intent'
     AND binding.intent_record#>'{envelope,allowed_actions}'=intent.required_actions
     AND binding.intent_record#>>'{intent,task}'=intent.task_resource_node_id::text
     AND binding.intent_record#>>'{intent,scope}'=intent.scope_resource_node_id::text
     AND binding.intent_record#>'{intent,required_actions}'=intent.required_actions
     AND binding.intent_record#>>'{intent,created_by}'=intent.principal_identity_id::text
     AND (binding.intent_record#>>'{intent,recorded_at}')::bigint=intent.semantic_position))
 AND binding.binding_hash=digest(convert_to(concat_ws(E'\n',
   'sprout-r541-task-operational-binding-v1',binding.trace_number::text,
   binding.project_id::text,binding.run_id::text,binding.task_effect_id::text,
   COALESCE(binding.intent_record::text,''),binding.provenance_record::text),'UTF8'),'sha256');

CREATE FUNCTION sprout_private.semantic_operational_state_snapshot(
  candidate_project_id uuid,candidate_run_id uuid)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
  SELECT jsonb_build_object(
    'proxy_transcripts',COALESCE((SELECT jsonb_agg(jsonb_build_object(
      'thread',jsonb_build_object('id',thread.id,'proxy_id',thread.proxy_id,
        'creator',thread.creator_identity_id),
      'messages',COALESCE((SELECT jsonb_agg(jsonb_build_object(
        'id',request.id,'thread_id',request.thread_id,'author',request.user_identity_id,
        'payload',sprout_private.try_parse_encrypted_payload(request.encrypted_payload),
        'previous_message',NULL,'message_ordinal',(SELECT count(*) FROM public.user_proxy_requests earlier
          WHERE earlier.project_id=request.project_id AND earlier.thread_id=request.thread_id
            AND (earlier.submitted_at,earlier.id)<=(request.submitted_at,request.id)))
        ORDER BY request.submitted_at,request.id) FROM public.user_proxy_requests request
        WHERE request.project_id=thread.project_id AND request.thread_id=thread.id),'[]'::jsonb))
      ORDER BY thread.id) FROM public.user_proxy_threads thread
      WHERE thread.project_id=candidate_project_id),'[]'::jsonb),
    'proxy_audit',COALESCE((SELECT jsonb_agg(to_jsonb(plan)-'recorded_at'
      ORDER BY plan.recorded_at,plan.id) FROM public.user_proxy_plans plan
      WHERE plan.project_id=candidate_project_id),'[]'::jsonb),
    'task_provenance',COALESCE((SELECT jsonb_agg(binding.provenance_record
      ORDER BY binding.recorded_at,binding.id) FROM public.agent_r541_exact_task_operational_bindings binding
      WHERE binding.project_id=candidate_project_id AND binding.run_id=candidate_run_id),'[]'::jsonb),
    'task_intents',COALESCE((SELECT jsonb_agg(binding.intent_record
      ORDER BY binding.recorded_at,binding.id) FILTER (WHERE binding.intent_record IS NOT NULL)
      FROM public.agent_r541_exact_task_operational_bindings binding
      WHERE binding.project_id=candidate_project_id AND binding.run_id=candidate_run_id),'[]'::jsonb)
  )
$$;
REVOKE ALL ON FUNCTION sprout_private.semantic_operational_state_snapshot(uuid,uuid) FROM PUBLIC;

CREATE FUNCTION sprout_private.bind_agent_r541_task_operational_record()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE root_row public.agent_r541_release_roots%ROWTYPE;
  intent_row public.agent_semantic_operational_ledger%ROWTYPE;
  provenance_row public.agent_semantic_operational_ledger%ROWTYPE;
  intent_json jsonb; provenance_json jsonb; candidate_hash bytea;
BEGIN
  SELECT * INTO root_row FROM public.agent_r541_release_roots
   WHERE project_id=NEW.project_id AND run_id=NEW.run_id;
  IF NOT FOUND THEN RETURN NEW; END IF;
  SELECT * INTO provenance_row FROM public.agent_semantic_operational_ledger
   WHERE project_id=NEW.project_id AND entry_kind='task_provenance'
     AND record_id=NEW.task_provenance_id;
  IF NOT FOUND OR provenance_row.task_resource_node_id<>NEW.task_resource_node_id
     OR provenance_row.target_agent_id<>NEW.target_agent_id THEN RETURN NEW; END IF;
  provenance_json:=jsonb_build_object('trace_number',root_row.trace_number,
    'provenance',jsonb_build_object('task',provenance_row.task_resource_node_id,
      'agent',provenance_row.principal_identity_id,
      'local_revision',provenance_row.local_goal_revision,
      'obligation',provenance_row.obligation_id,
      'work_spec_id',provenance_row.work_spec_ordinal,
      'recorded_at',provenance_row.semantic_position));
  IF NEW.task_intent_id IS NOT NULL THEN
    SELECT * INTO intent_row FROM public.agent_semantic_operational_ledger
     WHERE project_id=NEW.project_id AND entry_kind='task_intent'
       AND record_id=NEW.task_intent_id;
    IF NOT FOUND OR intent_row.task_resource_node_id<>NEW.task_resource_node_id THEN RETURN NEW; END IF;
    intent_json:=jsonb_build_object('trace_number',root_row.trace_number,
      'envelope',jsonb_build_object('task',jsonb_build_object(
        'id',intent_row.record_id,'kind','derive_task_intent',
        'input_item_count',jsonb_array_length(intent_row.required_actions),
        'max_input_items',jsonb_array_length(intent_row.required_actions),
        'max_output_items',GREATEST(1,jsonb_array_length(intent_row.required_actions)),
        'max_nesting_depth',1,'max_attempts',1,'closed_output_schema',true,
        'grounded_identifiers_only',true,'requires_formal_proof',false,
        'requires_permission_decision',false,'requires_exact_semantic_equivalence',false,
        'requires_exhaustive_world_knowledge',false,
        'allowed_resource_ids',jsonb_build_array(intent_row.task_resource_node_id,intent_row.scope_resource_node_id),
        'allowed_principal_ids',jsonb_build_array(intent_row.principal_identity_id),'allowed_tools','[]'::jsonb),
        'task_resource',intent_row.task_resource_node_id,'project_scope',intent_row.scope_resource_node_id,
        'allowed_actions',intent_row.required_actions,
        'max_actions',GREATEST(1,jsonb_array_length(intent_row.required_actions))),
      'intent',jsonb_build_object('task',intent_row.task_resource_node_id,
        'scope',intent_row.scope_resource_node_id,'required_actions',intent_row.required_actions,
        'created_by',intent_row.principal_identity_id,'recorded_at',intent_row.semantic_position));
  END IF;
  candidate_hash:=public.digest(pg_catalog.convert_to(concat_ws(E'\n',
    'sprout-r541-task-operational-binding-v1',root_row.trace_number::text,
    NEW.project_id::text,NEW.run_id::text,NEW.id::text,
    COALESCE(intent_json::text,''),provenance_json::text),'UTF8'),'sha256');
  INSERT INTO public.agent_r541_task_operational_bindings(
    id,trace_number,project_id,run_id,task_effect_id,intent_record,provenance_record,binding_hash)
  VALUES(gen_random_uuid(),root_row.trace_number,NEW.project_id,NEW.run_id,NEW.id,
    intent_json,provenance_json,candidate_hash) ON CONFLICT (trace_number,task_effect_id) DO NOTHING;
  IF NOT EXISTS (SELECT 1 FROM public.agent_r541_task_operational_bindings binding
    WHERE binding.trace_number=root_row.trace_number AND binding.task_effect_id=NEW.id
      AND binding.intent_record IS NOT DISTINCT FROM intent_json
      AND binding.provenance_record=provenance_json AND binding.binding_hash=candidate_hash)
  THEN RAISE EXCEPTION 'task operational binding equivocation' USING ERRCODE='40001'; END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER agent_r541_bind_task_operational AFTER INSERT ON agent_run_task_effects
FOR EACH ROW EXECUTE FUNCTION sprout_private.bind_agent_r541_task_operational_record();
REVOKE ALL ON FUNCTION sprout_private.bind_agent_r541_task_operational_record() FROM PUBLIC;

CREATE FUNCTION sprout_private.initialize_agent_formal_release(
    candidate_project_id uuid,
    candidate_run_id uuid,
    candidate_transition_id uuid
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
DECLARE
    root_row public.agent_r540_tool_trace_roots%ROWTYPE;
    transition_row public.agent_run_transitions%ROWTYPE;
    governance_snapshot jsonb;
    operational_snapshot jsonb;
    proxy_snapshot jsonb;
    comment_snapshot jsonb;
    candidate_hash bytea;
    initial_causal_link record;
BEGIN
    SELECT * INTO STRICT root_row FROM public.agent_r540_tool_trace_roots
      WHERE project_id=candidate_project_id AND run_id=candidate_run_id;
    SELECT * INTO STRICT transition_row FROM public.agent_run_transitions
      WHERE id=candidate_transition_id AND project_id=candidate_project_id
        AND run_id=candidate_run_id AND transition_kind='initialized'
        AND state_version=1 AND semantic_tick=root_row.start_tick;
    SELECT jsonb_build_object(
      'responsibilities',COALESCE((SELECT jsonb_agg(to_jsonb(record)-ARRAY['recorded_at','activated_at','superseded_at'] ORDER BY record.id,record.revision)
        FROM public.agent_responsibility_contracts record
        WHERE record.project_id=candidate_project_id AND record.state='active'),'[]'::jsonb),
      'local_goals',COALESCE((SELECT jsonb_agg(to_jsonb(record)-ARRAY['recorded_at','terminal_at'] ORDER BY record.id,record.revision)
        FROM public.agent_local_goal_contracts record
        WHERE record.project_id=candidate_project_id AND record.state='active'),'[]'::jsonb),
      'agents',COALESCE((SELECT jsonb_agg(to_jsonb(record)-ARRAY['created_at','suspended_at','retired_at'] ORDER BY record.id)
        FROM public.governed_agents record
        WHERE record.project_id=candidate_project_id AND record.state='active'),'[]'::jsonb),
      'approved_exceptions',COALESCE((SELECT jsonb_agg(to_jsonb(event)-'recorded_at' ORDER BY event.ledger_position)
        FROM public.agent_governance_authorization_events event
        WHERE event.project_id=candidate_project_id AND event.event_kind='approved_local_exception'),'[]'::jsonb),
      'global_assignments',COALESCE((SELECT jsonb_agg(to_jsonb(event)-'recorded_at' ORDER BY event.ledger_position)
        FROM public.agent_governance_authorization_events event
        WHERE event.project_id=candidate_project_id AND event.event_kind='global_mandate_assignment'),'[]'::jsonb),
      'administrator_creations',COALESCE((SELECT jsonb_agg(to_jsonb(record)-'recorded_at' ORDER BY record.approval_id)
        FROM public.agent_administrator_creation_approvals record
        WHERE record.project_id=candidate_project_id),'[]'::jsonb),
      'global_revisions',COALESCE((SELECT jsonb_agg(to_jsonb(record)-'recorded_at' ORDER BY record.id,record.revision)
        FROM public.agent_global_contracts record
        WHERE record.project_id=candidate_project_id),'[]'::jsonb),
      'history',COALESCE((SELECT jsonb_agg(to_jsonb(entry)-'recorded_at' ORDER BY entry.position)
        FROM public.agent_governance_ledger entry
        WHERE entry.project_id=candidate_project_id),'[]'::jsonb)
    ) INTO governance_snapshot;
    operational_snapshot := sprout_private.semantic_operational_state_snapshot(
      candidate_project_id,candidate_run_id);
    SELECT COALESCE(jsonb_agg(jsonb_build_object('identity',membership.identity_id,
      'kind',identity.principal_kind,'proxy',proxy.id) ORDER BY membership.identity_id),'[]'::jsonb)
      INTO proxy_snapshot
      FROM public.project_memberships membership
      JOIN public.identities identity ON identity.id=membership.identity_id
      JOIN public.user_proxies proxy ON proxy.project_id=membership.project_id
        AND proxy.user_identity_id=membership.identity_id
      WHERE membership.project_id=candidate_project_id AND membership.state='active'
        AND identity.status='active' AND identity.principal_kind IN ('administrator','user');
    SELECT COALESCE(jsonb_agg(jsonb_build_object(
      'id',comment.id,'author',comment.author_identity_id,
      'recipient',comment.recipient_identity_id,'target',comment.target_resource_node_id,
      'parent',comment.parent_comment_id,'agent_depth',comment.agent_depth,
      'payload',sprout_private.try_parse_encrypted_payload(comment.encrypted_payload),
      'key_epoch',comment.key_epoch) ORDER BY event.project_ordinal),'[]'::jsonb)
      INTO comment_snapshot
      FROM public.native_comment_events event
      JOIN public.native_comments comment ON comment.project_id=event.project_id
        AND comment.id=event.comment_id
      WHERE event.project_id=candidate_project_id
        AND comment.encrypted_payload IS NOT NULL
        AND sprout_private.try_parse_encrypted_payload(comment.encrypted_payload) IS NOT NULL;
    candidate_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
      'sprout-r541-release-root-v1', root_row.trace_number::text,
      candidate_project_id::text, candidate_run_id::text, root_row.goal_id::text,
      root_row.start_tick::text, candidate_transition_id::text,
      encode(root_row.root_hash, 'hex'), encode(public.digest(convert_to(governance_snapshot::text,'UTF8'),'sha256'),'hex'),
      encode(public.digest(convert_to(operational_snapshot::text,'UTF8'),'sha256'),'hex'),
      encode(public.digest(convert_to(proxy_snapshot::text,'UTF8'),'sha256'),'hex'),
      encode(public.digest(convert_to(comment_snapshot::text,'UTF8'),'sha256'),'hex'),'1'), 'UTF8'), 'sha256');
    INSERT INTO public.agent_r541_release_roots (
      trace_number, project_id, run_id, goal_id, start_tick,
      initialization_transition_id, coordination_policy_version,
      governance_start_snapshot,operational_start_snapshot,proxy_directory_start_snapshot,
      comment_start_snapshot,root_hash
    ) VALUES (
      root_row.trace_number, candidate_project_id, candidate_run_id, root_row.goal_id,
      root_row.start_tick, candidate_transition_id, 1,governance_snapshot,
      operational_snapshot,proxy_snapshot,comment_snapshot,candidate_hash
    ) ON CONFLICT (project_id, run_id) DO NOTHING;
    IF NOT EXISTS (SELECT 1 FROM public.agent_r541_release_roots root
      WHERE root.trace_number=root_row.trace_number AND root.project_id=candidate_project_id
        AND root.run_id=candidate_run_id AND root.goal_id=root_row.goal_id
        AND root.start_tick=root_row.start_tick AND root.initialization_transition_id=candidate_transition_id
        AND root.governance_start_snapshot=governance_snapshot
        AND root.operational_start_snapshot=operational_snapshot
        AND root.proxy_directory_start_snapshot=proxy_snapshot
        AND root.comment_start_snapshot=comment_snapshot
        AND root.root_hash=candidate_hash) THEN
      RAISE EXCEPTION 'formal release root equivocation' USING ERRCODE='40001';
    END IF;
    INSERT INTO public.agent_run_semantic_tick_cursors (
      project_id,run_id,trace_number,start_tick,last_tick,allocation_count,cursor_hash
    ) VALUES (
      candidate_project_id,candidate_run_id,root_row.trace_number,root_row.start_tick,
      root_row.start_tick,0,public.digest(pg_catalog.convert_to(concat_ws(E'\n',
        'sprout-run-semantic-tick-cursor-v1',root_row.trace_number::text,
        candidate_project_id::text,candidate_run_id::text,root_row.start_tick::text,
        encode(candidate_hash,'hex')),'UTF8'),'sha256')
    ) ON CONFLICT (project_id,run_id) DO NOTHING;
    IF NOT EXISTS (SELECT 1 FROM public.agent_run_semantic_tick_cursors cursor
      WHERE cursor.project_id=candidate_project_id AND cursor.run_id=candidate_run_id
        AND cursor.trace_number=root_row.trace_number AND cursor.start_tick=root_row.start_tick
        AND cursor.last_tick>=cursor.start_tick AND cursor.allocation_count>=0)
    THEN
      RAISE EXCEPTION 'semantic tick cursor equivocation' USING ERRCODE='40001';
    END IF;
    -- Contract-derived causal links are already part of the canonical
    -- initialized semantic state before the 0035 root exists, so their INSERT
    -- trigger correctly observes no root.  Project those exact authoritative
    -- rows now, in deterministic order, rather than backfilling from a mutable
    -- current row later.
    FOR initial_causal_link IN
      SELECT link.* FROM public.agent_run_causal_links link
      WHERE link.project_id=candidate_project_id AND link.run_id=candidate_run_id
        AND link.transition_id=candidate_transition_id
      ORDER BY link.observed_tick,link.id
    LOOP
      PERFORM sprout_private.append_agent_r540_release_event(
        root_row.trace_number,'causal_link',transition_row.semantic_tick,
        'agent_run_causal_links',initial_causal_link.id,jsonb_build_object(
          'trace_id',root_row.trace_number,'run',candidate_run_id,'goal',initial_causal_link.goal_id,
          'link',jsonb_build_object('run',candidate_run_id,'goal',initial_causal_link.goal_id,
            'predecessor',initial_causal_link.predecessor,
            'successor',initial_causal_link.successor,
            'observed_at',initial_causal_link.observed_tick),
          'recorded_at',transition_row.semantic_tick,'transition_id',candidate_transition_id));
    END LOOP;
    RETURN root_row.trace_number;
END
$$;

CREATE FUNCTION sprout_private.allocate_agent_run_semantic_tick(
    candidate_project_id uuid,
    candidate_run_id uuid,
    candidate_event_key uuid,
    candidate_event_kind text,
    candidate_request_hash bytea,
    candidate_minimum_tick bigint DEFAULT NULL,
    candidate_terminal_call_id uuid DEFAULT NULL,
    candidate_terminal_attempt integer DEFAULT NULL
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
DECLARE
    cursor_row public.agent_run_semantic_tick_cursors%ROWTYPE;
    existing_row public.agent_run_semantic_tick_allocations%ROWTYPE;
    next_tick bigint;
    next_ordinal bigint;
    candidate_hash bytea;
    earliest_pending_deadline bigint;
    terminal_deadline bigint;
BEGIN
    IF candidate_event_key IS NULL OR candidate_event_kind IS NULL OR candidate_event_kind=''
       OR candidate_request_hash IS NULL OR octet_length(candidate_request_hash)<>32
    THEN
      RAISE EXCEPTION 'invalid semantic tick allocation request' USING ERRCODE='23514';
    END IF;
    SELECT * INTO cursor_row FROM public.agent_run_semantic_tick_cursors
      WHERE project_id=candidate_project_id AND run_id=candidate_run_id FOR UPDATE;
    IF NOT FOUND THEN
      RAISE EXCEPTION 'run has no exact 0035 semantic timeline' USING ERRCODE='23514';
    END IF;
    SELECT * INTO existing_row FROM public.agent_run_semantic_tick_allocations
      WHERE project_id=candidate_project_id AND run_id=candidate_run_id
        AND event_key=candidate_event_key;
    IF FOUND THEN
      IF existing_row.trace_number IS DISTINCT FROM cursor_row.trace_number
         OR existing_row.event_kind IS DISTINCT FROM candidate_event_kind
         OR existing_row.request_hash IS DISTINCT FROM candidate_request_hash
      THEN
        RAISE EXCEPTION 'semantic tick allocation equivocation' USING ERRCODE='40001';
      END IF;
      RETURN existing_row.semantic_tick;
    END IF;
    IF candidate_minimum_tick IS NOT NULL AND candidate_minimum_tick<cursor_row.start_tick THEN
      RAISE EXCEPTION 'semantic tick lower bound predates run' USING ERRCODE='23514';
    END IF;
    next_tick := greatest(cursor_row.last_tick+1,
      COALESCE(candidate_minimum_tick,cursor_row.last_tick+1));
    SELECT min(binding.tool_deadline_tick) INTO earliest_pending_deadline
    FROM public.agent_tool_calls call
    JOIN public.agent_tool_attempt_clock_bindings binding
      ON binding.project_id=call.project_id AND binding.call_id=call.id
     AND binding.attempt=call.current_attempt
    WHERE call.project_id=candidate_project_id AND call.run_id=candidate_run_id
      AND call.current_status='pending';
    IF earliest_pending_deadline IS NOT NULL THEN
      IF candidate_event_kind='tool_terminal' THEN
        IF candidate_terminal_call_id IS NULL OR candidate_terminal_attempt IS NULL THEN
          RAISE EXCEPTION 'tool terminal semantic allocation lacks exact attempt'
            USING ERRCODE='23514';
        END IF;
        SELECT binding.tool_deadline_tick INTO terminal_deadline
        FROM public.agent_tool_calls call
        JOIN public.agent_tool_attempt_clock_bindings binding
          ON binding.project_id=call.project_id AND binding.call_id=call.id
         AND binding.attempt=call.current_attempt
        WHERE call.project_id=candidate_project_id AND call.run_id=candidate_run_id
          AND call.id=candidate_terminal_call_id
          AND call.current_attempt=candidate_terminal_attempt
          AND call.current_status='pending';
        IF terminal_deadline IS NULL OR next_tick>terminal_deadline
           OR (next_tick>=earliest_pending_deadline
               AND terminal_deadline<>earliest_pending_deadline) THEN
          RAISE EXCEPTION 'tool terminal cannot satisfy exact semantic deadline'
            USING ERRCODE='55000';
        END IF;
      ELSIF next_tick>=earliest_pending_deadline THEN
        RAISE EXCEPTION 'pending ToolCall semantic deadline must terminalize first'
          USING ERRCODE='55P03';
      END IF;
    ELSIF candidate_event_kind='tool_terminal' THEN
      RAISE EXCEPTION 'tool terminal has no exact pending semantic deadline'
        USING ERRCODE='23514';
    END IF;
    next_ordinal := cursor_row.allocation_count+1;
    candidate_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
      'sprout-run-semantic-tick-allocation-v1',cursor_row.trace_number::text,
      candidate_project_id::text,candidate_run_id::text,next_ordinal::text,next_tick::text,
      candidate_event_kind,candidate_event_key::text,encode(candidate_request_hash,'hex'),
      encode(cursor_row.cursor_hash,'hex')),'UTF8'),'sha256');
    INSERT INTO public.agent_run_semantic_tick_allocations (
      id,project_id,run_id,trace_number,ordinal,semantic_tick,event_kind,event_key,
      request_hash,previous_allocation_hash,allocation_hash
    ) VALUES (
      gen_random_uuid(),candidate_project_id,candidate_run_id,cursor_row.trace_number,
      next_ordinal,next_tick,candidate_event_kind,candidate_event_key,
      candidate_request_hash,cursor_row.cursor_hash,candidate_hash
    );
    UPDATE public.agent_run_semantic_tick_cursors
      SET last_tick=next_tick,allocation_count=next_ordinal,cursor_hash=candidate_hash,
          updated_at=clock_timestamp()
      WHERE project_id=candidate_project_id AND run_id=candidate_run_id;
    RETURN next_tick;
END
$$;

-- Stage a 0035-native ToolCall through the unchanged 0033 validator.  Trigger
-- execution order is intentional:
--   agent_0035_*  -> agent_tool_calls_exact_runtime -> zz_agent_0035_*
-- The first function proves and records the logical allocation, then exposes
-- an epoch projection only to the legacy validator.  The last function
-- restores the canonical Nat ticks.  Operational timestamps never change and
-- remain the values against which leases and timeout workers operate.
CREATE FUNCTION sprout_private.validate_and_stage_agent_tool_clock_v0035()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE
  root_row public.agent_r541_release_roots%ROWTYPE;
  allocation_row public.agent_run_semantic_tick_allocations%ROWTYPE;
  candidate_hash bytea;
  existing_hash bytea;
  operational_requested_tick bigint;
  operational_deadline_tick bigint;
BEGIN
  IF NOT (TG_OP='INSERT'
      OR (TG_OP='UPDATE' AND OLD.current_status IN ('failed','timed_out')
          AND NEW.current_status='pending')) THEN
    RETURN NEW;
  END IF;
  SELECT * INTO root_row FROM public.agent_r541_release_roots
    WHERE project_id=NEW.project_id AND run_id=NEW.run_id;
  IF NOT FOUND THEN RETURN NEW; END IF;

  SELECT * INTO STRICT allocation_row
  FROM public.agent_run_semantic_tick_allocations allocation
  WHERE allocation.project_id=NEW.project_id AND allocation.run_id=NEW.run_id
    AND allocation.trace_number=root_row.trace_number
    AND allocation.semantic_tick=NEW.requested_tick
    AND allocation.event_kind='tool_attempt_opened';
  IF NEW.tool_deadline_tick IS DISTINCT FROM
       NEW.requested_tick + NEW.timeout_seconds
     OR NEW.requested_at>clock_timestamp()
     OR NEW.tool_deadline_at IS DISTINCT FROM
       NEW.requested_at + pg_catalog.make_interval(secs=>NEW.timeout_seconds)
  THEN
    RAISE EXCEPTION '0035 tool semantic/operational clock mismatch'
      USING ERRCODE='23514';
  END IF;
  IF EXISTS (
    WITH pending_deadlines AS (
      SELECT binding.tool_deadline_tick
      FROM public.agent_tool_calls call
      JOIN public.agent_tool_attempt_clock_bindings binding
        ON binding.project_id=call.project_id AND binding.call_id=call.id
       AND binding.attempt=call.current_attempt
      WHERE call.project_id=NEW.project_id AND call.run_id=NEW.run_id
        AND call.current_status='pending'
        AND NOT (call.id=NEW.id AND call.current_attempt=NEW.current_attempt)
      UNION ALL SELECT NEW.tool_deadline_tick
    ), ordered AS (
      SELECT tool_deadline_tick,
        row_number() OVER (ORDER BY tool_deadline_tick) AS terminal_slot
      FROM pending_deadlines
    )
    SELECT 1 FROM ordered
    WHERE tool_deadline_tick < NEW.requested_tick + terminal_slot
  ) THEN
    RAISE EXCEPTION 'pending ToolCall semantic deadlines are not schedulable'
      USING ERRCODE='55000';
  END IF;
  candidate_hash:=public.digest(pg_catalog.convert_to(concat_ws(E'\n',
    'sprout-tool-attempt-clock-binding-v1',root_row.trace_number::text,
    NEW.project_id::text,NEW.run_id::text,NEW.id::text,NEW.current_attempt::text,
    allocation_row.id::text,NEW.requested_tick::text,NEW.tool_deadline_tick::text,
    NEW.requested_at::text,NEW.tool_deadline_at::text),'UTF8'),'sha256');
  INSERT INTO public.agent_tool_attempt_clock_bindings (
    id,project_id,run_id,trace_number,call_id,attempt,allocation_id,
    requested_tick,tool_deadline_tick,requested_at,tool_deadline_at,binding_hash
  ) VALUES (
    gen_random_uuid(),NEW.project_id,NEW.run_id,root_row.trace_number,NEW.id,
    NEW.current_attempt,allocation_row.id,NEW.requested_tick,NEW.tool_deadline_tick,
    NEW.requested_at,NEW.tool_deadline_at,candidate_hash
  ) ON CONFLICT (project_id,call_id,attempt) DO NOTHING;
  SELECT binding_hash INTO STRICT existing_hash
  FROM public.agent_tool_attempt_clock_bindings
  WHERE project_id=NEW.project_id AND call_id=NEW.id AND attempt=NEW.current_attempt;
  IF existing_hash IS DISTINCT FROM candidate_hash THEN
    RAISE EXCEPTION '0035 tool clock binding equivocation' USING ERRCODE='40001';
  END IF;

  -- 0033 intentionally validates second-granularity epoch ticks.  Keep that
  -- function byte/semantics unchanged and give it precisely that projection.
  operational_requested_tick:=extract(epoch FROM NEW.requested_at)::bigint;
  operational_deadline_tick:=extract(epoch FROM NEW.tool_deadline_at)::bigint;
  IF NEW.requested_at IS DISTINCT FROM to_timestamp(operational_requested_tick)
     OR NEW.tool_deadline_at IS DISTINCT FROM to_timestamp(operational_deadline_tick)
  THEN
    RAISE EXCEPTION 'operational ToolCall timestamps must use canonical seconds'
      USING ERRCODE='23514';
  END IF;
  NEW.requested_tick:=operational_requested_tick;
  NEW.tool_deadline_tick:=operational_deadline_tick;
  RETURN NEW;
END $$;

CREATE FUNCTION sprout_private.restore_agent_tool_semantic_clock_v0035()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE binding_row public.agent_tool_attempt_clock_bindings%ROWTYPE;
BEGIN
  IF NOT (TG_OP='INSERT'
      OR (TG_OP='UPDATE' AND OLD.current_status IN ('failed','timed_out')
          AND NEW.current_status='pending')) THEN
    RETURN NEW;
  END IF;
  SELECT * INTO binding_row FROM public.agent_tool_attempt_clock_bindings
    WHERE project_id=NEW.project_id AND call_id=NEW.id AND attempt=NEW.current_attempt;
  IF NOT FOUND THEN RETURN NEW; END IF;
  IF NEW.requested_at IS DISTINCT FROM binding_row.requested_at
     OR NEW.tool_deadline_at IS DISTINCT FROM binding_row.tool_deadline_at
  THEN
    RAISE EXCEPTION '0035 operational ToolCall clock changed during validation'
      USING ERRCODE='23514';
  END IF;
  NEW.requested_tick:=binding_row.requested_tick;
  NEW.tool_deadline_tick:=binding_row.tool_deadline_tick;
  RETURN NEW;
END $$;

CREATE TRIGGER agent_0035_tool_clock_stage
BEFORE INSERT OR UPDATE ON agent_tool_calls
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_and_stage_agent_tool_clock_v0035();
CREATE TRIGGER zz_agent_0035_tool_clock_restore
BEFORE INSERT OR UPDATE ON agent_tool_calls
FOR EACH ROW EXECUTE FUNCTION sprout_private.restore_agent_tool_semantic_clock_v0035();

CREATE VIEW agent_run_exact_semantic_timelines AS
WITH ordered AS (
  SELECT allocation.*,
    row_number() OVER (PARTITION BY allocation.trace_number ORDER BY allocation.ordinal) AS expected_ordinal,
    lag(allocation.semantic_tick) OVER (
      PARTITION BY allocation.trace_number ORDER BY allocation.ordinal) AS previous_tick,
    lag(allocation.allocation_hash) OVER (
      PARTITION BY allocation.trace_number ORDER BY allocation.ordinal) AS previous_hash
  FROM agent_run_semantic_tick_allocations allocation
)
SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,root.start_tick,
  cursor.last_tick,cursor.allocation_count,cursor.cursor_hash
FROM agent_r541_release_roots root
JOIN agent_run_semantic_tick_cursors cursor
  ON cursor.trace_number=root.trace_number AND cursor.project_id=root.project_id
 AND cursor.run_id=root.run_id AND cursor.start_tick=root.start_tick
WHERE cursor.allocation_count=(SELECT count(*) FROM ordered allocation
    WHERE allocation.trace_number=root.trace_number)
  AND cursor.last_tick=COALESCE((SELECT max(allocation.semantic_tick)
    FROM ordered allocation WHERE allocation.trace_number=root.trace_number),root.start_tick)
  AND cursor.cursor_hash=COALESCE((SELECT allocation.allocation_hash
    FROM ordered allocation WHERE allocation.trace_number=root.trace_number
    ORDER BY allocation.ordinal DESC LIMIT 1),
    digest(convert_to(concat_ws(E'\n','sprout-run-semantic-tick-cursor-v1',
      root.trace_number::text,root.project_id::text,root.run_id::text,root.start_tick::text,
      encode(root.root_hash,'hex')),'UTF8'),'sha256'))
  AND NOT EXISTS (SELECT 1 FROM ordered allocation
    WHERE allocation.trace_number=root.trace_number
      AND (allocation.ordinal<>allocation.expected_ordinal
        OR allocation.semantic_tick<=COALESCE(allocation.previous_tick,root.start_tick)
        OR allocation.previous_allocation_hash IS DISTINCT FROM COALESCE(
          allocation.previous_hash,digest(convert_to(concat_ws(E'\n',
            'sprout-run-semantic-tick-cursor-v1',root.trace_number::text,
            root.project_id::text,root.run_id::text,root.start_tick::text,
            encode(root.root_hash,'hex')),'UTF8'),'sha256'))
        OR allocation.allocation_hash IS DISTINCT FROM digest(convert_to(concat_ws(E'\n',
          'sprout-run-semantic-tick-allocation-v1',allocation.trace_number::text,
          allocation.project_id::text,allocation.run_id::text,allocation.ordinal::text,
          allocation.semantic_tick::text,allocation.event_kind,allocation.event_key::text,
          encode(allocation.request_hash,'hex'),encode(allocation.previous_allocation_hash,'hex')),
          'UTF8'),'sha256')))
  -- Every committed allocation must have an authoritative formal-event row
  -- on the same run tick.  Reservations that later fail roll back with their
  -- transaction and therefore cannot survive as orphan timeline entries.
  AND NOT EXISTS (SELECT 1 FROM ordered allocation
    WHERE allocation.trace_number=root.trace_number
      AND NOT (
        EXISTS (SELECT 1 FROM agent_run_transitions transition
          WHERE transition.project_id=root.project_id AND transition.run_id=root.run_id
            AND transition.semantic_tick=allocation.semantic_tick)
        OR EXISTS (SELECT 1 FROM native_comment_events comment_event
          WHERE comment_event.project_id=root.project_id AND comment_event.run_id=root.run_id
            AND comment_event.id=allocation.event_key
            AND comment_event.semantic_tick=allocation.semantic_tick)
        OR EXISTS (SELECT 1 FROM agent_r540_release_events release_event
          WHERE release_event.trace_number=root.trace_number
            AND release_event.semantic_tick=allocation.semantic_tick)
        OR EXISTS (SELECT 1 FROM agent_model_attempt_dispatches model_dispatch
          JOIN agent_invocations invocation
            ON invocation.project_id=model_dispatch.project_id
           AND invocation.id=model_dispatch.invocation_id
          WHERE model_dispatch.project_id=root.project_id
            AND invocation.run_id=root.run_id AND model_dispatch.id=allocation.event_key
            AND model_dispatch.semantic_tick=allocation.semantic_tick)
      ))
  -- A visible exact timeline can never have advanced beyond an unresolved
  -- pending attempt's formal timeout bound.
  AND NOT EXISTS (
    SELECT 1 FROM agent_tool_calls call
    JOIN agent_tool_attempt_clock_bindings binding
      ON binding.project_id=call.project_id AND binding.call_id=call.id
     AND binding.attempt=call.current_attempt
    WHERE call.project_id=root.project_id AND call.run_id=root.run_id
      AND call.current_status='pending' AND cursor.last_tick>binding.tool_deadline_tick
  );

CREATE FUNCTION sprout_private.append_agent_comment_certificate(
    candidate_trace_number bigint,
    candidate_end_tick bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
DECLARE
    root_row public.agent_r541_release_roots%ROWTYPE;
    previous_row public.agent_r541_comment_certificates%ROWTYPE;
    inventory_list jsonb;
    inventory_hash bytea;
    next_version integer;
    max_ordinal bigint;
    candidate_hash bytea;
BEGIN
    SELECT * INTO STRICT root_row FROM public.agent_r541_release_roots
      WHERE trace_number=candidate_trace_number FOR UPDATE;
    SELECT COALESCE(jsonb_agg(jsonb_build_object(
        'ordinal', inventory.ordinal, 'record_id', inventory.comment_record_id,
        'record_hash', encode(inventory.record_hash, 'hex')) ORDER BY inventory.ordinal), '[]'::jsonb),
      COALESCE(max(inventory.ordinal),0)
      INTO inventory_list, max_ordinal
      FROM public.agent_r541_comment_inventory inventory
      WHERE inventory.trace_number=candidate_trace_number;
    IF max_ordinal = 0 THEN
      RETURN;
    END IF;
    SELECT * INTO previous_row FROM public.agent_r541_comment_certificates
      WHERE trace_number=candidate_trace_number ORDER BY version DESC LIMIT 1;
    IF FOUND AND previous_row.last_ordinal = max_ordinal THEN
      RETURN;
    END IF;
    IF FOUND AND previous_row.last_ordinal >= max_ordinal THEN
      RAISE EXCEPTION 'comment certificate prefix regression' USING ERRCODE='40001';
    END IF;
    next_version := COALESCE(previous_row.version,0)+1;
    inventory_hash := public.digest(pg_catalog.convert_to(inventory_list::text,'UTF8'),'sha256');
    candidate_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
      'sprout-r541-comment-certificate-v1', candidate_trace_number::text,
      root_row.project_id::text, root_row.run_id::text, root_row.goal_id::text,
      next_version::text, candidate_end_tick::text, max_ordinal::text,
      encode(inventory_hash,'hex'),
      COALESCE(encode(previous_row.certificate_hash,'hex'),'')), 'UTF8'),'sha256');
    INSERT INTO public.agent_r541_comment_certificates (
      id, trace_number, project_id, run_id, goal_id, version, end_tick,
      last_ordinal, comment_inventory, inventory_commitment, comment_gate_mode,
      previous_certificate_hash, certificate_hash
    ) VALUES (
      gen_random_uuid(), candidate_trace_number, root_row.project_id, root_row.run_id,
      root_row.goal_id, next_version, candidate_end_tick, max_ordinal, inventory_list,
      inventory_hash, 'enabled', previous_row.certificate_hash, candidate_hash
    );
END
$$;

-- Full formal Comment reconstruction.  The public surface remains
-- metadata-only; this internal view resolves the exact encrypted payload and
-- proves the canonical project Comment-state append, event/notification
-- identity, path kind and record hash from immutable authoritative rows.
CREATE VIEW agent_native_comment_semantic_states AS
SELECT event.project_id,event.id AS comment_event_id,event.project_ordinal,
  event.semantic_tick,event.semantic_state_hash,
  state.base_comments
FROM native_comment_events event
CROSS JOIN LATERAL (
  SELECT jsonb_agg(jsonb_build_object(
      'id',comment.id,'author',comment.author_identity_id,
      'recipient',comment.recipient_identity_id,'target',comment.target_resource_node_id,
      'parent',comment.parent_comment_id,'agent_depth',comment.agent_depth,
      'payload',sprout_private.try_parse_encrypted_payload(comment.encrypted_payload),
      'key_epoch',comment.key_epoch
    ) ORDER BY prefix.project_ordinal) AS base_comments,
    count(*) AS prefix_count
  FROM native_comment_events prefix
  JOIN native_comments comment
    ON comment.project_id=prefix.project_id AND comment.id=prefix.comment_id
  WHERE prefix.project_id=event.project_id
    AND prefix.project_ordinal<=event.project_ordinal
    AND comment.encrypted_payload IS NOT NULL
    AND sprout_private.try_parse_encrypted_payload(comment.encrypted_payload) IS NOT NULL
    AND digest(comment.encrypted_payload,'sha256')=comment.payload_commitment
    AND prefix.event_hash=comment.event_hash
    AND prefix.event_hash=digest(convert_to(concat_ws(E'\n',
      'sprout-native-comment-event-v1',comment.project_id::text,
      prefix.comment_snapshot::text,prefix.semantic_tick::text,prefix.action_path,
      encode(comment.request_hash,'hex')),'UTF8'),'sha256')
    AND prefix.semantic_state_hash=digest(convert_to(concat_ws(E'\n',
      'sprout-native-comment-semantic-state-v1',prefix.project_id::text,
      prefix.project_ordinal::text,COALESCE(encode(prefix.previous_state_hash,'hex'),''),
      encode(prefix.event_hash,'hex')),'UTF8'),'sha256')
) state
WHERE state.prefix_count=event.project_ordinal
  AND ((event.project_ordinal=1 AND event.previous_state_hash IS NULL)
    OR (event.project_ordinal>1 AND event.previous_state_hash=(SELECT prior.semantic_state_hash
      FROM native_comment_events prior WHERE prior.project_id=event.project_id
        AND prior.project_ordinal=event.project_ordinal-1)));

-- `secured.certified.run.semanticState tick` is run-scoped.  Its Comment list
-- begins with the exact initialization snapshot and appends precisely the
-- run-bound commentPosted events whose canonical semantic tick is not later
-- than the observed event.  The project ledger remains an independent
-- append-only cross-check, not a substitute for this run state.
CREATE VIEW agent_native_comment_run_semantic_states AS
SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,
  event.id AS comment_event_id,event.semantic_tick,
  root.comment_start_snapshot || COALESCE(prefix.comments,'[]'::jsonb) AS base_comments
FROM agent_r541_release_roots root
JOIN native_comment_events event ON event.project_id=root.project_id
  AND event.run_id=root.run_id AND event.trace_number=root.trace_number
CROSS JOIN LATERAL (
  SELECT jsonb_agg(jsonb_build_object(
      'id',comment.id,'author',comment.author_identity_id,
      'recipient',comment.recipient_identity_id,'target',comment.target_resource_node_id,
      'parent',comment.parent_comment_id,'agent_depth',comment.agent_depth,
      'payload',sprout_private.try_parse_encrypted_payload(comment.encrypted_payload),
      'key_epoch',comment.key_epoch
    ) ORDER BY run_event.semantic_tick,run_event.id) AS comments,
    count(*) AS comment_count
  FROM native_comment_events run_event
  JOIN native_comments comment ON comment.project_id=run_event.project_id
    AND comment.id=run_event.comment_id
  WHERE run_event.project_id=root.project_id AND run_event.run_id=root.run_id
    AND run_event.trace_number=root.trace_number
    AND run_event.semantic_tick<=event.semantic_tick
    AND comment.encrypted_payload IS NOT NULL
    AND sprout_private.try_parse_encrypted_payload(comment.encrypted_payload) IS NOT NULL
    AND digest(comment.encrypted_payload,'sha256')=comment.payload_commitment
) prefix
WHERE prefix.comment_count=(SELECT count(*) FROM native_comment_events candidate
  WHERE candidate.project_id=root.project_id AND candidate.run_id=root.run_id
    AND candidate.trace_number=root.trace_number
    AND candidate.semantic_tick<=event.semantic_tick);

CREATE VIEW agent_r541_typed_exact_comment_records AS
SELECT record.*,
  event.project_ordinal AS semantic_state_ordinal,
  event.semantic_state_hash,
  semantic_state.base_comments AS semantic_state_comments,
  jsonb_build_object(
    'id',comment.id,'author',comment.author_identity_id,
    'recipient',comment.recipient_identity_id,'target',comment.target_resource_node_id,
    'parent',comment.parent_comment_id,'agent_depth',comment.agent_depth,
    'payload',sprout_private.try_parse_encrypted_payload(comment.encrypted_payload),
    'key_epoch',comment.key_epoch
  ) AS exact_comment
FROM agent_r541_comment_records record
JOIN native_comments comment
  ON comment.project_id=record.project_id AND comment.id=record.comment_id
JOIN native_comment_events event
  ON event.project_id=record.project_id AND event.id=record.comment_event_id
 AND event.comment_id=record.comment_id
JOIN native_comment_notifications notification
  ON notification.project_id=record.project_id AND notification.run_id=record.run_id
 AND notification.comment_id=record.comment_id
JOIN agent_r541_release_roots root
  ON root.trace_number=record.trace_number AND root.project_id=record.project_id
 AND root.run_id=record.run_id AND root.goal_id=record.goal_id
JOIN agent_run_exact_semantic_timelines timeline
  ON timeline.trace_number=root.trace_number
JOIN agent_run_semantic_tick_allocations allocation
  ON allocation.project_id=record.project_id AND allocation.run_id=record.run_id
 AND allocation.trace_number=record.trace_number AND allocation.event_key=event.id
 AND allocation.event_kind='comment_posted' AND allocation.semantic_tick=record.semantic_tick
 AND allocation.request_hash=comment.request_hash
JOIN agent_native_comment_semantic_states project_state
  ON project_state.project_id=event.project_id AND project_state.comment_event_id=event.id
JOIN agent_native_comment_run_semantic_states semantic_state
  ON semantic_state.trace_number=record.trace_number
 AND semantic_state.project_id=event.project_id
 AND semantic_state.run_id=record.run_id
 AND semantic_state.comment_event_id=event.id
WHERE comment.encrypted_payload IS NOT NULL
  AND sprout_private.try_parse_encrypted_payload(comment.encrypted_payload) IS NOT NULL
  AND digest(comment.encrypted_payload,'sha256')=comment.payload_commitment
  AND comment.trace_number=record.trace_number AND comment.run_id=record.run_id
  AND comment.goal_id=record.goal_id AND comment.semantic_tick=record.semantic_tick
  AND event.trace_number=record.trace_number AND event.run_id=record.run_id
  AND event.semantic_tick=record.semantic_tick AND event.event_kind='comment_posted'
  AND event.comment_snapshot=jsonb_build_object(
    'id',comment.id,'author',comment.author_identity_id,
    'recipient',comment.recipient_identity_id,'target',comment.target_resource_node_id,
    'parent',comment.parent_comment_id,'agent_depth',comment.agent_depth,
    'payload_commitment',encode(comment.payload_commitment,'hex'),'key_epoch',comment.key_epoch)
  AND record.comment_snapshot=event.comment_snapshot
  AND event.event_hash=comment.event_hash
  AND event.event_hash=digest(convert_to(concat_ws(E'\n',
    'sprout-native-comment-event-v1',comment.project_id::text,event.comment_snapshot::text,
    event.semantic_tick::text,event.action_path,encode(comment.request_hash,'hex')),'UTF8'),'sha256')
  AND event.semantic_state_hash=digest(convert_to(concat_ws(E'\n',
    'sprout-native-comment-semantic-state-v1',event.project_id::text,
    event.project_ordinal::text,COALESCE(encode(event.previous_state_hash,'hex'),''),
    encode(event.event_hash,'hex')),'UTF8'),'sha256')
  AND event.project_ordinal=(SELECT count(*) FROM native_comment_events prefix
    WHERE prefix.project_id=event.project_id AND prefix.project_ordinal<=event.project_ordinal)
  AND ((event.project_ordinal=1 AND event.previous_state_hash IS NULL)
    OR (event.project_ordinal>1 AND event.previous_state_hash=(SELECT prior.semantic_state_hash
      FROM native_comment_events prior WHERE prior.project_id=event.project_id
        AND prior.project_ordinal=event.project_ordinal-1)))
  AND semantic_state.base_comments @> jsonb_build_array(jsonb_build_object(
    'id',comment.id,'author',comment.author_identity_id,
    'recipient',comment.recipient_identity_id,'target',comment.target_resource_node_id,
    'parent',comment.parent_comment_id,'agent_depth',comment.agent_depth,
    'payload',sprout_private.try_parse_encrypted_payload(comment.encrypted_payload),
    'key_epoch',comment.key_epoch))
  AND notification.recipient_identity_id=comment.recipient_identity_id
  AND notification.semantic_tick=comment.semantic_tick
  AND notification.notification_hash=digest(convert_to(concat_ws(E'\n',
    'sprout-comment-notification-v1',comment.project_id::text,comment.run_id::text,
    comment.id::text,comment.recipient_identity_id::text,comment.semantic_tick::text),'UTF8'),'sha256')
  AND ((event.action_path='human_native' AND comment.author_kind IN ('administrator','user')
        AND comment.agent_depth=0
        AND NOT EXISTS (SELECT 1 FROM agent_native_comment_security_effects effect
          WHERE effect.project_id=comment.project_id AND effect.run_id=comment.run_id
            AND effect.comment_id=comment.id))
    OR (event.action_path='agent_action' AND comment.author_kind='agent'
        AND comment.agent_depth>0
        AND EXISTS (SELECT 1 FROM agent_native_comment_security_effects effect
          WHERE effect.project_id=comment.project_id AND effect.run_id=comment.run_id
            AND effect.comment_id=comment.id AND effect.goal_id=comment.goal_id
            AND effect.work_item_id=comment.work_item_id AND effect.claim_id=comment.claim_id
            AND effect.attempt=comment.attempt
            AND effect.actor_identity_id=comment.author_identity_id
            AND effect.target_resource_node_id=comment.target_resource_node_id
            AND effect.payload_commitment=comment.payload_commitment
            AND effect.observed_tick=comment.semantic_tick)))
  AND record.record_hash=digest(convert_to(concat_ws(E'\n',
    'sprout-r541-comment-record-v1',record.trace_number::text,
    record.project_id::text,record.run_id::text,record.goal_id::text,
    record.semantic_tick::text,record.comment_id::text,record.comment_event_id::text,
    record.comment_snapshot::text),'UTF8'),'sha256');

-- The only Comment writer.  It derives author/kind/depth/tick/trace and
-- validates permission, target, recipient, epoch, work and priority while all
-- relevant rows are locked in one transaction.
CREATE FUNCTION sprout_private.post_native_comment(
    candidate_project_id uuid,
    candidate_recipient_id uuid,
    candidate_target_id uuid,
    candidate_parent_id uuid,
    candidate_encrypted_payload bytea,
    candidate_key_epoch integer,
    candidate_idempotency_key uuid,
    candidate_run_id uuid DEFAULT NULL,
    candidate_work_item_id uuid DEFAULT NULL,
    candidate_claim_id uuid DEFAULT NULL,
    candidate_attempt integer DEFAULT NULL,
    candidate_semantic_tick bigint DEFAULT NULL
) RETURNS TABLE (comment_id uuid, replayed boolean)
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
DECLARE
    caller_id uuid := sprout_private.current_identity_id();
    caller_kind text;
    caller_role text;
    recipient_kind text;
    target_row public.resource_nodes%ROWTYPE;
    parent_row public.native_comments%ROWTYPE;
    run_row public.agent_collaborative_runs%ROWTYPE;
    release_root public.agent_r541_release_roots%ROWTYPE;
    transition_row public.agent_run_transitions%ROWTYPE;
    coordination_depth integer;
    derived_depth integer;
    semantic_now bigint := candidate_semantic_tick;
    payload_hash bytea;
    semantic_request_hash bytea;
    candidate_comment_id uuid;
    candidate_event_id uuid;
    candidate_event_hash bytea;
    prior_comment_state_hash bytea;
    candidate_comment_state_hash bytea;
    next_project_comment_ordinal bigint;
    candidate_snapshot jsonb;
    candidate_record_id uuid;
    candidate_record_hash bytea;
    next_ordinal bigint;
    work_json jsonb;
    claim_json jsonb;
    work_spec_id bigint;
    local_compiler jsonb;
    exact_work_spec jsonb;
    exact_policy jsonb;
    existing_row public.native_comments%ROWTYPE;
BEGIN
    IF caller_id IS NULL OR candidate_encrypted_payload IS NULL
       OR octet_length(candidate_encrypted_payload)=0 OR candidate_key_epoch <= 0
       OR (candidate_run_id IS NULL AND (semantic_now IS NULL OR semantic_now < 0))
       OR sprout_private.try_parse_encrypted_payload(candidate_encrypted_payload) IS NULL
    THEN
      RAISE EXCEPTION 'invalid comment request' USING ERRCODE='23514';
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended(candidate_project_id::text, 3501));

    SELECT identity.principal_kind, membership.role
      INTO STRICT caller_kind, caller_role
      FROM public.identities identity
      JOIN public.project_memberships membership ON membership.identity_id=identity.id
      WHERE identity.id=caller_id AND identity.status='active'
        AND membership.project_id=candidate_project_id AND membership.state='active';
    IF caller_kind = 'agent' THEN
      caller_kind := 'agent';
    ELSIF caller_role IN ('owner','admin') THEN
      caller_kind := 'administrator';
    ELSE
      caller_kind := 'user';
    END IF;
    payload_hash := public.digest(candidate_encrypted_payload,'sha256');
    semantic_request_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
      'sprout-native-comment-request-v1',candidate_project_id::text,caller_id::text,
      candidate_recipient_id::text,candidate_target_id::text,
      COALESCE(candidate_parent_id::text,''),encode(payload_hash,'hex'),candidate_key_epoch::text,
      COALESCE(candidate_run_id::text,''),COALESCE(candidate_work_item_id::text,''),
      COALESCE(candidate_claim_id::text,''),COALESCE(candidate_attempt::text,'')), 'UTF8'),'sha256');
    SELECT * INTO existing_row FROM public.native_comments
      WHERE project_id=candidate_project_id AND author_identity_id=caller_id
        AND idempotency_key=candidate_idempotency_key;
    IF FOUND THEN
      IF existing_row.request_hash<>semantic_request_hash THEN
        RAISE EXCEPTION 'comment idempotency equivocation' USING ERRCODE='40001';
      END IF;
      RETURN QUERY SELECT existing_row.id,true;
      RETURN;
    END IF;
    candidate_event_id := gen_random_uuid();
    SELECT identity.principal_kind INTO STRICT recipient_kind
      FROM public.identities identity
      JOIN public.project_memberships membership ON membership.identity_id=identity.id
      JOIN public.governed_agents agent
        ON agent.project_id=membership.project_id
       AND agent.principal_identity_id=identity.id AND agent.state='active'
      WHERE identity.id=candidate_recipient_id AND identity.status='active'
        AND membership.project_id=candidate_project_id AND membership.state='active';
    IF recipient_kind <> 'agent' OR caller_id=candidate_recipient_id THEN
      RAISE EXCEPTION 'invalid comment parties' USING ERRCODE='42501';
    END IF;
    SELECT * INTO STRICT target_row FROM public.resource_nodes
      WHERE project_id=candidate_project_id AND id=candidate_target_id
        AND deleted_at IS NULL FOR SHARE;
    IF NOT sprout_private.comment_permission_allowed(
      candidate_project_id,candidate_target_id,caller_id,'post_comment')
    THEN
      RAISE EXCEPTION 'comment post permission denied' USING ERRCODE='42501';
    END IF;
    IF NOT (target_row.created_by_identity_id=candidate_recipient_id OR
      sprout_private.comment_permission_allowed(
        candidate_project_id,candidate_target_id,candidate_recipient_id,'read_comment'))
    THEN
      RAISE EXCEPTION 'comment recipient cannot observe target' USING ERRCODE='42501';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM public.resource_epochs epoch
      WHERE epoch.project_id=candidate_project_id
        AND epoch.resource_node_id=candidate_target_id
        AND epoch.epoch=candidate_key_epoch AND epoch.retired_at IS NULL)
    THEN
      RAISE EXCEPTION 'stale comment key epoch' USING ERRCODE='23514';
    END IF;
    SELECT max_agent_comment_depth INTO STRICT coordination_depth
      FROM public.agent_coordination_policy_versions WHERE version=1;

    IF candidate_run_id IS NOT NULL THEN
      SELECT * INTO STRICT run_row FROM public.agent_collaborative_runs
        WHERE project_id=candidate_project_id AND id=candidate_run_id FOR UPDATE;
      SELECT * INTO STRICT release_root FROM public.agent_r541_release_roots
        WHERE project_id=candidate_project_id AND run_id=candidate_run_id;
      semantic_now := sprout_private.allocate_agent_run_semantic_tick(
        candidate_project_id,candidate_run_id,candidate_event_id,'comment_posted',
        semantic_request_hash);
    END IF;

    IF candidate_parent_id IS NOT NULL THEN
      SELECT * INTO STRICT parent_row FROM public.native_comments
        WHERE project_id=candidate_project_id AND id=candidate_parent_id FOR SHARE;
      IF parent_row.target_resource_node_id IS DISTINCT FROM candidate_target_id THEN
        RAISE EXCEPTION 'comment parent target mismatch' USING ERRCODE='23514';
      END IF;
    END IF;

    IF caller_kind='agent' THEN
      IF candidate_run_id IS NULL OR candidate_work_item_id IS NULL
         OR candidate_claim_id IS NULL OR candidate_attempt IS NULL THEN
        RAISE EXCEPTION 'agent comment requires exact work binding' USING ERRCODE='42501';
      END IF;
      IF run_row.run_status<>'running' THEN
        RAISE EXCEPTION 'agent comment run is not active' USING ERRCODE='42501';
      END IF;
      work_json := run_row.state->'work_items'->candidate_work_item_id::text;
      claim_json := run_row.state->'claims'->candidate_claim_id::text;
      IF work_json IS NULL OR claim_json IS NULL
         OR work_json->>'id' IS DISTINCT FROM candidate_work_item_id::text
         OR work_json->>'run' IS DISTINCT FROM candidate_run_id::text
         OR work_json->>'goal' IS DISTINCT FROM run_row.goal_id::text
         OR work_json->>'owner' IS DISTINCT FROM caller_id::text
         OR work_json->>'status' IS DISTINCT FROM 'claimed'
         OR work_json->>'attempt' IS NULL
         OR (work_json->>'attempt')::integer IS DISTINCT FROM candidate_attempt
         OR claim_json->>'id' IS DISTINCT FROM candidate_claim_id::text
         OR claim_json->>'work' IS DISTINCT FROM candidate_work_item_id::text
         OR claim_json->>'claimant' IS DISTINCT FROM caller_id::text
         OR claim_json->>'status' IS DISTINCT FROM 'active'
         OR claim_json->>'attempt' IS NULL
         OR (claim_json->>'attempt')::integer IS DISTINCT FROM candidate_attempt
         OR claim_json->>'acquired_at' IS NULL OR claim_json->>'expires_at' IS NULL
         OR (claim_json->>'acquired_at')::bigint>semantic_now
         OR semantic_now>=(claim_json->>'expires_at')::bigint
      THEN
        RAISE EXCEPTION 'agent comment claim binding mismatch' USING ERRCODE='42501';
      END IF;
      work_spec_id := (work_json->>'work_spec_id')::bigint;
      exact_work_spec := (SELECT value FROM jsonb_array_elements(run_row.contract->'work_specs') value
        WHERE (value->>'id')::bigint=work_spec_id);
      IF exact_work_spec IS NULL OR NOT (exact_work_spec->'allowed_actions' ? 'post_comment') THEN
        RAISE EXCEPTION 'WorkSpec does not allow postComment' USING ERRCODE='42501';
      END IF;
      SELECT certificate.canonical_output INTO STRICT local_compiler
        FROM public.agent_local_goal_contracts local
        JOIN public.agent_compilation_certificates certificate
          ON certificate.project_id=local.project_id
         AND certificate.id=local.compilation_certificate_id
         AND certificate.task_kind='local_goal'
         AND certificate.verification_state='verified'
        WHERE local.project_id=candidate_project_id
          AND local.id=run_row.local_goal_id AND local.revision=run_row.local_goal_revision;
      exact_policy := (SELECT value FROM jsonb_array_elements(local_compiler->'security_policies') value
        WHERE (value->>'work_spec_id')::bigint=work_spec_id);
      IF exact_policy IS NULL OR NOT (exact_policy->'allowed_operations' ? 'post_comment') THEN
        RAISE EXCEPTION 'security policy does not allow postComment' USING ERRCODE='42501';
      END IF;
      IF candidate_parent_id IS NULL THEN
        IF coordination_depth=0 OR EXISTS (SELECT 1 FROM public.native_comments existing
          WHERE existing.project_id=candidate_project_id AND existing.author_identity_id=caller_id
            AND existing.recipient_identity_id=candidate_recipient_id
            AND existing.target_resource_node_id=candidate_target_id
            AND existing.author_kind='agent' AND existing.parent_comment_id IS NULL)
        THEN
          RAISE EXCEPTION 'agent root comment is not fresh' USING ERRCODE='23514';
        END IF;
        derived_depth := 1;
      ELSE
        IF parent_row.recipient_identity_id<>caller_id
           OR parent_row.agent_depth>=coordination_depth THEN
          RAISE EXCEPTION 'invalid agent comment parent' USING ERRCODE='23514';
        END IF;
        derived_depth := parent_row.agent_depth+1;
      END IF;
    ELSE
      IF candidate_work_item_id IS NOT NULL OR candidate_claim_id IS NOT NULL
         OR candidate_attempt IS NOT NULL THEN
        RAISE EXCEPTION 'human comment cannot claim AgentAction provenance' USING ERRCODE='23514';
      END IF;
      derived_depth := 0;
    END IF;

    IF candidate_run_id IS NOT NULL AND semantic_now<release_root.start_tick THEN
      RAISE EXCEPTION 'comment semantic tick predates exact trace' USING ERRCODE='23514';
    END IF;

    -- A reply is the concrete response event.  Enforce administrator > user >
    -- agent before allowing a lower-priority parent to be answered.
    IF caller_kind='agent' AND candidate_parent_id IS NOT NULL
       AND EXISTS (
         SELECT 1 FROM public.native_comments high
         WHERE high.project_id=candidate_project_id
           AND high.recipient_identity_id=caller_id
           AND high.semantic_tick<=parent_row.semantic_tick
           AND CASE high.author_kind WHEN 'administrator' THEN 3 WHEN 'user' THEN 2 ELSE 1 END
             > CASE parent_row.author_kind WHEN 'administrator' THEN 3 WHEN 'user' THEN 2 ELSE 1 END
           AND NOT EXISTS (SELECT 1 FROM public.native_comment_responses response
             WHERE response.project_id=high.project_id
               AND response.run_id=candidate_run_id AND response.comment_id=high.id)
       )
    THEN
      RAISE EXCEPTION 'higher-priority comment remains unanswered' USING ERRCODE='42501';
    END IF;

    candidate_comment_id := gen_random_uuid();
    candidate_snapshot := jsonb_build_object(
      'id',candidate_comment_id,'author',caller_id,'recipient',candidate_recipient_id,
      'target',candidate_target_id,'parent',candidate_parent_id,'agent_depth',derived_depth,
      'payload_commitment',encode(payload_hash,'hex'),'key_epoch',candidate_key_epoch);
    candidate_event_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
      'sprout-native-comment-event-v1',candidate_project_id::text,candidate_snapshot::text,
      semantic_now::text,CASE WHEN caller_kind='agent' THEN 'agent_action' ELSE 'human_native' END,
      encode(semantic_request_hash,'hex')), 'UTF8'),'sha256');
    SELECT event.semantic_state_hash, event.project_ordinal+1
      INTO prior_comment_state_hash,next_project_comment_ordinal
      FROM public.native_comment_events event
      WHERE event.project_id=candidate_project_id
      ORDER BY event.project_ordinal DESC LIMIT 1;
    IF NOT FOUND THEN next_project_comment_ordinal:=1; END IF;
    candidate_comment_state_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
      'sprout-native-comment-semantic-state-v1',candidate_project_id::text,
      next_project_comment_ordinal::text,COALESCE(encode(prior_comment_state_hash,'hex'),''),
      encode(candidate_event_hash,'hex')), 'UTF8'),'sha256');
    INSERT INTO public.native_comments (
      id,project_id,author_identity_id,author_kind,recipient_identity_id,target_resource_node_id,
      parent_comment_id,agent_depth,encrypted_payload,payload_commitment,key_epoch,semantic_tick,
      idempotency_key,request_hash,run_id,goal_id,work_item_id,claim_id,attempt,trace_number,event_hash
    ) VALUES (
      candidate_comment_id,candidate_project_id,caller_id,caller_kind,candidate_recipient_id,
      candidate_target_id,candidate_parent_id,derived_depth,candidate_encrypted_payload,payload_hash,
      candidate_key_epoch,semantic_now,candidate_idempotency_key,semantic_request_hash,
      candidate_run_id,CASE WHEN candidate_run_id IS NULL THEN NULL ELSE run_row.goal_id END,
      candidate_work_item_id,candidate_claim_id,candidate_attempt,
      CASE WHEN candidate_run_id IS NULL THEN NULL ELSE release_root.trace_number END,candidate_event_hash);
    INSERT INTO public.native_comment_events (
      id,project_id,comment_id,project_ordinal,run_id,trace_number,semantic_tick,event_kind,
      action_path,comment_snapshot,event_hash,previous_state_hash,semantic_state_hash
    ) VALUES (candidate_event_id,candidate_project_id,candidate_comment_id,next_project_comment_ordinal,
      candidate_run_id,CASE WHEN candidate_run_id IS NULL THEN NULL ELSE release_root.trace_number END,
      semantic_now,'comment_posted',
      CASE WHEN caller_kind='agent' THEN 'agent_action' ELSE 'human_native' END,
      candidate_snapshot,candidate_event_hash,prior_comment_state_hash,candidate_comment_state_hash);
    IF candidate_run_id IS NOT NULL THEN
      INSERT INTO public.native_comment_notifications (
        id,project_id,run_id,comment_id,recipient_identity_id,semantic_tick,notification_hash
      ) VALUES (gen_random_uuid(),candidate_project_id,candidate_run_id,candidate_comment_id,
        candidate_recipient_id,semantic_now,public.digest(pg_catalog.convert_to(concat_ws(E'\n',
          'sprout-comment-notification-v1',candidate_project_id::text,candidate_run_id::text,
          candidate_comment_id::text,candidate_recipient_id::text,semantic_now::text),'UTF8'),'sha256'));
      IF caller_kind='agent' AND candidate_parent_id IS NOT NULL THEN
        INSERT INTO public.native_comment_responses (
          project_id,run_id,comment_id,response_comment_id,recipient_identity_id,response_tick,response_hash
        ) VALUES (candidate_project_id,candidate_run_id,candidate_parent_id,candidate_comment_id,
          caller_id,semantic_now,public.digest(pg_catalog.convert_to(concat_ws(E'\n',
            'sprout-comment-response-v1',candidate_project_id::text,candidate_run_id::text,
            candidate_parent_id::text,candidate_comment_id::text,semantic_now::text),'UTF8'),'sha256'));
      END IF;
      IF caller_kind='agent' THEN
        INSERT INTO public.agent_native_comment_security_effects (
          id,project_id,run_id,goal_id,work_item_id,claim_id,attempt,actor_identity_id,
          comment_id,target_resource_node_id,context_sources,payload_commitment,observed_tick,effect_hash
        ) VALUES (gen_random_uuid(),candidate_project_id,candidate_run_id,run_row.goal_id,
          candidate_work_item_id,candidate_claim_id,candidate_attempt,caller_id,candidate_comment_id,
          candidate_target_id,'[]'::jsonb,payload_hash,semantic_now,
          public.digest(pg_catalog.convert_to(concat_ws(E'\n','sprout-agent-comment-effect-v1',
            candidate_project_id::text,candidate_run_id::text,candidate_work_item_id::text,
            candidate_claim_id::text,candidate_attempt::text,caller_id::text,candidate_comment_id::text,
            candidate_target_id::text,encode(payload_hash,'hex'),semantic_now::text),'UTF8'),'sha256'));
      END IF;
      candidate_record_id := gen_random_uuid();
      candidate_record_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
        'sprout-r541-comment-record-v1',release_root.trace_number::text,
        candidate_project_id::text,candidate_run_id::text,run_row.goal_id::text,
        semantic_now::text,candidate_comment_id::text,candidate_event_id::text,
        candidate_snapshot::text), 'UTF8'),'sha256');
      INSERT INTO public.agent_r541_comment_records (
        id,trace_number,project_id,run_id,goal_id,semantic_tick,comment_id,
        comment_event_id,comment_snapshot,record_hash
      ) VALUES (candidate_record_id,release_root.trace_number,candidate_project_id,
        candidate_run_id,run_row.goal_id,semantic_now,candidate_comment_id,candidate_event_id,
        candidate_snapshot,candidate_record_hash);
      SELECT COALESCE(max(ordinal),0)+1 INTO next_ordinal
        FROM public.agent_r541_comment_inventory WHERE trace_number=release_root.trace_number;
      INSERT INTO public.agent_r541_comment_inventory (
        trace_number,project_id,ordinal,comment_record_id,record_hash
      ) VALUES (release_root.trace_number,candidate_project_id,next_ordinal,
        candidate_record_id,candidate_record_hash);
      PERFORM sprout_private.append_agent_comment_certificate(release_root.trace_number,semantic_now);
    END IF;
    RETURN QUERY SELECT candidate_comment_id,false;
END
$$;

CREATE VIEW agent_r541_exact_comment_inventory_state AS
SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,
  COALESCE(jsonb_agg(jsonb_build_object(
    'ordinal',inventory.ordinal,'record_id',inventory.comment_record_id,
    'record_hash',encode(inventory.record_hash,'hex')) ORDER BY inventory.ordinal), '[]'::jsonb)
    AS comment_inventory,
  COALESCE(max(inventory.ordinal),0) AS last_ordinal,
  count(record.id) FILTER (WHERE
    record.trace_number=root.trace_number AND record.project_id=root.project_id
    AND record.run_id=root.run_id AND record.goal_id=root.goal_id
    AND record.semantic_tick BETWEEN root.start_tick AND
      COALESCE((SELECT max(certificate.end_tick) FROM agent_r541_comment_certificates certificate
        WHERE certificate.trace_number=root.trace_number),record.semantic_tick)
    AND record.comment_snapshot=event.comment_snapshot
    AND event.project_id=record.project_id AND event.comment_id=record.comment_id
    AND event.run_id=record.run_id AND event.trace_number=record.trace_number
    AND event.semantic_tick=record.semantic_tick
    AND comment.id=record.comment_id AND comment.project_id=record.project_id
    AND comment.trace_number=record.trace_number AND comment.run_id=record.run_id
    AND comment.goal_id=record.goal_id AND comment.semantic_tick=record.semantic_tick
    AND comment.event_hash=event.event_hash
    AND exact.id=record.id
    AND inventory.record_hash=record.record_hash) AS exact_record_count
FROM agent_r541_release_roots root
LEFT JOIN agent_r541_comment_inventory inventory ON inventory.trace_number=root.trace_number
LEFT JOIN agent_r541_comment_records record ON record.id=inventory.comment_record_id
LEFT JOIN agent_r541_typed_exact_comment_records exact ON exact.id=record.id
LEFT JOIN native_comment_events event ON event.id=record.comment_event_id
LEFT JOIN native_comments comment ON comment.id=record.comment_id
GROUP BY root.trace_number,root.project_id,root.run_id,root.goal_id;

CREATE VIEW agent_r541_exact_comment_certificates AS
SELECT certificate.*
FROM agent_r541_comment_certificates certificate
JOIN agent_r541_exact_comment_inventory_state actual
  ON actual.trace_number=certificate.trace_number
LEFT JOIN agent_r541_comment_certificates previous
  ON previous.trace_number=certificate.trace_number AND previous.version=certificate.version-1
WHERE certificate.version=(SELECT max(candidate.version) FROM agent_r541_comment_certificates candidate
  WHERE candidate.trace_number=certificate.trace_number)
  AND actual.last_ordinal=certificate.last_ordinal
  AND actual.exact_record_count=certificate.last_ordinal
  AND actual.comment_inventory=certificate.comment_inventory
  AND certificate.inventory_commitment=digest(convert_to(actual.comment_inventory::text,'UTF8'),'sha256')
  AND ((certificate.version=1 AND certificate.previous_certificate_hash IS NULL AND previous.id IS NULL)
    OR (certificate.version>1 AND previous.certificate_hash=certificate.previous_certificate_hash
      AND previous.last_ordinal<certificate.last_ordinal AND previous.end_tick<=certificate.end_tick))
  AND certificate.certificate_hash=digest(convert_to(concat_ws(E'\n',
    'sprout-r541-comment-certificate-v1',certificate.trace_number::text,
    certificate.project_id::text,certificate.run_id::text,certificate.goal_id::text,
    certificate.version::text,certificate.end_tick::text,certificate.last_ordinal::text,
    encode(certificate.inventory_commitment,'hex'),
    COALESCE(encode(certificate.previous_certificate_hash,'hex'),'')), 'UTF8'),'sha256');

CREATE VIEW agent_r541_comment_surface_gates AS
SELECT COALESCE(run.project_id,root.project_id) AS project_id,
  COALESCE(run.id,root.run_id) AS run_id,root.trace_number,
  CASE WHEN exact.id IS NOT NULL AND exact.comment_gate_mode='enabled'
       THEN 'enabled' ELSE 'disabled_fail_closed' END AS comment_mode,
  CASE WHEN exact.id IS NOT NULL AND exact.comment_gate_mode='enabled'
       THEN exact.comment_inventory ELSE '[]'::jsonb END AS comment_records
FROM agent_collaborative_runs run
FULL OUTER JOIN agent_r541_release_roots root
  ON root.project_id=run.project_id AND root.run_id=run.id
LEFT JOIN agent_r541_exact_comment_certificates exact ON exact.trace_number=root.trace_number;

CREATE VIEW agent_r541_comment_surface_records AS
SELECT record.*,inventory.ordinal AS trace_ordinal
FROM agent_r541_comment_records record
JOIN agent_r541_comment_inventory inventory ON inventory.comment_record_id=record.id
JOIN agent_r541_comment_surface_gates gate
  ON gate.project_id=record.project_id AND gate.run_id=record.run_id
   AND gate.trace_number=record.trace_number
WHERE gate.comment_mode='enabled' AND jsonb_array_length(gate.comment_records)>0;

CREATE VIEW native_comment_readable AS
SELECT comment.id,comment.project_id,comment.author_identity_id,comment.author_kind,
  comment.recipient_identity_id,comment.target_resource_node_id,comment.parent_comment_id,
  comment.agent_depth,comment.encrypted_payload,comment.payload_commitment,comment.key_epoch,
  comment.semantic_tick,comment.run_id,comment.goal_id,comment.trace_number,comment.payload_purged_at
FROM native_comments comment
WHERE sprout_private.comment_permission_allowed(comment.project_id,
  comment.target_resource_node_id,sprout_private.current_identity_id(),'read_comment')
  AND comment.encrypted_payload IS NOT NULL;

CREATE VIEW agent_r540_exact_release_events AS
SELECT event.* FROM agent_r540_release_events event
JOIN agent_r541_release_roots root ON root.trace_number=event.trace_number
JOIN agent_run_exact_semantic_timelines timeline ON timeline.trace_number=root.trace_number
WHERE event.project_id=root.project_id AND event.run_id=root.run_id AND event.goal_id=root.goal_id
  AND event.semantic_tick>=root.start_tick
  AND (EXISTS (SELECT 1 FROM agent_run_semantic_tick_allocations allocation
    WHERE allocation.project_id=event.project_id AND allocation.run_id=event.run_id
      AND allocation.trace_number=event.trace_number
      AND allocation.semantic_tick=event.semantic_tick)
    OR (event.event_kind='causal_link' AND event.source_relation='agent_run_causal_links'
      AND event.semantic_tick=root.start_tick
      AND event.event_snapshot->>'transition_id'=root.initialization_transition_id::text))
  AND event.event_hash=digest(convert_to(concat_ws(E'\n','sprout-r540-release-event-v1',
    event.trace_number::text,event.event_kind,event.semantic_tick::text,event.source_relation,
    event.source_record_id::text,event.event_snapshot::text),'UTF8'),'sha256')
  AND CASE event.source_relation
    WHEN 'agent_run_transitions' THEN EXISTS (
      SELECT 1 FROM agent_run_transitions transition
      WHERE transition.id=event.source_record_id AND transition.project_id=event.project_id
        AND transition.run_id=event.run_id AND transition.semantic_tick=event.semantic_tick
        AND ((event.event_kind='work_attempt' AND transition.transition_kind='work_claimed'
              AND transition.state_snapshot->'work_items'->(event.event_snapshot->>'work')
                  =event.event_snapshot->'work_snapshot'
              AND transition.state_snapshot->'claims'->(event.event_snapshot->>'claim')
                  =event.event_snapshot->'claim_snapshot'
              AND event.event_snapshot->>'actor'=transition.actor_identity_id::text)
          OR (event.event_kind='work_outcome' AND transition.transition_kind IN ('work_succeeded','work_failed')
              AND transition.state_snapshot->'work_items'->(event.event_snapshot->>'work')
                  =event.event_snapshot->'work_snapshot'
              AND event.event_snapshot->>'status'=transition.state_snapshot->'work_items'
                  ->(event.event_snapshot->>'work')->>'status')))
    WHEN 'agent_r540_work_attempt_events' THEN EXISTS (
      SELECT 1 FROM agent_r540_exact_work_attempt_trace_records source
      WHERE source.id=event.source_record_id AND source.trace_number=event.trace_number
        AND source.tick=event.semantic_tick)
    WHEN 'agent_r540_tool_attempt_events' THEN EXISTS (
      SELECT 1 FROM agent_r540_exact_tool_trace_records source
      WHERE source.tool_event_id=event.source_record_id AND source.trace_number=event.trace_number
        AND source.observed_tick=event.semantic_tick)
    WHEN 'agent_r540_work_outcome_events' THEN EXISTS (
      SELECT 1 FROM agent_r540_exact_work_outcome_trace_records source
      WHERE source.id=event.source_record_id AND source.trace_number=event.trace_number
        AND source.observed_tick=event.semantic_tick)
    WHEN 'agent_run_blocker_resolutions' THEN EXISTS (
      SELECT 1 FROM agent_run_blocker_resolutions source
      JOIN agent_run_transitions transition ON transition.id=source.transition_id
      WHERE source.id=event.source_record_id AND source.project_id=event.project_id
        AND source.run_id=event.run_id AND transition.semantic_tick=event.semantic_tick
        AND transition.transition_kind='blocker_resolved'
        AND event.event_snapshot->>'blocker' IS NOT DISTINCT FROM source.blocker_id::text
        AND event.event_snapshot->>'observed_at' IS NOT DISTINCT FROM transition.semantic_tick::text
        AND event.event_snapshot#>>'{resolution,blocker}' IS NOT DISTINCT FROM source.blocker_id::text
        AND event.event_snapshot#>>'{resolution,observation_kind}' IS NOT DISTINCT FROM source.observation_kind
        AND event.event_snapshot#>>'{resolution,observation_id}' IS NOT DISTINCT FROM source.observation_id::text
        AND event.event_snapshot#>>'{resolution,terminal_status}' IS NOT DISTINCT FROM source.terminal_status
        AND event.event_snapshot#>>'{resolution,observed_at}' IS NOT DISTINCT FROM transition.semantic_tick::text
        AND event.event_snapshot#>>'{resolution,provenance_hash}' IS NOT DISTINCT FROM encode(source.provenance_hash,'hex')
        AND EXISTS (
          SELECT 1
          FROM agent_run_blockers blocker
          JOIN agent_collaborative_runs run
            ON run.project_id=blocker.project_id AND run.id=blocker.run_id
           AND run.goal_id=event.goal_id
          JOIN agent_run_work_slots slot
            ON slot.project_id=blocker.project_id AND slot.run_id=blocker.run_id
           AND slot.work_item_id=(blocker.scope->>'work')::uuid
          JOIN tasks task
            ON task.project_id=blocker.project_id
           AND task.resource_node_id=(blocker.waiting_condition->>'task')::uuid
           AND task.deleted_at IS NULL AND task.state='completed'
          JOIN task_completions completion
            ON completion.project_id=task.project_id AND completion.task_id=task.id
           AND completion.completed_at=task.completed_at
          JOIN task_assignments assignment
            ON assignment.project_id=completion.project_id
           AND assignment.task_id=completion.task_id
           AND assignment.id=completion.assignment_id
           AND assignment.revoked_at IS NULL
          JOIN identities assignee
            ON assignee.id=assignment.assignee_identity_id
           AND assignee.principal_kind='user' AND assignee.status='active'
          JOIN agent_run_causal_links causal
            ON causal.project_id=blocker.project_id AND causal.run_id=blocker.run_id
           AND causal.goal_id=event.goal_id
           AND causal.predecessor=jsonb_build_object(
             'kind','task','task',(blocker.waiting_condition->>'task')::uuid)
           AND causal.successor=jsonb_build_object(
             'kind','work','work',(blocker.scope->>'work')::uuid)
           AND causal.observed_tick<=transition.semantic_tick
          JOIN LATERAL jsonb_array_elements(run.contract->'waiting_rules') waiting_rule
            ON (waiting_rule->>'id')::bigint=blocker.waiting_rule_ordinal
           AND waiting_rule->>'obligation'=blocker.obligation_id::text
           AND waiting_rule#>>'{target,kind}'='task_from_work'
          JOIN LATERAL jsonb_array_elements(run.contract->'work_specs') work_spec
            ON (work_spec->>'id')::bigint=(waiting_rule#>>'{target,work_spec_id}')::bigint
           AND work_spec->>'obligation'=blocker.obligation_id::text
          WHERE blocker.project_id=source.project_id
            AND blocker.run_id=source.run_id AND blocker.id=source.blocker_id
            AND blocker.current_status=source.terminal_status
            AND blocker.current_status IN ('resolved','failed','cancelled')
            AND blocker.scope->>'kind'='work'
            AND blocker.waiting_condition->>'kind'='human_task_completed'
            AND transition.state_snapshot#>>ARRAY[
              'blockers',source.blocker_id::text,'run'] IS NOT DISTINCT FROM source.run_id::text
            AND transition.state_snapshot#>>ARRAY[
              'blockers',source.blocker_id::text,'goal'] IS NOT DISTINCT FROM event.goal_id::text
            AND transition.state_snapshot#>ARRAY[
              'blockers',source.blocker_id::text,'scope'] IS NOT DISTINCT FROM blocker.scope
            AND transition.state_snapshot#>ARRAY[
              'blockers',source.blocker_id::text,'condition'] IS NOT DISTINCT FROM blocker.waiting_condition
            AND transition.state_snapshot#>>ARRAY[
              'blockers',source.blocker_id::text,'status'] IS NOT DISTINCT FROM source.terminal_status
            AND transition.state_snapshot#>>ARRAY[
              'blockers',source.blocker_id::text,'terminal_at'] IS NOT DISTINCT FROM transition.semantic_tick::text
            AND transition.state_snapshot#>>ARRAY[
              'work_items',slot.work_item_id::text,'run'] IS NOT DISTINCT FROM source.run_id::text
            AND transition.state_snapshot#>>ARRAY[
              'work_items',slot.work_item_id::text,'goal'] IS NOT DISTINCT FROM event.goal_id::text
            AND transition.state_snapshot#>>ARRAY[
              'work_items',slot.work_item_id::text,'serves'] IS NOT DISTINCT FROM blocker.obligation_id::text
            AND transition.state_snapshot#>>ARRAY[
              'work_items',slot.work_item_id::text,'work_spec_id'] IS NOT DISTINCT FROM work_spec->>'id'
        ))
    WHEN 'agent_run_causal_links' THEN EXISTS (
      SELECT 1 FROM agent_run_causal_links source
      JOIN agent_run_transitions transition ON transition.id=source.transition_id
      WHERE source.id=event.source_record_id AND source.project_id=event.project_id
        AND source.run_id=event.run_id AND source.goal_id=event.goal_id
        AND transition.semantic_tick=event.semantic_tick AND source.observed_tick<=event.semantic_tick
        AND event.event_snapshot->>'recorded_at' IS NOT DISTINCT FROM transition.semantic_tick::text
        AND event.event_snapshot#>>'{link,run}' IS NOT DISTINCT FROM source.run_id::text
        AND event.event_snapshot#>>'{link,goal}' IS NOT DISTINCT FROM source.goal_id::text
        AND event.event_snapshot#>'{link,predecessor}' IS NOT DISTINCT FROM source.predecessor
        AND event.event_snapshot#>'{link,successor}' IS NOT DISTINCT FROM source.successor
        AND event.event_snapshot#>>'{link,observed_at}' IS NOT DISTINCT FROM source.observed_tick::text)
    WHEN 'agent_run_evidence_provenance' THEN EXISTS (
      SELECT 1 FROM agent_run_evidence_provenance source
      JOIN agent_run_transitions transition ON transition.id=source.transition_id
      JOIN agent_run_work_outcomes outcome
        ON outcome.project_id=source.project_id AND outcome.run_id=source.run_id
       AND outcome.work_item_id=source.work_item_id
       AND outcome.outcome_kind=source.product_event_kind
       AND outcome.product_event_id=source.product_event_id
       AND outcome.observed_at=source.observed_at
      JOIN agent_run_claim_leases claim
        ON claim.project_id=outcome.project_id AND claim.id=outcome.claim_id
       AND claim.run_id=outcome.run_id AND claim.work_item_id=outcome.work_item_id
       AND claim.attempt=outcome.attempt
      JOIN agent_run_transitions outcome_transition
        ON outcome_transition.id=outcome.transition_id
       AND outcome_transition.project_id=outcome.project_id
       AND outcome_transition.run_id=outcome.run_id
      JOIN task_completions completion
        ON completion.project_id=source.project_id AND completion.id=source.product_event_id
       AND completion.completed_at=source.observed_at
      JOIN tasks task ON task.project_id=completion.project_id AND task.id=completion.task_id
      JOIN agent_collaborative_runs run ON run.project_id=source.project_id AND run.id=source.run_id
      JOIN LATERAL jsonb_array_elements(run.contract->'evidence_rules') exact_rule
        ON (exact_rule->>'id')::bigint=source.evidence_rule_ordinal
      WHERE source.evidence_id=event.source_record_id AND source.project_id=event.project_id
        AND source.run_id=event.run_id AND transition.semantic_tick=event.semantic_tick
        AND transition.transition_kind IN ('evidence_accepted','work_succeeded')
        AND event.event_snapshot->>'work'=source.work_item_id::text
        AND event.event_snapshot->>'claim'=outcome.claim_id::text
        AND (event.event_snapshot->>'attempt')::integer=outcome.attempt
        AND event.event_snapshot->>'accepted_at'=transition.semantic_tick::text
        AND event.event_snapshot->'work_snapshot'=
            transition.state_snapshot->'work_items'->source.work_item_id::text
        AND event.event_snapshot#>>'{evidence,id}'=source.evidence_id::text
        AND event.event_snapshot#>>'{evidence,obligation}'=source.obligation_id::text
        AND (event.event_snapshot#>>'{evidence,rule_id}')::bigint=source.evidence_rule_ordinal
        AND event.event_snapshot#>>'{evidence,kind}'=source.evidence_kind
        AND event.event_snapshot#>>'{evidence,subject,kind}' IS NOT DISTINCT FROM 'task'
        AND event.event_snapshot#>>'{evidence,subject,task}' IS NOT DISTINCT FROM task.resource_node_id::text
        AND event.event_snapshot#>>'{evidence,observed_at}' IS NOT DISTINCT FROM outcome_transition.semantic_tick::text
        AND event.event_snapshot#>>'{evidence,verification}' IS NOT DISTINCT FROM 'mechanical'
        AND exact_rule->>'obligation' IS NOT DISTINCT FROM source.obligation_id::text
        AND exact_rule->>'kind' IS NOT DISTINCT FROM source.evidence_kind
        AND exact_rule->>'verification' IS NOT DISTINCT FROM source.verification_mode
        AND exact_rule#>>'{subject,kind}' IS NOT DISTINCT FROM 'work_result'
        AND (exact_rule#>>'{subject,work_spec_id}')::bigint=
            (event.event_snapshot#>>'{work_snapshot,work_spec_id}')::bigint
        AND event.event_snapshot#>>'{evidence,rule}' IS NOT DISTINCT FROM exact_rule::text
        AND event.event_snapshot#>>'{evidence,provenance_hash}'=encode(source.provenance_hash,'hex')
        AND (transition.state_snapshot#>>ARRAY[
              'work_items',source.work_item_id::text,'attempt'])::integer=outcome.attempt
        AND transition.state_snapshot#>>ARRAY[
              'work_items',source.work_item_id::text,'serves']=source.obligation_id::text
        AND 1=(SELECT count(*) FROM agent_run_claim_leases exact_claim
               WHERE exact_claim.project_id=source.project_id
                 AND exact_claim.run_id=source.run_id
                 AND exact_claim.work_item_id=source.work_item_id
                 AND exact_claim.attempt=outcome.attempt)
        AND 1=(SELECT count(*) FROM agent_r540_release_events work_event
               WHERE work_event.trace_number=event.trace_number
                 AND work_event.event_kind='work_attempt'
                 AND COALESCE(work_event.event_snapshot->>'work',
                              work_event.event_snapshot->>'work_item_id')=source.work_item_id::text
                 AND COALESCE(work_event.event_snapshot->>'claim',
                              work_event.event_snapshot->>'claim_id')=outcome.claim_id::text
                 AND (work_event.event_snapshot->>'attempt')::integer=outcome.attempt))
    WHEN 'agent_model_invocation_projections' THEN EXISTS (
      SELECT 1 FROM agent_model_invocation_projections source
      JOIN agent_invocations invocation ON invocation.project_id=source.project_id
        AND invocation.id=source.invocation_id AND invocation.status='succeeded'
      JOIN agent_model_attempt_dispatches dispatch ON dispatch.project_id=source.project_id
        AND dispatch.invocation_id=source.invocation_id AND dispatch.attempt=source.provider_attempt
      JOIN agent_model_attempt_observations observation ON observation.project_id=source.project_id
        AND observation.id=source.observation_id AND observation.status='succeeded'
      WHERE source.id=event.source_record_id AND source.project_id=event.project_id
        AND source.run_id=event.run_id AND source.goal_id=event.goal_id
        AND source.semantic_tick=event.semantic_tick
        AND dispatch.semantic_tick=event.semantic_tick
        AND event.event_snapshot->>'work' IS NOT DISTINCT FROM source.work_item_id::text
        AND event.event_snapshot->>'claim' IS NOT DISTINCT FROM source.work_claim_id::text
        AND event.event_snapshot->>'attempt' IS NOT DISTINCT FROM source.work_attempt::text
        AND event.event_snapshot->>'principal' IS NOT DISTINCT FROM source.principal_identity_id::text
        AND event.event_snapshot#>>'{context,direct_sources}' IS NOT DISTINCT FROM source.context_source_descriptors::text
        AND event.event_snapshot#>>'{projection,direct_sources_exposed}' IS NOT DISTINCT FROM source.context_source_descriptors::text
        AND event.event_snapshot#>>'{projection,hidden_persistent_model_memory_available}' IS NOT DISTINCT FROM 'false'
        AND event.event_snapshot->>'invoked_at' IS NOT DISTINCT FROM source.semantic_tick::text
        AND event.event_snapshot#>>'{input_payload_pointer,relation}' IS NOT DISTINCT FROM 'agent_invocations'
        AND event.event_snapshot#>>'{input_payload_pointer,id}' IS NOT DISTINCT FROM invocation.id::text
        AND event.event_snapshot#>>'{input_payload_pointer,commitment}' IS NOT DISTINCT FROM encode(digest(invocation.encrypted_input,'sha256'),'hex')
        AND event.event_snapshot#>>'{output_payload_pointer,relation}' IS NOT DISTINCT FROM 'agent_invocations'
        AND event.event_snapshot#>>'{output_payload_pointer,id}' IS NOT DISTINCT FROM invocation.id::text
        AND event.event_snapshot#>>'{output_payload_pointer,commitment}' IS NOT DISTINCT FROM encode(digest(invocation.encrypted_output,'sha256'),'hex'))
    WHEN 'agent_interrogation_answers' THEN EXISTS (
      SELECT 1 FROM agent_interrogation_answers source
      JOIN agent_interrogations session ON session.project_id=source.project_id
        AND session.id=source.interrogation_id
      JOIN agent_model_invocation_projections projection ON projection.project_id=source.project_id
        AND projection.invocation_id=source.invocation_id AND projection.status='succeeded'
      WHERE source.id=event.source_record_id AND source.project_id=event.project_id
        AND projection.run_id=event.run_id AND projection.goal_id=event.goal_id
        AND source.semantic_tick=event.semantic_tick
        AND session.semantic_tick IS NOT NULL AND session.semantic_tick<=source.semantic_tick
        AND source.question_state_fingerprint=source.answer_state_fingerprint
        AND event.event_snapshot->>'observed_at' IS NOT DISTINCT FROM source.semantic_tick::text
        AND event.event_snapshot#>>'{session,id}' IS NOT DISTINCT FROM session.id::text
        AND event.event_snapshot#>>'{session,creator}' IS NOT DISTINCT FROM session.creator_identity_id::text
        AND event.event_snapshot#>>'{session,target_agent}' IS NOT DISTINCT FROM session.target_agent_identity_id::text
        AND event.event_snapshot#>>'{session,created_at}' IS NOT DISTINCT FROM session.semantic_tick::text
        AND event.event_snapshot#>>'{question_payload_pointer,relation}' IS NOT DISTINCT FROM 'agent_interrogations'
        AND event.event_snapshot#>>'{question_payload_pointer,id}' IS NOT DISTINCT FROM session.id::text
        AND event.event_snapshot#>>'{question_payload_pointer,commitment}' IS NOT DISTINCT FROM encode(digest(session.encrypted_transcript,'sha256'),'hex')
        AND event.event_snapshot#>>'{answer_payload_pointer,relation}' IS NOT DISTINCT FROM 'agent_interrogation_answers'
        AND event.event_snapshot#>>'{answer_payload_pointer,id}' IS NOT DISTINCT FROM source.id::text
        AND event.event_snapshot#>>'{answer_payload_pointer,commitment}' IS NOT DISTINCT FROM encode(digest(source.encrypted_answer,'sha256'),'hex')
        AND event.event_snapshot->>'delta' IS NOT DISTINCT FROM session.causal_delta::text
        AND session.causal_delta=jsonb_build_object('resource_effects','[]'::jsonb,
          'tool_invocations','[]'::jsonb,'prompt_revisions','[]'::jsonb,
          'local_goal_revisions','[]'::jsonb,'created_work','[]'::jsonb,
          'activated_obligations','[]'::jsonb,'assigned_tasks','[]'::jsonb))
    WHEN 'agent_native_comment_security_effects' THEN EXISTS (
      SELECT 1 FROM agent_native_comment_security_effects source
      JOIN native_comments comment ON comment.project_id=source.project_id AND comment.id=source.comment_id
      WHERE source.id=event.source_record_id AND source.project_id=event.project_id
        AND source.run_id=event.run_id AND source.goal_id=event.goal_id
        AND source.observed_tick=event.semantic_tick AND source.payload_commitment=comment.payload_commitment
        AND comment.encrypted_payload IS NOT NULL
        AND event.event_snapshot#>>'{payload_pointer,relation}' IS NOT DISTINCT FROM 'native_comments'
        AND event.event_snapshot#>>'{payload_pointer,id}' IS NOT DISTINCT FROM comment.id::text
        AND event.event_snapshot#>>'{payload_pointer,commitment}' IS NOT DISTINCT FROM encode(digest(comment.encrypted_payload,'sha256'),'hex'))
    WHEN 'agent_effect_proposals' THEN EXISTS (
      SELECT 1 FROM agent_effect_proposals source
      JOIN agent_model_invocation_projections projection ON projection.project_id=source.project_id
        AND projection.invocation_id=source.invocation_id AND projection.status='succeeded'
      WHERE source.id=event.source_record_id AND source.project_id=event.project_id
        AND source.status='applied' AND projection.run_id=event.run_id
        AND projection.goal_id=event.goal_id
        AND source.applied_semantic_tick=event.semantic_tick
        AND source.effect#>>'{effect,operation}'='edit_info'
        AND event.event_snapshot#>>'{sink,kind}' IS NOT DISTINCT FROM 'info_document'
        AND event.event_snapshot#>>'{sink,container}' IS NOT DISTINCT FROM source.effect#>>'{effect,resource_id}'
        AND source.encrypted_materialization IS NOT NULL
        AND event.event_snapshot#>>'{payload_pointer,relation}' IS NOT DISTINCT FROM 'agent_effect_proposals'
        AND event.event_snapshot#>>'{payload_pointer,id}' IS NOT DISTINCT FROM source.id::text
        AND event.event_snapshot#>>'{payload_pointer,commitment}' IS NOT DISTINCT FROM encode(digest(source.encrypted_materialization,'sha256'),'hex'))
    ELSE false END;

-- Content-addressed rows above are only pointers.  This independent view is
-- the typed projection authority: it reconstructs each full encrypted/formal
-- value from its immutable operational source and disappears if that source
-- or payload is absent.  Certificates consume this view, never a hash alone.
CREATE VIEW agent_r540_typed_exact_release_events AS
SELECT typed.* FROM (
 SELECT exact.*,
   CASE exact.source_relation
   WHEN 'agent_run_transitions' THEN CASE exact.event_kind
     WHEN 'work_attempt' THEN jsonb_build_object(
       'trace_id',exact.trace_number,'run',exact.run_id,'goal',exact.goal_id,
       'work',exact.event_snapshot->'work','claim',exact.event_snapshot->'claim',
       'attempt',exact.event_snapshot->'attempt','actor',exact.event_snapshot->'actor',
       'tick',exact.semantic_tick)
     WHEN 'work_outcome' THEN jsonb_build_object(
       'trace_id',exact.trace_number,'run',exact.run_id,'goal',exact.goal_id,
       'work',exact.event_snapshot->'work','claim',exact.event_snapshot->'claim',
       'attempt',exact.event_snapshot->'attempt','status',exact.event_snapshot->'status',
       'observed_at',exact.semantic_tick)
     ELSE NULL END
   WHEN 'agent_r540_work_attempt_events' THEN jsonb_build_object(
     'trace_id',exact.trace_number,'run',exact.run_id,'goal',exact.goal_id,
     'work',exact.event_snapshot->'work_item_id','claim',exact.event_snapshot->'claim_id',
     'attempt',exact.event_snapshot->'attempt','actor',exact.event_snapshot->'actor_identity_id',
     'tick',exact.semantic_tick)
   WHEN 'agent_r540_work_outcome_events' THEN jsonb_build_object(
     'trace_id',exact.trace_number,'run',exact.run_id,'goal',exact.goal_id,
     'work',exact.event_snapshot->'work_item_id','claim',exact.event_snapshot->'claim_id',
     'attempt',exact.event_snapshot->'attempt','status',exact.event_snapshot->'status',
     'observed_at',exact.semantic_tick)
   WHEN 'agent_r540_tool_attempt_events' THEN jsonb_build_object(
     'trace_id',exact.trace_number,'run',exact.run_id,'goal',exact.goal_id,
     'work',exact.event_snapshot->'work_item_id','claim',exact.event_snapshot->'claim_id',
     'attempt',exact.event_snapshot->'attempt','owner',exact.event_snapshot->'owner_identity_id',
     'call_id',exact.event_snapshot->'call_id',
     'tool',jsonb_build_object('id',exact.event_snapshot->'tool_name',
       'version',exact.event_snapshot->'tool_version'),
     'input',exact.event_snapshot->'canonical_input_commitment',
     'status',exact.event_snapshot->'status',
     'output',exact.event_snapshot->'canonical_output_commitment',
     'requested_at',exact.event_snapshot->'requested_tick','observed_at',exact.semantic_tick)
   WHEN 'agent_run_blocker_resolutions' THEN jsonb_build_object(
     'trace_id',exact.trace_number,'run',exact.run_id,'goal',exact.goal_id,
     'blocker',exact.event_snapshot->'blocker','resolution',jsonb_build_object(
       'blocker',exact.event_snapshot#>'{resolution,blocker}',
       'observed_at',exact.event_snapshot#>'{resolution,observed_at}'),
     'observed_at',exact.semantic_tick)
   WHEN 'agent_run_causal_links' THEN jsonb_build_object(
     'trace_id',exact.trace_number,'run',exact.run_id,'goal',exact.goal_id,
     'link',exact.event_snapshot->'link','recorded_at',exact.semantic_tick)
   WHEN 'agent_run_evidence_provenance' THEN jsonb_build_object(
     'trace_id',exact.trace_number,'run',exact.run_id,'goal',exact.goal_id,
     'work',exact.event_snapshot->'work','claim',exact.event_snapshot->'claim',
     'attempt',exact.event_snapshot->'attempt','evidence',jsonb_build_object(
       'id',exact.event_snapshot#>'{evidence,id}',
       'run',exact.event_snapshot#>'{evidence,run}',
       'obligation',exact.event_snapshot#>'{evidence,obligation}',
       'kind',exact.event_snapshot#>'{evidence,kind}',
       'subject',exact.event_snapshot#>'{evidence,subject}',
       'observed_at',exact.event_snapshot#>'{evidence,observed_at}'),
     'accepted_at',exact.semantic_tick)
   WHEN 'agent_model_invocation_projections' THEN (
     SELECT jsonb_build_object(
         'trace_id',exact.trace_number,'run',exact.run_id,'goal',exact.goal_id,
         'work',projection.work_item_id,'attempt',projection.work_attempt,
         'principal',projection.principal_identity_id,
         'context',jsonb_build_object('direct_sources',projection.context_source_descriptors),
         'projection',jsonb_build_object(
           'direct_sources_exposed',projection.context_source_descriptors,
           'hidden_persistent_model_memory_available',false),
         'input_payload',sprout_private.try_parse_encrypted_payload(invocation.encrypted_input),
         'output_payload',sprout_private.try_parse_encrypted_payload(invocation.encrypted_output),
         'invoked_at',projection.semantic_tick)
     FROM agent_model_invocation_projections projection
     JOIN agent_invocations invocation ON invocation.project_id=projection.project_id
       AND invocation.id=projection.invocation_id
     WHERE projection.id=exact.source_record_id
       AND sprout_private.try_parse_encrypted_payload(invocation.encrypted_input) IS NOT NULL
       AND sprout_private.try_parse_encrypted_payload(invocation.encrypted_output) IS NOT NULL)
   WHEN 'agent_interrogation_answers' THEN (
     SELECT jsonb_build_object(
       'trace_id',exact.trace_number,'session',exact.event_snapshot->'session',
       'question',jsonb_build_object('session_id',session.id,
         'payload',sprout_private.try_parse_encrypted_payload(session.encrypted_transcript),
         'asked_at',session.semantic_tick),
       'answer',jsonb_build_object('session_id',session.id,
         'responder',session.target_agent_identity_id,
         'payload',sprout_private.try_parse_encrypted_payload(answer.encrypted_answer),
         'answered_at',answer.semantic_tick,
         'context_sources',answer.context_source_descriptors),
       'delta',session.causal_delta,
       'context',jsonb_build_object('direct_sources',answer.context_source_descriptors),
       'projection',jsonb_build_object('direct_sources_exposed',projection.context_source_descriptors,
         'hidden_persistent_model_memory_available',false),
       'observed_at',answer.semantic_tick)
     FROM agent_interrogation_answers answer
     JOIN agent_interrogations session ON session.project_id=answer.project_id
       AND session.id=answer.interrogation_id
     JOIN agent_model_invocation_projections projection ON projection.project_id=answer.project_id
       AND projection.invocation_id=answer.invocation_id AND projection.status='succeeded'
     WHERE answer.id=exact.source_record_id
       AND sprout_private.try_parse_encrypted_payload(session.encrypted_transcript) IS NOT NULL
       AND sprout_private.try_parse_encrypted_payload(answer.encrypted_answer) IS NOT NULL)
   WHEN 'agent_native_comment_security_effects' THEN (
     SELECT jsonb_build_object(
       'trace_id',exact.trace_number,'run',exact.run_id,'goal',exact.goal_id,
       'work',effect.work_item_id,'attempt',effect.attempt,'actor',effect.actor_identity_id,
       'sink',jsonb_build_object('kind','comment_on','target',effect.target_resource_node_id),
       'sources',effect.context_sources,
       'payload',sprout_private.try_parse_encrypted_payload(comment.encrypted_payload),
       'observed_at',effect.observed_tick)
     FROM agent_native_comment_security_effects effect
     JOIN native_comments comment ON comment.project_id=effect.project_id AND comment.id=effect.comment_id
     WHERE effect.id=exact.source_record_id
       AND sprout_private.try_parse_encrypted_payload(comment.encrypted_payload) IS NOT NULL)
   WHEN 'agent_effect_proposals' THEN (
     SELECT jsonb_build_object(
       'trace_id',exact.trace_number,'run',exact.run_id,'goal',exact.goal_id,
       'work',projection.work_item_id,'attempt',projection.work_attempt,
       'actor',projection.principal_identity_id,
       'sink',jsonb_build_object('kind','info_document',
         'container',effect.effect#>>'{effect,resource_id}'),
       'sources',projection.context_source_descriptors,
       'payload',sprout_private.try_parse_encrypted_payload(effect.encrypted_materialization),
       'observed_at',effect.applied_semantic_tick)
     FROM agent_effect_proposals effect
     JOIN agent_model_invocation_projections projection ON projection.project_id=effect.project_id
       AND projection.invocation_id=effect.invocation_id AND projection.status='succeeded'
     WHERE effect.id=exact.source_record_id
       AND sprout_private.try_parse_encrypted_payload(effect.encrypted_materialization) IS NOT NULL)
   ELSE NULL END AS formal_record
 FROM agent_r540_exact_release_events exact
) typed
WHERE typed.formal_record IS NOT NULL;

CREATE VIEW agent_r540_release_inventory_state AS
SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,root.start_tick,
 COALESCE(jsonb_agg(jsonb_build_object('ordinal',inventory.ordinal,'event_id',inventory.event_id,
  'event_hash',encode(inventory.event_hash,'hex')) ORDER BY inventory.ordinal),'[]'::jsonb) AS all_inventory,
 COALESCE(max(inventory.ordinal),0) AS last_ordinal,
 count(exact.id) AS exact_event_count,
 COALESCE(jsonb_agg(jsonb_build_object('ordinal',inventory.ordinal,'event_id',inventory.event_id,'event_hash',encode(inventory.event_hash,'hex')) ORDER BY inventory.ordinal) FILTER(WHERE inventory.event_kind='work_attempt'),'[]'::jsonb) AS work_attempt_inventory,
 COALESCE(jsonb_agg(jsonb_build_object('ordinal',inventory.ordinal,'event_id',inventory.event_id,'event_hash',encode(inventory.event_hash,'hex')) ORDER BY inventory.ordinal) FILTER(WHERE inventory.event_kind='work_outcome'),'[]'::jsonb) AS work_outcome_inventory,
 COALESCE(jsonb_agg(jsonb_build_object('ordinal',inventory.ordinal,'event_id',inventory.event_id,'event_hash',encode(inventory.event_hash,'hex')) ORDER BY inventory.ordinal) FILTER(WHERE inventory.event_kind='blocker_resolution'),'[]'::jsonb) AS blocker_inventory,
 COALESCE(jsonb_agg(jsonb_build_object('ordinal',inventory.ordinal,'event_id',inventory.event_id,'event_hash',encode(inventory.event_hash,'hex')) ORDER BY inventory.ordinal) FILTER(WHERE inventory.event_kind='causal_link'),'[]'::jsonb) AS causal_inventory,
 COALESCE(jsonb_agg(jsonb_build_object('ordinal',inventory.ordinal,'event_id',inventory.event_id,'event_hash',encode(inventory.event_hash,'hex')) ORDER BY inventory.ordinal) FILTER(WHERE inventory.event_kind='tool_event'),'[]'::jsonb) AS tool_inventory,
 COALESCE(jsonb_agg(jsonb_build_object('ordinal',inventory.ordinal,'event_id',inventory.event_id,'event_hash',encode(inventory.event_hash,'hex')) ORDER BY inventory.ordinal) FILTER(WHERE inventory.event_kind='evidence'),'[]'::jsonb) AS evidence_inventory,
 COALESCE(jsonb_agg(jsonb_build_object('ordinal',inventory.ordinal,'event_id',inventory.event_id,'event_hash',encode(inventory.event_hash,'hex')) ORDER BY inventory.ordinal) FILTER(WHERE inventory.event_kind='disclosure'),'[]'::jsonb) AS disclosure_inventory,
 COALESCE(jsonb_agg(jsonb_build_object('ordinal',inventory.ordinal,'event_id',inventory.event_id,'event_hash',encode(inventory.event_hash,'hex')) ORDER BY inventory.ordinal) FILTER(WHERE inventory.event_kind='model_invocation'),'[]'::jsonb) AS model_inventory,
 COALESCE(jsonb_agg(jsonb_build_object('ordinal',inventory.ordinal,'event_id',inventory.event_id,'event_hash',encode(inventory.event_hash,'hex')) ORDER BY inventory.ordinal) FILTER(WHERE inventory.event_kind='interrogation'),'[]'::jsonb) AS interrogation_inventory
FROM agent_r541_release_roots root
LEFT JOIN agent_r540_release_inventory inventory ON inventory.trace_number=root.trace_number
LEFT JOIN agent_r540_typed_exact_release_events exact ON exact.id=inventory.event_id
GROUP BY root.trace_number,root.project_id,root.run_id,root.goal_id,root.start_tick;

CREATE VIEW agent_r540_exact_release_trace_certificates AS
SELECT certificate.*
FROM agent_r540_release_certificates certificate
JOIN agent_r540_release_inventory_state actual ON actual.trace_number=certificate.trace_number
LEFT JOIN agent_r540_release_certificates previous
 ON previous.trace_number=certificate.trace_number AND previous.version=certificate.version-1
WHERE certificate.version=(SELECT max(candidate.version) FROM agent_r540_release_certificates candidate
  WHERE candidate.trace_number=certificate.trace_number)
 AND certificate.last_ordinal=actual.last_ordinal
 AND actual.exact_event_count=actual.last_ordinal
 AND certificate.work_attempt_inventory=actual.work_attempt_inventory
 AND certificate.work_outcome_inventory=actual.work_outcome_inventory
 AND certificate.blocker_inventory=actual.blocker_inventory
 AND certificate.causal_inventory=actual.causal_inventory
 AND certificate.tool_inventory=actual.tool_inventory
 AND certificate.evidence_inventory=actual.evidence_inventory
 AND certificate.disclosure_inventory=actual.disclosure_inventory
 AND certificate.model_inventory=actual.model_inventory
 AND certificate.interrogation_inventory=actual.interrogation_inventory
 AND certificate.inventory_commitment=digest(convert_to(actual.all_inventory::text,'UTF8'),'sha256')
 AND jsonb_array_length(actual.work_attempt_inventory)>0
 AND ((certificate.version=1 AND certificate.previous_certificate_hash IS NULL AND previous.id IS NULL)
   OR (certificate.version>1 AND previous.certificate_hash=certificate.previous_certificate_hash
     AND previous.last_ordinal<=certificate.last_ordinal AND previous.end_tick<=certificate.end_tick
     AND (previous.last_ordinal<certificate.last_ordinal OR previous.end_tick<certificate.end_tick)))
 AND certificate.certificate_hash=digest(convert_to(concat_ws(E'\n','sprout-r540-release-certificate-v1',
   certificate.trace_number::text,certificate.project_id::text,certificate.run_id::text,
   certificate.goal_id::text,certificate.version::text,certificate.end_tick::text,
   certificate.last_ordinal::text,encode(certificate.inventory_commitment,'hex'),
   COALESCE(encode(certificate.previous_certificate_hash,'hex'),'')),'UTF8'),'sha256')
 AND (SELECT bool_and((gate.gate_mode='enabled' AND jsonb_array_length(gate.records)>0)
                  OR (gate.gate_mode='disabled_fail_closed' AND jsonb_array_length(gate.records)=0))
      FROM (VALUES
       (certificate.outcome_gate_mode,certificate.work_outcome_inventory),
       (certificate.blocker_gate_mode,certificate.blocker_inventory),
       (certificate.causal_gate_mode,certificate.causal_inventory),
       (certificate.tool_gate_mode,certificate.tool_inventory),
       (certificate.evidence_gate_mode,certificate.evidence_inventory),
       (certificate.disclosure_gate_mode,certificate.disclosure_inventory),
       (certificate.model_gate_mode,certificate.model_inventory),
       (certificate.interrogation_gate_mode,certificate.interrogation_inventory)
      ) gate(gate_mode,records));

CREATE VIEW agent_r541_release_trace_surface_gates AS
SELECT run.project_id,run.id AS run_id,root.trace_number,
 CASE WHEN exact.id IS NULL THEN 'disabled_fail_closed' ELSE exact.outcome_gate_mode END AS outcome_mode,
 CASE WHEN exact.id IS NULL THEN '[]'::jsonb ELSE exact.work_outcome_inventory END AS outcome_records,
 CASE WHEN exact.id IS NULL THEN 'disabled_fail_closed' ELSE exact.blocker_gate_mode END AS blocker_mode,
 CASE WHEN exact.id IS NULL THEN '[]'::jsonb ELSE exact.blocker_inventory END AS blocker_records,
 CASE WHEN exact.id IS NULL THEN 'disabled_fail_closed' ELSE exact.causal_gate_mode END AS causal_mode,
 CASE WHEN exact.id IS NULL THEN '[]'::jsonb ELSE exact.causal_inventory END AS causal_records,
 CASE WHEN exact.id IS NULL THEN 'disabled_fail_closed' ELSE exact.tool_gate_mode END AS tool_mode,
 CASE WHEN exact.id IS NULL THEN '[]'::jsonb ELSE exact.tool_inventory END AS tool_records,
 CASE WHEN exact.id IS NULL THEN 'disabled_fail_closed' ELSE exact.evidence_gate_mode END AS evidence_mode,
 CASE WHEN exact.id IS NULL THEN '[]'::jsonb ELSE exact.evidence_inventory END AS evidence_records,
 CASE WHEN exact.id IS NULL THEN 'disabled_fail_closed' ELSE exact.disclosure_gate_mode END AS disclosure_mode,
 CASE WHEN exact.id IS NULL THEN '[]'::jsonb ELSE exact.disclosure_inventory END AS disclosure_records,
 CASE WHEN exact.id IS NULL THEN 'disabled_fail_closed' ELSE exact.model_gate_mode END AS model_mode,
 CASE WHEN exact.id IS NULL THEN '[]'::jsonb ELSE exact.model_inventory END AS model_records,
 CASE WHEN exact.id IS NULL THEN 'disabled_fail_closed' ELSE exact.interrogation_gate_mode END AS interrogation_mode,
 CASE WHEN exact.id IS NULL THEN '[]'::jsonb ELSE exact.interrogation_inventory END AS interrogation_records
FROM agent_collaborative_runs run
LEFT JOIN agent_r541_release_roots root ON root.project_id=run.project_id AND root.run_id=run.id
LEFT JOIN agent_r540_exact_release_trace_certificates exact ON exact.trace_number=root.trace_number;

-- Retention removes only opaque payload bytes.  The logical Comment descriptor,
-- event, inventory and hash chain remain immutable; served payload disappears.
CREATE FUNCTION sprout_private.purge_native_comments_with_resource()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
BEGIN
  IF NOT sprout_private.retention_purge_row_allowed(to_jsonb(OLD)) THEN
    RETURN OLD;
  END IF;
  PERFORM set_config('app.agent_comment_retention','authorized',true);
  UPDATE public.native_comments SET encrypted_payload=NULL,payload_purged_at=clock_timestamp()
    WHERE project_id=OLD.project_id AND target_resource_node_id=OLD.id
      AND encrypted_payload IS NOT NULL;
  RETURN OLD;
END
$$;

CREATE TRIGGER resource_nodes_purge_native_comment_payloads
BEFORE DELETE ON resource_nodes
FOR EACH ROW EXECUTE FUNCTION sprout_private.purge_native_comments_with_resource();

ALTER TABLE agent_coordination_policy_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_coordination_policy_versions FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_release_roots ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_release_roots FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_run_semantic_tick_cursors ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_run_semantic_tick_cursors FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_run_semantic_tick_allocations ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_run_semantic_tick_allocations FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_attempt_clock_bindings ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_attempt_clock_bindings FORCE ROW LEVEL SECURITY;
ALTER TABLE native_comments ENABLE ROW LEVEL SECURITY;
ALTER TABLE native_comments FORCE ROW LEVEL SECURITY;
ALTER TABLE native_comment_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE native_comment_events FORCE ROW LEVEL SECURITY;
ALTER TABLE native_comment_notifications ENABLE ROW LEVEL SECURITY;
ALTER TABLE native_comment_notifications FORCE ROW LEVEL SECURITY;
ALTER TABLE native_comment_responses ENABLE ROW LEVEL SECURITY;
ALTER TABLE native_comment_responses FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_native_comment_security_effects ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_native_comment_security_effects FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_comment_records ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_comment_records FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_comment_inventory ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_comment_inventory FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_comment_certificates ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_comment_certificates FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_release_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_release_events FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_release_inventory ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_release_inventory FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_release_certificates ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_release_certificates FORCE ROW LEVEL SECURITY;

CREATE POLICY agent_coordination_policy_read ON agent_coordination_policy_versions FOR SELECT USING (true);
CREATE POLICY agent_release_root_read ON agent_r541_release_roots FOR SELECT
USING (sprout_private.agent_run_access(project_id,run_id));
CREATE POLICY agent_run_semantic_tick_cursor_read ON agent_run_semantic_tick_cursors FOR SELECT
USING (sprout_private.agent_run_access(project_id,run_id));
CREATE POLICY agent_run_semantic_tick_allocation_read ON agent_run_semantic_tick_allocations FOR SELECT
USING (sprout_private.agent_run_access(project_id,run_id));
CREATE POLICY agent_tool_attempt_clock_binding_read ON agent_tool_attempt_clock_bindings FOR SELECT
USING (sprout_private.agent_run_access(project_id,run_id));
CREATE POLICY native_comment_read ON native_comments FOR SELECT USING (
  sprout_private.comment_permission_allowed(project_id,target_resource_node_id,
    sprout_private.current_identity_id(),'read_comment'));
CREATE POLICY native_comment_event_read ON native_comment_events FOR SELECT USING (
  EXISTS (SELECT 1 FROM native_comments comment WHERE comment.id=comment_id
    AND comment.project_id=project_id
    AND sprout_private.comment_permission_allowed(comment.project_id,
      comment.target_resource_node_id,sprout_private.current_identity_id(),'read_comment')));
CREATE POLICY native_comment_notification_read ON native_comment_notifications FOR SELECT USING (
  recipient_identity_id=sprout_private.current_identity_id()
  OR sprout_private.agent_run_access(project_id,run_id));
CREATE POLICY native_comment_response_read ON native_comment_responses FOR SELECT USING (
  sprout_private.agent_run_access(project_id,run_id));
CREATE POLICY agent_native_comment_effect_read ON agent_native_comment_security_effects FOR SELECT USING (
  sprout_private.agent_run_access(project_id,run_id));
CREATE POLICY agent_comment_record_read ON agent_r541_comment_records FOR SELECT
USING (sprout_private.agent_run_access(project_id,run_id));
CREATE POLICY agent_comment_inventory_read ON agent_r541_comment_inventory FOR SELECT USING (
  EXISTS (SELECT 1 FROM agent_r541_release_roots root
    WHERE root.trace_number=agent_r541_comment_inventory.trace_number
      AND sprout_private.agent_run_access(root.project_id,root.run_id)));
CREATE POLICY agent_comment_certificate_read ON agent_r541_comment_certificates FOR SELECT
USING (sprout_private.agent_run_access(project_id,run_id));
CREATE POLICY agent_r540_release_event_read ON agent_r540_release_events FOR SELECT
USING (sprout_private.agent_run_access(project_id,run_id));
CREATE POLICY agent_r540_release_inventory_read ON agent_r540_release_inventory FOR SELECT USING (
  EXISTS (SELECT 1 FROM agent_r541_release_roots root
    WHERE root.trace_number=agent_r540_release_inventory.trace_number
      AND sprout_private.agent_run_access(root.project_id,root.run_id)));
CREATE POLICY agent_r540_release_certificate_read ON agent_r540_release_certificates FOR SELECT
USING (sprout_private.agent_run_access(project_id,run_id));

REVOKE ALL ON TABLE agent_coordination_policy_versions FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_release_roots FROM PUBLIC;
REVOKE ALL ON TABLE agent_run_semantic_tick_cursors FROM PUBLIC;
REVOKE ALL ON TABLE agent_run_semantic_tick_allocations FROM PUBLIC;
REVOKE ALL ON TABLE agent_tool_attempt_clock_bindings FROM PUBLIC;
REVOKE ALL ON TABLE native_comments FROM PUBLIC;
REVOKE ALL ON TABLE native_comment_events FROM PUBLIC;
REVOKE ALL ON TABLE native_comment_notifications FROM PUBLIC;
REVOKE ALL ON TABLE native_comment_responses FROM PUBLIC;
REVOKE ALL ON TABLE agent_native_comment_security_effects FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_comment_records FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_comment_inventory FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_comment_certificates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_release_events FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_release_inventory FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_release_certificates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_comment_inventory_state FROM PUBLIC;
REVOKE ALL ON TABLE agent_run_exact_semantic_timelines FROM PUBLIC;
REVOKE ALL ON TABLE agent_native_comment_semantic_states FROM PUBLIC;
REVOKE ALL ON TABLE agent_native_comment_run_semantic_states FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_typed_exact_comment_records FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_comment_certificates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_comment_surface_gates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_comment_surface_records FROM PUBLIC;
REVOKE ALL ON TABLE native_comment_readable FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_exact_release_events FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_typed_exact_release_events FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_release_inventory_state FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_exact_release_trace_certificates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_release_trace_surface_gates FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_formal_release_history_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.try_parse_encrypted_payload(bytea) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.comment_permission_allowed(uuid,uuid,uuid,text) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.initialize_agent_formal_release(uuid,uuid,uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.allocate_agent_run_semantic_tick(uuid,uuid,uuid,text,bytea,bigint,uuid,integer) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_and_stage_agent_tool_clock_v0035() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.restore_agent_tool_semantic_clock_v0035() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.append_agent_comment_certificate(bigint,bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.append_agent_r540_release_certificate(bigint,bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.append_agent_r540_release_event(bigint,text,bigint,text,uuid,jsonb) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_r540_transition_event() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_r540_blocker_resolution() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_r540_causal_link() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_r540_evidence() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_r540_tool_cluster_event() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_r540_model_invocation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_r540_completed_model_invocation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_r540_interrogation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_r540_comment_disclosure() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_r540_applied_disclosure() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.post_native_comment(uuid,uuid,uuid,uuid,bytea,integer,uuid,uuid,uuid,uuid,integer,bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.purge_native_comments_with_resource() FROM PUBLIC;

-- R5.41 formal-release composition.  A persisted child is only an immutable
-- issuance descriptor.  `agent_r541_exact_formal_release_child_certificates`
-- independently reconstructs the complete authoritative source snapshot; a
-- hash or a caller-provided boolean can never promote a child.
CREATE TABLE agent_r541_formal_release_child_certificates (
    id uuid PRIMARY KEY,
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    start_tick bigint NOT NULL CHECK (start_tick >= 0),
    release_version integer NOT NULL CHECK (release_version > 0),
    root_field text NOT NULL CHECK (root_field IN (
      'run_goal_exact','trace_start_exact','governed_run_exact','secure_kernel',
      'governance_kernel','concrete_trace','trace_feature_gates',
      'compiler_action_exact','security_policies_exact','governance_operational',
      'local_revision_trace_bound','creation_trace_bound',
      'responsibility_trace_bound','global_trace_bound','proxy_trace_bound',
      'cross_owner_trace_bound','comments','proxy','global_inventory_exact',
      'global','cross_owner','interrogation','model','task_operational',
      'task_intent_trace_bound','task_provenance_trace_bound',
      'operational_history','operational_closure'
    )),
    source_relation text NOT NULL CHECK (source_relation <> ''),
    formal_record jsonb NOT NULL CHECK (jsonb_typeof(formal_record)='object'),
    source_hash bytea NOT NULL CHECK (octet_length(source_hash)=32),
    certificate_hash bytea NOT NULL CHECK (octet_length(certificate_hash)=32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number,project_id,run_id,goal_id)
      REFERENCES agent_r541_release_roots(trace_number,project_id,run_id,goal_id)
      ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (trace_number,release_version,root_field),
    UNIQUE (trace_number,release_version,certificate_hash)
);

CREATE TABLE agent_r541_formal_release_inventory (
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    project_id uuid NOT NULL,
    release_version integer NOT NULL CHECK (release_version > 0),
    ordinal integer NOT NULL CHECK (ordinal BETWEEN 1 AND 28),
    root_field text NOT NULL,
    child_certificate_id uuid NOT NULL,
    child_certificate_hash bytea NOT NULL CHECK (octet_length(child_certificate_hash)=32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (trace_number,release_version,ordinal),
    FOREIGN KEY (child_certificate_id)
      REFERENCES agent_r541_formal_release_child_certificates(id)
      ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (trace_number,release_version,root_field),
    UNIQUE (trace_number,release_version,child_certificate_id)
);

CREATE TABLE agent_r541_formal_release_certificates (
    id uuid PRIMARY KEY,
    trace_number bigint NOT NULL CHECK (trace_number > 0),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    start_tick bigint NOT NULL CHECK (start_tick >= 0),
    version integer NOT NULL CHECK (version > 0),
    child_inventory jsonb NOT NULL CHECK (
      jsonb_typeof(child_inventory)='array' AND jsonb_array_length(child_inventory)=28),
    child_inventory_commitment bytea NOT NULL CHECK (octet_length(child_inventory_commitment)=32),
    previous_certificate_hash bytea CHECK (
      previous_certificate_hash IS NULL OR octet_length(previous_certificate_hash)=32),
    certificate_hash bytea NOT NULL CHECK (octet_length(certificate_hash)=32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number,project_id,run_id,goal_id)
      REFERENCES agent_r541_release_roots(trace_number,project_id,run_id,goal_id)
      ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (trace_number,version),
    UNIQUE (trace_number,certificate_hash)
);

-- All data below is rebuilt from immutable/canonical product sources.  It is
-- intentionally restricted to local-contract 0035-native runs: an unsupported
-- global or ambiguous provenance cannot acquire a formal root accidentally.
CREATE FUNCTION sprout_private.jsonb_array_is_prefix(before_list jsonb, after_list jsonb)
RETURNS boolean LANGUAGE sql IMMUTABLE STRICT
SET search_path=pg_catalog AS $$
  SELECT jsonb_typeof(before_list)='array'
     AND jsonb_typeof(after_list)='array'
     AND jsonb_array_length(before_list)<=jsonb_array_length(after_list)
     AND NOT EXISTS (
       SELECT 1 FROM jsonb_array_elements(before_list) WITH ORDINALITY prior(value,ordinal)
       WHERE prior.value IS DISTINCT FROM (
         SELECT current.value FROM jsonb_array_elements(after_list)
           WITH ORDINALITY current(value,ordinal)
         WHERE current.ordinal=prior.ordinal))
$$;

CREATE FUNCTION sprout_private.jsonb_array_suffix(after_list jsonb, prefix_length integer)
RETURNS jsonb LANGUAGE sql IMMUTABLE STRICT
SET search_path=pg_catalog AS $$
  SELECT COALESCE(jsonb_agg(item.value ORDER BY item.ordinal),'[]'::jsonb)
  FROM jsonb_array_elements(after_list) WITH ORDINALITY item(value,ordinal)
  WHERE item.ordinal>prefix_length
$$;

CREATE VIEW agent_r541_secure_kernel_nested_sources AS
WITH base AS (
 SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,root.start_tick,
   run.state,run.contract,run.created_by_identity_id,
   trace.id AS trace_certificate_id,trace.work_attempt_inventory,trace.work_outcome_inventory,
   trace.evidence_inventory,trace.tool_inventory,trace.disclosure_inventory,trace.model_inventory
 FROM agent_r541_release_roots root
 JOIN agent_collaborative_runs run ON run.project_id=root.project_id AND run.id=root.run_id
 JOIN agent_r540_exact_release_trace_certificates trace ON trace.trace_number=root.trace_number
 WHERE run.run_status='completed' AND run.goal_status='completed'
), obligations(nested_field) AS (VALUES
 ('completion.progress'),('completion.evidence_discharge'),
 ('safety.sponsor_human'),('safety.run_authority_bounded_at_start'),
 ('safety.run_tool_authority_bounded_at_start'),('safety.work_origin_sound'),
 ('safety.effect_authority'),('safety.human_task_control_isolation'),
 ('safety.effect_run_scope'),('safety.model_context_run_scope'),
 ('safety.semantic_work_enabledness'),('safety.security_policy_allowance'),
 ('safety.core_action_contract_allowance'),('safety.canonical_resource_body'),
 ('safety.contextual_chat_and_disclosure'),('safety.authority_and_tool_attenuation')
)
SELECT base.trace_number,obligations.nested_field,
 jsonb_build_object('trace_number',base.trace_number,'run',base.run_id,'goal',base.goal_id,
   'start_tick',base.start_tick,'nested_field',obligations.nested_field,
   'authoritative_state',CASE obligations.nested_field
    WHEN 'completion.progress' THEN jsonb_build_object('state',base.state,
      'work_attempts',base.work_attempt_inventory,'work_outcomes',base.work_outcome_inventory,
      'semantic_allocator',to_regprocedure('sprout_private.allocate_agent_run_semantic_tick(uuid,uuid,uuid,text,bytea,bigint,uuid,integer)'))
    WHEN 'completion.evidence_discharge' THEN jsonb_build_object('state',base.state,
      'evidence',base.evidence_inventory)
    WHEN 'safety.sponsor_human' THEN jsonb_build_object('sponsor',base.created_by_identity_id)
    WHEN 'safety.run_authority_bounded_at_start' THEN jsonb_build_object('snapshot',
      (SELECT to_jsonb(snapshot)-'recorded_at' FROM agent_run_resource_authority_snapshots snapshot
        WHERE snapshot.project_id=base.project_id AND snapshot.run_id=base.run_id))
    WHEN 'safety.run_tool_authority_bounded_at_start' THEN jsonb_build_object(
      'snapshots',(SELECT jsonb_agg(to_jsonb(snapshot)-'recorded_at') FROM agent_run_tool_security_snapshots snapshot
        WHERE snapshot.project_id=base.project_id AND snapshot.run_id=base.run_id))
    WHEN 'safety.work_origin_sound' THEN jsonb_build_object('calls',base.tool_inventory)
    WHEN 'safety.effect_authority' THEN jsonb_build_object('tools',base.tool_inventory,
      'disclosures',base.disclosure_inventory)
    WHEN 'safety.human_task_control_isolation' THEN jsonb_build_object('task_effects',
      (SELECT COALESCE(jsonb_agg(to_jsonb(effect)-ARRAY['applied_at','recorded_at']),'[]'::jsonb)
       FROM agent_run_task_effects effect WHERE effect.project_id=base.project_id AND effect.run_id=base.run_id))
    WHEN 'safety.effect_run_scope' THEN jsonb_build_object('scope',
      (SELECT scope_resource_node_id FROM agent_collaborative_runs WHERE project_id=base.project_id AND id=base.run_id))
    WHEN 'safety.model_context_run_scope' THEN jsonb_build_object('models',base.model_inventory)
    WHEN 'safety.semantic_work_enabledness' THEN jsonb_build_object('work_attempts',base.work_attempt_inventory)
    WHEN 'safety.security_policy_allowance' THEN jsonb_build_object('contract',base.contract)
    WHEN 'safety.core_action_contract_allowance' THEN jsonb_build_object('contract',base.contract)
    WHEN 'safety.canonical_resource_body' THEN jsonb_build_object('disclosures',base.disclosure_inventory)
    WHEN 'safety.contextual_chat_and_disclosure' THEN jsonb_build_object('disclosures',base.disclosure_inventory)
    WHEN 'safety.authority_and_tool_attenuation' THEN jsonb_build_object('tools',base.tool_inventory)
   END) AS nested_record
FROM base CROSS JOIN obligations
WHERE CASE obligations.nested_field
 WHEN 'completion.progress' THEN jsonb_array_length(base.work_attempt_inventory)>0
   AND jsonb_array_length(base.work_attempt_inventory)=jsonb_array_length(base.work_outcome_inventory)
   AND to_regprocedure('sprout_private.allocate_agent_run_semantic_tick(uuid,uuid,uuid,text,bytea,bigint,uuid,integer)') IS NOT NULL
   AND NOT EXISTS (SELECT 1 FROM jsonb_each(base.state->'work_items') work
     WHERE work.value->>'status' NOT IN ('succeeded','failed','cancelled'))
 WHEN 'completion.evidence_discharge' THEN NOT EXISTS (SELECT 1 FROM jsonb_each(base.state->'obligations') obligation
     WHERE obligation.value->>'status'<>'discharged')
 WHEN 'safety.sponsor_human' THEN EXISTS (SELECT 1 FROM identities identity
     WHERE identity.id=base.created_by_identity_id AND identity.status='active'
       AND identity.principal_kind IN ('administrator','user'))
 WHEN 'safety.run_authority_bounded_at_start' THEN EXISTS (
   SELECT 1 FROM agent_run_resource_authority_snapshots snapshot
   WHERE snapshot.project_id=base.project_id AND snapshot.run_id=base.run_id
     AND snapshot.sponsor_identity_id=base.created_by_identity_id
     AND snapshot.resource_authority=snapshot.authority_statement::jsonb
     AND snapshot.authority_hash=digest(convert_to(snapshot.authority_statement,'UTF8'),'sha256'))
 WHEN 'safety.run_tool_authority_bounded_at_start' THEN NOT EXISTS (SELECT 1 FROM agent_tool_calls call
     LEFT JOIN agent_run_tool_security_snapshots snapshot ON snapshot.project_id=call.project_id
       AND snapshot.run_id=call.run_id AND snapshot.run_tool_ceiling_hash=call.run_tool_ceiling_hash
     WHERE call.project_id=base.project_id AND call.run_id=base.run_id AND snapshot.run_id IS NULL)
 WHEN 'safety.work_origin_sound' THEN NOT EXISTS (SELECT 1 FROM agent_tool_calls call
     WHERE call.project_id=base.project_id AND call.run_id=base.run_id
       AND call.work_authority_origin NOT IN ('run_sponsor','inherited_work'))
   AND NOT EXISTS (SELECT 1 FROM jsonb_each(base.state->'work_items') work
     WHERE work.value->>'id' IS DISTINCT FROM work.key
       OR work.value->>'run' IS DISTINCT FROM base.run_id::text
       OR work.value->>'goal' IS DISTINCT FROM base.goal_id::text
       OR work.value->>'owner' IS NULL OR work.value->>'work_spec_id' IS NULL
       OR NOT EXISTS (SELECT 1 FROM jsonb_array_elements(base.contract->'work_specs') spec
         WHERE spec->>'id'=work.value->>'work_spec_id'
           AND spec->>'owner'=work.value->>'owner')
       OR (work.value->>'parent' IS NOT NULL AND NOT (base.state->'work_items' ? (work.value->>'parent'))))
 WHEN 'safety.effect_authority' THEN NOT EXISTS (SELECT 1 FROM agent_r540_exact_release_events event
     LEFT JOIN agent_r540_typed_exact_release_events typed ON typed.id=event.id
     WHERE event.trace_number=base.trace_number
       AND event.event_kind IN ('tool_event','disclosure') AND typed.id IS NULL)
 WHEN 'safety.human_task_control_isolation' THEN NOT EXISTS (SELECT 1 FROM agent_run_task_effects effect
     LEFT JOIN agent_run_claim_leases claim ON claim.project_id=effect.project_id AND claim.id=effect.claim_id
       AND claim.claimant_identity_id=effect.actor_identity_id
     WHERE effect.project_id=base.project_id AND effect.run_id=base.run_id AND claim.id IS NULL)
 WHEN 'safety.effect_run_scope' THEN NOT EXISTS (SELECT 1 FROM agent_run_task_effects effect
     JOIN agent_collaborative_runs run ON run.project_id=effect.project_id AND run.id=effect.run_id
     LEFT JOIN resource_closure closure ON closure.project_id=run.project_id
       AND closure.ancestor_id=run.scope_resource_node_id AND closure.descendant_id=effect.task_resource_node_id
     WHERE effect.project_id=base.project_id AND effect.run_id=base.run_id AND closure.descendant_id IS NULL)
 WHEN 'safety.model_context_run_scope' THEN NOT EXISTS (SELECT 1 FROM agent_model_invocation_projections projection
     WHERE projection.project_id=base.project_id AND projection.run_id=base.run_id
       AND NOT EXISTS (SELECT 1 FROM agent_r540_typed_exact_release_events typed
         WHERE typed.trace_number=base.trace_number AND typed.source_record_id=projection.id))
 WHEN 'safety.semantic_work_enabledness' THEN jsonb_array_length(base.work_attempt_inventory)>0
 WHEN 'safety.security_policy_allowance' THEN jsonb_typeof(base.contract->'work_specs')='array'
 WHEN 'safety.core_action_contract_allowance' THEN jsonb_typeof(base.contract->'work_specs')='array'
 WHEN 'safety.canonical_resource_body' THEN NOT EXISTS (SELECT 1 FROM agent_effect_proposals effect
     JOIN agent_invocations invocation ON invocation.project_id=effect.project_id AND invocation.id=effect.invocation_id
     WHERE effect.project_id=base.project_id AND invocation.run_id=base.run_id AND effect.status='applied'
       AND effect.encrypted_materialization IS NULL)
 WHEN 'safety.contextual_chat_and_disclosure' THEN NOT EXISTS (SELECT 1 FROM agent_r540_exact_release_events event
     LEFT JOIN agent_r540_typed_exact_release_events typed ON typed.id=event.id
     WHERE event.trace_number=base.trace_number AND event.event_kind='disclosure' AND typed.id IS NULL)
 WHEN 'safety.authority_and_tool_attenuation' THEN NOT EXISTS (SELECT 1 FROM agent_tool_calls child
     LEFT JOIN agent_tool_calls parent ON parent.project_id=child.project_id AND parent.run_id=child.run_id
       AND parent.work_item_id=child.work_authority_parent_id
     WHERE child.project_id=base.project_id AND child.run_id=base.run_id
       AND child.work_authority_origin='inherited_work'
       AND (parent.id IS NULL OR NOT (child.work_tool_ceiling <@ parent.work_tool_ceiling)))
 ELSE false END;

-- R5.41 secure-kernel reconstruction.  These helpers deliberately expose
-- field-level semantic witnesses; the broad compatibility view above is not
-- consumed by the formal root.
CREATE FUNCTION sprout_private.r541_condition_holds(condition jsonb, facts jsonb)
RETURNS boolean LANGUAGE plpgsql IMMUTABLE STRICT
SET search_path=pg_catalog AS $$
DECLARE kind text;
BEGIN
 kind:=condition->>'kind';
 CASE kind
  WHEN 'always' THEN RETURN true;
  WHEN 'never' THEN RETURN false;
  WHEN 'task_done' THEN RETURN facts->'completed_tasks' ? (condition->>'task');
  WHEN 'obligation_done' THEN RETURN facts->'discharged_obligations' ? (condition->>'obligation');
  WHEN 'comment_by' THEN RETURN facts->'comment_authors' ? (condition->>'principal');
  WHEN 'administrator_approved' THEN RETURN EXISTS (
    SELECT 1 FROM jsonb_array_elements(facts->'administrator_approvals') approval
    WHERE approval->>'administrator'=condition->>'administrator'
      AND approval->>'review_work_spec_ordinal'=condition->>'review_work_spec_id');
  WHEN 'all' THEN RETURN sprout_private.r541_condition_holds(condition->'left',facts)
    AND sprout_private.r541_condition_holds(condition->'right',facts);
  WHEN 'any' THEN RETURN sprout_private.r541_condition_holds(condition->'left',facts)
    OR sprout_private.r541_condition_holds(condition->'right',facts);
  WHEN 'neg' THEN RETURN NOT sprout_private.r541_condition_holds(condition->'condition',facts);
  ELSE RETURN false;
 END CASE;
END $$;
REVOKE ALL ON FUNCTION sprout_private.r541_condition_holds(jsonb,jsonb) FROM PUBLIC;

-- Exact WorkAuthorityOrigin for 0035-native runs.  A Task -> Work edge is the
-- explicit possible/unsupported human-delegation signal and excludes that
-- work and all descendants.  Roots must be born in initialization; children
-- must be born in a contract continuation transition and inherit the sponsor.
CREATE VIEW agent_r541_exact_work_authority_origins AS
WITH RECURSIVE births AS (
 SELECT DISTINCT ON (root.trace_number,work.key)
   root.trace_number,root.project_id,root.run_id,root.goal_id,
   work.key::uuid AS work_item_id,work.value AS work_snapshot,
   transition.state_version,transition.transition_kind,transition.actor_identity_id
 FROM agent_r541_release_roots root
 JOIN agent_run_transitions transition ON transition.project_id=root.project_id
  AND transition.run_id=root.run_id AND transition.semantic_tick>=root.start_tick
 CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
 ORDER BY root.trace_number,work.key,transition.state_version
), sponsor AS (
 SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,
  snapshot.sponsor_identity_id
 FROM agent_r541_release_roots root
 JOIN agent_run_resource_authority_snapshots snapshot
  ON snapshot.project_id=root.project_id AND snapshot.run_id=root.run_id
), resolved AS (
 SELECT birth.trace_number,birth.project_id,birth.run_id,birth.goal_id,birth.work_item_id,
  NULL::uuid AS authority_parent_id,sponsor.sponsor_identity_id AS authority_principal_id,
  'run_sponsor'::text AS authority_origin,1 AS authority_depth,
  birth.state_version AS birth_state_version
 FROM births birth JOIN sponsor USING(trace_number,project_id,run_id,goal_id)
 WHERE birth.state_version=1 AND birth.transition_kind='initialized'
  AND birth.actor_identity_id=sponsor.sponsor_identity_id
  AND birth.work_snapshot->>'parent' IS NULL
  AND birth.work_snapshot->>'source_comment' IS NULL
  AND birth.work_snapshot->>'id'=birth.work_item_id::text
  AND birth.work_snapshot->>'run'=birth.run_id::text
  AND birth.work_snapshot->>'goal'=birth.goal_id::text
  AND NOT EXISTS (SELECT 1 FROM agent_run_causal_links link
    WHERE link.project_id=birth.project_id AND link.run_id=birth.run_id
      AND link.predecessor->>'kind'='task' AND link.successor->>'kind'='work'
      AND link.successor->>'work'=birth.work_item_id::text)
 UNION ALL
 SELECT child.trace_number,child.project_id,child.run_id,child.goal_id,child.work_item_id,
  parent.work_item_id,parent.authority_principal_id,'inherited_work',parent.authority_depth+1,
  child.state_version
 FROM births child JOIN resolved parent ON parent.trace_number=child.trace_number
  AND parent.work_item_id=NULLIF(child.work_snapshot->>'parent','')::uuid
 WHERE child.state_version>parent.birth_state_version
  AND child.transition_kind IN ('frontier_refreshed','work_succeeded','work_failed')
  AND child.work_snapshot->>'source_comment' IS NULL
  AND child.work_snapshot->>'id'=child.work_item_id::text
  AND child.work_snapshot->>'run'=child.run_id::text
  AND child.work_snapshot->>'goal'=child.goal_id::text
  AND NOT EXISTS (SELECT 1 FROM agent_run_causal_links link
    WHERE link.project_id=child.project_id AND link.run_id=child.run_id
      AND link.predecessor->>'kind'='task' AND link.successor->>'kind'='work'
      AND link.successor->>'work'=child.work_item_id::text)
)
SELECT resolved.* FROM resolved
WHERE 1=(SELECT count(*) FROM births birth WHERE birth.trace_number=resolved.trace_number
  AND birth.work_item_id=resolved.work_item_id)
  AND 1=(SELECT count(*) FROM resolved duplicate WHERE duplicate.trace_number=resolved.trace_number
  AND duplicate.work_item_id=resolved.work_item_id);

-- One normalized, independently reconstructed SecurityEffect for every
-- supported concrete AgentMove family.  A source row disappears unless its
-- exact WorkAttempt, WorkSpec, policy and source-specific payload all agree.
CREATE VIEW agent_r541_exact_agent_security_effects AS
WITH raw AS (
 SELECT tool.trace_number,tool.project_id,tool.run_id,tool.goal_id,tool.requested_tick AS semantic_tick,
  tool.tool_event_id AS source_record_id,'tool'::text AS source_kind,
  tool.work_item_id,tool.claim_id,tool.attempt,tool.owner_identity_id AS actor_identity_id,
  CASE WHEN tool.attempt=1 THEN 'invoke_tool' ELSE 'retry_tool' END AS action_class,
  catalog.required_effects AS footprint,
  jsonb_build_array(jsonb_build_object('call_id',tool.call_id,'owner',tool.owner_identity_id,
    'tool',tool.tool_name,'version',tool.tool_version,
    'input_commitment',encode(tool.canonical_input_commitment,'hex'))) AS tool_invocations,
  '[]'::jsonb AS context_sources,NULL::jsonb AS disclosure
 FROM agent_r540_exact_tool_trace_records tool
 JOIN agent_external_tool_catalog catalog ON catalog.tool_name=tool.tool_name
  AND catalog.version=tool.tool_version
 WHERE tool.phase='pending'
 UNION ALL
 SELECT root.trace_number,effect.project_id,effect.run_id,effect.goal_id,effect.observed_tick,
  effect.id,'comment',effect.work_item_id,effect.claim_id,effect.attempt,effect.actor_identity_id,
  'post_comment',jsonb_build_array(jsonb_build_object('resource_id',effect.target_resource_node_id,
    'operation','post_comment')),'[]'::jsonb,effect.context_sources,
  jsonb_build_object('kind','comment_on','target',effect.target_resource_node_id,
    'comment_id',effect.comment_id)
 FROM agent_native_comment_security_effects effect
 JOIN agent_r541_release_roots root ON root.project_id=effect.project_id AND root.run_id=effect.run_id
 JOIN agent_r540_typed_exact_release_events typed ON typed.trace_number=root.trace_number
  AND typed.source_relation='agent_native_comment_security_effects'
  AND typed.source_record_id=effect.id
 UNION ALL
 SELECT root.trace_number,effect.project_id,effect.run_id,root.goal_id,allocation.semantic_tick,
  effect.id,'task',effect.work_item_id,effect.claim_id,effect.attempt,effect.actor_identity_id,
  'mark_assigned_done',jsonb_build_array(jsonb_build_object('resource_id',effect.task_resource_node_id,
    'operation','complete_assigned_task')),'[]'::jsonb,'[]'::jsonb,NULL::jsonb
 FROM agent_run_task_effects effect
 JOIN agent_r541_release_roots root ON root.project_id=effect.project_id AND root.run_id=effect.run_id
 JOIN agent_run_semantic_tick_allocations allocation ON allocation.project_id=effect.project_id
  AND allocation.run_id=effect.run_id AND allocation.trace_number=root.trace_number
  AND allocation.event_kind='task_completion_materialized' AND allocation.event_key=effect.id
 JOIN agent_r541_exact_task_operational_bindings binding ON binding.trace_number=root.trace_number
  AND binding.task_effect_id=effect.id
), exact_work AS (
 SELECT typed.trace_number,typed.formal_record->>'work' AS work_id,
  typed.formal_record->>'claim' AS claim_id,(typed.formal_record->>'attempt')::integer AS attempt,
  typed.formal_record->>'actor' AS actor,exact.event_snapshot->'work_snapshot' AS work_snapshot,
  exact.event_snapshot->'claim_snapshot' AS claim_snapshot,
  transition.fact_references AS work_fact_references,
  transition.state_snapshot AS work_semantic_state
 FROM agent_r540_typed_exact_release_events typed
 JOIN agent_r540_exact_release_events exact ON exact.id=typed.id
 LEFT JOIN agent_r540_work_attempt_events projected_work
   ON exact.source_relation='agent_r540_work_attempt_events'
  AND projected_work.id=exact.source_record_id
 JOIN agent_run_transitions transition ON transition.id=CASE
   WHEN exact.source_relation='agent_run_transitions' THEN exact.source_record_id
   ELSE projected_work.transition_id END
 WHERE typed.event_kind='work_attempt'
)
SELECT raw.*,work.work_snapshot,work.claim_snapshot,spec.value AS work_spec,
 work.work_fact_references,work.work_semantic_state,
 policy.value AS security_policy,origin.authority_origin,origin.authority_parent_id,
 origin.authority_principal_id,
 digest(convert_to(concat_ws(E'\n','sprout-r541-security-effect-v1',raw.trace_number::text,
   raw.source_kind,raw.source_record_id::text,raw.semantic_tick::text,raw.work_item_id::text,
   raw.claim_id::text,raw.attempt::text,raw.actor_identity_id::text,raw.action_class,
   raw.footprint::text,raw.tool_invocations::text,raw.context_sources::text,
   COALESCE(raw.disclosure::text,'')),'UTF8'),'sha256') AS effect_hash
FROM raw
JOIN exact_work work ON work.trace_number=raw.trace_number AND work.work_id=raw.work_item_id::text
 AND work.claim_id=raw.claim_id::text AND work.attempt=raw.attempt AND work.actor=raw.actor_identity_id::text
JOIN agent_collaborative_runs run ON run.project_id=raw.project_id AND run.id=raw.run_id
JOIN agent_compilation_certificates compiler ON compiler.project_id=run.project_id
 AND compiler.id=(SELECT local.compilation_certificate_id FROM agent_local_goal_contracts local
   WHERE local.project_id=run.project_id AND local.id=run.local_goal_id
     AND local.revision=run.local_goal_revision)
JOIN LATERAL jsonb_array_elements(run.contract->'work_specs') spec(value)
 ON (spec.value->>'id')::bigint=(work.work_snapshot->>'work_spec_id')::bigint
JOIN LATERAL jsonb_array_elements(compiler.canonical_output->'security_policies') policy(value)
 ON policy.value->>'work_spec_id'=spec.value->>'id'
JOIN agent_r541_exact_work_authority_origins origin ON origin.trace_number=raw.trace_number
 AND origin.work_item_id=raw.work_item_id
WHERE raw.semantic_tick>=(SELECT start_tick FROM agent_r541_release_roots root
  WHERE root.trace_number=raw.trace_number)
 AND work.work_snapshot->>'id'=raw.work_item_id::text
 AND work.work_snapshot->>'run'=raw.run_id::text
 AND work.work_snapshot->>'goal'=raw.goal_id::text
 AND work.work_snapshot->>'owner'=raw.actor_identity_id::text
 AND (work.work_snapshot->>'attempt')::integer=raw.attempt
 AND work.claim_snapshot->>'id'=raw.claim_id::text
 AND work.claim_snapshot->>'work'=raw.work_item_id::text
 AND work.claim_snapshot->>'claimant'=raw.actor_identity_id::text
 AND (work.claim_snapshot->>'attempt')::integer=raw.attempt;

CREATE FUNCTION sprout_private.r541_completion_criterion(
  state_snapshot jsonb, contract_snapshot jsonb, facts jsonb)
RETURNS boolean LANGUAGE sql IMMUTABLE STRICT
SET search_path=pg_catalog AS $$
 SELECT sprout_private.r541_condition_holds(contract_snapshot->'completion_condition',facts)
  AND NOT EXISTS (
   SELECT 1 FROM jsonb_array_elements(contract_snapshot->'obligations') spec
   WHERE sprout_private.r541_condition_holds(spec->'required_for_completion',facts)
    AND NOT EXISTS (SELECT 1 FROM jsonb_each(state_snapshot->'obligations') obligation
      WHERE obligation.key=spec->>'id' AND obligation.value->>'run'=state_snapshot->>'id'
       AND obligation.value->>'spec'=spec->>'id' AND obligation.value->>'owner'=spec->>'owner'
       AND obligation.value->>'status'='discharged'))
  AND NOT EXISTS (SELECT 1 FROM jsonb_each(state_snapshot->'work_items') work
   WHERE work.value->>'run'=state_snapshot->>'id'
    AND work.value->>'goal'=contract_snapshot->>'goal'
    AND work.value->>'status' NOT IN ('succeeded','failed','cancelled'))
  AND NOT EXISTS (SELECT 1 FROM jsonb_each(state_snapshot->'blockers') blocker
   WHERE blocker.value->>'run'=state_snapshot->>'id'
    AND blocker.value->>'goal'=contract_snapshot->>'goal'
    AND blocker.value->>'status'='waiting')
$$;
REVOKE ALL ON FUNCTION sprout_private.r541_completion_criterion(jsonb,jsonb,jsonb) FROM PUBLIC;

CREATE FUNCTION sprout_private.r541_progress_rank(state_snapshot jsonb, contract_snapshot jsonb)
RETURNS bigint LANGUAGE sql IMMUTABLE STRICT
SET search_path=pg_catalog AS $$
 SELECT
  1000000000::bigint*(SELECT count(*) FROM jsonb_each(state_snapshot->'obligations') obligation
    WHERE obligation.value->>'status'='active')
  +1000000::bigint*COALESCE((SELECT sum(
    GREATEST(0,(spec.value->>'max_attempts')::integer-(work.value->>'attempt')::integer)*1000
    +GREATEST(0,(spec.value->>'generation_rank')::integer)*10
    +CASE work.value->>'status' WHEN 'eligible' THEN 3 WHEN 'blocked' THEN 2
      WHEN 'claimed' THEN 1 ELSE 0 END)
    FROM jsonb_each(state_snapshot->'work_items') work
    JOIN LATERAL jsonb_array_elements(contract_snapshot->'work_specs') spec(value)
      ON spec.value->>'id'=work.value->>'work_spec_id'
    WHERE work.value->>'status' NOT IN ('succeeded','failed','cancelled')),0)
  +COALESCE((SELECT count(*) FROM jsonb_each(state_snapshot->'blockers') blocker
    WHERE blocker.value->>'status'='waiting'),0)
$$;
REVOKE ALL ON FUNCTION sprout_private.r541_progress_rank(jsonb,jsonb) FROM PUBLIC;

CREATE VIEW agent_r541_collaborative_kernel_field_sources AS
WITH base AS (
 SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,root.start_tick,run.contract
 FROM agent_r541_release_roots root JOIN agent_collaborative_runs run
  ON run.project_id=root.project_id AND run.id=root.run_id
), fields(field_name) AS (VALUES
 ('contractWellFormed'),('requiredObligationClosure'),('requiredActiveHasMinimalFrontier'),
 ('entryWorkClosure'),('allRelevantWorkCertified'),('workIdentity'),('canonicalWorkId'),
 ('slotIdentityStable'),('slotUnique'),('slotUniqueAcrossRun'),('workActivationSound'),
 ('eligibleWorkStatusSound'),('blockedWorkHasWaitingBlocker'),
 ('waitingBlockersExternallyControlled'),('allRelevantBlockersCertified'),
 ('blockerRuleStable'),('ownerIsParticipant'),('parentGenerationSound'),
 ('validClaimsExclusive'),('expiredClaimsInvalid'),('causalRankDecreases'),('causalLinksComplete'))
SELECT base.trace_number,fields.field_name,jsonb_build_object(
 'trace_number',base.trace_number,'field',fields.field_name,'run',base.run_id,'goal',base.goal_id,
 'start_tick',base.start_tick,'contract',base.contract) AS exact_record
FROM base CROSS JOIN fields
WHERE CASE fields.field_name
 WHEN 'contractWellFormed' THEN jsonb_typeof(base.contract->'obligations')='array'
  AND jsonb_typeof(base.contract->'work_specs')='array'
  AND jsonb_typeof(base.contract->'dependencies')='array'
  AND jsonb_typeof(base.contract->'waiting_rules')='array'
  AND base.contract->>'goal'=base.goal_id::text
  AND (SELECT count(DISTINCT spec->>'id') FROM jsonb_array_elements(base.contract->'obligations') spec)
      =jsonb_array_length(base.contract->'obligations')
  AND (SELECT count(DISTINCT spec->>'id') FROM jsonb_array_elements(base.contract->'work_specs') spec)
      =jsonb_array_length(base.contract->'work_specs')
 WHEN 'requiredObligationClosure' THEN NOT EXISTS (
  SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_array_elements(base.contract->'obligations') spec
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND transition.semantic_tick>=base.start_tick
   AND sprout_private.r541_condition_holds(spec->'activation',transition.fact_references)
   AND (transition.state_snapshot#>>ARRAY['obligations',spec->>'id','run'] IS DISTINCT FROM base.run_id::text
    OR transition.state_snapshot#>>ARRAY['obligations',spec->>'id','spec'] IS DISTINCT FROM spec->>'id'
    OR transition.state_snapshot#>>ARRAY['obligations',spec->>'id','owner'] IS DISTINCT FROM spec->>'owner'))
 WHEN 'requiredActiveHasMinimalFrontier' THEN NOT EXISTS (
  SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'obligations') obligation
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND transition.semantic_tick>=base.start_tick AND obligation.value->>'status'='active'
   AND NOT EXISTS (SELECT 1 FROM jsonb_each(transition.state_snapshot->'obligations') minimal
    WHERE minimal.value->>'status'='active' AND NOT EXISTS (
      SELECT 1 FROM jsonb_array_elements(base.contract->'dependencies') dependency
      WHERE dependency->>'obligation'=minimal.key
       AND transition.state_snapshot#>>ARRAY['obligations',dependency->>'prerequisite','status']
          IS DISTINCT FROM 'discharged')))
 WHEN 'entryWorkClosure' THEN NOT EXISTS (
  SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'obligations') obligation
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND obligation.value->>'status'='active'
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(base.contract->'work_specs') spec
    JOIN agent_run_work_slots slot ON slot.project_id=base.project_id AND slot.run_id=base.run_id
     AND slot.work_spec_ordinal=(spec->>'id')::bigint AND slot.slot=0
    WHERE spec->>'obligation'=obligation.key AND (spec->>'is_entry')::boolean
     AND transition.state_snapshot#>>ARRAY['work_items',slot.work_item_id::text,'serves']=obligation.key
     AND transition.state_snapshot#>>ARRAY['work_items',slot.work_item_id::text,'status']
       IN ('eligible','blocked','claimed')))
 WHEN 'allRelevantWorkCertified' THEN NOT EXISTS (
  SELECT 1 FROM agent_run_transitions transition CROSS JOIN LATERAL
   jsonb_each(transition.state_snapshot->'work_items') work
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND NOT EXISTS (SELECT 1 FROM agent_run_work_slots slot
    JOIN LATERAL jsonb_array_elements(base.contract->'work_specs') spec
      ON (spec->>'id')::bigint=slot.work_spec_ordinal
    WHERE slot.project_id=base.project_id AND slot.run_id=base.run_id
     AND slot.work_item_id=work.key::uuid AND slot.slot=(work.value->>'slot')::integer
     AND spec->>'obligation'=work.value->>'serves' AND spec->>'owner'=work.value->>'owner'
     AND spec->>'kind'=work.value->>'kind'
     AND (work.value->>'attempt')::integer<(spec->>'max_attempts')::integer))
 WHEN 'workIdentity' THEN NOT EXISTS (SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND work.value->>'id' IS DISTINCT FROM work.key)
 WHEN 'canonicalWorkId' THEN NOT EXISTS (SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND NOT EXISTS (SELECT 1 FROM agent_run_work_slots slot
    WHERE slot.project_id=base.project_id AND slot.run_id=base.run_id
     AND slot.work_item_id=work.key::uuid AND slot.work_spec_ordinal=(work.value->>'work_spec_id')::bigint
     AND slot.slot=(work.value->>'slot')::integer))
 WHEN 'slotIdentityStable' THEN NOT EXISTS (SELECT 1 FROM agent_run_transitions earlier
  JOIN agent_run_transitions later ON later.project_id=earlier.project_id AND later.run_id=earlier.run_id
   AND later.state_version>=earlier.state_version
  CROSS JOIN LATERAL jsonb_each(earlier.state_snapshot->'work_items') left_work
  JOIN LATERAL jsonb_each(later.state_snapshot->'work_items') right_work ON right_work.key=left_work.key
  WHERE earlier.project_id=base.project_id AND earlier.run_id=base.run_id
   AND (left_work.value->>'work_spec_id' IS DISTINCT FROM right_work.value->>'work_spec_id'
    OR left_work.value->>'slot' IS DISTINCT FROM right_work.value->>'slot'))
 WHEN 'slotUnique' THEN NOT EXISTS (SELECT 1 FROM agent_run_work_slots left_slot
  JOIN agent_run_work_slots right_slot ON right_slot.project_id=left_slot.project_id
   AND right_slot.run_id=left_slot.run_id AND right_slot.work_spec_ordinal=left_slot.work_spec_ordinal
   AND right_slot.slot=left_slot.slot AND right_slot.work_item_id<>left_slot.work_item_id
  WHERE left_slot.project_id=base.project_id AND left_slot.run_id=base.run_id)
 WHEN 'slotUniqueAcrossRun' THEN NOT EXISTS (SELECT 1 FROM agent_run_transitions left_transition
  JOIN agent_run_transitions right_transition ON right_transition.project_id=left_transition.project_id
   AND right_transition.run_id=left_transition.run_id
  CROSS JOIN LATERAL jsonb_each(left_transition.state_snapshot->'work_items') left_work
  CROSS JOIN LATERAL jsonb_each(right_transition.state_snapshot->'work_items') right_work
  WHERE left_transition.project_id=base.project_id AND left_transition.run_id=base.run_id
   AND left_work.key<>right_work.key
   AND left_work.value->>'work_spec_id'=right_work.value->>'work_spec_id'
   AND left_work.value->>'slot'=right_work.value->>'slot')
 WHEN 'workActivationSound' THEN NOT EXISTS (SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND work.value->>'status' IN ('eligible','claimed') AND NOT sprout_private.r541_condition_holds(
    (SELECT spec->'activation' FROM jsonb_array_elements(base.contract->'work_specs') spec
      WHERE spec->>'id'=work.value->>'work_spec_id'),transition.fact_references))
 WHEN 'eligibleWorkStatusSound' THEN NOT EXISTS (SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND work.value->>'status'='eligible' AND
    ((work.value->>'attempt')::integer>=(SELECT (spec->>'max_attempts')::integer
      FROM jsonb_array_elements(base.contract->'work_specs') spec WHERE spec->>'id'=work.value->>'work_spec_id')
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(base.contract->'dependencies') dependency
      WHERE dependency->>'obligation'=work.value->>'serves'
       AND transition.state_snapshot#>>ARRAY['obligations',dependency->>'prerequisite','status']
          IS DISTINCT FROM 'discharged')))
 WHEN 'blockedWorkHasWaitingBlocker' THEN NOT EXISTS (SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND work.value->>'status'='blocked' AND NOT EXISTS (
    SELECT 1 FROM jsonb_each(transition.state_snapshot->'blockers') blocker
    WHERE blocker.value->>'status'='waiting' AND blocker.value->>'run'=base.run_id::text
     AND blocker.value->>'goal'=base.goal_id::text
     AND (blocker.value#>>'{scope,work}'=work.key OR blocker.value->>'obligation'=work.value->>'serves'
      OR blocker.value#>>'{scope,goal}'=base.goal_id::text)))
 WHEN 'waitingBlockersExternallyControlled' THEN NOT EXISTS (SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'blockers') blocker
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND blocker.value->>'status'='waiting'
   AND blocker.value#>>'{condition,kind}' NOT IN
    ('human_task_completed','administrator_approval','principal_response','external_outcome'))
 WHEN 'allRelevantBlockersCertified' THEN NOT EXISTS (SELECT 1 FROM agent_run_blockers blocker
  WHERE blocker.project_id=base.project_id AND blocker.run_id=base.run_id
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(base.contract->'waiting_rules') rule
    WHERE (rule->>'id')::bigint=blocker.waiting_rule_ordinal
     AND rule->>'obligation'=blocker.obligation_id::text))
 WHEN 'blockerRuleStable' THEN NOT EXISTS (SELECT 1 FROM agent_run_transitions earlier
  JOIN agent_run_transitions later ON later.project_id=earlier.project_id AND later.run_id=earlier.run_id
   AND later.state_version>=earlier.state_version
  CROSS JOIN LATERAL jsonb_each(earlier.state_snapshot->'blockers') left_blocker
  JOIN LATERAL jsonb_each(later.state_snapshot->'blockers') right_blocker ON right_blocker.key=left_blocker.key
  WHERE earlier.project_id=base.project_id AND earlier.run_id=base.run_id
   AND (left_blocker.value->>'obligation' IS DISTINCT FROM right_blocker.value->>'obligation'
    OR left_blocker.value->>'waiting_rule_id' IS DISTINCT FROM right_blocker.value->>'waiting_rule_id'))
 WHEN 'ownerIsParticipant' THEN NOT EXISTS (SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND NOT EXISTS (SELECT 1 FROM agent_run_participants participant
    WHERE participant.project_id=base.project_id AND participant.run_id=base.run_id
     AND participant.identity_id=(work.value->>'owner')::uuid))
 WHEN 'parentGenerationSound' THEN NOT EXISTS (SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') child
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND child.value->>'parent' IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM jsonb_each(transition.state_snapshot->'work_items') parent
    JOIN LATERAL jsonb_array_elements(base.contract->'work_specs') child_spec
      ON child_spec->>'id'=child.value->>'work_spec_id'
    JOIN LATERAL jsonb_array_elements(base.contract->'work_specs') parent_spec
      ON parent_spec->>'id'=parent.value->>'work_spec_id'
    WHERE parent.key=child.value->>'parent'
     AND ((parent_spec->'continuations' ? (child_spec->>'id'))
       OR parent_spec#>>'{failure_plan,kind}'='alternatives'
        AND parent_spec#>'{failure_plan,work_specs}' ? (child_spec->>'id'))
     AND (child_spec->>'generation_rank')::integer<(parent_spec->>'generation_rank')::integer))
 WHEN 'validClaimsExclusive' THEN NOT EXISTS (SELECT 1 FROM agent_run_claim_leases left_claim
  JOIN agent_run_claim_leases right_claim ON right_claim.project_id=left_claim.project_id
   AND right_claim.run_id=left_claim.run_id AND right_claim.work_item_id=left_claim.work_item_id
   AND right_claim.attempt=left_claim.attempt AND right_claim.id<>left_claim.id
  WHERE left_claim.project_id=base.project_id AND left_claim.run_id=base.run_id
   AND left_claim.status='active' AND right_claim.status='active')
 WHEN 'expiredClaimsInvalid' THEN NOT EXISTS (SELECT 1 FROM agent_run_claim_leases claim
  WHERE claim.project_id=base.project_id AND claim.run_id=base.run_id
   AND claim.status='active' AND claim.expires_at<=clock_timestamp())
 WHEN 'causalRankDecreases' THEN NOT EXISTS (WITH RECURSIVE path(predecessor,successor) AS (
   SELECT link.predecessor,link.successor FROM agent_run_causal_links link
    WHERE link.project_id=base.project_id AND link.run_id=base.run_id
   UNION SELECT path.predecessor,link.successor FROM path JOIN agent_run_causal_links link
    ON link.project_id=base.project_id AND link.run_id=base.run_id AND link.predecessor=path.successor)
   SELECT 1 FROM path WHERE predecessor=successor)
 WHEN 'causalLinksComplete' THEN
  NOT EXISTS (SELECT 1 FROM jsonb_array_elements(base.contract->'dependencies') dependency
   WHERE NOT EXISTS (SELECT 1 FROM agent_run_causal_links link
    WHERE link.project_id=base.project_id AND link.run_id=base.run_id
     AND link.predecessor#>>'{obligation}'=dependency->>'prerequisite'
     AND link.successor#>>'{obligation}'=dependency->>'obligation'))
  AND NOT EXISTS (SELECT 1 FROM agent_run_transitions transition
   CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
   WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
    AND NOT EXISTS (SELECT 1 FROM agent_run_causal_links link
     WHERE link.project_id=base.project_id AND link.run_id=base.run_id
      AND link.predecessor#>>'{obligation}'=work.value->>'serves'
      AND link.successor#>>'{work}'=work.key))
 ELSE false END;

CREATE VIEW agent_r541_exact_scheduler_selections AS
SELECT root.trace_number,current.project_id,current.run_id,current.state_version,
 current.semantic_tick,claim.key::uuid AS claim_id,(claim.value->>'work')::uuid AS work_item_id,
 (claim.value->>'attempt')::integer AS attempt,prior.state_snapshot AS prior_state,
 current.state_snapshot AS current_state
FROM agent_r541_release_roots root
JOIN agent_run_transitions current ON current.project_id=root.project_id
 AND current.run_id=root.run_id AND current.transition_kind='work_claimed'
JOIN agent_run_transitions prior ON prior.project_id=current.project_id AND prior.run_id=current.run_id
 AND prior.state_version=current.state_version-1
CROSS JOIN LATERAL jsonb_each(current.state_snapshot->'claims') claim
WHERE NOT (prior.state_snapshot->'claims' ? claim.key)
 AND claim.value->>'status'='active'
 AND prior.state_snapshot#>>ARRAY['work_items',claim.value->>'work','status']='eligible'
 AND current.state_snapshot#>>ARRAY['work_items',claim.value->>'work','status']='claimed'
 AND current.state_snapshot#>>ARRAY['work_items',claim.value->>'work','attempt']=claim.value->>'attempt'
 AND (claim.value->>'acquired_at')::bigint=current.semantic_tick
 AND 1=(SELECT count(*) FROM jsonb_each(current.state_snapshot->'claims') candidate
   WHERE NOT (prior.state_snapshot->'claims' ? candidate.key));

CREATE VIEW agent_r541_scheduler_kernel_field_sources AS
WITH base AS (
 SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,root.start_tick,run.contract
 FROM agent_r541_release_roots root JOIN agent_collaborative_runs run
  ON run.project_id=root.project_id AND run.id=root.run_id
), fields(field_name) AS (VALUES ('selectedOnlyEligible'),('positionZeroSelected'),
 ('unselectedEligiblePersists'),('unselectedPositionDecreases'))
SELECT base.trace_number,fields.field_name,jsonb_build_object('trace_number',base.trace_number,
 'field',fields.field_name,'run',base.run_id,'goal',base.goal_id,'start_tick',base.start_tick) AS exact_record
FROM base CROSS JOIN fields
WHERE CASE fields.field_name
 WHEN 'selectedOnlyEligible' THEN
  (SELECT count(*) FROM agent_r541_exact_scheduler_selections selection
    WHERE selection.trace_number=base.trace_number)
  =(SELECT count(*) FROM agent_run_transitions transition
    WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
     AND transition.transition_kind='work_claimed')
 WHEN 'positionZeroSelected' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_scheduler_selections selection
  WHERE selection.trace_number=base.trace_number
   AND selection.prior_state#>>ARRAY['dispatches',selection.work_item_id::text,
     'scheduler_position'] IS DISTINCT FROM '0')
 WHEN 'unselectedEligiblePersists' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_scheduler_selections selection
  CROSS JOIN LATERAL jsonb_each(selection.prior_state->'work_items') work
  WHERE selection.trace_number=base.trace_number AND work.key<>selection.work_item_id::text
   AND work.value->>'status'='eligible'
   AND selection.current_state#>>ARRAY['work_items',work.key,'status'] IS DISTINCT FROM 'eligible')
 WHEN 'unselectedPositionDecreases' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_scheduler_selections selection
  CROSS JOIN LATERAL jsonb_each(selection.prior_state->'work_items') work
  WHERE selection.trace_number=base.trace_number AND work.key<>selection.work_item_id::text
   AND work.value->>'status'='eligible'
   AND selection.current_state#>>ARRAY['work_items',work.key,'status']='eligible'
   AND (selection.current_state#>>ARRAY['dispatches',work.key,'scheduler_position'])::bigint>=
       (selection.prior_state#>>ARRAY['dispatches',work.key,'scheduler_position'])::bigint)
 ELSE false END;

CREATE VIEW agent_r541_progress_kernel_field_sources AS
WITH base AS (
 SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,root.start_tick,
  run.contract,run.state,initialization.state_snapshot AS start_state,
  completion.state_snapshot AS completion_state,completion.semantic_tick AS completion_tick,
  trace.work_attempt_inventory,trace.work_outcome_inventory
 FROM agent_r541_release_roots root
 JOIN agent_collaborative_runs run ON run.project_id=root.project_id AND run.id=root.run_id
 JOIN agent_run_transitions initialization ON initialization.id=root.initialization_transition_id
 JOIN agent_run_transitions completion ON completion.project_id=root.project_id
  AND completion.run_id=root.run_id AND completion.transition_kind='run_completed'
 JOIN agent_r540_exact_release_trace_certificates trace ON trace.trace_number=root.trace_number
 WHERE run.run_status='completed' AND run.goal_status='completed'
), fields(field_name) AS (VALUES ('base'),('completionCommit'),('dynamics'),('measureLaws'),
 ('goalValidityPersistsUntilTerminal'),('goalValidAtStart'))
SELECT base.trace_number,fields.field_name,
 jsonb_build_object('trace_number',base.trace_number,'field',fields.field_name,
  'start_tick',base.start_tick,'completion_tick',base.completion_tick,
  'contract',base.contract,'start_state',base.start_state,'completion_state',base.completion_state,
  'work_attempts',base.work_attempt_inventory,'work_outcomes',base.work_outcome_inventory,
  'collaborative_kernel',(SELECT COALESCE(jsonb_agg(jsonb_build_object(
    'field',core.field_name,'record',core.exact_record) ORDER BY core.field_name),'[]'::jsonb)
    FROM agent_r541_collaborative_kernel_field_sources core WHERE core.trace_number=base.trace_number),
  'scheduler',(SELECT COALESCE(jsonb_agg(jsonb_build_object(
    'field',scheduler.field_name,'record',scheduler.exact_record) ORDER BY scheduler.field_name),'[]'::jsonb)
    FROM agent_r541_scheduler_kernel_field_sources scheduler WHERE scheduler.trace_number=base.trace_number)
 ) AS exact_record
FROM base CROSS JOIN fields
WHERE CASE fields.field_name
 WHEN 'base' THEN
  (SELECT count(*) FROM agent_r541_collaborative_kernel_field_sources core
    WHERE core.trace_number=base.trace_number)=22
  AND (SELECT count(DISTINCT field_name) FROM agent_r541_collaborative_kernel_field_sources core
    WHERE core.trace_number=base.trace_number)=22
  AND (SELECT count(*) FROM agent_r541_scheduler_kernel_field_sources scheduler
    WHERE scheduler.trace_number=base.trace_number)=4
  AND (SELECT count(DISTINCT field_name) FROM agent_r541_scheduler_kernel_field_sources scheduler
    WHERE scheduler.trace_number=base.trace_number)=4
  AND jsonb_typeof(base.contract->'obligations')='array'
  AND jsonb_typeof(base.contract->'work_specs')='array'
  AND base.contract->>'goal'=base.goal_id::text
  AND NOT EXISTS (
   SELECT 1 FROM agent_run_transitions transition
   CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
   WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
    AND transition.semantic_tick>=base.start_tick
    AND (work.value->>'id' IS DISTINCT FROM work.key
      OR work.value->>'run' IS DISTINCT FROM base.run_id::text
      OR work.value->>'goal' IS DISTINCT FROM base.goal_id::text
      OR NOT EXISTS (SELECT 1 FROM agent_run_work_slots slot
        JOIN LATERAL jsonb_array_elements(base.contract->'work_specs') spec(value)
          ON (spec.value->>'id')::bigint=slot.work_spec_ordinal
        WHERE slot.project_id=base.project_id AND slot.run_id=base.run_id
          AND slot.work_item_id=work.key::uuid
          AND slot.slot=(work.value->>'slot')::integer
          AND spec.value->>'owner'=work.value->>'owner'
          AND spec.value->>'obligation'=work.value->>'serves'
          AND spec.value->>'kind'=work.value->>'kind'
          AND (work.value->>'attempt')::integer<(spec.value->>'max_attempts')::integer)))
  AND NOT EXISTS (
   SELECT 1 FROM agent_run_transitions transition
   CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
   WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
    AND transition.semantic_tick>=base.start_tick AND work.value->>'status' IN ('eligible','claimed')
    AND NOT sprout_private.r541_condition_holds(
      (SELECT spec->'activation' FROM jsonb_array_elements(base.contract->'work_specs') spec
       WHERE spec->>'id'=work.value->>'work_spec_id'),transition.fact_references))
  AND NOT EXISTS (
   SELECT 1 FROM agent_run_claim_leases left_claim JOIN agent_run_claim_leases right_claim
    ON right_claim.project_id=left_claim.project_id AND right_claim.run_id=left_claim.run_id
     AND right_claim.work_item_id=left_claim.work_item_id AND right_claim.attempt=left_claim.attempt
     AND right_claim.id<>left_claim.id
   WHERE left_claim.project_id=base.project_id AND left_claim.run_id=base.run_id
    AND left_claim.status='active' AND right_claim.status='active')
  AND NOT EXISTS (SELECT 1 FROM agent_run_causal_links link
   WHERE link.project_id=base.project_id AND link.run_id=base.run_id
    AND (link.goal_id<>base.goal_id OR NOT EXISTS (
      SELECT 1 FROM agent_r540_typed_exact_release_events typed
      WHERE typed.trace_number=base.trace_number AND typed.event_kind='causal_link'
        AND typed.source_record_id=link.id)))
  AND NOT EXISTS (WITH RECURSIVE path(predecessor,successor) AS (
      SELECT link.predecessor,link.successor FROM agent_run_causal_links link
      WHERE link.project_id=base.project_id AND link.run_id=base.run_id
      UNION
      SELECT path.predecessor,link.successor FROM path JOIN agent_run_causal_links link
        ON link.project_id=base.project_id AND link.run_id=base.run_id
       AND link.predecessor=path.successor)
    SELECT 1 FROM path WHERE predecessor=successor)
  AND (SELECT count(*) FROM agent_r541_exact_scheduler_selections selection
    WHERE selection.trace_number=base.trace_number)
   =(SELECT count(*) FROM agent_run_transitions transition
     WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
      AND transition.transition_kind='work_claimed')
 WHEN 'completionCommit' THEN
  sprout_private.r541_completion_criterion(base.completion_state,base.contract,
    (SELECT transition.fact_references FROM agent_run_transitions transition
      WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
       AND transition.transition_kind='run_completed'))
  AND NOT EXISTS (SELECT 1 FROM agent_run_transitions transition
   WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
    AND transition.semantic_tick>=base.start_tick
    AND ((transition.state_snapshot->>'goal_status'='completed') IS DISTINCT FROM
      sprout_private.r541_completion_criterion(transition.state_snapshot,base.contract,transition.fact_references)))
 WHEN 'dynamics' THEN
  NOT EXISTS (SELECT 1 FROM agent_r540_typed_exact_release_events attempt
   JOIN LATERAL (SELECT min((outcome.formal_record->>'observed_at')::bigint) AS outcome_tick
     FROM agent_r540_typed_exact_release_events outcome
     WHERE outcome.trace_number=attempt.trace_number AND outcome.event_kind='work_outcome'
      AND outcome.formal_record->>'work'=attempt.formal_record->>'work'
      AND outcome.formal_record->>'claim'=attempt.formal_record->>'claim'
      AND outcome.formal_record->>'attempt'=attempt.formal_record->>'attempt') terminal ON true
   JOIN LATERAL (SELECT spec FROM jsonb_array_elements(base.contract->'work_specs') spec
     WHERE spec->>'id'=(SELECT event.event_snapshot#>>'{work_snapshot,work_spec_id}'
       FROM agent_r540_exact_release_events event WHERE event.id=attempt.id)) work_spec ON true
   WHERE attempt.trace_number=base.trace_number AND attempt.event_kind='work_attempt'
    AND (terminal.outcome_tick IS NULL OR terminal.outcome_tick<=(attempt.formal_record->>'tick')::bigint
      OR terminal.outcome_tick>(attempt.formal_record->>'tick')::bigint
        +(work_spec.spec->>'max_resolution_ticks')::bigint))
 WHEN 'measureLaws' THEN
  NOT EXISTS (SELECT 1 FROM agent_run_transitions current
   JOIN agent_run_transitions previous ON previous.project_id=current.project_id
    AND previous.run_id=current.run_id AND previous.state_version=current.state_version-1
   WHERE current.project_id=base.project_id AND current.run_id=base.run_id
    AND current.semantic_tick>=base.start_tick
    AND (sprout_private.r541_progress_rank(current.state_snapshot,base.contract)>
         sprout_private.r541_progress_rank(previous.state_snapshot,base.contract)
      OR (current.transition_kind IN ('work_succeeded','work_failed','blocker_resolved')
       AND sprout_private.r541_progress_rank(current.state_snapshot,base.contract)>=
           sprout_private.r541_progress_rank(previous.state_snapshot,base.contract))))
 WHEN 'goalValidityPersistsUntilTerminal' THEN
  NOT EXISTS (SELECT 1 FROM agent_run_transitions earlier
   JOIN agent_run_transitions later ON later.project_id=earlier.project_id
    AND later.run_id=earlier.run_id AND later.semantic_tick>=earlier.semantic_tick
   WHERE earlier.project_id=base.project_id AND earlier.run_id=base.run_id
    AND earlier.semantic_tick>=base.start_tick
    AND earlier.state_snapshot->>'goal_status' IN ('active','completed')
    AND later.state_snapshot->>'goal_status' NOT IN
      ('completed','failed','cancelled','superseded')
    AND later.state_snapshot->>'goal_status' NOT IN ('active','completed'))
 WHEN 'goalValidAtStart' THEN base.start_state->>'goal_status'='active'
  AND base.start_state->>'goal'=base.goal_id::text
 ELSE false END;

CREATE VIEW agent_r541_evidence_discharge_field_sources AS
WITH base AS (
 SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,root.start_tick,run.contract
 FROM agent_r541_release_roots root JOIN agent_collaborative_runs run
  ON run.project_id=root.project_id AND run.id=root.run_id
), fields(field_name) AS (VALUES ('dischargeSound'),('acceptedEvidenceCloses'),('completionCommit'))
SELECT base.trace_number,fields.field_name,
 jsonb_build_object('trace_number',base.trace_number,'field',fields.field_name,
  'evidence',(SELECT COALESCE(jsonb_agg(typed.formal_record ORDER BY typed.semantic_tick,typed.id),'[]'::jsonb)
   FROM agent_r540_typed_exact_release_events typed
   WHERE typed.trace_number=base.trace_number AND typed.event_kind='evidence')) AS exact_record
FROM base CROSS JOIN fields
WHERE CASE fields.field_name
 WHEN 'dischargeSound' THEN NOT EXISTS (
  SELECT 1 FROM agent_run_transitions transition
  CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'obligations') obligation
  WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id
   AND transition.semantic_tick>=base.start_tick AND obligation.value->>'status'='discharged'
   AND NOT EXISTS (SELECT 1 FROM agent_r540_typed_exact_release_events evidence
    WHERE evidence.trace_number=base.trace_number AND evidence.event_kind='evidence'
     AND evidence.semantic_tick<=transition.semantic_tick
     AND evidence.formal_record#>>'{evidence,obligation}'=obligation.key))
 WHEN 'acceptedEvidenceCloses' THEN NOT EXISTS (
  SELECT 1 FROM agent_r540_typed_exact_release_events evidence
  LEFT JOIN agent_run_transitions transition ON transition.project_id=base.project_id
   AND transition.run_id=base.run_id AND transition.semantic_tick=evidence.semantic_tick
   AND transition.transition_kind IN ('evidence_accepted','work_succeeded')
  WHERE evidence.trace_number=base.trace_number AND evidence.event_kind='evidence'
   AND (transition.id IS NULL OR transition.state_snapshot#>>ARRAY['obligations',
     evidence.formal_record#>>'{evidence,obligation}','status'] IS DISTINCT FROM 'discharged'))
 WHEN 'completionCommit' THEN EXISTS (SELECT 1 FROM agent_r541_progress_kernel_field_sources progress
  WHERE progress.trace_number=base.trace_number AND progress.field_name='completionCommit')
 ELSE false END;

CREATE FUNCTION sprout_private.reconstruct_agent_r541_progress_kernel_fields(
 candidate_trace_number bigint
) RETURNS TABLE(field_name text,exact_record jsonb)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off
SET join_collapse_limit=1 SET from_collapse_limit=1 SET jit=off AS $$
BEGIN
 IF candidate_trace_number<=0 THEN RETURN; END IF;
 RETURN QUERY EXECUTE format(
  'SELECT source.field_name,source.exact_record '
  'FROM public.agent_r541_progress_kernel_field_sources source '
  'WHERE source.trace_number=%s ORDER BY source.field_name',candidate_trace_number);
END $$;
REVOKE ALL ON FUNCTION sprout_private.reconstruct_agent_r541_progress_kernel_fields(bigint)
 FROM PUBLIC;

CREATE FUNCTION sprout_private.reconstruct_agent_r541_evidence_discharge_fields(
 candidate_trace_number bigint
) RETURNS TABLE(field_name text,exact_record jsonb)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off
SET join_collapse_limit=1 SET from_collapse_limit=1 SET jit=off AS $$
BEGIN
 IF candidate_trace_number<=0 THEN RETURN; END IF;
 RETURN QUERY EXECUTE format(
  'SELECT source.field_name,source.exact_record '
  'FROM public.agent_r541_evidence_discharge_field_sources source '
  'WHERE source.trace_number=%s ORDER BY source.field_name',candidate_trace_number);
END $$;
REVOKE ALL ON FUNCTION sprout_private.reconstruct_agent_r541_evidence_discharge_fields(bigint)
 FROM PUBLIC;

CREATE VIEW agent_r541_authority_information_field_sources AS
WITH base AS (
 SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,root.start_tick,
  run.contract,run.scope_resource_node_id,run.created_by_identity_id AS sponsor_identity_id,
  resource_authority.resource_authority,
  COALESCE(tool_authority.run_tool_ceiling,'[]'::jsonb) AS run_tool_ceiling,
  compiler.canonical_output->'security_policies' AS security_policies
 FROM agent_r541_release_roots root
 JOIN agent_collaborative_runs run ON run.project_id=root.project_id AND run.id=root.run_id
 JOIN agent_run_resource_authority_snapshots resource_authority
  ON resource_authority.project_id=root.project_id AND resource_authority.run_id=root.run_id
 LEFT JOIN agent_run_tool_security_snapshots tool_authority
  ON tool_authority.project_id=root.project_id AND tool_authority.run_id=root.run_id
 JOIN agent_local_goal_contracts local ON local.project_id=run.project_id
  AND local.id=run.local_goal_id AND local.revision=run.local_goal_revision
 JOIN agent_compilation_certificates compiler ON compiler.project_id=local.project_id
  AND compiler.id=local.compilation_certificate_id AND compiler.verification_state='verified'
), fields(field_name) AS (VALUES
 ('sponsorIsHuman'),('runAuthorityBoundedAtStart'),('runToolAuthorityBoundedAtStart'),
 ('workOriginSound'),('humanDelegationAuthorityBound'),('humanDelegationToolAuthorityBound'),
 ('childWorkAuthorityAttenuates'),('childWorkToolAuthorityAttenuates'),
 ('agentMoveHasSecurityEffect'),('coreAgentActionFootprintComplete'),
 ('coreAgentActionAllowedByContract'),('securityPolicyBoundToContract'),
 ('effectWorkCertified'),('effectWorkSemanticallyEnabled'),('effectSecurityPolicyAllowed'),
 ('effectWorkOwned'),('humanAssignedTaskControlIsolation'),('effectAuthoritySafe'),
 ('toolUseAuthoritySafe'),('toolInvocationMatchesCall'),('toolFootprintComplete'),
 ('effectWithinRunScope'),('modelContextProjectionExact'),('modelContextAuthoritySafe'),
 ('modelContextWithinRunScope'),('infoContextContainerValid'),('toolContextSourceOwned'),
 ('disclosureFootprintSound'),('canonicalResourceBody'),('contextualChatActionSafe'),
 ('disclosureContextSafe'),('persistedDisclosureContextSafe'))
SELECT base.trace_number,fields.field_name,
 jsonb_build_object('trace_number',base.trace_number,'field',fields.field_name,
  'run',base.run_id,'goal',base.goal_id,'start_tick',base.start_tick,
  'authoritative_source',CASE
   WHEN fields.field_name='sponsorIsHuman' THEN
    jsonb_build_object('sponsor',base.sponsor_identity_id)
   WHEN fields.field_name IN ('runAuthorityBoundedAtStart','effectAuthoritySafe',
      'modelContextAuthoritySafe') THEN base.resource_authority
   WHEN fields.field_name IN ('runToolAuthorityBoundedAtStart','childWorkToolAuthorityAttenuates',
      'toolUseAuthoritySafe') THEN base.run_tool_ceiling
   WHEN fields.field_name IN ('workOriginSound','humanDelegationAuthorityBound',
      'humanDelegationToolAuthorityBound','childWorkAuthorityAttenuates') THEN
    (SELECT COALESCE(jsonb_agg(to_jsonb(origin) ORDER BY origin.authority_depth,
       origin.work_item_id),'[]'::jsonb)
     FROM agent_r541_exact_work_authority_origins origin
     WHERE origin.trace_number=base.trace_number)
   WHEN fields.field_name IN ('modelContextProjectionExact','modelContextWithinRunScope',
      'infoContextContainerValid','toolContextSourceOwned') THEN
    (SELECT COALESCE(jsonb_agg(typed.formal_record ORDER BY typed.semantic_tick,typed.id),'[]'::jsonb)
     FROM agent_r540_typed_exact_release_events typed
     WHERE typed.trace_number=base.trace_number AND typed.event_kind='model_invocation')
   WHEN fields.field_name IN ('persistedDisclosureContextSafe','canonicalResourceBody',
      'contextualChatActionSafe','disclosureContextSafe') THEN
    (SELECT COALESCE(jsonb_agg(typed.formal_record ORDER BY typed.semantic_tick,typed.id),'[]'::jsonb)
     FROM agent_r540_typed_exact_release_events typed
     WHERE typed.trace_number=base.trace_number AND typed.event_kind='disclosure')
   ELSE
    (SELECT COALESCE(jsonb_agg(jsonb_build_object('source_kind',effect.source_kind,
      'source_record_id',effect.source_record_id,'tick',effect.semantic_tick,
      'work',effect.work_item_id,'claim',effect.claim_id,'attempt',effect.attempt,
      'actor',effect.actor_identity_id,'action',effect.action_class,
      'footprint',effect.footprint,'tool_invocations',effect.tool_invocations,
      'context_sources',effect.context_sources,'disclosure',effect.disclosure,
      'effect_hash',encode(effect.effect_hash,'hex'))
      ORDER BY effect.semantic_tick,effect.source_record_id),'[]'::jsonb)
     FROM agent_r541_exact_agent_security_effects effect
     WHERE effect.trace_number=base.trace_number)
   END,
  'security_policies',CASE WHEN fields.field_name IN
    ('coreAgentActionAllowedByContract','securityPolicyBoundToContract',
     'effectWorkCertified','effectWorkSemanticallyEnabled','effectSecurityPolicyAllowed')
    THEN base.security_policies ELSE NULL END) AS exact_record
FROM base CROSS JOIN fields
WHERE CASE fields.field_name
 WHEN 'sponsorIsHuman' THEN EXISTS (SELECT 1 FROM identities identity
  WHERE identity.id=base.sponsor_identity_id AND identity.status='active'
   AND identity.principal_kind IN ('administrator','user'))
 WHEN 'runAuthorityBoundedAtStart' THEN jsonb_typeof(base.resource_authority)='array'
  AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(base.resource_authority) authority
   WHERE authority->>'resource_id' IS NULL OR authority->>'operation' IS NULL)
 WHEN 'runToolAuthorityBoundedAtStart' THEN jsonb_typeof(base.run_tool_ceiling)='array'
  AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements_text(base.run_tool_ceiling) tool
   WHERE NOT EXISTS (SELECT 1 FROM agent_tool_permissions permission
    WHERE permission.project_id=base.project_id
     AND permission.principal_identity_id=base.sponsor_identity_id
     AND permission.tool_name=tool AND permission.tool_version=1
     AND permission.granted_at<=(SELECT created_at FROM agent_collaborative_runs run
       WHERE run.project_id=base.project_id AND run.id=base.run_id)
     AND (permission.revoked_at IS NULL OR permission.revoked_at>
       (SELECT created_at FROM agent_collaborative_runs run
        WHERE run.project_id=base.project_id AND run.id=base.run_id))))
 WHEN 'workOriginSound' THEN
  (SELECT count(*) FROM agent_r541_exact_work_authority_origins origin
    WHERE origin.trace_number=base.trace_number)
  =(SELECT count(DISTINCT work.key) FROM agent_run_transitions transition
    CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
    WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id)
  AND NOT EXISTS (SELECT 1 FROM agent_run_causal_links link
    WHERE link.project_id=base.project_id AND link.run_id=base.run_id
     AND link.predecessor->>'kind'='task' AND link.successor->>'kind'='work')
 WHEN 'humanDelegationAuthorityBound' THEN NOT EXISTS (SELECT 1 FROM agent_run_causal_links link
  WHERE link.project_id=base.project_id AND link.run_id=base.run_id
   AND link.predecessor->>'kind'='task' AND link.successor->>'kind'='work')
 WHEN 'humanDelegationToolAuthorityBound' THEN NOT EXISTS (SELECT 1 FROM agent_run_causal_links link
  WHERE link.project_id=base.project_id AND link.run_id=base.run_id
   AND link.predecessor->>'kind'='task' AND link.successor->>'kind'='work')
 WHEN 'childWorkAuthorityAttenuates' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_work_authority_origins child
  LEFT JOIN agent_r541_exact_work_authority_origins parent ON parent.trace_number=child.trace_number
   AND parent.work_item_id=child.authority_parent_id
  WHERE child.trace_number=base.trace_number AND child.authority_origin='inherited_work'
   AND (parent.work_item_id IS NULL OR child.authority_principal_id<>parent.authority_principal_id))
 WHEN 'childWorkToolAuthorityAttenuates' THEN NOT EXISTS (
  SELECT 1 FROM agent_tool_calls child LEFT JOIN agent_tool_calls parent
   ON parent.project_id=child.project_id AND parent.run_id=child.run_id
    AND parent.work_item_id=child.work_authority_parent_id
  WHERE child.project_id=base.project_id AND child.run_id=base.run_id
   AND child.work_authority_origin='inherited_work'
   AND (parent.id IS NULL OR NOT (child.work_tool_ceiling<@parent.work_tool_ceiling)))
 WHEN 'agentMoveHasSecurityEffect' THEN
  (SELECT count(*) FROM agent_r541_exact_agent_security_effects effect
    WHERE effect.trace_number=base.trace_number)
  =(SELECT count(*) FROM (
    SELECT tool.tool_event_id FROM agent_r540_exact_tool_trace_records tool
      WHERE tool.trace_number=base.trace_number AND tool.phase='pending'
    UNION ALL SELECT effect.id FROM agent_native_comment_security_effects effect
      WHERE effect.project_id=base.project_id AND effect.run_id=base.run_id
    UNION ALL SELECT effect.id FROM agent_run_task_effects effect
      WHERE effect.project_id=base.project_id AND effect.run_id=base.run_id) actual)
  AND NOT EXISTS (SELECT 1 FROM agent_effect_proposals proposal
   JOIN agent_model_invocation_projections projection ON projection.project_id=proposal.project_id
    AND projection.invocation_id=proposal.invocation_id
   WHERE proposal.project_id=base.project_id AND projection.run_id=base.run_id
    AND proposal.status='applied')
 WHEN 'coreAgentActionFootprintComplete' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  WHERE effect.trace_number=base.trace_number AND
   ((effect.action_class='post_comment' AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(effect.footprint) item
      WHERE item->>'operation'='post_comment' AND item->>'resource_id'=effect.disclosure->>'target'))
    OR (effect.action_class='mark_assigned_done' AND NOT EXISTS (
      SELECT 1 FROM jsonb_array_elements(effect.footprint) item
      WHERE item->>'operation'='complete_assigned_task'))))
 WHEN 'coreAgentActionAllowedByContract' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  WHERE effect.trace_number=base.trace_number AND NOT (effect.work_spec->'allowed_actions' ? effect.action_class))
 WHEN 'securityPolicyBoundToContract' THEN
  jsonb_array_length(base.security_policies)=jsonb_array_length(base.contract->'work_specs')
  AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(base.contract->'work_specs') spec
   WHERE 1<>(SELECT count(*) FROM jsonb_array_elements(base.security_policies) policy
     WHERE policy->>'work_spec_id'=spec->>'id'))
 WHEN 'effectWorkCertified' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  WHERE effect.trace_number=base.trace_number AND
   (effect.work_snapshot->>'id'<>effect.work_item_id::text
    OR effect.work_snapshot->>'serves'<>effect.work_spec->>'obligation'
    OR effect.work_snapshot->>'owner'<>effect.work_spec->>'owner'
    OR effect.work_snapshot->>'kind'<>effect.work_spec->>'kind'))
 WHEN 'effectWorkSemanticallyEnabled' THEN NOT EXISTS (
 SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  WHERE effect.trace_number=base.trace_number AND (
   NOT sprout_private.r541_condition_holds(effect.work_spec->'activation',effect.work_fact_references)
   OR EXISTS (SELECT 1 FROM jsonb_array_elements(base.contract->'dependencies') dependency
     WHERE dependency->>'obligation'=effect.work_snapshot->>'serves'
      AND effect.work_semantic_state#>>ARRAY['obligations',dependency->>'prerequisite','status']
          IS DISTINCT FROM 'discharged')))
 WHEN 'effectSecurityPolicyAllowed' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  WHERE effect.trace_number=base.trace_number AND
   (EXISTS (SELECT 1 FROM jsonb_array_elements(effect.footprint) item
      WHERE NOT (effect.security_policy->'allowed_operations' ? (item->>'operation')))
    OR EXISTS (SELECT 1 FROM jsonb_array_elements(effect.tool_invocations) invocation
      WHERE NOT (effect.security_policy->'allowed_tools' ? (invocation->>'tool')))))
 WHEN 'effectWorkOwned' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  WHERE effect.trace_number=base.trace_number AND (effect.run_id<>base.run_id OR effect.goal_id<>base.goal_id
   OR effect.work_snapshot->>'owner'<>effect.actor_identity_id::text))
 WHEN 'humanAssignedTaskControlIsolation' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  JOIN LATERAL jsonb_array_elements(effect.footprint) item ON true
  WHERE effect.trace_number=base.trace_number AND item->>'operation' IN
    ('complete_assigned_task','delegate_assigned_work','write','manage')
   AND EXISTS (SELECT 1 FROM tasks task JOIN task_assignments assignment
     ON assignment.project_id=task.project_id AND assignment.task_id=task.id
    WHERE task.project_id=base.project_id AND task.resource_node_id=(item->>'resource_id')::uuid
     AND assignment.revoked_at IS NULL AND assignment.assignee_identity_id<>effect.actor_identity_id
     AND EXISTS (SELECT 1 FROM identities assignee WHERE assignee.id=assignment.assignee_identity_id
       AND assignee.principal_kind IN ('administrator','user'))))
 WHEN 'effectAuthoritySafe' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  JOIN LATERAL jsonb_array_elements(effect.footprint) item ON true
  WHERE effect.trace_number=base.trace_number
   AND item->>'operation'<>'complete_assigned_task'
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(base.resource_authority) authority
    WHERE authority->>'resource_id'=item->>'resource_id' AND authority->>'operation'=item->>'operation'))
 WHEN 'toolUseAuthoritySafe' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  JOIN LATERAL jsonb_array_elements(effect.tool_invocations) invocation ON true
  JOIN agent_tool_calls call ON call.project_id=effect.project_id AND call.id=(invocation->>'call_id')::uuid
  WHERE effect.trace_number=base.trace_number AND
   (NOT (call.work_tool_ceiling ? (invocation->>'tool'))
    OR NOT EXISTS (SELECT 1 FROM agent_tool_permissions permission
      WHERE permission.project_id=call.project_id
       AND permission.principal_identity_id=call.work_authority_principal_id
       AND permission.tool_name=call.tool_name AND permission.tool_version=call.tool_version
       AND permission.granted_at<=call.requested_at
       AND (permission.revoked_at IS NULL OR permission.revoked_at>call.requested_at))
    OR NOT EXISTS (SELECT 1 FROM agent_tool_permissions permission
      WHERE permission.project_id=call.project_id AND permission.principal_identity_id=call.owner_identity_id
       AND permission.tool_name=call.tool_name AND permission.tool_version=call.tool_version
       AND permission.granted_at<=call.requested_at
       AND (permission.revoked_at IS NULL OR permission.revoked_at>call.requested_at))))
 WHEN 'toolInvocationMatchesCall' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  JOIN LATERAL jsonb_array_elements(effect.tool_invocations) invocation ON true
  LEFT JOIN agent_tool_calls call ON call.project_id=effect.project_id AND call.id=(invocation->>'call_id')::uuid
  WHERE effect.trace_number=base.trace_number AND (call.id IS NULL
   OR call.owner_identity_id<>effect.actor_identity_id OR call.tool_name<>invocation->>'tool'
   OR encode(call.canonical_input_commitment,'hex')<>invocation->>'input_commitment'))
 WHEN 'toolFootprintComplete' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  JOIN LATERAL jsonb_array_elements(effect.tool_invocations) invocation ON true
  JOIN agent_external_tool_catalog catalog ON catalog.tool_name=invocation->>'tool'
   AND catalog.version=(invocation->>'version')::integer
  WHERE effect.trace_number=base.trace_number AND NOT (catalog.required_effects<@effect.footprint))
 WHEN 'effectWithinRunScope' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  JOIN LATERAL jsonb_array_elements(effect.footprint) item ON true
  LEFT JOIN resource_closure closure ON closure.project_id=base.project_id
   AND closure.ancestor_id=base.scope_resource_node_id
   AND closure.descendant_id=(item->>'resource_id')::uuid
  WHERE effect.trace_number=base.trace_number AND closure.descendant_id IS NULL)
 WHEN 'modelContextProjectionExact' THEN NOT EXISTS (
  SELECT 1 FROM agent_model_invocation_projections projection
  LEFT JOIN agent_model_attempt_dispatches dispatch ON dispatch.project_id=projection.project_id
   AND dispatch.invocation_id=projection.invocation_id AND dispatch.attempt=projection.provider_attempt
  LEFT JOIN agent_model_attempt_observations observation ON observation.project_id=projection.project_id
   AND observation.id=projection.observation_id
  WHERE projection.project_id=base.project_id AND projection.run_id=base.run_id
   AND (dispatch.id IS NULL OR observation.id IS NULL
    OR projection.context_source_descriptors<>dispatch.source_descriptors
    OR projection.context_source_descriptors<>observation.exposed_source_descriptors
    OR projection.context_source_descriptors<>(SELECT COALESCE(jsonb_agg(source.source_descriptor ORDER BY source.ordinal),'[]'::jsonb)
      FROM agent_invocation_sources source WHERE source.project_id=projection.project_id
       AND source.invocation_id=projection.invocation_id)))
 WHEN 'modelContextAuthoritySafe' THEN NOT EXISTS (
  SELECT 1 FROM agent_model_invocation_projections projection
  JOIN agent_invocation_sources source ON source.project_id=projection.project_id
   AND source.invocation_id=projection.invocation_id AND source.resource_node_id IS NOT NULL
  WHERE projection.project_id=base.project_id AND projection.run_id=base.run_id
   AND (NOT EXISTS (SELECT 1 FROM jsonb_array_elements(base.resource_authority) authority
      WHERE authority->>'resource_id'=source.resource_node_id::text
       AND authority->>'operation'=CASE source.source_kind WHEN 'comment' THEN 'read_comment' ELSE 'read' END)
    OR NOT EXISTS (SELECT 1 FROM resource_nodes node
      JOIN projects project ON project.id=node.project_id
      JOIN project_memberships membership ON membership.project_id=node.project_id
       AND membership.identity_id=projection.principal_identity_id AND membership.state='active'
      LEFT JOIN LATERAL sprout_private.effective_domain_permission(
        node.project_id,node.id,projection.principal_identity_id) permission ON true
      WHERE node.project_id=source.project_id AND node.id=source.resource_node_id
       AND node.deleted_at IS NULL AND (project.owner_identity_id=projection.principal_identity_id
        OR membership.role='admin' OR node.created_by_identity_id=projection.principal_identity_id
        OR (permission.access_scope='full' AND permission.access_level IS NOT NULL)))))
 WHEN 'modelContextWithinRunScope' THEN NOT EXISTS (
  SELECT 1 FROM agent_model_invocation_projections projection
  JOIN agent_invocation_sources source ON source.project_id=projection.project_id
   AND source.invocation_id=projection.invocation_id AND source.resource_node_id IS NOT NULL
  LEFT JOIN resource_closure closure ON closure.project_id=source.project_id
   AND closure.ancestor_id=base.scope_resource_node_id AND closure.descendant_id=source.resource_node_id
  WHERE projection.project_id=base.project_id AND projection.run_id=base.run_id
   AND closure.descendant_id IS NULL)
 WHEN 'infoContextContainerValid' THEN NOT EXISTS (
  SELECT 1 FROM agent_model_invocation_projections projection
  JOIN agent_invocation_sources source ON source.project_id=projection.project_id
   AND source.invocation_id=projection.invocation_id AND source.source_kind IN ('info_document','info_file')
  LEFT JOIN resource_nodes node ON node.project_id=source.project_id AND node.id=source.resource_node_id
   AND node.node_kind IN ('topic','task_list') AND node.deleted_at IS NULL
  WHERE projection.project_id=base.project_id AND projection.run_id=base.run_id AND node.id IS NULL)
 WHEN 'toolContextSourceOwned' THEN NOT EXISTS (
  SELECT 1 FROM agent_model_invocation_projections projection
  JOIN agent_invocation_sources source ON source.project_id=projection.project_id
   AND source.invocation_id=projection.invocation_id AND source.source_kind='tool_output'
  LEFT JOIN agent_tool_calls call ON call.project_id=source.project_id AND call.id=source.source_id
   AND call.current_status='succeeded' AND call.owner_identity_id=projection.principal_identity_id
   AND call.output_readable_by ? projection.principal_identity_id::text
  WHERE projection.project_id=base.project_id AND projection.run_id=base.run_id AND call.id IS NULL)
 WHEN 'disclosureFootprintSound' THEN NOT EXISTS (
  SELECT 1 FROM agent_r541_exact_agent_security_effects effect
  WHERE effect.trace_number=base.trace_number AND effect.disclosure IS NOT NULL
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(effect.footprint) item
    WHERE item->>'resource_id'=effect.disclosure->>'target' AND item->>'operation'='post_comment'))
 WHEN 'canonicalResourceBody' THEN NOT EXISTS (
  SELECT 1 FROM agent_effect_proposals proposal JOIN agent_model_invocation_projections projection
   ON projection.project_id=proposal.project_id AND projection.invocation_id=proposal.invocation_id
  WHERE proposal.project_id=base.project_id AND projection.run_id=base.run_id AND proposal.status='applied')
 WHEN 'contextualChatActionSafe' THEN NOT EXISTS (
  SELECT 1 FROM agent_r540_typed_exact_release_events disclosure
  WHERE disclosure.trace_number=base.trace_number AND disclosure.event_kind='disclosure'
   AND disclosure.formal_record#>>'{sink,kind}'='contextual_chat')
 WHEN 'disclosureContextSafe' THEN NOT EXISTS (
  SELECT 1 FROM agent_r540_exact_release_events disclosure
  LEFT JOIN agent_r540_typed_exact_release_events typed ON typed.id=disclosure.id
  WHERE disclosure.trace_number=base.trace_number AND disclosure.event_kind='disclosure'
   AND typed.id IS NULL)
 WHEN 'persistedDisclosureContextSafe' THEN NOT EXISTS (
  SELECT 1 FROM agent_native_comment_security_effects effect
  JOIN native_comments comment ON comment.project_id=effect.project_id AND comment.id=effect.comment_id
  WHERE effect.project_id=base.project_id AND effect.run_id=base.run_id
   AND (comment.payload_purged_at IS NOT NULL OR comment.encrypted_payload IS NULL
    OR digest(comment.encrypted_payload,'sha256')<>comment.payload_commitment))
 ELSE false END;

CREATE VIEW agent_r541_authority_information_base AS
SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,root.start_tick,
 run.contract,run.scope_resource_node_id,run.created_by_identity_id AS sponsor_identity_id,
 resource_authority.resource_authority,
 COALESCE(tool_authority.run_tool_ceiling,'[]'::jsonb) AS run_tool_ceiling,
 compiler.canonical_output->'security_policies' AS security_policies
FROM agent_r541_release_roots root
JOIN agent_collaborative_runs run ON run.project_id=root.project_id AND run.id=root.run_id
JOIN agent_run_resource_authority_snapshots resource_authority
 ON resource_authority.project_id=root.project_id AND resource_authority.run_id=root.run_id
LEFT JOIN agent_run_tool_security_snapshots tool_authority
 ON tool_authority.project_id=root.project_id AND tool_authority.run_id=root.run_id
JOIN agent_local_goal_contracts local ON local.project_id=run.project_id
 AND local.id=run.local_goal_id AND local.revision=run.local_goal_revision
JOIN agent_compilation_certificates compiler ON compiler.project_id=local.project_id
 AND compiler.id=local.compilation_certificate_id AND compiler.verification_state='verified';

CREATE FUNCTION sprout_private.reconstruct_agent_r541_authority_information_field(
 candidate_trace_number bigint,candidate_field text
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE base public.agent_r541_authority_information_base%ROWTYPE;
 holds boolean:=false; source_snapshot jsonb;
BEGIN
 SELECT * INTO base FROM public.agent_r541_authority_information_base
  WHERE trace_number=candidate_trace_number;
 IF NOT FOUND THEN RETURN NULL; END IF;
 CASE candidate_field
 WHEN 'sponsorIsHuman' THEN
  SELECT EXISTS (SELECT 1 FROM public.identities identity
   WHERE identity.id=base.sponsor_identity_id AND identity.status='active'
    AND identity.principal_kind IN ('administrator','user')) INTO holds;
  SELECT to_jsonb(identity)-ARRAY['created_at','updated_at'] INTO source_snapshot
   FROM public.identities identity WHERE identity.id=base.sponsor_identity_id;
 WHEN 'runAuthorityBoundedAtStart' THEN
  SELECT jsonb_typeof(base.resource_authority)='array' AND NOT EXISTS (
   SELECT 1 FROM jsonb_array_elements(base.resource_authority) authority
   WHERE authority->>'resource_id' IS NULL OR authority->>'operation' IS NULL) INTO holds;
  source_snapshot:=base.resource_authority;
 WHEN 'runToolAuthorityBoundedAtStart' THEN
  SELECT jsonb_typeof(base.run_tool_ceiling)='array' AND NOT EXISTS (
   SELECT 1 FROM jsonb_array_elements_text(base.run_tool_ceiling) tool
   WHERE NOT EXISTS (SELECT 1 FROM public.agent_tool_permissions permission
    JOIN public.agent_collaborative_runs run ON run.project_id=base.project_id AND run.id=base.run_id
    WHERE permission.project_id=base.project_id
     AND permission.principal_identity_id=base.sponsor_identity_id
     AND permission.tool_name=tool AND permission.tool_version=1
     AND permission.granted_at<=run.created_at
     AND (permission.revoked_at IS NULL OR permission.revoked_at>run.created_at))) INTO holds;
  source_snapshot:=base.run_tool_ceiling;
 WHEN 'workOriginSound' THEN
  SELECT (SELECT count(*) FROM public.agent_r541_exact_work_authority_origins origin
    WHERE origin.trace_number=base.trace_number)
   =(SELECT count(DISTINCT work.key) FROM public.agent_run_transitions transition
     CROSS JOIN LATERAL jsonb_each(transition.state_snapshot->'work_items') work
     WHERE transition.project_id=base.project_id AND transition.run_id=base.run_id)
   AND NOT EXISTS (SELECT 1 FROM public.agent_run_causal_links link
    WHERE link.project_id=base.project_id AND link.run_id=base.run_id
     AND link.predecessor->>'kind'='task' AND link.successor->>'kind'='work') INTO holds;
 WHEN 'humanDelegationAuthorityBound' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_run_causal_links link
   WHERE link.project_id=base.project_id AND link.run_id=base.run_id
    AND link.predecessor->>'kind'='task' AND link.successor->>'kind'='work') INTO holds;
 WHEN 'humanDelegationToolAuthorityBound' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_run_causal_links link
   WHERE link.project_id=base.project_id AND link.run_id=base.run_id
    AND link.predecessor->>'kind'='task' AND link.successor->>'kind'='work') INTO holds;
 WHEN 'childWorkAuthorityAttenuates' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_work_authority_origins child
   LEFT JOIN public.agent_r541_exact_work_authority_origins parent
    ON parent.trace_number=child.trace_number AND parent.work_item_id=child.authority_parent_id
   WHERE child.trace_number=base.trace_number AND child.authority_origin='inherited_work'
    AND (parent.work_item_id IS NULL OR child.authority_principal_id<>parent.authority_principal_id))
   INTO holds;
 WHEN 'childWorkToolAuthorityAttenuates' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_tool_calls child
   LEFT JOIN public.agent_tool_calls parent ON parent.project_id=child.project_id
    AND parent.run_id=child.run_id AND parent.work_item_id=child.work_authority_parent_id
   WHERE child.project_id=base.project_id AND child.run_id=base.run_id
    AND child.work_authority_origin='inherited_work'
    AND (parent.id IS NULL OR NOT (child.work_tool_ceiling<@parent.work_tool_ceiling))) INTO holds;
 WHEN 'agentMoveHasSecurityEffect' THEN
  SELECT (SELECT count(*) FROM public.agent_r541_exact_agent_security_effects effect
    WHERE effect.trace_number=base.trace_number)
   =(SELECT count(*) FROM (
     SELECT tool.tool_event_id FROM public.agent_r540_exact_tool_trace_records tool
      WHERE tool.trace_number=base.trace_number AND tool.phase='pending'
     UNION ALL SELECT effect.id FROM public.agent_native_comment_security_effects effect
      WHERE effect.project_id=base.project_id AND effect.run_id=base.run_id
     UNION ALL SELECT effect.id FROM public.agent_run_task_effects effect
      WHERE effect.project_id=base.project_id AND effect.run_id=base.run_id) actual)
   AND NOT EXISTS (SELECT 1 FROM public.agent_effect_proposals proposal
    JOIN public.agent_model_invocation_projections projection ON projection.project_id=proposal.project_id
     AND projection.invocation_id=proposal.invocation_id
    WHERE proposal.project_id=base.project_id AND projection.run_id=base.run_id
     AND proposal.status='applied') INTO holds;
 WHEN 'coreAgentActionFootprintComplete' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   WHERE effect.trace_number=base.trace_number AND
    ((effect.action_class='post_comment' AND NOT EXISTS (
       SELECT 1 FROM jsonb_array_elements(effect.footprint) item
       WHERE item->>'operation'='post_comment' AND item->>'resource_id'=effect.disclosure->>'target'))
     OR (effect.action_class='mark_assigned_done' AND NOT EXISTS (
       SELECT 1 FROM jsonb_array_elements(effect.footprint) item
       WHERE item->>'operation'='complete_assigned_task')))) INTO holds;
 WHEN 'coreAgentActionAllowedByContract' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   WHERE effect.trace_number=base.trace_number
    AND NOT (effect.work_spec->'allowed_actions' ? effect.action_class)) INTO holds;
 WHEN 'securityPolicyBoundToContract' THEN
  SELECT jsonb_array_length(base.security_policies)=jsonb_array_length(base.contract->'work_specs')
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(base.contract->'work_specs') spec
    WHERE 1<>(SELECT count(*) FROM jsonb_array_elements(base.security_policies) policy
      WHERE policy->>'work_spec_id'=spec->>'id')) INTO holds;
 WHEN 'effectWorkCertified' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   WHERE effect.trace_number=base.trace_number AND
    (effect.work_snapshot->>'id'<>effect.work_item_id::text
     OR effect.work_snapshot->>'serves'<>effect.work_spec->>'obligation'
     OR effect.work_snapshot->>'owner'<>effect.work_spec->>'owner'
     OR effect.work_snapshot->>'kind'<>effect.work_spec->>'kind')) INTO holds;
 WHEN 'effectWorkSemanticallyEnabled' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   WHERE effect.trace_number=base.trace_number AND
    (NOT sprout_private.r541_condition_holds(effect.work_spec->'activation',effect.work_fact_references)
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(base.contract->'dependencies') dependency
      WHERE dependency->>'obligation'=effect.work_snapshot->>'serves'
       AND effect.work_semantic_state#>>ARRAY['obligations',dependency->>'prerequisite','status']
        IS DISTINCT FROM 'discharged'))) INTO holds;
 WHEN 'effectSecurityPolicyAllowed' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   WHERE effect.trace_number=base.trace_number AND
    (EXISTS (SELECT 1 FROM jsonb_array_elements(effect.footprint) item
      WHERE NOT (effect.security_policy->'allowed_operations' ? (item->>'operation')))
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(effect.tool_invocations) invocation
      WHERE NOT (effect.security_policy->'allowed_tools' ? (invocation->>'tool'))))) INTO holds;
 WHEN 'effectWorkOwned' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   WHERE effect.trace_number=base.trace_number AND (effect.run_id<>base.run_id OR effect.goal_id<>base.goal_id
    OR effect.work_snapshot->>'owner'<>effect.actor_identity_id::text)) INTO holds;
 WHEN 'humanAssignedTaskControlIsolation' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   JOIN LATERAL jsonb_array_elements(effect.footprint) item ON true
   WHERE effect.trace_number=base.trace_number AND item->>'operation' IN
    ('complete_assigned_task','delegate_assigned_work','write','manage')
    AND EXISTS (SELECT 1 FROM public.tasks task JOIN public.task_assignments assignment
      ON assignment.project_id=task.project_id AND assignment.task_id=task.id
     WHERE task.project_id=base.project_id AND task.resource_node_id=(item->>'resource_id')::uuid
      AND assignment.revoked_at IS NULL AND assignment.assignee_identity_id<>effect.actor_identity_id
      AND EXISTS (SELECT 1 FROM public.identities assignee
       WHERE assignee.id=assignment.assignee_identity_id
        AND assignee.principal_kind IN ('administrator','user')))) INTO holds;
 WHEN 'effectAuthoritySafe' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   JOIN LATERAL jsonb_array_elements(effect.footprint) item ON true
   WHERE effect.trace_number=base.trace_number AND item->>'operation'<>'complete_assigned_task'
    AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(base.resource_authority) authority
     WHERE authority->>'resource_id'=item->>'resource_id'
      AND authority->>'operation'=item->>'operation')) INTO holds;
 WHEN 'toolUseAuthoritySafe' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   JOIN LATERAL jsonb_array_elements(effect.tool_invocations) invocation ON true
   JOIN public.agent_tool_calls call ON call.project_id=effect.project_id
    AND call.id=(invocation->>'call_id')::uuid
   WHERE effect.trace_number=base.trace_number AND
    (NOT (call.work_tool_ceiling ? (invocation->>'tool'))
     OR NOT EXISTS (SELECT 1 FROM public.agent_tool_permissions permission
       WHERE permission.project_id=call.project_id
        AND permission.principal_identity_id=call.work_authority_principal_id
        AND permission.tool_name=call.tool_name AND permission.tool_version=call.tool_version
        AND permission.granted_at<=call.requested_at
        AND (permission.revoked_at IS NULL OR permission.revoked_at>call.requested_at))
     OR NOT EXISTS (SELECT 1 FROM public.agent_tool_permissions permission
       WHERE permission.project_id=call.project_id
        AND permission.principal_identity_id=call.owner_identity_id
        AND permission.tool_name=call.tool_name AND permission.tool_version=call.tool_version
        AND permission.granted_at<=call.requested_at
        AND (permission.revoked_at IS NULL OR permission.revoked_at>call.requested_at)))) INTO holds;
 WHEN 'toolInvocationMatchesCall' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   JOIN LATERAL jsonb_array_elements(effect.tool_invocations) invocation ON true
   LEFT JOIN public.agent_tool_calls call ON call.project_id=effect.project_id
    AND call.id=(invocation->>'call_id')::uuid
   WHERE effect.trace_number=base.trace_number AND (call.id IS NULL
    OR call.owner_identity_id<>effect.actor_identity_id OR call.tool_name<>invocation->>'tool'
    OR encode(call.canonical_input_commitment,'hex')<>invocation->>'input_commitment')) INTO holds;
 WHEN 'toolFootprintComplete' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   JOIN LATERAL jsonb_array_elements(effect.tool_invocations) invocation ON true
   JOIN public.agent_external_tool_catalog catalog ON catalog.tool_name=invocation->>'tool'
    AND catalog.version=(invocation->>'version')::integer
   WHERE effect.trace_number=base.trace_number
    AND NOT (catalog.required_effects<@effect.footprint)) INTO holds;
 WHEN 'effectWithinRunScope' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   JOIN LATERAL jsonb_array_elements(effect.footprint) item ON true
   LEFT JOIN public.resource_closure closure ON closure.project_id=base.project_id
    AND closure.ancestor_id=base.scope_resource_node_id
    AND closure.descendant_id=(item->>'resource_id')::uuid
   WHERE effect.trace_number=base.trace_number AND closure.descendant_id IS NULL) INTO holds;
 WHEN 'modelContextProjectionExact' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_model_invocation_projections projection
   LEFT JOIN public.agent_model_attempt_dispatches dispatch ON dispatch.project_id=projection.project_id
    AND dispatch.invocation_id=projection.invocation_id AND dispatch.attempt=projection.provider_attempt
   LEFT JOIN public.agent_model_attempt_observations observation ON observation.project_id=projection.project_id
    AND observation.id=projection.observation_id
   WHERE projection.project_id=base.project_id AND projection.run_id=base.run_id
    AND (dispatch.id IS NULL OR observation.id IS NULL
     OR projection.context_source_descriptors<>dispatch.source_descriptors
     OR projection.context_source_descriptors<>observation.exposed_source_descriptors
     OR projection.context_source_descriptors<>(SELECT COALESCE(
       jsonb_agg(source.source_descriptor ORDER BY source.ordinal),'[]'::jsonb)
       FROM public.agent_invocation_sources source WHERE source.project_id=projection.project_id
        AND source.invocation_id=projection.invocation_id))) INTO holds;
 WHEN 'modelContextAuthoritySafe' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_model_invocation_projections projection
   JOIN public.agent_invocation_sources source ON source.project_id=projection.project_id
    AND source.invocation_id=projection.invocation_id AND source.resource_node_id IS NOT NULL
   WHERE projection.project_id=base.project_id AND projection.run_id=base.run_id
    AND (NOT EXISTS (SELECT 1 FROM jsonb_array_elements(base.resource_authority) authority
      WHERE authority->>'resource_id'=source.resource_node_id::text
       AND authority->>'operation'=CASE source.source_kind WHEN 'comment' THEN 'read_comment' ELSE 'read' END)
     OR NOT EXISTS (SELECT 1 FROM public.resource_nodes node
      JOIN public.projects project ON project.id=node.project_id
      JOIN public.project_memberships membership ON membership.project_id=node.project_id
       AND membership.identity_id=projection.principal_identity_id AND membership.state='active'
      LEFT JOIN LATERAL sprout_private.effective_domain_permission(
       node.project_id,node.id,projection.principal_identity_id) permission ON true
      WHERE node.project_id=source.project_id AND node.id=source.resource_node_id
       AND node.deleted_at IS NULL AND (project.owner_identity_id=projection.principal_identity_id
        OR membership.role='admin' OR node.created_by_identity_id=projection.principal_identity_id
        OR (permission.access_scope='full' AND permission.access_level IS NOT NULL))))) INTO holds;
 WHEN 'modelContextWithinRunScope' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_model_invocation_projections projection
   JOIN public.agent_invocation_sources source ON source.project_id=projection.project_id
    AND source.invocation_id=projection.invocation_id AND source.resource_node_id IS NOT NULL
   LEFT JOIN public.resource_closure closure ON closure.project_id=source.project_id
    AND closure.ancestor_id=base.scope_resource_node_id AND closure.descendant_id=source.resource_node_id
   WHERE projection.project_id=base.project_id AND projection.run_id=base.run_id
    AND closure.descendant_id IS NULL) INTO holds;
 WHEN 'infoContextContainerValid' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_model_invocation_projections projection
   JOIN public.agent_invocation_sources source ON source.project_id=projection.project_id
    AND source.invocation_id=projection.invocation_id
    AND source.source_kind IN ('info_document','info_file')
   LEFT JOIN public.resource_nodes node ON node.project_id=source.project_id
    AND node.id=source.resource_node_id AND node.node_kind IN ('topic','task_list')
    AND node.deleted_at IS NULL
   WHERE projection.project_id=base.project_id AND projection.run_id=base.run_id
    AND node.id IS NULL) INTO holds;
 WHEN 'toolContextSourceOwned' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_model_invocation_projections projection
   JOIN public.agent_invocation_sources source ON source.project_id=projection.project_id
    AND source.invocation_id=projection.invocation_id AND source.source_kind='tool_output'
   LEFT JOIN public.agent_tool_calls call ON call.project_id=source.project_id AND call.id=source.source_id
    AND call.current_status='succeeded' AND call.owner_identity_id=projection.principal_identity_id
    AND call.output_readable_by ? projection.principal_identity_id::text
   WHERE projection.project_id=base.project_id AND projection.run_id=base.run_id
    AND call.id IS NULL) INTO holds;
 WHEN 'disclosureFootprintSound' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r541_exact_agent_security_effects effect
   WHERE effect.trace_number=base.trace_number AND effect.disclosure IS NOT NULL
    AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(effect.footprint) item
     WHERE item->>'resource_id'=effect.disclosure->>'target'
      AND item->>'operation'='post_comment')) INTO holds;
 WHEN 'canonicalResourceBody' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_effect_proposals proposal
   JOIN public.agent_model_invocation_projections projection
    ON projection.project_id=proposal.project_id AND projection.invocation_id=proposal.invocation_id
   WHERE proposal.project_id=base.project_id AND projection.run_id=base.run_id
    AND proposal.status='applied') INTO holds;
 WHEN 'contextualChatActionSafe' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r540_typed_exact_release_events disclosure
   WHERE disclosure.trace_number=base.trace_number AND disclosure.event_kind='disclosure'
    AND disclosure.formal_record#>>'{sink,kind}'='contextual_chat') INTO holds;
 WHEN 'disclosureContextSafe' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_r540_exact_release_events disclosure
   LEFT JOIN public.agent_r540_typed_exact_release_events typed ON typed.id=disclosure.id
   WHERE disclosure.trace_number=base.trace_number AND disclosure.event_kind='disclosure'
    AND typed.id IS NULL) INTO holds;
 WHEN 'persistedDisclosureContextSafe' THEN
  SELECT NOT EXISTS (SELECT 1 FROM public.agent_native_comment_security_effects effect
   JOIN public.native_comments comment ON comment.project_id=effect.project_id
    AND comment.id=effect.comment_id
   WHERE effect.project_id=base.project_id AND effect.run_id=base.run_id
    AND (comment.payload_purged_at IS NOT NULL OR comment.encrypted_payload IS NULL
     OR public.digest(comment.encrypted_payload,'sha256')<>comment.payload_commitment)) INTO holds;
 ELSE RETURN NULL;
 END CASE;
 IF NOT holds THEN RETURN NULL; END IF;
 IF candidate_field IN ('workOriginSound','humanDelegationAuthorityBound',
   'humanDelegationToolAuthorityBound','childWorkAuthorityAttenuates') THEN
  SELECT COALESCE(jsonb_agg(to_jsonb(origin) ORDER BY origin.authority_depth,origin.work_item_id),'[]'::jsonb)
   INTO source_snapshot FROM public.agent_r541_exact_work_authority_origins origin
   WHERE origin.trace_number=base.trace_number;
 ELSIF candidate_field IN ('modelContextProjectionExact','modelContextAuthoritySafe',
   'modelContextWithinRunScope','infoContextContainerValid','toolContextSourceOwned') THEN
  SELECT COALESCE(jsonb_agg(typed.formal_record ORDER BY typed.semantic_tick,typed.id),'[]'::jsonb)
   INTO source_snapshot FROM public.agent_r540_typed_exact_release_events typed
   WHERE typed.trace_number=base.trace_number AND typed.event_kind='model_invocation';
 ELSIF candidate_field IN ('canonicalResourceBody','contextualChatActionSafe',
   'disclosureContextSafe','persistedDisclosureContextSafe') THEN
  SELECT COALESCE(jsonb_agg(typed.formal_record ORDER BY typed.semantic_tick,typed.id),'[]'::jsonb)
   INTO source_snapshot FROM public.agent_r540_typed_exact_release_events typed
   WHERE typed.trace_number=base.trace_number AND typed.event_kind='disclosure';
 ELSIF source_snapshot IS NULL THEN
  SELECT COALESCE(jsonb_agg(jsonb_build_object('source_kind',effect.source_kind,
    'source_record_id',effect.source_record_id,'tick',effect.semantic_tick,
    'work',effect.work_item_id,'claim',effect.claim_id,'attempt',effect.attempt,
    'actor',effect.actor_identity_id,'action',effect.action_class,
    'footprint',effect.footprint,'tool_invocations',effect.tool_invocations,
    'context_sources',effect.context_sources,'disclosure',effect.disclosure,
    'effect_hash',encode(effect.effect_hash,'hex')) ORDER BY effect.semantic_tick,effect.source_record_id),
    '[]'::jsonb) INTO source_snapshot
   FROM public.agent_r541_exact_agent_security_effects effect
   WHERE effect.trace_number=base.trace_number;
 END IF;
 RETURN jsonb_build_object('trace_number',base.trace_number,'field',candidate_field,
  'run',base.run_id,'goal',base.goal_id,'start_tick',base.start_tick,
  'authoritative_source',source_snapshot);
END $$;
REVOKE ALL ON FUNCTION sprout_private.reconstruct_agent_r541_authority_information_field(bigint,text)
 FROM PUBLIC;

CREATE FUNCTION sprout_private.reconstruct_agent_r541_authority_information(
 candidate_trace_number bigint
) RETURNS TABLE(field_name text,exact_record jsonb)
LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE
 candidate_field text;
 reconstructed jsonb;
BEGIN
 FOREACH candidate_field IN ARRAY ARRAY[
  'sponsorIsHuman','runAuthorityBoundedAtStart','runToolAuthorityBoundedAtStart',
  'workOriginSound','humanDelegationAuthorityBound','humanDelegationToolAuthorityBound',
  'childWorkAuthorityAttenuates','childWorkToolAuthorityAttenuates',
  'agentMoveHasSecurityEffect','coreAgentActionFootprintComplete',
  'coreAgentActionAllowedByContract','securityPolicyBoundToContract',
  'effectWorkCertified','effectWorkSemanticallyEnabled','effectSecurityPolicyAllowed',
  'effectWorkOwned','humanAssignedTaskControlIsolation','effectAuthoritySafe',
  'toolUseAuthoritySafe','toolInvocationMatchesCall','toolFootprintComplete',
  'effectWithinRunScope','modelContextProjectionExact','modelContextAuthoritySafe',
  'modelContextWithinRunScope','infoContextContainerValid','toolContextSourceOwned',
  'disclosureFootprintSound','canonicalResourceBody','contextualChatActionSafe',
  'disclosureContextSafe','persistedDisclosureContextSafe'
 ] LOOP
  reconstructed:=sprout_private.reconstruct_agent_r541_authority_information_field(
   candidate_trace_number,candidate_field);
  IF reconstructed IS NOT NULL THEN
   field_name:=candidate_field;
   exact_record:=reconstructed;
   RETURN NEXT;
  END IF;
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION sprout_private.reconstruct_agent_r541_authority_information(bigint)
 FROM PUBLIC;

CREATE OR REPLACE VIEW agent_r541_authority_information_field_sources AS
SELECT root.trace_number,reconstructed.field_name,reconstructed.exact_record
FROM agent_r541_release_roots root
CROSS JOIN LATERAL sprout_private.reconstruct_agent_r541_authority_information(
 root.trace_number) reconstructed;

-- The secure child consumes exactly 6 + 3 + 32 field-specific certificates.
-- A PL/pgSQL set projector is also an optimization fence: PostgreSQL must not
-- flatten all 41 independent universal predicates into one unbounded plan.
CREATE FUNCTION sprout_private.reconstruct_agent_r541_secure_kernel_nested(
 candidate_trace_number bigint
) RETURNS TABLE(nested_field text,nested_record jsonb)
LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off
SET join_collapse_limit=1 SET from_collapse_limit=1 SET jit=off AS $$
DECLARE candidate record;
BEGIN
 FOR candidate IN SELECT progress.field_name,progress.exact_record
   FROM sprout_private.reconstruct_agent_r541_progress_kernel_fields(
     candidate_trace_number) progress ORDER BY progress.field_name
 LOOP
  nested_field:='progress.'||candidate.field_name;
  nested_record:=candidate.exact_record;
  RETURN NEXT;
 END LOOP;
 FOR candidate IN SELECT evidence.field_name,evidence.exact_record
   FROM sprout_private.reconstruct_agent_r541_evidence_discharge_fields(
     candidate_trace_number) evidence ORDER BY evidence.field_name
 LOOP
  nested_field:='evidence_discharge.'||candidate.field_name;
  nested_record:=candidate.exact_record;
  RETURN NEXT;
 END LOOP;
 FOR candidate IN SELECT authority.field_name,authority.exact_record
   FROM sprout_private.reconstruct_agent_r541_authority_information(
     candidate_trace_number) authority ORDER BY authority.field_name
 LOOP
  nested_field:='authority_information.'||candidate.field_name;
  nested_record:=candidate.exact_record;
  RETURN NEXT;
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION sprout_private.reconstruct_agent_r541_secure_kernel_nested(bigint)
 FROM PUBLIC;

CREATE OR REPLACE VIEW agent_r541_secure_kernel_nested_sources AS
SELECT root.trace_number,nested.nested_field,nested.nested_record
FROM agent_r541_release_roots root
CROSS JOIN LATERAL sprout_private.reconstruct_agent_r541_secure_kernel_nested(
 root.trace_number) nested;

CREATE VIEW agent_r541_governance_kernel_nested_sources AS
WITH base AS (
 SELECT root.trace_number,root.project_id,root.run_id,root.governance_start_snapshot,
  COALESCE((SELECT jsonb_agg(to_jsonb(entry)-'recorded_at' ORDER BY entry.position)
    FROM agent_governance_ledger entry WHERE entry.project_id=root.project_id),'[]'::jsonb) history
 FROM agent_r541_release_roots root
), obligations(nested_field) AS (VALUES
 ('histories_append_only'),('responsibilities'),('agents'),('exceptions'),
 ('assignments'),('administrator_creations'),('prompt_local_consistency'),('global_revision'))
SELECT base.trace_number,obligations.nested_field,
 jsonb_build_object('trace_number',base.trace_number,'nested_field',obligations.nested_field,
  'authoritative_state',CASE obligations.nested_field
   WHEN 'histories_append_only' THEN jsonb_build_object('start',base.governance_start_snapshot,'after',base.history)
   WHEN 'responsibilities' THEN base.governance_start_snapshot->'responsibilities'
   WHEN 'agents' THEN base.governance_start_snapshot->'agents'
   WHEN 'exceptions' THEN base.governance_start_snapshot->'approved_exceptions'
   WHEN 'assignments' THEN base.governance_start_snapshot->'global_assignments'
   WHEN 'administrator_creations' THEN base.governance_start_snapshot->'administrator_creations'
   WHEN 'prompt_local_consistency' THEN base.governance_start_snapshot->'local_goals'
   WHEN 'global_revision' THEN base.governance_start_snapshot->'global_revisions' END) nested_record
FROM base CROSS JOIN obligations
WHERE CASE obligations.nested_field
 WHEN 'histories_append_only' THEN sprout_private.jsonb_array_is_prefix(
   base.governance_start_snapshot->'history',base.history)
 WHEN 'responsibilities' THEN NOT EXISTS (SELECT 1 FROM agent_responsibility_contracts responsibility
   LEFT JOIN identities admin ON admin.id=responsibility.administrator_identity_id
    AND admin.principal_kind='administrator' AND admin.status='active'
   LEFT JOIN agent_compilation_certificates compiler ON compiler.project_id=responsibility.project_id
    AND compiler.id=responsibility.compilation_certificate_id
    AND compiler.verification_state='verified' AND compiler.canonical_output->'contract'=responsibility.contract
   WHERE responsibility.project_id=base.project_id AND responsibility.state='active'
    AND (admin.id IS NULL OR compiler.id IS NULL))
 WHEN 'agents' THEN NOT EXISTS (SELECT 1 FROM governed_agents agent
   LEFT JOIN identities principal ON principal.id=agent.principal_identity_id
    AND principal.principal_kind='agent' AND principal.status='active'
   LEFT JOIN identities controller ON controller.id=agent.controller_identity_id
    AND controller.principal_kind IN ('administrator','user') AND controller.status='active'
   WHERE agent.project_id=base.project_id AND agent.state='active'
    AND (principal.id IS NULL OR controller.id IS NULL))
 WHEN 'exceptions' THEN NOT EXISTS (SELECT 1 FROM agent_governance_authorization_events event
   WHERE event.project_id=base.project_id AND event.event_kind='approved_local_exception'
    AND (event.administrator_identity_id IS NULL OR event.local_goal_id IS NULL
      OR event.local_goal_revision IS NULL OR event.responsibility_compilation_id IS NULL))
 WHEN 'assignments' THEN NOT EXISTS (SELECT 1 FROM agent_governance_authorization_events event
   WHERE event.project_id=base.project_id AND event.event_kind='global_mandate_assignment'
    AND (event.administrator_identity_id IS NULL OR event.agent_id IS NULL
      OR event.global_contract_id IS NULL OR event.global_revision IS NULL))
 WHEN 'administrator_creations' THEN NOT EXISTS (SELECT 1 FROM agent_administrator_creation_approvals approval
   LEFT JOIN identities admin ON admin.id=approval.administrator_identity_id
    AND admin.principal_kind='administrator' AND admin.status='active'
   LEFT JOIN governed_agents agent ON agent.project_id=approval.project_id
    AND agent.id=approval.governed_agent_id AND agent.principal_identity_id=approval.proposed_agent_identity_id
   WHERE approval.project_id=base.project_id AND (admin.id IS NULL OR agent.id IS NULL))
 WHEN 'prompt_local_consistency' THEN NOT EXISTS (SELECT 1 FROM agent_local_goal_contracts local
   LEFT JOIN agent_compilation_certificates compiler ON compiler.project_id=local.project_id
    AND compiler.id=local.compilation_certificate_id AND compiler.verification_state='verified'
    AND compiler.canonical_output->'contract'=local.contract->'contract'
   WHERE local.project_id=base.project_id AND local.state='active' AND compiler.id IS NULL)
 WHEN 'global_revision' THEN NOT EXISTS (SELECT 1 FROM agent_global_contracts global
   WHERE global.project_id=base.project_id AND global.synthesis_invocation_id IS NOT NULL
    AND NOT EXISTS (SELECT 1 FROM agent_model_invocation_projections projection
      WHERE projection.project_id=global.project_id AND projection.invocation_id=global.synthesis_invocation_id
       AND projection.status='succeeded' AND projection.language_task->>'kind'='synthesize_global_contract'))
 ELSE false END;

CREATE VIEW agent_r541_exact_formal_release_source_snapshots AS
WITH exact_base AS (
  SELECT root.trace_number,root.project_id,root.run_id,root.goal_id,root.start_tick,
    root.initialization_transition_id,root.root_hash,
    root.governance_start_snapshot,root.operational_start_snapshot,
    root.proxy_directory_start_snapshot,root.comment_start_snapshot,
    run.contract,run.contract_hash,run.state,run.state_hash,run.state_version,
    run.scope_resource_node_id,run.created_by_identity_id,
    initialization.next_state_hash AS initialization_state_hash,
    initialization.state_snapshot AS initialization_state,
    completion.id AS completion_transition_id,
    completion.semantic_tick AS completion_tick,
    completion.state_snapshot AS completion_state,
    completion.next_state_hash AS completion_state_hash,
    timeline.last_tick,timeline.allocation_count,timeline.cursor_hash,
    trace_certificate.id AS trace_certificate_id,
    trace_certificate.version AS trace_certificate_version,
    trace_certificate.certificate_hash AS trace_certificate_hash,
    trace_certificate.end_tick AS trace_end_tick,
    trace_certificate.work_attempt_inventory,
    trace_certificate.work_outcome_inventory,
    trace_certificate.blocker_inventory,trace_certificate.causal_inventory,
    trace_certificate.tool_inventory,trace_certificate.evidence_inventory,
    trace_certificate.disclosure_inventory,trace_certificate.model_inventory,
    trace_certificate.interrogation_inventory,
    compiler.id AS compiler_certificate_id,
    compiler.canonical_output AS compiler_output,
    compiler.compilation_envelope,
    compiler.certificate_hash AS compiler_certificate_hash,
    local.id AS local_goal_id,local.revision AS local_goal_revision,
    local.agent_id,local.agent_identity_id,local.controller_identity_id,
    governed.availability AS agent_availability,
    trace_gates.outcome_mode,trace_gates.outcome_records,
    trace_gates.blocker_mode,trace_gates.blocker_records,
    trace_gates.causal_mode,trace_gates.causal_records,
    trace_gates.tool_mode,trace_gates.tool_records,
    trace_gates.evidence_mode,trace_gates.evidence_records,
    trace_gates.disclosure_mode,trace_gates.disclosure_records,
    trace_gates.model_mode,trace_gates.model_records,
    trace_gates.interrogation_mode,trace_gates.interrogation_records,
    comment_gate.comment_mode,comment_gate.comment_records,
    COALESCE((SELECT jsonb_agg(jsonb_build_object('field',nested.nested_field,
      'record',nested.nested_record) ORDER BY nested.nested_field)
      FROM agent_r541_secure_kernel_nested_sources nested
      WHERE nested.trace_number=root.trace_number),'[]'::jsonb) AS secure_nested_records,
    COALESCE((SELECT jsonb_agg(jsonb_build_object('field',nested.nested_field,
      'record',nested.nested_record) ORDER BY nested.nested_field)
      FROM agent_r541_governance_kernel_nested_sources nested
      WHERE nested.trace_number=root.trace_number),'[]'::jsonb) AS governance_nested_records,
    COALESCE((SELECT jsonb_agg(to_jsonb(ledger)-'recorded_at' ORDER BY ledger.position)
      FROM agent_governance_ledger ledger WHERE ledger.project_id=root.project_id),'[]'::jsonb)
      AS governance_history,
    sprout_private.semantic_operational_state_snapshot(root.project_id,root.run_id) AS operational_history,
    COALESCE((SELECT jsonb_agg(jsonb_build_object('identity',membership.identity_id,
          'kind',identity.principal_kind,'proxy',proxy.id) ORDER BY membership.identity_id)
      FROM project_memberships membership
      JOIN identities identity ON identity.id=membership.identity_id
      JOIN user_proxies proxy ON proxy.project_id=membership.project_id
        AND proxy.user_identity_id=membership.identity_id
      WHERE membership.project_id=root.project_id AND membership.state='active'
        AND identity.status='active' AND identity.principal_kind IN ('administrator','user')),'[]'::jsonb)
      AS proxy_directory,
    COALESCE((SELECT jsonb_agg(to_jsonb(record)-'recorded_at' ORDER BY record.semantic_tick,record.id)
      FROM agent_r540_typed_exact_release_events record
      WHERE record.trace_number=root.trace_number AND record.event_kind='model_invocation'),'[]'::jsonb)
      AS exact_model_records,
    COALESCE((SELECT jsonb_agg(to_jsonb(record)-'recorded_at' ORDER BY record.semantic_tick,record.id)
      FROM agent_r540_typed_exact_release_events record
      WHERE record.trace_number=root.trace_number AND record.event_kind='interrogation'),'[]'::jsonb)
      AS exact_interrogation_records,
    COALESCE((SELECT jsonb_agg(to_jsonb(record) ORDER BY record.semantic_tick,record.comment_id)
      FROM agent_r541_typed_exact_comment_records record
      WHERE record.trace_number=root.trace_number),'[]'::jsonb) AS exact_comment_records,
    COALESCE((SELECT semantic_state.base_comments
      FROM agent_native_comment_run_semantic_states semantic_state
      WHERE semantic_state.trace_number=root.trace_number
      ORDER BY semantic_state.semantic_tick DESC,semantic_state.comment_event_id DESC LIMIT 1),
      root.comment_start_snapshot) AS comment_end_snapshot,
    COALESCE((SELECT jsonb_agg(jsonb_build_object('effect',effect.id,'work',effect.work_item_id,
        'task',effect.task_id,'task_intent',effect.task_intent_id) ORDER BY effect.applied_at,effect.id)
      FROM agent_run_task_effects effect
      WHERE effect.project_id=root.project_id AND effect.run_id=root.run_id),'[]'::jsonb)
      AS task_effect_records,
    COALESCE((SELECT jsonb_agg(jsonb_build_object(
        'call',call.id,'work',call.work_item_id,'claim',call.work_claim_id,
        'attempt',call.current_attempt,'actor',call.owner_identity_id,
        'authority_origin',call.work_authority_origin,
        'authority_principal',call.work_authority_principal_id,
        'run_ceiling_hash',encode(call.run_tool_ceiling_hash,'hex'),
        'work_ceiling_hash',encode(call.work_tool_ceiling_hash,'hex'),
        'work_ceiling',call.work_tool_ceiling,'required_effects',call.required_effects)
        ORDER BY call.requested_tick,call.id)
      FROM agent_tool_calls call WHERE call.project_id=root.project_id
        AND call.run_id=root.run_id),'[]'::jsonb) AS tool_security_records,
    COALESCE((SELECT jsonb_agg(jsonb_build_object(
        'effect',effect.id,'comment',effect.comment_id,'work',effect.work_item_id,
        'claim',effect.claim_id,'attempt',effect.attempt,'actor',effect.actor_identity_id,
        'target',effect.target_resource_node_id,'tick',effect.observed_tick,
        'payload_commitment',encode(effect.payload_commitment,'hex'))
        ORDER BY effect.observed_tick,effect.id)
      FROM agent_native_comment_security_effects effect
      WHERE effect.project_id=root.project_id AND effect.run_id=root.run_id),'[]'::jsonb)
      AS comment_security_records,
    COALESCE((SELECT jsonb_agg(to_jsonb(responsibility)-ARRAY['recorded_at','activated_at','superseded_at']
        ORDER BY responsibility.id,responsibility.revision)
      FROM agent_responsibility_contracts responsibility
      WHERE responsibility.project_id=root.project_id AND responsibility.state='active'),'[]'::jsonb)
      AS active_responsibilities,
    COALESCE((SELECT jsonb_agg(to_jsonb(approval)-'recorded_at' ORDER BY approval.approval_id)
      FROM agent_administrator_creation_approvals approval
      WHERE approval.project_id=root.project_id),'[]'::jsonb) AS administrator_creations,
    COALESCE((SELECT jsonb_agg(to_jsonb(event)-'recorded_at' ORDER BY event.ledger_position)
      FROM agent_governance_authorization_events event
      WHERE event.project_id=root.project_id AND event.event_kind='approved_local_exception'),
      '[]'::jsonb) AS approved_exceptions,
    COALESCE((SELECT jsonb_agg(to_jsonb(event)-'recorded_at' ORDER BY event.ledger_position)
      FROM agent_governance_authorization_events event
      WHERE event.project_id=root.project_id AND event.event_kind='global_mandate_assignment'),
      '[]'::jsonb) AS global_assignments,
    COALESCE((SELECT jsonb_agg(to_jsonb(global)-'recorded_at' ORDER BY global.id,global.revision)
      FROM agent_global_contracts global
      JOIN agent_invocations invocation ON invocation.project_id=global.project_id
        AND invocation.id=global.synthesis_invocation_id
        AND invocation.run_id=root.run_id AND invocation.goal_id=root.goal_id
      WHERE global.project_id=root.project_id),'[]'::jsonb)
      AS global_records,
    COALESCE((SELECT jsonb_agg(to_jsonb(plan)-'recorded_at' ORDER BY plan.id)
      FROM user_proxy_plans plan
      JOIN agent_invocations invocation ON invocation.project_id=plan.project_id
        AND invocation.id=plan.invocation_id AND invocation.invocation_surface='user_proxy'
        AND invocation.run_id=root.run_id AND invocation.goal_id=root.goal_id
      WHERE plan.project_id=root.project_id),'[]'::jsonb)
      AS proxy_records,
    COALESCE((SELECT jsonb_agg(to_jsonb(assignment)-ARRAY['requested_at','decided_at'] ORDER BY assignment.id)
      FROM agent_cross_owner_assignments assignment
      JOIN agent_cross_owner_assignment_effects cross_effect
        ON cross_effect.project_id=assignment.project_id
       AND cross_effect.cross_owner_assignment_id=assignment.id
      JOIN agent_run_task_effects run_effect ON run_effect.project_id=cross_effect.project_id
       AND run_effect.cross_owner_effect_id=cross_effect.id AND run_effect.run_id=root.run_id
      WHERE assignment.project_id=root.project_id),'[]'::jsonb) AS cross_owner_records
  FROM agent_r541_release_roots root
  JOIN agent_collaborative_runs run ON run.project_id=root.project_id AND run.id=root.run_id
    AND run.goal_id=root.goal_id AND run.goal_status='completed' AND run.run_status='completed'
    AND run.contract->>'goal'=root.goal_id::text
  JOIN agent_run_transitions initialization ON initialization.id=root.initialization_transition_id
    AND initialization.project_id=root.project_id AND initialization.run_id=root.run_id
    AND initialization.transition_kind='initialized' AND initialization.state_version=1
    AND initialization.semantic_tick=root.start_tick
  JOIN agent_run_transitions completion ON completion.project_id=root.project_id
    AND completion.run_id=root.run_id AND completion.transition_kind='run_completed'
    AND completion.state_version=run.state_version AND completion.next_state_hash=run.state_hash
    AND completion.state_snapshot=run.state
  JOIN agent_run_exact_semantic_timelines timeline ON timeline.trace_number=root.trace_number
    AND timeline.project_id=root.project_id AND timeline.run_id=root.run_id
  JOIN agent_r540_exact_release_trace_certificates trace_certificate
    ON trace_certificate.trace_number=root.trace_number
    AND trace_certificate.project_id=root.project_id AND trace_certificate.run_id=root.run_id
    AND trace_certificate.goal_id=root.goal_id
  JOIN agent_r541_release_trace_surface_gates trace_gates
    ON trace_gates.project_id=root.project_id AND trace_gates.run_id=root.run_id
    AND trace_gates.trace_number=root.trace_number
  JOIN agent_r541_comment_surface_gates comment_gate
    ON comment_gate.project_id=root.project_id AND comment_gate.run_id=root.run_id
    AND comment_gate.trace_number=root.trace_number
  JOIN agent_local_goal_contracts local ON local.project_id=root.project_id
    AND local.id=run.local_goal_id AND local.revision=run.local_goal_revision
    AND local.state='active'
  JOIN agent_compilation_certificates compiler ON compiler.project_id=local.project_id
    AND compiler.id=local.compilation_certificate_id AND compiler.task_kind='local_goal'
    AND compiler.verification_state='verified' AND compiler.canonical_output->'contract'=run.contract
    AND jsonb_typeof(compiler.canonical_output->'requirements')='array'
    AND jsonb_typeof(compiler.canonical_output->'bindings')='array'
    AND jsonb_typeof(compiler.canonical_output->'security_policies')='array'
  JOIN governed_agents governed ON governed.project_id=root.project_id
    AND governed.id=local.agent_id AND governed.principal_identity_id=local.agent_identity_id
    AND governed.controller_identity_id=local.controller_identity_id AND governed.state='active'
  JOIN identities agent_identity ON agent_identity.id=local.agent_identity_id
    AND agent_identity.principal_kind='agent' AND agent_identity.status='active'
  JOIN identities controller_identity ON controller_identity.id=local.controller_identity_id
    AND controller_identity.principal_kind IN ('administrator','user')
    AND controller_identity.status='active'
  WHERE trace_certificate.end_tick<=timeline.last_tick
    AND jsonb_array_length(trace_certificate.work_attempt_inventory)>0
    AND jsonb_array_length(trace_certificate.work_attempt_inventory)
        =jsonb_array_length(trace_certificate.work_outcome_inventory)
    AND NOT EXISTS (SELECT 1 FROM jsonb_each(completion.state_snapshot->'work_items') work
      WHERE work.value->>'status' NOT IN ('succeeded','failed','cancelled'))
    AND NOT EXISTS (SELECT 1 FROM jsonb_each(completion.state_snapshot->'obligations') obligation
      WHERE obligation.value->>'status'<>'discharged')
    AND NOT EXISTS (SELECT 1 FROM jsonb_each(completion.state_snapshot->'blockers') blocker
      WHERE blocker.value->>'status' NOT IN ('resolved','failed','cancelled'))
    AND NOT EXISTS (SELECT 1 FROM agent_run_claim_leases claim
      WHERE claim.project_id=root.project_id AND claim.run_id=root.run_id AND claim.status='active')
    AND NOT EXISTS (SELECT 1 FROM agent_tool_calls call
      WHERE call.project_id=root.project_id AND call.run_id=root.run_id AND call.current_status='pending')
    AND NOT EXISTS (
      SELECT 1 FROM project_memberships membership
      JOIN identities identity ON identity.id=membership.identity_id
      LEFT JOIN user_proxies proxy ON proxy.project_id=membership.project_id
        AND proxy.user_identity_id=membership.identity_id
      WHERE membership.project_id=root.project_id AND membership.state='active'
        AND identity.status='active' AND identity.principal_kind IN ('administrator','user')
        AND proxy.id IS NULL)
    AND NOT EXISTS (
      SELECT 1 FROM agent_responsibility_contracts responsibility
      LEFT JOIN agent_compilation_certificates certificate
        ON certificate.project_id=responsibility.project_id
       AND certificate.id=responsibility.compilation_certificate_id
       AND certificate.task_kind='responsibility' AND certificate.verification_state='verified'
      WHERE responsibility.project_id=root.project_id AND responsibility.state='active'
        AND certificate.id IS NULL)
    AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(root.governance_start_snapshot->'agents') snapshot
      WHERE NOT EXISTS (SELECT 1 FROM governed_agents record
        WHERE record.project_id=root.project_id AND record.id=(snapshot->>'id')::uuid
          AND to_jsonb(record)-ARRAY['created_at','suspended_at','retired_at']=snapshot))
    AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(root.governance_start_snapshot->'local_goals') snapshot
      WHERE NOT EXISTS (SELECT 1 FROM agent_local_goal_contracts record
        WHERE record.project_id=root.project_id AND record.id=(snapshot->>'id')::uuid
          AND record.revision=(snapshot->>'revision')::bigint
          AND to_jsonb(record)-ARRAY['recorded_at','terminal_at']=snapshot))
    AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(root.governance_start_snapshot->'responsibilities') snapshot
      WHERE NOT EXISTS (SELECT 1 FROM agent_responsibility_contracts record
        WHERE record.project_id=root.project_id AND record.id=(snapshot->>'id')::uuid
          AND record.revision=(snapshot->>'revision')::bigint
          AND to_jsonb(record)-ARRAY['recorded_at','activated_at','superseded_at']=snapshot))
    AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(root.comment_start_snapshot) snapshot
      WHERE NOT EXISTS (SELECT 1 FROM native_comments comment
        WHERE comment.project_id=root.project_id AND comment.id=(snapshot->>'id')::uuid
          AND comment.encrypted_payload IS NOT NULL
          AND digest(comment.encrypted_payload,'sha256')=comment.payload_commitment
          AND jsonb_build_object('id',comment.id,'author',comment.author_identity_id,
            'recipient',comment.recipient_identity_id,'target',comment.target_resource_node_id,
            'parent',comment.parent_comment_id,'agent_depth',comment.agent_depth,
            'payload',sprout_private.try_parse_encrypted_payload(comment.encrypted_payload),
            'key_epoch',comment.key_epoch)=snapshot))
    AND sprout_private.jsonb_array_is_prefix(
      root.operational_start_snapshot->'proxy_transcripts',
      sprout_private.semantic_operational_state_snapshot(root.project_id,root.run_id)->'proxy_transcripts')
    AND sprout_private.jsonb_array_is_prefix(
      root.operational_start_snapshot->'proxy_audit',
      sprout_private.semantic_operational_state_snapshot(root.project_id,root.run_id)->'proxy_audit')
    AND sprout_private.jsonb_array_is_prefix(
      root.operational_start_snapshot->'task_provenance',
      sprout_private.semantic_operational_state_snapshot(root.project_id,root.run_id)->'task_provenance')
    AND sprout_private.jsonb_array_is_prefix(
      root.operational_start_snapshot->'task_intents',
      sprout_private.semantic_operational_state_snapshot(root.project_id,root.run_id)->'task_intents')
    AND jsonb_array_length(root.governance_start_snapshot->'history')<=jsonb_array_length(
      COALESCE((SELECT jsonb_agg(to_jsonb(entry)-'recorded_at' ORDER BY entry.position)
        FROM agent_governance_ledger entry WHERE entry.project_id=root.project_id),'[]'::jsonb))
    AND NOT EXISTS (
      SELECT 1 FROM jsonb_array_elements(root.governance_start_snapshot->'history')
        WITH ORDINALITY prior(value,ordinal)
      WHERE prior.value IS DISTINCT FROM (
        SELECT current.value
        FROM jsonb_array_elements(COALESCE((SELECT jsonb_agg(to_jsonb(entry)-'recorded_at'
          ORDER BY entry.position) FROM agent_governance_ledger entry
          WHERE entry.project_id=root.project_id),'[]'::jsonb)) WITH ORDINALITY current(value,ordinal)
        WHERE current.ordinal=prior.ordinal))
), fields(ordinal,root_field) AS (VALUES
 (1,'run_goal_exact'),(2,'trace_start_exact'),(3,'governed_run_exact'),
 (4,'secure_kernel'),(5,'governance_kernel'),(6,'concrete_trace'),
 (7,'trace_feature_gates'),(8,'compiler_action_exact'),(9,'security_policies_exact'),
 (10,'governance_operational'),(11,'local_revision_trace_bound'),
 (12,'creation_trace_bound'),(13,'responsibility_trace_bound'),
 (14,'global_trace_bound'),(15,'proxy_trace_bound'),(16,'cross_owner_trace_bound'),
 (17,'comments'),(18,'proxy'),(19,'global_inventory_exact'),(20,'global'),
 (21,'cross_owner'),(22,'interrogation'),(23,'model'),(24,'task_operational'),
 (25,'task_intent_trace_bound'),(26,'task_provenance_trace_bound'),
 (27,'operational_history'),(28,'operational_closure')
), records AS (
 SELECT base.trace_number,base.project_id,base.run_id,base.goal_id,base.start_tick,
   fields.ordinal,fields.root_field,
   jsonb_build_object(
    'field',fields.root_field,
    'identity',jsonb_build_object('trace_number',base.trace_number,'run',base.run_id,
      'goal',base.goal_id,'start_tick',base.start_tick),
    'root',jsonb_build_object('transition',base.initialization_transition_id,
      'root_hash',encode(base.root_hash,'hex'),'initial_state_hash',encode(base.initialization_state_hash,'hex')),
    'run',jsonb_build_object('contract',base.contract,'contract_hash',encode(base.contract_hash,'hex'),
      'state',base.state,'state_hash',encode(base.state_hash,'hex'),'state_version',base.state_version,
      'completion_transition',base.completion_transition_id,'completion_tick',base.completion_tick,
      'completion_state',base.completion_state,'completion_state_hash',encode(base.completion_state_hash,'hex')),
    'timeline',jsonb_build_object('last_tick',base.last_tick,'allocation_count',base.allocation_count,
      'cursor_hash',encode(base.cursor_hash,'hex')),
    'trace',jsonb_build_object('certificate',base.trace_certificate_id,
      'version',base.trace_certificate_version,'hash',encode(base.trace_certificate_hash,'hex'),
      'end_tick',base.trace_end_tick,'work_attempts',base.work_attempt_inventory,
      'work_outcomes',base.work_outcome_inventory,'blockers',base.blocker_inventory,
      'causal',base.causal_inventory,'tools',base.tool_inventory,'evidence',base.evidence_inventory,
      'disclosures',base.disclosure_inventory,'models',base.model_inventory,
      'interrogations',base.interrogation_inventory),
    'compiler',jsonb_build_object('certificate',base.compiler_certificate_id,
      'hash',encode(base.compiler_certificate_hash,'hex'),'output',base.compiler_output,
      'envelope',base.compilation_envelope),
    'governance',jsonb_build_object('local_goal',base.local_goal_id,
      'local_revision',base.local_goal_revision,'agent',base.agent_id,
      'agent_principal',base.agent_identity_id,'controller',base.controller_identity_id,
      'availability',base.agent_availability,'history',base.governance_history,
      'responsibilities',base.active_responsibilities,
      'administrator_creations',base.administrator_creations,'globals',base.global_records),
    'trace_feature_gates',jsonb_build_object(
      'outcome',jsonb_build_object('mode',base.outcome_mode,'records',base.outcome_records),
      'blocker',jsonb_build_object('mode',base.blocker_mode,'records',base.blocker_records),
      'causal',jsonb_build_object('mode',base.causal_mode,'records',base.causal_records),
      'tool',jsonb_build_object('mode',base.tool_mode,'records',base.tool_records),
      'evidence',jsonb_build_object('mode',base.evidence_mode,'records',base.evidence_records),
      'disclosure',jsonb_build_object('mode',base.disclosure_mode,'records',base.disclosure_records)),
    'comments',jsonb_build_object('mode',base.comment_mode,'records',base.comment_records,
      'typed_exact',base.exact_comment_records),
    'proxy',jsonb_build_object('directory',base.proxy_directory,'records',base.proxy_records),
    'global',base.global_records,'cross_owner',base.cross_owner_records,
    'model',jsonb_build_object('mode',base.model_mode,'records',base.model_records,
      'typed_exact',base.exact_model_records),
    'interrogation',jsonb_build_object('mode',base.interrogation_mode,
      'records',base.interrogation_records,'typed_exact',base.exact_interrogation_records),
    'task_operational',base.task_effect_records,
    'operational_history',base.operational_history,
    'operational_closure',jsonb_build_object('proxy_directory',base.proxy_directory,
      'language_tasks',jsonb_build_array(base.compilation_envelope->'language_task'),
      'runtime_boundary',jsonb_build_object('verification_state','verified')),
    'field_certificate',CASE fields.root_field
      WHEN 'run_goal_exact' THEN jsonb_build_object(
        'trace_run',base.run_id,'trace_goal',base.goal_id,
        'contract_goal',base.contract->>'goal')
      WHEN 'trace_start_exact' THEN jsonb_build_object(
        'trace_start_tick',base.start_tick,'initialization_transition',base.initialization_transition_id,
        'initialization_state_hash',encode(base.initialization_state_hash,'hex'))
      WHEN 'governed_run_exact' THEN jsonb_build_object(
        'initial_state',base.initialization_state,'terminal_state',base.completion_state,
        'terminal_state_hash',encode(base.completion_state_hash,'hex'))
      WHEN 'secure_kernel' THEN jsonb_build_object(
        'completion',jsonb_build_object('run_status',base.state->>'run_status',
          'goal_status',base.state->>'goal_status','work_attempts',base.work_attempt_inventory,
          'work_outcomes',base.work_outcome_inventory,'evidence',base.evidence_inventory,
          'completion_transition',base.completion_transition_id),
        'nested_certificates',base.secure_nested_records,
        'safety',jsonb_build_object('compiler_certificate',base.compiler_certificate_id,
          'security_policies',base.compiler_output->'security_policies',
          'tool_authority_effects',base.tool_security_records,
          'comment_authority_effects',base.comment_security_records,
          'task_control_effects',base.task_effect_records,
          'model_context_events',base.exact_model_records,
          'disclosure_events',base.disclosure_records))
      WHEN 'governance_kernel' THEN jsonb_build_object(
        'nested_certificates',base.governance_nested_records,
        'start',base.governance_start_snapshot,'histories_append_only',base.governance_history,
        'responsibilities',base.active_responsibilities,
        'agent',jsonb_build_object('id',base.agent_id,'principal',base.agent_identity_id,
          'controller',base.controller_identity_id,'availability',base.agent_availability),
        'exceptions',base.approved_exceptions,'assignments',base.global_assignments,
        'administrator_creations',base.administrator_creations,
        'prompt_local_consistency',jsonb_build_object('local_goal',base.local_goal_id,
          'revision',base.local_goal_revision,'compiler',base.compiler_certificate_id),
        'global_revision',base.global_records)
      WHEN 'concrete_trace' THEN jsonb_build_object(
        'certificate',base.trace_certificate_id,'certificate_hash',encode(base.trace_certificate_hash,'hex'),
        'end_tick',base.trace_end_tick,'work_attempts',base.work_attempt_inventory,
        'work_outcomes',base.work_outcome_inventory,'blockers',base.blocker_inventory,
        'causal',base.causal_inventory,'tools',base.tool_inventory,
        'evidence',base.evidence_inventory,'disclosures',base.disclosure_inventory,
        'models',base.model_inventory,'interrogations',base.interrogation_inventory)
      WHEN 'trace_feature_gates' THEN jsonb_build_object(
        'outcome',jsonb_build_object('mode',base.outcome_mode,'records',base.outcome_records),
        'blocker',jsonb_build_object('mode',base.blocker_mode,'records',base.blocker_records),
        'causal',jsonb_build_object('mode',base.causal_mode,'records',base.causal_records),
        'tool',jsonb_build_object('mode',base.tool_mode,'records',base.tool_records),
        'evidence',jsonb_build_object('mode',base.evidence_mode,'records',base.evidence_records),
        'disclosure',jsonb_build_object('mode',base.disclosure_mode,'records',base.disclosure_records))
      WHEN 'compiler_action_exact' THEN jsonb_build_object(
        'requirements',base.compiler_output->'requirements','bindings',base.compiler_output->'bindings',
        'contract_work_specs',base.contract->'work_specs','compiler_envelope',base.compilation_envelope)
      WHEN 'security_policies_exact' THEN jsonb_build_object(
        'work_specs',base.contract->'work_specs','policies',base.compiler_output->'security_policies')
      WHEN 'governance_operational' THEN jsonb_build_object(
        'local_revisions',jsonb_path_query_array(sprout_private.jsonb_array_suffix(
          base.governance_history,jsonb_array_length(base.governance_start_snapshot->'history')),
          '$[*] ? (@.entry_kind == "local_goal_revision")'),
        'creations',jsonb_path_query_array(sprout_private.jsonb_array_suffix(
          base.governance_history,jsonb_array_length(base.governance_start_snapshot->'history')),
          '$[*] ? (@.entry_kind == "administrator_creation_approval")'),
        'responsibility_records',jsonb_path_query_array(sprout_private.jsonb_array_suffix(
          base.governance_history,jsonb_array_length(base.governance_start_snapshot->'history')),
          '$[*] ? (@.entry_kind == "responsibility_revision")'),
        'global_records',jsonb_path_query_array(sprout_private.jsonb_array_suffix(
          base.governance_history,jsonb_array_length(base.governance_start_snapshot->'history')),
          '$[*] ? (@.entry_kind == "global_agent_proposal")'),
        'authoritative_governance_history',base.governance_history)
      WHEN 'local_revision_trace_bound' THEN jsonb_build_object('records',jsonb_path_query_array(
        sprout_private.jsonb_array_suffix(base.governance_history,
          jsonb_array_length(base.governance_start_snapshot->'history')),
        '$[*] ? (@.entry_kind == "local_goal_revision")'),'trace',base.trace_number)
      WHEN 'creation_trace_bound' THEN jsonb_build_object('records',jsonb_path_query_array(
        sprout_private.jsonb_array_suffix(base.governance_history,
          jsonb_array_length(base.governance_start_snapshot->'history')),
        '$[*] ? (@.entry_kind == "administrator_creation_approval")'),'trace',base.trace_number)
      WHEN 'responsibility_trace_bound' THEN jsonb_build_object('records',jsonb_path_query_array(
        sprout_private.jsonb_array_suffix(base.governance_history,
          jsonb_array_length(base.governance_start_snapshot->'history')),
        '$[*] ? (@.entry_kind == "responsibility_revision")'),'trace',base.trace_number)
      WHEN 'global_trace_bound' THEN jsonb_build_object('records',jsonb_path_query_array(
        sprout_private.jsonb_array_suffix(base.governance_history,
          jsonb_array_length(base.governance_start_snapshot->'history')),
        '$[*] ? (@.entry_kind == "global_agent_proposal")'),'trace',base.trace_number)
      WHEN 'proxy_trace_bound' THEN jsonb_build_object('records',base.proxy_records,'trace',base.trace_number)
      WHEN 'cross_owner_trace_bound' THEN jsonb_build_object('records',base.cross_owner_records,'trace',base.trace_number)
      WHEN 'comments' THEN jsonb_build_object('mode',base.comment_mode,
        'inventory',base.comment_records,'typed_exact',base.exact_comment_records,
        'start_comments',base.comment_start_snapshot,
        'end_comments',base.comment_end_snapshot,
        'append_only',sprout_private.jsonb_array_is_prefix(
          base.comment_start_snapshot,base.comment_end_snapshot),
        'semantic_state_source','agent_native_comment_run_semantic_states')
      WHEN 'proxy' THEN jsonb_build_object('records',base.proxy_records,
        'directory',base.proxy_directory,'authority','user_principal_only')
      WHEN 'global_inventory_exact' THEN jsonb_build_object('gate_records',base.global_records,
        'operational_records',base.global_records)
      WHEN 'global' THEN jsonb_build_object('records',base.global_records,
        'responsibilities',base.active_responsibilities)
      WHEN 'cross_owner' THEN jsonb_build_object('records',base.cross_owner_records,
        'authority_source','product_authorization_and_responsibility_or_review')
      WHEN 'interrogation' THEN jsonb_build_object('mode',base.interrogation_mode,
        'trace_records',base.interrogation_records,'typed_runtime_records',base.exact_interrogation_records)
      WHEN 'model' THEN jsonb_build_object('mode',base.model_mode,
        'trace_records',base.model_records,'typed_runtime_records',base.exact_model_records,
        'hidden_persistent_model_memory',false)
      WHEN 'task_operational' THEN jsonb_build_object('effects',base.task_effect_records,
        'intent_records',base.operational_history->'task_intents',
        'provenance_records',base.operational_history->'task_provenance')
      WHEN 'task_intent_trace_bound' THEN jsonb_build_object(
        'records',base.operational_history->'task_intents','trace',base.trace_number)
      WHEN 'task_provenance_trace_bound' THEN jsonb_build_object(
        'records',base.operational_history->'task_provenance','trace',base.trace_number)
      WHEN 'operational_history' THEN jsonb_build_object(
        'start',base.operational_start_snapshot,'after',base.operational_history,
        'proxy_transcripts_prefix',sprout_private.jsonb_array_is_prefix(
          base.operational_start_snapshot->'proxy_transcripts',base.operational_history->'proxy_transcripts'),
        'proxy_audit_prefix',sprout_private.jsonb_array_is_prefix(
          base.operational_start_snapshot->'proxy_audit',base.operational_history->'proxy_audit'),
        'task_provenance_prefix',sprout_private.jsonb_array_is_prefix(
          base.operational_start_snapshot->'task_provenance',base.operational_history->'task_provenance'),
        'task_intents_prefix',sprout_private.jsonb_array_is_prefix(
          base.operational_start_snapshot->'task_intents',base.operational_history->'task_intents'))
      WHEN 'operational_closure' THEN jsonb_build_object(
        'proxy_directory_start',base.proxy_directory_start_snapshot,
        'proxy_directory',base.proxy_directory,
        'language_tasks',jsonb_build_array(base.compilation_envelope->'language_task'),
        'runtime_boundary',jsonb_build_object(
          'terminal_records',jsonb_build_array(jsonb_build_object(
            'task',base.compilation_envelope->'language_task',
            'status','schema_valid_success','certificate',base.compiler_certificate_id,
            'certificate_hash',encode(base.compiler_certificate_hash,'hex'))),
          'failure_source_relation','agent_model_attempt_observations'))
    END
   ) AS formal_record
 FROM exact_base base CROSS JOIN fields
)
SELECT records.*,digest(convert_to(records.formal_record::text,'UTF8'),'sha256') AS source_hash
FROM records;

-- Field-specific sources are deliberately separate objects.  The union is a
-- routing convenience only; it cannot turn one child's record into another
-- because each view fixes both its Lean field and source relation.
CREATE VIEW agent_r541_exact_child_run_goal_exact AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='run_goal_exact' AND formal_record#>>'{field_certificate,trace_run}'=run_id::text
   AND formal_record#>>'{field_certificate,trace_goal}'=goal_id::text
   AND formal_record#>>'{field_certificate,contract_goal}'=goal_id::text;
CREATE VIEW agent_r541_exact_child_trace_start_exact AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='trace_start_exact'
   AND (formal_record#>>'{field_certificate,trace_start_tick}')::bigint=start_tick
   AND formal_record#>>'{field_certificate,initialization_transition}'=
       formal_record#>>'{root,transition}';
CREATE VIEW agent_r541_exact_child_governed_run_exact AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='governed_run_exact'
   AND formal_record#>'{field_certificate,terminal_state}'=formal_record#>'{run,state}'
   AND formal_record#>>'{run,completion_state_hash}'=formal_record#>>'{field_certificate,terminal_state_hash}';
CREATE VIEW agent_r541_exact_child_secure_kernel AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='secure_kernel'
   AND jsonb_array_length(formal_record#>'{field_certificate,completion,work_attempts}')>0
   AND jsonb_array_length(formal_record#>'{field_certificate,completion,work_attempts}')
       =jsonb_array_length(formal_record#>'{field_certificate,completion,work_outcomes}')
   AND jsonb_array_length(formal_record#>'{field_certificate,nested_certificates}')=41
   AND (SELECT count(DISTINCT nested->>'field') FROM jsonb_array_elements(
       formal_record#>'{field_certificate,nested_certificates}') nested)=41
   AND NOT EXISTS (SELECT 1 FROM (VALUES
      ('progress.base'),('progress.completionCommit'),('progress.dynamics'),
      ('progress.measureLaws'),('progress.goalValidityPersistsUntilTerminal'),
      ('progress.goalValidAtStart'),
      ('evidence_discharge.dischargeSound'),
      ('evidence_discharge.acceptedEvidenceCloses'),
      ('evidence_discharge.completionCommit'),
      ('authority_information.sponsorIsHuman'),
      ('authority_information.runAuthorityBoundedAtStart'),
      ('authority_information.runToolAuthorityBoundedAtStart'),
      ('authority_information.workOriginSound'),
      ('authority_information.humanDelegationAuthorityBound'),
      ('authority_information.humanDelegationToolAuthorityBound'),
      ('authority_information.childWorkAuthorityAttenuates'),
      ('authority_information.childWorkToolAuthorityAttenuates'),
      ('authority_information.agentMoveHasSecurityEffect'),
      ('authority_information.coreAgentActionFootprintComplete'),
      ('authority_information.coreAgentActionAllowedByContract'),
      ('authority_information.securityPolicyBoundToContract'),
      ('authority_information.effectWorkCertified'),
      ('authority_information.effectWorkSemanticallyEnabled'),
      ('authority_information.effectSecurityPolicyAllowed'),
      ('authority_information.effectWorkOwned'),
      ('authority_information.humanAssignedTaskControlIsolation'),
      ('authority_information.effectAuthoritySafe'),
      ('authority_information.toolUseAuthoritySafe'),
      ('authority_information.toolInvocationMatchesCall'),
      ('authority_information.toolFootprintComplete'),
      ('authority_information.effectWithinRunScope'),
      ('authority_information.modelContextProjectionExact'),
      ('authority_information.modelContextAuthoritySafe'),
      ('authority_information.modelContextWithinRunScope'),
      ('authority_information.infoContextContainerValid'),
      ('authority_information.toolContextSourceOwned'),
      ('authority_information.disclosureFootprintSound'),
      ('authority_information.canonicalResourceBody'),
      ('authority_information.contextualChatActionSafe'),
      ('authority_information.disclosureContextSafe'),
      ('authority_information.persistedDisclosureContextSafe')) required(field)
     WHERE NOT EXISTS (SELECT 1 FROM jsonb_array_elements(
       formal_record#>'{field_certificate,nested_certificates}') nested
       WHERE nested->>'field'=required.field))
   AND NOT EXISTS (SELECT 1 FROM agent_r540_exact_release_events event
     LEFT JOIN agent_r540_typed_exact_release_events typed ON typed.id=event.id
     WHERE event.trace_number=source.trace_number AND typed.id IS NULL)
   AND NOT EXISTS (SELECT 1 FROM agent_tool_calls call
     WHERE call.project_id=source.project_id AND call.run_id=source.run_id
       AND (call.work_authority_principal_id IS NULL OR call.owner_identity_id IS NULL
         OR NOT EXISTS (SELECT 1 FROM agent_run_tool_security_snapshots snapshot
           WHERE snapshot.project_id=call.project_id AND snapshot.run_id=call.run_id
             AND snapshot.run_tool_ceiling_hash=call.run_tool_ceiling_hash
             AND call.work_tool_ceiling <@ snapshot.run_tool_ceiling
             AND EXISTS (SELECT 1 FROM jsonb_array_elements(snapshot.work_policies) policy
               WHERE (policy->>'work_spec_id')::bigint=call.work_spec_ordinal
                 AND policy->'allowed_tools'=call.work_tool_ceiling))))
   AND NOT EXISTS (SELECT 1 FROM agent_run_task_effects effect
     LEFT JOIN agent_run_claim_leases claim ON claim.project_id=effect.project_id
       AND claim.id=effect.claim_id AND claim.run_id=effect.run_id
       AND claim.work_item_id=effect.work_item_id AND claim.attempt=effect.attempt
       AND claim.claimant_identity_id=effect.actor_identity_id
     WHERE effect.project_id=source.project_id AND effect.run_id=source.run_id
       AND claim.id IS NULL)
   AND NOT EXISTS (SELECT 1 FROM agent_native_comment_security_effects effect
     LEFT JOIN native_comments comment ON comment.project_id=effect.project_id
       AND comment.id=effect.comment_id AND comment.run_id=effect.run_id
       AND comment.work_item_id=effect.work_item_id AND comment.claim_id=effect.claim_id
       AND comment.attempt=effect.attempt AND comment.author_identity_id=effect.actor_identity_id
       AND comment.target_resource_node_id=effect.target_resource_node_id
       AND comment.payload_commitment=effect.payload_commitment
     WHERE effect.project_id=source.project_id AND effect.run_id=source.run_id
       AND comment.id IS NULL);
CREATE VIEW agent_r541_exact_child_governance_kernel AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='governance_kernel'
   AND sprout_private.jsonb_array_is_prefix(formal_record#>'{field_certificate,start,history}',
       formal_record#>'{field_certificate,histories_append_only}')
   AND jsonb_typeof(formal_record#>'{field_certificate,responsibilities}')='array'
   AND jsonb_typeof(formal_record#>'{field_certificate,exceptions}')='array'
   AND jsonb_typeof(formal_record#>'{field_certificate,assignments}')='array'
   AND jsonb_typeof(formal_record#>'{field_certificate,administrator_creations}')='array'
   AND jsonb_array_length(formal_record#>'{field_certificate,nested_certificates}')=8
   AND NOT EXISTS (SELECT 1 FROM (VALUES ('histories_append_only'),('responsibilities'),
      ('agents'),('exceptions'),('assignments'),('administrator_creations'),
      ('prompt_local_consistency'),('global_revision')) required(field)
     WHERE NOT EXISTS (SELECT 1 FROM jsonb_array_elements(
       formal_record#>'{field_certificate,nested_certificates}') nested
       WHERE nested->>'field'=required.field))
   AND NOT EXISTS (SELECT 1 FROM governed_agents agent
     LEFT JOIN identities principal ON principal.id=agent.principal_identity_id
       AND principal.principal_kind='agent' AND principal.status='active'
     LEFT JOIN identities controller ON controller.id=agent.controller_identity_id
       AND controller.principal_kind IN ('administrator','user') AND controller.status='active'
     WHERE agent.project_id=source.project_id AND agent.state='active'
       AND (principal.id IS NULL OR controller.id IS NULL))
   AND NOT EXISTS (SELECT 1 FROM agent_responsibility_contracts responsibility
     LEFT JOIN identities administrator ON administrator.id=responsibility.administrator_identity_id
       AND administrator.principal_kind='administrator' AND administrator.status='active'
     LEFT JOIN agent_compilation_certificates compiler ON compiler.project_id=responsibility.project_id
       AND compiler.id=responsibility.compilation_certificate_id
       AND compiler.task_kind='responsibility' AND compiler.verification_state='verified'
       AND compiler.canonical_output->'contract'=responsibility.contract
     WHERE responsibility.project_id=source.project_id AND responsibility.state='active'
       AND (administrator.id IS NULL OR compiler.id IS NULL))
   AND NOT EXISTS (SELECT 1 FROM agent_administrator_creation_approvals approval
     LEFT JOIN identities administrator ON administrator.id=approval.administrator_identity_id
       AND administrator.principal_kind='administrator' AND administrator.status='active'
     LEFT JOIN governed_agents agent ON agent.project_id=approval.project_id
       AND agent.id=approval.governed_agent_id
       AND agent.principal_identity_id=approval.proposed_agent_identity_id
     LEFT JOIN agent_local_goal_contracts local ON local.project_id=approval.project_id
       AND local.id=approval.local_goal_id AND local.revision=approval.local_goal_revision
       AND local.agent_id=approval.governed_agent_id AND local.contract_hash=approval.contract_hash
     WHERE approval.project_id=source.project_id
       AND (administrator.id IS NULL OR agent.id IS NULL OR local.id IS NULL))
   AND NOT EXISTS (SELECT 1 FROM agent_global_contracts global
     WHERE global.project_id=source.project_id
       AND ((global.synthesis_invocation_id IS NOT NULL AND NOT EXISTS (
         SELECT 1 FROM agent_model_invocation_projections projection
         WHERE projection.project_id=global.project_id
           AND projection.invocation_id=global.synthesis_invocation_id
           AND projection.status='succeeded'
           AND projection.language_task->>'kind'='synthesize_global_contract'))
       OR NOT EXISTS (SELECT 1 FROM agent_global_contract_sources grounding
         WHERE grounding.project_id=global.project_id
           AND grounding.global_contract_id=global.id
           AND grounding.global_revision=global.revision)));
CREATE VIEW agent_r541_exact_child_concrete_trace AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='concrete_trace' AND EXISTS (SELECT 1 FROM agent_r540_exact_release_trace_certificates exact
   WHERE exact.id=(formal_record#>>'{field_certificate,certificate}')::uuid
     AND exact.trace_number=source.trace_number AND exact.run_id=source.run_id
     AND exact.project_id=source.project_id AND exact.goal_id=source.goal_id)
   AND NOT EXISTS (SELECT 1 FROM agent_effect_proposals effect
     JOIN agent_invocations invocation ON invocation.project_id=effect.project_id
       AND invocation.id=effect.invocation_id AND invocation.run_id=source.run_id
     WHERE effect.project_id=source.project_id AND effect.status='applied'
       AND NOT EXISTS (SELECT 1 FROM agent_r540_typed_exact_release_events typed
         WHERE typed.trace_number=source.trace_number AND typed.event_kind='disclosure'
           AND typed.source_record_id=effect.id))
   AND NOT EXISTS (SELECT 1 FROM agent_native_comment_security_effects effect
     WHERE effect.project_id=source.project_id AND effect.run_id=source.run_id
       AND NOT EXISTS (SELECT 1 FROM agent_r540_typed_exact_release_events typed
         WHERE typed.trace_number=source.trace_number AND typed.event_kind='disclosure'
           AND typed.source_record_id=effect.id));
CREATE VIEW agent_r541_exact_child_trace_feature_gates AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='trace_feature_gates'
   AND formal_record#>'{field_certificate,outcome,records}'=formal_record#>'{trace,work_outcomes}'
   AND formal_record#>'{field_certificate,blocker,records}'=formal_record#>'{trace,blockers}'
   AND formal_record#>'{field_certificate,causal,records}'=formal_record#>'{trace,causal}'
   AND formal_record#>'{field_certificate,tool,records}'=formal_record#>'{trace,tools}'
   AND formal_record#>'{field_certificate,evidence,records}'=formal_record#>'{trace,evidence}'
   AND formal_record#>'{field_certificate,disclosure,records}'=formal_record#>'{trace,disclosures}';
CREATE VIEW agent_r541_exact_child_compiler_action_exact AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='compiler_action_exact'
   AND jsonb_typeof(formal_record#>'{field_certificate,requirements}')='array'
   AND jsonb_typeof(formal_record#>'{field_certificate,bindings}')='array'
   AND jsonb_typeof(formal_record#>'{field_certificate,contract_work_specs}')='array'
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,requirements}') requirement
     WHERE NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,bindings}') binding
       WHERE binding->>'requirement_id'=requirement->>'id'))
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,bindings}') binding
     WHERE NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,requirements}') requirement
       WHERE requirement->>'id'=binding->>'requirement_id')
       OR NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,contract_work_specs}') work_spec
         WHERE work_spec->>'id'=binding->>'work_spec_id' AND work_spec->>'obligation'=binding->>'obligation'))
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,contract_work_specs}') work_spec
     WHERE NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,bindings}') binding
       WHERE binding->>'work_spec_id'=work_spec->>'id'));
CREATE VIEW agent_r541_exact_child_security_policies_exact AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='security_policies_exact'
   AND jsonb_typeof(formal_record#>'{field_certificate,policies}')='array'
   AND jsonb_array_length(formal_record#>'{field_certificate,policies}')
       =jsonb_array_length(formal_record#>'{field_certificate,work_specs}')
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,work_specs}') work_spec
     WHERE (SELECT count(*) FROM jsonb_array_elements(formal_record#>'{field_certificate,policies}') policy
       WHERE policy->>'work_spec_id'=work_spec->>'id')<>1
       OR NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,policies}') policy
         WHERE policy->>'work_spec_id'=work_spec->>'id'
           AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements_text(policy->'allowed_operations') operation
             WHERE NOT EXISTS (SELECT 1 FROM jsonb_array_elements_text(work_spec->'allowed_actions') action
               WHERE operation=CASE action
                 WHEN 'create_task' THEN 'write'
                 WHEN 'replace_own_task' THEN 'write'
                 WHEN 'delete_own_task' THEN 'write'
                 WHEN 'assign_own_task' THEN 'delegate_assigned_work'
                 WHEN 'unassign_own_task' THEN 'delegate_assigned_work'
                 WHEN 'mark_assigned_done' THEN 'complete_assigned_task'
                 WHEN 'append_assigned_note' THEN 'write'
                 WHEN 'add_assigned_attachment' THEN 'write'
                 WHEN 'post_comment' THEN 'post_comment'
                 ELSE NULL END))));
CREATE VIEW agent_r541_exact_child_governance_operational AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='governance_operational'
   AND jsonb_typeof(formal_record#>'{field_certificate,local_revisions}')='array'
   AND jsonb_typeof(formal_record#>'{field_certificate,creations}')='array'
   AND jsonb_typeof(formal_record#>'{field_certificate,responsibility_records}')='array'
   AND jsonb_typeof(formal_record#>'{field_certificate,global_records}')='array'
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,local_revisions}') record
     WHERE NOT EXISTS (SELECT 1 FROM agent_local_goal_contracts local
       JOIN agent_compilation_certificates compiler ON compiler.project_id=local.project_id
        AND compiler.id=local.compilation_certificate_id AND compiler.verification_state='verified'
        AND compiler.canonical_output->'contract'=local.contract->'contract'
       WHERE local.project_id=source.project_id AND local.id=(record->>'subject_id')::uuid
         AND local.revision=(record->>'subject_revision')::bigint))
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,creations}') record
     WHERE NOT EXISTS (SELECT 1 FROM agent_administrator_creation_approvals approval
       WHERE approval.project_id=source.project_id AND approval.approval_id=(record->>'entry_id')::uuid))
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,responsibility_records}') record
     WHERE NOT EXISTS (SELECT 1 FROM agent_responsibility_contracts responsibility
       JOIN agent_compilation_certificates compiler ON compiler.project_id=responsibility.project_id
        AND compiler.id=responsibility.compilation_certificate_id
        AND compiler.verification_state='verified' AND compiler.canonical_output->'contract'=responsibility.contract
       WHERE responsibility.project_id=source.project_id
         AND responsibility.id=(record->>'subject_id')::uuid
         AND responsibility.revision=(record->>'subject_revision')::bigint))
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,global_records}') record
     WHERE NOT EXISTS (SELECT 1 FROM agent_governance_authorization_events event
       WHERE event.project_id=source.project_id AND event.event_id=(record->>'entry_id')::uuid
         AND event.event_kind='global_agent_proposal'));
CREATE VIEW agent_r541_exact_child_local_revision_trace_bound AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='local_revision_trace_bound'
   AND formal_record#>>'{field_certificate,trace}'=trace_number::text;
CREATE VIEW agent_r541_exact_child_creation_trace_bound AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='creation_trace_bound'
   AND formal_record#>>'{field_certificate,trace}'=trace_number::text;
CREATE VIEW agent_r541_exact_child_responsibility_trace_bound AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='responsibility_trace_bound'
   AND formal_record#>>'{field_certificate,trace}'=trace_number::text;
CREATE VIEW agent_r541_exact_child_global_trace_bound AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='global_trace_bound'
   AND formal_record#>>'{field_certificate,trace}'=trace_number::text;
CREATE VIEW agent_r541_exact_child_proxy_trace_bound AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='proxy_trace_bound' AND formal_record#>>'{field_certificate,trace}'=trace_number::text;
CREATE VIEW agent_r541_exact_child_cross_owner_trace_bound AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='cross_owner_trace_bound' AND formal_record#>>'{field_certificate,trace}'=trace_number::text;
CREATE VIEW agent_r541_exact_child_comments AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='comments'
   AND formal_record#>>'{field_certificate,append_only}'='true'
   AND ((formal_record#>>'{field_certificate,mode}'='enabled'
      AND jsonb_array_length(formal_record#>'{field_certificate,typed_exact}')>0)
    OR (formal_record#>>'{field_certificate,mode}'='disabled_fail_closed'
      AND formal_record#>'{field_certificate,typed_exact}'='[]'::jsonb
      AND NOT EXISTS (SELECT 1 FROM native_comment_events event
        WHERE event.project_id=source.project_id AND event.run_id=source.run_id
          AND event.trace_number=source.trace_number)))
   AND NOT EXISTS (SELECT 1 FROM native_comment_events event
     LEFT JOIN agent_r541_typed_exact_comment_records exact
       ON exact.project_id=event.project_id AND exact.run_id=event.run_id
      AND exact.comment_event_id=event.id AND exact.trace_number=event.trace_number
     WHERE event.project_id=source.project_id AND event.run_id=source.run_id
       AND exact.comment_id IS NULL);
CREATE VIEW agent_r541_exact_child_proxy AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='proxy' AND NOT EXISTS (SELECT 1 FROM project_memberships membership
   JOIN identities identity ON identity.id=membership.identity_id
   LEFT JOIN user_proxies proxy ON proxy.project_id=membership.project_id
     AND proxy.user_identity_id=membership.identity_id
   WHERE membership.project_id=source.project_id AND membership.state='active'
     AND identity.status='active' AND identity.principal_kind IN ('administrator','user')
     AND proxy.id IS NULL)
   AND NOT EXISTS (SELECT 1 FROM user_proxy_plans plan
     JOIN agent_invocations invocation ON invocation.project_id=plan.project_id
       AND invocation.id=plan.invocation_id AND invocation.run_id=source.run_id
     LEFT JOIN user_proxy_requests request ON request.project_id=plan.project_id
       AND request.id=plan.request_id
     LEFT JOIN user_proxy_threads thread ON thread.project_id=request.project_id
       AND thread.id=request.thread_id AND thread.creator_identity_id=request.user_identity_id
     LEFT JOIN user_proxies proxy ON proxy.project_id=thread.project_id
       AND proxy.id=thread.proxy_id AND proxy.user_identity_id=request.user_identity_id
     WHERE plan.project_id=source.project_id
       AND (request.id IS NULL OR thread.id IS NULL OR proxy.id IS NULL
         OR plan.planning_envelope#>>'{request_id}' IS DISTINCT FROM request.id::text
         OR plan.planning_envelope#>>'{user}' IS DISTINCT FROM request.user_identity_id::text
         OR plan.action_plan#>>'{request_id}' IS DISTINCT FROM request.id::text
         OR plan.action_plan#>>'{thread_id}' IS DISTINCT FROM thread.id::text
         OR plan.action_plan#>>'{user}' IS DISTINCT FROM request.user_identity_id::text
         OR (plan.responsibility_id IS NULL AND plan.confirmation IS NULL)));
CREATE VIEW agent_r541_exact_child_global_inventory_exact AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='global_inventory_exact'
   AND formal_record#>'{field_certificate,gate_records}'=formal_record#>'{field_certificate,operational_records}';
CREATE VIEW agent_r541_exact_child_global AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='global' AND jsonb_typeof(formal_record#>'{field_certificate,records}')='array'
   AND NOT EXISTS (SELECT 1 FROM agent_global_contracts global
     JOIN agent_invocations invocation ON invocation.project_id=global.project_id
       AND invocation.id=global.synthesis_invocation_id AND invocation.run_id=source.run_id
     WHERE global.project_id=source.project_id
       AND (invocation.language_task->>'kind' IS DISTINCT FROM 'synthesize_global_contract'
         OR NOT EXISTS (SELECT 1 FROM agent_global_contract_sources grounding
           JOIN agent_local_goal_contracts local ON local.project_id=grounding.project_id
            AND local.id=grounding.local_goal_id AND local.revision=grounding.local_revision
            AND local.agent_id=grounding.agent_id
           WHERE grounding.project_id=global.project_id
             AND grounding.global_contract_id=global.id
             AND grounding.global_revision=global.revision)));
CREATE VIEW agent_r541_exact_child_cross_owner AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='cross_owner' AND jsonb_typeof(formal_record#>'{field_certificate,records}')='array'
   AND NOT EXISTS (SELECT 1 FROM agent_run_task_effects run_effect
     JOIN agent_cross_owner_assignment_effects cross_effect
       ON cross_effect.project_id=run_effect.project_id AND cross_effect.id=run_effect.cross_owner_effect_id
     JOIN agent_cross_owner_assignments assignment
       ON assignment.project_id=cross_effect.project_id
      AND assignment.id=cross_effect.cross_owner_assignment_id
     WHERE run_effect.project_id=source.project_id AND run_effect.run_id=source.run_id
       AND NOT ((assignment.route='automatic_existing_obligation' AND assignment.state='ready'
          AND run_effect.task_provenance_id IS NOT NULL)
        OR (assignment.route='controller_review' AND assignment.decision='approved'
          AND assignment.state='ready')));
CREATE VIEW agent_r541_exact_child_interrogation AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='interrogation'
   AND jsonb_array_length(formal_record#>'{field_certificate,trace_records}')
       =jsonb_array_length(formal_record#>'{field_certificate,typed_runtime_records}')
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,trace_records}') item
     WHERE NOT EXISTS (SELECT 1 FROM agent_r540_typed_exact_release_events typed
       WHERE typed.id=(item->>'event_id')::uuid AND typed.trace_number=source.trace_number
         AND typed.event_kind='interrogation'))
   AND NOT EXISTS (SELECT 1 FROM agent_invocations invocation
     JOIN agent_interrogation_answers answer ON answer.interrogation_id=invocation.interrogation_id
     WHERE invocation.project_id=source.project_id AND invocation.run_id=source.run_id
       AND invocation.invocation_surface='interrogation'
       AND NOT EXISTS (SELECT 1 FROM agent_r540_typed_exact_release_events typed
         WHERE typed.trace_number=source.trace_number AND typed.event_kind='interrogation'
           AND typed.source_record_id=answer.id));
CREATE VIEW agent_r541_exact_child_model AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='model'
   AND jsonb_array_length(formal_record#>'{field_certificate,trace_records}')
       =jsonb_array_length(formal_record#>'{field_certificate,typed_runtime_records}')
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,trace_records}') item
     WHERE NOT EXISTS (SELECT 1 FROM agent_r540_typed_exact_release_events typed
       WHERE typed.id=(item->>'event_id')::uuid AND typed.trace_number=source.trace_number
         AND typed.event_kind='model_invocation'))
   AND NOT EXISTS (SELECT 1 FROM agent_model_invocation_projections projection
     WHERE projection.project_id=source.project_id AND projection.run_id=source.run_id
       AND NOT EXISTS (SELECT 1 FROM agent_r540_typed_exact_release_events typed
         WHERE typed.trace_number=source.trace_number AND typed.event_kind='model_invocation'
           AND typed.source_record_id=projection.id))
   AND formal_record#>>'{field_certificate,hidden_persistent_model_memory}'='false';
CREATE VIEW agent_r541_exact_child_task_operational AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='task_operational'
   AND jsonb_typeof(formal_record#>'{field_certificate,intent_records}')='array'
   AND jsonb_typeof(formal_record#>'{field_certificate,provenance_records}')='array'
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,intent_records}') item
     WHERE item#>>'{envelope,task,kind}' IS DISTINCT FROM 'derive_task_intent'
       OR item#>>'{intent,task}' IS NULL OR item#>>'{intent,scope}' IS NULL
       OR item#>'{intent,required_actions}' IS NULL OR item#>>'{intent,created_by}' IS NULL
       OR item#>>'{intent,recorded_at}' IS NULL
       OR item#>'{intent,required_actions}'<>item#>'{envelope,allowed_actions}')
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,provenance_records}') item
     WHERE item#>>'{provenance,task}' IS NULL OR item#>>'{provenance,agent}' IS NULL
       OR item#>>'{provenance,local_revision}' IS NULL OR item#>>'{provenance,obligation}' IS NULL
       OR item#>>'{provenance,work_spec_id}' IS NULL OR item#>>'{provenance,recorded_at}' IS NULL)
   AND NOT EXISTS (SELECT 1 FROM agent_run_task_effects effect
     LEFT JOIN agent_r541_task_operational_bindings binding
       ON binding.project_id=effect.project_id AND binding.run_id=effect.run_id
      AND binding.task_effect_id=effect.id AND binding.trace_number=source.trace_number
     WHERE effect.project_id=source.project_id AND effect.run_id=source.run_id
       AND binding.id IS NULL);
CREATE VIEW agent_r541_exact_child_task_intent_trace_bound AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='task_intent_trace_bound' AND formal_record#>>'{field_certificate,trace}'=trace_number::text
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,records}') item
     WHERE (item->>'trace_number')::bigint IS DISTINCT FROM trace_number);
CREATE VIEW agent_r541_exact_child_task_provenance_trace_bound AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='task_provenance_trace_bound' AND formal_record#>>'{field_certificate,trace}'=trace_number::text
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,records}') item
     WHERE (item->>'trace_number')::bigint IS DISTINCT FROM trace_number);
CREATE VIEW agent_r541_exact_child_operational_history AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='operational_history'
   AND formal_record#>>'{field_certificate,proxy_transcripts_prefix}'='true'
   AND formal_record#>>'{field_certificate,proxy_audit_prefix}'='true'
   AND formal_record#>>'{field_certificate,task_provenance_prefix}'='true'
   AND formal_record#>>'{field_certificate,task_intents_prefix}'='true';
CREATE VIEW agent_r541_exact_child_operational_closure AS
 SELECT * FROM agent_r541_exact_formal_release_source_snapshots source
 WHERE root_field='operational_closure'
   AND formal_record#>'{field_certificate,proxy_directory_start}'=formal_record#>'{field_certificate,proxy_directory}'
   AND NOT EXISTS (SELECT 1 FROM user_proxies proxy
     LEFT JOIN project_memberships membership ON membership.project_id=proxy.project_id
       AND membership.identity_id=proxy.user_identity_id AND membership.state='active'
     LEFT JOIN identities identity ON identity.id=proxy.user_identity_id
       AND identity.status='active' AND identity.principal_kind IN ('administrator','user')
     WHERE proxy.project_id=source.project_id AND (membership.identity_id IS NULL OR identity.id IS NULL))
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(
       formal_record#>'{operational_history,proxy_transcripts}') transcript,
       LATERAL jsonb_array_elements(transcript->'messages') message
     WHERE message->'payload' IS NULL OR message->'payload'='null'::jsonb)
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(formal_record#>'{field_certificate,language_tasks}') task
     WHERE (task->>'input_item_count')::bigint>(task->>'max_input_items')::bigint
       OR (task->>'max_output_items')::bigint<=0 OR (task->>'max_nesting_depth')::bigint<=0
       OR (task->>'max_attempts')::bigint<=0 OR task->>'closed_output_schema'<>'true'
       OR task->>'grounded_identifiers_only'<>'true'
       OR task->>'requires_formal_proof'<>'false'
       OR task->>'requires_permission_decision'<>'false'
       OR task->>'requires_exact_semantic_equivalence'<>'false'
       OR task->>'requires_exhaustive_world_knowledge'<>'false')
   AND jsonb_array_length(formal_record#>'{field_certificate,language_tasks}')
       =jsonb_array_length(formal_record#>'{field_certificate,runtime_boundary,terminal_records}')
   AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(
       formal_record#>'{field_certificate,runtime_boundary,terminal_records}') terminal
     WHERE terminal->>'status' NOT IN ('schema_valid_success','explicit_failure')
       OR terminal->'task' IS NULL OR terminal->>'certificate' IS NULL);

CREATE VIEW agent_r541_field_specific_release_child_sources AS
SELECT *, 'agent_r541_exact_child_run_goal_exact'::text AS source_relation FROM agent_r541_exact_child_run_goal_exact UNION ALL
SELECT *, 'agent_r541_exact_child_trace_start_exact' FROM agent_r541_exact_child_trace_start_exact UNION ALL
SELECT *, 'agent_r541_exact_child_governed_run_exact' FROM agent_r541_exact_child_governed_run_exact UNION ALL
SELECT *, 'agent_r541_exact_child_secure_kernel' FROM agent_r541_exact_child_secure_kernel UNION ALL
SELECT *, 'agent_r541_exact_child_governance_kernel' FROM agent_r541_exact_child_governance_kernel UNION ALL
SELECT *, 'agent_r541_exact_child_concrete_trace' FROM agent_r541_exact_child_concrete_trace UNION ALL
SELECT *, 'agent_r541_exact_child_trace_feature_gates' FROM agent_r541_exact_child_trace_feature_gates UNION ALL
SELECT *, 'agent_r541_exact_child_compiler_action_exact' FROM agent_r541_exact_child_compiler_action_exact UNION ALL
SELECT *, 'agent_r541_exact_child_security_policies_exact' FROM agent_r541_exact_child_security_policies_exact UNION ALL
SELECT *, 'agent_r541_exact_child_governance_operational' FROM agent_r541_exact_child_governance_operational UNION ALL
SELECT *, 'agent_r541_exact_child_local_revision_trace_bound' FROM agent_r541_exact_child_local_revision_trace_bound UNION ALL
SELECT *, 'agent_r541_exact_child_creation_trace_bound' FROM agent_r541_exact_child_creation_trace_bound UNION ALL
SELECT *, 'agent_r541_exact_child_responsibility_trace_bound' FROM agent_r541_exact_child_responsibility_trace_bound UNION ALL
SELECT *, 'agent_r541_exact_child_global_trace_bound' FROM agent_r541_exact_child_global_trace_bound UNION ALL
SELECT *, 'agent_r541_exact_child_proxy_trace_bound' FROM agent_r541_exact_child_proxy_trace_bound UNION ALL
SELECT *, 'agent_r541_exact_child_cross_owner_trace_bound' FROM agent_r541_exact_child_cross_owner_trace_bound UNION ALL
SELECT *, 'agent_r541_exact_child_comments' FROM agent_r541_exact_child_comments UNION ALL
SELECT *, 'agent_r541_exact_child_proxy' FROM agent_r541_exact_child_proxy UNION ALL
SELECT *, 'agent_r541_exact_child_global_inventory_exact' FROM agent_r541_exact_child_global_inventory_exact UNION ALL
SELECT *, 'agent_r541_exact_child_global' FROM agent_r541_exact_child_global UNION ALL
SELECT *, 'agent_r541_exact_child_cross_owner' FROM agent_r541_exact_child_cross_owner UNION ALL
SELECT *, 'agent_r541_exact_child_interrogation' FROM agent_r541_exact_child_interrogation UNION ALL
SELECT *, 'agent_r541_exact_child_model' FROM agent_r541_exact_child_model UNION ALL
SELECT *, 'agent_r541_exact_child_task_operational' FROM agent_r541_exact_child_task_operational UNION ALL
SELECT *, 'agent_r541_exact_child_task_intent_trace_bound' FROM agent_r541_exact_child_task_intent_trace_bound UNION ALL
SELECT *, 'agent_r541_exact_child_task_provenance_trace_bound' FROM agent_r541_exact_child_task_provenance_trace_bound UNION ALL
SELECT *, 'agent_r541_exact_child_operational_history' FROM agent_r541_exact_child_operational_history UNION ALL
SELECT *, 'agent_r541_exact_child_operational_closure' FROM agent_r541_exact_child_operational_closure;

-- Reconstruct one canonical child at a time.  Keeping the dispatch whitelist
-- here avoids a PostgreSQL plan containing 28 expanded copies of the complete
-- run reconstruction while preserving independent field-specific predicates.
CREATE FUNCTION sprout_private.reconstruct_agent_r541_release_child(
  candidate_trace_number bigint,candidate_root_field text
) RETURNS TABLE(
  trace_number bigint,project_id uuid,run_id uuid,goal_id uuid,start_tick bigint,
  ordinal integer,root_field text,formal_record jsonb,source_hash bytea,source_relation text
) LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off
SET join_collapse_limit=1 SET from_collapse_limit=1 SET jit=off AS $$
DECLARE exact_relation text;
BEGIN
 exact_relation:=CASE candidate_root_field
  WHEN 'run_goal_exact' THEN 'agent_r541_exact_child_run_goal_exact'
  WHEN 'trace_start_exact' THEN 'agent_r541_exact_child_trace_start_exact'
  WHEN 'governed_run_exact' THEN 'agent_r541_exact_child_governed_run_exact'
  WHEN 'secure_kernel' THEN 'agent_r541_exact_child_secure_kernel'
  WHEN 'governance_kernel' THEN 'agent_r541_exact_child_governance_kernel'
  WHEN 'concrete_trace' THEN 'agent_r541_exact_child_concrete_trace'
  WHEN 'trace_feature_gates' THEN 'agent_r541_exact_child_trace_feature_gates'
  WHEN 'compiler_action_exact' THEN 'agent_r541_exact_child_compiler_action_exact'
  WHEN 'security_policies_exact' THEN 'agent_r541_exact_child_security_policies_exact'
  WHEN 'governance_operational' THEN 'agent_r541_exact_child_governance_operational'
  WHEN 'local_revision_trace_bound' THEN 'agent_r541_exact_child_local_revision_trace_bound'
  WHEN 'creation_trace_bound' THEN 'agent_r541_exact_child_creation_trace_bound'
  WHEN 'responsibility_trace_bound' THEN 'agent_r541_exact_child_responsibility_trace_bound'
  WHEN 'global_trace_bound' THEN 'agent_r541_exact_child_global_trace_bound'
  WHEN 'proxy_trace_bound' THEN 'agent_r541_exact_child_proxy_trace_bound'
  WHEN 'cross_owner_trace_bound' THEN 'agent_r541_exact_child_cross_owner_trace_bound'
  WHEN 'comments' THEN 'agent_r541_exact_child_comments'
  WHEN 'proxy' THEN 'agent_r541_exact_child_proxy'
  WHEN 'global_inventory_exact' THEN 'agent_r541_exact_child_global_inventory_exact'
  WHEN 'global' THEN 'agent_r541_exact_child_global'
  WHEN 'cross_owner' THEN 'agent_r541_exact_child_cross_owner'
  WHEN 'interrogation' THEN 'agent_r541_exact_child_interrogation'
  WHEN 'model' THEN 'agent_r541_exact_child_model'
  WHEN 'task_operational' THEN 'agent_r541_exact_child_task_operational'
  WHEN 'task_intent_trace_bound' THEN 'agent_r541_exact_child_task_intent_trace_bound'
  WHEN 'task_provenance_trace_bound' THEN 'agent_r541_exact_child_task_provenance_trace_bound'
  WHEN 'operational_history' THEN 'agent_r541_exact_child_operational_history'
  WHEN 'operational_closure' THEN 'agent_r541_exact_child_operational_closure'
  ELSE NULL END;
 IF exact_relation IS NULL THEN RETURN; END IF;
 RETURN QUERY EXECUTE format(
   'SELECT source.trace_number,source.project_id,source.run_id,source.goal_id,'
   'source.start_tick,source.ordinal,source.root_field,source.formal_record,'
   'source.source_hash,%L::text FROM public.%I source '
   'WHERE source.trace_number=$1 AND source.root_field=%L',
   exact_relation,exact_relation,candidate_root_field)
  USING candidate_trace_number;
END $$;
REVOKE ALL ON FUNCTION sprout_private.reconstruct_agent_r541_release_child(bigint,text) FROM PUBLIC;

CREATE VIEW agent_r541_exact_formal_release_child_certificates AS
SELECT child.*
FROM agent_r541_formal_release_child_certificates child
JOIN LATERAL sprout_private.reconstruct_agent_r541_release_child(
  child.trace_number,child.root_field) source
  ON source.trace_number=child.trace_number AND source.project_id=child.project_id
 AND source.run_id=child.run_id AND source.goal_id=child.goal_id
 AND source.start_tick=child.start_tick AND source.root_field=child.root_field
 AND source.source_relation=child.source_relation
 AND source.formal_record=child.formal_record AND source.source_hash=child.source_hash
WHERE child.certificate_hash=digest(convert_to(concat_ws(E'\n',
  'sprout-r541-formal-release-child-v1',child.trace_number::text,
  child.project_id::text,child.run_id::text,child.goal_id::text,child.start_tick::text,
  child.release_version::text,child.root_field,child.source_relation,
  encode(child.source_hash,'hex')),'UTF8'),'sha256');

CREATE VIEW agent_r541_exact_formal_release_certificates AS
WITH actual AS (
 SELECT certificate.trace_number,certificate.version,
   jsonb_agg(jsonb_build_object('ordinal',inventory.ordinal,'field',inventory.root_field,
     'child_certificate_id',inventory.child_certificate_id,
     'child_certificate_hash',encode(inventory.child_certificate_hash,'hex'))
     ORDER BY inventory.ordinal) AS inventory,
   count(*) AS child_count,
   count(exact.id) AS exact_child_count,
   count(DISTINCT inventory.root_field) AS distinct_field_count,
   count(DISTINCT inventory.child_certificate_id) AS distinct_child_count,
   min(inventory.ordinal) AS first_ordinal,max(inventory.ordinal) AS last_ordinal
 FROM agent_r541_formal_release_certificates certificate
 JOIN agent_r541_formal_release_inventory inventory
   ON inventory.trace_number=certificate.trace_number
  AND inventory.release_version=certificate.version
 LEFT JOIN agent_r541_exact_formal_release_child_certificates exact
   ON exact.id=inventory.child_certificate_id
  AND exact.certificate_hash=inventory.child_certificate_hash
 GROUP BY certificate.trace_number,certificate.version
)
SELECT certificate.*
FROM agent_r541_formal_release_certificates certificate
JOIN actual ON actual.trace_number=certificate.trace_number AND actual.version=certificate.version
LEFT JOIN agent_r541_formal_release_certificates previous
  ON previous.trace_number=certificate.trace_number AND previous.version=certificate.version-1
WHERE actual.child_count=28 AND actual.exact_child_count=28
 AND actual.distinct_field_count=28 AND actual.distinct_child_count=28
 AND actual.first_ordinal=1 AND actual.last_ordinal=28
 AND certificate.child_inventory=actual.inventory
 AND certificate.child_inventory_commitment=digest(convert_to(actual.inventory::text,'UTF8'),'sha256')
 AND ((certificate.version=1 AND certificate.previous_certificate_hash IS NULL AND previous.id IS NULL)
   OR (certificate.version>1 AND previous.certificate_hash=certificate.previous_certificate_hash))
 AND certificate.certificate_hash=digest(convert_to(concat_ws(E'\n',
   'sprout-r541-formal-release-root-v1',certificate.trace_number::text,
   certificate.project_id::text,certificate.run_id::text,certificate.goal_id::text,
   certificate.start_tick::text,certificate.version::text,
   encode(certificate.child_inventory_commitment,'hex'),
   COALESCE(encode(certificate.previous_certificate_hash,'hex'),'')),'UTF8'),'sha256')
 AND EXISTS (SELECT 1 FROM agent_r540_exact_release_trace_certificates trace
   WHERE trace.trace_number=certificate.trace_number
     AND jsonb_array_length(trace.work_attempt_inventory)>0);

CREATE FUNCTION sprout_private.issue_agent_r541_formal_release(candidate_trace_number bigint)
RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE root_row public.agent_r541_release_roots%ROWTYPE;
 previous_row public.agent_r541_formal_release_certificates%ROWTYPE;
 expected_row record; source_row record;
 next_version integer; source_count integer; candidate_id uuid;
 reconstructed_sources jsonb:='[]'::jsonb;
 inventory jsonb; inventory_hash bytea; candidate_hash bytea;
BEGIN
 SELECT * INTO STRICT root_row FROM public.agent_r541_release_roots
   WHERE trace_number=candidate_trace_number FOR UPDATE;
 -- Opportunistic issuance is invoked by several semantic mutation paths.  A
 -- non-terminal run cannot satisfy progress/completion or the exact trace
 -- child, so fail closed before expanding the 28 field-specific views.
 IF NOT EXISTS (
   SELECT 1
   FROM public.agent_collaborative_runs run
   JOIN public.agent_r540_exact_release_trace_certificates trace
     ON trace.trace_number=root_row.trace_number
    AND trace.project_id=root_row.project_id
    AND trace.run_id=root_row.run_id
    AND trace.goal_id=root_row.goal_id
   WHERE run.project_id=root_row.project_id AND run.id=root_row.run_id
     AND run.goal_id=root_row.goal_id
     AND run.run_status='completed' AND run.goal_status='completed'
     AND jsonb_array_length(trace.work_attempt_inventory)>0
 ) THEN
   RETURN NULL;
 END IF;
 FOR expected_row IN SELECT * FROM (VALUES
   (1,'run_goal_exact'),(2,'trace_start_exact'),(3,'governed_run_exact'),
   (4,'secure_kernel'),(5,'governance_kernel'),(6,'concrete_trace'),
   (7,'trace_feature_gates'),(8,'compiler_action_exact'),(9,'security_policies_exact'),
   (10,'governance_operational'),(11,'local_revision_trace_bound'),
   (12,'creation_trace_bound'),(13,'responsibility_trace_bound'),
   (14,'global_trace_bound'),(15,'proxy_trace_bound'),(16,'cross_owner_trace_bound'),
   (17,'comments'),(18,'proxy'),(19,'global_inventory_exact'),(20,'global'),
   (21,'cross_owner'),(22,'interrogation'),(23,'model'),(24,'task_operational'),
   (25,'task_intent_trace_bound'),(26,'task_provenance_trace_bound'),
   (27,'operational_history'),(28,'operational_closure'))
   AS expected(ordinal,root_field) ORDER BY ordinal
 LOOP
  SELECT * INTO source_row FROM sprout_private.reconstruct_agent_r541_release_child(
    candidate_trace_number,expected_row.root_field);
  IF NOT FOUND OR source_row.ordinal<>expected_row.ordinal
    OR source_row.root_field<>expected_row.root_field
    OR source_row.source_relation<>'agent_r541_exact_child_'||expected_row.root_field THEN
    RETURN NULL;
  END IF;
  reconstructed_sources:=reconstructed_sources||jsonb_build_array(jsonb_build_object(
    'trace_number',source_row.trace_number,'project_id',source_row.project_id,
    'run_id',source_row.run_id,'goal_id',source_row.goal_id,'start_tick',source_row.start_tick,
    'ordinal',source_row.ordinal,'root_field',source_row.root_field,
    'formal_record',source_row.formal_record,'source_hash',encode(source_row.source_hash,'hex'),
    'source_relation',source_row.source_relation));
 END LOOP;
 SELECT count(*),count(DISTINCT source->>'root_field') INTO source_count,next_version
 FROM jsonb_array_elements(reconstructed_sources) source;
 IF source_count<>28 OR next_version<>28 THEN RETURN NULL; END IF;
 SELECT * INTO previous_row FROM public.agent_r541_formal_release_certificates
   WHERE trace_number=candidate_trace_number ORDER BY version DESC LIMIT 1;
 IF FOUND AND (
   SELECT count(*)=28 AND count(DISTINCT child.root_field)=28
     AND count(DISTINCT child.id)=28
   FROM public.agent_r541_formal_release_child_certificates child
   JOIN LATERAL jsonb_array_elements(reconstructed_sources) source
     ON (source->>'trace_number')::bigint=child.trace_number
    AND source->>'root_field'=child.root_field
    AND source->>'source_relation'=child.source_relation
    AND decode(source->>'source_hash','hex')=child.source_hash
    AND source->'formal_record'=child.formal_record
   WHERE child.trace_number=candidate_trace_number
     AND child.release_version=previous_row.version
 ) THEN
   RETURN previous_row.id;
 END IF;
 next_version:=COALESCE(previous_row.version,0)+1;
 INSERT INTO public.agent_r541_formal_release_child_certificates(
   id,trace_number,project_id,run_id,goal_id,start_tick,release_version,root_field,source_relation,
   formal_record,source_hash,certificate_hash)
 SELECT gen_random_uuid(),(source->>'trace_number')::bigint,(source->>'project_id')::uuid,
   (source->>'run_id')::uuid,(source->>'goal_id')::uuid,(source->>'start_tick')::bigint,
   next_version,source->>'root_field',source->>'source_relation',source->'formal_record',
   decode(source->>'source_hash','hex'),
   public.digest(pg_catalog.convert_to(concat_ws(E'\n','sprout-r541-formal-release-child-v1',
     source->>'trace_number',source->>'project_id',source->>'run_id',source->>'goal_id',
     source->>'start_tick',next_version::text,source->>'root_field',
     source->>'source_relation',source->>'source_hash'),'UTF8'),'sha256')
 FROM jsonb_array_elements(reconstructed_sources) source
 ORDER BY (source->>'ordinal')::integer;
 INSERT INTO public.agent_r541_formal_release_inventory(
   trace_number,project_id,release_version,ordinal,root_field,
   child_certificate_id,child_certificate_hash)
 SELECT child.trace_number,child.project_id,child.release_version,(source->>'ordinal')::integer,
   child.root_field,child.id,child.certificate_hash
 FROM public.agent_r541_formal_release_child_certificates child
 JOIN LATERAL jsonb_array_elements(reconstructed_sources) source
   ON (source->>'trace_number')::bigint=child.trace_number
  AND source->>'root_field'=child.root_field
 WHERE child.trace_number=candidate_trace_number AND child.release_version=next_version
 ORDER BY (source->>'ordinal')::integer;
 SELECT jsonb_agg(jsonb_build_object('ordinal',item.ordinal,'field',item.root_field,
     'child_certificate_id',item.child_certificate_id,
     'child_certificate_hash',encode(item.child_certificate_hash,'hex')) ORDER BY item.ordinal)
 INTO STRICT inventory FROM public.agent_r541_formal_release_inventory item
 WHERE item.trace_number=candidate_trace_number AND item.release_version=next_version;
 inventory_hash:=public.digest(pg_catalog.convert_to(inventory::text,'UTF8'),'sha256');
 candidate_id:=gen_random_uuid();
 candidate_hash:=public.digest(pg_catalog.convert_to(concat_ws(E'\n',
   'sprout-r541-formal-release-root-v1',root_row.trace_number::text,
   root_row.project_id::text,root_row.run_id::text,root_row.goal_id::text,
   root_row.start_tick::text,next_version::text,encode(inventory_hash,'hex'),
   COALESCE(encode(previous_row.certificate_hash,'hex'),'')),'UTF8'),'sha256');
 INSERT INTO public.agent_r541_formal_release_certificates(
   id,trace_number,project_id,run_id,goal_id,start_tick,version,child_inventory,
   child_inventory_commitment,previous_certificate_hash,certificate_hash)
 VALUES(candidate_id,root_row.trace_number,root_row.project_id,root_row.run_id,
   root_row.goal_id,root_row.start_tick,next_version,inventory,inventory_hash,
   previous_row.certificate_hash,candidate_hash);
 RETURN candidate_id;
END $$;

-- Root issuance is lifecycle-driven, not tied to evidence acceptance.  Every
-- path that can supply the final exact child retries the same idempotent
-- issuer; an incomplete composition returns NULL and writes nothing.
CREATE FUNCTION sprout_private.try_issue_agent_r541_formal_release()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
DECLARE candidate_trace bigint;
BEGIN
 IF TG_TABLE_NAME IN ('agent_r540_release_certificates','agent_r541_comment_certificates',
                      'agent_r541_task_operational_bindings') THEN
   candidate_trace:=NEW.trace_number;
 ELSIF TG_TABLE_NAME='agent_run_transitions' THEN
   IF NEW.transition_kind<>'run_completed' THEN RETURN NEW; END IF;
   SELECT root.trace_number INTO candidate_trace FROM public.agent_r541_release_roots root
    WHERE root.project_id=NEW.project_id AND root.run_id=NEW.run_id;
 END IF;
 IF candidate_trace IS NOT NULL THEN
   PERFORM sprout_private.issue_agent_r541_formal_release(candidate_trace);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER agent_r541_issue_after_trace_certificate
 AFTER INSERT ON agent_r540_release_certificates FOR EACH ROW
 EXECUTE FUNCTION sprout_private.try_issue_agent_r541_formal_release();
CREATE TRIGGER agent_r541_issue_after_comment_certificate
 AFTER INSERT ON agent_r541_comment_certificates FOR EACH ROW
 EXECUTE FUNCTION sprout_private.try_issue_agent_r541_formal_release();
CREATE TRIGGER agent_r541_issue_after_task_operational_binding
 AFTER INSERT ON agent_r541_task_operational_bindings FOR EACH ROW
 EXECUTE FUNCTION sprout_private.try_issue_agent_r541_formal_release();
CREATE TRIGGER agent_r541_issue_after_run_completion
 AFTER INSERT ON agent_run_transitions FOR EACH ROW
 EXECUTE FUNCTION sprout_private.try_issue_agent_r541_formal_release();
REVOKE ALL ON FUNCTION sprout_private.try_issue_agent_r541_formal_release() FROM PUBLIC;

CREATE FUNCTION sprout_private.reject_agent_r541_formal_release_composition_mutation()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog SET row_security=off AS $$
BEGIN RAISE EXCEPTION 'R541 formal release composition is append-only' USING ERRCODE='55000'; END $$;
CREATE TRIGGER agent_r541_release_children_immutable BEFORE UPDATE OR DELETE
 ON agent_r541_formal_release_child_certificates FOR EACH ROW
 EXECUTE FUNCTION sprout_private.reject_agent_r541_formal_release_composition_mutation();
CREATE TRIGGER agent_r541_release_inventory_immutable BEFORE UPDATE OR DELETE
 ON agent_r541_formal_release_inventory FOR EACH ROW
 EXECUTE FUNCTION sprout_private.reject_agent_r541_formal_release_composition_mutation();
CREATE TRIGGER agent_r541_release_certificates_immutable BEFORE UPDATE OR DELETE
 ON agent_r541_formal_release_certificates FOR EACH ROW
 EXECUTE FUNCTION sprout_private.reject_agent_r541_formal_release_composition_mutation();

ALTER TABLE agent_r541_formal_release_child_certificates ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_formal_release_child_certificates FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_formal_release_inventory ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_formal_release_inventory FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_formal_release_certificates ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r541_formal_release_certificates FORCE ROW LEVEL SECURITY;
CREATE POLICY agent_r541_release_children_read ON agent_r541_formal_release_child_certificates
 FOR SELECT USING (sprout_private.agent_run_access(project_id,run_id));
CREATE POLICY agent_r541_release_inventory_read ON agent_r541_formal_release_inventory
 FOR SELECT USING (EXISTS (SELECT 1 FROM agent_r541_release_roots root
   WHERE root.trace_number=agent_r541_formal_release_inventory.trace_number
     AND sprout_private.agent_run_access(root.project_id,root.run_id)));
CREATE POLICY agent_r541_release_certificates_read ON agent_r541_formal_release_certificates
 FOR SELECT USING (sprout_private.agent_run_access(project_id,run_id));
REVOKE ALL ON TABLE agent_r541_formal_release_child_certificates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_formal_release_inventory FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_formal_release_certificates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_formal_release_source_snapshots FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_run_goal_exact FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_trace_start_exact FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_governed_run_exact FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_secure_kernel FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_governance_kernel FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_concrete_trace FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_trace_feature_gates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_compiler_action_exact FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_security_policies_exact FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_governance_operational FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_local_revision_trace_bound FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_creation_trace_bound FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_responsibility_trace_bound FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_global_trace_bound FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_proxy_trace_bound FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_cross_owner_trace_bound FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_comments FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_proxy FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_global_inventory_exact FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_global FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_cross_owner FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_interrogation FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_model FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_task_operational FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_task_intent_trace_bound FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_task_provenance_trace_bound FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_operational_history FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_child_operational_closure FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_task_operational_bindings FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_task_operational_bindings FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_field_specific_release_child_sources FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_formal_release_child_certificates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_exact_formal_release_certificates FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.issue_agent_r541_formal_release(bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.jsonb_array_is_prefix(jsonb,jsonb) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.jsonb_array_suffix(jsonb,integer) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_r541_formal_release_composition_mutation() FROM PUBLIC;
