//! The Settings hub, drawn like the Bento design (mock 04): a control board. The common
//! switches and sliders work right on their tiles (Wi-Fi, Bluetooth, display brightness and
//! scale, sound volume, appearance colour, dictation); every other setting is a small tile.
//! Tapping a tile opens its native page as a rounded sheet with a back arrow. Typing in the
//! search box filters tiles across every group.

use crate::common::label;
use crate::nm;
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The tile palette.
pub const PALETTE: [&str; 8] =
    ["#45c486", "#5aa9e6", "#ff8a5a", "#f783ac", "#ffd43b", "#b197fc", "#7b8cff", "#63e6be"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Toggle {
    Wifi,
    Bluetooth,
}

impl Toggle {
    fn get(self) -> bool {
        match self {
            Toggle::Wifi => nm::wifi_enabled(),
            Toggle::Bluetooth => crate::bt::status().0,
        }
    }
    fn set(self, on: bool) {
        let _ = match self {
            Toggle::Wifi => nm::set_wifi(on),
            Toggle::Bluetooth => crate::bt::set_power(on),
        };
    }
    fn state(self) -> String {
        match self {
            Toggle::Wifi => wifi_state(),
            Toggle::Bluetooth => bluetooth_state(),
        }
    }
}

/// One row of the hub.
pub struct Row {
    pub pivot: &'static str,
    /// The Settings section it opens ("" for a row that is only information).
    pub section: &'static str,
    pub title: &'static str,
    /// The state line when nothing live is known.
    pub hint: &'static str,
    pub color: &'static str,
    pub glyph: &'static str,
    pub live: Option<fn() -> String>,
    pub toggle: Option<Toggle>,
}

const WIFI: &str = "M2 9a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01";
const BT: &str = "M7 7l10 10-5 4V3l5 4L7 17";
const GLOBE: &str = "M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18";
const PRINT: &str = "M6 9V3h12v6M6 18H4v-7h16v7h-2M6 14h12v7H6z";
const DISPLAY: &str = "M3 4h18v12H3zM8 20h8M12 16v4";
const PALETTE_G: &str =
    "M12 3a9 9 0 1 0 0 18c1.5 0 2-1 1.5-2s0-2 1.5-2h2a3 3 0 0 0 3-3 9 9 0 0 0-8-11M7.5 11h.01M10 7h.01M14 7h.01";
const ACCESS: &str = "M12 4a1.5 1.5 0 1 0 0 3a1.5 1.5 0 1 0 0-3M5 9l7 1.5L19 9M12 10.5V15l-3 5M12 15l3 5";
const POINTER: &str = "M5 3l14 8-6 2-2 6z";
const KEY: &str = "M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10";
const VOLUME: &str = "M4 9h4l5-4v14l-5-4H4zM16 9a4 4 0 0 1 0 6M19 6a8 8 0 0 1 0 12";
const BATTERY: &str = "M3 7h15v10H3zM21 10v4M6 10v4M9 10v4";
const CLOCK: &str = "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M12 7v5l3 2";
const SPARK: &str = "M12 2l1.5 7.5L21 11l-7.5 1.5L12 20l-1.5-7.5L3 11l7.5-1.5z";
const MIC: &str = "M9 4a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0zM5 11a7 7 0 0 0 14 0M12 18v3";
const SHIELD: &str = "M12 3l8 3v6c0 5-4 8-8 9-4-1-8-4-8-9V6z";
const BELL: &str = "M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 20a2 2 0 0 0 4 0";
const USER: &str = "M12 4a4 4 0 1 0 0 8a4 4 0 1 0 0-8M4 21a8 8 0 0 1 16 0";
const GRID: &str = "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z";
const INFO: &str = "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M12 11v6M12 8h.01";
const ARCHIVE: &str = "M3 4h18v4H3zM5 8v12h14V8M10 12h4";
const DOWNLOAD: &str = "M12 4v12M7 11l5 5 5-5M5 20h14";

fn row(
    pivot: &'static str,
    section: &'static str,
    title: &'static str,
    hint: &'static str,
    color: usize,
    glyph: &'static str,
) -> Row {
    Row { pivot, section, title, hint, color: PALETTE[color], glyph, live: None, toggle: None }
}

fn wifi_state() -> String {
    if !nm::has_wifi() {
        return if nm::wired() { "Wired".into() } else { "No Wi-Fi adapter".into() };
    }
    if !nm::wifi_enabled() {
        return "Off".into();
    }
    match crate::common::wifi_name() {
        Some(n) => format!("{n} · connected"),
        None => "On · not connected".into(),
    }
}

fn bluetooth_state() -> String {
    if !crate::bt::has_adapter() {
        return "No adapter".into();
    }
    let (on, name) = crate::bt::status();
    match (on, name.is_empty()) {
        (false, _) => "Off".into(),
        (true, true) => "On".into(),
        (true, false) => format!("On · seen as {name}"),
    }
}

fn dictation_state() -> String {
    if crate::dictate::enabled() { "On · Super + M to speak".into() } else { "Off · turn it on in its page".into() }
}

fn battery_state() -> String {
    match crate::power::battery() {
        Some(b) => format!("{}% · {}", b.percent, b.state.replace('-', " ")),
        None => "On mains power".into(),
    }
}

pub fn rows() -> Vec<Row> {
    vec![
        Row { live: Some(wifi_state), toggle: Some(Toggle::Wifi), ..row("network", "wifi", "Wi-Fi", "", 6, WIFI) },
        Row {
            live: Some(bluetooth_state),
            toggle: Some(Toggle::Bluetooth),
            ..row("network", "bluetooth", "Bluetooth", "", 3, BT)
        },
        row("network", "network", "Network & VPN", "Wired, VPN, proxy, hotspot", 5, GLOBE),
        row("network", "printers", "Printers & scanners", "Add and manage", 4, PRINT),
        row("display", "display", "Display", "Screens, scale, night light", 1, DISPLAY),
        row("display", "appearance", "Appearance", "Wallpaper, text size, corners", 3, PALETTE_G),
        row("display", "accessibility", "Accessibility", "Seeing, hearing, moving", 4, ACCESS),
        row("display", "pointer", "Mouse & touchpad", "Speed, scrolling, tapping", 2, POINTER),
        row("display", "keyboard", "Keyboard", "Layouts and shortcuts", 5, KEY),
        row("sound", "sound", "Sound", "Volume, outputs and inputs", 6, VOLUME),
        Row { live: Some(battery_state), ..row("power", "power", "Power & battery", "", 0, BATTERY) },
        row("power", "time", "Date & time", "Clock and time zone", 1, CLOCK),
        row("ai", "ai", "AI", "Model, providers, what it may do", 0, SPARK),
        row("dictation", "sound", "Microphone", "Pick the input you speak into", 2, MIC),
        row("dictation", "language", "Language & region", "What you speak and read", 6, GLOBE),
        Row { live: Some(dictation_state), ..row("dictation", "dictation", "Dictation", "", 4, MIC) },
        row("account", "account", "Account", "Your name, picture and password; other people", 2, USER),
        row("privacy", "privacy", "Privacy", "Location, camera, microphone", 3, SHIELD),
        row("privacy", "notifications", "Notifications", "What may interrupt you", 5, BELL),
        row("apps", "apps", "Apps", "Installed apps and what they may use", 1, GRID),
        row("about", "about", "About", "This computer and its software", 4, INFO),
        row("about", "storage", "Storage", "Space used and free", 0, ARCHIVE),
        row("about", "updates", "Updates", "The system image and apps", 6, DOWNLOAD),
    ]
}

/// The rows whose title, state, pivot or section match `query` (case-insensitive, every
/// word); all rows for an empty query.
pub fn filter<'a>(rows: &'a [Row], query: &str) -> Vec<&'a Row> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    rows.iter()
        .filter(|r| {
            let hay = format!("{} {} {} {}", r.title, r.hint, r.pivot, r.section).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect()
}

/// The stylesheet of the hub and of the pages it opens (loaded after the base one).
pub fn load_css() {
    let Some(display) = gdk::Display::default() else { return };
    let provider = gtk::CssProvider::new();
    let mut css = include_str!("hub.css").to_string();
    for c in PALETTE {
        css.push_str(&format!(".hub-icon.c{0}, .cal-block.c{0}, .cal-bar.c{0} {{ background: {c}; }}\n", &c[1..]));
    }
    for (c, _) in crate::settings::ACCENTS {
        css.push_str(&format!("button.swatch.c{0}, button.dot-swatch.c{0} {{ background: {c}; }}\n", &c[1..]));
    }
    provider.load_from_string(&css);
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2);
}


