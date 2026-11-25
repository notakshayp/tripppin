//! The calendar's data: dates, events, the plain-text event parser ("call mum friday 6pm")
//! and iCalendar storage. No GTK in here, so it is all tested.
//!
//! Events are standard iCalendar files, one VEVENT per file in
//! `~/.local/share/tripppin/calendar/` (`$XDG_DATA_HOME` is honoured). Times are floating
//! local times (`DTSTART:20261127T180000`), which is what a personal calendar means.
//! Other programs (the start screen's live tile, the AI) read the same folder.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

const MONTHS: [&str; 12] = [
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
const WEEKDAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

impl Date {
    pub fn new(y: i32, m: u32, d: u32) -> Date {
        Date { y, m, d }
    }
    /// Days since 1970-01-01 (proleptic Gregorian).
    pub fn days(self) -> i64 {
        let y = i64::from(self.y) - i64::from(self.m <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = (i64::from(self.m) + 9) % 12;
        let doy = (153 * mp + 2) / 5 + i64::from(self.d) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }
    pub fn from_days(z: i64) -> Date {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = (yoe + era * 400 + i64::from(m <= 2)) as i32;
        Date { y, m, d }
    }
    pub fn plus(self, days: i64) -> Date {
        Date::from_days(self.days() + days)
    }
    /// 0 = Monday … 6 = Sunday.
    pub fn weekday(self) -> usize {
        (self.days() + 3).rem_euclid(7) as usize
    }
    pub fn days_in_month(y: i32, m: u32) -> u32 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
            _ => 28,
        }
    }
    pub fn month_name(self) -> &'static str {
        MONTHS[self.m as usize - 1]
    }
    pub fn weekday_name(self) -> &'static str {
        WEEKDAYS[self.weekday()]
    }
    /// The Monday of this date's week.
    pub fn week_start(self) -> Date {
        self.plus(-(self.weekday() as i64))
    }
    /// The same day in another month (clamped to the month's length).
    pub fn add_months(self, n: i32) -> Date {
        let total = self.y * 12 + self.m as i32 - 1 + n;
        let (y, m) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
        Date { y, m, d: self.d.min(Date::days_in_month(y, m)) }
    }
}

/// A moment: a date and the minute of the day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp {
    pub date: Date,
    pub min: u32,
}

impl Stamp {
    pub fn plus_minutes(self, n: u32) -> Stamp {
        let total = self.min + n;
        Stamp { date: self.date.plus(i64::from(total / 1440)), min: total % 1440 }
    }
    pub fn minutes_since_epoch(self) -> i64 {
        self.date.days() * 1440 + i64::from(self.min)
    }
}

pub fn clock(min: u32) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

pub fn duration_words(min: u32) -> String {
    match (min / 60, min % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub uid: String,
    pub title: String,
    pub start: Stamp,
    pub minutes: u32,
    pub detail: String,
}

impl Event {
    pub fn end(&self) -> Stamp {
        self.start.plus_minutes(self.minutes)
    }
    /// A colour from the tile palette, the same for the same title every time.
    pub fn colour(&self) -> usize {
        self.title.to_lowercase().bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b))) as usize % 7
    }
    /// The line under a title on a block: "video call · 1 h" (the length alone when it isn't an hour).
    pub fn sub(&self) -> String {
        match (self.detail.is_empty(), self.minutes == 60) {
            (true, true) => String::new(),
            (true, false) => duration_words(self.minutes),
            (false, _) => format!("{} · {}", self.detail.replace('\n', ", "), duration_words(self.minutes)),
        }
    }
    /// "video call · 10:00 – 11:00 · 1 h", for notifications.
    pub fn line(&self) -> String {
        let when = format!("{} – {} · {}", clock(self.start.min), clock(self.end().min), duration_words(self.minutes));
        if self.detail.is_empty() { when } else { format!("{} · {when}", self.detail) }
    }
}

