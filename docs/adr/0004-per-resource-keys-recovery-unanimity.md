# ADR-0004: Per-resource keys and unanimous owner recovery

- Status: Accepted
- Date: 2026-07-18

## Context

Authorization is resource-specific and asymmetric. A project-wide content key would make least-privilege revocation impossible and turn one compromise into project-wide disclosure. The service must not hold an owner escrow key, yet owner device loss needs a participant-controlled recovery path.

## Decision

Use an independent KEK per resource and epoch, with per-device authenticated envelopes and a mandatory owner envelope. Payload revisions/blobs use fresh DEKs wrapped by the resource KEK. `container_only` uses a separate minimum-header key that cannot derive body, child, or sibling keys.

Revocation starts a new epoch with new key material and envelopes for remaining authorized devices; it protects future revisions only.

Owner recovery uses a recovery secret split among **all active non-owner participants** at a frozen membership epoch with threshold `n-of-n`. Approvals are signed and bind project, epoch, requester/new device, participant device, nonce, and expiry. On success, rotate the owner wrapping key, recovery secret/epoch/shares, approvals, and affected envelopes. The service has no recovery bypass.

The exact secret-sharing implementation and hybrid envelope construction remain subject to the cryptographic production gate.

## Consequences

- Compromise of one resource key does not automatically expose siblings or descendants.
- Grants/revocations require potentially many device envelopes and subtree rekeys.
- Revocation cannot erase plaintext, keys, exports, screenshots, or ciphertext already downloaded.
- Unanimity prevents the owner, server, or a subset of participants from recovering the owner alone.
- **Availability is intentionally reduced:** one unreachable participant, a missing share, or an owner-only project makes owner recovery impossible. The UI must clearly warn users before accepting this risk.
- Membership changes require a new recovery epoch and redistributed shares.

## Rejected alternatives

- **One project key:** rejected because it violates least privilege and compromise isolation.
- **Server escrow/master key:** rejected because it defeats the E2EE trust boundary.
- **Threshold below unanimity:** rejected by the required recovery policy because a subset could recover owner capability.
- **Email recovery restores content keys:** rejected; account authentication recovery and E2EE key recovery are separate.
- **Promise retroactive revocation:** rejected as technically false once an authorized endpoint has received data.
