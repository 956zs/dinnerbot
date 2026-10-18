//! Local-time helpers that avoid a timezone database: a fixed UTC offset is
//! all the bot needs to tell breakfast from late night.

use std::time::{SystemTime, UNIX_EPOCH};

const SECONDS_PER_DAY: i64 = 86_400;

/// A moment expressed as a local day number and hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    /// Days since 1970-01-01 in local time. Changes at local midnight.
    pub day: i64,
    /// Local hour, 0-23.
    pub hour: u32,
}

impl LocalTime {
    pub fn from_unix(unix_seconds: i64, utc_offset_seconds: i32) -> Self {
        let local = unix_seconds + i64::from(utc_offset_seconds);
        let hour = local.rem_euclid(SECONDS_PER_DAY) / 3_600;
        Self {
            day: local.div_euclid(SECONDS_PER_DAY),
            // rem_euclid keeps this in 0..24.
            hour: u32::try_from(hour).unwrap_or_default(),
        }
    }

    pub fn now(utc_offset_seconds: i32) -> Self {
        let unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
            .unwrap_or_default();
        Self::from_unix(unix_seconds, utc_offset_seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAIPEI: i32 = 8 * 3_600;

    #[test]
    fn applies_the_offset() {
        // 2026-10-19T00:30:00Z is 08:30 in Taipei.
        let time = LocalTime::from_unix(1_792_369_800, TAIPEI);
        assert_eq!(time.hour, 8);
        assert_eq!(LocalTime::from_unix(1_792_369_800, 0).hour, 0);
    }

    #[test]
    fn day_changes_at_local_midnight() {
        let before = LocalTime::from_unix(16 * 3_600 - 1, TAIPEI); // 23:59:59 local
        let after = LocalTime::from_unix(16 * 3_600, TAIPEI); // 00:00:00 local
        assert_eq!(before.hour, 23);
        assert_eq!(after.hour, 0);
        assert_eq!(after.day, before.day + 1);
    }

    #[test]
    fn handles_times_before_the_epoch() {
        let time = LocalTime::from_unix(-1, 0);
        assert_eq!(time.day, -1);
        assert_eq!(time.hour, 23);
    }

    #[test]
    fn handles_negative_offsets() {
        let time = LocalTime::from_unix(0, -5 * 3_600);
        assert_eq!(time.day, -1);
        assert_eq!(time.hour, 19);
    }
}
