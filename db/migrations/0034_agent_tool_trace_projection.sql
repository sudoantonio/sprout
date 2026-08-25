-- Exact R5.40 external-tool cluster projection and list-exact R5.41 gates.
--
-- A trace_number is the positive, server-owned identity of one 0034-native run.
-- Existing runs receive neither a root nor synthetic events/certificates.

ALTER TABLE agent_run_transitions
    ADD COLUMN semantic_tick bigint CHECK (semantic_tick IS NULL OR semantic_tick >= 0);

ALTER TABLE agent_run_transitions
    DROP CONSTRAINT agent_run_transitions_transition_kind_check;
ALTER TABLE agent_run_transitions
    ADD CONSTRAINT agent_run_transitions_transition_kind_check CHECK (transition_kind IN (
        'initialized', 'frontier_refreshed', 'work_claimed',
        'claim_recovered', 'work_succeeded', 'work_failed',
        'tool_attempt_opened', 'tool_retry_rearmed',
        'blocker_created', 'blocker_resolved', 'evidence_accepted',
        'goal_completed', 'run_completed'
    ));

-- The 0033 append-only trigger is shared by heterogeneous row types. Keep its
-- retention exceptions exact, but branch on the relation before referencing
-- table-specific fields so envelope DELETE cannot resolve observation columns.
CREATE OR REPLACE FUNCTION sprout_private.reject_agent_tool_runtime_history_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF TG_TABLE_NAME = 'agent_tool_attempt_observations' THEN
        IF TG_OP = 'UPDATE'
           AND OLD.payload_purged_at IS NULL
           AND NEW.payload_purged_at IS NOT NULL
           AND NEW.encrypted_output IS NULL
           AND (to_jsonb(OLD) - ARRAY['encrypted_output','payload_purged_at'])
               = (to_jsonb(NEW) - ARRAY['encrypted_output','payload_purged_at'])
           AND EXISTS (
              SELECT 1
              FROM public.agent_tool_calls call
              JOIN public.governed_agents agent
                ON agent.project_id=call.project_id
               AND agent.principal_identity_id=call.owner_identity_id
              WHERE call.id=OLD.call_id
                AND NULLIF(current_setting('app.agent_retention_resource_id', true), '')::uuid
                      = agent.profile_resource_node_id
                AND sprout_private.retention_purge_row_allowed(jsonb_build_object(
                      'project_id', agent.project_id,
                      'resource_node_id', agent.profile_resource_node_id
                    ))
           ) THEN
            RETURN NEW;
        END IF;
    ELSIF TG_TABLE_NAME = 'agent_tool_output_key_envelopes' THEN
        IF TG_OP = 'DELETE' AND EXISTS (
            SELECT 1
            FROM public.agent_tool_calls call
            JOIN public.governed_agents agent
              ON agent.project_id = call.project_id
             AND agent.principal_identity_id = call.owner_identity_id
            WHERE call.id = OLD.call_id
              AND NULLIF(current_setting('app.agent_retention_resource_id', true), '')::uuid
                    = agent.profile_resource_node_id
              AND sprout_private.retention_purge_row_allowed(jsonb_build_object(
                    'project_id', agent.project_id,
                    'resource_node_id', agent.profile_resource_node_id
                  ))
        ) THEN
            RETURN OLD;
        END IF;
    END IF;
    RAISE EXCEPTION 'agent tool runtime history is append-only' USING ERRCODE = '55000';
END;
$$;

-- 0033 tool history is intentionally retained after encrypted payload purge,
-- while the older agent/kernel retention pipeline removes its live run,
-- work, claim, runner and governed-agent parents. Insert-time trusted
-- validators already prove these exact bindings. Remove only the historical
-- FKs to retention-purgeable parents so immutable IDs/hashes survive without
-- turning deleted operational state back into live authority.
ALTER TABLE agent_run_tool_security_snapshots
    DROP CONSTRAINT agent_run_tool_snapshot_run_fk;
ALTER TABLE agent_tool_runtime_capability_witnesses
    DROP CONSTRAINT agent_tool_runtime_witness_agent_fk,
    DROP CONSTRAINT agent_tool_runtime_witness_runner_fk;
ALTER TABLE agent_tool_calls
    DROP CONSTRAINT agent_tool_call_run_fk,
    DROP CONSTRAINT agent_tool_call_work_fk,
    DROP CONSTRAINT agent_tool_call_claim_fk;
ALTER TABLE agent_tool_attempt_dispatches
    DROP CONSTRAINT agent_tool_dispatch_runner_fk;
ALTER TABLE agent_run_external_tool_work_outcomes
    DROP CONSTRAINT agent_run_tool_outcome_work_fk,
    DROP CONSTRAINT agent_run_tool_outcome_claim_fk,
    DROP CONSTRAINT agent_run_tool_outcome_transition_fk;

