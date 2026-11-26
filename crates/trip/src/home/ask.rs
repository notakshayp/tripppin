//! The ask view: the agent at work. The plan ticks [x] / [~] / [ ], every tool call is a row you
//! can open (▸/▾) to see the exact query, command and output, and risky calls stop at an amber
//! "needs your ok" box wired to the permission settings.

use super::input::Input;
use super::ui::*;
use super::{App, Msg};
use crate::agent::{self, Ui};
use crate::model::{PlanStep, Usage};
use crate::session::{self, Session};
use crate::tools::{Confirm, Decision};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    /// Waiting for the person's ok.
    Waiting,
    Running,
    Ok,
    Fail,
}

#[derive(Clone, Debug)]
pub struct ToolRow {
    pub label: String,
    pub detail: String,
    pub full: String,
    pub status: Status,
    pub output: String,
    pub expanded: bool,
    pub started: Instant,
    pub took: Option<Duration>,
}

#[derive(Clone, Debug)]
pub enum Item {
    You(String),
    Text(String),
    Tool(ToolRow),
    Note(String),
    Error(String),
}

/// What the agent thread tells the screen.
pub enum Ev {
    Text(String),
    Step { label: String, detail: String, full: String },
    Output(String),
    Note(String),
    StepDone(bool),
    Waiting(String),
    Usage(Usage),
    Plan(Vec<PlanStep>),
    Confirm { label: String, detail: String, req: Confirm },
    Result(String),
}

pub struct Pending {
    pub req: Confirm,
}

pub struct AskState {
    pub items: Vec<Item>,
    pub input: Input,
    pub running: bool,
    pub cancel: Arc<AtomicBool>,
    pub session: Option<Session>,
    pub plan: Vec<PlanStep>,
    pub pending: Option<Pending>,
    pub reply: Option<Sender<Decision>>,
    /// Lines scrolled up from the bottom; 0 follows the newest.
    pub scroll: usize,
    pub waiting: String,
    pub usage: Usage,
    pub tools_used: BTreeMap<String, usize>,
    pub history: Vec<session::Summary>,
    pub started: Option<Instant>,
}

impl Default for AskState {
    fn default() -> Self {
        AskState {
            items: Vec::new(),
            input: Input::default(),
            running: false,
            cancel: Arc::new(AtomicBool::new(false)),
            session: None,
            plan: Vec::new(),
            pending: None,
            reply: None,
            scroll: 0,
            waiting: String::new(),
            usage: Usage::default(),
            tools_used: BTreeMap::new(),
            history: Vec::new(),
            started: None,
        }
    }
}

struct AgentUi {
    tx: Sender<Msg>,
    answers: Receiver<Decision>,
    cancel: Arc<AtomicBool>,
    full: String,
}

impl AgentUi {
    fn send(&self, ev: Ev) {
        let _ = self.tx.send(Msg::Agent(ev));
    }
}

impl Ui for AgentUi {
    fn text(&mut self, piece: &str) {
        self.send(Ev::Text(piece.to_string()));
    }
    fn step(&mut self, label: &str, detail: &str) {
        let full = std::mem::take(&mut self.full);
        self.send(Ev::Step { label: label.to_string(), detail: detail.to_string(), full });
    }
    fn output(&mut self, line: &str) {
        self.send(Ev::Output(line.to_string()));
    }
    fn note(&mut self, text: &str) {
        self.send(Ev::Note(text.to_string()));
    }
    fn step_done(&mut self, ok: bool) {
        self.send(Ev::StepDone(ok));
    }
    fn thinking(&mut self) {
        self.send(Ev::Waiting("thinking".into()));
    }
    fn waiting(&mut self, what: &str) {
        self.send(Ev::Waiting(what.to_string()));
    }
    fn usage(&mut self, usage: &Usage) {
        self.send(Ev::Usage(usage.clone()));
    }
    fn plan(&mut self, steps: &[PlanStep]) {
        self.send(Ev::Plan(steps.to_vec()));
    }
    fn full_detail(&mut self, full: &str) {
        self.full = full.to_string();
    }
    fn confirm(&mut self, label: &str, detail: &str, req: &Confirm) -> Decision {
        self.send(Ev::Confirm { label: label.to_string(), detail: detail.to_string(), req: req.clone() });
        loop {
            match self.answers.recv_timeout(Duration::from_millis(200)) {
                Ok(d) => return d,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if self.cancel.load(Ordering::Relaxed) {
                        return Decision::Deny;
                    }
                }
                Err(_) => return Decision::Deny,
            }
        }
    }
    fn result(&mut self, text: &str) {
        self.send(Ev::Result(text.to_string()));
    }
}

