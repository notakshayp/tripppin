//! The Settings hub, drawn like the Tiles design (mock 04): a small SETTINGS label, big
//! light lowercase pivots across the top (network, display, sound... scroll sideways, or
//! arrow / swipe between them) and under them a two-column grid of big rows: a flat coloured
//! 56 px square with a thin white glyph, a light title, the state written underneath and,
//! for the few that are plain on/off, a flat switch. A row opens the existing detail page.
//! Typing in the search box filters rows across every pivot.

use crate::common::label;
use crate::nm;
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The tile palette.
pub const PALETTE: [&str; 8] = ["#1f6b45", "#0f7c8c", "#c2502e", "#6b3f8f", "#3a4a5a", "#b8862b", "#2c5aa0", "#3a2a22"];

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

pub const PIVOTS: [&str; 9] = ["network", "display", "sound", "power", "ai", "dictation", "privacy", "apps", "about"];

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
        row("dictation", "", "Voice typing", "Not set up yet", 4, MIC),
        row("privacy", "privacy", "Privacy", "Location, camera, microphone", 3, SHIELD),
        row("privacy", "notifications", "Notifications", "What may interrupt you", 5, BELL),
        row("privacy", "accounts", "Accounts", "People who use this computer", 2, USER),
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

/// The pivot `by` places from `from`, stopping at the ends.
pub fn step(from: usize, by: i32, len: usize) -> usize {
    (from as i32 + by).clamp(0, len as i32 - 1) as usize
}

/// The stylesheet of the hub and of the pages it opens (loaded after the base one).
pub fn load_css() {
    let Some(display) = gdk::Display::default() else { return };
    let provider = gtk::CssProvider::new();
    let mut css = include_str!("hub.css").to_string();
    for c in PALETTE {
        css.push_str(&format!(".hub-tile.c{} {{ background: {c}; }}\n", &c[1..]));
    }
    provider.load_from_string(&css);
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2);
}

pub struct Hub {
    pub root: gtk::Box,
    pub search: gtk::SearchEntry,
    /// Moves to the pivot `by` places along.
    pub step: Rc<dyn Fn(i32)>,
    /// The first pivot, which holds the keyboard focus when the window opens.
    pub first: gtk::Button,
}

fn grid_of(rows: &[&Row], open: &Rc<dyn Fn(&str)>) -> gtk::Grid {
    let grid = gtk::Grid::new();
    grid.add_css_class("hub-grid");
    grid.set_column_homogeneous(true);
    grid.set_row_spacing(30);
    grid.set_column_spacing(60);
    for (i, r) in rows.iter().enumerate() {
        grid.attach(&row_widget(r, open), (i % 2) as i32, (i / 2) as i32, 1, 1);
    }
    if rows.is_empty() {
        grid.attach(&label("nothing matches", &["hub-empty"]), 0, 0, 2, 1);
    }
    grid.set_valign(gtk::Align::Start);
    crate::common::play(&grid, "rise");
    grid
}

fn row_widget(r: &Row, open: &Rc<dyn Fn(&str)>) -> gtk::Box {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    let button = gtk::Button::new();
    button.add_css_class("hub-row");
    button.set_hexpand(true);
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    let tile = gtk::Box::new(gtk::Orientation::Vertical, 0);
    tile.add_css_class("hub-tile");
    tile.add_css_class(&format!("c{}", &r.color[1..]));
    tile.set_size_request(56, 56);
    tile.set_valign(gtk::Align::Center);
    tile.set_halign(gtk::Align::Start);
    tile.set_hexpand(false);
    let g = crate::glyph::widget(r.glyph, 26);
    g.set_valign(gtk::Align::Center);
    g.set_halign(gtk::Align::Center);
    g.set_vexpand(true);
    g.set_hexpand(true);
    tile.append(&g);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_valign(gtk::Align::Center);
    let state = label(r.hint, &["hub-state"]);
    state.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let title = label(r.title, &["hub-title"]);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&title);
    text.append(&state);
    inner.append(&tile);
    inner.append(&text);
    button.set_child(Some(&inner));
    line.append(&button);
    if r.section.is_empty() {
        button.set_sensitive(false);
    } else {
        let (open, id) = (open.clone(), r.section);
        button.connect_clicked(move |_| open(id));
    }
    if let Some(live) = r.live {
        let state = state.clone();
        crate::settings::background(live, move |s| state.set_text(&s));
    }
    if let Some(t) = r.toggle {
        let sw = gtk::Switch::new();
        sw.add_css_class("hub-switch");
        sw.set_valign(gtk::Align::Center);
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
        line.append(&sw);
    }
    line
}

fn scrolled(grid: &gtk::Grid) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder().child(grid).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build()
}

