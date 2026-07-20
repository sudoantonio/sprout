-- HLR-02 keeps the domain-specific permission tables while adding the
-- materialized scope and root lineage needed for origin-aware revocation.

ALTER TABLE topic_permissions
    DROP CONSTRAINT topic_permissions_origin_shape,
    DROP CONSTRAINT topic_permissions_grant_origin_check;
ALTER TABLE task_list_permissions
    DROP CONSTRAINT task_list_permissions_origin_shape,
    DROP CONSTRAINT task_list_permissions_grant_origin_check;
ALTER TABLE task_permissions
    DROP CONSTRAINT task_permissions_origin_shape,
    DROP CONSTRAINT task_permissions_grant_origin_check;

DROP INDEX topic_permissions_active_grant_unique;
DROP INDEX task_list_permissions_active_grant_unique;
DROP INDEX task_permissions_active_grant_unique;

ALTER TABLE topic_permissions
    ADD COLUMN access_scope text NOT NULL DEFAULT 'full'
        CHECK (access_scope IN ('full', 'container_only')),
    ADD COLUMN root_grant_id uuid;
ALTER TABLE task_list_permissions
    ADD COLUMN access_scope text NOT NULL DEFAULT 'full'
        CHECK (access_scope IN ('full', 'container_only')),
    ADD COLUMN root_grant_id uuid;
ALTER TABLE task_permissions
    ADD COLUMN access_scope text NOT NULL DEFAULT 'full'
        CHECK (access_scope IN ('full', 'container_only')),
    ADD COLUMN root_grant_id uuid;

UPDATE topic_permissions
SET root_grant_id = CASE
    WHEN grant_origin = 'materialized' THEN grant_origin_id
    ELSE id
END;
UPDATE task_list_permissions
SET root_grant_id = CASE
    WHEN grant_origin = 'materialized' THEN grant_origin_id
    ELSE id
END;
UPDATE task_permissions
SET root_grant_id = CASE
    WHEN grant_origin = 'materialized' THEN grant_origin_id
    ELSE id
END;

ALTER TABLE topic_permissions
    ALTER COLUMN root_grant_id SET NOT NULL,
    ADD CONSTRAINT topic_permissions_grant_origin_check
        CHECK (grant_origin IN (
            'explicit', 'assignment', 'project_role', 'inherited',
            'preset', 'materialized', 'system'
        )),
    ADD CONSTRAINT topic_permissions_lineage_shape CHECK (
        (
            id = root_grant_id
            AND grant_origin <> 'materialized'
            AND (
                (grant_origin = 'explicit' AND grant_origin_id IS NULL)
                OR (grant_origin <> 'explicit' AND grant_origin_id IS NOT NULL)
            )
        )
        OR (
            id <> root_grant_id
            AND grant_origin = 'materialized'
            AND grant_origin_id = root_grant_id
        )
    );
ALTER TABLE task_list_permissions
    ALTER COLUMN root_grant_id SET NOT NULL,
    ADD CONSTRAINT task_list_permissions_grant_origin_check
        CHECK (grant_origin IN (
            'explicit', 'assignment', 'project_role', 'inherited',
            'preset', 'materialized', 'system'
        )),
    ADD CONSTRAINT task_list_permissions_lineage_shape CHECK (
        (
            id = root_grant_id
            AND grant_origin <> 'materialized'
            AND (
                (grant_origin = 'explicit' AND grant_origin_id IS NULL)
                OR (grant_origin <> 'explicit' AND grant_origin_id IS NOT NULL)
            )
        )
        OR (
            id <> root_grant_id
            AND grant_origin = 'materialized'
            AND grant_origin_id = root_grant_id
        )
    );
