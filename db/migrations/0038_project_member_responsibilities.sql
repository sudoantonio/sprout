ALTER TABLE project_memberships
ADD COLUMN responsibilities text;

ALTER TABLE project_memberships
ADD CONSTRAINT project_memberships_responsibilities_length
CHECK (
    responsibilities IS NULL
    OR char_length(responsibilities) <= 500
);

DROP FUNCTION sprout_private.project_member_directory(uuid);

CREATE FUNCTION sprout_private.project_member_directory(target_project_id uuid)
RETURNS TABLE (
    identity_id uuid,
    identity_handle text,
    email text,
    role text,
    membership_state text,
    joined_at timestamptz,
    responsibilities text
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
    SELECT
        membership.identity_id,
        directory.identity_handle,
        identity_email.normalized_email,
        membership.role,
        membership.state,
        membership.joined_at,
        membership.responsibilities
    FROM project_memberships membership
    JOIN identity_directory directory
      ON directory.identity_id = membership.identity_id
     AND directory.identity_status = 'active'
    LEFT JOIN identity_emails identity_email
      ON identity_email.identity_id = membership.identity_id
    WHERE membership.project_id = target_project_id
      AND membership.state = 'active'
      AND EXISTS (
          SELECT 1
          FROM project_memberships requester
          WHERE requester.project_id = target_project_id
            AND requester.identity_id = sprout_private.current_identity_id()
            AND requester.state = 'active'
      )
    ORDER BY
        CASE membership.role
            WHEN 'owner' THEN 0
            WHEN 'admin' THEN 1
            WHEN 'member' THEN 2
            ELSE 3
        END,
        directory.identity_handle,
        membership.identity_id
$$;

REVOKE ALL ON FUNCTION sprout_private.project_member_directory(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION sprout_private.project_member_directory(uuid) TO PUBLIC;
