# Checkpoint 0032 — client-owned multi-provider inference evidence

Date: 2026-08-23

## Chain of custody and scope

- starting HEAD: `2982cd6071db9e36cde4913d9f6c52b0876d7f9c`;
- branch: `codex/lean-concrete-refinement`;
- canonical specification: `Sprout_AgentSpec_R5_no_model_memory_draft.lean`;
- Lean SHA-256 before and after the checkpoint:
  `c9946730d79c534811ceffb4fc6c05035d93b3a34a61a7a014e533075b1d1b32`;
- normative boundary inherited from 0031: `StructuredLanguageModelRuntimeBoundary`,
  `StateGroundedModelInvocationCertificate`, `R540ModelRuntimeProjection`,
  `R540ModelInvocationEventExact`, and the R5.41 model/interrogation/proxy
  surface gates;
- implemented task kinds: `answerFromAuthorizedContext` and
  `interpretProxyRequest` only.

Lean was not modified. No provider SDK, API credential, model identifier,
provider URL, DS4/Ollama endpoint, TLS pin, VPN configuration, privacy-model
state, or plaintext AI input/output was added to the Sprout backend schema or
configuration.

## Server-blind architecture

The production boundary is:

```text
Sprout exact encrypted dispatch
  -> authorized device/edge TCB
  -> device-local profile and provider adapter
  -> exact serialized provider request
  -> strict structured validation and grounding
  -> encrypted output + dual-signed actual observation
  -> Sprout exact actual/projection verification
```

The server owns current authorization, exact ordered context references,
attempt/lease state, bounds, replay fencing and the R5.40/R5.41 projection. The
device owns provider selection, model, credential, endpoint, plaintext,
network transport and output encryption. The backend never calls an inference
provider and has no outbound AI client dependency.

The browser is a control plane. `LocalEdgeInferenceBridge` defines the native
user-owned transport contract, while direct browser inference is always
disabled because Node `fetch` success is not evidence of browser CORS or safe
credential handling. No Sprout-backend proxy exists.

**Current classification:** the provider adapters and edge contract are
concretely implemented and live-tested, but Mode A/B are not yet production
E2E because no native local-edge companion implementing the bridge is shipped
in this checkpoint.

## Trusted runtime and no-downgrade binding

Migration 0032 adds only provider-neutral witnesses:

- `agent_invocations.required_runtime_kind` is fixed at queue time by the
  authenticated server route;
- the legacy claim route selects only `legacy_0031` invocations;
- the client-provider claim route selects only `client_provider_v1` and binds
  an opaque `execution_profile_commitment` into the immutable dispatch before
  any provider request;
- submit/fail require dispatch runtime, signed observation runtime and
  projection runtime to match;
- successful client-provider execution requires a non-NULL exact endpoint
  request commitment; legacy NULL records remain readable but are not promoted.

The PostgreSQL E2E queues a client-provider invocation, proves that legacy
claim creates no lease/dispatch, then claims successfully through the exact
runtime route. Profile A at claim cannot be completed with profile B in the
observation/projection.

## Hiding execution-profile commitment

The profile witness is HMAC-SHA-256 with the domain
`sprout-client-provider-execution-profile-v1`, a random 32-byte device-only
secret, a persistent local profile revision and the complete local profile.
This prevents practical offline enumeration of a small provider/model/URL
space. The secret and revision are stored inside the existing PRF-wrapped local
KeyVault, survive restart, rotate when the profile changes, and are deleted by
“Elimina configurazione AI da questo dispositivo”. They are never synchronized
or included in the DEV vault export.

The server receives only the opaque 32-byte commitment. It cannot infer or
validate provider, model, endpoint or credential.

## Exact semantic wire request

The edge commits to the request after adapter serialization:

- versioned protocol identity;
- HTTP method and path;
- exact selected model present in the wire body;
- exact UTF-8 JSON body, including structured-output controls;
- every non-secret semantic header, fixed by the versioned protocol manifest.

The fixed header manifest covers JSON `Accept`/`Content-Type`; the Anthropic
protocol additionally fixes `anthropic-version: 2023-06-01`. Authorization
headers and API keys are deliberately excluded. Adapters cannot vary these
headers independently of the committed protocol identity. Body/model changes
change the commitment, and mismatch between endpoint actual observation and
projection is rejected.