pub struct Hub {
    pub root: gtk::Box,
    pub search: gtk::SearchEntry,
    /// Holds the keyboard focus when the window opens.
    pub first: gtk::Widget,
}

fn tile_box(small: bool, classes: &[&str]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
    b.add_css_class("hub-tile");
    b.set_hexpand(true);
    b.set_size_request(-1, if small { 76 } else { 132 });
    for c in classes {
        b.add_css_class(c);
    }
    b
}

/// A click anywhere on the tile that no control inside it claimed opens its page.
fn opens(tile: &gtk::Box, open: &Rc<dyn Fn(&str)>, section: &'static str) {
    if section.is_empty() {
        return;
    }
    tile.add_css_class("hub-link");
    let click = gtk::GestureClick::new();
    let open = open.clone();
    click.connect_released(move |_, _, _, _| open(section));
    tile.add_controller(click);
}

fn glyph_row(glyph: &str, size: i32) -> gtk::Widget {
    let g = crate::glyph::widget(glyph, size);
    g.set_halign(gtk::Align::Start);
    g.upcast()
}

fn spacer() -> gtk::Box {
    let s = gtk::Box::new(gtk::Orientation::Vertical, 0);
    s.set_vexpand(true);
    s
}

/// A slider that sends its value no more than every 120 ms.
fn slider(initial: f64, accent: &str, send: impl Fn(u32) + 'static) -> (gtk::Scale, gtk::Label) {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.add_css_class("bento-scale");
    scale.add_css_class(accent);
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    scale.set_value(initial);
    let value = label(&format!("{initial:.0}%"), &["tile-title"]);
    let pending = Rc::new(Cell::new(false));
    let send = Rc::new(send);
    {
        let value = value.clone();
        scale.connect_value_changed(move |s| {
            let v = s.value().round() as u32;
            value.set_text(&format!("{v}%"));
            if pending.replace(true) {
                return;
            }
            let (pending, send, s) = (pending.clone(), send.clone(), s.clone());
            glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
                pending.set(false);
                send(s.value().round() as u32);
            });
        });
    }
    (scale, value)
}

