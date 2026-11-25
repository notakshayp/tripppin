//! Thin white line glyphs drawn as vectors at the screen's own pixel density: an SVG path
//! (the subset our icons use: M L H V C A Z, absolute and relative) is parsed once and
//! stroked with cairo at whatever size the widget is, so nothing is a raster scaled up.

use gtk::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(f64, f64),
    Line(f64, f64),
    Curve(f64, f64, f64, f64, f64, f64),
    Close,
}

fn numbers(s: &str) -> Vec<f64> {
    // Numbers may run together: "1.5.5" is 1.5 and .5; "5-3" is 5 and -3.
    let mut out = Vec::new();
    let (mut cur, mut dot) = (String::new(), false);
    for ch in s.chars() {
        let starts_new = match ch {
            '-' => !cur.is_empty() && !cur.ends_with('e'),
            '.' => dot,
            ' ' | ',' => true,
            _ => false,
        };
        if starts_new {
            if let Ok(v) = cur.parse() {
                out.push(v);
            }
            cur.clear();
            dot = false;
        }
        if ch == ' ' || ch == ',' {
            continue;
        }
        if ch == '.' {
            dot = true;
        }
        cur.push(ch);
    }
    if let Ok(v) = cur.parse() {
        out.push(v);
    }
    out
}

