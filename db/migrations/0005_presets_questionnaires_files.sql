CREATE TABLE presets (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    encrypted_metadata bytea NOT NULL,
    state text NOT NULL DEFAULT 'active'
        CHECK (state IN ('active', 'archived', 'deleted')),
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    archived_at timestamptz,
    deleted_at timestamptz,
    CONSTRAINT presets_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT presets_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT presets_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT presets_metadata_nonempty CHECK (octet_length(encrypted_metadata) > 0),
    CONSTRAINT presets_archived_state CHECK ((state = 'archived') = (archived_at IS NOT NULL)),
    CONSTRAINT presets_deleted_state CHECK ((state = 'deleted') = (deleted_at IS NOT NULL))
);

CREATE INDEX presets_active_project_idx ON presets (project_id, created_at)
    WHERE state = 'active';
CREATE TRIGGER presets_touch_updated_at
BEFORE UPDATE ON presets
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE preset_versions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    preset_id uuid NOT NULL,
    version_number integer NOT NULL CHECK (version_number > 0),
    encrypted_payload bytea NOT NULL,
    content_hash bytea NOT NULL,
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT preset_versions_preset_fk
        FOREIGN KEY (project_id, preset_id) REFERENCES presets (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_versions_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_versions_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT preset_versions_number_unique UNIQUE (project_id, preset_id, version_number),
    CONSTRAINT preset_versions_payload_nonempty CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT preset_versions_hash_nonempty CHECK (octet_length(content_hash) >= 16)
);

CREATE INDEX preset_versions_latest_idx
    ON preset_versions (project_id, preset_id, version_number DESC);
