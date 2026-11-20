//! Commands the agent runs, as jobs that can outlive a single step.
//!
//! `exec` starts a job and waits a while. If the command finishes, the model gets the result; if
//! not, it keeps running in the background and the model can wait for it, check on it or stop it.
//! Every job's full output goes to a log file; the model is shown the newest lines, so a chatty
//! command can't fill its memory.

use crate::shell::{DIM, RESET};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Lines kept in memory per job; the log file has all of them.
const KEEP_LINES: usize = 400;
/// Lines of new output shown to the model per report.
const SHOW_LINES: usize = 40;
/// Characters per shown line; longer lines are cut.
const LINE_CHARS: usize = 300;

struct Output {
    total: usize,
    tail: VecDeque<String>,
}

struct Job {
    id: usize,
    command: String,
    child: Child,
    output: Arc<Mutex<Output>>,
    /// Whether output is echoed to the terminal right now (while someone is waiting on it).
    echo: Arc<AtomicBool>,
    started: Instant,
    log: Option<PathBuf>,
    /// How many lines the model has already been shown.
    seen: usize,
    status: Option<ExitStatus>,
    stopped: bool,
}

fn jobs() -> &'static Mutex<Vec<Job>> {
    static JOBS: OnceLock<Mutex<Vec<Job>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(Vec::new()))
}

