#!/usr/bin/env bash
set -euo pipefail

partial() {
  printf 'PARTIAL\t%s\t%s\n' "$1" "$2"
}

external() {
  printf 'EXTERNAL\t%s\t%s\n' "$1" "$2"
}

list_gates() {
  external "HLT-10" "Requires the Linux systemd and disaster-recovery release evidence"
  external "T-LLR-10.5" "Requires a real Linux systemd VM or equivalent release harness"
  external "T-LLR-10.6" "Requires a signed PostgreSQL/blob/archive disaster-recovery drill"
  external "HLT-11" "Requires one immutable release candidate and all release evidence"
  external "T-LLR-11.4" "Requires signed threat-model review evidence"
  external "T-LLR-11.5" "Requires independent audit and penetration-test reports"
}

require_evidence_file() {
  local variable_name="$1"
  local description="$2"
  local value="${!variable_name:-}"
  if [[ -z "${value}" || ! -f "${value}" ]]; then
    echo "Missing ${description}; set ${variable_name} to its evidence file" >&2
    return 1
  fi
}

case "${1:---list}" in
  --list)
    list_gates
    ;;
  --release)
    require_evidence_file \
      SPROUT_SYSTEMD_VM_EVIDENCE \
      "T-LLR-10.5 systemd VM evidence"
    require_evidence_file \
      SPROUT_DISASTER_RECOVERY_EVIDENCE \
      "T-LLR-10.6 disaster-recovery evidence"
    require_evidence_file \
      SPROUT_THREAT_MODEL_REVIEW_EVIDENCE \
      "T-LLR-11.4 signed threat-model review"
    require_evidence_file \
      SPROUT_INDEPENDENT_AUDIT_EVIDENCE \
      "T-LLR-11.5 independent audit and penetration evidence"
    echo "External requirements release evidence is present"
    ;;
  *)
    echo "Usage: $0 [--list|--release]" >&2
    exit 2
    ;;
esac
