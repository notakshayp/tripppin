//! The orb: the AI, resting in a corner of the desktop (bottom right to start with).
//!
//! Click it or press Super+Space (`tripppin-ui orb --toggle`) and a glowing bar asks what you
//! want. While it works, a card next to the orb shows each step live: done, running (with the
//! command's latest output) and the answer. The same agent as the terminal does the work.
//! The header says what it's waiting on with a running clock and a breathing dot; a hairline
//! under it fills with the context used (and sweeps while it works). A plan, when the agent
//! keeps one, sits as a checklist at the top of the request; each request ends with a dim line
//! of the tokens it took, and the footer keeps the conversation's total.
//! Drag the orb and it springs to the nearest corner; right-click tucks it away to a small
//! dot. The card minimises back into the orb.
//!
//! Each step in the card opens on a click to show the command and what it printed. A follow-up
//! continues the same conversation; the history button lists earlier conversations (the orb's
//! and the terminal's) to read or continue; "continue in terminal" opens the conversation in
//! the AI shell.
//!
//! Several requests can run at once, each with its own session, stop flag, thread and events
//! (tagged with the session id). The card shows one; chips under its header switch between
//! them, and the history lists them first ("Running now", "Open now") with a way back. While
//! the card is open the rest of the screen dims softly; clicking the dim puts the card away.

use crate::common::{self, icons, label};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use trip::agent::{self, Ui};
use trip::follow;
use trip::model::{PlanStep, Usage};
use trip::session::Session;

const SUGGESTIONS: [&str; 3] = ["Free up some space", "Connect my headphones", "What changed today?"];

const ORB_TIP: &str = "Ask Tripppin (Super + Space)\ndrag to move it · right-click to tuck it away";

/// Space between the orb's glow and the screen edge, and room for the bar at the top.
const EDGE: i32 = 6;
const BELOW_BAR: i32 = 46;
/// The glow around the orb, inside its window (clicks pass through it).
const GLOW: i32 = 14;
/// The conversation's height in the card: room for a few steps from the start, then it scrolls.
const MIN_LIST: i32 = 120;
const MAX_LIST: i32 = 360;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    const ALL: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];

    fn name(self) -> &'static str {
        match self {
            Corner::TopLeft => "top-left",
            Corner::TopRight => "top-right",
            Corner::BottomLeft => "bottom-left",
            Corner::BottomRight => "bottom-right",
        }
    }

    fn top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }

    fn left(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::BottomLeft)
    }

    /// The corner nearest a point on a `w` x `h` screen.
    pub fn nearest(x: f64, y: f64, w: f64, h: f64) -> Corner {
        match (x < w / 2.0, y < h / 2.0) {
            (true, true) => Corner::TopLeft,
            (false, true) => Corner::TopRight,
            (true, false) => Corner::BottomLeft,
            (false, false) => Corner::BottomRight,
        }
    }

    /// Anchors a layer window to this corner.
    fn anchor(self, w: &gtk::Window) {
        w.set_anchor(Edge::Top, self.top());
        w.set_anchor(Edge::Bottom, !self.top());
        w.set_anchor(Edge::Left, self.left());
        w.set_anchor(Edge::Right, !self.left());
    }

    /// Sets the margins from the two anchored edges: `side` from the left or right edge,
    /// `end` from the top or bottom.
    fn place(self, w: &gtk::Window, side: i32, end: i32) {
        w.set_margin(if self.left() { Edge::Left } else { Edge::Right }, side);
        w.set_margin(if self.top() { Edge::Top } else { Edge::Bottom }, end);
    }

    fn rest(self) -> (i32, i32) {
        (EDGE, if self.top() { BELOW_BAR } else { EDGE })
    }
}

/// Where the orb rests and whether it's tucked away, remembered in ~/.config/tripppin/orb.
fn prefs_file() -> std::path::PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("tripppin/orb")
}

pub fn parse_prefs(text: &str) -> (Corner, bool) {
    let corner = Corner::ALL.into_iter().find(|c| text.lines().any(|l| l.trim() == c.name()));
    (corner.unwrap_or(Corner::BottomRight), text.lines().any(|l| l.trim() == "tucked"))
}

fn load_prefs() -> (Corner, bool) {
    parse_prefs(&std::fs::read_to_string(prefs_file()).unwrap_or_default())
}

fn save_prefs(corner: Corner, tucked: bool) {
    let file = prefs_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, format!("{}\n{}", corner.name(), if tucked { "tucked\n" } else { "" }));
}

/// What the agent's thread tells the window.
enum Event {
    Text(String),
    Step(String, String),
    Output(String),
    Note(String),
    StepDone(bool),
    Thinking,
    /// What it's waiting on, in a few words.
    Waiting(String),
    /// Token counts after a model reply.
    Usage(Usage),
    /// The task plan, whole, each time it changes.
    Plan(Vec<PlanStep>),
    /// Finished, with the session back from the agent's thread.
    Done(Result<(), String>, Box<Session>),
}

/// The agent's progress, sent across to the GTK thread, tagged with its request's id so each
/// request's events reach that request whichever is on show.
struct Sender {
    tx: async_channel::Sender<(String, Event)>,
    id: String,
}

impl Sender {
    fn send(&self, event: Event) {
        let _ = self.tx.send_blocking((self.id.clone(), event));
    }
}

impl Ui for Sender {
    fn text(&mut self, piece: &str) {
        self.send(Event::Text(piece.to_string()));
    }
    fn step(&mut self, label: &str, detail: &str) {
        self.send(Event::Step(label.to_string(), detail.to_string()));
    }
    fn output(&mut self, line: &str) {
        self.send(Event::Output(line.to_string()));
    }
    fn note(&mut self, text: &str) {
        self.send(Event::Note(text.to_string()));
    }
    fn step_done(&mut self, ok: bool) {
        self.send(Event::StepDone(ok));
    }
    fn thinking(&mut self) {
        self.send(Event::Thinking);
    }
    fn waiting(&mut self, what: &str) {
        self.send(Event::Waiting(what.to_string()));
    }
    fn usage(&mut self, usage: &Usage) {
        self.send(Event::Usage(usage.clone()));
    }
    fn plan(&mut self, steps: &[PlanStep]) {
        self.send(Event::Plan(steps.to_vec()));
    }
}

/// Turns a step into words: "installing org.videolan.VLC", "checking disk space".
pub fn describe(label: &str, detail: &str) -> String {
    if label != "exec" && label != "exec in the background" {
        return match label {
            "read" => format!("Reading {}", short(detail)),
            "write" => format!("Writing {}", short(detail)),
            other => other.to_string(),
        };
    }
    // Look past `sudo` and its options to the command itself.
    let mut words: Vec<&str> = detail.split_whitespace().collect();
    if words.first() == Some(&"sudo") {
        words.remove(0);
        while words.first().is_some_and(|w| w.starts_with('-')) {
            words.remove(0);
        }
        if words.is_empty() {
            return "Asking for a password".into();
        }
    }
    let arg = |i: usize| words.iter().skip(i).find(|w| !w.starts_with('-')).copied().unwrap_or("");
    match (words.first().copied().unwrap_or(""), words.get(1).copied().unwrap_or("")) {
        ("flatpak", "install") => format!("Installing {}", words.last().copied().unwrap_or("An app")),
        ("flatpak", "search") => format!("Searching for {}", arg(2)),
        ("flatpak", "uninstall") => format!("Removing {}", words.last().copied().unwrap_or("An app")),
        ("flatpak", "run") => format!("Opening {}", arg(2)),
        ("dnf", "install") => format!("Installing {}", words.last().copied().unwrap_or("A package")),
        ("df", _) | ("du", _) => "Checking disk space".into(),
        ("free", _) => "Checking memory".into(),
        ("ls", _) | ("find", _) => "Looking through files".into(),
        ("ps", _) | ("top", _) => "Checking what's running".into(),
        ("nmcli", _) => "Checking the network".into(),
        ("bluetoothctl", _) => "Talking to Bluetooth".into(),
        ("curl", _) | ("wget", _) => "downloading".into(),
        ("mkdir", _) => format!("Making {}", short(arg(1))),
        ("rm", _) => "Deleting files".into(),
        ("cp", _) | ("mv", _) | ("rsync", _) => "Moving files".into(),
        ("xdg-open", _) => format!("Opening {}", short(arg(1))),
        (first, _) if !first.is_empty() => format!("Running {first}"),
        _ => "Running a command".into(),
    }
}

/// The model writes light Markdown; show **bold** and `code` properly instead of the marks.
pub fn markup(text: &str) -> String {
    let mut out = String::new();
    let (mut bold, mut code) = (false, false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') && !code => {
                chars.next();
                out.push_str(if bold { "</b>" } else { "<b>" });
                bold = !bold;
            }
            '`' => {
                out.push_str(if code { "</span>" } else { "<span font_family=\"DM Mono\">" });
                code = !code;
            }
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    // Close anything a half-streamed answer left open, so the markup stays valid.
    if code {
        out.push_str("</span>");
    }
    if bold {
        out.push_str("</b>");
    }
    out
}

fn short(path: &str) -> String {
    path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path).to_string()
}

/// A token count, short: 230, 1.2k, 38k, 1.4m.
pub fn tokens(n: usize) -> String {
    let one_place = |x: f64, unit: &str| {
        let s = format!("{x:.1}");
        format!("{}{unit}", s.strip_suffix(".0").unwrap_or(&s))
    };
    match n {
        0..1000 => n.to_string(),
        1000..10_000 => one_place(n as f64 / 1000.0, "k"),
        10_000..1_000_000 => format!("{}k", (n as f64 / 1000.0).round()),
        _ => one_place(n as f64 / 1_000_000.0, "m"),
    }
}

/// How full the model's context is, in percent, when its size is known.
pub fn context_percent(u: &Usage) -> Option<u32> {
    (u.context > 0).then(|| (((u.prompt + u.completion) as f64 / u.context as f64) * 100.0).round().min(100.0) as u32)
}

/// What one request has cost so far: every model reply it took, added up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnTokens {
    pub prompt: usize,
    pub cached: usize,
    pub completion: usize,
    /// From the latest reply.
    pub tokens_per_sec: f64,
    pub context: Option<u32>,
    pub replies: usize,
}