// ---- the plain-text parser ----------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub struct Parsed {
    pub title: String,
    pub start: Stamp,
    pub minutes: u32,
}

fn weekday_of(w: &str) -> Option<usize> {
    // monday, mon, mondays; tues, thurs
    let w = w.strip_suffix('s').unwrap_or(w);
    WEEKDAYS.iter().position(|d| w.len() >= 3 && d.starts_with(w))
}

fn month_of(w: &str) -> Option<u32> {
    let w = w.trim_end_matches('.');
    if w.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| m.starts_with(w)).map(|i| i as u32 + 1)
}

fn day_number(w: &str) -> Option<u32> {
    let n = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let suffix = &w[n.len()..];
    if !matches!(suffix, "" | "st" | "nd" | "rd" | "th") {
        return None;
    }
    n.parse().ok().filter(|d| (1..=31).contains(d))
}

/// "6pm", "6:30pm", "18:00" as minutes of the day.
fn clock_word(w: &str) -> Option<u32> {
    let (body, pm) = if let Some(b) = w.strip_suffix("pm") {
        (b, Some(true))
    } else if let Some(b) = w.strip_suffix("am") {
        (b, Some(false))
    } else {
        (w, None)
    };
    let (h, m) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if pm.is_some() => (body.parse::<u32>().ok()?, 0),
        None => return None,
    };
    if m > 59 {
        return None;
    }
    let h = match pm {
        Some(true) if (1..=12).contains(&h) => h % 12 + 12,
        Some(false) if (1..=12).contains(&h) => h % 12,
        None if h < 24 => h,
        _ => return None,
    };
    Some(h * 60 + m)
}

fn unit_minutes(u: &str) -> Option<u32> {
    match u {
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(60),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(1),
        _ => None,
    }
}

/// "90" then "min" as minutes.
fn spoken_duration(n: &str, unit: Option<&str>) -> Option<u32> {
    let v: f64 = n.parse().ok()?;
    Some((v * f64::from(unit_minutes(unit?)?)).round() as u32).filter(|m| *m > 0)
}

/// "1h", "90min", "1.5h" as minutes.
fn duration_word(w: &str) -> Option<u32> {
    let split = w.find(|c: char| c.is_ascii_alphabetic())?;
    let (n, u) = w.split_at(split);
    let n: f64 = n.parse().ok()?;
    Some((n * f64::from(unit_minutes(u)?)).round() as u32).filter(|m| *m > 0)
}