ALTER TABLE task_permissions
    ALTER COLUMN root_grant_id SET NOT NULL,
    ADD CONSTRAINT task_permissions_grant_origin_check
        CHECK (grant_origin IN (
            'explicit', 'assignment', 'project_role', 'inherited',
            'preset', 'materialized', 'system'
        )),
    ADD CONSTRAINT task_permissions_lineage_shape CHECK (
        (
            id = root_grant_id
            AND grant_origin <> 'materialized'
            AND (
                (grant_origin = 'explicit' AND grant_origin_id IS NULL)
                OR (grant_origin <> 'explicit' AND grant_origin_id IS NOT NULL)
            )
        )
        OR (
            id <> root_grant_id
            AND grant_origin = 'materialized'
            AND grant_origin_id = root_grant_id
        )
    );

CREATE UNIQUE INDEX topic_permissions_active_lineage_unique
    ON topic_permissions (project_id, topic_id, member_identity_id, root_grant_id)
    WHERE revoked_at IS NULL;
CREATE UNIQUE INDEX task_list_permissions_active_lineage_unique
    ON task_list_permissions (
        project_id, task_list_id, member_identity_id, root_grant_id
    )
    WHERE revoked_at IS NULL;
CREATE UNIQUE INDEX task_permissions_active_lineage_unique
    ON task_permissions (project_id, task_id, member_identity_id, root_grant_id)
    WHERE revoked_at IS NULL;

ALTER TABLE task_assignments
    ADD COLUMN permission_root_grant_id uuid;
UPDATE task_assignments
SET permission_root_grant_id = gen_random_uuid()
WHERE permission_root_grant_id IS NULL;
ALTER TABLE task_assignments
    ALTER COLUMN permission_root_grant_id SET NOT NULL;
CREATE UNIQUE INDEX task_assignments_permission_root_unique
    ON task_assignments (project_id, permission_root_grant_id);

-- Envelope provenance is recorded but signatures remain opaque. This migration
-- validates shape and key ownership only; it does not verify signatures.
ALTER TABLE resource_key_envelopes
    ADD COLUMN envelope_version smallint NOT NULL DEFAULT 1
        CHECK (envelope_version = 1),
    ADD COLUMN created_by_device_id uuid,
    ADD COLUMN created_by_device_key_version integer;

UPDATE resource_key_envelopes envelope
SET
    created_by_device_id = epoch.created_by_device_id,
    created_by_device_key_version = epoch.created_by_device_key_version
FROM resource_epochs epoch
WHERE epoch.project_id = envelope.project_id
  AND epoch.resource_node_id = envelope.resource_node_id
  AND epoch.epoch = envelope.epoch;

ALTER TABLE resource_key_envelopes
    ALTER COLUMN created_by_device_id SET NOT NULL,
    ALTER COLUMN created_by_device_key_version SET NOT NULL,
    ADD CONSTRAINT resource_key_envelopes_creator_device_key_fk
        FOREIGN KEY (
            created_by_identity_id,
            created_by_device_id,
            created_by_device_key_version
        ) REFERENCES device_keys (identity_id, device_id, key_version)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    DROP CONSTRAINT resource_key_envelopes_key_nonempty,
    DROP CONSTRAINT resource_key_envelopes_signature_nonempty,
    ADD CONSTRAINT resource_key_envelopes_key_length
        CHECK (octet_length(encrypted_key) >= 16),
    ADD CONSTRAINT resource_key_envelopes_signature_length
        CHECK (octet_length(sender_signature) = 64);

CREATE OR REPLACE VIEW sprout_private.domain_permission_rows AS
SELECT
    permission.id,
    permission.project_id,
    topic.resource_node_id,
    'topic'::text AS node_kind,
    permission.topic_id AS target_id,
    permission.member_identity_id,
    permission.access_level,
    permission.access_scope,
    permission.visibility,
    permission.grant_origin,
    permission.grant_origin_id,
    permission.root_grant_id,
    permission.granted_by_identity_id,
    permission.created_at,
    permission.revoked_at
FROM topic_permissions permission
JOIN topics topic
  ON topic.project_id = permission.project_id
 AND topic.id = permission.topic_id
