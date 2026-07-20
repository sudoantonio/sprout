use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionClass {
    DeletedOrObsolete,
    Completed,
}

impl RetentionClass {
    #[must_use]
    pub fn warning_from(self, event_at: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            Self::DeletedOrObsolete => event_at + Duration::days(15),
            Self::Completed => add_calendar_months(event_at, 6),
        }
    }

    #[must_use]
    pub fn purge_from(self, event_at: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            Self::DeletedOrObsolete => event_at + Duration::days(30),
            Self::Completed => add_calendar_months(event_at, 12),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetentionDeadline {
    pub class: RetentionClass,
    pub event_at: DateTime<Utc>,
    pub warning_at: DateTime<Utc>,
    pub purge_at: DateTime<Utc>,
}

impl RetentionDeadline {
    #[must_use]
    pub fn new(class: RetentionClass, event_at: DateTime<Utc>) -> Self {
        Self {
            class,
            event_at,
            warning_at: class.warning_from(event_at),
            purge_at: class.purge_from(event_at),
        }
    }

    /// Historical references may only lengthen retention. A provider remains
    /// until the latest dependent deadline, never the earliest one.
    #[must_use]
    pub fn extended_through(mut self, required_until: DateTime<Utc>) -> Self {
        self.purge_at = self.purge_at.max(required_until);
        self
    }

    #[must_use]
    pub fn archive_expires_at(self, actual_purged_at: DateTime<Utc>) -> DateTime<Utc> {
        actual_purged_at + Duration::days(30)
    }
}

/// Adds whole calendar months, clamping to the last valid day of the target
/// month while preserving the UTC wall-clock time.
#[must_use]
pub fn add_calendar_months(at: DateTime<Utc>, months: u16) -> DateTime<Utc> {
    let month_index = i64::from(at.year()) * 12 + i64::from(at.month0()) + i64::from(months);
    let year = i32::try_from(month_index.div_euclid(12))
        .expect("calendar month addition exceeded chrono year range");
    let month = u32::try_from(month_index.rem_euclid(12) + 1).expect("month is in range");
    let last_day = last_day_of_month(year, month);
    let date = NaiveDate::from_ymd_opt(year, month, at.day().min(last_day))
        .expect("clamped calendar date is valid");
    date.and_time(at.time()).and_utc()
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .expect("next month is valid")
        .pred_opt()
        .expect("previous date is valid")
        .day()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn hlt_08_utc_retention_windows_are_exact() {
        let event = Utc.with_ymd_and_hms(2024, 1, 31, 12, 30, 0).unwrap();
        let deleted = RetentionDeadline::new(RetentionClass::DeletedOrObsolete, event);
        assert_eq!(deleted.warning_at, event + Duration::days(15));
        assert_eq!(deleted.purge_at, event + Duration::days(30));

        let completed = RetentionDeadline::new(RetentionClass::Completed, event);
        assert_eq!(
            completed.warning_at,
            Utc.with_ymd_and_hms(2024, 7, 31, 12, 30, 0).unwrap()
        );
        assert_eq!(
            completed.purge_at,
            Utc.with_ymd_and_hms(2025, 1, 31, 12, 30, 0).unwrap()
        );
    }

    #[test]
    fn llr_08_1_month_end_and_leap_year_use_calendar_months() {
        let january_31 = Utc.with_ymd_and_hms(2024, 1, 31, 8, 0, 0).unwrap();
        assert_eq!(
            add_calendar_months(january_31, 1),
            Utc.with_ymd_and_hms(2024, 2, 29, 8, 0, 0).unwrap()
        );
        let leap_day = Utc.with_ymd_and_hms(2024, 2, 29, 23, 59, 59).unwrap();
        assert_eq!(
            add_calendar_months(leap_day, 12),
            Utc.with_ymd_and_hms(2025, 2, 28, 23, 59, 59).unwrap()
        );
    }

    #[test]
    fn llr_08_2_dependencies_only_extend_and_archive_expiry_uses_actual_purge() {
        let event = Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap();
        let required = event + Duration::days(90);
        let deadline = RetentionDeadline::new(RetentionClass::DeletedOrObsolete, event)
            .extended_through(required);
        assert_eq!(deadline.purge_at, required);
        assert_eq!(
            deadline.archive_expires_at(required + Duration::days(2)),
            required + Duration::days(32)
        );
    }

    proptest! {
        #[test]
        fn positive_calendar_months_always_move_forward(
            timestamp in 946_684_800i64..2_524_608_000i64,
            months in 1u16..=120u16,
        ) {
            let at = Utc.timestamp_opt(timestamp, 0).unwrap();
            prop_assert!(add_calendar_months(at, months) > at);
        }
    }
}
