//! The lock screen (Super+L, idle, before sleep). A big clock, what finished while you were
//! away, and the password. Uses the Wayland session-lock protocol, so nothing shows through
//! and a crash keeps the screen locked rather than opening it.
//!
//! Three processes, so a crash never strands anyone on niri's red "locked, but nothing to
//! show" screen:
//! - `tripppin-ui lock` starts a keeper and returns once the screen is locked (so swayidle
//!   and suspend can carry on, as with `swaylock -f`).
//! - `lock --keep` runs the lock screen and starts it again if it dies while locked: niri
//!   hands a dead client's lock to the next one that asks.
//! - `lock --surface` is the lock screen itself, one window per screen, including screens
//!   plugged in (or woken) while locked.
//!
//! `lock --release` is the way back from a TTY: it takes over a dead lock and opens it.
//! `loginctl unlock-session` opens a live one.

use crate::common::{self, label};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

/// `--surface` exit codes the keeper must not restart after.
const REFUSED: u8 = 3;
const UNSUPPORTED: u8 = 4;

pub fn run() -> glib::ExitCode {
    let has = |flag: &str| std::env::args().any(|a| a == flag);
    // `tripppin-ui lock --preview`: the same screen in an ordinary overlay (Esc closes it),
    // for checking the design; niri won't screenshot a locked session.
    if has("--preview") {
        let app = gtk::Application::builder().application_id("in.trippp.LockPreview").build();
        app.connect_activate(build_preview);
        return app.run_with_args(&[] as &[&str]);
    }
    if has("--surface") {
        return surface(false);
    }
    if has("--release") {
        return surface(true);
    }
    if has("--keep") {
        return keep();
    }
    launch()
}

/// Starts the keeper in its own process group and waits (a few seconds at most) until the
/// screen is locked.
fn launch() -> glib::ExitCode {
    use std::os::unix::process::CommandExt;
    let Ok(exe) = std::env::current_exe() else { return glib::ExitCode::FAILURE };
    let child =
        Command::new(exe).args(["lock", "--keep"]).stdin(Stdio::null()).stdout(Stdio::piped()).process_group(0).spawn();
    let Ok(mut child) = child else {
        eprintln!("couldn't start the lock screen");
        return glib::ExitCode::FAILURE;
    };
    let out = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        if let Some(out) = out {
            let _ = BufReader::new(out).read_line(&mut line);
        }
        let _ = tx.send(line);
    });
    // "locked", or nothing: already locked (the keeper handed over to the running lock
    // screen and ended), or slow. Either way, don't hold swayidle up for longer.
    let _ = rx.recv_timeout(Duration::from_secs(5));
    glib::ExitCode::SUCCESS
}

/// The compositor is still there to lock (its socket exists).
fn wayland_alive() -> bool {
    let display = std::env::var_os("WAYLAND_DISPLAY").unwrap_or_else(|| "wayland-0".into());
    let path = std::path::PathBuf::from(&display);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_default().join(path)
    };
    path.exists()
}

/// How long to wait before starting the lock screen again after it ended with `code`, or
/// None when it shouldn't come back: it was unlocked, someone else holds the lock, or the
/// session is gone. Quick crashes back off (up to five seconds) so a broken build can't spin.
pub fn restart_after(code: Option<i32>, quick_failures: u32, session_alive: bool) -> Option<Duration> {
    match code {
        Some(0) => None,
        Some(c) if c == i32::from(REFUSED) || c == i32::from(UNSUPPORTED) => None,
        _ if !session_alive => None,
        _ => Some(Duration::from_secs(u64::from(quick_failures.min(5)))),
    }
}

/// Runs the lock screen, and runs it again whenever it dies while the session is locked.
fn keep() -> glib::ExitCode {
    let Ok(exe) = std::env::current_exe() else { return glib::ExitCode::FAILURE };
    let mut quick_failures = 0;
    loop {
        let started = Instant::now();
        let child = Command::new(&exe).args(["lock", "--surface"]).stdin(Stdio::null()).stdout(Stdio::piped()).spawn();
        let Ok(mut child) = child else {
            eprintln!("couldn't start the lock screen; trying again");
            std::thread::sleep(Duration::from_secs(1));
            continue;
        };
        // Pass "locked" on to whoever started us (they may have gone: ignore errors).
        if let Some(out) = child.stdout.take() {
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    let mut stdout = std::io::stdout();
                    let _ = writeln!(stdout, "{line}");
                    let _ = stdout.flush();
                }
            });
        }
        let code = child.wait().ok().and_then(|s| s.code());
        quick_failures = if started.elapsed() < Duration::from_secs(10) { quick_failures + 1 } else { 0 };
        match restart_after(code, quick_failures, wayland_alive()) {
            None => return glib::ExitCode::SUCCESS,
            Some(wait) => {
                eprintln!("the lock screen ended ({code:?}) while locked; starting it again");
                std::thread::sleep(wait);
            }
        }
    }
}

