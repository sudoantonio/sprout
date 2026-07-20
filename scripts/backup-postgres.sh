#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: backup-postgres.sh --output-dir DIRECTORY --blobs-dir DIRECTORY
                          --archives-dir DIRECTORY --signing-key-file FILE

Creates a signed consistency-set directory. The signing key is read from its
protected external path and is never copied into the backup.
EOF
}

output_dir=""
blobs_dir=""
archives_dir=""
signing_key_file=""
while (($# > 0)); do
  case "$1" in
    --output-dir)
      output_dir="${2:-}"
      shift 2
      ;;
    --blobs-dir)
      blobs_dir="${2:-}"
      shift 2
      ;;
    --archives-dir)
      archives_dir="${2:-}"
      shift 2
      ;;
    --signing-key-file)
      signing_key_file="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "${output_dir}" || -z "${blobs_dir}" || -z "${archives_dir}" || -z "${signing_key_file}" ]]; then
  usage
  exit 2
fi
: "${PGDATABASE:?Set PGDATABASE to the database to back up}"
: "${PGUSER:?Set PGUSER to the database role}"

command -v pg_dump >/dev/null || {
  echo "pg_dump is required" >&2
  exit 127
}
command -v pg_restore >/dev/null || {
  echo "pg_restore is required" >&2
  exit 127
}
for required_command in openssl python3 tar; do
  command -v "${required_command}" >/dev/null || {
    echo "${required_command} is required" >&2
    exit 127
  }
done
if command -v sha256sum >/dev/null; then
  checksum_command=(sha256sum)
elif command -v shasum >/dev/null; then
  checksum_command=(shasum -a 256)
else
  echo "sha256sum or shasum is required" >&2
  exit 127
fi
if [[ ! -d "${blobs_dir}" || ! -r "${blobs_dir}" ]]; then
  echo "Blob directory is not readable: ${blobs_dir}" >&2
  exit 1
fi
if [[ ! -d "${archives_dir}" || ! -r "${archives_dir}" ]]; then
  echo "Archive directory is not readable: ${archives_dir}" >&2
  exit 1
fi
if [[ ! -f "${signing_key_file}" || ! -r "${signing_key_file}" ]]; then
  echo "Signing key is not a readable regular file" >&2
  exit 1
fi

canonical_dir() {
  (cd "$1" && pwd -P)
}
canonical_file() {
  local directory
  directory="$(cd "$(dirname "$1")" && pwd -P)"
  printf '%s/%s\n' "${directory}" "$(basename "$1")"
}

blobs_dir="$(canonical_dir "${blobs_dir}")"
archives_dir="$(canonical_dir "${archives_dir}")"
signing_key_file="$(canonical_file "${signing_key_file}")"
for data_dir in "${blobs_dir}" "${archives_dir}"; do
  if [[ "${signing_key_file}" == "${data_dir}/"* ]]; then
    echo "Refusing to archive a data directory containing the signing key" >&2
    exit 1
  fi
  python3 - "${data_dir}" <<'PY'
import os
import pathlib
import stat
import sys

root = pathlib.Path(sys.argv[1])
for directory, names, files in os.walk(root, followlinks=False):
    for name in [*names, *files]:
        candidate = pathlib.Path(directory, name)
        mode = candidate.lstat().st_mode
        if stat.S_ISLNK(mode) or not (stat.S_ISDIR(mode) or stat.S_ISREG(mode)):
            raise SystemExit(
                f"Refusing to back up links or special files: {candidate}"
            )
PY
done

umask 077
if [[ ! -d "${output_dir}" ]]; then
  mkdir -p -- "${output_dir}"
fi
if [[ ! -w "${output_dir}" ]]; then
  echo "Backup directory is not writable: ${output_dir}" >&2
  exit 1
fi

timestamp="$(date -u '+%Y%m%dT%H%M%SZ')"
database_name="${PGDATABASE##*/}"
database_name="${database_name//[^a-zA-Z0-9_.-]/_}"
destination="${output_dir%/}/${database_name}-${timestamp}"
temporary="${destination}.partial.$$"

if [[ -e "${destination}" ]]; then
  echo "Refusing to overwrite existing backup: ${destination}" >&2
  exit 1
fi

cleanup() {
  rm -rf -- "${temporary}"
}
trap cleanup EXIT
mkdir -m 0700 -- "${temporary}"

pg_dump \
  --format=custom \
  --compress=9 \
  --no-owner \
  --no-privileges \
  --file="${temporary}/postgres.dump"
pg_restore --list "${temporary}/postgres.dump" >/dev/null
tar -C "${blobs_dir}" -cf "${temporary}/blobs.tar" .
tar -C "${archives_dir}" -cf "${temporary}/archives.tar" .
cat >"${temporary}/manifest.json" <<EOF
{"format":"sprout-backup-v1","created_at":"${timestamp}","database":"${database_name}","artifacts":["postgres.dump","blobs.tar","archives.tar"]}
EOF
(
  cd "${temporary}"
  "${checksum_command[@]}" archives.tar blobs.tar manifest.json postgres.dump >manifest.sha256
)
openssl pkeyutl -sign -rawin \
  -inkey "${signing_key_file}" \
  -in "${temporary}/manifest.sha256" \
  -out "${temporary}/manifest.sig"
chmod 0400 "${temporary}"/*
mv -- "${temporary}" "${destination}"
trap - EXIT

echo "Signed backup consistency set written to ${destination}"
