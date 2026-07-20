#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: release-gate.sh --artifacts DIRECTORY --release-manifest-dir DIRECTORY
                       --release-verification-key-file FILE
                       --crypto-audit-evidence FILE
                       --penetration-test-evidence FILE
                       --threat-model-evidence FILE

All evidence files must be non-empty artifacts covered by the signed release
checksums. Their presence does not itself assert who performed the review.
EOF
}

artifacts=""
manifest_dir=""
verification_key=""
crypto_audit=""
penetration_test=""
threat_model=""
while (($# > 0)); do
  case "$1" in
    --artifacts) artifacts="${2:-}"; shift 2 ;;
    --release-manifest-dir) manifest_dir="${2:-}"; shift 2 ;;
    --release-verification-key-file) verification_key="${2:-}"; shift 2 ;;
    --crypto-audit-evidence) crypto_audit="${2:-}"; shift 2 ;;
    --penetration-test-evidence) penetration_test="${2:-}"; shift 2 ;;
    --threat-model-evidence) threat_model="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage; exit 2 ;;
  esac
done

if [[ -z "${artifacts}" || -z "${manifest_dir}" || -z "${verification_key}" || -z "${crypto_audit}" || -z "${penetration_test}" || -z "${threat_model}" ]]; then
  echo "Production release blocked: explicit audit, penetration-test, and threat-model evidence is required" >&2
  usage
  exit 1
fi
for required_command in cargo npm openssl; do
  command -v "${required_command}" >/dev/null || {
    echo "${required_command} is required" >&2
    exit 127
  }
done
if command -v sha256sum >/dev/null; then
  checksum_verify=(sha256sum --check)
elif command -v shasum >/dev/null; then
  checksum_verify=(shasum -a 256 --check)
else
  echo "sha256sum or shasum is required" >&2
  exit 127
fi
for file in release.json release.sha256 release.sig artifacts.sha256; do
  if [[ ! -s "${manifest_dir%/}/${file}" ]]; then
    echo "Release manifest artifact is missing or empty: ${file}" >&2
    exit 1
  fi
done
if [[ ! -s "${verification_key}" ]]; then
  echo "Release verification key is missing or empty" >&2
  exit 1
fi
for evidence in "${crypto_audit}" "${penetration_test}" "${threat_model}"; do
  if [[ ! -s "${evidence}" ]]; then
    echo "Required release evidence is missing or empty: ${evidence}" >&2
    exit 1
  fi
done

artifacts_root="$(cd "${artifacts}" && pwd -P)"
manifest_root="$(cd "${manifest_dir}" && pwd -P)"
canonical_file() {
  local directory
  directory="$(cd "$(dirname "$1")" && pwd -P)"
  printf '%s/%s\n' "${directory}" "$(basename "$1")"
}
for evidence in "${crypto_audit}" "${penetration_test}" "${threat_model}"; do
  canonical="$(canonical_file "${evidence}")"
  if [[ "${canonical}" != "${artifacts_root}/"* ]]; then
    echo "Release evidence must be inside the signed artifact directory" >&2
    exit 1
  fi
  relative="${canonical#"${artifacts_root}/"}"
  if ! awk -v expected="${relative}" '$2 == expected || $2 == "*" expected { found = 1 } END { exit !found }' \
    "${manifest_root}/artifacts.sha256"; then
    echo "Release evidence is not covered by artifact checksums: ${relative}" >&2
    exit 1
  fi
done
if ! awk '$2 ~ /\.spdx\.json$/ { spdx = 1 } $2 ~ /\.cyclonedx\.json$/ { cyclonedx = 1 } END { exit !(spdx && cyclonedx) }' \
  "${manifest_root}/artifacts.sha256"; then
  echo "Signed artifacts must include SPDX and CycloneDX SBOMs" >&2
  exit 1
fi

(
  cd "${manifest_root}"
  "${checksum_verify[@]}" release.sha256
)
openssl pkeyutl -verify -pubin -rawin \
  -inkey "${verification_key}" \
  -in "${manifest_root}/release.sha256" \
  -sigfile "${manifest_root}/release.sig" >/dev/null
(
  cd "${artifacts_root}"
  "${checksum_verify[@]}" "${manifest_root}/artifacts.sha256"
)

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
"${repository_root}/scripts/verify-requirements-traceability.sh"
(
  cd "${repository_root}"
  cargo deny check advisories licenses bans sources
  cargo audit --deny warnings
)
(
  cd "${repository_root}/apps/web"
  npm audit --audit-level=high
)

echo "Release gate passed supplied evidence and automated checks"
