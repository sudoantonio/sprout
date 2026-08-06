<p align="center">
  <img src=".github/banner.png" alt="Sprout" width="820">
</p>

# Sprout

End-to-end encrypted task platform: a Rust/Axum backend, PostgreSQL metadata store, encrypted filesystem blobs, and a React/TypeScript offline-first PWA with shared Rust/WebAssembly cryptography.

> **Branch `frontend/split`:** il client web è in `frontend/sprout-web/`. Vedi [docs/frontend-split.md](docs/frontend-split.md) per la strategia multi-prodotto.

> **Not production-ready.** The implementation is currently a scaffold. The byte-level cryptographic protocol is not frozen, and no independent cryptographic audit or penetration test is claimed. Production is blocked on the gates in [the protocol specification](docs/crypto-protocol.md) and [operations guide](docs/operations.md).

## Features

- **End-to-end encryption** — semantic content, questionnaires, filenames, logical paths, and files are encrypted on the client before they reach the server.
- **Offline-first PWA** — installable web app with encrypted local storage (IndexedDB / OPFS), offline queue, and deterministic sync convergence.
- **Passkey authentication** — WebAuthn-based identity with per-device encryption keys.
- **Hierarchical permissions** — recursive grants and revocations across projects, topics, task lists, and tasks.
- **Tasks and questionnaires** — pretasks, recurrence, versioned questionnaires, and encrypted attachments.
- **Owner recovery** — `n-of-n` participant approval ceremony (requires unanimous active non-owner consent).

## Tech stack

| Layer | Technologies |
| --- | --- |
| Backend | Rust, Axum, SQLx, PostgreSQL (RLS) |
| Frontend | React 19, TypeScript, Vite, PWA |
| Cryptography | Rust crates compiled to WebAssembly (AES-GCM, ML-KEM, ML-DSA, X25519) |
| Testing | Cargo test, Vitest, Playwright, Docker Compose validation |

## Security model

- Semantic content is encrypted on the client; the service sees only restricted metadata (email, opaque IDs, membership, timestamps, sizes, sync activity).
- Keys are independent per resource/epoch and wrapped to authorized devices; there is no global project content key.
- Revocation protects future key epochs but cannot erase data or keys already downloaded.
- Owner recovery requires `n-of-n` active non-owner participants. One unavailable participant — or an owner-only project — makes recovery impossible.
- A PWA cannot defend against malicious JavaScript served by its own compromised origin. CSP, Trusted Types, first-party-only scripts, signed immutable artifacts, and reproducible builds reduce but do not remove this risk.

See also: [threat model](docs/threat-model.md) · [data classification](docs/data-classification.md) · [crypto protocol](docs/crypto-protocol.md)

## Prerequisites

- **Rust** 1.88 (pinned in `rust-toolchain.toml`; install via [rustup](https://rustup.rs/))
- **Node.js** 22.12+ (pinned in `.nvmrc`)
- **wasm-pack** 0.15.0 (for the browser crypto build)
- **Docker** (optional, for the disposable validation journey)
- **PostgreSQL** 16+ (for local server development)

## Getting started

### 1. Clone and install dependencies

```sh
git clone https://github.com/abaco-click/sprout.git
cd sprout
git checkout frontend/split

npm --prefix frontend/sprout-web install
```

### 2. Configure environment

```sh
cp .env.example .env
# Edit .env with local PostgreSQL credentials and generated keys
```

### 3. Run local checks

With the repository's declared Rust and Node toolchains installed:

```sh
bash scripts/check-local.sh
```

The check script selects the rustup compiler explicitly so a Homebrew Rust installation earlier on `PATH` cannot silently select a different compiler. These checks do not satisfy the independent cryptographic production gate.

### 4. Build the web client

```sh
npm --prefix frontend/sprout-web run wasm:build
npm --prefix frontend/sprout-web run build
```

### 5. Run the disposable encrypted API journey (Docker)

```sh
docker compose -f compose.validation.yml up \
  --build --abort-on-container-exit --exit-code-from validation
```

## Repository layout

```
sprout/
├── apps/
│   └── server/              # Axum API and worker composition
├── frontend/
│   └── sprout-web/          # React PWA (Sprout client)
├── crates/
│   ├── domain/              # Domain invariants
│   ├── application/         # Use cases and authorization
│   ├── storage-postgres/    # Persistence, transactions, RLS, migrations
│   ├── crypto-protocol/     # Versioned encrypted formats and suite adapters
│   ├── crypto-wasm/         # Minimal browser bindings
│   ├── api-contract/        # Shared API DTOs/types
│   ├── test-support/        # Integration-test infrastructure
│   └── validation-cli/      # Disposable protocol-backed API validation client
├── db/migrations/           # PostgreSQL schema migrations
├── docs/                    # Architecture, requirements, ADRs
├── scripts/                 # Local checks and validation scripts
└── tests/                   # System and traceability tests
```

## Documentation

- [Frontend split strategy](docs/frontend-split.md)
- [Architecture](docs/architecture.md)
- [Traceable requirements](docs/requirements.md)
- [Threat model](docs/threat-model.md)
- [Data classification](docs/data-classification.md)
- [Cryptographic protocol](docs/crypto-protocol.md)
- [Retention policy](docs/retention-policy.md)
- [Operations and production readiness](docs/operations.md)
- [API and disposable validation guide](docs/api.md)
- [Licensing and dependency allow-list](docs/licensing.md)
- [Architecture decision records](docs/adr/)

## CI

GitHub Actions runs on every push and pull request:

- **System tests** — full backend + PostgreSQL + Playwright journeys (`.github/workflows/system-tests.yml`)
- **Migrations** — schema migration validation (`.github/workflows/migrations.yml`)

## Contributing

This project is in early development. Before opening a pull request:

1. Run `bash scripts/check-local.sh` locally.
2. Ensure new behavior is traceable to a requirement in [docs/requirements.md](docs/requirements.md) when applicable.
3. Do not commit secrets, `.env` files, or generated WASM artifacts (`frontend/sprout-web/public/wasm/` is built at CI/deploy time).

## Contributors

Sprout was directed and integrated by [Francesco Antonio De Luca](https://github.com/francescoantoniodeluca).

A substantial share of the source code, tests, and technical documentation was generated with **OpenAI ChatGPT**, under human review and integration.

See [CONTRIBUTORS.md](CONTRIBUTORS.md) for the full contributor list.

## License

Licensed under either the [MIT License](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE), at your option.
