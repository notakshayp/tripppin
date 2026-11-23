//! Mouse and touchpad, written to ~/.config/niri/tripppin-pointer.kdl. The person's niri
//! config includes that instead of /etc/niri/pointer.kdl, because a later include can add a
//! niri flag (tap, natural-scroll…) but never take one away.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Pointer {
    pub tap: bool,
    pub touchpad_natural: bool,
    /// Ignore the touchpad while typing.
    pub dwt: bool,
    /// -1.0 (slow) to 1.0 (fast)
    pub touchpad_speed: f64,
    pub mouse_natural: bool,
    pub mouse_speed: f64,
    pub left_handed: bool,
}

impl Default for Pointer {
    /// The same as /etc/niri/pointer.kdl.
    fn default() -> Self {
        Pointer {
            tap: true,
            touchpad_natural: true,
            dwt: true,
            touchpad_speed: 0.0,
            mouse_natural: false,
            mouse_speed: 0.0,
            left_handed: false,
        }
    }
}

pub const SPEEDS: [(f64, &str); 5] =
    [(-0.6, "slowest"), (-0.3, "slow"), (0.0, "normal"), (0.3, "fast"), (0.6, "fastest")];

fn file() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("niri/tripppin-pointer.kdl")
}

pub fn to_kdl(p: &Pointer) -> String {
    let flag = |on: bool, name: &str| if on { format!("        {name}\n") } else { String::new() };
    format!(
        "// Mouse and touchpad, written by tripppin settings.\ninput {{\n    touchpad {{\n{}{}{}        accel-speed {:.1}\n    }}\n    mouse {{\n{}{}        accel-speed {:.1}\n    }}\n}}\n",
        flag(p.tap, "tap"),
        flag(p.touchpad_natural, "natural-scroll"),
        flag(p.dwt, "dwt"),
        p.touchpad_speed,
        flag(p.mouse_natural, "natural-scroll"),
        flag(p.left_handed, "left-handed"),
        p.mouse_speed,
    )
}

pub fn parse_kdl(text: &str) -> Pointer {
    let mut p = Pointer { tap: false, touchpad_natural: false, dwt: false, ..Pointer::default() };
    let mut block = "";
    for line in text.lines().map(str::trim) {
        if line.starts_with("touchpad") {
            block = "touchpad";
        } else if line.starts_with("mouse") {
            block = "mouse";
        } else if let Some(v) = line.strip_prefix("accel-speed ") {
            let v: f64 = v.trim().parse().unwrap_or(0.0);
            if block == "touchpad" { p.touchpad_speed = v } else { p.mouse_speed = v }
        } else {
            match (block, line) {
                ("touchpad", "tap") => p.tap = true,
                ("touchpad", "natural-scroll") => p.touchpad_natural = true,
                ("touchpad", "dwt") => p.dwt = true,
                ("mouse", "natural-scroll") => p.mouse_natural = true,
                ("mouse", "left-handed") => p.left_handed = true,
                _ => {}
            }
        }
    }
    p
}

pub fn current() -> Pointer {
    std::fs::read_to_string(file()).map(|t| parse_kdl(&t)).unwrap_or_default()
}

/// Saves the settings; niri notices the change and applies them at once.
pub fn save(p: &Pointer) -> std::io::Result<()> {
    let path = file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, to_kdl(p))?;
    crate::display::include_in_niri("tripppin-pointer.kdl")
}

/// Whether a touchpad is plugged in (libinput lists it under /dev/input/by-path or sysfs).
pub fn has_touchpad() -> bool {
    std::fs::read_to_string("/proc/bus/input/devices")
        .is_ok_and(|t| t.lines().any(|l| l.starts_with("N: Name=") && l.to_lowercase().contains("touchpad")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for p in [
            Pointer::default(),
            Pointer {
                tap: false,
                touchpad_natural: false,
                mouse_natural: true,
                left_handed: true,
                mouse_speed: -0.3,
                ..Pointer::default()
            },
        ] {
            assert_eq!(parse_kdl(&to_kdl(&p)), p);
        }
        assert!(!to_kdl(&Pointer { tap: false, ..Pointer::default() }).contains("tap"));
    }
}
