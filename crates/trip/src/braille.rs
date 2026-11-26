//! The text-OS logo: braille dots generated from the mark's geometry (see `logo.rs`).
//!
//! Each character is a 2x4 grid of dots, so the round ring and the four-point spark keep their
//! shape at any size. A dot is lit when at least 4 of its 9 sub-samples are covered.

use crate::logo::{GAP_DEG, RING, SPAN, SPARK, spark_points};

/// What a braille cell is made of, for colouring: the spark is green, the ring bright.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Blank,
    Spark,
    Ring,
}

/// How long the boot animation runs, in seconds.
pub const BOOT_SECS: f64 = 1.2;

fn in_poly(x: f64, y: f64, p: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let mut j = p.len() - 1;
    for i in 0..p.len() {
        let ((xi, yi), (xj, yj)) = (p[i], p[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn in_ring(x: f64, y: f64, frac: f64) -> bool {
    if frac <= 0.0 {
        return false;
    }
    let (cx, cy, r, w) = RING;
    let d = (x - cx).hypot(y - cy);
    let ang = ((x - cx).atan2(-(y - cy)).to_degrees() + 360.0) % 360.0; // 0 = up, clockwise
    let end = GAP_DEG + SPAN * frac;
    if (d - r).abs() <= w / 2.0 && ang >= GAP_DEG && ang <= end {
        return true;
    }
    [GAP_DEG, end].iter().any(|a| {
        let rad = a.to_radians();
        (x - (cx + r * rad.sin())).hypot(y - (cy - r * rad.cos())) <= w / 2.0
    })
}

fn in_spark(x: f64, y: f64, s: f64) -> bool {
    if s <= 0.0 {
        return false;
    }
    let (sx, sy, _, _) = SPARK;
    in_poly(sx + (x - sx) / s, sy + (y - sy) / s, &spark_points(1.0, 0.0))
}

/// The logo as braille cells, `cols` characters wide (rows follow from the mark's shape).
/// `ring` is how much of the ring is drawn (0 to 1), `spark` the spark's scale (0 to 1).
pub fn braille(cols: usize, ring: f64, spark: f64) -> Vec<Vec<(char, Ink)>> {
    let (x0, y0, w, h) = (27.0, 10.0, 146.0, 180.0);
    let rows = (cols as f64 * h / w / 2.0).ceil() as usize;
    let (dw, dh) = (w / (cols * 2) as f64, h / (rows * 4) as f64);
    const BITS: [(usize, usize, u32); 8] = [
        (0, 0, 0x01),
        (0, 1, 0x02),
        (0, 2, 0x04),
        (1, 0, 0x08),
        (1, 1, 0x10),
        (1, 2, 0x20),
        (0, 3, 0x40),
        (1, 3, 0x80),
    ];
    (0..rows)
        .map(|r| {
            (0..cols)
                .map(|c| {
                    let (mut code, mut sp, mut rg) = (0u32, 0, 0);
                    for (dx, dy, bit) in BITS {
                        let (mut hs, mut hr) = (0, 0);
                        for sx in 0..3 {
                            for sy in 0..3 {
                                let x = x0 + ((c * 2 + dx) as f64 + (f64::from(sx) + 0.5) / 3.0) * dw;
                                let y = y0 + ((r * 4 + dy) as f64 + (f64::from(sy) + 0.5) / 3.0) * dh;
                                if in_spark(x, y, spark) {
                                    hs += 1;
                                } else if in_ring(x, y, ring) {
                                    hr += 1;
                                }
                            }
                        }
                        if hs + hr >= 4 {
                            code |= bit;
                            if hs >= hr {
                                sp += 1;
                            } else {
                                rg += 1;
                            }
                        }
                    }
                    let ch = char::from_u32(0x2800 + code).unwrap_or(' ');
                    match (code, sp > rg) {
                        (0, _) => (' ', Ink::Blank),
                        (_, true) => (ch, Ink::Spark),
                        _ => (ch, Ink::Ring),
                    }
                })
                .collect()
        })
        .collect()
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// The boot animation at `t` seconds: (spark scale, ring fraction, letters of the name typed).
/// The spark grows in (0.35 to 1), then the ring draws clockwise from the gap, then the name
/// types in. It never turns.
pub fn boot_pose(t: f64) -> (f64, f64, usize) {
    if t <= 0.0 {
        return (0.0, 0.0, 0);
    }
    let spark = 0.35 + 0.65 * ease_out(t / 0.3);
    let ring = ease_out((t - 0.25) / 0.6);
    let letters = (((t - 0.85) / 0.3).clamp(0.0, 1.0) * 8.0).round() as usize;
    (spark, ring, letters)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(g: &[Vec<(char, Ink)>]) -> String {
        g.iter().map(|r| r.iter().map(|c| c.0).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn small_logo_matches_golden() {
        let golden = include_str!("../tests/logo10.txt");
        assert_eq!(text(&braille(10, 1.0, 1.0)), golden.trim_end_matches('\n'));
    }

    #[test]
    fn spark_is_green_ring_is_bright_and_frames_grow() {
        let full = braille(18, 1.0, 1.0);
        assert!(full.iter().flatten().any(|c| c.1 == Ink::Spark));
        assert!(full.iter().flatten().any(|c| c.1 == Ink::Ring));
        let dots = |g: &[Vec<(char, Ink)>]| -> u32 {
            g.iter().flatten().map(|c| (c.0 as u32).saturating_sub(0x2800).count_ones()).sum()
        };
        assert!(dots(&braille(18, 0.0, 0.35)) < dots(&braille(18, 0.0, 1.0)));
        assert!(dots(&braille(18, 0.5, 1.0)) < dots(&full));
        assert_eq!(boot_pose(0.0), (0.0, 0.0, 0));
        assert_eq!(boot_pose(BOOT_SECS), (1.0, 1.0, 8));
    }
}
