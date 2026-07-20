use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{EncryptedPayload, IdempotencyKey, ProjectId, ResourceId, SyncEventId, UserId};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SyncCursor {
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMutation {
    Upserted,
    Deleted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SyncEvent {
    pub id: SyncEventId,
    pub project_id: ProjectId,
    pub sequence: u64,
    pub resource_id: ResourceId,
    pub mutation: SyncMutation,
    pub payload: Option<EncryptedPayload>,
    pub occurred_at: DateTime<Utc>,
}

impl SyncEvent {
    pub fn new(
        id: SyncEventId,
        project_id: ProjectId,
        sequence: u64,
        resource_id: ResourceId,
        mutation: SyncMutation,
        payload: Option<EncryptedPayload>,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, SyncError> {
        if sequence == 0 {
            return Err(SyncError::ZeroSequence);
        }
        if mutation == SyncMutation::Upserted && payload.is_none() {
            return Err(SyncError::UpsertPayloadRequired);
        }
        Ok(Self {
            id,
            project_id,
            sequence,
            resource_id,
            mutation,
            payload,
            occurred_at,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SyncBatch {
    pub project_id: ProjectId,
    pub from: SyncCursor,
    pub next: SyncCursor,
    pub events: Vec<SyncEvent>,
}

impl SyncBatch {
    pub fn new(
        project_id: ProjectId,
        from: SyncCursor,
        events: Vec<SyncEvent>,
    ) -> Result<Self, SyncError> {
        let mut previous = from.sequence;
        for event in &events {
            if event.project_id != project_id {
                return Err(SyncError::CrossProjectEvent);
            }
            if event.sequence <= previous {
                return Err(SyncError::NonMonotonicSequence);
            }
            previous = event.sequence;
        }
        Ok(Self {
            project_id,
            from,
            next: SyncCursor { sequence: previous },
            events,
        })
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct IdempotencyRecord {
    pub key: IdempotencyKey,
    pub project_id: ProjectId,
    pub actor_id: UserId,
    pub request_digest: Vec<u8>,
    pub event_ids: Vec<SyncEventId>,
    pub recorded_at: DateTime<Utc>,
}

impl IdempotencyRecord {
    pub fn replay(&self, request_digest: &[u8]) -> Result<&[SyncEventId], SyncError> {
        if self.request_digest == request_digest {
            Ok(&self.event_ids)
        } else {
            Err(SyncError::IdempotencyKeyReused)
        }
    }
}

impl fmt::Debug for IdempotencyRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IdempotencyRecord")
            .field("key", &self.key)
            .field("project_id", &self.project_id)
            .field("actor_id", &self.actor_id)
            .field("request_digest", &"[REDACTED]")
            .field("event_ids", &self.event_ids)
            .field("recorded_at", &self.recorded_at)
            .finish()
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SyncError {
    #[error("sync event sequence must be non-zero")]
    ZeroSequence,
    #[error("upsert events require an encrypted payload")]
    UpsertPayloadRequired,
    #[error("sync batch contains an event from another project")]
    CrossProjectEvent,
    #[error("sync event sequences must be strictly increasing after the cursor")]
    NonMonotonicSequence,
    #[error("idempotency key was reused for a different request")]
    IdempotencyKeyReused,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> EncryptedPayload {
        EncryptedPayload::new(1, "test", "key", vec![1], vec![2]).unwrap()
    }

    #[test]
    fn batch_requires_strictly_increasing_sequences() {
        let project = ProjectId::new();
        let make = |sequence| {
            SyncEvent::new(
                SyncEventId::new(),
                project,
                sequence,
                ResourceId::new(),
                SyncMutation::Upserted,
                Some(payload()),
                Utc::now(),
            )
            .unwrap()
        };
        assert_eq!(
            SyncBatch::new(project, SyncCursor { sequence: 2 }, vec![make(4), make(3)]),
            Err(SyncError::NonMonotonicSequence)
        );
    }

    #[test]
    fn idempotency_replays_only_the_same_digest() {
        let event = SyncEventId::new();
        let record = IdempotencyRecord {
            key: IdempotencyKey::new("request-1").unwrap(),
            project_id: ProjectId::new(),
            actor_id: UserId::new(),
            request_digest: vec![1, 2, 3],
            event_ids: vec![event],
            recorded_at: Utc::now(),
        };
        assert_eq!(record.replay(&[1, 2, 3]).unwrap(), &[event]);
        assert_eq!(record.replay(&[9]), Err(SyncError::IdempotencyKeyReused));
    }
}
