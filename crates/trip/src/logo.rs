//! The power spark: its geometry, its intro animation, and its drawing in the terminal.
//!
//! The geometry here is the one source for every drawing of the mark (`brand/logo.svg`,
//! the desktop's screens in `tripppin-ui`, the boot splash frames from
//! `scripts/brand-assets.sh`): a ring of radius 60 centred at (100, 114), stroke 22 with
//! round caps, open ±44° at the top, and in the opening an upright four-point spark centred
//! at (100, 50), its points 36 long and its waist 0.24 of that. The mark's
//! box is 146 × 175 units from (27, 12).
//!
//! In the terminal each character cell holds two square pixels (upper and lower half
//! blocks), and each pixel is shaded from 4×4 samples, so edges stay smooth.

use std::io::{self, IsTerminal, Write};
use std::thread::sleep;
use std::time::Duration;

/// The ring: centre x, centre y, radius, stroke width.
pub const RING: (f64, f64, f64, f64) = (100.0, 114.0, 60.0, 22.0);
/// Half the ring's opening at the top, in degrees either side of straight up.
pub const GAP_DEG: f64 = 44.0;
/// How far round the ring goes, in degrees.
pub const SPAN: f64 = 360.0 - 2.0 * GAP_DEG;
/// The spark: centre x, centre y, point length, waist ratio.
pub const SPARK: (f64, f64, f64, f64) = (100.0, 50.0, 36.0, 0.24);
/// The spark's resting turn: its long points straight up, down and across.
pub const SPARK_REST_DEG: f64 = 0.0;
/// The mark's box in logo units: x, y, width, height.
pub const BOX: (f64, f64, f64, f64) = (27.0, 12.0, 146.0, 175.0);
/// How long the intro takes, in seconds.
pub const INTRO_SECS: f64 = 1.3;

const VIEW: (f64, f64, f64, f64) = (24.0, 6.0, 152.0, 184.0); // x, y, width, height in logo units
const PX_H: usize = 52; // pixel rows (two per text row)
const PX_W: usize = 43; // pixel columns, keeping pixels square
const SS: usize = 4; // samples per pixel along each axis

type Rgb = (f64, f64, f64);
const RING_RGB: Rgb = (230.0, 236.0, 230.0);
const SPARK_RGB: Rgb = (87.0, 217.0, 154.0);
const GLOW_RGB: Rgb = (190.0, 245.0, 214.0);

/// One animation state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// How much of the ring is drawn, 0 to 1.
    pub ring: f64,
    /// How strongly the ring's newest stretch glows green, 0 to 1.
    pub head_glow: f64,
    pub spark_scale: f64,
    /// Degrees the spark is turned away from rest, clockwise.
    pub spark_turn: f64,
    /// Extra brightness on the spark, 0 to 1.
    pub glint: f64,
}

impl Pose {
    pub const FULL: Pose = Pose { ring: 1.0, head_glow: 0.0, spark_scale: 1.0, spark_turn: 0.0, glint: 0.0 };
}

/// The spark's eight corners (long points at even indices), turned `turn_deg` past rest and scaled.
pub fn spark_points(scale: f64, turn_deg: f64) -> [(f64, f64); 8] {
    let (cx, cy, r, waist) = SPARK;
    let mut pts = [(0.0, 0.0); 8];
    for (i, p) in pts.iter_mut().enumerate() {
        let rad = if i % 2 == 0 { r } else { r * waist } * scale;
        let a = (SPARK_REST_DEG + turn_deg + 45.0 * i as f64).to_radians();
        *p = (cx + rad * a.sin(), cy - rad * a.cos());
    }
    pts
}

