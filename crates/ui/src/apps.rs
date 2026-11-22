//! Installed apps from their .desktop files (system, Flatpak and the person's own), and the
//! notification settings that hang off them: do not disturb, quiet hours and muted apps,
//! all through SwayNotificationCenter.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    /// The desktop file's id, e.g. org.mozilla.firefox
    pub id: String,
    pub name: String,
    pub icon: String,
    pub flatpak: bool,
}

/// Reads the [Desktop Entry] group of a .desktop file; `None` for hidden entries.
pub fn parse_desktop(id: &str, text: &str, flatpak: bool) -> Option<App> {
    let (mut name, mut icon, mut in_entry, mut hidden, mut is_app) =
        (String::new(), String::new(), false, false, false);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "Name" => name = v.trim().to_string(),
            "Icon" => icon = v.trim().to_string(),
            "Type" => is_app = v.trim() == "Application",
            "NoDisplay" | "Hidden" if v.trim() == "true" => hidden = true,
            _ => {}
        }
    }
    (is_app && !hidden && !name.is_empty()).then(|| App { id: id.to_string(), name, icon, flatpak })
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Every app the launcher would show, by name; later folders override earlier ones.
pub fn installed() -> Vec<App> {
    let dirs: [(PathBuf, bool); 4] = [
        (PathBuf::from("/usr/share/applications"), false),
        (PathBuf::from("/var/lib/flatpak/exports/share/applications"), true),
        (data_home().join("flatpak/exports/share/applications"), true),
        (data_home().join("applications"), false),
    ];
    let mut apps: Vec<App> = Vec::new();
    for (dir, flatpak) in &dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            let Some(id) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".desktop")) else {
                continue;
            };
            let parsed = std::fs::read_to_string(&path).ok().and_then(|t| parse_desktop(id, &t, *flatpak));
            apps.retain(|a| a.id != id);
            if let Some(app) = parsed {
                apps.push(app);
            }
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

// ---- notifications -----------------------------------------------------------------------

