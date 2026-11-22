//! The orb: the AI, resting in the bottom-right corner of the desktop.
//!
//! Click it or press Super+Space (`tripppin-ui orb --toggle`) and a glowing bar asks what you
//! want. While it works, a card above the orb shows each step live: done, running (with the
//! command's latest output) and the answer. The same agent as the terminal does the work.

use crate::common::{self, label};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use trip::agent::{self, Ui};
use trip::model::Message;

const SUGGESTIONS: [&str; 3] = ["free up some space", "connect my headphones", "what changed today?"];

/// What the agent's thread tells the window.
enum Event {
    Text(String),
    Step(String, String),
    Output(String),
    Note(String),
    StepDone(bool),
    Thinking,
    Done(Result<(), String>),
}

/// The agent's progress, sent across to the GTK thread.
struct Sender(async_channel::Sender<Event>);

impl Ui for Sender {
    fn text(&mut self, piece: &str) {
        let _ = self.0.send_blocking(Event::Text(piece.to_string()));
    }
    fn step(&mut self, label: &str, detail: &str) {
        let _ = self.0.send_blocking(Event::Step(label.to_string(), detail.to_string()));
    }
    fn output(&mut self, line: &str) {
        let _ = self.0.send_blocking(Event::Output(line.to_string()));
    }
    fn note(&mut self, text: &str) {
        let _ = self.0.send_blocking(Event::Note(text.to_string()));
    }
    fn step_done(&mut self, ok: bool) {
        let _ = self.0.send_blocking(Event::StepDone(ok));
    }
    fn thinking(&mut self) {
        let _ = self.0.send_blocking(Event::Thinking);
    }
}