/// The lock screen. `release`: take the lock (a dead client's, which niri hands over) and
/// open it straight away, with no windows; does nothing when the session isn't locked.
fn surface(release: bool) -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(if release { "in.trippp.LockRelease" } else { "in.trippp.Lock" })
        .build();
    let code = std::rc::Rc::new(std::cell::Cell::new(0u8));
    // Asking to lock while already locked (idle and Super+L together) reaches the running
    // lock screen, which carries on: this one exits 0, and its keeper with it.
    let built = std::cell::Cell::new(false);
    {
        let code = code.clone();
        app.connect_activate(move |app| {
            if !built.replace(true) {
                if release { build_release(app, &code) } else { build(app, &code) }
            }
        });
    }
    app.run_with_args(&[] as &[&str]);
    glib::ExitCode::from(code.get())
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

fn system_bus() -> Option<gio::DBusConnection> {
    gio::bus_get_sync(gio::BusType::System, None::<&gio::Cancellable>).ok()
}

/// This login session's object path in logind.
fn logind_session(bus: &gio::DBusConnection) -> Option<String> {
    let id = std::env::var("XDG_SESSION_ID").unwrap_or_else(|_| "auto".into());
    let reply = bus
        .call_sync(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "GetSession",
            Some(&(id,).to_variant()),
            glib::VariantTy::new("(o)").ok(),
            gio::DBusCallFlags::NONE,
            2000,
            None::<&gio::Cancellable>,
        )
        .ok()?;
    reply.child_value(0).str().map(str::to_string)
}

/// logind's word on whether the session is locked (niri keeps it up to date); None if unknown.
fn locked_hint() -> Option<bool> {
    let bus = system_bus()?;
    let path = logind_session(&bus)?;
    let reply = bus
        .call_sync(
            Some("org.freedesktop.login1"),
            &path,
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&("org.freedesktop.login1.Session", "LockedHint").to_variant()),
            glib::VariantTy::new("(v)").ok(),
            gio::DBusCallFlags::NONE,
            2000,
            None::<&gio::Cancellable>,
        )
        .ok()?;
    reply.child_value(0).as_variant()?.get::<bool>()
}

/// Calls `f` when someone runs `loginctl unlock-session` for this session. Listens for as
/// long as the subscription is kept.
fn on_logind_unlock(f: impl Fn() + 'static) -> Option<gio::SignalSubscription> {
    let bus = system_bus()?;
    let path = logind_session(&bus)?;
    Some(bus.subscribe_to_signal(
        Some("org.freedesktop.login1"),
        Some("org.freedesktop.login1.Session"),
        Some("Unlock"),
        Some(&path),
        None,
        gio::DBusSignalFlags::NONE,
        move |_| f(),
    ))
}

/// `lock --release`: takes over a lock whose screen has died, and opens it.
fn build_release(app: &gtk::Application, code: &std::rc::Rc<std::cell::Cell<u8>>) {
    if locked_hint() == Some(false) || !gtk4_session_lock::is_supported() {
        eprintln!("not locked");
        return;
    }
    let hold = std::cell::RefCell::new(Some(app.hold()));
    let lock = gtk4_session_lock::Instance::new();
    {
        let (app, code) = (app.clone(), code.clone());
        lock.connect_failed(move |_| {
            eprintln!("the lock screen is still running: use loginctl unlock-session instead");
            code.set(REFUSED);
            app.quit();
        });
    }
    // Opened once we hold it (from the main loop, not inside the protocol's own callback).
    lock.connect_locked(|lock| {
        let lock = lock.clone();
        glib::idle_add_local_once(move || lock.unlock());
    });
    {
        let app = app.clone();
        lock.connect_unlocked(move |_| {
            hold.borrow_mut().take();
            app.quit();
        });
    }
    lock.lock();
    // The instance lives as long as the app does.
    app.connect_shutdown(move |_| {
        let _ = &lock;
    });
}

