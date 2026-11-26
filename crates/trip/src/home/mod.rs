//! `trip home`: the full-screen interface. One ratatui program draws everything; the mouse and
//! the keyboard reach every control; and it only redraws when something changed or an animation
//! is running, so an idle screen costs next to nothing.
//!
//! One thread reads the terminal, one polls /proc and /sys (once a second at most, and not at all
//! while the screen is out of focus), and the agent, downloads and searches each run on their own
//! threads. They all talk to the main loop through one channel.

mod ask;
mod dash;
mod input;
mod models;
mod packages;
mod settings;
pub mod ui;

use crate::apk::Pkg;
use crate::braille::{self, Ink};
use crate::dictate::{self, Pill};
use crate::session::Session;
use crate::sysx::{self, CpuCounters, Network, Playing, Snapshot};
use crate::timex::{Clock, Local};
use crate::tools::Decision;
use crate::widgets::{self, Widget};
use anyhow::Result;
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
    EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};
use ui::*;

pub enum Msg {
    Term(Event),
    Sys(Box<Snapshot>),
    Playing(Option<Playing>),
    WifiName(Option<String>),
    Agent(ask::Ev),
    AgentDone(Box<Session>, Result<String, String>),
    Dl(models::DlEv),
    Bench(Option<(String, f64)>),
    Provider(Result<crate::model::Endpoint, String>),
    Wifi(Vec<Network>, Option<bool>, Option<String>),
    Update(String),
    Pkgs(u64, Vec<Pkg>),
    PkgJob(packages::Job, bool, String),
    Widget(String, String, String),
    Dictate(Pill),
}

struct SliderDrag {
    id: Slider,
    rect: Rect,
}

pub struct App {
    pub view: View,
    pub tx: Sender<Msg>,
    pub dirty: bool,
    pub quit: bool,
    pub started: Instant,
    pub clock: Clock,
    pub now: Local,
    pub minute_changed: bool,
    pub snap: Snapshot,
    pub wifi: Option<String>,
    pub playing: Option<Playing>,
    pub playing_at: Instant,
    pub widgets: Vec<Widget>,
    pub runner: widgets::Runner,
    pub home: dash::HomeState,
    pub ask: ask::AskState,
    pub models: models::ModelsState,
    pub settings: settings::SettingsState,
    pub pkgs: packages::PkgState,
    pub files: packages::FilesState,
    pub modal: Option<packages::Modal>,
    pub focus_input: bool,
    pub cursor: Option<(u16, u16)>,
    pub hits: Vec<Hit>,
    pub hover: Option<Act>,
    pub hover_pane: Option<Pane>,
    pub pressed: Option<Act>,
    slider: Option<SliderDrag>,
    pub pill: Option<Pill>,
    pub levels: VecDeque<f32>,
    pub toast: Option<(String, Instant)>,
    pub focused: Arc<AtomicBool>,
    pub boot: Option<Instant>,
    pub size: (u16, u16),
    wake: Option<Sender<()>>,
    /// Whether the home screen is showing (the poller reads more often then).
    pub home_visible: Arc<AtomicBool>,
}

impl App {
    pub fn new(tx: Sender<Msg>, size: (u16, u16)) -> Self {
        let mut clock = Clock::new();
        let now = clock.now();
        let mut a = App {
            view: View::Home,
            tx,
            dirty: true,
            quit: false,
            started: Instant::now(),
            clock,
            now,
            minute_changed: true,
            snap: Snapshot::default(),
            wifi: None,
            playing: None,
            playing_at: Instant::now(),
            widgets: Vec::new(),
            runner: widgets::Runner::default(),
            home: dash::HomeState::default(),
            ask: ask::AskState::default(),
            models: models::ModelsState::default(),
            settings: settings::SettingsState::default(),
            pkgs: packages::PkgState::default(),
            files: packages::FilesState::default(),
            modal: None,
            focus_input: true,
            cursor: None,
            hits: Vec::new(),
            hover: None,
            hover_pane: None,
            pressed: None,
            slider: None,
            pill: None,
            levels: VecDeque::new(),
            toast: None,
            focused: Arc::new(AtomicBool::new(true)),
            boot: None,
            size,
            wake: None,
            home_visible: Arc::new(AtomicBool::new(true)),
        };
        a.load_layout();
        a.reload_widgets();
        a.ask.history = crate::session::list(8);
        a
    }

    // ---- navigation ---------------------------------------------------------------------

