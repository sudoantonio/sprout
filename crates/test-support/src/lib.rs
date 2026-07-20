//! Deterministic fixtures shared by integration and system tests.
//!
//! This crate deliberately contains no production code. Keeping clocks and
//! filesystem layouts here prevents retention and crash-recovery tests from
//! depending on wall-clock time or developer-machine paths.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use chrono::{DateTime, TimeDelta, Utc};
use tempfile::TempDir;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct VirtualClock {
    now: Arc<RwLock<DateTime<Utc>>>,
}

impl VirtualClock {
    #[must_use]
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: Arc::new(RwLock::new(now)),
        }
    }

    #[must_use]
    pub fn now(&self) -> DateTime<Utc> {
        *self.now.read().expect("virtual clock lock poisoned")
    }

    pub fn set(&self, now: DateTime<Utc>) {
        *self.now.write().expect("virtual clock lock poisoned") = now;
    }

    pub fn advance(&self, delta: TimeDelta) {
        let mut now = self.now.write().expect("virtual clock lock poisoned");
        *now = now
            .checked_add_signed(delta)
            .expect("virtual clock advance overflowed");
    }
}

#[derive(Debug)]
pub struct TempStorage {
    root: TempDir,
    blobs: PathBuf,
    archives: PathBuf,
}

impl TempStorage {
    pub fn new() -> std::io::Result<Self> {
        let root = tempfile::tempdir()?;
        let blobs = root.path().join("blobs");
        let archives = root.path().join("archives");
        fs::create_dir_all(&blobs)?;
        fs::create_dir_all(&archives)?;
        Ok(Self {
            root,
            blobs,
            archives,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    #[must_use]
    pub fn blobs(&self) -> &Path {
        &self.blobs
    }

    #[must_use]
    pub fn archives(&self) -> &Path {
        &self.archives
    }
}

#[must_use]
pub const fn fixture_uuid(index: u128) -> Uuid {
    Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0000 | index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn virtual_clock_is_shared_and_deterministic() {
        let start = Utc
            .with_ymd_and_hms(2026, 1, 31, 12, 0, 0)
            .single()
            .unwrap();
        let clock = VirtualClock::new(start);
        let clone = clock.clone();
        clone.advance(TimeDelta::days(15));
        assert_eq!(clock.now(), start + TimeDelta::days(15));
    }

    #[test]
    fn temporary_storage_has_separate_server_surfaces() {
        let storage = TempStorage::new().unwrap();
        assert!(storage.blobs().is_dir());
        assert!(storage.archives().is_dir());
        assert_ne!(storage.blobs(), storage.archives());
        assert!(storage.blobs().starts_with(storage.root()));
    }

    #[test]
    fn fixture_ids_are_stable_and_distinct() {
        assert_eq!(
            fixture_uuid(1).to_string(),
            "018f0000-0000-7000-8000-000000000001"
        );
        assert_ne!(fixture_uuid(1), fixture_uuid(2));
    }
}
