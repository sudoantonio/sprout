-- HLR-04 / HLR-05: immutable questionnaire history, task-pinned
-- submissions, and provenance-safe encrypted attachments.

-- Questionnaire versions are mutable only while unpublished. Publishing is
-- the one-way boundary that freezes the version, its questions, and options.
DROP TRIGGER questionnaire_versions_immutable ON questionnaire_versions;
DROP TRIGGER questionnaire_questions_immutable ON questionnaire_questions;
DROP TRIGGER questionnaire_options_immutable ON questionnaire_options;

ALTER TABLE questionnaire_versions
    ADD COLUMN source_version_id uuid,
    ADD COLUMN revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now(),
    ADD CONSTRAINT questionnaire_versions_source_fk
        FOREIGN KEY (project_id, source_version_id)
        REFERENCES questionnaire_versions (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT questionnaire_versions_not_self_source
        CHECK (source_version_id IS NULL OR source_version_id <> id);

CREATE UNIQUE INDEX questionnaire_versions_one_draft
    ON questionnaire_versions (project_id, questionnaire_id)
    WHERE published_at IS NULL;

CREATE TRIGGER questionnaire_versions_touch_updated_at
BEFORE UPDATE ON questionnaire_versions
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE FUNCTION sprout_private.validate_questionnaire_version_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.published_at IS NOT NULL THEN
            RAISE EXCEPTION 'published questionnaire versions are immutable'
                USING ERRCODE = '55000';
        END IF;
        RETURN OLD;
    END IF;

    IF OLD.published_at IS NOT NULL AND NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'published questionnaire versions are immutable'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.project_id IS DISTINCT FROM OLD.project_id
       OR NEW.questionnaire_id IS DISTINCT FROM OLD.questionnaire_id
       OR NEW.version_number IS DISTINCT FROM OLD.version_number
       OR NEW.source_version_id IS DISTINCT FROM OLD.source_version_id
       OR NEW.created_by_identity_id IS DISTINCT FROM OLD.created_by_identity_id
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
    THEN
        RAISE EXCEPTION 'questionnaire version identity is immutable'
            USING ERRCODE = '55000';
    END IF;
    IF (
        NEW.encrypted_payload IS DISTINCT FROM OLD.encrypted_payload
        OR NEW.content_hash IS DISTINCT FROM OLD.content_hash
        OR NEW.published_at IS DISTINCT FROM OLD.published_at
    ) AND NEW.revision <> OLD.revision + 1
    THEN
        RAISE EXCEPTION 'questionnaire version revision must advance exactly once'
            USING ERRCODE = '40001';
    END IF;
    IF OLD.published_at IS NULL
       AND NEW.published_at IS NOT NULL
       AND NEW.published_at < NEW.created_at
    THEN
        RAISE EXCEPTION 'questionnaire publication cannot precede creation'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER questionnaire_versions_validate_mutation
BEFORE UPDATE OR DELETE ON questionnaire_versions
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_questionnaire_version_mutation();

ALTER TABLE questionnaire_questions
    DROP CONSTRAINT questionnaire_questions_question_kind_check;
ALTER TABLE questionnaire_questions
    ADD CONSTRAINT questionnaire_questions_supported_kind
        CHECK (question_kind IN ('open', 'single_choice', 'multiple_choice', 'boolean'))
        NOT VALID;

CREATE FUNCTION sprout_private.validate_questionnaire_child_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    candidate_project_id uuid;
    candidate_version_id uuid;
    publication_time timestamptz;
BEGIN
    candidate_project_id := COALESCE(NEW.project_id, OLD.project_id);
    IF TG_TABLE_NAME = 'questionnaire_questions' THEN
        candidate_version_id :=
            COALESCE(NEW.questionnaire_version_id, OLD.questionnaire_version_id);
    ELSE
        SELECT question.questionnaire_version_id
        INTO candidate_version_id
        FROM questionnaire_questions question
        WHERE question.project_id = candidate_project_id
          AND question.id = COALESCE(NEW.question_id, OLD.question_id);
    END IF;

    SELECT version.published_at INTO publication_time
    FROM questionnaire_versions version
    WHERE version.project_id = candidate_project_id
      AND version.id = candidate_version_id
    FOR UPDATE;

    IF candidate_version_id IS NULL THEN
        RAISE EXCEPTION 'questionnaire child has no version'
            USING ERRCODE = '23503';
    END IF;
    IF publication_time IS NOT NULL THEN
        RAISE EXCEPTION 'published questionnaire questions and options are immutable'
            USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER questionnaire_questions_validate_mutation
BEFORE INSERT OR UPDATE OR DELETE ON questionnaire_questions
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_questionnaire_child_mutation();
CREATE TRIGGER questionnaire_options_validate_mutation
BEFORE INSERT OR UPDATE OR DELETE ON questionnaire_options
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_questionnaire_child_mutation();

CREATE FUNCTION sprout_private.validate_questionnaire_publication()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.published_at IS NULL AND NEW.published_at IS NOT NULL THEN
        IF NOT EXISTS (
            SELECT 1
            FROM questionnaire_questions question
            WHERE question.project_id = NEW.project_id
              AND question.questionnaire_version_id = NEW.id
        ) OR EXISTS (
            SELECT 1
            FROM questionnaire_questions question
            WHERE question.project_id = NEW.project_id
              AND question.questionnaire_version_id = NEW.id
              AND (
                  (
                      question.question_kind IN ('single_choice', 'multiple_choice')
                      AND NOT EXISTS (
                          SELECT 1
                          FROM questionnaire_options option
                          WHERE option.project_id = question.project_id
                            AND option.question_id = question.id
                      )
                  )
                  OR (
                      question.question_kind IN ('open', 'boolean')
                      AND EXISTS (
                          SELECT 1
                          FROM questionnaire_options option
                          WHERE option.project_id = question.project_id
                            AND option.question_id = question.id
                      )
                  )
              )
        ) THEN
            RAISE EXCEPTION 'questionnaire question kinds and options are inconsistent'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER questionnaire_versions_validate_publication
BEFORE UPDATE ON questionnaire_versions
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_questionnaire_publication();

-- A concrete task permanently pins one published questionnaire version.
ALTER TABLE tasks
    ADD COLUMN questionnaire_version_id uuid,
    ADD CONSTRAINT tasks_questionnaire_version_fk
        FOREIGN KEY (project_id, questionnaire_version_id)
        REFERENCES questionnaire_versions (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

CREATE INDEX tasks_questionnaire_version_idx
    ON tasks (project_id, questionnaire_version_id)
    WHERE questionnaire_version_id IS NOT NULL;

CREATE FUNCTION sprout_private.validate_task_questionnaire_pin()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    pinned_published_at timestamptz;
BEGIN
    IF TG_OP = 'UPDATE'
       AND NEW.questionnaire_version_id IS DISTINCT FROM OLD.questionnaire_version_id
    THEN
        RAISE EXCEPTION 'task questionnaire version pin is immutable'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.questionnaire_version_id IS NOT NULL THEN
        SELECT version.published_at INTO pinned_published_at
        FROM questionnaire_versions version
        WHERE version.project_id = NEW.project_id
          AND version.id = NEW.questionnaire_version_id;
        IF pinned_published_at IS NULL THEN
            RAISE EXCEPTION 'tasks may pin only a published questionnaire version'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER tasks_validate_questionnaire_pin
BEFORE INSERT OR UPDATE ON tasks
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_task_questionnaire_pin();

-- Draft responses can be replaced by the active assignee. Final responses are
-- immutable and carry both device-signature suites.
DROP TRIGGER questionnaire_submissions_immutable ON questionnaire_submissions;
DROP TRIGGER questionnaire_answers_immutable ON questionnaire_answers;

ALTER TABLE questionnaire_submissions
    RENAME COLUMN signature TO classical_signature;
ALTER TABLE questionnaire_submissions
    DROP CONSTRAINT questionnaire_submissions_state_check,
    ALTER COLUMN state SET DEFAULT 'draft',
    ALTER COLUMN classical_signature DROP NOT NULL,
    ALTER COLUMN submitted_at DROP NOT NULL,
    ALTER COLUMN submitted_at DROP DEFAULT,
    ADD COLUMN task_id uuid,
    ADD COLUMN assignment_id uuid,
    ADD COLUMN signer_device_id uuid,
    ADD COLUMN signer_device_key_version integer,
    ADD COLUMN post_quantum_signature bytea,
    ADD COLUMN revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    ADD COLUMN idempotency_key uuid,
    ADD COLUMN request_hash bytea,
    ADD COLUMN created_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now(),
    ADD CONSTRAINT questionnaire_submissions_state
        CHECK (state IN ('draft', 'submitted')),
    ADD CONSTRAINT questionnaire_submissions_task_fk
        FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT questionnaire_submissions_assignment_fk
        FOREIGN KEY (project_id, task_id, assignment_id)
        REFERENCES task_assignments (project_id, task_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT questionnaire_submissions_signer_key_fk
        FOREIGN KEY (
            submitted_by_identity_id,
            signer_device_id,
            signer_device_key_version
        ) REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT questionnaire_submissions_signature_shape CHECK (
        (
            state = 'draft'
            AND submitted_at IS NULL
            AND classical_signature IS NULL
            AND post_quantum_signature IS NULL
            AND signer_device_id IS NULL
            AND signer_device_key_version IS NULL
        )
        OR (
            state = 'submitted'
            AND submitted_at IS NOT NULL
            AND octet_length(classical_signature) = 64
            AND octet_length(post_quantum_signature) > 0
            AND signer_device_id IS NOT NULL
            AND signer_device_key_version > 0
            AND idempotency_key IS NOT NULL
            AND octet_length(request_hash) = 32
        )
    );

CREATE UNIQUE INDEX questionnaire_submissions_one_per_task
    ON questionnaire_submissions (project_id, task_id)
    WHERE task_id IS NOT NULL;
CREATE UNIQUE INDEX questionnaire_submissions_idempotency
    ON questionnaire_submissions (
        project_id, submitted_by_identity_id, idempotency_key
    )
    WHERE idempotency_key IS NOT NULL;
CREATE INDEX questionnaire_submissions_assignment_draft
    ON questionnaire_submissions (project_id, assignment_id, updated_at)
    WHERE state = 'draft';

CREATE TRIGGER questionnaire_submissions_touch_updated_at
BEFORE UPDATE ON questionnaire_submissions
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE FUNCTION sprout_private.validate_questionnaire_submission()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    pinned_version_id uuid;
    active_assignee_id uuid;
    version_published_at timestamptz;
    signer_active boolean;
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'questionnaire submission history is retained'
            USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'UPDATE' AND OLD.state = 'submitted' AND NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'submitted questionnaire responses are immutable'
            USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'UPDATE' AND (
        NEW.id IS DISTINCT FROM OLD.id
        OR NEW.project_id IS DISTINCT FROM OLD.project_id
        OR NEW.questionnaire_version_id IS DISTINCT FROM OLD.questionnaire_version_id
        OR NEW.task_id IS DISTINCT FROM OLD.task_id
        OR NEW.assignment_id IS DISTINCT FROM OLD.assignment_id
        OR NEW.submitted_by_identity_id IS DISTINCT FROM OLD.submitted_by_identity_id
        OR NEW.client_submission_id IS DISTINCT FROM OLD.client_submission_id
    ) THEN
        RAISE EXCEPTION 'questionnaire submission identity is immutable'
            USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT'
       AND sprout_private.current_identity_id()
           IS DISTINCT FROM NEW.submitted_by_identity_id
    THEN
        RAISE EXCEPTION 'only the active assignee may create or edit a draft submission'
            USING ERRCODE = '42501';
    ELSIF TG_OP = 'UPDATE'
       AND OLD.state = 'draft'
       AND sprout_private.current_identity_id()
           IS DISTINCT FROM NEW.submitted_by_identity_id
    THEN
        RAISE EXCEPTION 'only the active assignee may create or edit a draft submission'
            USING ERRCODE = '42501';
    END IF;

    SELECT
        task.questionnaire_version_id,
        assignment.assignee_identity_id
    INTO pinned_version_id, active_assignee_id
    FROM tasks task
    JOIN task_assignments assignment
      ON assignment.project_id = task.project_id
     AND assignment.task_id = task.id
     AND assignment.id = NEW.assignment_id
     AND assignment.revoked_at IS NULL
    WHERE task.project_id = NEW.project_id
      AND task.id = NEW.task_id
      AND task.deleted_at IS NULL;

    SELECT published_at INTO version_published_at
    FROM questionnaire_versions
    WHERE project_id = NEW.project_id
      AND id = NEW.questionnaire_version_id;

    IF pinned_version_id IS NULL
       OR pinned_version_id IS DISTINCT FROM NEW.questionnaire_version_id
       OR active_assignee_id IS DISTINCT FROM NEW.submitted_by_identity_id
       OR version_published_at IS NULL
    THEN
        RAISE EXCEPTION 'submission requires the active assignee and task-pinned published version'
            USING ERRCODE = '42501';
    END IF;

    IF TG_OP = 'UPDATE'
       AND (
           NEW.encrypted_payload IS DISTINCT FROM OLD.encrypted_payload
           OR NEW.state IS DISTINCT FROM OLD.state
           OR NEW.classical_signature IS DISTINCT FROM OLD.classical_signature
           OR NEW.post_quantum_signature IS DISTINCT FROM OLD.post_quantum_signature
       )
       AND NEW.revision <> OLD.revision + 1
    THEN
        RAISE EXCEPTION 'questionnaire submission revision must advance exactly once'
            USING ERRCODE = '40001';
    END IF;

    IF NEW.state = 'submitted' THEN
        SELECT EXISTS (
            SELECT 1
            FROM device_keys key
            WHERE key.identity_id = NEW.submitted_by_identity_id
              AND key.device_id = NEW.signer_device_id
              AND key.key_version = NEW.signer_device_key_version
              AND key.revoked_at IS NULL
        ) INTO signer_active;
        IF NOT signer_active THEN
            RAISE EXCEPTION 'submission signatures require an active assignee device key'
                USING ERRCODE = '42501';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER questionnaire_submissions_validate_mutation
BEFORE INSERT OR UPDATE OR DELETE ON questionnaire_submissions
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_questionnaire_submission();

ALTER TABLE questionnaire_answers
    ADD CONSTRAINT questionnaire_answers_full_identity_unique
        UNIQUE (
            project_id, questionnaire_version_id,
            submission_id, question_id, id
        );

CREATE TABLE questionnaire_answer_options (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    questionnaire_version_id uuid NOT NULL,
    submission_id uuid NOT NULL,
    answer_id uuid NOT NULL,
    question_id uuid NOT NULL,
    option_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT questionnaire_answer_options_answer_fk
        FOREIGN KEY (
            project_id, questionnaire_version_id,
            submission_id, question_id, answer_id
        ) REFERENCES questionnaire_answers (
            project_id, questionnaire_version_id,
            submission_id, question_id, id
        )
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_answer_options_option_fk
        FOREIGN KEY (project_id, question_id, option_id)
        REFERENCES questionnaire_options (project_id, question_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_answer_options_project_id_unique
        UNIQUE (project_id, id),
    CONSTRAINT questionnaire_answer_options_unique
        UNIQUE (project_id, answer_id, option_id)
);

CREATE FUNCTION sprout_private.validate_questionnaire_answer_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    candidate_project_id uuid;
    candidate_submission_id uuid;
    submission_state text;
BEGIN
    candidate_project_id := COALESCE(NEW.project_id, OLD.project_id);
    candidate_submission_id := COALESCE(NEW.submission_id, OLD.submission_id);
    SELECT state INTO submission_state
    FROM questionnaire_submissions
    WHERE project_id = candidate_project_id
      AND id = candidate_submission_id
    FOR UPDATE;
    IF submission_state IS DISTINCT FROM 'draft' THEN
        RAISE EXCEPTION 'submitted questionnaire answers are immutable'
            USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER questionnaire_answers_validate_mutation
BEFORE INSERT OR UPDATE OR DELETE ON questionnaire_answers
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_questionnaire_answer_mutation();
CREATE TRIGGER questionnaire_answer_options_validate_mutation
BEFORE INSERT OR UPDATE OR DELETE ON questionnaire_answer_options
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_questionnaire_answer_mutation();

CREATE FUNCTION sprout_private.validate_questionnaire_answer_options()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    answer_count bigint;
    selected_count bigint;
BEGIN
    IF NEW.state = 'submitted' AND OLD.state = 'draft' THEN
        IF EXISTS (
            SELECT 1
            FROM questionnaire_questions question
            WHERE question.project_id = NEW.project_id
              AND question.questionnaire_version_id = NEW.questionnaire_version_id
              AND question.required
              AND NOT EXISTS (
                  SELECT 1
                  FROM questionnaire_answers answer
                  WHERE answer.project_id = question.project_id
                    AND answer.questionnaire_version_id =
                        question.questionnaire_version_id
                    AND answer.submission_id = NEW.id
                    AND answer.question_id = question.id
              )
        ) THEN
            RAISE EXCEPTION 'required questionnaire answers are missing'
                USING ERRCODE = '23514';
        END IF;

        FOR answer_count, selected_count IN
            SELECT
                CASE question.question_kind
                    WHEN 'single_choice' THEN 1
                    WHEN 'multiple_choice' THEN 2
                    ELSE 0
                END,
                count(selected.id)
            FROM questionnaire_answers answer
            JOIN questionnaire_questions question
              ON question.project_id = answer.project_id
             AND question.questionnaire_version_id =
                 answer.questionnaire_version_id
             AND question.id = answer.question_id
            LEFT JOIN questionnaire_answer_options selected
              ON selected.project_id = answer.project_id
             AND selected.answer_id = answer.id
            WHERE answer.project_id = NEW.project_id
              AND answer.questionnaire_version_id = NEW.questionnaire_version_id
              AND answer.submission_id = NEW.id
            GROUP BY answer.id, question.question_kind
        LOOP
            IF (answer_count = 0 AND selected_count <> 0)
               OR (answer_count = 1 AND selected_count <> 1)
               OR (answer_count = 2 AND selected_count < 1)
            THEN
                RAISE EXCEPTION 'questionnaire answer options do not match question kind'
                    USING ERRCODE = '23514';
            END IF;
        END LOOP;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER questionnaire_submissions_validate_answers
BEFORE UPDATE ON questionnaire_submissions
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_questionnaire_answer_options();

-- Bind every blob to the resource-key epoch that protects its encrypted
-- metadata. New filesystem keys are server-generated opaque basenames.
ALTER TABLE file_blobs
    ADD COLUMN resource_node_id uuid,
    ADD CONSTRAINT file_blobs_resource_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT file_blobs_sha256_length
        CHECK (octet_length(ciphertext_hash) = 32) NOT VALID,
    ADD CONSTRAINT file_blobs_opaque_storage_key
        CHECK (storage_key ~ '^[0-9a-f]{32}[.]blob$') NOT VALID;

CREATE TABLE pretask_template_attachments (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    preset_version_id uuid NOT NULL,
    pretask_id uuid NOT NULL,
    blob_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    encrypted_metadata bytea NOT NULL,
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT pretask_template_attachments_pretask_fk
        FOREIGN KEY (project_id, preset_version_id, pretask_id)
        REFERENCES preset_pretasks (project_id, preset_version_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT pretask_template_attachments_blob_fk
        FOREIGN KEY (project_id, blob_id)
        REFERENCES file_blobs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT pretask_template_attachments_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT pretask_template_attachments_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT pretask_template_attachments_project_id_unique
        UNIQUE (project_id, id),
    CONSTRAINT pretask_template_attachments_blob_unique
        UNIQUE (project_id, blob_id),
    CONSTRAINT pretask_template_attachments_metadata_nonempty
        CHECK (octet_length(encrypted_metadata) > 0)
);

CREATE TABLE task_required_attachments (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    source_template_attachment_id uuid,
    blob_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    encrypted_snapshot bytea NOT NULL,
    materialized_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT task_required_attachments_task_fk
        FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_required_attachments_source_fk
        FOREIGN KEY (project_id, source_template_attachment_id)
        REFERENCES pretask_template_attachments (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_required_attachments_blob_fk
        FOREIGN KEY (project_id, blob_id)
        REFERENCES file_blobs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_required_attachments_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_required_attachments_materializer_fk
        FOREIGN KEY (project_id, materialized_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_required_attachments_project_id_unique
        UNIQUE (project_id, id),
    CONSTRAINT task_required_attachments_blob_unique
        UNIQUE (project_id, blob_id),
    CONSTRAINT task_required_attachments_snapshot_nonempty
        CHECK (octet_length(encrypted_snapshot) > 0)
);

CREATE TABLE task_completed_attachments (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    assignment_id uuid NOT NULL,
    required_attachment_id uuid,
    blob_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    encrypted_metadata bytea NOT NULL,
    uploaded_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT task_completed_attachments_assignment_fk
        FOREIGN KEY (project_id, task_id, assignment_id)
        REFERENCES task_assignments (project_id, task_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_completed_attachments_required_fk
        FOREIGN KEY (project_id, required_attachment_id)
        REFERENCES task_required_attachments (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_completed_attachments_blob_fk
        FOREIGN KEY (project_id, blob_id)
        REFERENCES file_blobs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_completed_attachments_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_completed_attachments_uploader_fk
        FOREIGN KEY (project_id, uploaded_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_completed_attachments_project_id_unique
        UNIQUE (project_id, id),
    CONSTRAINT task_completed_attachments_blob_unique
        UNIQUE (project_id, blob_id),
    CONSTRAINT task_completed_attachments_metadata_nonempty
        CHECK (octet_length(encrypted_metadata) > 0)
);

CREATE FUNCTION sprout_private.validate_attachment_provenance()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    task_resource_id uuid;
    task_pretask_id uuid;
    source_pretask_id uuid;
    source_template_attachment_id uuid;
    required_task_id uuid;
    active_assignee_id uuid;
BEGIN
    SELECT task.resource_node_id, task.source_pretask_id
    INTO task_resource_id, task_pretask_id
    FROM tasks task
    WHERE task.project_id = NEW.project_id
      AND task.id = NEW.task_id;
    IF task_resource_id IS DISTINCT FROM NEW.resource_node_id THEN
        RAISE EXCEPTION 'task attachment must use the task resource key'
            USING ERRCODE = '23514';
    END IF;

    IF TG_TABLE_NAME = 'task_required_attachments'
    THEN
        source_template_attachment_id :=
            (to_jsonb(NEW) ->> 'source_template_attachment_id')::uuid;
        IF source_template_attachment_id IS NOT NULL THEN
            SELECT template.pretask_id INTO source_pretask_id
            FROM pretask_template_attachments template
            WHERE template.project_id = NEW.project_id
              AND template.id = source_template_attachment_id;
            IF task_pretask_id IS DISTINCT FROM source_pretask_id THEN
                RAISE EXCEPTION 'required attachment provenance does not match task pretask'
                    USING ERRCODE = '23514';
            END IF;
        END IF;
    ELSIF TG_TABLE_NAME = 'task_completed_attachments' THEN
        SELECT assignment.assignee_identity_id INTO active_assignee_id
        FROM task_assignments assignment
        WHERE assignment.project_id = NEW.project_id
          AND assignment.task_id = NEW.task_id
          AND assignment.id = NEW.assignment_id
          AND assignment.revoked_at IS NULL;
        IF active_assignee_id IS DISTINCT FROM NEW.uploaded_by_identity_id THEN
            RAISE EXCEPTION 'only the active assignee may upload a completed attachment'
                USING ERRCODE = '42501';
        END IF;
        IF NEW.required_attachment_id IS NOT NULL THEN
            SELECT required.task_id INTO required_task_id
            FROM task_required_attachments required
            WHERE required.project_id = NEW.project_id
              AND required.id = NEW.required_attachment_id;
            IF required_task_id IS DISTINCT FROM NEW.task_id THEN
                RAISE EXCEPTION 'completed attachment requirement belongs to another task'
                    USING ERRCODE = '23514';
            END IF;
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER task_required_attachments_validate_provenance
BEFORE INSERT ON task_required_attachments
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_attachment_provenance();
CREATE TRIGGER task_completed_attachments_validate_provenance
BEFORE INSERT ON task_completed_attachments
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_attachment_provenance();

CREATE TRIGGER pretask_template_attachments_immutable
BEFORE UPDATE OR DELETE ON pretask_template_attachments
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();
CREATE TRIGGER task_required_attachments_immutable
BEFORE UPDATE OR DELETE ON task_required_attachments
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();
CREATE TRIGGER task_completed_attachments_immutable
BEFORE UPDATE OR DELETE ON task_completed_attachments
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE FUNCTION sprout_private.validate_file_blob_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.upload_state <> 'deleted' THEN
            RAISE EXCEPTION 'blob metadata must be retention-deleted before purge'
                USING ERRCODE = '55000';
        END IF;
        RETURN OLD;
    END IF;
    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.project_id IS DISTINCT FROM OLD.project_id
       OR NEW.storage_provider IS DISTINCT FROM OLD.storage_provider
       OR NEW.storage_key IS DISTINCT FROM OLD.storage_key
       OR NEW.ciphertext_size IS DISTINCT FROM OLD.ciphertext_size
       OR NEW.ciphertext_hash IS DISTINCT FROM OLD.ciphertext_hash
       OR NEW.key_epoch IS DISTINCT FROM OLD.key_epoch
       OR NEW.resource_node_id IS DISTINCT FROM OLD.resource_node_id
       OR NEW.encrypted_metadata IS DISTINCT FROM OLD.encrypted_metadata
       OR NEW.created_by_identity_id IS DISTINCT FROM OLD.created_by_identity_id
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
    THEN
        RAISE EXCEPTION 'blob identity, ciphertext declaration, and key binding are immutable'
            USING ERRCODE = '55000';
    END IF;
    IF NOT (
        NEW IS NOT DISTINCT FROM OLD
        OR (OLD.upload_state = 'pending' AND NEW.upload_state IN ('available', 'quarantined', 'deleted'))
        OR (OLD.upload_state = 'available' AND NEW.upload_state IN ('quarantined', 'deleted'))
        OR (OLD.upload_state = 'quarantined' AND NEW.upload_state = 'deleted')
    ) THEN
        RAISE EXCEPTION 'invalid blob state transition'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER file_blobs_validate_mutation
BEFORE UPDATE OR DELETE ON file_blobs
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_file_blob_mutation();

DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'questionnaire_answer_options',
        'pretask_template_attachments',
        'task_required_attachments',
        'task_completed_attachments'
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
