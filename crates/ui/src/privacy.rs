//! Privacy and accessibility switches: GNOME settings that GTK apps and the portals honour,
//! Flatpak's camera permissions, the firewall, the screen reader and the cursor size.

use std::path::PathBuf;
use std::process::Command;

pub fn gsettings_get(schema: &str, key: &str) -> String {
    Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

pub fn gsettings_set(schema: &str, key: &str, value: &str) {
    let _ = Command::new("gsettings").args(["set", schema, key, value]).status();
}

pub fn flag(schema: &str, key: &str) -> bool {
    gsettings_get(schema, key) == "true"
}

pub fn set_flag(schema: &str, key: &str, on: bool) {
    gsettings_set(schema, key, if on { "true" } else { "false" });
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn clear_recent() -> std::io::Result<()> {
    match std::fs::remove_file(home().join(".local/share/recently-used.xbel")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// (app id, allowed) from `flatpak permissions devices`: lines of table, object, app, value.
pub fn parse_camera(out: &str) -> Vec<(String, bool)> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() >= 4 && f[0] == "devices" && f[1] == "camera").then(|| (f[2].to_string(), f[3] == "yes"))
        })
        .collect()
}

pub fn camera_apps() -> Vec<(String, bool)> {
    Command::new("flatpak")
        .args(["permissions", "devices"])
        .output()
        .map(|o| parse_camera(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

pub fn set_camera(app: &str, allowed: bool) {
    let _ = Command::new("flatpak")
        .args(["permission-set", "devices", "camera", app, if allowed { "yes" } else { "no" }])
        .status();
}

/// Some(on) when firewalld is installed. (Asking systemd needs no authorisation;
/// `firewall-cmd --state` does outside the desktop session.)
pub fn firewall() -> Option<bool> {
    if !std::path::Path::new("/usr/lib/systemd/system/firewalld.service").exists() {
        return None;
    }
    let out = Command::new("systemctl").args(["is-active", "--quiet", "firewalld.service"]).status().ok()?;
    Some(out.success())
}

/// Screen reader: Orca, when installed.
pub fn has_orca() -> bool {
    std::path::Path::new("/usr/bin/orca").exists()
}

pub fn set_screen_reader(on: bool) {
    set_flag("org.gnome.desktop.a11y.applications", "screen-reader-enabled", on);
    if on {
        let _ = Command::new("setsid").args(["-f", "orca", "--replace"]).status();
    } else {
        let _ = Command::new("pkill").args(["-x", "orca"]).status();
    }
}

pub const CURSOR_SIZES: [(u32, &str); 4] = [(24, "Normal"), (32, "Large"), (48, "Larger"), (64, "Largest")];

pub fn cursor_size() -> u32 {
    match crate::kde::read_num("kcminputrc", "Mouse", "cursorSize", 0u32) {
        0 => gsettings_get("org.gnome.desktop.interface", "cursor-size").parse().unwrap_or(24),
        size => size,
    }
}

/// Sets the cursor size for apps (GTK) and for Plasma, which draws it everywhere.
pub fn set_cursor_size(size: u32) -> std::io::Result<()> {
    gsettings_set("org.gnome.desktop.interface", "cursor-size", &size.to_string());
    let theme = crate::kde::read("kcminputrc", "Mouse", "cursorTheme").unwrap_or_else(|| "breeze_cursors".into());
    crate::kde::run("plasma-apply-cursortheme", &["--size", &size.to_string(), &theme])
        .map(|_| ())
        .map_err(std::io::Error::other)
}
