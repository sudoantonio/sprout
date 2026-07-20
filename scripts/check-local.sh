#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repository_root}"

toolchain="1.88.0"
export RUSTC
export RUSTDOC
RUSTC="$(rustup which --toolchain "${toolchain}" rustc)"
RUSTDOC="$(rustup which --toolchain "${toolchain}" rustdoc)"
cargo_bin="$(rustup which --toolchain "${toolchain}" cargo)"

"${cargo_bin}" fmt --all --check
"${cargo_bin}" clippy --workspace --all-targets -- -D warnings
"${cargo_bin}" test --workspace

if [[ -s "${HOME}/.nvm/nvm.sh" ]]; then
  # shellcheck source=/dev/null
  source "${HOME}/.nvm/nvm.sh"
  nvm use --silent
fi

npm --prefix apps/web run lint
npm --prefix apps/web run test
npm --prefix apps/web run build

bash scripts/validate-migrations.sh
bash scripts/verify-requirements-traceability.sh
bash scripts/verify-systemd-hardening.sh
