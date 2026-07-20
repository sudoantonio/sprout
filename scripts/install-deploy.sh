#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: install-deploy.sh --artifact FILE [--environment FILE]
                         [--worker-environment FILE]
                         [--migrations DIRECTORY]
                         [--unit-dir DIR] [--restart]

Installs without enabling or starting the services. --restart only restarts
already-running sprout.service and sprout-worker.service.
EOF
}

artifact=""
environment_file=""
worker_environment_file=""
migrations_dir=""
prefix="/opt/sprout"
unit_dir="/etc/systemd/system"
restart="false"
while (($# > 0)); do
  case "$1" in
    --artifact)
      artifact="${2:-}"
      shift 2
      ;;
    --environment)
      environment_file="${2:-}"
      shift 2
      ;;
    --worker-environment)
      worker_environment_file="${2:-}"
      shift 2
      ;;
    --migrations)
      migrations_dir="${2:-}"
      shift 2
      ;;
    --unit-dir)
      unit_dir="${2:-}"
      shift 2
      ;;
    --restart)
      restart="true"
      shift
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

if ((EUID != 0)); then
  echo "Run this installer as root" >&2
  exit 1
fi
if [[ -z "${artifact}" || ! -f "${artifact}" || ! -r "${artifact}" ]]; then
  echo "--artifact must name a readable regular file" >&2
  exit 2
fi
if [[ -n "${environment_file}" && (! -f "${environment_file}" || ! -r "${environment_file}") ]]; then
  echo "--environment must name a readable regular file" >&2
  exit 2
fi
if [[ -n "${worker_environment_file}" && (! -f "${worker_environment_file}" || ! -r "${worker_environment_file}") ]]; then
  echo "--worker-environment must name a readable regular file" >&2
  exit 2
fi
if [[ "${unit_dir}" != /* || "${unit_dir}" == "/" ]]; then
  echo "The systemd unit directory must be absolute and cannot be /" >&2
  exit 2
fi

for required_command in getent groupadd id install mktemp mv rm systemctl useradd; do
  command -v "${required_command}" >/dev/null || {
    echo "${required_command} is required" >&2
    exit 127
  }
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
migrations_dir="${migrations_dir:-${script_dir}/../db/migrations}"
source_unit="${script_dir}/../deploy/sprout.service"
source_worker_unit="${script_dir}/../deploy/sprout-worker.service"
if [[ ! -f "${source_unit}" || ! -f "${source_worker_unit}" ]]; then
  echo "Missing systemd unit: ${source_unit} or ${source_worker_unit}" >&2
  exit 1
fi
if [[ ! -d "${migrations_dir}" ]]; then
  echo "Migration directory is missing: ${migrations_dir}" >&2
  exit 1
fi
shopt -s nullglob
migration_files=("${migrations_dir}"/*.sql)
shopt -u nullglob
if ((${#migration_files[@]} == 0)); then
  echo "Migration directory contains no SQL files: ${migrations_dir}" >&2
  exit 1
fi

if ! getent group sprout >/dev/null; then
  groupadd --system sprout
fi
if ! id -u sprout >/dev/null 2>&1; then
  useradd --system --gid sprout --home-dir /var/lib/sprout \
    --shell /usr/sbin/nologin sprout
fi

binary_dir="${prefix%/}/bin"
migrations_destination="${prefix%/}/migrations"
migrations_previous="${prefix%/}/migrations.previous"
destination="${binary_dir}/sprout-server"
previous="${binary_dir}/sprout-server.previous"
unit_destination="${unit_dir%/}/sprout.service"
worker_unit_destination="${unit_dir%/}/sprout-worker.service"
install -d -m 0755 "${binary_dir}" "${unit_dir}"
install -d -o sprout -g sprout -m 0700 \
  /var/lib/sprout /var/lib/sprout/blobs /var/lib/sprout/archives

binary_temp="$(mktemp "${binary_dir}/.sprout-server.XXXXXX")"
migrations_temp="$(mktemp -d "${prefix%/}/.migrations.XXXXXX")"
unit_temp="$(mktemp "${unit_dir%/}/.sprout.service.XXXXXX")"
worker_unit_temp="$(mktemp "${unit_dir%/}/.sprout-worker.service.XXXXXX")"
environment_temp=""
worker_environment_temp=""
previous_temp=""
cleanup() {
  rm -f -- "${binary_temp}" "${unit_temp}" "${worker_unit_temp}"
  rm -rf -- "${migrations_temp}"
  if [[ -n "${environment_temp}" ]]; then
    rm -f -- "${environment_temp}"
  fi
  if [[ -n "${worker_environment_temp}" ]]; then
    rm -f -- "${worker_environment_temp}"
  fi
  if [[ -n "${previous_temp}" ]]; then
    rm -f -- "${previous_temp}"
  fi
}
trap cleanup EXIT

install -m 0755 "${artifact}" "${binary_temp}"
for migration in "${migration_files[@]}"; do
  install -m 0644 "${migration}" "${migrations_temp}/${migration##*/}"
done
install -m 0644 "${source_unit}" "${unit_temp}"
install -m 0644 "${source_worker_unit}" "${worker_unit_temp}"

if [[ -e "${destination}" ]]; then
  previous_temp="$(mktemp "${binary_dir}/.sprout-server.previous.XXXXXX")"
  install -m 0755 "${destination}" "${previous_temp}"
  mv -f -- "${previous_temp}" "${previous}"
  previous_temp=""
fi
mv -f -- "${binary_temp}" "${destination}"
if [[ -e "${migrations_previous}" ]]; then
  rm -rf -- "${migrations_previous}"
fi
if [[ -e "${migrations_destination}" ]]; then
  mv -- "${migrations_destination}" "${migrations_previous}"
fi
mv -- "${migrations_temp}" "${migrations_destination}"
mv -f -- "${unit_temp}" "${unit_destination}"
mv -f -- "${worker_unit_temp}" "${worker_unit_destination}"

if [[ -n "${environment_file}" ]]; then
  install -d -o root -g sprout -m 0750 /etc/sprout
  environment_temp="$(mktemp /etc/sprout/.sprout.env.XXXXXX)"
  install -o root -g sprout -m 0640 "${environment_file}" "${environment_temp}"
  mv -f -- "${environment_temp}" /etc/sprout/sprout.env
  environment_temp=""
fi
if [[ -n "${worker_environment_file}" ]]; then
  install -d -o root -g sprout -m 0750 /etc/sprout
  worker_environment_temp="$(mktemp /etc/sprout/.sprout-worker.env.XXXXXX)"
  install -o root -g sprout -m 0640 "${worker_environment_file}" "${worker_environment_temp}"
  mv -f -- "${worker_environment_temp}" /etc/sprout/sprout-worker.env
  worker_environment_temp=""
fi

systemctl daemon-reload
if [[ "${restart}" == "true" ]]; then
  if systemctl is-active --quiet sprout.service; then
    systemctl restart sprout.service
  else
    echo "sprout.service is not running; it was not started"
  fi
  if systemctl is-active --quiet sprout-worker.service; then
    systemctl restart sprout-worker.service
  else
    echo "sprout-worker.service is not running; it was not started"
  fi
fi

trap - EXIT
echo "Installed ${destination}, ${unit_destination}, and ${worker_unit_destination}"
