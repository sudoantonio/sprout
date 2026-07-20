# Retention, export, and purge policy

## Scope and clock

This policy covers deleted objects, obsolete immutable versions, completed items and their historical dependencies, encrypted per-user export archives, events/snapshots, blobs, and purge evidence.

All decisions use a server-controlled UTC clock. “Day” means 24 hours from the eligible timestamp. “Calendar month” means adding months in UTC, clamping to the final valid day when the target month is shorter. Tests must cover just before, exactly at, and just after a threshold, including month ends and leap years. Client clocks never decide retention.

## Schedule

| Data state | Warning eligibility | Source purge eligibility | Archive expiry |
| --- | --- | --- | --- |
| Deleted object or obsolete version | 15 days after deletion/supersession | 30 days after deletion/supersession | 30 days after actual source purge |
| Completed item | 6 calendar months after completion | 12 calendar months after completion | 30 days after actual source purge |

The eligible timestamp is immutable and recorded with the state transition. A retry does not move it.

Historical dependencies extend retention. A question/option/version, attachment requirement/template, event, or other record needed to interpret a retained task or submission cannot purge before the latest dependent deadline. The worker computes a referential retention closure and stores the reason and effective deadline.

## Notification

At each warning threshold, the owner and users who currently have effective access receive one in-app notification and, where configured, one email per recipient/channel/window. Concurrency-safe uniqueness prevents duplicate delivery. Delivery failure is retried but does not create a new notification identity.

Notices state:

- source data category and planned purge date;
- whether the recipient has opted into export;
- that exports are encrypted, scoped to that user's authorized data, and temporary;
- that a failed, declined, or undownloaded export does not postpone source purge.

No sensitive title or content appears in email or server-rendered notification plaintext.

## Per-user export

Only opted-in users receive an archive. Authorization is evaluated per item for that user, and the archive contains no data merely because another participant or owner can access it. The browser constructs or can decrypt the authorized content; the service stores only an encrypted archive and minimum expiry/receipt metadata.

Each archive has a versioned encrypted manifest, ciphertext hashes, signature, source purge receipt, creation time, and expiry. The client verifies manifest/signature/checksums before use. The browser first attempts a standard attachment download, then presents a manual user-action fallback if automatic download is blocked.

An export is a convenience copy, not a prerequisite to deletion. Export generation failure, opt-out, missing login, or failure to download never blocks source purge.

## Purge transaction

1. Lease the job with a unique operation ID and verify the server clock threshold.
2. Recompute authorization-independent retention closure and confirm no retained dependency requires the source.
3. Record the exact encrypted records/blobs/events covered by a versioned purge manifest.
4. Delete source rows/blobs through controlled, idempotent operations; broad historical cascade deletion is forbidden.
5. Write durable tombstones/checkpoints so stale offline clients cannot resurrect purged events.
6. Commit a purge receipt and schedule each successfully created archive for 30-day expiry measured from actual source purge.
7. Retry incomplete technical deletion with the same operation ID until all manifest entries are gone.

A partially failed purge is not reported complete. Database/file ordering uses a recoverable state machine so a crash can distinguish pending, deleted, and receipted entries. Worker leases prevent concurrent effects and expire safely after crashes.

## Archive expiry

An encrypted archive is deleted 30 days after the corresponding actual source purge whether or not it was downloaded. Download does not extend or shorten expiry. Expiry removes archive bytes and active delivery references, then records a minimal non-content receipt under the approved operational retention schedule.

## Backups and stale clients

Backups may retain ciphertext beyond online purge only for the documented backup rotation period and must not be used to restore purged data into the live service. Restore procedures replay purge receipts/tombstones before admitting traffic. Backup expiry and destruction must be operationally documented and tested.

A stale client event referencing a purged generation is rejected; it cannot recreate the old resource by replay. A user may create a genuinely new resource only through a new command, identity, and current authorization.

## Failures, holds, and evidence

- Disk-full, DB restart, network failure, and worker crash are tested with idempotent retries.
- Technical purge failure raises an alert; export failure does not delay purge.
- Counts, lag, and operation IDs may be metered; content, filenames, and keys may not.
- Every threshold, notice, archive, purge, retry, and expiry has requirement-linked test evidence.

No legal-hold exception is specified in the current product plan. Adding one requires legal review, a narrowly scoped design, user-facing disclosure, and an ADR before implementation; operators must not improvise an undeclared hold.