    pub fn go(&mut self, v: View) {
        self.view = v;
        self.home_visible.store(v == View::Home, Ordering::Relaxed);
        self.pressed = None;
        self.focus_input = matches!(v, View::Home | View::Ask);
        match v {
            View::Models => self.models_open(),
            View::Settings => self.settings_open(),
            View::Packages => self.pkgs_open(),
            View::Files => self.files_open(),
            _ => {}
        }
        if let Some(w) = &self.wake {
            let _ = w.send(());
        }
        self.dirty = true;
    }

    fn cycle(&mut self, dir: i32) {
        let n = View::ALL.len() as i32;
        let i = (self.view.index() as i32 + dir).rem_euclid(n) as usize;
        self.go(View::ALL[i]);
    }

    pub fn toast(&mut self, text: &str) {
        self.toast = Some((text.to_string(), Instant::now()));
        self.dirty = true;
    }

    pub fn mic(&mut self) {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe)
                .arg("dictate")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }

    // ---- clicks -------------------------------------------------------------------------

    pub fn act(&mut self, a: Act) {
        match a {
            Act::Noop | Act::Title(_) | Act::Slider(_) => {}
            Act::View(v) => self.go(v),
            Act::NewChat => self.ask_new_chat(),
            Act::Close(p) => self.close_pane(p),
            Act::AddPane => self.home.add_open = !self.home.add_open,
            Act::PickPane(i) => self.add_pane(i),
            Act::Mic => self.mic(),
            Act::Send => {
                if self.view == View::Home {
                    self.home_send();
                } else {
                    let t = self.ask.input.take();
                    self.ask_send(&t);
                }
            }
            Act::FocusInput => self.focus_input = true,
            Act::Toggle(i) => self.ask_toggle(i),
            Act::Allow | Act::AllowAlways | Act::Deny => {
                let d = match a {
                    Act::Allow => Decision::Allow,
                    Act::AllowAlways => Decision::AllowAlways,
                    _ => Decision::Deny,
                };
                self.decide(d);
            }
            Act::ModalYes => self.decide(Decision::Allow),
            Act::ModalNo => self.decide(Decision::Deny),
            Act::Stop => self.ask_stop(),
            Act::ModelRow(i) => self.models.selected = i,
            Act::ModelUse(i) => self.model_use(i),
            Act::ModelDownload(i) => self.model_download(i),
            Act::ModelDelete(i) => self.model_delete(i),
            Act::ModelPause => {
                if let Some(d) = &self.models.dl {
                    d.cancel.store(true, Ordering::Relaxed);
                }
            }
            Act::Provider(i) => self.provider_open(i),
            Act::ProviderSave => self.provider_save(),
            Act::ProviderCancel => self.models.form = None,
            Act::LocalAgain => {
                let _ = crate::model::set_endpoint(None);
                self.models.message = "using the model on this computer again".into();
                self.models_reload();
            }
            Act::Field(0) => {
                if let Some(f) = &mut self.models.form {
                    f.field = 0;
                }
            }
            Act::Field(1) => {
                if let Some(f) = &mut self.models.form {
                    f.field = 1;
                }
            }
            Act::Field(9) => {
                self.home.input = input::Input::new("add to my calendar: ");
                self.focus_input = true;
            }
            Act::SetTab(_)
            | Act::WifiToggle
            | Act::WifiConnect(_)
            | Act::WifiForget
            | Act::WifiRescan
            | Act::Mute
            | Act::PowerProfile(_)
            | Act::AskKind(_)
            | Act::SearchProvider(_)
            | Act::KeyboardLayout(_)
            | Act::UpdateCheck
            | Act::UpdateRun
            | Act::TestDictation
            | Act::Field(_) => {
                if matches!(a, Act::SetTab(_)) && self.view != View::Settings {
                    self.go(View::Settings);
                }
                self.settings_act(a);
            }
            Act::PkgRow(i) => self.pkgs.selected = i,
            Act::PkgGo(i) => self.pkg_go(i),
            Act::PkgOpen(i) => self.pkg_open(i),
            Act::PkgFocusSearch => self.pkgs.focus_search = true,
            Act::FileRow(i) => {
                if i == usize::MAX {
                    self.files_up();
                } else {
                    self.files.selected = i;
                    self.file_enter(i);
                }
            }
            Act::Player(cmd) => {
                std::thread::spawn(move || sysx::player(cmd));
            }
            Act::Workspace(i) => {
                if let Some(v) = View::ALL.get(i) {
                    self.go(*v);
                }
            }
        }
        self.dirty = true;
    }

    fn decide(&mut self, d: Decision) {
        if let Some(m) = self.modal.take() {
            if d == Decision::AllowAlways {
                crate::tools::allow_always(m.req.kind);
            }
            if d.allowed() {
                self.pkg_run(m.job);
            }
        } else {
            self.ask_decide(d);
        }
        self.dirty = true;
    }

    // ---- keys ---------------------------------------------------------------------------

    fn on_key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        if self.boot.take().is_some() {
            self.dirty = true;
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        match k.code {
            KeyCode::Char('q') if ctrl => return self.quit = true,
            KeyCode::F(n @ 1..=6) => return self.go(View::ALL[n as usize - 1]),
            KeyCode::Char(c @ '1'..='6') if alt => return self.go(View::ALL[c as usize - '1' as usize]),
            KeyCode::Char('m') if alt => return self.mic(),
            _ => {}
        }
        if self.modal.is_some() {
            match k.code {
                KeyCode::Char('y') | KeyCode::Enter => self.decide(Decision::Allow),
                KeyCode::Char('a') => self.decide(Decision::AllowAlways),
                KeyCode::Char('n') | KeyCode::Esc => self.decide(Decision::Deny),
                _ => {}
            }
            return;
        }
        match self.view {
            View::Home => {
                if self.home.add_open {
                    match k.code {
                        KeyCode::Esc => self.home.add_open = false,
                        KeyCode::Char(c @ '1'..='9') => self.add_pane(c as usize - '1' as usize),
                        _ => {}
                    }
                    self.dirty = true;
                } else if k.code == KeyCode::Tab && self.home.input.is_empty() {
                    self.cycle(1);
                } else if k.code == KeyCode::BackTab {
                    self.cycle(-1);
                } else {
                    self.home_key(k);
                }
            }
            View::Ask => {
                if k.code == KeyCode::Tab && self.ask.input.is_empty() && self.ask.pending.is_none() {
                    self.cycle(1);
                } else if k.code == KeyCode::BackTab {
                    self.cycle(-1);
                } else if k.code == KeyCode::Esc && self.ask.pending.is_none() && !self.ask.running {
                    self.go(View::Home);
                } else {
                    self.ask_key(k);
                }
            }
            View::Files => match k.code {
                KeyCode::Esc => self.go(View::Home),
                KeyCode::Tab => self.cycle(1),
                KeyCode::BackTab => self.cycle(-1),
                _ => self.files_key(k),
            },
            View::Models => {
                if k.code == KeyCode::Esc && self.models.form.is_none() {
                    self.go(View::Home);
                } else if k.code == KeyCode::Tab && self.models.form.is_none() {
                    self.cycle(1);
                } else {
                    self.models_key(k);
                }
            }
            View::Settings => {
                let idle = self.settings.pw.is_none() && !self.settings.key_focus;
                if k.code == KeyCode::Esc && idle {
                    self.go(View::Home);
                } else {
                    self.settings_key(k);
                }
            }
            View::Packages => {
                if k.code == KeyCode::Esc && !self.pkgs.focus_search {
                    self.go(View::Home);
                } else {
                    self.pkgs_key(k);
                }
            }
        }
    }

    // ---- mouse --------------------------------------------------------------------------

    fn pane_at(&self, x: u16, y: u16) -> Option<Pane> {
        self.home
            .pane_rects
            .iter()
            .find(|(_, r)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
            .map(|(p, _)| *p)
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        if self.boot.take().is_some() {
            self.dirty = true;
            return;
        }
        let (x, y) = (m.column, m.row);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(h) = hit_at(&self.hits, x, y).cloned() {
                    match (h.kind, &h.act) {
                        (HitKind::Slider, Act::Slider(id)) => {
                            self.slider = Some(SliderDrag { id: *id, rect: h.rect });
                            self.slider_set(*id, slider_value(h.rect, x));
                        }
                        (_, Act::Title(p)) => self.home.drag = Some(*p),
                        _ => {}
                    }
                    self.pressed = Some(h.act);
                } else if self.home.add_open && self.view == View::Home {
                    self.home.add_open = false;
                }
                self.dirty = true;
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(s) = &self.slider {
                    let (id, v) = (s.id, slider_value(s.rect, x));
                    self.slider_set(id, v);
                } else if self.home.drag.is_some() {
                    let over = self.pane_at(x, y);
                    if over != self.hover_pane {
                        self.hover_pane = over;
                        self.dirty = true;
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.slider = None;
                if let Some(dragged) = self.home.drag.take() {
                    if let Some(over) = self.pane_at(x, y).filter(|o| *o != dragged) {
                        self.swap_panes(dragged, over);
                    }
                    self.hover_pane = None;
                    self.pressed = None;
                    self.dirty = true;
                    return;
                }
                let pressed = self.pressed.take();
                let under = hit_at(&self.hits, x, y).map(|h| h.act.clone());
                if let (Some(p), Some(u)) = (pressed, under)
                    && p == u
                {
                    self.act(u);
                }
                self.dirty = true;
            }
            MouseEventKind::Moved => {
                let under = hit_at(&self.hits, x, y).map(|h| h.act.clone()).filter(|a| *a != Act::Noop);
                if under != self.hover {
                    self.hover = under;
                    self.dirty = true;
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = m.kind == MouseEventKind::ScrollUp;
                match scroll_at(&self.hits, x, y) {
                    Some(ScrollId::Ask) => {
                        self.ask.scroll = if up { self.ask.scroll + 3 } else { self.ask.scroll.saturating_sub(3) };
                    }
                    Some(ScrollId::Models) => {
                        let n = self.models.list.len().saturating_sub(1);
                        self.models.selected =
                            if up { self.models.selected.saturating_sub(1) } else { (self.models.selected + 1).min(n) };
                    }
                    Some(ScrollId::Packages) => {
                        let n = self.pkgs.results.len().saturating_sub(1);
                        self.pkgs.selected =
                            if up { self.pkgs.selected.saturating_sub(1) } else { (self.pkgs.selected + 1).min(n) };
                    }
                    Some(ScrollId::Files) => {
                        let n = self.files.entries.len().saturating_sub(1);
                        self.files.selected =
                            if up { self.files.selected.saturating_sub(1) } else { (self.files.selected + 1).min(n) };
                    }
                    _ => {}
                }
                self.dirty = true;
            }
            _ => {}
        }
    }

    // ---- messages -----------------------------------------------------------------------

    fn on_msg(&mut self, m: Msg) {
        match m {
            Msg::Term(e) => match e {
                Event::Key(k) => self.on_key(k),
                Event::Mouse(m) => self.on_mouse(m),
                Event::Resize(w, h) => {
                    self.size = (w, h);
                    self.dirty = true;
                }
                Event::FocusGained => {
                    self.focused.store(true, Ordering::Relaxed);
                    self.minute_changed = true;
                    if let Some(w) = &self.wake {
                        let _ = w.send(());
                    }
                    self.dirty = true;
                }
                Event::FocusLost => {
                    self.focused.store(false, Ordering::Relaxed);
                    self.pressed = None;
                }
                Event::Paste(t) => {
                    match self.view {
                        View::Ask => self.ask.input.insert(&t),
                        View::Packages => self.pkgs.input.insert(&t),
                        _ => self.home.input.insert(&t),
                    }
                    self.dirty = true;
                }
            },
            Msg::Sys(s) => {
                let changed = s.battery != self.snap.battery;
                self.snap = *s;
                self.dirty |= self.view == View::Home || changed;
            }
            Msg::Playing(p) => {
                self.dirty |= p != self.playing;
                self.playing = p;
                self.playing_at = Instant::now();
            }
            Msg::WifiName(n) => {
                self.dirty |= n != self.wifi;
                self.wifi = n;
            }
            Msg::Agent(ev) => self.ask_event(ev),
            Msg::AgentDone(s, r) => self.ask_done(*s, r),
            Msg::Dl(ev) => self.dl_event(ev),
            Msg::Bench(r) => self.bench_done(r),
            Msg::Provider(r) => self.provider_done(r),
            Msg::Wifi(n, on, cur) => self.wifi_done(n, on, cur),
            Msg::Update(t) => {
                self.settings.updating = false;
                self.settings.message = t;
                self.dirty = true;
            }
            Msg::Pkgs(serial, r) => {
                if serial == self.pkgs.serial {
                    self.pkgs_done(r);
                }
            }
            Msg::PkgJob(j, ok, t) => self.pkg_job_done(j, ok, t),
            Msg::Widget(name, text, at) => {
                self.runner.finished(&name, text, at);
                self.dirty |= self.view == View::Home;
            }
            Msg::Dictate(p) => self.on_dictate(p),
        }
    }

    fn on_dictate(&mut self, p: Pill) {
        match &p {
            Pill::Listening(l) => {
                self.levels.push_back(*l);
                while self.levels.len() > 16 {
                    self.levels.pop_front();
                }
                self.pill = Some(p);
            }
            Pill::Transcribing => self.pill = Some(p),
            Pill::Text(t) => {
                self.pill = None;
                self.levels.clear();
                let t = format!("{t} ");
                match self.view {
                    View::Ask => self.ask.input.insert(&t),
                    View::Packages => {
                        self.pkgs.input.insert(&t);
                        self.pkgs.focus_search = true;
                    }
                    _ => {
                        self.home.input.insert(&t);
                        self.focus_input = true;
                    }
                }
            }
            Pill::Failed(e) => {
                self.pill = None;
                self.levels.clear();
                self.toast(&format!("dictation: {e}"));
            }
        }
        self.dirty = true;
    }

    // ---- time ---------------------------------------------------------------------------

    fn is_focused(&self) -> bool {
        self.focused.load(Ordering::Relaxed)
    }

    fn tick(&mut self) {
        let n = self.clock.now();
        if (n.min, n.hour) != (self.now.min, self.now.hour) {
            self.minute_changed = true;
            self.dirty = true;
        }
        self.now = n;
        if self.minute_changed {
            self.refresh_events();
            self.minute_changed = false;
        }
        if let Some((_, t)) = &self.toast
            && t.elapsed() > Duration::from_secs(4)
        {
            self.toast = None;
            self.dirty = true;
        }
        if let Some(t) = self.boot
            && t.elapsed().as_secs_f64() > braille::BOOT_SECS + 0.25
        {
            self.boot = None;
            self.home.born.clear();
            self.dirty = true;
        }
        if self.is_focused() {
            let due = self.runner.due(&self.widgets, Instant::now());
            for w in due {
                dash::run_widget(self.tx.clone(), w, n.hhmm());
            }
        }
    }

    /// Something is moving that needs frames: the boot logo, a pane opening, a spinner.
    fn animating(&self) -> Option<Duration> {
        if self.boot.is_some() {
            return Some(Duration::from_millis(33));
        }
        if !self.is_focused() {
            return None;
        }
        let now = Instant::now();
        if self.view == View::Home && self.home.born.values().any(|b| now.duration_since(*b).as_millis() < 140) {
            return Some(Duration::from_millis(33));
        }
        let spin = self.ask.running
            || self.models.bench_running
            || self.pkgs.searching
            || self.pkgs.running.is_some()
            || self.settings.scanning
            || self.models.form.as_ref().is_some_and(|f| f.busy)
            || (self.view == View::Home
                && self.widgets.iter().any(|w| self.runner.states.get(&w.name).is_none_or(|s| s.text.is_empty())))
            || (self.view == View::Home && self.playing.as_ref().is_some_and(|p| p.playing));
        spin.then_some(Duration::from_millis(125))
    }

    fn next_wake(&self) -> Duration {
        let secs_into_minute = (crate::timex::unix_now().rem_euclid(60)) as u64;
        let mut d = Duration::from_secs(61 - secs_into_minute);
        if let Some(a) = self.animating() {
            d = d.min(a);
        }
        if self.is_focused()
            && let Some(due) = self.runner.next_due(&self.widgets, Instant::now())
        {
            d = d.min(due.max(Duration::from_millis(50)));
        }
        if let Some((_, t)) = &self.toast {
            d = d.min((Duration::from_secs(4).saturating_sub(t.elapsed())).max(Duration::from_millis(50)));
        }
        d.max(Duration::from_millis(10))
    }

    // ---- drawing ------------------------------------------------------------------------

    pub fn draw(&mut self, p: &mut Painter) {
        let area = p.area;
        p.buf.set_style(area, Style::default().bg(BG).fg(TEXT));
        self.cursor = None;
        if let Some(t0) = self.boot {
            return self.draw_boot(p, t0.elapsed().as_secs_f64());
        }
        if area.width < 60 || area.height < 16 {
            p.text(1, 1, "make this window bigger", fg(AMBER));
            p.text(1, 2, "(at least 60 columns by 16 rows)", fg(MUTED));
            return;
        }
        // Tabs.
        p.fill(Rect::new(0, 0, area.width, 1), BAR);
        let mut x = 1;
        for v in View::ALL {
            let label = format!("{} {}", v.index() + 1, v.name());
            let st = if v == self.view { on(GREEN, SELECT).add_modifier(Modifier::BOLD) } else { on(TEXT, BAR) };
            let w = p.link(x, 0, &label, Act::View(v), st);
            if v == self.view {
                p.fill(Rect::new(x.saturating_sub(1), 0, w + 2, 1), SELECT);
                p.text(x, 0, &label, st);
            }
            x += w + 2;
        }
        p.link(x, 0, "+", Act::NewChat, on(MUTED, BAR));
        let (m, local) = crate::models::model_label();
        let right = format!("{m} · {}  {}@{}", if local { "local" } else { "cloud" }, sysx::user(), sysx::hostname());
        let rw = right.chars().count() as u16;
        if area.width > x + rw + 4 {
            let rx = area.width - rw - 1;
            let mw = format!("{m} · {}", if local { "local" } else { "cloud" }).chars().count() as u16;
            p.link(
                rx,
                0,
                &format!("{m} · {}", if local { "local" } else { "cloud" }),
                Act::View(View::Models),
                on(GREEN, BAR),
            );
            p.text(rx + mw + 2, 0, &format!("{}@{}", sysx::user(), sysx::hostname()), on(MUTED, BAR));
        }
        let content = Rect::new(0, 1, area.width, area.height.saturating_sub(2));
        let inner = Rect::new(content.x + 1, content.y, content.width.saturating_sub(2), content.height);
        match self.view {
            View::Home => self.draw_home(p, inner),
            View::Ask => self.draw_ask(p, inner),
            View::Files => self.draw_files(p, inner),
            View::Models => self.draw_models(p, inner),
            View::Settings => self.draw_settings(p, inner),
            View::Packages => self.draw_packages(p, inner),
        }
        self.draw_status(p, area);
        if let Some(m) = &self.modal {
            let req = m.req.clone();
            let w = 56.min(area.width);
            let h = 14.min(area.height);
            let r = Rect::new((area.width - w) / 2, (area.height - h) / 2, w, h);
            for y in r.y..r.y + r.height {
                p.text(r.x, y, &" ".repeat(r.width as usize), fg(TEXT));
            }
            if let Some(inner) = p.pane(r, "needs your ok", true, Some(AMBER), None, None, 1.0) {
                ask::draw_confirm(p, inner, &req, true);
            }
        }
        if let Some(pill) = self.pill.clone() {
            self.draw_pill(p, area, &pill);
        }
        if let Some((t, _)) = &self.toast {
            let w = (t.chars().count() as u16 + 4).min(area.width);
            let r = Rect::new(area.width.saturating_sub(w + 1), 1, w, 3);
            for y in r.y..r.y + r.height {
                p.text(r.x, y, &" ".repeat(r.width as usize), fg(TEXT));
            }
            if let Some(inner) = p.pane(r, "", false, Some(AMBER), None, None, 1.0) {
                p.text(inner.x, inner.y, t, fg(BRIGHT));
            }
        }
    }

    fn draw_pill(&mut self, p: &mut Painter, area: Rect, pill: &Pill) {
        let w = 60.min(area.width);
        let r = Rect::new((area.width - w) / 2, area.height.saturating_sub(6), w, 3);
        for y in r.y..r.y + r.height {
            p.text(r.x, y, &" ".repeat(r.width as usize), fg(TEXT));
        }
        if let Some(inner) = p.pane(r, "", false, Some(AMBER), None, None, 1.0) {
            let y = inner.y;
            match pill {
                Pill::Listening(_) => {
                    p.text(inner.x, y, "●", fg(RED));
                    p.text(inner.x + 2, y, "listening", fg(BRIGHT));
                    let lv: Vec<f32> = self.levels.iter().copied().collect();
                    p.text(inner.x + 13, y, &dictate::bars(&lv), fg(AMBER));
                    let hint = "super+m done";
                    p.text(inner.x + inner.width - hint.len() as u16, y, hint, fg(MUTED));
                }
                _ => {
                    p.text(
                        inner.x,
                        y,
                        &format!("transcribing {}", spinner(self.started.elapsed().as_millis())),
                        fg(BRIGHT),
                    );
                    p.text(
                        inner.x + 16,
                        y,
                        "→ the words land in the ask bar. whisper runs on this machine.",
                        fg(MUTED),
                    );
                }
            }
        }
    }

    fn draw_status(&mut self, p: &mut Painter, area: Rect) {
        let y = area.height - 1;
        p.fill(Rect::new(0, y, area.width, 1), BAR);
        let mut x = 1;
        for v in View::ALL {
            let i = v.index();
            let st = if v == self.view { on(BRIGHT, EMPTY) } else { on(TEXT, BAR) };
            x += p.link(x, y, &format!(" {} ", i + 1), Act::Workspace(i), st);
        }
        x += 1;
        x += p.text(x, y, self.view.name(), on(MUTED, BAR)) + 2;
        if self.ask.running {
            let t = format!("{} working", spinner(self.started.elapsed().as_millis()));
            p.link(x, y, &t, Act::View(View::Ask), on(AMBER, BAR));
        }
        // Right-hand segments, laid out from the right edge.
        let (m, _) = crate::models::model_label();
        let mut segs: Vec<(String, Act, Style)> = Vec::new();
        if self.pill.is_some() {
            segs.push(("● rec".into(), Act::Mic, on(RED, BAR)));
        }
        segs.push((m, Act::View(View::Models), on(TEXT, BAR)));
        segs.push((
            self.wifi.clone().map_or("wifi off".into(), |w| format!("wifi {w}")),
            Act::SetTab(0),
            on(TEXT, BAR),
        ));
        if let Some((b, ch)) = self.snap.battery {
            segs.push((
                format!("bat {b}%{}", if ch { "⚡" } else { "" }),
                Act::SetTab(3),
                on(if b < 20 && !ch { RED } else { TEXT }, BAR),
            ));
        }
        segs.push((format!(" {} ", self.now.hhmm()), Act::View(View::Home), on(BRIGHT, EMPTY)));
        let total: u16 = segs.iter().map(|s| s.0.chars().count() as u16 + 2).sum();
        let mut rx = area.width.saturating_sub(total + 1);
        for (t, act, st) in segs {
            let w = p.link(rx, y, &t, act, st);
            rx += w + 2;
        }
    }

    fn draw_boot(&mut self, p: &mut Painter, t: f64) {
        let area = p.area;
        let (spark, ring, letters) = braille::boot_pose(t);
        let cols = ((area.height as usize).saturating_sub(8) * 2).clamp(14, 30).min(area.width as usize / 2);
        let logo = braille::braille(cols, ring, spark);
        let top = (area.height.saturating_sub(logo.len() as u16 + 6)) / 2;
        let left = (area.width.saturating_sub(cols as u16)) / 2;
        for (r, line) in logo.iter().enumerate() {
            for (c, (ch, ink)) in line.iter().enumerate() {
                let color = match ink {
                    Ink::Spark => GREEN,
                    Ink::Ring => BRIGHT,
                    Ink::Blank => continue,
                };
                p.text(left + c as u16, top + r as u16, &ch.to_string(), fg(color));
            }
        }
        let name: String = "tripppin".chars().take(letters).map(|c| format!("{c} ")).collect();
        let nx = (area.width.saturating_sub(15)) / 2;
        let ny = top + logo.len() as u16 + 1;
        p.text(nx, ny, name.trim_end(), fg(BRIGHT).add_modifier(Modifier::BOLD));
        let lines: Vec<String> = [
            std::fs::read_to_string("/proc/sys/kernel/osrelease").ok().map(|k| format!("kernel {}", k.trim())),
            Some(format!("host {}", sysx::hostname())),
        ]
        .into_iter()
        .flatten()
        .collect();
        if letters >= 4 {
            for (i, l) in lines.iter().enumerate() {
                p.text(left, ny + 2 + i as u16, l, fg(MUTED));
            }
        }
    }

    /// Draws one frame into a buffer (for tests and `trip home --dump`).
    pub fn render(&mut self, w: u16, h: u16) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        let mut hits = std::mem::take(&mut self.hits);
        hits.clear();
        {
            let mut p = Painter {
                buf: &mut buf,
                hits: &mut hits,
                hover: self.hover.clone(),
                pressed: self.pressed.clone(),
                area,
            };
            self.draw(&mut p);
        }
        self.hits = hits;
        buf
    }
}

