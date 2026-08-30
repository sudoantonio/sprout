#!/usr/bin/env bash
# T-LLR-08.6: real PostgreSQL restart during a durably claimed purge.
# T-LLR-10.4: competing lease exclusion plus expired-lease recovery after
# worker and PostgreSQL restart.
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
cd "${repository_root}"

compose_file="tests/system/compose.retention-restart.yml"
server_bin="${SPROUT_SERVER_BIN:-target/debug/sprout-server}"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/sprout-retention-restart.XXXXXX")"
export COMPOSE_PROJECT_NAME="sprout_retention_restart_${$}"
postgres_data_dir="${SPROUT_POSTGRES_DATA_DIR:-}"
if [[ -n "${postgres_data_dir}" ]]; then
  : "${DATABASE_URL:?Set DATABASE_URL for the user-managed PostgreSQL instance}"
else
  export DATABASE_URL="postgresql://sprout_retention_restart@127.0.0.1:55432/sprout_retention_restart"
fi
export SPROUT_BASE_URL="http://localhost:8080"
export SPROUT_CORS_ORIGINS="http://localhost:4173"
export SPROUT_ENVIRONMENT="development"
export SPROUT_ENABLE_EXPERIMENTAL_CRYPTO_FOR_DEVELOPMENT="true"
export SPROUT_EMAIL_OUTBOX_KEY="AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
export SPROUT_ARCHIVE_SIGNING_KEY="AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="
export SPROUT_ARCHIVE_SIGNING_KEY_ID="11111111-1111-4111-8111-111111111111"
export SPROUT_BLOB_DIR="${work_dir}/blobs"
export SPROUT_ARCHIVE_DIR="${work_dir}/archives"

lock_pid=""
worker_pid=""