fn chip(text: &str) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("hub-chip");
    b
}

fn toggle_widget(t: Toggle, state: &gtk::Label, dark: bool) -> gtk::Switch {
    let sw = gtk::Switch::new();
    sw.add_css_class("hub-switch");
    if dark {
        sw.add_css_class("on-colour");
    }
    sw.set_valign(gtk::Align::Start);
    let loaded = Rc::new(Cell::new(false));
    {
        let (sw, loaded) = (sw.clone(), loaded.clone());
        crate::settings::background(
            move || t.get(),
            move |on| {
                sw.set_active(on);
                loaded.set(true);
            },
        );
    }
    let state = state.clone();
    sw.connect_state_set(move |_, on| {
        if loaded.get() {
            let state = state.clone();
            crate::settings::background(
                move || {
                    t.set(on);
                    t.state()
                },
                move |s| state.set_text(&s),
            );
        }
        glib::Propagation::Proceed
    });
    sw
}

/// A tile for a setting that has a switch of its own (Wi-Fi, Bluetooth).
fn toggle_tile(r: &Row, colour: &str, open: &Rc<dyn Fn(&str)>) -> gtk::Box {
    let tile = tile_box(false, &["bento-tile", colour]);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let g = glyph_row(r.glyph, 24);
    g.set_hexpand(true);
    top.append(&g);
    let state = label(r.hint, &["tile-sub"]);
    state.set_ellipsize(gtk::pango::EllipsizeMode::End);
    if let Some(t) = r.toggle {
        top.append(&toggle_widget(t, &state, true));
    }
    tile.append(&top);
    tile.append(&spacer());
    tile.append(&label(r.title, &["tile-title", "big-title"]));
    tile.append(&state);
    if let Some(live) = r.live {
        let state = state.clone();
        crate::settings::background(live, move |s| state.set_text(&s));
    }
    opens(&tile, open, r.section);
    tile
}