For DeepSeek V4 the committed `deepseek_chat_v4` wire protocol sends
`thinking: {"type":"disabled"}` with JSON Output. This follows the current
official DeepSeek API contract and avoids consuming a small bounded response
budget on reasoning before the closed JSON artifact. The model remains exactly
`deepseek-v4-flash`; no silent substitution is allowed.

## Exact provider attempts

The operational edge executes exactly one provider HTTP request per Sprout
dispatch. Retry is not hidden inside an adapter loop:

1. attempt 1 is claimed and receives an immutable dispatch/profile witness;
2. a timeout or malformed/post-request failure persists its exact wire witness;
3. only a retryable failure returns the invocation to `pending`;
4. attempt 2 obtains a new lease/dispatch and its own wire witness;
5. success preserves attempt 1 history; exact replay creates no new call/row.

The server rejects `provider_timeout`, `invalid_structured_output`, or
post-request `provider_unavailable` without a request witness. A genuinely
pre-request failure is represented separately as `local_execution_failed` and
does not enable a successful surface. Non-retryable provider/auth failure
terminates after one attempt. `maxAttempts` remains server-authoritative.

The PostgreSQL E2E verifies distinct ordinals and immutable attempt rows,
timeout witness retention, success after retry, no retry after non-retryable
failure, and replay without a third attempt.

## Provider and mode inventory

| Mode/provider | Result | Exact classification |
| --- | --- | --- |
| DeepSeek / `deepseek-v4-flash` | model discovery, structured adapter generation, and live edge-boundary executions for both supported task kinds passed | **LIVE ADAPTER + EDGE-BOUNDARY TESTED; not continuous browser/native-companion→DB production E2E** |
| OpenAI / OpenAI-compatible | contract, wire witness, strict schema, failure and redaction tests passed | **ADAPTER/CONTRACT REFINED** |
| Anthropic / Anthropic-compatible | independent Messages protocol and fixed header contract tested | **ADAPTER/CONTRACT REFINED** |
| xAI | OpenAI-compatible contract path present | **ADAPTER/CONTRACT REFINED; not live-tested** |
| Ollama local | Ollama 0.32.15, real discovery/generation and cancellation passed using `qwen2.5:0.5b-instruct` | **LOCAL LIVE FEATURE TESTED; not browser/native-companion production E2E** |
| DS4 LAN | configured local DS4 LAN endpoint returned the exact model inventory, real strict JSON generation, exact wire witness, cancellation and timeout behavior | **LIVE DS4 LAN DEVELOPMENT FEATURE TEST: PASS; HTTP development transport only, not production TLS validated** |
| Remote DS4/Ollama | strict `/32` and `/128` parsing; unvalidated transport fails closed | **DESIGNED / CONTRACT-TESTED / NOT LIVE-VALIDATED** |
| WireGuard | no route or VPN changes | **DEFERRED / NOT LIVE-VALIDATED** |
| Privacy mode D | isolated deterministic pseudonymization/reconstruction, consent and failure contracts tested | **EXPERIMENTAL / NOT YET FORMALLY ENABLED** |

The DeepSeek live process reads `DEEPSEEK_API_KEY` internally from
`process.env`; the credential is never expanded into process arguments or
printed. DS4/Ollama endpoints and all model configuration remain device-local.

The DS4 adapter accepts origin-style or `/v1` API bases with or without a
trailing slash and emits exactly `/v1/models` and `/v1/chat/completions`.
Discovery preserves the model-declared parameter inventory. The tested model
did not declare `response_format`, so the versioned `ds4_openai_chat_v1`
protocol does not send it: it uses a deterministic JSON-only scaffold plus
`reasoning_effort=none`, which the model did declare. Sprout's strict parser,
closed schema and grounding checks remain authoritative; no JSON repair,
parameter fallback, model substitution or cloud-provider fallback exists.

## Ollama lifecycle

The product separates installation consent from model-pull consent. The web
control plane opens the official Ollama installer handoff when no native edge
lifecycle is connected; a native implementation must re-detect after install.
Ollama is not uninstalled after tests and remains usable by other projects.

For the checkpoint live test, the official Linux amd64 Ollama 0.32.15 archive
was installed persistently in the user's local prefix after system-wide
installation required unavailable sudo interaction. The official archive hash
observed was
`50539c5fe9bf85887733355098dcdb266b433cb8c73fa180713417e9ed6e42bb`.
The separately consented `qwen2.5:0.5b-instruct` model was downloaded directly
through Ollama and remains local. No installer/model bytes crossed Sprout or
entered Git.