/// Builds the hub: header, pivots and the stack of their grids.
pub fn build(open: Rc<dyn Fn(&str)>) -> Hub {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("hub");
    let all: Rc<Vec<Row>> = Rc::new(rows());

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let app = label("SETTINGS", &["hub-app"]);
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
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(if std::env::var_os("TRIPPPIN_SHOT").is_some() { 0 } else { 220 });
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 26);
    bar.add_css_class("hub-pivots");
    let buttons: Rc<Vec<gtk::Button>> = Rc::new(
        PIVOTS
            .iter()
            .map(|p| {
                let b = gtk::Button::new();
                b.add_css_class("hub-pivot");
                b.set_child(Some(&label(p, &[])));
                bar.append(&b);
                b
            })
            .collect(),
    );
    let scroll_bar = gtk::ScrolledWindow::builder()
        .child(&bar)
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .build();
    scroll_bar.add_css_class("hub-pivot-scroll");
    root.append(&scroll_bar);
    root.append(&stack);

    let current = Rc::new(Cell::new(0usize));
    let built: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(vec![false; PIVOTS.len()]));
    let searching = Rc::new(Cell::new(false));
    let select: Rc<dyn Fn(usize)> = {
        let (stack, buttons, current, built, all, open, searching) = (
            stack.clone(),
            buttons.clone(),
            current.clone(),
            built.clone(),
            all.clone(),
            open.clone(),
            searching.clone(),
        );
        let (scroll_bar, bar) = (scroll_bar.clone(), bar.clone());
        Rc::new(move |i: usize| {
            let name = PIVOTS[i];
            if !built.borrow()[i] {
                built.borrow_mut()[i] = true;
                let rows: Vec<&Row> = all.iter().filter(|r| r.pivot == name).collect();
                stack.add_named(&scrolled(&grid_of(&rows, &open)), Some(name));
            }
            searching.set(false);
            stack.set_visible_child_name(name);
            current.set(i);
            for (j, b) in buttons.iter().enumerate() {
                if j == i {
                    b.add_css_class("on");
                    // Scroll the pivot row so the open one is in view.
                    let adj = scroll_bar.hadjustment();
                    let (b, bar) = (b.clone(), bar.clone());
                    glib::idle_add_local_once(move || {
                        let Some(r) = b.compute_bounds(&bar) else { return };
                        let (x, w) = (f64::from(r.x()), f64::from(r.width()));
                        if x + w > adj.value() + adj.page_size() {
                            adj.set_value(x + w - adj.page_size() + 40.0);
                        } else if x < adj.value() {
                            adj.set_value((x - 40.0).max(0.0));
                        }
                    });
                } else {
                    b.remove_css_class("on");
                }
            }
        })
    };
    for (i, b) in buttons.iter().enumerate() {
        let select = select.clone();
        b.connect_clicked(move |_| select(i));
    }
    select(0);

    {
        // Search: one extra page of matches across every pivot.
        let (stack, buttons, all, open, searching, select, current) = (
            stack.clone(),
            buttons.clone(),
            all.clone(),
            open.clone(),
            searching.clone(),
            select.clone(),
            current.clone(),
        );
        search.connect_search_changed(move |s| {
            let q = s.text().to_string();
            if q.trim().is_empty() {
                if searching.get() {
                    select(current.get());
                }
                return;
            }
            if let Some(old) = stack.child_by_name("results") {
                stack.remove(&old);
            }
            let rows = filter(&all, &q);
            stack.add_named(&scrolled(&grid_of(&rows, &open)), Some("results"));
            stack.set_visible_child_name("results");
            searching.set(true);
            for b in buttons.iter() {
                b.remove_css_class("on");
            }
        });
    }

    // Swipe sideways between pivots (touch and trackpad).
    let swipe = gtk::GestureSwipe::new();
    {
        let (select, current) = (select.clone(), current.clone());
        swipe.connect_swipe(move |_, vx, vy| {
            if vx.abs() > 400.0 && vx.abs() > vy.abs() * 2.0 {
                select(step(current.get(), if vx < 0.0 { 1 } else { -1 }, PIVOTS.len()));
            }
        });
    }
    stack.add_controller(swipe);

    let stepper: Rc<dyn Fn(i32)> = {
        let (select, current) = (select.clone(), current.clone());
        Rc::new(move |by| select(step(current.get(), by, PIVOTS.len())))
    };
    Hub { root, search, step: stepper, first: buttons[0].clone() }
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
        for r in &rows {
            assert!(PIVOTS.contains(&r.pivot), "{} is on an unknown pivot", r.title);
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
        let privacy = filter(&rows, "privacy");
        assert!(privacy.iter().any(|r| r.section == "accounts"), "pivot name matches its rows");
    }

    #[test]
    fn pivots_stop_at_the_ends() {
        assert_eq!(step(0, -1, 9), 0);
        assert_eq!(step(8, 1, 9), 8);
        assert_eq!(step(3, 1, 9), 4);
    }
}
