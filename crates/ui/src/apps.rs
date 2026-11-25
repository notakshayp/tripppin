//! Installed apps from their .desktop files (system, Flatpak and the person's own), and the
//! notification settings that hang off them: do not disturb, quiet hours and muted apps,
//! all through SwayNotificationCenter.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    /// The desktop file's id, e.g. org.mozilla.firefox
    pub id: String,
    pub name: String,
    pub icon: String,
    pub flatpak: bool,
    /// The file types it says it can open.
    pub mime: Vec<String>,
}

/// Reads the [Desktop Entry] group of a .desktop file; `None` for hidden entries.
pub fn parse_desktop(id: &str, text: &str, flatpak: bool) -> Option<App> {
    let (mut name, mut icon, mut in_entry, mut hidden, mut is_app) =
        (String::new(), String::new(), false, false, false);
    let mut mime = Vec::new();
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
            "MimeType" => mime = v.split(';').map(str::trim).filter(|m| !m.is_empty()).map(str::to_string).collect(),
            "Type" => is_app = v.trim() == "Application",
            "NoDisplay" | "Hidden" if v.trim() == "true" => hidden = true,
            _ => {}
        }
    }
    (is_app && !hidden && !name.is_empty()).then(|| App { id: id.to_string(), name, icon, flatpak, mime })
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

// ---- flatpak ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Flatpak {
    pub id: String,
    pub name: String,
    pub version: String,
    pub size: String,
    /// "system" or "user"
    pub installation: String,
}

/// Parses `flatpak list --app --columns=application,name,version,size,installation`.
pub fn parse_flatpaks(out: &str) -> Vec<Flatpak> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 5 && !f[0].is_empty()).then(|| Flatpak {
                id: f[0].to_string(),
                name: f[1].to_string(),
                version: f[2].to_string(),
                size: f[3].to_lowercase(),
                installation: f[4].to_string(),
            })
        })
        .collect()
}

fn flatpak(args: &[&str]) -> Result<String, String> {
    let out = Command::new("flatpak").args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("Flatpak failed").trim().to_lowercase())
    }
}

pub fn flatpaks() -> Vec<Flatpak> {
    flatpak(&["list", "--app", "--columns=application,name,version,size,installation"])
        .map(|o| parse_flatpaks(&o))
        .unwrap_or_default()
}

/// Ids of apps with an update waiting.
pub fn updates() -> Vec<String> {
    flatpak(&["remote-ls", "--updates", "--app", "--columns=application"])
        .map(|o| o.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

pub fn update_all() -> Result<(), String> {
    flatpak(&["update", "-y", "--noninteractive"]).map(|_| ())
}

pub fn remove(app: &Flatpak) -> Result<(), String> {
    flatpak(&["uninstall", "-y", "--noninteractive", &format!("--{}", app.installation), &app.id]).map(|_| ())
}

/// One app found on Flathub.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub id: String,
    pub name: String,
    pub summary: String,
}

/// Parses `flatpak search --columns=application,name,description,remotes`, keeping Flathub's.
pub fn parse_search(out: &str) -> Vec<Found> {
    let mut seen = Vec::new();
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 4 && f[3].contains("flathub") && !seen.contains(&f[0].to_string())).then(|| {
                seen.push(f[0].to_string());
                Found { id: f[0].to_string(), name: f[1].to_string(), summary: f[2].to_string() }
            })
        })
        .take(12)
        .collect()
}

/// Puts the apps whose name and description mention every word first.
pub fn rank(mut found: Vec<Found>, words: &[String]) -> Vec<Found> {
    let hits = |f: &Found| {
        let text = format!("{} {}", f.name, f.summary).to_lowercase();
        words.iter().filter(|w| text.contains(w.as_str())).count()
    };
    found.sort_by_key(|f| std::cmp::Reverse(hits(f)));
    found
}

/// flatpak matches the whole phrase, so "photo editor" finds nothing; search the longest word
/// and rank by the rest.
pub fn search(query: &str) -> Vec<Found> {
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
    let Some(longest) = words.iter().max_by_key(|w| w.len()) else { return Vec::new() };
    let found = flatpak(&["search", "--columns=application,name,description,remotes", longest])
        .map(|o| parse_search(&o))
        .unwrap_or_default();
    rank(found, &words)
}

/// Installs for everyone on the computer; administrators need no password (Flatpak's rule).
pub fn install(id: &str) -> Result<(), String> {
    flatpak(&["install", "-y", "--noninteractive", "--system", "flathub", id]).map(|_| ())
}

