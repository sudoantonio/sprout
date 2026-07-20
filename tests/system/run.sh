#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "${script_dir}/../.." && pwd)"

echo "Running health smoke test"
"${repository_root}/scripts/health-smoke.sh"

echo "Running request traceability test"
"${repository_root}/scripts/verify-traceability.sh"

echo "Running T-LLR-10.7 protected observability and redaction smoke"
"${repository_root}/scripts/verify-observability.sh"

echo "Running bounded concurrent load and quota test"
"${script_dir}/load-smoke.sh"

echo "System tests passed"
