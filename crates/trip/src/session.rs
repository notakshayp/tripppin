//! Sessions: every conversation with the AI, kept on disk so it can be looked back on (the orb's
//! history, `/history` in the terminal) and picked up again.
//!
//! Each session also keeps a notebook for the model: the request, the notes the model writes
//! with its `note` tool and one line per step the harness ran. The notebook is shown to the
//! model at every step, so when old messages are dropped to fit a small context window it still
//! knows what it set out to do, what it tried and what happened. It's saved next to the session
//! as Markdown (`<id>.md`), readable by people too.

use crate::model::Message;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Sessions kept on disk; older ones are deleted.
const KEEP: usize = 200;
/// Characters of a step's output kept for the history view.
const OUTPUT_CHARS: usize = 4000;
/// Steps of the current request shown in the notebook; older ones are counted, not listed.
const NOTEBOOK_STEPS: usize = 14;
/// Notes shown in the notebook (the newest).
const NOTEBOOK_NOTES: usize = 12;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Step {
    /// What ran: "exec", "read", "write", "waiting on job 2"…
    pub tool: String,
    /// The command or path.
    pub detail: String,
    /// `None` while it runs (or when it carried on in the background).
    pub ok: Option<bool>,
    /// The end of what it printed.
    pub output: String,
}

/// One request and what came of it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Turn {
    pub request: String,
    pub steps: Vec<Step>,
    pub answer: String,
    /// `None` while it's being worked on.
    pub ok: Option<bool>,
    /// Why it stopped, when it didn't finish.
    #[serde(default)]
    pub error: String,
    /// Index in `history` of this request's message.
    #[serde(default)]
    pub start: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    /// "orb" or "terminal".
    pub source: String,
    /// Seconds since 1970.
    pub started: u64,
    pub turns: Vec<Turn>,
    /// The model's own notes (its `note` tool).
    pub notes: Vec<String>,
    /// How many times old messages were dropped to fit the model's memory.
    #[serde(default)]
    pub compactions: usize,
    /// The whole conversation, as sent to the model.
    pub history: Vec<Message>,
}

/// A line in the history list.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub id: String,
    pub source: String,
    pub started: u64,
    pub title: String,
    pub turns: usize,
    pub steps: usize,
    pub ok: Option<bool>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub fn dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("tripppin/sessions")
}

impl Session {
    pub fn new(source: &str) -> Self {
        let started = now();
        // Sorts by time as text; the process id and nanoseconds keep two sessions apart.
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        Session {
            id: format!("{started:012}-{}-{:03}", std::process::id(), nanos / 1_000_000),
            source: source.into(),
            started,
            history: vec![Message::system()],
            ..Default::default()
        }
    }

    /// Starts a new request in this session.
    pub fn ask(&mut self, request: &str) {
        self.history.push(Message::user(request));
        self.turns.push(Turn { request: request.into(), start: self.history.len() - 1, ..Default::default() });
    }

    pub fn title(&self) -> &str {
        self.turns.first().map_or("", |t| t.request.as_str())
    }

    pub fn turn(&mut self) -> Option<&mut Turn> {
        self.turns.last_mut()
    }

    pub fn step_started(&mut self, tool: &str, detail: &str) {
        if let Some(t) = self.turn() {
            t.steps.push(Step { tool: tool.into(), detail: detail.into(), ..Default::default() });
        }
    }

    /// Records how the newest step went and the end of its output.
    pub fn step_finished(&mut self, ok: Option<bool>, output: &str) {
        if let Some(step) = self.turn().and_then(|t| t.steps.last_mut()) {
            step.ok = ok;
            step.output = tail(output, OUTPUT_CHARS);
        }
    }

    /// Marks the current request done (or not) and saves.
    pub fn finish(&mut self, result: Result<&str, &str>) {
        if let Some(t) = self.turn() {
            match result {
                Ok(answer) => {
                    t.ok = Some(true);
                    t.answer = answer.trim().into();
                }
                Err(e) => {
                    t.ok = Some(false);
                    t.error = e.into();
                }
            }
        }
        self.save();
    }

    /// A failed request leaves half-finished tool turns behind; drops them from the
    /// conversation (the record of the steps stays).
    pub fn forget_unfinished(&mut self) {
        let Some(t) = self.turns.last() else { return };
        let keep = (t.start + 1).min(self.history.len());
        self.history.truncate(keep);
        let reason = if t.error.is_empty() { "it was stopped" } else { t.error.as_str() };
        self.history.push(Message::assistant(&format!("(I didn't finish that: {reason}.)")));
    }