CREATE TABLE agent_r540_tool_trace_roots (
    trace_number bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY CHECK (trace_number > 0),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    start_tick bigint NOT NULL CHECK (start_tick >= 0),
    initialization_transition_id uuid NOT NULL,
    root_hash bytea NOT NULL CHECK (octet_length(root_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    -- Live run/transition rows are retention-purgeable. The root keeps their
    -- exact immutable identities and hash without blocking that lifecycle;
    -- exact views fail closed when the operational provenance is absent.
    UNIQUE (project_id, run_id),
    UNIQUE (trace_number, project_id)
);

CREATE TABLE agent_r540_work_attempt_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    trace_number bigint NOT NULL,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    claim_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    actor_identity_id uuid NOT NULL,
    tick bigint NOT NULL CHECK (tick >= 0),
    transition_id uuid NOT NULL,
    work_snapshot jsonb NOT NULL CHECK (jsonb_typeof(work_snapshot) = 'object'),
    claim_snapshot jsonb NOT NULL CHECK (jsonb_typeof(claim_snapshot) = 'object'),
    event_hash bytea NOT NULL CHECK (octet_length(event_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number, project_id) REFERENCES agent_r540_tool_trace_roots (trace_number, project_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    -- Work slots, claims and transitions are exact-joined by the certificate
    -- views but deliberately are not FK parents: retention may purge them.
    UNIQUE (trace_number, work_item_id, claim_id, attempt),
    UNIQUE (transition_id),
    UNIQUE (trace_number, event_hash)
);

CREATE TABLE agent_r540_tool_attempt_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    trace_number bigint NOT NULL,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    claim_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    owner_identity_id uuid NOT NULL,
    call_id uuid NOT NULL,
    tool_name text NOT NULL,
    tool_version integer NOT NULL CHECK (tool_version > 0),
    canonical_input_commitment bytea NOT NULL CHECK (octet_length(canonical_input_commitment) = 32),
    phase text NOT NULL CHECK (phase IN ('pending', 'terminal')),
    status text NOT NULL CHECK (status IN ('pending', 'succeeded', 'failed', 'timed_out')),
    canonical_output_commitment bytea CHECK (
        canonical_output_commitment IS NULL OR octet_length(canonical_output_commitment) = 32
    ),
    requested_tick bigint NOT NULL CHECK (requested_tick >= 0),
    observed_tick bigint NOT NULL CHECK (observed_tick >= requested_tick),
    tool_deadline_tick bigint NOT NULL CHECK (tool_deadline_tick > requested_tick),
    work_attempt_event_id uuid NOT NULL,
    transition_id uuid NOT NULL,
    observation_id uuid,
    terminal_origin text CHECK (terminal_origin IN ('signed_edge_observation', 'server_timeout')),
    dispatch_id uuid,
    request_id uuid,
    wire_request_commitment bytea CHECK (
        wire_request_commitment IS NULL OR octet_length(wire_request_commitment) = 32
    ),
    execution_profile_commitment bytea CHECK (
        execution_profile_commitment IS NULL OR octet_length(execution_profile_commitment) = 32
    ),
    output_readable_by jsonb NOT NULL CHECK (jsonb_typeof(output_readable_by) = 'array'),
    call_snapshot jsonb NOT NULL CHECK (jsonb_typeof(call_snapshot) = 'object'),
    event_hash bytea NOT NULL CHECK (octet_length(event_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number, project_id) REFERENCES agent_r540_tool_trace_roots (trace_number, project_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (work_attempt_event_id) REFERENCES agent_r540_work_attempt_events (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    -- Operational call/transition/observation rows remain independent,
    -- purgeable authorities that exact views must still join when present.
    CONSTRAINT agent_r540_tool_event_shape CHECK (
        (phase = 'pending' AND status = 'pending' AND observation_id IS NULL
         AND terminal_origin IS NULL AND canonical_output_commitment IS NULL
         AND observed_tick = requested_tick)
        OR
        (phase = 'terminal' AND status <> 'pending' AND observation_id IS NOT NULL
         AND terminal_origin IS NOT NULL
         AND ((status = 'succeeded' AND canonical_output_commitment IS NOT NULL)
              OR (status IN ('failed', 'timed_out') AND canonical_output_commitment IS NULL)))
    ),
    UNIQUE (trace_number, call_id, attempt, phase),
    UNIQUE (trace_number, event_hash)
);

CREATE TABLE agent_r540_work_outcome_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    trace_number bigint NOT NULL,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    claim_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    status text NOT NULL CHECK (status IN ('succeeded', 'failed', 'cancelled')),
    observed_tick bigint NOT NULL CHECK (observed_tick >= 0),
    work_attempt_event_id uuid NOT NULL,
    tool_event_id uuid NOT NULL,
    observation_id uuid NOT NULL,
    operational_outcome_id uuid NOT NULL,
    transition_id uuid NOT NULL,
    state_snapshot jsonb NOT NULL CHECK (jsonb_typeof(state_snapshot) = 'object'),
    event_hash bytea NOT NULL CHECK (octet_length(event_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number, project_id) REFERENCES agent_r540_tool_trace_roots (trace_number, project_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (work_attempt_event_id) REFERENCES agent_r540_work_attempt_events (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (tool_event_id) REFERENCES agent_r540_tool_attempt_events (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    -- Operational outcome/observation/transition rows are exact-joined rather
    -- than FK-owned so retention can remove them and force gate disablement.
    UNIQUE (trace_number, work_item_id, claim_id, attempt),
    UNIQUE (trace_number, observation_id),
    UNIQUE (trace_number, event_hash)
);

-- A single ordinal space gives the tool cluster a canonical total order.
-- Exactly one typed event FK is populated in every immutable inventory row.
CREATE TABLE agent_r540_tool_trace_inventory (
    trace_number bigint NOT NULL,
    project_id uuid NOT NULL,
    ordinal bigint NOT NULL CHECK (ordinal > 0),
    event_kind text NOT NULL CHECK (event_kind IN ('work_attempt', 'tool_event', 'work_outcome')),
    work_attempt_event_id uuid,
    tool_event_id uuid,
    work_outcome_event_id uuid,
    event_hash bytea NOT NULL CHECK (octet_length(event_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (trace_number, ordinal),
    FOREIGN KEY (trace_number, project_id) REFERENCES agent_r540_tool_trace_roots (trace_number, project_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (work_attempt_event_id) REFERENCES agent_r540_work_attempt_events (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (tool_event_id) REFERENCES agent_r540_tool_attempt_events (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (work_outcome_event_id) REFERENCES agent_r540_work_outcome_events (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_r540_inventory_typed_event CHECK (
        (event_kind = 'work_attempt' AND work_attempt_event_id IS NOT NULL
         AND tool_event_id IS NULL AND work_outcome_event_id IS NULL)
        OR (event_kind = 'tool_event' AND work_attempt_event_id IS NULL
         AND tool_event_id IS NOT NULL AND work_outcome_event_id IS NULL)
        OR (event_kind = 'work_outcome' AND work_attempt_event_id IS NULL
         AND tool_event_id IS NULL AND work_outcome_event_id IS NOT NULL)
    )
);
CREATE UNIQUE INDEX agent_r540_inventory_work_event_unique
    ON agent_r540_tool_trace_inventory (trace_number, work_attempt_event_id)
    WHERE work_attempt_event_id IS NOT NULL;
CREATE UNIQUE INDEX agent_r540_inventory_tool_event_unique
    ON agent_r540_tool_trace_inventory (trace_number, tool_event_id)
    WHERE tool_event_id IS NOT NULL;
CREATE UNIQUE INDEX agent_r540_inventory_outcome_event_unique
    ON agent_r540_tool_trace_inventory (trace_number, work_outcome_event_id)
    WHERE work_outcome_event_id IS NOT NULL;

CREATE TABLE agent_r540_tool_trace_certificates (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    trace_number bigint NOT NULL,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    end_tick bigint NOT NULL CHECK (end_tick >= 0),
    last_inventory_ordinal bigint NOT NULL CHECK (last_inventory_ordinal >= 0),
    work_attempt_inventory jsonb NOT NULL CHECK (jsonb_typeof(work_attempt_inventory) = 'array'),
    work_outcome_inventory jsonb NOT NULL CHECK (jsonb_typeof(work_outcome_inventory) = 'array'),
    tool_event_inventory jsonb NOT NULL CHECK (jsonb_typeof(tool_event_inventory) = 'array'),
    work_attempt_inventory_commitment bytea NOT NULL CHECK (octet_length(work_attempt_inventory_commitment) = 32),
    work_outcome_inventory_commitment bytea NOT NULL CHECK (octet_length(work_outcome_inventory_commitment) = 32),
    tool_event_inventory_commitment bytea NOT NULL CHECK (octet_length(tool_event_inventory_commitment) = 32),
    outcome_gate_mode text NOT NULL CHECK (outcome_gate_mode IN ('enabled', 'disabled_fail_closed')),
    tool_gate_mode text NOT NULL CHECK (tool_gate_mode IN ('enabled', 'disabled_fail_closed')),
    blocker_gate_mode text NOT NULL DEFAULT 'disabled_fail_closed' CHECK (blocker_gate_mode = 'disabled_fail_closed'),
    causal_gate_mode text NOT NULL DEFAULT 'disabled_fail_closed' CHECK (causal_gate_mode = 'disabled_fail_closed'),
    evidence_gate_mode text NOT NULL DEFAULT 'disabled_fail_closed' CHECK (evidence_gate_mode = 'disabled_fail_closed'),
    disclosure_gate_mode text NOT NULL DEFAULT 'disabled_fail_closed' CHECK (disclosure_gate_mode = 'disabled_fail_closed'),
    previous_certificate_hash bytea CHECK (
        previous_certificate_hash IS NULL OR octet_length(previous_certificate_hash) = 32
    ),
    certificate_hash bytea NOT NULL CHECK (octet_length(certificate_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (trace_number, project_id) REFERENCES agent_r540_tool_trace_roots (trace_number, project_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    UNIQUE (trace_number, version),
    UNIQUE (trace_number, certificate_hash),
    CONSTRAINT agent_r540_certificate_gate_nonvacuity CHECK (
        (tool_gate_mode = 'enabled' AND jsonb_array_length(tool_event_inventory) > 0
         OR tool_gate_mode = 'disabled_fail_closed' AND jsonb_array_length(tool_event_inventory) = 0)
        AND
        (outcome_gate_mode = 'enabled' AND jsonb_array_length(work_outcome_inventory) > 0
         OR outcome_gate_mode = 'disabled_fail_closed' AND jsonb_array_length(work_outcome_inventory) = 0)
    )
);

CREATE FUNCTION sprout_private.reject_agent_tool_trace_history_mutation()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
BEGIN
    RAISE EXCEPTION 'R540 tool trace history is append-only' USING ERRCODE = '55000';
END
$$;

CREATE TRIGGER agent_r540_trace_roots_immutable BEFORE UPDATE OR DELETE ON agent_r540_tool_trace_roots
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_trace_history_mutation();
CREATE TRIGGER agent_r540_work_attempts_immutable BEFORE UPDATE OR DELETE ON agent_r540_work_attempt_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_trace_history_mutation();
CREATE TRIGGER agent_r540_tool_events_immutable BEFORE UPDATE OR DELETE ON agent_r540_tool_attempt_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_trace_history_mutation();
CREATE TRIGGER agent_r540_work_outcomes_immutable BEFORE UPDATE OR DELETE ON agent_r540_work_outcome_events
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_trace_history_mutation();
CREATE TRIGGER agent_r540_inventory_immutable BEFORE UPDATE OR DELETE ON agent_r540_tool_trace_inventory
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_trace_history_mutation();
CREATE TRIGGER agent_r540_certificates_immutable BEFORE UPDATE OR DELETE ON agent_r540_tool_trace_certificates
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_trace_history_mutation();

-- Append one list-exact prefix certificate. Stored JSON arrays are the formal
-- ordered lists; commitments and the latest view independently recompute them.
CREATE FUNCTION sprout_private.append_agent_tool_trace_certificate(
    candidate_trace_number bigint,
    candidate_end_tick bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
DECLARE
    root_row public.agent_r540_tool_trace_roots%ROWTYPE;
    previous_row public.agent_r540_tool_trace_certificates%ROWTYPE;
    work_list jsonb;
    outcome_list jsonb;
    tool_list jsonb;
    work_hash bytea;
    outcome_hash bytea;
    tool_hash bytea;
    max_ordinal bigint;
    next_version integer;
    certificate_hash bytea;
BEGIN
    SELECT * INTO STRICT root_row FROM public.agent_r540_tool_trace_roots
    WHERE trace_number = candidate_trace_number FOR UPDATE;
    IF candidate_end_tick < root_row.start_tick THEN
        RAISE EXCEPTION 'trace end precedes trace start' USING ERRCODE = '23514';
    END IF;

    SELECT
      COALESCE(jsonb_agg(jsonb_build_object(
        'ordinal', ordinal, 'event_id', work_attempt_event_id,
        'event_hash', encode(event_hash, 'hex')) ORDER BY ordinal)
        FILTER (WHERE event_kind = 'work_attempt'), '[]'::jsonb),
      COALESCE(jsonb_agg(jsonb_build_object(
        'ordinal', ordinal, 'event_id', work_outcome_event_id,
        'event_hash', encode(event_hash, 'hex')) ORDER BY ordinal)
        FILTER (WHERE event_kind = 'work_outcome'), '[]'::jsonb),
      COALESCE(jsonb_agg(jsonb_build_object(
        'ordinal', ordinal, 'event_id', tool_event_id,
        'event_hash', encode(event_hash, 'hex')) ORDER BY ordinal)
        FILTER (WHERE event_kind = 'tool_event'), '[]'::jsonb),
      COALESCE(max(ordinal), 0)
    INTO work_list, outcome_list, tool_list, max_ordinal
    FROM public.agent_r540_tool_trace_inventory WHERE trace_number = candidate_trace_number;

    work_hash := public.digest(pg_catalog.convert_to(work_list::text, 'UTF8'), 'sha256');
    outcome_hash := public.digest(pg_catalog.convert_to(outcome_list::text, 'UTF8'), 'sha256');
    tool_hash := public.digest(pg_catalog.convert_to(tool_list::text, 'UTF8'), 'sha256');
    SELECT * INTO previous_row FROM public.agent_r540_tool_trace_certificates
      WHERE trace_number = candidate_trace_number ORDER BY version DESC LIMIT 1;
    IF FOUND AND previous_row.last_inventory_ordinal = max_ordinal
       AND previous_row.work_attempt_inventory = work_list
       AND previous_row.work_outcome_inventory = outcome_list
       AND previous_row.tool_event_inventory = tool_list THEN
        RETURN;
    END IF;
    next_version := COALESCE(previous_row.version, 0) + 1;
    certificate_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
      'sprout-r540-tool-cluster-certificate-v1', root_row.trace_number::text,
      root_row.project_id::text, root_row.run_id::text, root_row.goal_id::text,
      next_version::text, candidate_end_tick::text, max_ordinal::text,
      encode(work_hash, 'hex'), encode(outcome_hash, 'hex'), encode(tool_hash, 'hex'),
      CASE WHEN jsonb_array_length(outcome_list) > 0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
      CASE WHEN jsonb_array_length(tool_list) > 0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
      COALESCE(encode(previous_row.certificate_hash, 'hex'), '')), 'UTF8'), 'sha256');

    INSERT INTO public.agent_r540_tool_trace_certificates (
      trace_number, project_id, run_id, goal_id, version, end_tick,
      last_inventory_ordinal, work_attempt_inventory, work_outcome_inventory,
      tool_event_inventory, work_attempt_inventory_commitment,
      work_outcome_inventory_commitment, tool_event_inventory_commitment,
      outcome_gate_mode, tool_gate_mode, previous_certificate_hash, certificate_hash
    ) VALUES (
      root_row.trace_number, root_row.project_id, root_row.run_id, root_row.goal_id,
      next_version, candidate_end_tick, max_ordinal, work_list, outcome_list, tool_list,
      work_hash, outcome_hash, tool_hash,
      CASE WHEN jsonb_array_length(outcome_list) > 0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
      CASE WHEN jsonb_array_length(tool_list) > 0 THEN 'enabled' ELSE 'disabled_fail_closed' END,
      previous_row.certificate_hash, certificate_hash
    );
END
$$;

-- Only the initialization transition can create a run-level trace root.
CREATE FUNCTION sprout_private.initialize_agent_tool_trace(
    candidate_project_id uuid,
    candidate_run_id uuid,
    candidate_transition_id uuid
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
DECLARE
    run_row public.agent_collaborative_runs%ROWTYPE;
    transition_row public.agent_run_transitions%ROWTYPE;
    trace_number_value bigint;
BEGIN
    SELECT * INTO STRICT run_row FROM public.agent_collaborative_runs
      WHERE project_id = candidate_project_id AND id = candidate_run_id FOR SHARE;
    SELECT * INTO STRICT transition_row FROM public.agent_run_transitions
      WHERE project_id = candidate_project_id AND run_id = candidate_run_id
        AND id = candidate_transition_id FOR SHARE;
    IF transition_row.transition_kind <> 'initialized'
       OR transition_row.state_version <> 1
       OR transition_row.semantic_tick IS NULL
       OR transition_row.actor_identity_id IS DISTINCT FROM sprout_private.current_identity_id()
       OR transition_row.actor_identity_id IS DISTINCT FROM run_row.created_by_identity_id
       OR transition_row.state_snapshot ->> 'id' <> candidate_run_id::text
       OR transition_row.state_snapshot ->> 'goal' <> run_row.goal_id::text THEN
        RAISE EXCEPTION 'tool trace initialization is not exact' USING ERRCODE = '23514';
    END IF;
    INSERT INTO public.agent_r540_tool_trace_roots (
      project_id, run_id, goal_id, start_tick, initialization_transition_id, root_hash
    ) VALUES (
      candidate_project_id, candidate_run_id, run_row.goal_id, transition_row.semantic_tick,
      candidate_transition_id, public.digest(pg_catalog.convert_to(concat_ws(E'\n',
        'sprout-r540-run-trace-root-v1', candidate_project_id::text,
        candidate_run_id::text, run_row.goal_id::text,
        transition_row.semantic_tick::text, candidate_transition_id::text), 'UTF8'), 'sha256')
    ) ON CONFLICT (project_id, run_id) DO NOTHING RETURNING trace_number INTO trace_number_value;
    IF trace_number_value IS NULL THEN
      SELECT trace_number INTO STRICT trace_number_value FROM public.agent_r540_tool_trace_roots
        WHERE project_id = candidate_project_id AND run_id = candidate_run_id
          AND goal_id = run_row.goal_id AND start_tick = transition_row.semantic_tick
          AND initialization_transition_id = candidate_transition_id;
    END IF;
    PERFORM sprout_private.append_agent_tool_trace_certificate(
      trace_number_value, transition_row.semantic_tick);
    RETURN trace_number_value;
END
$$;

-- Open one exact WorkAttempt and pending ToolEvent in a 0034-native trace.
CREATE FUNCTION sprout_private.project_agent_tool_attempt(
    candidate_project_id uuid,
    candidate_run_id uuid,
    candidate_call_id uuid,
    candidate_transition_id uuid
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
DECLARE
    call_row public.agent_tool_calls%ROWTYPE;
    transition_row public.agent_run_transitions%ROWTYPE;
    root_row public.agent_r540_tool_trace_roots%ROWTYPE;
    work_event_id uuid;
    tool_event_id uuid;
    work_json jsonb;
    claim_json jsonb;
    work_event_hash bytea;
    tool_event_hash bytea;
    next_ordinal bigint;
BEGIN
    SELECT * INTO STRICT call_row FROM public.agent_tool_calls
      WHERE project_id = candidate_project_id AND run_id = candidate_run_id
        AND id = candidate_call_id FOR SHARE;
    IF call_row.owner_identity_id <> sprout_private.current_identity_id()
       OR call_row.current_status <> 'pending' THEN
        RAISE EXCEPTION 'tool trace attempt caller or call mismatch' USING ERRCODE = '42501';
    END IF;
    SELECT * INTO root_row FROM public.agent_r540_tool_trace_roots
      WHERE project_id = candidate_project_id AND run_id = candidate_run_id FOR UPDATE;
    IF NOT FOUND THEN RETURN NULL; END IF;
    IF root_row.goal_id <> call_row.goal_id THEN
        RAISE EXCEPTION 'tool trace goal mismatch' USING ERRCODE = '23514';
    END IF;
    SELECT * INTO STRICT transition_row FROM public.agent_run_transitions
      WHERE id = candidate_transition_id AND project_id = candidate_project_id
        AND run_id = candidate_run_id FOR SHARE;
    IF transition_row.transition_kind <> 'tool_attempt_opened'
       OR transition_row.semantic_tick IS DISTINCT FROM call_row.requested_tick
       OR transition_row.actor_identity_id IS DISTINCT FROM call_row.owner_identity_id
       OR transition_row.observation_kind IS DISTINCT FROM 'tool_attempt'
       OR transition_row.observation_id IS DISTINCT FROM call_row.id THEN
        RAISE EXCEPTION 'tool trace opening transition mismatch' USING ERRCODE = '23514';
    END IF;
    work_json := transition_row.state_snapshot -> 'work_items' -> call_row.work_item_id::text;
    claim_json := transition_row.state_snapshot -> 'claims' -> call_row.work_claim_id::text;
    IF work_json IS NULL OR claim_json IS NULL
       OR work_json ->> 'run' <> call_row.run_id::text
       OR work_json ->> 'goal' <> call_row.goal_id::text
       OR work_json ->> 'id' <> call_row.work_item_id::text
       OR work_json ->> 'owner' <> call_row.owner_identity_id::text
       OR work_json ->> 'status' <> 'claimed'
       OR (work_json ->> 'attempt')::integer <> call_row.current_attempt
       OR claim_json ->> 'id' <> call_row.work_claim_id::text
       OR claim_json ->> 'work' <> call_row.work_item_id::text
       OR claim_json ->> 'claimant' <> call_row.owner_identity_id::text
       OR claim_json ->> 'status' <> 'active'
       OR (claim_json ->> 'attempt')::integer <> call_row.current_attempt
       OR (claim_json ->> 'acquired_at')::bigint > call_row.requested_tick
       OR call_row.requested_tick >= (claim_json ->> 'expires_at')::bigint THEN
        RAISE EXCEPTION 'tool trace WorkAttempt snapshot mismatch' USING ERRCODE = '23514';
    END IF;
    work_event_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
      'sprout-r540-work-attempt-v1', root_row.trace_number::text,
      candidate_project_id::text, candidate_run_id::text, call_row.goal_id::text,
      call_row.work_item_id::text, call_row.work_claim_id::text,
      call_row.current_attempt::text, call_row.owner_identity_id::text,
      call_row.requested_tick::text, candidate_transition_id::text,
      work_json::text, claim_json::text), 'UTF8'), 'sha256');
    INSERT INTO public.agent_r540_work_attempt_events (
      trace_number, project_id, run_id, goal_id, work_item_id, claim_id, attempt,
      actor_identity_id, tick, transition_id, work_snapshot, claim_snapshot, event_hash
    ) VALUES (
      root_row.trace_number, candidate_project_id, candidate_run_id, call_row.goal_id,
      call_row.work_item_id, call_row.work_claim_id, call_row.current_attempt,
      call_row.owner_identity_id, call_row.requested_tick, candidate_transition_id,
      work_json, claim_json, work_event_hash
    ) ON CONFLICT (trace_number, work_item_id, claim_id, attempt) DO NOTHING
      RETURNING id INTO work_event_id;
    IF work_event_id IS NULL THEN
      SELECT id INTO STRICT work_event_id FROM public.agent_r540_work_attempt_events
       WHERE trace_number = root_row.trace_number AND work_item_id = call_row.work_item_id
         AND claim_id = call_row.work_claim_id AND attempt = call_row.current_attempt
         AND event_hash = work_event_hash;
    END IF;
    tool_event_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
      'sprout-r540-tool-event-v1', 'pending', root_row.trace_number::text,
      candidate_project_id::text, candidate_run_id::text, call_row.goal_id::text,
      call_row.work_item_id::text, call_row.work_claim_id::text,
      call_row.current_attempt::text, call_row.owner_identity_id::text,
      call_row.id::text, call_row.tool_name, call_row.tool_version::text,
      encode(call_row.canonical_input_commitment, 'hex'), call_row.requested_tick::text,
      candidate_transition_id::text), 'UTF8'), 'sha256');
    INSERT INTO public.agent_r540_tool_attempt_events (
      trace_number, project_id, run_id, goal_id, work_item_id, claim_id, attempt,
      owner_identity_id, call_id, tool_name, tool_version, canonical_input_commitment,
      phase, status, canonical_output_commitment, requested_tick, observed_tick,
      tool_deadline_tick, work_attempt_event_id, transition_id, output_readable_by,
      call_snapshot, event_hash
    ) VALUES (
      root_row.trace_number, candidate_project_id, candidate_run_id, call_row.goal_id,
      call_row.work_item_id, call_row.work_claim_id, call_row.current_attempt,
      call_row.owner_identity_id, call_row.id, call_row.tool_name, call_row.tool_version,
      call_row.canonical_input_commitment, 'pending', 'pending', NULL,
      call_row.requested_tick, call_row.requested_tick, call_row.tool_deadline_tick,
      work_event_id, candidate_transition_id, call_row.output_readable_by,
      jsonb_build_object('id', call_row.id, 'owner', call_row.owner_identity_id,
        'tool', call_row.tool_name, 'tool_version', call_row.tool_version,
        'input', encode(call_row.canonical_input_commitment, 'hex'),
        'attempt', call_row.current_attempt, 'max_attempts', call_row.max_attempts,
        'timeout_ticks', call_row.timeout_seconds, 'status', 'pending',
        'output', NULL, 'failure', NULL), tool_event_hash
    ) ON CONFLICT (trace_number, call_id, attempt, phase) DO NOTHING
      RETURNING id INTO tool_event_id;
    IF tool_event_id IS NULL THEN
      SELECT id INTO STRICT tool_event_id FROM public.agent_r540_tool_attempt_events
       WHERE trace_number = root_row.trace_number AND call_id = call_row.id
         AND attempt = call_row.current_attempt AND phase = 'pending'
         AND event_hash = tool_event_hash;
    END IF;
    SELECT COALESCE(max(ordinal), 0) + 1 INTO next_ordinal
      FROM public.agent_r540_tool_trace_inventory WHERE trace_number = root_row.trace_number;
    INSERT INTO public.agent_r540_tool_trace_inventory (
      trace_number, project_id, ordinal, event_kind, work_attempt_event_id, event_hash
    ) VALUES (root_row.trace_number, candidate_project_id, next_ordinal,
      'work_attempt', work_event_id, work_event_hash)
    ON CONFLICT DO NOTHING;
    SELECT COALESCE(max(ordinal), 0) + 1 INTO next_ordinal
      FROM public.agent_r540_tool_trace_inventory WHERE trace_number = root_row.trace_number;
    INSERT INTO public.agent_r540_tool_trace_inventory (
      trace_number, project_id, ordinal, event_kind, tool_event_id, event_hash
    ) VALUES (root_row.trace_number, candidate_project_id, next_ordinal,
      'tool_event', tool_event_id, tool_event_hash)
    ON CONFLICT DO NOTHING;
    PERFORM sprout_private.append_agent_tool_trace_certificate(
      root_row.trace_number, call_row.requested_tick);
    RETURN root_row.trace_number;
END
$$;

-- Common terminal projector. Only the two narrow origin-specific wrappers call it.
CREATE FUNCTION sprout_private.project_agent_tool_terminal_internal(
    candidate_project_id uuid, candidate_run_id uuid, candidate_call_id uuid,
    candidate_attempt integer, candidate_observation_id uuid,
    candidate_transition_id uuid, expected_origin text
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
DECLARE
    call_row public.agent_tool_calls%ROWTYPE;
    observation_row public.agent_tool_attempt_observations%ROWTYPE;
    operational_outcome public.agent_run_external_tool_work_outcomes%ROWTYPE;
    transition_row public.agent_run_transitions%ROWTYPE;
    root_row public.agent_r540_tool_trace_roots%ROWTYPE;
    work_event public.agent_r540_work_attempt_events%ROWTYPE;
    tool_event_id uuid;
    outcome_event_id uuid;
    work_json jsonb;
    tool_event_hash bytea;
    outcome_event_hash bytea;
    formal_work_status text;
    next_ordinal bigint;
BEGIN
    SELECT * INTO STRICT call_row FROM public.agent_tool_calls
      WHERE project_id = candidate_project_id AND run_id = candidate_run_id
        AND id = candidate_call_id FOR SHARE;
    SELECT * INTO root_row FROM public.agent_r540_tool_trace_roots
      WHERE project_id = candidate_project_id AND run_id = candidate_run_id FOR UPDATE;
    IF NOT FOUND THEN RETURN NULL; END IF;
    IF call_row.current_attempt <> candidate_attempt OR call_row.current_status = 'pending' THEN
      RAISE EXCEPTION 'terminal tool trace call mismatch' USING ERRCODE = '23514';
    END IF;
    SELECT * INTO STRICT work_event FROM public.agent_r540_work_attempt_events
      WHERE trace_number = root_row.trace_number AND work_item_id = call_row.work_item_id
        AND claim_id = call_row.work_claim_id AND attempt = candidate_attempt;
    SELECT * INTO STRICT observation_row FROM public.agent_tool_attempt_observations
      WHERE project_id = candidate_project_id AND call_id = candidate_call_id
        AND attempt = candidate_attempt AND id = candidate_observation_id FOR SHARE;
    IF observation_row.terminal_origin <> expected_origin THEN
      RAISE EXCEPTION 'terminal origin branch mismatch' USING ERRCODE = '23514';
    END IF;
    IF expected_origin = 'signed_edge_observation'
       AND (observation_row.signer_identity_id IS DISTINCT FROM call_row.owner_identity_id
            OR observation_row.dispatch_id IS NULL OR observation_row.request_id IS NULL
            OR observation_row.wire_request_commitment IS NULL) THEN
      RAISE EXCEPTION 'signed terminal provenance mismatch' USING ERRCODE = '23514';
    ELSIF expected_origin = 'server_timeout'
       AND (observation_row.signer_identity_id IS NOT NULL
            OR observation_row.classical_signature IS NOT NULL
            OR observation_row.post_quantum_signature IS NOT NULL) THEN
      RAISE EXCEPTION 'server timeout must be unsigned' USING ERRCODE = '23514';
    END IF;
    SELECT * INTO STRICT operational_outcome FROM public.agent_run_external_tool_work_outcomes
      WHERE project_id = candidate_project_id AND run_id = candidate_run_id
        AND work_item_id = call_row.work_item_id AND claim_id = call_row.work_claim_id
        AND attempt = candidate_attempt AND observation_id = candidate_observation_id
        AND transition_id = candidate_transition_id FOR SHARE;
    SELECT * INTO STRICT transition_row FROM public.agent_run_transitions
      WHERE id = candidate_transition_id AND project_id = candidate_project_id
        AND run_id = candidate_run_id FOR SHARE;
    formal_work_status := CASE WHEN observation_row.terminal_status = 'succeeded'
                              THEN 'succeeded' ELSE 'failed' END;
    work_json := transition_row.state_snapshot -> 'work_items' -> call_row.work_item_id::text;
    IF transition_row.semantic_tick IS NULL OR transition_row.semantic_tick < work_event.tick
       OR transition_row.transition_kind <> (CASE WHEN formal_work_status = 'succeeded'
                                                  THEN 'work_succeeded' ELSE 'work_failed' END)
       OR transition_row.observation_kind IS DISTINCT FROM 'tool_terminal'
       OR transition_row.observation_id IS DISTINCT FROM candidate_observation_id
       OR work_json IS NULL OR work_json ->> 'run' <> call_row.run_id::text
       OR work_json ->> 'goal' <> call_row.goal_id::text
       OR work_json ->> 'id' <> call_row.work_item_id::text
       OR work_json ->> 'owner' <> call_row.owner_identity_id::text
       OR (work_json ->> 'attempt')::integer <> candidate_attempt
       OR work_json ->> 'status' <> formal_work_status
       OR operational_outcome.work_status <> formal_work_status
       OR observation_row.terminal_status <> call_row.current_status
       OR observation_row.canonical_input_commitment <> call_row.canonical_input_commitment
       OR observation_row.canonical_output_commitment IS DISTINCT FROM call_row.current_output_commitment THEN
      RAISE EXCEPTION 'terminal tool trace exactness mismatch' USING ERRCODE = '23514';
    END IF;
    tool_event_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
      'sprout-r540-tool-event-v1', 'terminal', root_row.trace_number::text,
      candidate_project_id::text, candidate_run_id::text, call_row.goal_id::text,
      call_row.work_item_id::text, call_row.work_claim_id::text,
      candidate_attempt::text, call_row.owner_identity_id::text, call_row.id::text,
      call_row.tool_name, call_row.tool_version::text,
      encode(call_row.canonical_input_commitment, 'hex'), observation_row.terminal_status,
      COALESCE(encode(observation_row.canonical_output_commitment, 'hex'), ''),
      call_row.requested_tick::text, transition_row.semantic_tick::text,
      candidate_transition_id::text, candidate_observation_id::text), 'UTF8'), 'sha256');
    INSERT INTO public.agent_r540_tool_attempt_events (
      trace_number, project_id, run_id, goal_id, work_item_id, claim_id, attempt,
      owner_identity_id, call_id, tool_name, tool_version, canonical_input_commitment,
      phase, status, canonical_output_commitment, requested_tick, observed_tick,
      tool_deadline_tick, work_attempt_event_id, transition_id, observation_id,
      terminal_origin, dispatch_id, request_id, wire_request_commitment,
      execution_profile_commitment, output_readable_by, call_snapshot, event_hash
    ) VALUES (
      root_row.trace_number, candidate_project_id, candidate_run_id, call_row.goal_id,
      call_row.work_item_id, call_row.work_claim_id, candidate_attempt,
      call_row.owner_identity_id, call_row.id, call_row.tool_name, call_row.tool_version,
      call_row.canonical_input_commitment, 'terminal', observation_row.terminal_status,
      observation_row.canonical_output_commitment, call_row.requested_tick,
      transition_row.semantic_tick, call_row.tool_deadline_tick, work_event.id,
      candidate_transition_id, candidate_observation_id, observation_row.terminal_origin,
      observation_row.dispatch_id, observation_row.request_id,
      observation_row.wire_request_commitment, observation_row.execution_profile_commitment,
      observation_row.output_readable_by,
      jsonb_build_object('id', call_row.id, 'owner', call_row.owner_identity_id,
        'tool', call_row.tool_name, 'tool_version', call_row.tool_version,
        'input', encode(call_row.canonical_input_commitment, 'hex'),
        'attempt', candidate_attempt, 'max_attempts', call_row.max_attempts,
        'timeout_ticks', call_row.timeout_seconds, 'status', call_row.current_status,
        'output', CASE WHEN call_row.current_output_commitment IS NULL THEN NULL
                       ELSE encode(call_row.current_output_commitment, 'hex') END,
        'failure', observation_row.failure_code), tool_event_hash
    ) ON CONFLICT (trace_number, call_id, attempt, phase) DO NOTHING
      RETURNING id INTO tool_event_id;
    IF tool_event_id IS NULL THEN
      SELECT id INTO STRICT tool_event_id FROM public.agent_r540_tool_attempt_events
       WHERE trace_number = root_row.trace_number AND call_id = call_row.id
         AND attempt = candidate_attempt AND phase = 'terminal'
         AND event_hash = tool_event_hash;
    END IF;
    outcome_event_hash := public.digest(pg_catalog.convert_to(concat_ws(E'\n',
      'sprout-r540-work-outcome-v1', root_row.trace_number::text,
      candidate_project_id::text, candidate_run_id::text, call_row.goal_id::text,
      call_row.work_item_id::text, call_row.work_claim_id::text,
      candidate_attempt::text, formal_work_status, transition_row.semantic_tick::text,
      candidate_transition_id::text, candidate_observation_id::text,
      transition_row.state_snapshot::text), 'UTF8'), 'sha256');
    INSERT INTO public.agent_r540_work_outcome_events (
      trace_number, project_id, run_id, goal_id, work_item_id, claim_id, attempt,
      status, observed_tick, work_attempt_event_id, tool_event_id, observation_id,
      operational_outcome_id, transition_id, state_snapshot, event_hash
    ) VALUES (
      root_row.trace_number, candidate_project_id, candidate_run_id, call_row.goal_id,
      call_row.work_item_id, call_row.work_claim_id, candidate_attempt,
      formal_work_status, transition_row.semantic_tick, work_event.id, tool_event_id,
      candidate_observation_id, operational_outcome.id, candidate_transition_id,
      transition_row.state_snapshot, outcome_event_hash
    ) ON CONFLICT (trace_number, work_item_id, claim_id, attempt) DO NOTHING
      RETURNING id INTO outcome_event_id;
    IF outcome_event_id IS NULL THEN
      SELECT id INTO STRICT outcome_event_id FROM public.agent_r540_work_outcome_events
       WHERE trace_number = root_row.trace_number AND work_item_id = call_row.work_item_id
         AND claim_id = call_row.work_claim_id AND attempt = candidate_attempt
         AND event_hash = outcome_event_hash;
    END IF;
    SELECT COALESCE(max(ordinal), 0) + 1 INTO next_ordinal
      FROM public.agent_r540_tool_trace_inventory WHERE trace_number = root_row.trace_number;
    INSERT INTO public.agent_r540_tool_trace_inventory (
      trace_number, project_id, ordinal, event_kind, tool_event_id, event_hash
    ) VALUES (root_row.trace_number, candidate_project_id, next_ordinal,
      'tool_event', tool_event_id, tool_event_hash)
    ON CONFLICT DO NOTHING;
    SELECT COALESCE(max(ordinal), 0) + 1 INTO next_ordinal
      FROM public.agent_r540_tool_trace_inventory WHERE trace_number = root_row.trace_number;
    INSERT INTO public.agent_r540_tool_trace_inventory (
      trace_number, project_id, ordinal, event_kind, work_outcome_event_id, event_hash
    ) VALUES (root_row.trace_number, candidate_project_id, next_ordinal,
      'work_outcome', outcome_event_id, outcome_event_hash)
    ON CONFLICT DO NOTHING;
    PERFORM sprout_private.append_agent_tool_trace_certificate(
      root_row.trace_number, transition_row.semantic_tick);
    RETURN root_row.trace_number;
END
$$;

CREATE FUNCTION sprout_private.project_agent_tool_signed_terminal(
    candidate_project_id uuid, candidate_run_id uuid, candidate_call_id uuid,
    candidate_attempt integer, candidate_observation_id uuid, candidate_transition_id uuid
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
BEGIN
  RETURN sprout_private.project_agent_tool_terminal_internal(
    candidate_project_id, candidate_run_id, candidate_call_id, candidate_attempt,
    candidate_observation_id, candidate_transition_id, 'signed_edge_observation');
END
$$;

CREATE FUNCTION sprout_private.project_agent_tool_server_timeout(
    candidate_project_id uuid, candidate_run_id uuid, candidate_call_id uuid,
    candidate_attempt integer, candidate_observation_id uuid, candidate_transition_id uuid
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog SET row_security = off AS $$
BEGIN
  RETURN sprout_private.project_agent_tool_terminal_internal(
    candidate_project_id, candidate_run_id, candidate_call_id, candidate_attempt,
    candidate_observation_id, candidate_transition_id, 'server_timeout');
END
$$;

-- Recompute the canonical ordered inventories independently of the certificate.
CREATE VIEW agent_r540_tool_trace_inventory_state AS
SELECT root.trace_number, root.project_id, root.run_id, root.goal_id, root.start_tick,
  COALESCE(jsonb_agg(jsonb_build_object('ordinal', inventory.ordinal,
    'event_id', inventory.work_attempt_event_id,
    'event_hash', encode(inventory.event_hash, 'hex')) ORDER BY inventory.ordinal)
    FILTER (WHERE inventory.event_kind = 'work_attempt'), '[]'::jsonb) AS work_attempt_inventory,
  COALESCE(jsonb_agg(jsonb_build_object('ordinal', inventory.ordinal,
    'event_id', inventory.work_outcome_event_id,
    'event_hash', encode(inventory.event_hash, 'hex')) ORDER BY inventory.ordinal)
    FILTER (WHERE inventory.event_kind = 'work_outcome'), '[]'::jsonb) AS work_outcome_inventory,
  COALESCE(jsonb_agg(jsonb_build_object('ordinal', inventory.ordinal,
    'event_id', inventory.tool_event_id,
    'event_hash', encode(inventory.event_hash, 'hex')) ORDER BY inventory.ordinal)
    FILTER (WHERE inventory.event_kind = 'tool_event'), '[]'::jsonb) AS tool_event_inventory,
  COALESCE(max(inventory.ordinal), 0) AS last_inventory_ordinal
FROM agent_r540_tool_trace_roots root
LEFT JOIN agent_r540_tool_trace_inventory inventory ON inventory.trace_number = root.trace_number
GROUP BY root.trace_number, root.project_id, root.run_id, root.goal_id, root.start_tick;

CREATE VIEW agent_r540_exact_tool_trace_certificates AS
SELECT certificate.*
FROM agent_r540_tool_trace_certificates certificate
JOIN agent_r540_tool_trace_inventory_state actual ON actual.trace_number = certificate.trace_number
LEFT JOIN agent_r540_tool_trace_certificates previous
  ON previous.trace_number = certificate.trace_number AND previous.version = certificate.version - 1
WHERE certificate.version = (
    SELECT max(candidate.version) FROM agent_r540_tool_trace_certificates candidate
    WHERE candidate.trace_number = certificate.trace_number)
  AND certificate.last_inventory_ordinal = actual.last_inventory_ordinal
  AND certificate.work_attempt_inventory = actual.work_attempt_inventory
  AND certificate.work_outcome_inventory = actual.work_outcome_inventory
  AND certificate.tool_event_inventory = actual.tool_event_inventory
  AND certificate.work_attempt_inventory_commitment =
      digest(convert_to(actual.work_attempt_inventory::text, 'UTF8'), 'sha256')
  AND certificate.work_outcome_inventory_commitment =
      digest(convert_to(actual.work_outcome_inventory::text, 'UTF8'), 'sha256')
  AND certificate.tool_event_inventory_commitment =
      digest(convert_to(actual.tool_event_inventory::text, 'UTF8'), 'sha256')
  AND ((certificate.version = 1 AND certificate.previous_certificate_hash IS NULL
        AND previous.id IS NULL)
       OR (certificate.version > 1 AND previous.certificate_hash =
           certificate.previous_certificate_hash
           AND previous.end_tick <= certificate.end_tick
           AND previous.last_inventory_ordinal < certificate.last_inventory_ordinal))
  AND certificate.certificate_hash = digest(convert_to(concat_ws(E'\n',
      'sprout-r540-tool-cluster-certificate-v1', certificate.trace_number::text,
      certificate.project_id::text, certificate.run_id::text, certificate.goal_id::text,
      certificate.version::text, certificate.end_tick::text,
      certificate.last_inventory_ordinal::text,
      encode(certificate.work_attempt_inventory_commitment, 'hex'),
      encode(certificate.work_outcome_inventory_commitment, 'hex'),
      encode(certificate.tool_event_inventory_commitment, 'hex'),
      certificate.outcome_gate_mode, certificate.tool_gate_mode,
      COALESCE(encode(certificate.previous_certificate_hash, 'hex'), '')), 'UTF8'), 'sha256')
  AND ((certificate.tool_gate_mode = 'enabled'
        AND jsonb_array_length(actual.tool_event_inventory) > 0)
       OR (certificate.tool_gate_mode = 'disabled_fail_closed'
        AND jsonb_array_length(actual.tool_event_inventory) = 0))
  AND ((certificate.outcome_gate_mode = 'enabled'
        AND jsonb_array_length(actual.work_outcome_inventory) > 0)
       OR (certificate.outcome_gate_mode = 'disabled_fail_closed'
        AND jsonb_array_length(actual.work_outcome_inventory) = 0));

CREATE VIEW agent_r540_exact_work_attempt_trace_records AS
SELECT inventory.ordinal AS trace_ordinal, work.*
FROM agent_r540_tool_trace_inventory inventory
JOIN agent_r540_work_attempt_events work ON work.id=inventory.work_attempt_event_id
JOIN agent_r540_tool_trace_roots root ON root.trace_number=work.trace_number
JOIN agent_run_transitions transition ON transition.id=work.transition_id
WHERE inventory.event_kind='work_attempt' AND inventory.trace_number=work.trace_number
  AND inventory.project_id=work.project_id AND inventory.event_hash=work.event_hash
  AND root.project_id=work.project_id AND root.run_id=work.run_id
  AND root.goal_id=work.goal_id AND root.start_tick <= work.tick
  AND transition.project_id=work.project_id AND transition.run_id=work.run_id
  AND transition.transition_kind='tool_attempt_opened'
  AND transition.semantic_tick=work.tick
  AND transition.actor_identity_id=work.actor_identity_id
  AND transition.observation_kind='tool_attempt'
  AND transition.state_snapshot -> 'work_items' -> work.work_item_id::text = work.work_snapshot
  AND transition.state_snapshot -> 'claims' -> work.claim_id::text = work.claim_snapshot
  AND work.work_snapshot ->> 'run'=work.run_id::text
  AND work.work_snapshot ->> 'goal'=work.goal_id::text
  AND work.work_snapshot ->> 'id'=work.work_item_id::text
  AND work.work_snapshot ->> 'owner'=work.actor_identity_id::text
  AND work.work_snapshot ->> 'status'='claimed'
  AND (work.work_snapshot ->> 'attempt')::integer=work.attempt
  AND work.claim_snapshot ->> 'id'=work.claim_id::text
  AND work.claim_snapshot ->> 'work'=work.work_item_id::text
  AND work.claim_snapshot ->> 'claimant'=work.actor_identity_id::text
  AND work.claim_snapshot ->> 'status'='active'
  AND (work.claim_snapshot ->> 'attempt')::integer=work.attempt
  AND (work.claim_snapshot ->> 'acquired_at')::bigint <= work.tick
  AND work.tick < (work.claim_snapshot ->> 'expires_at')::bigint;

CREATE VIEW agent_r540_exact_tool_trace_records AS
SELECT inventory.ordinal AS trace_ordinal, root.trace_number, root.project_id, root.run_id,
  root.goal_id, work.id AS work_attempt_event_id, tool.id AS tool_event_id,
  outcome.id AS work_outcome_event_id, inventory.event_hash, tool.work_item_id, tool.claim_id,
  tool.attempt, tool.owner_identity_id, tool.call_id, tool.tool_name,
  tool.tool_version, tool.canonical_input_commitment, tool.phase, tool.status,
  tool.canonical_output_commitment, tool.requested_tick, tool.observed_tick,
  tool.tool_deadline_tick, tool.terminal_origin, tool.dispatch_id, tool.request_id,
  tool.wire_request_commitment, tool.execution_profile_commitment,
  tool.output_readable_by, tool.transition_id, tool.observation_id
FROM agent_r540_tool_trace_roots root
JOIN agent_r540_tool_trace_inventory inventory
  ON inventory.trace_number = root.trace_number AND inventory.event_kind = 'tool_event'
JOIN agent_r540_tool_attempt_events tool ON tool.id = inventory.tool_event_id
JOIN agent_r540_exact_work_attempt_trace_records work ON work.id = tool.work_attempt_event_id
JOIN agent_tool_calls call ON call.id=tool.call_id AND call.project_id=tool.project_id
LEFT JOIN agent_r540_work_outcome_events outcome
  ON outcome.tool_event_id = tool.id AND outcome.trace_number = root.trace_number
LEFT JOIN agent_tool_attempt_observations observation ON observation.id=tool.observation_id
LEFT JOIN agent_tool_attempt_dispatches dispatch ON dispatch.id=tool.dispatch_id
LEFT JOIN agent_tool_attempt_requests request ON request.id=tool.request_id
WHERE inventory.event_hash=tool.event_hash
  AND call.run_id=tool.run_id AND call.goal_id=tool.goal_id
  AND call.owner_identity_id=tool.owner_identity_id AND call.tool_name=tool.tool_name
  AND call.tool_version=tool.tool_version
  AND call.canonical_input_commitment=tool.canonical_input_commitment
  AND tool.call_snapshot ->> 'id'=tool.call_id::text
  AND tool.call_snapshot ->> 'owner'=tool.owner_identity_id::text
  AND tool.call_snapshot ->> 'tool'=tool.tool_name
  AND (tool.call_snapshot ->> 'tool_version')::integer=tool.tool_version
  AND tool.call_snapshot ->> 'input'=encode(tool.canonical_input_commitment, 'hex')
  AND (tool.call_snapshot ->> 'attempt')::integer=tool.attempt
  AND tool.call_snapshot ->> 'status'=tool.status
  AND tool.call_snapshot ->> 'output' IS NOT DISTINCT FROM
      CASE WHEN tool.canonical_output_commitment IS NULL THEN NULL
           ELSE encode(tool.canonical_output_commitment, 'hex') END
  AND work.trace_number = root.trace_number AND work.project_id = tool.project_id
  AND work.run_id = tool.run_id AND work.goal_id = tool.goal_id
  AND work.work_item_id = tool.work_item_id AND work.claim_id = tool.claim_id
  AND work.attempt = tool.attempt AND work.actor_identity_id = tool.owner_identity_id
  AND work.tick = tool.requested_tick
  AND ((tool.phase = 'pending' AND outcome.id IS NULL AND observation.id IS NULL
        AND dispatch.id IS NULL AND request.id IS NULL)
       OR (tool.phase = 'terminal' AND outcome.id IS NOT NULL
        AND observation.id IS NOT NULL
        AND observation.project_id=tool.project_id
        AND observation.call_id=tool.call_id AND observation.attempt=tool.attempt
        AND observation.terminal_origin=tool.terminal_origin
        AND observation.terminal_status=tool.status
        AND observation.canonical_input_commitment=tool.canonical_input_commitment
        AND observation.canonical_output_commitment IS NOT DISTINCT FROM
            tool.canonical_output_commitment
        AND observation.output_readable_by=tool.output_readable_by
        AND observation.dispatch_id IS NOT DISTINCT FROM tool.dispatch_id
        AND observation.request_id IS NOT DISTINCT FROM tool.request_id
        AND observation.wire_request_commitment IS NOT DISTINCT FROM
            tool.wire_request_commitment
        AND observation.execution_profile_commitment IS NOT DISTINCT FROM
            tool.execution_profile_commitment
        AND (tool.dispatch_id IS NULL OR (dispatch.id IS NOT NULL
          AND dispatch.project_id=tool.project_id AND dispatch.call_id=tool.call_id
          AND dispatch.attempt=tool.attempt
          AND dispatch.canonical_input_commitment=tool.canonical_input_commitment
          AND dispatch.execution_profile_commitment=tool.execution_profile_commitment))
        AND (tool.request_id IS NULL OR (request.id IS NOT NULL
          AND request.project_id=tool.project_id AND request.call_id=tool.call_id
          AND request.attempt=tool.attempt AND request.dispatch_id=tool.dispatch_id
          AND request.wire_request_commitment=tool.wire_request_commitment))
        AND outcome.work_item_id = tool.work_item_id AND outcome.claim_id = tool.claim_id
        AND outcome.attempt = tool.attempt AND outcome.observed_tick = tool.observed_tick
        AND outcome.transition_id = tool.transition_id
        AND outcome.observation_id = tool.observation_id
        AND outcome.status = CASE WHEN tool.status = 'succeeded'
                                  THEN 'succeeded' ELSE 'failed' END));

CREATE VIEW agent_r540_exact_work_outcome_trace_records AS
SELECT inventory.ordinal AS trace_ordinal, outcome.*
FROM agent_r540_tool_trace_inventory inventory
JOIN agent_r540_work_outcome_events outcome ON outcome.id=inventory.work_outcome_event_id
JOIN agent_r540_exact_work_attempt_trace_records work
  ON work.id=outcome.work_attempt_event_id
JOIN agent_r540_exact_tool_trace_records tool ON tool.tool_event_id=outcome.tool_event_id
JOIN agent_run_transitions transition ON transition.id=outcome.transition_id
WHERE inventory.event_kind='work_outcome' AND inventory.trace_number=outcome.trace_number
  AND inventory.project_id=outcome.project_id AND inventory.event_hash=outcome.event_hash
  AND work.trace_number=outcome.trace_number AND work.run_id=outcome.run_id
  AND work.goal_id=outcome.goal_id AND work.work_item_id=outcome.work_item_id
  AND work.claim_id=outcome.claim_id AND work.attempt=outcome.attempt
  AND work.tick <= outcome.observed_tick
  AND tool.trace_number=outcome.trace_number AND tool.phase='terminal'
  AND tool.run_id=outcome.run_id AND tool.goal_id=outcome.goal_id
  AND tool.work_item_id=outcome.work_item_id AND tool.claim_id=outcome.claim_id
  AND tool.attempt=outcome.attempt AND tool.observed_tick=outcome.observed_tick
  AND tool.transition_id=outcome.transition_id AND tool.observation_id=outcome.observation_id
  AND transition.semantic_tick=outcome.observed_tick
  AND transition.state_snapshot=outcome.state_snapshot
  AND transition.state_snapshot -> 'work_items' -> outcome.work_item_id::text ->> 'status'=outcome.status
  AND (transition.state_snapshot -> 'work_items' -> outcome.work_item_id::text ->> 'attempt')::integer=outcome.attempt;

CREATE VIEW agent_r540_tool_trace_exact_inventory_state AS
SELECT root.trace_number, root.project_id, root.run_id,
  COALESCE(jsonb_agg(jsonb_build_object('ordinal', inventory.ordinal,
    'event_id', inventory.work_attempt_event_id,
    'event_hash', encode(inventory.event_hash, 'hex')) ORDER BY inventory.ordinal)
    FILTER (WHERE inventory.event_kind='work_attempt' AND exact_work.id IS NOT NULL), '[]'::jsonb)
    AS work_attempt_inventory,
  COALESCE(jsonb_agg(jsonb_build_object('ordinal', inventory.ordinal,
    'event_id', inventory.work_outcome_event_id,
    'event_hash', encode(inventory.event_hash, 'hex')) ORDER BY inventory.ordinal)
    FILTER (WHERE inventory.event_kind='work_outcome' AND exact_outcome.id IS NOT NULL), '[]'::jsonb)
    AS work_outcome_inventory,
  COALESCE(jsonb_agg(jsonb_build_object('ordinal', inventory.ordinal,
    'event_id', inventory.tool_event_id,
    'event_hash', encode(inventory.event_hash, 'hex')) ORDER BY inventory.ordinal)
    FILTER (WHERE inventory.event_kind='tool_event' AND exact_tool.tool_event_id IS NOT NULL), '[]'::jsonb)
    AS tool_event_inventory
FROM agent_r540_tool_trace_roots root
LEFT JOIN agent_r540_tool_trace_inventory inventory ON inventory.trace_number=root.trace_number
LEFT JOIN agent_r540_exact_work_attempt_trace_records exact_work
  ON exact_work.id=inventory.work_attempt_event_id
LEFT JOIN agent_r540_exact_tool_trace_records exact_tool
  ON exact_tool.tool_event_id=inventory.tool_event_id
LEFT JOIN agent_r540_exact_work_outcome_trace_records exact_outcome
  ON exact_outcome.id=inventory.work_outcome_event_id
GROUP BY root.trace_number, root.project_id, root.run_id;

CREATE VIEW agent_r541_tool_run_surface_gates AS
SELECT run.project_id, run.id AS run_id, root.trace_number,
  CASE WHEN exact.trace_number IS NOT NULL THEN certificate.tool_gate_mode
       ELSE 'disabled_fail_closed' END AS tool_mode,
  CASE WHEN exact.trace_number IS NOT NULL AND certificate.tool_gate_mode = 'enabled'
       THEN certificate.tool_event_inventory ELSE '[]'::jsonb END AS tool_records,
  CASE WHEN exact.trace_number IS NOT NULL THEN certificate.outcome_gate_mode
       ELSE 'disabled_fail_closed' END AS outcome_mode,
  CASE WHEN exact.trace_number IS NOT NULL AND certificate.outcome_gate_mode = 'enabled'
       THEN certificate.work_outcome_inventory ELSE '[]'::jsonb END AS outcome_records,
  'disabled_fail_closed'::text AS blocker_mode, '[]'::jsonb AS blocker_records,
  'disabled_fail_closed'::text AS causal_mode, '[]'::jsonb AS causal_records,
  'disabled_fail_closed'::text AS evidence_mode, '[]'::jsonb AS evidence_records,
  'disabled_fail_closed'::text AS disclosure_mode, '[]'::jsonb AS disclosure_records
FROM agent_collaborative_runs run
LEFT JOIN agent_r540_tool_trace_roots root
  ON root.project_id = run.project_id AND root.run_id = run.id
LEFT JOIN agent_r540_exact_tool_trace_certificates certificate
  ON certificate.trace_number = root.trace_number
LEFT JOIN agent_r540_tool_trace_exact_inventory_state exact
  ON exact.trace_number=certificate.trace_number
 AND exact.work_attempt_inventory=certificate.work_attempt_inventory
 AND exact.work_outcome_inventory=certificate.work_outcome_inventory
 AND exact.tool_event_inventory=certificate.tool_event_inventory;

CREATE VIEW agent_r541_tool_outcome_surface_records AS
SELECT exact.* FROM agent_r540_exact_work_outcome_trace_records exact
JOIN agent_r541_tool_run_surface_gates gate
  ON gate.project_id=exact.project_id AND gate.run_id=exact.run_id
    AND gate.trace_number=exact.trace_number
WHERE gate.outcome_mode='enabled' AND jsonb_array_length(gate.outcome_records) > 0;

DROP VIEW agent_r541_tool_surface_records;
CREATE VIEW agent_r541_tool_surface_records AS
SELECT exact.* FROM agent_r540_exact_tool_trace_records exact
JOIN agent_r541_tool_run_surface_gates gate
  ON gate.project_id = exact.project_id AND gate.run_id = exact.run_id
    AND gate.trace_number = exact.trace_number
WHERE gate.tool_mode = 'enabled' AND jsonb_array_length(gate.tool_records) > 0;

ALTER TABLE agent_r540_tool_trace_roots ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_tool_trace_roots FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_work_attempt_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_work_attempt_events FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_tool_attempt_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_tool_attempt_events FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_work_outcome_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_work_outcome_events FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_tool_trace_inventory ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_tool_trace_inventory FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_tool_trace_certificates ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_r540_tool_trace_certificates FORCE ROW LEVEL SECURITY;

CREATE POLICY agent_r540_root_read ON agent_r540_tool_trace_roots FOR SELECT
USING (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_r540_work_event_read ON agent_r540_work_attempt_events FOR SELECT
USING (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_r540_tool_event_read ON agent_r540_tool_attempt_events FOR SELECT
USING (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_r540_outcome_event_read ON agent_r540_work_outcome_events FOR SELECT
USING (sprout_private.agent_run_access(project_id, run_id));
CREATE POLICY agent_r540_inventory_read ON agent_r540_tool_trace_inventory FOR SELECT
USING (EXISTS (SELECT 1 FROM agent_r540_tool_trace_roots root
  WHERE root.trace_number = agent_r540_tool_trace_inventory.trace_number
    AND sprout_private.agent_run_access(root.project_id, root.run_id)));
CREATE POLICY agent_r540_certificate_read ON agent_r540_tool_trace_certificates FOR SELECT
USING (sprout_private.agent_run_access(project_id, run_id));

REVOKE ALL ON TABLE agent_r540_tool_trace_roots FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_work_attempt_events FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_tool_attempt_events FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_work_outcome_events FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_tool_trace_inventory FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_tool_trace_certificates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_tool_trace_inventory_state FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_exact_tool_trace_certificates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_exact_work_attempt_trace_records FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_exact_tool_trace_records FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_exact_work_outcome_trace_records FROM PUBLIC;
REVOKE ALL ON TABLE agent_r540_tool_trace_exact_inventory_state FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_tool_run_surface_gates FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_tool_outcome_surface_records FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_tool_surface_records FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_tool_trace_history_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.append_agent_tool_trace_certificate(bigint,bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.initialize_agent_tool_trace(uuid,uuid,uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_tool_attempt(uuid,uuid,uuid,uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_tool_terminal_internal(uuid,uuid,uuid,integer,uuid,uuid,text) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_tool_signed_terminal(uuid,uuid,uuid,integer,uuid,uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.project_agent_tool_server_timeout(uuid,uuid,uuid,integer,uuid,uuid) FROM PUBLIC;
