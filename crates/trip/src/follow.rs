//! Following a request that runs in another process. The orb appends each agent event as a
//! JSON line to `<id>.live` beside the saved session; `trip shell --continue <id>` reads it
//! and draws the events the way the shell draws its own runs. A `<id>.stop` file asks the
//! run to stop (Ctrl+C in the follower).

use crate::agent::Ui;
use crate::model::{PlanStep, Usage};
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

/// A live file nobody has written to for this long belongs to a run that died.
pub const STALE: Duration = Duration::from_secs(20);
const BEAT: Duration = Duration::from_millis(5000);

/// One line of the live file: the agent's `Ui` calls, plus the run's start and end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Live {
    /// A run began. `prior` is how many turns the saved session had before this request.
    Start {
        request: String,
        prior: usize,
    },
    Text {
        s: String,
    },
    Step {
        label: String,
        detail: String,
    },
    Output {
        s: String,
    },
    Note {
        s: String,
    },
    StepDone {
        ok: bool,
    },
    Thinking,
    Waiting {
        s: String,
    },
    Usage {
        usage: Usage,
    },
    Plan {
        steps: Vec<PlanStep>,
    },
    /// Proof the run is alive during quiet moments.
    Beat,
    /// The run is over (the session is saved by now); `error` when it failed or was stopped.
    End {
        error: Option<String>,
    },
}

pub fn live_path(id: &str) -> PathBuf {
    crate::session::dir().join(format!("{id}.live"))
}

pub fn stop_path(id: &str) -> PathBuf {
    crate::session::dir().join(format!("{id}.stop"))
}

/// Writes the events of a run to its live file.
pub struct Log {
    file: Option<std::fs::File>,
}

impl Log {
    /// Starts a fresh live file at `path` (clearing any stop request), with the start marker.
    pub fn create(path: &Path, request: &str, prior: usize) -> Self {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
            prune(dir);
        }
        let _ = std::fs::remove_file(path.with_extension("stop"));
        let file = std::fs::File::create(path).ok();
        let mut log = Log { file };
        log.write(&Live::Start { request: request.into(), prior });
        log
    }

    pub fn write(&mut self, event: &Live) {
        if let Some(f) = &mut self.file
            && let Ok(mut line) = serde_json::to_string(event)
        {
            line.push('\n');
            // One write per line, so a reader sees whole lines or a clean cut.
            let _ = f.write_all(line.as_bytes());
        }
    }

    pub fn end(&mut self, error: Option<String>) {
        self.write(&Live::End { error });
    }
}

/// Forgets live files from days ago.
fn prune(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|d| d > Duration::from_secs(86_400));
        if old && matches!(p.extension().and_then(|x| x.to_str()), Some("live" | "stop")) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Passes every call to `inner` and records it in the log.
pub struct Tee<U: Ui> {
    pub inner: U,
    pub log: Log,
}

impl<U: Ui> Ui for Tee<U> {
    fn text(&mut self, piece: &str) {
        self.log.write(&Live::Text { s: piece.into() });
        self.inner.text(piece);
    }
    fn step(&mut self, label: &str, detail: &str) {
        self.log.write(&Live::Step { label: label.into(), detail: detail.into() });
        self.inner.step(label, detail);
    }
    fn output(&mut self, line: &str) {
        self.log.write(&Live::Output { s: line.into() });
        self.inner.output(line);
    }
    fn note(&mut self, text: &str) {
        self.log.write(&Live::Note { s: text.into() });
        self.inner.note(text);
    }
    fn step_done(&mut self, ok: bool) {
        self.log.write(&Live::StepDone { ok });
        self.inner.step_done(ok);
    }
    fn thinking(&mut self) {
        self.log.write(&Live::Thinking);
        self.inner.thinking();
    }
    fn waiting(&mut self, what: &str) {
        self.log.write(&Live::Waiting { s: what.into() });
        self.inner.waiting(what);
    }
    fn usage(&mut self, usage: &Usage) {
        self.log.write(&Live::Usage { usage: usage.clone() });
        self.inner.usage(usage);
    }
    fn plan(&mut self, steps: &[PlanStep]) {
        self.log.write(&Live::Plan { steps: steps.to_vec() });
        self.inner.plan(steps);
    }
}

/// Plays one event into a `Ui`. `Start`, `Beat` and `End` aren't the Ui's business.
pub fn replay(event: &Live, ui: &mut dyn Ui) {
    match event {
        Live::Text { s } => ui.text(s),
        Live::Step { label, detail } => ui.step(label, detail),
        Live::Output { s } => ui.output(s),
        Live::Note { s } => ui.note(s),
        Live::StepDone { ok } => ui.step_done(*ok),
        Live::Thinking => ui.thinking(),
        Live::Waiting { s } => ui.waiting(s),
        Live::Usage { usage } => ui.usage(usage),
        Live::Plan { steps } => ui.plan(steps),
        Live::Start { .. } | Live::Beat | Live::End { .. } => {}
    }
}

