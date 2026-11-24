//! The widget host: one small card per widget folder on the desktop (layer-shell, below the
//! windows), drawn in our style from whatever its script printed. Checks the folders every
//! second, so new, changed and removed widgets show up by themselves; a script that fails,
//! hangs or prints nonsense shows its error inside its own card and never stops the host.

use super::spec::{self, Config, Item, Place, Shown, Status};
use crate::common::{GREEN, label};
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant, SystemTime};

struct Live {
    window: gtk::Window,
    title: gtk::Label,
    dot: gtk::Box,
    body: gtk::Box,
    dir: PathBuf,
    config: Config,
    fingerprint: String,
    next_run: Instant,
    running: bool,
}

#[derive(Default)]
struct Host {
    live: HashMap<String, Live>,
}

/// Changes to any of these mean the widget is built again: its files, its place, the screens.
fn fingerprint(dir: &Path, place: &Place) -> String {
    let mut files: Vec<(String, Option<SystemTime>, u64)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            let meta = e.metadata().ok();
            (
                e.file_name().to_string_lossy().into_owned(),
                meta.as_ref().and_then(|m| m.modified().ok()),
                meta.map_or(0, |m| m.len()),
            )
        })
        .collect();
    files.sort();
    format!("{files:?}{place:?}{:?}", connectors())
}

fn connectors() -> Vec<String> {
    let Some(display) = gtk::gdk::Display::default() else { return Vec::new() };
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|i| monitors.item(i).and_downcast::<gtk::gdk::Monitor>())
        .filter_map(|m| m.connector().map(|c| c.to_string()))
        .collect()
}

fn edges(place: &Place) -> (Edge, Edge) {
    (
        if place.anchor.left() { Edge::Left } else { Edge::Right },
        if place.anchor.top() { Edge::Top } else { Edge::Bottom },
    )
}

fn build(
    app: &gtk::Application,
    host: Weak<RefCell<Host>>,
    name: &str,
    dir: &Path,
    config: Config,
    place: &Place,
) -> Live {
    let window = gtk::Window::builder().application(app).decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("clear");
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-widget"));
    window.set_layer(Layer::Bottom);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_anchor(Edge::Top, place.anchor.top());
    window.set_anchor(Edge::Bottom, !place.anchor.top());
    window.set_anchor(Edge::Left, place.anchor.left());
    window.set_anchor(Edge::Right, !place.anchor.left());
    let (ex, ey) = edges(place);
    window.set_margin(ex, place.x);
    window.set_margin(ey, place.y);
    let monitor = match &place.output {
        Some(o) => crate::common::monitor_named(o),
        None => gtk::gdk::Display::default().and_then(|d| d.monitors().item(0)).and_downcast::<gtk::gdk::Monitor>(),
    };
    if let Some(m) = &monitor {
        window.set_monitor(Some(m));
    }

    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("widget-card");
    card.set_size_request(config.width.unwrap_or(260).clamp(120, 1200), -1);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = label(config.name.as_deref().unwrap_or(name), &["widget-title"]);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("widget-dot");
    dot.set_size_request(8, 8);
    dot.set_valign(gtk::Align::Center);
    head.append(&title);
    head.append(&dot);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
    card.append(&head);
    card.append(&body);
    window.set_child(Some(&card));
    draggable(&window, &card, host, name, place.clone());

    // Pinned to a screen that isn't plugged in: wait for it rather than appear elsewhere.
    if monitor.is_some() {
        window.present();
    } else {
        super::write_status(name, &format!("waiting for {}", place.output.as_deref().unwrap_or("a screen")));
    }
    Live {
        window,
        title,
        dot,
        body,
        dir: dir.to_path_buf(),
        fingerprint: fingerprint(dir, place),
        config,
        next_run: Instant::now(),
        running: false,
    }
}

