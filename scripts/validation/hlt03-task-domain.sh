#!/usr/bin/env bash
set -euo pipefail

: "${DATABASE_URL:?DATABASE_URL is required}"

# HLT-03: one preset version contains all three pretask kinds, independently
# selected values become immutable snapshots, then completion, copy and
# recurrence preserve provenance.
behavior_sql="${HLT03_BEHAVIOR_SQL:-db/tests/verify_behavior.sql}"
if [[ ! -f "${behavior_sql}" ]]; then
  echo "HLT-03 behavior oracle not found: ${behavior_sql}" >&2
  exit 1
fi
psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 --file "${behavior_sql}" >/dev/null

# Persistent, isolated fixture for the concurrent workers below. The lifecycle
# oracle above intentionally rolls back all of its direct-insert fixtures.
psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 >/dev/null <<'SQL'
INSERT INTO identities (id, identity_handle, encrypted_profile)
VALUES (
    '90000000-0000-0000-0000-000000000001',
    'hlt03-concurrency', decode('01', 'hex')
);
INSERT INTO devices (
    id, identity_id, device_kind, encrypted_label, trust_state
) VALUES (
    '92000000-0000-0000-0000-000000000001',
    '90000000-0000-0000-0000-000000000001',
    'service', decode('01', 'hex'), 'trusted'
);
INSERT INTO device_keys (
    id, identity_id, device_id, key_version,
    encryption_public_key, signing_public_key,
    previous_package_hash, package_hash,
    x25519_public_key, ed25519_public_key
) VALUES (
    '92100000-0000-0000-0000-000000000001',
    '90000000-0000-0000-0000-000000000001',
    '92000000-0000-0000-0000-000000000001', 1,
    decode('01', 'hex'), decode('02', 'hex'),
    decode(repeat('00', 32), 'hex'),
    digest('92000000-0000-0000-0000-000000000001', 'sha256'),
    decode('01', 'hex'), decode('02', 'hex')
);
INSERT INTO projects (id, owner_identity_id, encrypted_metadata)
VALUES (
    '93000000-0000-0000-0000-000000000001',
    '90000000-0000-0000-0000-000000000001', decode('01', 'hex')
);
INSERT INTO project_memberships (project_id, identity_id, role)
VALUES (
    '93000000-0000-0000-0000-000000000001',
    '90000000-0000-0000-0000-000000000001', 'owner'
);
INSERT INTO resource_nodes (
    id, project_id, parent_id, node_kind,
    encrypted_metadata, created_by_identity_id
) VALUES
    (
        '94000000-0000-0000-0000-000000000001',
        '93000000-0000-0000-0000-000000000001',
        NULL, 'root', decode('01', 'hex'),
        '90000000-0000-0000-0000-000000000001'
    ),
    (
        '94000000-0000-0000-0000-000000000002',
        '93000000-0000-0000-0000-000000000001',
        '94000000-0000-0000-0000-000000000001',
        'topic', decode('02', 'hex'),
        '90000000-0000-0000-0000-000000000001'
    ),
    (
        '94000000-0000-0000-0000-000000000003',
        '93000000-0000-0000-0000-000000000001',
        '94000000-0000-0000-0000-000000000002',
        'task_list', decode('03', 'hex'),
        '90000000-0000-0000-0000-000000000001'
    );
INSERT INTO resource_epochs (
    id, project_id, resource_node_id, epoch,
    created_by_identity_id, created_by_device_id,
    created_by_device_key_version, key_commitment, reason
) VALUES
    (
        '96000000-0000-0000-0000-000000000002',
        '93000000-0000-0000-0000-000000000001',
        '94000000-0000-0000-0000-000000000002', 1,
        '90000000-0000-0000-0000-000000000001',
        '92000000-0000-0000-0000-000000000001', 1,
        decode(repeat('02', 16), 'hex'), 'created'
    ),
    (
        '96000000-0000-0000-0000-000000000003',
        '93000000-0000-0000-0000-000000000001',
        '94000000-0000-0000-0000-000000000003', 1,
        '90000000-0000-0000-0000-000000000001',
        '92000000-0000-0000-0000-000000000001', 1,
        decode(repeat('03', 16), 'hex'), 'created'
    );