// ---- default apps -------------------------------------------------------------------------

/// What a default is for, and the file types that make it up (the first decides which app
/// shows as current).
pub const DEFAULTS: [(&str, &[&str]); 6] = [
    ("Web browser", &["x-scheme-handler/https", "x-scheme-handler/http", "text/html"]),
    ("mail", &["x-scheme-handler/mailto"]),
    ("files", &["inode/directory"]),
    ("music", &["audio/mpeg", "audio/flac", "audio/ogg", "audio/x-wav"]),
    ("video", &["video/mp4", "video/x-matroska", "video/webm"]),
    ("pictures", &["image/png", "image/jpeg", "image/webp"]),
];

fn mimeapps() -> PathBuf {
    config_home().join("mimeapps.list")
}

/// The desktop id set as default for `mime` in a mimeapps.list.
pub fn default_in(text: &str, mime: &str) -> Option<String> {
    let mut in_defaults = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_defaults = line == "[Default Applications]";
        } else if in_defaults
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == mime
        {
            return v.split(';').next().map(|d| d.trim().trim_end_matches(".desktop").to_string());
        }
    }
    None
}

/// The mimeapps.list text with `app` as the default for every type in `mimes`.
pub fn with_default(text: &str, mimes: &[&str], app: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_defaults = false;
    let mut found = false;
    let entries: Vec<String> = mimes.iter().map(|m| format!("{m}={app}.desktop;")).collect();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if in_defaults {
                out.extend(entries.iter().cloned());
            }
            in_defaults = t == "[Default Applications]";
            found |= in_defaults;
            out.push(line.to_string());
            continue;
        }
        if in_defaults && t.split_once('=').is_some_and(|(k, _)| mimes.contains(&k.trim())) {
            continue;
        }
        out.push(line.to_string());
    }
    if in_defaults {
        out.extend(entries.iter().cloned());
    }
    if !found {
        out.push("[Default Applications]".to_string());
        out.extend(entries);
    }
    out.join("\n") + "\n"
}

pub fn default_for(mimes: &[&str]) -> Option<String> {
    let mine = std::fs::read_to_string(mimeapps()).unwrap_or_default();
    let system = std::fs::read_to_string("/usr/share/applications/mimeapps.list").unwrap_or_default();
    default_in(&mine, mimes[0]).or_else(|| default_in(&system, mimes[0]))
}

pub fn set_default(mimes: &[&str], app: &str) -> std::io::Result<()> {
    let path = mimeapps();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    std::fs::write(&path, with_default(&text, mimes, app))
}

// ---- notifications -----------------------------------------------------------------------
//
// Plasma's notification server reads plasmanotifyrc: do not disturb is `[DoNotDisturb] Until`
// (a time in the future; far ahead means "until I turn it off") and an app is muted with
// `[Applications][<desktop id>] ShowPopups=false`.

/// How far ahead "until I turn it off" is.
const FOREVER: &str = "2099-12-31T23:59:59";

