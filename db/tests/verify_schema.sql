\set ON_ERROR_STOP on

-- T-LLR-05.1 verifies that template, required, and completed attachment
-- provenance is represented by distinct constrained tables.
DO $verification$
DECLARE
    missing_tables text[];
    unprotected_tables text[];
    unsafe_foreign_keys text[];
    naive_timestamps text[];
    non_uuid_ids text[];
BEGIN
    WITH expected(table_name) AS (
        SELECT unnest(ARRAY[
            'identities', 'identity_directory', 'identity_emails', 'email_verification_tokens',
            'account_recovery_tokens', 'webauthn_ceremonies', 'email_outbox',
            'passkeys', 'devices', 'sessions', 'device_key_transparency_log',
            'project_recovery_requests', 'project_recovery_electorate',
            'project_recovery_approvals', 'project_recovery_sets',
            'project_recovery_shares', 'sync_aggregates', 'sync_device_heads',
            'sync_current_projections', 'operational_metrics',
            'projects', 'project_memberships', 'project_invitations',
            'resource_nodes', 'resource_closure',
            'topics', 'task_lists', 'tasks', 'info_documents',
            'topic_permissions', 'task_list_permissions', 'task_permissions',
            'encrypted_domain_snapshots', 'task_assignments',
            'task_recurrences', 'task_completions', 'recurrence_series',
            'task_snapshot_history',
            'presets', 'preset_versions', 'preset_pretasks',
            'preset_materializations', 'preset_materialized_tasks',
            'preset_assignments', 'preset_assignment_values',
            'preset_assignment_materialized_tasks',
            'questionnaires', 'questionnaire_versions',
            'questionnaire_questions', 'questionnaire_options',
            'questionnaire_submissions', 'questionnaire_answers',
            'questionnaire_answer_options',
            'file_blobs', 'file_links',
            'pretask_template_attachments', 'task_required_attachments',
            'task_completed_attachments',
            'device_keys', 'resource_epochs',
            'resource_key_envelopes', 'recovery_sets', 'recovery_shares',
            'sync_events', 'sync_idempotency', 'sync_snapshots',
            'retention_policies', 'retention_leases', 'notifications',
            'exports', 'audit_log', 'outbox',
            'identity_retention_preferences', 'retention_subjects',
            'retention_dependencies', 'retention_warning_deliveries',
            'retention_archives', 'retention_archive_device_envelopes',
            'retention_archive_receipts', 'purge_markers'
        ])
    )
    SELECT array_agg(expected.table_name ORDER BY expected.table_name)
    INTO missing_tables
    FROM expected
    LEFT JOIN pg_class relation
      ON relation.relname = expected.table_name
     AND relation.relnamespace = 'public'::regnamespace
     AND relation.relkind IN ('r', 'p')
    WHERE relation.oid IS NULL;

    IF missing_tables IS NOT NULL THEN
        RAISE EXCEPTION 'missing expected tables: %', missing_tables;
    END IF;

    WITH expected(table_name) AS (
        SELECT unnest(ARRAY[
            'identities', 'identity_directory', 'identity_emails', 'email_verification_tokens',
            'account_recovery_tokens', 'webauthn_ceremonies', 'email_outbox',
            'passkeys', 'devices', 'sessions', 'device_key_transparency_log',
            'project_recovery_requests', 'project_recovery_electorate',
            'project_recovery_approvals', 'project_recovery_sets',
            'project_recovery_shares', 'sync_aggregates', 'sync_device_heads',
            'sync_current_projections',
            'projects', 'project_memberships', 'project_invitations',
            'resource_nodes', 'resource_closure',
            'topics', 'task_lists', 'tasks', 'info_documents',
            'topic_permissions', 'task_list_permissions', 'task_permissions',
            'encrypted_domain_snapshots', 'task_assignments',
            'task_recurrences', 'task_completions', 'recurrence_series',
            'task_snapshot_history',
            'presets', 'preset_versions', 'preset_pretasks',
            'preset_materializations', 'preset_materialized_tasks',
            'preset_assignments', 'preset_assignment_values',
            'preset_assignment_materialized_tasks',
            'questionnaires', 'questionnaire_versions',
            'questionnaire_questions', 'questionnaire_options',
            'questionnaire_submissions', 'questionnaire_answers',
            'questionnaire_answer_options',
            'file_blobs', 'file_links',
            'pretask_template_attachments', 'task_required_attachments',
            'task_completed_attachments',
            'device_keys', 'resource_epochs',
            'resource_key_envelopes', 'recovery_sets', 'recovery_shares',
            'sync_events', 'sync_idempotency', 'sync_snapshots',
            'retention_policies', 'retention_leases', 'notifications',
            'exports', 'audit_log', 'outbox',
            'identity_retention_preferences', 'retention_subjects',
            'retention_dependencies', 'retention_warning_deliveries',
            'retention_archives', 'retention_archive_device_envelopes',
            'retention_archive_receipts', 'purge_markers'
        ])
    )
    SELECT array_agg(expected.table_name ORDER BY expected.table_name)
    INTO unprotected_tables
    FROM expected
    JOIN pg_class relation
      ON relation.relname = expected.table_name
     AND relation.relnamespace = 'public'::regnamespace
    WHERE NOT relation.relrowsecurity;

    IF unprotected_tables IS NOT NULL THEN
        RAISE EXCEPTION 'tables without RLS: %', unprotected_tables;
    END IF;

    SELECT array_agg(constraint_record.conname ORDER BY constraint_record.conname)
    INTO unsafe_foreign_keys
    FROM pg_constraint constraint_record
    WHERE constraint_record.contype = 'f'
      AND constraint_record.connamespace = 'public'::regnamespace
      AND (
          constraint_record.confdeltype = 'c'
          OR constraint_record.confupdtype = 'c'
      );

    IF unsafe_foreign_keys IS NOT NULL THEN
        RAISE EXCEPTION 'historical safety forbids cascading foreign keys: %',
            unsafe_foreign_keys;
    END IF;

    SELECT array_agg(format('%I.%I', columns.table_name, columns.column_name))
    INTO naive_timestamps
    FROM information_schema.columns columns
    WHERE columns.table_schema = 'public'
      AND columns.data_type = 'timestamp without time zone';

    IF naive_timestamps IS NOT NULL THEN
        RAISE EXCEPTION 'timestamp columns must carry time zones: %', naive_timestamps;
    END IF;

    SELECT array_agg(format('%I.%I', columns.table_name, columns.column_name))
    INTO non_uuid_ids
    FROM information_schema.columns columns
    WHERE columns.table_schema = 'public'
      AND (columns.column_name = 'id' OR columns.column_name LIKE '%\_id' ESCAPE '\')
      AND columns.column_name NOT IN ('credential_id', 'event_sequence', 'audit_sequence')
      AND columns.data_type <> 'uuid';

    IF non_uuid_ids IS NOT NULL THEN
        RAISE EXCEPTION 'identifier columns must be UUIDs: %', non_uuid_ids;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgrelid = 'public.resource_nodes'::regclass
          AND tgname = 'resource_nodes_validate_parent'
          AND NOT tgisinternal
    ) THEN
        RAISE EXCEPTION 'resource hierarchy cycle guard is missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'public'
          AND indexname = 'task_recurrences_one_active_rule_idx'
          AND indexdef LIKE '%WHERE (retired_at IS NULL)%'
    ) THEN
        RAISE EXCEPTION 'active recurrence uniqueness index is missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'public.recovery_sets'::regclass
          AND conname = 'recovery_sets_n_of_n'
    ) THEN
        RAISE EXCEPTION 'n-of-n recovery constraint is missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'public'
          AND indexname = 'identity_emails_normalized_unique'
    ) THEN
        RAISE EXCEPTION 'normalized email uniqueness is missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'public'
          AND indexname = 'webauthn_ceremonies_one_active_kind_idx'
    ) THEN
        RAISE EXCEPTION 'WebAuthn one-active-ceremony constraint is missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_proc
        WHERE oid = 'sprout_private.accept_project_invitation(uuid,uuid,bytea,uuid)'::regprocedure
          AND prosecdef
    ) THEN
        RAISE EXCEPTION 'secure invitation acceptance function is missing';
    END IF;

    IF to_regclass('public.access_grants') IS NOT NULL THEN
        RAISE EXCEPTION 'HLR-02 must not expose a generic access_grants table';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM (VALUES
            ('topic_permissions'),
            ('task_list_permissions'),
            ('task_permissions')
        ) expected(table_name)
        WHERE NOT EXISTS (
            SELECT 1
            FROM information_schema.columns columns
            WHERE columns.table_schema = 'public'
              AND columns.table_name = expected.table_name
              AND columns.column_name = 'access_scope'
              AND columns.is_nullable = 'NO'
        )
        OR NOT EXISTS (
            SELECT 1
            FROM information_schema.columns columns
            WHERE columns.table_schema = 'public'
              AND columns.table_name = expected.table_name
              AND columns.column_name = 'root_grant_id'
              AND columns.is_nullable = 'NO'
        )
    ) THEN
        RAISE EXCEPTION 'HLR-02 permission scope or root lineage columns are missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_proc
        WHERE oid = 'sprout_private.grant_hierarchical_permission(uuid,uuid,uuid,text,text,text,uuid,uuid,text,uuid)'::regprocedure
          AND prosecdef
    ) OR NOT EXISTS (
        SELECT 1
        FROM pg_proc
        WHERE oid = 'sprout_private.revoke_hierarchical_permission(uuid,uuid,uuid,uuid,bytea)'::regprocedure
          AND prosecdef
    ) OR NOT EXISTS (
        SELECT 1
        FROM pg_proc
        WHERE oid = 'sprout_private.effective_domain_permission(uuid,uuid,uuid)'::regprocedure
          AND prosecdef
    ) THEN
        RAISE EXCEPTION 'HLR-02 security-definer permission functions are missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'resource_key_envelopes'
          AND column_name = 'envelope_version'
    ) OR NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'public.resource_key_envelopes'::regclass
          AND conname = 'resource_key_envelopes_signature_length'
    ) THEN
        RAISE EXCEPTION 'HLR-02 versioned envelope shape validation is missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'device_keys'
          AND column_name = 'ml_dsa_65_public_key'
    ) THEN
        RAISE EXCEPTION 'hybrid device signing key fields are missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgrelid = 'public.device_key_transparency_log'::regclass
          AND tgname = 'device_key_transparency_immutable'
          AND NOT tgisinternal
    ) THEN
        RAISE EXCEPTION 'device key transparency immutability is missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'public.sync_events'::regclass
          AND conname = 'sync_events_resource_version_unique'
    ) THEN
        RAISE EXCEPTION 'sync aggregate optimistic concurrency is missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'public'
          AND indexname = 'tasks_recurrence_occurrence_unique'
    ) OR NOT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'public'
          AND indexname = 'task_assignments_one_active_task_idx'
    ) THEN
        RAISE EXCEPTION 'HLR-03 recurrence or active-assignee uniqueness is missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgrelid = 'public.task_snapshot_history'::regclass
          AND tgname = 'task_snapshot_history_immutable'
          AND NOT tgisinternal
    ) OR NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgrelid = 'public.preset_assignments'::regclass
          AND tgname = 'preset_assignments_validate_materialization'
          AND NOT tgisinternal
    ) THEN
        RAISE EXCEPTION 'HLR-03 snapshot or materialization guards are missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'tasks'
          AND column_name = 'questionnaire_version_id'
    ) OR NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgrelid = 'public.questionnaire_versions'::regclass
          AND tgname = 'questionnaire_versions_validate_mutation'
          AND NOT tgisinternal
    ) OR NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgrelid = 'public.questionnaire_submissions'::regclass
          AND tgname = 'questionnaire_submissions_validate_answers'
          AND NOT tgisinternal
    ) THEN
        RAISE EXCEPTION 'HLR-04 version pinning or immutable submission guards are missing';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name IN (
              'file_blobs',
              'pretask_template_attachments',
              'task_required_attachments',
              'task_completed_attachments'
          )
          AND column_name IN (
              'path', 'file_path', 'client_path', 'name', 'filename',
              'mime', 'mime_type', 'media_type'
          )
    ) OR NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgrelid = 'public.file_blobs'::regclass
          AND tgname = 'file_blobs_validate_mutation'
          AND NOT tgisinternal
    ) THEN
        RAISE EXCEPTION 'HLR-05 encrypted attachment separation or blob guards are missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_proc
        WHERE oid =
            'sprout_private.retention_effective_purge_at(uuid)'::regprocedure
          AND prosecdef
    ) OR NOT EXISTS (
        SELECT 1 FROM pg_proc
        WHERE oid =
            'sprout_private.purge_retention_subject(uuid,uuid,timestamp with time zone)'::regprocedure
          AND prosecdef
    ) OR NOT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'public'
          AND indexname = 'retention_warning_deliveries_once'
    ) OR NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'public.retention_archives'::regclass
          AND conname = 'retention_archives_expiry_shape'
    ) THEN
        RAISE EXCEPTION 'HLR-08 retention, exactly-once warning, or expiry controls are missing';
    END IF;
END;
$verification$;

SELECT 'sprout schema verification passed' AS result;
