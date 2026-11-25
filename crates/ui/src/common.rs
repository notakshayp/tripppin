//! Pieces every screen shares: the stylesheet, the logo, and small facts about the machine.

use gtk::prelude::*;
use gtk::{cairo, gdk, gio, glib};
use std::path::PathBuf;
use trip::logo;

pub const INK: (f64, f64, f64) = (0.949, 0.961, 0.937);
pub const GREEN: (f64, f64, f64) = (0.341, 0.851, 0.604);

thread_local! {
    /// The corner radii on top of the stylesheet, and the watch on the file that picks them.
    static CORNERS: std::cell::RefCell<Option<(gtk::CssProvider, gio::FileMonitor)>> = const { std::cell::RefCell::new(None) };
}

/// Loads the Tripppin stylesheet for the whole app, with the corners chosen in Settings →
/// Appearance; when that choice changes, every running screen restyles itself.
pub fn load_css() {
    let Some(display) = gdk::Display::default() else { return };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    CORNERS.with(|slot| {
        if slot.borrow().is_some() {
            return;
        }
        let corners = gtk::CssProvider::new();
        corners.load_from_string(&crate::look::css(crate::look::current()));
        gtk::style_context_add_provider_for_display(&display, &corners, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
        // The folder must exist for the watch to see the file appear.
        if let Some(dir) = crate::look::file().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(monitor) =
            gio::File::for_path(crate::look::file()).monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        else {
            return;
        };
        let watched = corners.clone();
        monitor.connect_changed(move |_, _, _, _| watched.load_from_string(&crate::look::css(crate::look::current())));
        *slot.borrow_mut() = Some((corners, monitor));
    });
}

/// Development aid: with `TRIPPPIN_SHOT=/path/name` set, saves a PNG of `w` a moment after
/// it is shown (name.png), so screens can be checked on a headless display.
pub fn snapshot_hook(w: &impl IsA<gtk::Widget>) {
    let Ok(path) = std::env::var("TRIPPPIN_SHOT") else { return };
    let w = w.clone().upcast::<gtk::Widget>();
    glib::timeout_add_local_once(std::time::Duration::from_millis(2500), move || {
        let (width, height) = (w.width(), w.height());
        let Some(renderer) = w.native().and_then(|n| n.renderer()) else { return };
        let paintable = gtk::WidgetPaintable::new(Some(&w));
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
        if let Some(node) = snapshot.to_node() {
            let texture = renderer.render_texture(&node, None);
            let _ = texture.save_to_png(format!("{path}.png"));
        }
    });
}

/// A label with the given CSS classes.
pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

type Rgba = (f64, f64, f64, f64);

/// Draws the power-spark mark into a box whose width is `w`, as it stands at `pose`.
/// Every drawing of the mark on the desktop goes through here; the geometry (and the intro's
/// timing) is `trip::logo`'s, the same as the terminal's and the brand files'.
pub fn draw_mark_posed(cr: &cairo::Context, x: f64, y: f64, w: f64, ring: Rgba, spark: Rgba, pose: logo::Pose) {
    let (bx, by, bw, _) = logo::BOX;
    let s = w / bw;
    cr.save().ok();
    cr.translate(x, y);
    cr.scale(s, s);
    cr.translate(-bx, -by);
    // The ring, drawn from its left end down and round (cairo's angles run clockwise from +x).
    let (cx, cy, r, width) = logo::RING;
    if pose.ring > 0.0 {
        let left = (-90.0 - logo::GAP_DEG).to_radians();
        cr.set_source_rgba(ring.0, ring.1, ring.2, ring.3);
        cr.set_line_width(width);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.arc_negative(cx, cy, r, left, left - (logo::SPAN * pose.ring.min(1.0)).to_radians());
        cr.stroke().ok();
    }
    // The spark: an × in the ring's opening.
    if pose.spark_scale > 0.0 {
        let pts = logo::spark_points(pose.spark_scale, pose.spark_turn);
        cr.move_to(pts[0].0, pts[0].1);
        for p in &pts[1..] {
            cr.line_to(p.0, p.1);
        }
        cr.close_path();
        cr.set_source_rgba(spark.0, spark.1, spark.2, spark.3);
        cr.fill().ok();
    }
    cr.restore().ok();
}

/// Draws the finished mark into a box whose width is `w` (the mark is 146 x 175 units).
pub fn draw_mark(cr: &cairo::Context, x: f64, y: f64, w: f64, ring: Rgba, spark: Rgba) {
    draw_mark_posed(cr, x, y, w, ring, spark, logo::Pose::FULL);
}

/// The mark's height for a given width.
pub fn mark_height(width: f64) -> f64 {
    width * logo::BOX.3 / logo::BOX.2
}

/// The full mark, paper ring and green spark, `width` px wide (and 175/146 as tall).
pub fn logo(width: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(width);
    area.set_content_height(mark_height(f64::from(width)).round() as i32);
    area.set_can_target(false);
    // Always the asked-for size, centred, however much room the parent gives it.
    area.set_halign(gtk::Align::Center);
    area.set_valign(gtk::Align::Center);
    let size = f64::from(width);
    area.set_draw_func(move |_, cr, w, h| {
        let (x, y) = ((f64::from(w) - size) / 2.0, (f64::from(h) - mark_height(size)) / 2.0);
        draw_mark(cr, x, y, size, (INK.0, INK.1, INK.2, 1.0), (GREEN.0, GREEN.1, GREEN.2, 1.0));
    });
    area
}

/// The mark playing its intro (the ring draws itself, the × spins in and settles), then,
/// if `waiting`, making a gentle quarter turn every couple of seconds. It starts when first
/// shown. With reduce motion on, it's the finished mark, still.
pub fn animated_logo(width: i32, waiting: bool) -> gtk::DrawingArea {
    let area = logo(width);
    if crate::display::reduce_motion() {
        return area;
    }
    let size = f64::from(width);
    let start: std::rc::Rc<std::cell::Cell<Option<i64>>> = Default::default();
    {
        let start = start.clone();
        area.set_draw_func(move |a, cr, w, h| {
            let now = a.frame_clock().map_or(0, |c| c.frame_time());
            let t = (now - start.get().unwrap_or(now)) as f64 / 1_000_000.0;
            // The spark never turns; once in place the mark simply rests.
            let pose = logo::intro_pose(t);
            let _ = waiting;
            let (x, y) = ((f64::from(w) - size) / 2.0, (f64::from(h) - mark_height(size)) / 2.0);
            draw_mark_posed(cr, x, y, size, (INK.0, INK.1, INK.2, 1.0), (GREEN.0, GREEN.1, GREEN.2, 1.0), pose);
        });
    }
    area.add_tick_callback(move |a, clock| {
        let now = clock.frame_time();
        let began = start.get().unwrap_or(now);
        start.set(Some(began));
        let t = (now - began) as f64 / 1_000_000.0;
        a.queue_draw();
        if waiting || t < logo::INTRO_SECS + 0.1 { glib::ControlFlow::Continue } else { glib::ControlFlow::Break }
    });
    area
}

/// The desktop's backdrop: the huge, faint mark off the top right, as on the wallpaper.
pub fn backdrop() -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_can_target(false);
    area.set_draw_func(|_, cr, w, h| {
        // Scaled from the 1280 x 800 mockup: a 900 px mark at right -260, top -150.
        let k = (f64::from(w) / 1280.0).max(f64::from(h) / 800.0);
        let mark_w = 900.0 * k;
        let x = f64::from(w) + 260.0 * k - mark_w;
        let y = -150.0 * k;
        draw_mark(cr, x, y, mark_w, (0.086, 0.125, 0.114, 0.55), (0.086, 0.227, 0.165, 0.55));
    });
    area
}

