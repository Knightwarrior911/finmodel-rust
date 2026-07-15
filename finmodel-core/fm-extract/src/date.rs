//! Small civil-date helpers shared across extraction (fiscal-year labelling,
//! period detection). One implementation — no hardcoded "current year".

use std::time::{SystemTime, UNIX_EPOCH};

/// The current calendar year (UTC), computed from days-since-epoch via the
/// standard civil-from-days algorithm. Good enough for fiscal-year labels.
pub fn current_year() -> i32 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    year_from_unix_secs(secs)
}

/// Civil year from unix seconds (pure; testable). Algorithm from Howard Hinnant's
/// `civil_from_days`.
pub fn year_from_unix_secs(secs: i64) -> i32 {
    let days = secs.div_euclid(86_400) + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    // civil_from_days returns Jan/Feb as the prior year; correct for it.
    let mp = (5 * (doe - (365 * yoe + yoe / 4 - yoe / 100)) + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (y + i64::from(month <= 2)) as i32
}

/// Today's date (UTC) as `YYYY-MM-DD`.
pub fn today_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400) + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_epochs_map_to_correct_year() {
        // 2021-01-01T00:00:00Z = 1_609_459_200
        assert_eq!(year_from_unix_secs(1_609_459_200), 2021);
        // 2024-07-01T00:00:00Z = 1_719_792_000
        assert_eq!(year_from_unix_secs(1_719_792_000), 2024);
        // 2026-12-31T23:59:59Z = 1_798_761_599
        assert_eq!(year_from_unix_secs(1_798_761_599), 2026);
        // 1970-01-01 boundary
        assert_eq!(year_from_unix_secs(0), 1970);
    }
}
