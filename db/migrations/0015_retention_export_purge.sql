-- HLR-08: UTC retention warnings, authorization-filtered encrypted exports,
-- leased idempotent purge, and non-sensitive anti-resurrection markers.

CREATE FUNCTION sprout_private.add_utc_calendar_months(
    candidate_at timestamptz,
    month_count integer
)
RETURNS timestamptz
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
    WITH source AS (
        SELECT candidate_at AT TIME ZONE 'UTC' AS value
    ),
    target AS (
        SELECT
            source.value,
            date_trunc('month', source.value)
                + make_interval(months => month_count) AS first_of_month
        FROM source
    )
    SELECT (
        target.first_of_month
        + make_interval(
            days => least(
                extract(day FROM target.value)::integer,
                extract(
                    day FROM (
                        target.first_of_month
                        + interval '1 month'
                        - interval '1 day'
                    )
                )::integer
            ) - 1
        )
        + (target.value - date_trunc('day', target.value))
    ) AT TIME ZONE 'UTC'
    FROM target
$$;

ALTER TABLE email_outbox
    DROP CONSTRAINT email_outbox_message_kind_check,
    ADD CONSTRAINT email_outbox_message_kind_check
        CHECK (message_kind IN (
            'signup_verification', 'account_recovery',
            'project_invitation', 'retention_warning'
        )),
    ADD COLUMN deduplication_key bytea,
    ADD CONSTRAINT email_outbox_deduplication_key_length
        CHECK (
            deduplication_key IS NULL
            OR octet_length(deduplication_key) = 32
        );

CREATE UNIQUE INDEX email_outbox_deduplication_unique
    ON email_outbox (identity_id, message_kind, deduplication_key)
    WHERE deduplication_key IS NOT NULL;