/// An icon-sized arrow (right) or caret (up), drawn so it never depends on an icon theme.
pub fn arrow(up: bool) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(20);
    area.set_content_height(20);
    area.set_can_target(false);
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (f64::from(w), f64::from(h));
        cr.set_source_rgb(0.051, 0.071, 0.067);
        cr.set_line_width(2.5);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_line_join(cairo::LineJoin::Round);
        if up {
            cr.move_to(w / 2.0, h * 0.82);
            cr.line_to(w / 2.0, h * 0.2);
            cr.move_to(w * 0.25, h * 0.45);
            cr.line_to(w / 2.0, h * 0.2);
            cr.line_to(w * 0.75, h * 0.45);
        } else {
            cr.move_to(w * 0.2, h / 2.0);
            cr.line_to(w * 0.8, h / 2.0);
            cr.move_to(w * 0.55, h * 0.25);
            cr.line_to(w * 0.8, h / 2.0);
            cr.line_to(w * 0.55, h * 0.75);
        }
        cr.stroke().ok();
    });
    area
}

/// Icons are Lucide, drawn from its font (installed by os/Containerfile), so they take the
/// text colour and any size. Code points from lucide-static's font/lucide.css.
pub mod icons {
    pub const WIFI: char = '\u{e1ae}';
    pub const NETWORK: char = '\u{e125}';
    pub const BLUETOOTH: char = '\u{e05c}';
    pub const SOUND: char = '\u{e1ab}';
    pub const MIC: char = '\u{e118}';
    pub const MUTED: char = '\u{e1ac}';
    pub const MIC_OFF: char = '\u{e119}';
    pub const SUN: char = '\u{e178}';
    pub const KEYBOARD: char = '\u{e284}';
    pub const DISPLAY: char = '\u{e11d}';
    pub const PALETTE: char = '\u{e1dd}';
    pub const BATTERY: char = '\u{e055}';
    pub const SPARKLES: char = '\u{e412}';
    pub const BELL: char = '\u{e059}';
    pub const APPS: char = '\u{e0ff}';
    pub const POINTER: char = '\u{e1c3}';
    pub const PRINTER: char = '\u{e141}';
    pub const CLOCK: char = '\u{e087}';
    pub const LANGUAGE: char = '\u{e0fe}';
    pub const USERS: char = '\u{e1a4}';
    pub const SHIELD: char = '\u{e1ff}';
    pub const ACCESSIBILITY: char = '\u{e297}';
    pub const DISK: char = '\u{e0ed}';
    pub const UPDATE: char = '\u{e145}';
    pub const INFO: char = '\u{e0f9}';
    pub const CHECK: char = '\u{e06c}';
    pub const ALERT: char = '\u{e077}';
    pub const MINIMIZE: char = '\u{e11b}';
    pub const CAPS: char = '\u{e04a}';
    pub const HISTORY: char = '\u{e1f5}';
    pub const CHEVRON_DOWN: char = '\u{e06d}';
    pub const CHEVRON_LEFT: char = '\u{e06e}';
    pub const CHEVRON_RIGHT: char = '\u{e06f}';
    pub const TERMINAL: char = '\u{e181}';
    pub const NEW: char = '\u{e172}';
    pub const KEY: char = '\u{e4a3}';
    pub const EXTERNAL: char = '\u{e0b9}';
    pub const CLOUD: char = '\u{e088}';
    pub const CPU: char = '\u{e0a9}';
}