fn build(app: &gtk::Application, code: &std::rc::Rc<std::cell::Cell<u8>>) {
    common::load_css();
    if !gtk4_session_lock::is_supported() {
        eprintln!("this compositor can't lock the screen");
        code.set(UNSUPPORTED);
        return;
    }
    // Windows come and go with screens; the app stays until it's unlocked, even with none.
    let hold = std::rc::Rc::new(std::cell::RefCell::new(Some(app.hold())));
    let lock = gtk4_session_lock::Instance::new();
    let started = SystemTime::now();
    let user = common::current_user();
    {
        let (app, code, hold) = (app.clone(), code.clone(), hold.clone());
        lock.connect_failed(move |_| {
            eprintln!("couldn't lock: another lock screen is running");
            code.set(REFUSED);
            hold.borrow_mut().take();
            app.quit();
        });
    }
    lock.connect_locked(|_| {
        let mut out = std::io::stdout();
        let _ = writeln!(out, "locked");
        let _ = out.flush();
    });
    {
        let app = app.clone();
        lock.connect_unlocked(move |_| {
            hold.borrow_mut().take();
            app.quit();
        });
    }
    // One window per screen: those there now (emitted from within lock()) and any added
    // later, such as a screen that comes back after sleep. Windows must be assigned after
    // lock(): one assigned before is shown as an ordinary window and never becomes the lock
    // surface, which leaves niri showing red with nowhere to type.
    {
        let (app, user) = (app.clone(), user.clone());
        lock.connect_monitor(move |lock, monitor| {
            let (window, entry) = lock_window(&app, lock, &user, started);
            lock.assign_window_to_monitor(&window, monitor);
            entry.grab_focus();
        });
    }
    if lock.lock() {
        // `loginctl unlock-session` (from a TTY, say) opens it too.
        let listening = {
            let lock = lock.clone();
            on_logind_unlock(move || lock.unlock())
        };
        app.connect_shutdown(move |_| {
            let _ = &listening;
        });
    }
    // The instance lives as long as the app does: dropping it mid-lock crashes in its signals.
    app.connect_shutdown(move |_| {
        let _ = &lock;
    });
}

/// One screen's lock window (not yet shown) and its password entry.
fn lock_window(
    app: &gtk::Application,
    lock: &gtk4_session_lock::Instance,
    user: &str,
    started: SystemTime,
) -> (gtk::Window, gtk::PasswordEntry) {
    let window = gtk::Window::builder().application(app).decorated(false).build();
    // Not "background": the person's gtk.css rounds `window.background` corners, which on a
    // lock surface would show niri's red backdrop in each corner.
    window.remove_css_class("background");
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

    // The timers stop when the window goes (its screen unplugged, or unlocked).
    let alive = window.downgrade();
    glib::timeout_add_local(Duration::from_millis(300), move || {
        if alive.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        caps.set_visible(crate::osd::caps_lock() == Some(true));
        glib::ControlFlow::Continue
    });
    let alive = window.downgrade();
    glib::timeout_add_seconds_local(2, move || {
        if alive.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
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
        let (lock, user) = (lock.clone(), user.to_string());
        entry.connect_activate(move |e| {
            if password_ok(&user, &e.text()) {
                // Ends the lock; ::unlocked quits.
                lock.unlock();
            } else {
                e.set_text("");
                error.set_text("that's not it. try again.");
                error.set_visible(true);
            }
        });
    }
    (window, entry)
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
    if let Some(c) = window.child() {
        crate::common::play(&c, "fade");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeper_restarts_only_a_crashed_lock() {
        // Unlocked, or another lock screen holds it: done.
        assert_eq!(restart_after(Some(0), 0, true), None);
        assert_eq!(restart_after(Some(i32::from(REFUSED)), 0, true), None);
        assert_eq!(restart_after(Some(i32::from(UNSUPPORTED)), 0, true), None);
        // Killed (no code) or crashed: straight back, then backing off to five seconds.
        assert_eq!(restart_after(None, 0, true), Some(Duration::ZERO));
        assert_eq!(restart_after(Some(101), 2, true), Some(Duration::from_secs(2)));
        assert_eq!(restart_after(None, 40, true), Some(Duration::from_secs(5)));
        // The session has gone (logged out): nothing to lock.
        assert_eq!(restart_after(None, 0, false), None);
    }

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
