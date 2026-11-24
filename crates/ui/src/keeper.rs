//! Keeps floating windows usable, and snaps windows dragged against the screen's edges.
//!
//! Every window floats on Tripppin, and niri lets a floating window go anywhere, so this
//! watches niri's event stream and, once windows have settled for a moment:
//!
//! - a window that was dragged (moved without changing size) until it went past an edge snaps
//!   there, like Rectangle on the Mac: left or right edge a half, top edge fill, corners a
//!   quarter (snap.rs has the geometry). While it's held there, a translucent preview shows
//!   where it will go.
//! - a snapped window dragged away gets back the size it had before it was snapped.
//! - otherwise a window is never left under the bar (where its title bar can't be grabbed),
//!   bigger than the screen, or dragged entirely off it.
//!
//! niri's IPC has no pointer position and no "drag started/ended" signal (checked against
//! niri-ipc 26.04: only window, workspace, layout and focus events), so a drag is recognised
//! from a run of WindowLayoutsChanged events that move a window without resizing it, and its
//! end from the events stopping for a moment.
//!
//! Run from the orb, which is always there.

use crate::snap::{self, Rect};
use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

/// The bar's reserved height (its height plus margin in /etc/xdg/waybar) and niri's gaps.
const BAR: f64 = snap::BAR as f64;
const GAP: f64 = snap::GAP as f64;
/// How much of a window must stay on screen sideways, so it can be grabbed again.
const KEEP_VISIBLE: f64 = 80.0;
/// How long windows must stay put before anything happens, so a drag isn't fought.
const SETTLE: Duration = Duration::from_millis(500);

/// A floating window as niri reports it: its tile's top-left in the output's view and its size.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: u64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Placed {
    /// As snap.rs measures it: rounded, `y` from the top of the area below the bar.
    pub fn rect(&self) -> Rect {
        Rect::new(self.x.round() as i32, (self.y - BAR).round() as i32, self.w.round() as i32, self.h.round() as i32)
    }
}

/// Whether going from `a` to `b` is a move by dragging: the place changed, the size didn't.
pub fn dragged(a: &Placed, b: &Placed) -> bool {
    let moved = (a.x - b.x).abs() >= 1.0 || (a.y - b.y).abs() >= 1.0;
    let same_size = (a.w - b.w).abs() < 1.0 && (a.h - b.h).abs() < 1.0;
    moved && same_size
}

/// The niri actions that bring a window back into the area below the bar on a `sw` x `sh`
/// screen. Positions for move-floating-window are measured from the area below the bar.
pub fn corrections(win: &Placed, sw: f64, sh: f64) -> Vec<Vec<String>> {
    let (area_w, area_h) = (sw - 2.0 * GAP, sh - BAR - 2.0 * GAP);
    let id = win.id.to_string();
    let mut out = Vec::new();
    let (mut w, mut h) = (win.w, win.h);
    if w > area_w + 1.0 {
        w = area_w;
        out.push(vec!["set-window-width".into(), "--id".into(), id.clone(), format!("{}", area_w.round())]);
    }
    if h > area_h + 1.0 {
        h = area_h;
        out.push(vec!["set-window-height".into(), "--id".into(), id.clone(), format!("{}", area_h.round())]);
    }
    // Where it should be, in the output's view.
    let mut x = win.x;
    let mut y = win.y;
    if y < BAR {
        y = BAR + GAP;
    }
    if y + h > sh {
        y = (sh - h - GAP).max(BAR + GAP);
    }
    if w < win.w {
        x = GAP;
    } else if x + w < KEEP_VISIBLE {
        x = KEEP_VISIBLE - w;
    } else if x > sw - KEEP_VISIBLE {
        x = sw - KEEP_VISIBLE;
    }
    if (x - win.x).abs() >= 1.0 || (y - win.y).abs() >= 1.0 {
        out.push(vec![
            "move-floating-window".into(),
            "--id".into(),
            id,
            "-x".into(),
            // niri reads "-40" as "40 to the left", so a place left of the screen goes as
            // a move from where it is.
            if x < 0.0 { format!("{:+}", (x - win.x).round()) } else { format!("{}", x.round()) },
            "-y".into(),
            format!("{}", (y - BAR).round()),
        ]);
    }
    out
}

/// One window object from niri's IPC, with its workspace (None if not floating).
fn parse_window(w: &Value) -> Option<(Placed, u64)> {
    if w["is_floating"].as_bool() != Some(true) {
        return None;
    }
    Some((parse_layout(w["id"].as_u64()?, &w["layout"])?, w["workspace_id"].as_u64()?))
}

