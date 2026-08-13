# API and disposable validation guide

Sprout exposes a JSON API under `/v1`. User-authored semantic content must be
encrypted before it crosses the API boundary. The service authenticates the
caller from `Authorization: Bearer <session>`; creator/actor IDs are never
trusted from request bodies.

This document describes the implemented routes. It is not yet a generated
OpenAPI contract. The authoritative route registry remains
`apps/server/src/routes/mod.rs`, request/response types live in
`crates/api-contract`, and the browser client is in
`frontend/sprout-web/src/api/client.ts`.

## One-command validation

Requirements: Docker with Compose v2. No local Rust, Node, PostgreSQL, `jq`, or
`curl` installation is needed.

From the repository root:

```sh
docker compose -f compose.validation.yml up \
  --build \
  --abort-on-container-exit \
  --exit-code-from validation
```

That single command builds the API and the protocol-backed validation client,
starts a disposable PostgreSQL 14 database, applies migrations, and runs a
curl-based journey which:

1. registers two independent users through the email API;
2. decrypts development outbox tokens using the configured outbox key;
3. encrypts project/topic/list/task documents with `sprout-crypto-protocol`;
4. sends and retrieves ciphertext through the real HTTP API;
5. decrypts the returned task only with the retained DEK and expected context;
6. proves an unrelated user is denied, then accepts an invitation and reads
   the ciphertext as an authorized administrator;
7. proves a wrong key cannot decrypt the ciphertext;
8. races two authenticated task updates and requires exactly one `200` and one
   `409`;
9. scans a PostgreSQL dump for the classified plaintext canary.

Expected final output:

```text
HLT-12 encrypted API validation passed
T-LLR-12.1 authorization transition passed (...)
T-LLR-12.2 ciphertext round-trip, wrong-key denial, and plaintext scan passed
T-LLR-12.3 concurrent authenticated update returned exactly one commit
T-LLR-12.4 disposable Docker harness completed
```

Remove stopped containers and networks after validation:

```sh
docker compose -f compose.validation.yml down --volumes --remove-orphans
```

The PostgreSQL data directory is a `tmpfs`; validation data is not persisted.
The fixed keys in `compose.validation.yml` are public test fixtures and must
never be reused outside this disposable environment.

## Start the validation API for manual curl commands

```sh
docker compose -f compose.validation.yml up --build -d postgres api
curl --fail --silent http://localhost:18080/health/ready
```

The API is then available at `http://localhost:18080`.

Unauthenticated example:

```sh
curl --silent --output /dev/null --write-out '%{http_code}\n' \
  http://localhost:18080/v1/projects
```

Expected status: `401`.

Run the complete encrypted curl pipeline at any time:

```sh
docker compose -f compose.validation.yml run --build --rm validation
```

## Encrypt and decrypt with the protocol client

The helper is deliberately a validation tool. It links the same
`sprout-crypto-protocol` crate used by the application; it does not implement a
second ad-hoc cipher format.

Encrypt stdin:

```sh
RESOURCE_ID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
KEY_ID="$(uuidgen | tr '[:upper:]' '[:lower:]')"

printf '{"name":"encrypted task"}' |
  docker compose -f compose.validation.yml run --rm -T \
    --entrypoint sprout-validation-crypto validation \
    encrypt \
    --resource-id "$RESOURCE_ID" \
    --key-id "$KEY_ID" \
    --context "manual/task/$RESOURCE_ID" \
  > /tmp/sprout-encrypted.json

python3 -m json.tool /tmp/sprout-encrypted.json
```

`payload` is the object sent in API fields of type `EncryptedPayloadDto`.
`dek_b64` stays client-side and must not be sent to the service.

Decrypt the saved payload:

```sh
docker compose -f compose.validation.yml run --rm -T \
  --entrypoint sprout-validation-crypto validation \
  decrypt \
  --resource-id "$RESOURCE_ID" \
  --context "manual/task/$RESOURCE_ID" \
  < /tmp/sprout-encrypted.json
```

Changing the resource ID, context, key, nonce, or ciphertext makes
authentication fail.

## Authentication model

Public account-ceremony routes:

- `POST /v1/auth/email/verification/start`
- `POST /v1/auth/email/verification/finish`
- `POST /v1/auth/email/recovery/start`
- `POST /v1/auth/email/recovery/finish`
- `POST /v1/auth/passkeys/register/start`
- `POST /v1/auth/passkeys/register/finish`
- `POST /v1/auth/passkeys/authenticate/start`
- `POST /v1/auth/passkeys/authenticate/finish`

