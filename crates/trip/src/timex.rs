//! Local date and time without a time-zone library: UTC from the system clock plus the offset
//! `date +%z` reports (re-read now and then, so a change of zone or daylight time follows).

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    /// 0 = Monday.
    pub weekday: u32,
}

pub const WEEKDAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
pub const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// Days since 1970-01-01 to (year, month, day).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Unix seconds (already shifted to local time) to a date and time.
pub fn from_unix(secs: i64) -> Local {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    Local {
        year,
        month,
        day,
        hour: (rem / 3600) as u32,
        min: (rem % 3600 / 60) as u32,
        sec: (rem % 60) as u32,
        weekday: (days + 3).rem_euclid(7) as u32,
    }
}

/// "+0530" or "-0800" to seconds.
pub fn parse_offset(z: &str) -> Option<i64> {
    let z = z.trim();
    let sign = match z.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let (h, m) = (z.get(1..3)?.parse::<i64>().ok()?, z.get(3..5)?.parse::<i64>().ok()?);
    Some(sign * (h * 3600 + m * 60))
}

pub fn system_offset() -> i64 {
    Command::new("date")
        .arg("+%z")
        .output()
        .ok()
        .and_then(|o| parse_offset(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or(0)
}

pub fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// A clock that remembers the zone offset.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    offset: i64,
    read_at: i64,
}

impl Clock {
    pub fn new() -> Self {
        Clock { offset: system_offset(), read_at: unix_now() }
    }

    pub fn now(&mut self) -> Local {
        let t = unix_now();
        if t - self.read_at > 3600 {
            self.offset = system_offset();
            self.read_at = t;
        }
        from_unix(t + self.offset)
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Local {
    pub fn hhmm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.min)
    }

    /// "wednesday 26 november 2026".
    pub fn long_date(&self) -> String {
        format!("{} {} {} {}", WEEKDAYS[self.weekday as usize], self.day, MONTHS[self.month as usize - 1], self.year)
    }

    /// "20261126".
    pub fn ymd(&self) -> String {
        format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_dates() {
        // 2026-11-26 21:14:05 UTC
        let t = days_from_civil(2026, 11, 26) * 86400 + 21 * 3600 + 14 * 60 + 5;
        let l = from_unix(t);
        assert_eq!((l.year, l.month, l.day, l.hour, l.min, l.sec), (2026, 11, 26, 21, 14, 5));
        assert_eq!(WEEKDAYS[l.weekday as usize], "thursday");
        assert_eq!(l.long_date(), "thursday 26 november 2026");
        assert_eq!(from_unix(0).weekday, 3); // 1970-01-01 was a thursday
        assert_eq!(from_unix(951782400).day, 29); // 2000-02-29
    }

    #[test]
    fn parses_offsets() {
        assert_eq!(parse_offset("+0530\n"), Some(19800));
        assert_eq!(parse_offset("-0800"), Some(-28800));
        assert_eq!(parse_offset("x"), None);
    }
}
