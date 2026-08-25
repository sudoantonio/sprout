# Data classification and encrypted-payload boundary

## Policy

Classification is based on meaning, not type or field name. New fields default to **Sensitive content** until a documented review moves them. The client must encrypt sensitive content before transport or persistence outside trusted client memory. Server code, contracts, queries, telemetry, and administrative tools must not require its plaintext.

## Classes

| Class | Examples | Allowed locations | Rules |
| --- | --- | --- | --- |
| **Public** | Published documentation, protocol version identifiers, static assets | Anywhere | Integrity still required |
| **Service-visible restricted metadata** | Normalized email; opaque user/device/project/resource IDs; membership and permission relations; creator/assignee IDs; operational timestamps; versions/epochs; cursors, idempotency keys, tombstones; sizes, hashes of ciphertext; retention/export status | API memory, PostgreSQL, minimally in logs/metrics, encrypted backups | Minimize, access-control, retention-limit, never repurpose as semantic content |
| **Sensitive content** | Names/phone; project/topic/list/task titles and bodies; info-document Markdown, block order and child labels; priorities/deadlines/recurrence details; questionnaire questions/options/answers; filenames, logical paths, manifests; comments; semantic links; archive contents | Trusted client memory in plaintext; authenticated ciphertext in client/server storage and transport | Encrypt client-side; no server-side search/filter/order by meaning |
| **Minimum container header** | Minimum label or navigation data needed to show an ancestor | Client plaintext only when a `container_only` key is authorized; ciphertext elsewhere | Separate key; must reveal no body, child, sibling, counts, or download data |
| **Cryptographic secret** | DEKs, per-resource KEKs, device private keys, owner wrapping key, recovery secret/shares, passkey private material, plaintext nonces before use | Trusted client/secure authenticator memory and approved encrypted client storage | Never server-side plaintext; never log/telemetry; zeroize where practical; tightly scope lifetime |
| **Authentication secret/token** | WebAuthn challenge, session token, email verification/recovery token | Only the component completing the ceremony; hashed/encrypted at rest as appropriate | Single-use, expiring, origin/session bound, redacted |
| **Encrypted artifact** | AEAD payload/blob, key envelope, signed event, encrypted archive, encrypted local cache | Client, network, PostgreSQL, blob/archive filesystem, backups | Validate version, bounds, signature/hash, AAD, and authorization before use |
| **Prohibited** | Sensitive plaintext in server persistence, logs, metrics, traces, backups, archives, service-worker caches; private/content keys server-side; client local paths in sync | Nowhere listed | Release-blocking incident |

Email is intentionally service-visible for identity and invitation delivery. Names and phone numbers are not. Creator and assignee IDs are visible, while their semantic task context is encrypted.

## Location matrix

| Surface | Restricted metadata | Sensitive plaintext | Keys/shares plaintext | Ciphertext |
| --- | ---: | ---: | ---: | ---: |
| PWA runtime after authorization | Yes | Yes, transient | Yes, transient | Yes |
| IndexedDB / OPFS | Minimized | **No** | **No**, unless wrapped under approved client protection | Yes |
| Service worker cache | Public/static only | **No** | **No** | Only explicitly reviewed opaque responses |
| HTTPS/WSS request bodies | Yes | **No** | **No**; public keys/envelopes only | Yes |
| API/worker memory | Yes | **No** | **No** | Yes |
| PostgreSQL | Yes | **No** | **No**; encrypted recovery shares/envelopes only | Yes |
| Blob/archive filesystem | Opaque IDs and minimum manifest metadata | **No** | **No** | Yes |
| Logs, traces, metrics | Allow-listed minimum | **No** | **No** | Avoid payloads; opaque IDs only when needed |
| Server backup | Yes, protected operationally | **No** | **No** | Yes |
| Per-user export | Encrypted manifest | **No** server-side | **No** server-side | Yes |

## Boundary contract

Client-to-server DTOs may contain:

- opaque identifiers and project/resource relationships needed for routing and policy;
- protocol/suite/key epoch and payload version;
- ciphertext, nonce, authenticated tag, key envelopes, signatures, hashes;
- base version, device-chain predecessor, cursor, idempotency key;
- byte size, media size limits, retention state, and operational timestamps.

They must not contain semantic titles, descriptions, questionnaire material, filenames, logical paths, decrypted filter/sort values, private keys, DEKs/KEKs, recovery secrets/shares in plaintext, or decrypted archive manifests.

The API rejects unknown fields on security-sensitive protocol objects, validates sizes before allocation, and treats client MIME declarations as untrusted. Any field proposed for server-side filtering must receive a classification and threat-model review first.

## Metadata leakage and minimization

E2EE does not conceal account and collaboration graphs, resource hierarchy/routing, creator/assignee links, timing, event frequency/order, protocol epochs, ciphertext sizes, IP/network data, or retention/export activity. Even opaque identifiers can be correlated. Bucket padding can reduce exact-size leakage but does not hide timing or access patterns.

Minimization requirements:

1. collect only data required for identity, routing, authorization, sync, abuse controls, and retention;
2. use opaque identifiers and avoid semantic labels;
3. avoid payloads and high-cardinality user data in telemetry;
4. restrict operator access and record administrative actions;
5. delete operational metadata under an approved schedule;
6. document user-visible metadata exposure without claiming anonymity.

## Enforcement

- Contract/schema review checks every field against this file.
- Tests seed unique plaintext canaries and scan PostgreSQL, filesystems, caches, logs, metrics, backups, and exports.
- Browser tests inspect IndexedDB, OPFS, Cache Storage, requests, and download behavior.
- Logging uses an explicit allow-list; debug serialization of requests, cryptographic structs, and archive manifests is forbidden.
- A detected prohibited value blocks release, triggers incident handling, removes exposed artifacts where possible, and requires root-cause review.
