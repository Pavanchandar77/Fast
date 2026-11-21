//! Shell clock. Tests use a pushed [`ClockSample`] so they stay deterministic.
//! The live island reads the local OS clock via [`ClockSample::now_local`].

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weekday {
    Sun,
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
}

impl Weekday {
    pub fn short(self) -> &'static str {
        match self {
            Self::Sun => "Sun",
            Self::Mon => "Mon",
            Self::Tue => "Tue",
            Self::Wed => "Wed",
            Self::Thu => "Thu",
            Self::Fri => "Fri",
            Self::Sat => "Sat",
        }
    }

    pub fn from_index(index: u8) -> Self {
        match index % 7 {
            0 => Self::Sun,
            1 => Self::Mon,
            2 => Self::Tue,
            3 => Self::Wed,
            4 => Self::Thu,
            5 => Self::Fri,
            _ => Self::Sat,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Month {
    Jan,
    Feb,
    Mar,
    Apr,
    May,
    Jun,
    Jul,
    Aug,
    Sep,
    Oct,
    Nov,
    Dec,
}

impl Month {
    pub fn short(self) -> &'static str {
        match self {
            Self::Jan => "Jan",
            Self::Feb => "Feb",
            Self::Mar => "Mar",
            Self::Apr => "Apr",
            Self::May => "May",
            Self::Jun => "Jun",
            Self::Jul => "Jul",
            Self::Aug => "Aug",
            Self::Sep => "Sep",
            Self::Oct => "Oct",
            Self::Nov => "Nov",
            Self::Dec => "Dec",
        }
    }

    pub fn from_index(index: u8) -> Self {
        match index {
            1 => Self::Jan,
            2 => Self::Feb,
            3 => Self::Mar,
            4 => Self::Apr,
            5 => Self::May,
            6 => Self::Jun,
            7 => Self::Jul,
            8 => Self::Aug,
            9 => Self::Sep,
            10 => Self::Oct,
            11 => Self::Nov,
            12 => Self::Dec,
            _ => Self::Jan,
        }
    }
}

/// A clock reading supplied by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockSample {
    pub hour: u8,
    pub minute: u8,
    pub weekday: Weekday,
    pub month: Month,
    pub day: u8,
}

impl ClockSample {
    pub fn new(hour: u8, minute: u8, weekday: Weekday, month: Month, day: u8) -> Self {
        Self {
            hour: hour.min(23),
            minute: minute.min(59),
            weekday,
            month,
            day: day.clamp(1, 31),
        }
    }

    /// macOS menu-bar order: `Fri 18 Nov  21:04` or `Fri 18 Nov  9:04 PM`.
    pub fn format(self, hour24: bool) -> String {
        let day = self.day;
        let wd = self.weekday.short();
        let mo = self.month.short();
        if hour24 {
            format!("{wd} {day} {mo}  {:02}:{:02}", self.hour, self.minute)
        } else {
            let suffix = if self.hour < 12 { "AM" } else { "PM" };
            let h12 = match self.hour % 12 {
                0 => 12,
                h => h,
            };
            format!("{wd} {day} {mo}  {h12}:{:02} {suffix}", self.minute)
        }
    }
}

impl Default for ClockSample {
    fn default() -> Self {
        Self::new(9, 41, Weekday::Fri, Month::Nov, 18)
    }
}

impl ClockSample {
    /// Current local wall time. Used by the Dynamic Island; tests keep using `default()`.
    pub fn now_local() -> Self {
        now_local_impl()
    }
}

#[cfg(unix)]
fn now_local_impl() -> ClockSample {
    #[repr(C)]
    struct Tm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        tm_mday: i32,
        tm_mon: i32,
        tm_year: i32,
        tm_wday: i32,
        tm_yday: i32,
        tm_isdst: i32,
        tm_gmtoff: i64,
        tm_zone: *const i8,
    }

    unsafe extern "C" {
        fn time(tloc: *mut i64) -> i64;
        fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm;
    }

    unsafe {
        let mut t: i64 = 0;
        if time(&mut t) == -1 {
            return ClockSample::default();
        }
        let mut tm = std::mem::zeroed::<Tm>();
        if localtime_r(&t, &mut tm).is_null() {
            return ClockSample::default();
        }
        ClockSample::new(
            tm.tm_hour as u8,
            tm.tm_min as u8,
            Weekday::from_index(tm.tm_wday as u8),
            Month::from_index((tm.tm_mon + 1) as u8),
            tm.tm_mday as u8,
        )
    }
}

#[cfg(windows)]
fn now_local_impl() -> ClockSample {
    use windows::Win32::System::SystemInformation::GetLocalTime;

    let st = unsafe { GetLocalTime() };
    ClockSample::new(
        st.wHour as u8,
        st.wMinute as u8,
        Weekday::from_index(st.wDayOfWeek as u8),
        Month::from_index(st.wMonth as u8),
        st.wDay as u8,
    )
}

#[cfg(not(any(unix, windows)))]
fn now_local_impl() -> ClockSample {
    ClockSample::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_four_hour_clock_is_zero_padded() {
        let sample = ClockSample::new(21, 4, Weekday::Fri, Month::Nov, 18);
        assert_eq!(sample.format(true), "Fri 18 Nov  21:04");
    }

    #[test]
    fn twelve_hour_clock_uses_ampm() {
        let morning = ClockSample::new(0, 5, Weekday::Mon, Month::Jan, 1);
        assert_eq!(morning.format(false), "Mon 1 Jan  12:05 AM");
        let noon = ClockSample::new(12, 0, Weekday::Tue, Month::Jun, 2);
        assert_eq!(noon.format(false), "Tue 2 Jun  12:00 PM");
        let evening = ClockSample::new(21, 4, Weekday::Fri, Month::Nov, 18);
        assert_eq!(evening.format(false), "Fri 18 Nov  9:04 PM");
    }

    #[test]
    fn out_of_range_fields_clamp() {
        let sample = ClockSample::new(40, 90, Weekday::Sun, Month::from_index(99), 0);
        assert_eq!(sample.hour, 23);
        assert_eq!(sample.minute, 59);
        assert_eq!(sample.day, 1);
        assert_eq!(sample.month, Month::Jan);
    }

    #[test]
    fn now_local_is_in_range() {
        let sample = ClockSample::now_local();
        assert!(sample.hour <= 23);
        assert!(sample.minute <= 59);
        assert!(sample.day >= 1 && sample.day <= 31);
    }
}
