#!/usr/bin/env bash
set -euo pipefail

base_url="${SPROUT_BASE_URL:-http://127.0.0.1:8080}"
requests="${SPROUT_LOAD_REQUESTS:-200}"
concurrency="${SPROUT_LOAD_CONCURRENCY:-20}"
body_limit="${SPROUT_BODY_LIMIT_BYTES:-1048576}"
if [[ ! "${requests}" =~ ^[0-9]+$ ]] || ((requests < 1 || requests > 10000)); then
  echo "SPROUT_LOAD_REQUESTS must be between 1 and 10000" >&2
  exit 2
fi
if [[ ! "${concurrency}" =~ ^[0-9]+$ ]] || ((concurrency < 1 || concurrency > 100)); then
  echo "SPROUT_LOAD_CONCURRENCY must be between 1 and 100" >&2
  exit 2
fi
if [[ ! "${body_limit}" =~ ^[0-9]+$ ]] || ((body_limit < 1 || body_limit > 104857600)); then
  echo "SPROUT_BODY_LIMIT_BYTES must be between 1 and 104857600" >&2
  exit 2
fi
for required_command in awk curl mktemp seq xargs; do
  command -v "${required_command}" >/dev/null || {
    echo "${required_command} is required" >&2
    exit 127
  }
done

statuses="$(mktemp "${TMPDIR:-/tmp}/sprout-load-status.XXXXXX")"
body="$(mktemp "${TMPDIR:-/tmp}/sprout-load-body.XXXXXX")"
cleanup() {
  rm -f -- "${statuses}" "${body}"
}
trap cleanup EXIT

seq 1 "${requests}" | xargs -P "${concurrency}" -I '{}' \
  curl --silent --show-error --max-time 10 --output /dev/null --write-out '%{http_code}\n' \
  "${base_url%/}/health/live" >"${statuses}"
if ! awk '$1 != "200" { failures += 1 } END { exit failures > 0 }' "${statuses}"; then
  echo "Concurrent liveness load produced non-200 responses" >&2
  exit 1
fi

awk -v bytes="$((body_limit + 1))" 'BEGIN { for (i = 0; i < bytes; i++) printf "x" }' >"${body}"
quota_status="$(
  curl --silent --show-error --max-time 10 \
    --output /dev/null --write-out '%{http_code}' \
    --header 'Content-Type: application/json' \
    --data-binary "@${body}" \
    "${base_url%/}/v1/auth/passkeys/authenticate/start"
)"
if [[ "${quota_status}" != "413" ]]; then
  echo "Oversized request was not rejected with 413" >&2
  exit 1
fi

echo "Concurrent request load and body quota checks passed"
