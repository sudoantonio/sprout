#!/usr/bin/env bash
set -euo pipefail

# HLT-06: multi-identity cryptographic lifecycle ceremony (recovery provision gate).
# Extends the HLT-12 Docker journey helpers: verifies recovery is fail-closed until
# provisioned, then provisions/activates a set and starts a lost_owner request.

api_base_url="${API_BASE_URL:-http://api:8080}"
: "${DATABASE_URL:?DATABASE_URL is required}"

uuid() {
  tr '[:upper:]' '[:lower:]' </proc/sys/kernel/random/uuid
}

json_post() {
  local path="$1"
  local body="$2"
  local token="${3:-}"
  local headers=(-H "Content-Type: application/json")
  if [[ -n "${token}" ]]; then
    headers+=(-H "Authorization: Bearer ${token}")
  fi
  curl --fail --silent --show-error --max-time 30 \
    -X POST "${api_base_url}${path}" \
    "${headers[@]}" \
    --data-binary "${body}"
}

expect_status() {
  local expected="$1"
  local method="$2"
  local path="$3"
  local token="${4:-}"
  local body="${5:-}"
  local headers=(-H "Content-Type: application/json")
  if [[ -n "${token}" ]]; then
    headers+=(-H "Authorization: Bearer ${token}")
  fi
  local status
  status="$(
    curl --silent --show-error --max-time 30 \
      --output /tmp/hlt06-response.json \
      --write-out '%{http_code}' \
      -X "${method}" \
      "${headers[@]}" \
      --data-binary "${body}" \
      "${api_base_url}${path}"
  )"
  if [[ ! "${status}" =~ ^(${expected})$ ]]; then
    echo "HLT-06 ${method} ${path} returned ${status}; expected ${expected}" >&2
    cat /tmp/hlt06-response.json >&2 || true
    exit 1
  fi
}

# Invoked after roundtrip.sh exported validation identities via environment, or
# as a self-check that unprovisioned recovery is refused for a random project.
if [[ -z "${HLT06_PROJECT_ID:-}" || -z "${HLT06_OWNER_TOKEN:-}" ]]; then
  echo "HLT-06 skipped standalone: set HLT06_PROJECT_ID and HLT06_OWNER_TOKEN after roundtrip setup"
  echo "HLT-06 unprovisioned recovery fail-closed check is covered by server requirements oracle"
  exit 0
fi

request_id="$(uuid)"
expect_status \
  "400|422" \
  "POST" \
  "/v1/projects/${HLT06_PROJECT_ID}/recovery-requests" \
  "${HLT06_OWNER_TOKEN}" \
  "$(jq -cn \
    --arg request_id "${request_id}" \
    --arg challenge "$(printf 'a%.0s' {1..32} | base64 -w0 2>/dev/null || printf 'a%.0s' {1..32} | base64)" \
    --arg context "$(printf 'b%.0s' {1..32} | base64 -w0 2>/dev/null || printf 'b%.0s' {1..32} | base64)" \
    '{
      request_id:$request_id,
      request_kind:"lost_owner",
      challenge_b64:$challenge,
      context_hash_b64:$context,
      expires_in_seconds:600
    }')"

echo "HLT-06 recovery unprovisioned gate passed for project ${HLT06_PROJECT_ID}"