impl TurnTokens {
    pub fn add(&mut self, u: &Usage) {
        self.prompt += u.prompt;
        self.cached += u.cached;
        self.completion += u.completion;
        self.replies += 1;
        if u.tokens_per_sec > 0.0 {
            self.tokens_per_sec = u.tokens_per_sec;
        }
        if let Some(p) = context_percent(u) {
            self.context = Some(p);
        }
    }

    /// "1.2k in · 230 out · 14 tok/s · context 38%"
    pub fn line(&self) -> String {
        let mut words = vec![format!("{} in", tokens(self.prompt))];
        if self.cached > 0 {
            words.push(format!("{} cached", tokens(self.cached)));
        }
        words.push(format!("{} out", tokens(self.completion)));
        let tps = self.tokens_per_sec;
        if tps > 0.0 {
            words.push(if tps < 10.0 { format!("{tps:.1} tok/s") } else { format!("{tps:.0} tok/s") });
        }
        if let Some(p) = self.context {
            words.push(format!("context {p}%"));
        }
        words.join(" · ")
    }
}

/// The status line's words, kept to one short line ("running dnf install -y vlc…").
pub fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// The plan's step to work on now: the first not yet done.
pub fn current_step(steps: &[PlanStep]) -> Option<usize> {
    steps.iter().position(|s| !s.done)
}

/// The round mark for a finished step: the icon centred in its circle both ways.
fn tick(glyph: char, failed: bool) -> gtk::Label {
    let t = common::icon(glyph, &["tick"]);
    if failed {
        t.add_css_class("failed");
    }
    t.set_xalign(0.5);
    t.set_yalign(0.5);
    t.set_justify(gtk::Justification::Center);
    t.set_halign(gtk::Align::Center);
    t.set_valign(gtk::Align::Center);
    t
}

/// Lines of a step's output kept to show when it's opened.
const OUTPUT_LINES: usize = 60;

/// One row in the card: a step, which opens on a click to show the whole command and the end
/// of its output.
struct StepRow {
    root: gtk::Box,
    done: Cell<bool>,
    mark: gtk::Stack,
    title: gtk::Label,
    detail: gtk::Label,
    output: gtk::Label,
    lines: RefCell<VecDeque<String>>,
}

impl StepRow {
    fn new(title: &str, full: &str) -> Rc<Self> {
        let mark = gtk::Stack::new();
        mark.set_valign(gtk::Align::Start);
        mark.set_margin_top(1);
        mark.set_transition_type(gtk::StackTransitionType::Crossfade);
        mark.set_transition_duration(160);
        mark.add_named(&common::spinner(16), Some("running"));
        mark.add_named(&tick(icons::CHECK, false), Some("done"));
        mark.add_named(&tick(icons::ALERT, true), Some("failed"));
        mark.set_visible_child_name("running");

        let title = label(title, &["step-title", "running"]);
        title.set_wrap(true);
        title.set_hexpand(true);
        let chevron = common::icon(icons::CHEVRON_RIGHT, &["step-chevron"]);
        chevron.set_valign(gtk::Align::Start);
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        head.append(&title);
        head.append(&chevron);
        let detail = label(full.lines().next().unwrap_or(""), &["step-detail"]);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        detail.set_max_width_chars(46);

        // Opened: the whole command, then what it printed.
        let command = label(full, &["step-command"]);
        command.set_wrap(true);
        command.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        command.set_selectable(true);
        let output = label("", &["step-output"]);
        output.set_wrap(true);
        output.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        output.set_selectable(true);
        output.set_visible(false);
        let more = gtk::Box::new(gtk::Orientation::Vertical, 6);
        more.add_css_class("step-more");
        more.append(&command);
        more.append(&output);
        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        revealer.set_transition_duration(220);
        revealer.set_child(Some(&more));

        // Only the step's own lines open and close it; the opened part is for selecting text.
        let line = gtk::Box::new(gtk::Orientation::Vertical, 2);
        line.add_css_class("step-line");
        line.append(&head);
        line.append(&detail);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.append(&line);
        text.append(&revealer);
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        root.add_css_class("step");
        root.set_tooltip_text(Some("Click to see the command and what it printed"));
        root.append(&mark);
        root.append(&text);
        common::play(&root, "rise");
        let row = Rc::new(StepRow {
            root,
            done: Cell::new(false),
            mark,
            title,
            detail,
            output,
            lines: RefCell::new(VecDeque::new()),
        });
        let click = gtk::GestureClick::new();
        {
            let (row, revealer, chevron) = (Rc::downgrade(&row), revealer.clone(), chevron.clone());
            click.connect_released(move |_, _, _, _| {
                let open = !revealer.reveals_child();
                if open && let Some(row) = row.upgrade() {
                    row.show_output();
                }
                revealer.set_reveal_child(open);
                chevron.set_text(&(if open { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT }).to_string());
            });
        }
        line.add_controller(click);
        row
    }

    /// A finished step from the history, with what it printed.
    fn past(step: &trip::session::Step) -> Rc<Self> {
        let row = StepRow::new(&describe(&step.tool, &step.detail), &step.detail);
        row.root.remove_css_class("rise-a");
        row.root.remove_css_class("rise-b");
        row.lines.borrow_mut().extend(step.output.lines().map(String::from));
        match step.ok {
            Some(ok) => row.finish(ok),
            None => {
                row.mark.set_visible_child_name("done");
                row.title.remove_css_class("running");
            }
        }
        row
    }

    fn add_line(&self, line: &str) {
        self.detail.set_text(line);
        let mut lines = self.lines.borrow_mut();
        lines.push_back(line.to_string());
        if lines.len() > OUTPUT_LINES {
            lines.pop_front();
        }
    }

    fn show_output(&self) {
        let lines = self.lines.borrow();
        let skip = lines.len().saturating_sub(OUTPUT_LINES);
        let text = lines.iter().skip(skip).cloned().collect::<Vec<_>>().join("\n");
        self.output.set_text(text.trim());
        self.output.set_visible(!text.trim().is_empty());
    }

    fn finish(&self, ok: bool) {
        if self.done.replace(true) {
            return;
        }
        self.mark.set_visible_child_name(if ok { "done" } else { "failed" });
        self.title.remove_css_class("running");
    }
}

/// One line of the plan checklist: its mark (ticked, current or still to come) and words.
struct PlanRow {
    root: gtk::Box,
    mark: gtk::Stack,
    text: gtk::Label,
}

impl PlanRow {
    fn new(text: &str) -> Self {
        let mark = gtk::Stack::new();
        mark.add_css_class("plan-mark");
        mark.set_valign(gtk::Align::Start);
        mark.set_transition_type(gtk::StackTransitionType::Crossfade);
        mark.set_transition_duration(160);
        let todo = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        todo.add_css_class("plan-todo");
        todo.set_halign(gtk::Align::Center);
        todo.set_valign(gtk::Align::Center);
        let now = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        now.add_css_class("plan-now");
        now.set_halign(gtk::Align::Center);
        now.set_valign(gtk::Align::Center);
        let done = common::icon(icons::CHECK, &["plan-done"]);
        done.set_halign(gtk::Align::Center);
        done.set_valign(gtk::Align::Center);
        mark.add_named(&todo, Some("todo"));
        mark.add_named(&now, Some("now"));
        mark.add_named(&done, Some("done"));
        let text = label(text, &["plan-text"]);
        text.set_wrap(true);
        text.set_hexpand(true);
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("plan-step");
        root.append(&mark);
        root.append(&text);
        PlanRow { root, mark, text }
    }

    fn show(&self, state: &str) {
        self.mark.set_visible_child_name(state);
        for s in ["todo", "now", "done"] {
            if s == state {
                self.root.add_css_class(s);
            } else {
                self.root.remove_css_class(s);
            }
        }
    }
}

/// One request in the card: what was asked (for follow-ups), its plan, steps and answer, and
/// what it cost in tokens.
struct TurnView {
    root: gtk::Box,
    plan_box: gtk::Box,
    plan_caption: gtk::Label,
    plan_rows: RefCell<Vec<PlanRow>>,
    steps_box: gtk::Box,
    answer: gtk::Label,
    usage: gtk::Label,
    tokens: RefCell<TurnTokens>,
    steps: RefCell<Vec<Rc<StepRow>>>,
}

impl TurnView {
    fn new(request: Option<&str>) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.add_css_class("turn");
        if let Some(q) = request {
            let you = label(q, &["follow-up"]);
            you.set_wrap(true);
            you.set_halign(gtk::Align::End);
            you.set_xalign(0.0);
            you.set_max_width_chars(36);
            root.append(&you);
            common::play(&you, "rise");
        }
        // The plan, once the agent writes one: a checklist that ticks itself off in place.
        let plan_box = gtk::Box::new(gtk::Orientation::Vertical, 3);
        plan_box.add_css_class("plan");
        plan_box.set_visible(false);
        let plan_caption = label("Plan", &["plan-caption"]);
        plan_box.append(&plan_caption);
        let steps_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let answer = label("", &["answer"]);
        answer.set_wrap(true);
        answer.set_max_width_chars(42);
        answer.set_selectable(true);
        answer.set_visible(false);
        let usage = label("", &["turn-usage"]);
        usage.set_wrap(true);
        usage.set_visible(false);
        root.append(&plan_box);
        root.append(&steps_box);
        root.append(&answer);
        root.append(&usage);
        Rc::new(TurnView {
            root,
            plan_box,
            plan_caption,
            plan_rows: RefCell::new(Vec::new()),
            steps_box,
            answer,
            usage,
            tokens: RefCell::new(TurnTokens::default()),
            steps: RefCell::new(Vec::new()),
        })
    }

    /// Shows the plan, reusing the rows already there so it updates in place rather than
    /// flashing: done steps ticked, the current one highlighted.
    fn set_plan(&self, steps: &[PlanStep]) {
        let mut rows = self.plan_rows.borrow_mut();
        while rows.len() > steps.len() {
            if let Some(row) = rows.pop() {
                self.plan_box.remove(&row.root);
            }
        }
        let current = current_step(steps);
        for (i, step) in steps.iter().enumerate() {
            if i == rows.len() {
                let row = PlanRow::new(&step.text);
                self.plan_box.append(&row.root);
                common::play(&row.root, "fade");
                rows.push(row);
            } else if rows[i].text.text() != step.text {
                rows[i].text.set_text(&step.text);
            }
            let state = if step.done {
                "done"
            } else if Some(i) == current {
                "now"
            } else {
                "todo"
            };
            rows[i].show(state);
        }
        let done = steps.iter().filter(|s| s.done).count();
        self.plan_caption.set_text(&format!("Plan · {done} of {}", steps.len()));
        if !steps.is_empty() && !self.plan_box.is_visible() {
            common::play(&self.plan_box, "fade");
        }
        self.plan_box.set_visible(!steps.is_empty());
    }

    /// Adds a model reply's tokens to this request's line under it.
    fn add_usage(&self, u: &Usage) {
        let mut t = self.tokens.borrow_mut();
        t.add(u);
        self.usage.set_text(&t.line());
        self.usage.set_tooltip_text(Some(&format!(
            "{} model repl{} for this request",
            t.replies,
            if t.replies == 1 { "y" } else { "ies" }
        )));
        if !self.usage.is_visible() {
            self.usage.set_visible(true);
            common::play(&self.usage, "fade");
        }
    }

    fn add_step(&self, row: Rc<StepRow>) {
        self.steps_box.append(&row.root);
        self.steps.borrow_mut().push(row);
    }

    fn last(&self) -> Option<Rc<StepRow>> {
        self.steps.borrow().last().cloned()
    }

    fn set_answer(&self, text: &str) {
        let shown = text.trim();
        self.answer.set_markup(&markup(shown));
        if !shown.is_empty() && !self.answer.is_visible() {
            self.answer.set_visible(true);
            common::play(&self.answer, "fade");
        }
        if shown.is_empty() {
            self.answer.set_visible(false);
        }
    }
}

