-- Encrypted overview documents for topics and task lists. The server stores
-- hierarchy and concurrency metadata only; titles, text, URLs, file labels,
-- and ordered blocks remain inside the opaque encrypted payload.

CREATE TABLE info_documents (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    topic_id uuid,
    task_list_id uuid,
    parent_document_id uuid,
    resource_node_id uuid NOT NULL,
    encrypted_payload bytea NOT NULL,
    key_epoch integer NOT NULL CHECK (key_epoch > 0),
    payload_version bigint NOT NULL DEFAULT 1 CHECK (payload_version > 0),
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    CONSTRAINT info_documents_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT info_documents_topic_fk
        FOREIGN KEY (project_id, topic_id) REFERENCES topics (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT info_documents_task_list_fk
        FOREIGN KEY (project_id, task_list_id) REFERENCES task_lists (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT info_documents_resource_fk
        FOREIGN KEY (project_id, resource_node_id)
        REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT info_documents_epoch_fk
        FOREIGN KEY (project_id, resource_node_id, key_epoch)
        REFERENCES resource_epochs (project_id, resource_node_id, epoch)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT info_documents_creator_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT info_documents_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT info_documents_parent_fk
        FOREIGN KEY (project_id, parent_document_id)
        REFERENCES info_documents (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT info_documents_one_container
        CHECK ((topic_id IS NOT NULL)::integer + (task_list_id IS NOT NULL)::integer = 1),
    CONSTRAINT info_documents_not_self_parent
        CHECK (parent_document_id IS NULL OR parent_document_id <> id),
    CONSTRAINT info_documents_payload_nonempty
        CHECK (octet_length(encrypted_payload) > 0)
);

CREATE UNIQUE INDEX info_documents_topic_root_unique
    ON info_documents (project_id, topic_id)
    WHERE topic_id IS NOT NULL
      AND parent_document_id IS NULL
      AND deleted_at IS NULL;
CREATE UNIQUE INDEX info_documents_task_list_root_unique
    ON info_documents (project_id, task_list_id)
    WHERE task_list_id IS NOT NULL
      AND parent_document_id IS NULL
      AND deleted_at IS NULL;
CREATE INDEX info_documents_topic_active_idx
    ON info_documents (project_id, topic_id, created_at, id)
    WHERE topic_id IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX info_documents_task_list_active_idx
    ON info_documents (project_id, task_list_id, created_at, id)
    WHERE task_list_id IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX info_documents_parent_active_idx
    ON info_documents (project_id, parent_document_id, created_at, id)
    WHERE parent_document_id IS NOT NULL AND deleted_at IS NULL;

CREATE FUNCTION sprout_private.validate_info_document_container()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    expected_resource_node_id uuid;
    parent_record info_documents%ROWTYPE;
BEGIN
    IF TG_OP = 'UPDATE' AND (
        NEW.project_id IS DISTINCT FROM OLD.project_id
        OR NEW.topic_id IS DISTINCT FROM OLD.topic_id
        OR NEW.task_list_id IS DISTINCT FROM OLD.task_list_id
        OR NEW.parent_document_id IS DISTINCT FROM OLD.parent_document_id
        OR NEW.resource_node_id IS DISTINCT FROM OLD.resource_node_id
    ) THEN
        RAISE EXCEPTION 'info documents cannot move between containers'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.topic_id IS NOT NULL THEN
        SELECT resource_node_id INTO expected_resource_node_id
        FROM topics
        WHERE project_id = NEW.project_id
          AND id = NEW.topic_id
          AND deleted_at IS NULL;
    ELSE
        SELECT resource_node_id INTO expected_resource_node_id
        FROM task_lists
        WHERE project_id = NEW.project_id
          AND id = NEW.task_list_id
          AND deleted_at IS NULL;
    END IF;

    IF expected_resource_node_id IS NULL
       OR expected_resource_node_id <> NEW.resource_node_id THEN
        RAISE EXCEPTION 'info document must use its container resource node'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.parent_document_id IS NOT NULL THEN
        SELECT * INTO parent_record
        FROM info_documents
        WHERE project_id = NEW.project_id
          AND id = NEW.parent_document_id
          AND deleted_at IS NULL;

        IF NOT FOUND
           OR parent_record.topic_id IS DISTINCT FROM NEW.topic_id
           OR parent_record.task_list_id IS DISTINCT FROM NEW.task_list_id
           OR parent_record.resource_node_id <> NEW.resource_node_id THEN
            RAISE EXCEPTION 'nested info document must share its parent container'
                USING ERRCODE = '23514';
        END IF;
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER info_documents_validate_container
BEFORE INSERT OR UPDATE ON info_documents
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_info_document_container();
CREATE TRIGGER info_documents_touch_updated_at
BEFORE UPDATE ON info_documents
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

ALTER TABLE info_documents ENABLE ROW LEVEL SECURITY;
ALTER TABLE info_documents FORCE ROW LEVEL SECURITY;
CREATE POLICY project_isolation ON info_documents
    USING (sprout_private.is_project_member(project_id))
    WITH CHECK (sprout_private.is_project_member(project_id));

-- Physical deletion remains restricted to the retention pipeline. Container
-- rows are soft-deleted by normal API operations, so these cleanup triggers
-- run only when an authorized retention purge removes the parent itself.
CREATE TRIGGER info_documents_retention_delete
BEFORE DELETE ON info_documents
FOR EACH ROW EXECUTE FUNCTION sprout_private.retention_only_delete();

CREATE FUNCTION sprout_private.delete_task_list_info_documents()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    DELETE FROM info_documents
    WHERE project_id = OLD.project_id AND task_list_id = OLD.id;
    RETURN OLD;
END;
$$;

CREATE FUNCTION sprout_private.delete_topic_info_documents()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    DELETE FROM info_documents
    WHERE project_id = OLD.project_id AND topic_id = OLD.id;
    RETURN OLD;
END;
$$;

CREATE TRIGGER task_lists_delete_info_documents
BEFORE DELETE ON task_lists
FOR EACH ROW EXECUTE FUNCTION sprout_private.delete_task_list_info_documents();
CREATE TRIGGER topics_delete_info_documents
BEFORE DELETE ON topics
FOR EACH ROW EXECUTE FUNCTION sprout_private.delete_topic_info_documents();

REVOKE ALL ON FUNCTION sprout_private.validate_info_document_container() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.delete_task_list_info_documents() FROM PUBLIC;
REVOKE ALL ON FUNCTION sprout_private.delete_topic_info_documents() FROM PUBLIC;
