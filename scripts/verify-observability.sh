#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 [--base-url URL]" >&2
}

base_url="${SPROUT_BASE_URL:-http://127.0.0.1:8080}"
while (($# > 0)); do
  case "$1" in
    --base-url) base_url="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage; exit 2 ;;
  esac
done
if [[ ! "${base_url}" =~ ^https?://[^[:space:]]+$ ]]; then
  echo "Base URL must be an HTTP(S) URL without whitespace" >&2
  exit 2
fi
: "${SPROUT_METRICS_TOKEN:?Set SPROUT_METRICS_TOKEN for the protected metrics smoke test}"
command -v curl >/dev/null || {
  echo "curl is required" >&2
  exit 127
}

curl --fail --silent --show-error --max-time 5 \
  --output /dev/null "${base_url%/}/health/live"
curl --fail --silent --show-error --max-time 5 \
  --output /dev/null "${base_url%/}/health/ready"

unauthorized_status="$(
  curl --silent --show-error --max-time 5 \
    --output /dev/null --write-out '%{http_code}' \
    "${base_url%/}/internal/metrics"
)"
if [[ "${unauthorized_status}" != "401" ]]; then
  echo "Metrics endpoint did not reject a missing bearer token" >&2
  exit 1
fi

metrics="$(
  curl --fail --silent --show-error --max-time 5 \
    --header "Authorization: Bearer ${SPROUT_METRICS_TOKEN}" \
    "${base_url%/}/internal/metrics"
)"
for metric in \
  sprout_http_requests_total \
  sprout_http_errors_total \
  sprout_rate_limit_rejections_total \
  sprout_quota_rejections_total \
  sprout_worker_lag_seconds; do
  if [[ "${metrics}" != *"${metric}"* ]]; then
    echo "Missing Prometheus metric: ${metric}" >&2
    exit 1
  fi
done
if [[ "${metrics}" == *"${SPROUT_METRICS_TOKEN}"* ]]; then
  echo "Metrics output disclosed its bearer token" >&2
  exit 1
fi

echo "Protected health and redacted Prometheus metrics passed"