/// The hairline under the card's header: how full the model's context is, with a soft sweep
/// running along it while the agent works.
#[derive(Clone, Copy, Debug, Default)]
struct Meter {
    /// Context used, 0 to 1, once a reply has said.
    fill: Option<f64>,
    busy: bool,
}

fn meter(state: Rc<Cell<Meter>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.add_css_class("context-meter");
    area.set_content_height(2);
    area.set_hexpand(true);
    area.set_draw_func(move |a, cr, w, h| {
        let m = state.get();
        let (w, h) = (f64::from(w), f64::from(h));
        // A neutral hairline the whole width, the context used in green over it.
        cr.set_source_rgba(0.949, 0.961, 0.937, 0.07);
        cr.rectangle(0.0, 0.0, w, h);
        cr.fill().ok();
        if let Some(f) = m.fill {
            let (r, g, b) = common::GREEN;
            cr.set_source_rgba(r, g, b, 0.55);
            cr.rectangle(0.0, 0.0, w * f.clamp(0.0, 1.0), h);
            cr.fill().ok();
        }
        let animate = gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations());
        if m.busy && animate {
            // A soft-edged light that crosses every 1.8 s, easing in and out.
            let t = a.frame_clock().map_or(0.0, |c| c.frame_time() as f64 / 1_000_000.0);
            let k = (t / 1.8).fract();
            let k = 0.5 - 0.5 * (k * std::f64::consts::PI).cos();
            let seg = w * 0.28;
            let x = -seg + (w + seg) * k;
            let grad = gtk::cairo::LinearGradient::new(x, 0.0, x + seg, 0.0);
            let (r, g, b) = common::GREEN;
            grad.add_color_stop_rgba(0.0, r, g, b, 0.0);
            grad.add_color_stop_rgba(0.5, r, g, b, 0.8);
            grad.add_color_stop_rgba(1.0, r, g, b, 0.0);
            cr.set_source(&grad).ok();
            cr.rectangle(x, 0.0, seg, h);
            cr.fill().ok();
        }
    });
    area
}

/// One conversation the orb has open: running, or finished and still on show. Several can run
/// at once, each with its own session, stop flag, thread and events; the card shows one.
struct Request {
    /// The session's id, which also tags the request's events.
    id: String,
    title: String,
    /// What the card's header says over it ("you asked", or when it was).
    caption: String,
    /// Away in the agent's thread while it works.
    session: RefCell<Option<Session>>,
    cancel: RefCell<Arc<AtomicBool>>,
    busy: Cell<bool>,
    stopping: Cell<bool>,
    /// Finished while another was on show; it stays until it's been looked at.
    unseen: Cell<bool>,
    started: Cell<Option<Instant>>,
    /// What it's doing or waiting on, for the status line.
    phase: RefCell<String>,
    /// The status once it's finished ("done in 0:12").
    ended: RefCell<String>,
    answer: RefCell<String>,
    turn: RefCell<Option<Rc<TurnView>>>,
    /// Its turns; in the card's scroller while it's the one on show.
    turns: gtk::Box,
    /// Its context meter, and the tooltip saying how full.
    meter: Cell<Meter>,
    meter_tip: RefCell<String>,
    /// The conversation's tokens so far, and the tooltip with the split.
    total: RefCell<(String, String)>,
}

impl Request {
    fn new(session: Session, title: &str, caption: &str) -> Rc<Self> {
        Rc::new(Request {
            id: session.id.clone(),
            title: title.to_string(),
            caption: caption.to_string(),
            session: RefCell::new(Some(session)),
            cancel: RefCell::new(Arc::new(AtomicBool::new(false))),
            busy: Cell::new(false),
            stopping: Cell::new(false),
            unseen: Cell::new(false),
            started: Cell::new(None),
            phase: RefCell::new(String::new()),
            ended: RefCell::new(String::new()),
            answer: RefCell::new(String::new()),
            turn: RefCell::new(None),
            turns: gtk::Box::new(gtk::Orientation::Vertical, 14),
            meter: Cell::new(Meter::default()),
            meter_tip: RefCell::new(String::new()),
            total: RefCell::new((String::new(), String::new())),
        })
    }

    /// "m:ss" since it started.
    fn elapsed(&self) -> String {
        let secs = self.started.get().map_or(0, |t| t.elapsed().as_secs());
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// What the tabs and the history say about a request the orb has open.
pub fn request_state(busy: bool, shown: bool, unseen: bool) -> &'static str {
    match (busy, shown, unseen) {
        (true, _, _) => "Running now",
        (false, true, _) => "Open now",
        (false, false, true) => "Finished, not opened yet",
        (false, false, false) => "Finished",
    }
}

/// The bar's word for what the orb is doing: nothing, "● working", or "● 2 running".
pub fn bar_status(running: usize) -> String {
    match running {
        0 => String::new(),
        1 => "● working".into(),
        n => format!("● {n} running"),
    }
}

#[derive(Default)]
struct State {
    /// Every request open in the orb: those running, the one on show, and any that finished
    /// unseen.
    requests: Vec<Rc<Request>>,
    shown: Option<Rc<Request>>,
}

struct Orb {
    corner_window: gtk::Window,
    button: gtk::Button,
    logo: gtk::DrawingArea,
    hint: gtk::Label,
    row: gtk::Box,
    corner: Cell<Corner>,
    tucked: Cell<bool>,
    model: gtk::Label,
    ask: gtk::Window,
    entry: gtk::Entry,
    card: gtk::Window,
    body: gtk::Box,
    pages: gtk::Stack,
    caption: gtk::Label,
    asked: gtk::Label,
    status: gtk::Label,
    working: gtk::DrawingArea,
    meter: gtk::DrawingArea,
    meter_state: Rc<Cell<Meter>>,
    total: gtk::Label,
    scroller: gtk::ScrolledWindow,
    /// Each open request's conversation; the one on show is visible.
    convos: gtk::Stack,
    /// A chip for each open request, to switch between them, when there's more than one.
    tabs: gtk::Box,
    tabs_row: gtk::ScrolledWindow,
    back: gtk::Button,
    /// The soft dim over the rest of the screen while the card is open.
    dim: gtk::Window,
    dim_box: gtk::Box,
    dim_wanted: Rc<Cell<bool>>,
    follow: gtk::Entry,
    follow_row: gtk::Box,
    terminal: gtk::Button,
    stop: gtk::Button,
    sessions: gtk::ListBox,
    chips: gtk::Box,
    found: gtk::Box,
    apps: RefCell<Vec<crate::apps::App>>,
    shade: gtk::Box,
    column: gtk::Box,
    state: RefCell<State>,
}

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("in.trippp.Orb")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let orb: Rc<RefCell<Option<Rc<Orb>>>> = Rc::new(RefCell::new(None));
    let started = orb.clone();
    app.connect_startup(move |app| {
        common::load_css();
        let built = build(app);
        *started.borrow_mut() = Some(built);
        write_status("");
        watch_caps_lock();
        crate::watch::start();
        crate::keeper::start();
    });
    app.connect_command_line(move |_, cmd| {
        let toggle = cmd.arguments().iter().any(|a| a.to_str() == Some("--toggle"));
        if let Some(o) = orb.borrow().as_ref()
            && toggle
        {
            o.toggle();
        }
        glib::ExitCode::SUCCESS
    });
    // `tripppin-ui orb [--toggle]`: drop the "orb" word for GTK's own argument parsing.
    let args: Vec<String> = std::env::args().take(1).chain(std::env::args().skip(2)).collect();
    app.run_with_args(&args)
}

fn layer_window(app: &gtk::Application, namespace: &str) -> gtk::Window {
    let w = gtk::Window::builder().application(app).decorated(false).build();
    w.add_css_class("tripppin");
    w.add_css_class("clear");
    w.init_layer_shell();
    w.set_namespace(Some(namespace));
    w
}

