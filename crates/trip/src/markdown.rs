//! A small Markdown renderer for the terminal that works one line at a time, so an answer can
//! be drawn while it streams in. It covers what models write in answers: headings, bold,
//! italics, inline code, links, lists (with task boxes), quotes, rules, simple tables and
//! fenced code blocks, which get a frame. Text is wrapped to the width it's given.

use crate::term::{ACCENT, BOLD, DIM, FOREST, ITALIC, RESET, width};
use unicode_width::UnicodeWidthChar;

/// How the output may look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// Colours and bold/italic.
    pub color: bool,
    /// Box-drawing characters and bullets; plain ASCII otherwise (the Linux console).
    pub unicode: bool,
}

impl Theme {
    pub const PLAIN: Theme = Theme { color: false, unicode: true };
    pub const FULL: Theme = Theme { color: true, unicode: true };
}

/// How a run of text looks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Attr {
    bold: bool,
    italic: bool,
    code: bool,
    link: bool,
    dim: bool,
    accent: bool,
}

impl Attr {
    fn sgr(self) -> String {
        let mut s = String::new();
        if self.bold {
            s += BOLD;
        }
        if self.italic {
            s += ITALIC;
        }
        if self.dim {
            s += DIM;
        }
        if self.link {
            s += "\x1b[4m";
        }
        if self.code || self.accent {
            s += ACCENT;
        }
        s
    }
}

type Spans = Vec<(String, Attr)>;

/// The renderer's state between lines: whether a code block is open, and the rows of a table
/// that's still coming (a table is drawn once it ends, so its columns can line up).
#[derive(Clone, Debug)]
pub struct Markdown {
    theme: Theme,
    /// The open fence (``` or ~~~) while inside a code block.
    fence: Option<String>,
    table: Vec<String>,
    width: usize,
}

impl Markdown {
    pub fn new(theme: Theme) -> Self {
        Markdown { theme, fence: None, table: Vec::new(), width: 80 }
    }

    pub fn in_code(&self) -> bool {
        self.fence.is_some()
    }

    /// The rows for a line that's still being written, including a table held back so far.
    /// The state isn't changed, so the same line can be drawn again as more of it arrives.
    pub fn partial(&self, src: &str, width: usize) -> Vec<String> {
        let mut md = self.clone();
        let mut rows = md.line(src, width);
        rows.extend(md.flush_table(width.max(12)));
        rows
    }

    /// What's held back (a table still coming), drawn as it would be if it ended now.
    pub fn pending(&self, width: usize) -> Vec<String> {
        self.clone().flush_table(width.max(12))
    }

    /// Ends the answer: draws a table still held back and closes a code block left open.
    pub fn finish(&mut self) -> Vec<String> {
        let mut rows = self.flush_table(self.width);
        if self.fence.take().is_some() {
            rows.push(self.frame_end());
        }
        rows
    }

    /// The rows for one finished line of Markdown, each at most `width` columns. A table row
    /// gives none: the table comes out whole when a line that isn't part of it arrives.
    pub fn line(&mut self, src: &str, width: usize) -> Vec<String> {
        let width = width.max(12);
        self.width = width;
        let trimmed = src.trim();
        if self.fence.is_none() && trimmed.starts_with('|') {
            self.table.push(trimmed.to_string());
            return Vec::new();
        }
        let mut rows = self.flush_table(width);
        rows.extend(self.block(src, width));
        rows
    }

