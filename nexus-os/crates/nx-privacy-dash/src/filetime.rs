//! Windows FILETIME handling and conversions.
//!
//! A Windows `FILETIME` is a 64-bit value representing the number of
//! 100-nanosecond intervals that have elapsed since 12:00 A.M. January 1, 1601
//! Coordinated Universal Time (UTC).

use serde::{Deserialize, Serialize};

/// Number of 100-nanosecond intervals between January 1, 1601 and January 1, 1970 UTC.
pub const FILETIME_UNIX_EPOCH_OFFSET_TICKS: u64 = 116_444_736_000_000_000;

/// Number of 100-nanosecond ticks in one millisecond.
pub const TICKS_PER_MILLISECOND: u64 = 10_000;

/// Number of 100-nanosecond ticks in one second.
pub const TICKS_PER_SECOND: u64 = 10_000_000;

/// Strong wrapper around Windows FILETIME ticks.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct FileTime(pub u64);

impl FileTime {
    /// Zero/empty timestamp.
    pub const ZERO: Self = Self(0);

    /// Construct from raw 100-nanosecond ticks.
    pub const fn from_raw(ticks: u64) -> Self {
        Self(ticks)
    }

    /// Raw 64-bit FILETIME value.
    pub const fn raw(&self) -> u64 {
        self.0
    }

    /// Whether this timestamp represents zero or uninitialized time.
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Construct from low and high 32-bit words (as returned by Windows Win32 API).
    pub const fn from_parts(low: u32, high: u32) -> Self {
        let ticks = ((high as u64) << 32) | (low as u64);
        Self(ticks)
    }

    /// Split into low and high 32-bit words.
    pub const fn to_parts(&self) -> (u32, u32) {
        let low = (self.0 & 0xffff_ffff) as u32;
        let high = (self.0 >> 32) as u32;
        (low, high)
    }

    /// Convert to Unix timestamp in milliseconds.
    /// Returns 0 if timestamp is before Unix epoch.
    pub fn to_unix_millis(&self) -> u64 {
        if self.0 <= FILETIME_UNIX_EPOCH_OFFSET_TICKS {
            0
        } else {
            (self.0 - FILETIME_UNIX_EPOCH_OFFSET_TICKS) / TICKS_PER_MILLISECOND
        }
    }

    /// Construct from Unix timestamp in milliseconds.
    pub fn from_unix_millis(ms: u64) -> Self {
        let ticks = ms
            .saturating_mul(TICKS_PER_MILLISECOND)
            .saturating_add(FILETIME_UNIX_EPOCH_OFFSET_TICKS);
        Self(ticks)
    }

    /// Compute duration elapsed between this start time and a stop time in milliseconds.
    /// If `stop` is None, zero, or less than self, calculates duration up to `now_ms`.
    pub fn duration_ms(&self, stop: Option<FileTime>, now_ms: u64) -> u64 {
        let start_ms = self.to_unix_millis();
        if start_ms == 0 {
            return 0;
        }

        match stop {
            Some(stop_ft) if !stop_ft.is_zero() && stop_ft.0 >= self.0 => {
                let stop_ms = stop_ft.to_unix_millis();
                stop_ms.saturating_sub(start_ms)
            }
            _ => now_ms.saturating_sub(start_ms),
        }
    }

    /// Determine if an access session is currently ongoing.
    ///
    /// In Windows CapabilityAccessManager ConsentStore:
    /// - `LastUsedTimeStart` > 0 and `LastUsedTimeStop == 0` means access is currently ongoing.
    /// - `LastUsedTimeStop < LastUsedTimeStart` also indicates active access (started but not completed).
    pub fn is_currently_active(&self, stop: Option<FileTime>) -> bool {
        if self.0 == 0 {
            return false;
        }
        match stop {
            None => true,
            Some(stop_ft) => stop_ft.is_zero() || stop_ft.0 < self.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_conversions() {
        // Test Unix epoch (1970-01-01 00:00:00 UTC)
        let epoch = FileTime::from_raw(FILETIME_UNIX_EPOCH_OFFSET_TICKS);
        assert_eq!(epoch.to_unix_millis(), 0);

        // Test 1 second after Unix epoch
        let ft_1s = FileTime::from_raw(FILETIME_UNIX_EPOCH_OFFSET_TICKS + TICKS_PER_SECOND);
        assert_eq!(ft_1s.to_unix_millis(), 1000);

        // Roundtrip from unix millis
        let now_ms = 1_700_000_000_123u64;
        let ft = FileTime::from_unix_millis(now_ms);
        assert_eq!(ft.to_unix_millis(), now_ms);

        // Parts conversion
        let (low, high) = ft.to_parts();
        let reconstructed = FileTime::from_parts(low, high);
        assert_eq!(ft, reconstructed);
    }

    #[test]
    fn filetime_active_status() {
        let start = FileTime::from_unix_millis(10_000);

        // Active if stop is None or Zero
        assert!(start.is_currently_active(None));
        assert!(start.is_currently_active(Some(FileTime::ZERO)));

        // Active if stop < start
        let stop_earlier = FileTime::from_unix_millis(9_000);
        assert!(start.is_currently_active(Some(stop_earlier)));

        // Not active if stop >= start
        let stop_later = FileTime::from_unix_millis(15_000);
        assert!(!start.is_currently_active(Some(stop_later)));

        // Duration test
        assert_eq!(start.duration_ms(Some(stop_later), 20_000), 5_000);
        assert_eq!(start.duration_ms(None, 20_000), 10_000);
    }
}
