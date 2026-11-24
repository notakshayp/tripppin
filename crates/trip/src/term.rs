//! Terminal colours shared by the shell and the agent's terminal output, and the measuring
//! that drawing in a terminal needs: how many columns text takes once its colours are removed.

use unicode_width::UnicodeWidthChar;

/// Green #57d99a: the spark.
pub const ACCENT: &str = "\x1b[38;2;87;217;154m";
/// Forest #1f6b45: frames and quiet marks.
pub const FOREST: &str = "\x1b[38;2;31;107;69m";
/// Paper #f2f5ef: text that should stand out.
pub const PAPER: &str = "\x1b[38;2;242;245;239m";
/// A soft red for failures.
pub const RED: &str = "\x1b[38;2;240;113;113m";
pub const BOLD: &str = "\x1b[1m";
pub const ITALIC: &str = "\x1b[3m";
pub const DIM: &str = "\x1b[2m";
pub const RESET: &str = "\x1b[0m";

/// The text without its escape sequences (colours, cursor moves).
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // CSI: ESC [ … final byte in @–~
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: ESC ] … BEL or ESC \
            Some(']') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.peek() == Some(&'\\')) {
                        if c == '\x1b' {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// Columns the text takes on screen, ignoring escape sequences. Wide characters count two.
pub fn width(text: &str) -> usize {
    strip_ansi(text).chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Plain text cut to `max` columns, with `…` where it was cut.
pub fn truncate(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        used += w;
        out.push(c);
    }
    out.push('…');
    out
}

/// The text with its first letter capitalised, for labels and messages written in lowercase
/// elsewhere ("loading the model" → "Loading the model").
pub fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Output from a command made safe to show inside a frame: no escape sequences, no carriage
/// returns (progress bars keep only their newest state) and tabs as spaces.
pub fn sanitize(line: &str) -> String {
    let line = line.rsplit('\r').find(|part| !part.is_empty()).unwrap_or("");
    strip_ansi(line).replace('\t', "    ").chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_without_colours() {
        assert_eq!(width(&format!("{ACCENT}hi{RESET} there")), 8);
        assert_eq!(width("日本"), 4);
        assert_eq!(strip_ansi("\x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\"), "link");
    }

    #[test]
    fn truncates_to_columns() {
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hello world", 6), "hello…");
        assert_eq!(width(&truncate("日本語のテキスト", 7)), 7);
    }

    #[test]
    fn sentence_case() {
        assert_eq!(sentence("loading the model"), "Loading the model");
        assert_eq!(sentence(""), "");
        assert_eq!(sentence("échec"), "Échec");
    }

    #[test]
    fn sanitizes_command_output() {
        assert_eq!(sanitize("\x1b[32mok\x1b[0m\tdone"), "ok    done");
        assert_eq!(sanitize(" 10%\r 50%\r100%"), "100%");
        assert_eq!(sanitize("progress\r"), "progress");
    }
}
