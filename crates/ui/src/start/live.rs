//! What the live tiles show, read off the UI thread: the next calendar event (from the .ics
//! files the calendar writes) and what is playing (MPRIS, through playerctl). Nothing here is
//! invented: no event file or no player means no content.

use gtk::glib;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct EventInfo {
    pub title: String,
    /// "today", "tomorrow" or the weekday, lowercase.
    pub when: String,
    /// "10:00", or "all day".
    pub time: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Playing {
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub art: Option<PathBuf>,
    /// The player's name (spotify, firefox, ...), for its icon.
    pub player: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Live {
    pub event: Option<EventInfo>,
    pub playing: Option<Playing>,
}

/// A VEVENT's start as written in the file: y, m, d, h, min, whether it is UTC, whether it is
/// a whole day.
#[derive(Clone, Debug, PartialEq)]
pub struct Ev {
    pub start: Stamp,
    pub utc: bool,
    pub all_day: bool,
    pub summary: String,
}

fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for l in text.lines() {
        let l = l.trim_end_matches('\r');
        if let (Some(rest), Some(last)) = (l.strip_prefix([' ', '\t']), lines.last_mut()) {
            last.push_str(rest);
        } else {
            lines.push(l.to_string());
        }
    }
    lines
}

fn unescape(s: &str) -> String {
    s.replace("\\n", " ").replace("\\N", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

type Stamp = (i32, u32, u32, u32, u32);

fn parse_stamp(v: &str) -> Option<(Stamp, bool, bool)> {
    let v = v.trim();
    let utc = v.ends_with('Z');
    let v = v.trim_end_matches('Z');
    let (date, time) = match v.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (v, None),
    };
    if date.len() != 8 || !date.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (y, m, d) = (date[0..4].parse().ok()?, date[4..6].parse().ok()?, date[6..8].parse().ok()?);
    let (h, mi) = match time {
        Some(t) if t.len() >= 4 => (t[0..2].parse().ok()?, t[2..4].parse().ok()?),
        Some(_) => return None,
        None => (0, 0),
    };
    Some(((y, m, d, h, mi), utc, time.is_none()))
}

/// The simple VEVENTs of one .ics file: summary and start. Repeating events are not expanded.
pub fn parse_events(text: &str) -> Vec<Ev> {
    let (mut out, mut cur, mut inside) = (Vec::new(), (String::new(), None), false);
    for line in unfold(text) {
        match line.as_str() {
            "BEGIN:VEVENT" => {
                inside = true;
                cur = (String::new(), None);
            }
            "END:VEVENT" => {
                if let (true, (summary, Some((start, utc, all_day)))) = (inside, std::mem::take(&mut cur)) {
                    out.push(Ev {
                        start,
                        utc,
                        all_day,
                        summary: if summary.is_empty() { "(no title)".into() } else { summary },
                    });
                }
                inside = false;
            }
            _ if inside => {
                let Some((name, value)) = line.split_once(':') else { continue };
                let key = name.split(';').next().unwrap_or("");
                match key {
                    "SUMMARY" => cur.0 = unescape(value),
                    "DTSTART" => cur.1 = parse_stamp(value),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

/// The first event still to come: timed ones from `now` on, whole-day ones through their day.
pub fn upcoming(items: &[(i64, bool, String)], now: i64) -> Option<&(i64, bool, String)> {
    items.iter().filter(|(ts, all_day, _)| if *all_day { ts + 86_400 > now } else { *ts >= now }).min_by_key(|i| i.0)
}

fn calendar_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("tripppin/calendar")
}

fn stamp_unix(e: &Ev) -> Option<i64> {
    let (y, m, d, h, mi) = e.start;
    let dt = if e.utc {
        glib::DateTime::from_utc(y, m as i32, d as i32, h as i32, mi as i32, 0.0).ok()?
    } else {
        glib::DateTime::new(&glib::TimeZone::local(), y, m as i32, d as i32, h as i32, mi as i32, 0.0).ok()?
    };
    Some(dt.to_unix())
}

fn next_event(dir: &Path) -> Option<EventInfo> {
    let mut items = Vec::new();
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        if e.path().extension().and_then(|x| x.to_str()) != Some("ics") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        for ev in parse_events(&text) {
            if let Some(ts) = stamp_unix(&ev) {
                items.push((ts, ev.all_day, ev.summary));
            }
        }
    }
    let now = glib::DateTime::now_local().ok()?;
    let (ts, all_day, title) = upcoming(&items, now.to_unix())?;
    let at = glib::DateTime::from_unix_local(*ts).ok()?;
    let day = |d: &glib::DateTime| d.format("%Y%m%d").map(|s| s.to_string()).unwrap_or_default();
    let when = if day(&at) == day(&now) {
        "today".to_string()
    } else if now.add_days(1).is_ok_and(|t| day(&t) == day(&at)) {
        "tomorrow".to_string()
    } else {
        at.format("%A").map(|s| s.to_lowercase()).unwrap_or_default()
    };
    let time =
        if *all_day { "all day".to_string() } else { at.format("%H:%M").map(|s| s.to_string()).unwrap_or_default() };
    Some(EventInfo { title: title.clone(), when, time })
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The cover as a local file: file:// URLs directly, web ones fetched once into the cache.
fn artwork(url: &str) -> Option<PathBuf> {
    if let Some(p) = url.strip_prefix("file://") {
        return Some(PathBuf::from(p.replace("%20", " "))).filter(|p| p.exists());
    }
    if !url.starts_with("http") {
        return None;
    }
    let path = crate::common::runtime_dir().join(format!("start-art-{:x}", fnv(url)));
    if !path.exists() {
        let ok = Command::new("curl")
            .args(["-sfL", "-m", "6", "-o"])
            .arg(&path)
            .arg(url)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            let _ = std::fs::remove_file(&path);
            return None;
        }
    }
    Some(path)
}

pub fn parse_playing(out: &str) -> Option<Playing> {
    let mut f = out.trim_end_matches('\n').split('\t');
    let status = f.next()?;
    let title = f.next()?.trim().to_string();
    let artist = f.next().unwrap_or("").trim().to_string();
    let art = f.next().unwrap_or("").trim().to_string();
    let player = f.next().unwrap_or("").trim().to_string();
    if title.is_empty() || !(status == "Playing" || status == "Paused") {
        return None;
    }
    let art = (!art.is_empty()).then(|| artwork(&art)).flatten();
    Some(Playing { playing: status == "Playing", title, artist, art, player })
}

fn playing() -> Option<Playing> {
    let out = Command::new("playerctl")
        .args(["metadata", "--format", "{{status}}\t{{title}}\t{{artist}}\t{{mpris:artUrl}}\t{{playerName}}"])
        .output()
        .ok()?;
    out.status.success().then(|| parse_playing(&String::from_utf8_lossy(&out.stdout))).flatten()
}

/// Polls in a thread and sends a `Live` whenever something changed; stops when the receiver is gone.
pub fn watch() -> async_channel::Receiver<Live> {
    let (tx, rx) = async_channel::unbounded();
    std::thread::spawn(move || {
        let (mut last, mut tick) = (None::<Live>, 0u32);
        let mut event = None;
        loop {
            if tick % 15 == 0 {
                event = next_event(&calendar_dir());
            }
            tick = tick.wrapping_add(1);
            let now = Live { event: event.clone(), playing: playing() };
            if last.as_ref() != Some(&now) {
                if tx.send_blocking(now.clone()).is_err() {
                    return;
                }
                last = Some(now);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:Design\r\n  review\\, again\r\nDTSTART;TZID=Europe/London:20261126T100000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261201\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Broken\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART:20261127T090000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn reads_simple_events() {
        let evs = parse_events(ICS);
        assert_eq!(evs.len(), 3);
        assert_eq!(evs[0].summary, "Design review, again");
        assert_eq!(evs[0].start, (2026, 11, 26, 10, 0));
        assert!(!evs[0].utc && !evs[0].all_day);
        assert!(evs[1].all_day);
        assert!(evs[2].utc);
        assert_eq!(evs[2].summary, "(no title)");
        assert!(parse_events("").is_empty());
    }

    #[test]
    fn picks_the_next_one() {
        let items =
            vec![(100, false, "past".to_string()), (500, false, "b".to_string()), (300, false, "a".to_string())];
        assert_eq!(upcoming(&items, 200).map(|i| i.2.as_str()), Some("a"));
        assert!(upcoming(&items, 600).is_none());
        let day = vec![(1000, true, "today all day".to_string())];
        assert!(upcoming(&day, 1000 + 3600).is_some());
        assert!(upcoming(&day, 1000 + 86_400).is_none());
    }

    #[test]
    fn reads_playerctl() {
        let p = parse_playing("Playing\tSlow morning\tLo-fi mix\t\tspotify\n").unwrap();
        assert!(p.playing && p.art.is_none());
        assert_eq!((p.title.as_str(), p.player.as_str()), ("Slow morning", "spotify"));
        assert!(parse_playing("Stopped\t\t\t\t\n").is_none());
    }
}
