//! Widgets the AI makes (a small command, how often to run it, shown as text), and the
//! calendar behind the "today" pane. Widgets live in `~/.local/share/tripppin/widgets/*.json`;
//! the calendar is plain `.ics` files in `~/.local/share/tripppin/calendar/`.

use crate::paths;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MAX_LINES: usize = 14;
pub const MAX_CHARS: usize = 1500;
pub const MIN_EVERY_SECS: u64 = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Widget {
    pub name: String,
    pub command: String,
    pub every_secs: u64,
}

pub fn dir() -> PathBuf {
    paths::data_dir().join("widgets")
}

pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 32 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ')
}

fn file_for(name: &str) -> PathBuf {
    dir().join(format!("{}.json", name.trim().replace(' ', "-")))
}

pub fn list() -> Vec<Widget> {
    let mut out: Vec<Widget> = std::fs::read_dir(dir())
        .map(|d| {
            d.flatten().filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()).collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn create(name: &str, command: &str, every_secs: u64) -> Result<Widget, String> {
    if !valid_name(name.trim()) {
        return Err("the widget name must be letters, numbers, spaces or dashes, up to 32 characters.".into());
    }
    if command.trim().is_empty() {
        return Err("the command is empty.".into());
    }
    let w = Widget {
        name: name.trim().to_string(),
        command: command.trim().to_string(),
        every_secs: every_secs.max(MIN_EVERY_SECS),
    };
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    std::fs::write(file_for(&w.name), serde_json::to_string_pretty(&w).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(w)
}

pub fn remove(name: &str) -> bool {
    std::fs::remove_file(file_for(name)).is_ok()
}

/// Runs a widget's command (through `sh -c`), waiting at most `timeout`, and returns what it
/// printed, cut to a handful of short lines.
pub fn run_command(command: &str, timeout: Duration) -> String {
    let child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return "(could not start)".into() };
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.take(8192).read_to_end(&mut buf);
        }
        buf
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let bytes = reader.join().unwrap_or_default();
    shorten(&crate::term::strip_ansi(&String::from_utf8_lossy(&bytes)))
}

pub fn shorten(text: &str) -> String {
    let mut out: Vec<String> = text.lines().map(|l| l.trim_end().chars().take(120).collect()).take(MAX_LINES).collect();
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    let s = out.join("\n");
    s.chars().take(MAX_CHARS).collect()
}

/// The last result of a widget and when it was asked for.
#[derive(Clone, Debug)]
pub struct State {
    pub text: String,
    pub updated: Option<Instant>,
    pub updated_at: String,
    pub running: bool,
    pub last_start: Option<Instant>,
}

#[derive(Default)]
pub struct Runner {
    pub states: std::collections::HashMap<String, State>,
}

impl Runner {
    /// The widgets whose time has come (and that aren't running); marks them running.
    pub fn due(&mut self, widgets: &[Widget], now: Instant) -> Vec<Widget> {
        let mut out = Vec::new();
        for w in widgets {
            let st = self.states.entry(w.name.clone()).or_insert(State {
                text: String::new(),
                updated: None,
                updated_at: String::new(),
                running: false,
                last_start: None,
            });
            let due = st.last_start.is_none_or(|t| now.duration_since(t) >= Duration::from_secs(w.every_secs));
            if due && !st.running {
                st.running = true;
                st.last_start = Some(now);
                out.push(w.clone());
            }
        }
        out
    }

    pub fn finished(&mut self, name: &str, text: String, at: String) {
        if let Some(s) = self.states.get_mut(name) {
            s.text = text;
            s.updated = Some(Instant::now());
            s.updated_at = at;
            s.running = false;
        }
    }

    /// When the next widget is due, to sleep until.
    pub fn next_due(&self, widgets: &[Widget], now: Instant) -> Option<Duration> {
        widgets
            .iter()
            .map(|w| {
                let st = self.states.get(&w.name);
                let at = st.and_then(|s| s.last_start).map(|t| t + Duration::from_secs(w.every_secs));
                at.map_or(Duration::ZERO, |a| a.saturating_duration_since(now))
            })
            .min()
    }
}

// ---- calendar ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// "HH:MM", or empty for an all-day event.
    pub time: String,
    pub title: String,
}

pub fn calendar_dir() -> PathBuf {
    paths::data_dir().join("calendar")
}

/// Today's events from the .ics text of one file (`ymd` is "20261126"), by start time.
pub fn events_on(ics: &str, ymd: &str) -> Vec<Event> {
    // Long lines are folded: a continuation starts with a space.
    let mut unfolded: Vec<String> = Vec::new();
    for l in ics.lines() {
        match (l.strip_prefix(' ').or_else(|| l.strip_prefix('\t')), unfolded.last_mut()) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ => unfolded.push(l.trim_end().to_string()),
        }
    }
    let mut out = Vec::new();
    let (mut start, mut title, mut inside) = (String::new(), String::new(), false);
    for l in &unfolded {
        match l.as_str() {
            "BEGIN:VEVENT" => {
                inside = true;
                start.clear();
                title.clear();
            }
            "END:VEVENT" if inside => {
                inside = false;
                if start.starts_with(ymd) && !title.is_empty() {
                    let time = start
                        .get(9..13)
                        .filter(|_| start.len() >= 13)
                        .map(|t| format!("{}:{}", &t[..2], &t[2..]))
                        .unwrap_or_default();
                    out.push(Event { time, title: title.clone() });
                }
            }
            l if inside && l.starts_with("DTSTART") => start = l.rsplit(':').next().unwrap_or("").to_string(),
            l if inside && l.starts_with("SUMMARY") => {
                title = l.split_once(':').map(|(_, v)| v.replace("\\,", ",").replace("\\;", ";")).unwrap_or_default()
            }
            _ => {}
        }
    }
    out
}