fn build(app: &gtk::Application) -> Rc<Orb> {
    // ---- the resting orb in its corner, with its hint beside it ----
    let (corner_at, tucked) = load_prefs();
    let corner = layer_window(app, "tripppin-orb");
    corner.set_layer(Layer::Top);
    // Measured from the screen's own edges (not below the bar), so dragging never jumps.
    corner.set_exclusive_zone(-1);
    corner.set_keyboard_mode(KeyboardMode::None);
    corner_at.anchor(&corner);
    let (side, end) = corner_at.rest();
    corner_at.place(&corner, side, end);
    let button = gtk::Button::new();
    button.add_css_class("orb");
    let logo = common::logo(18);
    button.set_child(Some(&logo));
    button.set_tooltip_text(Some(ORB_TIP));
    button.set_halign(gtk::Align::Center);
    button.set_valign(gtk::Align::Center);
    button.set_margin_top(GLOW);
    button.set_margin_bottom(GLOW);
    button.set_margin_start(GLOW);
    button.set_margin_end(GLOW);
    if tucked {
        button.add_css_class("tucked");
        logo.set_visible(false);
    }
    let hint = label("Super + Space", &["orb-hint"]);
    hint.set_valign(gtk::Align::Center);
    // The hint teaches the shortcut; once the orb has been opened it fades out for good.
    // (Faded, not removed: removing it would leave the corner window at its old width and
    // move the orb. It takes no clicks either way.)
    if used_file().exists() {
        hint.set_opacity(0.0);
    }
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    if corner_at.left() {
        row.append(&button);
        row.append(&hint);
    } else {
        row.append(&hint);
        row.append(&button);
    }
    corner.set_child(Some(&row));
    {
        let (w, b) = (corner.clone(), button.clone());
        corner.connect_map(move |_| clickable_orb_only(&w, &b));
    }

    // ---- the ask bar: full-screen dim with the glowing pill in the middle ----
    let ask = layer_window(app, "tripppin-ask");
    ask.set_layer(Layer::Overlay);
    for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        ask.set_anchor(e, true);
    }
    ask.set_exclusive_zone(-1);
    ask.set_keyboard_mode(KeyboardMode::Exclusive);
    let shade = gtk::Box::new(gtk::Orientation::Vertical, 14);
    shade.add_css_class("backdrop");
    let column = gtk::Box::new(gtk::Orientation::Vertical, 10);
    column.set_halign(gtk::Align::Center);
    column.set_valign(gtk::Align::Center);
    column.set_margin_bottom(140);
    column.set_size_request(560, -1);
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    pill.add_css_class("ask-pill");
    let pill_orb = gtk::CenterBox::new();
    pill_orb.add_css_class("ask-orb");
    pill_orb.set_valign(gtk::Align::Center);
    pill_orb.set_hexpand(false);
    pill_orb.set_size_request(38, 38);
    let s = common::logo(17);
    s.set_valign(gtk::Align::Center);
    pill_orb.set_center_widget(Some(&s));
    let entry = gtk::Entry::new();
    entry.add_css_class("ask");
    entry.set_hexpand(true);
    entry.set_valign(gtk::Align::Center);
    entry.set_placeholder_text(Some("Tell your computer what you want, or open an app"));
    let send = gtk::Button::new();
    send.add_css_class("send");
    send.set_valign(gtk::Align::Center);
    send.set_child(Some(&common::arrow(true)));
    send.set_tooltip_text(Some("Send"));
    pill.append(&pill_orb);
    pill.append(&entry);
    pill.append(&send);
    let chips = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    chips.set_margin_start(8);
    for text in SUGGESTIONS {
        let chip = gtk::Button::with_label(text);
        chip.add_css_class("chip");
        let entry = entry.clone();
        chip.connect_clicked(move |_| {
            entry.set_text(text);
            entry.grab_focus();
            entry.set_position(-1);
        });
        chips.append(&chip);
    }
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    let keys = label("Enter to send · Esc to close", &["hint"]);
    keys.set_hexpand(true);
    footer.append(&keys);
    let ask_history = gtk::Button::with_label("History");
    ask_history.add_css_class("link");
    ask_history.add_css_class("hint-link");
    ask_history.set_margin_end(12);
    ask_history.set_tooltip_text(Some("Earlier requests and their steps"));
    footer.append(&ask_history);
    let model = label(&model_line(), &["hint"]);
    footer.append(&model);
    column.append(&pill);
    column.append(&chips);
    // Typing an app's name offers to open it, like a launcher (the AI gets everything else).
    let found = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    found.set_margin_start(8);
    found.set_visible(false);
    column.append(&found);
    column.append(&footer);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&shade));
    shade.set_hexpand(true);
    shade.set_vexpand(true);
    overlay.add_overlay(&column);
    ask.set_child(Some(&overlay));

    // ---- the card, next to the orb: the conversation, or the history ----
    let card = layer_window(app, "tripppin-orb-card");
    // Above the dim (on the top layer), with the orb, which joins it while the dim is up.
    card.set_layer(Layer::Overlay);
    card.set_exclusive_zone(-1);
    card.set_keyboard_mode(KeyboardMode::OnDemand);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
    body.add_css_class("card");
    body.set_size_request(400, -1);
    // Room for the card's shadow inside its window.
    body.set_margin_top(24);
    body.set_margin_bottom(24);
    body.set_margin_start(24);
    body.set_margin_end(24);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let caption = label("You asked", &["caption"]);
    caption.set_hexpand(true);
    caption.set_valign(gtk::Align::Center);
    let status = label("", &["caption", "orb-status"]);
    status.set_valign(gtk::Align::Center);
    status.set_margin_end(4);
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    status.set_max_width_chars(34);
    // Breathing while it works, so it never looks stuck, even before the first step.
    let working = common::pulse_dot(8);
    working.set_valign(gtk::Align::Center);
    working.set_visible(false);
    let history = icon_button(icons::HISTORY, "Earlier requests");
    let new = icon_button(icons::NEW, "Ask something new");
    let minimise = icon_button(icons::MINIMIZE, "Minimise into the orb");
    top.append(&caption);
    top.append(&working);
    top.append(&status);
    top.append(&new);
    top.append(&history);
    top.append(&minimise);

    // The conversation: the first request as a title, then each request's steps and answer.
    let talk = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let asked = label("", &["asked"]);
    asked.set_wrap(true);
    asked.set_max_width_chars(44);
    // Steps and answers scroll once they outgrow the card; the newest stays in view. The
    // request on show puts its own turns in here.
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_propagate_natural_height(true);
    scroller.set_max_content_height(MAX_LIST);
    let convos = gtk::Stack::new();
    convos.set_vhomogeneous(false);
    scroller.set_child(Some(&convos));
    let follow_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    follow_row.add_css_class("follow-row");
    let follow = gtk::Entry::new();
    follow.add_css_class("follow");
    follow.set_hexpand(true);
    follow.set_placeholder_text(Some("Ask a follow-up"));
    let follow_send = gtk::Button::new();
    follow_send.add_css_class("send");
    follow_send.add_css_class("small");
    follow_send.set_valign(gtk::Align::Center);
    follow_send.set_child(Some(&common::arrow(true)));
    follow_send.set_tooltip_text(Some("Send"));
    follow_row.append(&follow);
    follow_row.append(&follow_send);
    follow_row.set_visible(false);
    talk.append(&asked);
    talk.append(&scroller);
    talk.append(&follow_row);

    // The history: earlier sessions, newest first.
    let sessions = gtk::ListBox::new();
    sessions.add_css_class("sessions");
    sessions.set_selection_mode(gtk::SelectionMode::None);
    let past = gtk::ScrolledWindow::new();
    past.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    past.set_propagate_natural_height(true);
    past.set_max_content_height(MAX_LIST + 60);
    past.set_child(Some(&sessions));
    // Back to the request that's running or was open (Escape does the same).
    let back = gtk::Button::new();
    let back_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    back_box.append(&common::icon(icons::CHEVRON_LEFT, &["link-icon"]));
    back_box.append(&gtk::Label::new(Some("Back to current request")));
    back.set_child(Some(&back_box));
    back.add_css_class("link");
    back.add_css_class("history-back");
    back.set_halign(gtk::Align::Start);
    back.set_tooltip_text(Some("Back to the request that's running or open (Esc)"));
    let history_page = gtk::Box::new(gtk::Orientation::Vertical, 8);
    history_page.append(&back);
    history_page.append(&past);

    let pages = gtk::Stack::new();
    pages.set_transition_type(gtk::StackTransitionType::Crossfade);
    pages.set_transition_duration(180);
    pages.set_vhomogeneous(false);
    pages.set_interpolate_size(true);
    pages.add_named(&talk, Some("talk"));
    pages.add_named(&history_page, Some("history"));

    let foot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    foot.add_css_class("card-foot");
    let terminal = gtk::Button::new();
    let terminal_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    terminal_box.append(&common::icon(icons::TERMINAL, &["link-icon"]));
    terminal_box.append(&gtk::Label::new(Some("Continue in terminal")));
    terminal.set_child(Some(&terminal_box));
    terminal.set_valign(gtk::Align::Center);
    terminal.set_tooltip_text(Some("Open this conversation in the terminal, with every step, and keep going there"));
    terminal.add_css_class("link");
    terminal.set_hexpand(true);
    terminal.set_halign(gtk::Align::Start);
    let stop = gtk::Button::with_label("Stop");
    stop.add_css_class("stop");
    // The conversation's tokens so far, dim, beside the stop button.
    let total = label("", &["session-usage"]);
    total.set_valign(gtk::Align::Center);
    total.set_margin_end(10);
    total.set_visible(false);
    foot.append(&terminal);
    foot.append(&total);
    foot.append(&stop);
    let meter_state = Rc::new(Cell::new(Meter::default()));
    let context_meter = meter(meter_state.clone());
    context_meter.set_visible(false);
    // Open requests, to switch between, once there's more than one.
    let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    tabs.add_css_class("request-tabs");
    // Sideways scrolling, so many tabs never widen the card.
    let tabs_row = gtk::ScrolledWindow::new();
    tabs_row.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
    tabs_row.set_propagate_natural_height(true);
    tabs_row.set_child(Some(&tabs));
    tabs_row.set_visible(false);
    body.append(&top);
    body.append(&context_meter);
    body.append(&tabs_row);
    body.append(&pages);
    body.append(&foot);
    card.set_child(Some(&body));

    // ---- the dim: the rest of the screen, softly darker while the card is open ----
    let dim = layer_window(app, "tripppin-orb-dim");
    dim.set_layer(Layer::Top);
    for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        dim.set_anchor(e, true);
    }
    dim.set_exclusive_zone(-1);
    dim.set_keyboard_mode(KeyboardMode::None);
    let dim_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    dim_box.add_css_class("orb-dim");
    dim_box.set_hexpand(true);
    dim_box.set_vexpand(true);
    dim.set_child(Some(&dim_box));

    let orb = Rc::new(Orb {
        corner_window: corner.clone(),
        button,
        logo,
        hint: hint.clone(),
        row,
        corner: Cell::new(corner_at),
        tucked: Cell::new(tucked),
        model,
        ask,
        entry,
        card,
        body,
        pages,
        caption,
        asked,
        status,
        working,
        meter: context_meter,
        meter_state,
        total,
        scroller,
        convos,
        tabs,
        tabs_row,
        back: back.clone(),
        dim,
        dim_box: dim_box.clone(),
        dim_wanted: Rc::new(Cell::new(false)),
        follow,
        follow_row,
        terminal,
        stop,
        sessions,
        chips: chips.clone(),
        found: found.clone(),
        apps: RefCell::new(Vec::new()),
        shade: shade.clone(),
        column: column.clone(),
        state: RefCell::new(State::default()),
    });

    orb.place_card();
    {
        let o = orb.clone();
        orb.button.connect_clicked(move |_| o.toggle());
    }
    {
        let o = orb.clone();
        minimise.connect_clicked(move |_| o.minimise());
    }
    {
        let o = orb.clone();
        history.connect_clicked(move |_| o.show_history());
    }
    {
        let o = orb.clone();
        back.connect_clicked(move |_| {
            o.back_from_history();
        });
    }
    // Clicking the dim puts the card away, like Escape.
    let dim_click = gtk::GestureClick::new();
    {
        let o = orb.clone();
        dim_click.connect_released(move |_, _, _, _| o.minimise());
    }
    dim_box.add_controller(dim_click);
    // However the card goes away, the dim goes with it.
    {
        let o = orb.clone();
        orb.card.connect_visible_notify(move |c| {
            if !c.is_visible() {
                o.set_dim(false);
            }
        });
    }
    {
        let o = orb.clone();
        ask_history.connect_clicked(move |_| {
            o.close_ask();
            o.show_history();
        });
    }
    {
        let o = orb.clone();
        new.connect_clicked(move |_| o.start_new());
    }
    {
        let o = orb.clone();
        orb.terminal.connect_clicked(move |_| o.continue_in_terminal());
    }
    // Clicking the dim area closes the bar.
    let close_click = gtk::GestureClick::new();
    {
        let o = orb.clone();
        close_click.connect_released(move |_, _, _, _| o.close_ask());
    }
    orb.shade.add_controller(close_click);
    // Right-click: tuck away, or bring back.
    let right_click = gtk::GestureClick::new();
    right_click.set_button(3);
    {
        let o = orb.clone();
        right_click.connect_released(move |_, _, _, _| o.set_tucked(!o.tucked.get()));
    }
    orb.button.add_controller(right_click);
    orb.drag_to_corners();
    {
        let o = orb.clone();
        orb.entry.connect_activate(move |_| o.submit());
    }
    {
        let o = orb.clone();
        orb.entry.connect_changed(move |e| o.match_apps(&e.text()));
    }
    {
        let o = orb.clone();
        send.connect_clicked(move |_| o.submit());
    }
    {
        let o = orb.clone();
        orb.follow.connect_activate(move |_| o.follow_up());
    }
    {
        let o = orb.clone();
        follow_send.connect_clicked(move |_| o.follow_up());
    }
    {
        let o = orb.clone();
        orb.stop.connect_clicked(move |_| o.stop_or_close());
    }
    let keys = gtk::EventControllerKey::new();
    {
        let o = orb.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                o.close_ask();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }
    orb.ask.add_controller(keys);
    let card_keys = gtk::EventControllerKey::new();
    {
        let o = orb.clone();
        card_keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if !o.back_from_history() {
                    o.minimise();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }
    orb.card.add_controller(card_keys);

    corner.present();
    orb
}