CREATE TRIGGER preset_versions_immutable
BEFORE UPDATE OR DELETE ON preset_versions
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE preset_pretasks (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    preset_version_id uuid NOT NULL,
    client_key uuid NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    encrypted_payload bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT preset_pretasks_version_fk
        FOREIGN KEY (project_id, preset_version_id)
        REFERENCES preset_versions (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_pretasks_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT preset_pretasks_client_unique
        UNIQUE (project_id, preset_version_id, client_key),
    CONSTRAINT preset_pretasks_ordinal_unique
        UNIQUE (project_id, preset_version_id, ordinal),
    CONSTRAINT preset_pretasks_payload_nonempty CHECK (octet_length(encrypted_payload) > 0)
);

CREATE TRIGGER preset_pretasks_immutable
BEFORE UPDATE OR DELETE ON preset_pretasks
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE preset_materializations (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    preset_version_id uuid NOT NULL,
    target_resource_node_id uuid NOT NULL,
    requested_by_identity_id uuid NOT NULL,
    idempotency_key uuid NOT NULL,
    state text NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'running', 'succeeded', 'failed')),
    encrypted_result bytea,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    CONSTRAINT preset_materializations_version_fk
        FOREIGN KEY (project_id, preset_version_id)
        REFERENCES preset_versions (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_materializations_target_fk
        FOREIGN KEY (project_id, target_resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_materializations_requester_fk
        FOREIGN KEY (project_id, requested_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_materializations_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT preset_materializations_idempotency_unique
        UNIQUE (project_id, requested_by_identity_id, idempotency_key),
    CONSTRAINT preset_materializations_result_nonempty
        CHECK (encrypted_result IS NULL OR octet_length(encrypted_result) > 0),
    CONSTRAINT preset_materializations_completion_state
        CHECK ((state IN ('succeeded', 'failed')) = (completed_at IS NOT NULL))
);

CREATE INDEX preset_materializations_work_idx
    ON preset_materializations (state, created_at)
    WHERE state IN ('pending', 'running');
CREATE TRIGGER preset_materializations_touch_updated_at
BEFORE UPDATE ON preset_materializations
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE preset_materialized_tasks (
    project_id uuid NOT NULL,
    materialization_id uuid NOT NULL,
    pretask_id uuid NOT NULL,
    task_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, materialization_id, pretask_id),
    CONSTRAINT preset_materialized_tasks_materialization_fk
        FOREIGN KEY (project_id, materialization_id)
        REFERENCES preset_materializations (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_materialized_tasks_pretask_fk
        FOREIGN KEY (project_id, pretask_id) REFERENCES preset_pretasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_materialized_tasks_task_fk
        FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT preset_materialized_tasks_task_unique UNIQUE (project_id, materialization_id, task_id)
);

CREATE TABLE questionnaires (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    encrypted_metadata bytea NOT NULL,
    state text NOT NULL DEFAULT 'draft'
        CHECK (state IN ('draft', 'published', 'closed', 'archived')),
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    closed_at timestamptz,
    CONSTRAINT questionnaires_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaires_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaires_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT questionnaires_metadata_nonempty CHECK (octet_length(encrypted_metadata) > 0),
    CONSTRAINT questionnaires_closed_state CHECK ((state = 'closed') = (closed_at IS NOT NULL))
);

CREATE TRIGGER questionnaires_touch_updated_at
BEFORE UPDATE ON questionnaires
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE questionnaire_versions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    questionnaire_id uuid NOT NULL,
    version_number integer NOT NULL CHECK (version_number > 0),
    encrypted_payload bytea NOT NULL,
    content_hash bytea NOT NULL,
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    published_at timestamptz,
    CONSTRAINT questionnaire_versions_questionnaire_fk
        FOREIGN KEY (project_id, questionnaire_id)
        REFERENCES questionnaires (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_versions_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_versions_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT questionnaire_versions_number_unique
        UNIQUE (project_id, questionnaire_id, version_number),
    CONSTRAINT questionnaire_versions_payload_nonempty CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT questionnaire_versions_hash_nonempty CHECK (octet_length(content_hash) >= 16)
);

CREATE INDEX questionnaire_versions_latest_idx
    ON questionnaire_versions (project_id, questionnaire_id, version_number DESC);
CREATE TRIGGER questionnaire_versions_immutable
BEFORE UPDATE OR DELETE ON questionnaire_versions
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE questionnaire_questions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    questionnaire_version_id uuid NOT NULL,
    client_key uuid NOT NULL,
    question_kind text NOT NULL
        CHECK (question_kind IN ('text', 'single_choice', 'multiple_choice', 'boolean', 'number', 'date')),
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    required boolean NOT NULL DEFAULT false,
    encrypted_payload bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT questionnaire_questions_version_fk
        FOREIGN KEY (project_id, questionnaire_version_id)
        REFERENCES questionnaire_versions (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_questions_project_version_id_unique
        UNIQUE (project_id, questionnaire_version_id, id),
    CONSTRAINT questionnaire_questions_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT questionnaire_questions_client_unique
        UNIQUE (project_id, questionnaire_version_id, client_key),
    CONSTRAINT questionnaire_questions_ordinal_unique
        UNIQUE (project_id, questionnaire_version_id, ordinal),
    CONSTRAINT questionnaire_questions_payload_nonempty CHECK (octet_length(encrypted_payload) > 0)
);

CREATE TRIGGER questionnaire_questions_immutable
BEFORE UPDATE OR DELETE ON questionnaire_questions
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE questionnaire_options (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    question_id uuid NOT NULL,
    client_key uuid NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    encrypted_payload bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT questionnaire_options_question_fk
        FOREIGN KEY (project_id, question_id)
        REFERENCES questionnaire_questions (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_options_project_question_id_unique
        UNIQUE (project_id, question_id, id),
    CONSTRAINT questionnaire_options_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT questionnaire_options_client_unique
        UNIQUE (project_id, question_id, client_key),
    CONSTRAINT questionnaire_options_ordinal_unique
        UNIQUE (project_id, question_id, ordinal),
    CONSTRAINT questionnaire_options_payload_nonempty CHECK (octet_length(encrypted_payload) > 0)
);

CREATE TRIGGER questionnaire_options_immutable
BEFORE UPDATE OR DELETE ON questionnaire_options
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE questionnaire_submissions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    questionnaire_version_id uuid NOT NULL,
    submitted_by_identity_id uuid NOT NULL,
    client_submission_id uuid NOT NULL,
    encrypted_payload bytea NOT NULL,
    signature bytea NOT NULL,
    state text NOT NULL DEFAULT 'submitted'
        CHECK (state IN ('submitted', 'superseded', 'withdrawn')),
    submitted_at timestamptz NOT NULL DEFAULT now(),
    supersedes_submission_id uuid,
    CONSTRAINT questionnaire_submissions_version_fk
        FOREIGN KEY (project_id, questionnaire_version_id)
        REFERENCES questionnaire_versions (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_submissions_submitter_fk
        FOREIGN KEY (project_id, submitted_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_submissions_project_version_id_unique
        UNIQUE (project_id, questionnaire_version_id, id),
    CONSTRAINT questionnaire_submissions_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT questionnaire_submissions_client_unique
        UNIQUE (project_id, submitted_by_identity_id, client_submission_id),
    CONSTRAINT questionnaire_submissions_supersedes_fk
        FOREIGN KEY (project_id, supersedes_submission_id)
        REFERENCES questionnaire_submissions (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_submissions_payload_nonempty CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT questionnaire_submissions_signature_nonempty CHECK (octet_length(signature) > 0),
    CONSTRAINT questionnaire_submissions_not_self_superseding
        CHECK (supersedes_submission_id IS NULL OR supersedes_submission_id <> id)
);

CREATE INDEX questionnaire_submissions_version_time_idx
    ON questionnaire_submissions (project_id, questionnaire_version_id, submitted_at DESC);
CREATE TRIGGER questionnaire_submissions_immutable
BEFORE UPDATE OR DELETE ON questionnaire_submissions
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE questionnaire_answers (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    questionnaire_version_id uuid NOT NULL,
    submission_id uuid NOT NULL,
    question_id uuid NOT NULL,
    selected_option_id uuid,
    encrypted_payload bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT questionnaire_answers_submission_fk
        FOREIGN KEY (project_id, questionnaire_version_id, submission_id)
        REFERENCES questionnaire_submissions (project_id, questionnaire_version_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_answers_question_fk
        FOREIGN KEY (project_id, questionnaire_version_id, question_id)
        REFERENCES questionnaire_questions (project_id, questionnaire_version_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_answers_option_fk
        FOREIGN KEY (project_id, question_id, selected_option_id)
        REFERENCES questionnaire_options (project_id, question_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT questionnaire_answers_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT questionnaire_answers_one_per_question
        UNIQUE (project_id, submission_id, question_id),
    CONSTRAINT questionnaire_answers_payload_nonempty CHECK (octet_length(encrypted_payload) > 0)
);

CREATE TRIGGER questionnaire_answers_immutable
BEFORE UPDATE OR DELETE ON questionnaire_answers
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE file_blobs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    storage_provider text NOT NULL
        CHECK (storage_provider IN ('s3', 'gcs', 'azure', 'filesystem', 'other')),
    storage_key text NOT NULL,
    ciphertext_size bigint NOT NULL CHECK (ciphertext_size >= 0),
    ciphertext_hash bytea NOT NULL,
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    encrypted_metadata bytea NOT NULL,
    upload_state text NOT NULL DEFAULT 'pending'
        CHECK (upload_state IN ('pending', 'available', 'quarantined', 'deleted')),
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    available_at timestamptz,
    deleted_at timestamptz,
    CONSTRAINT file_blobs_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT file_blobs_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT file_blobs_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT file_blobs_storage_key_unique UNIQUE (storage_provider, storage_key),
    CONSTRAINT file_blobs_hash_nonempty CHECK (octet_length(ciphertext_hash) >= 16),
    CONSTRAINT file_blobs_metadata_nonempty CHECK (octet_length(encrypted_metadata) > 0),
    CONSTRAINT file_blobs_available_state
        CHECK ((upload_state = 'available') = (available_at IS NOT NULL)),
    CONSTRAINT file_blobs_deleted_state
        CHECK ((upload_state = 'deleted') = (deleted_at IS NOT NULL))
);

CREATE INDEX file_blobs_project_state_idx
    ON file_blobs (project_id, upload_state, created_at);

CREATE TABLE file_links (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    blob_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    link_kind text NOT NULL
        CHECK (link_kind IN ('attachment', 'cover', 'inline', 'export', 'other')),
    encrypted_metadata bytea NOT NULL,
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    removed_at timestamptz,
    CONSTRAINT file_links_blob_fk
        FOREIGN KEY (project_id, blob_id) REFERENCES file_blobs (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT file_links_resource_fk
        FOREIGN KEY (project_id, resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT file_links_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT file_links_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT file_links_metadata_nonempty CHECK (octet_length(encrypted_metadata) > 0)
);

CREATE UNIQUE INDEX file_links_active_unique
    ON file_links (project_id, blob_id, resource_node_id, link_kind)
    WHERE removed_at IS NULL;
CREATE INDEX file_links_resource_active_idx
    ON file_links (project_id, resource_node_id, created_at)
    WHERE removed_at IS NULL;
