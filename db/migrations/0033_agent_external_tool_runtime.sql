-- Client-owned governed external-tool runtime. Connector execution, plaintext,
-- credentials, endpoints, cookies and filesystem paths remain on the authorized
-- device. PostgreSQL persists only exact work/call coordinates, ciphertext,
-- commitments, bounded leases and append-only structural audit.

-- A retry re-arm is a semantic transition distinct from both the failed
-- WorkOutcome and the later WorkAttempt claim. This preserves the exact
-- failed-attempt snapshot required by R5 before materializing attempt N+1.
ALTER TABLE agent_run_transitions
    DROP CONSTRAINT agent_run_transitions_transition_kind_check;
ALTER TABLE agent_run_transitions
    ADD CONSTRAINT agent_run_transitions_transition_kind_check CHECK (transition_kind IN (
        'initialized', 'frontier_refreshed', 'work_claimed',
        'claim_recovered', 'work_succeeded', 'work_failed',
        'tool_retry_rearmed',
        'blocker_created', 'blocker_resolved', 'evidence_accepted',
        'goal_completed', 'run_completed'
    ));

CREATE TABLE agent_external_tool_catalog (
    tool_name text NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    adapter_protocol text NOT NULL,
    operation text NOT NULL CHECK (operation IN ('read', 'edit', 'send')),
    risk_tier text NOT NULL CHECK (risk_tier IN ('tr0', 'tr1', 'tr2', 'tr3')),
    availability text NOT NULL CHECK (availability IN (
        'executable', 'contract_only', 'fail_closed'
    )),
    effect_class text NOT NULL CHECK (effect_class IN (
        'no_sprout_mutation', 'external_network_egress_boundary',
        'external_side_effect_boundary',
        'external_disclosure_unsupported'
    )),
    max_attempts integer NOT NULL CHECK (max_attempts BETWEEN 1 AND 16),
    max_timeout_seconds integer NOT NULL CHECK (max_timeout_seconds BETWEEN 1 AND 300),
    max_input_bytes integer NOT NULL CHECK (max_input_bytes BETWEEN 1 AND 1048576),
    max_output_bytes integer NOT NULL CHECK (max_output_bytes BETWEEN 1 AND 1048576),
    input_schema jsonb NOT NULL CHECK (jsonb_typeof(input_schema) = 'object'),
    input_schema_hash bytea NOT NULL CHECK (octet_length(input_schema_hash) = 32),
    output_schema jsonb NOT NULL CHECK (jsonb_typeof(output_schema) = 'object'),
    output_schema_hash bytea NOT NULL CHECK (octet_length(output_schema_hash) = 32),
    required_effects jsonb NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(required_effects) = 'array'),
    output_audience_kind text NOT NULL DEFAULT 'owner_from_canonical_input'
        CHECK (output_audience_kind = 'owner_from_canonical_input'),
    terminal_status_mapping jsonb NOT NULL CHECK (
        terminal_status_mapping = '{"cancelled":"failed","error":"failed","success":"succeeded","timeout":"timed_out"}'::jsonb
    ),
    manifest_hash bytea NOT NULL CHECK (octet_length(manifest_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (tool_name, version),
    CONSTRAINT agent_external_tool_catalog_id_check CHECK (
        tool_name ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$'
        AND tool_name NOT LIKE 'workspace.%'
        AND tool_name NOT LIKE 'task.%'
        AND tool_name NOT LIKE 'topic.%'
        AND tool_name NOT LIKE 'task_list.%'
        AND tool_name NOT LIKE 'info.%'
        AND tool_name NOT LIKE 'comment.%'
    )
);

INSERT INTO agent_external_tool_catalog (
    tool_name, version, adapter_protocol, operation, risk_tier, availability,
    effect_class, max_attempts, max_timeout_seconds,
    max_input_bytes, max_output_bytes, input_schema, input_schema_hash,
    output_schema, output_schema_hash, terminal_status_mapping, manifest_hash
)
SELECT row.tool_id, row.version, row.adapter_protocol, row.operation,
       row.risk_tier, row.availability, row.effect_class,
       row.max_attempts, row.max_timeout_seconds,
       row.max_input_bytes, row.max_output_bytes,
       row.input_schema::jsonb, public.digest((row.input_schema::jsonb)::text, 'sha256'),
       row.output_schema::jsonb, public.digest((row.output_schema::jsonb)::text, 'sha256'),
       '{"cancelled":"failed","error":"failed","success":"succeeded","timeout":"timed_out"}'::jsonb,
       public.digest(convert_to(concat_ws(E'\n',
           'sprout-external-tool-manifest-v1', row.tool_id, row.version::text,
           row.adapter_protocol, row.operation, row.risk_tier, row.availability,
           row.effect_class, row.max_attempts::text, row.max_timeout_seconds::text,
           row.max_input_bytes::text, row.max_output_bytes::text,
           (row.input_schema::jsonb)::text, (row.output_schema::jsonb)::text,
           'owner_from_canonical_input', '[]',
           '{"cancelled":"failed","error":"failed","success":"succeeded","timeout":"timed_out"}'::jsonb::text
       ), 'UTF8'), 'sha256')
FROM (VALUES
('web.read', 1, 'sprout-edge-web-read-v1', 'read', 'tr2', 'executable',
 'external_network_egress_boundary', 4, 60, 16384, 1048576,
 '{"type":"object","additionalProperties":false,"required":["url"],"properties":{"url":{"type":"string"}}}',
 '{"type":"object","additionalProperties":false,"required":["final_url","content_type","text","title"],"properties":{"final_url":{"type":"string"},"content_type":{"type":"string"},"text":{"type":"string"},"title":{"type":["string","null"]}}}'),
('document.local.read', 1, 'sprout-edge-document-read-v1', 'read', 'tr1', 'executable',
 'no_sprout_mutation', 4, 60, 16384, 1048576,
 '{"type":"object","additionalProperties":false,"required":["document_capability_id"],"properties":{"document_capability_id":{"type":"string"}}}',
 '{"type":"object","additionalProperties":false,"required":["content","version_hash"],"properties":{"content":{"type":"string"},"version_hash":{"type":"string"}}}'),
('document.local.edit', 1, 'sprout-edge-document-edit-v1', 'edit', 'tr1', 'contract_only',
 'external_side_effect_boundary', 1, 60, 1048576, 16384,
 '{"type":"object","additionalProperties":false,"required":["document_capability_id","expected_version_hash","replacement"],"properties":{"document_capability_id":{"type":"string"},"expected_version_hash":{"type":"string"},"replacement":{"type":"string"}}}',
 '{"type":"object","additionalProperties":false,"required":["version_hash"],"properties":{"version_hash":{"type":"string"}}}'),
('mail.receive', 1, 'sprout-edge-mail-receive-v1', 'read', 'tr2', 'contract_only',
 'no_sprout_mutation', 4, 60, 16384, 1048576,
 '{"type":"object","additionalProperties":false}',
 '{"type":"object","additionalProperties":false}'),
('mail.send', 1, 'sprout-edge-mail-send-v1', 'send', 'tr3', 'fail_closed',
 'external_disclosure_unsupported', 1, 60, 16384, 16384,
 '{"type":"object","additionalProperties":false}',
 '{"type":"object","additionalProperties":false}'),
('telegram.receive', 1, 'sprout-edge-telegram-receive-v1', 'read', 'tr2', 'contract_only',
 'no_sprout_mutation', 4, 60, 16384, 1048576,
 '{"type":"object","additionalProperties":false}',
 '{"type":"object","additionalProperties":false}'),
('telegram.send', 1, 'sprout-edge-telegram-send-v1', 'send', 'tr3', 'fail_closed',
 'external_disclosure_unsupported', 1, 60, 16384, 16384,
 '{"type":"object","additionalProperties":false}',
 '{"type":"object","additionalProperties":false}')
) AS row(
    tool_id, version, adapter_protocol, operation, risk_tier, availability,
    effect_class, max_attempts, max_timeout_seconds, max_input_bytes,
    max_output_bytes, input_schema, output_schema
);

CREATE TABLE agent_tool_permissions (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    principal_identity_id uuid NOT NULL,
    tool_name text NOT NULL,
    tool_version integer NOT NULL,
    granted_by_identity_id uuid NOT NULL,
    idempotency_key uuid NOT NULL,
    grant_hash bytea NOT NULL CHECK (octet_length(grant_hash) = 32),
    granted_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    revoked_at timestamptz,
    revoked_by_identity_id uuid,
    CONSTRAINT agent_tool_permission_principal_fk
        FOREIGN KEY (project_id, principal_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_permission_catalog_fk
        FOREIGN KEY (tool_name, tool_version)
        REFERENCES agent_external_tool_catalog (tool_name, version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_permission_grantor_fk
        FOREIGN KEY (project_id, granted_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_permission_revoker_fk
        FOREIGN KEY (project_id, revoked_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_permission_state_shape CHECK (
        (revoked_at IS NULL AND revoked_by_identity_id IS NULL)
        OR (revoked_at IS NOT NULL AND revoked_by_identity_id IS NOT NULL)
    ),
    CONSTRAINT agent_tool_permission_identity_unique
        UNIQUE (project_id, principal_identity_id, tool_name, tool_version, id),
    CONSTRAINT agent_tool_permission_idempotency_unique
        UNIQUE (project_id, granted_by_identity_id, idempotency_key)
);

CREATE UNIQUE INDEX agent_tool_permission_active_unique
    ON agent_tool_permissions (project_id, principal_identity_id, tool_name, tool_version)
    WHERE revoked_at IS NULL;

-- Tool permissions are a principal-level product authorization ledger. The
-- normal application path never receives table DML: the Rust route validates
-- the request and this narrow writer independently revalidates the current
-- human caller, exact project/scope/target and Manage decision. A compromised
-- Rust TCB/database owner remains inside the documented trust boundary.
CREATE FUNCTION sprout_private.grant_agent_tool_permission(
    candidate_id uuid,
    candidate_project_id uuid,
    candidate_scope_id uuid,
    candidate_target_principal_id uuid,
    candidate_target_agent_id uuid,
    candidate_tool_id text,
    candidate_tool_version integer,
    candidate_grantor_identity_id uuid,
    candidate_idempotency_key uuid,
    candidate_grant_hash bytea
)
RETURNS TABLE (permission_id uuid, replayed boolean, active boolean)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
DECLARE
    existing public.agent_tool_permissions%ROWTYPE;
BEGIN
    IF candidate_grantor_identity_id IS DISTINCT FROM sprout_private.current_identity_id()
       OR NULLIF(current_setting('app.project_id', true), '')::uuid
            IS DISTINCT FROM candidate_project_id
       OR octet_length(candidate_grant_hash) <> 32
       OR NOT EXISTS (
            SELECT 1
              FROM public.identities identity
              JOIN public.project_memberships membership
                ON membership.project_id = candidate_project_id
               AND membership.identity_id = identity.id
               AND membership.state = 'active'
             WHERE identity.id = candidate_grantor_identity_id
               AND identity.status = 'active'
               AND identity.principal_kind = 'user'
       )
       OR NOT EXISTS (
            SELECT 1
              FROM public.resource_nodes node
              JOIN public.projects project ON project.id = node.project_id
              JOIN public.project_memberships membership
                ON membership.project_id = node.project_id
               AND membership.identity_id = candidate_grantor_identity_id
               AND membership.state = 'active'
              LEFT JOIN LATERAL sprout_private.effective_domain_permission(
                    candidate_project_id, candidate_scope_id,
                    candidate_grantor_identity_id
              ) permission ON true
             WHERE node.project_id = candidate_project_id
               AND node.id = candidate_scope_id
               AND node.deleted_at IS NULL
               AND (
                    project.owner_identity_id = candidate_grantor_identity_id
                    OR membership.role = 'admin'
                    OR (
                        permission.access_level = 'manage'
                        AND permission.access_scope = 'full'
                    )
               )
       )
       OR NOT EXISTS (
            SELECT 1 FROM public.agent_external_tool_catalog catalog
             WHERE catalog.tool_name = candidate_tool_id
               AND catalog.version = candidate_tool_version
               AND catalog.availability <> 'fail_closed'
       )
       OR NOT EXISTS (
            SELECT 1 FROM public.project_memberships target
             WHERE target.project_id = candidate_project_id
               AND target.identity_id = candidate_target_principal_id
               AND target.state = 'active'
       )
       OR (candidate_target_agent_id IS NULL AND NOT EXISTS (
            SELECT 1 FROM public.identities target_identity
             WHERE target_identity.id = candidate_target_principal_id
               AND target_identity.principal_kind = 'user'
               AND target_identity.status = 'active'
       ))
    THEN
        RAISE EXCEPTION 'untrusted tool permission grant'
            USING ERRCODE = '42501';
    END IF;

    IF candidate_target_agent_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM public.governed_agents agent
         WHERE agent.project_id = candidate_project_id
           AND agent.id = candidate_target_agent_id
           AND agent.principal_identity_id = candidate_target_principal_id
           AND agent.controller_identity_id = candidate_grantor_identity_id
           AND agent.profile_resource_node_id = candidate_scope_id
           AND agent.state = 'active'
    ) THEN
        RAISE EXCEPTION 'tool permission target agent mismatch'
            USING ERRCODE = '42501';
    END IF;

    PERFORM pg_advisory_xact_lock(hashtextextended(
        concat_ws(':', candidate_project_id::text,
                       candidate_target_principal_id::text,
                       candidate_tool_id, candidate_tool_version::text), 33
    ));
    SELECT * INTO existing
      FROM public.agent_tool_permissions permission
     WHERE permission.project_id = candidate_project_id
       AND permission.granted_by_identity_id = candidate_grantor_identity_id
       AND permission.idempotency_key = candidate_idempotency_key;
    IF FOUND THEN
        IF existing.id IS DISTINCT FROM candidate_id
           OR existing.principal_identity_id IS DISTINCT FROM candidate_target_principal_id
           OR existing.tool_name IS DISTINCT FROM candidate_tool_id
           OR existing.tool_version IS DISTINCT FROM candidate_tool_version
           OR existing.grant_hash IS DISTINCT FROM candidate_grant_hash
        THEN
            RAISE EXCEPTION 'tool permission grant equivocation'
                USING ERRCODE = '23505';
        END IF;
        permission_id := existing.id;
        replayed := true;
        active := existing.revoked_at IS NULL;
        RETURN NEXT;
        RETURN;
    END IF;

    INSERT INTO public.agent_tool_permissions (
        id, project_id, principal_identity_id, tool_name, tool_version,
        granted_by_identity_id, idempotency_key, grant_hash
    ) VALUES (
        candidate_id, candidate_project_id, candidate_target_principal_id,
        candidate_tool_id, candidate_tool_version,
        candidate_grantor_identity_id, candidate_idempotency_key,
        candidate_grant_hash
    );
    permission_id := candidate_id;
    replayed := false;
    active := true;
    RETURN NEXT;
END;
$$;

CREATE FUNCTION sprout_private.revoke_agent_tool_permission(
    candidate_project_id uuid,
    candidate_scope_id uuid,
    candidate_target_principal_id uuid,
    candidate_target_agent_id uuid,
    candidate_tool_id text,
    candidate_tool_version integer,
    candidate_revoker_identity_id uuid
)
RETURNS TABLE (permission_id uuid, replayed boolean, active boolean)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
DECLARE
    current_permission public.agent_tool_permissions%ROWTYPE;
BEGIN
    -- Reuse the exact caller/scope/target authorization predicate from grant.
    IF candidate_revoker_identity_id IS DISTINCT FROM sprout_private.current_identity_id()
       OR NULLIF(current_setting('app.project_id', true), '')::uuid
            IS DISTINCT FROM candidate_project_id
       OR NOT EXISTS (
            SELECT 1
              FROM public.identities identity
              JOIN public.project_memberships membership
                ON membership.project_id = candidate_project_id
               AND membership.identity_id = identity.id
               AND membership.state = 'active'
              JOIN public.resource_nodes node
                ON node.project_id = membership.project_id
               AND node.id = candidate_scope_id
               AND node.deleted_at IS NULL
              JOIN public.projects project ON project.id = node.project_id
              LEFT JOIN LATERAL sprout_private.effective_domain_permission(
                    candidate_project_id, candidate_scope_id,
                    candidate_revoker_identity_id
              ) permission ON true
             WHERE identity.id = candidate_revoker_identity_id
               AND identity.status = 'active'
               AND identity.principal_kind = 'user'
               AND (
                    project.owner_identity_id = candidate_revoker_identity_id
                    OR membership.role = 'admin'
                    OR (
                        permission.access_level = 'manage'
                        AND permission.access_scope = 'full'
                    )
               )
       )
       OR (candidate_target_agent_id IS NOT NULL AND NOT EXISTS (
            SELECT 1 FROM public.governed_agents agent
             WHERE agent.project_id = candidate_project_id
               AND agent.id = candidate_target_agent_id
               AND agent.principal_identity_id = candidate_target_principal_id
               AND agent.controller_identity_id = candidate_revoker_identity_id
               AND agent.profile_resource_node_id = candidate_scope_id
               AND agent.state = 'active'
       ))
       OR NOT EXISTS (
            SELECT 1 FROM public.project_memberships target
             WHERE target.project_id = candidate_project_id
               AND target.identity_id = candidate_target_principal_id
               AND target.state = 'active'
       )
       OR (candidate_target_agent_id IS NULL AND NOT EXISTS (
            SELECT 1 FROM public.identities target_identity
             WHERE target_identity.id = candidate_target_principal_id
               AND target_identity.principal_kind = 'user'
               AND target_identity.status = 'active'
       ))
    THEN
        RAISE EXCEPTION 'untrusted tool permission revocation'
            USING ERRCODE = '42501';
    END IF;

    PERFORM pg_advisory_xact_lock(hashtextextended(
        concat_ws(':', candidate_project_id::text,
                       candidate_target_principal_id::text,
                       candidate_tool_id, candidate_tool_version::text), 33
    ));
    SELECT * INTO current_permission
      FROM public.agent_tool_permissions permission
     WHERE permission.project_id = candidate_project_id
       AND permission.principal_identity_id = candidate_target_principal_id
       AND permission.tool_name = candidate_tool_id
       AND permission.tool_version = candidate_tool_version
     ORDER BY permission.granted_at DESC
     LIMIT 1
     FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'tool permission not found' USING ERRCODE = '02000';
    END IF;
    IF current_permission.revoked_at IS NOT NULL THEN
        IF current_permission.revoked_by_identity_id
              IS DISTINCT FROM candidate_revoker_identity_id
        THEN
            RAISE EXCEPTION 'tool permission revocation equivocation'
                USING ERRCODE = '23505';
        END IF;
        permission_id := current_permission.id;
        replayed := true;
        active := false;
        RETURN NEXT;
        RETURN;
    END IF;
    UPDATE public.agent_tool_permissions permission
       SET revoked_at = clock_timestamp(),
           revoked_by_identity_id = candidate_revoker_identity_id
     WHERE permission.id = current_permission.id;
    permission_id := current_permission.id;
    replayed := false;
    active := false;
    RETURN NEXT;
END;
$$;

-- Immutable run/work ceilings are copied from the certified GoalContract at
-- run creation. Existing 0032 runs receive no synthetic snapshot and therefore
-- remain fail-closed for external tool invocation.
CREATE TABLE agent_run_tool_security_snapshots (
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    contract_source_kind text NOT NULL CHECK (contract_source_kind IN ('local')),
    contract_source_id uuid NOT NULL,
    contract_source_revision bigint NOT NULL CHECK (contract_source_revision > 0),
    run_sponsor_identity_id uuid NOT NULL,
    run_tool_ceiling jsonb NOT NULL CHECK (jsonb_typeof(run_tool_ceiling) = 'array'),
    run_tool_ceiling_hash bytea NOT NULL CHECK (octet_length(run_tool_ceiling_hash) = 32),
    work_policies jsonb NOT NULL CHECK (jsonb_typeof(work_policies) = 'array'),
    work_policies_hash bytea NOT NULL CHECK (octet_length(work_policies_hash) = 32),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (project_id, run_id),
    CONSTRAINT agent_run_tool_snapshot_run_fk
        FOREIGN KEY (project_id, run_id)
        REFERENCES agent_collaborative_runs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_tool_snapshot_sponsor_fk
        FOREIGN KEY (project_id, run_sponsor_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

-- A migration-owned manifest is not runtime availability. This short-lived,
-- hybrid-signed witness comes from the exact active local-edge runner and leaks
-- neither connector endpoint nor credential.
CREATE TABLE agent_tool_runtime_capability_witnesses (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    owner_identity_id uuid NOT NULL,
    runner_id uuid NOT NULL,
    signer_device_id uuid NOT NULL,
    signer_device_key_version integer NOT NULL CHECK (signer_device_key_version > 0),
    tool_name text NOT NULL,
    tool_version integer NOT NULL,
    manifest_hash bytea NOT NULL CHECK (octet_length(manifest_hash) = 32),
    execution_profile_commitment bytea NOT NULL CHECK (octet_length(execution_profile_commitment) = 32),
    issued_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    classical_signature bytea NOT NULL CHECK (octet_length(classical_signature) = 64),
    post_quantum_signature bytea NOT NULL CHECK (octet_length(post_quantum_signature) > 0),
    statement_hash bytea NOT NULL CHECK (octet_length(statement_hash) = 32),
    idempotency_key uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_tool_runtime_witness_agent_fk
        FOREIGN KEY (project_id, agent_id, owner_identity_id)
        REFERENCES governed_agents (project_id, id, principal_identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_runtime_witness_runner_fk
        FOREIGN KEY (project_id, runner_id)
        REFERENCES agent_runners (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_runtime_witness_key_fk
        FOREIGN KEY (owner_identity_id, signer_device_id, signer_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_runtime_witness_catalog_fk
        FOREIGN KEY (tool_name, tool_version)
        REFERENCES agent_external_tool_catalog (tool_name, version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_runtime_witness_time CHECK (
        expires_at > issued_at AND expires_at <= issued_at + interval '5 minutes'
    ),
    CONSTRAINT agent_tool_runtime_witness_idempotency UNIQUE (
        project_id, owner_identity_id, idempotency_key
    ),
    CONSTRAINT agent_tool_runtime_witness_statement UNIQUE (project_id, statement_hash)
);

CREATE TABLE agent_tool_calls (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    work_claim_id uuid NOT NULL,
    work_attempt integer NOT NULL CHECK (work_attempt > 0),
    work_spec_ordinal bigint NOT NULL CHECK (work_spec_ordinal >= 0),
    owner_identity_id uuid NOT NULL,
    work_authority_origin text NOT NULL CHECK (
        work_authority_origin IN ('run_sponsor', 'inherited_work')
    ),
    work_authority_parent_id uuid,
    work_authority_principal_id uuid NOT NULL,
    tool_name text NOT NULL,
    tool_version integer NOT NULL,
    runtime_capability_witness_id uuid NOT NULL,
    adapter_protocol text NOT NULL,
    encrypted_input bytea CHECK (encrypted_input IS NULL OR octet_length(encrypted_input) > 0),
    encrypted_input_payload_commitment bytea NOT NULL
        CHECK (octet_length(encrypted_input_payload_commitment) = 32),
    canonical_input_commitment bytea NOT NULL
        CHECK (octet_length(canonical_input_commitment) = 32),
    canonical_input_statement text NOT NULL CHECK (octet_length(canonical_input_statement) > 0),
    input_signer_device_id uuid NOT NULL,
    input_signer_device_key_version integer NOT NULL CHECK (input_signer_device_key_version > 0),
    input_classical_signature bytea NOT NULL CHECK (octet_length(input_classical_signature) = 64),
    input_post_quantum_signature bytea NOT NULL CHECK (octet_length(input_post_quantum_signature) > 0),
    security_policy_hash bytea NOT NULL CHECK (octet_length(security_policy_hash) = 32),
    run_tool_ceiling_hash bytea NOT NULL CHECK (octet_length(run_tool_ceiling_hash) = 32),
    work_tool_ceiling_hash bytea NOT NULL CHECK (octet_length(work_tool_ceiling_hash) = 32),
    work_tool_ceiling jsonb NOT NULL CHECK (jsonb_typeof(work_tool_ceiling) = 'array'),
    required_effects jsonb NOT NULL CHECK (jsonb_typeof(required_effects) = 'array'),
    output_readable_by jsonb NOT NULL CHECK (jsonb_typeof(output_readable_by) = 'array'),
    max_attempts integer NOT NULL CHECK (max_attempts BETWEEN 1 AND 16),
    timeout_seconds integer NOT NULL CHECK (timeout_seconds BETWEEN 1 AND 300),
    current_attempt integer NOT NULL DEFAULT 1 CHECK (current_attempt BETWEEN 1 AND 16),
    current_status text NOT NULL DEFAULT 'pending'
        CHECK (current_status IN ('pending', 'succeeded', 'failed', 'timed_out')),
    current_output_commitment bytea CHECK (
        current_output_commitment IS NULL OR octet_length(current_output_commitment) = 32
    ),
    idempotency_key uuid NOT NULL,
    request_hash bytea NOT NULL CHECK (octet_length(request_hash) = 32),
    requested_tick bigint NOT NULL CHECK (requested_tick >= 0),
    requested_at timestamptz NOT NULL,
    tool_deadline_tick bigint NOT NULL CHECK (tool_deadline_tick > requested_tick),
    tool_deadline_at timestamptz NOT NULL CHECK (tool_deadline_at > requested_at),
    terminal_at timestamptz,
    payload_purged_at timestamptz,
    CONSTRAINT agent_tool_call_run_fk
        FOREIGN KEY (project_id, run_id)
        REFERENCES agent_collaborative_runs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_call_work_fk
        FOREIGN KEY (project_id, run_id, work_item_id)
        REFERENCES agent_run_work_slots (project_id, run_id, work_item_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_call_claim_fk
        FOREIGN KEY (project_id, work_claim_id)
        REFERENCES agent_run_claim_leases (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_call_owner_fk
        FOREIGN KEY (project_id, owner_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_call_authority_principal_fk
        FOREIGN KEY (project_id, work_authority_principal_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_call_authority_origin_shape CHECK (
        (work_authority_origin = 'run_sponsor' AND work_authority_parent_id IS NULL)
        OR (work_authority_origin = 'inherited_work' AND work_authority_parent_id IS NOT NULL)
    ),
    CONSTRAINT agent_tool_call_catalog_fk
        FOREIGN KEY (tool_name, tool_version)
        REFERENCES agent_external_tool_catalog (tool_name, version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_call_runtime_witness_fk
        FOREIGN KEY (runtime_capability_witness_id)
        REFERENCES agent_tool_runtime_capability_witnesses (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_call_input_signer_fk
        FOREIGN KEY (owner_identity_id, input_signer_device_id, input_signer_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_call_security_snapshot_fk
        FOREIGN KEY (project_id, run_id)
        REFERENCES agent_run_tool_security_snapshots (project_id, run_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_call_terminal_shape CHECK (
        (current_status = 'pending' AND terminal_at IS NULL
         AND current_output_commitment IS NULL)
        OR (current_status = 'succeeded' AND terminal_at IS NOT NULL
            AND current_output_commitment IS NOT NULL)
        OR (current_status IN ('failed', 'timed_out') AND terminal_at IS NOT NULL
            AND current_output_commitment IS NULL)
    ),
    CONSTRAINT agent_tool_call_attempt_bound CHECK (current_attempt <= max_attempts),
    CONSTRAINT agent_tool_call_payload_retention_shape CHECK (
        (payload_purged_at IS NULL AND encrypted_input IS NOT NULL)
        OR (payload_purged_at IS NOT NULL AND encrypted_input IS NULL)
    ),
    CONSTRAINT agent_tool_call_idempotency_unique
        UNIQUE (project_id, owner_identity_id, idempotency_key),
    CONSTRAINT agent_tool_call_request_unique UNIQUE (project_id, request_hash),
    CONSTRAINT agent_tool_call_work_attempt_unique
        UNIQUE (project_id, run_id, work_item_id, work_attempt)
);

CREATE INDEX agent_tool_calls_pending_idx
    ON agent_tool_calls (project_id, owner_identity_id, requested_at, id)
    WHERE current_status = 'pending';

CREATE TABLE agent_tool_attempt_dispatches (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    call_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    work_claim_id uuid NOT NULL,
    work_attempt integer NOT NULL CHECK (work_attempt > 0),
    owner_identity_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    lease_id uuid NOT NULL,
    runner_id uuid NOT NULL,
    runner_identity_id uuid NOT NULL,
    runner_device_id uuid NOT NULL,
    runner_key_version integer NOT NULL CHECK (runner_key_version > 0),
    runtime_capability_witness_id uuid NOT NULL,
    adapter_protocol text NOT NULL,
    canonical_input_commitment bytea NOT NULL CHECK (octet_length(canonical_input_commitment) = 32),
    execution_profile_commitment bytea NOT NULL
        CHECK (octet_length(execution_profile_commitment) = 32),
    requested_at timestamptz NOT NULL,
    dispatched_at timestamptz NOT NULL,
    lease_expires_at timestamptz NOT NULL,
    tool_deadline_at timestamptz NOT NULL,
    CONSTRAINT agent_tool_dispatch_call_fk
        FOREIGN KEY (call_id) REFERENCES agent_tool_calls (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_dispatch_runner_fk
        FOREIGN KEY (project_id, runner_id)
        REFERENCES agent_runners (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_dispatch_runtime_witness_fk
        FOREIGN KEY (runtime_capability_witness_id)
        REFERENCES agent_tool_runtime_capability_witnesses (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_dispatch_attempt_unique UNIQUE (project_id, call_id, attempt),
    CONSTRAINT agent_tool_dispatch_lease_unique UNIQUE (project_id, lease_id),
    CONSTRAINT agent_tool_dispatch_time_shape CHECK (
        requested_at <= dispatched_at
        AND lease_expires_at > dispatched_at
        AND tool_deadline_at > dispatched_at
    )
);

-- The exact request witness is written before the edge performs the outbound
-- request. It contains no authorization header, credential, endpoint plaintext
-- or connector configuration: only a commitment to the canonical wire request.
CREATE TABLE agent_tool_attempt_requests (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    dispatch_id uuid NOT NULL,
    call_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    adapter_protocol text NOT NULL,
    canonical_input_commitment bytea NOT NULL CHECK (octet_length(canonical_input_commitment) = 32),
    wire_request_commitment bytea NOT NULL CHECK (octet_length(wire_request_commitment) = 32),
    execution_profile_commitment bytea NOT NULL CHECK (octet_length(execution_profile_commitment) = 32),
    signer_identity_id uuid NOT NULL,
    signer_device_id uuid NOT NULL,
    signer_device_key_version integer NOT NULL CHECK (signer_device_key_version > 0),
    signed_at timestamptz NOT NULL,
    classical_signature bytea NOT NULL CHECK (octet_length(classical_signature) = 64),
    post_quantum_signature bytea NOT NULL CHECK (octet_length(post_quantum_signature) > 0),
    statement_hash bytea NOT NULL CHECK (octet_length(statement_hash) = 32),
    idempotency_key uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_tool_request_dispatch_fk
        FOREIGN KEY (dispatch_id) REFERENCES agent_tool_attempt_dispatches (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_request_call_fk
        FOREIGN KEY (call_id) REFERENCES agent_tool_calls (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_request_signer_key_fk
        FOREIGN KEY (signer_identity_id, signer_device_id, signer_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_request_attempt_unique UNIQUE (project_id, call_id, attempt),
    CONSTRAINT agent_tool_request_dispatch_unique UNIQUE (project_id, dispatch_id),
    CONSTRAINT agent_tool_request_idempotency UNIQUE (project_id, call_id, idempotency_key),
    CONSTRAINT agent_tool_request_statement_unique UNIQUE (project_id, statement_hash)
);

CREATE TABLE agent_tool_attempt_observations (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    dispatch_id uuid,
    call_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    lease_id uuid,
    terminal_origin text NOT NULL CHECK (terminal_origin IN (
        'signed_edge_observation', 'server_timeout'
    )),
    terminal_status text NOT NULL CHECK (terminal_status IN (
        'succeeded', 'failed', 'timed_out'
    )),
    request_id uuid,
    wire_request_commitment bytea CHECK (wire_request_commitment IS NULL OR octet_length(wire_request_commitment) = 32),
    canonical_input_commitment bytea NOT NULL CHECK (octet_length(canonical_input_commitment) = 32),
    execution_profile_commitment bytea NOT NULL
        CHECK (octet_length(execution_profile_commitment) = 32),
    encrypted_output bytea,
    encrypted_output_payload_commitment bytea CHECK (
        encrypted_output_payload_commitment IS NULL OR octet_length(encrypted_output_payload_commitment) = 32
    ),
    canonical_output_commitment bytea CHECK (
        canonical_output_commitment IS NULL OR octet_length(canonical_output_commitment) = 32
    ),
    output_readable_by jsonb NOT NULL CHECK (jsonb_typeof(output_readable_by) = 'array'),
    failure_code text,
    signer_identity_id uuid,
    signer_device_id uuid,
    signer_device_key_version integer CHECK (signer_device_key_version > 0),
    signed_at timestamptz,
    classical_signature bytea CHECK (classical_signature IS NULL OR octet_length(classical_signature) = 64),
    post_quantum_signature bytea,
    statement_hash bytea CHECK (statement_hash IS NULL OR octet_length(statement_hash) = 32),
    idempotency_key uuid NOT NULL,
    observation_hash bytea NOT NULL CHECK (octet_length(observation_hash) = 32),
    observed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    payload_purged_at timestamptz,
    CONSTRAINT agent_tool_observation_dispatch_fk
        FOREIGN KEY (dispatch_id) REFERENCES agent_tool_attempt_dispatches (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_observation_call_fk
        FOREIGN KEY (call_id) REFERENCES agent_tool_calls (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_observation_request_fk
        FOREIGN KEY (request_id) REFERENCES agent_tool_attempt_requests (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_observation_signer_key_fk
        FOREIGN KEY (signer_identity_id, signer_device_id, signer_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_observation_origin_shape CHECK (
        (terminal_origin = 'signed_edge_observation'
         AND dispatch_id IS NOT NULL AND lease_id IS NOT NULL
         AND signer_identity_id IS NOT NULL AND signer_device_id IS NOT NULL
         AND signer_device_key_version IS NOT NULL AND signed_at IS NOT NULL
         AND classical_signature IS NOT NULL AND post_quantum_signature IS NOT NULL
         AND statement_hash IS NOT NULL
         AND request_id IS NOT NULL AND wire_request_commitment IS NOT NULL)
        OR
        (terminal_origin = 'server_timeout' AND terminal_status = 'timed_out'
         AND ((dispatch_id IS NULL AND lease_id IS NULL)
              OR (dispatch_id IS NOT NULL AND lease_id IS NOT NULL))
         AND ((request_id IS NULL AND wire_request_commitment IS NULL)
              OR (request_id IS NOT NULL AND wire_request_commitment IS NOT NULL))
         AND signer_identity_id IS NULL AND signer_device_id IS NULL
         AND signer_device_key_version IS NULL AND signed_at IS NULL
         AND classical_signature IS NULL AND post_quantum_signature IS NULL
         AND statement_hash IS NULL)
    ),
    CONSTRAINT agent_tool_observation_terminal_shape CHECK (
        (terminal_status = 'succeeded'
         AND terminal_origin = 'signed_edge_observation' AND request_id IS NOT NULL
         AND wire_request_commitment IS NOT NULL
         AND ((payload_purged_at IS NULL AND encrypted_output IS NOT NULL)
              OR (payload_purged_at IS NOT NULL AND encrypted_output IS NULL))
         AND encrypted_output_payload_commitment IS NOT NULL
         AND canonical_output_commitment IS NOT NULL
         AND failure_code IS NULL)
        OR
        (terminal_status IN ('failed', 'timed_out')
         AND encrypted_output IS NULL
         AND encrypted_output_payload_commitment IS NULL
         AND canonical_output_commitment IS NULL
         AND failure_code IS NOT NULL)
    ),
    CONSTRAINT agent_tool_observation_dispatch_unique UNIQUE (project_id, dispatch_id),
    CONSTRAINT agent_tool_observation_attempt_unique UNIQUE (project_id, call_id, attempt),
    CONSTRAINT agent_tool_observation_idempotency_unique
        UNIQUE (project_id, call_id, idempotency_key),
    CONSTRAINT agent_tool_observation_hash_unique UNIQUE (project_id, observation_hash)
);

CREATE TABLE agent_tool_output_key_envelopes (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    observation_id uuid NOT NULL,
    call_id uuid NOT NULL,
    recipient_identity_id uuid NOT NULL,
    recipient_device_id uuid NOT NULL,
    recipient_device_key_version integer NOT NULL CHECK (recipient_device_key_version > 0),
    envelope_version smallint NOT NULL CHECK (envelope_version = 2),
    key_purpose text NOT NULL CHECK (key_purpose = 'tool_output'),
    encrypted_key bytea NOT NULL CHECK (octet_length(encrypted_key) > 0),
    envelope_commitment bytea NOT NULL CHECK (octet_length(envelope_commitment) = 32),
    sender_identity_id uuid NOT NULL,
    sender_device_id uuid NOT NULL,
    sender_device_key_version integer NOT NULL CHECK (sender_device_key_version > 0),
    sender_signature bytea NOT NULL CHECK (octet_length(sender_signature) = 64),
    sender_post_quantum_signature bytea NOT NULL CHECK (octet_length(sender_post_quantum_signature) > 0),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_tool_output_envelope_observation_fk
        FOREIGN KEY (observation_id) REFERENCES agent_tool_attempt_observations (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_output_envelope_call_fk
        FOREIGN KEY (call_id) REFERENCES agent_tool_calls (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_output_envelope_recipient_key_fk
        FOREIGN KEY (recipient_identity_id, recipient_device_id, recipient_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_output_envelope_sender_key_fk
        FOREIGN KEY (sender_identity_id, sender_device_id, sender_device_key_version)
        REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_tool_output_envelope_exact_unique
        UNIQUE (project_id, observation_id, recipient_device_id, recipient_device_key_version)
);

-- Tool outcomes use the same run/work/claim/attempt coordinate as the tool
-- event. They are separate from TaskCompleted outcomes because 0028's table is
-- intentionally task-specific, but the referenced transition snapshot is the
-- canonical CollaborativeRunState.
CREATE TABLE agent_run_external_tool_work_outcomes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    run_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    claim_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    work_status text NOT NULL CHECK (work_status IN ('succeeded', 'failed')),
    observation_id uuid NOT NULL,
    observed_at timestamptz NOT NULL,
    provenance_hash bytea NOT NULL CHECK (octet_length(provenance_hash) = 32),
    transition_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_run_tool_outcome_work_fk
        FOREIGN KEY (project_id, run_id, work_item_id)
        REFERENCES agent_run_work_slots (project_id, run_id, work_item_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_tool_outcome_claim_fk
        FOREIGN KEY (project_id, claim_id)
        REFERENCES agent_run_claim_leases (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_tool_outcome_observation_fk
        FOREIGN KEY (observation_id) REFERENCES agent_tool_attempt_observations (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_tool_outcome_transition_fk
        FOREIGN KEY (transition_id) REFERENCES agent_run_transitions (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT agent_run_tool_outcome_attempt_unique
        UNIQUE (project_id, run_id, work_item_id, attempt),
    CONSTRAINT agent_run_tool_outcome_observation_unique
        UNIQUE (project_id, observation_id)
);

CREATE SEQUENCE agent_tool_audit_position_seq;

CREATE TABLE agent_tool_audit (
    semantic_position bigint PRIMARY KEY DEFAULT nextval('agent_tool_audit_position_seq')
        CHECK (semantic_position > 0),
    id uuid NOT NULL UNIQUE,
    project_id uuid NOT NULL,
    call_id uuid NOT NULL,
    run_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    work_item_id uuid NOT NULL,
    work_claim_id uuid NOT NULL,
    work_attempt integer NOT NULL CHECK (work_attempt > 0),
    owner_identity_id uuid NOT NULL,
    tool_name text NOT NULL,
    tool_version integer NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    kind text NOT NULL CHECK (kind IN (
        'requested', 'retry_started', 'completed', 'failed', 'timed_out'
    )),
    call_snapshot jsonb NOT NULL CHECK (jsonb_typeof(call_snapshot) = 'object'),
    observation_id uuid,
    event_hash bytea NOT NULL CHECK (octet_length(event_hash) = 32),
    idempotency_key uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT agent_tool_audit_identity_unique
        UNIQUE (project_id, call_id, attempt, kind),
    CONSTRAINT agent_tool_audit_idempotency_unique
        UNIQUE (project_id, owner_identity_id, idempotency_key)
);

CREATE FUNCTION sprout_private.reject_agent_tool_catalog_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    RAISE EXCEPTION 'external tool catalog is migration-owned and immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE FUNCTION sprout_private.reject_agent_tool_audit_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    RAISE EXCEPTION 'agent tool audit is append-only' USING ERRCODE = '55000';
END;
$$;

CREATE FUNCTION sprout_private.reject_agent_tool_runtime_history_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND TG_TABLE_NAME = 'agent_tool_attempt_observations'
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
       )
    THEN
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' THEN
        IF TG_TABLE_NAME = 'agent_tool_output_key_envelopes' AND EXISTS (
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

CREATE FUNCTION sprout_private.guard_agent_tool_permission_update()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF OLD.id IS DISTINCT FROM NEW.id
       OR OLD.project_id IS DISTINCT FROM NEW.project_id
       OR OLD.principal_identity_id IS DISTINCT FROM NEW.principal_identity_id
       OR OLD.tool_name IS DISTINCT FROM NEW.tool_name
       OR OLD.tool_version IS DISTINCT FROM NEW.tool_version
       OR OLD.granted_by_identity_id IS DISTINCT FROM NEW.granted_by_identity_id
       OR OLD.idempotency_key IS DISTINCT FROM NEW.idempotency_key
       OR OLD.grant_hash IS DISTINCT FROM NEW.grant_hash
       OR OLD.granted_at IS DISTINCT FROM NEW.granted_at
       OR OLD.revoked_at IS NOT NULL
       OR NEW.revoked_at IS NULL
       OR NEW.revoked_by_identity_id IS NULL THEN
        RAISE EXCEPTION 'tool permission history is immutable except for one-way revocation'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_tool_audit()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM public.agent_tool_calls call
        WHERE call.project_id = NEW.project_id
          AND call.id = NEW.call_id
          AND call.run_id = NEW.run_id
          AND call.goal_id = NEW.goal_id
          AND call.work_item_id = NEW.work_item_id
          AND call.work_claim_id = NEW.work_claim_id
          AND call.work_attempt = NEW.work_attempt
          AND call.owner_identity_id = NEW.owner_identity_id
          AND call.tool_name = NEW.tool_name
          AND call.tool_version = NEW.tool_version
          AND call.current_attempt = NEW.attempt
          AND NEW.call_snapshot = jsonb_build_object(
                'call_id', call.id,
                'canonical_input_commitment_hex', encode(call.canonical_input_commitment, 'hex'),
                'canonical_output_commitment_hex', CASE
                    WHEN call.current_output_commitment IS NULL THEN NULL
                    ELSE encode(call.current_output_commitment, 'hex') END,
                'max_attempts', call.max_attempts,
                'timeout_seconds', call.timeout_seconds,
                'status', call.current_status
              )
    ) THEN
        RAISE EXCEPTION 'tool audit is not the exact current call projection'
            USING ERRCODE = '55000';
    END IF;
    IF (NEW.kind IN ('completed', 'failed', 'timed_out')) <> (NEW.observation_id IS NOT NULL) THEN
        RAISE EXCEPTION 'terminal tool audit requires exact observation'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.observation_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM public.agent_tool_attempt_observations observation
        WHERE observation.project_id = NEW.project_id
          AND observation.id = NEW.observation_id
          AND observation.call_id = NEW.call_id
          AND observation.attempt = NEW.attempt
          AND observation.terminal_status = CASE NEW.kind
              WHEN 'completed' THEN 'succeeded'
              WHEN 'failed' THEN 'failed'
              WHEN 'timed_out' THEN 'timed_out'
          END
    ) THEN
        RAISE EXCEPTION 'terminal tool audit observation mismatch'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

-- Resolve WorkAuthorityOrigin from the exact first transition that contains
-- each WorkItem. A parent field is only one conjunct: the chain must terminate
-- in a run-initialized root, every child must first appear in a known
-- contract-continuation transition, and any Task -> Work causal edge fails
-- closed as possible/unsupported human-delegation provenance because 0033
-- cannot yet certify the remaining delegation conjuncts.
CREATE FUNCTION sprout_private.resolve_exact_work_authority_origin(
    candidate_project_id uuid,
    candidate_run_id uuid,
    candidate_work_id uuid
)
RETURNS TABLE (
    authority_origin text,
    authority_parent_id uuid,
    authority_principal_id uuid
)
LANGUAGE plpgsql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
DECLARE
    cursor_work_id uuid := candidate_work_id;
    direct_parent_id uuid;
    cursor_parent_id uuid;
    cursor_snapshot jsonb;
    cursor_transition_kind text;
    cursor_state_version bigint;
    prior_state_version bigint;
    cursor_actor_id uuid;
    run_sponsor_id uuid;
    visited uuid[] := ARRAY[]::uuid[];
BEGIN
    SELECT snapshot.run_sponsor_identity_id
      INTO run_sponsor_id
      FROM public.agent_run_tool_security_snapshots snapshot
     WHERE snapshot.project_id = candidate_project_id
       AND snapshot.run_id = candidate_run_id;
    IF run_sponsor_id IS NULL THEN
        RETURN;
    END IF;

    LOOP
        IF cursor_work_id = ANY(visited) THEN
            RETURN;
        END IF;
        visited := array_append(visited, cursor_work_id);

        -- A concrete Task -> Work edge is possible human-delegation provenance.
        -- It is not the complete Lean certificate; therefore neither that work
        -- nor descendants may be reclassified under runSponsor.
        IF EXISTS (
            SELECT 1
              FROM public.agent_run_causal_links link
             WHERE link.project_id = candidate_project_id
               AND link.run_id = candidate_run_id
               AND link.predecessor ->> 'kind' = 'task'
               AND link.successor ->> 'kind' = 'work'
               AND link.successor ->> 'work' = cursor_work_id::text
        ) THEN
            RETURN;
        END IF;

        SELECT transition.transition_kind,
               transition.state_version,
               transition.actor_identity_id,
               transition.state_snapshot -> 'work_items' -> cursor_work_id::text
          INTO cursor_transition_kind, cursor_state_version,
               cursor_actor_id, cursor_snapshot
          FROM public.agent_run_transitions transition
         WHERE transition.project_id = candidate_project_id
           AND transition.run_id = candidate_run_id
           AND transition.state_snapshot -> 'work_items' ? cursor_work_id::text
         ORDER BY transition.state_version
         LIMIT 1;
        IF NOT FOUND OR cursor_snapshot IS NULL THEN
            RETURN;
        END IF;
        IF (cursor_snapshot ->> 'id')::uuid <> cursor_work_id
           OR (cursor_snapshot ->> 'run')::uuid <> candidate_run_id
           OR cursor_snapshot ->> 'source_comment' IS NOT NULL
        THEN
            RETURN;
        END IF;
        IF prior_state_version IS NOT NULL
           AND cursor_state_version >= prior_state_version
        THEN
            RETURN;
        END IF;
        prior_state_version := cursor_state_version;
        cursor_parent_id := NULLIF(cursor_snapshot ->> 'parent', '')::uuid;
        IF cursor_work_id = candidate_work_id THEN
            direct_parent_id := cursor_parent_id;
        END IF;

        IF cursor_parent_id IS NULL THEN
            IF cursor_state_version <> 1
               OR cursor_transition_kind <> 'initialized'
               OR cursor_actor_id IS DISTINCT FROM run_sponsor_id
            THEN
                RETURN;
            END IF;
            authority_origin := CASE
                WHEN candidate_work_id = cursor_work_id THEN 'run_sponsor'
                ELSE 'inherited_work'
            END;
            authority_parent_id := direct_parent_id;
            authority_principal_id := run_sponsor_id;
            RETURN NEXT;
            RETURN;
        END IF;

        IF cursor_state_version <= 1
           OR cursor_transition_kind NOT IN (
                'frontier_refreshed', 'work_succeeded', 'work_failed'
           )
        THEN
            RETURN;
        END IF;
        cursor_work_id := cursor_parent_id;
    END LOOP;
EXCEPTION WHEN invalid_text_representation THEN
    RETURN;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_tool_call_runtime()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF TG_OP = 'UPDATE'
       AND OLD.payload_purged_at IS NULL
       AND NEW.payload_purged_at IS NOT NULL
       AND NEW.encrypted_input IS NULL
       AND (to_jsonb(OLD) - ARRAY['encrypted_input','payload_purged_at'])
           = (to_jsonb(NEW) - ARRAY['encrypted_input','payload_purged_at'])
       AND EXISTS (
          SELECT 1 FROM public.governed_agents agent
          WHERE agent.project_id=OLD.project_id
            AND agent.principal_identity_id=OLD.owner_identity_id
            AND NULLIF(current_setting('app.agent_retention_resource_id', true), '')::uuid
                  = agent.profile_resource_node_id
            AND sprout_private.retention_purge_row_allowed(jsonb_build_object(
                  'project_id', agent.project_id,
                  'resource_node_id', agent.profile_resource_node_id
                ))
       )
    THEN
        RETURN NEW;
    END IF;
    -- A terminal transition preserves the complete original call/attempt and
    -- deliberately does not re-evaluate current ToolReady or claim validity.
    IF TG_OP = 'UPDATE' AND OLD.current_status = 'pending'
       AND NEW.current_status IN ('succeeded', 'failed', 'timed_out') THEN
        IF ROW(
            OLD.id, OLD.project_id, OLD.run_id, OLD.goal_id, OLD.work_item_id,
            OLD.work_claim_id, OLD.work_attempt, OLD.work_spec_ordinal,
            OLD.owner_identity_id, OLD.work_authority_origin,
            OLD.work_authority_parent_id, OLD.work_authority_principal_id,
            OLD.tool_name, OLD.tool_version,
            OLD.runtime_capability_witness_id, OLD.adapter_protocol,
            OLD.encrypted_input, OLD.encrypted_input_payload_commitment,
            OLD.canonical_input_commitment, OLD.canonical_input_statement,
            OLD.input_signer_device_id, OLD.input_signer_device_key_version,
            OLD.input_classical_signature, OLD.input_post_quantum_signature,
            OLD.security_policy_hash, OLD.run_tool_ceiling_hash,
            OLD.work_tool_ceiling_hash, OLD.work_tool_ceiling, OLD.required_effects,
            OLD.output_readable_by, OLD.max_attempts, OLD.timeout_seconds,
            OLD.current_attempt, OLD.idempotency_key, OLD.request_hash,
            OLD.requested_tick, OLD.requested_at,
            OLD.tool_deadline_tick, OLD.tool_deadline_at
        ) IS DISTINCT FROM ROW(
            NEW.id, NEW.project_id, NEW.run_id, NEW.goal_id, NEW.work_item_id,
            NEW.work_claim_id, NEW.work_attempt, NEW.work_spec_ordinal,
            NEW.owner_identity_id, NEW.work_authority_origin,
            NEW.work_authority_parent_id, NEW.work_authority_principal_id,
            NEW.tool_name, NEW.tool_version,
            NEW.runtime_capability_witness_id, NEW.adapter_protocol,
            NEW.encrypted_input, NEW.encrypted_input_payload_commitment,
            NEW.canonical_input_commitment, NEW.canonical_input_statement,
            NEW.input_signer_device_id, NEW.input_signer_device_key_version,
            NEW.input_classical_signature, NEW.input_post_quantum_signature,
            NEW.security_policy_hash, NEW.run_tool_ceiling_hash,
            NEW.work_tool_ceiling_hash, NEW.work_tool_ceiling, NEW.required_effects,
            NEW.output_readable_by, NEW.max_attempts, NEW.timeout_seconds,
            NEW.current_attempt, NEW.idempotency_key, NEW.request_hash,
            NEW.requested_tick, NEW.requested_at,
            NEW.tool_deadline_tick, NEW.tool_deadline_at
        ) THEN
            RAISE EXCEPTION 'terminal tool update changed immutable call/attempt bindings'
                USING ERRCODE = '55000';
        END IF;
        IF NOT EXISTS (
            SELECT 1
            FROM public.agent_tool_attempt_observations observation
            LEFT JOIN public.agent_tool_attempt_dispatches dispatch
              ON dispatch.project_id = observation.project_id
             AND dispatch.id = observation.dispatch_id
             AND dispatch.call_id = observation.call_id
             AND dispatch.attempt = observation.attempt
            WHERE observation.project_id = NEW.project_id
              AND observation.call_id = NEW.id
              AND observation.attempt = NEW.current_attempt
              AND observation.terminal_status = NEW.current_status
              AND observation.observed_at = NEW.terminal_at
              AND observation.canonical_output_commitment
                    IS NOT DISTINCT FROM NEW.current_output_commitment
              AND (observation.terminal_origin = 'server_timeout'
                   OR (dispatch.run_id = NEW.run_id
                       AND dispatch.goal_id = NEW.goal_id
                       AND dispatch.work_item_id = NEW.work_item_id
                       AND dispatch.work_claim_id = NEW.work_claim_id
                       AND dispatch.work_attempt = NEW.work_attempt
                       AND dispatch.owner_identity_id = NEW.owner_identity_id
                       AND dispatch.requested_at = NEW.requested_at))
        ) THEN
            RAISE EXCEPTION 'terminal tool update lacks exact immutable observation/WorkAttempt'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;

    -- Invoke starts at attempt one. Retry is the only update that may advance
    -- the same ToolCallId to the next exact WorkAttempt.
    IF TG_OP = 'INSERT' AND (NEW.current_attempt <> 1 OR NEW.work_attempt <> 1) THEN
        RAISE EXCEPTION 'new tool call must start at WorkAttempt one'
            USING ERRCODE = '55000';
    ELSIF TG_OP = 'UPDATE' THEN
        IF OLD.current_status NOT IN ('failed', 'timed_out')
           OR NEW.current_status <> 'pending'
           OR NEW.current_attempt <> OLD.current_attempt + 1
           OR NEW.work_attempt <> NEW.current_attempt
           OR ROW(
                OLD.id, OLD.project_id, OLD.run_id, OLD.goal_id,
                OLD.owner_identity_id, OLD.tool_name, OLD.tool_version,
                OLD.adapter_protocol, OLD.encrypted_input,
                OLD.encrypted_input_payload_commitment,
                OLD.canonical_input_commitment, OLD.canonical_input_statement,
                OLD.input_signer_device_id, OLD.input_signer_device_key_version,
                OLD.input_classical_signature, OLD.input_post_quantum_signature,
                OLD.max_attempts, OLD.timeout_seconds, OLD.idempotency_key,
                OLD.request_hash
              ) IS DISTINCT FROM ROW(
                NEW.id, NEW.project_id, NEW.run_id, NEW.goal_id,
                NEW.owner_identity_id, NEW.tool_name, NEW.tool_version,
                NEW.adapter_protocol, NEW.encrypted_input,
                NEW.encrypted_input_payload_commitment,
                NEW.canonical_input_commitment, NEW.canonical_input_statement,
                NEW.input_signer_device_id, NEW.input_signer_device_key_version,
                NEW.input_classical_signature, NEW.input_post_quantum_signature,
                NEW.max_attempts, NEW.timeout_seconds, NEW.idempotency_key,
                NEW.request_hash
              )
        THEN
            RAISE EXCEPTION 'retry must preserve call/tool/input and advance the exact WorkAttempt'
                USING ERRCODE = '55000';
        END IF;
    END IF;

    IF TG_OP = 'INSERT' AND NOT EXISTS (
        SELECT 1 FROM public.agent_tool_runtime_capability_witnesses witness
        WHERE witness.id = NEW.runtime_capability_witness_id
          AND witness.signer_device_id = NEW.input_signer_device_id
          AND witness.signer_device_key_version = NEW.input_signer_device_key_version
    ) THEN
        RAISE EXCEPTION 'initial canonical ToolInput signer must be the exact runtime witness device'
            USING ERRCODE = '55000';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM public.agent_run_claim_leases claim
        JOIN public.agent_run_work_slots work
          ON work.project_id = claim.project_id
         AND work.run_id = claim.run_id
         AND work.work_item_id = claim.work_item_id
        JOIN public.agent_external_tool_catalog catalog
          ON catalog.tool_name = NEW.tool_name
         AND catalog.version = NEW.tool_version
         AND catalog.availability = 'executable'
        JOIN public.agent_run_tool_security_snapshots snapshot
          ON snapshot.project_id = NEW.project_id
         AND snapshot.run_id = NEW.run_id
        JOIN public.agent_collaborative_runs run
          ON run.project_id = NEW.project_id AND run.id = NEW.run_id
        JOIN LATERAL sprout_private.resolve_exact_work_authority_origin(
            NEW.project_id, NEW.run_id, NEW.work_item_id
        ) authority ON true
        JOIN public.agent_tool_runtime_capability_witnesses witness
          ON witness.id = NEW.runtime_capability_witness_id
         AND witness.project_id = NEW.project_id
         AND witness.owner_identity_id = NEW.owner_identity_id
         AND witness.tool_name = NEW.tool_name
         AND witness.tool_version = NEW.tool_version
         AND witness.manifest_hash = catalog.manifest_hash
        WHERE claim.project_id = NEW.project_id
          AND claim.id = NEW.work_claim_id
          AND claim.run_id = NEW.run_id
          AND claim.work_item_id = NEW.work_item_id
          AND claim.attempt = NEW.work_attempt
          AND claim.claimant_identity_id = NEW.owner_identity_id
          AND claim.status = 'active'
          AND claim.acquired_at <= NEW.requested_at
          AND NEW.requested_at < claim.expires_at
          AND NEW.requested_at <= clock_timestamp()
          AND NEW.requested_at = to_timestamp(NEW.requested_tick)
          AND NEW.tool_deadline_tick = NEW.requested_tick + NEW.timeout_seconds
          AND NEW.tool_deadline_at = to_timestamp(NEW.tool_deadline_tick)
          AND work.work_spec_ordinal = NEW.work_spec_ordinal
          AND NEW.work_attempt = NEW.current_attempt
          AND NEW.adapter_protocol = catalog.adapter_protocol
          AND NEW.max_attempts <= catalog.max_attempts
          AND NEW.timeout_seconds <= catalog.max_timeout_seconds
          AND public.digest(convert_to(NEW.canonical_input_statement, 'UTF8'), 'sha256')
                = NEW.canonical_input_commitment
          AND (NEW.canonical_input_statement::jsonb ->> 'owner_identity_id')::uuid
                = NEW.owner_identity_id
          AND NEW.canonical_input_statement::jsonb ->> 'tool_id' = NEW.tool_name
          AND (NEW.canonical_input_statement::jsonb ->> 'tool_version')::integer
                = NEW.tool_version
          AND NEW.run_tool_ceiling_hash = snapshot.run_tool_ceiling_hash
          AND snapshot.run_tool_ceiling @> jsonb_build_array(NEW.tool_name)
          AND NEW.work_authority_origin = authority.authority_origin
          AND NEW.work_authority_parent_id IS NOT DISTINCT FROM authority.authority_parent_id
          AND NEW.work_authority_principal_id = authority.authority_principal_id
          AND NEW.work_tool_ceiling @> jsonb_build_array(NEW.tool_name)
          AND snapshot.run_tool_ceiling @> NEW.work_tool_ceiling
          AND EXISTS (
              SELECT 1 FROM jsonb_array_elements(snapshot.work_policies) policy
              WHERE (policy ->> 'work_spec_id')::bigint = NEW.work_spec_ordinal
                AND NEW.max_attempts <= (policy ->> 'max_attempts')::integer
                AND decode(policy ->> 'policy_hash_hex', 'hex') = NEW.security_policy_hash
                AND policy -> 'tool_ceiling' @> NEW.work_tool_ceiling
                AND policy -> 'policy' -> 'allowed_tools' @> jsonb_build_array(NEW.tool_name)
          )
          AND NEW.required_effects = catalog.required_effects
          AND NEW.output_readable_by = jsonb_build_array(NEW.owner_identity_id)
          AND witness.execution_profile_commitment IS NOT NULL
          AND witness.issued_at <= clock_timestamp()
          AND clock_timestamp() < witness.expires_at
          AND EXISTS (
              SELECT 1 FROM public.agent_tool_permissions permission
              WHERE permission.project_id = NEW.project_id
                AND permission.principal_identity_id = NEW.owner_identity_id
                AND permission.tool_name = NEW.tool_name
                AND permission.tool_version = NEW.tool_version
                AND permission.revoked_at IS NULL
          )
          AND EXISTS (
              SELECT 1 FROM public.agent_tool_permissions permission
              WHERE permission.project_id = NEW.project_id
                AND permission.principal_identity_id = NEW.work_authority_principal_id
                AND permission.tool_name = NEW.tool_name
                AND permission.tool_version = NEW.tool_version
                AND permission.revoked_at IS NULL
          )
    ) THEN
        RAISE EXCEPTION 'tool call lacks exact live work, claim, catalog or permission binding'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_tool_runtime_witness()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM public.agent_external_tool_catalog catalog
        JOIN public.agent_runners runner
          ON runner.project_id = NEW.project_id
         AND runner.id = NEW.runner_id
        JOIN public.device_keys key
          ON key.identity_id = NEW.owner_identity_id
         AND key.device_id = NEW.signer_device_id
         AND key.key_version = NEW.signer_device_key_version
        JOIN public.devices device
          ON device.identity_id = key.identity_id
         AND device.id = key.device_id
        WHERE catalog.tool_name = NEW.tool_name
          AND catalog.version = NEW.tool_version
          AND catalog.availability = 'executable'
          AND catalog.manifest_hash = NEW.manifest_hash
          AND runner.agent_id = NEW.agent_id
          AND runner.principal_identity_id = NEW.owner_identity_id
          AND runner.device_id = NEW.signer_device_id
          AND runner.activated_key_version = NEW.signer_device_key_version
          AND runner.state = 'active'
          AND device.trust_state = 'trusted'
          AND device.created_at <= NEW.issued_at
          AND (device.retired_at IS NULL OR NEW.issued_at < device.retired_at)
          AND key.created_at <= NEW.issued_at
          AND (key.revoked_at IS NULL OR NEW.issued_at < key.revoked_at)
          AND NEW.issued_at <= clock_timestamp()
          AND clock_timestamp() < NEW.expires_at
    ) THEN
        RAISE EXCEPTION 'tool runtime capability witness is not exact, current and device-bound'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_tool_dispatch()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM public.agent_tool_calls call
        JOIN public.agent_run_claim_leases claim
          ON claim.project_id = call.project_id
         AND claim.id = call.work_claim_id
        JOIN public.agent_runners runner
          ON runner.project_id = NEW.project_id
         AND runner.id = NEW.runner_id
        JOIN public.device_keys key
          ON key.identity_id = runner.principal_identity_id
         AND key.device_id = runner.device_id
         AND key.key_version = runner.activated_key_version
        JOIN public.agent_tool_runtime_capability_witnesses witness
          ON witness.id = NEW.runtime_capability_witness_id
         AND witness.project_id = NEW.project_id
         AND witness.runner_id = NEW.runner_id
         AND witness.owner_identity_id = NEW.runner_identity_id
         AND witness.signer_device_id = NEW.runner_device_id
         AND witness.signer_device_key_version = NEW.runner_key_version
        WHERE call.project_id = NEW.project_id
          AND call.id = NEW.call_id
          AND NEW.run_id = call.run_id
          AND NEW.goal_id = call.goal_id
          AND NEW.work_item_id = call.work_item_id
          AND NEW.work_claim_id = call.work_claim_id
          AND NEW.work_attempt = call.work_attempt
          AND NEW.owner_identity_id = call.owner_identity_id
          AND call.current_status = 'pending'
          AND call.current_attempt = NEW.attempt
          AND call.owner_identity_id = NEW.runner_identity_id
          AND call.runtime_capability_witness_id = NEW.runtime_capability_witness_id
          AND call.adapter_protocol = NEW.adapter_protocol
          AND call.canonical_input_commitment = NEW.canonical_input_commitment
          AND NEW.requested_at = call.requested_at
          AND witness.tool_name = call.tool_name
          AND witness.tool_version = call.tool_version
          AND witness.execution_profile_commitment = NEW.execution_profile_commitment
          AND witness.issued_at <= NEW.dispatched_at
          AND NEW.dispatched_at < witness.expires_at
          AND claim.status = 'active'
          AND claim.claimant_identity_id = NEW.runner_identity_id
          AND claim.attempt = NEW.attempt
          AND claim.acquired_at <= NEW.requested_at
          AND NEW.requested_at < claim.expires_at
          AND claim.acquired_at <= NEW.dispatched_at
          AND NEW.dispatched_at < claim.expires_at
          AND runner.principal_identity_id = NEW.runner_identity_id
          AND runner.device_id = NEW.runner_device_id
          AND runner.activated_key_version = NEW.runner_key_version
          AND runner.state = 'active'
          AND key.revoked_at IS NULL
          AND NEW.tool_deadline_at = call.tool_deadline_at
          AND NEW.dispatched_at < call.tool_deadline_at
    ) THEN
        RAISE EXCEPTION 'tool dispatch lacks exact live runner/work claim binding'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_tool_request()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM public.agent_tool_attempt_dispatches dispatch
        JOIN public.agent_tool_calls call
          ON call.project_id = dispatch.project_id
         AND call.id = dispatch.call_id
        JOIN public.device_keys key
          ON key.identity_id = NEW.signer_identity_id
         AND key.device_id = NEW.signer_device_id
         AND key.key_version = NEW.signer_device_key_version
        JOIN public.devices device
          ON device.identity_id = key.identity_id
         AND device.id = key.device_id
        WHERE dispatch.project_id = NEW.project_id
          AND dispatch.id = NEW.dispatch_id
          AND dispatch.call_id = NEW.call_id
          AND dispatch.attempt = NEW.attempt
          AND dispatch.adapter_protocol = NEW.adapter_protocol
          AND dispatch.canonical_input_commitment = NEW.canonical_input_commitment
          AND dispatch.execution_profile_commitment = NEW.execution_profile_commitment
          AND dispatch.runner_identity_id = NEW.signer_identity_id
          AND dispatch.runner_device_id = NEW.signer_device_id
          AND dispatch.runner_key_version = NEW.signer_device_key_version
          AND call.current_status = 'pending'
          AND call.current_attempt = NEW.attempt
          AND NEW.signed_at >= dispatch.dispatched_at
          AND NEW.signed_at <= clock_timestamp()
          AND device.created_at <= NEW.signed_at
          AND (device.retired_at IS NULL OR NEW.signed_at < device.retired_at)
          AND key.created_at <= NEW.signed_at
          AND (key.revoked_at IS NULL OR NEW.signed_at < key.revoked_at)
    ) THEN
        RAISE EXCEPTION 'external request witness lacks exact immutable dispatch and temporal signer binding'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_tool_observation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    NEW.observed_at := clock_timestamp();
    IF NOT EXISTS (
        SELECT 1
        FROM public.agent_tool_calls call
        JOIN public.agent_tool_runtime_capability_witnesses witness
          ON witness.id = call.runtime_capability_witness_id
        WHERE call.project_id = NEW.project_id AND call.id = NEW.call_id
          AND call.current_status = 'pending'
          AND call.current_attempt = NEW.attempt
          AND call.canonical_input_commitment = NEW.canonical_input_commitment
          AND call.output_readable_by = NEW.output_readable_by
          AND witness.execution_profile_commitment = NEW.execution_profile_commitment
          AND call.requested_at <= NEW.observed_at
    ) THEN
        RAISE EXCEPTION 'tool observation lacks exact pending call binding'
            USING ERRCODE = '55000';
    END IF;

    IF NEW.terminal_origin = 'server_timeout' THEN
        IF NOT EXISTS (
            SELECT 1 FROM public.agent_tool_calls call
            WHERE call.project_id=NEW.project_id AND call.id=NEW.call_id
              AND NEW.observed_at >= call.tool_deadline_at
        ) THEN
            RAISE EXCEPTION 'server timeout precedes call-level pending deadline'
                USING ERRCODE = '55000';
        END IF;
        IF NEW.dispatch_id IS NULL THEN
            IF NEW.request_id IS NOT NULL OR NEW.wire_request_commitment IS NOT NULL
               OR EXISTS (
                  SELECT 1 FROM public.agent_tool_attempt_dispatches dispatch
                  WHERE dispatch.project_id=NEW.project_id
                    AND dispatch.call_id=NEW.call_id AND dispatch.attempt=NEW.attempt
               )
            THEN
                RAISE EXCEPTION 'no-dispatch timeout must preserve absent dispatch/request'
                    USING ERRCODE = '55000';
            END IF;
        ELSIF NOT EXISTS (
            SELECT 1
            FROM public.agent_tool_attempt_dispatches dispatch
            JOIN public.agent_tool_calls call
              ON call.project_id=dispatch.project_id AND call.id=dispatch.call_id
            WHERE dispatch.project_id=NEW.project_id AND dispatch.id=NEW.dispatch_id
              AND dispatch.call_id=NEW.call_id AND dispatch.attempt=NEW.attempt
              AND dispatch.lease_id=NEW.lease_id
              AND dispatch.run_id=call.run_id AND dispatch.goal_id=call.goal_id
              AND dispatch.work_item_id=call.work_item_id
              AND dispatch.work_claim_id=call.work_claim_id
              AND dispatch.work_attempt=call.work_attempt
              AND dispatch.owner_identity_id=call.owner_identity_id
              AND dispatch.canonical_input_commitment=NEW.canonical_input_commitment
              AND dispatch.execution_profile_commitment=NEW.execution_profile_commitment
              AND dispatch.tool_deadline_at=call.tool_deadline_at
              AND ((NEW.request_id IS NULL AND NEW.wire_request_commitment IS NULL
                    AND NOT EXISTS (
                      SELECT 1 FROM public.agent_tool_attempt_requests request
                      WHERE request.project_id=NEW.project_id
                        AND request.dispatch_id=NEW.dispatch_id
                    ))
                   OR EXISTS (
                      SELECT 1 FROM public.agent_tool_attempt_requests request
                      WHERE request.project_id=NEW.project_id AND request.id=NEW.request_id
                        AND request.dispatch_id=NEW.dispatch_id
                        AND request.call_id=NEW.call_id AND request.attempt=NEW.attempt
                        AND request.canonical_input_commitment=NEW.canonical_input_commitment
                        AND request.wire_request_commitment=NEW.wire_request_commitment
                        AND request.execution_profile_commitment=NEW.execution_profile_commitment
                   ))
        ) THEN
            RAISE EXCEPTION 'server timeout dispatch/request provenance mismatch'
                USING ERRCODE = '55000';
        END IF;
    ELSIF NOT EXISTS (
        SELECT 1
        FROM public.agent_tool_attempt_dispatches dispatch
        JOIN public.agent_tool_attempt_requests request
          ON request.project_id=dispatch.project_id AND request.dispatch_id=dispatch.id
        JOIN public.device_keys key
          ON key.identity_id=NEW.signer_identity_id
         AND key.device_id=NEW.signer_device_id
         AND key.key_version=NEW.signer_device_key_version
        JOIN public.devices device
          ON device.identity_id=key.identity_id AND device.id=key.device_id
        WHERE dispatch.project_id=NEW.project_id AND dispatch.id=NEW.dispatch_id
          AND dispatch.call_id=NEW.call_id AND dispatch.attempt=NEW.attempt
          AND dispatch.lease_id=NEW.lease_id
          AND dispatch.canonical_input_commitment=NEW.canonical_input_commitment
          AND dispatch.execution_profile_commitment=NEW.execution_profile_commitment
          AND dispatch.runner_identity_id=NEW.signer_identity_id
          AND dispatch.runner_device_id=NEW.signer_device_id
          AND dispatch.runner_key_version=NEW.signer_device_key_version
          AND request.id=NEW.request_id AND request.call_id=NEW.call_id
          AND request.attempt=NEW.attempt
          AND request.wire_request_commitment=NEW.wire_request_commitment
          AND request.canonical_input_commitment=NEW.canonical_input_commitment
          AND request.execution_profile_commitment=NEW.execution_profile_commitment
          AND NEW.signed_at >= dispatch.dispatched_at
          AND NEW.signed_at <= NEW.observed_at
          AND device.created_at <= NEW.signed_at
          AND (device.retired_at IS NULL OR NEW.signed_at < device.retired_at)
          AND key.created_at <= NEW.signed_at
          AND (key.revoked_at IS NULL OR NEW.signed_at < key.revoked_at)
    ) THEN
        RAISE EXCEPTION 'signed observation lacks exact dispatch/request/key provenance'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_tool_output_envelope()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM public.agent_tool_attempt_observations observation
        JOIN public.agent_tool_calls call
          ON call.project_id = observation.project_id
         AND call.id = observation.call_id
        JOIN public.agent_tool_attempt_dispatches dispatch
          ON dispatch.project_id = observation.project_id
         AND dispatch.id = observation.dispatch_id
         AND dispatch.call_id = observation.call_id
         AND dispatch.attempt = observation.attempt
        JOIN public.device_keys key
          ON key.identity_id = NEW.recipient_identity_id
         AND key.device_id = NEW.recipient_device_id
         AND key.key_version = NEW.recipient_device_key_version
        JOIN public.devices device
          ON device.identity_id = key.identity_id AND device.id = key.device_id
        WHERE observation.project_id = NEW.project_id
          AND observation.id = NEW.observation_id
          AND observation.call_id = NEW.call_id
          AND observation.terminal_status = 'succeeded'
          AND observation.canonical_output_commitment IS NOT NULL
          AND call.current_status = 'pending'
          AND call.owner_identity_id = NEW.recipient_identity_id
          AND call.output_readable_by = jsonb_build_array(NEW.recipient_identity_id)
          AND NEW.sender_identity_id = observation.signer_identity_id
          AND NEW.sender_device_id = observation.signer_device_id
          AND NEW.sender_device_key_version = observation.signer_device_key_version
          AND NEW.envelope_commitment = public.digest(NEW.encrypted_key, 'sha256')
          AND key.revoked_at IS NULL
          AND device.trust_state = 'trusted'
          AND device.retired_at IS NULL
    ) THEN
        RAISE EXCEPTION 'tool output envelope lacks exact succeeded output audience binding'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.validate_agent_run_tool_work_outcome()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM public.agent_tool_attempt_observations observation
        JOIN public.agent_tool_calls call
          ON call.project_id = observation.project_id
         AND call.id = observation.call_id
        JOIN public.agent_run_transitions transition
          ON transition.project_id = NEW.project_id
         AND transition.id = NEW.transition_id
        JOIN public.agent_run_claim_leases claim
          ON claim.project_id = NEW.project_id
         AND claim.id = NEW.claim_id
        WHERE observation.project_id = NEW.project_id
          AND observation.id = NEW.observation_id
          AND observation.call_id = call.id
          AND observation.attempt = NEW.attempt
          AND observation.observed_at = NEW.observed_at
          AND call.run_id = NEW.run_id
          AND call.work_item_id = NEW.work_item_id
          AND call.work_claim_id = NEW.claim_id
          AND call.work_attempt = NEW.attempt
          AND claim.acquired_at <= call.requested_at
          AND call.requested_at < claim.expires_at
          AND call.current_attempt = NEW.attempt
          AND claim.run_id = NEW.run_id
          AND claim.work_item_id = NEW.work_item_id
          AND claim.attempt = NEW.attempt
          AND claim.claimant_identity_id = call.owner_identity_id
          AND transition.run_id = NEW.run_id
          AND transition.observation_kind = 'tool_terminal'
          AND transition.observation_id = NEW.observation_id
          AND transition.state_snapshot #>> ARRAY[
                'work_items', NEW.work_item_id::text, 'attempt'
              ] = NEW.attempt::text
          AND transition.state_snapshot #>> ARRAY[
                'work_items', NEW.work_item_id::text, 'status'
              ] = NEW.work_status
          AND transition.state_snapshot #>> ARRAY[
                'claims', NEW.claim_id::text, 'status'
              ] = 'released'
          AND NEW.work_status = CASE observation.terminal_status
                WHEN 'succeeded' THEN 'succeeded' ELSE 'failed'
              END
          AND NEW.provenance_hash = public.digest(convert_to(concat_ws(':',
                call.id::text, call.run_id::text, call.goal_id::text,
                call.work_item_id::text, call.work_claim_id::text,
                call.work_attempt::text, observation.id::text,
                observation.terminal_status,
                encode(call.canonical_input_commitment, 'hex'),
                encode(COALESCE(observation.canonical_output_commitment, ''::bytea), 'hex')
              ), 'UTF8'), 'sha256')
    ) THEN
        RAISE EXCEPTION 'tool WorkOutcome lacks exact call/claim/attempt/transition provenance'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.reject_agent_tool_call_delete()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    RAISE EXCEPTION 'agent tool call structural history cannot be deleted'
        USING ERRCODE = '55000';
END;
$$;

CREATE FUNCTION sprout_private.purge_agent_tools_for_resource()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
BEGIN
    IF TG_OP <> 'DELETE' OR NOT sprout_private.retention_purge_row_allowed(
        jsonb_build_object('project_id', OLD.project_id, 'resource_node_id', OLD.id)
    ) THEN
        RETURN OLD;
    END IF;
    PERFORM set_config('app.agent_retention_resource_id', OLD.id::text, true);
    DELETE FROM public.agent_tool_output_key_envelopes envelope
    USING public.agent_tool_calls call, public.governed_agents agent
    WHERE agent.project_id = OLD.project_id
      AND agent.profile_resource_node_id = OLD.id
      AND call.project_id = agent.project_id
      AND call.owner_identity_id = agent.principal_identity_id
      AND envelope.project_id=call.project_id
      AND envelope.call_id=call.id;
    UPDATE public.agent_tool_attempt_observations observation
    SET encrypted_output=NULL, payload_purged_at=clock_timestamp()
    FROM public.agent_tool_calls call, public.governed_agents agent
    WHERE agent.project_id=OLD.project_id
      AND agent.profile_resource_node_id=OLD.id
      AND call.project_id=agent.project_id
      AND call.owner_identity_id=agent.principal_identity_id
      AND observation.project_id=call.project_id
      AND observation.call_id=call.id
      AND observation.encrypted_output IS NOT NULL
      AND observation.payload_purged_at IS NULL;
    UPDATE public.agent_tool_calls call
    SET encrypted_input=NULL, payload_purged_at=clock_timestamp()
    FROM public.governed_agents agent
    WHERE agent.project_id=OLD.project_id
      AND agent.profile_resource_node_id=OLD.id
      AND call.project_id=agent.project_id
      AND call.owner_identity_id=agent.principal_identity_id
      AND call.payload_purged_at IS NULL;
    UPDATE public.agent_tool_permissions permission
    SET revoked_at = COALESCE(permission.revoked_at, clock_timestamp()),
        revoked_by_identity_id = COALESCE(
            permission.revoked_by_identity_id,
            (SELECT project.owner_identity_id FROM public.projects project
             WHERE project.id = permission.project_id)
        )
    FROM public.governed_agents agent
    WHERE agent.project_id = OLD.project_id
      AND agent.profile_resource_node_id = OLD.id
      AND permission.project_id = agent.project_id
      AND permission.principal_identity_id = agent.principal_identity_id
      AND permission.revoked_at IS NULL;
    RETURN OLD;
END;
$$;

CREATE TRIGGER agent_external_tool_catalog_immutable
BEFORE UPDATE OR DELETE ON agent_external_tool_catalog
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_catalog_mutation();
CREATE TRIGGER agent_tool_audit_exact
BEFORE INSERT ON agent_tool_audit
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_tool_audit();
CREATE TRIGGER agent_tool_calls_exact_runtime
BEFORE INSERT OR UPDATE ON agent_tool_calls
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_tool_call_runtime();
CREATE TRIGGER agent_tool_runtime_witness_exact
BEFORE INSERT ON agent_tool_runtime_capability_witnesses
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_tool_runtime_witness();
CREATE TRIGGER agent_tool_dispatches_exact_runtime
BEFORE INSERT ON agent_tool_attempt_dispatches
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_tool_dispatch();
CREATE TRIGGER agent_tool_requests_exact_runtime
BEFORE INSERT ON agent_tool_attempt_requests
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_tool_request();
CREATE TRIGGER agent_tool_observations_exact_runtime
BEFORE INSERT ON agent_tool_attempt_observations
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_tool_observation();
CREATE TRIGGER agent_tool_output_envelopes_exact_runtime
BEFORE INSERT ON agent_tool_output_key_envelopes
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_tool_output_envelope();
CREATE TRIGGER agent_run_tool_work_outcomes_exact
BEFORE INSERT ON agent_run_external_tool_work_outcomes
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_agent_run_tool_work_outcome();
CREATE TRIGGER agent_run_tool_work_outcomes_append_only
BEFORE UPDATE OR DELETE ON agent_run_external_tool_work_outcomes
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_runtime_history_mutation();
CREATE TRIGGER agent_tool_audit_append_only
BEFORE UPDATE OR DELETE ON agent_tool_audit
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_audit_mutation();
CREATE TRIGGER agent_tool_dispatches_append_only
BEFORE UPDATE OR DELETE ON agent_tool_attempt_dispatches
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_runtime_history_mutation();
CREATE TRIGGER agent_tool_runtime_witnesses_append_only
BEFORE UPDATE OR DELETE ON agent_tool_runtime_capability_witnesses
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_runtime_history_mutation();
CREATE TRIGGER agent_run_tool_security_snapshots_append_only
BEFORE UPDATE OR DELETE ON agent_run_tool_security_snapshots
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_runtime_history_mutation();
CREATE TRIGGER agent_tool_requests_append_only
BEFORE UPDATE OR DELETE ON agent_tool_attempt_requests
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_runtime_history_mutation();
CREATE TRIGGER agent_tool_observations_append_only
BEFORE UPDATE OR DELETE ON agent_tool_attempt_observations
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_runtime_history_mutation();
CREATE TRIGGER agent_tool_output_envelopes_append_only
BEFORE UPDATE OR DELETE ON agent_tool_output_key_envelopes
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_runtime_history_mutation();
CREATE TRIGGER agent_tool_permissions_one_way
BEFORE UPDATE ON agent_tool_permissions
FOR EACH ROW EXECUTE FUNCTION sprout_private.guard_agent_tool_permission_update();
CREATE TRIGGER agent_tool_calls_retention_only_delete
BEFORE DELETE ON agent_tool_calls
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_agent_tool_call_delete();
CREATE TRIGGER aa_agent_external_tools_retention
BEFORE DELETE ON resource_nodes
FOR EACH ROW EXECUTE FUNCTION sprout_private.purge_agent_tools_for_resource();

ALTER TABLE agent_tool_permissions ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_permissions FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_calls ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_calls FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_run_tool_security_snapshots ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_run_tool_security_snapshots FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_runtime_capability_witnesses ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_runtime_capability_witnesses FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_attempt_dispatches ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_attempt_dispatches FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_attempt_requests ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_attempt_requests FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_attempt_observations ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_attempt_observations FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_output_key_envelopes ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_output_key_envelopes FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_tool_audit FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_run_external_tool_work_outcomes ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_run_external_tool_work_outcomes FORCE ROW LEVEL SECURITY;

CREATE POLICY agent_tool_permission_party_isolation ON agent_tool_permissions
    USING (EXISTS (
        SELECT 1 FROM project_memberships membership
        WHERE membership.project_id=agent_tool_permissions.project_id
          AND membership.identity_id=sprout_private.current_identity_id()
    ))
    WITH CHECK (false);
CREATE POLICY agent_tool_call_party_isolation ON agent_tool_calls
    USING (owner_identity_id = sprout_private.current_identity_id())
    WITH CHECK (owner_identity_id = sprout_private.current_identity_id());
CREATE POLICY agent_run_tool_snapshot_party_isolation ON agent_run_tool_security_snapshots
    USING (EXISTS (
        SELECT 1 FROM agent_run_participants participant
        WHERE participant.project_id = agent_run_tool_security_snapshots.project_id
          AND participant.run_id = agent_run_tool_security_snapshots.run_id
          AND participant.identity_id = sprout_private.current_identity_id()
    ));
CREATE POLICY agent_tool_runtime_witness_party_isolation ON agent_tool_runtime_capability_witnesses
    USING (sprout_private.agent_party_access(project_id, agent_id));
CREATE POLICY agent_tool_dispatch_party_isolation ON agent_tool_attempt_dispatches
    USING (EXISTS (
        SELECT 1 FROM agent_tool_calls call
        WHERE call.project_id = agent_tool_attempt_dispatches.project_id
          AND call.id = agent_tool_attempt_dispatches.call_id
          AND call.owner_identity_id = sprout_private.current_identity_id()
    ));
CREATE POLICY agent_tool_observation_party_isolation ON agent_tool_attempt_observations
    USING (EXISTS (
        SELECT 1 FROM agent_tool_calls call
        WHERE call.project_id = agent_tool_attempt_observations.project_id
          AND call.id = agent_tool_attempt_observations.call_id
          AND call.output_readable_by @>
              jsonb_build_array(sprout_private.current_identity_id())
    ));
CREATE POLICY agent_tool_request_party_isolation ON agent_tool_attempt_requests
    USING (EXISTS (
        SELECT 1 FROM agent_tool_calls call
        WHERE call.project_id = agent_tool_attempt_requests.project_id
          AND call.id = agent_tool_attempt_requests.call_id
          AND call.owner_identity_id = sprout_private.current_identity_id()
    ));
CREATE POLICY agent_tool_output_envelope_party_isolation ON agent_tool_output_key_envelopes
    FOR SELECT USING (recipient_identity_id = sprout_private.current_identity_id());
CREATE POLICY agent_tool_audit_party_isolation ON agent_tool_audit
    FOR SELECT USING (EXISTS (
        SELECT 1 FROM project_memberships membership
        WHERE membership.project_id = agent_tool_audit.project_id
          AND membership.identity_id = sprout_private.current_identity_id()
          AND (
              membership.identity_id = agent_tool_audit.owner_identity_id
              OR membership.role IN ('owner', 'admin')
          )
    ));
CREATE POLICY agent_run_tool_outcome_party_isolation ON agent_run_external_tool_work_outcomes
    USING (EXISTS (
        SELECT 1 FROM agent_run_participants participant
        WHERE participant.project_id = agent_run_external_tool_work_outcomes.project_id
          AND participant.run_id = agent_run_external_tool_work_outcomes.run_id
          AND participant.identity_id = sprout_private.current_identity_id()
    ));

-- 0033 has no authoritative shared traceId projection yet. Operational tool
-- audit remains available, but the formal R5.41 tool surface is deliberately
-- disabledFailClosed and therefore exactly empty.
CREATE VIEW agent_r541_tool_surface_records AS
SELECT audit.project_id, audit.call_id, audit.run_id, audit.goal_id,
       audit.work_item_id, audit.work_claim_id, audit.work_attempt,
       audit.owner_identity_id, audit.tool_name, audit.tool_version,
       audit.attempt, audit.kind, audit.semantic_position, audit.recorded_at
FROM agent_tool_audit audit
WHERE false;

CREATE FUNCTION sprout_private.semantic_tool_audit_list(candidate_project_id uuid)
RETURNS TABLE (
    semantic_position bigint, call_id uuid, run_id uuid, goal_id uuid,
    work_item_id uuid, work_claim_id uuid, work_attempt integer,
    owner_identity_id uuid, tool_name text, tool_version integer,
    attempt integer, kind text, call_snapshot jsonb, recorded_at timestamptz
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
SET row_security = off
AS $$
    SELECT audit.semantic_position, audit.call_id, audit.run_id, audit.goal_id,
           audit.work_item_id, audit.work_claim_id, audit.work_attempt,
           audit.owner_identity_id, audit.tool_name, audit.tool_version,
           audit.attempt, audit.kind, audit.call_snapshot, audit.recorded_at
    FROM public.agent_tool_audit audit
    WHERE audit.project_id = candidate_project_id
      AND EXISTS (
          SELECT 1 FROM public.project_memberships membership
          WHERE membership.project_id = audit.project_id
            AND membership.identity_id = sprout_private.current_identity_id()
            AND (
                membership.identity_id = audit.owner_identity_id
                OR membership.role IN ('owner', 'admin')
            )
      )
    ORDER BY audit.semantic_position
$$;

REVOKE ALL ON TABLE agent_external_tool_catalog FROM PUBLIC;
GRANT SELECT ON TABLE agent_external_tool_catalog TO PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_tool_permissions FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_tool_calls FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_run_tool_security_snapshots FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_tool_runtime_capability_witnesses FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_tool_attempt_dispatches FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_tool_attempt_requests FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_tool_attempt_observations FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_tool_output_key_envelopes FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_tool_audit FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON agent_run_external_tool_work_outcomes FROM PUBLIC;
REVOKE ALL ON SEQUENCE agent_tool_audit_position_seq FROM PUBLIC;
REVOKE ALL ON TABLE agent_r541_tool_surface_records FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_tool_catalog_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.grant_agent_tool_permission(
    uuid, uuid, uuid, uuid, uuid, text, integer, uuid, uuid, bytea
) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.revoke_agent_tool_permission(
    uuid, uuid, uuid, uuid, text, integer, uuid
) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_tool_audit_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_tool_runtime_history_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.guard_agent_tool_permission_update() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_tool_audit() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.resolve_exact_work_authority_origin(uuid, uuid, uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_tool_call_runtime() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_tool_runtime_witness() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_tool_dispatch() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_tool_request() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_tool_observation() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_tool_output_envelope() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.validate_agent_run_tool_work_outcome() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.reject_agent_tool_call_delete() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.purge_agent_tools_for_resource() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.semantic_tool_audit_list(uuid) FROM PUBLIC;

COMMENT ON TABLE agent_tool_calls IS
    'Exact R5 tool call projection. Input/output are ciphertext or commitments; connector configuration and secrets never enter Sprout.';
COMMENT ON TABLE agent_tool_audit IS
    'Append-only ordered ToolAuditEntry projection; persists structural history across encrypted call retention.';
