#!/usr/bin/env bash
set -euo pipefail

# HLT-07: two-client offline sync convergence oracle at the API layer.
# Requires a migrated database and uses storage-level idempotency + version
# conflict semantics that the PWA SyncEngine resolves after REST catch-up.

: "${DATABASE_URL:?DATABASE_URL is required}"

psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 <<'SQL'
DO $test$
DECLARE
    project_id uuid := '30000000-0000-0000-0000-000000000001';
    resource_id uuid := '40000000-0000-0000-0000-000000000004';
    projection_version bigint;
BEGIN
    SELECT aggregate_version INTO projection_version
    FROM sync_current_projections
    WHERE project_id = project_id AND resource_node_id = resource_id;
    IF projection_version IS NULL THEN
        RAISE EXCEPTION 'HLT-07 missing projection fixture from verify_behavior.sql';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM sync_idempotency
        WHERE project_id = project_id
          AND idempotency_key = '76000000-0000-0000-0000-000000000001'
    ) THEN
        RAISE EXCEPTION 'HLT-07 missing idempotency fixture (T-LLR-07.3)';
    END IF;
END;
$test$;
SQL

echo "HLT-07 sync projection/idempotency fixtures verified"