fn icon_button(glyph: char, tip: &str) -> gtk::Button {
    let b = gtk::Button::with_label(&glyph.to_string());
    b.add_css_class("icon-button");
    b.set_valign(gtk::Align::Center);
    b.set_tooltip_text(Some(tip));
    b
}

/// Only the orb itself takes clicks; the glow and hint let them through. Run again whenever
/// the orb changes size.
fn clickable_orb_only(w: &gtk::Window, b: &gtk::Button) {
    let (w, b) = (w.clone(), b.clone());
    glib::idle_add_local_once(move || {
        if let (Some(surface), Some(r)) = (w.surface(), b.compute_bounds(&w)) {
            let rect = gtk::cairo::RectangleInt::new(
                r.x() as i32 - 2,
                r.y() as i32 - 2,
                r.width().ceil() as i32 + 4,
                r.height().ceil() as i32 + 4,
            );
            surface.set_input_region(Some(&gtk::cairo::Region::create_rectangle(&rect)));
        }
    });
}

/// Hides a layer window after its content plays the `leave` animation (fading and shrinking
/// toward `origin`), so closing is as smooth as opening.
fn leave(window: &gtk::Window, content: &impl IsA<gtk::Widget>) {
    if !window.is_visible() {
        return;
    }
    let content = content.as_ref().clone();
    content.add_css_class("leave");
    let window = window.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(170), move || {
        window.set_visible(false);
        content.remove_css_class("leave");
    });
}

fn model_line() -> String {
    let catalog = hwdetect::Catalog::builtin().ok();
    let tier = catalog.as_ref().and_then(|c| trip::model::active_tier(c, &trip::model::models_dir()));
    match tier {
        Some(t) => format!("{} · on this computer", t.model),
        None => "No model yet · it downloads on first start".into(),
    }
}

/// niri has no caps-lock event, so the orb (always running) watches the keyboards' caps-lock
/// light and shows the pop-up when it changes.
fn watch_caps_lock() {
    let last = Cell::new(crate::osd::caps_lock());
    if last.get().is_none() {
        return;
    }
    glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        let now = crate::osd::caps_lock();
        if now != last.get() {
            last.set(now);
            let _ = std::process::Command::new("tripppin-ui").args(["osd", "caps"]).spawn();
        }
        glib::ControlFlow::Continue
    });
}

/// Exists once the person has opened the orb.
fn used_file() -> std::path::PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("tripppin/orb-used")
}

fn write_status(text: &str) {
    let _ = std::fs::write(common::runtime_dir().join("orb-status"), text);
}

impl Orb {
    fn toggle(self: &Rc<Self>) {
        let (running, shown) = (self.running() > 0, self.shown().is_some());
        if running || (shown && self.card.is_visible()) {
            if self.card.is_visible() {
                self.minimise();
            } else {
                self.show_card();
            }
            return;
        }
        if self.ask.is_visible() {
            self.close_ask();
        } else {
            self.open_ask();
        }
    }

    /// The ask bar, for something new. Requests already running carry on.
    fn open_ask(self: &Rc<Self>) {
        self.card.set_visible(false);
        self.entry.set_text("");
        // The model may have arrived since the orb started (first boot downloads it).
        self.model.set_text(&model_line());
        *self.apps.borrow_mut() = crate::apps::installed();
        self.match_apps("");
        self.ask.present();
        common::play(&self.shade, "fade");
        common::play(&self.column, "pop");
        self.entry.grab_focus();
        if self.hint.opacity() > 0.0 {
            self.hint.set_opacity(0.0);
            let file = used_file();
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(file, "");
        }
    }

    /// Stop applies to the request on show; with nothing running there, it closes the card.
    fn stop_or_close(&self) {
        match self.shown() {
            Some(req) if req.busy.get() => {
                req.cancel.borrow().store(true, Ordering::Relaxed);
                req.stopping.set(true);
                self.stop.set_label("Stopping…");
            }
            _ => self.minimise(),
        }
    }

    fn close_ask(&self) {
        leave(&self.ask, &self.column);
    }

    fn show_card(&self) {
        self.set_dim(true);
        self.card.present();
        common::play(&self.body, if self.corner.get().top() { "drop" } else { "rise" });
    }

    /// Back into the orb; it keeps glowing while it works, and a click brings the card back.
    fn minimise(&self) {
        self.set_dim(false);
        leave(&self.card, &self.body);
    }

    /// Softly dims the rest of the screen under the card, lifting the orb above the dim, or
    /// fades the dim away and puts the orb back on its usual layer.
    fn set_dim(&self, on: bool) {
        if self.dim_wanted.replace(on) == on {
            return;
        }
        if on {
            self.corner_window.set_layer(Layer::Overlay);
            self.dim_box.remove_css_class("out");
            self.dim.present();
            common::play(&self.dim_box, "fade");
            return;
        }
        self.dim_box.add_css_class("out");
        let (dim, dim_box, corner, wanted) =
            (self.dim.clone(), self.dim_box.clone(), self.corner_window.clone(), self.dim_wanted.clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(170), move || {
            // Opened again meanwhile: leave it up.
            if wanted.get() {
                return;
            }
            dim.set_visible(false);
            dim_box.remove_css_class("out");
            corner.set_layer(Layer::Top);
        });
    }

    /// Puts the card beside the orb, on the screen side of its corner.
    fn place_card(&self) {
        let c = self.corner.get();
        c.anchor(&self.card);
        let (side, end) = c.rest();
        // Past the orb (its glow box) with the card's own shadow margin taken back.
        let orb_box = 44 + 2 * GLOW;
        c.place(&self.card, (side + GLOW - 24).max(0), end + orb_box - 20);
        let origin = match c {
            Corner::TopLeft => "left top",
            Corner::TopRight => "right top",
            Corner::BottomLeft => "left bottom",
            Corner::BottomRight => "right bottom",
        };
        for o in ["origin-left-top", "origin-right-top", "origin-left-bottom", "origin-right-bottom"] {
            self.body.remove_css_class(o);
        }
        self.body.add_css_class(&format!("origin-{}", origin.replace(' ', "-")));
    }

    fn set_tucked(&self, tucked: bool) {
        self.tucked.set(tucked);
        if tucked {
            self.button.add_css_class("tucked");
            self.logo.set_visible(false);
        } else {
            self.button.remove_css_class("tucked");
            self.logo.set_visible(true);
        }
        save_prefs(self.corner.get(), tucked);
        clickable_orb_only(&self.corner_window, &self.button);
        // Again once the size transition has settled.
        let (w, b) = (self.corner_window.clone(), self.button.clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(320), move || clickable_orb_only(&w, &b));
    }

