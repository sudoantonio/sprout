#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 [--base-url URL] [--path PATH] [--attempts COUNT] [--delay SECONDS]" >&2
}

base_url="${SPROUT_BASE_URL:-http://127.0.0.1:8080}"
health_path="${SPROUT_HEALTH_PATH:-/health/ready}"
attempts="${SPROUT_HEALTH_ATTEMPTS:-30}"
delay="${SPROUT_HEALTH_DELAY_SECONDS:-1}"

while (($# > 0)); do
  case "$1" in
    --base-url)
      base_url="${2:-}"
      shift 2
      ;;
    --path)
      health_path="${2:-}"
      shift 2
      ;;
    --attempts)
      attempts="${2:-}"
      shift 2
      ;;
    --delay)
      delay="${2:-}"
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
if [[ "${health_path}" != /* ]]; then
  echo "Health path must start with /" >&2
  exit 2
fi
if [[ ! "${attempts}" =~ ^[1-9][0-9]*$ ]]; then
  echo "Attempts must be a positive integer" >&2
  exit 2
fi
if [[ ! "${delay}" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  echo "Delay must be a non-negative number" >&2
  exit 2
fi

command -v curl >/dev/null || {
  echo "curl is required" >&2
  exit 127
}

url="${base_url%/}${health_path}"
for ((attempt = 1; attempt <= 10#${attempts}; attempt++)); do
  if curl \
    --fail \
    --silent \
    --show-error \
    --max-time 5 \
    --output /dev/null \
    "${url}"; then
    echo "Health check passed"
    exit 0
  fi
  if ((attempt < 10#${attempts})); then
    sleep "${delay}"
  fi
done

echo "Health check failed after ${attempts} attempt(s)" >&2
exit 1
