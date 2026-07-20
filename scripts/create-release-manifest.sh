#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: create-release-manifest.sh --artifacts DIRECTORY --output-dir DIRECTORY
                                  --signing-key-file FILE

The Git worktree must be clean. The external signing key is never copied.
EOF
}

artifacts=""
output_dir=""
signing_key_file=""
while (($# > 0)); do
  case "$1" in
    --artifacts)
      artifacts="${2:-}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:-}"
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

if [[ -z "${artifacts}" || -z "${output_dir}" || -z "${signing_key_file}" ]]; then
  usage
  exit 2
fi
for required_command in find git openssl sort; do
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
if [[ ! -d "${artifacts}" || ! -r "${artifacts}" ]]; then
  echo "Artifact directory is not readable: ${artifacts}" >&2
  exit 1
fi
if [[ ! -f "${signing_key_file}" || ! -r "${signing_key_file}" ]]; then
  echo "Signing key is not a readable regular file" >&2
  exit 1
fi
if [[ -e "${output_dir}" ]]; then
  echo "Refusing to overwrite release manifest directory: ${output_dir}" >&2
  exit 1
fi

artifacts="$(cd "${artifacts}" && pwd -P)"
signing_key_directory="$(cd "$(dirname "${signing_key_file}")" && pwd -P)"
signing_key_file="${signing_key_directory}/$(basename "${signing_key_file}")"
output_parent="$(cd "$(dirname "${output_dir}")" && pwd -P)"
output_dir="${output_parent}/$(basename "${output_dir}")"
if [[ "${signing_key_file}" == "${artifacts}/"* ]]; then
  echo "Refusing to include the release signing key in signed artifacts" >&2
  exit 1
fi
if [[ "${output_dir}" == "${artifacts}" || "${output_dir}" == "${artifacts}/"* ]]; then
  echo "Release manifest output must be outside the artifact directory" >&2
  exit 1
fi

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
if [[ -n "$(git -C "${repository_root}" status --porcelain --untracked-files=all)" ]]; then
  echo "Refusing to create an immutable release manifest from a dirty worktree" >&2
  exit 1
fi
commit="$(git -C "${repository_root}" rev-parse --verify HEAD)"
created_epoch="${SOURCE_DATE_EPOCH:-$(date -u '+%s')}"
if [[ ! "${created_epoch}" =~ ^[0-9]+$ ]]; then
  echo "SOURCE_DATE_EPOCH must be an unsigned integer" >&2
  exit 2
fi

umask 077
mkdir -m 0700 -- "${output_dir}"
cleanup() {
  chmod -R u+w "${output_dir}" 2>/dev/null || true
  rm -rf -- "${output_dir}"
}
trap cleanup EXIT

regular_files=()
while IFS= read -r file; do
  relative="${file#"${artifacts%/}/"}"
  if [[ ! "${relative}" =~ ^[a-zA-Z0-9._/-]+$ ]]; then
    echo "Artifact path is not manifest-safe: ${relative}" >&2
    exit 1
  fi
  regular_files+=("${file}")
done < <(find "${artifacts}" -type f -print | LC_ALL=C sort)
if ((${#regular_files[@]} == 0)); then
  echo "Artifact directory contains no regular files" >&2
  exit 1
fi

{
  printf '{"format":"sprout-release-v1","git_commit":"%s","source_date_epoch":%s,"artifacts":[' \
    "${commit}" "${created_epoch}"
  separator=""
  for file in "${regular_files[@]}"; do
    relative="${file#"${artifacts%/}/"}"
    printf '%s"%s"' "${separator}" "${relative}"
    separator=","
  done
  printf ']}\n'
} >"${output_dir%/}/release.json"

(
  cd "${artifacts}"
  for file in "${regular_files[@]}"; do
    relative="${file#"${artifacts%/}/"}"
    "${checksum_command[@]}" "${relative}"
  done
) >"${output_dir%/}/artifacts.sha256"
(
  cd "${output_dir}"
  "${checksum_command[@]}" release.json >release.sha256
)
openssl pkeyutl -sign -rawin \
  -inkey "${signing_key_file}" \
  -in "${output_dir%/}/release.sha256" \
  -out "${output_dir%/}/release.sig"

chmod 0444 "${output_dir%/}"/*
chmod 0555 "${output_dir}"
trap - EXIT
echo "Immutable release manifest written to ${output_dir}"