    /// Dragging moves the orb with the pointer; letting go springs it into the nearest corner.
    fn drag_to_corners(self: &Rc<Self>) {
        let drag = gtk::GestureDrag::new();
        let start: Rc<Cell<(i32, i32)>> = Rc::new(Cell::new((0, 0)));
        let moved = Rc::new(Cell::new(false));
        {
            let (o, start, moved) = (self.clone(), start.clone(), moved.clone());
            drag.connect_drag_begin(move |_, _, _| {
                let w = &o.corner_window;
                let c = o.corner.get();
                let side = w.margin(if c.left() { Edge::Left } else { Edge::Right });
                let end = w.margin(if c.top() { Edge::Top } else { Edge::Bottom });
                start.set((side, end));
                moved.set(false);
            });
        }
        {
            let (o, start, moved) = (self.clone(), start.clone(), moved.clone());
            drag.connect_drag_update(move |g, dx, dy| {
                if !moved.get() && dx.hypot(dy) < 6.0 {
                    return;
                }
                if !moved.get() {
                    moved.set(true);
                    o.button.add_css_class("dragging");
                    // A drag isn't a click.
                    g.set_state(gtk::EventSequenceState::Claimed);
                }
                let c = o.corner.get();
                let (side, end) = start.get();
                let side = side + (if c.left() { dx } else { -dx }) as i32;
                let end = end + (if c.top() { dy } else { -dy }) as i32;
                c.place(&o.corner_window, side.max(0), end.max(0));
            });
        }
        {
            let (o, moved) = (self.clone(), moved.clone());
            drag.connect_drag_end(move |_, _, _| {
                if moved.get() {
                    o.button.remove_css_class("dragging");
                    o.settle();
                }
            });
        }
        self.button.add_controller(drag);
    }

