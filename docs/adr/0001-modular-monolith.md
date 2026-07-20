# ADR-0001: Rust modular monolith

- Status: Accepted
- Date: 2026-07-18

## Context

Identity, recursive authorization, encrypted synchronization, task invariants, retention, and file metadata frequently change in one transaction. Early service decomposition would add distributed transactions, duplicated authorization, protocol/version coordination, and operational overhead before independent scaling boundaries are known.

## Decision

Build one Rust workspace as a modular monolith with strict dependency boundaries:

- domain invariants are independent of HTTP and storage;
- application use cases own authorization and transaction orchestration;
- API contracts, PostgreSQL adapters, and cryptographic protocol are separate crates;
- `apps/server` is composition/configuration only;
- API and worker are independently runnable processes over shared application/domain modules.

Modules communicate through typed internal interfaces, not private-table access. Domain changes, permission propagation, events, and outbox records commit atomically in PostgreSQL.

## Consequences

- Cross-module invariants remain locally testable and transactional.
- Deployment, migration, tracing, and incident response begin with fewer moving parts.
- API and worker can scale/restart separately.
- Poorly enforced module boundaries could become tight coupling; dependency-direction and architecture tests are required.
- A fault in the shared codebase can affect multiple modules, so process isolation, bounded work, and defensive parsing remain necessary.

## Rejected alternatives

- **Initial microservices:** rejected because distributed authorization/transactions and larger attack/operations surfaces outweigh unproven scaling benefits.
- **Single undifferentiated crate:** rejected because it obscures trust boundaries and makes later extraction unsafe.
- **Serverless functions per use case:** rejected because transaction, WebSocket/sync, worker lease, filesystem, and reproducible-deployment requirements fit long-running services better.

Extraction of a module requires measured isolation/scaling need, versioned contracts, equivalent authorization, and a new ADR.