/// An icon (see `icons`) as a label with the given CSS classes.
pub fn icon(ch: char, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(&ch.to_string()));
    l.add_css_class("icon");
    for c in classes {
        l.add_css_class(c);
    }
    l
}

/// Plays one of the stylesheet's entrance animations (`pop`, `rise`, `drop`) on `w` again.
/// GTK starts a CSS animation when the animation's name changes, so each comes in two
/// identical copies, -a and -b, and this swaps between them.
pub fn play(w: &impl IsA<gtk::Widget>, name: &str) {
    let (a, b) = (format!("{name}-a"), format!("{name}-b"));
    let (from, to) = if w.has_css_class(&a) { (a, b) } else { (b, a) };
    w.remove_css_class(&from);
    w.add_css_class(&to);
}

/// Spring-like easing for motion drawn in code: fast out, settling with no bounce, the same
/// feel as the stylesheet's cubic-bezier(0.32, 0.72, 0, 1).
pub fn ease_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(4)
}

/// The working spinner: a green arc that sweeps round, lengthening and shortening as it goes,
/// drawn every frame by the frame clock so it stays smooth. It only runs while shown.
pub fn spinner(size: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size);
    area.set_can_target(false);
    area.add_css_class("spinner");
    area.set_draw_func(|a, cr, w, h| {
        let t = a.frame_clock().map_or(0.0, |c| c.frame_time() as f64 / 1_000_000.0);
        let (w, h) = (f64::from(w), f64::from(h));
        let r = w.min(h) / 2.0 - 2.0;
        let (cx, cy) = (w / 2.0, h / 2.0);
        cr.set_line_width((w / 8.0).max(2.0));
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_source_rgba(GREEN.0, GREEN.1, GREEN.2, 0.18);
        cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
        cr.stroke().ok();
        // One turn every 0.9 s; the arc breathes between a fifth and two thirds of the ring.
        let turn = (t / 0.9).fract() * std::f64::consts::TAU;
        let breathe = 0.5 - 0.5 * (t * std::f64::consts::TAU / 1.6).cos();
        let length = (0.2 + 0.47 * breathe) * std::f64::consts::TAU;
        cr.set_source_rgb(GREEN.0, GREEN.1, GREEN.2);
        cr.arc(cx, cy, r, turn - std::f64::consts::FRAC_PI_2, turn - std::f64::consts::FRAC_PI_2 + length);
        cr.stroke().ok();
    });
    area.add_tick_callback(|a, _| {
        a.queue_draw();
        glib::ControlFlow::Continue
    });
    area
}