    fn block(&mut self, src: &str, width: usize) -> Vec<String> {
        let src = src.trim_end_matches(['\r', '\n']).replace('\t', "    ");
        let trimmed = src.trim_start();
        let indent = src.len() - trimmed.len();

        if let Some(fence) = &self.fence {
            if trimmed.starts_with(fence.as_str())
                && trimmed.trim_start_matches(fence.chars().next().unwrap_or('`')).trim().is_empty()
            {
                self.fence = None;
                return vec![self.frame_end()];
            }
            return self.code_line(&src, width);
        }
        for fence in ["```", "~~~"] {
            if let Some(lang) = trimmed.strip_prefix(fence) {
                let mark = trimmed.chars().take_while(|c| *c == fence.chars().next().unwrap_or('`')).collect();
                self.fence = Some(mark);
                let lang = lang.trim_start_matches(['`', '~']).trim();
                return vec![self.frame_start(lang)];
            }
        }
        if trimmed.is_empty() {
            return vec![String::new()];
        }
        if let Some((level, text)) = heading(trimmed) {
            let attr = Attr { bold: true, accent: level <= 2, ..Attr::default() };
            return self.wrap(&inline(text, attr), width, "", "");
        }
        if is_rule(trimmed) {
            let rule = if self.theme.unicode { "─" } else { "-" };
            return vec![self.paint(&rule.repeat(width.min(36)), Attr { dim: true, ..Attr::default() })];
        }
        if let Some(rest) = trimmed.strip_prefix('>') {
            let bar = self.paint(if self.theme.unicode { "│ " } else { "| " }, Attr { dim: true, ..Attr::default() });
            let attr = Attr { italic: true, ..Attr::default() };
            return self.wrap(&inline(rest.trim_start(), attr), width, &bar, &bar);
        }
        let pad = " ".repeat((indent / 2 * 2).min(8));
        if let Some((marker, text)) = list_item(trimmed) {
            let (first, text) = match marker {
                Marker::Bullet => {
                    let (task, text) = task_box(text);
                    let mark = match task {
                        Some(true) => self.paint(
                            if self.theme.unicode { "✓" } else { "x" },
                            Attr { accent: true, ..Attr::default() },
                        ),
                        Some(false) => self
                            .paint(if self.theme.unicode { "○" } else { "o" }, Attr { dim: true, ..Attr::default() }),
                        None => self.paint(
                            if self.theme.unicode { "•" } else { "*" },
                            Attr { accent: true, ..Attr::default() },
                        ),
                    };
                    (format!("{pad}{mark} "), text)
                }
                Marker::Number(n) => {
                    (format!("{pad}{} ", self.paint(&n, Attr { accent: true, ..Attr::default() })), text)
                }
            };
            let rest = " ".repeat(width_of(&first));
            return self.wrap(&inline(text, Attr::default()), width, &first, &rest);
        }
        self.wrap(&inline(trimmed, Attr::default()), width, &pad, &pad)
    }

    fn paint(&self, text: &str, attr: Attr) -> String {
        if !self.theme.color || attr == Attr::default() {
            return text.to_string();
        }
        format!("{}{text}{RESET}", attr.sgr())
    }

    fn frame(&self, text: &str) -> String {
        if self.theme.color { format!("{FOREST}{text}{RESET}") } else { text.to_string() }
    }

    fn frame_start(&self, lang: &str) -> String {
        let corner = if self.theme.unicode { "╭─" } else { "+-" };
        if lang.is_empty() {
            self.frame(corner)
        } else {
            format!("{} {}", self.frame(corner), self.paint(lang, Attr { dim: true, ..Attr::default() }))
        }
    }

    fn frame_end(&self) -> String {
        self.frame(if self.theme.unicode { "╰─" } else { "+-" })
    }

    /// A line of code inside the frame: kept as written, cut into rows when it's too long.
    fn code_line(&self, src: &str, width: usize) -> Vec<String> {
        let bar = self.frame(if self.theme.unicode { "│ " } else { "| " });
        let room = width.saturating_sub(2).max(4);
        let mut rows = Vec::new();
        let mut row = String::new();
        let mut used = 0;
        for c in src.chars() {
            let w = c.width().unwrap_or(0);
            if used + w > room {
                rows.push(std::mem::take(&mut row));
                used = 0;
            }
            row.push(c);
            used += w;
        }
        rows.push(row);
        rows.into_iter().map(|r| format!("{bar}{}", self.paint(&r, Attr { code: true, ..Attr::default() }))).collect()
    }

    /// Styled text on one line, without wrapping.
    fn paint_spans(&self, spans: &Spans) -> String {
        spans
            .iter()
            .map(|(t, a)| if !self.theme.color && a.code { format!("`{t}`") } else { self.paint(t, *a) })
            .collect()
    }

