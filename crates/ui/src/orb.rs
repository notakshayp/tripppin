//! The orb: the AI, resting in a corner of the desktop (bottom right to start with).
//!
//! Click it or press Super+Space (`tripppin-ui orb --toggle`) and a glowing bar asks what you
//! want. While it works, a card next to the orb shows each step live: done, running (with the
//! command's latest output) and the answer. The same agent as the terminal does the work.
//! Drag the orb and it springs to the nearest corner; right-click tucks it away to a small
//! dot. The card minimises back into the orb.

use crate::common::{self, icons, label};
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

/// Space between the orb's glow and the screen edge, and room for the bar at the top.
const EDGE: i32 = 6;
const BELOW_BAR: i32 = 46;
/// The glow around the orb, inside its window (clicks pass through it).
const GLOW: i32 = 14;

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
        mark.set_transition_type(gtk::StackTransitionType::Crossfade);
        mark.set_transition_duration(160);
        mark.add_named(&common::spinner(16), Some("running"));
        mark.add_named(&label(&icons::CHECK.to_string(), &["tick"]), Some("done"));
        mark.add_named(&label(&icons::ALERT.to_string(), &["tick", "failed"]), Some("failed"));
        mark.set_visible_child_name("running");

        let title = label(title, &["step-title", "running"]);
        title.set_wrap(true);
        let detail = label(detail, &["step-detail"]);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        detail.set_max_width_chars(48);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.append(&title);
        text.append(&detail);
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        root.append(&mark);
        root.append(&text);
        common::play(&root, "rise");
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
    corner_window: gtk::Window,
    button: gtk::Button,
    logo: gtk::DrawingArea,
    hint: gtk::Label,
    row: gtk::Box,
    corner: std::cell::Cell<Corner>,
    tucked: std::cell::Cell<bool>,
    model: gtk::Label,
    ask: gtk::Window,
    entry: gtk::Entry,
    card: gtk::Window,
    body: gtk::Box,
    asked: gtk::Label,
    status: gtk::Label,
    working: gtk::DrawingArea,
    scroller: gtk::ScrolledWindow,
    inner: gtk::Box,
    steps_box: gtk::Box,
    answer: gtk::Label,
    stop: gtk::Button,
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
    button.set_tooltip_text(Some("ask tripppin (super + space)\ndrag to move it · right-click to tuck it away"));
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
    let hint = label("super + space", &["orb-hint"]);
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
    entry.set_placeholder_text(Some("tell your computer what you want"));
    let send = gtk::Button::new();
    send.add_css_class("send");
    send.set_valign(gtk::Align::Center);
    send.set_child(Some(&common::arrow(true)));
    send.set_tooltip_text(Some("send"));
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

    // ---- the working card, next to the orb ----
    let card = layer_window(app, "tripppin-orb-card");
    card.set_layer(Layer::Top);
    card.set_exclusive_zone(-1);
    card.set_keyboard_mode(KeyboardMode::OnDemand);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.add_css_class("card");
    body.set_size_request(380, -1);
    // Room for the card's shadow inside its window.
    body.set_margin_top(24);
    body.set_margin_bottom(24);
    body.set_margin_start(24);
    body.set_margin_end(24);
    let you = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let caption = label("you asked", &["caption"]);
    caption.set_hexpand(true);
    caption.set_valign(gtk::Align::Center);
    let status = label("", &["caption", "live"]);
    status.set_valign(gtk::Align::Center);
    // Moving while it works, so it never looks stuck, even before the first step.
    let working = common::spinner(12);
    working.set_valign(gtk::Align::Center);
    working.set_visible(false);
    let minimise = gtk::Button::with_label(&icons::MINIMIZE.to_string());
    minimise.add_css_class("icon-button");
    minimise.set_tooltip_text(Some("minimise into the orb"));
    top.append(&caption);
    top.append(&working);
    top.append(&status);
    top.append(&minimise);
    you.append(&top);
    let asked = label("", &["asked"]);
    asked.set_wrap(true);
    asked.set_max_width_chars(44);
    you.append(&asked);
    let steps_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let answer = label("", &["answer"]);
    answer.set_wrap(true);
    answer.set_max_width_chars(42);
    answer.set_selectable(true);
    answer.set_visible(false);
    let foot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    foot.add_css_class("card-foot");
    let terminal = gtk::Button::with_label("show in terminal");
    terminal.set_valign(gtk::Align::Center);
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
    let inner = gtk::Box::new(gtk::Orientation::Vertical, 12);
    inner.append(&steps_box);
    inner.append(&answer);
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_propagate_natural_height(true);
    scroller.set_max_content_height(340);
    scroller.set_child(Some(&inner));
    body.append(&you);
    body.append(&scroller);
    body.append(&foot);
    card.set_child(Some(&body));

    let orb = Rc::new(Orb {
        corner_window: corner.clone(),
        button,
        logo,
        hint: hint.clone(),
        row,
        corner: std::cell::Cell::new(corner_at),
        tucked: std::cell::Cell::new(tucked),
        model,
        ask,
        entry,
        card,
        body,
        asked,
        status,
        working,
        scroller,
        inner,
        steps_box,
        answer,
        stop,
        shade: shade.clone(),
        column: column.clone(),
        state: RefCell::new(State {
            cancel: Arc::new(AtomicBool::new(false)),
            busy: false,
            started: None,
            phase: "",
            answer: String::new(),
            steps: Vec::new(),
        }),
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
        send.connect_clicked(move |_| o.submit());
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

    corner.present();
    orb
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
        Some(t) => format!("{} · on this computer", t.model.to_lowercase()),
        None => "no model yet · it downloads on first start".into(),
    }
}