CREATE TABLE identity_retention_preferences (
    identity_id uuid PRIMARY KEY,
    auto_export_enabled boolean NOT NULL DEFAULT false,
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT identity_retention_preferences_identity_fk
        FOREIGN KEY (identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE TRIGGER identity_retention_preferences_touch_updated_at
BEFORE UPDATE ON identity_retention_preferences
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE retention_subjects (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    source_kind text NOT NULL
        CHECK (source_kind IN ('task_completed', 'resource_deleted')),
    source_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    owner_identity_id uuid NOT NULL,
    retention_class text NOT NULL
        CHECK (retention_class IN ('deleted_or_obsolete', 'completed')),
    source_at timestamptz NOT NULL,
    warning_at timestamptz NOT NULL,
    purge_at timestamptz NOT NULL,
    state text NOT NULL DEFAULT 'scheduled'
        CHECK (state IN ('scheduled', 'purging', 'retry', 'purged')),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    retry_at timestamptz,
    lease_owner uuid,
    lease_token uuid,
    leased_until timestamptz,
    last_error_code text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    purged_at timestamptz,
    CONSTRAINT retention_subjects_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_subjects_owner_fk
        FOREIGN KEY (owner_identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_subjects_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT retention_subjects_source_unique
        UNIQUE (project_id, source_kind, source_id),
    CONSTRAINT retention_subjects_deadline_order CHECK (
        warning_at > source_at AND purge_at > warning_at
    ),
    CONSTRAINT retention_subjects_lease_shape CHECK (
        (lease_owner IS NULL AND lease_token IS NULL AND leased_until IS NULL)
        OR (
            lease_owner IS NOT NULL
            AND lease_token IS NOT NULL
            AND leased_until IS NOT NULL
        )
    ),
    CONSTRAINT retention_subjects_purged_shape CHECK (
        (state = 'purged') = (purged_at IS NOT NULL)
    ),
    CONSTRAINT retention_subjects_retry_shape CHECK (
        (state = 'retry') = (retry_at IS NOT NULL)
    )
);

CREATE INDEX retention_subjects_warning_work_idx
    ON retention_subjects (warning_at, project_id)
    WHERE state <> 'purged';
CREATE INDEX retention_subjects_purge_work_idx
    ON retention_subjects (purge_at, project_id)
    WHERE state IN ('scheduled', 'retry', 'purging');
CREATE INDEX retention_subjects_retry_idx
    ON retention_subjects (retry_at)
    WHERE state = 'retry';
CREATE TRIGGER retention_subjects_touch_updated_at
BEFORE UPDATE ON retention_subjects
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE retention_dependencies (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    subject_id uuid NOT NULL,
    depends_on_subject_id uuid NOT NULL,
    reason text NOT NULL CHECK (length(reason) BETWEEN 1 AND 128),
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT retention_dependencies_subject_fk
        FOREIGN KEY (project_id, subject_id)
        REFERENCES retention_subjects (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_dependencies_provider_fk
        FOREIGN KEY (project_id, depends_on_subject_id)
        REFERENCES retention_subjects (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_dependencies_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT retention_dependencies_unique
        UNIQUE (project_id, subject_id, depends_on_subject_id),
    CONSTRAINT retention_dependencies_not_self
        CHECK (subject_id <> depends_on_subject_id)
);

CREATE TABLE retention_warning_deliveries (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    subject_id uuid NOT NULL,
    recipient_identity_id uuid NOT NULL,
    warning_at timestamptz NOT NULL,
    in_app_enqueued_at timestamptz,
    email_enqueued_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT retention_warning_deliveries_subject_fk
        FOREIGN KEY (project_id, subject_id)
        REFERENCES retention_subjects (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_warning_deliveries_recipient_fk
        FOREIGN KEY (project_id, recipient_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_warning_deliveries_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT retention_warning_deliveries_once
        UNIQUE (subject_id, recipient_identity_id, warning_at)
);

CREATE TABLE retention_archives (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    subject_id uuid NOT NULL,
    recipient_identity_id uuid NOT NULL,
    state text NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'running', 'succeeded', 'failed')),
    storage_key text,
    ciphertext_size bigint,
    ciphertext_sha256 bytea,
    canonical_manifest bytea,
    manifest_signature bytea,
    failure_code text,
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    lease_owner uuid,
    lease_token uuid,
    leased_until timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    source_purged_at timestamptz,
    expires_at timestamptz,
    CONSTRAINT retention_archives_subject_fk
        FOREIGN KEY (project_id, subject_id)
        REFERENCES retention_subjects (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_archives_recipient_fk
        FOREIGN KEY (project_id, recipient_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_archives_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT retention_archives_one_per_recipient
        UNIQUE (subject_id, recipient_identity_id),
    CONSTRAINT retention_archives_storage_key CHECK (
        storage_key IS NULL
        OR storage_key ~ '^[0-9a-f]{32}[.]archive$'
    ),
    CONSTRAINT retention_archives_digest_length CHECK (
        ciphertext_sha256 IS NULL OR octet_length(ciphertext_sha256) = 32
    ),
    CONSTRAINT retention_archives_signature_length CHECK (
        manifest_signature IS NULL OR octet_length(manifest_signature) = 64
    ),
    CONSTRAINT retention_archives_result_shape CHECK (
        (
            state = 'succeeded'
            AND storage_key IS NOT NULL
            AND ciphertext_size > 0
            AND ciphertext_sha256 IS NOT NULL
            AND canonical_manifest IS NOT NULL
            AND manifest_signature IS NOT NULL
            AND completed_at IS NOT NULL
            AND failure_code IS NULL
        )
        OR (
            state = 'failed'
            AND completed_at IS NOT NULL
            AND failure_code IS NOT NULL
            AND storage_key IS NULL
            AND ciphertext_sha256 IS NULL
            AND canonical_manifest IS NULL
            AND manifest_signature IS NULL
        )
        OR state IN ('pending', 'running')
    ),
    CONSTRAINT retention_archives_lease_shape CHECK (
        (lease_owner IS NULL AND lease_token IS NULL AND leased_until IS NULL)
        OR (
            lease_owner IS NOT NULL
            AND lease_token IS NOT NULL
            AND leased_until IS NOT NULL
        )
    ),
    CONSTRAINT retention_archives_expiry_shape CHECK (
        (
            source_purged_at IS NULL
            AND expires_at IS NULL
        )
        OR expires_at = source_purged_at + interval '720 hours'
    )
);

CREATE INDEX retention_archives_work_idx
    ON retention_archives (state, created_at)
    WHERE state IN ('pending', 'running');
CREATE INDEX retention_archives_recipient_idx
    ON retention_archives (recipient_identity_id, created_at DESC);
CREATE INDEX retention_archives_expiry_idx
    ON retention_archives (expires_at)
    WHERE expires_at IS NOT NULL;
CREATE TRIGGER retention_archives_touch_updated_at
BEFORE UPDATE ON retention_archives
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE retention_archive_device_envelopes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    archive_id uuid NOT NULL,
    recipient_identity_id uuid NOT NULL,
    recipient_device_id uuid NOT NULL,
    recipient_device_key_version integer NOT NULL CHECK (recipient_device_key_version > 0),
    ephemeral_x25519_public_key bytea NOT NULL,
    wrap_nonce bytea NOT NULL,
    wrapped_archive_key bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT retention_archive_device_envelopes_archive_fk
        FOREIGN KEY (project_id, archive_id)
        REFERENCES retention_archives (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_archive_device_envelopes_device_fk
        FOREIGN KEY (
            recipient_identity_id,
            recipient_device_id,
            recipient_device_key_version
        ) REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_archive_device_envelopes_project_id_unique
        UNIQUE (project_id, id),
    CONSTRAINT retention_archive_device_envelopes_coverage_unique
        UNIQUE (archive_id, recipient_device_id, recipient_device_key_version),
    CONSTRAINT retention_archive_device_envelopes_ephemeral_length
        CHECK (octet_length(ephemeral_x25519_public_key) = 32),
    CONSTRAINT retention_archive_device_envelopes_nonce_length
        CHECK (octet_length(wrap_nonce) = 12),
    CONSTRAINT retention_archive_device_envelopes_wrapped_length
        CHECK (octet_length(wrapped_archive_key) = 48)
);

CREATE TABLE retention_archive_receipts (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    archive_id uuid NOT NULL,
    recipient_identity_id uuid NOT NULL,
    ciphertext_sha256 bytea NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT retention_archive_receipts_archive_fk
        FOREIGN KEY (project_id, archive_id)
        REFERENCES retention_archives (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_archive_receipts_recipient_fk
        FOREIGN KEY (recipient_identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT retention_archive_receipts_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT retention_archive_receipts_once
        UNIQUE (archive_id, recipient_identity_id),
    CONSTRAINT retention_archive_receipts_digest_length
        CHECK (octet_length(ciphertext_sha256) = 32)
);

CREATE TABLE purge_markers (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    source_kind text NOT NULL,
    source_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    final_aggregate_version bigint NOT NULL DEFAULT 0 CHECK (final_aggregate_version >= 0),
    final_event_hash bytea,
    purged_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT purge_markers_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT purge_markers_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT purge_markers_source_unique
        UNIQUE (project_id, source_kind, source_id),
    CONSTRAINT purge_markers_resource_unique
        UNIQUE (project_id, resource_node_id),
    CONSTRAINT purge_markers_hash_length
        CHECK (final_event_hash IS NULL OR octet_length(final_event_hash) = 32)
);

CREATE FUNCTION sprout_private.retention_effective_purge_at(candidate_subject_id uuid)
RETURNS timestamptz
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
    WITH RECURSIVE dependents(id, purge_at, path) AS (
        SELECT subject.id, subject.purge_at, ARRAY[subject.id]
        FROM retention_subjects subject
        WHERE subject.id = candidate_subject_id

        UNION ALL

        SELECT dependent.id, dependent.purge_at, dependents.path || dependent.id
        FROM dependents
        JOIN retention_dependencies dependency
          ON dependency.depends_on_subject_id = dependents.id
        JOIN retention_subjects dependent
          ON dependent.id = dependency.subject_id
        WHERE NOT dependent.id = ANY(dependents.path)
    )
    SELECT max(purge_at) FROM dependents
$$;

CREATE FUNCTION sprout_private.materialize_retention_subjects(candidate_now timestamptz)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    inserted_count bigint;
    resource_count bigint;
BEGIN
    INSERT INTO retention_subjects (
        project_id, source_kind, source_id, resource_node_id,
        owner_identity_id, retention_class,
        source_at, warning_at, purge_at
    )
    SELECT
        task.project_id,
        'task_completed',
        task.id,
        task.resource_node_id,
        project.owner_identity_id,
        'completed',
        task.completed_at,
        sprout_private.add_utc_calendar_months(task.completed_at, 6),
        sprout_private.add_utc_calendar_months(task.completed_at, 12)
    FROM tasks task
    JOIN projects project ON project.id = task.project_id
    WHERE task.state = 'completed'
      AND task.completed_at <= candidate_now
    ON CONFLICT (project_id, source_kind, source_id) DO NOTHING;
    GET DIAGNOSTICS inserted_count = ROW_COUNT;

    INSERT INTO retention_subjects (
        project_id, source_kind, source_id, resource_node_id,
        owner_identity_id, retention_class,
        source_at, warning_at, purge_at
    )
    SELECT
        node.project_id,
        'resource_deleted',
        node.id,
        node.id,
        project.owner_identity_id,
        'deleted_or_obsolete',
        node.deleted_at,
        node.deleted_at + interval '360 hours',
        node.deleted_at + interval '720 hours'
    FROM resource_nodes node
    JOIN projects project ON project.id = node.project_id
    WHERE node.deleted_at IS NOT NULL
      AND node.deleted_at <= candidate_now
      AND NOT EXISTS (
          SELECT 1
          FROM tasks task
          WHERE task.project_id = node.project_id
            AND task.resource_node_id = node.id
            AND task.state = 'completed'
      )
    ON CONFLICT (project_id, source_kind, source_id) DO NOTHING;
    GET DIAGNOSTICS resource_count = ROW_COUNT;
    inserted_count := inserted_count + resource_count;

    INSERT INTO retention_dependencies (
        project_id, subject_id, depends_on_subject_id, reason
    )
    SELECT
        copy.project_id,
        copy_subject.id,
        source_subject.id,
        'copied_task_history'
    FROM tasks copy
    JOIN retention_subjects copy_subject
      ON copy_subject.project_id = copy.project_id
     AND copy_subject.source_kind = 'task_completed'
     AND copy_subject.source_id = copy.id
    JOIN retention_subjects source_subject
      ON source_subject.project_id = copy.project_id
     AND source_subject.source_kind = 'task_completed'
     AND source_subject.source_id = copy.copied_from_task_id
    WHERE copy.copied_from_task_id IS NOT NULL
    ON CONFLICT (project_id, subject_id, depends_on_subject_id) DO NOTHING;

    RETURN inserted_count;
END;
$$;

CREATE FUNCTION sprout_private.retention_interested_users(candidate_subject_id uuid)
RETURNS TABLE(identity_id uuid)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
    WITH subject AS (
        SELECT *
        FROM retention_subjects
        WHERE id = candidate_subject_id
    ),
    interested AS (
        SELECT project.owner_identity_id AS identity_id
        FROM subject
        JOIN projects project ON project.id = subject.project_id

        UNION

        SELECT membership.identity_id
        FROM subject
        JOIN project_memberships membership
          ON membership.project_id = subject.project_id
         AND membership.state = 'active'
        JOIN resource_nodes node
          ON node.project_id = subject.project_id
         AND node.id = subject.resource_node_id
        WHERE membership.role IN ('owner', 'admin')
           OR node.created_by_identity_id = membership.identity_id
           OR EXISTS (
               SELECT 1
               FROM sprout_private.effective_domain_permission(
                   subject.project_id,
                   subject.resource_node_id,
                   membership.identity_id
               )
           )

        UNION

        SELECT assignment.assignee_identity_id
        FROM subject
        JOIN task_assignments assignment
          ON assignment.project_id = subject.project_id
         AND assignment.task_id = subject.source_id
         AND assignment.revoked_at IS NULL
        JOIN project_memberships membership
          ON membership.project_id = assignment.project_id
         AND membership.identity_id = assignment.assignee_identity_id
         AND membership.state = 'active'
        WHERE subject.source_kind = 'task_completed'
    )
    SELECT DISTINCT interested.identity_id
    FROM interested
    JOIN identities identity ON identity.id = interested.identity_id
    WHERE identity.status = 'active'
$$;

CREATE FUNCTION sprout_private.retention_purge_row_allowed(row_data jsonb)
RETURNS boolean
LANGUAGE plpgsql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    candidate_subject_id uuid;
    candidate_lease_token uuid;
    subject retention_subjects%ROWTYPE;
    row_project_id uuid;
    row_resource_node_id uuid;
    row_task_id uuid;
    row_submission_id uuid;
    row_stream_id uuid;
    row_id uuid;
    row_aggregate_id uuid;
BEGIN
    candidate_subject_id :=
        NULLIF(current_setting('app.retention_purge_subject', true), '')::uuid;
    candidate_lease_token :=
        NULLIF(current_setting('app.retention_purge_lease_token', true), '')::uuid;
    IF candidate_subject_id IS NULL OR candidate_lease_token IS NULL THEN
        RETURN false;
    END IF;
    SELECT * INTO subject
    FROM retention_subjects
    WHERE id = candidate_subject_id
      AND state = 'purging'
      AND lease_token = candidate_lease_token;
    IF NOT FOUND THEN
        RETURN false;
    END IF;
    row_project_id := NULLIF(row_data ->> 'project_id', '')::uuid;
    row_resource_node_id :=
        NULLIF(row_data ->> 'resource_node_id', '')::uuid;
    row_task_id := NULLIF(row_data ->> 'task_id', '')::uuid;
    row_submission_id := NULLIF(row_data ->> 'submission_id', '')::uuid;
    row_stream_id := NULLIF(row_data ->> 'stream_id', '')::uuid;
    row_id := NULLIF(row_data ->> 'id', '')::uuid;
    row_aggregate_id := NULLIF(row_data ->> 'aggregate_id', '')::uuid;
    RETURN row_project_id = subject.project_id
       AND (
           row_resource_node_id = subject.resource_node_id
           OR row_stream_id = subject.resource_node_id
           OR row_task_id = subject.source_id
           OR EXISTS (
               SELECT 1
               FROM tasks task
               WHERE task.project_id = subject.project_id
                 AND task.id = row_task_id
                 AND task.resource_node_id = subject.resource_node_id
           )
           OR EXISTS (
               SELECT 1
               FROM questionnaire_submissions submission
               JOIN tasks task
                 ON task.project_id = submission.project_id
                AND task.id = submission.task_id
               WHERE submission.project_id = subject.project_id
                 AND submission.id = row_submission_id
                 AND task.resource_node_id = subject.resource_node_id
           )
           OR row_id IN (subject.source_id, subject.resource_node_id)
           OR (
               row_aggregate_id IN (subject.source_id, subject.resource_node_id)
               AND row_data ->> 'aggregate_kind' IN ('task', 'resource')
           )
       );
EXCEPTION
    WHEN invalid_text_representation THEN RETURN false;
END;
$$;

CREATE OR REPLACE FUNCTION sprout_private.reject_historical_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE'
       AND sprout_private.retention_purge_row_allowed(to_jsonb(OLD))
    THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'historical records are immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE FUNCTION sprout_private.retention_only_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF sprout_private.retention_purge_row_allowed(to_jsonb(OLD)) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'physical deletion requires an active retention lease'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER tasks_validate_mutation ON tasks;
CREATE TRIGGER tasks_validate_mutation
BEFORE UPDATE ON tasks
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_task_mutation();
CREATE TRIGGER tasks_retention_delete
BEFORE DELETE ON tasks
FOR EACH ROW EXECUTE FUNCTION sprout_private.retention_only_delete();

DROP TRIGGER task_assignments_validate_mutation ON task_assignments;
CREATE TRIGGER task_assignments_validate_mutation
BEFORE INSERT OR UPDATE ON task_assignments
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_task_assignment_mutation();
CREATE TRIGGER task_assignments_retention_delete
BEFORE DELETE ON task_assignments
FOR EACH ROW EXECUTE FUNCTION sprout_private.retention_only_delete();

DROP TRIGGER questionnaire_submissions_validate_mutation
    ON questionnaire_submissions;
CREATE TRIGGER questionnaire_submissions_validate_mutation
BEFORE INSERT OR UPDATE ON questionnaire_submissions
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_questionnaire_submission();
CREATE TRIGGER questionnaire_submissions_retention_delete
BEFORE DELETE ON questionnaire_submissions
FOR EACH ROW EXECUTE FUNCTION sprout_private.retention_only_delete();

DROP TRIGGER questionnaire_answers_validate_mutation ON questionnaire_answers;
CREATE TRIGGER questionnaire_answers_validate_mutation
BEFORE INSERT OR UPDATE ON questionnaire_answers
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_questionnaire_answer_mutation();
CREATE TRIGGER questionnaire_answers_retention_delete
BEFORE DELETE ON questionnaire_answers
FOR EACH ROW EXECUTE FUNCTION sprout_private.retention_only_delete();

DROP TRIGGER questionnaire_answer_options_validate_mutation
    ON questionnaire_answer_options;
CREATE TRIGGER questionnaire_answer_options_validate_mutation
BEFORE INSERT OR UPDATE ON questionnaire_answer_options
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_questionnaire_answer_mutation();
CREATE TRIGGER questionnaire_answer_options_retention_delete
BEFORE DELETE ON questionnaire_answer_options
FOR EACH ROW EXECUTE FUNCTION sprout_private.retention_only_delete();

CREATE TRIGGER resource_nodes_retention_delete
BEFORE DELETE ON resource_nodes
FOR EACH ROW EXECUTE FUNCTION sprout_private.retention_only_delete();
CREATE TRIGGER topics_retention_delete
BEFORE DELETE ON topics
FOR EACH ROW EXECUTE FUNCTION sprout_private.retention_only_delete();
CREATE TRIGGER task_lists_retention_delete
BEFORE DELETE ON task_lists
FOR EACH ROW EXECUTE FUNCTION sprout_private.retention_only_delete();

-- Once a resource has been purged, even a correctly signed stale client event
-- must not recreate it.
CREATE OR REPLACE FUNCTION sprout_private.validate_sync_event_chain()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    expected_previous_hash bytea;
    expected_device_sequence bigint;
    aggregate_state sync_aggregates%ROWTYPE;
BEGIN
    IF EXISTS (
        SELECT 1
        FROM purge_markers marker
        WHERE marker.project_id = NEW.project_id
          AND marker.resource_node_id = NEW.resource_node_id
    ) THEN
        RAISE EXCEPTION 'purged resources cannot be resurrected'
            USING ERRCODE = '55000';
    END IF;

    PERFORM pg_advisory_xact_lock(
        hashtextextended(NEW.project_id::text || ':' || NEW.actor_device_id::text, 0)
    );
    SELECT last_event_hash, device_sequence + 1
    INTO expected_previous_hash, expected_device_sequence
    FROM sync_device_heads
    WHERE project_id = NEW.project_id
      AND actor_device_id = NEW.actor_device_id
    FOR UPDATE;
    IF NEW.previous_hash IS DISTINCT FROM expected_previous_hash
       OR NEW.device_sequence IS DISTINCT FROM COALESCE(expected_device_sequence, 1)
    THEN
        RAISE EXCEPTION 'sync device hash chain mismatch' USING ERRCODE = '40001';
    END IF;

    PERFORM pg_advisory_xact_lock(
        hashtextextended(NEW.project_id::text || ':' || NEW.resource_node_id::text, 7)
    );
    SELECT * INTO aggregate_state
    FROM sync_aggregates
    WHERE project_id = NEW.project_id
      AND resource_node_id = NEW.resource_node_id
    FOR UPDATE;
    IF FOUND THEN
        IF NEW.base_version <> aggregate_state.current_version THEN
            RAISE EXCEPTION 'stale sync base version' USING ERRCODE = '40001';
        END IF;
        IF aggregate_state.tombstoned AND NEW.mutation_kind <> 'tombstone' THEN
            RAISE EXCEPTION 'signed tombstone prevents resurrection'
                USING ERRCODE = '40001';
        END IF;
    ELSIF NEW.base_version <> 0 THEN
        RAISE EXCEPTION 'stale sync base version' USING ERRCODE = '40001';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION sprout_private.purge_retention_subject(
    candidate_subject_id uuid,
    candidate_lease_token uuid,
    candidate_now timestamptz
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
DECLARE
    subject retention_subjects%ROWTYPE;
    resource_id uuid;
    task_ids uuid[];
    blob_ids uuid[];
    final_version bigint;
    final_hash bytea;
BEGIN
    SELECT * INTO subject
    FROM retention_subjects
    WHERE id = candidate_subject_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RETURN false;
    END IF;
    IF subject.state = 'purged' THEN
        RETURN true;
    END IF;
    IF (
        subject.source_kind = 'task_completed'
        AND NOT EXISTS (
            SELECT 1
            FROM tasks task
            WHERE task.project_id = subject.project_id
              AND task.id = subject.source_id
              AND task.resource_node_id = subject.resource_node_id
              AND task.state = 'completed'
              AND task.completed_at = subject.source_at
        )
    ) OR (
        subject.source_kind = 'resource_deleted'
        AND (
            subject.source_id <> subject.resource_node_id
            OR NOT EXISTS (
                SELECT 1
                FROM resource_nodes node
                WHERE node.project_id = subject.project_id
                  AND node.id = subject.source_id
                  AND node.deleted_at = subject.source_at
            )
        )
    ) THEN
        RETURN false;
    END IF;
    IF subject.state <> 'purging'
       OR subject.lease_token IS DISTINCT FROM candidate_lease_token
       OR subject.leased_until <= candidate_now
       OR candidate_now < sprout_private.retention_effective_purge_at(subject.id)
       OR EXISTS (
           SELECT 1
           FROM retention_dependencies dependency
           JOIN retention_subjects dependent
             ON dependent.id = dependency.subject_id
           WHERE dependency.depends_on_subject_id = subject.id
             AND dependent.state <> 'purged'
       )
       OR EXISTS (
           SELECT 1
           FROM tasks dependent_task
           WHERE subject.source_kind = 'task_completed'
             AND dependent_task.project_id = subject.project_id
             AND dependent_task.copied_from_task_id = subject.source_id
             AND NOT EXISTS (
                 SELECT 1
                 FROM purge_markers marker
                 WHERE marker.project_id = dependent_task.project_id
                   AND marker.source_id = dependent_task.id
             )
       )
    THEN
        RETURN false;
    END IF;

    resource_id := subject.resource_node_id;
    SELECT array_agg(task.id) INTO task_ids
    FROM tasks task
    WHERE task.project_id = subject.project_id
      AND (
          (
              subject.source_kind = 'task_completed'
              AND task.id = subject.source_id
          )
          OR (
              subject.source_kind = 'resource_deleted'
              AND task.resource_node_id = resource_id
          )
      );
    SET CONSTRAINTS ALL DEFERRED;
    PERFORM set_config('app.retention_purge_subject', subject.id::text, true);
    PERFORM set_config(
        'app.retention_purge_lease_token',
        candidate_lease_token::text,
        true
    );

    SELECT
        COALESCE(max(event.aggregate_version), aggregate.current_version, 0),
        COALESCE(
            (array_agg(event.event_hash ORDER BY event.aggregate_version DESC))[1],
            aggregate.last_event_hash
        )
    INTO final_version, final_hash
    FROM sync_aggregates aggregate
    FULL JOIN sync_events event
      ON event.project_id = aggregate.project_id
     AND event.resource_node_id = aggregate.resource_node_id
    WHERE COALESCE(event.project_id, aggregate.project_id) = subject.project_id
      AND COALESCE(event.resource_node_id, aggregate.resource_node_id) = resource_id
    GROUP BY aggregate.current_version, aggregate.last_event_hash;

    INSERT INTO purge_markers (
        project_id, source_kind, source_id, resource_node_id,
        final_aggregate_version, final_event_hash, purged_at
    )
    VALUES (
        subject.project_id, subject.source_kind, subject.source_id, resource_id,
        COALESCE(final_version, 0), final_hash, candidate_now
    )
    ON CONFLICT (project_id, source_kind, source_id) DO NOTHING;

    SELECT array_agg(blob_id) INTO blob_ids
    FROM (
        SELECT blob_id
        FROM task_completed_attachments
        WHERE project_id = subject.project_id
          AND (
              task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]))
              OR resource_node_id = resource_id
          )
        UNION
        SELECT blob_id
        FROM task_required_attachments
        WHERE project_id = subject.project_id
          AND (
              task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]))
              OR resource_node_id = resource_id
          )
        UNION
        SELECT blob_id
        FROM pretask_template_attachments
        WHERE project_id = subject.project_id
          AND resource_node_id = resource_id
        UNION
        SELECT id
        FROM file_blobs
        WHERE project_id = subject.project_id
          AND resource_node_id = resource_id
    ) controlled_blobs;

    DELETE FROM questionnaire_answer_options selected
    USING questionnaire_submissions submission
    WHERE selected.project_id = submission.project_id
      AND selected.submission_id = submission.id
      AND submission.project_id = subject.project_id
      AND submission.task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]));
    DELETE FROM questionnaire_answers answer
    USING questionnaire_submissions submission
    WHERE answer.project_id = submission.project_id
      AND answer.submission_id = submission.id
      AND submission.project_id = subject.project_id
      AND submission.task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]));
    DELETE FROM questionnaire_submissions
    WHERE project_id = subject.project_id
      AND task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]));

    DELETE FROM task_completed_attachments
    WHERE project_id = subject.project_id
      AND (
          task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]))
          OR resource_node_id = resource_id
      );
    DELETE FROM task_required_attachments
    WHERE project_id = subject.project_id
      AND (
          task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]))
          OR resource_node_id = resource_id
      );
    DELETE FROM pretask_template_attachments
    WHERE project_id = subject.project_id
      AND resource_node_id = resource_id;
    DELETE FROM file_links
    WHERE project_id = subject.project_id
      AND (
          resource_node_id = resource_id
          OR blob_id = ANY(COALESCE(blob_ids, ARRAY[]::uuid[]))
      );
    UPDATE exports
    SET
        state = 'expired',
        result_blob_id = NULL,
        completed_at = NULL
    WHERE project_id = subject.project_id
      AND result_blob_id = ANY(COALESCE(blob_ids, ARRAY[]::uuid[]));
    UPDATE file_blobs
    SET upload_state = 'deleted'
    WHERE project_id = subject.project_id
      AND id = ANY(COALESCE(blob_ids, ARRAY[]::uuid[]))
      AND upload_state <> 'deleted';
    DELETE FROM file_blobs
    WHERE project_id = subject.project_id
      AND id = ANY(COALESCE(blob_ids, ARRAY[]::uuid[]));

    DELETE FROM task_completions
    WHERE project_id = subject.project_id
      AND task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]));
    DELETE FROM preset_materialized_tasks
    WHERE project_id = subject.project_id
      AND task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]));
    DELETE FROM preset_assignment_materialized_tasks
    WHERE project_id = subject.project_id
      AND task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]));
    DELETE FROM task_snapshot_history
    WHERE project_id = subject.project_id
      AND task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]));
    DELETE FROM task_recurrences
    WHERE project_id = subject.project_id
      AND task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]));
    DELETE FROM task_permissions
    WHERE project_id = subject.project_id
      AND task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]));
    DELETE FROM task_assignments
    WHERE project_id = subject.project_id
      AND task_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]));
    DELETE FROM encrypted_domain_snapshots
    WHERE project_id = subject.project_id
      AND (
          (
              aggregate_kind = 'task'
              AND aggregate_id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]))
          )
          OR (aggregate_kind = 'resource' AND aggregate_id = resource_id)
      );

    DELETE FROM sync_idempotency idempotency
    USING sync_events event
    WHERE idempotency.project_id = event.project_id
      AND idempotency.sync_event_id = event.id
      AND event.project_id = subject.project_id
      AND event.resource_node_id = resource_id;
    DELETE FROM sync_snapshots
    WHERE project_id = subject.project_id
      AND stream_id = resource_id;
    DELETE FROM sync_events
    WHERE project_id = subject.project_id
      AND resource_node_id = resource_id;
    DELETE FROM sync_aggregates
    WHERE project_id = subject.project_id
      AND resource_node_id = resource_id;

    DELETE FROM recovery_shares share
    USING recovery_sets recovery
    WHERE share.project_id = recovery.project_id
      AND share.recovery_set_id = recovery.id
      AND recovery.project_id = subject.project_id
      AND recovery.resource_node_id = resource_id;
    DELETE FROM recovery_sets
    WHERE project_id = subject.project_id
      AND resource_node_id = resource_id;
    DELETE FROM resource_key_envelopes
    WHERE project_id = subject.project_id
      AND resource_node_id = resource_id;
    DELETE FROM resource_epochs
    WHERE project_id = subject.project_id
      AND resource_node_id = resource_id;

    DELETE FROM tasks
    WHERE project_id = subject.project_id
      AND id = ANY(COALESCE(task_ids, ARRAY[]::uuid[]));
    DELETE FROM task_list_permissions permission
    USING task_lists list
    WHERE permission.project_id = list.project_id
      AND permission.task_list_id = list.id
      AND list.project_id = subject.project_id
      AND list.resource_node_id = resource_id;
    DELETE FROM task_lists
    WHERE project_id = subject.project_id
      AND resource_node_id = resource_id;
    DELETE FROM topic_permissions permission
    USING topics topic
    WHERE permission.project_id = topic.project_id
      AND permission.topic_id = topic.id
      AND topic.project_id = subject.project_id
      AND topic.resource_node_id = resource_id;
    DELETE FROM topics
    WHERE project_id = subject.project_id
      AND resource_node_id = resource_id;
    DELETE FROM preset_materializations
    WHERE project_id = subject.project_id
      AND target_resource_node_id = resource_id;
    DELETE FROM resource_closure
    WHERE project_id = subject.project_id
      AND (ancestor_id = resource_id OR descendant_id = resource_id);
    DELETE FROM resource_nodes
    WHERE project_id = subject.project_id
      AND id = resource_id;

    UPDATE retention_subjects
    SET
        state = 'purged',
        purged_at = candidate_now,
        retry_at = NULL,
        lease_owner = NULL,
        lease_token = NULL,
        leased_until = NULL,
        last_error_code = NULL
    WHERE id = subject.id;
    UPDATE retention_archives
    SET
        source_purged_at = candidate_now,
        expires_at = candidate_now + interval '720 hours'
    WHERE subject_id = subject.id;
    RETURN true;
