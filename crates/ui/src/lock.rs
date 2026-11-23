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
    // `tripppin-ui lock --preview`: the same screen in an ordinary overlay (Esc closes it),
    // for checking the design; niri won't screenshot a locked session.
    let preview = std::env::args().any(|a| a == "--preview");
    let app = gtk::Application::builder()
        .application_id(if preview { "in.trippp.LockPreview" } else { "in.trippp.Lock" })
        .build();
    // Asking to lock while already locked (idle and Super+L together) does nothing.
    let built = std::cell::Cell::new(false);
    app.connect_activate(move |app| {
        if !built.replace(true) {
            if preview { build_preview(app) } else { build(app) }
        }
    });
    app.run_with_args(&[] as &[&str])
}

/// What's playing, from any MPRIS player: (playing, title, artist).
/// Files without a title (a plain .wav) go by their name.
pub fn parse_playing(out: &str) -> Option<(bool, String, String)> {
    let mut f = out.trim_end_matches('\n').splitn(4, '\t');
    let status = f.next()?;
    let mut title = f.next().unwrap_or("").trim().to_string();
    let artist = f.next().unwrap_or("").trim().to_string();
    if title.is_empty() {
        let url = f.next().unwrap_or("").trim();
        let file = url.rsplit('/').next().unwrap_or("").replace("%20", " ");
        title = file.rsplit_once('.').map(|(stem, _)| stem.to_string()).unwrap_or(file).replace(['-', '_'], " ");
    }
    (!title.trim().is_empty()).then(|| (status == "Playing", title.trim().to_string(), artist))
}

fn playing() -> Option<(bool, String, String)> {
    let out = Command::new("playerctl")
        .args(["metadata", "--format", "{{status}}\t{{title}}\t{{artist}}\t{{xesam:url}}"])
        .output()
        .ok()?;
    out.status.success().then(|| parse_playing(&String::from_utf8_lossy(&out.stdout))).flatten()
}

fn notification_count() -> u32 {
    Command::new("swaync-client")
        .arg("-c")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

#[derive(Clone, Copy)]
enum Shape {
    Previous,
    Play,
    Pause,
    Next,
}

/// The media symbols, drawn so they never depend on an icon theme.
fn media_icon(shape: Shape) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(16);
    area.set_content_height(16);
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (f64::from(w), f64::from(h));
        cr.set_source_rgb(0.85, 0.88, 0.87);
        let tri = |cr: &gtk::cairo::Context, x0: f64, x1: f64| {
            cr.move_to(x0, h * 0.15);
            cr.line_to(x1, h / 2.0);
            cr.line_to(x0, h * 0.85);
            cr.close_path();
        };
        match shape {
            Shape::Play => tri(cr, w * 0.25, w * 0.85),
            Shape::Pause => {
                cr.rectangle(w * 0.22, h * 0.15, w * 0.2, h * 0.7);
                cr.rectangle(w * 0.58, h * 0.15, w * 0.2, h * 0.7);
            }
            Shape::Next => {
                tri(cr, w * 0.15, w * 0.7);
                cr.rectangle(w * 0.72, h * 0.15, w * 0.13, h * 0.7);
            }
            Shape::Previous => {
                cr.move_to(w * 0.85, h * 0.15);
                cr.line_to(w * 0.3, h / 2.0);
                cr.line_to(w * 0.85, h * 0.85);
                cr.close_path();
                cr.rectangle(w * 0.15, h * 0.15, w * 0.13, h * 0.7);
            }
        }
        let _ = cr.fill();
    });
    area
}