/// Reads an event out of plain words. `now` is the moment "today" and "tonight" are measured
/// from. Dates: today, tomorrow, a weekday (the next one), "25 dec", "dec 25", "25/12",
/// "2026-12-25". Times: 6pm, 6:30pm, 18:00, "at 6", noon, midnight. Length: "for 2h",
/// "90min", "for an hour". With no time the event starts at 09:00 (or, with no date either,
/// at the next whole hour); with no duration it lasts an hour.
pub fn parse_event(text: &str, now: Stamp) -> Option<Parsed> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let low: Vec<String> = words.iter().map(|w| w.to_lowercase().trim_end_matches([',', '.']).to_string()).collect();
    let mut used = vec![false; words.len()];
    let (mut date, mut time, mut minutes): (Option<Date>, Option<u32>, Option<u32>) = (None, None, None);
    let mut tonight = false;

    let mut i = 0;
    while i < words.len() {
        let w = low[i].as_str();
        let next = low.get(i + 1).map(String::as_str);
        let mut take = 0;
        if date.is_none() && matches!(w, "today" | "tonight" | "tomorrow" | "tmrw") {
            date = Some(if w.starts_with('t') && w != "today" && w != "tonight" { now.date.plus(1) } else { now.date });
            tonight = w == "tonight";
            take = 1;
        } else if date.is_none()
            && let Some(target) = weekday_of(w)
        {
            // a weekday: the next one after today
            let ahead = (target as i64 - now.date.weekday() as i64).rem_euclid(7);
            date = Some(now.date.plus(if ahead == 0 { 7 } else { ahead }));
            take = 1;
        } else if date.is_none()
            && let (Some(d), Some(m)) = (day_number(w), next.and_then(month_of))
        {
            date = Some(resolve(now.date, d, m));
            take = 2;
        } else if date.is_none()
            && let (Some(m), Some(d)) = (month_of(w), next.and_then(day_number))
        {
            date = Some(resolve(now.date, d, m));
            take = 2;
        } else if date.is_none() && w.contains('/') {
            if let Some((d, m)) = w.split_once('/')
                && let (Ok(d), Ok(m)) = (d.parse::<u32>(), m.parse::<u32>())
                && (1..=31).contains(&d)
                && (1..=12).contains(&m)
            {
                date = Some(resolve(now.date, d, m));
                take = 1;
            }
        } else if date.is_none() && w.len() == 10 && w.as_bytes()[4] == b'-' {
            let p: Vec<&str> = w.split('-').collect();
            if let [y, m, d] = p[..]
                && let (Ok(y), Ok(m), Ok(d)) = (y.parse::<i32>(), m.parse::<u32>(), d.parse::<u32>())
                && (1..=12).contains(&m)
                && (1..=31).contains(&d)
            {
                date = Some(Date::new(y, m, d));
                take = 1;
            }
        }
        if take == 0 && time.is_none() {
            if w == "noon" {
                (time, take) = (Some(720), 1);
            } else if w == "midnight" {
                (time, take) = (Some(0), 1);
            } else if let Some(t) = clock_word(w) {
                (time, take) = (Some(t), 1);
            } else if let (Ok(h), Some(ap @ ("am" | "pm"))) = (w.parse::<u32>(), next)
                && (1..=12).contains(&h)
            {
                time = clock_word(&format!("{h}{ap}"));
                take = 2;
            } else if w == "at"
                && let Some(n) = next
                && let Ok(h) = n.parse::<u32>()
                && (1..=12).contains(&h)
                && !matches!(low.get(i + 2).map(String::as_str), Some("am" | "pm"))
            {
                // "at 6": small hours mean the evening, 7 to 11 the morning
                time = Some(if h <= 6 { (h + 12) * 60 } else { h % 12 * 60 });
                take = 2;
            }
        }
        if take == 0 && minutes.is_none() {
            if w == "for"
                && let Some(n) = next
                && let Some(m) = duration_word(n).or_else(|| spoken_duration(n, low.get(i + 2).map(String::as_str)))
            {
                take = if duration_word(n).is_some() { 2 } else { 3 };
                minutes = Some(m);
            } else if w == "for"
                && matches!(next, Some("an" | "a"))
                && matches!(low.get(i + 2).map(String::as_str), Some("hour"))
            {
                (minutes, take) = (Some(60), 3);
            } else if w == "for"
                && next == Some("half")
                && matches!(low.get(i + 2).map(String::as_str), Some("an" | "hour"))
            {
                (minutes, take) = (Some(30), if low.get(i + 2).map(String::as_str) == Some("an") { 4 } else { 3 });
            } else if let Some(m) = duration_word(w) {
                (minutes, take) = (Some(m), 1);
            }
        }
        if take == 0 {
            i += 1;
            continue;
        }
        for u in used.iter_mut().skip(i).take(take) {
            *u = true;
        }
        // The little word in front ("on friday", "next monday", "at 6pm", "for 2h") goes too.
        let mut back = i;
        while back > 0
            && !used[back - 1]
            && matches!(low[back - 1].as_str(), "on" | "at" | "next" | "this" | "by" | "from" | "for")
        {
            used[back - 1] = true;
            back -= 1;
        }
        i += take;
    }

    let title: Vec<&str> = words.iter().zip(&used).filter(|(_, u)| !**u).map(|(w, _)| *w).collect();
    let mut title = title.join(" ");
    if title.is_empty() {
        return None;
    }
    if let Some(c) = title.chars().next() {
        title = c.to_uppercase().collect::<String>() + &title[c.len_utf8()..];
    }
    let (date, time) = match (date, time) {
        (Some(d), Some(t)) => (d, t),
        (Some(d), None) => (d, if tonight { 19 * 60 } else { 9 * 60 }),
        (None, Some(t)) => (if t > now.min { now.date } else { now.date.plus(1) }, t),
        (None, None) => {
            let next_hour = (now.min / 60 + 1) * 60;
            if next_hour >= 1440 { (now.date.plus(1), 9 * 60) } else { (now.date, next_hour) }
        }
    };
    Some(Parsed { title, start: Stamp { date, min: time }, minutes: minutes.unwrap_or(60) })
}