All other `/v1` routes require:

```http
Authorization: Bearer v1.<identity-id>.<session-id>.<secret>
```

The token is returned by a successful email or passkey ceremony. A client must
treat it as a secret and keep it out of URLs and logs.

## Implemented route inventory

Identity and devices:

- `POST /v1/auth/email/verification/{start,finish}`
- `POST /v1/auth/email/recovery/{start,finish}`
- `POST /v1/auth/passkeys/register/{start,finish}`
- `POST /v1/auth/passkeys/authenticate/{start,finish}`
- `GET|POST /v1/devices/{device_id}/key-packages`
- `DELETE /v1/devices/{device_id}/key-packages/{key_version}`
- `GET /v1/devices/{device_id}/key-transparency`

Projects, invitations, keys, and recovery:

- `GET|POST /v1/projects`
- `GET /v1/projects/{project_id}`
- `GET|POST /v1/projects/{project_id}/invitations`
- `POST /v1/projects/{project_id}/invitations/accept`
- `POST /v1/projects/{project_id}/participant-suggestions`
- `GET /v1/projects/{project_id}/device-key-packages`
- `POST /v1/projects/{project_id}/recovery-requests`
- `GET /v1/projects/{project_id}/recovery-requests/{request_id}`
- `POST /v1/projects/{project_id}/recovery-requests/{request_id}/approvals`
- `POST /v1/projects/{project_id}/recovery-requests/{request_id}/finalize`

Resources and permissions:

- `POST /v1/projects/{project_id}/resources`
- `GET /v1/projects/{project_id}/resources/{resource_id}`
- `GET|POST /v1/projects/{project_id}/resources/{resource_id}/permissions`
- `GET /v1/projects/{project_id}/resources/{resource_id}/permissions/{grant_id}/rotation-plan`
- `DELETE /v1/projects/{project_id}/resources/{resource_id}/permissions/{grant_id}`

Task domain:

- `GET|POST /v1/projects/{project_id}/topics`
- `GET|PUT|DELETE /v1/projects/{project_id}/topics/{topic_id}`
- `POST /v1/projects/{project_id}/resources/{resource_id}/epochs`
- `GET /v1/projects/{project_id}/resource-key-envelopes`
- `GET /v1/projects/{project_id}/resources/{resource_id}/envelope-plan`
- `POST /v1/projects/{project_id}/member-resource-keys`
- `GET|POST /v1/projects/{project_id}/topics/{topic_id}/task-lists`
- `GET|PUT|DELETE /v1/projects/{project_id}/task-lists/{list_id}`
- `GET /v1/projects/{project_id}/task-lists/{list_id}/tasks`
- `GET|POST /v1/projects/{project_id}/topics/{topic_id}/info-documents`
- `GET|POST /v1/projects/{project_id}/task-lists/{list_id}/info-documents`
- `GET|PUT|DELETE /v1/projects/{project_id}/info-documents/{document_id}`
- `POST /v1/projects/{project_id}/tasks`
- `GET|PUT|DELETE /v1/projects/{project_id}/tasks/{task_id}`
- `POST /v1/projects/{project_id}/tasks/{task_id}/{complete,copy,move}`
- `GET|POST /v1/projects/{project_id}/tasks/{task_id}/assignments`
- `DELETE /v1/projects/{project_id}/tasks/{task_id}/assignments/{assignment_id}`
- `POST /v1/projects/{project_id}/tasks/{task_id}/complete-assignment`
- `GET|POST /v1/projects/{project_id}/presets`
- `GET|DELETE /v1/projects/{project_id}/presets/{preset_id}`
- `POST /v1/projects/{project_id}/presets/{preset_id}/versions`
- `GET /v1/projects/{project_id}/presets/{preset_id}/versions/{version_id}`
- `POST /v1/projects/{project_id}/preset-assignments`
- `POST /v1/projects/{project_id}/preset-assignments/{assignment_id}/materialize`
- `POST /v1/projects/{project_id}/recurrence-series`
- `GET /v1/projects/{project_id}/recurrence-series/{series_id}`

Creating a topic, task list, or task also requires `epoch` and
`envelopes` in the request body. They register epoch one and signed hybrid
resource-key envelopes in the same database transaction as the resource.
Project roots are initialized immediately after project creation through the
`/resources/{resource_id}/epochs` route because the API allocates the root ID.
The envelope collection returns only active envelopes addressed to the
authenticated session's identity and device. The web client verifies the
sender package digest and both Ed25519/ML-DSA signatures before hybrid
unwrapping and local vault persistence.

