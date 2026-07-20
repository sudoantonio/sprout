#!/usr/bin/env bash
set -euo pipefail

compose_file="${1:-compose.validation.yml}"
work_dir="$(mktemp -d)"
cleanup() {
  rm -rf -- "${work_dir}"
}
trap cleanup EXIT

mkdir -p "${work_dir}/blobs"
docker compose -f "${compose_file}" cp \
  api:/var/lib/sprout/blobs/. "${work_dir}/blobs" >/dev/null

blob_count="$(
  rg --files "${work_dir}/blobs" -g '*.blob' | wc -l | tr -d '[:space:]'
)"
if [[ "${blob_count}" -eq 0 ]]; then
  echo "HLT-05 expected a populated encrypted blob store" >&2
  exit 1
fi
if rg --text --fixed-strings --quiet \
  "sprout-classified-" "${work_dir}/blobs"; then
  echo "T-LLR-05.2 found classified plaintext in the server blob store" >&2
  exit 1
fi

echo "T-LLR-05.2 populated server blob plaintext scan passed"