fn now_iso() -> String {
    Command::new("date")
        .arg("+%Y-%m-%dT%H:%M:%S")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Whether do not disturb is on, from its `Until` time and the time now (both
/// `YYYY-MM-DDTHH:MM:SS`, which sort as text).
pub fn dnd_active(until: &str, now: &str) -> bool {
    let (until, now) = (until.get(..19).unwrap_or(""), now.get(..19).unwrap_or(""));
    !until.is_empty() && until > now
}

pub fn dnd() -> bool {
    crate::kde::read("plasmanotifyrc", "DoNotDisturb", "Until").is_some_and(|u| dnd_active(&u, &now_iso()))
}

pub fn set_dnd(on: bool) {
    if on {
        crate::kde::write("plasmanotifyrc", "DoNotDisturb", "Until", FOREVER);
    } else {
        crate::kde::delete("plasmanotifyrc", "DoNotDisturb", "Until");
    }
}

/// The desktop ids of the apps whose pop-ups are off, from plasmanotifyrc's text.
pub fn muted_in(config: &str) -> Vec<String> {
    let mut ids: Vec<String> = crate::kde::ini_groups(config, "Applications][")
        .into_iter()
        .filter_map(|g| g.strip_prefix("Applications][").map(str::to_string))
        .filter(|id| {
            crate::kde::ini_get(config, &format!("Applications][{id}"), "ShowPopups").as_deref() == Some("false")
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// The desktop ids of the muted apps.
pub fn muted() -> Vec<String> {
    muted_in(&crate::kde::file_text("plasmanotifyrc"))
}

/// Mutes exactly these apps (by desktop id): their pop-ups stop, but they stay in history.
pub fn set_muted(ids: &[String]) -> std::io::Result<()> {
    let mut ok = true;
    for old in muted() {
        if !ids.contains(&old) {
            ok &= crate::kde::delete("plasmanotifyrc", &format!("Applications][{old}"), "ShowPopups");
        }
    }
    for id in ids {
        ok &= crate::kde::write("plasmanotifyrc", &format!("Applications][{id}"), "ShowPopups", "false");
    }
    if ok { Ok(()) } else { Err(std::io::Error::other("kwriteconfig6 failed")) }
}

// Quiet hours: two systemd user timers turn do-not-disturb on and off each day.

fn config_home() -> PathBuf {
    crate::kde::config_home()
}

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
    for (n, at, state) in [(names[0], from, "on"), (names[1], to, "off")] {
        write_unit(&dir, n, at, state)?;
    }
    systemctl_user(&["daemon-reload"]);
    systemctl_user(&["enable", "--now", "tripppin-quiet-on.timer", "tripppin-quiet-off.timer"]);
    Ok(())
}

fn write_unit(dir: &Path, name: &str, at: &str, state: &str) -> std::io::Result<()> {
    std::fs::write(
        dir.join(format!("{name}.service")),
        format!(
            "[Unit]\nDescription=Quiet hours (tripppin)\n\n[Service]\nType=oneshot\nExecStart=/usr/bin/tripppin-ui dnd {state}\n"
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
            Some(App {
                id: "firefox".into(),
                name: "Firefox".into(),
                icon: "firefox".into(),
                flatpak: false,
                mime: Vec::new()
            })
        );
        assert!(parse_desktop("x", "[Desktop Entry]\nType=Application\nName=X\nNoDisplay=true\n", false).is_none());
    }

    #[test]
    fn reads_flatpaks_and_defaults() {
        let out = "org.videolan.VLC\tVLC\t3.0.24\t165.3 MB\tsystem\n";
        assert_eq!(parse_flatpaks(out)[0].id, "org.videolan.VLC");
        assert_eq!(parse_flatpaks(out)[0].installation, "system");
        let text = "[Added Associations]\nvideo/mp4=a.desktop;\n[Default Applications]\nvideo/mp4=old.desktop;\ntext/plain=gedit.desktop;\n";
        assert_eq!(default_in(text, "video/mp4").as_deref(), Some("old"));
        let next = with_default(text, &["video/mp4", "video/webm"], "org.videolan.VLC");
        assert_eq!(default_in(&next, "video/mp4").as_deref(), Some("org.videolan.VLC"));
        assert_eq!(default_in(&next, "text/plain").as_deref(), Some("gedit"));
        assert!(next.contains("[Added Associations]\nvideo/mp4=a.desktop;"));
        assert_eq!(
            default_in(&with_default("", &["inode/directory"], "thunar"), "inode/directory").as_deref(),
            Some("thunar")
        );
    }

    #[test]
    fn reads_flathub_search() {
        let out = "org.videolan.VLC\tVLC\tVLC media player\tflathub\nio.mpv.Mpv\tmpv\tA media player\tflathub\norg.videolan.VLC\tVLC\tdup\tflathub\nx.y\tZ\tz\tother\n";
        let found = parse_search(out);
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[0],
            Found { id: "org.videolan.VLC".into(), name: "VLC".into(), summary: "VLC media player".into() }
        );
        let ranked = rank(found, &["mpv".into(), "player".into()]);
        assert_eq!(ranked[0].id, "io.mpv.Mpv");
    }

    #[test]
    fn reads_muted_apps_and_do_not_disturb() {
        let config = "[Applications][org.mozilla.firefox]\nShowPopups=false\n\n[Applications][org.kde.konsole]\nShowPopups=true\n\n[Applications][slack]\nShowInHistory=false\n\n[Applications][a.b]\nShowPopups=false\n";
        assert_eq!(muted_in(config), vec!["a.b".to_string(), "org.mozilla.firefox".to_string()]);
        assert!(muted_in("").is_empty());
        assert!(dnd_active("2099-12-31T23:59:59", "2026-11-25T10:00:00"));
        assert!(!dnd_active("2026-11-25T09:00:00", "2026-11-25T10:00:00"));
        assert!(!dnd_active("", "2026-11-25T10:00:00"));
    }
}
