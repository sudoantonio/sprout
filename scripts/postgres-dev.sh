#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 {start|stop|status|logs|destroy [--confirm]}" >&2
}

container="${SPROUT_POSTGRES_CONTAINER:-sprout-postgres-dev}"
volume="${SPROUT_POSTGRES_VOLUME:-sprout-postgres-data}"
image="${SPROUT_POSTGRES_IMAGE:-postgres:14-bookworm}"
host_port="${SPROUT_POSTGRES_PORT:-5432}"

if [[ ! "${container}" =~ ^[a-zA-Z0-9][a-zA-Z0-9_.-]*$ ]]; then
  echo "Invalid container name: ${container}" >&2
  exit 2
fi
if [[ ! "${volume}" =~ ^[a-zA-Z0-9][a-zA-Z0-9_.-]*$ ]]; then
  echo "Invalid volume name: ${volume}" >&2
  exit 2
fi
if [[ ! "${host_port}" =~ ^[0-9]+$ ]] || ((10#${host_port} < 1 || 10#${host_port} > 65535)); then
  echo "Invalid PostgreSQL host port: ${host_port}" >&2
  exit 2
fi

command="${1:-}"
case "${command}" in
  start)
    if docker container inspect "${container}" >/dev/null 2>&1; then
      if [[ "$(docker inspect --format '{{.State.Running}}' "${container}")" == "true" ]]; then
        echo "${container} is already running"
      else
        docker start "${container}" >/dev/null
      fi
    else
      docker volume create "${volume}" >/dev/null
      docker run --detach \
        --name "${container}" \
        --publish "127.0.0.1:${host_port}:5432" \
        --env POSTGRES_DB=sprout \
        --env POSTGRES_USER=sprout \
        --env POSTGRES_HOST_AUTH_METHOD=trust \
        --mount "type=volume,source=${volume},target=/var/lib/postgresql/data" \
        --health-cmd "pg_isready -U sprout -d sprout" \
        --health-interval 2s \
        --health-timeout 3s \
        --health-retries 15 \
        "${image}" >/dev/null
    fi

    for ((attempt = 1; attempt <= 30; attempt++)); do
      status="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}starting{{end}}' "${container}")"
      if [[ "${status}" == "healthy" ]]; then
        echo "PostgreSQL is ready at postgresql://sprout@127.0.0.1:${host_port}/sprout"
        exit 0
      fi
      if [[ "${status}" == "unhealthy" ]]; then
        echo "PostgreSQL container became unhealthy" >&2
        exit 1
      fi
      sleep 1
    done
    echo "Timed out waiting for PostgreSQL" >&2
    exit 1
    ;;
  stop)
    if docker container inspect "${container}" >/dev/null 2>&1; then
      docker stop "${container}" >/dev/null
      echo "Stopped ${container}; volume ${volume} was preserved"
    else
      echo "${container} does not exist"
    fi
    ;;
  status)
    if docker container inspect "${container}" >/dev/null 2>&1; then
      docker inspect --format 'state={{.State.Status}} health={{if .State.Health}}{{.State.Health.Status}}{{else}}not-configured{{end}}' "${container}"
    else
      echo "${container} does not exist"
    fi
    ;;
  logs)
    docker logs --tail 100 "${container}"
    ;;
  destroy)
    if [[ "${2:-}" != "--confirm" ]]; then
      echo "Refusing to remove the development database without --confirm" >&2
      exit 2
    fi
    if docker container inspect "${container}" >/dev/null 2>&1; then
      docker rm --force "${container}" >/dev/null
    fi
    if docker volume inspect "${volume}" >/dev/null 2>&1; then
      docker volume rm "${volume}" >/dev/null
    fi
    echo "Removed ${container} and ${volume}"
    ;;
  *)
    usage
    exit 2
    ;;
esac