## Security and no-model-memory properties

- every invocation is semantically self-contained and rebuilt from the exact
  current 0031 context;
- no provider conversation/session ID, previous-response ID, remote memory,
  DS4/Ollama session or persistent cognitive memory is used;
- Ollama sets `keep_alive: 0`; cache reuse, if any, is not a semantic source;
- output remains a candidate until closed-schema parsing, bounds and grounded
  identifiers pass the existing validator;
- provider output never decides permission, Responsibility, confirmation,
  LocalGoal or authority;
- save/delete of the local AI profile performs zero Sprout requests, does not
  enter DEV export/localStorage/logging, and clears local credential/config;
- Mode D has no silent fallback to A and does not initialize when unselected;
- comment/disclosure remain disabled/fail-closed.

The endpoint and provider remain external TCB assumptions for plaintext
fidelity and provider-side storage. Sprout proves what its authorized edge
declared and committed; it does not prove that a remote provider retained no
data or that a specific executable ran.

## Migration and verification matrix

| Gate | Observed result |
| --- | --- |
| migration static/install-state | **PASS** — 32 files validated, 32 installed |
| fresh install 1→32 | **PASS** |
| populated 0031→0032 upgrade | **PASS** — legacy observation/projection IDs preserved as `legacy_0031`, exact=false, endpoint/profile commitments NULL |
| `verify_schema.sql` | **PASS** — `sprout schema verification passed` |
| `verify_behavior.sql` | **PASS** — `sprout behavioral verification passed` |
| targeted runtime/attempt PostgreSQL E2E | **PASS** — 1 passed, 0 failed, 0 ignored, 17 filtered |
| all DB-enabled ignored tests, serial | **PASS** — 27 passed, 0 failed, 0 ignored, 212 filtered |
| ordinary Rust workspace | **PASS** — 212 passed, 0 failed, 27 ignored |
| frontend ordinary tests | **PASS** — 266 passed, 0 failed, 6 skipped live tests |
| provider/edge focused conformance | **PASS** — 23 passed |
| DeepSeek live | **PASS** — 2 passed, 4 other-provider tests skipped |
| Ollama local live | **PASS** — 2 passed, 4 other-provider tests skipped |
| DS4 LAN development live | **PASS** — 2 passed, 4 other-provider tests skipped; real discovery/generation/cancellation/timeout, HTTP development transport only |
| frontend lint/build | **PASS** |
| `cargo fmt --check` / Clippy `-D warnings` | **PASS** |
| cargo deny / audit / npm audit | **PASS** — allowed duplicate diagnostics; 0 Rust vulnerabilities |
| WASM parity | **PASS** — 4 passed |
| WASM reproducibility | **PASS** — byte-for-byte |
| Lean 4.30.0 byte-identical compile | **PASS** — no diagnostics; source hash unchanged |

## R5.40/R5.41 mapping and residual limits

**FORMALLY SPECIFIED:** exact state-grounded invocation, actual/projection
separation, bounded structured runtime, no hidden Sprout model memory,
interrogation read-only, proxy authority preservation and non-vacuous surface
gates.

**CONCRETELY REFINED:** trusted runtime-kind claim separation; pre-bound opaque
profile commitment; exact provider-neutral wire witness; dual-signed actual
observation; server/DB attempt history and retry fencing; client-local vault;
strict provider adapters; local Ollama and DeepSeek edge-boundary live behavior.
DS4 LAN development discovery and generation are also live-tested against the
exact configured model without exposing its configuration to Sprout.

**FAIL-CLOSED / NOT YET IMPLEMENTED:** production native local-edge companion;
continuous browser/native-companion→provider→real PostgreSQL production E2E;
production-secure DS4 LAN TLS/pinning; remote private transport/WireGuard;
Mode D formal exposure projection; comment/disclosure; other language task
kinds.

**EXTERNAL TCB ASSUMPTION:** authorized endpoint integrity, correspondence of
encrypted plaintext to the device's commitments, provider semantic fidelity,
provider data retention/internal state, and executable identity.

**CHECKPOINT 0032: CLOSED for the explicitly refined scope**: client-owned
multi-provider boundary, exact request/profile/attempt provenance,
server-blind local configuration, live DeepSeek, live local Ollama and live DS4
LAN development behavior. This is not a claim that Mode A/B are
production-complete, that the native companion/product packaging exists, or
that full R5 concrete refinement has been achieved.