fn swaync(args: &[&str]) -> Option<String> {
    let out = Command::new("swaync-client").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn dnd() -> bool {
    swaync(&["-D"]).is_some_and(|s| s == "true")
}

pub fn set_dnd(on: bool) {
    swaync(&[if on { "-dn" } else { "-df" }]);
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

const SYSTEM_CONFIG: &str = "/etc/xdg/swaync/config.json";
const RULE_PREFIX: &str = "tripppin-muted: ";

/// App names whose notifications are dropped, from swaync's notification-visibility rules.
pub fn muted_in(config: &Value) -> Vec<String> {
    let mut names: Vec<String> = config["notification-visibility"]
        .as_object()
        .map(|rules| rules.keys().filter_map(|k| k.strip_prefix(RULE_PREFIX)).map(str::to_string).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// The config with exactly these apps muted (other rules are kept).
pub fn with_muted(mut config: Value, names: &[String]) -> Value {
    if !config["notification-visibility"].is_object() {
        config["notification-visibility"] = Value::Object(Default::default());
    }
    let rules = config["notification-visibility"].as_object_mut().expect("just made an object");
    rules.retain(|k, _| !k.starts_with(RULE_PREFIX));
    for n in names {
        // swaync matches app-name as a regular expression; match this name exactly.
        let pattern = format!("^{}$", regex_escape(n));
        rules.insert(format!("{RULE_PREFIX}{n}"), serde_json::json!({ "state": "ignored", "app-name": pattern }));
    }
    config
}

fn regex_escape(s: &str) -> String {
    s.chars().fold(String::new(), |mut out, c| {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

/// The person's swaync config if they have one, else the system's.
fn notify_config() -> (PathBuf, Value) {
    let mine = config_home().join("swaync/config.json");
    let text = std::fs::read_to_string(&mine).or_else(|_| std::fs::read_to_string(SYSTEM_CONFIG)).unwrap_or_default();
    (mine, serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({})))
}

pub fn muted() -> Vec<String> {
    muted_in(&notify_config().1)
}

/// Saves the muted list into ~/.config/swaync/config.json (a copy of the system config plus
/// the rules) and tells swaync to reload it.
pub fn set_muted(names: &[String]) -> std::io::Result<()> {
    let (path, config) = notify_config();
    let config = with_muted(config, names);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap_or_default())?;
    // swaync only reads visibility rules at start (-R doesn't apply them), so restart it,
    // keeping do-not-disturb as it was. The bar's counter reconnects by itself.
    let quiet = dnd();
    let _ = Command::new("pkill").args(["-x", "swaync"]).status();
    let _ = Command::new("niri").args(["msg", "action", "spawn", "--", "swaync"]).status();
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if swaync(&["-D"]).is_some() {
            break;
        }
    }
    if quiet {
        set_dnd(true);
    }
    Ok(())
}

// Quiet hours: two systemd user timers turn do-not-disturb on and off each day.

fn unit_dir() -> PathBuf {
    config_home().join("systemd/user")
}

/// "22:00" and "07:00", when quiet hours are set.
pub fn quiet_hours() -> Option<(String, String)> {
    let read = |name: &str| -> Option<String> {
        let text = std::fs::read_to_string(unit_dir().join(name)).ok()?;
        text.lines().find_map(|l| l.strip_prefix("OnCalendar=*-*-* ")).map(|t| t.trim_end_matches(":00").to_string())
    };
    Some((read("tripppin-quiet-on.timer")?, read("tripppin-quiet-off.timer")?))
}

fn systemctl_user(args: &[&str]) {
    let _ = Command::new("systemctl").arg("--user").args(args).status();
}

pub fn set_quiet_hours(hours: Option<(&str, &str)>) -> std::io::Result<()> {
    let dir = unit_dir();
    let names = ["tripppin-quiet-on", "tripppin-quiet-off"];
    let Some((from, to)) = hours else {
        systemctl_user(&["disable", "--now", "tripppin-quiet-on.timer", "tripppin-quiet-off.timer"]);
        for n in names {
            for ext in ["timer", "service"] {
                let _ = std::fs::remove_file(dir.join(format!("{n}.{ext}")));
            }
        }
        systemctl_user(&["daemon-reload"]);
        return Ok(());
    };
    std::fs::create_dir_all(&dir)?;
    for (n, at, flag) in [(names[0], from, "-dn"), (names[1], to, "-df")] {
        write_unit(&dir, n, at, flag)?;
    }
    systemctl_user(&["daemon-reload"]);
    systemctl_user(&["enable", "--now", "tripppin-quiet-on.timer", "tripppin-quiet-off.timer"]);
    Ok(())
}

fn write_unit(dir: &Path, name: &str, at: &str, flag: &str) -> std::io::Result<()> {
    std::fs::write(
        dir.join(format!("{name}.service")),
        format!(
            "[Unit]\nDescription=Quiet hours (tripppin)\n\n[Service]\nType=oneshot\nExecStart=/usr/bin/swaync-client {flag}\n"
        ),
    )?;
    std::fs::write(
        dir.join(format!("{name}.timer")),
        format!(
            "[Unit]\nDescription=Quiet hours (tripppin)\n\n[Timer]\nOnCalendar=*-*-* {at}:00\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_desktop_files() {
        let text =
            "[Desktop Entry]\nType=Application\nName=Firefox\nIcon=firefox\n[Desktop Action new]\nName=New Window\n";
        assert_eq!(
            parse_desktop("firefox", text, false),
            Some(App { id: "firefox".into(), name: "Firefox".into(), icon: "firefox".into(), flatpak: false })
        );
        assert!(parse_desktop("x", "[Desktop Entry]\nType=Application\nName=X\nNoDisplay=true\n", false).is_none());
    }

    #[test]
    fn mutes_apps_in_swaync_config() {
        let base =
            serde_json::json!({ "positionX": "right", "notification-visibility": { "mine": { "state": "muted" } } });
        let c = with_muted(base, &["Slack".to_string(), "a.b".to_string()]);
        assert_eq!(muted_in(&c), vec!["Slack".to_string(), "a.b".to_string()]);
        assert_eq!(c["notification-visibility"]["tripppin-muted: a.b"]["app-name"], "^a\\.b$");
        assert!(c["notification-visibility"]["mine"].is_object());
        assert!(muted_in(&with_muted(c, &[])).is_empty());
    }
}