UNION ALL
SELECT
    permission.id,
    permission.project_id,
    task_list.resource_node_id,
    'task_list'::text,
    permission.task_list_id,
    permission.member_identity_id,
    permission.access_level,
    permission.access_scope,
    permission.visibility,
    permission.grant_origin,
    permission.grant_origin_id,
    permission.root_grant_id,
    permission.granted_by_identity_id,
    permission.created_at,
    permission.revoked_at
FROM task_list_permissions permission
JOIN task_lists task_list
  ON task_list.project_id = permission.project_id
 AND task_list.id = permission.task_list_id
UNION ALL
SELECT
    permission.id,
    permission.project_id,
    task.resource_node_id,
    'task'::text,
    permission.task_id,
    permission.member_identity_id,
    permission.access_level,
    permission.access_scope,
    permission.visibility,
    permission.grant_origin,
    permission.grant_origin_id,
    permission.root_grant_id,
    permission.granted_by_identity_id,
    permission.created_at,
    permission.revoked_at
FROM task_permissions permission
JOIN tasks task
  ON task.project_id = permission.project_id
 AND task.id = permission.task_id;

REVOKE ALL ON sprout_private.domain_permission_rows FROM PUBLIC;

CREATE OR REPLACE FUNCTION sprout_private.expected_hierarchical_permission_rows(
    candidate_project_id uuid,
    candidate_resource_node_id uuid,
    candidate_access_scope text
)
RETURNS TABLE (
    project_id uuid,
    resource_node_id uuid,
    node_kind text,
    target_id uuid,
    access_scope text,
    is_root boolean
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    WITH affected AS (
        SELECT
            ancestor.id AS resource_node_id,
            'container_only'::text AS access_scope
        FROM resource_closure closure
        JOIN resource_nodes ancestor
          ON ancestor.project_id = closure.project_id
         AND ancestor.id = closure.ancestor_id
        WHERE closure.project_id = candidate_project_id
          AND closure.descendant_id = candidate_resource_node_id
          AND ancestor.node_kind IN ('topic', 'task_list', 'task')
          AND ancestor.deleted_at IS NULL

        UNION ALL

        SELECT
            descendant.id,
            'full'::text
        FROM resource_closure closure
        JOIN resource_nodes descendant
          ON descendant.project_id = closure.project_id
         AND descendant.id = closure.descendant_id
        WHERE candidate_access_scope = 'full'
          AND closure.project_id = candidate_project_id
          AND closure.ancestor_id = candidate_resource_node_id
          AND descendant.node_kind IN ('topic', 'task_list', 'task')
          AND descendant.deleted_at IS NULL
    ),
    collapsed AS (
        SELECT
            affected.resource_node_id,
            CASE
                WHEN bool_or(affected.access_scope = 'full') THEN 'full'
                ELSE 'container_only'
            END AS access_scope
        FROM affected
        GROUP BY affected.resource_node_id
    )
    SELECT
        node.project_id,
        node.id,
        node.node_kind,
        COALESCE(topic.id, task_list.id, task.id),
        CASE
            WHEN node.id = candidate_resource_node_id
                THEN candidate_access_scope
            ELSE collapsed.access_scope
        END,
        node.id = candidate_resource_node_id
    FROM collapsed
    JOIN resource_nodes node
      ON node.project_id = candidate_project_id
     AND node.id = collapsed.resource_node_id
    LEFT JOIN topics topic
      ON topic.project_id = node.project_id
     AND topic.resource_node_id = node.id
     AND topic.deleted_at IS NULL
    LEFT JOIN task_lists task_list
      ON task_list.project_id = node.project_id
     AND task_list.resource_node_id = node.id
     AND task_list.deleted_at IS NULL
    LEFT JOIN tasks task
      ON task.project_id = node.project_id
     AND task.resource_node_id = node.id
     AND task.deleted_at IS NULL
    WHERE candidate_access_scope IN ('full', 'container_only')
      AND COALESCE(topic.id, task_list.id, task.id) IS NOT NULL
$$;

REVOKE ALL ON FUNCTION sprout_private.expected_hierarchical_permission_rows(
    uuid, uuid, text
) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.expected_hierarchical_permission_rows(
    uuid, uuid, text
) TO PUBLIC;

CREATE OR REPLACE FUNCTION sprout_private.grant_hierarchical_permission(
    candidate_project_id uuid,
    candidate_resource_node_id uuid,
    candidate_member_identity_id uuid,
    candidate_access_level text,
    candidate_access_scope text,
    candidate_visibility text,
    candidate_root_grant_id uuid,
    candidate_granted_by_identity_id uuid,
    candidate_origin text DEFAULT 'explicit',
    candidate_origin_id uuid DEFAULT NULL
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    permission_row record;
    materialized_id uuid;
    row_origin text;
    row_origin_id uuid;
BEGIN
    IF candidate_access_level NOT IN ('view', 'comment', 'edit', 'manage')
       OR candidate_access_scope NOT IN ('full', 'container_only')
       OR candidate_visibility NOT IN ('private', 'restricted', 'project', 'inherited')
       OR candidate_origin NOT IN ('explicit', 'assignment')
       OR (candidate_origin = 'explicit' AND candidate_origin_id IS NOT NULL)
       OR (candidate_origin = 'assignment' AND candidate_origin_id IS NULL)
    THEN
        RAISE EXCEPTION 'invalid hierarchical permission input'
            USING ERRCODE = '23514';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM project_memberships membership
        WHERE membership.project_id = candidate_project_id
          AND membership.identity_id = candidate_member_identity_id
          AND membership.state = 'active'
    ) OR NOT EXISTS (
        SELECT 1
        FROM project_memberships membership
        WHERE membership.project_id = candidate_project_id
          AND membership.identity_id = candidate_granted_by_identity_id
          AND membership.state = 'active'
    ) THEN
        RAISE EXCEPTION 'grant recipient and grantor must be active project members'
            USING ERRCODE = '23503';
    END IF;

    PERFORM pg_advisory_xact_lock(
        hashtextextended(
            candidate_project_id::text || ':' ||
            candidate_member_identity_id::text,
            11
        )
    );

    IF EXISTS (
        SELECT 1
        FROM sprout_private.domain_permission_rows permission
        WHERE permission.project_id = candidate_project_id
          AND (
              permission.id = candidate_root_grant_id
              OR permission.root_grant_id = candidate_root_grant_id
          )
    ) THEN
        RAISE EXCEPTION 'permission root grant identifier already exists'
            USING ERRCODE = '23505';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM sprout_private.expected_hierarchical_permission_rows(
            candidate_project_id,
            candidate_resource_node_id,
            candidate_access_scope
        )
        WHERE is_root
    ) THEN
        RAISE EXCEPTION 'permission target is not an active domain resource'
            USING ERRCODE = '23503';
    END IF;

    FOR permission_row IN
        SELECT *
        FROM sprout_private.expected_hierarchical_permission_rows(
            candidate_project_id,
            candidate_resource_node_id,
            candidate_access_scope
        )
    LOOP
        materialized_id := CASE
            WHEN permission_row.is_root THEN candidate_root_grant_id
            ELSE gen_random_uuid()
        END;
        row_origin := CASE
            WHEN permission_row.is_root THEN candidate_origin
            ELSE 'materialized'
        END;
        row_origin_id := CASE
            WHEN permission_row.is_root THEN candidate_origin_id
            ELSE candidate_root_grant_id
        END;

        CASE permission_row.node_kind
            WHEN 'topic' THEN
                INSERT INTO topic_permissions (
                    id, project_id, topic_id, member_identity_id,
                    access_level, access_scope, visibility, grant_origin,
                    grant_origin_id, root_grant_id, granted_by_identity_id
                )
                VALUES (
                    materialized_id,
                    candidate_project_id,
                    permission_row.target_id,
                    candidate_member_identity_id,
                    candidate_access_level,
                    permission_row.access_scope,
                    candidate_visibility,
                    row_origin,
                    row_origin_id,
                    candidate_root_grant_id,
                    candidate_granted_by_identity_id
                );
            WHEN 'task_list' THEN
                INSERT INTO task_list_permissions (
                    id, project_id, task_list_id, member_identity_id,
                    access_level, access_scope, visibility, grant_origin,
                    grant_origin_id, root_grant_id, granted_by_identity_id
                )
                VALUES (
                    materialized_id,
                    candidate_project_id,
                    permission_row.target_id,
                    candidate_member_identity_id,
                    candidate_access_level,
                    permission_row.access_scope,
                    candidate_visibility,
                    row_origin,
                    row_origin_id,
                    candidate_root_grant_id,
                    candidate_granted_by_identity_id
                );
            WHEN 'task' THEN
                INSERT INTO task_permissions (
                    id, project_id, task_id, member_identity_id,
                    access_level, access_scope, visibility, grant_origin,
                    grant_origin_id, root_grant_id, granted_by_identity_id
                )
                VALUES (
                    materialized_id,
                    candidate_project_id,
                    permission_row.target_id,
                    candidate_member_identity_id,
                    candidate_access_level,
                    permission_row.access_scope,
                    candidate_visibility,
                    row_origin,
                    row_origin_id,
                    candidate_root_grant_id,
                    candidate_granted_by_identity_id
                );
        END CASE;
    END LOOP;