END;
$$;

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'retention_subjects',
        'retention_dependencies',
        'retention_warning_deliveries',
        'retention_archives',
        'retention_archive_device_envelopes',
        'retention_archive_receipts',
        'purge_markers'
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

DROP POLICY project_isolation ON retention_warning_deliveries;
CREATE POLICY retention_warning_deliveries_recipient
    ON retention_warning_deliveries
    USING (
        recipient_identity_id = sprout_private.current_identity_id()
    )
    WITH CHECK (
        recipient_identity_id = sprout_private.current_identity_id()
    );

DROP POLICY project_isolation ON retention_archives;
CREATE POLICY retention_archives_recipient
    ON retention_archives
    USING (
        recipient_identity_id = sprout_private.current_identity_id()
    )
    WITH CHECK (
        recipient_identity_id = sprout_private.current_identity_id()
    );

DROP POLICY project_isolation ON retention_archive_device_envelopes;
CREATE POLICY retention_archive_device_envelopes_recipient
    ON retention_archive_device_envelopes
    USING (
        recipient_identity_id = sprout_private.current_identity_id()
    )
    WITH CHECK (
        recipient_identity_id = sprout_private.current_identity_id()
    );

DROP POLICY project_isolation ON retention_archive_receipts;
CREATE POLICY retention_archive_receipts_recipient
    ON retention_archive_receipts
    USING (
        recipient_identity_id = sprout_private.current_identity_id()
    )
    WITH CHECK (
        recipient_identity_id = sprout_private.current_identity_id()
    );

ALTER TABLE identity_retention_preferences ENABLE ROW LEVEL SECURITY;
ALTER TABLE identity_retention_preferences FORCE ROW LEVEL SECURITY;
CREATE POLICY identity_retention_preferences_self
    ON identity_retention_preferences
    USING (identity_id = sprout_private.current_identity_id())
    WITH CHECK (identity_id = sprout_private.current_identity_id());

REVOKE ALL ON FUNCTION sprout_private.retention_effective_purge_at(uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.materialize_retention_subjects(timestamptz) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.retention_interested_users(uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.retention_purge_row_allowed(jsonb) FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.retention_only_delete() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.purge_retention_subject(uuid, uuid, timestamptz) FROM PUBLIC;
