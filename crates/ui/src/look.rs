//! Corners: one choice (Sharp, Default or Round), set with `tripppin-ui corners`, applied to
//! our own screens and the pre-Plasma bar files. (Settings no longer offers it: the Tiles
//! design is square.)
//!
//! - Our screens: style.css reads its radii from `--r-*` variables; `css` gives the values
//!   for the choice, and `common::load_css` reloads them in every running screen when the
//!   choice changes (it watches the file).
//! - The bar, quick settings, the power menu: a small stylesheet in ~/.config/<app>/ that
//!   imports the system one and changes only the radii; the bar and quick settings reload.
//! - The launcher: ~/.config/fuzzel/fuzzel.ini includes the system file and sets the radius.
//!
//! The system files are the Default look, so Default just removes our override files. A file
//! someone wrote themselves (without our first line) is never touched.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corners {
    Sharp,
    Default,
    Round,
}

/// Every choice, in the order Settings shows them: (choice, its word, a short description).
#[allow(dead_code)] // Settings no longer lists the choices; kept for the CLI and the AI
pub const ALL: [(Corners, &str, &str); 3] = [
    (Corners::Sharp, "Sharp", "Crisp, nearly square"),
    (Corners::Default, "Default", "Softly rounded"),
    (Corners::Round, "Round", "Fully rounded"),
];

/// The first line of every file written here: the mark that it's ours to replace.
const MARK: &str = "Corners, written by Tripppin Settings";

/// The radii for one choice, in px.
struct Radii {
    xs: u32,
    sm: u32,
    md: u32,
    lg: u32,
    xl: u32,
    /// Buttons, fields and chips; Round makes them pills.
    control: u32,
    /// Windows (GTK apps' own frames).
    window: u32,
}

impl Corners {
    pub fn id(self) -> &'static str {
        match self {
            Corners::Sharp => "sharp",
            Corners::Default => "default",
            Corners::Round => "round",
        }
    }

    pub fn from_id(id: &str) -> Corners {
        match id.trim() {
            "sharp" => Corners::Sharp,
            "round" => Corners::Round,
            _ => Corners::Default,
        }
    }

    fn radii(self) -> Radii {
        match self {
            Corners::Sharp => Radii { xs: 2, sm: 3, md: 4, lg: 4, xl: 4, control: 4, window: 0 },
            Corners::Default => Radii { xs: 8, sm: 12, md: 14, lg: 20, xl: 26, control: 12, window: 14 },
            Corners::Round => Radii { xs: 10, sm: 14, md: 18, lg: 26, xl: 32, control: 9999, window: 22 },
        }
    }
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Where the choice is kept: ~/.config/tripppin/corners.
pub fn file() -> PathBuf {
    config_home().join("tripppin").join("corners")
}

pub fn current() -> Corners {
    std::fs::read_to_string(file()).map(|s| Corners::from_id(&s)).unwrap_or(Corners::Default)
}

/// The `--r-*` variables for our screens.
pub fn css(c: Corners) -> String {
    let r = c.radii();
    format!(
        ":root {{ --r-xs: {}px; --r-sm: {}px; --r-md: {}px; --r-lg: {}px; --r-xl: {}px; --r-control: {}px; }}",
        r.xs, r.sm, r.md, r.lg, r.xl, r.control
    )
}

fn waybar_css(c: Corners) -> String {
    let r = c.radii();
    let bar = r.lg.min(16);
    let item = if r.control > 100 { 9999 } else { r.sm };
    format!(
        "/* {MARK} → Appearance. */\n@import url(\"file:///etc/xdg/waybar/style.css\");\n\
         window#waybar {{ border-radius: {bar}px; }}\n\
         #network, #pulseaudio, #bluetooth, #battery, #custom-notifications, #custom-power, #custom-clock, #language {{ border-radius: {item}px; }}\n\
         #tray menu {{ border-radius: {}px; }}\n",
        r.md
    )
}

fn swaync_css(c: Corners) -> String {
    let r = c.radii();
    format!(
        "/* {MARK} → Appearance. */\n@import url(\"file:///etc/xdg/swaync/style.css\");\n\
         .control-center {{ border-radius: {xl}px; }}\n\
         .notification, .notification-default-action {{ border-radius: {lg}px; }}\n\
         .notification-action {{ border-radius: 0; }}\n\
         .widget-buttons-grid > flowbox > flowboxchild > button, .widget-title > button, .widget-menubar button {{ border-radius: {control}px; }}\n\
         .widget-volume, .widget-slider, .widget-backlight, .widget-mpris-player, .widget-dnd {{ border-radius: {md}px; }}\n\
         .inline-reply-entry, .inline-reply-button, .app-icon, .image {{ border-radius: {sm}px; }}\n",
        xl = r.xl,
        lg = r.lg,
        md = r.md,
        sm = r.sm,
        control = r.control,
    )
}

fn wlogout_css(c: Corners) -> String {
    let r = c.radii();
    format!(
        "/* {MARK} → Appearance. */\n@import url(\"file:///etc/wlogout/style.css\");\nbutton {{ border-radius: {}px; }}\n",
        r.xl
    )
}

fn fuzzel_ini(c: Corners) -> String {
    let r = c.radii();
    format!(
        "# {MARK} → Appearance.\ninclude=/etc/xdg/fuzzel/fuzzel.ini\n\n[border]\nradius={}\nselection-radius={}\n",
        r.xl, r.sm
    )
}