fn display_tile(r: &Row, open: &Rc<dyn Fn(&str)>) -> gtk::Box {
    let tile = tile_box(false, &[]);
    tile.append(&label(r.title, &["tile-title", "big-title"]));
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    line.set_margin_top(4);
    line.append(&glyph_row(r.glyph, 20));
    let (scale, value) = slider(70.0, "amber", |p| {
        std::thread::spawn(move || crate::display::set_brightness(p));
    });
    line.append(&scale);
    line.append(&value);
    tile.append(&line);
    let chips = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    chips.set_margin_top(6);
    let scales: Vec<(f64, gtk::Button)> =
        [1.0, 1.25, 1.5].iter().map(|s| (*s, chip(&format!("{:.0}%", s * 100.0)))).collect();
    for (_, b) in &scales {
        chips.append(b);
    }
    let night = chip("Night light");
    chips.append(&night);
    tile.append(&chips);
    // Load what the screen is doing now; hide the slider when there's no backlight.
    {
        let (scale, line, scales, night) = (scale.clone(), line.clone(), scales.clone(), night.clone());
        crate::settings::background(
            || {
                let out = crate::display::outputs().into_iter().find(|o| o.enabled);
                (crate::display::brightness(), out.map(|o| o.scale), crate::display::night_light())
            },
            move |(brightness, current, nl)| {
                match brightness {
                    Some(b) => scale.set_value((b * 100.0).round()),
                    None => line.set_visible(false),
                }
                if let Some(c) = current {
                    for (s, b) in &scales {
                        if (s - c).abs() < 0.02 {
                            b.add_css_class("on");
                        }
                    }
                }
                if nl {
                    night.add_css_class("on");
                }
            },
        );
    }
    for (s, b) in &scales {
        let (s, all) = (*s, scales.clone());
        b.connect_clicked(move |_| {
            for (v, x) in &all {
                if (v - s).abs() < 0.001 {
                    x.add_css_class("on");
                } else {
                    x.remove_css_class("on");
                }
            }
            std::thread::spawn(move || {
                if let Some(o) = crate::display::outputs().into_iter().find(|o| o.enabled)
                    && let Some(m) = o.current.and_then(|i| o.modes.get(i))
                {
                    let _ = crate::display::apply(&o, m, s, &o.transform);
                }
            });
        });
    }
    night.connect_clicked(move |b| {
        let on = !b.has_css_class("on");
        if on {
            b.add_css_class("on");
        } else {
            b.remove_css_class("on");
        }
        std::thread::spawn(move || crate::display::set_night_light(on));
    });
    opens(&tile, open, r.section);
    tile
}

