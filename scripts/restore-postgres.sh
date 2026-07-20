#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: restore-postgres.sh --backup-dir DIRECTORY --blobs-dir DIRECTORY
                           --archives-dir DIRECTORY
                           --verification-key-file FILE --confirm

The target database must be empty. Blob and archive targets must be empty
directories. Signature and checksums are verified before any restore begins.
EOF
}

backup_dir=""
blobs_dir=""
archives_dir=""
verification_key_file=""
confirmed="false"
while (($# > 0)); do
  case "$1" in
    --backup-dir)
      backup_dir="${2:-}"
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
    --verification-key-file)
      verification_key_file="${2:-}"
      shift 2
      ;;
    --confirm)
      confirmed="true"
      shift
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

if [[ -z "${backup_dir}" || -z "${blobs_dir}" || -z "${archives_dir}" || -z "${verification_key_file}" || "${confirmed}" != "true" ]]; then
  echo "Restore requires every path and explicit --confirm" >&2
  usage
  exit 2
fi
if [[ ! -d "${backup_dir}" || ! -r "${backup_dir}" ]]; then
  echo "Backup is not a readable directory: ${backup_dir}" >&2
  exit 1
fi
: "${PGDATABASE:?Set PGDATABASE to the empty target database}"
: "${PGUSER:?Set PGUSER to the database role}"
if [[ ! "${PGDATABASE}" =~ ^[a-zA-Z0-9_.-]+$ ]]; then
  echo "PGDATABASE must be a database name, not a connection URI" >&2
  exit 2
fi

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
  checksum_command=(sha256sum --check)
elif command -v shasum >/dev/null; then
  checksum_command=(shasum -a 256 --check)
else
  echo "sha256sum or shasum is required" >&2
  exit 127
fi
for file in postgres.dump blobs.tar archives.tar manifest.json manifest.sha256 manifest.sig; do
  if [[ ! -f "${backup_dir%/}/${file}" || ! -r "${backup_dir%/}/${file}" ]]; then
    echo "Backup artifact is missing or unreadable: ${file}" >&2
    exit 1
  fi
done
if [[ ! -f "${verification_key_file}" || ! -r "${verification_key_file}" ]]; then
  echo "Verification key is not a readable regular file" >&2
  exit 1
fi
for target in "${blobs_dir}" "${archives_dir}"; do
  if [[ ! -d "${target}" || -n "$(ls -A "${target}")" ]]; then
    echo "Restore target must be an existing empty directory: ${target}" >&2
    exit 1
  fi
done

openssl pkeyutl -verify -pubin -rawin \
  -inkey "${verification_key_file}" \
  -in "${backup_dir%/}/manifest.sha256" \
  -sigfile "${backup_dir%/}/manifest.sig" >/dev/null
(
  cd "${backup_dir}"
  "${checksum_command[@]}" manifest.sha256
)

validate_tar_paths() {
  local archive="$1"
  python3 - "${archive}" <<'PY'
import pathlib
import sys
import tarfile

archive = pathlib.Path(sys.argv[1])
with tarfile.open(archive, mode="r:") as members:
    for member in members:
        path = pathlib.PurePosixPath(member.name)
        if path.is_absolute() or ".." in path.parts:
            raise SystemExit(f"Unsafe path in backup archive: {member.name}")
        if not (member.isdir() or member.isreg()):
            raise SystemExit(
                f"Links and special files are forbidden in backup archives: {member.name}"
            )
PY
}
validate_tar_paths "${backup_dir%/}/blobs.tar"
validate_tar_paths "${backup_dir%/}/archives.tar"

pg_restore --list "${backup_dir%/}/postgres.dump" >/dev/null
echo "Restoring into host=${PGHOST:-local-default} port=${PGPORT:-5432} database=${PGDATABASE} user=${PGUSER}"
echo "The script will not drop, clean, or create the target database."

umask 077
blobs_parent="$(cd "$(dirname "${blobs_dir}")" && pwd -P)"
archives_parent="$(cd "$(dirname "${archives_dir}")" && pwd -P)"
blobs_stage="$(mktemp -d "${blobs_parent}/.sprout-blobs-restore.XXXXXX")"
archives_stage="$(mktemp -d "${archives_parent}/.sprout-archives-restore.XXXXXX")"
cleanup() {
  rm -rf -- "${blobs_stage}" "${archives_stage}"
}
trap cleanup EXIT
tar -C "${blobs_stage}" -xf "${backup_dir%/}/blobs.tar"
tar -C "${archives_stage}" -xf "${backup_dir%/}/archives.tar"

pg_restore \
  --exit-on-error \
  --single-transaction \
  --no-owner \
  --no-privileges \
  --dbname="${PGDATABASE}" \
  "${backup_dir%/}/postgres.dump"

rmdir -- "${blobs_dir}" "${archives_dir}"
mv -- "${blobs_stage}" "${blobs_dir}"
mv -- "${archives_stage}" "${archives_dir}"
trap - EXIT

echo "Signed consistency-set restore completed; verify integrity before serving traffic"
