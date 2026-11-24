//! The agent's progress in the terminal: the AI shell's conversation view.
//!
//! It stays an inline terminal app, so everything lands in the normal scrollback. What's
//! finished is printed once and never touched again; below it sits a small *live region* that
//! is redrawn in place: the line of the answer still being written, the step that's running
//! (spinner, elapsed time and its newest output lines) or a status line saying what the model
//! is waiting on. A ticker thread redraws it a dozen times a second, so the screen keeps moving
//! even while no tokens arrive and a slow first word never looks like a hang.
//!
//! When the output isn't a terminal (a pipe, a file) there's no live region: the same finished
//! rows are written once, without colour unless `CLICOLOR_FORCE` asks for it.

use crate::agent::Ui;
use crate::markdown::{Markdown, Theme};
use crate::model::{PlanStep, Usage};
use crate::term::{self, ACCENT, BOLD, DIM, FOREST, PAPER, RED, RESET};
use std::collections::VecDeque;
use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Columns before the text: the speaker's label ("You", "Tripppin") and the space after it.
pub const GUTTER: usize = 10;
/// Output lines a running step shows under it.
const TAIL: usize = 4;
/// Output lines kept under a step that failed, so the reason stays on screen.
const FAILED_TAIL: usize = 3;
/// How often the live region is redrawn.
const TICK: Duration = Duration::from_millis(80);
/// How long the answer can pause mid-stream before the status line comes back.
const STALL: Duration = Duration::from_millis(1200);

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPINNER_ASCII: [&str; 4] = ["|", "/", "-", "\\"];

/// How the terminal can draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    /// Redraw the live region in place (an interactive terminal).
    pub live: bool,
    /// Colours, and the speaker gutter.
    pub color: bool,
    /// Braille spinner and box drawing; ASCII on the Linux console.
    pub unicode: bool,
}

impl Mode {
    /// What stdout supports.
    pub fn detect() -> Mode {
        let tty = io::stdout().is_terminal();
        let term = std::env::var("TERM").unwrap_or_default();
        let no_color = std::env::var_os("NO_COLOR").is_some() || term == "dumb";
        let forced = std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| v != "0");
        Mode { live: tty && term != "dumb", color: (tty || forced) && !no_color, unicode: term != "linux" }
    }

    pub const PLAIN: Mode = Mode { live: false, color: false, unicode: true };

    pub fn theme(self) -> Theme {
        Theme { color: self.color, unicode: self.unicode }
    }

    pub fn spinner(self, at: Duration) -> &'static str {
        let n = (at.as_millis() / TICK.as_millis()) as usize;
        if self.unicode { SPINNER[n % SPINNER.len()] } else { SPINNER_ASCII[n % SPINNER_ASCII.len()] }
    }
}

/// Token counts added up over one task (every model reply it took).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Totals {
    pub prompt: usize,
    pub cached: usize,
    pub completion: usize,
    pub replies: usize,
}

impl Totals {
    pub fn add(&mut self, u: &Usage) {
        self.prompt += u.prompt;
        self.cached += u.cached;
        self.completion += u.completion;
        self.replies += 1;
    }
}

/// A context size: 32k for 32,768 tokens.
pub fn context_size(n: usize) -> String {
    if n >= 1024 && n % 1024 == 0 { format!("{}k", n / 1024) } else { short(n) }
}

/// A count the way people read it: 950, 1.2k, 38k, 1.2M.
pub fn short(n: usize) -> String {
    match n {
        0..1000 => n.to_string(),
        1000..10_000 => format!("{:.1}k", n as f64 / 1000.0).replace(".0k", "k"),
        10_000..1_000_000 => format!("{}k", (n + 500) / 1000),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0).replace(".0M", "M"),
    }
}

/// How full the context is after a reply, in percent (0 when the size isn't known).
pub fn context_fill(u: &Usage) -> usize {
    ((u.prompt + u.completion) * 100).checked_div(u.context).unwrap_or(0).min(100)
}