/// Turns a step into words: "installing org.videolan.VLC", "checking disk space".
pub fn describe(label: &str, detail: &str) -> String {
    if label != "exec" && label != "exec in the background" {
        return match label {
            "read" => format!("reading {}", short(detail)),
            "write" => format!("writing {}", short(detail)),
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
            return "asking for a password".into();
        }
    }
    let arg = |i: usize| words.iter().skip(i).find(|w| !w.starts_with('-')).copied().unwrap_or("");
    match (words.first().copied().unwrap_or(""), words.get(1).copied().unwrap_or("")) {
        ("flatpak", "install") => format!("installing {}", words.last().copied().unwrap_or("an app")),
        ("flatpak", "search") => format!("searching for {}", arg(2)),
        ("flatpak", "uninstall") => format!("removing {}", words.last().copied().unwrap_or("an app")),
        ("flatpak", "run") => format!("opening {}", arg(2)),
        ("dnf", "install") => format!("installing {}", words.last().copied().unwrap_or("a package")),
        ("df", _) | ("du", _) => "checking disk space".into(),
        ("free", _) => "checking memory".into(),
        ("ls", _) | ("find", _) => "looking through files".into(),
        ("ps", _) | ("top", _) => "checking what's running".into(),
        ("nmcli", _) => "checking the network".into(),
        ("bluetoothctl", _) => "talking to bluetooth".into(),
        ("curl", _) | ("wget", _) => "downloading".into(),
        ("mkdir", _) => format!("making {}", short(arg(1))),
        ("rm", _) => "deleting files".into(),
        ("cp", _) | ("mv", _) | ("rsync", _) => "moving files".into(),
        ("xdg-open", _) => format!("opening {}", short(arg(1))),
        (first, _) if !first.is_empty() => format!("running {first}"),
        _ => "running a command".into(),
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

/// One row in the working card.
struct StepRow {
    root: gtk::Box,
    done: std::cell::Cell<bool>,
    mark: gtk::Stack,
    title: gtk::Label,
    detail: gtk::Label,
}

impl StepRow {
    fn new(title: &str, detail: &str) -> Self {
        let mark = gtk::Stack::new();
        mark.set_valign(gtk::Align::Start);
        mark.set_margin_top(1);
        let spinner = gtk::Spinner::new();
        spinner.set_size_request(18, 18);
        spinner.start();
        mark.add_named(&spinner, Some("running"));
        let tick = gtk::Label::new(Some("✓"));
        tick.add_css_class("tick");
        mark.add_named(&tick, Some("done"));
        let cross = gtk::Label::new(Some("!"));
        cross.add_css_class("tick");
        cross.add_css_class("failed");
        mark.add_named(&cross, Some("failed"));
        mark.set_visible_child_name("running");

        let title = label(title, &["step-title", "running"]);
        title.set_wrap(true);
        let detail = label(detail, &["step-detail"]);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        detail.set_max_width_chars(48);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.append(&title);
        text.append(&detail);
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        root.append(&mark);
        root.append(&text);
        StepRow { root, done: std::cell::Cell::new(false), mark, title, detail }
    }

    fn finish(&self, ok: bool) {
        if self.done.replace(true) {
            return;
        }
        self.mark.set_visible_child_name(if ok { "done" } else { "failed" });
        self.title.remove_css_class("running");
    }
}

struct State {
    cancel: Arc<AtomicBool>,
    busy: bool,
    started: Option<Instant>,
    phase: &'static str,
    answer: String,
    steps: Vec<StepRow>,
}

struct Orb {
    button: gtk::Button,
    model: gtk::Label,
    ask: gtk::Window,
    entry: gtk::Entry,
    card: gtk::Window,
    asked: gtk::Label,
    status: gtk::Label,
    scroller: gtk::ScrolledWindow,
    inner: gtk::Box,
    steps_box: gtk::Box,
    answer: gtk::Label,
    stop: gtk::Button,
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
    // ---- the resting orb, bottom right, with its hint to the left ----
    let corner = layer_window(app, "tripppin-orb");
    corner.set_layer(Layer::Top);
    corner.set_anchor(Edge::Bottom, true);
    corner.set_anchor(Edge::Right, true);
    corner.set_keyboard_mode(KeyboardMode::None);
    let button = gtk::Button::new();
    button.add_css_class("orb");
    button.set_child(Some(&common::spark(28, false)));
    button.set_tooltip_text(Some("ask tripppin (super + space)"));
    button.set_margin_end(36);
    button.set_margin_bottom(30);
    button.set_margin_top(40);
    button.set_margin_start(20);
    let hint = label("super + space", &["orb-hint"]);
    hint.set_valign(gtk::Align::End);
    hint.set_margin_bottom(50);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.append(&hint);
    row.append(&button);
    corner.set_child(Some(&row));
    // Only the orb itself takes clicks; the glow and hint let them through.
    let target = button.clone();
    corner.connect_map(move |w| {
        let w = w.clone();
        let target = target.clone();
        glib::idle_add_local_once(move || {
            if let (Some(surface), Some(b)) = (w.surface(), target.compute_bounds(&w)) {
                let rect = gtk::cairo::RectangleInt::new(
                    b.x() as i32,
                    b.y() as i32,
                    b.width().ceil() as i32,
                    b.height().ceil() as i32,
                );
                surface.set_input_region(Some(&gtk::cairo::Region::create_rectangle(&rect)));
            }
        });
    });

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
    let column = gtk::Box::new(gtk::Orientation::Vertical, 14);
    column.set_halign(gtk::Align::Center);
    column.set_valign(gtk::Align::Center);
    column.set_margin_bottom(120);
    column.set_size_request(700, -1);
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    pill.add_css_class("ask-pill");
    let pill_orb = gtk::CenterBox::new();
    pill_orb.add_css_class("ask-orb");
    pill_orb.set_valign(gtk::Align::Center);
    pill_orb.set_hexpand(false);
    pill_orb.set_size_request(52, 52);
    let s = common::spark(24, false);
    s.set_valign(gtk::Align::Center);
    pill_orb.set_center_widget(Some(&s));
    let entry = gtk::Entry::new();
    entry.add_css_class("ask");
    entry.set_hexpand(true);
    entry.set_valign(gtk::Align::Center);
    entry.set_placeholder_text(Some("tell your computer what you want"));
    let send = gtk::Button::new();
    send.add_css_class("send");
    send.set_valign(gtk::Align::Center);
    send.set_child(Some(&common::arrow(true)));
    send.set_tooltip_text(Some("send"));
    pill.append(&pill_orb);
    pill.append(&entry);
    pill.append(&send);
    let chips = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    chips.set_margin_start(12);
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
    footer.set_margin_start(16);
    footer.set_margin_end(16);
    let keys = label("enter to send · esc to close", &["hint"]);
    keys.set_hexpand(true);
    footer.append(&keys);
    let model = label(&model_line(), &["hint"]);
    footer.append(&model);
    column.append(&pill);
    column.append(&chips);
    column.append(&footer);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&shade));
    shade.set_hexpand(true);
    shade.set_vexpand(true);
    overlay.add_overlay(&column);
    ask.set_child(Some(&overlay));
    // Clicking the dim area closes the bar.
    let close_click = gtk::GestureClick::new();
    {
        let ask = ask.clone();
        close_click.connect_released(move |_, _, _, _| ask.set_visible(false));
    }
    shade.add_controller(close_click);

    // ---- the working card, above the orb ----
    let card = layer_window(app, "tripppin-orb-card");
    card.set_layer(Layer::Top);
    card.set_anchor(Edge::Bottom, true);
    card.set_anchor(Edge::Right, true);
    card.set_margin(Edge::Bottom, 112);
    card.set_margin(Edge::Right, 36);
    card.set_keyboard_mode(KeyboardMode::OnDemand);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 16);
    body.add_css_class("card");
    body.set_size_request(440, -1);
    body.set_margin_top(30);
    body.set_margin_start(30);
    body.set_margin_end(0);
    body.set_margin_bottom(0);
    let you = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let caption = label("you asked", &["caption"]);
    caption.set_hexpand(true);
    let status = label("", &["caption", "live"]);
    top.append(&caption);
    top.append(&status);
    you.append(&top);
    let asked = label("", &["asked"]);
    asked.set_wrap(true);
    asked.set_max_width_chars(44);
    you.append(&asked);
    let steps_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let answer = label("", &["answer"]);
    answer.set_wrap(true);
    answer.set_max_width_chars(46);
    answer.set_selectable(true);
    answer.set_visible(false);
    let foot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    foot.add_css_class("card-foot");
    let terminal = gtk::Button::with_label("show in terminal");
    terminal.add_css_class("link");
    terminal.set_hexpand(true);
    terminal.set_halign(gtk::Align::Start);
    terminal.connect_clicked(|_| {
        let _ = std::process::Command::new("foot").spawn();
    });
    let stop = gtk::Button::with_label("stop");
    stop.add_css_class("stop");
    foot.append(&terminal);
    foot.append(&stop);
    // Steps and the answer scroll once they outgrow the card; the newest stays in view.
    let inner = gtk::Box::new(gtk::Orientation::Vertical, 14);
    inner.append(&steps_box);
    inner.append(&answer);
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_propagate_natural_height(true);
    scroller.set_max_content_height(380);
    scroller.set_child(Some(&inner));
    body.append(&you);
    body.append(&scroller);
    body.append(&foot);
    card.set_child(Some(&body));

    let orb = Rc::new(Orb {
        button,
        model,
        ask,
        entry,
        card,
        asked,
        status,
        scroller,
        inner,
        steps_box,
        answer,
        stop,
        state: RefCell::new(State {
            cancel: Arc::new(AtomicBool::new(false)),
            busy: false,
            started: None,
            phase: "",
            answer: String::new(),
            steps: Vec::new(),
        }),
    });

    {
        let o = orb.clone();
        orb.button.connect_clicked(move |_| o.toggle());
    }
    {
        let o = orb.clone();
        orb.entry.connect_activate(move |_| o.submit());
    }
    {
        let o = orb.clone();
        send.connect_clicked(move |_| o.submit());
    }
    {
        let o = orb.clone();
        orb.stop.connect_clicked(move |_| o.stop_or_close());
    }
    let keys = gtk::EventControllerKey::new();
    {
        let ask = orb.ask.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                ask.set_visible(false);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }
    orb.ask.add_controller(keys);

    corner.present();
    orb
}