/// While a run is on: honours the stop file by setting `cancel`, and beats into the live
/// file so followers can tell it is alive. Stops when `done` is set.
pub fn watch(id: &str, cancel: Arc<AtomicBool>, done: Arc<AtomicBool>) {
    let (live, stop) = (live_path(id), stop_path(id));
    std::thread::spawn(move || {
        let mut last = std::time::Instant::now();
        while !done.load(Ordering::Relaxed) {
            if stop.exists() {
                cancel.store(true, Ordering::Relaxed);
                let _ = std::fs::remove_file(&stop);
            }
            if last.elapsed() >= BEAT {
                last = std::time::Instant::now();
                if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&live) {
                    let _ = f.write_all(b"{\"t\":\"beat\"}\n");
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });
}

/// Asks the run to stop.
pub fn request_stop(id: &str) {
    let _ = std::fs::write(stop_path(id), "");
}

/// Reads a live file as it grows. A last line without its newline waits for the rest.
pub struct Reader {
    path: PathBuf,
    pos: u64,
    pending: Vec<u8>,
    pub ended: bool,
}

fn age(path: &Path) -> Option<Duration> {
    let t = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    SystemTime::now().duration_since(t).ok()
}

impl Reader {
    pub fn new(path: PathBuf) -> Self {
        Reader { path, pos: 0, pending: Vec::new(), ended: false }
    }

    /// Whether a run is going: a live file that hasn't ended and has been written to lately.
    pub fn running(path: &Path) -> bool {
        if !age(path).is_some_and(|d| d < STALE) {
            return false;
        }
        let mut r = Reader::new(path.to_path_buf());
        r.poll();
        !r.ended
    }

    /// The whole lines that arrived since the last call.
    pub fn poll(&mut self) -> Vec<Live> {
        let Ok(mut f) = std::fs::File::open(&self.path) else { return Vec::new() };
        if f.seek(SeekFrom::Start(self.pos)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        self.pos += buf.len() as u64;
        self.pending.extend_from_slice(&buf);
        let mut out = Vec::new();
        while let Some(i) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=i).collect();
            if let Ok(ev) = serde_json::from_slice::<Live>(&line) {
                if matches!(ev, Live::End { .. }) {
                    self.ended = true;
                }
                out.push(ev);
            }
        }
        out
    }

    /// Nothing has been written for `STALE`: the run died without an end marker.
    pub fn stale(&self) -> bool {
        !age(&self.path).is_some_and(|d| d < STALE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("trip-follow-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d.join("s.live")
    }

    #[derive(Default)]
    struct Rec(Vec<String>);
    impl Ui for Rec {
        fn text(&mut self, p: &str) {
            self.0.push(format!("text:{p}"));
        }
        fn step(&mut self, l: &str, d: &str) {
            self.0.push(format!("step:{l}:{d}"));
        }
        fn output(&mut self, l: &str) {
            self.0.push(format!("out:{l}"));
        }
        fn note(&mut self, t: &str) {
            self.0.push(format!("note:{t}"));
        }
        fn step_done(&mut self, ok: bool) {
            self.0.push(format!("done:{ok}"));
        }
    }

    #[test]
    fn events_round_trip() {
        let evs = vec![
            Live::Start { request: "hi".into(), prior: 2 },
            Live::Step { label: "exec".into(), detail: "ls".into() },
            Live::Output { s: "a\"b\n".into() },
            Live::Usage { usage: Usage { prompt: 5, ..Default::default() } },
            Live::Plan { steps: vec![PlanStep { text: "x".into(), done: true }] },
            Live::End { error: Some("stopped".into()) },
        ];
        for e in &evs {
            let s = serde_json::to_string(e).unwrap();
            assert!(!s.contains('\n'));
            assert_eq!(&serde_json::from_str::<Live>(&s).unwrap(), e);
        }
    }

    #[test]
    fn tee_logs_and_reader_replays() {
        let p = temp("tee");
        let mut tee = Tee { inner: Rec::default(), log: Log::create(&p, "go", 0) };
        tee.step("exec", "ls");
        tee.output("file");
        tee.step_done(true);
        tee.text("ok");
        tee.log.end(None);
        let mut r = Reader::new(p);
        let evs = r.poll();
        assert!(r.ended);
        assert!(matches!(evs[0], Live::Start { prior: 0, .. }));
        let mut rec = Rec::default();
        for e in &evs {
            replay(e, &mut rec);
        }
        assert_eq!(rec.0, tee.inner.0);
    }

    #[test]
    fn partial_last_line_waits() {
        let p = temp("partial");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(b"{\"t\":\"thinking\"}\n{\"t\":\"note\",\"s\":\"he").unwrap();
        let mut r = Reader::new(p.clone());
        assert_eq!(r.poll(), vec![Live::Thinking]);
        assert!(r.poll().is_empty());
        f.write_all(b"llo\"}\n{\"t\":\"end\",\"error\":null}\n").unwrap();
        assert_eq!(r.poll(), vec![Live::Note { s: "hello".into() }, Live::End { error: None }]);
        assert!(r.ended);
    }

    #[test]
    fn unfinished_file_is_running_ended_is_not() {
        let p = temp("running");
        let mut log = Log::create(&p, "go", 0);
        assert!(Reader::running(&p));
        log.end(None);
        assert!(!Reader::running(&p));
        assert!(!Reader::running(&p.with_extension("none")));
    }
}
