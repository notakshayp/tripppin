//! The power spark drawn in the terminal, optionally animated.
//!
//! Each character cell holds two square pixels (upper and lower half blocks), and
//! each pixel is shaded from 4×4 samples, so edges stay smooth. Geometry matches
//! `brand/logo.svg`: a ring of radius 60 centred at (100, 114), stroke 22 with
//! round caps, open ±44° at the top, and a four-point star at (100, 50), radius 36.

use std::f64::consts::PI;
use std::io::{self, IsTerminal, Write};
use std::thread::sleep;
use std::time::Duration;

const VIEW: (f64, f64, f64, f64) = (24.0, 6.0, 152.0, 184.0); // x, y, width, height in logo units
const PX_H: usize = 52; // pixel rows (two per text row)
const PX_W: usize = 43; // pixel columns, keeping pixels square
const RING: (f64, f64, f64, f64) = (100.0, 114.0, 60.0, 11.0); // cx, cy, radius, half stroke
const GAP_DEG: f64 = 44.0;
const SPAN: f64 = 360.0 - 2.0 * GAP_DEG;
const STAR: (f64, f64, f64, f64) = (100.0, 50.0, 36.0, 0.26); // cx, cy, radius, waist ratio
const SS: usize = 4; // samples per pixel along each axis

type Rgb = (f64, f64, f64);
const RING_RGB: Rgb = (230.0, 236.0, 230.0);
const SPARK_RGB: Rgb = (87.0, 217.0, 154.0);
const GLOW_RGB: Rgb = (190.0, 245.0, 214.0);

/// One animation state.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    /// How much of the ring is drawn, 0 to 1.
    pub ring: f64,
    /// How strongly the ring's newest stretch glows green, 0 to 1.
    pub head_glow: f64,
    pub spark_scale: f64,
    pub spark_turn: f64,
    /// Extra brightness on the spark, 0 to 1.
    pub glint: f64,
    /// Sparkle burst progress, 0 (none) to 1 (faded out).
    pub burst: f64,
}

impl Pose {
    pub const FULL: Pose =
        Pose { ring: 1.0, head_glow: 0.0, spark_scale: 1.0, spark_turn: 0.0, glint: 0.0, burst: 0.0 };
}

/// How the terminal can show colour.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Palette {
    /// 24-bit colour with smooth edges.
    TrueColor,
    /// The 16 standard colours (the Linux console): crisp pixels, white ring, green spark.
    Basic,
    /// No colour at all, for files like the login banner.
    Plain,
}

/// Angle measured clockwise from straight up, in degrees, range (-180, 180].
fn bearing(dx: f64, dy: f64) -> f64 {
    dx.atan2(-dy).to_degrees()
}

/// Fraction of the way along the ring, starting at the left end of the gap and running down and around.
fn along(deg: f64) -> f64 {
    (-GAP_DEG - deg).rem_euclid(360.0) / SPAN
}

/// Whether a point is on the drawn part of the ring, and if so how far behind the drawn end it is (0 = at the head).
fn ring_at(x: f64, y: f64, progress: f64) -> Option<f64> {
    if progress <= 0.0 {
        return None;
    }
    let (cx, cy, r, half) = RING;
    let (dx, dy) = (x - cx, y - cy);
    let drawn = progress.min(1.0);
    let deg = bearing(dx, dy);
    let d = (dx * dx + dy * dy).sqrt();
    let end_deg = -GAP_DEG - drawn * SPAN;
    let on_cap = |a_deg: f64| {
        let a = a_deg.to_radians();
        (x - (cx + r * a.sin())).powi(2) + (y - (cy - r * a.cos())).powi(2) <= half * half
    };
    let on_band = (d - r).abs() <= half && deg.abs() > GAP_DEG && along(deg) <= drawn;
    if on_band {
        Some(drawn - along(deg))
    } else if on_cap(end_deg) {
        Some(0.0)
    } else if on_cap(-GAP_DEG) {
        Some(drawn)
    } else {
        None
    }
}