/// The card for what's playing, with back, play/pause and next.
fn media_card() -> (gtk::Box, Box<dyn Fn()>) {
    let card = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    card.add_css_class("media-card");
    card.set_halign(gtk::Align::Center);
    let words = gtk::Box::new(gtk::Orientation::Vertical, 2);
    words.set_hexpand(true);
    words.set_valign(gtk::Align::Center);
    let title = label("", &["media-title"]);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(28);
    let artist = label("", &["status"]);
    artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
    artist.set_max_width_chars(28);
    words.append(&title);
    words.append(&artist);
    card.append(&words);
    let button = |shape: Shape, verb: &'static str| {
        let b = gtk::Button::new();
        b.set_child(Some(&media_icon(shape)));
        b.add_css_class("media-button");
        b.set_valign(gtk::Align::Center);
        b.connect_clicked(move |_| {
            let _ = Command::new("playerctl").arg(verb).status();
        });
        b
    };
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    controls.set_valign(gtk::Align::Center);
    controls.append(&button(Shape::Previous, "previous"));
    let play = button(Shape::Pause, "play-pause");
    controls.append(&play);
    controls.append(&button(Shape::Next, "next"));
    card.append(&controls);
    card.set_visible(false);
    let update = {
        let card = card.clone();
        move || match playing() {
            Some((on, t, a)) => {
                title.set_text(&t.to_lowercase());
                artist.set_text(&a.to_lowercase());
                artist.set_visible(!a.is_empty());
                play.set_child(Some(&media_icon(if on { Shape::Pause } else { Shape::Play })));
                card.set_visible(true);
            }
            None => card.set_visible(false),
        }
    };
    update();
    (card, Box::new(update))
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
        notice.append(&common::logo(13));
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
        let caps = label("caps lock is on", &["caps-warning"]);
        caps.set_xalign(0.5);
        caps.set_visible(crate::osd::caps_lock() == Some(true));
        form.append(&who);
        form.append(&entry);
        form.append(&caps);
        form.append(&error);
        let (media, update_media) = media_card();
        column.append(&time);
        column.append(&notice);
        column.append(&media);
        column.append(&form);
        let (footer, update_footer) = footer();
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&column));
        overlay.add_overlay(&footer);
        window.set_child(Some(&overlay));

        {
            let caps = caps.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
                caps.set_visible(crate::osd::caps_lock() == Some(true));
                glib::ControlFlow::Continue
            });
        }
        glib::timeout_add_seconds_local(2, move || {
            clock.set_text(&common::clock());
            date.set_text(&common::long_date());
            if let Some(text) = finished_since(started) {
                notice_text.set_text(&text);
                notice.set_visible(true);
            }
            update_media();
            update_footer();
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

/// Bottom right: the battery and how many notifications came (never what they say).
fn footer() -> (gtk::Box, Box<dyn Fn()>) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 22);
    row.set_halign(gtk::Align::End);
    row.set_valign(gtk::Align::End);
    row.set_margin_end(48);
    row.set_margin_bottom(40);
    let notes = label("", &["status"]);
    let battery = label("", &["status"]);
    row.append(&notes);
    row.append(&battery);
    let update = {
        let (notes, battery) = (notes.clone(), battery.clone());
        move || {
            let n = notification_count();
            notes.set_text(&match n {
                0 => String::new(),
                1 => "1 notification".to_string(),
                n => format!("{n} notifications"),
            });
            notes.set_visible(n > 0);
            match common::battery() {
                Some(b) => {
                    battery.set_text(&format!("battery {b}%"));
                    battery.set_visible(true);
                }
                None => battery.set_visible(false),
            }
        }
    };
    update();
    (row, Box::new(update))
}

/// The lock screen's content in a plain overlay window, to look at.
fn build_preview(app: &gtk::Application) {
    use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
    common::load_css();
    let window = gtk::Window::builder().application(app).decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("screen");
    window.add_css_class("lock");
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(e, true);
    }
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
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
    let (media, _update) = media_card();
    let form = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let who = label(&format!("{} · locked", common::current_user()), &["status"]);
    who.set_xalign(0.5);
    let entry = gtk::PasswordEntry::new();
    entry.add_css_class("pw-center");
    entry.set_placeholder_text(Some("password"));
    let caps = label("caps lock is on", &["caps-warning"]);
    caps.set_xalign(0.5);
    caps.set_visible(crate::osd::caps_lock() == Some(true));
    form.append(&who);
    form.append(&entry);
    form.append(&caps);
    column.append(&time);
    column.append(&media);
    column.append(&form);
    let (footer, _f) = footer();
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&column));
    overlay.add_overlay(&footer);
    window.set_child(Some(&overlay));
    let keys = gtk::EventControllerKey::new();
    {
        let app = app.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                app.quit();
            }
            glib::Propagation::Proceed
        });
    }
    window.add_controller(keys);
    window.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_playerctl() {
        assert_eq!(
            parse_playing("Playing\tTum Hi Ho\tArijit Singh\n"),
            Some((true, "Tum Hi Ho".into(), "Arijit Singh".into()))
        );
        assert_eq!(parse_playing("Paused\tA podcast\t"), Some((false, "A podcast".into(), String::new())));
        assert_eq!(parse_playing("Stopped\t\t"), None);
        assert_eq!(
            parse_playing("Playing\t\t\tfile:///home/a/Music/evening-raga.wav\n"),
            Some((true, "evening raga".into(), String::new()))
        );
    }
}
