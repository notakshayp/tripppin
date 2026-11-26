//! Keyboard: layouts, the keys that switch between them and key repeat. Layouts live in
//! kxkbrc (Plasma's keyboard daemon reloads on a D-Bus signal), repeat in kcminputrc (KWin
//! watches it).

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

/// The keyboard from the text of kxkbrc and kcminputrc.
pub fn parse(kxkbrc: &str, kcminputrc: &str) -> Keyboard {
    let mut k = Keyboard::default();
    if let Some(list) = crate::kde::ini_get(kxkbrc, "Layout", "LayoutList") {
        k.layouts = list.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
    }
    if k.layouts.is_empty() {
        k.layouts = Keyboard::default().layouts;
    }
    if let Some(options) = crate::kde::ini_get(kxkbrc, "Layout", "Options") {
        // Only the switching key is ours; any other option is kept as it was when saving.
        if let Some(s) = options.split(',').map(str::trim).find(|o| o.starts_with("grp:")) {
            k.switch = s.to_string();
        }
    }
    k.delay =
        crate::kde::ini_get(kcminputrc, "Keyboard", "RepeatDelay").and_then(|v| v.parse().ok()).unwrap_or(k.delay);
    k.rate = crate::kde::ini_get(kcminputrc, "Keyboard", "RepeatRate")
        .and_then(|v| v.parse::<f64>().ok())
        .map_or(k.rate, |r| r.round() as u32);
    k
}

/// kxkbrc's VariantList for `n` layouts: one empty variant each, so "us,in" pairs up.
pub fn variant_list(n: usize) -> String {
    ",".repeat(n.saturating_sub(1))
}

/// kxkbrc's Options with the switching key replaced by `switch`.
pub fn options_with(old: &str, switch: &str) -> String {
    let mut parts: Vec<&str> =
        old.split(',').map(str::trim).filter(|o| !o.is_empty() && !o.starts_with("grp:")).collect();
    parts.push(switch);
    parts.join(",")
}

pub fn current() -> Keyboard {
    parse(&crate::kde::file_text("kxkbrc"), &crate::kde::file_text("kcminputrc"))
}

/// Saves the settings and tells Plasma, which applies them at once.
pub fn save(k: &Keyboard) -> std::io::Result<()> {
    use crate::kde::{emit, read, write, write_bool};
    let old_options = read("kxkbrc", "Layout", "Options").unwrap_or_default();
    let ok = write_bool("kxkbrc", "Layout", "Use", true)
        && write("kxkbrc", "Layout", "LayoutList", &k.layouts.join(","))
        && write("kxkbrc", "Layout", "VariantList", &variant_list(k.layouts.len()))
        && write("kxkbrc", "Layout", "Options", &options_with(&old_options, &k.switch))
        && write_bool("kxkbrc", "Layout", "ResetOldOptions", true)
        && write("kcminputrc", "Keyboard", "RepeatDelay", &k.delay.to_string())
        && write("kcminputrc", "Keyboard", "RepeatRate", &k.rate.to_string());
    emit("/Layouts", "org.kde.keyboard", "reloadConfig");
    crate::kde::kwin_reconfigure();
    if ok { Ok(()) } else { Err(std::io::Error::other("kwriteconfig6 failed")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_layouts_xkb_knows() {
        let lst = "! model\n  pc105 Generic\n\n! layout\n  custom          A user-defined custom Layout\n  us              English (US)\n  in              Indian\n\n! variant\n  dvorak us: English (Dvorak)\n";
        assert_eq!(parse_layouts(lst), vec![("us".into(), "English (US)".into()), ("in".into(), "Indian".into())]);
    }

    #[test]
    fn reads_kxkbrc_and_kcminputrc() {
        let kxkb = "[Layout]\nLayoutList=us,in\nOptions=caps:escape,grp:caps_toggle\nUse=true\n";
        let input = "[Keyboard]\nRepeatDelay=250\nRepeatRate=40.0\n";
        assert_eq!(
            parse(kxkb, input),
            Keyboard {
                layouts: vec!["us".into(), "in".into()],
                switch: "grp:caps_toggle".into(),
                delay: 250,
                rate: 40
            }
        );
        assert_eq!(parse("", ""), Keyboard::default());
    }

    #[test]
    fn keeps_other_options_when_switching() {
        assert_eq!(
            options_with("caps:escape,grp:caps_toggle", "grp:alt_shift_toggle"),
            "caps:escape,grp:alt_shift_toggle"
        );
        assert_eq!(options_with("", "grp:menu_toggle"), "grp:menu_toggle");
        assert_eq!(variant_list(1), "");
        assert_eq!(variant_list(3), ",,");
    }
}
