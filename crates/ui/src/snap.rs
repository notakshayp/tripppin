//! Window snapping, like Rectangle on the Mac. Every window floats on Tripppin, so niri's own
//! column layout doesn't apply: `tripppin-ui snap <where>` sizes and places the focused window
//! itself, and the keeper (keeper.rs) does the same when a window is dragged against an edge.
//!
//! - halves: `left`, `right`, `top`, `bottom`. Asking for the same half again cycles its size
//!   through 1/2, 1/3 and 2/3 of the screen.
//! - quarters: `top-left`, `top-right`, `bottom-left`, `bottom-right`.
//! - `fill`: the whole area below the bar; `max`: niri's maximise (again to undo).
//! - `centre`: the same size, in the middle.
//! - `restore`: back to the size and place it had before it was first snapped.
//!
//! What a window looked like before its first snap is remembered per window id in the runtime
//! directory, shared by the keyboard shortcuts and the keeper.

use gtk::glib;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

/// The bar's reserved height (its margin plus height in /etc/xdg/waybar) and niri's gaps.
pub const BAR: i32 = 40;
pub const GAP: i32 = 16;
/// How far short of a snapped size an app may stop (a character cell, a few lines).
const SIZE_SLACK: i32 = 40;

/// The widths (or heights) a half cycles through when its shortcut is pressed again.
const FRACTIONS: [(i32, i32); 3] = [(1, 2), (1, 3), (2, 3)];

/// A window's size and place. `y` is measured from the top of the area below the bar, as
/// niri's move-floating-window takes it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    /// The same place, and the same size give or take what an app rounds it to: terminals
    /// size themselves in whole characters, so a snapped foot is a few pixels short.
    pub fn near(&self, o: &Rect) -> bool {
        (self.x - o.x).abs() <= 2 && (self.y - o.y).abs() <= 2 && self.same_size(o)
    }

    pub fn same_size(&self, o: &Rect) -> bool {
        (self.w - o.w).abs() <= SIZE_SLACK && (self.h - o.h).abs() <= SIZE_SLACK
    }
}

/// Whether a spot is a half, whose size cycles when it's asked for again.
pub fn is_half(spot: &str) -> bool {
    matches!(spot, "left" | "right" | "top" | "bottom")
}

/// Where a spot is on a screen of `sw` x `sh` (logical pixels, bar included). `step` picks
/// a half's size from 1/2, 1/3, 2/3; other spots ignore it.
pub fn spot_rect(spot: &str, step: usize, sw: i32, sh: i32) -> Option<Rect> {
    let area_h = sh - BAR;
    let (usable_w, usable_h) = (sw - 3 * GAP, area_h - 3 * GAP);
    let (full_w, full_h) = (sw - 2 * GAP, area_h - 2 * GAP);
    let (n, d) = FRACTIONS[step % FRACTIONS.len()];
    let (part_w, part_h) = (usable_w * n / d, usable_h * n / d);
    let (half_w, half_h) = (usable_w / 2, usable_h / 2);
    let right = |w: i32| sw - GAP - w;
    let lower = |h: i32| area_h - GAP - h;
    Some(match spot {
        "left" => Rect::new(GAP, GAP, part_w, full_h),
        "right" => Rect::new(right(part_w), GAP, part_w, full_h),
        "top" => Rect::new(GAP, GAP, full_w, part_h),
        "bottom" => Rect::new(GAP, lower(part_h), full_w, part_h),
        "top-left" => Rect::new(GAP, GAP, half_w, half_h),
        "top-right" => Rect::new(right(half_w), GAP, half_w, half_h),
        "bottom-left" => Rect::new(GAP, lower(half_h), half_w, half_h),
        "bottom-right" => Rect::new(right(half_w), lower(half_h), half_w, half_h),
        "fill" => Rect::new(GAP, GAP, full_w, full_h),
        _ => return None,
    })
}

/// The same size, in the middle of the area below the bar (shrunk to fit if it doesn't).
pub fn centred(cur: Rect, sw: i32, sh: i32) -> Rect {
    let area_h = sh - BAR;
    let w = cur.w.min(sw - 2 * GAP);
    let h = cur.h.min(area_h - 2 * GAP);
    Rect::new((sw - w) / 2, (area_h - h) / 2, w, h)
}