    /// The table rows held back so far, drawn with their columns lined up. A table too wide
    /// for the width falls back to one row per line with thin bars between the cells.
    fn flush_table(&mut self, width: usize) -> Vec<String> {
        if self.table.is_empty() {
            return Vec::new();
        }
        let lines = std::mem::take(&mut self.table);
        let split = |line: &str| -> Vec<String> {
            let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
            inner.split('|').map(|c| c.trim().to_string()).collect()
        };
        let is_divider =
            |cells: &[String]| cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')));
        let rows: Vec<Vec<String>> = lines.iter().map(|l| split(l)).collect();
        let divider = rows.iter().position(|r| is_divider(r));
        let right: Vec<bool> = divider
            .map(|d| rows[d].iter().map(|c| c.ends_with(':') && !c.starts_with(':')).collect())
            .unwrap_or_default();
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        let painted: Vec<Option<Vec<String>>> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                (Some(i) != divider).then(|| {
                    let header = divider.is_some_and(|d| i < d);
                    let attr = Attr { bold: header, ..Attr::default() };
                    r.iter().map(|c| self.paint_spans(&inline(c, attr))).collect()
                })
            })
            .collect();
        let mut widths = vec![0; cols];
        for cells in painted.iter().flatten() {
            for (i, c) in cells.iter().enumerate() {
                widths[i] = widths[i].max(width_of(c));
            }
        }
        let dim = Attr { dim: true, ..Attr::default() };
        let (bar, line, cross) = if self.theme.unicode { (" │ ", "─", "─┼─") } else { (" | ", "-", "-+-") };
        let total: usize = widths.iter().sum::<usize>() + 3 * cols.saturating_sub(1);
        if total > width {
            // Too wide to line up: each row as text, wrapped.
            return painted
                .iter()
                .flat_map(|cells| match cells {
                    None => vec![self.paint(&line.repeat(width.min(36)), dim)],
                    Some(cells) => {
                        let mut spans = Spans::new();
                        for (i, c) in cells.iter().enumerate() {
                            if i > 0 {
                                spans.push((bar.to_string(), dim));
                            }
                            spans.push((crate::term::strip_ansi(c), Attr::default()));
                        }
                        self.wrap(&spans, width, "", "")
                    }
                })
                .collect();
        }
        painted
            .iter()
            .map(|cells| match cells {
                None => self.paint(&widths.iter().map(|w| line.repeat(*w)).collect::<Vec<_>>().join(cross), dim),
                Some(cells) => {
                    let mut row = String::new();
                    for (i, w) in widths.iter().enumerate() {
                        if i > 0 {
                            row += &self.paint(bar, dim);
                        }
                        let cell = cells.get(i).map_or("", String::as_str);
                        let pad = " ".repeat(w - width_of(cell));
                        if right.get(i).copied().unwrap_or(false) {
                            row += &pad;
                            row += cell;
                        } else {
                            row += cell;
                            if i + 1 < widths.len() {
                                row += &pad;
                            }
                        }
                    }
                    row
                }
            })
            .collect()
    }

    /// Fills rows up to `width` word by word. `first` and `rest` start the first and the
    /// following rows (a bullet, then the space under it).
    fn wrap(&self, spans: &Spans, width: usize, first: &str, rest: &str) -> Vec<String> {
        let words = words(spans, self.theme);
        let mut rows = Vec::new();
        let mut row = first.to_string();
        let mut used = width_of(first);
        let mut empty = true;
        for word in words {
            let w: usize = word.iter().map(|(t, _)| width_of(t)).sum();
            if !empty && used + 1 + w > width {
                rows.push(std::mem::take(&mut row));
                row = rest.to_string();
                used = width_of(rest);
                empty = true;
            }
            if !empty {
                row.push(' ');
                used += 1;
            }
            for (text, attr) in word {
                // A word longer than the row is cut wherever the row ends.
                let mut piece = String::new();
                for c in text.chars() {
                    let cw = c.width().unwrap_or(0);
                    if used + cw > width && used > width_of(rest) {
                        row += &self.paint(&std::mem::take(&mut piece), attr);
                        rows.push(std::mem::take(&mut row));
                        row = rest.to_string();
                        used = width_of(rest);
                    }
                    piece.push(c);
                    used += cw;
                }
                row += &self.paint(&piece, attr);
            }
            empty = false;
        }
        rows.push(row);
        rows
    }
}

