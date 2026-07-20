CREATE TABLE projects (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_identity_id uuid NOT NULL,
    encrypted_metadata bytea NOT NULL,
    key_epoch integer NOT NULL DEFAULT 1 CHECK (key_epoch > 0),
    status text NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'archived', 'deleted')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    archived_at timestamptz,
    deleted_at timestamptz,
    CONSTRAINT projects_owner_fk
        FOREIGN KEY (owner_identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT projects_encrypted_metadata_nonempty
        CHECK (octet_length(encrypted_metadata) > 0),
    CONSTRAINT projects_archived_state
        CHECK ((status = 'archived') = (archived_at IS NOT NULL)),
    CONSTRAINT projects_deleted_state
        CHECK ((status = 'deleted') = (deleted_at IS NOT NULL))
);

CREATE INDEX projects_owner_idx ON projects (owner_identity_id, created_at);

CREATE TRIGGER projects_touch_updated_at
BEFORE UPDATE ON projects
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE TABLE project_memberships (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    identity_id uuid NOT NULL,
    role text NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'guest')),
    state text NOT NULL DEFAULT 'active'
        CHECK (state IN ('active', 'suspended', 'left')),
    encrypted_preferences bytea,
    joined_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    suspended_at timestamptz,
    left_at timestamptz,
    CONSTRAINT project_memberships_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_memberships_identity_fk
        FOREIGN KEY (identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_memberships_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT project_memberships_project_identity_unique UNIQUE (project_id, identity_id),
    CONSTRAINT project_memberships_preferences_nonempty
        CHECK (encrypted_preferences IS NULL OR octet_length(encrypted_preferences) > 0),
    CONSTRAINT project_memberships_state_timestamps
        CHECK (
            (state = 'active' AND suspended_at IS NULL AND left_at IS NULL)
            OR (state = 'suspended' AND suspended_at IS NOT NULL AND left_at IS NULL)
            OR (state = 'left' AND left_at IS NOT NULL)
        )
);

CREATE UNIQUE INDEX project_memberships_one_owner_idx
    ON project_memberships (project_id)
    WHERE role = 'owner' AND state = 'active';
CREATE INDEX project_memberships_identity_active_idx
    ON project_memberships (identity_id, project_id)
    WHERE state = 'active';

CREATE TRIGGER project_memberships_touch_updated_at
BEFORE UPDATE ON project_memberships
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();

CREATE OR REPLACE FUNCTION sprout_private.is_project_member(candidate_project_id uuid)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM project_memberships membership
        WHERE membership.project_id = candidate_project_id
          AND membership.identity_id = sprout_private.current_identity_id()
          AND membership.state = 'active'
    )
$$;

REVOKE ALL ON FUNCTION sprout_private.is_project_member(uuid) FROM PUBLIC;

