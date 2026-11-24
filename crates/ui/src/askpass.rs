//! The password pop-up sudo uses when the AI runs an administrator command without a terminal
//! (`SUDO_ASKPASS`, through /usr/libexec/tripppin-askpass). It says which command wants the
//! password (`TRIPPPIN_SUDO_FOR`), prints the password for sudo on "allow", and exits with 1 on
//! "cancel" or Escape, which sudo treats as no password. It records which it was in the file
//! named by `TRIPPPIN_SUDO_MARK`, for the activity list in Settings → AI, and warns when the
//! command is a risky one (see `trip::permissions::is_risky`).

use crate::common::{self, icons, label};
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
    let card = gtk::Box::new(gtk::Orientation::Vertical, 12);
    card.add_css_class("card");
    card.add_css_class("askpass");
    card.set_halign(gtk::Align::Center);
    card.set_valign(gtk::Align::Center);
    card.set_vexpand(true);
    card.set_size_request(420, -1);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    head.append(&common::icon(icons::KEY, &["askpass-icon"]));
    let title = label("The AI wants to run this as administrator", &["asked"]);
    title.set_wrap(true);
    title.set_xalign(0.0);
    head.append(&title);
    card.append(&head);
    let command = std::env::var("TRIPPPIN_SUDO_FOR").unwrap_or_default();
    let shown: String = command.trim().chars().take(COMMAND_CHARS).collect();
    let more = if command.trim().chars().count() > COMMAND_CHARS { "…" } else { "" };
    if !shown.is_empty() {
        let cmd = label(&format!("{shown}{more}"), &["step-command"]);
        cmd.set_wrap(true);
        cmd.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        cmd.set_xalign(0.0);
        card.append(&cmd);
    }
    if trip::permissions::is_risky(&command) {
        let warn = label(
            "This can delete data, change accounts or turn off protection. Allow it only if you asked for this.",
            &["error"],
        );
        warn.set_wrap(true);
        warn.set_xalign(0.0);
        card.append(&warn);
    }
    let why = label(
        "Type your password to allow it. Settings → AI → Permissions can let some commands run without asking.",
        &["row-sub"],
    );
    why.set_wrap(true);
    why.set_xalign(0.0);
    card.append(&why);
    let entry = gtk::PasswordEntry::builder().placeholder_text("Your password").build();
    entry.add_css_class("field");
    card.append(&entry);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("link");
    let allow = gtk::Button::with_label("Allow");
    allow.add_css_class("pill");
    buttons.append(&cancel);
    buttons.append(&allow);
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
    entry.grab_focus();
}
