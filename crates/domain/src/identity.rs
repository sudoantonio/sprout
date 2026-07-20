use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{EncryptedPayload, ProjectId, UserId};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GlobalUser {
    pub id: UserId,
    pub profile: EncryptedPayload,
    pub state: UserState,
    pub created_at: DateTime<Utc>,
    pub revision: u64,
}

impl GlobalUser {
    #[must_use]
    pub fn new(id: UserId, profile: EncryptedPayload, created_at: DateTime<Utc>) -> Self {
        Self {
            id,
            profile,
            state: UserState::Active,
            created_at,
            revision: 0,
        }
    }

    pub fn schedule_deletion(
        &mut self,
        requested_at: DateTime<Utc>,
        purge_at: DateTime<Utc>,
    ) -> Result<(), IdentityError> {
        if purge_at <= requested_at {
            return Err(IdentityError::InvalidPurgeDeadline);
        }
        if !matches!(self.state, UserState::Active) {
            return Err(IdentityError::AlreadyInactive);
        }
        self.state = UserState::PendingDeletion {
            requested_at,
            purge_at,
        };
        self.revision += 1;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UserState {
    Active,
    PendingDeletion {
        requested_at: DateTime<Utc>,
        purge_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Project {
    pub id: ProjectId,
    pub owner_id: UserId,
    pub payload: EncryptedPayload,
    pub state: ProjectState,
    pub created_at: DateTime<Utc>,
    pub revision: u64,
}

impl Project {
    #[must_use]
    pub fn new(
        id: ProjectId,
        owner_id: UserId,
        payload: EncryptedPayload,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            owner_id,
            payload,
            state: ProjectState::Active,
            created_at,
            revision: 0,
        }
    }

    pub fn schedule_deletion(
        &mut self,
        requested_at: DateTime<Utc>,
        purge_at: DateTime<Utc>,
    ) -> Result<(), IdentityError> {
        if purge_at <= requested_at {
            return Err(IdentityError::InvalidPurgeDeadline);
        }
        if !matches!(self.state, ProjectState::Active) {
            return Err(IdentityError::AlreadyInactive);
        }
        self.state = ProjectState::PendingDeletion {
            requested_at,
            purge_at,
        };
        self.revision += 1;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProjectState {
    Active,
    PendingDeletion {
        requested_at: DateTime<Utc>,
        purge_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum IdentityError {
    #[error("purge deadline must be later than the deletion request")]
    InvalidPurgeDeadline,
    #[error("entity is already inactive")]
    AlreadyInactive,
}