/// The spot a window dragged against the screen's edges should snap to, from its tile's
/// position in the output's view (`y` includes the bar). niri doesn't say where the pointer
/// is, but a window only gets past an edge when the pointer dragging it reaches that edge:
/// past the left or right edge is a half, past the top is fill, and down in a bottom corner
/// or up in a top corner is a quarter.
pub fn edge_spot(x: f64, y: f64, w: f64, sw: f64, sh: f64) -> Option<&'static str> {
    let left = x < 0.0;
    let right = x + w > sw;
    let top = y <= 0.0;
    // A window's top this far down only happens when it's held near the bottom edge.
    let bottom = y >= sh - 120.0;
    Some(match (left, right, top, bottom) {
        (true, true, _, _) => return None,
        (true, _, true, _) => "top-left",
        (_, true, true, _) => "top-right",
        (true, _, _, true) => "bottom-left",
        (_, true, _, true) => "bottom-right",
        (true, _, _, _) => "left",
        (_, true, _, _) => "right",
        (_, _, true, _) => "fill",
        _ => return None,
    })
}

// ---- memory: what to restore, and where a half is in its cycle ------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    /// Before its first snap.
    pub before: Rect,
    /// Where it was last snapped to.
    pub snapped: Rect,
    pub spot: String,
    pub step: usize,
}

/// What to remember after snapping a window that is now at `current` to `spot`, and which of
/// the half's sizes to use. Snapping again from a snapped place keeps the first "before".
pub fn plan(mem: Option<&Memory>, spot: &str, current: Rect) -> (Rect, usize) {
    let still_snapped = mem.filter(|m| m.snapped.near(&current));
    let before = still_snapped.map_or(current, |m| m.before);
    let step = match still_snapped {
        Some(m) if m.spot == spot && is_half(spot) => (m.step + 1) % FRACTIONS.len(),
        _ => 0,
    };
    (before, step)
}

fn memory_path() -> PathBuf {
    crate::common::runtime_dir().join("snaps.json")
}