fn width_of(text: &str) -> usize {
    width(text)
}

/// Splits styled text into words (runs without spaces), each a list of styled pieces. Without
/// colour, the marks that carry meaning stay as text: `code` keeps its backticks.
fn words(spans: &Spans, theme: Theme) -> Vec<Spans> {
    let mut words = Vec::new();
    let mut word = Spans::new();
    for (text, attr) in spans {
        let text = if !theme.color && attr.code { format!("`{text}`") } else { text.clone() };
        let mut piece = String::new();
        for c in text.chars() {
            if c.is_whitespace() && !attr.code {
                if !piece.is_empty() {
                    word.push((std::mem::take(&mut piece), *attr));
                }
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            } else {
                piece.push(c);
            }
        }
        if !piece.is_empty() {
            word.push((piece, *attr));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some((level, rest.trim().trim_end_matches('#').trim_end()))
}

fn is_rule(line: &str) -> bool {
    let chars: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    chars.len() >= 3 && ['-', '*', '_'].iter().any(|m| chars.iter().all(|c| c == m))
}

enum Marker {
    Bullet,
    Number(String),
}

fn list_item(line: &str) -> Option<(Marker, &str)> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(bullet) {
            return Some((Marker::Bullet, rest.trim_start()));
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        let rest = &line[digits..];
        for end in [". ", ") "] {
            if let Some(text) = rest.strip_prefix(end) {
                return Some((Marker::Number(format!("{}.", &line[..digits])), text.trim_start()));
            }
        }
    }
    None
}

fn task_box(text: &str) -> (Option<bool>, &str) {
    for (mark, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
        if let Some(rest) = text.strip_prefix(mark) {
            return (Some(done), rest);
        }
    }
    (None, text)
}

/// Inline Markdown: `code`, **bold**, *italic* / _italic_, [links](url) and \escapes. Marks
/// without a partner (a lone `*`, or one still streaming in) stay as text.
fn inline(text: &str, base: Attr) -> Spans {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Spans::new();
    let mut plain = String::new();
    let push = |out: &mut Spans, text: String, attr: Attr| {
        if text.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((t, a)) if *a == attr => t.push_str(&text),
            _ => out.push((text, attr)),
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && chars.get(i + 1).is_some_and(|n| n.is_ascii_punctuation()) {
            plain.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '`' {
            let run = count(&chars, i, '`');
            if let Some(end) = find_run(&chars, i + run, '`', run) {
                push(&mut out, std::mem::take(&mut plain), base);
                let code: String = chars[i + run..end].iter().collect();
                let code = if code.len() > 2 && code.starts_with(' ') && code.ends_with(' ') {
                    code[1..code.len() - 1].to_string()
                } else {
                    code
                };
                push(&mut out, code, Attr { code: true, ..base });
                i = end + run;
                continue;
            }
            plain.extend(&chars[i..i + run]);
            i += run;
            continue;
        }
        if c == '*' || c == '_' {
            let run = count(&chars, i, c).min(3);
            let opens = chars.get(i + run).is_some_and(|n| !n.is_whitespace());
            let word_before = i > 0 && chars[i - 1].is_alphanumeric();
            if opens
                && !(c == '_' && word_before)
                && let Some(end) = find_closer(&chars, i + run, c, run)
            {
                push(&mut out, std::mem::take(&mut plain), base);
                let inner: String = chars[i + run..end].iter().collect();
                let attr = Attr { bold: base.bold || run >= 2, italic: base.italic || run != 2, ..base };
                for (t, a) in inline(&inner, attr) {
                    push(&mut out, t, a);
                }
                i = end + run;
                continue;
            }
            plain.extend(&chars[i..i + run]);
            i += run;
            continue;
        }
        if c == '['
            && let Some((label_end, url_end)) = link(&chars, i)
        {
            push(&mut out, std::mem::take(&mut plain), base);
            let label: String = chars[i + 1..label_end].iter().collect();
            let url: String = chars[label_end + 2..url_end].iter().collect();
            for (t, a) in inline(&label, Attr { link: true, ..base }) {
                push(&mut out, t, a);
            }
            if !url.is_empty() && url != label {
                push(&mut out, format!(" ({url})"), Attr { dim: true, ..base });
            }
            i = url_end + 1;
            continue;
        }
        plain.push(c);
        i += 1;
    }
    push(&mut out, plain, base);
    out
}

fn count(chars: &[char], from: usize, c: char) -> usize {
    chars[from..].iter().take_while(|x| **x == c).count()
}

/// The start of the next run of exactly `len` `c`s at or after `from`.
fn find_run(chars: &[char], from: usize, c: char, len: usize) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == c {
            let run = count(chars, i, c);
            if run == len {
                return Some(i);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

/// Where emphasis opened with `len` `c`s closes: the same run, right after a non-space,
/// and (for `_`) not inside a word.
fn find_closer(chars: &[char], from: usize, c: char, len: usize) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '`' {
            // Code spans hide their contents from emphasis.
            let run = count(chars, i, '`');
            i = find_run(chars, i + run, '`', run).map_or(i + run, |e| e + run);
            continue;
        }
        if chars[i] == c {
            let run = count(chars, i, c);
            let after_word = chars.get(i + run).is_some_and(|n| n.is_alphanumeric());
            if run >= len && i > from && !chars[i - 1].is_whitespace() && !(c == '_' && after_word) {
                return Some(i);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

/// `[label](url)` starting at `from`: where the label ends (at `]`) and where the url ends (at `)`).
fn link(chars: &[char], from: usize) -> Option<(usize, usize)> {
    let close = (from + 1..chars.len()).find(|&j| chars[j] == ']')?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = (close + 2..chars.len()).find(|&j| chars[j] == ')')?;
    let url = &chars[close + 2..end];
    (!url.iter().any(|c| c.is_whitespace())).then_some((close, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::strip_ansi;

    fn plain(src: &str, width: usize) -> Vec<String> {
        let mut md = Markdown::new(Theme::PLAIN);
        let mut rows: Vec<String> = src.lines().flat_map(|l| md.line(l, width)).collect();
        rows.extend(md.finish());
        rows
    }

    fn styled(src: &str) -> String {
        let mut md = Markdown::new(Theme::FULL);
        src.lines().flat_map(|l| md.line(l, 60)).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn bold_italic_and_code() {
        let out = styled("this is **bold**, *soft* and `code`");
        assert!(out.contains(&format!("{BOLD}bold{RESET}")), "{out:?}");
        assert!(out.contains(&format!("{ITALIC}soft{RESET}")), "{out:?}");
        assert!(out.contains(&format!("{ACCENT}code{RESET}")), "{out:?}");
        assert_eq!(strip_ansi(&out), "this is bold, soft and code");
    }

    #[test]
    fn lone_marks_stay_text() {
        assert_eq!(plain("2 * 3 = 6", 40), ["2 * 3 = 6"]);
        assert_eq!(plain("snake_case_name and **unfinished", 40), ["snake_case_name and **unfinished"]);
        assert_eq!(plain("a ` b", 40), ["a ` b"]);
        assert_eq!(plain(r"\*not italic\*", 40), ["*not italic*"]);
    }

    #[test]
    fn code_keeps_backticks_without_colour() {
        assert_eq!(plain("run `ls -la` now", 40), ["run `ls -la` now"]);
    }

    #[test]
    fn emphasis_ignores_marks_inside_code() {
        let out = styled("*use `a*b` here*");
        assert_eq!(strip_ansi(&out), "use a*b here");
    }

    #[test]
    fn links_show_their_address() {
        assert_eq!(plain("see [the docs](https://x.io/d)", 60), ["see the docs (https://x.io/d)"]);
        assert_eq!(plain("[https://x.io](https://x.io)", 60), ["https://x.io"]);
    }

    #[test]
    fn headings_rules_and_quotes() {
        assert_eq!(plain("## Disk usage", 40), ["Disk usage"]);
        assert_eq!(plain("#hashtag", 40), ["#hashtag"]);
        assert_eq!(plain("---", 10), ["─".repeat(12)]);
        assert_eq!(plain("> careful", 40), ["│ careful"]);
        assert!(styled("# Title").contains(BOLD));
    }

    #[test]
    fn lists_hang_their_text() {
        let rows = plain("- one two three four five six", 16);
        assert_eq!(rows, ["• one two three", "  four five six"]);
        assert_eq!(plain("  - nested", 40), ["  • nested"]);
        assert_eq!(plain("3. third", 40), ["3. third"]);
        assert_eq!(plain("- [x] done\n- [ ] todo", 40), ["✓ done", "○ todo"]);
    }

    #[test]
    fn wraps_to_width() {
        let text = "the quick brown fox jumps over the lazy dog and keeps running far away";
        for w in [12, 20, 33, 80] {
            let rows = plain(text, w);
            assert!(rows.iter().all(|r| width(r) <= w), "{w}: {rows:?}");
            assert_eq!(rows.join(" "), text);
        }
        // A word longer than the row is cut.
        let rows = plain(&"x".repeat(30), 12);
        assert_eq!(rows, ["x".repeat(12), "x".repeat(12), "x".repeat(6)]);
    }

    #[test]
    fn wide_characters_wrap_by_columns() {
        let rows = plain("日本語 日本語 日本語 日本語", 14);
        assert!(rows.iter().all(|r| width(r) <= 14), "{rows:?}");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn code_blocks_get_a_frame() {
        let rows = plain("```bash\nls -la\n  echo hi\n```\nafter", 40);
        assert_eq!(rows, ["╭─ bash", "│ ls -la", "│   echo hi", "╰─", "after"]);
    }

    #[test]
    fn code_inside_blocks_is_left_alone() {
        let rows = plain("```\n# not a heading\n**not bold**\n```", 40);
        assert_eq!(rows, ["╭─", "│ # not a heading", "│ **not bold**", "╰─"]);
    }

    #[test]
    fn long_code_lines_are_cut_into_rows() {
        let rows = plain(&format!("```\n{}\n```", "y".repeat(25)), 12);
        assert_eq!(rows[1..4], ["│ yyyyyyyyyy", "│ yyyyyyyyyy", "│ yyyyy"]);
    }

    #[test]
    fn an_unfinished_block_is_closed_at_the_end() {
        let mut md = Markdown::new(Theme::PLAIN);
        md.line("```", 40);
        md.line("code", 40);
        assert!(md.in_code());
        assert_eq!(md.finish(), ["╰─"]);
        assert!(!md.in_code());
    }

    #[test]
    fn partial_lines_leave_the_state_alone() {
        let md = Markdown::new(Theme::PLAIN);
        assert_eq!(md.partial("```py", 40), ["╭─ py"]);
        assert!(!md.in_code());
        assert_eq!(md.partial("some **bo", 40), ["some **bo"]);
    }

    #[test]
    fn tables_get_thin_dividers() {
        let rows = plain("| name | size |\n|---|---:|\n| ~/Downloads | 8 GB |\n| ~/.cache | 620 MB |\nafter", 40);
        assert_eq!(
            rows,
            ["name        │   size", "────────────┼───────", "~/Downloads │   8 GB", "~/.cache    │ 620 MB", "after"]
        );
    }

    #[test]
    fn tables_wait_for_their_last_row() {
        let mut md = Markdown::new(Theme::PLAIN);
        assert!(md.line("| a | b |", 40).is_empty());
        assert_eq!(md.pending(40), ["a │ b"]);
        assert_eq!(md.partial("| ccc | d |", 40), ["a   │ b", "ccc │ d"]);
        assert_eq!(md.line("text", 40), ["a │ b", "text"]);
    }

    #[test]
    fn wide_tables_fall_back_to_rows() {
        let rows = plain(&format!("| {} | {} |", "a ".repeat(10), "b ".repeat(10)), 20);
        assert!(rows.iter().all(|r| width(r) <= 20), "{rows:?}");
        assert!(rows.len() >= 2);
    }

    #[test]
    fn ascii_theme_for_the_console() {
        let mut md = Markdown::new(Theme { color: false, unicode: false });
        let rows: Vec<String> = "- a\n```\nx\n```".lines().flat_map(|l| md.line(l, 20)).collect();
        assert_eq!(md.line("|a|b|", 20), Vec::<String>::new());
        assert_eq!(md.finish(), ["a | b"]);
        assert_eq!(rows, ["* a", "+-", "| x", "+-"]);
    }
}
