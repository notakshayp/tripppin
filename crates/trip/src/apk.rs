//! Alpine packages through `apk`: search with descriptions, sizes, which are installed, and which
//! are apps with a window (marked ▣). Parsing is separate from running so it can be tested.

use std::process::Command;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pkg {
    pub name: String,
    pub version: String,
    /// What it is, in plain words.
    pub desc: String,
    pub installed: bool,
    /// Has a window: opens as a full-screen app.
    pub gui: bool,
    /// Installed size in bytes, when known.
    pub size: Option<u64>,
}

/// Programs people know as apps, for packages that don't list a .desktop file in the index.
const KNOWN_GUI: &[&str] = &[
    "firefox",
    "firefox-esr",
    "chromium",
    "epiphany",
    "qutebrowser",
    "gimp",
    "inkscape",
    "krita",
    "libreoffice",
    "vlc",
    "mpv",
    "gedit",
    "mousepad",
    "thunar",
    "pcmanfm",
    "evince",
    "zathura",
    "imv",
    "foot",
    "alacritty",
    "kitty",
    "obs-studio",
    "audacity",
    "blender",
    "thunderbird",
    "keepassxc",
    "pavucontrol",
    "gnome-calculator",
    "gnome-text-editor",
    "nautilus",
    "falkon",
    "okular",
    "kate",
    "dolphin",
    "lxappearance",
    "transmission-gtk",
    "rhythmbox",
    "gparted",
];

/// Whether a package is an app with a window, from its name and (when known) its file list.
pub fn is_gui(name: &str, files: &[String]) -> bool {
    KNOWN_GUI.contains(&name) || files.iter().any(|f| f.contains("usr/share/applications/") && f.ends_with(".desktop"))
}

/// "firefox-128.0.1-r0" → ("firefox", "128.0.1-r0").
pub fn split_name_version(nv: &str) -> (String, String) {
    // The release suffix (-r0) belongs to the version; the version starts at the last "-<digit>".
    let body = nv.rfind("-r").filter(|&i| nv[i + 2..].chars().all(|c| c.is_ascii_digit()) && nv.len() > i + 2).unwrap_or(nv.len());
    let head = &nv[..body];
    let cut = head
        .char_indices()
        .filter(|&(i, c)| c == '-' && head[i + 1..].starts_with(|d: char| d.is_ascii_digit()))
        .map(|(i, _)| i)
        .next_back();
    match cut {
        Some(i) => (nv[..i].to_string(), nv[i + 1..].to_string()),
        None => (nv.to_string(), String::new()),
    }
}

/// `apk search -v -d` lines: "name-1.2-r0 - A description".
pub fn parse_search(out: &str) -> Vec<Pkg> {
    out.lines()
        .filter_map(|l| {
            let (nv, desc) = l.split_once(" - ")?;
            let (name, version) = split_name_version(nv.trim());
            if name.is_empty() || name.contains(char::is_whitespace) {
                return None;
            }
            Some(Pkg { gui: is_gui(&name, &[]), name, version, desc: plain(desc), ..Pkg::default() })
        })
        .collect()
}

/// A description in plain words: no trailing full stop, first letter lower case.
pub fn plain(desc: &str) -> String {
    let d = desc.trim().trim_end_matches('.');
    let mut c = d.chars();
    match c.next() {
        Some(f) if c.clone().next().is_none_or(|n| !n.is_uppercase()) => f.to_lowercase().chain(c).collect(),
        _ => d.to_string(),
    }
}

