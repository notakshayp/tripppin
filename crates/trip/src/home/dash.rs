//! The home dashboard: the pixel clock, system meters with the logo, today's calendar, panes the
//! AI made, what's playing, and the ask bar. Panes open by drawing their border, close with [×],
//! and swap places when you drag a title onto another pane.

use super::input::Input;
use super::ui::*;
use super::{App, Msg};
use crate::braille::{self, Ink};
use crate::sysx;
use crate::widgets::{self, Event};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::collections::HashMap;
use std::time::Instant;

pub const OPEN_MS: f32 = 120.0;

#[derive(Default)]
pub struct HomeState {
    /// Pane keys in the order they're laid out.
    pub order: Vec<String>,
    pub hidden: Vec<String>,
    pub born: HashMap<String, Instant>,
    pub input: Input,
    pub pane_rects: Vec<(Pane, Rect)>,
    pub drag: Option<Pane>,
    pub add_open: bool,
    pub events: Vec<Event>,
    pub events_day: String,
}

const DIGITS: [[&str; 5]; 10] = [
    ["11111", "10001", "10001", "10001", "11111"],
    ["00100", "01100", "00100", "00100", "01110"],
    ["11111", "00001", "11111", "10000", "11111"],
    ["11111", "00001", "11111", "00001", "11111"],
    ["10001", "10001", "11111", "00001", "00001"],
    ["11111", "10000", "11111", "00001", "11111"],
    ["11111", "10000", "11111", "10001", "11111"],
    ["11111", "00001", "00010", "00100", "00100"],
    ["11111", "10001", "11111", "10001", "11111"],
    ["11111", "10001", "11111", "00001", "11111"],
];

/// The big clock as lines of text: each pixel is two block cells wide, `sy` rows tall.
pub fn big_clock(hhmm: &str, sy: usize) -> Vec<String> {
    let mut rows = vec![String::new(); 5 * sy];
    for (n, ch) in hhmm.chars().enumerate() {
        if n > 0 {
            for r in rows.iter_mut() {
                r.push_str("  ");
            }
        }
        for (y, row) in rows.iter_mut().enumerate() {
            let src = y / sy;
            match ch.to_digit(10) {
                Some(d) => {
                    for c in DIGITS[d as usize][src].chars() {
                        row.push_str(if c == '1' { "██" } else { "  " });
                    }
                }
                None => row.push_str(if src == 1 || src == 3 { " ██ " } else { "    " }),
            }
        }
    }
    rows
}

pub fn pane_key(p: Pane, widgets: &[widgets::Widget]) -> String {
    match p {
        Pane::Clock => "clock".into(),
        Pane::System => "system".into(),
        Pane::Today => "today".into(),
        Pane::Playing => "playing".into(),
        Pane::Widget(i) => widgets.get(i).map_or("w:?".into(), |w| format!("w:{}", w.name)),
    }
}

pub fn key_pane(k: &str, widgets: &[widgets::Widget]) -> Option<Pane> {
    Some(match k {
        "clock" => Pane::Clock,
        "system" => Pane::System,
        "today" => Pane::Today,
        "playing" => Pane::Playing,
        _ => Pane::Widget(widgets.iter().position(|w| k.strip_prefix("w:") == Some(w.name.as_str()))?),
    })
}

/// Where each pane goes: two across the top, then rows of three.
pub fn layout(panes: &[Pane], area: Rect) -> Vec<(Pane, Rect)> {
    let n = panes.len();
    if n == 0 || area.height < 5 || area.width < 20 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let top_n = n.min(2);
    let rest = n - top_n;
    let top_h = if rest == 0 { area.height } else { (area.height * 42 / 100).clamp(9, 14).min(area.height - 6) };
    let split = |count: usize, y: u16, h: u16, first_wide: bool| -> Vec<Rect> {
        let gap = 1u16;
        let total = area.width.saturating_sub(gap * (count as u16 - 1));
        let widths: Vec<u16> = if count == 2 && first_wide {
            let a = total * 47 / 100;
            vec![a, total - a]
        } else {
            let each = total / count as u16;
            (0..count).map(|i| if i + 1 == count { total - each * (count as u16 - 1) } else { each }).collect()
        };
        let mut x = area.x;
        widths
            .into_iter()
            .map(|w| {
                let r = Rect::new(x, y, w, h);
                x += w + gap;
                r
            })
            .collect()
    };
    for (p, r) in panes.iter().take(top_n).zip(split(top_n, area.y, top_h, true)) {
        out.push((*p, r));
    }
    if rest > 0 {
        let rows = rest.div_ceil(3);
        let avail = area.height - top_h;
        let mut y = area.y + top_h;
        for (ri, chunk) in panes[top_n..].chunks(3).enumerate() {
            let h = if ri + 1 == rows { area.y + area.height - y } else { avail / rows as u16 };
            for (p, r) in chunk.iter().zip(split(chunk.len(), y, h, false)) {
                out.push((*p, r));
            }
            y += h;
        }
    }
    out
}

