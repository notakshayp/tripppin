//! Mouse and touchpad through KWin's input-device D-Bus service (org.kde.KWin
//! /org/kde/KWin/InputDevice): each change applies to the device at once and is written to
//! kcminputrc, under that device's own `[Libinput][vendor][product][name]` group, which is
//! where KWin reads it back from at the next login.

use gtk::glib;
use gtk::prelude::*;

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

const DEST: &str = "org.kde.KWin";
const MANAGER: &str = "/org/kde/KWin/InputDevice";
const DEVICE: &str = "org.kde.KWin.InputDevice";

/// One input device KWin knows.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub path: String,
    pub name: String,
    pub vendor: u32,
    pub product: u32,
    pub touchpad: bool,
}

/// A value of a device setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Val {
    On(bool),
    Speed(f64),
}

/// The kcminputrc group for a device: `[Libinput][vendor][product][name]`, as "A][B][C][D".
pub fn config_group(d: &Device) -> String {
    format!("Libinput][{}][{}][{}", d.vendor, d.product, d.name)
}

/// What to set on a touchpad or a mouse: (D-Bus property, kcminputrc key, value).
pub fn settings_for(p: &Pointer, touchpad: bool) -> Vec<(&'static str, &'static str, Val)> {
    if touchpad {
        vec![
            ("tapToClick", "TapToClick", Val::On(p.tap)),
            ("naturalScroll", "NaturalScroll", Val::On(p.touchpad_natural)),
            ("disableWhileTyping", "DisableWhileTyping", Val::On(p.dwt)),
            ("pointerAcceleration", "PointerAcceleration", Val::Speed(p.touchpad_speed)),
        ]
    } else {
        vec![
            ("naturalScroll", "NaturalScroll", Val::On(p.mouse_natural)),
            ("leftHanded", "LeftHanded", Val::On(p.left_handed)),
            ("pointerAcceleration", "PointerAcceleration", Val::Speed(p.mouse_speed)),
        ]
    }
}

/// How a value is written in kcminputrc.
pub fn written(v: Val) -> String {
    match v {
        Val::On(b) => b.to_string(),
        Val::Speed(s) => format!("{s:.1}"),
    }
}

/// The devices KWin lists: mice and touchpads.
pub fn devices() -> Vec<Device> {
    let names = crate::kde::call(
        DEST,
        MANAGER,
        "org.freedesktop.DBus.Properties",
        "Get",
        Some(&("org.kde.KWin.InputDeviceManager", "devicesSysNames").to_variant()),
    )
    .ok()
    .and_then(|v| v.get::<(glib::Variant,)>())
    .and_then(|t| t.0.get::<Vec<String>>())
    .unwrap_or_default();
    names
        .iter()
        .filter_map(|sys| {
            let path = format!("{MANAGER}/{sys}");
            let props = crate::kde::properties(DEST, &path, DEVICE);
            let flag = |k: &str| props.get(k).and_then(|v| v.get::<bool>()).unwrap_or(false);
            let (touchpad, pointer) = (flag("touchpad"), flag("pointer"));
            (touchpad || pointer).then(|| Device {
                name: props.get("name").and_then(|v| v.get::<String>()).unwrap_or_default(),
                vendor: props.get("vendor").and_then(|v| v.get::<u32>()).unwrap_or(0),
                product: props.get("product").and_then(|v| v.get::<u32>()).unwrap_or(0),
                touchpad,
                path,
            })
        })
        .collect()
}

pub fn current() -> Pointer {
    let mut p = Pointer::default();
    let (mut seen_pad, mut seen_mouse) = (false, false);
    for d in devices() {
        let props = crate::kde::properties(DEST, &d.path, DEVICE);
        let flag = |k: &str, default: bool| props.get(k).and_then(|v| v.get::<bool>()).unwrap_or(default);
        let speed = props.get("pointerAcceleration").and_then(|v| v.get::<f64>()).unwrap_or(0.0);
        if d.touchpad && !seen_pad {
            seen_pad = true;
            p.tap = flag("tapToClick", p.tap);
            p.touchpad_natural = flag("naturalScroll", p.touchpad_natural);
            p.dwt = flag("disableWhileTyping", p.dwt);
            p.touchpad_speed = speed;
        } else if !d.touchpad && !seen_mouse {
            seen_mouse = true;
            p.mouse_natural = flag("naturalScroll", p.mouse_natural);
            p.left_handed = flag("leftHanded", p.left_handed);
            p.mouse_speed = speed;
        }
    }
    p
}

/// Applies the settings to every device now and remembers them for next time.
pub fn save(p: &Pointer) -> std::io::Result<()> {
    let mut ok = true;
    for d in devices() {
        for (prop, key, val) in settings_for(p, d.touchpad) {
            let variant = match val {
                Val::On(b) => b.to_variant(),
                Val::Speed(s) => s.to_variant(),
            };
            // A device may not offer every setting (a mouse has no tap): that isn't an error.
            let _ = crate::kde::set_property(DEST, &d.path, DEVICE, prop, &variant);
            ok &= crate::kde::write("kcminputrc", &config_group(&d), key, &written(val));
        }
    }
    if ok { Ok(()) } else { Err(std::io::Error::other("kwriteconfig6 failed")) }
}

/// Whether a touchpad is plugged in.
pub fn has_touchpad() -> bool {
    devices().iter().any(|d| d.touchpad)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad() -> Device {
        Device {
            path: "/x/event5".into(),
            name: "SYNA8004:00 06CB:CD8B Touchpad".into(),
            vendor: 1739,
            product: 52619,
            touchpad: true,
        }
    }

    #[test]
    fn names_the_config_group_by_device() {
        assert_eq!(config_group(&pad()), "Libinput][1739][52619][SYNA8004:00 06CB:CD8B Touchpad");
    }

    #[test]
    fn touchpads_and_mice_get_their_own_settings() {
        let p = Pointer { tap: false, mouse_natural: true, left_handed: true, mouse_speed: 0.3, ..Pointer::default() };
        let pad = settings_for(&p, true);
        assert!(pad.contains(&("tapToClick", "TapToClick", Val::On(false))));
        assert!(pad.iter().any(|s| s.0 == "disableWhileTyping"));
        let mouse = settings_for(&p, false);
        assert!(mouse.contains(&("naturalScroll", "NaturalScroll", Val::On(true))));
        assert!(mouse.contains(&("leftHanded", "LeftHanded", Val::On(true))));
        assert!(mouse.contains(&("pointerAcceleration", "PointerAcceleration", Val::Speed(0.3))));
        assert!(!mouse.iter().any(|s| s.0 == "tapToClick"));
        assert_eq!(written(Val::On(true)), "true");
        assert_eq!(written(Val::Speed(-0.3)), "-0.3");
    }
}