pub fn today(ymd: &str) -> Vec<Event> {
    let mut all: Vec<Event> = std::fs::read_dir(calendar_dir())
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "ics"))
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .flat_map(|t| events_on(&t, ymd))
                .collect()
        })
        .unwrap_or_default();
    all.sort_by(|a, b| a.time.cmp(&b.time));
    all
}

/// An .ics file's text for one event (`when` is "YYYY-MM-DD HH:MM" local time).
pub fn ics_for(title: &str, when: &str, minutes: u32, uid: &str) -> Result<String, String> {
    let bad = || "when must look like 2026-11-27 18:30.".to_string();
    let (date, time) = when.trim().split_once(' ').ok_or_else(bad)?;
    let d: Vec<&str> = date.split('-').collect();
    let t: Vec<&str> = time.split(':').collect();
    if d.len() != 3 || t.len() < 2 || d[0].len() != 4 || d[1].len() != 2 || d[2].len() != 2 {
        return Err(bad());
    }
    let nums: Vec<u32> = d.iter().chain(t.iter().take(2)).filter_map(|x| x.parse().ok()).collect();
    if nums.len() != 5 || nums[1] == 0 || nums[1] > 12 || nums[2] == 0 || nums[2] > 31 || nums[3] > 23 || nums[4] > 59 {
        return Err(bad());
    }
    let start = format!("{:04}{:02}{:02}T{:02}{:02}00", nums[0], nums[1], nums[2], nums[3], nums[4]);
    let end_min = nums[3] * 60 + nums[4] + minutes.max(1);
    let end = format!("{:04}{:02}{:02}T{:02}{:02}00", nums[0], nums[1], nums[2], (end_min / 60).min(23), end_min % 60);
    let title = title.replace(['\n', '\r'], " ").replace(',', "\\,").replace(';', "\\;");
    Ok(format!(
        "BEGIN:VCALENDAR\nVERSION:2.0\nPRODID:-//tripppin//EN\nBEGIN:VEVENT\nUID:{uid}\nDTSTART:{start}\nDTEND:{end}\nSUMMARY:{title}\nEND:VEVENT\nEND:VCALENDAR\n"
    ))
}

pub fn add_event(title: &str, when: &str, minutes: u32) -> Result<String, String> {
    let uid = format!("{}-{}@tripppin", crate::timex::unix_now(), title.len());
    let ics = ics_for(title, when, minutes, &uid)?;
    std::fs::create_dir_all(calendar_dir()).map_err(|e| e.to_string())?;
    std::fs::write(calendar_dir().join(format!("{uid}.ics")), ics).map_err(|e| e.to_string())?;
    Ok(format!("Added \"{}\" at {when}.", title.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_a_widget_command_and_caps_output() {
        assert_eq!(run_command("echo hello; echo world", Duration::from_secs(5)), "hello\nworld");
        let long = run_command("seq 1 100", Duration::from_secs(5));
        assert_eq!(long.lines().count(), MAX_LINES);
        assert_eq!(run_command("sleep 5; echo late", Duration::from_millis(200)), "");
    }

    #[test]
    fn runner_schedules_by_interval() {
        let w = vec![Widget { name: "a".into(), command: "true".into(), every_secs: 10 }];
        let mut r = Runner::default();
        let t0 = Instant::now();
        assert_eq!(r.due(&w, t0).len(), 1);
        assert!(r.due(&w, t0 + Duration::from_secs(11)).is_empty(), "still running");
        r.finished("a", "x".into(), "21:00".into());
        assert!(r.due(&w, t0 + Duration::from_secs(5)).is_empty());
        assert_eq!(r.due(&w, t0 + Duration::from_secs(11)).len(), 1);
        assert_eq!(r.states["a"].text, "x");
    }

    #[test]
    fn widget_names_are_checked() {
        assert!(valid_name("weather"));
        assert!(valid_name("my clock-2"));
        assert!(!valid_name("../x"));
        assert!(!valid_name(""));
        assert!(create("../x", "true", 10).is_err());
    }

    #[test]
    fn reads_todays_events_from_ics() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20261126T190000\nSUMMARY:gym\nEND:VEVENT\n\
BEGIN:VEVENT\nDTSTART;TZID=Asia/Kolkata:20261126T100000\nSUMMARY:design\n review\nEND:VEVENT\n\
BEGIN:VEVENT\nDTSTART;VALUE=DATE:20261126\nSUMMARY:birthday\nEND:VEVENT\n\
BEGIN:VEVENT\nDTSTART:20261127T090000\nSUMMARY:tomorrow\nEND:VEVENT\nEND:VCALENDAR\n";
        let e = events_on(ics, "20261126");
        assert_eq!(e.len(), 3);
        assert_eq!(e[0], Event { time: "19:00".into(), title: "gym".into() });
        assert_eq!(e[1], Event { time: "10:00".into(), title: "designreview".into() });
        assert_eq!(e[2].time, "");
    }

    #[test]
    fn makes_ics_that_reads_back() {
        let ics = ics_for("lunch, priya", "2026-11-27 13:00", 60, "u1").unwrap();
        let e = events_on(&ics, "20261127");
        assert_eq!(e, vec![Event { time: "13:00".into(), title: "lunch, priya".into() }]);
        assert!(ics_for("x", "tomorrow", 30, "u").is_err());
        assert!(ics_for("x", "2026-13-01 10:00", 30, "u").is_err());
    }
}
