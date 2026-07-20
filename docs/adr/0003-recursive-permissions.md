# ADR-0003: Origin-aware recursive permissions

- Status: Accepted
- Date: 2026-07-18

## Context

Projects contain topic → task-list → task hierarchies. Access granted to an ancestor must cover current and future descendants. Access granted only to a descendant must permit navigation through ancestors without exposing siblings or ancestor bodies. Access can arise independently from manual grant, ownership, creation, or assignment; revoking one source must not erase another.

## Decision

Represent hierarchy with project-scoped `resource_nodes` and a closure relation that rejects cycles. Use domain-specific topic, task-list, and task permission records rather than a generic domain ACL. Each record stores:

- project and resource;
- subject;
- effective visibility (`full` or `container_only`);
- origin type and origin/root identifier;
- creation/revocation state and applicable epoch.

Effective access is the union of valid origins. Ancestor full access propagates to its subtree, including descendants created later. Descendant access creates minimum `container_only` ancestor visibility and never sibling visibility. Removing list access preserves an independently assigned task, its completion right, required container ancestry, and an administrative warning.

Application policy and PostgreSQL RLS enforce the same project/resource scope. Composite project FKs prevent cross-project edges. Permission updates, key-envelope effects, domain changes, events, and outbox records are transactional.

## Consequences

- Revocation can target one causal grant without destroying independent access.
- Queries and tests are more complex than a single inherited role.
- Closure/propagation writes require concurrency and cycle controls.
- `container_only` leaks ancestor existence and minimum header/routing metadata by design, but no sibling counts/names/bodies/downloads.
- Cryptographic envelope delivery must remain consistent with effective permission epochs; database visibility alone is insufficient.

Property tests must randomize trees and grant/revoke order, while API, direct-SQL/RLS, and cryptographic tests verify the same visibility matrix.

## Rejected alternatives

- **Materialized effective ACL without origins:** rejected because revocation cannot preserve independent causes reliably.
- **Compute all ancestry recursively on each request:** rejected as the only mechanism because later-descendant propagation, auditability, and RLS verification become harder.
- **Expose full ancestors for navigation:** rejected because it leaks unrelated content.
- **Per-project global content key:** rejected because any member could decrypt resources outside their effective permission.
