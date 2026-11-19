//! Menu-bar clock. The host pushes a sample. The shell never reads the OS clock,
//! so tests are deterministic and nothing is written down.

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
}
