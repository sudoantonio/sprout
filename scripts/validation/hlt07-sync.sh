#!/usr/bin/env bash
set -euo pipefail

# HLT-07: storage-level idempotency, version-conflict, projection rollback,
# reconstruction, and stale-replay oracle. The behavior fixture is deliberately
# transactional and rolls itself back, so this gate must execute it directly
# instead of assuming another process left its rows behind.

: "${DATABASE_URL:?DATABASE_URL is required}"

behavior_sql="${HLT07_BEHAVIOR_SQL:-db/tests/verify_behavior.sql}"
if [[ ! -f "${behavior_sql}" ]]; then
  echo "HLT-07 behavior oracle not found: ${behavior_sql}" >&2
  exit 1
fi
psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 --file "${behavior_sql}" >/dev/null

echo "HLT-07 sync projection/idempotency behavior verified"
