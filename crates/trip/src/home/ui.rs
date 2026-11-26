//! A tiny immediate-mode toolkit over ratatui's buffer: colours, titled panes, buttons, bars,
//! sliders. Everything that can be clicked registers a [`Hit`] while it's drawn, so the mouse
//! always matches what's on screen (and the layout can be tested without a terminal).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

pub const BG: Color = Color::Rgb(0x0a, 0x0d, 0x0c);
pub const TEXT: Color = Color::Rgb(0xcf, 0xd8, 0xd3);
pub const BRIGHT: Color = Color::Rgb(0xee, 0xf3, 0xef);
pub const MUTED: Color = Color::Rgb(0x6f, 0x7c, 0x77);
pub const BORDER: Color = Color::Rgb(0x2d, 0x3a, 0x36);
pub const GREEN: Color = Color::Rgb(0x46, 0xc3, 0x7b);
pub const AMBER: Color = Color::Rgb(0xe5, 0xb4, 0x5a);
pub const RED: Color = Color::Rgb(0xe0, 0x6c, 0x5a);
pub const BLUE: Color = Color::Rgb(0x6c, 0xb6, 0xe8);
pub const BAR: Color = Color::Rgb(0x11, 0x18, 0x16);
pub const EMPTY: Color = Color::Rgb(0x26, 0x30, 0x2d);
pub const SELECT: Color = Color::Rgb(0x13, 0x20, 0x1c);
pub const HOVER: Color = Color::Rgb(0x1a, 0x27, 0x23);

pub fn fg(c: Color) -> Style {
    Style::default().fg(c).bg(BG)
}

pub fn on(c: Color, bg: Color) -> Style {
    Style::default().fg(c).bg(bg)
}

pub const SPINNER: [&str; 4] = ["◐", "◓", "◑", "◒"];

/// The spinner frame at `ms` since start, 8 frames a second.
pub fn spinner(ms: u128) -> &'static str {
    SPINNER[(ms / 125 % 4) as usize]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Home,
    Ask,
    Files,
    Models,
    Settings,
    Packages,
}

impl View {
    pub const ALL: [View; 6] = [View::Home, View::Ask, View::Files, View::Models, View::Settings, View::Packages];