/// The next occurrence of day/month on or after `today`.
fn resolve(today: Date, d: u32, m: u32) -> Date {
    let d = d.min(Date::days_in_month(today.y, m));
    let this = Date::new(today.y, m, d);
    if this >= today { this } else { Date::new(today.y + 1, m, d.min(Date::days_in_month(today.y + 1, m))) }
}

// ---- iCalendar ---------------------------------------------------------------------------

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn stamp_text(s: Stamp) -> String {
    format!("{:04}{:02}{:02}T{:02}{:02}00", s.date.y, s.date.m, s.date.d, s.min / 60, s.min % 60)
}

/// Reads 20261127T180000, 20261127T180000Z or 20261127 (a whole day, at 00:00).
fn parse_stamp(v: &str) -> Option<Stamp> {
    let v = v.trim().trim_end_matches('Z');
    let (d, t) = v.split_once('T').unwrap_or((v, "000000"));
    if d.len() != 8 || t.len() < 4 {
        return None;
    }
    let date = Date::new(d[..4].parse().ok()?, d[4..6].parse().ok()?, d[6..8].parse().ok()?);
    let (h, m): (u32, u32) = (t[..2].parse().ok()?, t[2..4].parse().ok()?);
    Some(Stamp { date, min: h * 60 + m })
}

fn parse_iso_duration(v: &str) -> Option<u32> {
    let v = v.strip_prefix('P')?;
    let (days, rest) = match v.split_once('D') {
        Some((d, r)) => (d.parse::<u32>().ok()?, r),
        None => (0, v),
    };
    let rest = rest.strip_prefix('T').unwrap_or(rest);
    let (mut total, mut num) = (days * 1440, String::new());
    for c in rest.chars() {
        match c {
            '0'..='9' => num.push(c),
            'H' => total += num.parse::<u32>().ok()? * 60,
            'M' => total += num.parse::<u32>().ok()?,
            _ => {}
        }
        if c.is_ascii_alphabetic() {
            num.clear();
        }
    }
    Some(total)
}

pub fn to_ics(e: &Event) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".into(),
        "PRODID:-//Tripppin//Calendar//EN".into(),
        "BEGIN:VEVENT".into(),
        format!("UID:{}", e.uid),
        format!("DTSTAMP:{}", stamp_text(e.start)),
        format!("DTSTART:{}", stamp_text(e.start)),
        format!("DTEND:{}", stamp_text(e.end())),
        format!("SUMMARY:{}", escape(&e.title)),
    ];
    if !e.detail.is_empty() {
        lines.push(format!("DESCRIPTION:{}", escape(&e.detail)));
    }
    lines.push("END:VEVENT".into());
    lines.push("END:VCALENDAR".into());
    lines.join("\r\n") + "\r\n"
}

/// uid, title, detail, start, end, duration, all-day: an event as it is being read.
type Raw = (String, String, String, Option<Stamp>, Option<Stamp>, Option<u32>, bool);