/// Sets the window corner radius in a GTK `gtk.css` that came from Tripppin (the skel copy
/// starts with "Tripppin colours"): `--window-radius` and the `border-radius` inside any
/// `window.background` rule. Someone's own file is left alone.
pub fn window_radius_css(old: &str, px: u32) -> Option<String> {
    if !old.lines().next().is_some_and(|l| l.contains("Tripppin colours")) {
        return None;
    }
    let mut in_window = false;
    let mut out = String::with_capacity(old.len());
    for line in old.lines() {
        let t = line.trim_start();
        let indent = &line[..line.len() - t.len()];
        if t.starts_with("window.background") && t.ends_with('{') {
            in_window = true;
        }
        if t.starts_with("--window-radius:") {
            out.push_str(&format!("{indent}--window-radius: {px}px;"));
        } else if in_window && t.starts_with("border-radius:") {
            out.push_str(&format!("{indent}border-radius: {px}px;"));
        } else {
            out.push_str(line);
        }
        out.push('\n');
        if t.starts_with('}') {
            in_window = false;
        }
    }
    Some(out)
}

/// Writes `text` to `path`, or removes the file when `text` is None, unless the file is
/// someone's own (it exists without our mark).
fn write_ours(path: &Path, text: Option<String>) -> std::io::Result<()> {
    if let Ok(old) = std::fs::read_to_string(path)
        && !old.lines().next().is_some_and(|l| l.contains(MARK))
    {
        return Ok(());
    }
    match text {
        Some(t) => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, t)
        }
        None => match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
    }
}

/// Saves the choice and applies it everywhere it can be applied now.
pub fn set(c: Corners) -> std::io::Result<()> {
    let home = config_home();
    std::fs::create_dir_all(home.join("tripppin"))?;
    // Our screens watch this file and restyle themselves.
    std::fs::write(file(), c.id())?;
    let default = c == Corners::Default;
    write_ours(&home.join("waybar/style.css"), (!default).then(|| waybar_css(c)))?;
    write_ours(&home.join("swaync/style.css"), (!default).then(|| swaync_css(c)))?;
    write_ours(&home.join("wlogout/style.css"), (!default).then(|| wlogout_css(c)))?;
    write_ours(&home.join("fuzzel/fuzzel.ini"), (!default).then(|| fuzzel_ini(c)))?;
    // Every GTK app's own window corners, so the window itself changes shape, not just niri's
    // outline around it. GTK reloads these files by itself.
    for gtk in ["gtk-4.0", "gtk-3.0"] {
        let path = home.join(gtk).join("gtk.css");
        if let Some(new) = std::fs::read_to_string(&path).ok().and_then(|old| window_radius_css(&old, c.radii().window))
        {
            std::fs::write(&path, new)?;
        }
    }
    // The bar reloads its style on SIGUSR2; quick settings on request.
    let _ = Command::new("pkill").args(["-USR2", "-x", "waybar"]).status();
    // Only when it's running: asking swaync-client otherwise starts it through D-Bus.
    if Command::new("pgrep").args(["-x", "swaync"]).status().is_ok_and(|s| s.success()) {
        let _ = Command::new("swaync-client").args(["-rs", "-sw"]).status();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_corners_follow_the_choice() {
        let ours = "/* Tripppin colours for every GTK 4 app */\n:root {\n    --window-radius: 10px;\n}\nwindow.background,\nwindow.background.csd {\n    border-radius: 10px;\n}\nbutton {\n    border-radius: 10px;\n}\n";
        let sharp = window_radius_css(ours, 4).unwrap();
        assert!(sharp.contains("--window-radius: 4px;"));
        assert!(sharp.contains("window.background.csd {\n    border-radius: 4px;"));
        assert!(sharp.contains("button {\n    border-radius: 10px;"), "only windows change");
        assert_eq!(window_radius_css("/* mine */\nwindow {}\n", 4), None);
    }

    #[test]
    fn ids_round_trip() {
        for (c, ..) in ALL {
            assert_eq!(Corners::from_id(c.id()), c);
        }
        assert_eq!(Corners::from_id("nonsense"), Corners::Default);
        assert_eq!(Corners::from_id("round\n"), Corners::Round);
    }

    #[test]
    fn default_is_less_round_than_round() {
        let (d, r) = (Corners::Default.radii(), Corners::Round.radii());
        assert!(d.xl < r.xl && d.window < r.window && d.control < r.control);
        assert!(css(Corners::Sharp).contains("--r-control: 4px"));
    }

    #[test]
    fn overrides_import_the_system_files() {
        assert!(waybar_css(Corners::Round).contains("@import url(\"file:///etc/xdg/waybar/style.css\")"));
        assert!(swaync_css(Corners::Sharp).contains("/etc/xdg/swaync/style.css"));
        assert!(fuzzel_ini(Corners::Sharp).contains("include=/etc/xdg/fuzzel/fuzzel.ini"));
    }

    #[test]
    fn someone_elses_file_is_left_alone() {
        let dir = std::env::temp_dir().join(format!("tripppin-look-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let theirs = dir.join("style.css");
        std::fs::write(&theirs, "* { color: red; }\n").unwrap();
        write_ours(&theirs, Some(waybar_css(Corners::Round))).unwrap();
        assert_eq!(std::fs::read_to_string(&theirs).unwrap(), "* { color: red; }\n");
        let ours = dir.join("ours.css");
        write_ours(&ours, Some(waybar_css(Corners::Round))).unwrap();
        assert!(std::fs::read_to_string(&ours).unwrap().contains(MARK));
        write_ours(&ours, None).unwrap();
        assert!(!ours.exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
