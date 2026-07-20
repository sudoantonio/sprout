#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 [--check|--apply] [--directory MIGRATIONS_DIR]" >&2
}

mode="check"
migrations_dir="${MIGRATIONS_DIR:-db/migrations}"
while (($# > 0)); do
  case "$1" in
    --check)
      mode="check"
      shift
      ;;
    --apply)
      mode="apply"
      shift
      ;;
    --directory)
      migrations_dir="${2:-}"
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

: "${DATABASE_URL:?Set DATABASE_URL through the runtime environment}"
command -v sqlx >/dev/null || {
  echo "sqlx-cli is required (expected version 0.8.6)" >&2
  exit 127
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"${script_dir}/validate-migrations.sh" --directory "${migrations_dir}"

shopt -s nullglob
migrations=("${migrations_dir}"/*.sql)
shopt -u nullglob
if ((${#migrations[@]} == 0)); then
  echo "No migrations to inspect or apply"
  exit 0
fi

case "${mode}" in
  check)
    sqlx migrate info --source "${migrations_dir}"
    ;;
  apply)
    sqlx migrate run --source "${migrations_dir}"
    sqlx migrate info --source "${migrations_dir}"
    ;;
esac