/// Every VEVENT in an iCalendar file (an event with no usable start is skipped).
pub fn from_ics(text: &str) -> Vec<Event> {
    // Unfold: a line starting with a space or tab continues the one before.
    let mut lines: Vec<String> = Vec::new();
    for l in text.lines() {
        if (l.starts_with(' ') || l.starts_with('\t')) && !lines.is_empty() {
            if let Some(last) = lines.last_mut() {
                last.push_str(&l[1..]);
            }
        } else {
            lines.push(l.to_string());
        }
    }
    let mut out = Vec::new();
    let mut cur: Option<Raw> = None;
    for l in lines {
        let l = l.trim_end();
        if l == "BEGIN:VEVENT" {
            cur = Some((String::new(), String::new(), String::new(), None, None, None, false));
        } else if l == "END:VEVENT" {
            if let Some((uid, title, detail, start, end, dur, all_day)) = cur.take()
                && let Some(start) = start
            {
                let minutes = match (end, dur) {
                    (Some(e), _) if e > start => (e.minutes_since_epoch() - start.minutes_since_epoch()) as u32,
                    (_, Some(d)) => d,
                    _ if all_day => 1440,
                    _ => 60,
                };
                out.push(Event {
                    uid,
                    title: if title.is_empty() { "(no title)".into() } else { title },
                    start,
                    minutes,
                    detail,
                });
            }
        } else if let Some(c) = cur.as_mut()
            && let Some((key, value)) = l.split_once(':')
        {
            let name = key.split(';').next().unwrap_or("").to_ascii_uppercase();
            match name.as_str() {
                "UID" => c.0 = value.to_string(),
                "SUMMARY" => c.1 = unescape(value),
                "DESCRIPTION" => c.2 = unescape(value),
                "LOCATION" if c.2.is_empty() => c.2 = unescape(value),
                "DTSTART" => {
                    c.3 = parse_stamp(value);
                    c.6 = key.to_ascii_uppercase().contains("VALUE=DATE") && !value.contains('T');
                }
                "DTEND" => c.4 = parse_stamp(value),
                "DURATION" => c.5 = parse_iso_duration(value),
                _ => {}
            }
        }
    }
    out
}

// ---- the folder --------------------------------------------------------------------------

pub fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("tripppin/calendar")
}

pub fn new_uid() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("{n:x}@tripppin")
}

pub fn save_in(dir: &std::path::Path, e: &Event) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let name: String = e.uid.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    std::fs::write(dir.join(format!("{name}.ics")), to_ics(e))
}

pub fn load_from(dir: &std::path::Path) -> Vec<Event> {
    let mut all: Vec<Event> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|f| f.path().extension().is_some_and(|x| x == "ics"))
        .filter_map(|f| std::fs::read_to_string(f.path()).ok())
        .flat_map(|t| from_ics(&t))
        .collect();
    all.sort_by_key(|e| (e.start, e.title.clone()));
    all
}

pub fn delete_in(dir: &std::path::Path, uid: &str) {
    let name: String = uid.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    let _ = std::fs::remove_file(dir.join(format!("{name}.ics")));
}

pub fn on(events: &[Event], date: Date) -> Vec<&Event> {
    events.iter().filter(|e| e.start.date == date).collect()
}

/// Events that started in the last `window` minutes (up to and including `now`).
pub fn due(events: &[Event], now: Stamp, window: i64) -> Vec<&Event> {
    events
        .iter()
        .filter(|e| {
            let ago = now.minutes_since_epoch() - e.start.minutes_since_epoch();
            (0..window).contains(&ago)
        })
        .collect()
}

/// Sends a notification for each event now due that hasn't been announced yet. Safe to call
/// from the app and from the timer: a note of what was announced is kept next to the events.
pub fn remind(now: Stamp) -> usize {
    let dir = dir();
    let events = load_from(&dir);
    let marks_file = dir.join(".reminded");
    let marks = std::fs::read_to_string(&marks_file).unwrap_or_default();
    let mut sent = 0;
    for e in due(&events, now, 5) {
        let mark = format!("{}@{}", e.uid, stamp_text(e.start));
        if marks.lines().any(|l| l == mark) {
            continue;
        }
        let _ = std::process::Command::new("notify-send")
            .args(["-a", "Calendar", "-i", "x-office-calendar", &e.title, &e.line()])
            .status();
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&marks_file) {
            let _ = writeln!(f, "{mark}");
        }
        sent += 1;
    }
    sent
}

