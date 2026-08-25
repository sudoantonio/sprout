#!/usr/bin/env bash
set -euo pipefail

compose_file="${1:-compose.validation.yml}"
scan_dir="${SPROUT_BLOB_SCAN_DIR:-}"
work_dir=""
cleanup() {
  if [[ -n "${work_dir}" ]]; then
    rm -rf -- "${work_dir}"
  fi
}
trap cleanup EXIT

if [[ -z "${scan_dir}" ]]; then
  work_dir="$(mktemp -d)"
  scan_dir="${work_dir}/blobs"
  mkdir -p "${scan_dir}"
  docker compose -f "${compose_file}" cp \
    api:/var/lib/sprout/blobs/. "${scan_dir}" >/dev/null
elif [[ ! -d "${scan_dir}" ]]; then
  echo "SPROUT_BLOB_SCAN_DIR is not a directory: ${scan_dir}" >&2
  exit 1
fi

blob_count="$(
  rg --files "${scan_dir}" -g '*.blob' | wc -l | tr -d '[:space:]'
)"
if [[ "${blob_count}" -eq 0 ]]; then
  echo "HLT-05 expected a populated encrypted blob store" >&2
  exit 1
fi
if rg --text --fixed-strings --quiet \
  "sprout-classified-" "${scan_dir}"; then
  echo "T-LLR-05.2 found classified plaintext in the server blob store" >&2
  exit 1
fi

echo "T-LLR-05.2 populated server blob plaintext scan passed"