After an invitation is accepted, a project manager can generate view grants
and per-device envelopes from the People screen. Domain envelopes are committed
with each hierarchical permission grant; the project-root key is shared only
after those grants succeed. The disposable journey proves that the invited
device independently unwraps its task key before decrypting the task payload.
It then revokes the hierarchical grant, rotates all affected resources to
epoch two, confirms that only remaining recipients receive new envelopes, and
proves the revoked device's old key cannot decrypt the new payload.

The rotation plan is manager-only metadata. It returns the affected resource
IDs, active epoch commitments, and remaining recipient identities needed to
construct exact envelope coverage. Revocation then commits all next epochs and
the permission removal in one transaction. Existing ciphertext keeps its
original `key_epoch`; a later edit must encrypt under the active epoch and
submit that epoch with the update.

Info documents form an ordered, recursively nested document tree within a
topic or task-list resource. PostgreSQL stores only container/parent UUIDs,
versions, epochs, tombstones, and one opaque payload per document. Markdown,
URLs, filenames, MIME types, block order, and child labels stay inside the
client-encrypted payload. The payload is protected by the container resource
key and binds both the document UUID and container kind into canonical AAD.

The validation image exposes matching helpers:

```bash
sprout-validation-crypto device-create --device-id "$DEVICE_ID"
sprout-validation-crypto initial-epoch \
  --project-id "$PROJECT_ID" \
  --resource-id "$RESOURCE_ID" \
  --recipient-identity-id "$IDENTITY_ID" \
  --recipient-device-id "$DEVICE_ID" < epoch-input.json
sprout-validation-crypto unwrap-envelope < envelope-input.json
```
- `POST /v1/projects/{project_id}/recurrence-series/{series_id}/archive`

Questionnaires:

- `GET|POST /v1/projects/{project_id}/questionnaires`
- `GET /v1/projects/{project_id}/questionnaires/{questionnaire_id}`
- `GET|POST /v1/projects/{project_id}/questionnaires/{questionnaire_id}/versions`
- `GET|PUT /v1/projects/{project_id}/questionnaires/{questionnaire_id}/versions/{version_id}`
- `POST /v1/projects/{project_id}/questionnaires/{questionnaire_id}/versions/{version_id}/publish`
- `GET|PUT /v1/projects/{project_id}/tasks/{task_id}/questionnaire-submission`
- `POST /v1/projects/{project_id}/tasks/{task_id}/questionnaire-submission/submit`

Files and attachments:

- `GET|POST /v1/projects/{project_id}/preset-versions/{version_id}/pretasks/{pretask_id}/attachments`
- `GET|POST /v1/projects/{project_id}/tasks/{task_id}/required-attachments`
- `GET|POST /v1/projects/{project_id}/tasks/{task_id}/completed-attachments`
- `POST /v1/projects/{project_id}/info-documents/{document_id}/files`
- `GET /v1/projects/{project_id}/files/{blob_id}`
- `GET|PUT /v1/projects/{project_id}/files/{blob_id}/content`

Synchronization and retention:

- `POST /v1/sync/{push,pull}`
- `GET /v1/sync/wake` (WebSocket upgrade)
- `GET|PUT /v1/retention/preferences`
- `GET /v1/retention/archives`
- `GET /v1/retention/archives/{archive_id}/download`
- `POST /v1/retention/archives/{archive_id}/receipt`

Collection and document request schemas are defined in
`crates/api-contract/src/lib.rs`. The current project and invitation handlers
still contain local DTOs, so CI must not claim a frozen public contract until a
generated OpenAPI document and drift test are added.

## Status codes

- `200`/`201`: successful read or mutation
- `202`: accepted account ceremony
- `204`: successful deletion/revocation
- `400`: malformed encrypted envelope or invalid command shape
- `401`: missing, invalid, expired, or revoked session
- `403`: authenticated actor lacks permission
- `404`: absent resource or intentionally non-disclosing authorization result
- `409`: stale version, idempotency collision, or invalid state transition
- `413`: request exceeds the configured body limit
- `429`: rate limit exceeded

## Scope of this harness

The harness proves encrypted transport, authenticated multi-user authorization,
wrong-key rejection, plaintext absence in PostgreSQL, and one real concurrent
business mutation.

It currently transfers the validation DEK to the invited validator out of band.
It therefore does **not** close `T-LLR-12.5` or `HLT-06`: a separate
three-client test must generate independent device packages, propagate
resource-key envelopes through the API, revoke a device, rotate the resource
epoch, and prove that the revoked device cannot decrypt the new revision.
