#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 [--directory MIGRATIONS_DIR]" >&2
}

migrations_dir="${MIGRATIONS_DIR:-db/migrations}"
while (($# > 0)); do
  case "$1" in
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

set_present_output() {
  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    printf 'present=%s\n' "$1" >>"${GITHUB_OUTPUT}"
  fi
}

if [[ ! -d "${migrations_dir}" ]]; then
  set_present_output false
  echo "No migration directory exists yet: ${migrations_dir}"
  exit 0
fi

shopt -s nullglob
migrations=("${migrations_dir}"/*.sql)
shopt -u nullglob
if ((${#migrations[@]} == 0)); then
  set_present_output false
  echo "No SQL migrations found in ${migrations_dir}"
  exit 0
fi

versions=()
descriptions=()
forms=()
filenames=()
for migration in "${migrations[@]}"; do
  filename="${migration##*/}"
  if [[ ! "${filename}" =~ ^([0-9]+)_([a-zA-Z0-9][a-zA-Z0-9_-]*)(\.(up|down))?\.sql$ ]]; then
    echo "Invalid migration name: ${filename}" >&2
    echo "Expected VERSION_description.sql or a matched .up.sql/.down.sql pair" >&2
    exit 1
  fi
  if [[ ! -s "${migration}" ]]; then
    echo "Migration is empty: ${migration}" >&2
    exit 1
  fi

  version="${BASH_REMATCH[1]}"
  description="${BASH_REMATCH[2]}"
  form="${BASH_REMATCH[4]:-simple}"
  for index in "${!versions[@]}"; do
    if [[ "${versions[${index}]}" == "${version}" && "${descriptions[${index}]}" != "${description}" ]]; then
      echo "Migration version ${version} has inconsistent descriptions" >&2
      exit 1
    fi
    if [[ "${versions[${index}]}" == "${version}" && "${forms[${index}]}" == "${form}" ]]; then
      echo "Duplicate ${form} migration for version ${version}" >&2
      exit 1
    fi
  done
  versions+=("${version}")
  descriptions+=("${description}")
  forms+=("${form}")
  filenames+=("${filename}")
done

# A sentinel keeps Bash 3.2 + `set -u` from treating an empty array expansion
# as an unbound variable on macOS.
checked_versions=("")
for version in "${versions[@]}"; do
  already_checked="false"
  for checked_version in "${checked_versions[@]}"; do
    if [[ "${checked_version}" == "${version}" ]]; then
      already_checked="true"
      break
    fi
  done
  if [[ "${already_checked}" == "true" ]]; then
    continue
  fi
  checked_versions+=("${version}")

  simple=""
  up=""
  down=""
  for index in "${!versions[@]}"; do
    if [[ "${versions[${index}]}" == "${version}" ]]; then
      case "${forms[${index}]}" in
        simple) simple="${filenames[${index}]}" ;;
        up) up="${filenames[${index}]}" ;;
        down) down="${filenames[${index}]}" ;;
      esac
    fi
  done
  if [[ -n "${simple}" && ( -n "${up}" || -n "${down}" ) ]]; then
    echo "Version ${version} mixes simple and reversible migration forms" >&2
    exit 1
  fi
  if [[ -z "${simple}" && ( -z "${up}" || -z "${down}" ) ]]; then
    echo "Version ${version} must provide both .up.sql and .down.sql files" >&2
    exit 1
  fi
done

set_present_output true
echo "Validated ${#migrations[@]} migration file(s) in ${migrations_dir}"
