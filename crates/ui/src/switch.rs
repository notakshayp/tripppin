//! Window switching. `tripppin-ui switch` (Alt + Tab) shows the open windows, most recently
//! used first, with the one before selected; Tab moves on, letting go of Alt switches, Esc
//! cancels. `tripppin-ui snap <where>` puts the focused window in half or a quarter of the
//! screen (every window floats, so niri's own column layout doesn't apply).

use crate::common::label;
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::process::Command;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: u64,
    pub title: String,
    pub app_id: String,
    /// Seconds (and nanoseconds) since niri started that it last had focus.
    pub focused_at: (u64, u32),
}

/// `niri msg -j windows`, most recently focused first.
pub fn parse_windows(json: &str) -> Vec<Window> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let mut list: Vec<Window> = items
        .iter()
        .map(|w| Window {
            id: w["id"].as_u64().unwrap_or(0),
            title: w["title"].as_str().unwrap_or("").to_string(),
            app_id: w["app_id"].as_str().unwrap_or("").to_string(),
            focused_at: (
                w["focus_timestamp"]["secs"].as_u64().unwrap_or(0),
                w["focus_timestamp"]["nanos"].as_u64().unwrap_or(0) as u32,
            ),
        })
        .collect();
    list.sort_by_key(|w| std::cmp::Reverse(w.focused_at));
    list
}

fn niri(args: &[&str]) -> Option<String> {
    let out = Command::new("niri").arg("msg").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

// ---- snapping -------------------------------------------------------------------------------

/// The bar's reserved height (its margin plus height in /etc/xdg/waybar) and niri's gaps.
pub const BAR: i32 = 40;
const GAP: i32 = 16;

/// Size and position (width, height, x, y) for a spot on a screen of `w` × `h`. niri places
/// floating windows relative to the area below the bar, so y starts there.
pub fn snap_rect(spot: &str, w: i32, h: i32) -> Option<(i32, i32, i32, i32)> {
    let area_h = h - BAR;
    let half_w = (w - 3 * GAP) / 2;
    let half_h = (area_h - 3 * GAP) / 2;
    let full_h = area_h - 2 * GAP;
    let right = 2 * GAP + half_w;
    let lower = 2 * GAP + half_h;
    Some(match spot {
        "left" => (half_w, full_h, GAP, GAP),
        "right" => (half_w, full_h, right, GAP),
        "top-left" => (half_w, half_h, GAP, GAP),
        "top-right" => (half_w, half_h, right, GAP),
        "bottom-left" => (half_w, half_h, GAP, lower),
        "bottom-right" => (half_w, half_h, right, lower),
        _ => return None,
    })
}

pub fn snap(spot: &str) -> glib::ExitCode {
    if spot == "max" {
        niri(&["action", "maximize-window-to-edges"]);
        return glib::ExitCode::SUCCESS;
    }
    let out: Value = niri(&["-j", "focused-output"]).and_then(|o| serde_json::from_str(&o).ok()).unwrap_or_default();
    let w = out["logical"]["width"].as_i64().unwrap_or(1280) as i32;
    let h = out["logical"]["height"].as_i64().unwrap_or(800) as i32;
    let Some((ww, wh, x, y)) = snap_rect(spot, w, h) else {
        eprintln!("usage: tripppin-ui snap left|right|top-left|top-right|bottom-left|bottom-right|max");
        return glib::ExitCode::FAILURE;
    };
    niri(&["action", "set-window-width", &ww.to_string()]);
    niri(&["action", "set-window-height", &wh.to_string()]);
    niri(&["action", "move-floating-window", "-x", &x.to_string(), "-y", &y.to_string()]);
    glib::ExitCode::SUCCESS
}

// ---- the switcher --------------------------------------------------------------------------

struct Switcher {
    windows: Vec<Window>,
    cards: Vec<gtk::Box>,
    selected: Cell<usize>,
}

impl Switcher {
    fn select(&self, i: usize) {
        if self.cards.is_empty() {
            return;
        }
        let i = i % self.cards.len();
        for (n, c) in self.cards.iter().enumerate() {
            if n == i {
                c.add_css_class("selected");
            } else {
                c.remove_css_class("selected");
            }
        }
        self.selected.set(i);
    }

    fn next(&self, back: bool) {
        let n = self.cards.len().max(1);
        let i = self.selected.get();
        self.select(if back { (i + n - 1) % n } else { (i + 1) % n });
    }

    fn choose(&self) {
        if let Some(w) = self.windows.get(self.selected.get()) {
            niri(&["action", "focus-window", "--id", &w.id.to_string()]);
        }
    }
}

/// The icon a window's app names in its .desktop file.
fn icon_for(app_id: &str, apps: &[crate::apps::App]) -> Option<String> {
    let id = app_id.to_lowercase();
    apps.iter()
        .find(|a| a.id.to_lowercase() == id || a.id.to_lowercase().ends_with(&format!(".{id}")))
        .map(|a| a.icon.clone())
        .filter(|i| !i.is_empty())
}

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("in.trippp.Switch")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let state: Rc<RefCell<Option<Rc<Switcher>>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, _| {
        let mut slot = state.borrow_mut();
        if let Some(s) = slot.as_ref() {
            // Alt + Tab again while it's open: niri runs us again, so step to the next window.
            s.next(false);
            return glib::ExitCode::SUCCESS;
        }
        let windows = parse_windows(&niri(&["-j", "windows"]).unwrap_or_default());
        if windows.len() < 2 {
            app.quit();
            return glib::ExitCode::SUCCESS;
        }
        let s = build(app, windows);
        *slot = Some(s);
        glib::ExitCode::SUCCESS
    });
    let args: Vec<String> = std::env::args().take(1).collect();
    app.run_with_args(&args)
}