pub fn buffer_text(buf: &Buffer) -> String {
    let a = buf.area;
    (a.y..a.y + a.height)
        .map(|y| {
            (a.x..a.x + a.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The poller: reads the machine's state on its own thread, and only as often and as long as
/// someone is looking.
fn poller(tx: Sender<Msg>, focused: Arc<AtomicBool>, wake: Receiver<()>, view_home: Arc<AtomicBool>) {
    let mut prev = CpuCounters::default();
    let mut snap = Snapshot::default();
    let mut tick = 0u64;
    let has_player = crate::apk::which("playerctl");
    loop {
        let looking = focused.load(Ordering::Relaxed);
        let home = view_home.load(Ordering::Relaxed);
        if looking {
            if tick.is_multiple_of(30) {
                sysx::read_slow(&mut snap);
            }
            let new = sysx::read_fast(&mut prev, &snap);
            if new != snap || tick == 0 {
                snap = new;
                if tx.send(Msg::Sys(Box::new(snap.clone()))).is_err() {
                    return;
                }
            }
            if tick.is_multiple_of(15) && tx.send(Msg::WifiName(sysx::wifi_status())).is_err() {
                return;
            }
            if has_player && home && tick.is_multiple_of(3) && tx.send(Msg::Playing(sysx::playing())).is_err() {
                return;
            }
            tick += 1;
        }
        // Once a second on the home screen, every ten seconds elsewhere, and not at all out of focus.
        let wait = match (looking, home) {
            (false, _) => Duration::from_secs(30),
            (true, true) => Duration::from_secs(1),
            (true, false) => Duration::from_secs(10),
        };
        if let Err(std::sync::mpsc::RecvTimeoutError::Disconnected) = wake.recv_timeout(wait) {
            return;
        }
    }
}

struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        let mut out = std::io::stdout();
        let _ = crossterm::execute!(
            out,
            crossterm::cursor::Show,
            DisableBracketedPaste,
            DisableFocusChange,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}

/// `trip home`.
pub fn run(skip_boot: bool) -> Result<()> {
    let mut out = std::io::stdout();
    enable_raw_mode()?;
    crossterm::execute!(out, EnterAlternateScreen, EnableMouseCapture, EnableFocusChange, EnableBracketedPaste)?;
    let _restore = Restore;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut o = std::io::stdout();
        let _ = crossterm::execute!(o, crossterm::cursor::Show, DisableMouseCapture, LeaveAlternateScreen);
        let _ = disable_raw_mode();
        hook(info);
    }));
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let size = crossterm::terminal::size().unwrap_or((100, 30));
    let (tx, rx) = channel::<Msg>();
    let mut app = App::new(tx.clone(), size);
    if !skip_boot {
        app.boot = Some(Instant::now());
    }
    // Terminal events: a blocking read on its own thread costs nothing while idle.
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            while let Ok(ev) = crossterm::event::read() {
                if tx.send(Msg::Term(ev)).is_err() {
                    break;
                }
            }
        });
    }
    dictate::listen(app.focused.clone(), tx.clone(), Msg::Dictate);
    let (wake_tx, wake_rx) = channel();
    app.wake = Some(wake_tx);
    {
        let (tx, focused, vh) = (tx.clone(), app.focused.clone(), app.home_visible.clone());
        std::thread::spawn(move || poller(tx, focused, wake_rx, vh));
    }
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    loop {
        if app.dirty {
            let since = last_draw.elapsed();
            if since >= Duration::from_millis(16) {
                app.dirty = false;
                last_draw = Instant::now();
                let mut hits = std::mem::take(&mut app.hits);
                hits.clear();
                let (hover, pressed) = (app.hover.clone(), app.pressed.clone());
                terminal.draw(|f| {
                    let area = f.area();
                    let mut p = Painter { buf: f.buffer_mut(), hits: &mut hits, hover, pressed, area };
                    app.draw(&mut p);
                    if let Some((x, y)) = app.cursor {
                        f.set_cursor_position((x, y));
                    }
                })?;
                app.hits = hits;
            }
        }
        let wait = if app.dirty {
            Duration::from_millis(16).saturating_sub(last_draw.elapsed()).max(Duration::from_millis(1))
        } else {
            app.next_wake()
        };
        match rx.recv_timeout(wait) {
            Ok(m) => {
                app.on_msg(m);
                while let Ok(m) = rx.try_recv() {
                    app.on_msg(m);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if app.animating().is_some() {
                    app.dirty = true;
                }
            }
            Err(_) => break,
        }
        app.tick();
        if app.quit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
