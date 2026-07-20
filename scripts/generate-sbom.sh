#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 --output-dir DIRECTORY" >&2
}

output_dir=""
while (($# > 0)); do
  case "$1" in
    --output-dir)
      output_dir="${2:-}"
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

if [[ -z "${output_dir}" ]]; then
  usage
  exit 2
fi
command -v syft >/dev/null || {
  echo "syft is required to generate lockfile-derived SBOMs" >&2
  exit 127
}

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
umask 077
mkdir -p -- "${output_dir}"
spdx="${output_dir%/}/sprout.spdx.json"
cyclonedx="${output_dir%/}/sprout.cyclonedx.json"
for destination in "${spdx}" "${cyclonedx}"; do
  if [[ -e "${destination}" ]]; then
    echo "Refusing to overwrite existing SBOM: ${destination}" >&2
    exit 1
  fi
done

syft "dir:${repository_root}" \
  --exclude "${repository_root}/target/**" \
  --exclude "${repository_root}/**/node_modules/**" \
  --exclude "${repository_root}/var/**" \
  --exclude "${repository_root}/secrets/**" \
  --output "spdx-json=${spdx}" \
  --output "cyclonedx-json=${cyclonedx}"
chmod 0444 "${spdx}" "${cyclonedx}"
echo "Generated SPDX and CycloneDX SBOMs in ${output_dir}"
