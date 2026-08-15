-- Concrete refinement of the agent-governance specification for a personal
-- edge runner. The service persists only ciphertext plus deterministic
-- governance metadata. Runner private keys and plaintext never enter these
-- tables.

ALTER TABLE identities
    ADD COLUMN principal_kind text NOT NULL DEFAULT 'user'
        CHECK (principal_kind IN ('user', 'agent'));

CREATE TABLE governed_agents (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    principal_identity_id uuid NOT NULL,
    controller_identity_id uuid NOT NULL,
    profile_resource_node_id uuid NOT NULL,
    encrypted_system_prompt bytea NOT NULL,
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    availability text NOT NULL
        CHECK (availability IN ('controller_private', 'project_delegable')),
    state text NOT NULL DEFAULT 'active'
        CHECK (state IN ('active', 'suspended', 'retired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    suspended_at timestamptz,
    retired_at timestamptz,
    CONSTRAINT governed_agents_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT governed_agents_principal_fk
        FOREIGN KEY (project_id, principal_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT governed_agents_controller_fk
        FOREIGN KEY (project_id, controller_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT governed_agents_profile_resource_fk
        FOREIGN KEY (project_id, profile_resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE CASCADE,
    CONSTRAINT governed_agents_epoch_fk
        FOREIGN KEY (project_id, profile_resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT governed_agents_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT governed_agents_principal_unique UNIQUE (principal_identity_id),
    CONSTRAINT governed_agents_distinct_controller
        CHECK (principal_identity_id <> controller_identity_id),
    CONSTRAINT governed_agents_prompt_nonempty
        CHECK (octet_length(encrypted_system_prompt) > 0),
    CONSTRAINT governed_agents_state_timestamps CHECK (
        (state = 'active' AND suspended_at IS NULL AND retired_at IS NULL)
        OR (state = 'suspended' AND suspended_at IS NOT NULL AND retired_at IS NULL)
        OR (state = 'retired' AND retired_at IS NOT NULL)
    )
);

CREATE INDEX governed_agents_controller_active_idx
    ON governed_agents (project_id, controller_identity_id, created_at)
    WHERE state = 'active';

CREATE TABLE agent_runners (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    principal_identity_id uuid NOT NULL,
    device_id uuid NOT NULL,
    activated_key_version integer,
    state text NOT NULL DEFAULT 'pending_key'
        CHECK (state IN ('pending_key', 'active', 'revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    activated_at timestamptz,
    revoked_at timestamptz,
    last_seen_at timestamptz,
    CONSTRAINT agent_runners_agent_fk
        FOREIGN KEY (project_id, agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE CASCADE,
    CONSTRAINT agent_runners_device_fk
        FOREIGN KEY (principal_identity_id, device_id)
        REFERENCES devices (identity_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_runners_device_key_fk
        FOREIGN KEY (principal_identity_id, device_id, activated_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_runners_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT agent_runners_device_unique UNIQUE (device_id),
    CONSTRAINT agent_runners_agent_device_unique UNIQUE (agent_id, device_id),
    CONSTRAINT agent_runners_state_shape CHECK (
        (state = 'pending_key' AND activated_key_version IS NULL
            AND activated_at IS NULL AND revoked_at IS NULL)
        OR (state = 'active' AND activated_key_version IS NOT NULL
            AND activated_at IS NOT NULL AND revoked_at IS NULL)
        OR (state = 'revoked' AND revoked_at IS NOT NULL)
    )
);

CREATE INDEX agent_runners_claimable_idx
    ON agent_runners (agent_id, device_id)
    WHERE state = 'active';

CREATE TABLE agent_responsibility_contracts (
    id uuid NOT NULL,
    project_id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision > 0),
    administrator_identity_id uuid NOT NULL,
    user_identity_id uuid NOT NULL,
    contract jsonb NOT NULL CHECK (jsonb_typeof(contract) = 'object'),
    contract_hash bytea NOT NULL CHECK (octet_length(contract_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, id, revision),
    CONSTRAINT agent_responsibilities_administrator_fk
        FOREIGN KEY (project_id, administrator_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_responsibilities_user_fk
        FOREIGN KEY (project_id, user_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_responsibilities_hash_unique
        UNIQUE (project_id, contract_hash)
);

CREATE INDEX agent_responsibilities_current_idx
    ON agent_responsibility_contracts (project_id, user_identity_id, revision DESC);

CREATE TABLE agent_local_goal_contracts (
    id uuid NOT NULL,
    project_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    agent_identity_id uuid NOT NULL,
    controller_identity_id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision > 0),
    contract jsonb NOT NULL CHECK (jsonb_typeof(contract) = 'object'),
    contract_hash bytea NOT NULL CHECK (octet_length(contract_hash) = 32),
    state text NOT NULL DEFAULT 'active'
        CHECK (state IN ('active', 'completed', 'failed', 'superseded')),
    recorded_at timestamptz NOT NULL DEFAULT now(),
    terminal_at timestamptz,
    PRIMARY KEY (project_id, id, revision),
    CONSTRAINT agent_local_goals_agent_fk
        FOREIGN KEY (project_id, agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE CASCADE,
    CONSTRAINT agent_local_goals_agent_member_fk
        FOREIGN KEY (project_id, agent_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_local_goals_controller_member_fk
        FOREIGN KEY (project_id, controller_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_local_goals_hash_unique UNIQUE (project_id, contract_hash),
    CONSTRAINT agent_local_goals_terminal_shape CHECK (
        (state = 'active' AND terminal_at IS NULL)
        OR (state <> 'active' AND terminal_at IS NOT NULL)
    )
);

CREATE INDEX agent_local_goals_current_idx
    ON agent_local_goal_contracts (project_id, agent_id, revision DESC);

CREATE TABLE agent_invocations (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    agent_identity_id uuid NOT NULL,
    local_goal_id uuid,
    local_goal_revision bigint,
    language_task jsonb NOT NULL CHECK (jsonb_typeof(language_task) = 'object'),
    authority_envelope jsonb NOT NULL CHECK (jsonb_typeof(authority_envelope) = 'object'),
    encrypted_input bytea NOT NULL CHECK (octet_length(encrypted_input) > 0),
    request_hash bytea NOT NULL CHECK (octet_length(request_hash) = 32),
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'leased', 'succeeded', 'failed', 'cancelled')),
    attempt integer NOT NULL DEFAULT 0 CHECK (attempt >= 0),
    max_attempts integer NOT NULL CHECK (max_attempts > 0),
    runner_id uuid,
    lease_id uuid,
    leased_at timestamptz,
    lease_expires_at timestamptz,
    completed_at timestamptz,
    encrypted_output bytea,
    output_hash bytea CHECK (output_hash IS NULL OR octet_length(output_hash) = 32),
    failure_code text,
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT agent_invocations_agent_fk
        FOREIGN KEY (project_id, agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE CASCADE,
    CONSTRAINT agent_invocations_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_invocations_local_goal_fk
        FOREIGN KEY (project_id, local_goal_id, local_goal_revision)
        REFERENCES agent_local_goal_contracts (project_id, id, revision)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_invocations_runner_fk
        FOREIGN KEY (project_id, runner_id)
        REFERENCES agent_runners (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_invocations_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT agent_invocations_request_unique UNIQUE (project_id, request_hash),
    CONSTRAINT agent_invocations_local_goal_shape CHECK (
        (local_goal_id IS NULL) = (local_goal_revision IS NULL)
    ),
    CONSTRAINT agent_invocations_lease_shape CHECK (
        (status = 'pending' AND runner_id IS NULL AND lease_id IS NULL
            AND leased_at IS NULL AND lease_expires_at IS NULL
            AND completed_at IS NULL)
        OR (status = 'leased' AND runner_id IS NOT NULL AND lease_id IS NOT NULL
            AND leased_at IS NOT NULL AND lease_expires_at > leased_at
            AND completed_at IS NULL)
        OR (status IN ('succeeded', 'failed', 'cancelled')
            AND completed_at IS NOT NULL)
    ),
    CONSTRAINT agent_invocations_output_shape CHECK (
        (status = 'succeeded' AND encrypted_output IS NOT NULL
            AND output_hash IS NOT NULL AND failure_code IS NULL)
        OR (status = 'failed' AND encrypted_output IS NULL
            AND output_hash IS NULL AND failure_code IS NOT NULL)
        OR (status NOT IN ('succeeded', 'failed'))
    )
);

CREATE INDEX agent_invocations_claim_idx
    ON agent_invocations (project_id, agent_id, created_at, id)
    WHERE status IN ('pending', 'leased');

CREATE TABLE agent_invocation_sources (
    project_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    source_kind text NOT NULL CHECK (source_kind IN (
        'resource_body', 'comment', 'info_document', 'info_file',
        'tool_output', 'proxy_transcript', 'event_history', 'provenance'
    )),
    resource_node_id uuid,
    source_id uuid,
    source_descriptor jsonb NOT NULL CHECK (jsonb_typeof(source_descriptor) = 'object'),
    PRIMARY KEY (project_id, invocation_id, ordinal),
    CONSTRAINT agent_invocation_sources_invocation_fk
        FOREIGN KEY (project_id, invocation_id)
        REFERENCES agent_invocations (project_id, id)
        ON UPDATE RESTRICT ON DELETE CASCADE,
    CONSTRAINT agent_invocation_sources_resource_fk
        FOREIGN KEY (project_id, resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE TABLE agent_effect_proposals (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    effect jsonb NOT NULL CHECK (jsonb_typeof(effect) = 'object'),
    encrypted_materialization bytea,
    proposal_hash bytea NOT NULL CHECK (octet_length(proposal_hash) = 32),
    status text NOT NULL DEFAULT 'accepted'
        CHECK (status IN ('accepted', 'rejected', 'applied')),
    rejection_code text,
    decided_at timestamptz NOT NULL DEFAULT now(),
    applied_at timestamptz,
    CONSTRAINT agent_effects_invocation_fk
        FOREIGN KEY (project_id, invocation_id)
        REFERENCES agent_invocations (project_id, id)
        ON UPDATE RESTRICT ON DELETE CASCADE,
    CONSTRAINT agent_effects_agent_fk
        FOREIGN KEY (project_id, agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE CASCADE,
    CONSTRAINT agent_effects_ordinal_unique
        UNIQUE (project_id, invocation_id, ordinal),
    CONSTRAINT agent_effects_hash_unique UNIQUE (project_id, proposal_hash),
    CONSTRAINT agent_effects_status_shape CHECK (
        (status = 'accepted' AND rejection_code IS NULL AND applied_at IS NULL)
        OR (status = 'rejected' AND rejection_code IS NOT NULL AND applied_at IS NULL)
        OR (status = 'applied' AND rejection_code IS NULL AND applied_at IS NOT NULL)
    )
);

CREATE TABLE agent_audit_log (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    sequence bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    project_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    invocation_id uuid,
    actor_identity_id uuid NOT NULL,
    actor_device_id uuid,
    event_kind text NOT NULL CHECK (event_kind IN (
        'agent_provisioned', 'runner_activated', 'responsibility_recorded',
        'local_goal_recorded', 'invocation_queued', 'invocation_leased',
        'invocation_succeeded', 'invocation_failed', 'effect_rejected',
        'effect_applied', 'runner_revoked'
    )),
    facts jsonb NOT NULL CHECK (jsonb_typeof(facts) = 'object'),
    previous_hash bytea,
    entry_hash bytea NOT NULL CHECK (octet_length(entry_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT agent_audit_agent_fk
        FOREIGN KEY (project_id, agent_id)
        REFERENCES governed_agents (project_id, id)
        ON UPDATE RESTRICT ON DELETE CASCADE,
    CONSTRAINT agent_audit_invocation_fk
        FOREIGN KEY (project_id, invocation_id)
        REFERENCES agent_invocations (project_id, id)
        ON UPDATE RESTRICT ON DELETE CASCADE,
    CONSTRAINT agent_audit_actor_fk
        FOREIGN KEY (actor_identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_audit_device_fk
        FOREIGN KEY (actor_identity_id, actor_device_id)
        REFERENCES devices (identity_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_audit_previous_hash_length
        CHECK (previous_hash IS NULL OR octet_length(previous_hash) = 32)
);

CREATE INDEX agent_audit_lookup_idx
    ON agent_audit_log (project_id, agent_id, sequence);

CREATE FUNCTION sprout_private.validate_governed_agent()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    principal_type text;
    controller_type text;
BEGIN
    SELECT principal_kind INTO principal_type
    FROM identities WHERE id = NEW.principal_identity_id AND status = 'active';
    SELECT principal_kind INTO controller_type
    FROM identities WHERE id = NEW.controller_identity_id AND status = 'active';
    IF principal_type IS DISTINCT FROM 'agent'
       OR controller_type IS NULL
       OR controller_type = 'agent'
    THEN
        RAISE EXCEPTION 'invalid governed agent principal/controller kinds'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM resource_epochs epoch
        WHERE epoch.project_id = NEW.project_id
          AND epoch.resource_node_id = NEW.profile_resource_node_id
          AND epoch.epoch = NEW.key_epoch
          AND epoch.retired_at IS NULL
    ) THEN
        RAISE EXCEPTION 'agent prompt must use an active resource epoch'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER governed_agents_validate
BEFORE INSERT OR UPDATE ON governed_agents
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_governed_agent();

CREATE FUNCTION sprout_private.validate_agent_runner()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM governed_agents agent
        JOIN devices device
          ON device.identity_id = agent.principal_identity_id
         AND device.id = NEW.device_id
        WHERE agent.project_id = NEW.project_id
          AND agent.id = NEW.agent_id
          AND agent.principal_identity_id = NEW.principal_identity_id
          AND device.device_kind = 'service'
          AND device.trust_state = 'trusted'
          AND device.retired_at IS NULL
    ) THEN
        RAISE EXCEPTION 'agent runner must be a trusted service device of the agent principal'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.state = 'active' AND NOT EXISTS (
        SELECT 1 FROM device_keys key
        WHERE key.identity_id = NEW.principal_identity_id
          AND key.device_id = NEW.device_id
          AND key.key_version = NEW.activated_key_version
          AND key.revoked_at IS NULL
    ) THEN
        RAISE EXCEPTION 'agent runner activation requires an active device key'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_runners_validate
BEFORE INSERT OR UPDATE ON agent_runners
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_runner();

-- Identity/device tables are FORCE RLS self-only. Provisioning therefore uses
-- one narrow SECURITY DEFINER operation which rechecks project management and
-- creates no permissions or key envelopes for the new principal.
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
    candidate_session_expires_at timestamptz
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
    IF NOT EXISTS (
        SELECT 1 FROM project_memberships membership
        WHERE membership.project_id = candidate_project_id
          AND membership.identity_id = caller_identity_id
          AND membership.state = 'active'
          AND membership.role IN ('owner', 'admin')
    ) THEN
        RAISE EXCEPTION 'project management permission required'
            USING ERRCODE = '42501';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM project_memberships membership
        JOIN identities identity ON identity.id = membership.identity_id
        WHERE membership.project_id = candidate_project_id
          AND membership.identity_id = candidate_controller_identity_id
          AND membership.state = 'active'
          AND identity.status = 'active'
          AND identity.principal_kind = 'user'
    ) THEN
        RAISE EXCEPTION 'agent controller must be an active human project member'
            USING ERRCODE = '23514';
    END IF;
    IF candidate_session_expires_at <= clock_timestamp()
       OR octet_length(candidate_token_hash) < 32
    THEN
        RAISE EXCEPTION 'invalid runner bootstrap session'
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
    uuid, uuid, bytea, uuid, bytea, timestamptz
) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.provision_edge_agent(
    uuid, uuid, uuid, uuid, text, bytea, uuid, bytea, integer, text,
    uuid, uuid, bytea, uuid, bytea, timestamptz
) TO PUBLIC;

CREATE FUNCTION sprout_private.reject_agent_audit_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'agent audit records are append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_audit_append_only
BEFORE UPDATE OR DELETE ON agent_audit_log
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_audit_mutation();

CREATE FUNCTION sprout_private.agent_party_access(
    candidate_project_id uuid,
    candidate_agent_id uuid
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
        FROM governed_agents agent
        JOIN project_memberships requester
          ON requester.project_id = agent.project_id
         AND requester.identity_id = sprout_private.current_identity_id()
         AND requester.state = 'active'
        WHERE agent.project_id = candidate_project_id
          AND agent.id = candidate_agent_id
          AND (
              agent.principal_identity_id = requester.identity_id
              OR agent.controller_identity_id = requester.identity_id
              OR requester.role IN ('owner', 'admin')
          )
    )
$$;

REVOKE ALL ON FUNCTION sprout_private.agent_party_access(uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.agent_party_access(uuid, uuid) TO PUBLIC;

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'governed_agents',
        'agent_runners',
        'agent_responsibility_contracts',
        'agent_local_goal_contracts',
        'agent_invocations',
        'agent_invocation_sources',
        'agent_effect_proposals',
        'agent_audit_log'
    ]
    LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
    END LOOP;
END;
$$;

CREATE POLICY agent_party_isolation ON governed_agents
    USING (sprout_private.agent_party_access(project_id, id))
    WITH CHECK (EXISTS (
        SELECT 1 FROM project_memberships membership
        WHERE membership.project_id = governed_agents.project_id
          AND membership.identity_id = sprout_private.current_identity_id()
          AND membership.state = 'active'
          AND membership.role IN ('owner', 'admin')
    ));
CREATE POLICY agent_runner_party_isolation ON agent_runners
    USING (sprout_private.agent_party_access(project_id, agent_id))
    WITH CHECK (sprout_private.agent_party_access(project_id, agent_id));
CREATE POLICY responsibility_party_isolation ON agent_responsibility_contracts
    USING (
        sprout_private.is_project_member(project_id)
        AND (
            administrator_identity_id = sprout_private.current_identity_id()
            OR user_identity_id = sprout_private.current_identity_id()
            OR EXISTS (
                SELECT 1 FROM project_memberships membership
                WHERE membership.project_id = agent_responsibility_contracts.project_id
                  AND membership.identity_id = sprout_private.current_identity_id()
                  AND membership.state = 'active'
                  AND membership.role IN ('owner', 'admin')
            )
        )
    )
    WITH CHECK (
        sprout_private.is_project_member(project_id)
        AND administrator_identity_id = sprout_private.current_identity_id()
    );
CREATE POLICY local_goal_party_isolation ON agent_local_goal_contracts
    USING (sprout_private.agent_party_access(project_id, agent_id))
    WITH CHECK (sprout_private.agent_party_access(project_id, agent_id));
CREATE POLICY invocation_party_isolation ON agent_invocations
    USING (sprout_private.agent_party_access(project_id, agent_id))
    WITH CHECK (sprout_private.agent_party_access(project_id, agent_id));
CREATE POLICY invocation_source_party_isolation ON agent_invocation_sources
    USING (EXISTS (
        SELECT 1 FROM agent_invocations invocation
        WHERE invocation.project_id = agent_invocation_sources.project_id
          AND invocation.id = agent_invocation_sources.invocation_id
          AND sprout_private.agent_party_access(invocation.project_id, invocation.agent_id)
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM agent_invocations invocation
        WHERE invocation.project_id = agent_invocation_sources.project_id
          AND invocation.id = agent_invocation_sources.invocation_id
          AND sprout_private.agent_party_access(invocation.project_id, invocation.agent_id)
    ));
CREATE POLICY effect_party_isolation ON agent_effect_proposals
    USING (sprout_private.agent_party_access(project_id, agent_id))
    WITH CHECK (sprout_private.agent_party_access(project_id, agent_id));
CREATE POLICY agent_audit_party_isolation ON agent_audit_log
    USING (sprout_private.agent_party_access(project_id, agent_id))
    WITH CHECK (sprout_private.agent_party_access(project_id, agent_id));

REVOKE ALL ON FUNCTION sprout_private.validate_governed_agent() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_runner() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_audit_mutation() FROM PUBLIC;