impl App {
    pub fn reload_widgets(&mut self) {
        self.widgets = widgets::list();
        let mut keys: Vec<String> = ["clock", "system", "today", "playing"].iter().map(|s| s.to_string()).collect();
        keys.extend(self.widgets.iter().map(|w| format!("w:{}", w.name)));
        let h = &mut self.home;
        h.order.retain(|k| keys.contains(k));
        for k in keys {
            if !h.order.contains(&k) && !h.hidden.contains(&k) {
                h.order.push(k);
            }
        }
        h.hidden.retain(|k| h.order.iter().all(|o| o != k));
        self.dirty = true;
    }

    pub fn visible_panes(&self) -> Vec<Pane> {
        self.home
            .order
            .iter()
            .filter(|k| !self.home.hidden.contains(k))
            .filter_map(|k| key_pane(k, &self.widgets))
            .collect()
    }

    pub fn close_pane(&mut self, p: Pane) {
        let k = pane_key(p, &self.widgets);
        self.home.hidden.push(k);
        self.save_layout();
        self.dirty = true;
    }

    pub fn add_pane(&mut self, i: usize) {
        let items = self.add_items();
        if let Some((k, _)) = items.get(i) {
            let k = k.clone();
            if k == "ask:new-widget" {
                self.home.input = Input::new("make a home widget that shows ");
                self.home.add_open = false;
                self.focus_input = true;
                self.view = View::Home;
            } else {
                self.home.hidden.retain(|h| *h != k);
                self.home.born.remove(&k);
                self.home.add_open = false;
                self.save_layout();
            }
        }
        self.dirty = true;
    }

    /// What the [+] chooser offers: panes that are closed, then the AI.
    pub fn add_items(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> =
            self.home.hidden.iter().map(|k| (k.clone(), format!("show {}", k.trim_start_matches("w:")))).collect();
        v.push(("ask:new-widget".into(), "ask the ai to make a new widget".into()));
        v
    }

    pub fn swap_panes(&mut self, a: Pane, b: Pane) {
        let (ka, kb) = (pane_key(a, &self.widgets), pane_key(b, &self.widgets));
        let o = &mut self.home.order;
        if let (Some(i), Some(j)) = (o.iter().position(|k| *k == ka), o.iter().position(|k| *k == kb)) {
            o.swap(i, j);
        }
        self.save_layout();
        self.dirty = true;
    }

    pub fn save_layout(&self) {
        let f = crate::paths::config_dir().join("home-layout");
        let _ = std::fs::create_dir_all(crate::paths::config_dir());
        let _ = std::fs::write(f, format!("{}\n--\n{}\n", self.home.order.join("\n"), self.home.hidden.join("\n")));
    }

    pub fn load_layout(&mut self) {
        let Ok(t) = std::fs::read_to_string(crate::paths::config_dir().join("home-layout")) else { return };
        let (order, hidden) = t.split_once("--\n").unwrap_or((&t, ""));
        self.home.order = order.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
        self.home.hidden = hidden.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
    }

    pub fn refresh_events(&mut self) {
        let ymd = self.now.ymd();
        if self.home.events_day != ymd || self.minute_changed {
            self.home.events = widgets::today(&ymd);
            self.home.events_day = ymd;
        }
    }

    pub fn home_send(&mut self) {
        let t = self.home.input.take();
        self.ask_send(&t);
    }