/// The first line of a result, short, for the row's right-hand side.
pub fn summary_of(result: &str) -> String {
    let first = result.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let n = result.lines().filter(|l| !l.trim().is_empty()).count();
    let s: String = first.chars().take(44).collect();
    if n > 1 { format!("{s} · {n} lines") } else { s }
}

struct Line {
    spans: Vec<(String, Style)>,
    act: Option<Act>,
}

impl Line {
    fn new(spans: Vec<(String, Style)>) -> Self {
        Line { spans, act: None }
    }
    fn plain(text: &str, st: Style) -> Self {
        Line::new(vec![(text.to_string(), st)])
    }
}

impl App {
    /// Starts the agent on `text` (from the ask bar anywhere).
    pub fn ask_send(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() || self.ask.running {
            return;
        }
        let mut session = self.ask.session.take().unwrap_or_else(|| Session::new("tui"));
        session.ask(text);
        self.ask.items.push(Item::You(text.to_string()));
        self.ask.running = true;
        self.ask.cancel = Arc::new(AtomicBool::new(false));
        self.ask.scroll = 0;
        self.ask.started = Some(Instant::now());
        self.ask.waiting = "thinking".into();
        let (reply_tx, reply_rx) = channel();
        self.ask.reply = Some(reply_tx);
        let tx = self.tx.clone();
        let cancel = self.ask.cancel.clone();
        let mut ui = AgentUi { tx: tx.clone(), answers: reply_rx, cancel: cancel.clone(), full: String::new() };
        std::thread::spawn(move || {
            let r = agent::run(&mut session, &cancel, &mut ui);
            let r = r.map(|d| d.answer).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::AgentDone(Box::new(session), r));
        });
        self.view = View::Ask;
        self.dirty = true;
    }

    pub fn ask_new_chat(&mut self) {
        if self.ask.running {
            self.ask.cancel.store(true, Ordering::Relaxed);
        }
        let history = std::mem::take(&mut self.ask.history);
        self.ask = AskState { history, ..AskState::default() };
        self.ask.history = session::list(8);
        self.view = View::Ask;
        self.dirty = true;
    }

    pub fn ask_event(&mut self, ev: Ev) {
        let a = &mut self.ask;
        match ev {
            Ev::Text(piece) => match a.items.last_mut() {
                Some(Item::Text(t)) => t.push_str(&piece),
                _ => {
                    if !piece.trim().is_empty() {
                        a.items.push(Item::Text(piece.trim_start().to_string()));
                    }
                }
            },
            Ev::Step { label, detail, full } => {
                if matches!(label.as_str(), "plan" | "done") {
                    return;
                }
                *a.tools_used.entry(label.split('.').next().unwrap_or(&label).to_string()).or_default() += 1;
                // A call that asked first already has its row; the answer reuses it.
                if let Some(Item::Tool(t)) = a.items.last_mut()
                    && t.status == Status::Waiting
                {
                    t.status = Status::Running;
                    t.started = Instant::now();
                    if !full.is_empty() {
                        t.full = full;
                    }
                    return;
                }
                a.items.push(Item::Tool(ToolRow {
                    label,
                    full: if full.is_empty() { detail.clone() } else { full },
                    detail,
                    status: Status::Running,
                    output: String::new(),
                    expanded: false,
                    started: Instant::now(),
                    took: None,
                }));
            }
            Ev::Output(line) => {
                if let Some(Item::Tool(t)) = a.items.iter_mut().rev().find(|i| matches!(i, Item::Tool(_))) {
                    if t.output.len() < 20_000 {
                        t.output.push_str(&line);
                        t.output.push('\n');
                    }
                }
            }
            Ev::Note(text) => a.items.push(Item::Note(text)),
            Ev::StepDone(ok) => {
                if let Some(Item::Tool(t)) = a.items.iter_mut().rev().find(|i| matches!(i, Item::Tool(_))) {
                    t.status = if ok { Status::Ok } else { Status::Fail };
                    t.took = Some(t.started.elapsed());
                }
            }
            Ev::Waiting(w) => a.waiting = w,
            Ev::Usage(u) => a.usage = u,
            Ev::Plan(p) => a.plan = p,
            Ev::Confirm { label, detail, req } => {
                a.items.push(Item::Tool(ToolRow {
                    label,
                    full: detail.clone(),
                    detail,
                    status: Status::Waiting,
                    output: String::new(),
                    expanded: false,
                    started: Instant::now(),
                    took: None,
                }));
                a.pending = Some(Pending { req });
            }
            Ev::Result(text) => {
                if let Some(Item::Tool(t)) = a.items.iter_mut().rev().find(|i| matches!(i, Item::Tool(_))) {
                    t.output = text;
                }
            }
        }
        self.dirty = true;
    }

    pub fn ask_done(&mut self, session: Session, result: Result<String, String>) {
        let a = &mut self.ask;
        a.running = false;
        a.pending = None;
        a.waiting.clear();
        a.session = Some(session);
        for i in a.items.iter_mut() {
            if let Item::Tool(t) = i
                && matches!(t.status, Status::Running | Status::Waiting)
            {
                t.status = Status::Fail;
            }
        }
        if let Err(e) = result
            && e != "stopped"
        {
            a.items.push(Item::Error(e));
        }
        a.history = session::list(8);
        self.reload_widgets();
        self.dirty = true;
    }

    pub fn ask_decide(&mut self, d: Decision) {
        if self.ask.pending.take().is_some()
            && let Some(tx) = &self.ask.reply
        {
            let _ = tx.send(d);
        }
        self.dirty = true;
    }

    pub fn ask_stop(&mut self) {
        if self.ask.running {
            self.ask.cancel.store(true, Ordering::Relaxed);
            self.ask.waiting = "stopping".into();
            self.dirty = true;
        }
    }

    pub fn ask_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if self.ask.pending.is_some() {
            match k.code {
                KeyCode::Char('y') | KeyCode::Enter => return self.ask_decide(Decision::Allow),
                KeyCode::Char('a') => return self.ask_decide(Decision::AllowAlways),
                KeyCode::Char('n') | KeyCode::Esc => return self.ask_decide(Decision::Deny),
                _ => {}
            }
        }
        match k.code {
            KeyCode::Char('c') if ctrl => {
                if self.ask.running {
                    self.ask_stop();
                } else {
                    self.ask.input.take();
                }
            }
            KeyCode::Enter => {
                let t = self.ask.input.take();
                self.ask_send(&t);
            }
            KeyCode::PageUp => self.ask.scroll += 8,
            KeyCode::PageDown => self.ask.scroll = self.ask.scroll.saturating_sub(8),
            KeyCode::Up if self.ask.input.is_empty() => self.ask.scroll += 1,
            KeyCode::Down if self.ask.input.is_empty() => self.ask.scroll = self.ask.scroll.saturating_sub(1),
            _ => {
                self.ask.input.key(k);
            }
        }
        self.dirty = true;
    }

    pub fn ask_toggle(&mut self, i: usize) {
        if let Some(Item::Tool(t)) = self.ask.items.get_mut(i) {
            t.expanded = !t.expanded;
            self.dirty = true;
        }
    }

    fn ask_lines(&self, width: u16, ms: u128) -> Vec<Line> {
        let w = width as usize;
        let a = &self.ask;
        let mut out: Vec<Line> = Vec::new();
        let last_you = a.items.iter().rposition(|i| matches!(i, Item::You(_)));
        for (idx, item) in a.items.iter().enumerate() {
            match item {
                Item::You(t) => {
                    if idx > 0 {
                        out.push(Line::plain("", fg(TEXT)));
                    }
                    for (n, l) in wrap(t, w.saturating_sub(6)).into_iter().enumerate() {
                        out.push(Line::new(vec![
                            (if n == 0 { "you › ".into() } else { "      ".into() }, fg(GREEN)),
                            (l, fg(BRIGHT)),
                        ]));
                    }
                    if last_you == Some(idx) && !a.plan.is_empty() {
                        out.push(Line::plain("", fg(TEXT)));
                        out.push(Line::plain("plan", fg(MUTED)));
                        let current = a.plan.iter().position(|s| !s.done);
                        for (n, s) in a.plan.iter().enumerate() {
                            let (mark, c) = if s.done {
                                ("[x]", GREEN)
                            } else if Some(n) == current && a.running {
                                ("[~]", AMBER)
                            } else {
                                ("[ ]", TEXT)
                            };
                            let extra =
                                if a.pending.is_some() && Some(n) == current { "  waiting for you" } else { "" };
                            out.push(Line::new(vec![
                                (format!("{mark} "), fg(c)),
                                (s.text.clone(), fg(if s.done { MUTED } else { TEXT })),
                                (extra.into(), fg(MUTED)),
                            ]));
                        }
                        out.push(Line::plain("", fg(TEXT)));
                    }
                }
                Item::Text(t) => {
                    for l in wrap(t.trim_end(), w) {
                        out.push(Line::plain(&l, fg(TEXT)));
                    }
                }
                Item::Note(t) => {
                    for l in wrap(t, w.saturating_sub(2)) {
                        out.push(Line::plain(&format!("· {l}"), fg(MUTED)));
                    }
                }
                Item::Error(t) => {
                    for l in wrap(t, w.saturating_sub(2)) {
                        out.push(Line::plain(&format!("! {l}"), fg(RED)));
                    }
                }
                Item::Tool(t) => {
                    let (mark, color) = match t.status {
                        Status::Waiting => (spinner(ms), AMBER),
                        Status::Running => (spinner(ms), BLUE),
                        Status::Ok => (if t.expanded { "▾" } else { "▸" }, GREEN),
                        Status::Fail => (if t.expanded { "▾" } else { "▸" }, RED),
                    };
                    let note = match t.status {
                        Status::Waiting => "needs your ok".to_string(),
                        Status::Running => format!("{}…", duration_words(t.started.elapsed())),
                        _ => {
                            let s = summary_of(&t.output);
                            match t.took {
                                Some(d) if d.as_millis() > 400 => format!("{s} · {:.1} s", d.as_secs_f32()),
                                _ => s,
                            }
                        }
                    };
                    let label = format!("{mark} {:<11}", t.label);
                    let room = w.saturating_sub(label.chars().count() + 2);
                    let note: String = note.chars().take((room / 3).max(14)).collect();
                    let detail: String =
                        t.detail.chars().take(room.saturating_sub(note.chars().count() + 2).max(10)).collect();
                    let note_style = if t.status == Status::Fail { fg(RED) } else { fg(MUTED) };
                    out.push(Line {
                        spans: vec![
                            (label, fg(color).add_modifier(Modifier::BOLD)),
                            (format!("{detail}  "), fg(TEXT)),
                            (note, note_style),
                        ],
                        act: Some(Act::Toggle(idx)),
                    });
                    if t.expanded {
                        for l in wrap(&t.full, w.saturating_sub(14)).into_iter().take(12) {
                            out.push(Line::plain(&format!("    {l}"), fg(BRIGHT)));
                        }
                        let body: Vec<&str> = t.output.lines().collect();
                        for l in body.iter().take(40) {
                            let l: String = l.chars().take(w.saturating_sub(6)).collect();
                            out.push(Line::plain(&format!("  │ {l}"), fg(MUTED)));
                        }
                        if body.len() > 40 {
                            out.push(Line::plain(&format!("  │ … {} more lines", body.len() - 40), fg(MUTED)));
                        }
                    }
                }
            }
        }
        if a.running
            && a.pending.is_none()
            && !matches!(a.items.last(), Some(Item::Tool(t)) if t.status == Status::Running)
        {
            out.push(Line::plain(
                &format!("{} {}", spinner(ms), if a.waiting.is_empty() { "thinking" } else { &a.waiting }),
                fg(AMBER),
            ));
        }
        out
    }

    pub fn draw_ask(&mut self, p: &mut Painter, area: Rect) {
        let ms = self.started.elapsed().as_millis();
        let right_w = 40.min(area.width / 3);
        let main = Rect::new(area.x, area.y, area.width - right_w - 1, area.height);
        let side = Rect::new(area.x + main.width + 1, area.y, right_w, area.height);
        let title = if self.ask.items.is_empty() {
            "ask".to_string()
        } else {
            self.ask.session.as_ref().map(|s| s.title().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| {
                self.ask
                    .items
                    .iter()
                    .find_map(|i| if let Item::You(t) = i { Some(t.clone()) } else { None })
                    .unwrap_or_default()
            })
        };
        let close = if self.ask.running { Some(Act::Stop) } else { Some(Act::NewChat) };
        if let Some(inner) = p.pane(main, &title, true, None, close, None, 1.0) {
            let body = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(2));
            if self.ask.items.is_empty() {
                let (m, local) = crate::models::model_label();
                p.text(body.x, body.y, "what do you want to do?", fg(BRIGHT));
                p.text(
                    body.x,
                    body.y + 2,
                    "type it in plain words. the ai plans it, shows each step, and asks before anything risky.",
                    fg(MUTED),
                );
                p.text(
                    body.x,
                    body.y + 3,
                    &format!("model: {m} · {}", if local { "local" } else { "cloud" }),
                    fg(MUTED),
                );
                for (i, ex) in [
                    "search the web for the best terminal text editor",
                    "install a browser",
                    "what is using my disk space?",
                ]
                .iter()
                .enumerate()
                {
                    p.text(body.x, body.y + 5 + i as u16, &format!("  e.g. {ex}"), fg(TEXT));
                }
            }
            let lines = self.ask_lines(body.width, ms);
            let h = body.height as usize;
            let max_scroll = lines.len().saturating_sub(h);
            let scroll = self.ask.scroll.min(max_scroll);
            self.ask.scroll = scroll;
            let start = lines.len().saturating_sub(h + scroll);
            for (row, line) in lines.iter().skip(start).take(h).enumerate() {
                let y = body.y + row as u16;
                let mut x = body.x;
                if let Some(act) = &line.act {
                    p.row(Rect::new(body.x, y, body.width, 1), act.clone(), false);
                }
                for (s, st) in &line.spans {
                    x += p.text(x, y, s, *st);
                }
            }
            p.scroll_area(body, ScrollId::Ask);
            // The input.
            let iy = inner.y + inner.height - 1;
            for x in inner.x..inner.x + inner.width {
                p.text(x, iy - 1, "─", fg(BORDER));
            }
            p.text(inner.x, iy, "› ", fg(GREEN));
            let shown = self.ask.input.shown();
            let hint =
                if self.ask.running { "ctrl-c stop · click ▸ to open" } else { "enter send · click ▸ to open" };
            let room = inner.width.saturating_sub(2 + hint.len() as u16 + 2) as usize;
            let skip = self.ask.input.cursor.saturating_sub(room.saturating_sub(1));
            let vis: String = shown.chars().skip(skip).take(room).collect();
            p.text(inner.x + 2, iy, &vis, fg(BRIGHT));
            p.text(inner.x + inner.width - hint.len() as u16, iy, hint, fg(MUTED));
            self.cursor = Some((inner.x + 2 + (self.ask.input.cursor - skip) as u16, iy));
            p.hit(Rect::new(inner.x, iy, inner.width, 1), Act::FocusInput);
            if self.ask.pending.is_some() {
                self.cursor = None;
            }
        }
        // Right column: context, then "needs your ok" or history.
        let ctx_h = 10.min(side.height);
        if let Some(inner) =
            p.pane(Rect::new(side.x, side.y, side.width, ctx_h), "context", false, None, None, None, 1.0)
        {
            let u = &self.ask.usage;
            let ctx = u.context.max(1);
            let used = crate::tui::context_fill(u);
            p.text(inner.x, inner.y, "tokens", fg(MUTED));
            let bw = inner.width.saturating_sub(14).max(4);
            p.bar(inner.x, inner.y + 1, bw, used as f32 / ctx as f32, GREEN);
            let total = if u.context == 0 { "?".to_string() } else { crate::tui::short(ctx) };
            p.text(inner.x + bw + 1, inner.y + 1, &format!("{}/{total}", crate::tui::short(used)), fg(TEXT));
            p.text(inner.x, inner.y + 3, "tools used", fg(MUTED));
            let tools = if self.ask.tools_used.is_empty() {
                "none yet".to_string()
            } else {
                self.ask.tools_used.iter().map(|(k, v)| format!("{k} ×{v}")).collect::<Vec<_>>().join(" · ")
            };
            p.text(inner.x, inner.y + 4, &tools, fg(TEXT));
            let (m, local) = crate::models::model_label();
            p.text(inner.x, inner.y + 6, "model", fg(MUTED));
            let w = p.text(inner.x, inner.y + 7, &format!("{m} "), fg(BRIGHT));
            p.text(inner.x + w, inner.y + 7, if local { "local " } else { "cloud " }, fg(MUTED));
            p.button(inner.x + w + 7, inner.y + 7, "switch", Act::View(View::Models), false);
        }
        let below = Rect::new(side.x, side.y + ctx_h, side.width, side.height.saturating_sub(ctx_h));
        if let Some(pend) = &self.ask.pending {
            let req = pend.req.clone();
            if let Some(inner) = p.pane(below, "needs your ok", true, Some(AMBER), None, None, 1.0) {
                draw_confirm(p, inner, &req, true);
            }
        } else if let Some(inner) = p.pane(below, "history", false, None, None, None, 1.0) {
            if self.ask.history.is_empty() {
                p.text(inner.x, inner.y, "past chats show here", fg(MUTED));
            }
            for (i, s) in self.ask.history.iter().take(inner.height as usize).enumerate() {
                let t: String = s.title.chars().take(inner.width as usize - 2).collect();
                p.text(
                    inner.x,
                    inner.y + i as u16,
                    &format!("{} {t}", if s.ok == Some(false) { "!" } else { "·" }),
                    fg(MUTED),
                );
            }
        }
    }
}

