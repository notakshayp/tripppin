//! The power spark drawn in terminal characters, optionally animated.
//!
//! Geometry matches `brand/logo.svg`: a ring of radius 60 centred at (100, 114),
//! stroke 22 with round caps, open ±44° at the top, and a four-point star at
//! (100, 50), radius 36, waist ratio 0.24. Everything is sampled on a grid of
//! terminal cells (about twice as tall as wide) with 3×3 subsamples per cell.

use std::f64::consts::PI;
use std::io::{self, IsTerminal, Write};
use std::thread::sleep;
use std::time::Duration;

const COLS: usize = 30;
const ROWS: usize = 16;
const VIEW: (f64, f64, f64, f64) = (27.0, 12.0, 146.0, 175.0); // x, y, width, height
const RING: (f64, f64, f64, f64) = (100.0, 114.0, 60.0, 11.0); // cx, cy, radius, half stroke
const GAP_DEG: f64 = 44.0;
const STAR: (f64, f64, f64, f64) = (100.0, 50.0, 36.0, 0.24); // cx, cy, radius, waist
const SHADES: [char; 5] = [' ', '░', '▒', '▓', '█'];
const SPARK_COLOR: &str = "\x1b[38;2;87;217;154m";
const RESET: &str = "\x1b[0m";

/// One animation state: how much of the ring is drawn, and the spark's scale and turn.
#[derive(Clone, Copy)]
pub struct Pose {
    pub ring: f64,
    pub spark_scale: f64,
    pub spark_turn: f64,
}

impl Pose {
    pub const FULL: Pose = Pose { ring: 1.0, spark_scale: 1.0, spark_turn: 0.0 };
}

#[derive(Clone, Copy, PartialEq)]
enum Ink {
    None,
    Ring,
    Spark,
}

/// Angle measured clockwise from straight up, in degrees, range (-180, 180].
fn bearing(dx: f64, dy: f64) -> f64 {
    dx.atan2(-dy).to_degrees()
}

fn in_ring(x: f64, y: f64, progress: f64) -> bool {
    if progress <= 0.0 {
        return false;
    }
    let (cx, cy, r, half) = RING;
    let (dx, dy) = (x - cx, y - cy);
    // Distance along the ring, starting at the left end of the gap and running down and around.
    let span = 360.0 - 2.0 * GAP_DEG;
    let along = |deg: f64| (-GAP_DEG - deg).rem_euclid(360.0) / span;
    let drawn = progress.min(1.0);
    let d = (dx * dx + dy * dy).sqrt();
    let deg = bearing(dx, dy);
    if (d - r).abs() <= half && deg.abs() > GAP_DEG && along(deg) <= drawn {
        return true;
    }
    // Round caps at the start and at the current end of the stroke.
    let cap = |a_deg: f64| {
        let a = a_deg.to_radians();
        let (px, py) = (cx + r * a.sin(), cy - r * a.cos());
        (x - px).powi(2) + (y - py).powi(2) <= half * half
    };
    let end_deg = -GAP_DEG - drawn * span;
    cap(-GAP_DEG) || cap(end_deg)
}