fn parse_layout(id: u64, layout: &Value) -> Option<Placed> {
    let pos = layout["tile_pos_in_workspace_view"].as_array()?;
    let size = layout["tile_size"].as_array()?;
    Some(Placed {
        id,
        x: pos.first()?.as_f64()?,
        y: pos.get(1)?.as_f64()?,
        w: size.first()?.as_f64()?,
        h: size.get(1)?.as_f64()?,
    })
}

/// Floating windows and their workspaces, from `niri msg -j windows`.
pub fn parse_floating(json: &str) -> Vec<(Placed, u64)> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    items.iter().filter_map(parse_window).collect()
}

/// What one line of `niri msg -j event-stream` says about floating windows' places: each with
/// its workspace when the event names it. `None` for events about something else.
pub fn parse_event(line: &str) -> Option<Vec<(Placed, Option<u64>)>> {
    let v: Value = serde_json::from_str(line).ok()?;
    if let Some(changes) = v["WindowLayoutsChanged"]["changes"].as_array() {
        return Some(
            changes.iter().filter_map(|c| Some((parse_layout(c.get(0)?.as_u64()?, c.get(1)?)?, None))).collect(),
        );
    }
    if let Some(w) = v.get("WindowOpenedOrChanged") {
        return Some(parse_window(&w["window"]).map(|(p, ws)| (p, Some(ws))).into_iter().collect());
    }
    if let Some(ws) = v["WindowsChanged"]["windows"].as_array() {
        return Some(ws.iter().filter_map(parse_window).map(|(p, ws)| (p, Some(ws))).collect());
    }
    None
}

fn niri(args: &[&str]) -> Option<String> {
    let out = Command::new("niri").arg("msg").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A workspace's output: its connector name and logical size.
#[derive(Clone, Debug)]
struct Screen {
    ws: u64,
    output: String,
    w: f64,
    h: f64,
}

/// Each workspace's output, from `niri msg -j workspaces` and `outputs`.
fn screens() -> Vec<Screen> {
    let ws: Value = niri(&["-j", "workspaces"]).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let outs: Value = niri(&["-j", "outputs"]).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    ws.as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let name = w["output"].as_str()?;
            let logical = &outs[name]["logical"];
            Some(Screen {
                ws: w["id"].as_u64()?,
                output: name.to_string(),
                w: logical["width"].as_f64()?,
                h: logical["height"].as_f64()?,
            })
        })
        .collect()
}

/// What the keeper knows between events.
#[derive(Default)]
struct Keeper {
    /// Each floating window's place when things last settled, and its workspace.
    settled: HashMap<u64, (Placed, u64)>,
    /// Windows that changed since things last settled: where they started, where they are.
    moving: HashMap<u64, (Placed, Placed)>,
    screens: Vec<Screen>,
    preview: Option<gtk::Window>,
    /// What the preview shows now: the output and the spot.
    showing: Option<(String, &'static str)>,
    pending: Option<glib::SourceId>,
}

impl Keeper {
    fn screen_of(&self, id: u64) -> Option<&Screen> {
        let ws = self.settled.get(&id)?.1;
        self.screens.iter().find(|s| s.ws == ws)
    }

    /// A window moved or changed: remember where it started, and show where it would snap.
    fn changed(&mut self, now: Placed, ws: Option<u64>) {
        let id = now.id;
        if let Some(ws) = ws {
            let start = self.settled.get(&id).map_or_else(|| now.clone(), |(p, _)| p.clone());
            self.settled.insert(id, (start, ws));
        }
        let start = self.settled.get(&id).map_or_else(|| now.clone(), |(p, _)| p.clone());
        let entry = self.moving.entry(id).or_insert_with(|| (start, now.clone()));
        entry.1 = now.clone();
        let spot = if dragged(&entry.0, &now) {
            self.screen_of(id).and_then(|s| {
                snap::edge_spot(now.x, now.y, now.w, s.w, s.h).map(|spot| (s.output.clone(), spot, s.w, s.h))
            })
        } else {
            None
        };
        match spot {
            Some((output, spot, sw, sh)) => self.show_preview(&output, spot, sw as i32, sh as i32),
            None => self.hide_preview(),
        }
    }