/// The ring's two ends, left then right.
pub fn ring_ends() -> [(f64, f64); 2] {
    let (cx, cy, r, _) = RING;
    let a = GAP_DEG.to_radians();
    [(cx - r * a.sin(), cy - r * a.cos()), (cx + r * a.sin(), cy - r * a.cos())]
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// The intro at `t` seconds: the ring draws itself from its left end, down and round, with a
/// glowing head; then the spark spins in half a turn, settling from just past full size; then it
/// glints once. From [`INTRO_SECS`] on, the finished mark.
pub fn intro_pose(t: f64) -> Pose {
    // The spark first, growing into place without turning; then the ring draws itself.
    const SPARK_END: f64 = 0.35;
    const RING_START: f64 = 0.3;
    const RING_END: f64 = 0.95;
    let spark_scale = ease_out(t / SPARK_END);
    let u = ((t - RING_START) / (RING_END - RING_START)).clamp(0.0, 1.0);
    let ring = if u <= 0.0 { 0.0 } else { ease_out(u) };
    let glint = if t > RING_END { 0.6 * (1.0 - (t - RING_END) / (INTRO_SECS - RING_END)).clamp(0.0, 1.0) } else { 0.0 };
    Pose { ring, head_glow: if u > 0.0 && u < 1.0 { 1.0 } else { 0.0 }, spark_scale, spark_turn: 0.0, glint }
}

/// The waiting motion after the intro, `t` seconds after it ended: every 2.4 s the spark makes
/// a gentle quarter turn (a spark turned a quarter is the same spark, so it always rests looking
/// right). Returns the extra turn in degrees.
pub fn waiting_turn(t: f64) -> f64 {
    const EVERY: f64 = 2.4;
    const TURN: f64 = 0.7;
    if t <= 0.0 {
        return 0.0;
    }
    let n = (t / EVERY).floor();
    let s = ((t - n * EVERY - (EVERY - TURN)) / TURN).clamp(0.0, 1.0);
    // Eases in and out, so it never jolts.
    (90.0 * (n + s * s * (3.0 - 2.0 * s))) % 360.0
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
    let (cx, cy, r, width) = RING;
    let half = width / 2.0;
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

fn in_spark(x: f64, y: f64, scale: f64, turn_deg: f64) -> bool {
    scale > 0.0 && in_polygon(x, y, &spark_points(scale, turn_deg))
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
                            let c = if in_spark(x, y, pose.spark_scale, pose.spark_turn) {
                                Some(mix(SPARK_RGB, GLOW_RGB, pose.glint))
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

/// The intro as terminal frames, each with its delay in ms (the last has none).
pub fn intro() -> Vec<(Pose, u64)> {
    const STEP_MS: u64 = 28;
    let n = (INTRO_SECS * 1000.0 / STEP_MS as f64).ceil() as u64;
    let mut frames: Vec<(Pose, u64)> = (1..n).map(|i| (intro_pose((i * STEP_MS) as f64 / 1000.0), STEP_MS)).collect();
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
    fn spark_points_straight_up_and_across() {
        let (cx, cy, r, _) = SPARK;
        let d = r * 0.6;
        let diag = d / 2f64.sqrt();
        // Straight up, down and across there is spark; out along the diagonals there isn't.
        for (dx, dy) in [(0.0, d), (0.0, -d), (d, 0.0), (-d, 0.0)] {
            assert!(in_spark(cx + dx, cy + dy, 1.0, 0.0));
        }
        for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            assert!(!in_spark(cx + sx * diag, cy + sy * diag, 1.0, 0.0));
        }
        // A quarter turn is the same spark.
        assert!(in_spark(cx, cy - d, 1.0, 90.0));
    }

    #[test]
    fn spark_clears_the_ring() {
        // The spark's lower points stop well short of the ring's round ends.
        let (_, _, _, width) = RING;
        for (ex, ey) in ring_ends() {
            let nearest = spark_points(1.0, 0.0)
                .iter()
                .map(|(x, y)| ((x - ex).powi(2) + (y - ey).powi(2)).sqrt())
                .fold(f64::MAX, f64::min);
            assert!(nearest - width / 2.0 > 5.0, "{nearest}");
        }
    }

    #[test]
    fn spark_first_then_the_ring() {
        // The spark is in place before the ring starts, and the ring only grows after that.
        assert_eq!(intro_pose(0.3).ring, 0.0);
        assert!(intro_pose(0.3).spark_scale > 0.9);
        let sizes: Vec<usize> = intro()
            .iter()
            .filter(|(p, _)| p.spark_scale >= 1.0)
            .map(|(p, _)| inked(&render(*p, Palette::Plain)))
            .collect();
        assert!(sizes.len() > 8);
        assert!(sizes.windows(2).all(|w| w[1] >= w[0]), "{sizes:?}");
    }

    #[test]
    fn intro_settles() {
        assert_eq!(intro_pose(0.0).spark_scale, 0.0);
        assert_eq!(intro_pose(INTRO_SECS), Pose::FULL);
        assert_eq!(intro().last().map(|f| f.0), Some(Pose::FULL));
        // The spark never turns.
        assert!(intro().iter().all(|(p, _)| p.spark_turn == 0.0));
    }

    #[test]
    fn waiting_turns_in_quarters() {
        assert_eq!(waiting_turn(0.0), 0.0);
        assert_eq!(waiting_turn(1.0), 0.0);
        assert_eq!(waiting_turn(2.4), 90.0);
        let mid = waiting_turn(2.05);
        assert!(mid > 0.0 && mid < 90.0, "{mid}");
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
        assert!(in_spark(100.0, 50.0, 1.0, 0.0));
        assert!(!in_spark(140.0, 50.0, 1.0, 0.0));
        assert!(ring_at(40.0, 114.0, 1.0).is_some());
        assert!(ring_at(100.0, 54.0, 1.0).is_none()); // the gap
        let [(lx, ly), (rx, ry)] = ring_ends();
        assert!((lx - 58.32).abs() < 0.01 && (rx - 141.68).abs() < 0.01 && (ly - 70.84).abs() < 0.01 && ly == ry);
    }
}

#[cfg(test)]
mod preview {
    use super::*;

    /// Six frames of the terminal intro as an SVG of square pixels, for brand/terminal-logo.png:
    /// `cargo test -p trip --lib terminal_logo_svg -- --ignored` (scripts/brand-assets.sh runs it).
    #[test]
    #[ignore]
    fn terminal_logo_svg() {
        const PX: usize = 7;
        const MARGIN: usize = 32;
        const GAP: usize = 36;
        let times = [0.25, 0.5, 0.7, 0.85, 1.05, INTRO_SECS];
        let (fw, fh) = (PX_W * PX, PX_H * PX);
        let (w, h) = (2 * MARGIN + 3 * fw + 2 * GAP, 2 * MARGIN + 2 * fh + GAP);
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" shape-rendering=\"crispEdges\"><rect width=\"{w}\" height=\"{h}\" fill=\"#0d1211\"/>"
        );
        for (n, t) in times.iter().enumerate() {
            let (ox, oy) = (MARGIN + (n % 3) * (fw + GAP), MARGIN + (n / 3) * (fh + GAP));
            for (row, line) in pixels(intro_pose(*t)).iter().enumerate() {
                for (col, px) in line.iter().enumerate() {
                    if let Some(c) = ink(*px, Palette::TrueColor) {
                        svg += &format!(
                            "<rect x=\"{}\" y=\"{}\" width=\"{PX}\" height=\"{PX}\" fill=\"rgb({},{},{})\"/>",
                            ox + col * PX,
                            oy + row * PX,
                            c.0 as u8,
                            c.1 as u8,
                            c.2 as u8
                        );
                    }
                }
            }
        }
        svg += "</svg>\n";
        let out = std::env::var("TRIP_LOGO_SVG")
            .unwrap_or_else(|_| std::env::temp_dir().join("terminal-logo.svg").display().to_string());
        std::fs::write(out, svg).unwrap();
    }
}