    pub fn home_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match k.code {
            KeyCode::Enter => self.home_send(),
            KeyCode::Esc if self.home.add_open => self.home.add_open = false,
            _ => {
                self.home.input.key(k);
            }
        }
        self.dirty = true;
    }

    pub fn draw_home(&mut self, p: &mut Painter, area: Rect) {
        let ask_h = 3u16;
        let panes_area = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(ask_h + 1));
        let panes = self.visible_panes();
        let rects = layout(&panes, panes_area);
        self.home.pane_rects = rects.clone();
        let now = Instant::now();
        for (pane, r) in rects {
            let key = pane_key(pane, &self.widgets);
            let born = *self.home.born.entry(key).or_insert(now);
            let progress = (now.duration_since(born).as_millis() as f32 / OPEN_MS).min(1.0);
            let title = match pane {
                Pane::Clock => "now".to_string(),
                Pane::System => "system".to_string(),
                Pane::Today => "today".to_string(),
                Pane::Playing => "playing".to_string(),
                Pane::Widget(i) => format!("{} · made by ai", self.widgets.get(i).map_or("", |w| w.name.as_str())),
            };
            let target = self.home.drag.is_some_and(|d| d != pane) && self.hover_pane == Some(pane);
            let color = target.then_some(AMBER);
            if let Some(inner) =
                p.pane(r, &title, false, color, Some(Act::Close(pane)), Some(Act::Title(pane)), progress)
            {
                self.draw_pane_body(p, pane, inner);
            }
        }
        if panes.is_empty() {
            p.text(area.x + 2, area.y + 1, "no panes; click [+] to add one", fg(MUTED));
        }
        // [+] add.
        let add = "[+] add";
        let ax = area.x + area.width.saturating_sub(add.len() as u16 + 1);
        p.link(ax, panes_area.y + panes_area.height, add, Act::AddPane, fg(MUTED));
        // The ask bar.
        let bar = Rect::new(area.x, area.y + area.height - ask_h, area.width, ask_h);
        if let Some(inner) = p.pane(bar, "", self.focus_input, None, None, None, 1.0) {
            let y = inner.y;
            p.text(inner.x, y, "✦ ask ›", fg(GREEN));
            let tx = inner.x + 8;
            if self.home.input.is_empty() {
                p.text(tx, y, "what do you want to do?", fg(MUTED));
            } else {
                let room = inner.width.saturating_sub(8 + 20) as usize;
                let skip = self.home.input.cursor.saturating_sub(room.saturating_sub(1));
                let vis: String = self.home.input.shown().chars().skip(skip).take(room).collect();
                p.text(tx, y, &vis, fg(BRIGHT));
            }
            if self.focus_input && self.view == View::Home && !self.home.add_open {
                let room = inner.width.saturating_sub(8 + 20) as usize;
                let skip = self.home.input.cursor.saturating_sub(room.saturating_sub(1));
                self.cursor = Some((tx + (self.home.input.cursor - skip) as u16, y));
            }
            p.hit(Rect::new(inner.x, y, inner.width.saturating_sub(18), 1), Act::FocusInput);
            let recording = self.pill.is_some();
            let bx = inner.x + inner.width - 17;
            let w = p.button(bx, y, if recording { "● rec" } else { "● mic" }, Act::Mic, recording);
            p.button(bx + w + 1, y, "↵", Act::Send, false);
        }
        if self.home.add_open {
            self.draw_add(p, area);
        }
    }

    fn draw_add(&mut self, p: &mut Painter, area: Rect) {
        let items = self.add_items();
        let w = 44.min(area.width);
        let h = (items.len() as u16 + 4).min(area.height);
        let r = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
        for y in r.y..r.y + r.height {
            p.text(r.x, y, &" ".repeat(r.width as usize), fg(TEXT));
        }
        if let Some(inner) = p.pane(r, "add", true, None, None, None, 1.0) {
            for (i, (_, label)) in items.iter().enumerate() {
                let y = inner.y + i as u16;
                p.row(Rect::new(inner.x, y, inner.width, 1), Act::PickPane(i), false);
                p.text(inner.x, y, label, fg(TEXT));
            }
            p.text(inner.x, inner.y + inner.height - 1, "esc closes", fg(MUTED));
        }
    }

    fn draw_pane_body(&mut self, p: &mut Painter, pane: Pane, inner: Rect) {
        let ms = self.started.elapsed().as_millis();
        match pane {
            Pane::Clock => {
                let sy = if inner.height >= 12 { 2 } else { 1 };
                let rows = big_clock(&self.now.hhmm(), sy);
                let w = rows.first().map_or(0, |r| r.chars().count() as u16);
                let x = if inner.width > w + 2 { inner.x + 1 } else { inner.x };
                for (i, r) in rows.iter().enumerate() {
                    p.text(x, inner.y + 1 + i as u16, r, fg(BRIGHT));
                }
                let y = inner.y + 1 + rows.len() as u16 + 1;
                p.text(x, y, &self.now.long_date(), fg(MUTED));
            }
            Pane::System => {
                let cols = if inner.height >= 9 { 12 } else { 8 };
                let logo = braille::braille(cols, 1.0, 1.0);
                for (r, line) in logo.iter().enumerate() {
                    for (c, (ch, ink)) in line.iter().enumerate() {
                        let color = match ink {
                            Ink::Spark => GREEN,
                            Ink::Ring => BRIGHT,
                            Ink::Blank => continue,
                        };
                        p.text(inner.x + c as u16, inner.y + r as u16, &ch.to_string(), fg(color));
                    }
                }
                let x = inner.x + cols as u16 + 3;
                let bw = 10u16;
                let s = &self.snap;
                let mut y = inner.y;
                let label = |p: &mut Painter, y: u16, l: &str| {
                    p.text(x, y, &format!("{l:<8}"), fg(MUTED));
                };
                label(p, y, "battery");
                match s.battery {
                    Some((pct, ch)) => {
                        p.bar(x + 8, y, bw, f32::from(pct) / 100.0, if pct < 20 && !ch { RED } else { GREEN });
                        let t = match s.battery_hours {
                            Some(h) => format!(" {pct}%  {h:.0} h"),
                            None => format!(" {pct}%{}", if ch { " ⚡" } else { "" }),
                        };
                        p.text(x + 8 + bw, y, &t, fg(TEXT));
                    }
                    None => {
                        p.text(x + 8, y, "plugged in", fg(TEXT));
                    }
                }
                y += 1;
                label(p, y, "wi-fi");
                match &self.wifi {
                    Some(n) => {
                        let w = p.text(x + 8, y, n, fg(TEXT));
                        p.text(x + 9 + w, y, &signal(4), fg(GREEN));
                    }
                    None => {
                        p.text(x + 8, y, "not connected", fg(MUTED));
                    }
                }
                y += 1;
                label(p, y, "cpu");
                p.bar(x + 8, y, bw, f32::from(s.cpu_percent) / 100.0, GREEN);
                p.text(x + 8 + bw, y, &format!(" {}%", s.cpu_percent), fg(TEXT));
                y += 1;
                label(p, y, "memory");
                let (u, t) = (s.mem_used_kib as f32, s.mem_total_kib.max(1) as f32);
                p.bar(x + 8, y, bw, u / t, if u / t > 0.9 { AMBER } else { GREEN });
                p.text(x + 8 + bw, y, &format!(" {:.1}/{:.0} G", u / 1048576.0, t / 1048576.0), fg(TEXT));
                y += 1;
                label(p, y, "model");
                let (m, local) = crate::models::model_label();
                let w = p.link(x + 8, y, &m, Act::View(View::Models), fg(TEXT));
                p.text(x + 9 + w, y, if local { "· local" } else { "· cloud" }, fg(MUTED));
                y += 1;
                label(p, y, "disk");
                p.text(
                    x + 8,
                    y,
                    &format!(
                        "{} used of {}",
                        crate::apk::size_words(s.disk_used),
                        crate::apk::size_words(s.disk_total)
                    ),
                    fg(TEXT),
                );
            }
            Pane::Today => {
                let colors = [BLUE, GREEN, AMBER];
                if self.home.events.is_empty() {
                    p.text(inner.x, inner.y, "nothing planned today", fg(MUTED));
                }
                let mut y = inner.y;
                for (i, e) in self.home.events.iter().enumerate().take(inner.height.saturating_sub(2) as usize / 2) {
                    let t = if e.time.is_empty() { "all day".to_string() } else { e.time.clone() };
                    p.text(inner.x, y, &format!("{t:<6} ┃"), fg(MUTED));
                    p.text(inner.x + 9, y, &e.title, fg(colors[i % 3]));
                    p.text(inner.x, y + 1, "       ┃", fg(MUTED));
                    y += 2;
                }
                p.button(inner.x, inner.y + inner.height - 1, "+ add", Act::Field(9), false);
            }
            Pane::Playing => match &self.playing {
                None => {
                    p.text(inner.x, inner.y, "nothing playing", fg(MUTED));
                    p.text(inner.x, inner.y + 1, "start music in any app", fg(MUTED));
                }
                Some(pl) => {
                    let pos = if pl.playing {
                        (pl.position + self.playing_at.elapsed().as_secs_f64()).min(pl.length.max(pl.position))
                    } else {
                        pl.position
                    };
                    p.text(inner.x, inner.y, &pl.title, fg(BRIGHT));
                    p.text(
                        inner.x,
                        inner.y + 1,
                        &format!("{} · {} / {}", pl.artist, sysx::mmss(pos), sysx::mmss(pl.length)),
                        fg(MUTED),
                    );
                    let bw = inner.width.min(30);
                    p.slider(
                        inner.x,
                        inner.y + 3,
                        bw,
                        if pl.length > 0.0 { (pos / pl.length) as f32 } else { 0.0 },
                        Slider::Seek,
                        GREEN,
                    );
                    let mut x = inner.x;
                    for (label, cmd) in
                        [("⏮", "previous"), (if pl.playing { "⏸" } else { "▶" }, "play-pause"), ("⏭", "next")]
                    {
                        x += p.button(x, inner.y + 5, label, Act::Player(cmd), cmd == "play-pause") + 1;
                    }
                    if pl.playing && inner.height > 7 {
                        const B: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
                        let n = ms / 125;
                        let eq: String = (0..16u128).map(|i| B[((i * 7 + n * 3 + (i * n) % 5) % 8) as usize]).collect();
                        p.text(inner.x, inner.y + 7, &eq, fg(AMBER));
                    }
                }
            },
            Pane::Widget(i) => {
                let Some(w) = self.widgets.get(i) else { return };
                let st = self.runner.states.get(&w.name);
                let text = st.map_or("", |s| s.text.as_str());
                if text.is_empty() {
                    p.text(inner.x, inner.y, &format!("{} loading", spinner(ms)), fg(AMBER));
                }
                for (n, l) in text.lines().take(inner.height.saturating_sub(1) as usize).enumerate() {
                    p.text(inner.x, inner.y + n as u16, l, fg(if n == 0 { BRIGHT } else { TEXT }));
                }
                if let Some(s) = st.filter(|s| !s.updated_at.is_empty()) {
                    p.text(inner.x, inner.y + inner.height - 1, &format!("updated {}", s.updated_at), fg(MUTED));
                }
            }
        }
        let _ = Modifier::BOLD;
    }
}