/// Dragging a card moves it; where it's let go is remembered (widgets.json), on its screen.
fn draggable(window: &gtk::Window, card: &gtk::Box, host: Weak<RefCell<Host>>, name: &str, place: Place) {
    let drag = gtk::GestureDrag::new();
    let (ex, ey) = edges(&place);
    let (sx, sy) = (if place.anchor.left() { 1.0 } else { -1.0 }, if place.anchor.top() { 1.0 } else { -1.0 });
    {
        let w = window.clone();
        // The offset is measured on the card, which moves with each step, so the new place is
        // where the card is now plus the offset: that keeps the grabbed point under the pointer.
        drag.connect_drag_update(move |_, dx, dy| {
            w.set_margin(ex, (w.margin(ex) + (sx * dx).round() as i32).max(0));
            w.set_margin(ey, (w.margin(ey) + (sy * dy).round() as i32).max(0));
        });
    }
    {
        let (w, name) = (window.clone(), name.to_string());
        drag.connect_drag_end(move |_, _, _| {
            let mut all = super::load_placements();
            let p = all.entry(name.clone()).or_default();
            p.x = Some(w.margin(ex));
            p.y = Some(w.margin(ey));
            p.anchor = Some(place.anchor);
            p.output = place.output.clone();
            super::save_placements(&all);
            // Already where it was dropped: don't build it again for the new place.
            if let Some(host) = host.upgrade()
                && let Some(live) = host.borrow_mut().live.get_mut(&name)
            {
                let now = Place { x: w.margin(ex), y: w.margin(ey), ..place.clone() };
                live.fingerprint = fingerprint(&live.dir, &now);
            }
        });
    }
    card.add_controller(drag);
}

fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

fn set_status(dot: &gtk::Box, status: Status) {
    for c in ["warn", "error"] {
        dot.remove_css_class(c);
    }
    if let Some(c) = status.class() {
        dot.add_css_class(c);
    }
}

fn wrapping(text: &str, classes: &[&str]) -> gtk::Label {
    let l = label(text, classes);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l.set_max_width_chars(28);
    l.set_hexpand(true);
    l
}

fn spark(values: Vec<f64>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_height(32);
    area.set_hexpand(true);
    area.add_css_class("widget-spark");
    area.set_draw_func(move |_, cr, w, h| {
        if values.len() < 2 {
            return;
        }
        let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let span = if (hi - lo).abs() < f64::EPSILON { 1.0 } else { hi - lo };
        let (w, h) = (f64::from(w), f64::from(h) - 2.0);
        let step = w / (values.len() - 1) as f64;
        let y = |v: f64| 1.0 + h - (v - lo) / span * h;
        cr.move_to(0.0, y(values[0]));
        for (i, v) in values.iter().enumerate().skip(1) {
            cr.line_to(i as f64 * step, y(*v));
        }
        cr.set_source_rgba(GREEN.0, GREEN.1, GREEN.2, 1.0);
        cr.set_line_width(1.5);
        cr.stroke_preserve().ok();
        cr.line_to(w, h + 2.0);
        cr.line_to(0.0, h + 2.0);
        cr.close_path();
        cr.set_source_rgba(GREEN.0, GREEN.1, GREEN.2, 0.12);
        cr.fill().ok();
    });
    area
}

fn item_widget(item: &Item) -> gtk::Widget {
    match item {
        Item::Row { label: l, value, bar, status } => {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 4);
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let name = label(l, &["widget-label"]);
            name.set_hexpand(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let v = label(value, &["widget-value"]);
            v.set_xalign(1.0);
            if let Some(c) = status.class() {
                v.add_css_class(c);
            }
            row.append(&name);
            row.append(&v);
            col.append(&row);
            if let Some(f) = bar {
                let b = gtk::ProgressBar::new();
                b.add_css_class("widget-bar");
                if let Some(c) = status.class() {
                    b.add_css_class(c);
                }
                b.set_fraction(*f);
                col.append(&b);
            }
            col.upcast()
        }
        Item::Text(t) => wrapping(t, &["widget-text"]).upcast(),
        Item::Big { value, label: caption } => {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 2);
            col.append(&wrapping(value, &["widget-big"]));
            if let Some(c) = caption {
                col.append(&wrapping(c, &["widget-caption"]));
            }
            col.upcast()
        }
        Item::Spark { values, label: caption } => {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 4);
            if let Some(c) = caption {
                col.append(&label(c, &["widget-caption"]));
            }
            col.append(&spark(values.clone()));
            col.upcast()
        }
    }
}