fn sound_tile(r: &Row, open: &Rc<dyn Fn(&str)>) -> gtk::Box {
    let tile = tile_box(false, &[]);
    tile.append(&label(r.title, &["tile-title", "big-title"]));
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    line.set_margin_top(4);
    line.append(&glyph_row(r.glyph, 20));
    let device: Rc<RefCell<String>> = Rc::default();
    let dev = device.clone();
    let (scale, value) = slider(60.0, "pink", move |p| {
        let name = dev.borrow().clone();
        if !name.is_empty() {
            std::thread::spawn(move || crate::snd::set_volume(true, &name, p));
        }
    });
    line.append(&scale);
    line.append(&value);
    tile.append(&line);
    let note = label("Volume, outputs and inputs", &["tile-sub"]);
    note.set_ellipsize(gtk::pango::EllipsizeMode::End);
    note.set_margin_top(6);
    tile.append(&note);
    {
        let (scale, note) = (scale.clone(), note.clone());
        crate::settings::background(
            || {
                let (outs, def) = crate::snd::outputs();
                let (ins, defin) = crate::snd::inputs();
                let out = outs.into_iter().find(|d| d.name == def);
                let inp = ins.into_iter().find(|d| d.name == defin);
                (out, inp.map(|d| d.description))
            },
            move |(out, inp)| {
                let Some(out) = out else { return };
                *device.borrow_mut() = out.name.clone();
                scale.set_value(f64::from(out.volume));
                let mut text = format!("Output: {}", out.description);
                if let Some(i) = inp {
                    text.push_str(&format!(" · Input: {i}"));
                }
                note.set_text(&text);
            },
        );
    }
    opens(&tile, open, r.section);
    tile
}

fn battery_tile(r: &Row, open: &Rc<dyn Fn(&str)>) -> gtk::Box {
    let tile = tile_box(false, &["bento-tile", "mint"]);
    tile.append(&glyph_row(r.glyph, 24));
    tile.append(&spacer());
    let big = label("", &["bento-big", "tile-number"]);
    let sub = label("", &["tile-sub"]);
    tile.append(&big);
    tile.append(&sub);
    crate::settings::background(crate::power::battery, move |b| match b {
        Some(b) => {
            big.set_text(&format!("{}%", b.percent));
            sub.set_text(&if b.time_left.is_empty() { b.state.replace('-', " ") } else { b.time_left.clone() });
        }
        None => {
            big.set_text("AC");
            sub.set_text("On mains power");
        }
    });
    opens(&tile, open, r.section);
    tile
}

fn appearance_tile(r: &Row, open: &Rc<dyn Fn(&str)>) -> gtk::Box {
    let tile = tile_box(false, &[]);
    let g = glyph_row(r.glyph, 22);
    g.add_css_class("violet-glyph");
    tile.append(&g);
    tile.append(&spacer());
    tile.append(&label(r.title, &["tile-title"]));
    let swatches = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let all: Rc<RefCell<Vec<gtk::Button>>> = Rc::default();
    let picked = crate::settings::accent_now();
    for (hex, name) in crate::settings::ACCENTS.iter().take(4) {
        let b = gtk::Button::new();
        b.add_css_class("dot-swatch");
        b.add_css_class(&format!("c{}", &hex[1..]));
        b.set_tooltip_text(Some(name));
        if *hex == picked {
            b.add_css_class("selected");
        }
        let (all2, hex) = (all.clone(), *hex);
        b.connect_clicked(move |b| {
            let (b, all) = (b.clone(), all2.clone());
            crate::settings::background(
                move || crate::kde::run("plasma-apply-colorscheme", &["--accent-color", hex]),
                move |r| {
                    if r.is_ok() {
                        for x in all.borrow().iter() {
                            x.remove_css_class("selected");
                        }
                        b.add_css_class("selected");
                    }
                },
            );
        });
        all.borrow_mut().push(b.clone());
        swatches.append(&b);
    }
    tile.append(&swatches);
    opens(&tile, open, r.section);
    tile
}

