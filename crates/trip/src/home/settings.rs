//! The settings view: tabs across, rows with real switches, sliders you can click or drag.
//! Network (iwd), sound (wpctl), display, power, ai, dictation, keyboard, account, updates.

use super::input::Input;
use super::ui::*;
use super::{App, Msg};
use crate::sysx::{self, Network};
use crate::{apk, dictate, tools, web};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::process::Command;

pub const TABS: [&str; 9] =
    ["network", "sound", "display", "power", "ai", "dictation", "keyboard", "account", "updates"];
const LAYOUTS: [&str; 8] = ["us", "gb", "de", "fr", "es", "in", "br", "jp"];
const PROFILES: [&str; 3] = ["performance", "balanced", "power-saver"];
const SEARCH: [(&str, &str); 4] = [
    ("duckduckgo", "duckduckgo (no key)"),
    ("brave", "brave search (key)"),
    ("searxng", "searxng (your server)"),
    ("tavily", "tavily (key)"),
];

#[derive(Default)]
pub struct SettingsState {
    pub tab: usize,
    pub sel: usize,
    pub networks: Vec<Network>,
    pub scanning: bool,
    pub wifi_powered: Option<bool>,
    pub pw: Option<(usize, Input)>,
    pub volume: Option<(u8, bool)>,
    pub brightness: Option<u8>,
    pub profile: Option<String>,
    pub search: web::SearchConfig,
    pub key: Input,
    pub key_focus: bool,
    pub message: String,
    pub updating: bool,
    /// The interactive rows of the tab as last drawn, for the keyboard.
    pub rows: Vec<Act>,
}

pub fn wifi_powered(dev: &str) -> Option<bool> {
    let o = Command::new("iwctl").args(["device", dev, "show"]).output().ok()?;
    let text = sysx::strip_ansi(&String::from_utf8_lossy(&o.stdout));
    text.lines().find(|l| l.trim_start().starts_with("Powered")).map(|l| l.contains(" on"))
}

impl App {
    pub fn settings_open(&mut self) {
        self.settings_refresh();
    }

