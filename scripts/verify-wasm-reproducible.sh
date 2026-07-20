#!/usr/bin/env bash
set -euo pipefail

if command -v sha256sum >/dev/null; then
  checksum_command=(sha256sum)
elif command -v shasum >/dev/null; then
  checksum_command=(shasum -a 256)
else
  echo "sha256sum or shasum is required" >&2
  exit 127
fi
for required_command in diff mktemp npm; do
  command -v "${required_command}" >/dev/null || {
    echo "${required_command} is required" >&2
    exit 127
  }
done

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
web_root="${repository_root}/frontend/sprout-web"
output_dir="${web_root}/public/wasm"
first="$(mktemp "${TMPDIR:-/tmp}/sprout-wasm-first.XXXXXX")"
second="$(mktemp "${TMPDIR:-/tmp}/sprout-wasm-second.XXXXXX")"
cleanup() {
  rm -f -- "${first}" "${second}"
}
trap cleanup EXIT

checksum_output() {
  local destination="$1"
  (
    cd "${output_dir}"
    shopt -s nullglob
    files=(*)
    shopt -u nullglob
    for file in "${files[@]}"; do
      [[ -f "${file}" ]] || continue
      "${checksum_command[@]}" "${file}"
    done
  ) >"${destination}"
}

SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}" npm --prefix "${web_root}" run wasm:build
checksum_output "${first}"
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}" npm --prefix "${web_root}" run wasm:build
checksum_output "${second}"

if ! diff -u "${first}" "${second}"; then
  echo "WASM build is not reproducible with the pinned inputs" >&2
  exit 1
fi
echo "WASM build reproduced byte-for-byte"
