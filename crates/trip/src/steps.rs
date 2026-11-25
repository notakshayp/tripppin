//! The AI's plan as flat coloured rows in a right-hand column beside the shell (Tiles mock
//! 05): done steps are solid green blocks, the running one is amber with marching dots,
//! the next ones are dim, and the token line sits at the bottom. Only when the terminal is
//! wide enough; narrower ones keep the inline checklist.

use crate::model::PlanStep;
use crate::term::{self, RESET};

/// The narrowest terminal that gets the column.
pub const MIN_WIDTH: usize = 100;
/// Columns between the shell text and the column.
pub const GAP: usize = 2;

const WHITE: &str = "\x1b[38;2;255;255;255m";
const MUTED: &str = "\x1b[38;2;154;164;171m";
const FAINT: &str = "\x1b[38;2;230;236;232m";
const DONE_BG: &str = "\x1b[48;2;31;107;69m";
const RUN_BG: &str = "\x1b[48;2;184;134;43m";
const NEXT_BG: &str = "\x1b[48;2;29;36;41m";

/// How wide the column is on a terminal `width` columns wide; `None` below `MIN_WIDTH`,
/// where the steps stay inline.
pub fn side_width(width: usize) -> Option<usize> {
    (width >= MIN_WIDTH).then(|| (width * 3 / 10).clamp(28, 38))
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum State {
    Done,
    Running,
    Next,
}

/// A row of exactly `col` columns: `text` padded after one column of margin.
fn cell(bg: &str, fg: &str, text: &str, col: usize, color: bool) -> String {
    let body = term::truncate(text, col.saturating_sub(2));
    let pad = col.saturating_sub(1 + term::width(&body));
    let row = format!(" {body}{}", " ".repeat(pad));
    if color { format!("{bg}{fg}{row}{RESET}") } else { row }
}

fn dots(tick: usize, unicode: bool, bg: &str, color: bool, col: usize) -> String {
    let (on, off) = if unicode { ("▪", "▪") } else { ("*", ".") };
    let mut row = String::from(" ");
    for i in 0..5 {
        let lit = (i + 5 - tick % 5) % 5 < 3;
        let glyph = if lit { on } else { off };
        if color {
            row += &format!("{}{glyph} ", if lit { WHITE } else { "\x1b[38;2;214;178;108m" });
        } else {
            row += &format!("{glyph} ");
        }
    }
    let used = 1 + 10;
    let pad = " ".repeat(col.saturating_sub(used));
    if color { format!("{bg}{row}{pad}{RESET}") } else { format!("{row}{pad}") }
}

/// The rows of the column, each `col` columns wide (so they can be joined to the shell's
/// rows), at most `max_rows` of them with the token line last. `tick` moves the dots.
pub fn blocks(
    plan: &[PlanStep],
    tick: usize,
    col: usize,
    max_rows: usize,
    tokens: &str,
    color: bool,
    unicode: bool,
) -> Vec<String> {
    let running = plan.iter().position(|s| !s.done);
    let state = |i: usize| match running {
        Some(r) if i == r => State::Running,
        Some(r) if i > r => State::Next,
        _ => State::Done,
    };
    let height = |s: State| if s == State::Running { 3 } else { 2 };
    let reserve = if tokens.is_empty() { 0 } else { 2 };
    let budget = max_rows.saturating_sub(reserve).max(4);
    let need = |from: usize, gaps: bool| -> usize {
        (from..plan.len()).map(|i| height(state(i)) + usize::from(gaps)).sum::<usize>()
    };
    // Too many for the room: the oldest finished steps fold into one "n done" row, then
    // the gaps between blocks go.
    let gaps = need(0, true) <= budget;
    let mut first = 0;
    while !gaps
        && need(first, false) + usize::from(first > 0) > budget
        && first < running.unwrap_or(plan.len()).min(plan.len())
    {
        first += 1;
    }
    let mut rows = Vec::new();
    if first > 0 {
        rows.push(cell(DONE_BG, WHITE, &format!("{first} done"), col, color));
    }
    for (i, s) in plan.iter().enumerate().skip(first) {
        let st = state(i);
        let (bg, small, big) = match st {
            State::Done => (DONE_BG, FAINT, WHITE),
            State::Running => (RUN_BG, FAINT, WHITE),
            State::Next => (NEXT_BG, MUTED, MUTED),
        };
        let label = match st {
            State::Done => "done".to_string(),
            State::Running => format!("step {} of {}", i + 1, plan.len()),
            State::Next => "next".to_string(),
        };
        rows.push(cell(bg, small, &label, col, color));
        rows.push(cell(bg, big, &s.text, col, color));
        if st == State::Running {
            rows.push(dots(tick, unicode, bg, color, col));
        }
        if gaps && i + 1 < plan.len() {
            rows.push(" ".repeat(col));
        }
    }
    if !tokens.is_empty() {
        rows.push(" ".repeat(col));
        let t = term::truncate(tokens, col.saturating_sub(2));
        let pad = " ".repeat(col.saturating_sub(1 + term::width(&t)));
        rows.push(if color { format!(" {MUTED}{t}{RESET}{pad}") } else { format!(" {t}{pad}") });
    }
    rows
}

/// The shell's rows with the column down the right: each left row padded to `left_w`
/// columns, the column's rows after the gap. The token line (the column's last row) sits on
/// the last row of the result, so the column hangs from the top and the line from the bottom.
pub fn join(left: &[String], right: &[String], left_w: usize) -> Vec<String> {
    let n = left.len().max(right.len());
    (0..n)
        .map(|i| {
            let l = left.get(i).cloned().unwrap_or_default();
            let pad = " ".repeat(left_w.saturating_sub(term::width(&l)));
            match right.get(i) {
                Some(r) => format!("{l}{RESET}{pad}{}{r}", " ".repeat(GAP)),
                None => l,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::{strip_ansi, width};

    fn plan(done: usize, total: usize) -> Vec<PlanStep> {
        (0..total).map(|i| PlanStep { text: format!("Step number {}", i + 1), done: i < done }).collect()
    }

    #[test]
    fn the_column_needs_a_wide_terminal() {
        assert_eq!(side_width(80), None);
        assert_eq!(side_width(99), None);
        assert_eq!(side_width(100), Some(30));
        assert_eq!(side_width(120), Some(36));
        assert_eq!(side_width(300), Some(38));
    }

    #[test]
    fn rows_are_exactly_the_column_wide() {
        for color in [true, false] {
            let rows = blocks(&plan(2, 4), 1, 30, 40, "2.1k tokens · 12s", color, true);
            for r in &rows {
                assert_eq!(width(r), 30, "{:?}", strip_ansi(r));
            }
        }
        let long = vec![PlanStep { text: "x".repeat(80), done: false }];
        for r in blocks(&long, 0, 30, 40, "", true, true) {
            assert_eq!(width(&r), 30);
        }
    }

    #[test]
    fn done_running_and_next() {
        let rows: Vec<String> = blocks(&plan(1, 3), 0, 30, 40, "2.1k tokens", false, true)
            .iter()
            .map(|r| r.trim_end().to_string())
            .collect();
        let text = rows.join("\n");
        assert!(text.contains(" done\n Step number 1"), "{text}");
        assert!(text.contains(" step 2 of 3\n Step number 2\n ▪ ▪ ▪ ▪ ▪"), "{text}");
        assert!(text.contains(" next\n Step number 3"), "{text}");
        assert_eq!(rows.last().map(String::as_str), Some(" 2.1k tokens"));
        // colours: green, amber, dim
        let coloured = blocks(&plan(1, 3), 0, 30, 40, "", true, true).join("\n");
        assert!(coloured.contains("48;2;31;107;69"));
        assert!(coloured.contains("48;2;184;134;43"));
        assert!(coloured.contains("48;2;29;36;41"));
    }

    #[test]
    fn the_dots_march() {
        let at = |t| blocks(&plan(0, 1), t, 30, 40, "", false, false)[2].trim_end().to_string();
        assert_eq!(at(0), " * * * . .");
        assert_eq!(at(1), " . * * * .");
        assert_ne!(at(0), at(2));
        assert_eq!(at(0), at(5));
    }

    #[test]
    fn a_long_plan_folds_to_fit() {
        let rows = blocks(&plan(8, 12), 0, 30, 14, "tokens", false, true);
        assert!(rows.len() <= 14, "{} rows", rows.len());
        assert!(strip_ansi(&rows[0]).contains("done"));
        assert!(rows.iter().any(|r| r.contains("step 9 of 12")), "the running step stays");
        assert!(rows.iter().any(|r| r.contains("tokens")));
    }

    #[test]
    fn joining_keeps_the_column_aligned() {
        let left = vec!["› put invoices in folders".to_string(), "$ mkdir x".to_string()];
        let right = blocks(&plan(1, 2), 0, 30, 40, "", false, true);
        let rows = join(&left, &right, 60);
        assert_eq!(rows.len(), right.len());
        for (i, r) in rows.iter().enumerate() {
            let flat = strip_ansi(r);
            assert_eq!(width(&flat), 60 + GAP + 30, "row {i}: {flat:?}");
        }
        assert!(strip_ansi(&rows[0]).starts_with("› put invoices in folders"));
    }
}