fn star_points(cx: f64, cy: f64, r: f64, waist: f64) -> [(f64, f64); 8] {
    let mut pts = [(0.0, 0.0); 8];
    for (i, p) in pts.iter_mut().enumerate() {
        let rad = if i % 2 == 0 { r } else { r * waist };
        let a = -PI / 2.0 + 2.0 * PI * i as f64 / 8.0;
        *p = (cx + rad * a.cos(), cy + rad * a.sin());
    }
    pts
}

fn in_polygon(x: f64, y: f64, pts: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let ((xi, yi), (xj, yj)) = (pts[i], pts[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn in_star(x: f64, y: f64, scale: f64, turn_deg: f64) -> bool {
    if scale <= 0.0 {
        return false;
    }
    let (cx, cy, r, waist) = STAR;
    let t = -turn_deg.to_radians();
    let (dx, dy) = (x - cx, y - cy);
    let (px, py) = (cx + dx * t.cos() - dy * t.sin(), cy + dx * t.sin() + dy * t.cos());
    in_polygon(px, py, &star_points(cx, cy, r * scale, waist))
}

/// Four small sparkles flying out diagonally from the spark, shrinking as they go.
fn in_burst(x: f64, y: f64, t: f64) -> bool {
    if t <= 0.0 || t >= 1.0 {
        return false;
    }
    let (cx, cy, ..) = STAR;
    let dist = 30.0 + 34.0 * t;
    let size = 12.0 * (1.0 - t);
    [45.0f64, 135.0, 225.0, 315.0].iter().any(|deg| {
        let a = deg.to_radians();
        in_polygon(x, y, &star_points(cx + dist * a.sin(), cy - dist * a.cos(), size, 0.3))
    })
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
}

/// Colour and coverage (0 to 1) of every pixel.
fn pixels(pose: Pose) -> Vec<Vec<(Rgb, f64)>> {
    let (vx, vy, vw, vh) = VIEW;
    let (pw, ph) = (vw / PX_W as f64, vh / PX_H as f64);
    let n = (SS * SS) as f64;
    (0..PX_H)
        .map(|row| {
            (0..PX_W)
                .map(|col| {
                    let (mut acc, mut hits) = ((0.0, 0.0, 0.0), 0.0);
                    for sy in 0..SS {
                        for sx in 0..SS {
                            let x = vx + (col as f64 + (sx as f64 + 0.5) / SS as f64) * pw;
                            let y = vy + (row as f64 + (sy as f64 + 0.5) / SS as f64) * ph;
                            let c = if in_star(x, y, pose.spark_scale, pose.spark_turn) {
                                Some(mix(SPARK_RGB, GLOW_RGB, pose.glint))
                            } else if in_burst(x, y, pose.burst) {
                                Some(GLOW_RGB)
                            } else {
                                ring_at(x, y, pose.ring).map(|behind| {
                                    // The newest stretch of ring glows green, fading back to the ring colour.
                                    let glow = pose.head_glow * (1.0 - behind / 0.18).clamp(0.0, 1.0);
                                    mix(RING_RGB, SPARK_RGB, glow)
                                })
                            };
                            if let Some(c) = c {
                                acc = (acc.0 + c.0, acc.1 + c.1, acc.2 + c.2);
                                hits += 1.0;
                            }
                        }
                    }
                    if hits == 0.0 {
                        ((0.0, 0.0, 0.0), 0.0)
                    } else {
                        ((acc.0 / hits, acc.1 / hits, acc.2 / hits), hits / n)
                    }
                })
                .collect()
        })
        .collect()
}

/// A pixel as it will be drawn: `None` for empty, otherwise its colour.
fn ink(px: (Rgb, f64), palette: Palette) -> Option<Rgb> {
    let (rgb, cover) = px;
    match palette {
        // Smooth edges: fade partly covered pixels toward a dark background.
        Palette::TrueColor => (cover >= 0.12).then_some((rgb.0 * cover, rgb.1 * cover, rgb.2 * cover)),
        Palette::Basic | Palette::Plain => (cover >= 0.45).then_some(rgb),
    }
}

/// 16-colour codes: green for anything spark-coloured, bright white for the ring.
fn basic_code(rgb: Rgb, background: bool) -> u8 {
    let green = rgb.1 - (rgb.0 + rgb.2) / 2.0 > 40.0;
    match (green, background) {
        (true, false) => 92,
        (false, false) => 97,
        (true, true) => 102,
        (false, true) => 107,
    }
}

fn paint(line: &mut String, glyph: char, fg: Option<Rgb>, bg: Option<Rgb>, palette: Palette) {
    if palette == Palette::Plain || (fg.is_none() && bg.is_none()) {
        line.push(glyph);
        return;
    }
    for (c, background) in [(fg, false), (bg, true)] {
        let Some(c) = c else { continue };
        if palette == Palette::TrueColor {
            let layer = if background { 48 } else { 38 };
            *line += &format!("\x1b[{layer};2;{};{};{}m", c.0 as u8, c.1 as u8, c.2 as u8);
        } else {
            *line += &format!("\x1b[{}m", basic_code(c, background));
        }
    }
    line.push(glyph);
    *line += "\x1b[0m";
}

/// Renders one frame as text lines, two pixels per character cell.
pub fn render(pose: Pose, palette: Palette) -> Vec<String> {
    pixels(pose)
        .chunks(2)
        .map(|pair| {
            let mut line = String::new();
            for col in 0..PX_W {
                let top = ink(pair[0][col], palette);
                let bottom = pair.get(1).and_then(|r| ink(r[col], palette));
                let (glyph, fg, bg) = match (top, bottom) {
                    (None, None) => (' ', None, None),
                    (Some(t), None) => ('▀', Some(t), None),
                    (None, Some(b)) => ('▄', Some(b), None),
                    (Some(t), Some(_)) if palette == Palette::Plain => ('█', Some(t), None),
                    (Some(t), Some(b)) => ('▀', Some(t), Some(b)),
                };
                paint(&mut line, glyph, fg, bg, palette);
            }
            line
        })
        .collect()
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// The intro: the ring draws itself with a glowing head, the spark spins in,
/// sparkles burst outward, and the spark glints once. Each frame has its delay in ms.
pub fn intro() -> Vec<(Pose, u64)> {
    let base = Pose { ring: 0.0, head_glow: 1.0, spark_scale: 0.0, spark_turn: 0.0, glint: 0.0, burst: 0.0 };
    let mut frames = Vec::new();
    for i in 1..=22 {
        let t = f64::from(i) / 22.0;
        frames.push((Pose { ring: ease_out(t), ..base }, 24));
    }
    for i in 1..=14 {
        let t = f64::from(i) / 14.0;
        // Overshoot past full size, then settle; the ring's glow fades as the spark arrives.
        let scale = ease_out(t) * (1.0 + 0.22 * (PI * t).sin());
        let turn = -135.0 * (1.0 - ease_out(t));
        frames.push((Pose { ring: 1.0, head_glow: 1.0 - t, spark_scale: scale, spark_turn: turn, ..base }, 30));
    }
    for i in 1..=12 {
        let t = f64::from(i) / 12.0;
        frames.push((Pose { burst: t, glint: (1.0 - t) * 0.8, ..Pose::FULL }, 32));
    }
    frames.push((Pose::FULL, 0));
    frames
}

/// Which palette the current terminal supports.
pub fn detect_palette(is_tty: bool) -> Palette {
    if !is_tty || std::env::var_os("NO_COLOR").is_some() {
        return Palette::Plain;
    }
    let term = std::env::var("TERM").unwrap_or_default();
    let colorterm = std::env::var("COLORTERM").unwrap_or_default();
    if term == "dumb" {
        Palette::Plain
    } else if colorterm.contains("truecolor") || colorterm.contains("24bit") {
        Palette::TrueColor
    } else {
        Palette::Basic
    }
}

/// Puts `info` beside the logo, vertically centred; the first line is bold.
fn with_info(lines: Vec<String>, info: &[String], palette: Palette) -> Vec<String> {
    let top = lines.len().saturating_sub(info.len()) / 2;
    lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| match i.checked_sub(top).and_then(|k| info.get(k).map(|t| (k, t))) {
            Some((0, text)) if palette != Palette::Plain => format!("{l}   \x1b[1m{text}\x1b[0m"),
            Some((_, text)) => format!("{l}   {text}"),
            None => l,
        })
        .collect()
}

/// Prints the logo with `info` lines beside it. Animates on an interactive colour terminal.
pub fn show(info: &[String], animate: bool) -> io::Result<()> {
    let mut out = io::stdout().lock();
    let palette = detect_palette(out.is_terminal());
    if animate && palette != Palette::Plain {
        write!(out, "\x1b[?25l")?; // hide the cursor while drawing
        let frames = intro();
        for (n, (pose, wait)) in frames.iter().enumerate() {
            let last = n + 1 == frames.len();
            let mut lines = render(*pose, palette);
            if last {
                lines = with_info(lines, info, palette);
            }
            for l in &lines {
                writeln!(out, "\x1b[2K{l}")?;
            }
            out.flush()?;
            if !last {
                sleep(Duration::from_millis(*wait));
                write!(out, "\x1b[{}A", lines.len())?;
            }
        }
        write!(out, "\x1b[?25h")?;
    } else {
        for l in with_info(render(Pose::FULL, palette), info, palette) {
            writeln!(out, "{}", l.trim_end())?;
        }
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(s: &str) -> String {
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

    fn inked(lines: &[String]) -> usize {
        lines.iter().map(|l| strip(l).chars().filter(|c| *c != ' ').count()).sum()
    }

    #[test]
    fn full_logo_shape() {
        let lines = render(Pose::FULL, Palette::Plain);
        assert_eq!(lines.len(), PX_H.div_ceil(2));
        assert!(inked(&lines) > 250, "logo too sparse:\n{}", lines.join("\n"));
        // Every line keeps the full width, so text beside it lines up.
        assert!(lines.iter().all(|l| strip(l).chars().count() == PX_W));
    }

    #[test]
    fn spark_has_points_not_a_plus() {
        // Partway out along an arm, the arm is much narrower than across the centre.
        let width_at = |y: f64| (0..200).filter(|i| in_star(f64::from(*i) * 0.5 + 50.0, y, 1.0, 0.0)).count();
        let (_, cy, r, _) = STAR;
        assert!(width_at(cy) > 4 * width_at(cy - r * 0.6), "{} vs {}", width_at(cy), width_at(cy - r * 0.6));
    }

    #[test]
    fn ring_draws_in_order() {
        let sizes: Vec<usize> = intro().iter().take(22).map(|(p, _)| inked(&render(*p, Palette::Plain))).collect();
        assert!(sizes.windows(2).all(|w| w[1] >= w[0]), "{sizes:?}");
    }

    #[test]
    fn palettes_share_a_layout() {
        let plain = render(Pose::FULL, Palette::Plain);
        for palette in [Palette::TrueColor, Palette::Basic] {
            for (p, c) in plain.iter().zip(render(Pose::FULL, palette)) {
                assert_eq!(strip(&c).chars().count(), p.chars().count());
            }
        }
    }

    #[test]
    fn geometry() {
        assert!(in_star(100.0, 50.0, 1.0, 0.0));
        assert!(!in_star(140.0, 50.0, 1.0, 0.0));
        assert!(ring_at(40.0, 114.0, 1.0).is_some());
        assert!(ring_at(100.0, 54.0, 1.0).is_none()); // the gap
    }
}

#[cfg(test)]
mod preview {
    /// Writes a few frames as ANSI text for visual review: `cargo test -p trip dump_frames -- --ignored`.
    #[test]
    #[ignore]
    fn dump_frames() {
        let frames = super::intro();
        for n in [8usize, 21, 28, 33, 40, frames.len() - 1] {
            let text = super::render(frames[n].0, super::Palette::TrueColor).join("\n");
            std::fs::write(std::env::temp_dir().join(format!("trip-logo-{n:02}.ans")), text).unwrap();
        }
    }
}