/// A small green dot that breathes while something is under way: quieter than the spinner,
/// for a status line. Holds still when animations are off.
pub fn pulse_dot(size: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size);
    area.set_can_target(false);
    area.add_css_class("pulse-dot");
    area.set_draw_func(|a, cr, w, h| {
        let animate = gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations());
        let t = if animate { a.frame_clock().map_or(0.0, |c| c.frame_time() as f64 / 1_000_000.0) } else { 0.0 };
        // One breath every 1.4 s: the dot swells a little and brightens, then settles.
        let k = 0.5 - 0.5 * (t * std::f64::consts::TAU / 1.4).cos();
        let (w, h) = (f64::from(w), f64::from(h));
        let full = w.min(h) / 2.0;
        cr.set_source_rgba(GREEN.0, GREEN.1, GREEN.2, 0.45 + 0.55 * k);
        cr.arc(w / 2.0, h / 2.0, full * (0.6 + 0.4 * k), 0.0, std::f64::consts::TAU);
        cr.fill().ok();
    });
    area.add_tick_callback(|a, _| {
        a.queue_draw();
        glib::ControlFlow::Continue
    });
    area
}

/// The monitor plugged into connector `name` (`eDP-1`, `HDMI-A-1`), as niri names outputs.
pub fn monitor_named(name: &str) -> Option<gdk::Monitor> {
    let monitors = gdk::Display::default()?.monitors();
    (0..monitors.n_items())
        .filter_map(|i| monitors.item(i).and_downcast::<gdk::Monitor>())
        .find(|m| m.connector().as_deref() == Some(name))
}

pub fn clock() -> String {
    let format = if crate::region::twelve_hour() { "%-I:%M %p" } else { "%H:%M" };
    glib::DateTime::now_local().ok().and_then(|t| t.format(format).ok()).map(|s| s.to_string()).unwrap_or_default()
}

