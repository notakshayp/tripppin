//! Talking to Plasma: reading and writing its config files, and calling KWin and the other
//! Plasma services over D-Bus. Settings pages use this instead of sending people to KDE's
//! System Settings, which is hidden.
//!
//! Reads parse the INI files directly (fast, no process). Writes go through `kwriteconfig6`,
//! which keeps KDE's file format and tells watching services (KWin, powerdevil, the
//! notification server) that something changed.

use gtk::prelude::*;
use gtk::{gio, glib};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

pub fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// The value of `key` in `[group]` of INI `text`. Nested groups are written `A][B` (for
/// `[A][B]`). The last assignment wins, as in KDE's own reader.
pub fn ini_get(text: &str, group: &str, key: &str) -> Option<String> {
    let header = format!("[{group}]");
    let mut inside = false;
    let mut found = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == header;
        } else if inside && let Some((k, v)) = line.split_once('=') {
            // Keys may carry a locale or flags: key[$i]=…
            if k.trim().split('[').next() == Some(key) {
                found = Some(v.trim().to_string());
            }
        }
    }
    found
}

/// The group names of INI `text` that start with `prefix` (the whole header, without the
/// outer brackets), in file order.
pub fn ini_groups(text: &str, prefix: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with('[') && l.ends_with(']'))
        .map(|l| l[1..l.len() - 1].to_string())
        .filter(|g| g.starts_with(prefix))
        .collect()
}

/// The text of a Plasma config file: the person's, else the system's.
pub fn file_text(file: &str) -> String {
    std::fs::read_to_string(config_home().join(file))
        .or_else(|_| std::fs::read_to_string(format!("/etc/xdg/{file}")))
        .unwrap_or_default()
}

/// One setting, from the person's file first and then the system's.
pub fn read(file: &str, group: &str, key: &str) -> Option<String> {
    let mine = std::fs::read_to_string(config_home().join(file)).unwrap_or_default();
    ini_get(&mine, group, key).or_else(|| {
        let system = std::fs::read_to_string(format!("/etc/xdg/{file}")).unwrap_or_default();
        ini_get(&system, group, key)
    })
}

pub fn read_bool(file: &str, group: &str, key: &str, default: bool) -> bool {
    read(file, group, key).map_or(default, |v| v == "true")
}