/// `apk info -s` output ("name-1.0 installed size:\n240 KiB") to bytes.
pub fn parse_size(out: &str) -> Option<u64> {
    let line = out.lines().find(|l| l.trim().chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let mut it = line.split_whitespace();
    let n: f64 = it.next()?.parse().ok()?;
    let mult = match it.next()? {
        "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1048576.0,
        "GiB" => 1073741824.0,
        _ => return None,
    };
    Some((n * mult) as u64)
}

pub fn size_words(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / 1073741824.0),
        b if b >= 1 << 20 => format!("{:.0} MB", b as f64 / 1048576.0),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

fn run(args: &[&str]) -> String {
    Command::new("apk").args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// Search the catalogue by name and description.
pub fn search(query: &str) -> Vec<Pkg> {
    let q = query.trim();
    if q.is_empty() || q.starts_with('-') {
        return Vec::new();
    }
    let mut pkgs = parse_search(&run(&["search", "-v", "-d", q]));
    pkgs.sort_by_key(|p| (!p.name.starts_with(q), !p.name.contains(q), p.name.len()));
    pkgs.truncate(40);
    let have = installed_names();
    for p in &mut pkgs {
        p.installed = have.iter().any(|h| *h == p.name);
    }
    pkgs
}

/// Fills in the installed size of one package.
pub fn fill_size(p: &mut Pkg) {
    p.size = parse_size(&run(&["info", "-s", &p.name]));
}

pub fn installed_names() -> Vec<String> {
    run(&["info"]).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

pub fn is_installed(name: &str) -> bool {
    Command::new("apk").args(["info", "-e", name]).output().is_ok_and(|o| o.status.success())
}

/// Files an installed package owns.
pub fn files(name: &str) -> Vec<String> {
    run(&["info", "-L", name]).lines().map(str::to_string).collect()
}

/// The command a package's .desktop file runs, for launching it (field codes dropped).
pub fn desktop_exec(name: &str) -> Option<String> {
    let files = files(name);
    let desktop = files.iter().find(|f| f.contains("usr/share/applications/") && f.ends_with(".desktop"))?;
    let text = std::fs::read_to_string(format!("/{}", desktop.trim_start_matches('/'))).ok()?;
    exec_line(&text)
}

pub fn exec_line(desktop: &str) -> Option<String> {
    let l = desktop.lines().find_map(|l| l.strip_prefix("Exec="))?;
    let cmd: Vec<&str> = l.split_whitespace().filter(|w| !(w.starts_with('%') && w.len() == 2)).collect();
    (!cmd.is_empty()).then(|| cmd.join(" "))
}

/// Whether a package name is safe to hand to apk.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() < 80 && n.starts_with(|c: char| c.is_ascii_alphanumeric()) && n.chars().all(|c| c.is_ascii_alphanumeric() || "._+-:=<>~".contains(c))
}

/// The command prefix that gets administrator rights: none for root, else doas or sudo.
pub fn privilege() -> &'static str {
    let root = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("Uid:").and_then(|v| v.split_whitespace().next().map(str::to_string))))
        .is_some_and(|u| u == "0");
    if root {
        ""
    } else if which("doas") {
        "doas "
    } else {
        "sudo "
    }
}

pub fn which(cmd: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

pub fn add_command(names: &[String]) -> String {
    format!("{}apk add --no-progress {}", privilege(), names.join(" "))
}

pub fn del_command(names: &[String]) -> String {
    format!("{}apk del --no-progress {}", privilege(), names.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_names_and_versions() {
        assert_eq!(split_name_version("firefox-128.0.1-r0"), ("firefox".into(), "128.0.1-r0".into()));
        assert_eq!(split_name_version("py3-pip-25.3-r1"), ("py3-pip".into(), "25.3-r1".into()));
        assert_eq!(split_name_version("w3m-0.5.3_git20230121-r2"), ("w3m".into(), "0.5.3_git20230121-r2".into()));
        assert_eq!(split_name_version("lib-2to3-1.0-r0"), ("lib-2to3".into(), "1.0-r0".into()));
    }

    #[test]
    fn parses_search_output() {
        let out = "firefox-128.0.1-r0 - Safe and easy web browser from Mozilla\n\
w3m-0.5.3_git20230121-r2 - Text based web browser\nlynx-2.9.2-r0 - A text browser for the World Wide Web.\nbad line\n";
        let pkgs = parse_search(out);
        assert_eq!(pkgs.len(), 3);
        assert_eq!(pkgs[0].name, "firefox");
        assert!(pkgs[0].gui);
        assert_eq!(pkgs[0].desc, "safe and easy web browser from Mozilla");
        assert!(!pkgs[1].gui);
        assert_eq!(pkgs[2].desc, "a text browser for the World Wide Web");
    }

    #[test]
    fn reads_sizes_and_gui_files() {
        assert_eq!(parse_size("w3m-0.5.3-r2 installed size:\n2352 KiB\n"), Some(2352 * 1024));
        assert_eq!(parse_size("x installed size:\n1.5 MiB\n"), Some(1572864));
        assert_eq!(parse_size("nothing"), None);
        assert_eq!(size_words(2352 * 1024), "2 MB");
        assert!(is_gui("foo", &["usr/share/applications/foo.desktop".into()]));
        assert!(!is_gui("foo", &["usr/bin/foo".into()]));
        assert_eq!(
            exec_line("[Desktop Entry]\nName=Foo\nExec=foo --new-window %U\n").as_deref(),
            Some("foo --new-window")
        );
    }

    #[test]
    fn package_names_are_checked() {
        assert!(valid_name("py3-pip"));
        assert!(valid_name("g++"));
        assert!(!valid_name("-rf"));
        assert!(!valid_name("a b"));
        assert!(!valid_name("a;rm"));
    }
}