    /// The notebook as the model sees it: the request, its notes, the steps so far.
    pub fn notebook(&self) -> String {
        let Some(turn) = self.turns.last() else { return String::new() };
        let mut out = format!(
            "TASK NOTEBOOK (kept for you; it stays even when old messages are dropped)\nRequest: {}\n",
            turn.request
        );
        if !self.notes.is_empty() {
            out.push_str("Your notes:\n");
            let skip = self.notes.len().saturating_sub(NOTEBOOK_NOTES);
            for n in &self.notes[skip..] {
                out.push_str(&format!("- {n}\n"));
            }
        }
        if !turn.steps.is_empty() {
            out.push_str("Steps so far:\n");
            let skip = turn.steps.len().saturating_sub(NOTEBOOK_STEPS);
            if skip > 0 {
                out.push_str(&format!("  ({skip} earlier steps not listed)\n"));
            }
            for (i, s) in turn.steps.iter().enumerate().skip(skip) {
                out.push_str(&format!("  {}. {}\n", i + 1, step_line(s)));
            }
        }
        out.push_str(
            "Don't repeat steps that already worked. If a step failed twice, change approach or stop and explain.",
        );
        out
    }

    /// The whole session as Markdown, for people (and the notebook file).
    pub fn markdown(&self) -> String {
        let mut out = format!("# {}\n\n", self.title());
        if !self.notes.is_empty() {
            out.push_str("## notes\n\n");
            for n in &self.notes {
                out.push_str(&format!("- {n}\n"));
            }
            out.push('\n');
        }
        for (i, t) in self.turns.iter().enumerate() {
            out.push_str(&format!("## {}. {}\n\n", i + 1, t.request));
            for (n, s) in t.steps.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", n + 1, step_line(s)));
            }
            if !t.answer.is_empty() {
                out.push_str(&format!("\n{}\n", t.answer));
            }
            if !t.error.is_empty() {
                out.push_str(&format!("\n(stopped: {})\n", t.error));
            }
            out.push('\n');
        }
        if self.compactions > 0 {
            out.push_str(&format!(
                "_older messages were dropped {} time(s) to fit the model's memory; the steps above are complete._\n",
                self.compactions
            ));
        }
        out
    }

    pub fn save(&self) {
        let dir = dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(dir.join(format!("{}.json", self.id)), json);
        }
        let _ = std::fs::write(dir.join(format!("{}.md", self.id)), self.markdown());
    }

    pub fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            source: self.source.clone(),
            started: self.started,
            title: self.title().to_string(),
            turns: self.turns.len(),
            steps: self.turns.iter().map(|t| t.steps.len()).sum(),
            ok: self.turns.last().and_then(|t| t.ok),
        }
    }
}

fn step_line(s: &Step) -> String {
    let mark = match s.ok {
        Some(true) => "ok",
        Some(false) => "FAILED",
        None => "running",
    };
    let detail: String = s.detail.lines().next().unwrap_or("").chars().take(160).collect();
    let mut line = format!("[{mark}] {} {detail}", s.tool);
    if s.ok == Some(false) {
        let last = s.output.lines().rev().find(|l| !l.trim().is_empty() && !l.starts_with("Job ")).unwrap_or("");
        let last: String = last.trim().chars().take(140).collect();
        if !last.is_empty() {
            line.push_str(&format!(" → {last}"));
        }
    }
    line
}

fn tail(text: &str, max: usize) -> String {
    let n = text.chars().count();
    if n <= max { text.to_string() } else { format!("…{}", text.chars().skip(n - max).collect::<String>()) }
}

pub fn load(id: &str) -> Option<Session> {
    if id.contains('/') {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(dir().join(format!("{id}.json"))).ok()?).ok()
}

/// The newest sessions first, at most `limit`. Deletes the oldest beyond `KEEP`.
pub fn list(limit: usize) -> Vec<Summary> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir())
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
        .unwrap_or_default();
    files.sort();
    files.reverse();
    for old in files.iter().skip(KEEP) {
        let _ = std::fs::remove_file(old);
        let _ = std::fs::remove_file(old.with_extension("md"));
    }
    files
        .iter()
        .take(limit)
        .filter_map(|p| serde_json::from_str::<Session>(&std::fs::read_to_string(p).ok()?).ok())
        .filter(|s| !s.turns.is_empty())
        .map(|s| s.summary())
        .collect()
}

