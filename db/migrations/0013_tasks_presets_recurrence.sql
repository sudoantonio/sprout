-- HLR-03: mixed task kinds, immutable preset snapshots, assignee-only
-- completion, and client-calculated recurrence.

ALTER TABLE task_lists
    ADD COLUMN archived_at timestamptz;

CREATE TABLE recurrence_series (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    task_list_id uuid NOT NULL,
    encrypted_rule bytea NOT NULL,
    payload_version bigint NOT NULL DEFAULT 1 CHECK (payload_version > 0),
    state text NOT NULL DEFAULT 'active'
        CHECK (state IN ('active', 'archived')),
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    archived_at timestamptz,
    CONSTRAINT recurrence_series_list_fk
        FOREIGN KEY (project_id, task_list_id)
        REFERENCES task_lists (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT recurrence_series_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT recurrence_series_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT recurrence_series_rule_nonempty
        CHECK (octet_length(encrypted_rule) > 0),
    CONSTRAINT recurrence_series_archive_shape
        CHECK ((state = 'archived') = (archived_at IS NOT NULL))
);

CREATE INDEX recurrence_series_list_active_idx
    ON recurrence_series (project_id, task_list_id, created_at)
    WHERE state = 'active';
CREATE TRIGGER recurrence_series_touch_updated_at
BEFORE UPDATE ON recurrence_series
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

ALTER TABLE preset_pretasks
    ADD COLUMN task_kind text NOT NULL DEFAULT 'priority'
        CHECK (task_kind IN ('priority', 'deadline', 'recurring')),
    ADD CONSTRAINT preset_pretasks_project_version_id_unique
        UNIQUE (project_id, preset_version_id, id);

CREATE TABLE preset_assignments (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    preset_version_id uuid NOT NULL,
    destination_task_list_id uuid NOT NULL,
    assigned_to_identity_id uuid NOT NULL,
    assigned_by_identity_id uuid NOT NULL,
    encrypted_payload bytea NOT NULL,
    payload_version bigint NOT NULL DEFAULT 1 CHECK (payload_version > 0),
    state text NOT NULL DEFAULT 'active'
        CHECK (state IN ('active', 'materialized', 'revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    materialized_at timestamptz,
    revoked_at timestamptz,
    CONSTRAINT preset_assignments_version_fk
        FOREIGN KEY (project_id, preset_version_id)
        REFERENCES preset_versions (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_assignments_list_fk
        FOREIGN KEY (project_id, destination_task_list_id)
        REFERENCES task_lists (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_assignments_assignee_fk
        FOREIGN KEY (project_id, assigned_to_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_assignments_assigner_fk
        FOREIGN KEY (project_id, assigned_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_assignments_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT preset_assignments_project_version_id_unique
        UNIQUE (project_id, preset_version_id, id),
    CONSTRAINT preset_assignments_payload_nonempty
        CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT preset_assignments_state_shape CHECK (
        (state = 'active' AND materialized_at IS NULL AND revoked_at IS NULL)
        OR (state = 'materialized' AND materialized_at IS NOT NULL AND revoked_at IS NULL)
        OR (state = 'revoked' AND revoked_at IS NOT NULL)
    )
);

CREATE INDEX preset_assignments_assignee_active_idx
    ON preset_assignments (project_id, assigned_to_identity_id, created_at)
    WHERE state = 'active';
CREATE TRIGGER preset_assignments_touch_updated_at
BEFORE UPDATE ON preset_assignments
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE preset_assignment_values (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    preset_assignment_id uuid NOT NULL,
    preset_version_id uuid NOT NULL,
    pretask_id uuid NOT NULL,
    task_kind text NOT NULL
        CHECK (task_kind IN ('priority', 'deadline', 'recurring')),
    encrypted_selected_value bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT preset_assignment_values_assignment_fk
        FOREIGN KEY (project_id, preset_version_id, preset_assignment_id)
        REFERENCES preset_assignments (project_id, preset_version_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_assignment_values_pretask_fk
        FOREIGN KEY (project_id, preset_version_id, pretask_id)
        REFERENCES preset_pretasks (project_id, preset_version_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_assignment_values_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT preset_assignment_values_one_per_pretask
        UNIQUE (project_id, preset_assignment_id, pretask_id),
    CONSTRAINT preset_assignment_values_selected_nonempty
        CHECK (octet_length(encrypted_selected_value) > 0)
);

CREATE TRIGGER preset_assignment_values_immutable
BEFORE UPDATE OR DELETE ON preset_assignment_values
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

ALTER TABLE tasks
    ADD COLUMN task_kind text,
    ADD COLUMN encrypted_value_snapshot bytea,
    ADD COLUMN source_pretask_id uuid,
    ADD COLUMN preset_assignment_id uuid,
    ADD COLUMN copied_from_task_id uuid,
    ADD COLUMN recurrence_series_id uuid,
    ADD COLUMN occurrence_number bigint,
    ADD COLUMN completed_by_identity_id uuid,
    ADD COLUMN completed_at timestamptz,
    ADD COLUMN created_by_identity_id uuid;

UPDATE tasks task
SET
    task_kind = 'priority',
    encrypted_value_snapshot = task.encrypted_payload,
    created_by_identity_id = resource.created_by_identity_id
FROM resource_nodes resource
WHERE resource.project_id = task.project_id
  AND resource.id = task.resource_node_id;

ALTER TABLE tasks
    ALTER COLUMN task_kind SET NOT NULL,
    ALTER COLUMN encrypted_value_snapshot SET NOT NULL,
    ALTER COLUMN created_by_identity_id SET NOT NULL,
    ADD CONSTRAINT tasks_kind_check
        CHECK (task_kind IN ('priority', 'deadline', 'recurring')),
    ADD CONSTRAINT tasks_value_snapshot_nonempty
        CHECK (octet_length(encrypted_value_snapshot) > 0),
    ADD CONSTRAINT tasks_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT tasks_copied_from_fk
        FOREIGN KEY (project_id, copied_from_task_id)
        REFERENCES tasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT tasks_recurrence_fk
        FOREIGN KEY (project_id, recurrence_series_id)
        REFERENCES recurrence_series (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT tasks_completed_by_fk
        FOREIGN KEY (project_id, completed_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT tasks_copy_new_identity
        CHECK (copied_from_task_id IS NULL OR copied_from_task_id <> id),
    ADD CONSTRAINT tasks_recurrence_shape CHECK (
        (
            task_kind = 'recurring'
            AND recurrence_series_id IS NOT NULL
            AND occurrence_number IS NOT NULL
            AND occurrence_number > 0
        )
        OR (
            task_kind <> 'recurring'
            AND recurrence_series_id IS NULL
            AND occurrence_number IS NULL
        )
    ) NOT VALID,
    ADD CONSTRAINT tasks_completion_shape CHECK (
        (
            state = 'completed'
            AND completed_by_identity_id IS NOT NULL
            AND completed_at IS NOT NULL
        )
        OR (
            state <> 'completed'
            AND completed_by_identity_id IS NULL
            AND completed_at IS NULL
        )
    ) NOT VALID,
    ADD CONSTRAINT tasks_preset_source_shape CHECK (
        (source_pretask_id IS NULL AND preset_assignment_id IS NULL)
        OR (source_pretask_id IS NOT NULL AND preset_assignment_id IS NOT NULL)
    );

CREATE UNIQUE INDEX tasks_recurrence_occurrence_unique
    ON tasks (project_id, recurrence_series_id, occurrence_number)
    WHERE recurrence_series_id IS NOT NULL;
CREATE INDEX tasks_mixed_list_idx
    ON tasks (project_id, task_list_id, task_kind, created_at)
    WHERE deleted_at IS NULL;
CREATE INDEX tasks_source_pretask_idx
    ON tasks (project_id, preset_assignment_id, source_pretask_id)
    WHERE source_pretask_id IS NOT NULL;

CREATE FUNCTION sprout_private.default_task_encrypted_fields()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    source_kind text;
    source_state text;
BEGIN
    NEW.task_kind := COALESCE(NEW.task_kind, 'priority');
    NEW.encrypted_value_snapshot :=
        COALESCE(NEW.encrypted_value_snapshot, NEW.encrypted_payload);
    IF NEW.created_by_identity_id IS NULL THEN
        SELECT created_by_identity_id INTO NEW.created_by_identity_id
        FROM resource_nodes
        WHERE project_id = NEW.project_id
          AND id = NEW.resource_node_id;
    END IF;
    IF NEW.copied_from_task_id IS NOT NULL THEN
        SELECT task_kind, state INTO source_kind, source_state
        FROM tasks
        WHERE project_id = NEW.project_id
          AND id = NEW.copied_from_task_id;
        IF source_state IS DISTINCT FROM 'completed'
           OR source_kind IS DISTINCT FROM NEW.task_kind
        THEN
            RAISE EXCEPTION 'copies require a completed source of the same kind'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER tasks_default_encrypted_fields
BEFORE INSERT ON tasks
FOR EACH ROW EXECUTE FUNCTION sprout_private.default_task_encrypted_fields();

CREATE TABLE preset_assignment_materialized_tasks (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    preset_assignment_id uuid NOT NULL,
    preset_version_id uuid NOT NULL,
    pretask_id uuid NOT NULL,
    task_id uuid NOT NULL,
    task_kind text NOT NULL
        CHECK (task_kind IN ('priority', 'deadline', 'recurring')),
    encrypted_selected_value_snapshot bytea NOT NULL,
    encrypted_task_snapshot bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT preset_assignment_materialized_assignment_fk
        FOREIGN KEY (project_id, preset_version_id, preset_assignment_id)
        REFERENCES preset_assignments (project_id, preset_version_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_assignment_materialized_value_fk
        FOREIGN KEY (project_id, preset_assignment_id, pretask_id)
        REFERENCES preset_assignment_values (
            project_id, preset_assignment_id, pretask_id
        )
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_assignment_materialized_task_fk
        FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT preset_assignment_materialized_project_id_unique
        UNIQUE (project_id, id),
    CONSTRAINT preset_assignment_materialized_one_per_pretask
        UNIQUE (project_id, preset_assignment_id, pretask_id),
    CONSTRAINT preset_assignment_materialized_task_unique
        UNIQUE (project_id, task_id),
    CONSTRAINT preset_assignment_materialized_selected_nonempty
        CHECK (octet_length(encrypted_selected_value_snapshot) > 0),
    CONSTRAINT preset_assignment_materialized_task_nonempty
        CHECK (octet_length(encrypted_task_snapshot) > 0)
);

ALTER TABLE tasks
    ADD CONSTRAINT tasks_preset_snapshot_fk
        FOREIGN KEY (project_id, preset_assignment_id, source_pretask_id)
        REFERENCES preset_assignment_materialized_tasks (
            project_id, preset_assignment_id, pretask_id
        )
        ON UPDATE RESTRICT ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED;

CREATE TRIGGER preset_assignment_materialized_tasks_immutable
BEFORE UPDATE OR DELETE ON preset_assignment_materialized_tasks
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE task_snapshot_history (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    payload_version bigint NOT NULL CHECK (payload_version > 0),
    task_kind text NOT NULL
        CHECK (task_kind IN ('priority', 'deadline', 'recurring')),
    encrypted_payload bytea NOT NULL,
    encrypted_value_snapshot bytea NOT NULL,
    source_pretask_id uuid,
    preset_assignment_id uuid,
    copied_from_task_id uuid,
    recurrence_series_id uuid,
    occurrence_number bigint,
    state text NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT task_snapshot_history_task_fk
        FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_snapshot_history_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT task_snapshot_history_version_unique
        UNIQUE (project_id, task_id, payload_version),
    CONSTRAINT task_snapshot_history_payload_nonempty
        CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT task_snapshot_history_value_nonempty
        CHECK (octet_length(encrypted_value_snapshot) > 0)
);

CREATE TRIGGER task_snapshot_history_immutable
BEFORE UPDATE OR DELETE ON task_snapshot_history
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

INSERT INTO task_snapshot_history (
    project_id, task_id, payload_version, task_kind,
    encrypted_payload, encrypted_value_snapshot,
    source_pretask_id, preset_assignment_id, copied_from_task_id,
    recurrence_series_id, occurrence_number, state
)
SELECT
    project_id, id, payload_version, task_kind,
    encrypted_payload, encrypted_value_snapshot,
    source_pretask_id, preset_assignment_id, copied_from_task_id,
    recurrence_series_id, occurrence_number, state
FROM tasks;

CREATE FUNCTION sprout_private.capture_task_snapshot()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'INSERT' OR NEW.payload_version IS DISTINCT FROM OLD.payload_version THEN
        INSERT INTO task_snapshot_history (
            project_id, task_id, payload_version, task_kind,
            encrypted_payload, encrypted_value_snapshot,
            source_pretask_id, preset_assignment_id, copied_from_task_id,
            recurrence_series_id, occurrence_number, state
        )
        VALUES (
            NEW.project_id, NEW.id, NEW.payload_version, NEW.task_kind,
            NEW.encrypted_payload, NEW.encrypted_value_snapshot,
            NEW.source_pretask_id, NEW.preset_assignment_id, NEW.copied_from_task_id,
            NEW.recurrence_series_id, NEW.occurrence_number, NEW.state
        );
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER tasks_capture_snapshot
AFTER INSERT OR UPDATE ON tasks
FOR EACH ROW EXECUTE FUNCTION sprout_private.capture_task_snapshot();

ALTER TABLE task_completions
    ADD COLUMN idempotency_key uuid,
    ADD COLUMN request_hash bytea,
    ADD COLUMN recurrence_series_id uuid,
    ADD COLUMN occurrence_number bigint,
    ADD COLUMN next_task_id uuid,
    ADD CONSTRAINT task_completions_series_fk
        FOREIGN KEY (project_id, recurrence_series_id)
        REFERENCES recurrence_series (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT task_completions_next_task_fk
        FOREIGN KEY (project_id, next_task_id)
        REFERENCES tasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT task_completions_request_hash_nonempty
        CHECK (request_hash IS NULL OR octet_length(request_hash) >= 16),
    ADD CONSTRAINT task_completions_recurrence_shape CHECK (
        (
            recurrence_series_id IS NULL
            AND occurrence_number IS NULL
            AND next_task_id IS NULL
        )
        OR (
            recurrence_series_id IS NOT NULL
            AND occurrence_number IS NOT NULL
            AND occurrence_number > 0
            AND next_task_id IS NOT NULL
            AND idempotency_key IS NOT NULL
            AND request_hash IS NOT NULL
        )
    );

CREATE UNIQUE INDEX task_completions_one_per_task_idx
    ON task_completions (project_id, task_id);
CREATE UNIQUE INDEX task_completions_idempotency_idx
    ON task_completions (
        project_id, recorded_by_identity_id, idempotency_key
    )
    WHERE idempotency_key IS NOT NULL;

CREATE FUNCTION sprout_private.validate_task_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.state = 'completed' THEN
            RAISE EXCEPTION 'completed tasks are immutable' USING ERRCODE = '55000';
        END IF;
        RETURN OLD;
    END IF;

    IF OLD.state = 'completed' AND NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'completed tasks are immutable' USING ERRCODE = '55000';
    END IF;
    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.project_id IS DISTINCT FROM OLD.project_id
       OR NEW.task_list_id IS DISTINCT FROM OLD.task_list_id
       OR NEW.resource_node_id IS DISTINCT FROM OLD.resource_node_id
       OR NEW.task_kind IS DISTINCT FROM OLD.task_kind
       OR NEW.source_pretask_id IS DISTINCT FROM OLD.source_pretask_id
       OR NEW.preset_assignment_id IS DISTINCT FROM OLD.preset_assignment_id
       OR NEW.copied_from_task_id IS DISTINCT FROM OLD.copied_from_task_id
       OR NEW.recurrence_series_id IS DISTINCT FROM OLD.recurrence_series_id
       OR NEW.occurrence_number IS DISTINCT FROM OLD.occurrence_number
       OR NEW.created_by_identity_id IS DISTINCT FROM OLD.created_by_identity_id
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
    THEN
        RAISE EXCEPTION 'task identity and provenance are immutable'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.state <> OLD.state AND NOT (OLD.state = 'open' AND NEW.state = 'completed') THEN
        RAISE EXCEPTION 'invalid task state transition' USING ERRCODE = '23514';
    END IF;
    IF (
        NEW.encrypted_payload IS DISTINCT FROM OLD.encrypted_payload
        OR NEW.encrypted_value_snapshot IS DISTINCT FROM OLD.encrypted_value_snapshot
        OR NEW.state IS DISTINCT FROM OLD.state
    ) AND NEW.payload_version <> OLD.payload_version + 1
    THEN
        RAISE EXCEPTION 'task payload version must advance exactly once'
            USING ERRCODE = '40001';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER tasks_validate_mutation
BEFORE UPDATE OR DELETE ON tasks
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_task_mutation();

DROP INDEX task_assignments_one_active_assignee_idx;
CREATE UNIQUE INDEX task_assignments_one_active_task_idx
    ON task_assignments (project_id, task_id)
    WHERE revoked_at IS NULL;

CREATE FUNCTION sprout_private.validate_task_assignment_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    task_state text;
BEGIN
    SELECT state INTO task_state
    FROM tasks
    WHERE project_id = COALESCE(NEW.project_id, OLD.project_id)
      AND id = COALESCE(NEW.task_id, OLD.task_id)
    FOR UPDATE;
    IF task_state = 'completed' THEN
        RAISE EXCEPTION 'completed tasks cannot be assigned or reassigned'
            USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'task assignment history is immutable'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER task_assignments_validate_mutation
BEFORE INSERT OR UPDATE OR DELETE ON task_assignments
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_task_assignment_mutation();

CREATE FUNCTION sprout_private.validate_task_completion()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    active_assignee uuid;
    task_state text;
    task_series uuid;
    task_occurrence bigint;
BEGIN
    SELECT
        task.state, task.recurrence_series_id, task.occurrence_number,
        assignment.assignee_identity_id
    INTO task_state, task_series, task_occurrence, active_assignee
    FROM tasks task
    JOIN task_assignments assignment
      ON assignment.project_id = task.project_id
     AND assignment.task_id = task.id
     AND assignment.id = NEW.assignment_id
     AND assignment.revoked_at IS NULL
    WHERE task.project_id = NEW.project_id
      AND task.id = NEW.task_id
    FOR UPDATE OF task;

    IF task_state IS NULL OR task_state <> 'open'
       OR active_assignee IS DISTINCT FROM NEW.assignee_identity_id
       OR active_assignee IS DISTINCT FROM NEW.recorded_by_identity_id
    THEN
        RAISE EXCEPTION 'only the active assignee may complete an open task'
            USING ERRCODE = '42501';
    END IF;
    IF task_series IS NULL THEN
        IF NEW.recurrence_series_id IS NOT NULL
           OR NEW.occurrence_number IS NOT NULL
           OR NEW.next_task_id IS NOT NULL
        THEN
            RAISE EXCEPTION 'non-recurring completion cannot create an occurrence'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.recurrence_series_id IS DISTINCT FROM task_series
          OR NEW.occurrence_number IS DISTINCT FROM task_occurrence + 1
          OR NOT EXISTS (
              SELECT 1 FROM tasks next_task
              WHERE next_task.project_id = NEW.project_id
                AND next_task.id = NEW.next_task_id
                AND next_task.recurrence_series_id = task_series
                AND next_task.occurrence_number = task_occurrence + 1
                AND next_task.state = 'open'
          )
    THEN
        RAISE EXCEPTION 'recurring completion requires the next sequential task'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER task_completions_validate_assignee
BEFORE INSERT ON task_completions
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_task_completion();

CREATE FUNCTION sprout_private.validate_completed_task_commit()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_TABLE_NAME = 'tasks' THEN
        IF NEW.state = 'completed' AND NOT EXISTS (
            SELECT 1
            FROM task_completions completion
            WHERE completion.project_id = NEW.project_id
              AND completion.task_id = NEW.id
              AND completion.recorded_by_identity_id =
                  NEW.completed_by_identity_id
        ) THEN
            RAISE EXCEPTION 'completed task requires an immutable completion audit'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NOT EXISTS (
        SELECT 1
        FROM tasks task
        WHERE task.project_id = NEW.project_id
          AND task.id = NEW.task_id
          AND task.state = 'completed'
          AND task.completed_by_identity_id = NEW.recorded_by_identity_id
    ) THEN
        RAISE EXCEPTION 'completion audit and completed task must commit atomically'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE CONSTRAINT TRIGGER tasks_require_completion_audit
AFTER INSERT OR UPDATE ON tasks
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
WHEN (NEW.state = 'completed')
EXECUTE FUNCTION sprout_private.validate_completed_task_commit();

CREATE CONSTRAINT TRIGGER task_completions_require_completed_task
AFTER INSERT ON task_completions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_completed_task_commit();

CREATE FUNCTION sprout_private.validate_preset_assignment()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    expected_count bigint;
    value_count bigint;
    task_count bigint;
    device_count bigint;
    uncovered_count bigint;
BEGIN
    IF OLD.state <> 'active' AND NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'terminal preset assignments are immutable'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.state = 'materialized' AND OLD.state = 'active' THEN
        SELECT count(*) INTO expected_count
        FROM preset_pretasks
        WHERE project_id = NEW.project_id
          AND preset_version_id = NEW.preset_version_id;
        SELECT count(*) INTO value_count
        FROM preset_assignment_values value
        JOIN preset_pretasks pretask
          ON pretask.project_id = value.project_id
         AND pretask.preset_version_id = value.preset_version_id
         AND pretask.id = value.pretask_id
         AND pretask.task_kind = value.task_kind
        WHERE value.project_id = NEW.project_id
          AND value.preset_assignment_id = NEW.id;
        SELECT count(*) INTO task_count
        FROM preset_assignment_materialized_tasks materialized
        JOIN tasks task
          ON task.project_id = materialized.project_id
         AND task.id = materialized.task_id
         AND task.task_kind = materialized.task_kind
         AND task.source_pretask_id = materialized.pretask_id
         AND task.preset_assignment_id = materialized.preset_assignment_id
         AND task.encrypted_value_snapshot =
             materialized.encrypted_selected_value_snapshot
         AND task.encrypted_payload = materialized.encrypted_task_snapshot
        JOIN task_assignments task_assignment
          ON task_assignment.project_id = task.project_id
         AND task_assignment.task_id = task.id
         AND task_assignment.assignee_identity_id =
             NEW.assigned_to_identity_id
         AND task_assignment.revoked_at IS NULL
        WHERE materialized.project_id = NEW.project_id
          AND materialized.preset_assignment_id = NEW.id;
        SELECT count(*) INTO device_count
        FROM device_keys
        WHERE identity_id = NEW.assigned_to_identity_id
          AND revoked_at IS NULL;
        SELECT count(*) INTO uncovered_count
        FROM preset_assignment_materialized_tasks materialized
        JOIN tasks task
          ON task.project_id = materialized.project_id
         AND task.id = materialized.task_id
        JOIN device_keys device_key
          ON device_key.identity_id = NEW.assigned_to_identity_id
         AND device_key.revoked_at IS NULL
        WHERE materialized.project_id = NEW.project_id
          AND materialized.preset_assignment_id = NEW.id
          AND NOT EXISTS (
              SELECT 1
              FROM resource_epochs epoch
              JOIN resource_key_envelopes envelope
                ON envelope.project_id = epoch.project_id
               AND envelope.resource_node_id = epoch.resource_node_id
               AND envelope.epoch = epoch.epoch
               AND envelope.recipient_identity_id = NEW.assigned_to_identity_id
               AND envelope.recipient_device_id = device_key.device_id
               AND envelope.recipient_device_key_version = device_key.key_version
               AND envelope.revoked_at IS NULL
              WHERE epoch.project_id = task.project_id
                AND epoch.resource_node_id = task.resource_node_id
                AND epoch.retired_at IS NULL
          );
        IF expected_count = 0
           OR value_count <> expected_count
           OR task_count <> expected_count
           OR device_count = 0
           OR uncovered_count <> 0
        THEN
            RAISE EXCEPTION 'preset materialization requires one compatible value, snapshot, and key envelope set per pretask'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER preset_assignments_validate_materialization
BEFORE UPDATE ON preset_assignments
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_preset_assignment();

ALTER TABLE recurrence_series ENABLE ROW LEVEL SECURITY;
ALTER TABLE recurrence_series FORCE ROW LEVEL SECURITY;
CREATE POLICY recurrence_series_project_isolation ON recurrence_series
    USING (sprout_private.is_project_member(project_id))
    WITH CHECK (sprout_private.is_project_member(project_id));

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'preset_assignments',
        'preset_assignment_values',
        'preset_assignment_materialized_tasks',
        'task_snapshot_history'
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
