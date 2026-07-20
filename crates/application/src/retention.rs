use chrono::{DateTime, Utc};
use sprout_domain::{RetentionClass, RetentionDeadline};
use uuid::Uuid;

use crate::Clock;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionPlan {
    pub subject_id: Uuid,
    pub deadline: RetentionDeadline,
    pub effective_purge_at: DateTime<Utc>,
}

impl RetentionPlan {
    #[must_use]
    pub fn new(
        subject_id: Uuid,
        class: RetentionClass,
        source_at: DateTime<Utc>,
        dependency_deadlines: impl IntoIterator<Item = DateTime<Utc>>,
    ) -> Self {
        let deadline = dependency_deadlines.into_iter().fold(
            RetentionDeadline::new(class, source_at),
            |deadline, required| deadline.extended_through(required),
        );
        Self {
            subject_id,
            effective_purge_at: deadline.purge_at,
            deadline,
        }
    }

    #[must_use]
    pub fn warning_due<C: Clock>(&self, clock: &C) -> bool {
        clock.now() >= self.deadline.warning_at
    }

    #[must_use]
    pub fn purge_due<C: Clock>(&self, clock: &C) -> bool {
        clock.now() >= self.effective_purge_at
    }

    /// Stable across retries and worker crashes. The database additionally
    /// enforces uniqueness for each recipient and delivery channel.
    #[must_use]
    pub fn warning_deduplication_key(
        &self,
        recipient_identity_id: Uuid,
        channel: WarningChannel,
    ) -> String {
        format!(
            "retention:{}:{}:{}:{}",
            self.subject_id,
            recipient_identity_id,
            self.deadline.warning_at.timestamp_micros(),
            channel.as_str()
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WarningChannel {
    InApp,
    Email,
}

impl WarningChannel {
    const fn as_str(self) -> &'static str {
        match self {
            Self::InApp => "in_app",
            Self::Email => "email",
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;

    struct VirtualClock(DateTime<Utc>);

    impl Clock for VirtualClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    #[test]
    fn llr_08_1_virtual_clock_observes_inclusive_utc_thresholds() {
        let source = Utc.with_ymd_and_hms(2024, 2, 29, 23, 0, 0).unwrap();
        let plan = RetentionPlan::new(Uuid::from_u128(1), RetentionClass::Completed, source, []);
        assert!(!plan.warning_due(&VirtualClock(
            plan.deadline.warning_at - Duration::microseconds(1)
        )));
        assert!(plan.warning_due(&VirtualClock(plan.deadline.warning_at)));
        assert!(!plan.purge_due(&VirtualClock(
            plan.effective_purge_at - Duration::microseconds(1)
        )));
        assert!(plan.purge_due(&VirtualClock(plan.effective_purge_at)));
    }

    #[test]
    fn llr_08_2_maximum_dependency_deadline_wins() {
        let source = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let base = RetentionDeadline::new(RetentionClass::DeletedOrObsolete, source).purge_at;
        let plan = RetentionPlan::new(
            Uuid::from_u128(2),
            RetentionClass::DeletedOrObsolete,
            source,
            [base + Duration::days(2), base + Duration::days(20)],
        );
        assert_eq!(plan.effective_purge_at, base + Duration::days(20));
    }

    #[test]
    fn llr_08_3_warning_keys_are_retry_stable_and_channel_distinct() {
        let plan = RetentionPlan::new(
            Uuid::from_u128(3),
            RetentionClass::DeletedOrObsolete,
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            [],
        );
        let recipient = Uuid::from_u128(4);
        assert_eq!(
            plan.warning_deduplication_key(recipient, WarningChannel::InApp),
            plan.warning_deduplication_key(recipient, WarningChannel::InApp)
        );
        assert_ne!(
            plan.warning_deduplication_key(recipient, WarningChannel::InApp),
            plan.warning_deduplication_key(recipient, WarningChannel::Email)
        );
    }
}