fn in_star(x: f64, y: f64, scale: f64, turn_deg: f64) -> bool {
    if scale <= 0.0 {
        return false;
    }
    let (cx, cy, r, waist) = STAR;
    // Undo the spark's rotation, then test against the eight-point outline.
    let t = -turn_deg.to_radians();
    let (dx, dy) = (x - cx, y - cy);
    let (px, py) = (dx * t.cos() - dy * t.sin(), dx * t.sin() + dy * t.cos());
    let pts: Vec<(f64, f64)> = (0..8)
        .map(|i| {
            let rad = if i % 2 == 0 { r } else { r * waist } * scale;
            let a = -PI / 2.0 + 2.0 * PI * f64::from(i) / 8.0;
            (rad * a.cos(), rad * a.sin())
        })
        .collect();
    let mut inside = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let ((xi, yi), (xj, yj)) = (pts[i], pts[j]);
        if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Renders one frame as rows of (character, ink) cells.
fn frame(pose: Pose) -> Vec<Vec<(char, Ink)>> {
    let (vx, vy, vw, vh) = VIEW;
    let (cw, ch) = (vw / COLS as f64, vh / ROWS as f64);
    (0..ROWS)
        .map(|row| {
            (0..COLS)
                .map(|col| {
                    let (mut ring, mut spark) = (0, 0);
                    for sy in 0..3 {
                        for sx in 0..3 {
                            let x = vx + (col as f64 + (f64::from(sx) + 0.5) / 3.0) * cw;
                            let y = vy + (row as f64 + (f64::from(sy) + 0.5) / 3.0) * ch;
                            if in_star(x, y, pose.spark_scale, pose.spark_turn) {
                                spark += 1;
                            } else if in_ring(x, y, pose.ring) {
                                ring += 1;
                            }
                        }
                    }
                    let hits = ring + spark;
                    let shade = SHADES[(hits * (SHADES.len() - 1)).div_ceil(9)];
                    let ink = if hits == 0 {
                        Ink::None
                    } else if spark >= ring {
                        Ink::Spark
                    } else {
                        Ink::Ring
                    };
                    (shade, ink)
                })
                .collect()
        })
        .collect()
}

/// Turns a frame into printable lines, coloured or plain.
pub fn render(pose: Pose, color: bool) -> Vec<String> {
    frame(pose)
        .into_iter()
        .map(|row| {
            let mut line = String::new();
            let mut current = Ink::None;
            for (c, ink) in row {
                if color && ink != current {
                    line.push_str(if ink == Ink::Spark { SPARK_COLOR } else { RESET });
                    current = ink;
                }
                line.push(c);
            }
            if color && current != Ink::None {
                line.push_str(RESET);
            }
            line.trim_end().to_string()
        })
        .collect()
}

/// The intro: the ring draws itself, then the spark pops in with a half turn.
pub fn intro() -> Vec<Pose> {
    let mut poses: Vec<Pose> =
        (1..=16).map(|i| Pose { ring: f64::from(i) / 16.0, spark_scale: 0.0, spark_turn: 0.0 }).collect();
    for i in 1..=10 {
        let t = f64::from(i) / 10.0;
        // Ease out with a slight overshoot.
        let s = 1.0 + 0.12 * (PI * t).sin() * (1.0 - t) * 4.0;
        poses.push(Pose { ring: 1.0, spark_scale: t.min(1.0) * s.min(1.15), spark_turn: -120.0 * (1.0 - t).powi(2) });
    }
    poses.push(Pose::FULL);
    poses
}

/// Prints the logo with `info` lines beside it. Animates on an interactive terminal.
pub fn show(info: &[String], animate: bool) -> io::Result<()> {
    let mut out = io::stdout().lock();
    let tty = out.is_terminal() && std::env::var("TERM").map_or(true, |t| t != "dumb");
    let color = tty && std::env::var_os("NO_COLOR").is_none();
    let place = |lines: Vec<String>| -> Vec<String> {
        let top = (ROWS.saturating_sub(info.len())) / 2;
        lines
            .into_iter()
            .enumerate()
            .map(|(i, l)| match i.checked_sub(top).and_then(|k| info.get(k)) {
                Some(text) => format!("{l:<width$}   {text}", width = visible_pad(&l, COLS)),
                None => l,
            })
            .collect()
    };
    if animate && tty {
        write!(out, "\x1b[?25l")?; // hide the cursor while drawing
        let poses = intro();
        for (n, pose) in poses.iter().enumerate() {
            let lines = if n + 1 == poses.len() { place(render(*pose, color)) } else { render(*pose, color) };
            for l in &lines {
                writeln!(out, "\x1b[2K{l}")?;
            }
            out.flush()?;
            if n + 1 < poses.len() {
                sleep(Duration::from_millis(if pose.spark_scale > 0.0 { 40 } else { 28 }));
                write!(out, "\x1b[{}A", lines.len())?;
            }
        }
        write!(out, "\x1b[?25h")?;
    } else {
        for l in place(render(Pose::FULL, color)) {
            writeln!(out, "{l}")?;
        }
    }
    out.flush()
}

/// Width to pad a line to, in characters, so colour codes don't count toward the column.
fn visible_pad(line: &str, cols: usize) -> usize {
    let visible = strip_ansi(line).chars().count();
    line.chars().count() + cols.saturating_sub(visible)
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(lines: &[String]) -> usize {
        lines.iter().map(|l| strip_ansi(l).chars().filter(|c| *c != ' ').count()).sum()
    }

    #[test]
    fn full_logo_has_ring_and_spark() {
        let lines = render(Pose::FULL, false);
        assert_eq!(lines.len(), ROWS);
        assert!(ink(&lines) > 60, "logo too sparse:\n{}", lines.join("\n"));
        // The spark sits in the gap at the top: the top row has ink only near the middle.
        let top = &lines[0];
        assert!(top.trim_start().len() < COLS / 2);
    }

    #[test]
    fn ring_grows_during_intro() {
        let poses = intro();
        let sizes: Vec<usize> = poses.iter().take(16).map(|p| ink(&render(*p, false))).collect();
        assert!(sizes.windows(2).all(|w| w[1] >= w[0]), "{sizes:?}");
        assert!(sizes[0] < sizes[15]);
    }

    #[test]
    fn color_codes_do_not_change_layout() {
        let plain = render(Pose::FULL, false);
        let colored = render(Pose::FULL, true);
        for (p, c) in plain.iter().zip(&colored) {
            assert_eq!(p, &strip_ansi(c));
        }
    }

    #[test]
    fn spark_is_inside_its_own_bounds() {
        assert!(in_star(100.0, 50.0, 1.0, 0.0));
        assert!(!in_star(140.0, 50.0, 1.0, 0.0));
        assert!(in_ring(40.0, 114.0, 1.0));
        assert!(!in_ring(100.0, 54.0, 1.0)); // the gap
    }
}
