# PostgreSQL persistence

`migrations/` is the authoritative, forward-only Sprout schema. SQLx applies
files in numeric order and records them in `_sqlx_migrations`.

## Apply

The storage crate loads migrations at runtime, so building does not require a
database:

```rust
storage.migrate("db/migrations").await?;
```

For local administration with SQLx CLI:

```sh
sqlx migrate run --source db/migrations
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f db/tests/verify_schema.sql
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f db/tests/verify_behavior.sql
```

PostgreSQL 15 or newer is recommended. The migration role must be allowed to
create the `pgcrypto` extension, or an administrator must install it first.

## Security model

- Application connections use a role different from the migration/table-owner
  role. The table owner bypasses RLS on `projects` and `project_memberships` so
  the `SECURITY DEFINER` policy helpers can inspect those policy-root tables
  without recursion.
- User transactions set `app.identity_id`, `app.device_id`, and
  `app.project_id` through `PostgresStorage::begin`. RLS fails closed when the
  identity is missing.
- Cross-project workers use a separately provisioned PostgreSQL role with
  `BYPASSRLS`; no session variable can enable a bypass.
- RLS enforces identity/project boundaries. Topic, task-list, and task access
  levels are resolved separately by `resolve_permission`.
- Ciphertexts, signatures, hashes, and public keys are stored as `BYTEA`.
  PostgreSQL never receives plaintext domain payloads.

Grant application roles only the statements they need. In particular, do not
grant `UPDATE` or `DELETE` on append-only history tables. Schema triggers also
protect snapshots, completions, questionnaire responses, signed sync events,
and audit entries from mutation.

## Migration layout

1. Foundation functions and extensions
2. Identities, passkeys, devices, and sessions
3. Projects, memberships, invitations, and resource hierarchy
4. Encrypted domain entities, permissions, assignments, and recurrence
5. Presets, questionnaires, and file metadata
6. Device keys, resource epochs, envelopes, and n-of-n recovery
7. Signed sync events, idempotency, and snapshots
8. Retention, notifications, exports, audit, and outbox
9. Row-level security policies

Foreign keys intentionally use `RESTRICT`, including historical records.
Deletion is modeled with state/timestamp columns; retention jobs must make
explicit, policy-checked deletions rather than relying on cascading deletes.

## Operational notes

- Store and compare all instants as `timestamptz`; render them in UTC at system
  boundaries.
- A resource-parent trigger rejects cross-project parents and cycles, then
  rebuilds closure rows in the same transaction.
- Sync and audit chains take transaction-scoped advisory locks before checking
  their previous hash.
- Idempotency rows are intentionally deletable after expiry. Signed events and
  snapshots remain append-only.
- Recovery sets can become active only after all declared shares exist, and
  `threshold = share_count` enforces n-of-n recovery.
