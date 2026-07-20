#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
units=(
  "${repository_root}/deploy/sprout.service"
  "${repository_root}/deploy/sprout-worker.service"
)

required_exact=(
  "User=sprout"
  "Group=sprout"
  "UMask=0077"
  "NoNewPrivileges=yes"
  "ProtectSystem=strict"
  "ProtectHome=yes"
  "PrivateDevices=yes"
  "PrivateTmp=yes"
  "RestrictSUIDSGID=yes"
  "MemoryDenyWriteExecute=yes"
  "TimeoutStopSec=30s"
)

for unit in "${units[@]}"; do
  [[ -s "${unit}" ]] || {
    echo "Missing systemd unit: ${unit}" >&2
    exit 1
  }
  for directive in "${required_exact[@]}"; do
    if ! rg --fixed-strings --line-regexp "${directive}" "${unit}" >/dev/null; then
      echo "${unit} is missing hardening directive: ${directive}" >&2
      exit 1
    fi
  done
  if ! rg '^CapabilityBoundingSet=$' "${unit}" >/dev/null; then
    echo "${unit} must drop all Linux capabilities" >&2
    exit 1
  fi
  if ! rg '^ReadWritePaths=/var/lib/sprout/blobs /var/lib/sprout/archives$' "${unit}" >/dev/null; then
    echo "${unit} must restrict writable data paths" >&2
    exit 1
  fi
done

if [[ "${SPROUT_SYSTEMD_ANALYZE:-0}" == "1" ]] \
  && command -v systemd-analyze >/dev/null 2>&1; then
  systemd-analyze verify "${units[@]}"
fi

echo "Verified systemd hardening for ${#units[@]} units"