fn dictation_tile(r: &Row, open: &Rc<dyn Fn(&str)>) -> gtk::Box {
    let tile = tile_box(false, &[]);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.set_valign(gtk::Align::Center);
    line.set_vexpand(true);
    let g = glyph_row(r.glyph, 22);
    g.add_css_class("orange-glyph");
    line.append(&g);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(&label(r.title, &["tile-title"]));
    text.append(&label("Press Super M and speak", &["tile-sub"]));
    line.append(&text);
    let sw = gtk::Switch::new();
    sw.add_css_class("hub-switch");
    sw.set_valign(gtk::Align::Center);
    sw.set_active(crate::dictate::enabled());
    sw.connect_state_set(|_, on| {
        crate::dictate::set_enabled(on);
        glib::Propagation::Proceed
    });
    line.append(&sw);
    tile.append(&line);
    opens(&tile, open, r.section);
    tile
}

fn ai_tile(r: &Row, open: &Rc<dyn Fn(&str)>) -> gtk::Box {
    let tile = tile_box(false, &[]);
    let g = glyph_row(r.glyph, 22);
    g.add_css_class("green-glyph");
    tile.append(&g);
    tile.append(&spacer());
    tile.append(&label(r.title, &["tile-title"]));
    tile.append(&label("Model, providers, limits", &["tile-sub"]));
    opens(&tile, open, r.section);
    tile
}

/// A small tile for every other setting: filled icon, title, state.
fn small_tile(r: &Row, open: &Rc<dyn Fn(&str)>) -> gtk::Box {
    let tile = tile_box(true, &[]);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.set_valign(gtk::Align::Center);
    line.set_vexpand(true);
    let chip = gtk::Box::new(gtk::Orientation::Vertical, 0);
    chip.add_css_class("hub-icon");
    chip.add_css_class(&format!("c{}", &r.color[1..]));
    chip.set_valign(gtk::Align::Center);
    chip.set_halign(gtk::Align::Start);
    chip.set_hexpand(false);
    chip.set_size_request(40, 40);
    let g = crate::glyph::widget(r.glyph, 20);
    g.set_valign(gtk::Align::Center);
    g.set_halign(gtk::Align::Center);
    g.set_vexpand(true);
    g.set_hexpand(true);
    chip.append(&g);
    line.append(&chip);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_valign(gtk::Align::Center);
    let title = label(r.title, &["tile-title"]);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let state = label(r.hint, &["tile-sub"]);
    state.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&title);
    text.append(&state);
    line.append(&text);
    tile.append(&line);
    if let Some(live) = r.live {
        let state = state.clone();
        crate::settings::background(live, move |s| state.set_text(&s));
    }
    opens(&tile, open, r.section);
    tile
}

/// Where each control tile sits on the board: (section, column, row, width).
const BOARD: [(&str, i32, i32, i32); 8] = [
    ("wifi", 0, 0, 1),
    ("bluetooth", 1, 0, 1),
    ("display", 2, 0, 2),
    ("sound", 0, 1, 2),
    ("power", 2, 1, 1),
    ("ai", 3, 1, 1),
    ("appearance", 0, 2, 1),
    ("dictation", 1, 2, 2),
];

fn is_control(r: &Row) -> bool {
    BOARD.iter().any(|b| b.0 == r.section) && !(r.section == "sound" && r.pivot == "dictation")
}

fn control_tile(r: &Row, open: &Rc<dyn Fn(&str)>) -> gtk::Box {
    match r.section {
        "wifi" => toggle_tile(r, "blue", open),
        "bluetooth" => toggle_tile(r, "indigo", open),
        "display" => display_tile(r, open),
        "sound" => sound_tile(r, open),
        "power" => battery_tile(r, open),
        "ai" => ai_tile(r, open),
        "appearance" => appearance_tile(r, open),
        _ => dictation_tile(r, open),
    }
}

fn grid() -> gtk::Grid {
    let grid = gtk::Grid::new();
    grid.add_css_class("hub-grid");
    grid.set_column_homogeneous(true);
    grid.set_row_spacing(12);
    grid.set_column_spacing(12);
    grid.set_valign(gtk::Align::Start);
    grid
}

