CREATE OR REPLACE FUNCTION sprout_private.suggest_project_participants(
    target_project_id uuid,
    handle_prefix text,
    result_limit integer
)
RETURNS TABLE (
    identity_id uuid,
    identity_handle text,
    shared_project_count bigint,
    most_recent_shared_project_at timestamptz
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
SET row_security = off
AS $$
    WITH requester AS (
        SELECT membership.project_id
        FROM project_memberships membership
        WHERE membership.identity_id = sprout_private.current_identity_id()
          AND membership.state = 'active'
    ),
    guard AS (
        SELECT 1
        FROM requester
        WHERE requester.project_id = target_project_id
    )
    SELECT
        candidate_identity.identity_id,
        candidate_identity.identity_handle,
        count(DISTINCT candidate_membership.project_id),
        max(shared_project.updated_at)
    FROM guard
    CROSS JOIN requester
    JOIN projects shared_project
      ON shared_project.id = requester.project_id
     AND shared_project.deleted_at IS NULL
    JOIN project_memberships candidate_membership
      ON candidate_membership.project_id = requester.project_id
     AND candidate_membership.state = 'active'
     AND candidate_membership.identity_id <> sprout_private.current_identity_id()
    JOIN identity_directory candidate_identity
      ON candidate_identity.identity_id = candidate_membership.identity_id
     AND candidate_identity.identity_status = 'active'
    WHERE result_limit BETWEEN 1 AND 50
      AND length(handle_prefix) <= 128
      AND left(candidate_identity.identity_handle, char_length(handle_prefix)) = handle_prefix
      AND NOT EXISTS (
          SELECT 1
          FROM project_memberships target_membership
          WHERE target_membership.project_id = target_project_id
            AND target_membership.identity_id = candidate_identity.identity_id
            AND target_membership.state = 'active'
      )
    GROUP BY candidate_identity.identity_id, candidate_identity.identity_handle
    ORDER BY
        count(DISTINCT candidate_membership.project_id) DESC,
        max(shared_project.updated_at) DESC,
        candidate_identity.identity_handle,
        candidate_identity.identity_id
    LIMIT result_limit
$$;