/// "Sunday, 22 November"
pub fn long_date() -> String {
    glib::DateTime::now_local()
        .ok()
        .and_then(|t| t.format("%A, %-d %B").ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// The connected Wi-Fi network's name, if any.
pub fn wifi_name() -> Option<String> {
    let out = std::process::Command::new("nmcli").args(["-t", "-f", "ACTIVE,SSID", "dev", "wifi"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("yes:").map(str::to_string))
        .filter(|s| !s.is_empty())
}

/// The battery's charge in percent, if there's a battery.
pub fn battery() -> Option<u8> {
    crate::battery::read(std::path::Path::new("/sys/class/power_supply")).map(|c| c.percent)
}

/// Where the desktop's small shared state lives (the orb's status, what finished last).
pub fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join("tripppin");
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// A person's display name: the first part of their GECOS field, else their user name.
pub fn display_name(user: &str) -> String {
    std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|p| {
            p.lines().find_map(|l| {
                let f: Vec<&str> = l.split(':').collect();
                (f.first() == Some(&user)).then(|| f.get(4).and_then(|g| g.split(',').next()).unwrap_or("").to_string())
            })
        })
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| user.to_string())
}

/// The people who can log in: accounts with a uid from 1000 and a real shell.
pub fn people() -> Vec<String> {
    std::fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            let uid: u32 = f.get(2)?.parse().ok()?;
            let shell = f.get(6)?;
            ((1000..60000).contains(&uid) && !shell.ends_with("nologin") && !shell.ends_with("false"))
                .then(|| f[0].to_string())
        })
        .collect()
}

pub fn current_user() -> String {
    std::env::var("USER").unwrap_or_else(|_| "you".into())
}

/// The shortcuts niri's config binds, grouped, for the cheat sheet (Super + /) and
/// Settings → Keyboard. Keep in step with os/files/etc/niri/desktop.kdl.
pub const SHORTCUTS: [(&str, &[(&str, &str)]); 3] = [
    (
        "Tripppin",
        &[
            ("Ask Tripppin", "Super + Space"),
            ("terminal", "Super + Return"),
            ("apps", "Super + A, or type its name after Super + Space"),
            ("Quick settings", "Super + N"),
            ("Clipboard history", "Super + V"),
            ("Dictate: speak, press again to type", "Super + M"),
            ("emoji", "Super + ."),
            ("These shortcuts", "Super + /"),
        ],
    ),
    (
        "Windows",
        &[
            ("Close a window", "Super + Q"),
            ("Move a window", "Super + drag"),
            ("Resize a window", "Super + right-drag"),
            ("Full screen", "Super + F"),
            ("Float or tile a window", "Super + T"),
            ("Switch windows", "Alt + Tab"),
            ("See every window", "Super + Tab"),
            ("Snap left or right", "Super + ← →"),
            ("Fill the screen", "Super + ↑ (again to undo)"),
            ("Centre a window", "Super + ↓"),
            ("Snap to a corner", "Super + Alt + arrows"),
            ("Snap by dragging", "Drag to an edge or a corner"),
            ("Halves (again for ⅓, ⅔)", "Ctrl + Alt + arrows"),
            ("Quarters", "Ctrl + Alt + U, I, J, K"),
            ("Fill or centre", "Ctrl + Alt + Return, C"),
            ("Back to before snapping", "Ctrl + Alt + Backspace"),
            ("Workspace 1, 2, 3", "Super + 1, 2, 3"),
            ("Move a window to 1, 2, 3", "Super + Shift + 1, 2, 3"),
        ],
    ),
    (
        "System",
        &[
            ("lock", "Super + L"),
            ("Power menu", "Super + Escape"),
            ("Screenshot of an area", "print"),
            ("Screenshot of the screen", "Ctrl + Print"),
            ("Screenshot of a window", "Alt + Print"),
            ("Draw on the last screenshot", "Super + Shift + S"),
            ("Record the screen", "Super + Shift + R"),
        ],
    ),
];