fn model_line() -> String {
    let catalog = hwdetect::Catalog::builtin().ok();
    let tier = catalog.as_ref().and_then(|c| trip::model::active_tier(c, &trip::model::models_dir()));
    match tier {
        Some(t) => format!("{} · on this computer", t.model.to_lowercase()),
        None => "no model yet · it downloads on first start".into(),
    }
}

fn write_status(text: &str) {
    let _ = std::fs::write(common::runtime_dir().join("orb-status"), text);
}

impl Orb {
    fn toggle(self: &Rc<Self>) {
        let busy = self.state.borrow().busy;
        let has_result = !self.state.borrow().steps.is_empty() || !self.state.borrow().answer.is_empty();
        if busy || (has_result && self.card.is_visible()) {
            self.card.set_visible(!self.card.is_visible());
            return;
        }
        if self.ask.is_visible() {
            self.ask.set_visible(false);
        } else {
            self.card.set_visible(false);
            self.entry.set_text("");
            // The model may have arrived since the orb started (first boot downloads it).
            self.model.set_text(&model_line());
            self.ask.present();
            self.entry.grab_focus();
        }
    }

    fn stop_or_close(&self) {
        let st = self.state.borrow();
        if st.busy {
            st.cancel.store(true, Ordering::Relaxed);
            self.stop.set_label("stopping…");
        } else {
            drop(st);
            self.card.set_visible(false);
        }
    }