/// "today 14:05", "yesterday 09:12" or "3 mar 18:40", in local time.
pub fn when(started: u64, now: u64) -> String {
    let offset = local_offset();
    let local = |t: u64| (t as i64 + offset).max(0) as u64;
    let (t, n) = (local(started), local(now));
    let clock = format!("{:02}:{:02}", t % 86400 / 3600, t % 3600 / 60);
    match n / 86400 - (t / 86400).min(n / 86400) {
        0 => format!("today {clock}"),
        1 => format!("yesterday {clock}"),
        _ => {
            let (_, m, d) = civil(t / 86400);
            let months = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
            format!("{d} {} {clock}", months[(m - 1) as usize])
        }
    }
}

pub fn now_secs() -> u64 {
    now()
}

/// The local time zone's offset from UTC in seconds, from `date +%z` (no time zone crate).
fn local_offset() -> i64 {
    let out = std::process::Command::new("date").arg("+%z").output().ok();
    let z = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let (sign, digits) = match z.split_at_checked(1) {
        Some(("-", d)) => (-1, d),
        Some((_, d)) => (1, d),
        None => return 0,
    };
    let h: i64 = digits.get(0..2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let m: i64 = digits.get(2..4).and_then(|s| s.parse().ok()).unwrap_or(0);
    sign * (h * 3600 + m * 60)
}

/// Year, month and day for days since 1970 (Howard Hinnant's civil_from_days).
fn civil(days: u64) -> (i64, u32, u32) {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_notebook_of_the_steps() {
        let mut s = Session::new("orb");
        s.ask("install vlc");
        s.notes.push("plan: search flathub, then install".into());
        s.step_started("exec", "flatpak search vlc");
        s.step_finished(Some(true), "VLC org.videolan.VLC");
        s.step_started("exec", "flatpak install -y flathub org.videolan.VLC");
        s.step_finished(Some(false), "Job 2 finished with exit code 1\nerror: no space left on device\n");
        let nb = s.notebook();
        assert!(nb.contains("Request: install vlc"));
        assert!(nb.contains("- plan: search flathub"));
        assert!(nb.contains("1. [ok] exec flatpak search vlc"));
        assert!(nb.contains("2. [FAILED] exec flatpak install") && nb.contains("→ error: no space left on device"));
        assert_eq!(s.summary().steps, 2);
    }

    #[test]
    fn lists_only_recent_steps() {
        let mut s = Session::new("terminal");
        s.ask("x");
        for i in 0..20 {
            s.step_started("exec", &format!("echo {i}"));
            s.step_finished(Some(true), "");
        }
        let nb = s.notebook();
        assert!(nb.contains("(6 earlier steps not listed)"));
        assert!(nb.contains("20. [ok] exec echo 19"));
        assert!(!nb.contains("echo 5\n"));
    }

    #[test]
    fn forgets_half_finished_tool_turns() {
        let mut s = Session::new("terminal");
        s.ask("first");
        s.history.push(Message::assistant("done"));
        s.ask("second");
        s.history.push(Message::tool("c", "partial"));
        s.finish(Err("stopped"));
        s.forget_unfinished();
        let roles: Vec<_> = s.history.iter().map(|m| m.0["role"].as_str().unwrap().to_string()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user", "assistant"]);
        std::fs::remove_file(dir().join(format!("{}.json", s.id))).ok();
        std::fs::remove_file(dir().join(format!("{}.md", s.id))).ok();
    }

    #[test]
    fn round_trips_through_json() {
        let mut s = Session::new("orb");
        s.ask("hello");
        s.finish(Ok("hi"));
        let back: Session = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.summary(), s.summary());
        assert_eq!(back.history.len(), 2);
        std::fs::remove_file(dir().join(format!("{}.json", s.id))).ok();
        std::fs::remove_file(dir().join(format!("{}.md", s.id))).ok();
    }

    #[test]
    fn dates_read_like_words() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_416), (2025, 11, 24));
        let day = 86_400;
        let t = 20_416 * day + 3600;
        let o = local_offset();
        assert!(when((t as i64 - o) as u64, (t as i64 - o) as u64 + 60).starts_with("today 01:00"));
        assert!(when((t as i64 - o) as u64, (t as i64 - o) as u64 + day).starts_with("yesterday"));
        assert_eq!(when((t as i64 - o) as u64, (t as i64 - o) as u64 + 5 * day), "24 nov 01:00");
    }
}