/// The parts of the line under an answer: `1.2k in (900 cached) · 230 out · 14 tok/s ·
/// context 38% · 4.1k this session`. Parts with nothing to say are left out.
pub fn usage_parts(t: &Totals, last: &Usage) -> Vec<String> {
    let mut parts = Vec::new();
    if t.cached > 0 {
        parts.push(format!("{} in ({} cached)", short(t.prompt), short(t.cached)));
    } else {
        parts.push(format!("{} in", short(t.prompt)));
    }
    parts.push(format!("{} out", short(t.completion)));
    if last.tokens_per_sec >= 0.5 {
        parts.push(format!("{:.0} tok/s", last.tokens_per_sec));
    }
    if last.context > 0 {
        parts.push(format!("context {}%", context_fill(last)));
    }
    let session = last.session_prompt + last.session_completion;
    if session > 0 {
        parts.push(format!("{} this session", short(session)));
    }
    parts
}

/// Joins parts with ` · ` into rows no wider than `width`, breaking only between parts.
pub fn fit(parts: &[String], width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut row = String::new();
    for part in parts {
        if !row.is_empty() && term::width(&row) + 3 + term::width(part) > width {
            rows.push(std::mem::take(&mut row));
        }
        if !row.is_empty() {
            row += " · ";
        }
        row += part;
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

/// Elapsed time, short: 0.4s, 12s, 3m 05s.
pub fn elapsed(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s < 10.0 {
        format!("{s:.1}s")
    } else if s < 60.0 {
        format!("{}s", s as u64)
    } else {
        format!("{}m {:02}s", s as u64 / 60, s as u64 % 60)
    }
}

/// The terminal's width in columns; 80 when it can't say (a pipe, or a terminal reporting 0).
pub fn terminal_width() -> usize {
    crossterm::terminal::size().ok().map(|(w, _)| w as usize).filter(|w| *w > 0).unwrap_or(80)
}

/// A step's kind as people read it: the agent's tool names, in plain words.
pub fn step_name(label: &str) -> String {
    match label {
        "exec" => "Run".into(),
        "exec in the background" => "Start".into(),
        "read" => "Read".into(),
        "write" => "Write".into(),
        "note" => "Note".into(),
        other => term::sentence(other),
    }
}

/// What a finished row was, for spacing: a blank row goes between the answer and the steps.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Text,
    Work,
    Plan,
    Info,
}

struct Step {
    label: String,
    detail: String,
    started: Instant,
    tail: VecDeque<String>,
    lines: usize,
    /// Printed as it goes rather than redrawn: the command may ask for a password on the
    /// terminal, and redrawing would wipe the question.
    direct: bool,
}

struct Screen {
    out: Box<dyn Write + Send>,
    mode: Mode,
    /// Fixed width for tests and recordings; the terminal's own width otherwise.
    fixed_width: Option<usize>,
    md: Markdown,
    /// The answer's current line, still being written.
    partial: String,
    /// When the model started on its current reply, and whether text has come yet.
    turn: Option<Instant>,
    wrote: bool,
    waiting: Option<String>,
    last_text: Option<Instant>,
    step: Option<Step>,
    /// Whether the "Tripppin" label has been printed for this answer.
    labelled: bool,
    last: Option<Kind>,
    last_blank: bool,
    live_rows: usize,
    cursor_hidden: bool,
    started: Instant,
    cancel: Option<Arc<AtomicBool>>,
    totals: Totals,
    usage: Option<Usage>,
    plan: Vec<PlanStep>,
}

impl Screen {
    fn width(&self) -> usize {
        self.fixed_width
            .or_else(|| self.mode.live.then(|| crossterm::terminal::size().ok().map(|(w, _)| w as usize)).flatten())
            .filter(|w| *w > 0)
            .unwrap_or(80)
    }

    fn height(&self) -> usize {
        if self.fixed_width.is_some() {
            return 1000;
        }
        crossterm::terminal::size().ok().map_or(24, |(_, h)| h as usize).max(4)
    }

    fn gutter(&self) -> usize {
        if self.mode.color { GUTTER } else { 0 }
    }

    /// Room for text after the gutter, keeping the last column free so rows never wrap by
    /// themselves.
    fn room(&self) -> usize {
        self.width().saturating_sub(self.gutter() + 1).max(20)
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.mode.color && !text.is_empty() { format!("{code}{text}{RESET}") } else { text.to_string() }
    }

    fn label(&self) -> String {
        if !self.mode.color {
            return String::new();
        }
        if self.labelled {
            " ".repeat(GUTTER)
        } else {
            format!("{ACCENT}{BOLD}Tripppin{RESET}{}", " ".repeat(GUTTER - 8))
        }
    }

    // ---- drawing ----

    fn write(&mut self, text: &str) {
        let _ = self.out.write_all(text.as_bytes());
    }

    fn flush(&mut self) {
        let _ = self.out.flush();
    }

    /// Erases the live region, leaving the cursor at the start of its first row.
    fn clear_live(&mut self) {
        if self.live_rows == 0 {
            return;
        }
        let mut s = String::from("\r");
        if self.live_rows > 1 {
            s += &format!("\x1b[{}A", self.live_rows - 1);
        }
        s += "\x1b[J";
        self.write(&s);
        self.live_rows = 0;
    }

    fn draw_live(&mut self) {
        if !self.mode.live {
            return;
        }
        let mut rows = self.live();
        let max = self.height().saturating_sub(2).max(1);
        if rows.len() > max {
            rows.drain(..rows.len() - max);
        }
        self.clear_live();
        if rows.is_empty() {
            self.flush();
            return;
        }
        if !self.cursor_hidden {
            self.write("\x1b[?25l");
            self.cursor_hidden = true;
        }
        let mut text = String::new();
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                text += "\r\n";
            }
            let gutter = if i == 0 { self.label() } else { " ".repeat(self.gutter()) };
            text += &gutter;
            text += row;
        }
        self.write(&text);
        self.live_rows = rows.len();
        self.flush();
    }

    /// Prints finished rows above the live region, then redraws it.
    fn commit(&mut self, rows: Vec<String>, kind: Kind) {
        if rows.is_empty() {
            return;
        }
        self.clear_live();
        // Steps and notes sit together; the answer and the plan get a blank row around them.
        let group = |k: Kind| if k == Kind::Info { Kind::Work } else { k };
        let separate = self.last.is_some_and(|last| group(last) != group(kind));
        let mut text = String::new();
        if separate && !self.last_blank {
            text += "\r\n";
            self.last_blank = true;
        }
        for row in rows {
            if row.is_empty() {
                // No blank rows at the start, and never two in a row.
                if !self.last_blank && self.last.is_some() {
                    text += "\r\n";
                    self.last_blank = true;
                }
                continue;
            }
            text += &self.label();
            text += &row;
            text += "\r\n";
            self.labelled = true;
            self.last_blank = false;
        }
        if !self.mode.live {
            text = text.replace("\r\n", "\n");
        }
        self.write(&text);
        self.last = Some(kind);
        self.draw_live();
    }

    // ---- what the live region shows ----

    fn live(&self) -> Vec<String> {
        let mut rows = Vec::new();
        if !self.partial.is_empty() {
            rows.extend(self.md.partial(&self.partial, self.room()));
        } else {
            rows.extend(self.md.pending(self.room()));
        }
        if let Some(step) = &self.step {
            if !step.direct {
                rows.push(self.step_row(step, None));
                rows.extend(self.tail_rows(step, TAIL));
            }
        } else if let Some(status) = self.status() {
            rows.push(status);
        }
        rows
    }

    /// The status line: spinner, what's happening and for how long. Only while nothing else
    /// on screen is moving.
    fn status(&self) -> Option<String> {
        let now = Instant::now();
        let stopping = self.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed));
        let since = self.turn?;
        if !stopping && self.last_text.is_some_and(|t| now - t < STALL) {
            return None;
        }
        let what = if stopping {
            "Stopping".to_string()
        } else if let Some(w) = &self.waiting {
            term::sentence(w)
        } else if self.wrote {
            "Writing".to_string()
        } else {
            "Thinking".to_string()
        };
        let spin = self.paint(ACCENT, self.mode.spinner(now - self.started));
        let what = self.shimmer(&what, now - self.started);
        let time = self.paint(DIM, &format!("· {}", elapsed(now - since)));
        let mut row = format!("{spin} {what} {time}");
        let hint = if stopping { "" } else { "Ctrl+C stops" };
        let room = self.room();
        if !hint.is_empty() && term::width(&row) + 3 + hint.len() <= room && now - since > Duration::from_secs(2) {
            row += &format!("   {}", self.paint(DIM, hint));
        }
        Some(row)
    }

    /// The status words with a soft highlight sweeping across them, so a long wait visibly moves.
    fn shimmer(&self, text: &str, at: Duration) -> String {
        if !self.mode.color {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let pos = (at.as_millis() / 70) as usize % (chars.len() + 12);
        let mut out = String::new();
        for (i, c) in chars.iter().enumerate() {
            let near = pos.abs_diff(i);
            let code = match near {
                0 => PAPER,
                1 => ACCENT,
                _ => "\x1b[38;2;70;160;118m",
            };
            out += code;
            out.push(*c);
        }
        out + RESET
    }

    /// A step on one row: mark, what it is, what it works on and how long it took. `done` is
    /// `None` while it runs, then whether it worked (or `Some(None)`: carrying on in the background).
    fn step_row(&self, step: &Step, done: Option<Option<bool>>) -> String {
        let now = Instant::now();
        let mark = match done {
            None => self.paint(ACCENT, self.mode.spinner(now - self.started)),
            Some(Some(true)) => self.paint(ACCENT, if self.mode.unicode { "✓" } else { "+" }),
            Some(Some(false)) => self.paint(RED, if self.mode.unicode { "✗" } else { "x" }),
            Some(None) => self.paint(DIM, if self.mode.unicode { "◌" } else { "~" }),
        };
        let time = match done {
            Some(None) => "In the background".to_string(),
            _ => elapsed(now - step.started),
        };
        let room = self.room();
        let label = step_name(&step.label);
        // mark, space, label, space, detail, two spaces, time
        let fixed = 2 + term::width(&label) + 1 + 2 + time.chars().count();
        let detail = term::truncate(&term::sanitize(&step.detail), room.saturating_sub(fixed).max(8));
        let used = fixed + term::width(&detail);
        let pad = " ".repeat(room.saturating_sub(used));
        let detail = if done.is_some() && self.mode.color { detail } else { self.paint(PAPER, &detail) };
        format!("{mark} {} {detail}{pad}  {}", self.paint(DIM, &label), self.paint(DIM, &time))
    }

    fn tail_rows(&self, step: &Step, n: usize) -> Vec<String> {
        let bar = if self.mode.unicode { "│" } else { "|" };
        let room = self.room().saturating_sub(4);
        let mut rows = Vec::new();
        let hidden = step.lines.saturating_sub(n.min(step.tail.len()));
        if hidden > 0 && n > 0 && !step.tail.is_empty() {
            let more = format!("… {hidden} earlier line{}", if hidden == 1 { "" } else { "s" });
            rows.push(format!("  {} {}", self.paint(FOREST, bar), self.paint(DIM, &more)));
        }
        let skip = step.tail.len().saturating_sub(n);
        for line in step.tail.iter().skip(skip) {
            rows.push(format!("  {} {}", self.paint(FOREST, bar), self.paint(DIM, &term::truncate(line, room))));
        }
        rows
    }

    // ---- events ----

    /// Ends the step on screen. `ok` is `None` when it carries on in the background.
    fn end_step(&mut self, ok: Option<bool>) {
        let Some(step) = self.step.take() else { return };
        let mut rows = Vec::new();
        if step.direct {
            let mark = match ok {
                Some(true) => self.paint(ACCENT, if self.mode.unicode { "✓" } else { "+" }),
                Some(false) => self.paint(RED, if self.mode.unicode { "✗" } else { "x" }),
                None => self.paint(DIM, if self.mode.unicode { "◌" } else { "~" }),
            };
            let what = match ok {
                Some(true) => "Done",
                Some(false) => "Failed",
                None => "Carrying on in the background",
            };
            rows.push(format!("{mark} {}", self.paint(DIM, &format!("{what} · {}", elapsed(step.started.elapsed())))));
        } else {
            rows.push(self.step_row(&step, Some(ok)));
            if ok == Some(false) {
                rows.extend(self.tail_rows(&step, FAILED_TAIL));
            }
        }
        self.commit(rows, Kind::Work);
    }

    /// Prints the answer's finished line, if there is one waiting.
    fn flush_partial(&mut self) {
        if self.partial.is_empty() {
            return;
        }
        let line = std::mem::take(&mut self.partial);
        let room = self.room();
        let rows = self.md.line(&line, room);
        self.commit(rows, Kind::Text);
    }

    fn text(&mut self, piece: &str) {
        if piece.is_empty() {
            return;
        }
        if self.step.is_some() {
            self.end_step(None);
        }
        let now = Instant::now();
        self.last_text = Some(now);
        self.wrote = true;
        let mut rest = piece;
        while let Some(i) = rest.find('\n') {
            self.partial += &rest[..i];
            let line = std::mem::take(&mut self.partial);
            let room = self.room();
            let rows = self.md.line(&line, room);
            // Leading blank lines of an answer aren't worth a row.
            if !(rows.len() == 1 && rows[0].is_empty() && self.last != Some(Kind::Text)) {
                self.commit(rows, Kind::Text);
            }
            rest = &rest[i + 1..];
        }
        self.partial += rest;
        self.draw_live();
    }

    fn step(&mut self, label: &str, detail: &str) {
        self.flush_partial();
        let room = self.room();
        let closing = self.md.finish();
        self.commit(closing, Kind::Text);
        if self.step.is_some() {
            self.end_step(None);
        }
        self.turn = None;
        self.waiting = None;
        // A command that may ask for a password on this terminal is printed, not redrawn.
        let direct = self.mode.live && io::stdin().is_terminal() && detail.split_whitespace().any(|w| w == "sudo");
        let step = Step {
            label: label.to_string(),
            detail: detail.to_string(),
            started: Instant::now(),
            tail: VecDeque::new(),
            lines: 0,
            direct,
        };
        if direct {
            let mark = self.paint(ACCENT, if self.mode.unicode { "▸" } else { ">" });
            let detail = term::truncate(&term::sanitize(detail), room.saturating_sub(label.len() + 4));
            let note = self.paint(DIM, "It may ask for your password");
            let row = format!("{mark} {} {}", self.paint(DIM, &step_name(label)), self.paint(PAPER, &detail));
            self.step = Some(step);
            self.commit(vec![row, note], Kind::Work);
        } else {
            self.step = Some(step);
            self.draw_live();
        }
    }

    fn output(&mut self, line: &str) {
        let line = term::sanitize(line);
        let direct = self.step.as_ref().is_some_and(|s| s.direct);
        if let Some(step) = &mut self.step {
            step.lines += 1;
            step.tail.push_back(line.clone());
            if step.tail.len() > TAIL.max(FAILED_TAIL) {
                step.tail.pop_front();
            }
        }
        if direct || self.step.is_none() {
            let bar = self.paint(FOREST, if self.mode.unicode { "│" } else { "|" });
            let row = format!("  {bar} {}", self.paint(DIM, &term::truncate(&line, self.room().saturating_sub(4))));
            self.commit(vec![row], Kind::Work);
        } else {
            self.draw_live();
        }
    }

    fn note(&mut self, text: &str) {
        let text = text.trim();
        if text.contains("still running") && self.step.is_some() {
            self.end_step(None);
        }
        self.flush_partial();
        let mark = self.paint(FOREST, if self.mode.unicode { "·" } else { "-" });
        let rows =
            crate::markdown::Markdown::new(Theme::PLAIN).line(&term::sentence(text), self.room().saturating_sub(2));
        let rows = rows.into_iter().map(|r| format!("{mark} {}", self.paint(DIM, &r))).collect();
        self.commit(rows, Kind::Info);
    }

    fn plan(&mut self, steps: &[PlanStep]) {
        if steps == self.plan.as_slice() || steps.is_empty() {
            self.plan = steps.to_vec();
            return;
        }
        self.plan = steps.to_vec();
        self.flush_partial();
        let done = steps.iter().filter(|s| s.done).count();
        let mut rows = vec![self.paint(DIM, &format!("Plan · {done} of {} done", steps.len()))];
        let room = self.room();
        for s in steps {
            let (mark, text) = if s.done {
                (self.paint(ACCENT, if self.mode.unicode { "✓" } else { "x" }), self.paint(DIM, &s.text))
            } else {
                (self.paint(DIM, if self.mode.unicode { "○" } else { "o" }), s.text.clone())
            };
            let wrapped = Markdown::new(Theme::PLAIN).line(&s.text, room.saturating_sub(2));
            for (i, row) in wrapped.iter().enumerate() {
                let body = if wrapped.len() == 1 {
                    text.clone()
                } else if s.done {
                    self.paint(DIM, row)
                } else {
                    row.clone()
                };
                rows.push(if i == 0 { format!("{mark} {body}") } else { format!("  {body}") });
            }
        }
        self.commit(rows, Kind::Plan);
    }

    fn thinking(&mut self) {
        self.flush_partial();
        if self.step.is_some() {
            self.end_step(None);
        }
        self.turn = Some(Instant::now());
        self.wrote = false;
        self.waiting = None;
        self.last_text = None;
        self.draw_live();
    }

    fn waiting(&mut self, what: &str) {
        if self.turn.is_none() && self.step.is_none() {
            self.turn = Some(Instant::now());
        }
        self.waiting = (!what.trim().is_empty()).then(|| what.trim().to_string());
        // A new wait mid-answer shows straight away rather than after the stall delay.
        self.last_text = None;
        self.draw_live();
    }

    fn usage(&mut self, u: &Usage) {
        self.totals.add(u);
        self.usage = Some(u.clone());
    }

    /// The end of a task: what's left of the answer, then the token line.
    fn finish(&mut self, stats: Option<&str>) {
        self.flush_partial();
        let closing = self.md.finish();
        self.commit(closing, Kind::Text);
        if self.step.is_some() {
            self.end_step(None);
        }
        self.turn = None;
        self.clear_live();
        let parts = match &self.usage {
            Some(u) => {
                let mut parts = vec![elapsed(self.started.elapsed())];
                parts.extend(usage_parts(&self.totals, u));
                parts
            }
            None => stats.map(|s| vec![elapsed(self.started.elapsed()), s.to_string()]).unwrap_or_default(),
        };
        if !parts.is_empty() {
            let rows = fit(&parts, self.room());
            if self.mode.color {
                let rows: Vec<String> = rows.iter().map(|r| self.paint(DIM, r)).collect();
                self.commit(rows, Kind::Info);
            } else {
                // Without a terminal the numbers go to stderr, so the answer can be piped on its own.
                for row in rows {
                    eprintln!("{row}");
                }
            }
        }
        self.restore();
    }

    fn tick(&mut self) {
        if self.live_rows > 0 || self.status().is_some() || self.step.as_ref().is_some_and(|s| !s.direct) {
            self.draw_live();
        }
    }

    fn restore(&mut self) {
        self.clear_live();
        if self.cursor_hidden {
            self.write("\x1b[?25h");
            self.cursor_hidden = false;
        }
        self.flush();
    }
}