    pub fn name(self) -> &'static str {
        match self {
            View::Home => "home",
            View::Ask => "ask",
            View::Files => "files",
            View::Models => "models",
            View::Settings => "settings",
            View::Packages => "packages",
        }
    }

    pub fn index(self) -> usize {
        View::ALL.iter().position(|v| *v == self).unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    Clock,
    System,
    Today,
    Playing,
    Widget(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slider {
    Volume,
    Brightness,
    Seek,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollId {
    Ask,
    Models,
    Packages,
    Files,
    Settings,
}

/// What a click on something does.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Act {
    Noop,
    View(View),
    NewChat,
    Close(Pane),
    AddPane,
    PickPane(usize),
    Title(Pane),
    Mic,
    Send,
    FocusInput,
    // Ask
    Toggle(usize),
    Allow,
    AllowAlways,
    Deny,
    Stop,
    // Models
    ModelRow(usize),
    ModelUse(usize),
    ModelDownload(usize),
    ModelDelete(usize),
    ModelPause,
    Provider(usize),
    ProviderSave,
    ProviderCancel,
    LocalAgain,
    // Settings
    SetTab(usize),
    WifiToggle,
    WifiConnect(usize),
    WifiForget,
    WifiRescan,
    Mute,
    PowerProfile(usize),
    AskKind(usize),
    SearchProvider(usize),
    KeyboardLayout(usize),
    UpdateCheck,
    UpdateRun,
    TestDictation,
    // Packages
    PkgRow(usize),
    PkgGo(usize),
    PkgOpen(usize),
    PkgFocusSearch,
    // Files
    FileRow(usize),
    // Player
    Player(&'static str),
    // Status line
    Workspace(usize),
    Slider(Slider),
    ModalYes,
    ModalNo,
    Field(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    Click,
    /// Click or drag sets a value from the x position.
    Slider,
    /// Mouse wheel scrolls this.
    Scroll(ScrollId),
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub rect: Rect,
    pub act: Act,
    pub kind: HitKind,
}

pub struct Painter<'a> {
    pub buf: &'a mut Buffer,
    pub hits: &'a mut Vec<Hit>,
    pub hover: Option<Act>,
    pub pressed: Option<Act>,
    pub area: Rect,
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// The topmost hit under a point (hits drawn later sit on top).
pub fn hit_at(hits: &[Hit], x: u16, y: u16) -> Option<&Hit> {
    hits.iter().rev().find(|h| contains(h.rect, x, y) && !matches!(h.kind, HitKind::Scroll(_)))
}

pub fn scroll_at(hits: &[Hit], x: u16, y: u16) -> Option<ScrollId> {
    hits.iter().rev().find_map(|h| match h.kind {
        HitKind::Scroll(id) if contains(h.rect, x, y) => Some(id),
        _ => None,
    })
}

/// A slider's value (0 to 1) for a click at column `x`.
pub fn slider_value(rect: Rect, x: u16) -> f32 {
    if rect.width <= 1 {
        return 0.0;
    }
    (f32::from(x.saturating_sub(rect.x)) / f32::from(rect.width - 1)).clamp(0.0, 1.0)
}

impl Painter<'_> {
    /// Writes text clipped to the drawing area; returns the columns it took.
    pub fn text(&mut self, x: u16, y: u16, s: &str, st: Style) -> u16 {
        if y < self.area.y || y >= self.area.y + self.area.height || x >= self.area.x + self.area.width {
            return 0;
        }
        let room = (self.area.x + self.area.width - x) as usize;
        let mut shown = String::new();
        let mut w = 0;
        for c in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            if w + cw > room {
                break;
            }
            shown.push(c);
            w += cw;
        }
        self.buf.set_string(x, y, &shown, st);
        w as u16
    }

    pub fn fill(&mut self, r: Rect, bg: Color) {
        let r = r.intersection(self.area);
        for y in r.y..r.y + r.height {
            for x in r.x..r.x + r.width {
                if let Some(c) = self.buf.cell_mut((x, y)) {
                    c.set_bg(bg);
                }
            }
        }
    }

    pub fn hit(&mut self, rect: Rect, act: Act) {
        self.hits.push(Hit { rect: rect.intersection(self.area), act, kind: HitKind::Click });
    }

    pub fn scroll_area(&mut self, rect: Rect, id: ScrollId) {
        self.hits.push(Hit { rect, act: Act::Noop, kind: HitKind::Scroll(id) });
    }

    pub fn is_hover(&self, act: &Act) -> bool {
        self.hover.as_ref() == Some(act)
    }

    pub fn is_pressed(&self, act: &Act) -> bool {
        self.pressed.as_ref() == Some(act)
    }

    /// `[ label ]`, lit when `on`, brighter under the mouse, filled while pressed. Returns its width.
    pub fn button(&mut self, x: u16, y: u16, label: &str, act: Act, active: bool) -> u16 {
        let text = format!("[ {label} ]");
        let w = UnicodeWidthStr::width(text.as_str()) as u16;
        let style = if self.is_pressed(&act) {
            on(BG, GREEN).add_modifier(Modifier::BOLD)
        } else if self.is_hover(&act) {
            on(BRIGHT, HOVER).add_modifier(Modifier::BOLD)
        } else if active {
            on(GREEN, SELECT)
        } else {
            fg(TEXT)
        };
        self.text(x, y, &text, style);
        self.hit(Rect::new(x, y, w, 1), act);
        w
    }

    /// A flat label that is clickable (tabs, status segments).
    pub fn link(&mut self, x: u16, y: u16, label: &str, act: Act, st: Style) -> u16 {
        let hovered = self.is_hover(&act);
        let st = if hovered { st.fg(BRIGHT).add_modifier(Modifier::UNDERLINED) } else { st };
        let w = self.text(x, y, label, st);
        self.hit(Rect::new(x, y, w, 1), act);
        w
    }

    /// Full-width row highlight for hover or selection, with a hit over it.
    pub fn row(&mut self, rect: Rect, act: Act, selected: bool) {
        if selected {
            self.fill(rect, SELECT);
        } else if self.is_hover(&act) {
            self.fill(rect, HOVER);
        }
        self.hit(rect, act);
    }

    /// A pane with a titled border. `progress` (0 to 1) is how far its opening animation has got:
    /// corners first, then the edges, then the content. Returns the inner area once it's open.
    #[allow(clippy::too_many_arguments)]
    pub fn pane(
        &mut self,
        r: Rect,
        title: &str,
        focused: bool,
        color: Option<Color>,
        close: Option<Act>,
        title_act: Option<Act>,
        progress: f32,
    ) -> Option<Rect> {
        if r.width < 4 || r.height < 3 {
            return None;
        }
        let border = color.unwrap_or(if focused { GREEN } else { BORDER });
        let st = fg(border);
        let (x1, y1) = (r.x + r.width - 1, r.y + r.height - 1);
        self.text(r.x, r.y, "┌", st);
        self.text(x1, r.y, "┐", st);
        self.text(r.x, y1, "└", st);
        self.text(x1, y1, "┘", st);
        if progress < 0.34 {
            return None;
        }
        // Edges grow out from the corners.
        let reach = if progress >= 0.67 { 1.0 } else { (progress - 0.34) / 0.33 };
        let half_w = (f32::from(r.width.saturating_sub(2)) / 2.0 * reach).ceil() as u16;
        let half_h = (f32::from(r.height.saturating_sub(2)) / 2.0 * reach).ceil() as u16;
        for i in 0..half_w {
            for x in [r.x + 1 + i, x1.saturating_sub(1 + i)] {
                self.text(x, r.y, "─", st);
                self.text(x, y1, "─", st);
            }
        }
        for i in 0..half_h {
            for y in [r.y + 1 + i, y1.saturating_sub(1 + i)] {
                self.text(r.x, y, "│", st);
                self.text(x1, y, "│", st);
            }
        }
        if progress < 0.67 {
            return None;
        }
        if title.is_empty() {
            return Some(Rect::new(r.x + 2, r.y + 1, r.width - 4, r.height - 2));
        }
        let tcolor = color.unwrap_or(if focused { BRIGHT } else { TEXT });
        let label = format!("┤ {title} ├");
        let max = r.width.saturating_sub(10) as usize;
        let label: String = if UnicodeWidthStr::width(label.as_str()) > max {
            let t: String = title.chars().take(max.saturating_sub(5)).collect();
            format!("┤ {t}… ├")
        } else {
            label
        };
        let tw = UnicodeWidthStr::width(label.as_str()) as u16;
        self.text(r.x + 2, r.y, &label, fg(border));
        // The words inside the brackets are bright.
        self.text(
            r.x + 4,
            r.y,
            &label.chars().skip(2).take(label.chars().count().saturating_sub(4)).collect::<String>(),
            fg(tcolor),
        );
        if let Some(a) = title_act {
            let hovered = self.is_hover(&a);
            if hovered {
                self.text(
                    r.x + 4,
                    r.y,
                    &label.chars().skip(2).take(label.chars().count().saturating_sub(4)).collect::<String>(),
                    fg(BRIGHT).add_modifier(Modifier::UNDERLINED),
                );
            }
            self.hit(Rect::new(r.x + 2, r.y, tw, 1), a);
        }
        if let Some(a) = close {
            let (word, w) = match a {
                Act::Stop => ("[ stop ]", 8),
                _ => ("[×]", 3),
            };
            let x = x1.saturating_sub(w + 1);
            let st = if self.is_pressed(&a) {
                on(BG, RED)
            } else if self.is_hover(&a) {
                fg(RED).add_modifier(Modifier::BOLD)
            } else {
                fg(MUTED)
            };
            self.text(x, r.y, word, st);
            self.hit(Rect::new(x, r.y, w, 1), a);
        }
        Some(Rect::new(r.x + 2, r.y + 1, r.width - 4, r.height - 2))
    }

    /// A bar of `w` cells filled to `frac`, with eighth blocks for the partial cell.
    pub fn bar(&mut self, x: u16, y: u16, w: u16, frac: f32, color: Color) {
        let (full, part) = bar_cells(frac, w);
        let mut s = "█".repeat(full as usize);
        if let Some(p) = part {
            s.push(p);
        }
        let n = s.chars().count() as u16;
        self.text(x, y, &s, fg(color));
        if n < w {
            self.text(x + n, y, &"█".repeat((w - n) as usize), fg(EMPTY));
        }
    }

    /// A bar you can click or drag.
    pub fn slider(&mut self, x: u16, y: u16, w: u16, frac: f32, id: Slider, color: Color) {
        self.bar(x, y, w, frac, color);
        self.hits.push(Hit { rect: Rect::new(x, y, w, 1), act: Act::Slider(id), kind: HitKind::Slider });
    }
}

/// How a bar of `w` cells fills: whole cells, and an eighth-block for the rest.
pub fn bar_cells(frac: f32, w: u16) -> (u16, Option<char>) {
    const PARTS: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let total = (frac.clamp(0.0, 1.0) * f32::from(w) * 8.0).round() as u32;
    let full = (total / 8).min(u32::from(w)) as u16;
    let rem = (total % 8) as usize;
    (full, if rem > 0 && full < w { Some(PARTS[rem - 1]) } else { None })
}

/// Wi-Fi signal as `▂▄▆█` with `bars` lit.
pub fn signal(bars: u8) -> String {
    ["▂", "▄", "▆", "█"].iter().enumerate().map(|(i, b)| if (i as u8) < bars { *b } else { " " }).collect()
}

/// Word-wraps `text` to `width` columns (display width, not bytes).
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut w = 0;
        for word in para.split(' ') {
            let ww = UnicodeWidthStr::width(word);
            if w > 0 && w + 1 + ww > width {
                out.push(std::mem::take(&mut line));
                w = 0;
            }
            if ww > width {
                // A word longer than the line is cut.
                for c in word.chars() {
                    let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                    if w + cw > width {
                        out.push(std::mem::take(&mut line));
                        w = 0;
                    }
                    line.push(c);
                    w += cw;
                }
                continue;
            }
            if w > 0 {
                line.push(' ');
                w += 1;
            }
            line.push_str(word);
            w += ww;
        }
        out.push(line);
    }
    out
}