/// niri has no caps-lock event, so the orb (always running) watches the keyboards' caps-lock
/// light and shows the pop-up when it changes.
fn watch_caps_lock() {
    let last = std::cell::Cell::new(crate::osd::caps_lock());
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
        let busy = self.state.borrow().busy;
        let has_result = !self.state.borrow().steps.is_empty() || !self.state.borrow().answer.is_empty();
        if busy || (has_result && self.card.is_visible()) {
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
            self.card.set_visible(false);
            self.entry.set_text("");
            // The model may have arrived since the orb started (first boot downloads it).
            self.model.set_text(&model_line());
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
    }

    fn stop_or_close(&self) {
        let st = self.state.borrow();
        if st.busy {
            st.cancel.store(true, Ordering::Relaxed);
            self.stop.set_label("stopping…");
        } else {
            drop(st);
            leave(&self.card, &self.body);
        }
    }

    fn close_ask(&self) {
        leave(&self.ask, &self.column);
    }

    fn show_card(&self) {
        self.card.present();
        common::play(&self.body, if self.corner.get().top() { "drop" } else { "rise" });
    }

    /// Back into the orb; it keeps glowing while it works, and a click brings the card back.
    fn minimise(&self) {
        leave(&self.card, &self.body);
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
        let start: Rc<std::cell::Cell<(i32, i32)>> = Rc::new(std::cell::Cell::new((0, 0)));
        let moved = Rc::new(std::cell::Cell::new(false));
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
        let begun = std::cell::Cell::new(None::<i64>);
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
        self.working.set_visible(true);
        write_status("● working");
        self.show_card();
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
        self.status.set_text(&format!("{phase} · {}", self.elapsed()));
    }

    /// Grows the list with its content up to `MAX_LIST`, then it scrolls; keeps the newest in view.
    /// (A layer-shell window doesn't grow on its own when its content does.)
    fn scroll_to_end(&self) {
        // Room for a few steps from the start, so the list reads as a list.
        const MIN_LIST: i32 = 120;
        const MAX_LIST: i32 = 340;
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
                if !shown.is_empty() && !self.answer.is_visible() {
                    self.answer.set_visible(true);
                    common::play(&self.answer, "fade");
                }
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
                // Finished while minimised: come back to show the answer.
                if !self.card.is_visible() {
                    self.show_card();
                }
                self.button.remove_css_class("working");
                self.working.set_visible(false);
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
    use super::{Corner, describe, markup, parse_prefs};

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