    pub fn settings_refresh(&mut self) {
        let s = &mut self.settings;
        s.volume = sysx::volume();
        s.brightness = sysx::brightness();
        s.profile = sysx::power_profile();
        s.search = web::load_config();
        s.key = Input::new(&s.search.key);
        s.key.masked = true;
        if TABS[s.tab] == "network" && !s.scanning {
            s.scanning = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let dev = sysx::wifi_device();
                let on = dev.as_deref().and_then(wifi_powered);
                let nets = if on == Some(false) { Vec::new() } else { sysx::wifi_networks() };
                let _ = tx.send(Msg::Wifi(nets, on, sysx::wifi_status()));
            });
        }
        self.dirty = true;
    }

    pub fn wifi_done(&mut self, nets: Vec<Network>, on: Option<bool>, current: Option<String>) {
        self.settings.networks = nets;
        self.settings.scanning = false;
        self.settings.wifi_powered = on;
        self.wifi = current;
        self.dirty = true;
    }

    pub fn set_tab(&mut self, t: usize) {
        self.settings.tab = t % TABS.len();
        self.settings.sel = 0;
        self.settings.pw = None;
        self.settings.key_focus = false;
        self.settings.message.clear();
        self.settings_refresh();
    }

    pub fn slider_set(&mut self, id: Slider, frac: f32) {
        let pct = (frac * 100.0).round() as u8;
        match id {
            Slider::Volume => {
                if self.settings.volume.map(|v| v.0) != Some(pct) && sysx::set_volume(pct) {
                    self.settings.volume = Some((pct, false));
                }
            }
            Slider::Brightness => {
                let pct = pct.max(1);
                if self.settings.brightness != Some(pct) && sysx::set_brightness(pct) {
                    self.settings.brightness = Some(pct);
                }
            }
            Slider::Seek => {
                if let Some(pl) = &self.playing
                    && pl.length > 0.0
                {
                    let _ = Command::new("playerctl")
                        .args(["position", &format!("{:.0}", f64::from(frac) * pl.length)])
                        .output();
                }
            }
        }
        self.dirty = true;
    }

    /// Settings-only clicks.
    pub fn settings_act(&mut self, a: Act) {
        match a {
            Act::SetTab(t) => self.set_tab(t),
            Act::WifiToggle => {
                if let Some(dev) = sysx::wifi_device() {
                    let on = self.settings.wifi_powered != Some(true);
                    let _ = Command::new("iwctl")
                        .args(["device", &dev, "set-property", "Powered", if on { "on" } else { "off" }])
                        .output();
                    self.settings.wifi_powered = Some(on);
                    self.settings_refresh();
                }
            }
            Act::WifiRescan => self.settings_refresh(),
            Act::WifiConnect(i) => {
                let Some(n) = self.settings.networks.get(i).cloned() else { return };
                if n.secured {
                    self.settings.pw = Some((i, Input::secret()));
                } else {
                    self.wifi_connect(&n.name, None);
                }
            }
            Act::WifiForget => {
                if let Some(c) = self.wifi.clone() {
                    sysx::wifi_forget(&c);
                    self.settings.message = format!("forgot {c}");
                    self.wifi = None;
                    self.settings_refresh();
                }
            }
            Act::Mute => {
                let muted = self.settings.volume.is_some_and(|v| v.1);
                sysx::set_mute(!muted);
                self.settings.volume = sysx::volume();
            }
            Act::PowerProfile(i) => {
                if sysx::set_power_profile(PROFILES[i]) {
                    self.settings.profile = Some(PROFILES[i].to_string());
                }
            }
            Act::AskKind(i) => {
                let kind = tools::KINDS[i].0.to_string();
                let mut now = tools::ask_before();
                if now.contains(&kind) {
                    now.retain(|k| *k != kind);
                } else {
                    now.push(kind);
                }
                tools::set_ask_before(&now);
            }
            Act::SearchProvider(i) => {
                self.settings.search.provider = if i == 0 { String::new() } else { SEARCH[i].0.to_string() };
                let _ = web::save_config(&self.settings.search);
            }
            Act::KeyboardLayout(i) => {
                let f = crate::paths::config_dir().join("keyboard-layout");
                let _ = std::fs::create_dir_all(crate::paths::config_dir());
                let _ = std::fs::write(f, LAYOUTS[i]);
                self.settings.message = format!("layout {} is used from the next login", LAYOUTS[i]);
            }
            Act::UpdateCheck => self.update_check(),
            Act::UpdateRun => {
                self.ask_send("upgrade all installed packages with apk upgrade, then tell me what changed")
            }
            Act::TestDictation => {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = Command::new(exe).arg("dictate").spawn();
                }
            }
            Act::Field(2) => self.settings.key_focus = true,
            _ => {}
        }
        self.dirty = true;
    }

    fn wifi_connect(&mut self, ssid: &str, pw: Option<&str>) {
        self.settings.message = match sysx::wifi_connect(ssid, pw) {
            Ok(()) => format!("connected to {ssid}"),
            Err(e) => format!("couldn't connect: {e}"),
        };
        self.settings.pw = None;
        self.settings_refresh();
    }

    fn update_check(&mut self) {
        if self.settings.updating {
            return;
        }
        self.settings.updating = true;
        self.settings.message = "checking…".into();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let up = format!("{}apk update", apk::privilege());
            let _ = Command::new("sh").arg("-c").arg(up).output();
            let out = Command::new("apk")
                .args(["version", "-l", "<"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            let n = out.lines().filter(|l| l.contains(" < ")).count();
            let _ = tx.send(Msg::Update(match n {
                0 => "everything is up to date".to_string(),
                1 => "1 package can be updated".to_string(),
                n => format!("{n} packages can be updated"),
            }));
        });
    }

    pub fn settings_key(&mut self, k: KeyEvent) {
        if let Some((i, pw)) = &mut self.settings.pw {
            match k.code {
                KeyCode::Esc => self.settings.pw = None,
                KeyCode::Enter => {
                    let (i, pw) = (*i, pw.text.clone());
                    if let Some(n) = self.settings.networks.get(i).cloned() {
                        self.wifi_connect(&n.name, Some(&pw));
                    }
                }
                _ => {
                    pw.key(k);
                }
            }
            self.dirty = true;
            return;
        }
        if self.settings.key_focus {
            match k.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.settings.key_focus = false;
                    self.settings.search.key = self.settings.key.text.clone();
                    let _ = web::save_config(&self.settings.search);
                    self.settings.message = "saved".into();
                }
                _ => {
                    self.settings.key.key(k);
                }
            }
            self.dirty = true;
            return;
        }
        let n = self.settings.rows.len();
        match k.code {
            KeyCode::Tab => self.set_tab(self.settings.tab + 1),
            KeyCode::BackTab => self.set_tab(self.settings.tab + TABS.len() - 1),
            KeyCode::Up => self.settings.sel = self.settings.sel.saturating_sub(1),
            KeyCode::Down => self.settings.sel = (self.settings.sel + 1).min(n.saturating_sub(1)),
            KeyCode::Left | KeyCode::Right => {
                if let Some(Act::Slider(id)) = self.settings.rows.get(self.settings.sel).cloned() {
                    let cur = match id {
                        Slider::Volume => self.settings.volume.map_or(0, |v| v.0),
                        Slider::Brightness => self.settings.brightness.unwrap_or(50),
                        Slider::Seek => 0,
                    };
                    let step = if k.code == KeyCode::Left { -5i32 } else { 5 };
                    let next = (i32::from(cur) + step).clamp(0, 100) as f32 / 100.0;
                    self.slider_set(id, next);
                } else {
                    self.set_tab(if k.code == KeyCode::Left {
                        self.settings.tab + TABS.len() - 1
                    } else {
                        self.settings.tab + 1
                    });
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(a) = self.settings.rows.get(self.settings.sel).cloned() {
                    self.act(a);
                }
            }
            KeyCode::Char(c @ '1'..='9') => self.set_tab(c as usize - '1' as usize),
            _ => {}
        }
        self.dirty = true;
    }

    fn row_mark(&mut self, p: &mut Painter, x: u16, y: u16, act: Act) {
        let idx = self.settings.rows.len();
        if idx == self.settings.sel {
            p.text(x.saturating_sub(2), y, "›", fg(GREEN));
        }
        self.settings.rows.push(act);
    }

    pub fn draw_settings(&mut self, p: &mut Painter, area: Rect) {
        let area = Rect::new(area.x + 2.min(area.width / 8), area.y, area.width - 4.min(area.width / 4), area.height);
        let Some(inner) = p.pane(area, "settings", true, None, Some(Act::View(View::Home)), None, 1.0) else { return };
        self.settings.rows.clear();
        let mut x = inner.x;
        for (i, t) in TABS.iter().enumerate() {
            x += p.button(x, inner.y, t, Act::SetTab(i), i == self.settings.tab) + 1;
        }
        let (lx, vx) = (inner.x + 2, inner.x + 22);
        let mut y = inner.y + 2;
        let tab = TABS[self.settings.tab];
        let label = |p: &mut Painter, y: u16, s: &str| {
            p.text(lx, y, s, fg(MUTED));
        };
        match tab {
            "network" => {
                label(p, y, "wi-fi");
                let on = self.settings.wifi_powered != Some(false);
                self.row_mark(p, lx, y, Act::WifiToggle);
                let w = p.link(
                    vx,
                    y,
                    if on { "(•) on " } else { "( ) on " },
                    Act::WifiToggle,
                    fg(if on { GREEN } else { TEXT }),
                );
                p.link(
                    vx + w + 1,
                    y,
                    if on { "( ) off" } else { "(•) off" },
                    Act::WifiToggle,
                    fg(if on { TEXT } else { GREEN }),
                );
                y += 2;
                label(p, y, "connected to");
                match self.wifi.clone() {
                    Some(c) => {
                        let w = p.text(vx, y, &c, fg(BRIGHT));
                        let bars = self.settings.networks.iter().find(|n| n.connected).map_or(4, |n| n.bars);
                        p.text(vx + w + 2, y, &signal(bars), fg(GREEN));
                        self.row_mark(p, lx, y, Act::WifiForget);
                        p.button(vx + w + 8, y, "forget", Act::WifiForget, false);
                    }
                    None => {
                        p.text(vx, y, "not connected", fg(MUTED));
                    }
                }
                y += 2;
                label(p, y, "other networks");
                if self.settings.scanning {
                    p.text(vx, y, &format!("{} scanning…", spinner(self.started.elapsed().as_millis())), fg(AMBER));
                } else if self.settings.networks.is_empty() {
                    p.text(vx, y, if on { "none found" } else { "wi-fi is off" }, fg(MUTED));
                    self.row_mark(p, lx, y, Act::WifiRescan);
                    p.button(vx + 16, y, "scan", Act::WifiRescan, false);
                }
                let nets = self.settings.networks.clone();
                for (i, n) in nets.iter().enumerate().filter(|(_, n)| !n.connected) {
                    p.text(vx, y, &format!("{:<22}", n.name.chars().take(21).collect::<String>()), fg(TEXT));
                    p.text(
                        vx + 23,
                        y,
                        &format!("{} {}", if n.secured { "secured" } else { "open   " }, signal(n.bars)),
                        fg(MUTED),
                    );
                    self.row_mark(p, lx, y, Act::WifiConnect(i));
                    p.button(vx + 40, y, "connect", Act::WifiConnect(i), false);
                    y += 1;
                    if let Some((pi, pw)) = &self.settings.pw
                        && *pi == i
                    {
                        p.text(vx + 2, y, "password ", fg(MUTED));
                        p.text(vx + 11, y, &pw.shown(), fg(BRIGHT));
                        p.text(vx + 11 + pw.cursor as u16 + 2, y, "enter to connect, esc to cancel", fg(MUTED));
                        self.cursor = Some((vx + 11 + pw.cursor as u16, y));
                        y += 1;
                    }
                }
            }
            "sound" => {
                label(p, y, "volume");
                let (v, muted) = self.settings.volume.unwrap_or((0, false));
                self.row_mark(p, lx, y, Act::Slider(Slider::Volume));
                p.slider(vx, y, 28, f32::from(v) / 100.0, Slider::Volume, if muted { MUTED } else { GREEN });
                p.text(vx + 30, y, &format!("{v}%"), fg(TEXT));
                y += 2;
                label(p, y, "sound");
                self.row_mark(p, lx, y, Act::Mute);
                p.link(vx, y, if muted { "[ ] on  [x] muted" } else { "[x] on  [ ] muted" }, Act::Mute, fg(TEXT));
                if self.settings.volume.is_none() {
                    p.text(vx, y + 2, "no sound server found (wpctl)", fg(MUTED));
                }
            }
            "display" => {
                label(p, y, "brightness");
                match self.settings.brightness {
                    Some(b) => {
                        self.row_mark(p, lx, y, Act::Slider(Slider::Brightness));
                        p.slider(vx, y, 28, f32::from(b) / 100.0, Slider::Brightness, GREEN);
                        p.text(vx + 30, y, &format!("{b}%"), fg(TEXT));
                    }
                    None => {
                        p.text(vx, y, "no adjustable screen found", fg(MUTED));
                    }
                }
            }
            "power" => {
                label(p, y, "battery");
                match self.snap.battery {
                    Some((pct, ch)) => {
                        p.bar(vx, y, 20, f32::from(pct) / 100.0, if pct < 20 { RED } else { GREEN });
                        p.text(vx + 22, y, &format!("{pct}%{}", if ch { " charging" } else { "" }), fg(TEXT));
                        if let Some(h) = self.snap.battery_hours {
                            p.text(vx + 22 + 12, y, &format!("{h:.1} h left"), fg(MUTED));
                        }
                    }
                    None => {
                        p.text(vx, y, "no battery", fg(MUTED));
                    }
                }
                y += 2;
                label(p, y, "profile");
                match self.settings.profile.clone() {
                    Some(cur) => {
                        let mut x = vx;
                        for (i, pr) in PROFILES.iter().enumerate() {
                            if i == 0 {
                                self.row_mark(p, lx, y, Act::PowerProfile(0));
                            }
                            x += p.button(x, y, pr, Act::PowerProfile(i), cur == *pr) + 1;
                        }
                    }
                    None => {
                        p.text(vx, y, "not available on this machine", fg(MUTED));
                    }
                }
            }
            "ai" => {
                label(p, y, "model");
                let (m, local) = crate::models::model_label();
                let w = p.text(vx, y, &m, fg(BRIGHT));
                p.text(vx + w + 1, y, if local { "local" } else { "cloud" }, fg(MUTED));
                self.row_mark(p, lx, y, Act::View(View::Models));
                p.button(vx + w + 8, y, "switch", Act::View(View::Models), false);
                y += 2;
                label(p, y, "ai asks before");
                let ask = tools::ask_before();
                let mut x = vx;
                for (i, (id, name)) in tools::KINDS.iter().enumerate() {
                    let on = ask.iter().any(|k| k == id);
                    if i == 0 {
                        self.row_mark(p, lx, y, Act::AskKind(0));
                    }
                    let t = format!("{} {name}", if on { "[x]" } else { "[ ]" });
                    x += p.link(x, y, &t, Act::AskKind(i), fg(if on { GREEN } else { TEXT })) + 2;
                    if x > inner.x + inner.width.saturating_sub(24) && i + 1 < tools::KINDS.len() {
                        x = vx;
                        y += 1;
                    }
                }
                y += 2;
                label(p, y, "web search");
                let cur = SEARCH.iter().position(|s| s.0 == self.settings.search.provider).unwrap_or(0);
                for (i, (_, name)) in SEARCH.iter().enumerate() {
                    if i == 0 {
                        self.row_mark(p, lx, y, Act::SearchProvider(0));
                    }
                    p.link(
                        vx,
                        y,
                        &format!("{} {name}", if i == cur { "(•)" } else { "( )" }),
                        Act::SearchProvider(i),
                        fg(if i == cur { GREEN } else { TEXT }),
                    );
                    y += 1;
                }
                if cur != 0 {
                    label(p, y, if cur == 2 { "server address" } else { "key" });
                    let shown = if cur == 2 { self.settings.search.url.clone() } else { self.settings.key.shown() };
                    p.text(vx, y, &shown, fg(if self.settings.key_focus { BRIGHT } else { TEXT }));
                    p.hit(Rect::new(vx, y, 40, 1), Act::Field(2));
                    if self.settings.key_focus {
                        self.cursor = Some((vx + self.settings.key.cursor as u16, y));
                    }
                    p.text(vx + 42, y, "click, type, enter", fg(MUTED));
                }
            }
            "dictation" => {
                label(p, y, "shortcut");
                p.text(vx, y, "super+m starts and stops", fg(TEXT));
                y += 2;
                label(p, y, "speech model");
                match dictate::model_path() {
                    Some(m) => {
                        p.text(vx, y, &m.display().to_string(), fg(GREEN));
                    }
                    None => {
                        p.text(vx, y, "not installed", fg(RED));
                    }
                }
                y += 2;
                label(p, y, "try it");
                self.row_mark(p, lx, y, Act::TestDictation);
                p.button(vx, y, "start dictation", Act::TestDictation, false);
            }
            "keyboard" => {
                label(p, y, "layout");
                let cur = sysx::keyboard_layout();
                let mut x = vx;
                for (i, l) in LAYOUTS.iter().enumerate() {
                    if i == 0 {
                        self.row_mark(p, lx, y, Act::KeyboardLayout(0));
                    }
                    x += p.button(x, y, l, Act::KeyboardLayout(i), *l == cur) + 1;
                }
            }
            "account" => {
                label(p, y, "user");
                p.text(vx, y, &format!("{}@{}", sysx::user(), sysx::hostname()), fg(BRIGHT));
                y += 2;
                label(p, y, "settings kept in");
                p.text(vx, y, &crate::paths::config_dir().display().to_string(), fg(TEXT));
            }
            _ => {
                label(p, y, "packages");
                self.row_mark(p, lx, y, Act::UpdateCheck);
                let w = p.button(
                    vx,
                    y,
                    if self.settings.updating { "checking…" } else { "check for updates" },
                    Act::UpdateCheck,
                    false,
                );
                p.text(vx + w + 2, y, &self.settings.message.clone(), fg(AMBER));
                y += 2;
                label(p, y, "install them");
                self.row_mark(p, lx, y, Act::UpdateRun);
                p.button(vx, y, "update everything", Act::UpdateRun, false);
                p.text(vx + 24, y, "the ai does it and asks first", fg(MUTED));
            }
        }
        if tab != "updates" && !self.settings.message.is_empty() {
            p.text(inner.x, inner.y + inner.height - 2, &self.settings.message, fg(AMBER));
        }
        let hint = "tab next · space toggle · ←→ adjust · click anything";
        p.text(
            inner.x + inner.width.saturating_sub(hint.chars().count() as u16),
            inner.y + inner.height - 1,
            hint,
            fg(MUTED),
        );
        let _ = Modifier::BOLD;
    }
}
