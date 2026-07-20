#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 [--base-url URL] [--path PATH]" >&2
}

base_url="${SPROUT_BASE_URL:-http://127.0.0.1:8080}"
request_path="${SPROUT_TRACE_PATH:-${SPROUT_HEALTH_PATH:-/health/ready}}"
while (($# > 0)); do
  case "$1" in
    --base-url)
      base_url="${2:-}"
      shift 2
      ;;
    --path)
      request_path="${2:-}"
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

if [[ ! "${base_url}" =~ ^https?:// ]] || [[ "${base_url}" =~ ^https?://[^/]*@ ]]; then
  echo "Base URL must be HTTP(S) and must not contain credentials" >&2
  exit 2
fi
if [[ "${request_path}" != /* ]]; then
  echo "Request path must start with /" >&2
  exit 2
fi

for required_command in curl od tr awk mktemp; do
  command -v "${required_command}" >/dev/null || {
    echo "${required_command} is required" >&2
    exit 127
  }
done

random_hex() {
  local byte_count="$1"
  od -An -N"${byte_count}" -tx1 /dev/urandom | tr -d ' \n'
}

trace_id="$(random_hex 16)"
span_id="$(random_hex 8)"
request_id="sprout-smoke-${trace_id}"
traceparent="00-${trace_id}-${span_id}-01"
headers_file="$(mktemp "${TMPDIR:-/tmp}/sprout-trace-headers.XXXXXX")"
body_file="$(mktemp "${TMPDIR:-/tmp}/sprout-trace-body.XXXXXX")"

cleanup() {
  rm -f -- "${headers_file}" "${body_file}"
}
trap cleanup EXIT

curl \
  --fail \
  --silent \
  --show-error \
  --max-time 10 \
  --dump-header "${headers_file}" \
  --output "${body_file}" \
  --header "x-request-id: ${request_id}" \
  --header "traceparent: ${traceparent}" \
  "${base_url%/}${request_path}"

response_request_id="$(
  awk -F ': *' '
    tolower($1) == "x-request-id" {
      sub(/\r$/, "", $2)
      print $2
      exit
    }
  ' "${headers_file}"
)"

if [[ "${response_request_id}" != "${request_id}" ]]; then
  echo "Response did not preserve x-request-id" >&2
  exit 1
fi

echo "Traceability check passed request_id=${request_id} trace_id=${trace_id}"