/// Progress drawn in the terminal, the way the AI shell shows it. Create one per task and call
/// [`Terminal::finish`] when the task ends.
pub struct Terminal {
    screen: Arc<Mutex<Screen>>,
    stop: Arc<AtomicBool>,
    ticker: Option<JoinHandle<()>>,
}

impl Terminal {
    /// Draws to stdout. `cancel` is the flag Ctrl+C sets; the status line says "Stopping" as
    /// soon as it's set.
    pub fn new(cancel: Option<Arc<AtomicBool>>) -> Self {
        Self::with_output(Box::new(io::stdout()), Mode::detect(), None, cancel)
    }

    /// Draws to `out` the way `mode` says, at a fixed width when one is given.
    pub fn with_output(
        out: Box<dyn Write + Send>,
        mode: Mode,
        width: Option<usize>,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Self {
        let screen = Arc::new(Mutex::new(Screen {
            out,
            mode,
            fixed_width: width,
            md: Markdown::new(mode.theme()),
            partial: String::new(),
            turn: None,
            wrote: false,
            waiting: None,
            last_text: None,
            step: None,
            labelled: false,
            last: None,
            last_blank: false,
            live_rows: 0,
            cursor_hidden: false,
            started: Instant::now(),
            cancel,
            totals: Totals::default(),
            usage: None,
            plan: Vec::new(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let ticker = mode.live.then(|| {
            let (screen, stop) = (screen.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(TICK);
                    if let Ok(mut s) = screen.lock() {
                        s.tick();
                    }
                }
            })
        });
        Terminal { screen, stop, ticker }
    }

    fn screen(&self) -> MutexGuard<'_, Screen> {
        self.screen.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The newest token counts, if the model reported any.
    pub fn last_usage(&self) -> Option<Usage> {
        self.screen().usage.clone()
    }

    /// Ends the task on screen: the rest of the answer, then a dim line with the time and the
    /// tokens it took (or `stats` when the model didn't report tokens).
    pub fn finish(&mut self, stats: Option<&str>) {
        self.stop_ticker();
        self.screen().finish(stats);
    }

    fn stop_ticker(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.ticker.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.stop_ticker();
        self.screen().restore();
    }
}

impl Ui for Terminal {
    fn text(&mut self, piece: &str) {
        self.screen().text(piece);
    }
    fn step(&mut self, label: &str, detail: &str) {
        self.screen().step(label, detail);
    }
    fn output(&mut self, line: &str) {
        self.screen().output(line);
    }
    fn note(&mut self, text: &str) {
        self.screen().note(text);
    }
    fn step_done(&mut self, ok: bool) {
        self.screen().end_step(Some(ok));
    }
    fn thinking(&mut self) {
        self.screen().thinking();
    }
    fn waiting(&mut self, what: &str) {
        self.screen().waiting(what);
    }
    fn usage(&mut self, usage: &Usage) {
        self.screen().usage(usage);
    }
    fn plan(&mut self, steps: &[PlanStep]) {
        self.screen().plan(steps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::strip_ansi;

    /// A writer the test can read back.
    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);

    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Buf {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    fn terminal(mode: Mode, width: usize) -> (Terminal, Buf) {
        let buf = Buf::default();
        (Terminal::with_output(Box::new(buf.clone()), mode, Some(width), None), buf)
    }

    fn usage() -> Usage {
        Usage {
            prompt: 1200,
            cached: 900,
            completion: 230,
            context: 32768,
            tokens_per_sec: 14.2,
            session_prompt: 3600,
            session_completion: 500,
        }
    }

    #[test]
    fn short_counts() {
        assert_eq!(short(0), "0");
        assert_eq!(short(950), "950");
        assert_eq!(short(1000), "1k");
        assert_eq!(short(1234), "1.2k");
        assert_eq!(short(38_400), "38k");
        assert_eq!(short(1_250_000), "1.2M");
    }

    #[test]
    fn token_line() {
        let mut t = Totals::default();
        t.add(&usage());
        let line = usage_parts(&t, &usage()).join(" · ");
        assert_eq!(line, "1.2k in (900 cached) · 230 out · 14 tok/s · context 4% · 4.1k this session");
        let bare = Usage { prompt: 10, completion: 2, ..Usage::default() };
        let mut t = Totals::default();
        t.add(&bare);
        assert_eq!(usage_parts(&t, &bare).join(" · "), "10 in · 2 out");
    }

    #[test]
    fn context_fill_is_capped() {
        assert_eq!(context_fill(&Usage { prompt: 900, completion: 100, context: 4000, ..Usage::default() }), 25);
        assert_eq!(context_fill(&Usage { prompt: 9000, context: 4000, ..Usage::default() }), 100);
        assert_eq!(context_fill(&Usage::default()), 0);
    }

    #[test]
    fn fit_breaks_between_parts() {
        let parts: Vec<String> = ["aaaa", "bbbb", "cccc"].map(String::from).to_vec();
        assert_eq!(fit(&parts, 80), ["aaaa · bbbb · cccc"]);
        assert_eq!(fit(&parts, 12), ["aaaa · bbbb", "cccc"]);
        assert_eq!(fit(&parts, 2), ["aaaa", "bbbb", "cccc"]);
    }

    #[test]
    fn elapsed_reads_well() {
        assert_eq!(elapsed(Duration::from_millis(420)), "0.4s");
        assert_eq!(elapsed(Duration::from_secs(12)), "12s");
        assert_eq!(elapsed(Duration::from_secs(185)), "3m 05s");
    }

    #[test]
    fn plain_output_for_pipes() {
        let (mut t, buf) = terminal(Mode::PLAIN, 80);
        t.thinking();
        t.step("exec", "du -sh ~/*");
        t.output("8.1G /home/a/Downloads");
        t.step_done(true);
        t.thinking();
        t.text("Your **Downloads** folder");
        t.text(" is the biggest:\n\n- 8.1 GB\n");
        t.finish(None);
        drop(t);
        let out = buf.text();
        assert!(!out.contains('\x1b'), "{out:?}");
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("✓ Run du -sh ~/*"), "{lines:?}");
        assert_eq!(&lines[1..], ["", "Your Downloads folder is the biggest:", "", "• 8.1 GB"]);
    }

    #[test]
    fn rows_fit_the_width() {
        for width in [40, 80, 120] {
            let mode = Mode { live: false, color: true, unicode: true };
            let (mut t, buf) = terminal(mode, width);
            t.step("exec", &"x".repeat(300));
            t.output(&"y".repeat(300));
            t.step_done(false);
            t.text(&format!("{}\n", "word ".repeat(80)));
            t.usage(&usage());
            t.finish(None);
            drop(t);
            for line in buf.text().lines() {
                assert!(term::width(line) < width, "{width}: {:?}", strip_ansi(line));
            }
        }
    }

    #[test]
    fn the_first_row_carries_the_label() {
        let mode = Mode { live: false, color: true, unicode: true };
        let (mut t, buf) = terminal(mode, 80);
        t.text("hello\nthere\n");
        t.finish(None);
        drop(t);
        let out = strip_ansi(&buf.text());
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "Tripppin  hello");
        assert_eq!(lines[1], "          there");
    }

    #[test]
    fn a_failed_step_keeps_its_last_lines() {
        let (mut t, buf) = terminal(Mode::PLAIN, 80);
        t.step("exec", "make");
        for i in 0..10 {
            t.output(&format!("line {i}"));
        }
        t.step_done(false);
        t.finish(None);
        drop(t);
        let out = buf.text();
        assert!(out.contains("✗ Run make"), "{out}");
        assert!(out.contains("… 7 earlier lines") && out.contains("line 9") && !out.contains("line 6"), "{out}");
    }

    #[test]
    fn a_successful_step_collapses_to_one_row() {
        let (mut t, buf) = terminal(Mode::PLAIN, 80);
        t.step("exec", "ls");
        t.output("a");
        t.output("b");
        t.step_done(true);
        t.finish(None);
        drop(t);
        assert_eq!(buf.text().lines().count(), 1);
    }

    #[test]
    fn plans_show_as_checklists_when_they_change() {
        let (mut t, buf) = terminal(Mode::PLAIN, 80);
        let plan = vec![
            PlanStep { text: "find big folders".into(), done: true },
            PlanStep { text: "clear caches".into(), done: false },
        ];
        t.plan(&plan);
        t.plan(&plan);
        t.finish(None);
        drop(t);
        let out = buf.text();
        assert_eq!(out.matches("Plan · 1 of 2 done").count(), 1, "{out}");
        assert!(out.contains("✓ find big folders\n○ clear caches"), "{out}");
    }

    #[test]
    fn live_region_redraws_in_place() {
        let mode = Mode { live: true, color: true, unicode: true };
        let buf = Buf::default();
        let mut t = Terminal::with_output(Box::new(buf.clone()), mode, Some(60), None);
        t.thinking();
        t.text("partial line");
        t.text(" more");
        t.text("\ndone\n");
        t.finish(None);
        drop(t);
        let out = buf.text();
        // The partial line was drawn, erased and finally committed once.
        assert!(out.contains("\x1b[J"));
        assert!(out.ends_with("\x1b[?25h") || !out.contains("\x1b[?25l"));
        let committed = strip_ansi(&out);
        assert!(committed.contains("partial line more\r\n"), "{committed:?}");
    }

    #[test]
    fn status_says_what_its_waiting_for() {
        let mode = Mode { live: false, color: false, unicode: true };
        let (mut t, _buf) = terminal(mode, 80);
        t.thinking();
        assert!(t.screen().status().unwrap().starts_with("⠋ Thinking · 0.0s"));
        t.waiting("loading the model");
        assert!(t.screen().status().unwrap().contains("Loading the model"));
        t.text("hi");
        assert!(t.screen().status().is_none(), "hidden while text streams");
        t.step("exec", "ls");
        assert!(t.screen().status().is_none());
    }

    #[test]
    fn usage_totals_add_up_over_a_task() {
        let (mut t, buf) = terminal(Mode { live: false, color: true, unicode: true }, 100);
        t.usage(&Usage { prompt: 1000, completion: 100, ..usage() });
        t.usage(&Usage { prompt: 1500, completion: 200, ..usage() });
        t.finish(None);
        drop(t);
        let out = strip_ansi(&buf.text());
        assert!(out.contains("2.5k in (1.8k cached) · 300 out"), "{out}");
    }
}
