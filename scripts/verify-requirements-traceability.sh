#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 [--requirements FILE] [--matrix FILE]" >&2
}

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
requirements="${repository_root}/docs/requirements.md"
matrix="${repository_root}/tests/traceability.tsv"
while (($# > 0)); do
  case "$1" in
    --requirements) requirements="${2:-}"; shift 2 ;;
    --matrix) matrix="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage; exit 2 ;;
  esac
done

for required_command in awk comm mktemp rg sort; do
  command -v "${required_command}" >/dev/null || {
    echo "${required_command} is required" >&2
    exit 127
  }
done
if [[ ! -f "${requirements}" || ! -f "${matrix}" ]]; then
  echo "Requirements and traceability matrix must both exist" >&2
  exit 1
fi

required_ids="$(mktemp "${TMPDIR:-/tmp}/sprout-required.XXXXXX")"
mapped_ids="$(mktemp "${TMPDIR:-/tmp}/sprout-mapped.XXXXXX")"
statuses="$(mktemp "${TMPDIR:-/tmp}/sprout-statuses.XXXXXX")"
cleanup() {
  rm -f -- "${required_ids}" "${mapped_ids}" "${statuses}"
}
trap cleanup EXIT

rg --only-matching 'HLT-[0-9]{2}|T-LLR-[0-9]{2}\.[0-9]+' "${requirements}" \
  | sort -u >"${required_ids}"
awk -F '\t' '
  $0 !~ /^#/ && NF {
    if (NF != 4 || $1 == "" || $2 == "" || $3 == "" || $4 == "") {
      print "Invalid traceability row " NR > "/dev/stderr"
      exit 1
    }
    if ($1 !~ /^(HLT-[0-9][0-9]|T-LLR-[0-9][0-9][.][0-9]+)$/) {
      print "Invalid declared test ID on traceability row " NR > "/dev/stderr"
      exit 1
    }
    if ($4 !~ /^(automated|partial|external)$/) {
      print "Invalid traceability status on row " NR > "/dev/stderr"
      exit 1
    }
    print $1
  }
' "${matrix}" | sort >"${mapped_ids}"

if [[ -n "$(sort "${mapped_ids}" | uniq -d)" ]]; then
  echo "Traceability matrix contains duplicate requirement IDs" >&2
  exit 1
fi
if ! comm -3 "${required_ids}" "${mapped_ids}" | awk 'NF { found = 1; print > "/dev/stderr" } END { exit found }'; then
  echo "Traceability matrix does not exactly cover every declared HLT and T-LLR" >&2
  exit 1
fi

while IFS=$'\t' read -r test_id path test_name status; do
  [[ -n "${test_id}" && "${test_id}" != \#* ]] || continue
  if [[ "${path}" == /* || "${path}" == *".."* ]]; then
    echo "${test_id} uses an unsafe artifact path: ${path}" >&2
    exit 1
  fi
  case "${path}" in
    apps/server/tests/*|frontend/sprout-web/tests/*|db/tests/*|tests/system/*|scripts/validation/*|.github/workflows/*) ;;
    *)
      echo "${test_id} maps to non-test product or prose artifact: ${path}" >&2
      exit 1
      ;;
  esac
  if [[ ! -f "${repository_root}/${path}" ]]; then
    echo "${test_id} maps to missing test artifact: ${path}" >&2
    exit 1
  fi
  if [[ -z "${test_name}" ]]; then
    echo "${test_id} has no named evidence" >&2
    exit 1
  fi
  if ! rg --fixed-strings --quiet "${test_id}" "${repository_root}/${path}"; then
    echo "${test_id} is absent from its referenced test artifact: ${path}" >&2
    exit 1
  fi
  if [[ "${status}" == "external" ]] \
    && ! rg --fixed-strings --quiet "external \"${test_id}\"" "${repository_root}/${path}"
  then
    echo "${test_id} is not explicitly marked ${status} in ${path}" >&2
    exit 1
  fi
  if [[ "${status}" == "partial" && "${path}" == "tests/system/requirements-gates.sh" ]] \
    && ! rg --fixed-strings --quiet "partial \"${test_id}\"" "${repository_root}/${path}"
  then
    echo "${test_id} is not explicitly marked ${status} in ${path}" >&2
    exit 1
  fi
  printf '%s\n' "${status}" >>"${statuses}"
done <"${matrix}"

automated="$(awk '$1 == "automated" { count += 1 } END { print count + 0 }' "${statuses}")"
partial="$(awk '$1 == "partial" { count += 1 } END { print count + 0 }' "${statuses}")"
external="$(awk '$1 == "external" { count += 1 } END { print count + 0 }' "${statuses}")"
echo "Traceability covers $(wc -l <"${required_ids}" | tr -d ' ') declared tests: ${automated} automated, ${partial} partial, ${external} external"
