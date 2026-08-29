-- Extend encrypted Info documents to the project root used by the Generali
-- overview. A project document has both topic_id and task_list_id set to NULL
-- and is governed by the project's root resource node and key epoch.

ALTER TABLE info_documents
    DROP CONSTRAINT info_documents_one_container,
    ADD CONSTRAINT info_documents_one_container
        CHECK (
            (topic_id IS NOT NULL)::integer
            + (task_list_id IS NOT NULL)::integer <= 1
        );

CREATE UNIQUE INDEX info_documents_project_root_unique
    ON info_documents (project_id)
    WHERE topic_id IS NULL
      AND task_list_id IS NULL
      AND parent_document_id IS NULL
      AND deleted_at IS NULL;

CREATE INDEX info_documents_project_active_idx
    ON info_documents (project_id, created_at, id)
    WHERE topic_id IS NULL
      AND task_list_id IS NULL
      AND deleted_at IS NULL;

CREATE OR REPLACE FUNCTION sprout_private.validate_info_document_container()
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
    ELSIF NEW.task_list_id IS NOT NULL THEN
        SELECT resource_node_id INTO expected_resource_node_id
        FROM task_lists
        WHERE project_id = NEW.project_id
          AND id = NEW.task_list_id
          AND deleted_at IS NULL;
    ELSE
        SELECT id INTO expected_resource_node_id
        FROM resource_nodes
        WHERE project_id = NEW.project_id
          AND node_kind = 'root'
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

REVOKE ALL ON FUNCTION sprout_private.validate_info_document_container() FROM PUBLIC;
