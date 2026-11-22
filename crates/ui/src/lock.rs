//! The lock screen (Super+L, idle, before sleep). A big clock, what finished while you were
//! away, and the password. Uses the Wayland session-lock protocol, so nothing shows through
//! and a crash keeps the screen locked rather than opening it.

use crate::common::{self, label};
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::SystemTime;

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder().application_id("in.trippp.Lock").build();
    // Asking to lock while already locked (idle and Super+L together) does nothing.
    let built = std::cell::Cell::new(false);
    app.connect_activate(move |app| {
        if !built.replace(true) {
            build(app);
        }
    });
    app.run_with_args(&[] as &[&str])
}

/// Checks the person's own password with the system's helper, without linking PAM.
fn password_ok(user: &str, password: &str) -> bool {
    let child = Command::new("/usr/sbin/unix_chkpwd")
        .args([user, "nullok"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return false };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(password.as_bytes());
        let _ = stdin.write_all(&[0]);
    }
    child.wait().is_ok_and(|s| s.success())
}

/// "finished while you were away: …" when a task ended after `since`.
fn finished_since(since: SystemTime) -> Option<String> {
    let path = common::runtime_dir().join("last-done");
    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
    if modified < since {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim().trim_end_matches('.').to_lowercase();
    (!text.is_empty()).then(|| format!("finished while you were away: {text}."))
}

fn build(app: &gtk::Application) {
    common::load_css();
    if !gtk4_session_lock::is_supported() {
        eprintln!("this compositor can't lock the screen");
        app.quit();
        return;
    }
    let lock = gtk4_session_lock::Instance::new();
    let started = SystemTime::now();
    let user = common::current_user();
    let display = gdk::Display::default().expect("a display");
    let monitors = display.monitors();
    let mut first_entry: Option<gtk::PasswordEntry> = None;
    for i in 0..monitors.n_items() {
        let Some(monitor) = monitors.item(i).and_downcast::<gdk::Monitor>() else { continue };
        let window = gtk::Window::builder().application(app).decorated(false).build();
        window.add_css_class("tripppin");
        window.add_css_class("screen");
        window.add_css_class("lock");
        let column = gtk::Box::new(gtk::Orientation::Vertical, 40);
        column.set_halign(gtk::Align::Center);
        column.set_valign(gtk::Align::Center);
        let time = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let clock = label(&common::clock(), &["clock-huge"]);
        clock.set_xalign(0.5);
        let date = label(&common::long_date(), &["date"]);
        date.set_xalign(0.5);
        time.append(&clock);
        time.append(&date);
        let notice = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        notice.add_css_class("notice");
        notice.set_halign(gtk::Align::Center);
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("ask-orb");
        dot.set_size_request(22, 22);
        let notice_text = label("", &[]);
        notice.append(&common::spark(14, false));
        notice.append(&notice_text);
        notice.set_visible(false);
        let form = gtk::Box::new(gtk::Orientation::Vertical, 12);
        form.set_halign(gtk::Align::Center);
        let who = label(&format!("{user} · locked"), &["status"]);
        who.set_xalign(0.5);
        let entry = gtk::PasswordEntry::new();
        entry.add_css_class("pw-center");
        entry.set_placeholder_text(Some("password"));
        let error = label("", &["error"]);
        error.set_xalign(0.5);
        error.set_visible(false);
        form.append(&who);
        form.append(&entry);
        form.append(&error);
        column.append(&time);
        column.append(&notice);
        column.append(&form);
        window.set_child(Some(&column));

        glib::timeout_add_seconds_local(2, move || {
            clock.set_text(&common::clock());
            date.set_text(&common::long_date());
            if let Some(text) = finished_since(started) {
                notice_text.set_text(&text);
                notice.set_visible(true);
            }
            glib::ControlFlow::Continue
        });
        {
            let (lock, app, user, error) = (lock.clone(), app.clone(), user.clone(), error.clone());
            entry.connect_activate(move |e| {
                if password_ok(&user, &e.text()) {
                    lock.unlock();
                    app.quit();
                } else {
                    e.set_text("");
                    error.set_text("that's not it. try again.");
                    error.set_visible(true);
                }
            });
        }
        lock.assign_window_to_monitor(&window, &monitor);
        if first_entry.is_none() {
            first_entry = Some(entry);
        }
    }
    {
        let app = app.clone();
        lock.connect_failed(move |_| app.quit());
    }
    lock.lock();
    if let Some(entry) = first_entry {
        entry.grab_focus();
    }
}