/// The control board, then a grid of small tiles for everything else.
fn board_of(all: &[Row], open: &Rc<dyn Fn(&str)>) -> gtk::Grid {
    let grid = grid();
    for (section, c, r, w) in BOARD {
        if let Some(row) = all.iter().find(|x| x.section == section && is_control(x)) {
            grid.attach(&control_tile(row, open), c, r, w, 1);
        }
    }
    let rest: Vec<&Row> = all.iter().filter(|r| !is_control(r)).collect();
    for (i, r) in rest.iter().enumerate() {
        // One small tile fills the board's last gap, the rest run two to a row below it.
        if i == 0 {
            grid.attach(&small_tile(r, open), 3, 2, 1, 1);
        } else {
            grid.attach(&small_tile(r, open), ((i - 1) % 2 * 2) as i32, 3 + ((i - 1) / 2) as i32, 2, 1);
        }
    }
    crate::common::play(&grid, "rise");
    grid
}

fn results_of(rows: &[&Row], open: &Rc<dyn Fn(&str)>) -> gtk::Grid {
    let grid = grid();
    for (i, r) in rows.iter().enumerate() {
        grid.attach(&small_tile(r, open), (i % 2 * 2) as i32, (i / 2) as i32, 2, 1);
    }
    if rows.is_empty() {
        grid.attach(&label("nothing matches", &["hub-empty"]), 0, 0, 4, 1);
    }
    crate::common::play(&grid, "rise");
    grid
}

fn scrolled(grid: &gtk::Grid) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder().child(grid).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build()
}

/// Builds the hub: the title and search, and the board (or the matches while searching).
pub fn build(open: Rc<dyn Fn(&str)>) -> Hub {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.add_css_class("hub");
    let all: Rc<Vec<Row>> = Rc::new(rows());

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let app = label("Settings", &["hub-app"]);
    app.set_hexpand(true);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search settings"));
    search.add_css_class("hub-search");
    search.set_valign(gtk::Align::Center);
    search.set_width_chars(22);
    top.append(&app);
    top.append(&search);
    root.append(&top);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(if std::env::var_os("TRIPPPIN_SHOT").is_some() { 0 } else { 150 });
    stack.add_named(&scrolled(&board_of(&all, &open)), Some("board"));
    root.append(&stack);

    {
        let (stack, all, open) = (stack.clone(), all.clone(), open.clone());
        search.connect_search_changed(move |s| {
            let q = s.text().to_string();
            if let Some(old) = stack.child_by_name("results") {
                stack.remove(&old);
            }
            if q.trim().is_empty() {
                stack.set_visible_child_name("board");
                return;
            }
            let rows = filter(&all, &q);
            stack.add_named(&scrolled(&results_of(&rows, &open)), Some("results"));
            stack.set_visible_child_name("results");
        });
    }
    let first = search.clone().upcast();
    Hub { root, search, first }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_has_a_row() {
        let rows = rows();
        for (id, ..) in crate::settings::SECTIONS {
            assert!(rows.iter().any(|r| r.section == id), "no hub row opens {id}");
        }
    }

    #[test]
    fn filtering_matches_across_pivots() {
        let rows = rows();
        assert_eq!(filter(&rows, "").len(), rows.len());
        let wifi = filter(&rows, "wi-fi");
        assert_eq!(wifi.len(), 1);
        assert_eq!(wifi[0].section, "wifi");
        assert!(filter(&rows, "volume sound").iter().any(|r| r.section == "sound"));
        assert!(filter(&rows, "zzz").is_empty());
        let account = filter(&rows, "account");
        assert!(account.iter().any(|r| r.section == "account"), "pivot name matches its rows");
        assert!(filter(&rows, "password").iter().any(|r| r.section == "account"), "the hint is searched too");
    }
}
