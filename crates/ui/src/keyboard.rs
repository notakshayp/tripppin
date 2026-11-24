//! Keyboard: layouts, the keys that switch between them and key repeat, written to
//! ~/.config/niri/tripppin-keyboard.kdl (niri merges its `input` block with the system's).

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Keyboard {
    /// xkb layout codes, first is the default: ["us", "in"]
    pub layouts: Vec<String>,
    /// An xkb grp: option, e.g. "grp:alt_shift_toggle"
    pub switch: String,
    /// Milliseconds before a held key repeats.
    pub delay: u32,
    /// Repeats per second.
    pub rate: u32,
}

impl Default for Keyboard {
    fn default() -> Self {
        Keyboard { layouts: vec!["us".into()], switch: SWITCHES[0].0.into(), delay: 600, rate: 25 }
    }
}

/// Ways to switch layout. Super+Space is the orb's, so it isn't offered.
pub const SWITCHES: [(&str, &str); 4] = [
    ("grp:alt_shift_toggle", "Alt + Shift"),
    ("grp:ctrl_shift_toggle", "Ctrl + Shift"),
    ("grp:caps_toggle", "Caps lock"),
    ("grp:menu_toggle", "Menu key"),
];
pub const DELAYS: [(u32, &str); 3] = [(250, "short"), (400, "medium"), (600, "long")];
pub const RATES: [(u32, &str); 3] = [(15, "slow"), (25, "normal"), (40, "fast")];

/// Layouts xkb knows, (code, name), from evdev.lst's "! layout" section.
pub fn parse_layouts(lst: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in lst.lines() {
        if line.starts_with('!') {
            inside = line.trim() == "! layout";
            continue;
        }
        if !inside {
            continue;
        }
        let line = line.trim();
        if let Some((code, name)) = line.split_once(char::is_whitespace)
            && code != "custom"
        {
            out.push((code.to_string(), name.trim().to_string()));
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

pub fn all_layouts() -> Vec<(String, String)> {
    parse_layouts(&std::fs::read_to_string("/usr/share/X11/xkb/rules/evdev.lst").unwrap_or_default())
}

fn file() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("niri/tripppin-keyboard.kdl")
}

pub fn to_kdl(k: &Keyboard) -> String {
    format!(
        "// Keyboard, written by tripppin settings.\ninput {{\n    keyboard {{\n        xkb {{\n            layout \"{}\"\n            options \"{}\"\n        }}\n        repeat-delay {}\n        repeat-rate {}\n    }}\n}}\n",
        k.layouts.join(","),
        k.switch,
        k.delay,
        k.rate
    )
}

pub fn parse_kdl(text: &str) -> Keyboard {
    let mut k = Keyboard::default();
    let quoted = |l: &str| l.split('"').nth(1).unwrap_or("").to_string();
    for line in text.lines().map(str::trim) {
        if line.starts_with("layout ") {
            k.layouts = quoted(line).split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
        } else if line.starts_with("options ") {
            k.switch = quoted(line);
        } else if let Some(v) = line.strip_prefix("repeat-delay ") {
            k.delay = v.trim().parse().unwrap_or(k.delay);
        } else if let Some(v) = line.strip_prefix("repeat-rate ") {
            k.rate = v.trim().parse().unwrap_or(k.rate);
        }
    }
    if k.layouts.is_empty() {
        k.layouts = Keyboard::default().layouts;
    }
    k
}

pub fn current() -> Keyboard {
    std::fs::read_to_string(file()).map(|t| parse_kdl(&t)).unwrap_or_default()
}

/// Saves the settings; niri notices the file change and applies them at once.
pub fn save(k: &Keyboard) -> std::io::Result<()> {
    let path = file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, to_kdl(k))?;
    crate::display::include_in_niri("tripppin-keyboard.kdl")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_and_settings_round_trip() {
        let lst = "! model\n  pc105 Generic\n\n! layout\n  custom          A user-defined custom Layout\n  us              English (US)\n  in              Indian\n\n! variant\n  dvorak us: English (Dvorak)\n";
        assert_eq!(parse_layouts(lst), vec![("us".into(), "English (US)".into()), ("in".into(), "Indian".into())]);
        let k = Keyboard {
            layouts: vec!["us".into(), "in".into()],
            switch: "grp:caps_toggle".into(),
            delay: 250,
            rate: 40,
        };
        assert_eq!(parse_kdl(&to_kdl(&k)), k);
        assert_eq!(parse_kdl(""), Keyboard::default());
    }
}