CREATE TABLE project_invitations (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    invited_by_identity_id uuid NOT NULL,
    accepted_by_identity_id uuid,
    invitee_lookup_hash bytea NOT NULL,
    token_hash bytea NOT NULL,
    encrypted_payload bytea NOT NULL,
    role text NOT NULL CHECK (role IN ('admin', 'member', 'guest')),
    state text NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'accepted', 'revoked', 'expired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    accepted_at timestamptz,
    revoked_at timestamptz,
    CONSTRAINT project_invitations_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_invitations_inviter_membership_fk
        FOREIGN KEY (project_id, invited_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_invitations_acceptor_identity_fk
        FOREIGN KEY (accepted_by_identity_id) REFERENCES identities (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT project_invitations_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT project_invitations_lookup_hash_nonempty
        CHECK (octet_length(invitee_lookup_hash) >= 16),
    CONSTRAINT project_invitations_token_hash_nonempty
        CHECK (octet_length(token_hash) >= 32),
    CONSTRAINT project_invitations_payload_nonempty
        CHECK (octet_length(encrypted_payload) > 0),
    CONSTRAINT project_invitations_expiry_after_creation CHECK (expires_at > created_at),
    CONSTRAINT project_invitations_acceptance_state
        CHECK (
            (state = 'accepted' AND accepted_by_identity_id IS NOT NULL AND accepted_at IS NOT NULL)
            OR (state <> 'accepted' AND accepted_by_identity_id IS NULL AND accepted_at IS NULL)
        ),
    CONSTRAINT project_invitations_revocation_state
        CHECK ((state = 'revoked') = (revoked_at IS NOT NULL))
);

CREATE UNIQUE INDEX project_invitations_token_hash_unique
    ON project_invitations (token_hash);
CREATE UNIQUE INDEX project_invitations_pending_invitee_unique
    ON project_invitations (project_id, invitee_lookup_hash)
    WHERE state = 'pending';
CREATE INDEX project_invitations_pending_expiry_idx
    ON project_invitations (expires_at)
    WHERE state = 'pending';

CREATE TABLE resource_nodes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    parent_id uuid,
    node_kind text NOT NULL
        CHECK (node_kind IN ('root', 'topic', 'task_list', 'task', 'file', 'other')),
    encrypted_metadata bytea NOT NULL,
    created_by_identity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    CONSTRAINT resource_nodes_project_fk
        FOREIGN KEY (project_id) REFERENCES projects (id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_nodes_project_id_unique UNIQUE (project_id, id),
    CONSTRAINT resource_nodes_parent_fk
        FOREIGN KEY (project_id, parent_id) REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
        DEFERRABLE INITIALLY IMMEDIATE,
    CONSTRAINT resource_nodes_creator_membership_fk
        FOREIGN KEY (project_id, created_by_identity_id)
        REFERENCES project_memberships (project_id, identity_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_nodes_metadata_nonempty
        CHECK (octet_length(encrypted_metadata) > 0),
    CONSTRAINT resource_nodes_root_parent
        CHECK ((node_kind = 'root' AND parent_id IS NULL)
            OR (node_kind <> 'root' AND parent_id IS NOT NULL)),
    CONSTRAINT resource_nodes_not_self_parent CHECK (parent_id IS NULL OR parent_id <> id)
);

CREATE UNIQUE INDEX resource_nodes_one_root_per_project_idx
    ON resource_nodes (project_id)
    WHERE node_kind = 'root' AND deleted_at IS NULL;
CREATE INDEX resource_nodes_parent_idx
    ON resource_nodes (project_id, parent_id)
    WHERE deleted_at IS NULL;

CREATE TABLE resource_closure (
    project_id uuid NOT NULL,
    ancestor_id uuid NOT NULL,
    descendant_id uuid NOT NULL,
    depth integer NOT NULL CHECK (depth >= 0),
    PRIMARY KEY (project_id, ancestor_id, descendant_id),
    CONSTRAINT resource_closure_ancestor_fk
        FOREIGN KEY (project_id, ancestor_id) REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_closure_descendant_fk
        FOREIGN KEY (project_id, descendant_id) REFERENCES resource_nodes (project_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT resource_closure_depth_shape
        CHECK ((ancestor_id = descendant_id AND depth = 0)
            OR (ancestor_id <> descendant_id AND depth > 0))
);

CREATE INDEX resource_closure_descendant_idx
    ON resource_closure (project_id, descendant_id, depth);

CREATE OR REPLACE FUNCTION sprout_private.validate_resource_parent()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    parent_project_id uuid;
BEGIN
    IF TG_OP = 'UPDATE' AND NEW.project_id <> OLD.project_id THEN
        RAISE EXCEPTION 'resource nodes cannot move between projects'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.parent_id IS NULL THEN
        RETURN NEW;
    END IF;

    PERFORM pg_advisory_xact_lock(hashtextextended(NEW.project_id::text, 3));

    SELECT project_id INTO parent_project_id
    FROM resource_nodes
    WHERE id = NEW.parent_id;

    IF parent_project_id IS NULL OR parent_project_id <> NEW.project_id THEN
        RAISE EXCEPTION 'resource parent must exist in the same project'
            USING ERRCODE = '23503';
    END IF;

    IF TG_OP = 'UPDATE'
       AND NEW.parent_id IS DISTINCT FROM OLD.parent_id
       AND EXISTS (
           SELECT 1
           FROM resource_closure
           WHERE project_id = NEW.project_id
             AND ancestor_id = NEW.id
             AND descendant_id = NEW.parent_id
       )
    THEN
        RAISE EXCEPTION 'resource hierarchy cycle detected'
            USING ERRCODE = '23514';
    END IF;

    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION sprout_private.rebuild_resource_closure()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    affected_project_id uuid;
BEGIN
    affected_project_id := NEW.project_id;

    DELETE FROM resource_closure
    WHERE project_id = affected_project_id;

    WITH RECURSIVE paths (ancestor_id, descendant_id, depth) AS (
        SELECT node.id, node.id, 0
        FROM resource_nodes node
        WHERE node.project_id = affected_project_id
        UNION ALL
        SELECT paths.ancestor_id, child.id, paths.depth + 1
        FROM paths
        JOIN resource_nodes child
          ON child.project_id = affected_project_id
         AND child.parent_id = paths.descendant_id
    )
    INSERT INTO resource_closure (project_id, ancestor_id, descendant_id, depth)
    SELECT affected_project_id, ancestor_id, descendant_id, depth
    FROM paths;

    RETURN NEW;
END;
$$;

CREATE TRIGGER resource_nodes_validate_parent
BEFORE INSERT OR UPDATE OF project_id, parent_id ON resource_nodes
FOR EACH ROW EXECUTE FUNCTION sprout_private.validate_resource_parent();

CREATE TRIGGER resource_nodes_rebuild_closure
AFTER INSERT OR UPDATE OF parent_id ON resource_nodes
FOR EACH ROW EXECUTE FUNCTION sprout_private.rebuild_resource_closure();

CREATE TRIGGER resource_nodes_touch_updated_at
BEFORE UPDATE ON resource_nodes
FOR EACH ROW EXECUTE FUNCTION sprout_private.touch_updated_at();