/// The amber box's content: what it wants, and the three buttons.
pub fn draw_confirm(p: &mut Painter, inner: Rect, req: &Confirm, with_keys: bool) {
    p.text(inner.x, inner.y, &req.title, fg(BRIGHT).add_modifier(Modifier::BOLD));
    let mut y = inner.y + 2;
    for (k, v) in &req.lines {
        p.text(inner.x, y, &format!("{k:<9}"), fg(MUTED));
        let vw = inner.width.saturating_sub(9) as usize;
        for (i, l) in wrap(v, vw).into_iter().take(3).enumerate() {
            p.text(inner.x + 9, y + i as u16, &l, fg(TEXT));
            if i > 0 {
                y += 1;
            }
        }
        y += 1;
    }
    y += 1;
    let kind = crate::tools::KINDS.iter().find(|k| k.0 == req.kind).map_or(req.kind, |k| k.1);
    let w1 = p.button(inner.x, y, "allow", Act::Allow, true);
    p.button(inner.x + w1 + 1, y, "no", Act::Deny, false);
    let always = format!("allow always for {kind}");
    if inner.width as usize >= always.len() + 4 {
        p.button(inner.x, y + 2, &always, Act::AllowAlways, false);
    } else {
        p.button(inner.x, y + 2, "allow always", Act::AllowAlways, false);
    }
    if with_keys {
        let hint = "y / n / a";
        p.text(inner.x + inner.width - hint.len() as u16, inner.y + inner.height - 1, hint, fg(MUTED));
    }
}