pub fn duration_words(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 { format!("{s}s") } else { format!("{}m {:02}s", s / 60, s % 60) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_fill_in_eighths() {
        assert_eq!(bar_cells(0.0, 10), (0, None));
        assert_eq!(bar_cells(1.0, 10), (10, None));
        assert_eq!(bar_cells(0.5, 10), (5, None));
        assert_eq!(bar_cells(0.55, 10), (5, Some('▌')));
        assert_eq!(bar_cells(2.0, 4), (4, None));
    }

    #[test]
    fn slider_maps_columns_to_values() {
        let r = Rect::new(10, 3, 21, 1);
        assert_eq!(slider_value(r, 10), 0.0);
        assert_eq!(slider_value(r, 30), 1.0);
        assert!((slider_value(r, 20) - 0.5).abs() < 0.01);
        assert_eq!(slider_value(r, 2), 0.0);
    }

    #[test]
    fn topmost_hit_wins() {
        let hits = vec![
            Hit { rect: Rect::new(0, 0, 10, 10), act: Act::Noop, kind: HitKind::Click },
            Hit { rect: Rect::new(2, 2, 3, 1), act: Act::Send, kind: HitKind::Click },
        ];
        assert_eq!(hit_at(&hits, 3, 2).map(|h| h.act.clone()), Some(Act::Send));
        assert_eq!(hit_at(&hits, 8, 8).map(|h| h.act.clone()), Some(Act::Noop));
        assert!(hit_at(&hits, 20, 20).is_none());
    }

    #[test]
    fn wraps_words_and_long_words() {
        assert_eq!(wrap("aaa bbb ccc", 8), vec!["aaa bbb", "ccc"]);
        assert_eq!(wrap("a\nb", 8), vec!["a", "b"]);
        assert_eq!(wrap("abcdefghijkl", 8), vec!["abcdefgh", "ijkl"]);
    }

    #[test]
    fn opening_pane_draws_corners_then_edges_then_content() {
        let area = Rect::new(0, 0, 20, 6);
        let mut buf = Buffer::empty(area);
        let mut hits = Vec::new();
        let mut p = Painter { buf: &mut buf, hits: &mut hits, hover: None, pressed: None, area };
        assert!(p.pane(area, "t", false, None, None, None, 0.1).is_none());
        let s: String = (0..20).map(|x| p.buf[(x, 0)].symbol().to_string()).collect();
        assert!(s.starts_with('┌') && s.ends_with('┐') && !s.contains('─'));
        assert!(p.pane(area, "t", false, None, None, None, 0.5).is_none());
        assert!(p.pane(area, "t", false, None, Some(Act::Close(Pane::Clock)), None, 1.0).is_some());
        assert!(!hits.is_empty());
    }
}