/// Starts the widget runner thread for one due widget.
pub fn run_widget(tx: std::sync::mpsc::Sender<Msg>, w: widgets::Widget, at: String) {
    std::thread::spawn(move || {
        let text = widgets::run_command(&w.command, std::time::Duration::from_secs(10));
        let _ = tx.send(Msg::Widget(w.name, text, at));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_is_five_rows_of_blocks() {
        let rows = big_clock("21:14", 1);
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().all(|r| r.chars().count() == rows[0].chars().count()));
        assert_eq!(big_clock("00:00", 2).len(), 10);
        assert!(rows[0].starts_with("██████████")); // the 2
    }

    #[test]
    fn lays_out_two_on_top_then_rows_of_three() {
        let area = Rect::new(0, 1, 120, 30);
        let panes = [Pane::Clock, Pane::System, Pane::Today, Pane::Widget(0), Pane::Playing];
        let l = layout(&panes, area);
        assert_eq!(l.len(), 5);
        assert_eq!(l[0].1.y, 1);
        assert_eq!(l[0].1.y, l[1].1.y);
        assert!(l[0].1.width < l[1].1.width);
        assert_eq!(l[2].1.y, l[3].1.y);
        assert_eq!(l[3].1.y, l[4].1.y);
        assert!(l[2].1.y >= l[0].1.y + l[0].1.height);
        // Nothing overlaps and everything is inside the area.
        for (i, (_, a)) in l.iter().enumerate() {
            assert!(a.x + a.width <= area.width && a.y + a.height <= area.y + area.height);
            for (_, b) in &l[i + 1..] {
                assert!(a.intersection(*b).is_empty(), "{a:?} overlaps {b:?}");
            }
        }
        assert_eq!(layout(&[Pane::Clock], area)[0].1.height, 30);
        assert!(layout(&[], area).is_empty());
    }

    #[test]
    fn pane_keys_round_trip() {
        let ws = vec![widgets::Widget { name: "weather".into(), command: "x".into(), every_secs: 60 }];
        for p in [Pane::Clock, Pane::System, Pane::Today, Pane::Playing, Pane::Widget(0)] {
            assert_eq!(key_pane(&pane_key(p, &ws), &ws), Some(p));
        }
        assert_eq!(key_pane("w:gone", &ws), None);
    }
}
