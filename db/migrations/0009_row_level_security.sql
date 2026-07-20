-- RLS is a project-boundary backstop. Application authorization remains responsible
-- for topic/list/task access levels. Background workers should use a separately
-- provisioned PostgreSQL role with BYPASSRLS rather than a user-controlled GUC.

REVOKE CREATE ON SCHEMA sprout_private FROM PUBLIC;
GRANT USAGE ON SCHEMA sprout_private TO PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.current_identity_id() TO PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.current_device_id() TO PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.is_project_member(uuid) TO PUBLIC;

CREATE OR REPLACE FUNCTION sprout_private.is_project_owner(candidate_project_id uuid)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM projects project
        WHERE project.id = candidate_project_id
          AND project.owner_identity_id = sprout_private.current_identity_id()
          AND project.deleted_at IS NULL
    )
$$;

REVOKE ALL ON FUNCTION sprout_private.is_project_owner(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.is_project_owner(uuid) TO PUBLIC;

ALTER TABLE identities ENABLE ROW LEVEL SECURITY;
ALTER TABLE identities FORCE ROW LEVEL SECURITY;
CREATE POLICY identities_self_access ON identities
    USING (id = sprout_private.current_identity_id())
    WITH CHECK (id = sprout_private.current_identity_id());

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'passkeys',
        'devices',
        'sessions',
        'device_keys'
    ]
    LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
        EXECUTE format(
            'CREATE POLICY identity_isolation ON %I
             USING (identity_id = sprout_private.current_identity_id())
             WITH CHECK (identity_id = sprout_private.current_identity_id())',
            table_name
        );
    END LOOP;
END;
$$;

ALTER TABLE projects ENABLE ROW LEVEL SECURITY;
-- These two policy-root tables are intentionally not FORCEd: the SECURITY
-- DEFINER membership helpers are owned by the migration role and must be able
-- to inspect them without recursively re-entering their own policies.
ALTER TABLE projects NO FORCE ROW LEVEL SECURITY;
CREATE POLICY projects_member_read_write ON projects
    USING (sprout_private.is_project_member(id))
    WITH CHECK (sprout_private.is_project_member(id));
CREATE POLICY projects_owner_insert ON projects
    FOR INSERT
    WITH CHECK (owner_identity_id = sprout_private.current_identity_id());

ALTER TABLE project_memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE project_memberships NO FORCE ROW LEVEL SECURITY;
CREATE POLICY project_memberships_member_access ON project_memberships
    USING (sprout_private.is_project_member(project_id))
    WITH CHECK (
        sprout_private.is_project_member(project_id)
        OR (
            sprout_private.is_project_owner(project_id)
            AND identity_id = sprout_private.current_identity_id()
            AND role = 'owner'
        )
    );
CREATE POLICY project_memberships_owner_bootstrap_read ON project_memberships
    FOR SELECT
    USING (
        sprout_private.is_project_owner(project_id)
        AND identity_id = sprout_private.current_identity_id()
        AND role = 'owner'
    );

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'project_invitations',
        'resource_nodes',
        'resource_closure',
        'topics',
        'task_lists',
        'tasks',
        'topic_permissions',
        'task_list_permissions',
        'task_permissions',
        'encrypted_domain_snapshots',
        'task_assignments',
        'task_recurrences',
        'task_completions',
        'presets',
        'preset_versions',
        'preset_pretasks',
        'preset_materializations',
        'preset_materialized_tasks',
        'questionnaires',
        'questionnaire_versions',
        'questionnaire_questions',
        'questionnaire_options',
        'questionnaire_submissions',
        'questionnaire_answers',
        'file_blobs',
        'file_links',
        'resource_epochs',
        'resource_key_envelopes',
        'recovery_sets',
        'recovery_shares',
        'sync_events',
        'sync_idempotency',
        'sync_snapshots',
        'retention_policies',
        'retention_leases',
        'notifications',
        'exports',
        'audit_log',
        'outbox'
    ]
    LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
        EXECUTE format(
            'CREATE POLICY project_isolation ON %I
             USING (sprout_private.is_project_member(project_id))
             WITH CHECK (sprout_private.is_project_member(project_id))',
            table_name
        );
    END LOOP;
END;
$$;