INSERT INTO topics (
    id, project_id, resource_node_id, encrypted_payload
) VALUES (
    '95000000-0000-0000-0000-000000000001',
    '93000000-0000-0000-0000-000000000001',
    '94000000-0000-0000-0000-000000000002', decode('01', 'hex')
);
INSERT INTO task_lists (
    id, project_id, topic_id, resource_node_id, encrypted_payload
) VALUES (
    '95100000-0000-0000-0000-000000000001',
    '93000000-0000-0000-0000-000000000001',
    '95000000-0000-0000-0000-000000000001',
    '94000000-0000-0000-0000-000000000003', decode('01', 'hex')
);
INSERT INTO recurrence_series (
    id, project_id, task_list_id, encrypted_rule, created_by_identity_id
) VALUES (
    '95500000-0000-0000-0000-000000000001',
    '93000000-0000-0000-0000-000000000001',
    '95100000-0000-0000-0000-000000000001',
    decode('01', 'hex'),
    '90000000-0000-0000-0000-000000000001'
);
SQL

# T-LLR-03.6 / T-LLR-07.5: two workers race on the same series occurrence.
# The first keeps its transaction open while the second reaches the UNIQUE
# index and blocks; after commit exactly one worker succeeds.
worker() {
  local task_id="$1"
  local resource_id="$2"
  local pause="$3"
  psql "${DATABASE_URL}" --set ON_ERROR_STOP=1 >/dev/null <<SQL
BEGIN;
INSERT INTO resource_nodes (
    id, project_id, parent_id, node_kind,
    encrypted_metadata, created_by_identity_id
) VALUES (
    '${resource_id}',
    '93000000-0000-0000-0000-000000000001',
    '94000000-0000-0000-0000-000000000003',
    'task', decode('6801', 'hex'),
    '90000000-0000-0000-0000-000000000001'
);
INSERT INTO resource_epochs (
    id, project_id, resource_node_id, epoch,
    created_by_identity_id, created_by_device_id,
    created_by_device_key_version, key_commitment, reason
) VALUES (
    gen_random_uuid(),
    '93000000-0000-0000-0000-000000000001',
    '${resource_id}', 1,
    '90000000-0000-0000-0000-000000000001',
    '92000000-0000-0000-0000-000000000001', 1,
    decode(repeat('68', 16), 'hex'), 'created'
);
INSERT INTO tasks (
    id, project_id, task_list_id, resource_node_id,
    task_kind, encrypted_payload, encrypted_value_snapshot,
    recurrence_series_id, occurrence_number, created_by_identity_id
) VALUES (
    '${task_id}',
    '93000000-0000-0000-0000-000000000001',
    '95100000-0000-0000-0000-000000000001',
    '${resource_id}',
    'recurring', decode('6801', 'hex'), decode('6802', 'hex'),
    '95500000-0000-0000-0000-000000000001', 99,
    '90000000-0000-0000-0000-000000000001'
);
SELECT pg_sleep(${pause});
COMMIT;
SQL
}

set +e
worker \
  "56400000-0000-0000-0000-000000000098" \
  "40000000-0000-0000-0000-000000000098" \
  "1" &
first_pid=$!
sleep 0.1
worker \
  "56400000-0000-0000-0000-000000000099" \
  "40000000-0000-0000-0000-000000000099" \
  "0" &
second_pid=$!
wait "${first_pid}"
first_status=$?
wait "${second_pid}"
second_status=$?
set -e

if [[ "${first_status}" -eq 0 && "${second_status}" -eq 0 ]] ||
   [[ "${first_status}" -ne 0 && "${second_status}" -ne 0 ]]; then
  echo "T-LLR-03.6 expected exactly one concurrent worker to succeed" >&2
  exit 1
fi

count="$(
  psql "${DATABASE_URL}" --tuples-only --no-align --set ON_ERROR_STOP=1 \
    --command "
      SELECT count(*)
      FROM tasks
      WHERE recurrence_series_id =
            '95500000-0000-0000-0000-000000000001'
        AND occurrence_number = 99
    "
)"
if [[ "${count}" != "1" ]]; then
  echo "T-LLR-03.6 persisted ${count} concurrent occurrences; expected 1" >&2
  exit 1
fi

echo "HLT-03 task lifecycle and T-LLR-03.6 concurrency passed"
