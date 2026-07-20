CREATE TABLE topics (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    visibility text NOT NULL DEFAULT 'inherited'
        CHECK (visibility IN ('private', 'restricted', 'project', 'inherited')),
    encrypted_payload bytea NOT NULL,
    payload_version bigint NOT NULL DEFAULT 1 CHECK (payload_version > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    CONSTRAINT topics_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT topics_resource_fk
        FOREIGN KEY (project_id, resource_node_id) REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT topics_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT topics_resource_unique UNIQUE (project_id, resource_node_id),
    CONSTRAINT topics_payload_nonempty CHECK (octet_length(encrypted_payload) > 0)
);

CREATE INDEX topics_project_active_idx ON topics (project_id, created_at)
    WHERE deleted_at IS NULL;
CREATE TRIGGER topics_touch_updated_at
BEFORE UPDATE ON topics
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE task_lists (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    topic_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    visibility text NOT NULL DEFAULT 'inherited'
        CHECK (visibility IN ('private', 'restricted', 'project', 'inherited')),
    encrypted_payload bytea NOT NULL,
    payload_version bigint NOT NULL DEFAULT 1 CHECK (payload_version > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    CONSTRAINT task_lists_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_lists_topic_fk
        FOREIGN KEY (project_id, topic_id) REFERENCES topics (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_lists_resource_fk
        FOREIGN KEY (project_id, resource_node_id) REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_lists_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT task_lists_resource_unique UNIQUE (project_id, resource_node_id),
    CONSTRAINT task_lists_payload_nonempty CHECK (octet_length(encrypted_payload) > 0)
);

CREATE INDEX task_lists_topic_active_idx ON task_lists (project_id, topic_id, created_at)
    WHERE deleted_at IS NULL;
CREATE TRIGGER task_lists_touch_updated_at
BEFORE UPDATE ON task_lists
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE tasks (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    task_list_id uuid NOT NULL,
    resource_node_id uuid NOT NULL,
    visibility text NOT NULL DEFAULT 'inherited'
        CHECK (visibility IN ('private', 'restricted', 'project', 'inherited')),
    encrypted_payload bytea NOT NULL,
    payload_version bigint NOT NULL DEFAULT 1 CHECK (payload_version > 0),
    state text NOT NULL DEFAULT 'open'
        CHECK (state IN ('open', 'completed', 'cancelled', 'archived')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    CONSTRAINT tasks_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT tasks_task_list_fk
        FOREIGN KEY (project_id, task_list_id) REFERENCES task_lists (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT tasks_resource_fk
        FOREIGN KEY (project_id, resource_node_id) REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT tasks_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT tasks_resource_unique UNIQUE (project_id, resource_node_id),
    CONSTRAINT tasks_payload_nonempty CHECK (octet_length(encrypted_payload) > 0)
);

CREATE INDEX tasks_list_active_idx ON tasks (project_id, task_list_id, created_at)
    WHERE deleted_at IS NULL;
CREATE INDEX tasks_project_state_idx ON tasks (project_id, state, updated_at);
CREATE TRIGGER tasks_touch_updated_at
BEFORE UPDATE ON tasks
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE OR REPLACE FUNCTION sprout_private.validate_domain_resource_kind()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    actual_kind text;
BEGIN
    SELECT node_kind INTO actual_kind
    FROM resource_nodes
    WHERE project_id = NEW.project_id
      AND id = NEW.resource_node_id;

    IF actual_kind IS DISTINCT FROM TG_ARGV[0] THEN
        RAISE EXCEPTION '% must reference a % resource node', TG_TABLE_NAME, TG_ARGV[0]
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER topics_validate_resource_kind
BEFORE INSERT OR UPDATE OF project_id, resource_node_id ON topics
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_domain_resource_kind('topic');
CREATE TRIGGER task_lists_validate_resource_kind
BEFORE INSERT OR UPDATE OF project_id, resource_node_id ON task_lists
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_domain_resource_kind('task_list');
CREATE TRIGGER tasks_validate_resource_kind
BEFORE INSERT OR UPDATE OF project_id, resource_node_id ON tasks
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_domain_resource_kind('task');

CREATE TABLE topic_permissions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    topic_id uuid NOT NULL,
    member_identity_id uuid NOT NULL,
    access_level text NOT NULL CHECK (access_level IN ('view', 'comment', 'edit', 'manage')),
    visibility text NOT NULL CHECK (visibility IN ('private', 'restricted', 'project', 'inherited')),
    grant_origin text NOT NULL
        CHECK (grant_origin IN ('explicit', 'project_role', 'inherited', 'preset', 'materialized', 'system')),
    grant_origin_id uuid,
    granted_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    CONSTRAINT topic_permissions_topic_fk
        FOREIGN KEY (project_id, topic_id) REFERENCES topics (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT topic_permissions_member_fk
        FOREIGN KEY (project_id, member_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT topic_permissions_grantor_fk
        FOREIGN KEY (project_id, granted_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT topic_permissions_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT topic_permissions_origin_shape
        CHECK ((grant_origin = 'explicit' AND grant_origin_id IS NULL)
            OR (grant_origin <> 'explicit' AND grant_origin_id IS NOT NULL))
);

CREATE UNIQUE INDEX topic_permissions_active_grant_unique
    ON topic_permissions (
        project_id, topic_id, member_identity_id, grant_origin,
        COALESCE(grant_origin_id, '00000000-0000-0000-0000-000000000000'::uuid)
    )
    WHERE revoked_at IS NULL;
CREATE INDEX topic_permissions_member_active_idx
    ON topic_permissions (project_id, member_identity_id, topic_id)
    WHERE revoked_at IS NULL;

CREATE TABLE task_list_permissions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    task_list_id uuid NOT NULL,
    member_identity_id uuid NOT NULL,
    access_level text NOT NULL CHECK (access_level IN ('view', 'comment', 'edit', 'manage')),
    visibility text NOT NULL CHECK (visibility IN ('private', 'restricted', 'project', 'inherited')),
    grant_origin text NOT NULL
        CHECK (grant_origin IN ('explicit', 'project_role', 'inherited', 'preset', 'materialized', 'system')),
    grant_origin_id uuid,
    granted_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    CONSTRAINT task_list_permissions_task_list_fk
        FOREIGN KEY (project_id, task_list_id) REFERENCES task_lists (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_list_permissions_member_fk
        FOREIGN KEY (project_id, member_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_list_permissions_grantor_fk
        FOREIGN KEY (project_id, granted_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_list_permissions_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT task_list_permissions_origin_shape
        CHECK ((grant_origin = 'explicit' AND grant_origin_id IS NULL)
            OR (grant_origin <> 'explicit' AND grant_origin_id IS NOT NULL))
);

CREATE UNIQUE INDEX task_list_permissions_active_grant_unique
    ON task_list_permissions (
        project_id, task_list_id, member_identity_id, grant_origin,
        COALESCE(grant_origin_id, '00000000-0000-0000-0000-000000000000'::uuid)
    )
    WHERE revoked_at IS NULL;
CREATE INDEX task_list_permissions_member_active_idx
    ON task_list_permissions (project_id, member_identity_id, task_list_id)
    WHERE revoked_at IS NULL;

CREATE TABLE task_permissions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    member_identity_id uuid NOT NULL,
    access_level text NOT NULL CHECK (access_level IN ('view', 'comment', 'edit', 'manage')),
    visibility text NOT NULL CHECK (visibility IN ('private', 'restricted', 'project', 'inherited')),
    grant_origin text NOT NULL
        CHECK (grant_origin IN ('explicit', 'project_role', 'inherited', 'preset', 'materialized', 'system')),
    grant_origin_id uuid,
    granted_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    CONSTRAINT task_permissions_task_fk
        FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_permissions_member_fk
        FOREIGN KEY (project_id, member_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_permissions_grantor_fk
        FOREIGN KEY (project_id, granted_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_permissions_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT task_permissions_origin_shape
        CHECK ((grant_origin = 'explicit' AND grant_origin_id IS NULL)
            OR (grant_origin <> 'explicit' AND grant_origin_id IS NOT NULL))
);

CREATE UNIQUE INDEX task_permissions_active_grant_unique
    ON task_permissions (
        project_id, task_id, member_identity_id, grant_origin,
        COALESCE(grant_origin_id, '00000000-0000-0000-0000-000000000000'::uuid)
    )
    WHERE revoked_at IS NULL;
CREATE INDEX task_permissions_member_active_idx
    ON task_permissions (project_id, member_identity_id, task_id)
    WHERE revoked_at IS NULL;

CREATE TABLE encrypted_domain_snapshots (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    aggregate_kind text NOT NULL
        CHECK (aggregate_kind IN (
            'project', 'resource', 'topic', 'task_list', 'task',
            'preset', 'questionnaire', 'submission', 'file'
        )),
    aggregate_id uuid NOT NULL,
    version bigint NOT NULL CHECK (version > 0),
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    ciphertext bytea NOT NULL,
    nonce bytea NOT NULL,
    content_hash bytea NOT NULL,
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT encrypted_domain_snapshots_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT encrypted_domain_snapshots_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT encrypted_domain_snapshots_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT encrypted_domain_snapshots_version_unique
        UNIQUE (project_id, aggregate_kind, aggregate_id, version),
    CONSTRAINT encrypted_domain_snapshots_ciphertext_nonempty CHECK (octet_length(ciphertext) > 0),
    CONSTRAINT encrypted_domain_snapshots_nonce_nonempty CHECK (octet_length(nonce) > 0),
    CONSTRAINT encrypted_domain_snapshots_hash_nonempty CHECK (octet_length(content_hash) >= 16)
);

CREATE INDEX encrypted_domain_snapshots_latest_idx
    ON encrypted_domain_snapshots (project_id, aggregate_kind, aggregate_id, version DESC);
CREATE TRIGGER encrypted_domain_snapshots_immutable
BEFORE UPDATE OR DELETE ON encrypted_domain_snapshots
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();

CREATE TABLE task_assignments (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    assignee_identity_id uuid NOT NULL,
    assigned_by_identity_id uuid NOT NULL,
    encrypted_payload bytea NOT NULL,
    assigned_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    CONSTRAINT task_assignments_task_fk
        FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_assignments_assignee_fk
        FOREIGN KEY (project_id, assignee_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_assignments_assigner_fk
        FOREIGN KEY (project_id, assigned_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_assignments_project_task_id_unique UNIQUE (project_id, task_id, id),
    CONSTRAINT task_assignments_payload_nonempty CHECK (octet_length(encrypted_payload) > 0)
);

CREATE UNIQUE INDEX task_assignments_one_active_assignee_idx
    ON task_assignments (project_id, task_id, assignee_identity_id)
    WHERE revoked_at IS NULL;
CREATE INDEX task_assignments_assignee_active_idx
    ON task_assignments (project_id, assignee_identity_id, assigned_at)
    WHERE revoked_at IS NULL;

CREATE TABLE task_recurrences (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    client_recurrence_id uuid NOT NULL,
    encrypted_rule bytea NOT NULL,
    rule_hash bytea NOT NULL,
    starts_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    retired_at timestamptz,
    CONSTRAINT task_recurrences_task_fk
        FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_recurrences_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT task_recurrences_client_unique UNIQUE (project_id, task_id, client_recurrence_id),
    CONSTRAINT task_recurrences_rule_nonempty CHECK (octet_length(encrypted_rule) > 0),
    CONSTRAINT task_recurrences_hash_nonempty CHECK (octet_length(rule_hash) >= 16)
);

CREATE UNIQUE INDEX task_recurrences_one_active_rule_idx
    ON task_recurrences (project_id, task_id)
    WHERE retired_at IS NULL;

CREATE TABLE task_completions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    assignment_id uuid,
    assignee_identity_id uuid NOT NULL,
    recorded_by_identity_id uuid NOT NULL,
    occurrence_key uuid NOT NULL,
    encrypted_payload bytea NOT NULL,
    completed_at timestamptz NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    supersedes_completion_id uuid,
    CONSTRAINT task_completions_task_fk
        FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_completions_assignment_fk
        FOREIGN KEY (project_id, task_id, assignment_id)
        REFERENCES task_assignments (project_id, task_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_completions_assignee_fk
        FOREIGN KEY (project_id, assignee_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_completions_recorder_fk
        FOREIGN KEY (project_id, recorded_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_completions_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT task_completions_supersedes_fk
        FOREIGN KEY (project_id, supersedes_completion_id)
        REFERENCES task_completions (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT task_completions_occurrence_unique
        UNIQUE (project_id, task_id, assignee_identity_id, occurrence_key),
    CONSTRAINT task_completions_payload_nonempty CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT task_completions_not_self_superseding
        CHECK (supersedes_completion_id IS NULL OR supersedes_completion_id <> id)
);

CREATE INDEX task_completions_task_time_idx
    ON task_completions (project_id, task_id, completed_at DESC);
CREATE TRIGGER task_completions_immutable
BEFORE UPDATE OR DELETE ON task_completions
FOR EACH ROW EXECUTE FUNCTION sprout_private.reject_historical_mutation();