    fn show_preview(&mut self, output: &str, spot: &'static str, sw: i32, sh: i32) {
        if self.showing.as_ref().is_some_and(|(o, s)| o == output && *s == spot) {
            return;
        }
        let Some(r) = snap::spot_rect(spot, 0, sw, sh) else { return };
        let window = self.preview.get_or_insert_with(preview_window).clone();
        // Hidden while it's resized and moved, so it never shows at the old size.
        window.set_visible(false);
        if let Some(m) = crate::common::monitor_named(output) {
            window.set_monitor(Some(&m));
        }
        window.set_margin(Edge::Left, r.x);
        window.set_margin(Edge::Top, snap::BAR + r.y);
        if let Some(child) = window.child() {
            child.set_size_request(r.w, r.h);
        }
        window.set_default_size(r.w, r.h);
        window.set_visible(true);
        self.showing = Some((output.to_string(), spot));
    }

    fn hide_preview(&mut self) {
        if let Some(w) = &self.preview {
            w.set_visible(false);
        }
        self.showing = None;
    }

    /// Things have settled: snap what was dragged against an edge, give a snapped window
    /// dragged away its old size back, and bring anything out of reach back on screen.
    fn settle(&mut self) {
        self.hide_preview();
        let moving = std::mem::take(&mut self.moving);
        let Some(json) = niri(&["-j", "windows"]) else { return };
        self.screens = screens();
        let windows = parse_floating(&json);
        let mut memory = snap::load_memory();
        let before = memory.len();
        memory.retain(|id, _| windows.iter().any(|(w, _)| w.id == *id));
        if memory.len() != before {
            snap::save_memory(&memory);
        }
        for (win, ws) in &windows {
            let Some(screen) = self.screens.iter().find(|s| s.ws == *ws) else { continue };
            let (sw, sh) = (screen.w, screen.h);
            let mut win = win.clone();
            if let Some((start, _)) = moving.get(&win.id).filter(|(start, _)| dragged(start, &win)) {
                if let Some(spot) = snap::edge_spot(win.x, win.y, win.w, sw, sh) {
                    snap::snap_window(win.id, start.rect(), spot, sw as i32, sh as i32, false);
                    continue;
                }
                // Dragged out of a snapped spot: back to the size from before it was snapped.
                if let Some(m) = memory.get(&win.id)
                    && !m.snapped.near(&win.rect())
                    && m.snapped.same_size(&win.rect())
                {
                    let id = win.id.to_string();
                    niri(&["action", "set-window-width", "--id", &id, &m.before.w.to_string()]);
                    niri(&["action", "set-window-height", "--id", &id, &m.before.h.to_string()]);
                    win.w = f64::from(m.before.w);
                    win.h = f64::from(m.before.h);
                    memory.remove(&win.id);
                    snap::save_memory(&memory);
                }
            }
            for action in corrections(&win, sw, sh) {
                let mut args = vec!["action"];
                args.extend(action.iter().map(String::as_str));
                niri(&args);
            }
        }
        // Where everything is now is where the next drag starts from. (Snaps and fixes just
        // sent arrive as events and are folded in the same way.)
        self.settled = windows.into_iter().map(|(p, ws)| (p.id, (p, ws))).collect();
    }
}

/// The translucent rectangle that shows where a dragged window will snap. It never takes
/// the pointer.
fn preview_window() -> gtk::Window {
    let window = gtk::Window::builder().decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("clear");
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-snap-preview"));
    window.set_layer(Layer::Top);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Left, true);
    window.set_exclusive_zone(-1);
    let area = gtk::Box::new(gtk::Orientation::Vertical, 0);
    area.add_css_class("snap-preview");
    window.set_child(Some(&area));
    window.connect_map(|w| {
        if let Some(surface) = w.surface() {
            surface.set_input_region(Some(&gtk::cairo::Region::create()));
        }
    });
    window
}