pub fn read_num<T: std::str::FromStr>(file: &str, group: &str, key: &str, default: T) -> T {
    read(file, group, key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn kwrite(file: &str, group: &str, key: &str, value: Option<&str>, notify: bool) -> bool {
    let mut cmd = Command::new("kwriteconfig6");
    cmd.args(["--file", file]);
    // "A][B" is a nested group: one --group for each level.
    for g in group.split("][") {
        cmd.args(["--group", g]);
    }
    cmd.args(["--key", key]);
    if notify {
        cmd.arg("--notify");
    }
    match value {
        Some(v) => cmd.arg(v),
        None => cmd.arg("--delete"),
    };
    cmd.status().is_ok_and(|s| s.success())
}

/// Writes a setting. Asks watchers to reload when this `kwriteconfig6` knows how.
pub fn write(file: &str, group: &str, key: &str, value: &str) -> bool {
    kwrite(file, group, key, Some(value), true) || kwrite(file, group, key, Some(value), false)
}

pub fn write_bool(file: &str, group: &str, key: &str, on: bool) -> bool {
    write(file, group, key, if on { "true" } else { "false" })
}

/// Removes a setting, so the system default applies again.
pub fn delete(file: &str, group: &str, key: &str) -> bool {
    kwrite(file, group, key, None, true) || kwrite(file, group, key, None, false)
}

/// "63,174,122" (how kdeglobals writes a colour) as "#3fae7a".
pub fn rgb_to_hex(rgb: &str) -> Option<String> {
    let mut parts = rgb.split(',').map(|p| p.trim().parse::<u8>());
    let (r, g, b) = (parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?);
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

// ---- D-Bus ----------------------------------------------------------------------------

fn session() -> Result<gio::DBusConnection, String> {
    gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).map_err(|e| e.to_string())
}

/// Calls a method on the session bus and returns its reply.
pub fn call(
    dest: &str,
    path: &str,
    interface: &str,
    method: &str,
    args: Option<&glib::Variant>,
) -> Result<glib::Variant, String> {
    session()?
        .call_sync(
            Some(dest),
            path,
            interface,
            method,
            args,
            None,
            gio::DBusCallFlags::NONE,
            3000,
            gio::Cancellable::NONE,
        )
        .map_err(|e| e.to_string())
}

/// Sends a signal on the session bus.
pub fn emit(path: &str, interface: &str, signal: &str) {
    if let Ok(conn) = session() {
        let _ = conn.emit_signal(None, path, interface, signal, None);
        let _ = conn.flush_sync(gio::Cancellable::NONE);
    }
}

/// All properties of a D-Bus object.
pub fn properties(dest: &str, path: &str, interface: &str) -> HashMap<String, glib::Variant> {
    call(dest, path, "org.freedesktop.DBus.Properties", "GetAll", Some(&(interface,).to_variant()))
        .ok()
        .and_then(|v| v.get::<(HashMap<String, glib::Variant>,)>())
        .map(|t| t.0)
        .unwrap_or_default()
}

/// Sets one property of a D-Bus object.
pub fn set_property(dest: &str, path: &str, interface: &str, name: &str, value: &glib::Variant) -> Result<(), String> {
    call(dest, path, "org.freedesktop.DBus.Properties", "Set", Some(&(interface, name, value.clone()).to_variant()))
        .map(|_| ())
}

/// Tells KWin to read its settings again (effects, night light, animations).
pub fn kwin_reconfigure() {
    let _ = call("org.kde.KWin", "/KWin", "org.kde.KWin", "reconfigure", None);
}

/// Runs a command and gives its trimmed output, or its error text.
pub fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program).args(args).output().map_err(|e| format!("couldn't run {program}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = if err.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { err };
        Err(err.trim().to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[General]\nAccentColor=1,2,3\n\n[KDE]\nAnimationDurationFactor=0\n\n\
        [Libinput][1267][12345][Touchpad]\nTapToClick=true\nNaturalScroll[$i]=false\n";

    #[test]
    fn reads_ini_values() {
        assert_eq!(ini_get(SAMPLE, "General", "AccentColor").as_deref(), Some("1,2,3"));
        assert_eq!(ini_get(SAMPLE, "KDE", "AnimationDurationFactor").as_deref(), Some("0"));
        assert_eq!(ini_get(SAMPLE, "Libinput][1267][12345][Touchpad", "TapToClick").as_deref(), Some("true"));
        assert_eq!(ini_get(SAMPLE, "Libinput][1267][12345][Touchpad", "NaturalScroll").as_deref(), Some("false"));
        assert_eq!(ini_get(SAMPLE, "General", "Nope"), None);
        assert_eq!(ini_get(SAMPLE, "Other", "AccentColor"), None);
    }

    #[test]
    fn lists_groups_by_prefix() {
        assert_eq!(ini_groups(SAMPLE, "Libinput"), vec!["Libinput][1267][12345][Touchpad".to_string()]);
        assert_eq!(ini_groups(SAMPLE, "K"), vec!["KDE".to_string()]);
    }

    #[test]
    fn colours_convert() {
        assert_eq!(rgb_to_hex("63,174,122").as_deref(), Some("#3fae7a"));
        assert_eq!(rgb_to_hex(" 0, 0, 255 ").as_deref(), Some("#0000ff"));
        assert_eq!(rgb_to_hex("1,2"), None);
        assert_eq!(rgb_to_hex("a,b,c"), None);
    }

    #[test]
    fn last_assignment_wins() {
        assert_eq!(ini_get("[A]\nk=1\nk=2\n", "A", "k").as_deref(), Some("2"));
    }
}