/// An SVG elliptical arc from (x0,y0) as cubic curves.
#[allow(clippy::too_many_arguments)]
fn arc(out: &mut Vec<Seg>, x0: f64, y0: f64, rx: f64, ry: f64, rot: f64, large: bool, sweep: bool, x: f64, y: f64) {
    if (x0 - x).abs() < 1e-9 && (y0 - y).abs() < 1e-9 {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        out.push(Seg::Line(x, y));
        return;
    }
    let phi = rot.to_radians();
    let (c, s) = (phi.cos(), phi.sin());
    let (dx, dy) = ((x0 - x) / 2.0, (y0 - y) / 2.0);
    let (x1, y1) = (c * dx + s * dy, -s * dx + c * dy);
    let lambda = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (num / den).max(0.0).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
    let (cx, cy) = (c * cx1 - s * cy1 + (x0 + x) / 2.0, s * cx1 + c * cy1 + (y0 + y) / 2.0);
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = ang(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dt = ang((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !sweep && dt > 0.0 {
        dt -= std::f64::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f64::consts::TAU;
    }
    let n = (dt.abs() / (std::f64::consts::FRAC_PI_2)).ceil().max(1.0) as usize;
    let step = dt / n as f64;
    let alpha = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |t: f64| (cx + rx * t.cos() * c - ry * t.sin() * s, cy + rx * t.cos() * s + ry * t.sin() * c);
    let deriv = |t: f64| (-rx * t.sin() * c - ry * t.cos() * s, -rx * t.sin() * s + ry * t.cos() * c);
    for i in 0..n {
        let (a, b) = (t1 + step * i as f64, t1 + step * (i + 1) as f64);
        let (p0, p1) = (point(a), point(b));
        let (d0, d1) = (deriv(a), deriv(b));
        out.push(Seg::Curve(
            p0.0 + alpha * d0.0,
            p0.1 + alpha * d0.1,
            p1.0 - alpha * d1.0,
            p1.1 - alpha * d1.1,
            p1.0,
            p1.1,
        ));
    }
}

pub fn parse(d: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    let (mut x, mut y, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0);
    let mut chars = d.char_indices().peekable();
    while let Some((i, cmd)) = chars.next() {
        if !cmd.is_ascii_alphabetic() {
            continue;
        }
        let end = chars
            .peek()
            .map_or(d.len(), |_| d[i + 1..].find(|c: char| c.is_ascii_alphabetic()).map_or(d.len(), |p| i + 1 + p));
        let args = numbers(&d[i + 1..end]);
        let rel = cmd.is_ascii_lowercase();
        let base = |x: f64, y: f64| if rel { (x, y) } else { (0.0, 0.0) };
        let (bx, by) = base(x, y);
        match cmd.to_ascii_uppercase() {
            'M' | 'L' => {
                for (n, p) in args.chunks_exact(2).enumerate() {
                    let (px, py) = if rel { (x + p[0], y + p[1]) } else { (p[0], p[1]) };
                    if cmd.eq_ignore_ascii_case(&'M') && n == 0 {
                        out.push(Seg::Move(px, py));
                        sx = px;
                        sy = py;
                    } else {
                        out.push(Seg::Line(px, py));
                    }
                    x = px;
                    y = py;
                }
            }
            'H' => {
                for v in &args {
                    x = if rel { x + v } else { *v };
                    out.push(Seg::Line(x, y));
                }
            }
            'V' => {
                for v in &args {
                    y = if rel { y + v } else { *v };
                    out.push(Seg::Line(x, y));
                }
            }
            'C' => {
                for p in args.chunks_exact(6) {
                    let (ax, ay) = if rel { (x, y) } else { (0.0, 0.0) };
                    out.push(Seg::Curve(ax + p[0], ay + p[1], ax + p[2], ay + p[3], ax + p[4], ay + p[5]));
                    x = ax + p[4];
                    y = ay + p[5];
                }
            }
            'A' => {
                for p in args.chunks_exact(7) {
                    let (ex, ey) = (bx + p[5], by + p[6]);
                    arc(&mut out, x, y, p[0], p[1], p[2], p[3] != 0.0, p[4] != 0.0, ex, ey);
                    x = ex;
                    y = ey;
                }
            }
            'Z' => {
                out.push(Seg::Close);
                x = sx;
                y = sy;
            }
            _ => {}
        }
    }
    out
}

/// Strokes `segs` (24-unit grid) into a square of `size` at (0,0), in the current colour.
pub fn stroke(cr: &gtk::cairo::Context, segs: &[Seg], size: f64) {
    let k = size / 24.0;
    cr.set_line_width(1.5 * k);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    for s in segs {
        match *s {
            Seg::Move(x, y) => cr.move_to(x * k, y * k),
            Seg::Line(x, y) => {
                // A zero-length line is how a dot is drawn ("h.01"); round caps make it one.
                cr.line_to(x * k, y * k)
            }
            Seg::Curve(a, b, c, d, e, f) => cr.curve_to(a * k, b * k, c * k, d * k, e * k, f * k),
            Seg::Close => cr.close_path(),
        }
    }
    let _ = cr.stroke();
}

/// A widget that draws the glyph at `size` logical pixels, in white (or `colour`).
pub fn widget(d: &str, size: i32) -> gtk::DrawingArea {
    let segs = parse(d);
    let area = gtk::DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size);
    area.add_css_class("hub-glyph");
    area.set_draw_func(move |a, cr, w, h| {
        let c = a.color();
        cr.set_source_rgba(c.red().into(), c.green().into(), c.blue().into(), c.alpha().into());
        let s = f64::from(w.min(h));
        cr.translate((f64::from(w) - s) / 2.0, (f64::from(h) - s) / 2.0);
        stroke(cr, &segs, s);
    });
    area
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_relative_and_dots() {
        let s = parse("M4 6h9M17 6h3");
        assert_eq!(s, vec![Seg::Move(4.0, 6.0), Seg::Line(13.0, 6.0), Seg::Move(17.0, 6.0), Seg::Line(20.0, 6.0)]);
        let dot = parse("M12 19.5h.01");
        assert_eq!(dot[1], Seg::Line(12.01, 19.5));
    }

    #[test]
    fn arcs_end_where_they_say() {
        let s = parse("M2 9a15 15 0 0 1 20 0");
        let Some(Seg::Curve(.., x, y)) = s.last().copied() else { panic!("no curve") };
        assert!((x - 22.0).abs() < 1e-6 && (y - 9.0).abs() < 1e-6);
    }

    #[test]
    fn numbers_run_together() {
        assert_eq!(numbers("1.5.5-3"), vec![1.5, 0.5, -3.0]);
    }
}
