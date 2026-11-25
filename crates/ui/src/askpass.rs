//! The password pop-up sudo uses when the AI runs an administrator command without a terminal
//! (`SUDO_ASKPASS`, through /usr/libexec/tripppin-askpass). It says which command wants the
//! password (`TRIPPPIN_SUDO_FOR`), prints the password for sudo on "allow", and exits with 1 on
//! "cancel" or Escape, which sudo treats as no password. It records which it was in the file
//! named by `TRIPPPIN_SUDO_MARK`, for the activity list in Settings → AI, and warns when the
//! command is a risky one (see `trip::permissions::is_risky`).

use crate::common::{self, label};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::Cell;
use std::io::Write;
use std::rc::Rc;

/// How much of the command to show; the rest is cut.
const COMMAND_CHARS: usize = 220;

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("in.trippp.Askpass")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let allowed = Rc::new(Cell::new(false));
    {
        let allowed = allowed.clone();
        app.connect_activate(move |app| build(app, allowed.clone()));
    }
    let args: Vec<String> = std::env::args().take(1).collect();
    app.run_with_args(&args);
    mark(if allowed.get() { "asked" } else { "cancelled" });
    if allowed.get() { glib::ExitCode::SUCCESS } else { glib::ExitCode::FAILURE }
}

/// Tells the sudo wrapper what happened here (never the password).
fn mark(what: &str) {
    if let Some(path) = std::env::var_os("TRIPPPIN_SUDO_MARK").filter(|p| !p.is_empty() && p != "/dev/null") {
        let _ = std::fs::write(path, what);
    }
}

fn build(app: &gtk::Application, allowed: Rc<Cell<bool>>) {
    common::load_css();
    let window = gtk::Window::builder().application(app).decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("clear");
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-askpass"));
    window.set_layer(Layer::Overlay);
    for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(e, true);
    }
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::Exclusive);

    let shade = gtk::Box::new(gtk::Orientation::Vertical, 0);
    shade.add_css_class("backdrop");
    // The risky-action stop of the orb, as Bento draws it: one big amber tile with the facts
    // and two buttons. Nothing happens until you choose.
    let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    card.add_css_class("ask-ok");
    card.set_halign(gtk::Align::Center);
    card.set_valign(gtk::Align::Center);
    card.set_vexpand(true);
    card.set_size_request(480, -1);
    card.append(&label("WAITING FOR YOU", &["k"]));
    let command = std::env::var("TRIPPPIN_SUDO_FOR").unwrap_or_default();
    let risky = trip::permissions::is_risky(&command);
    let title = label(
        if risky { "Run this risky command as administrator?" } else { "Run this as administrator?" },
        &["ok-title"],
    );
    title.set_wrap(true);
    title.set_xalign(0.0);
    card.append(&title);
    let shown: String = command.trim().chars().take(COMMAND_CHARS).collect();
    let more = if command.trim().chars().count() > COMMAND_CHARS { "\u{2026}" } else { "" };
    if !shown.is_empty() {
        let cmd = label(&format!("{shown}{more}"), &["ok-command"]);
        cmd.set_wrap(true);
        cmd.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        cmd.set_xalign(0.0);
        card.append(&cmd);
    }
    let why = label(
        if risky {
            "This can delete data, change accounts or turn off protection. Allow it only if you asked for this."
        } else {
            "The AI needs your password for this. Settings \u{2192} AI \u{2192} Permissions can let some commands run without asking."
        },
        &["ok-body"],
    );
    why.set_wrap(true);
    why.set_xalign(0.0);
    card.append(&why);
    let facts = gtk::Grid::new();
    facts.set_column_homogeneous(true);
    facts.set_column_spacing(8);
    let first = command.split_whitespace().next().unwrap_or("command");
    let first = first.rsplit('/').next().unwrap_or(first);
    for (i, (k, v)) in [
        ("RUNS AS", "administrator".to_string()),
        ("RISK", if risky { "can't be undone" } else { "ordinary" }.to_string()),
        ("COMMAND", first.chars().take(14).collect()),
    ]
    .iter()
    .enumerate()
    {
        let fact = gtk::Box::new(gtk::Orientation::Vertical, 1);
        fact.add_css_class("ok-fact");
        fact.append(&label(k, &["k", "ok-fact-k"]));
        fact.append(&label(v, &["ok-fact-v"]));
        facts.attach(&fact, i as i32, 0, 1, 1);
    }
    card.append(&facts);
    let entry = gtk::PasswordEntry::builder().placeholder_text("Your password").build();
    entry.add_css_class("ok-field");
    entry.set_margin_top(4);
    card.append(&entry);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_homogeneous(true);
    let allow = gtk::Button::with_label("Allow");
    allow.add_css_class("ok-allow");
    let cancel = gtk::Button::with_label("Keep it blocked");
    cancel.add_css_class("ok-cancel");
    buttons.append(&allow);
    buttons.append(&cancel);
    card.append(&buttons);
    shade.append(&card);
    window.set_child(Some(&shade));

    let give = {
        let (entry, window, allowed) = (entry.clone(), window.clone(), allowed.clone());
        move || {
            let mut out = std::io::stdout();
            let _ = writeln!(out, "{}", entry.text());
            let _ = out.flush();
            allowed.set(true);
            window.close();
        }
    };
    {
        let give = give.clone();
        allow.connect_clicked(move |_| give());
    }
    entry.connect_activate(move |_| give());
    {
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
    }
    let keys = gtk::EventControllerKey::new();
    {
        let window = window.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                window.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }
    window.add_controller(keys);
    window.present();
    common::play(&card, "pop");
    common::snapshot_hook(&card);
    entry.grab_focus();
}