#[cfg(test)]
mod tests {
    use super::*;

    // Wednesday 25 November 2026, 21:14.
    fn now() -> Stamp {
        Stamp { date: Date::new(2026, 11, 25), min: 21 * 60 + 14 }
    }

    fn p(text: &str) -> Parsed {
        parse_event(text, now()).unwrap_or_else(|| panic!("no event in {text:?}"))
    }

    #[test]
    fn date_arithmetic() {
        assert_eq!(Date::new(2026, 11, 25).weekday(), 2, "a Wednesday");
        assert_eq!(Date::new(2026, 11, 25).plus(6), Date::new(2026, 12, 1));
        assert_eq!(Date::new(2026, 11, 25).plus(-25), Date::new(2026, 10, 31));
        assert_eq!(Date::new(2024, 2, 28).plus(1), Date::new(2024, 2, 29));
        assert_eq!(Date::new(2026, 11, 25).week_start(), Date::new(2026, 11, 23));
        assert_eq!(Date::new(2026, 1, 31).add_months(1), Date::new(2026, 2, 28));
        assert_eq!(Date::new(2026, 12, 5).add_months(1), Date::new(2027, 1, 5));
        assert_eq!(Date::from_days(Date::new(2000, 3, 1).days()), Date::new(2000, 3, 1));
    }

    #[test]
    fn the_mock_example() {
        let e = p("call mum friday 6pm");
        assert_eq!(e.title, "Call mum");
        assert_eq!(e.start, Stamp { date: Date::new(2026, 11, 27), min: 18 * 60 });
        assert_eq!(e.minutes, 60);
    }

    #[test]
    fn words_for_days() {
        assert_eq!(p("gym tomorrow 7am").start.date, Date::new(2026, 11, 26));
        assert_eq!(p("gym today 10pm").start.date, Date::new(2026, 11, 25));
        assert_eq!(p("dinner tonight").start.min, 19 * 60);
        // the weekday you're on means next week's
        assert_eq!(p("standup wednesday 10am").start.date, Date::new(2026, 12, 2));
        assert_eq!(
            p("standup on next monday at 9:30").start,
            Stamp { date: Date::new(2026, 11, 30), min: 9 * 60 + 30 }
        );
        assert_eq!(p("brunch sat 11am").start.date, Date::new(2026, 11, 28));
        assert_eq!(p("party 5 dec 8pm").start.date, Date::new(2026, 12, 5));
        assert_eq!(p("party december 5th 8pm").start.date, Date::new(2026, 12, 5));
        assert_eq!(p("renew passport 12/3").start.date, Date::new(2027, 3, 12), "a past date is next year");
        assert_eq!(p("flight 2027-01-04 06:45").start, Stamp { date: Date::new(2027, 1, 4), min: 6 * 60 + 45 });
    }

    #[test]
    fn times_and_lengths() {
        assert_eq!(p("lunch tomorrow noon").start.min, 720);
        assert_eq!(p("call tomorrow at 6").start.min, 18 * 60);
        assert_eq!(p("call tomorrow at 8").start.min, 8 * 60);
        assert_eq!(p("call tomorrow 6 pm").start.min, 18 * 60);
        assert_eq!(p("call tomorrow 12am").start.min, 0);
        assert_eq!(p("gym friday 7pm for 90 min").minutes, 90);
        assert_eq!(p("gym friday 7pm for 2h").minutes, 120);
        assert_eq!(p("gym friday 7pm 45min").minutes, 45);
        assert_eq!(p("gym friday 7pm for an hour").minutes, 60);
        assert_eq!(p("gym friday 7pm for 1.5 hours").minutes, 90);
        assert_eq!(p("gym friday 7pm for half an hour").minutes, 30);
        assert_eq!(p("gym friday 7pm for 90 min").title, "Gym");
    }

