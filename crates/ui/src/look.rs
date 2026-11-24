//! Corners: one choice (Sharp, Default or Round) in Settings → Appearance, applied to every
//! part of the desktop that draws a rounded corner.
//!
//! - Our screens: style.css reads its radii from `--r-*` variables; `css` gives the values
//!   for the choice, and `common::load_css` reloads them in every running screen when the
//!   choice changes (it watches the file).
//! - niri: a `window-rule` in ~/.config/niri/tripppin-corners.kdl, included like the other
//!   settings; niri reloads its config by itself.
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
    /// niri's windows.
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
            Corners::Sharp => Radii { xs: 2, sm: 3, md: 4, lg: 5, xl: 6, control: 4, window: 4 },
            Corners::Default => Radii { xs: 4, sm: 6, md: 8, lg: 12, xl: 16, control: 8, window: 10 },
            Corners::Round => Radii { xs: 6, sm: 10, md: 14, lg: 20, xl: 26, control: 9999, window: 18 },
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

fn niri_kdl(c: Corners) -> String {
    format!("// {MARK} → Appearance.\nwindow-rule {{\n    geometry-corner-radius {}\n}}\n", c.radii().window)
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
    // niri: an include that's always there, so the choice can change back and forth.
    std::fs::create_dir_all(home.join("niri"))?;
    std::fs::write(home.join("niri").join("tripppin-corners.kdl"), niri_kdl(c))?;
    crate::display::include_in_niri("tripppin-corners.kdl")?;
    let default = c == Corners::Default;
    write_ours(&home.join("waybar/style.css"), (!default).then(|| waybar_css(c)))?;
    write_ours(&home.join("swaync/style.css"), (!default).then(|| swaync_css(c)))?;
    write_ours(&home.join("wlogout/style.css"), (!default).then(|| wlogout_css(c)))?;
    write_ours(&home.join("fuzzel/fuzzel.ini"), (!default).then(|| fuzzel_ini(c)))?;
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
        assert!(niri_kdl(Corners::Round).contains("geometry-corner-radius 18"));
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
