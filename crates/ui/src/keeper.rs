//! Keeps floating windows usable: never tucked under the bar (where their title bar can't be
//! grabbed), never bigger than the screen, never dragged entirely off it. Every window floats on
//! Tripppin, and niri lets a floating window go anywhere, so this watches niri's event stream
//! and, once a window has settled for a moment, nudges it back.
//!
//! Run from the orb, which is always there.

use gtk::glib;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

/// The bar's reserved height (its height plus margin in /etc/xdg/waybar) and niri's gaps.
const BAR: f64 = super::switch::BAR as f64;
const GAP: f64 = 16.0;
/// How much of a window must stay on screen sideways, so it can be grabbed again.
const KEEP_VISIBLE: f64 = 80.0;
/// How long a window must stay put before it's nudged, so a drag isn't fought.
const SETTLE: Duration = Duration::from_millis(700);

/// A floating window as niri reports it: its tile's top-left in the output's view and its size.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: u64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
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
            format!("{}", x.round()),
            "-y".into(),
            format!("{}", (y - BAR).round()),
        ]);
    }
    out
}

/// Floating windows on `workspace`'s outputs, from `niri msg -j windows`.
pub fn parse_floating(json: &str) -> Vec<(Placed, u64)> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    items
        .iter()
        .filter(|w| w["is_floating"].as_bool() == Some(true))
        .filter_map(|w| {
            let layout = &w["layout"];
            let pos = layout["tile_pos_in_workspace_view"].as_array()?;
            let size = layout["tile_size"].as_array()?;
            Some((
                Placed {
                    id: w["id"].as_u64()?,
                    x: pos.first()?.as_f64()?,
                    y: pos.get(1)?.as_f64()?,
                    w: size.first()?.as_f64()?,
                    h: size.get(1)?.as_f64()?,
                },
                w["workspace_id"].as_u64()?,
            ))
        })
        .collect()
}

fn niri(args: &[&str]) -> Option<String> {
    let out = Command::new("niri").arg("msg").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Each workspace's output size, from `niri msg -j workspaces` and `outputs`.
fn screens() -> Vec<(u64, f64, f64)> {
    let ws: Value = niri(&["-j", "workspaces"]).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let outs: Value = niri(&["-j", "outputs"]).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    ws.as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let name = w["output"].as_str()?;
            let logical = &outs[name]["logical"];
            Some((w["id"].as_u64()?, logical["width"].as_f64()?, logical["height"].as_f64()?))
        })
        .collect()
}

fn check() {
    let Some(json) = niri(&["-j", "windows"]) else { return };
    let screens = screens();
    for (win, ws) in parse_floating(&json) {
        let Some(&(_, sw, sh)) = screens.iter().find(|(id, _, _)| *id == ws) else { continue };
        for action in corrections(&win, sw, sh) {
            let mut args = vec!["action"];
            args.extend(action.iter().map(String::as_str));
            niri(&args);
        }
    }
}

pub fn start() {
    let (tx, rx) = async_channel::unbounded::<()>();
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
            if line.contains("WindowLayoutsChanged") || line.contains("WindowOpenedOrChanged") {
                let _ = tx.send_blocking(());
            }
        }
    });
    // Each change restarts the wait, so a window being dragged is left alone until let go.
    let pending: std::rc::Rc<std::cell::RefCell<Option<glib::SourceId>>> = Default::default();
    glib::spawn_future_local(async move {
        while rx.recv().await.is_ok() {
            if let Some(old) = pending.borrow_mut().take() {
                old.remove();
            }
            let slot = pending.clone();
            let id = glib::timeout_add_local_once(SETTLE, move || {
                slot.borrow_mut().take();
                check();
            });
            *pending.borrow_mut() = Some(id);
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
    fn reads_floating_windows() {
        let json = r#"[{"id":3,"is_floating":true,"workspace_id":1,"layout":{"tile_pos_in_workspace_view":[10.0,20.0],"tile_size":[300.0,200.0]}},
                       {"id":4,"is_floating":false,"workspace_id":1,"layout":{"tile_pos_in_workspace_view":null,"tile_size":[1,1]}}]"#;
        assert_eq!(parse_floating(json), [(at(10.0, 20.0, 300.0, 200.0).with_id(3), 1)]);
    }

    impl Placed {
        fn with_id(mut self, id: u64) -> Self {
            self.id = id;
            self
        }
    }
}
