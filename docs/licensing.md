# Licensing and dependency policy

## Project license

Except where a file states otherwise, Sprout source and documentation are offered under either:

- the MIT License in [`../LICENSE-MIT`](../LICENSE-MIT); or
- the Apache License 2.0 in [`../LICENSE-APACHE`](../LICENSE-APACHE),

at the recipient's option. The SPDX expression is:

```text
MIT OR Apache-2.0
```

Contributions are accepted under that same dual license unless an explicit, reviewed agreement says otherwise. Third-party components retain their own notices and licenses.

This policy is an engineering control, not legal advice. Ambiguous ownership, license text, linking obligations, patent terms, cryptographic export restrictions, or distribution terms require qualified legal review.

## Dependency allow-list

No dependency may be added merely because it is technically suitable. The resolved production graph, build tooling distributed with artifacts, vendored code, browser assets, fonts, and copied snippets are in scope.

| License family | Default decision | Conditions |
| --- | --- | --- |
| MIT, Apache-2.0 | Allow | Preserve notices; verify actual package/source license |
| BSD-2-Clause, BSD-3-Clause, ISC, Zlib | Allow | Preserve notices and attribution |
| PostgreSQL License | Allow | Preserve license/notice |
| MPL-2.0 | Review required | Record file-level copyleft and distribution/update process |
| LGPL (any version) | Review required | Record linking, relinking, source/notice, and platform implications |
| GPL/AGPL (any version) | Block pending explicit legal/architecture approval | Assess combined-work and network-copyleft obligations |
| SSPL, Commons Clause, BUSL/source-available, non-commercial, field-of-use restrictions, unknown/custom | Deny by default | Exception requires written legal and project-owner approval |
| Unlicensed or unverifiable | Deny | Replace or obtain clear permission |

An exception record identifies the exact package/version, transitive impact, shipped artifact, rationale, obligations, owner, reviewer, and expiry/re-review trigger. An exception is not a general approval for later versions.

## Admission checklist

Before merging a dependency or material upgrade:

1. demonstrate that standard library or an already approved package is insufficient;
2. verify package identity, source repository, maintainers, release provenance, and license from source—not only registry metadata;
3. inspect the complete resolved transitive graph and enabled features;
4. record license, notice obligations, cryptographic role, network/build scripts, native code, and runtime privilege;
5. assess maintenance, advisories, unsafe code, parser attack surface, bundle size, and reproducibility;
6. pin through the ecosystem lockfile and update SBOM/notice outputs;
7. obtain explicit review for cryptographic, authentication, serialization, database-driver, or build-chain dependencies.

Cryptographic and secret-sharing packages also require stable APIs/formats, known-answer vectors, native/WASM parity where applicable, versioned adapters, and independent-review evidence before production use. “Popular” is not equivalent to audited.

## Automated release checks

CI must:

- evaluate Rust sources and advisories with `cargo-deny` (or an approved equivalent);
- inspect npm's resolved production and build graph for advisories and licenses;
- reject unknown, denied, or unreviewed licenses;
- reject unresolved critical/high vulnerabilities;
- verify lockfiles are committed and unchanged by a frozen install/build;
- generate SBOMs and third-party notice/license reports from the release graph;
- compare reports against time-bounded exceptions.

Tool output is evidence, not legal interpretation. Package metadata conflicts and dual/multi-license expressions require source verification.

## Notices and artifacts

Release artifacts include the appropriate project license texts plus generated third-party notices. Source archives retain copyright and SPDX headers where present. Minification, WASM compilation, static linking, containerization, or vendoring does not remove notice/source obligations.

Security audit reports, trademarks, logos, user-provided content, and deployment-specific configuration are not automatically relicensed by the project license. No independent security or cryptographic audit is represented by including this policy.
