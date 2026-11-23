//! Pieces every screen shares: the stylesheet, the logo, and small facts about the machine.

use gtk::prelude::*;
use gtk::{cairo, gdk, glib};
use std::path::PathBuf;

pub const INK: (f64, f64, f64) = (0.949, 0.961, 0.937);
pub const GREEN: (f64, f64, f64) = (0.341, 0.851, 0.604);

/// Loads the Tripppin stylesheet for the whole app.
pub fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
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

/// Draws the power-spark mark into a box whose width is `w` (the mark is 146 x 175 units).
fn draw_mark(cr: &cairo::Context, x: f64, y: f64, w: f64, ring: (f64, f64, f64, f64), spark: (f64, f64, f64, f64)) {
    let s = w / 146.0;
    cr.save().ok();
    cr.translate(x, y);
    cr.scale(s, s);
    cr.translate(-27.0, -12.0);
    // The ring: centre (100, 114), radius 60, open +-44 degrees around the top.
    let start = (-90.0f64 + 44.0).to_radians();
    let end = (-90.0f64 - 44.0 + 360.0).to_radians();
    cr.set_source_rgba(ring.0, ring.1, ring.2, ring.3);
    cr.set_line_width(22.0);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.arc(100.0, 114.0, 60.0, start, end);
    cr.stroke().ok();
    // The spark: a four-point star at (100, 50).
    let pts = [
        (100.00, 14.00),
        (106.11, 43.89),
        (136.00, 50.00),
        (106.11, 56.11),
        (100.00, 86.00),
        (93.89, 56.11),
        (64.00, 50.00),
        (93.89, 43.89),
    ];
    cr.move_to(pts[0].0, pts[0].1);
    for p in &pts[1..] {
        cr.line_to(p.0, p.1);
    }
    cr.close_path();
    cr.set_source_rgba(spark.0, spark.1, spark.2, spark.3);
    cr.fill().ok();
    cr.restore().ok();
}

/// The full mark, paper ring and green spark, `width` px wide (and 175/146 as tall).
pub fn logo(width: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(width);
    area.set_content_height((f64::from(width) * 175.0 / 146.0).round() as i32);
    area.set_can_target(false);
    // Always the asked-for size, centred, however much room the parent gives it.
    area.set_halign(gtk::Align::Center);
    area.set_valign(gtk::Align::Center);
    let size = f64::from(width);
    area.set_draw_func(move |_, cr, w, h| {
        let height = size * 175.0 / 146.0;
        let (x, y) = ((f64::from(w) - size) / 2.0, (f64::from(h) - height) / 2.0);
        draw_mark(cr, x, y, size, (INK.0, INK.1, INK.2, 1.0), (GREEN.0, GREEN.1, GREEN.2, 1.0));
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

pub fn clock() -> String {
    let format = if crate::region::twelve_hour() { "%-I:%M %p" } else { "%H:%M" };
    glib::DateTime::now_local().ok().and_then(|t| t.format(format).ok()).map(|s| s.to_lowercase()).unwrap_or_default()
}

/// "sunday, 22 november"
pub fn long_date() -> String {
    glib::DateTime::now_local()
        .ok()
        .and_then(|t| t.format("%A, %-d %B").ok())
        .map(|s| s.to_lowercase())
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
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in dir.flatten() {
        let path = entry.path();
        if std::fs::read_to_string(path.join("type")).is_ok_and(|t| t.trim() == "Battery") {
            return std::fs::read_to_string(path.join("capacity")).ok()?.trim().parse().ok();
        }
    }
    None
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
        "tripppin",
        &[
            ("ask tripppin", "super + space"),
            ("terminal", "super + return"),
            ("apps", "super + d"),
            ("quick settings", "super + n"),
            ("clipboard history", "super + v"),
            ("emoji", "super + ."),
            ("these shortcuts", "super + /"),
        ],
    ),
    (
        "windows",
        &[
            ("close a window", "super + q"),
            ("full screen", "super + f"),
            ("float or tile a window", "super + t"),
            ("next window", "super + tab"),
            ("workspace 1, 2, 3", "super + 1, 2, 3"),
            ("move a window to 1, 2, 3", "super + shift + 1, 2, 3"),
        ],
    ),
    (
        "system",
        &[
            ("lock", "super + l"),
            ("power menu", "super + escape"),
            ("screenshot of an area", "print"),
            ("screenshot of the screen", "ctrl + print"),
            ("screenshot of a window", "alt + print"),
            ("draw on the last screenshot", "super + shift + s"),
            ("record the screen", "super + shift + r"),
        ],
    ),
];
