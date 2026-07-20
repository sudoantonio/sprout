# Sprout web

React/TypeScript offline-first PWA for Sprout's encrypted `/v1` API.

## Runtime requirements

- Node 22.12 or newer.
- `wasm-pack` 0.15.0 on `PATH` for the release build.
- A browser origin that satisfies the server WebAuthn RP ID and origin policy.

Build the Rust browser package before deploying:

```sh
npm run wasm:build
npm run build
```

`wasm:build` compiles `../../crates/crypto-wasm` into `public/wasm`. Generated
JavaScript and binaries are intentionally ignored by Git; CI must build and
publish them together with the Vite assets. The browser adapter refuses to
start unless the package exports:

`initialize`, `hash`, `canonicalHeader`, `encrypt`, `decrypt`,
`generateDevicePackage`, `signDual`, `verifyDual`, `wrapResourceKey`,
`unwrapResourceKey`, `splitRecoverySecretNOfN`, and
`combineRecoverySecretNOfN`.

## Security boundary

- Rust/WASM encrypts resource documents with resource/version-bound
  authenticated context before API or local persistence.
- IndexedDB contains encrypted payloads, dual-signed queue requests,
  ciphertext conflicts, tombstones, cursors, and PRF-wrapped vault records.
- OPFS is reserved for ciphertext attachment blobs under opaque identifiers.
- WebAuthn authenticates a user. It does not reveal encryption keys.
- When WebAuthn PRF output is available, HKDF derives a non-exportable AES-GCM
  key that wraps the local device vault. Without PRF, private material remains
  session-only and another authorized device or unanimous recovery is required
  after memory is cleared.
- Logout zeros reachable JavaScript byte arrays and releases non-exportable
  browser keys. JavaScript engines do not guarantee immediate erasure of all
  internal copies.
- The service worker caches only application assets, WASM, manifest, and
  artwork. It never handles `/v1` responses, encrypted records, attachments, or
  exports.
- Downloads are always user initiated. A downloaded ciphertext archive leaves
  browser-managed storage and becomes visible to operating-system history and
  backup policy.

Production hosting must send the HTML CSP as an HTTP response header.
`frame-ancestors` is not enforced from a meta policy.

## Current integration constraints

The UI uses the current server routes directly. These integration constraints
remain:

- Presets, questionnaires, and attachments have get/create routes but no
  collection routes, so their screens open resources by opaque ID.
- Sessions are intentionally memory-only. Offline edits and reads work after an
  active unlock; a full offline reload cannot run a server-backed passkey
  ceremony and therefore remains locked.

## Checks

```sh
npm run lint
npm test
npm run build
npm run test:e2e
npm audit --audit-level=high
```

Playwright is configured for its bundled Chromium, Firefox, and WebKit engines.
That matrix is not a Safari claim and does not by itself establish cross-browser
WebAuthn, PRF, persistence, or OPFS coverage.