END;
$$;

REVOKE ALL ON FUNCTION sprout_private.grant_hierarchical_permission(
    uuid, uuid, uuid, text, text, text, uuid, uuid, text, uuid
) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.grant_hierarchical_permission(
    uuid, uuid, uuid, text, text, text, uuid, uuid, text, uuid
) TO PUBLIC;

CREATE OR REPLACE FUNCTION sprout_private.materialize_new_domain_descendant()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    source_permission record;
BEGIN
    FOR source_permission IN
        SELECT DISTINCT ON (
            permission.member_identity_id,
            permission.root_grant_id
        )
            permission.*
        FROM resource_closure closure
        JOIN sprout_private.domain_permission_rows permission
          ON permission.project_id = closure.project_id
         AND permission.resource_node_id = closure.ancestor_id
         AND permission.revoked_at IS NULL
         AND permission.access_scope = 'full'
        WHERE closure.project_id = NEW.project_id
          AND closure.descendant_id = NEW.resource_node_id
          AND closure.depth > 0
        ORDER BY
            permission.member_identity_id,
            permission.root_grant_id,
            closure.depth
    LOOP
        CASE TG_ARGV[0]
            WHEN 'topic' THEN
                INSERT INTO topic_permissions (
                    project_id, topic_id, member_identity_id,
                    access_level, access_scope, visibility, grant_origin,
                    grant_origin_id, root_grant_id, granted_by_identity_id
                )
                VALUES (
                    NEW.project_id, NEW.id, source_permission.member_identity_id,
                    source_permission.access_level, 'full',
                    source_permission.visibility, 'materialized',
                    source_permission.root_grant_id,
                    source_permission.root_grant_id,
                    source_permission.granted_by_identity_id
                )
                ON CONFLICT DO NOTHING;
            WHEN 'task_list' THEN
                INSERT INTO task_list_permissions (
                    project_id, task_list_id, member_identity_id,
                    access_level, access_scope, visibility, grant_origin,
                    grant_origin_id, root_grant_id, granted_by_identity_id
                )
                VALUES (
                    NEW.project_id, NEW.id, source_permission.member_identity_id,
                    source_permission.access_level, 'full',
                    source_permission.visibility, 'materialized',
                    source_permission.root_grant_id,
                    source_permission.root_grant_id,
                    source_permission.granted_by_identity_id
                )
                ON CONFLICT DO NOTHING;
            WHEN 'task' THEN
                INSERT INTO task_permissions (
                    project_id, task_id, member_identity_id,
                    access_level, access_scope, visibility, grant_origin,
                    grant_origin_id, root_grant_id, granted_by_identity_id
                )
                VALUES (
                    NEW.project_id, NEW.id, source_permission.member_identity_id,
                    source_permission.access_level, 'full',
                    source_permission.visibility, 'materialized',
                    source_permission.root_grant_id,
                    source_permission.root_grant_id,
                    source_permission.granted_by_identity_id
                )
                ON CONFLICT DO NOTHING;
        END CASE;
    END LOOP;
    RETURN NEW;
