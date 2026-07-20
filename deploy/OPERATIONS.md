# Sprout operations

## Prerequisites

- Rust 1.88.0 and Cargo
- Node 22.12 or newer and npm
- Docker for the local PostgreSQL container and image builds
- PostgreSQL client tools for backup and restore
- `sqlx-cli` 0.8.6 when migrations are present
- `wasm-pack` 0.15.0 for PWA release builds
- `curl` for smoke and traceability checks

Docker Compose is not required.

## Local PostgreSQL

Start a loopback-only PostgreSQL 14 container:

```sh
scripts/postgres-dev.sh start
export DATABASE_URL=postgresql://sprout@127.0.0.1:5432/sprout
```

Inspect it with `scripts/postgres-dev.sh status` or
`scripts/postgres-dev.sh logs`. Stopping the container preserves its named
volume. Data removal requires the explicit
`scripts/postgres-dev.sh destroy --confirm` command.

The development container uses PostgreSQL trust authentication and publishes
only on `127.0.0.1`. Do not expose it on a shared or public host.

## Migrations

Migration files belong in `db/migrations` and use sqlx names such as
`0010_create_users.sql`.

```sh
scripts/validate-migrations.sh
scripts/migrate.sh --check
scripts/migrate.sh --apply
```

Applying migrations is always explicit. Set `DATABASE_URL` through the runtime
environment; do not put credentials in the repository.

## Build and install

The container build uses the committed Cargo lockfile and Rust 1.88.0:

```sh
docker build --tag sprout:local .
docker run --rm --read-only --cap-drop=ALL \
  --env-file /path/to/protected/sprout.env \
  --mount type=volume,source=sprout-blobs,target=/var/lib/sprout/blobs \
  --mount type=volume,source=sprout-archives,target=/var/lib/sprout/archives \
  --publish 127.0.0.1:8080:8080 \
  sprout:local
```

Build the PWA, including pinned `wasm-pack`, in its separate image:

```sh
docker build --file Dockerfile.web --tag sprout-web:local .
docker run --rm --read-only --cap-drop=ALL \
  --tmpfs /tmp --publish 127.0.0.1:4173:8080 sprout-web:local
```

For systemd, build a release binary and install it without starting the
service:

```sh
cargo build --locked --release --package sprout-server
sudo scripts/install-deploy.sh \
  --artifact target/release/sprout-server \
  --environment /path/to/protected/sprout.env
sudo systemctl enable --now sprout.service
sudo systemctl enable --now sprout-worker.service
```

The installer creates the non-login `sprout` account, installs all migrations
read-only under `/opt/sprout/migrations`, and owns only
`/var/lib/sprout/blobs` and `/var/lib/sprout/archives` for runtime writes.
Use `--restart` only for an intentional deployment to an already running
service. The installer keeps the previous binary as
`/opt/sprout/bin/sprout-server.previous`.

## Verification

The server contract used by CI is:

- `GET /health/live` reports process liveness.
- `GET /health/ready` verifies PostgreSQL readiness.
- A valid `x-request-id` request header is returned unchanged in the response.
- `traceparent` is accepted so request logs can be correlated by trace ID.
- `GET /internal/metrics` requires the `SPROUT_METRICS_TOKEN` bearer token and
  emits fixed-label Prometheus request, error, worker-lag, and quota metrics.

```sh
SPROUT_BASE_URL=http://127.0.0.1:8080 scripts/health-smoke.sh
SPROUT_BASE_URL=http://127.0.0.1:8080 scripts/verify-traceability.sh
SPROUT_METRICS_TOKEN=... scripts/verify-observability.sh
tests/system/run.sh
```

## Backup and restore

Use standard libpq environment variables or a protected `PGPASSFILE`. Backups
are owner-only signed consistency-set directories containing the PostgreSQL
custom dump, ciphertext blobs, ciphertext retention archives, metadata,
checksums, and a detached signature. Keep the private signing key outside all
backed-up directories. Client content keys are never required or included.
Quiesce both services before backup (or use coordinated database and filesystem
snapshots); the script signs one set of artifacts but cannot make live writes
across PostgreSQL and the filesystem atomic.

```sh
export PGHOST=127.0.0.1 PGPORT=5432 PGUSER=sprout PGDATABASE=sprout
scripts/backup-postgres.sh \
  --output-dir /secure/backups \
  --blobs-dir /var/lib/sprout/blobs \
  --archives-dir /var/lib/sprout/archives \
  --signing-key-file /secure/keys/backup-ed25519-private.pem
scripts/restore-postgres.sh \
  --backup-dir /secure/backups/sprout-YYYYmmddTHHMMSSZ \
  --blobs-dir /restore/sprout/blobs \
  --archives-dir /restore/sprout/archives \
  --verification-key-file /secure/keys/backup-ed25519-public.pem \
  --confirm
```

Restore does not drop, clean, or create a database. Restore into an empty,
explicitly selected database and empty blob/archive directories.

## Release evidence

Generate lockfile-derived SBOMs with `scripts/generate-sbom.sh`, then create a
signed immutable artifact manifest with `scripts/create-release-manifest.sh`.
`scripts/release-gate.sh` fails unless non-empty cryptographic-audit,
penetration-test, and threat-model evidence is explicitly supplied and covered
by that signed manifest. The repository does not claim that any independent
audit, penetration test, Safari test, or hardware WebAuthn test has occurred.