fn render(name: &str, live: &Live, result: Result<Shown, String>) {
    clear(&live.body);
    let fallback = live.config.name.clone().unwrap_or_else(|| name.to_string());
    match result {
        Ok(shown) => {
            live.title.set_text(shown.title.as_deref().unwrap_or(&fallback));
            set_status(&live.dot, shown.status);
            for item in &shown.items {
                live.body.append(&item_widget(item));
            }
            if let Some(f) = &shown.footer {
                live.body.append(&wrapping(f, &["widget-foot"]));
            }
            super::write_status(name, &format!("works ({})", spec::summary(&shown)));
        }
        Err(e) => {
            live.title.set_text(&fallback);
            set_status(&live.dot, Status::Error);
            live.body.append(&wrapping(&e, &["widget-error"]));
            super::write_status(name, &format!("error: {e}"));
        }
    }
}

/// Runs a widget's script off the main thread and shows the result when it's back.
fn run_widget(host: &Rc<RefCell<Host>>, name: &str) {
    let (dir, config) = {
        let mut h = host.borrow_mut();
        let Some(live) = h.live.get_mut(name) else { return };
        live.running = true;
        (live.dir.clone(), live.config.clone())
    };
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(super::run_once(&dir, &config));
    });
    let (host, name) = (Rc::downgrade(host), name.to_string());
    glib::spawn_future_local(async move {
        let Ok(result) = rx.recv().await else { return };
        let Some(host) = host.upgrade() else { return };
        let mut h = host.borrow_mut();
        // Removed or rebuilt while it ran: the result belongs to the old one.
        let Some(live) = h.live.get_mut(&name) else { return };
        if !live.running {
            return;
        }
        render(&name, live, result);
        live.running = false;
        live.next_run = Instant::now() + Duration::from_secs(live.config.every_secs());
    });
}

/// Brings the cards in line with the folders, and runs the scripts that are due.
fn tick(app: &gtk::Application, host: &Rc<RefCell<Host>>) {
    let placements = super::load_placements();
    let found = super::installed();
    let mut due = Vec::new();
    {
        let mut h = host.borrow_mut();
        h.live.retain(|name, live| {
            let keep = found.iter().any(|(n, _, c)| n == name && c.is_ok());
            if !keep {
                live.window.destroy();
            }
            keep
        });
        for (name, dir, config) in found {
            let config = match config {
                Ok(c) => c,
                Err(e) => {
                    super::write_status(&name, &format!("broken: {e}"));
                    continue;
                }
            };
            let place = spec::place(&config, placements.get(&name));
            let changed =
                h.live.get(&name).is_none_or(|l| l.fingerprint != fingerprint(&dir, &place) || l.config != config);
            if changed {
                if let Some(old) = h.live.remove(&name) {
                    old.window.destroy();
                }
                let live = build(app, Rc::downgrade(host), &name, &dir, config, &place);
                h.live.insert(name.clone(), live);
            }
            if let Some(l) = h.live.get(&name)
                && !l.running
                && Instant::now() >= l.next_run
                && l.window.is_visible()
            {
                due.push(name);
            }
        }
    }
    for name in due {
        run_widget(host, &name);
    }
}

pub fn run() -> glib::ExitCode {
    let app =
        gtk::Application::builder().application_id("in.trippp.Widgets").flags(gio::ApplicationFlags::default()).build();
    let started: Rc<RefCell<Option<gio::ApplicationHoldGuard>>> = Rc::default();
    app.connect_activate(move |app| {
        if started.borrow().is_some() {
            return;
        }
        // Stays running with no widgets, so one added later appears.
        *started.borrow_mut() = Some(app.hold());
        crate::common::load_css();
        let host: Rc<RefCell<Host>> = Rc::default();
        let app = app.clone();
        tick(&app, &host);
        glib::timeout_add_local(Duration::from_secs(1), move || {
            tick(&app, &host);
            glib::ControlFlow::Continue
        });
    });
    let args: Vec<String> = std::env::args().take(1).collect();
    app.run_with_args(&args)
}
