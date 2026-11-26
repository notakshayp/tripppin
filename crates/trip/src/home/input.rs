//! A one-line text field: typing, cursor movement, editing, and a masked mode for keys.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    pub text: String,
    /// Cursor position in characters.
    pub cursor: usize,
    pub masked: bool,
}

impl Input {
    pub fn new(text: &str) -> Self {
        Input { text: text.to_string(), cursor: text.chars().count(), masked: false }
    }

    pub fn secret() -> Self {
        Input { masked: true, ..Input::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    fn byte(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map_or(self.text.len(), |(i, _)| i)
    }

    pub fn insert(&mut self, s: &str) {
        let s: String = s.chars().filter(|c| !c.is_control()).collect();
        let at = self.byte(self.cursor);
        self.text.insert_str(at, &s);
        self.cursor += s.chars().count();
    }

    /// What's drawn: the text, or dots when masked.
    pub fn shown(&self) -> String {
        if self.masked { "•".repeat(self.text.chars().count()) } else { self.text.clone() }
    }

    /// Handles an editing key. Returns whether it was one (Enter and Esc are left to the caller).
    pub fn key(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('u') if ctrl => {
                let at = self.byte(self.cursor);
                self.text.drain(..at);
                self.cursor = 0;
            }
            KeyCode::Char('w') if ctrl => {
                let before: Vec<char> = self.text.chars().take(self.cursor).collect();
                let mut i = before.len();
                while i > 0 && before[i - 1] == ' ' {
                    i -= 1;
                }
                while i > 0 && before[i - 1] != ' ' {
                    i -= 1;
                }
                let (a, b) = (self.byte(i), self.byte(self.cursor));
                self.text.drain(a..b);
                self.cursor = i;
            }
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.text.chars().count(),
            KeyCode::Char(c) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => self.insert(&c.to_string()),
            KeyCode::Backspace if self.cursor > 0 => {
                let (a, b) = (self.byte(self.cursor - 1), self.byte(self.cursor));
                self.text.drain(a..b);
                self.cursor -= 1;
            }
            KeyCode::Delete if self.cursor < self.text.chars().count() => {
                let (a, b) = (self.byte(self.cursor), self.byte(self.cursor + 1));
                self.text.drain(a..b);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.text.chars().count()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.chars().count(),
            KeyCode::Backspace | KeyCode::Delete => {}
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(i: &mut Input, code: KeyCode, m: KeyModifiers) {
        i.key(KeyEvent::new(code, m));
    }

    #[test]
    fn edits_text_by_characters() {
        let mut i = Input::default();
        for c in "héllo".chars() {
            press(&mut i, KeyCode::Char(c), KeyModifiers::NONE);
        }
        assert_eq!((i.text.as_str(), i.cursor), ("héllo", 5));
        press(&mut i, KeyCode::Left, KeyModifiers::NONE);
        press(&mut i, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(i.text, "hélo");
        press(&mut i, KeyCode::Home, KeyModifiers::NONE);
        press(&mut i, KeyCode::Delete, KeyModifiers::NONE);
        assert_eq!(i.text, "élo");
        press(&mut i, KeyCode::End, KeyModifiers::NONE);
        press(&mut i, KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!(i.text, "");
        i.insert("a b\nc");
        assert_eq!(i.text, "a bc");
        assert_eq!(i.take(), "a bc");
        assert!(i.is_empty());
    }

    #[test]
    fn masks_secrets() {
        let mut i = Input::secret();
        i.insert("abc");
        assert_eq!(i.shown(), "•••");
        assert_eq!(i.text, "abc");
    }
}