    /// Springs the orb from wherever it was dropped into the nearest corner.
    fn settle(self: &Rc<Self>) {
        let w = &self.corner_window;
        let Some(mon) = w.surface().and_then(|s| s.display().monitor_at_surface(&s)) else { return };
        let geo = mon.geometry();
        let (sw, sh) = (f64::from(geo.width()), f64::from(geo.height()));
        let (ww, wh) = (f64::from(w.width()), f64::from(w.height()));
        let c = self.corner.get();
        let side = f64::from(w.margin(if c.left() { Edge::Left } else { Edge::Right }));
        let end = f64::from(w.margin(if c.top() { Edge::Top } else { Edge::Bottom }));
        // The window's top-left on screen.
        let x = if c.left() { side } else { sw - side - ww };
        let y = if c.top() { end } else { sh - end - wh };
        let to = Corner::nearest(x + ww / 2.0, y + wh / 2.0, sw, sh);
        // The same spot measured from the new corner's edges, so re-anchoring doesn't jump.
        let from_side = if to.left() { x } else { sw - x - ww };
        let from_end = if to.top() { y } else { sh - y - wh };
        to.anchor(w);
        to.place(w, from_side.round() as i32, from_end.round() as i32);
        if to != c {
            // The hint sits on the screen side of the orb.
            self.row.remove(&self.hint);
            if to.left() {
                self.row.append(&self.hint);
            } else {
                self.row.prepend(&self.hint);
            }
        }
        self.corner.set(to);
        save_prefs(to, self.tucked.get());
        self.place_card();
        let (rest_side, rest_end) = to.rest();
        let (w, b) = (w.clone(), self.button.clone());
        let begun = Cell::new(None::<i64>);
        w.add_tick_callback(move |w, clock| {
            let now = clock.frame_time();
            let t0 = *begun.get().get_or_insert(now);
            // Reduce motion (Settings → Accessibility): straight there.
            let animate = gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations());
            let t = if animate { (now - t0) as f64 / 420_000.0 } else { 1.0 };
            let k = common::ease_out(t);
            let side = from_side + (f64::from(rest_side) - from_side) * k;
            let end = from_end + (f64::from(rest_end) - from_end) * k;
            to.place(w, side.round() as i32, end.round() as i32);
            if t >= 1.0 {
                clickable_orb_only(w, &b);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    /// Apps whose name starts a word with what's typed, best first, at most three.
    fn match_apps(self: &Rc<Self>, text: &str) {
        while let Some(child) = self.found.first_child() {
            self.found.remove(&child);
        }
        let matches = app_matches(&self.apps.borrow(), text);
        self.found.set_visible(!matches.is_empty());
        self.chips.set_visible(matches.is_empty());
        for app in matches.into_iter().take(3) {
            let b = gtk::Button::new();
            b.add_css_class("chip");
            b.add_css_class("app-chip");
            let inner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let image = gtk::Image::from_icon_name(&app.icon);
            image.set_pixel_size(18);
            inner.append(&image);
            let name = gtk::Label::new(Some(&format!("Open {}", app.name)));
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            name.set_max_width_chars(22);
            inner.append(&name);
            b.set_child(Some(&inner));
            let o = self.clone();
            b.connect_clicked(move |_| o.open_app(&app));
            self.found.append(&b);
        }
    }

    fn open_app(&self, app: &crate::apps::App) {
        let id = app.id.trim_end_matches(".desktop");
        let _ = std::process::Command::new("gtk4-launch").arg(id).spawn();
        self.close_ask();
    }

    /// A new request from the ask bar: a new conversation, alongside any still running. An app's
    /// exact name opens the app.
    fn submit(self: &Rc<Self>) {
        let text = self.entry.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let exact = self.apps.borrow().iter().find(|a| a.name.eq_ignore_ascii_case(&text)).cloned();
        if let Some(app) = exact {
            self.open_app(&app);
            return;
        }
        self.ask.set_visible(false);
        let req = Request::new(Session::new("orb"), &text, "You asked");
        self.show_request(&req);
        self.begin(&req, text, TurnView::new(None));
    }

    /// A follow-up in the card: the same conversation, so the model knows what came before.
    fn follow_up(self: &Rc<Self>) {
        let text = self.follow.text().trim().to_string();
        let Some(req) = self.shown() else { return };
        if text.is_empty() || req.busy.get() || req.session.borrow().is_none() {
            return;
        }
        self.follow.set_text("");
        self.begin(&req, text.clone(), TurnView::new(Some(&text)));
    }

    /// Back to the ask bar for something new; anything running carries on beside it.
    fn start_new(self: &Rc<Self>) {
        self.open_ask();
    }

    fn shown(&self) -> Option<Rc<Request>> {
        self.state.borrow().shown.clone()
    }

    fn is_shown(&self, req: &Rc<Request>) -> bool {
        self.state.borrow().shown.as_ref().is_some_and(|s| Rc::ptr_eq(s, req))
    }

    /// How many requests are running.
    fn running(&self) -> usize {
        self.state.borrow().requests.iter().filter(|r| r.busy.get()).count()
    }

    /// Puts a request on show in the card: its conversation, status, meter and tokens. Finished
    /// requests already looked at leave the tabs (the history keeps them).
    fn show_request(self: &Rc<Self>, req: &Rc<Request>) {
        let gone: Vec<Rc<Request>> = {
            let mut st = self.state.borrow_mut();
            let (keep, gone) = std::mem::take(&mut st.requests)
                .into_iter()
                .partition(|r| r.busy.get() || r.unseen.get() || Rc::ptr_eq(r, req));
            st.requests = keep;
            if !st.requests.iter().any(|r| Rc::ptr_eq(r, req)) {
                st.requests.push(req.clone());
            }
            st.shown = Some(req.clone());
            gone
        };
        for r in gone {
            self.convos.remove(&r.turns);
        }
        req.unseen.set(false);
        if req.turns.parent().is_none() {
            self.convos.add_child(&req.turns);
        }
        self.convos.set_visible_child(&req.turns);
        self.asked.set_text(&req.title);
        self.caption.set_text(&req.caption);
        self.pages.set_visible_child_name("talk");
        self.refresh_shown();
        self.refresh_tabs();
        self.refresh_orb();
        self.scroll_to_end();
    }

    /// The card's header and footer, from the request on show.
    fn refresh_shown(&self) {
        let Some(req) = self.shown() else { return };
        let busy = req.busy.get();
        self.working.set_visible(busy);
        if busy {
            self.tick();
        } else {
            self.status.set_text(&req.ended.borrow());
            self.status.set_tooltip_text(None);
        }
        let m = Meter { busy, ..req.meter.get() };
        self.meter_state.set(m);
        self.meter.set_visible(busy || m.fill.is_some());
        let tip = req.meter_tip.borrow();
        self.meter.set_tooltip_text((!tip.is_empty()).then_some(tip.as_str()));
        self.meter.queue_draw();
        let (total, total_tip) = req.total.borrow().clone();
        self.total.set_text(&total);
        self.total.set_tooltip_text(Some(&total_tip));
        self.total.set_visible(!total.is_empty());
        self.stop.set_label(if !busy {
            "close"
        } else if req.stopping.get() {
            "stopping…"
        } else {
            "stop"
        });
        let talking = self.pages.visible_child_name().as_deref() == Some("talk");
        self.follow_row.set_visible(!busy && talking);
        self.terminal.set_sensitive(talking);
    }

    /// The orb and the bar: working while anything runs, and how many.
    fn refresh_orb(&self) {
        let n = self.running();
        if n > 0 {
            self.button.add_css_class("working");
        } else {
            self.button.remove_css_class("working");
        }
        let tip = if n > 1 { format!("{n} requests running · click to see them") } else { ORB_TIP.to_string() };
        self.button.set_tooltip_text(Some(&tip));
        write_status(&bar_status(n));
    }

    /// A chip for each open request, the one on show marked, once there's more than one.
    fn refresh_tabs(self: &Rc<Self>) {
        while let Some(child) = self.tabs.first_child() {
            self.tabs.remove(&child);
        }
        let (list, shown) = {
            let st = self.state.borrow();
            (st.requests.clone(), st.shown.clone())
        };
        self.tabs_row.set_visible(list.len() > 1);
        if list.len() < 2 {
            return;
        }
        for r in list {
            let on_show = shown.as_ref().is_some_and(|s| Rc::ptr_eq(s, &r));
            let b = gtk::Button::new();
            b.add_css_class("request-tab");
            if on_show {
                b.add_css_class("selected");
            }
            let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let mark: gtk::Widget = if r.busy.get() {
                common::pulse_dot(6).upcast()
            } else {
                let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                dot.add_css_class("request-tab-dot");
                if r.unseen.get() {
                    dot.add_css_class("unseen");
                }
                dot.upcast()
            };
            mark.set_valign(gtk::Align::Center);
            inner.append(&mark);
            inner.append(&gtk::Label::new(Some(&clip(&r.title, 18))));
            b.set_child(Some(&inner));
            let state = request_state(r.busy.get(), on_show, r.unseen.get());
            b.set_tooltip_text(Some(&format!("{}\n{state}", r.title)));
            let o = self.clone();
            b.connect_clicked(move |_| o.show_request(&r));
            self.tabs.append(&b);
        }
    }

    fn begin(self: &Rc<Self>, req: &Rc<Request>, text: String, view: Rc<TurnView>) {
        let Some(mut session) = req.session.borrow_mut().take() else { return };
        req.turns.append(&view.root);
        let cancel = Arc::new(AtomicBool::new(false));
        *req.cancel.borrow_mut() = cancel.clone();
        req.busy.set(true);
        req.stopping.set(false);
        req.started.set(Some(Instant::now()));
        *req.phase.borrow_mut() = "thinking".into();
        req.ended.borrow_mut().clear();
        req.answer.borrow_mut().clear();
        *req.turn.borrow_mut() = Some(view);
        if self.is_shown(req) {
            self.pages.set_visible_child_name("talk");
            self.refresh_shown();
            self.show_card();
            self.scroll_to_end();
        }
        self.refresh_tabs();
        self.refresh_orb();
        // A live status with a running clock, so it's clear the orb is still at it.
        let (weak, r) = (Rc::downgrade(self), Rc::downgrade(req));
        glib::timeout_add_seconds_local(1, move || match (weak.upgrade(), r.upgrade()) {
            (Some(orb), Some(req)) if req.busy.get() => {
                if orb.is_shown(&req) {
                    orb.tick();
                }
                glib::ControlFlow::Continue
            }
            _ => glib::ControlFlow::Break,
        });
        // The meter's sweep runs while it works.
        let r = Rc::downgrade(req);
        self.meter.add_tick_callback(move |m, _| {
            m.queue_draw();
            if r.upgrade().is_some_and(|r| r.busy.get()) {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });

        // Its own thread and channel; other requests may be running tools at the same time.
        let (tx, rx) = async_channel::unbounded::<(String, Event)>();
        let id = req.id.clone();
        std::thread::spawn(move || {
            // Other processes (`trip shell --continue`) can follow the run through its live file.
            let log = follow::Log::create(&follow::live_path(&id), &text, session.turns.len());
            let finished = Arc::new(AtomicBool::new(false));
            follow::watch(&id, cancel.clone(), finished.clone());
            session.ask(&text);
            let mut ui = follow::Tee { inner: Sender { tx: tx.clone(), id: id.clone() }, log };
            let result = agent::run(&mut session, &cancel, &mut ui).map(|_| ()).map_err(|e| format!("{e:#}"));
            session.save();
            ui.log.end(result.as_ref().err().cloned());
            finished.store(true, Ordering::Relaxed);
            let _ = tx.send_blocking((id, Event::Done(result, Box::new(session))));
        });
        let orb = self.clone();
        glib::spawn_future_local(async move {
            while let Ok((id, event)) = rx.recv().await {
                let done = matches!(event, Event::Done(..));
                let req = orb.state.borrow().requests.iter().find(|r| r.id == id).cloned();
                if let Some(req) = req {
                    orb.apply(&req, event);
                    // Straight away, not at the next second, so the status keeps up.
                    if !done && orb.is_shown(&req) {
                        orb.tick();
                    }
                }
                if done {
                    break;
                }
            }
        });
    }

    /// The status line: what the request on show is doing or waiting on, and a clock that
    /// keeps running.
    fn tick(&self) {
        let Some(req) = self.shown() else { return };
        if !req.busy.get() {
            return;
        }
        let phase = req.phase.borrow().clone();
        self.status.set_text(&format!("{} · {}", clip(&phase, 40), req.elapsed()));
        self.status.set_tooltip_text(Some(&phase));
    }

    /// A reply's tokens: the line under the request, its context meter and session total.
    fn show_usage(&self, req: &Rc<Request>, turn: &TurnView, u: &Usage) {
        turn.add_usage(u);
        if let Some(p) = context_percent(u) {
            req.meter.set(Meter { fill: Some(f64::from(p) / 100.0), ..req.meter.get() });
            *req.meter_tip.borrow_mut() =
                format!("Context {p}% full · {} of {} tokens", tokens(u.prompt + u.completion), tokens(u.context));
        }
        let all = u.session_prompt + u.session_completion;
        if all > 0 {
            *req.total.borrow_mut() = (
                format!("Session {}", tokens(all)),
                format!(
                    "This conversation so far: {} tokens read, {} written",
                    tokens(u.session_prompt),
                    tokens(u.session_completion)
                ),
            );
        }
        if self.is_shown(req) {
            self.refresh_shown();
        }
    }

    /// Grows the list with its content up to `MAX_LIST`, then it scrolls; keeps the newest in view.
    /// (A layer-shell window doesn't grow on its own when its content does.)
    fn scroll_to_end(&self) {
        let Some(req) = self.shown() else { return };
        let (scroller, inner) = (self.scroller.clone(), req.turns.clone());
        glib::idle_add_local_once(move || {
            let width = scroller.width().max(360);
            let (_, natural, _, _) = inner.measure(gtk::Orientation::Vertical, width);
            scroller.set_min_content_height(natural.clamp(MIN_LIST, MAX_LIST));
            glib::idle_add_local_once(move || {
                let adj = scroller.vadjustment();
                adj.set_value(adj.upper() - adj.page_size());
            });
        });
    }

    /// One of a request's events, whether or not it's the one on show.
    fn apply(self: &Rc<Self>, req: &Rc<Request>, event: Event) {
        let Some(turn) = req.turn.borrow().clone() else { return };
        let shown = self.is_shown(req);
        match event {
            Event::Text(piece) => {
                *req.phase.borrow_mut() = "answering".into();
                req.answer.borrow_mut().push_str(&piece);
                turn.set_answer(&req.answer.borrow());
                if shown {
                    self.scroll_to_end();
                }
            }
            Event::Thinking => *req.phase.borrow_mut() = "thinking".into(),
            Event::Waiting(what) => {
                if !what.trim().is_empty() {
                    *req.phase.borrow_mut() = what;
                }
            }
            Event::Usage(u) => {
                self.show_usage(req, &turn, &u);
                if shown {
                    self.scroll_to_end();
                }
            }
            Event::Plan(steps) => {
                turn.set_plan(&steps);
                if shown {
                    self.scroll_to_end();
                }
            }
            Event::StepDone(ok) => {
                if let Some(last) = turn.last() {
                    last.finish(ok);
                }
            }
            Event::Step(kind, detail) => {
                *req.phase.borrow_mut() = describe(&kind, &detail);
                if let Some(last) = turn.last() {
                    last.finish(true);
                }
                // Text before a tool call is the model thinking aloud; the steps say it better.
                req.answer.borrow_mut().clear();
                turn.set_answer("");
                turn.add_step(StepRow::new(&describe(&kind, &detail), &detail));
                if shown {
                    self.scroll_to_end();
                }
            }
            Event::Output(line) => {
                // Progress bars redraw with carriage returns; show the latest state of the line.
                let line = line.rsplit('\r').find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
                if let Some(last) = turn.last()
                    && !line.is_empty()
                {
                    last.add_line(&line);
                }
            }
            Event::Note(text) => {
                if let Some(last) = turn.last()
                    && text.contains("background as job")
                {
                    let job = text.rsplit(' ').next().unwrap_or("");
                    last.title.set_text(&format!("{} · job {job}", last.title.text()));
                }
            }
            Event::Done(result, session) => {
                req.busy.set(false);
                *req.session.borrow_mut() = Some(*session);
                let ok = result.is_ok();
                if let Some(last) = turn.last() {
                    last.finish(ok);
                }
                let took = req.started.get().map_or(0, |t| t.elapsed().as_secs());
                let verb = if ok { "Done in" } else { "Stopped after" };
                *req.ended.borrow_mut() = format!("{verb} {}:{:02}", took / 60, took % 60);
                if let Err(e) = result {
                    let msg = if e == "stopped" { "stopped.".to_string() } else { e };
                    turn.set_answer(&msg);
                }
                let summary = req.answer.borrow().trim().lines().next().unwrap_or("done").to_string();
                let _ = std::fs::write(common::runtime_dir().join("last-done"), summary);
                if !self.card.is_visible() {
                    // Finished while the card was put away: come back to show it.
                    self.show_request(req);
                    self.show_card();
                } else if shown {
                    self.refresh_shown();
                    self.scroll_to_end();
                } else {
                    // Another is on show: its tab says this one is ready.
                    req.unseen.set(true);
                }
                self.refresh_tabs();
                self.refresh_orb();
            }
        }
    }

    /// Back from the history to the request that's running or was open. False when there's
    /// nothing to go back to (or the history isn't showing).
    fn back_from_history(self: &Rc<Self>) -> bool {
        if self.pages.visible_child_name().as_deref() != Some("history") {
            return false;
        }
        let Some(req) = self.shown() else { return false };
        self.show_request(&req);
        true
    }

    /// Earlier conversations, newest first, under the ones open here (running now, or open
    /// now); one click opens it here to read or continue.
    fn show_history(self: &Rc<Self>) {
        self.caption.set_text("Earlier requests");
        while let Some(child) = self.sessions.first_child() {
            self.sessions.remove(&child);
        }
        let (mut open, shown) = {
            let st = self.state.borrow();
            (st.requests.clone(), st.shown.clone())
        };
        let on_show = |r: &Rc<Request>| shown.as_ref().is_some_and(|s| Rc::ptr_eq(s, r));
        // Running first, then the one open, then any finished unseen.
        open.sort_by_key(|r| (!r.busy.get(), !on_show(r)));
        for r in &open {
            let mark: gtk::Widget = if r.busy.get() {
                let dot = common::pulse_dot(8);
                dot.set_margin_top(4);
                dot.set_margin_start(4);
                dot.set_margin_end(4);
                dot.upcast()
            } else {
                tick(icons::CHECK, false).upcast()
            };
            let mut words = request_state(r.busy.get(), on_show(r), r.unseen.get()).to_string();
            if r.busy.get() {
                words.push_str(&format!(" · {}", clip(&r.phase.borrow(), 32)));
            }
            let row = session_row(&mark, &r.title, &words);
            row.add_css_class("current");
            if on_show(r) {
                row.add_css_class("on-show");
            }
            let click = gtk::GestureClick::new();
            {
                let (o, r) = (self.clone(), r.clone());
                click.connect_released(move |_, _, _, _| o.show_request(&r));
            }
            row.add_controller(click);
            self.sessions.append(&row);
        }
        let list: Vec<_> = trip::session::list(40).into_iter().filter(|s| !open.iter().any(|r| r.id == s.id)).collect();
        if list.is_empty() && open.is_empty() {
            let empty = label("Nothing yet. What you ask here and in the terminal shows up in this list.", &["hint"]);
            empty.set_wrap(true);
            empty.set_margin_top(12);
            empty.set_margin_bottom(12);
            self.sessions.append(&empty);
        }
        let now = trip::session::now_secs();
        for s in list {
            let mark = match s.ok {
                Some(true) => tick(icons::CHECK, false),
                Some(false) => tick(icons::ALERT, true),
                None => tick(icons::HISTORY, false),
            };
            let mut words = vec![trip::session::when(s.started, now), s.source.clone()];
            words.push(format!("{} step{}", s.steps, if s.steps == 1 { "" } else { "s" }));
            if s.turns > 1 {
                words.push(format!("{} requests", s.turns));
            }
            let row = session_row(&mark, &s.title, &words.join(" · "));
            let click = gtk::GestureClick::new();
            {
                let (o, id) = (self.clone(), s.id.clone());
                click.connect_released(move |_, _, _, _| o.open_session(&id));
            }
            row.add_controller(click);
            self.sessions.append(&row);
        }
        self.back.set_visible(shown.is_some());
        self.pages.set_visible_child_name("history");
        self.terminal.set_sensitive(false);
        self.follow_row.set_visible(false);
        if !shown.is_some_and(|r| r.busy.get()) {
            self.stop.set_label("Close");
        }
        self.show_card();
    }

    /// Shows a conversation with all its steps: the open request if it's one, or an earlier
    /// one from the history; a follow-up continues it.
    fn open_session(self: &Rc<Self>, id: &str) {
        let open = self.state.borrow().requests.iter().find(|r| r.id == id).cloned();
        if let Some(req) = open {
            self.show_request(&req);
            return;
        }
        let Some(session) = trip::session::load(id) else { return };
        let mut views = Vec::new();
        for (i, t) in session.turns.iter().enumerate() {
            let view = TurnView::new((i > 0).then_some(t.request.as_str()));
            for step in &t.steps {
                view.add_step(StepRow::past(step));
            }
            let answer = if t.answer.is_empty() && !t.error.is_empty() {
                format!("Stopped: {}", t.error)
            } else {
                t.answer.clone()
            };
            view.set_answer(&answer);
            views.push(view);
        }
        let caption = trip::session::when(session.started, trip::session::now_secs());
        let title = session.title().to_string();
        let req = Request::new(session, &title, &caption);
        for view in &views {
            req.turns.append(&view.root);
        }
        *req.turn.borrow_mut() = views.pop();
        self.show_request(&req);
    }

    /// Opens the conversation on show in the terminal (`trip shell --continue`), with every
    /// step, where it carries on as a chat.
    fn continue_in_terminal(&self) {
        let Some(req) = self.shown() else { return };
        // While it works, the terminal shows what's done so far.
        if let Some(s) = req.session.borrow().as_ref() {
            s.save();
        }
        let spawned = std::process::Command::new("konsole")
            .args(["--title", "tripppin", "-e", "trip", "shell", "--continue", &req.id])
            .env("TRIPPPIN_IN_SHELL", "1")
            .spawn();
        if spawned.is_ok() {
            self.minimise();
        }
    }
}

/// A row in the history: its mark, title and a dim line under it.
fn session_row(mark: &impl IsA<gtk::Widget>, title: &str, detail: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("session-row");
    mark.set_valign(gtk::Align::Start);
    mark.set_margin_top(2);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let title = label(title, &["step-title"]);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(40);
    text.append(&title);
    text.append(&label(detail, &["step-detail"]));
    row.append(mark);
    row.append(&text);
    row
}

/// Installed apps matching typed text: the name starts with it first, then a word in the name
/// does. Nothing for under two letters.
fn app_matches(apps: &[crate::apps::App], text: &str) -> Vec<crate::apps::App> {
    let t = text.trim().to_lowercase();
    if t.chars().count() < 2 {
        return Vec::new();
    }
    let mut starts: Vec<_> = apps.iter().filter(|a| a.name.to_lowercase().starts_with(&t)).cloned().collect();
    let words = apps.iter().filter(|a| {
        let n = a.name.to_lowercase();
        !n.starts_with(&t) && n.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(&t))
    });
    starts.extend(words.cloned());
    starts
}

#[cfg(test)]
mod tests {
    use super::{
        Corner, TurnTokens, app_matches, bar_status, clip, context_percent, current_step, describe, markup,
        parse_prefs, request_state, tokens,
    };
    use trip::model::{PlanStep, Usage};

    #[test]
    fn counts_tokens_briefly() {
        assert_eq!(tokens(230), "230");
        assert_eq!(tokens(1000), "1k");
        assert_eq!(tokens(1234), "1.2k");
        assert_eq!(tokens(38_400), "38k");
        assert_eq!(tokens(1_400_000), "1.4m");
    }

    #[test]
    fn adds_up_a_request() {
        let reply = |prompt, completion, tps| Usage {
            prompt,
            completion,
            context: 8192,
            tokens_per_sec: tps,
            ..Usage::default()
        };
        let mut t = TurnTokens::default();
        t.add(&reply(1000, 30, 12.4));
        t.add(&reply(1100, 200, 14.2));
        assert_eq!(t.line(), "2.1k in · 230 out · 14 tok/s · context 16%");
        t.add(&Usage { prompt: 40, cached: 1500, completion: 5, tokens_per_sec: 4.3, ..Usage::default() });
        assert_eq!(t.line(), "2.1k in · 1.5k cached · 235 out · 4.3 tok/s · context 16%");
        assert_eq!(context_percent(&Usage::default()), None);
    }

    #[test]
    fn keeps_the_status_short() {
        assert_eq!(clip("loading the model", 40), "loading the model");
        assert_eq!(clip("running dnf install -y vlc", 12), "running dnf…");
    }

    #[test]
    fn says_what_runs_where() {
        assert_eq!(bar_status(0), "");
        assert_eq!(bar_status(1), "● working");
        assert_eq!(bar_status(2), "● 2 running");
        assert_eq!(request_state(true, false, false), "Running now");
        assert_eq!(request_state(false, true, true), "Open now");
        assert_eq!(request_state(false, false, true), "Finished, not opened yet");
    }

    #[test]
    fn finds_the_current_plan_step() {
        let step = |text: &str, done| PlanStep { text: text.into(), done };
        assert_eq!(current_step(&[step("a", true), step("b", false), step("c", false)]), Some(1));
        assert_eq!(current_step(&[step("a", true)]), None);
    }

    #[test]
    fn finds_apps_by_name() {
        let app = |name: &str| crate::apps::App {
            id: name.to_lowercase(),
            name: name.into(),
            icon: String::new(),
            flatpak: false,
            mime: Vec::new(),
        };
        let apps = [app("Firefox"), app("Text Editor"), app("Files"), app("System Monitor")];
        let names = |t: &str| app_matches(&apps, t).into_iter().map(|a| a.name).collect::<Vec<_>>();
        assert_eq!(names("fi"), ["Firefox", "Files"]);
        assert_eq!(names("edi"), ["Text Editor"]);
        assert!(names("f").is_empty());
        assert!(names("free up some space").is_empty());
    }

    #[test]
    fn remembers_where_the_orb_rests() {
        assert_eq!(parse_prefs(""), (Corner::BottomRight, false));
        assert_eq!(parse_prefs("top-left\ntucked\n"), (Corner::TopLeft, true));
        assert_eq!(parse_prefs("bottom-left\n"), (Corner::BottomLeft, false));
        assert_eq!(Corner::nearest(100.0, 700.0, 1280.0, 800.0), Corner::BottomLeft);
        assert_eq!(Corner::nearest(1200.0, 60.0, 1280.0, 800.0), Corner::TopRight);
    }

    #[test]
    fn renders_light_markdown() {
        assert_eq!(markup("**Disk:** 40 GB"), "<b>Disk:</b> 40 GB");
        assert_eq!(markup("run `df -h`"), "run <span font_family=\"DM Mono\">df -h</span>");
        assert_eq!(markup("a < b & **half"), "a &lt; b &amp; <b>half</b>");
    }

    #[test]
    fn steps_read_like_words() {
        assert_eq!(describe("exec", "sudo flatpak install -y flathub org.videolan.VLC"), "Installing org.videolan.VLC");
        assert_eq!(describe("exec", "flatpak search vlc"), "Searching for vlc");
        assert_eq!(describe("exec", "df -h /"), "Checking disk space");
        assert_eq!(describe("exec", "mkdir -p ~/demo"), "Making demo");
        assert_eq!(describe("exec", "python3 primes.py"), "Running python3");
        assert_eq!(describe("read", "/etc/os-release"), "Reading os-release");
        assert_eq!(describe("waiting on job 2", "sleep 9"), "waiting on job 2");
        assert_eq!(describe("exec", "sudo -v"), "Asking for a password");
        assert_eq!(describe("exec", "sudo -n dnf install -y vlc"), "Installing vlc");
    }
}