END;
$$;

CREATE TRIGGER topics_materialize_inherited_permissions
AFTER INSERT ON topics
FOR EACH ROW EXECUTE FUNCTION sprout_private.materialize_new_domain_descendant('topic');
CREATE TRIGGER task_lists_materialize_inherited_permissions
AFTER INSERT ON task_lists
FOR EACH ROW EXECUTE FUNCTION sprout_private.materialize_new_domain_descendant('task_list');
CREATE TRIGGER tasks_materialize_inherited_permissions
AFTER INSERT ON tasks
FOR EACH ROW EXECUTE FUNCTION sprout_private.materialize_new_domain_descendant('task');

CREATE OR REPLACE FUNCTION sprout_private.permission_lineage_resources(
    candidate_project_id uuid,
    candidate_root_grant_id uuid,
    candidate_member_identity_id uuid
)
RETURNS TABLE (
    resource_node_id uuid,
    access_scope text
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT permission.resource_node_id, permission.access_scope
    FROM sprout_private.domain_permission_rows permission
    WHERE permission.project_id = candidate_project_id
      AND permission.root_grant_id = candidate_root_grant_id
      AND permission.member_identity_id = candidate_member_identity_id
      AND permission.revoked_at IS NULL
$$;

REVOKE ALL ON FUNCTION sprout_private.permission_lineage_resources(
    uuid, uuid, uuid
) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.permission_lineage_resources(
    uuid, uuid, uuid
) TO PUBLIC;

CREATE OR REPLACE FUNCTION sprout_private.revoke_hierarchical_permission(
    candidate_project_id uuid,
    candidate_root_grant_id uuid,
    candidate_member_identity_id uuid,
    candidate_revoked_by_identity_id uuid,
    assignment_notification_payload bytea DEFAULT NULL
)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    root_resource_node_id uuid;
    root_node_kind text;
    revoked_count bigint := 0;
    changed_count bigint;
    assigned_tasks_remain boolean;
BEGIN
    PERFORM pg_advisory_xact_lock(
        hashtextextended(
            candidate_project_id::text || ':' ||
            candidate_member_identity_id::text,
            11
        )
    );

    SELECT permission.resource_node_id, permission.node_kind
    INTO root_resource_node_id, root_node_kind
    FROM sprout_private.domain_permission_rows permission
    WHERE permission.project_id = candidate_project_id
      AND permission.id = candidate_root_grant_id
      AND permission.root_grant_id = candidate_root_grant_id
      AND permission.member_identity_id = candidate_member_identity_id
      AND permission.revoked_at IS NULL;

    IF root_resource_node_id IS NULL THEN
        RAISE EXCEPTION 'active root permission grant was not found'
            USING ERRCODE = 'P0002';
    END IF;

    assigned_tasks_remain := root_node_kind = 'task_list' AND EXISTS (
        SELECT 1
        FROM resource_closure closure
        JOIN tasks task
          ON task.project_id = closure.project_id
         AND task.resource_node_id = closure.descendant_id
         AND task.deleted_at IS NULL
        JOIN task_assignments assignment
          ON assignment.project_id = task.project_id
         AND assignment.task_id = task.id
         AND assignment.assignee_identity_id = candidate_member_identity_id
         AND assignment.revoked_at IS NULL
        WHERE closure.project_id = candidate_project_id
          AND closure.ancestor_id = root_resource_node_id
    );

    IF assigned_tasks_remain AND (
        assignment_notification_payload IS NULL
        OR octet_length(assignment_notification_payload) = 0
    ) THEN
        RAISE EXCEPTION 'assigned tasks require an encrypted admin notification'
            USING ERRCODE = '23514';
    END IF;

    UPDATE topic_permissions
    SET revoked_at = clock_timestamp()
    WHERE project_id = candidate_project_id
      AND root_grant_id = candidate_root_grant_id
      AND member_identity_id = candidate_member_identity_id
      AND revoked_at IS NULL;
    GET DIAGNOSTICS changed_count = ROW_COUNT;
    revoked_count := revoked_count + changed_count;

    UPDATE task_list_permissions
    SET revoked_at = clock_timestamp()
    WHERE project_id = candidate_project_id
      AND root_grant_id = candidate_root_grant_id
      AND member_identity_id = candidate_member_identity_id
      AND revoked_at IS NULL;
    GET DIAGNOSTICS changed_count = ROW_COUNT;
    revoked_count := revoked_count + changed_count;

    UPDATE task_permissions
    SET revoked_at = clock_timestamp()
    WHERE project_id = candidate_project_id
      AND root_grant_id = candidate_root_grant_id
      AND member_identity_id = candidate_member_identity_id
      AND revoked_at IS NULL;
    GET DIAGNOSTICS changed_count = ROW_COUNT;
    revoked_count := revoked_count + changed_count;

    IF assigned_tasks_remain THEN
        INSERT INTO notifications (
            project_id,
            recipient_identity_id,
            notification_kind,
            delivery_channel,
            encrypted_payload
        )
        SELECT
            candidate_project_id,
            membership.identity_id,
            'assigned_task_list_access_removed',
            'in_app',
            assignment_notification_payload
        FROM project_memberships membership
        WHERE membership.project_id = candidate_project_id
          AND membership.state = 'active'
          AND membership.role IN ('owner', 'admin');
    END IF;

    RETURN revoked_count;
END;
$$;

REVOKE ALL ON FUNCTION sprout_private.revoke_hierarchical_permission(
    uuid, uuid, uuid, uuid, bytea
) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.revoke_hierarchical_permission(
    uuid, uuid, uuid, uuid, bytea
) TO PUBLIC;

CREATE OR REPLACE FUNCTION sprout_private.effective_domain_permission(
    candidate_project_id uuid,
    candidate_resource_node_id uuid,
    candidate_identity_id uuid
)
RETURNS TABLE (
    access_level text,
    access_scope text,
    root_grant_id uuid,
    source_resource_node_id uuid
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT
        permission.access_level,
        permission.access_scope,
        permission.root_grant_id,
        permission.resource_node_id
    FROM sprout_private.domain_permission_rows permission
    JOIN resource_closure closure
      ON closure.project_id = permission.project_id
     AND closure.ancestor_id = permission.resource_node_id
     AND closure.descendant_id = candidate_resource_node_id
    WHERE permission.project_id = candidate_project_id
      AND permission.member_identity_id = candidate_identity_id
      AND permission.revoked_at IS NULL
      AND (
          (
              permission.resource_node_id = candidate_resource_node_id
              AND permission.access_scope IN ('full', 'container_only')
          )
          OR (
              permission.access_scope = 'full'
              AND closure.depth > 0
          )
      )
    ORDER BY
        CASE permission.access_scope
            WHEN 'full' THEN 2
            ELSE 1
        END DESC,
        CASE permission.access_level
            WHEN 'manage' THEN 4
            WHEN 'edit' THEN 3
            WHEN 'comment' THEN 2
            ELSE 1
        END DESC,
        closure.depth ASC
    LIMIT 1
$$;

REVOKE ALL ON FUNCTION sprout_private.effective_domain_permission(
    uuid, uuid, uuid
) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.effective_domain_permission(
    uuid, uuid, uuid
) TO PUBLIC;