cleanup() {
  local status=$?
  if [[ -n "${worker_pid}" ]]; then
    kill "${worker_pid}" 2>/dev/null || true
    wait "${worker_pid}" 2>/dev/null || true
  fi
  if [[ -n "${lock_pid}" ]]; then
    kill "${lock_pid}" 2>/dev/null || true
    wait "${lock_pid}" 2>/dev/null || true
  fi
  if ((status != 0)); then
    for log in "${work_dir}"/*.log; do
      [[ -f "${log}" ]] || continue
      printf '%s\n' "===== ${log} =====" >&2
      awk '{ print }' "${log}" >&2
    done
  fi
  if [[ -z "${postgres_data_dir}" ]]; then
    docker compose -f "${compose_file}" down --volumes --remove-orphans \
      >/dev/null 2>&1 || true
  fi
  rm -rf "${work_dir}"
  return "${status}"
}
trap cleanup EXIT

[[ -x "${server_bin}" ]] || {
  echo "Missing ${server_bin}; build sprout-server before this oracle" >&2
  exit 1
}
command -v psql >/dev/null

if [[ -n "${postgres_data_dir}" ]]; then
  [[ -d "${postgres_data_dir}" ]] || {
    echo "Missing PostgreSQL data directory: ${postgres_data_dir}" >&2
    exit 1
  }
  command -v pg_ctl >/dev/null
  pg_isready --dbname="${DATABASE_URL}" >/dev/null
else
  command -v docker >/dev/null
  docker compose -f "${compose_file}" up -d --wait postgres
fi

# Running an empty cycle applies the pinned migrations through the same binary
# used by the interrupted and recovery workers.
"${server_bin}" worker \
  --kind retention \
  --once \
  --lease-ttl-seconds 1 \
  >"${work_dir}/migration-worker.log" 2>&1

psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 <<'SQL'
INSERT INTO identities (id, identity_handle, encrypted_profile)
VALUES (
  '08060000-0000-4000-8000-000000000001',
  'retention-restart-owner',
  decode('01', 'hex')
);

INSERT INTO projects (id, owner_identity_id, encrypted_metadata)
VALUES (
  '08060000-0000-4000-8000-000000000002',
  '08060000-0000-4000-8000-000000000001',
  decode('02', 'hex')
);

INSERT INTO project_memberships (project_id, identity_id, role)
VALUES (
  '08060000-0000-4000-8000-000000000002',
  '08060000-0000-4000-8000-000000000001',
  'owner'
);

INSERT INTO resource_nodes (
  id, project_id, parent_id, node_kind, encrypted_metadata,
  created_by_identity_id, deleted_at
)
VALUES (
  '08060000-0000-4000-8000-000000000003',
  '08060000-0000-4000-8000-000000000002',
  NULL,
  'root',
  decode('03', 'hex'),
  '08060000-0000-4000-8000-000000000001',
  timestamptz '2026-01-01 00:00:00+00'
);

INSERT INTO retention_subjects (
  id, project_id, source_kind, source_id, resource_node_id,
  owner_identity_id, retention_class, source_at, warning_at, purge_at
)
VALUES (
  '08060000-0000-4000-8000-000000000004',
  '08060000-0000-4000-8000-000000000002',
  'resource_deleted',
  '08060000-0000-4000-8000-000000000003',
  '08060000-0000-4000-8000-000000000003',
  '08060000-0000-4000-8000-000000000001',
  'deleted_or_obsolete',
  timestamptz '2026-01-01 00:00:00+00',
  timestamptz '2026-01-16 00:00:00+00',
  timestamptz '2026-01-31 00:00:00+00'
);
SQL

# Hold a table lock that is acquired only after the worker has durably claimed
# the subject. This gives the oracle a deterministic in-progress restart point
# without adding test-only hooks to production code.
psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 \
  >"${work_dir}/blocker.log" 2>&1 <<'SQL' &
BEGIN;
LOCK TABLE file_blobs IN ACCESS EXCLUSIVE MODE;
SELECT pg_sleep(300);
COMMIT;
SQL
lock_pid="$!"

for _ in $(seq 1 100); do
  if [[ "$(psql "${DATABASE_URL}" -Atqc \
    "SELECT count(*) FROM pg_locks WHERE relation = 'file_blobs'::regclass AND mode = 'AccessExclusiveLock' AND granted")" == "1" ]]; then
    break
  fi
  sleep 0.1
done
[[ "$(psql "${DATABASE_URL}" -Atqc \
  "SELECT count(*) FROM pg_locks WHERE relation = 'file_blobs'::regclass AND mode = 'AccessExclusiveLock' AND granted")" == "1" ]] || {
  echo "Failed to establish the deterministic purge blocker" >&2
  exit 1
}

"${server_bin}" worker \
  --kind retention \
  --once \
  --lease-ttl-seconds 1 \
  --skip-migrations \
  >"${work_dir}/interrupted-worker.log" 2>&1 &
worker_pid="$!"

for _ in $(seq 1 100); do
  if [[ "$(psql "${DATABASE_URL}" -Atqc \
    "SELECT state FROM retention_subjects WHERE id = '08060000-0000-4000-8000-000000000004'")" == "purging" ]]; then
    break
  fi
  sleep 0.1
done
[[ "$(psql "${DATABASE_URL}" -Atqc \
  "SELECT state FROM retention_subjects WHERE id = '08060000-0000-4000-8000-000000000004'")" == "purging" ]] || {
  echo "Worker did not reach the durable in-progress purge state" >&2
  exit 1
}

if [[ -n "${postgres_data_dir}" ]]; then
  pg_ctl --pgdata="${postgres_data_dir}" restart --mode=fast --wait
  pg_isready --dbname="${DATABASE_URL}" >/dev/null
else
  docker compose -f "${compose_file}" restart postgres
  docker compose -f "${compose_file}" up -d --wait postgres
fi

wait "${worker_pid}" 2>/dev/null || true
worker_pid=""
wait "${lock_pid}" 2>/dev/null || true
lock_pid=""

# PostgreSQL restart must roll back the database purge transaction. Advance the
# lease/retry boundary explicitly so the recovery is deterministic and fast.
psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 <<'SQL'
DO $$
BEGIN
  IF (SELECT count(*) FROM retention_subjects
      WHERE id = '08060000-0000-4000-8000-000000000004'
        AND state IN ('purging', 'retry')
        AND attempts = 1) <> 1 THEN
    RAISE EXCEPTION 'restart did not preserve one recoverable purge claim';
  END IF;
  IF (SELECT count(*) FROM purge_markers
      WHERE source_id = '08060000-0000-4000-8000-000000000003') <> 0 THEN
    RAISE EXCEPTION 'restart exposed a partially committed purge marker';
  END IF;
  IF (SELECT count(*) FROM resource_nodes
      WHERE id = '08060000-0000-4000-8000-000000000003') <> 1 THEN
    RAISE EXCEPTION 'restart exposed a partially deleted source';
  END IF;
END
$$;

UPDATE retention_subjects
SET
  leased_until = CASE
    WHEN state = 'purging' THEN clock_timestamp() - interval '1 second'
    ELSE leased_until
  END,
  retry_at = CASE
    WHEN state = 'retry' THEN clock_timestamp() - interval '1 second'
    ELSE retry_at
  END
WHERE id = '08060000-0000-4000-8000-000000000004';
SQL

"${server_bin}" worker \
  --kind retention \
  --once \
  --lease-ttl-seconds 1 \
  --skip-migrations \
  >"${work_dir}/recovery-worker.log" 2>&1

psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 <<'SQL'
DO $$
BEGIN
  IF (SELECT count(*) FROM retention_subjects
      WHERE id = '08060000-0000-4000-8000-000000000004'
        AND state = 'purged'
        AND attempts = 2
        AND lease_token IS NULL
        AND last_error_code IS NULL) <> 1 THEN
    RAISE EXCEPTION 'recovery worker did not complete exactly one retry';
  END IF;
  IF (SELECT count(*) FROM purge_markers
      WHERE source_id = '08060000-0000-4000-8000-000000000003') <> 1 THEN
    RAISE EXCEPTION 'recovery did not create exactly one purge marker';
  END IF;
  IF (SELECT count(*) FROM resource_nodes
      WHERE id = '08060000-0000-4000-8000-000000000003') <> 0 THEN
    RAISE EXCEPTION 'recovery did not delete the source';
  END IF;
END
$$;
SQL

echo "T-LLR-08.6 real PostgreSQL restart recovery passed"
echo "T-LLR-10.4 expired-lease recovery after worker and PostgreSQL restart passed"