pub fn load_memory() -> HashMap<u64, Memory> {
    std::fs::read_to_string(memory_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_memory(m: &HashMap<u64, Memory>) {
    if let Ok(text) = serde_json::to_string(m) {
        std::fs::write(memory_path(), text).ok();
    }
}

// ---- niri -----------------------------------------------------------------------------------

fn niri(args: &[&str]) -> Option<String> {
    let out = Command::new("niri").arg("msg").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Sizes and places a window by id.
pub fn apply(id: u64, r: Rect) {
    let id = id.to_string();
    niri(&["action", "set-window-width", "--id", &id, &r.w.to_string()]);
    niri(&["action", "set-window-height", "--id", &id, &r.h.to_string()]);
    niri(&["action", "move-floating-window", "--id", &id, "-x", &r.x.to_string(), "-y", &r.y.to_string()]);
}

/// Snaps window `id` to `spot` on a `sw` x `sh` screen, remembering how to restore it.
/// `from` is where it was before this snap (for a drag, where the drag started); `cycle` lets
/// a repeated half step through its sizes (shortcuts do, drags don't). Shared by the shortcuts
/// and the keeper.
pub fn snap_window(id: u64, from: Rect, spot: &str, sw: i32, sh: i32, cycle: bool) {
    let mut memory = load_memory();
    let (before, step) = plan(memory.get(&id), spot, from);
    let step = if cycle { step } else { 0 };
    let target = if spot == "centre" { Some(centred(from, sw, sh)) } else { spot_rect(spot, step, sw, sh) };
    let Some(target) = target else { return };
    memory.insert(id, Memory { before, snapped: target, spot: spot.to_string(), step });
    save_memory(&memory);
    apply(id, target);
}

/// The focused window's id and place, from `niri msg -j focused-window`.
pub fn parse_focused(json: &str) -> Option<(u64, Rect)> {
    let w: Value = serde_json::from_str(json).ok()?;
    let pos = w["layout"]["tile_pos_in_workspace_view"].as_array()?;
    let size = w["layout"]["tile_size"].as_array()?;
    let f = |v: Option<&Value>| v.and_then(Value::as_f64).map(|n| n.round() as i32);
    Some((w["id"].as_u64()?, Rect::new(f(pos.first())?, f(pos.get(1))? - BAR, f(size.first())?, f(size.get(1))?)))
}

const USAGE: &str =
    "usage: tripppin-ui snap left|right|top|bottom|top-left|top-right|bottom-left|bottom-right|fill|max|centre|restore";

pub fn run(spot: &str) -> glib::ExitCode {
    let spot = if spot == "center" { "centre" } else { spot };
    if spot == "max" {
        niri(&["action", "maximize-window-to-edges"]);
        return glib::ExitCode::SUCCESS;
    }
    if spot != "centre" && spot != "restore" && spot_rect(spot, 0, 1280, 800).is_none() {
        eprintln!("{USAGE}");
        return glib::ExitCode::FAILURE;
    }
    let Some((id, current)) = niri(&["-j", "focused-window"]).as_deref().and_then(parse_focused) else {
        return glib::ExitCode::SUCCESS;
    };
    if spot == "restore" {
        let mut memory = load_memory();
        if let Some(m) = memory.remove(&id) {
            save_memory(&memory);
            apply(id, m.before);
        }
        return glib::ExitCode::SUCCESS;
    }
    let out: Value = niri(&["-j", "focused-output"]).and_then(|o| serde_json::from_str(&o).ok()).unwrap_or_default();
    let sw = out["logical"]["width"].as_i64().unwrap_or(1280) as i32;
    let sh = out["logical"]["height"].as_i64().unwrap_or(800) as i32;
    snap_window(id, current, spot, sw, sh, true);
    glib::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_and_quarters() {
        assert_eq!(spot_rect("left", 0, 1280, 800), Some(Rect::new(16, 16, 616, 728)));
        assert_eq!(spot_rect("right", 0, 1280, 800), Some(Rect::new(648, 16, 616, 728)));
        assert_eq!(spot_rect("bottom-right", 0, 1280, 800), Some(Rect::new(648, 388, 616, 356)));
        assert_eq!(spot_rect("top", 0, 1280, 800), Some(Rect::new(16, 16, 1248, 356)));
        assert_eq!(spot_rect("bottom", 0, 1280, 800), Some(Rect::new(16, 388, 1248, 356)));
        assert_eq!(spot_rect("fill", 0, 1280, 800), Some(Rect::new(16, 16, 1248, 728)));
        assert_eq!(spot_rect("middle", 0, 1280, 800), None);
    }

    #[test]
    fn halves_cycle_through_thirds() {
        // A third, then two thirds, of the 1232 px left between the gaps.
        assert_eq!(spot_rect("left", 1, 1280, 800), Some(Rect::new(16, 16, 410, 728)));
        assert_eq!(spot_rect("right", 2, 1280, 800), Some(Rect::new(443, 16, 821, 728)));
        assert_eq!(spot_rect("left", 3, 1280, 800), spot_rect("left", 0, 1280, 800));
    }

    #[test]
    fn centres_and_shrinks_to_fit() {
        assert_eq!(centred(Rect::new(0, 0, 600, 400), 1280, 800), Rect::new(340, 180, 600, 400));
        assert_eq!(centred(Rect::new(0, 0, 2000, 400), 1280, 800), Rect::new(16, 180, 1248, 400));
    }

    #[test]
    fn remembers_the_first_place_and_cycles() {
        let first = Rect::new(100, 100, 500, 300);
        assert_eq!(plan(None, "left", first), (first, 0));
        let left = spot_rect("left", 0, 1280, 800).unwrap();
        let m = Memory { before: first, snapped: left, spot: "left".into(), step: 0 };
        // Left again: still the first place, the next size.
        assert_eq!(plan(Some(&m), "left", left), (first, 1));
        // Another spot from the snapped place: starts its own cycle, keeps the first place.
        assert_eq!(plan(Some(&m), "right", left), (first, 0));
        // Moved since: that's the new place to come back to.
        let moved = Rect::new(300, 200, 616, 728);
        assert_eq!(plan(Some(&m), "left", moved), (moved, 0));
        // Quarters don't cycle.
        let q = Memory { spot: "top-left".into(), snapped: spot_rect("top-left", 0, 1280, 800).unwrap(), ..m };
        assert_eq!(plan(Some(&q), "top-left", q.snapped).1, 0);
    }

    #[test]
    fn dragging_against_an_edge() {
        let (sw, sh) = (1280.0, 800.0);
        assert_eq!(edge_spot(-200.0, 300.0, 600.0, sw, sh), Some("left"));
        assert_eq!(edge_spot(900.0, 300.0, 600.0, sw, sh), Some("right"));
        assert_eq!(edge_spot(300.0, -12.0, 600.0, sw, sh), Some("fill"));
        assert_eq!(edge_spot(-20.0, -5.0, 600.0, sw, sh), Some("top-left"));
        assert_eq!(edge_spot(1000.0, 700.0, 600.0, sw, sh), Some("bottom-right"));
        assert_eq!(edge_spot(-50.0, 690.0, 600.0, sw, sh), Some("bottom-left"));
        // A tall window held by its title bar halfway down is a half, not a corner.
        assert_eq!(edge_spot(-50.0, 300.0, 600.0, sw, sh), Some("left"));
        // Inside the screen, or wider than it: nothing.
        assert_eq!(edge_spot(100.0, 100.0, 600.0, sw, sh), None);
        assert_eq!(edge_spot(-10.0, 100.0, 1400.0, sw, sh), None);
    }

    #[test]
    fn reads_the_focused_window() {
        let json = r#"{"id":9,"layout":{"tile_pos_in_workspace_view":[100.0,156.0],"tile_size":[600.0,400.0]}}"#;
        assert_eq!(parse_focused(json), Some((9, Rect::new(100, 116, 600, 400))));
        assert_eq!(parse_focused(r#"{"id":9,"layout":{"tile_pos_in_workspace_view":null}}"#), None);
    }
}