fn log_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    let dir = base.join("tripppin/jobs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Starts `command` with bash. A foreground job keeps the keyboard so sudo can ask for a password;
/// a background one gets no input, so it can't steal keystrokes from the shell.
pub fn start(command: &str, foreground: bool) -> Result<usize, String> {
    let mut list = jobs().lock().unwrap_or_else(|e| e.into_inner());
    let id = list.last().map_or(1, |j| j.id + 1);
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(command)
        .stdin(if foreground { Stdio::inherit() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TRIPPPIN_IN_SHELL", "1")
        .spawn()
        .map_err(|e| format!("couldn't start bash: {e}"))?;
    let log = log_dir().map(|d| d.join(format!("{}-{id}.log", std::process::id())));
    let file = log.as_ref().and_then(|p| File::create(p).ok()).map(|f| Arc::new(Mutex::new(f)));
    let output = Arc::new(Mutex::new(Output { total: 0, tail: VecDeque::new() }));
    let echo = Arc::new(AtomicBool::new(foreground));
    let streams: Vec<Box<dyn Read + Send>> = [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .collect();
    for stream in streams {
        let (output, echo, file) = (output.clone(), echo.clone(), file.clone());
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            let mut buf = Vec::new();
            while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string();
                buf.clear();
                if echo.load(Ordering::Relaxed) {
                    println!("{DIM}  │ {line}{RESET}");
                }
                if let Some(f) = &file
                    && let Ok(mut f) = f.lock()
                {
                    let _ = writeln!(f, "{line}");
                }
                let mut out = output.lock().unwrap_or_else(|e| e.into_inner());
                out.total += 1;
                out.tail.push_back(line);
                if out.tail.len() > KEEP_LINES {
                    out.tail.pop_front();
                }
            }
        });
    }
    list.push(Job {
        id,
        command: command.to_string(),
        child,
        output,
        echo,
        started: Instant::now(),
        log,
        seen: 0,
        status: None,
        stopped: false,
    });
    Ok(id)
}

/// Waits up to `limit` for job `id` to finish, showing its output live, then reports to the model.
/// Ctrl-C (`cancel`) stops the job.
pub fn wait(id: usize, limit: Duration, cancel: &AtomicBool) -> String {
    let mut list = jobs().lock().unwrap_or_else(|e| e.into_inner());
    let Some(index) = list.iter().position(|j| j.id == id) else { return no_such_job(&list, id) };
    let job = &mut list[index];
    if job.status.is_none() {
        job.echo.store(true, Ordering::Relaxed);
        let start = Instant::now();
        loop {
            if let Ok(Some(status)) = job.child.try_wait() {
                job.status = Some(status);
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                kill(job);
                break;
            }
            if start.elapsed() >= limit {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if job.status.is_some() {
            settle(job);
        }
        job.echo.store(false, Ordering::Relaxed);
    }
    report(job)
}

/// The job's state and any new output, without waiting.
pub fn output(id: usize) -> String {
    let mut list = jobs().lock().unwrap_or_else(|e| e.into_inner());
    let Some(index) = list.iter().position(|j| j.id == id) else { return no_such_job(&list, id) };
    let job = &mut list[index];
    if job.status.is_none()
        && let Ok(Some(status)) = job.child.try_wait()
    {
        job.status = Some(status);
        settle(job);
    }
    report(job)
}

pub fn stop(id: usize) -> String {
    let mut list = jobs().lock().unwrap_or_else(|e| e.into_inner());
    let Some(index) = list.iter().position(|j| j.id == id) else { return no_such_job(&list, id) };
    let job = &mut list[index];
    if job.status.is_none() {
        kill(job);
    }
    report(job)
}

/// Tells the person a job is carrying on without them watching.
pub fn note_backgrounded(id: usize) {
    println!("{DIM}  still running; carrying on in the background as job {id}{RESET}");
}

fn no_such_job(list: &[Job], id: usize) -> String {
    let ids: Vec<_> = list.iter().map(|j| j.id.to_string()).collect();
    if ids.is_empty() {
        format!("There's no job {id}; nothing has been started yet.")
    } else {
        format!("There's no job {id}. Jobs so far: {}.", ids.join(", "))
    }
}

fn kill(job: &mut Job) {
    let _ = job.child.kill();
    job.status = job.child.wait().ok();
    job.stopped = true;
    settle(job);
}

/// After a job exits, gives the readers a moment to pass on the last lines.
fn settle(job: &Job) {
    let mut last = usize::MAX;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        let now = job.output.lock().map(|o| o.total).unwrap_or(0);
        if now == last {
            break;
        }
        last = now;
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// What the model is told: the state, then the output it hasn't seen yet (the newest lines).
fn report(job: &mut Job) -> String {
    let secs = job.started.elapsed().as_secs();
    let state = match (job.status, job.stopped) {
        (Some(_), true) => format!("Job {} was stopped after {secs} s.", job.id),
        (Some(s), false) => match s.code() {
            Some(code) => format!("Job {} finished with exit code {code} after {secs} s.", job.id),
            None => format!("Job {} was ended by a signal after {secs} s.", job.id),
        },
        (None, _) => format!(
            "Job {} is still running ({secs} s so far). Use job_wait to wait for it, job_output to check on it, \
or job_stop to stop it.",
            job.id
        ),
    };
    let out = job.output.lock().unwrap_or_else(|e| e.into_inner());
    let new = out.total - job.seen.min(out.total);
    let shown = new.min(SHOW_LINES).min(out.tail.len());
    let lines: Vec<String> = out
        .tail
        .iter()
        .skip(out.tail.len() - shown)
        .map(|l| {
            if l.chars().count() > LINE_CHARS {
                format!("{}…", l.chars().take(LINE_CHARS).collect::<String>())
            } else {
                l.clone()
            }
        })
        .collect();
    let total = out.total;
    drop(out);
    job.seen = total;
    let log = job.log.as_ref().map(|p| p.display().to_string());
    let body = match (new, shown) {
        (0, _) if total == 0 => "No output.".to_string(),
        (0, _) => "No new output since the last check.".to_string(),
        (n, s) if n == s => format!("Output:\n{}", lines.join("\n")),
        (n, s) => format!(
            "{n} new lines of output; the last {s}{}:\n{}",
            log.map(|p| format!(" (all of it is in {p})")).unwrap_or_default(),
            lines.join("\n")
        ),
    };
    format!("{state} {body}")
}

/// The command a job runs, for showing in the terminal.
pub fn command(id: usize) -> Option<String> {
    jobs().lock().ok()?.iter().find(|j| j.id == id).map(|j| j.command.clone())
}

/// Whether job `id` is done.
pub fn finished(id: usize) -> bool {
    jobs().lock().ok().and_then(|l| l.iter().find(|j| j.id == id).map(|j| j.status.is_some())).unwrap_or(true)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn short_commands_finish_in_one_go() {
        let cancel = AtomicBool::new(false);
        let id = start("echo one; echo two >&2; exit 4", false).unwrap();
        let r = wait(id, Duration::from_secs(10), &cancel);
        assert!(r.starts_with(&format!("Job {id} finished with exit code 4")), "{r}");
        assert!(r.contains("one") && r.contains("two"), "{r}");
        assert!(output(id).contains("No new output"), "already shown");
    }

    #[test]
    fn long_commands_keep_running_and_can_be_waited_on() {
        let cancel = AtomicBool::new(false);
        let id = start("echo started; sleep 2; echo done", false).unwrap();
        let first = wait(id, Duration::from_millis(400), &cancel);
        assert!(first.contains("still running") && first.contains("started"), "{first}");
        assert!(!finished(id));
        let second = wait(id, Duration::from_secs(10), &cancel);
        assert!(second.contains("exit code 0") && second.contains("done") && !second.contains("started"), "{second}");
    }

    #[test]
    fn chatty_commands_show_only_the_newest_lines() {
        let cancel = AtomicBool::new(false);
        let id = start("seq 1 500", false).unwrap();
        let r = wait(id, Duration::from_secs(10), &cancel);
        assert!(r.contains("500 new lines") && r.contains("\n500") && !r.contains("\n400\n"), "{r}");
    }

    #[test]
    fn jobs_can_be_stopped() {
        let cancel = AtomicBool::new(false);
        let id = start("sleep 30", false).unwrap();
        assert!(wait(id, Duration::from_millis(200), &cancel).contains("still running"));
        assert!(stop(id).contains("was stopped"));
        assert!(output(99_999).contains("no job 99999"));
    }
}