    fn submit(self: &Rc<Self>) {
        let text = self.entry.text().trim().to_string();
        if text.is_empty() || self.state.borrow().busy {
            return;
        }
        self.ask.set_visible(false);
        while let Some(child) = self.steps_box.first_child() {
            self.steps_box.remove(&child);
        }
        self.scroller.set_min_content_height(0);
        self.asked.set_text(&text);
        self.answer.set_text("");
        self.answer.set_visible(false);
        self.stop.set_label("stop");
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut st = self.state.borrow_mut();
            st.busy = true;
            st.started = Some(Instant::now());
            st.phase = "thinking";
            st.answer.clear();
            st.steps.clear();
            st.cancel = cancel.clone();
        }
        self.button.add_css_class("working");
        write_status("● working");
        self.card.present();
        self.tick();
        // A live status with a running clock, so it's clear the orb is still at it.
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local(1, move || match weak.upgrade() {
            Some(orb) if orb.state.borrow().busy => {
                orb.tick();
                glib::ControlFlow::Continue
            }
            _ => glib::ControlFlow::Break,
        });

        let (tx, rx) = async_channel::unbounded::<Event>();
        std::thread::spawn(move || {
            let mut history = vec![Message::system(), Message::user(&text)];
            let mut ui = Sender(tx.clone());
            let result = agent::run(&mut history, &cancel, &mut ui).map(|_| ()).map_err(|e| format!("{e:#}"));
            let _ = tx.send_blocking(Event::Done(result));
        });
        let orb = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = rx.recv().await {
                let done = matches!(event, Event::Done(_));
                orb.apply(event);
                if done {
                    break;
                }
            }
        });
    }

    fn elapsed(&self) -> String {
        let secs = self.state.borrow().started.map_or(0, |t| t.elapsed().as_secs());
        format!("{}:{:02}", secs / 60, secs % 60)
    }

    fn tick(&self) {
        let phase = self.state.borrow().phase;
        self.status.set_text(&format!("● {phase} · {}", self.elapsed()));
    }

    /// Grows the list with its content up to `MAX_LIST`, then it scrolls; keeps the newest in view.
    /// (A layer-shell window doesn't grow on its own when its content does.)
    fn scroll_to_end(&self) {
        // Room for three or four steps from the start, so the list reads as a list.
        const MIN_LIST: i32 = 200;
        const MAX_LIST: i32 = 380;
        let (scroller, inner) = (self.scroller.clone(), self.inner.clone());
        glib::idle_add_local_once(move || {
            let width = scroller.width().max(396);
            let (_, natural, _, _) = inner.measure(gtk::Orientation::Vertical, width);
            scroller.set_min_content_height(natural.clamp(MIN_LIST, MAX_LIST));
            glib::idle_add_local_once(move || {
                let adj = scroller.vadjustment();
                adj.set_value(adj.upper() - adj.page_size());
            });
        });
    }

    fn apply(&self, event: Event) {
        let mut st = self.state.borrow_mut();
        match event {
            Event::Text(piece) => {
                st.phase = "answering";
                st.answer.push_str(&piece);
                self.scroll_to_end();
                let shown = st.answer.trim().to_string();
                self.answer.set_markup(&markup(&shown));
                self.answer.set_visible(!shown.is_empty());
            }
            Event::Thinking => st.phase = "thinking",
            Event::StepDone(ok) => {
                if let Some(last) = st.steps.last() {
                    last.finish(ok);
                }
            }
            Event::Step(kind, detail) => {
                st.phase = "working";
                if let Some(last) = st.steps.last() {
                    last.finish(true);
                }
                // Text before a tool call is the model thinking aloud; the steps say it better.
                st.answer.clear();
                self.answer.set_visible(false);
                let row = StepRow::new(&describe(&kind, &detail), &detail);
                self.steps_box.append(&row.root);
                st.steps.push(row);
                self.scroll_to_end();
            }
            Event::Output(line) => {
                // Progress bars redraw with carriage returns; show the latest state of the line.
                let line = line.rsplit('\r').find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
                if let Some(last) = st.steps.last()
                    && !line.is_empty()
                {
                    last.detail.set_text(&line);
                }
            }
            Event::Note(text) => {
                if let Some(last) = st.steps.last()
                    && text.contains("background as job")
                {
                    let job = text.rsplit(' ').next().unwrap_or("");
                    last.title.set_text(&format!("{} · job {job}", last.title.text()));
                }
            }
            Event::Done(result) => {
                st.busy = false;
                let ok = result.is_ok();
                if let Some(last) = st.steps.last() {
                    last.finish(ok);
                }
                let took = st.started.map_or(0, |t| t.elapsed().as_secs());
                let verb = if ok { "done in" } else { "stopped after" };
                self.status.set_text(&format!("{verb} {}:{:02}", took / 60, took % 60));
                self.scroll_to_end();
                if let Err(e) = result {
                    let msg = if e == "stopped" { "stopped.".to_string() } else { e };
                    self.answer.set_text(&msg);
                    self.answer.set_visible(true);
                }
                self.button.remove_css_class("working");
                self.stop.set_label("close");
                write_status("");
                let summary = st.answer.trim().lines().next().unwrap_or("done").to_string();
                let _ = std::fs::write(common::runtime_dir().join("last-done"), summary);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{describe, markup};

    #[test]
    fn renders_light_markdown() {
        assert_eq!(markup("**Disk:** 40 GB"), "<b>Disk:</b> 40 GB");
        assert_eq!(markup("run `df -h`"), "run <span font_family=\"DM Mono\">df -h</span>");
        assert_eq!(markup("a < b & **half"), "a &lt; b &amp; <b>half</b>");
    }

    #[test]
    fn steps_read_like_words() {
        assert_eq!(describe("exec", "sudo flatpak install -y flathub org.videolan.VLC"), "installing org.videolan.VLC");
        assert_eq!(describe("exec", "flatpak search vlc"), "searching for vlc");
        assert_eq!(describe("exec", "df -h /"), "checking disk space");
        assert_eq!(describe("exec", "mkdir -p ~/demo"), "making demo");
        assert_eq!(describe("exec", "python3 primes.py"), "running python3");
        assert_eq!(describe("read", "/etc/os-release"), "reading os-release");
        assert_eq!(describe("waiting on job 2", "sleep 9"), "waiting on job 2");
        assert_eq!(describe("exec", "sudo -v"), "asking for a password");
        assert_eq!(describe("exec", "sudo -n dnf install -y vlc"), "installing vlc");
    }
}