fn build(app: &gtk::Application, windows: Vec<Window>) -> Rc<Switcher> {
    crate::common::load_css();
    let window = gtk::Window::builder().application(app).decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("clear");
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-switch"));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(e, true);
    }
    window.set_exclusive_zone(-1);
    let shade = gtk::Box::new(gtk::Orientation::Vertical, 0);
    shade.add_css_class("backdrop");
    let strip = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    strip.add_css_class("switch-strip");
    strip.set_halign(gtk::Align::Center);
    strip.set_valign(gtk::Align::Center);
    strip.set_vexpand(true);
    let apps = crate::apps::installed();
    let theme = gtk::IconTheme::for_display(&gtk::prelude::WidgetExt::display(&window));
    let mut cards = Vec::new();
    for w in &windows {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
        card.add_css_class("switch-card");
        let icon_name = icon_for(&w.app_id, &apps).unwrap_or_else(|| w.app_id.clone());
        let picture: gtk::Widget = if theme.has_icon(&icon_name) {
            let img = gtk::Image::from_icon_name(&icon_name);
            img.set_pixel_size(56);
            img.upcast()
        } else {
            let letter = w.title.chars().next().map(|c| c.to_lowercase().to_string()).unwrap_or_default();
            let l = label(&letter, &["avatar"]);
            l.set_size_request(56, 56);
            l.set_halign(gtk::Align::Center);
            l.set_xalign(0.5);
            l.upcast()
        };
        card.append(&picture);
        let title = label(&w.title.to_lowercase(), &["switch-title"]);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_max_width_chars(16);
        title.set_xalign(0.5);
        card.append(&title);
        strip.append(&card);
        cards.push(card);
    }
    shade.append(&strip);
    window.set_child(Some(&shade));
    let s = Rc::new(Switcher { windows, cards, selected: Cell::new(0) });
    s.select(1);

    let keys = gtk::EventControllerKey::new();
    {
        let (s, app) = (s.clone(), app.clone());
        keys.connect_key_pressed(move |_, key, _, state| {
            use gtk::gdk::Key;
            match key {
                Key::Tab | Key::Right | Key::Down => s.next(state.contains(gtk::gdk::ModifierType::SHIFT_MASK)),
                Key::ISO_Left_Tab | Key::Left | Key::Up => s.next(true),
                Key::Return | Key::KP_Enter => {
                    s.choose();
                    app.quit();
                }
                Key::Escape => app.quit(),
                _ => {}
            }
            glib::Propagation::Stop
        });
    }
    {
        let (s, app) = (s.clone(), app.clone());
        keys.connect_key_released(move |_, key, _, _| {
            if matches!(key, gtk::gdk::Key::Alt_L | gtk::gdk::Key::Alt_R) {
                s.choose();
                app.quit();
            }
        });
    }
    window.add_controller(keys);
    window.present();
    if let Some(c) = window.child() {
        crate::common::play(&c, "pop");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_and_snapping() {
        let json = r#"[{"id":6,"title":"settings","app_id":"in.trippp.Settings","focus_timestamp":{"secs":839,"nanos":5}},
                      {"id":5,"title":"terminal","app_id":"foot","focus_timestamp":{"secs":840,"nanos":0}}]"#;
        let w = parse_windows(json);
        assert_eq!(w.iter().map(|w| w.id).collect::<Vec<_>>(), vec![5, 6]);
        assert_eq!(snap_rect("left", 1280, 800), Some((616, 728, 16, 16)));
        assert_eq!(snap_rect("right", 1280, 800), Some((616, 728, 648, 16)));
        assert_eq!(snap_rect("bottom-right", 1280, 800), Some((616, 356, 648, 388)));
        assert_eq!(snap_rect("middle", 1280, 800), None);
    }
}