pub fn start() {
    let (tx, rx) = async_channel::unbounded::<Vec<(Placed, Option<u64>)>>();
    std::thread::spawn(move || {
        let Ok(mut child) = Command::new("niri")
            .args(["msg", "-j", "event-stream"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let Some(out) = child.stdout.take() else { return };
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(changes) = parse_event(&line) {
                let _ = tx.send_blocking(changes);
            }
        }
    });
    let keeper: Rc<RefCell<Keeper>> = Rc::new(RefCell::new(Keeper { screens: screens(), ..Default::default() }));
    glib::spawn_future_local(async move {
        while let Ok(changes) = rx.recv().await {
            let mut k = keeper.borrow_mut();
            for (placed, ws) in changes {
                k.changed(placed, ws);
            }
            // Each change restarts the wait, so a window being dragged is left alone until
            // let go.
            if let Some(old) = k.pending.take() {
                old.remove();
            }
            let slot = keeper.clone();
            k.pending = Some(glib::timeout_add_local_once(SETTLE, move || {
                let mut k = slot.borrow_mut();
                k.pending = None;
                k.settle();
            }));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, y: f64, w: f64, h: f64) -> Placed {
        Placed { id: 7, x, y, w, h }
    }

    #[test]
    fn leaves_a_good_window_alone() {
        assert!(corrections(&at(100.0, 100.0, 600.0, 400.0), 1280.0, 800.0).is_empty());
    }

    #[test]
    fn brings_a_window_out_from_under_the_bar() {
        let fix = corrections(&at(200.0, 10.0, 600.0, 400.0), 1280.0, 800.0);
        assert_eq!(fix, [["move-floating-window", "--id", "7", "-x", "200", "-y", "16"]]);
    }

    #[test]
    fn shrinks_a_window_wider_than_the_screen() {
        let fix = corrections(&at(-40.0, 60.0, 1900.0, 500.0), 1280.0, 800.0);
        assert_eq!(fix[0], ["set-window-width", "--id", "7", "1248"]);
        assert_eq!(fix[1], ["move-floating-window", "--id", "7", "-x", "16", "-y", "20"]);
    }

    #[test]
    fn pulls_back_a_window_dragged_off_the_side() {
        let fix = corrections(&at(1250.0, 200.0, 500.0, 300.0), 1280.0, 800.0);
        assert_eq!(fix, [["move-floating-window", "--id", "7", "-x", "1200", "-y", "160"]]);
    }

    #[test]
    fn pulls_back_a_window_dragged_off_the_left_as_a_relative_move() {
        // Wanted at x = 80 - 500 = -420, which niri would read as a move; so: +180, to the right.
        let fix = corrections(&at(-600.0, 200.0, 500.0, 300.0), 1280.0, 800.0);
        assert_eq!(fix, [["move-floating-window", "--id", "7", "-x", "+180", "-y", "160"]]);
    }

    #[test]
    fn reads_floating_windows() {
        let json = r#"[{"id":3,"is_floating":true,"workspace_id":1,"layout":{"tile_pos_in_workspace_view":[10.0,20.0],"tile_size":[300.0,200.0]}},
                       {"id":4,"is_floating":false,"workspace_id":1,"layout":{"tile_pos_in_workspace_view":null,"tile_size":[1,1]}}]"#;
        assert_eq!(parse_floating(json), [(at(10.0, 20.0, 300.0, 200.0).with_id(3), 1)]);
    }

    #[test]
    fn reads_events() {
        let moved = r#"{"WindowLayoutsChanged":{"changes":[[3,{"pos_in_scrolling_layout":null,"tile_size":[300.0,200.0],"window_size":[300,200],"tile_pos_in_workspace_view":[-40.0,90.0],"window_offset_in_tile":[0.0,0.0]}]]}}"#;
        assert_eq!(parse_event(moved), Some(vec![(at(-40.0, 90.0, 300.0, 200.0).with_id(3), None)]));
        let opened = r#"{"WindowOpenedOrChanged":{"window":{"id":3,"is_floating":true,"workspace_id":2,"layout":{"tile_pos_in_workspace_view":[1.0,2.0],"tile_size":[3.0,4.0]}}}}"#;
        assert_eq!(parse_event(opened), Some(vec![(at(1.0, 2.0, 3.0, 4.0).with_id(3), Some(2))]));
        assert_eq!(parse_event(r#"{"WindowFocusChanged":{"id":3}}"#), None);
    }

    #[test]
    fn a_drag_moves_without_resizing() {
        let a = at(100.0, 100.0, 600.0, 400.0);
        assert!(dragged(&a, &at(-80.0, 120.0, 600.0, 400.0)));
        // Resizing (super + right-drag, or a shortcut that resizes and moves) isn't a drag.
        assert!(!dragged(&a, &at(16.0, 56.0, 616.0, 728.0)));
        assert!(!dragged(&a, &a));
        assert_eq!(at(16.0, 56.0, 616.0, 728.0).rect(), Rect::new(16, 16, 616, 728));
    }

    impl Placed {
        fn with_id(mut self, id: u64) -> Self {
            self.id = id;
            self
        }
    }
}
