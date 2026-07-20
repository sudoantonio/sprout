#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
cd "${repository_root}"

run() {
  local test_id="$1"
  shift
  echo "Running ${test_id}: $*"
  "$@"
}

run "T-LLR-02.6" \
  cargo test --locked -p sprout-application inherited_grant_dies_with_its_origin

run "T-LLR-03.1" \
  npm --prefix frontend/sprout-web test -- src/domain/tasks.test.ts
run "T-LLR-03.7" \
  npm --prefix frontend/sprout-web test -- src/domain/tasks.test.ts

run "T-LLR-04.2" \
  npm --prefix frontend/sprout-web test -- src/domain/questionnaires.test.ts

run "T-LLR-05.4" \
  cargo test --locked -p sprout-server llr_05_4_atomic_faults_leave_no_partial_or_temp_blob
run "T-LLR-05.4" \
  cargo test --locked -p sprout-server llr_05_4_rejects_symlink_and_hardlink_hazards
run "T-LLR-05.6" \
  npm --prefix frontend/sprout-web test -- src/downloads/download.test.ts

run "T-LLR-06.1" \
  npm --prefix frontend/sprout-web run test:wasm:parity
run "T-LLR-06.2" \
  cargo test --locked -p sprout-crypto-protocol wrong_key_and_wrong_aad_fail
run "T-LLR-06.3" \
  cargo test --locked -p sprout-crypto-protocol \
  experimental_device_generation_produces_four_interoperable_key_pairs
run "T-LLR-06.4" \
  cargo test --locked -p sprout-crypto-protocol \
  resource_epoch_rotation_is_fresh_separated_and_chained
run "T-LLR-06.6" \
  bash -c 'cargo test --locked -p sprout-crypto-protocol container_only_header_key_cannot_open_body_ciphertext && npm --prefix frontend/sprout-web test -- src/domain/envelopes.integration.test.ts'
run "T-LLR-06.7" \
  cargo test --locked -p sprout-crypto-protocol \
  recovery_xor_n_of_n_enforces_all_unique_committed_shares

run "T-LLR-07.1" \
  cargo test --locked -p sprout-crypto-protocol hash_chain_detects_reordering_and_tamper
run "T-LLR-07.2" \
  npm --prefix frontend/sprout-web test -- src/sync/sync-engine.test.ts
run "T-LLR-07.4" \
  npm --prefix frontend/sprout-web test -- src/sync/sync-engine.test.ts

run "T-LLR-08.6" \
  cargo test --locked -p sprout-server \
  llr_08_6_crash_and_disk_full_leave_no_partial_archive
run "T-LLR-08.6" \
  cargo test --locked -p sprout-server \
  llr_08_6_expired_or_stolen_lease_cannot_match_active_token
run "T-LLR-08.7" \
  npm --prefix frontend/sprout-web test -- src/domain/retention.test.ts
run "T-LLR-08.8" \
  cargo test --locked -p sprout-server \
  llr_08_8_download_is_forced_and_never_claimed_automatic

run "T-LLR-09.2" \
  npm --prefix frontend/sprout-web test -- src/domain/tasks.test.ts