    #[test]
    fn defaults_and_titles() {
        // a time with no day: today if it's still ahead, otherwise tomorrow
        assert_eq!(p("lunch 11pm").start.date, Date::new(2026, 11, 25));
        assert_eq!(p("lunch 1pm").start.date, Date::new(2026, 11, 26));
        // a day with no time: nine
        assert_eq!(p("dentist friday").start.min, 9 * 60);
        // nothing at all: the next whole hour
        assert_eq!(p("think about it").start.min, 22 * 60);
        // words that merely look like dates stay in the title
        assert_eq!(p("lunch with Priya at Cafe Rosa tomorrow 1pm").title, "Lunch with Priya at Cafe Rosa");
        assert_eq!(p("Meet Mona in Sunderland").title, "Meet Mona in Sunderland");
        assert!(parse_event("friday 6pm", now()).is_none(), "no title, no event");
        assert!(parse_event("   ", now()).is_none());
    }

    fn sample() -> Event {
        Event {
            uid: "abc123@tripppin".into(),
            title: "Design review, round 2; bring notes".into(),
            start: Stamp { date: Date::new(2026, 11, 26), min: 10 * 60 },
            minutes: 90,
            detail: "video call\nroom 4".into(),
        }
    }

    #[test]
    fn ics_round_trip() {
        let e = sample();
        let text = to_ics(&e);
        assert!(text.contains("DTSTART:20261126T100000\r\n"));
        assert!(text.contains("DTEND:20261126T113000\r\n"));
        assert!(text.contains("SUMMARY:Design review\\, round 2\\; bring notes\r\n"));
        assert_eq!(from_ics(&text), vec![e]);
    }

    #[test]
    fn ics_from_other_programs() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x1\r\nDTSTART;TZID=Europe/London:20261201T090000\r\n\
                    DURATION:PT1H30M\r\nSUMMARY:A very long title that was\r\n  folded by another program\r\nLOCATION:Cafe Rosa\r\nEND:VEVENT\r\n\
                    BEGIN:VEVENT\r\nUID:x2\r\nDTSTART;VALUE=DATE:20261224\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\n\
                    BEGIN:VEVENT\r\nUID:x3\r\nSUMMARY:No start\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ev = from_ics(text);
        assert_eq!(ev.len(), 2, "the event without a start is skipped");
        assert_eq!(ev[0].title, "A very long title that was folded by another program");
        assert_eq!(ev[0].minutes, 90);
        assert_eq!(ev[0].detail, "Cafe Rosa");
        assert_eq!(ev[1].minutes, 1440);
        assert_eq!(ev[1].start.min, 0);
    }

    #[test]
    fn the_folder_round_trips_and_deletes() {
        let dir = std::env::temp_dir().join(format!("tripppin-cal-{}", new_uid()));
        let e = sample();
        save_in(&dir, &e).unwrap();
        assert_eq!(load_from(&dir), vec![e.clone()]);
        assert_eq!(on(&load_from(&dir), Date::new(2026, 11, 26)).len(), 1);
        assert!(on(&load_from(&dir), Date::new(2026, 11, 27)).is_empty());
        delete_in(&dir, &e.uid);
        assert!(load_from(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reminders_are_for_events_just_started() {
        let e = sample();
        let at = |min| Stamp { date: Date::new(2026, 11, 26), min };
        let events = [e];
        assert!(due(&events, at(9 * 60 + 59), 5).is_empty(), "not yet");
        assert_eq!(due(&events, at(10 * 60), 5).len(), 1);
        assert_eq!(due(&events, at(10 * 60 + 4), 5).len(), 1);
        assert!(due(&events, at(10 * 60 + 5), 5).is_empty(), "too late to announce");
    }

    #[test]
    fn colours_are_stable() {
        let (a, b) = (sample(), sample());
        assert_eq!(a.colour(), b.colour());
        assert!(a.colour() < 7);
    }
}
