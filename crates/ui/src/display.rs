//! Screens through niri: resolution, refresh rate, scale and rotation, changed live with
//! `niri msg output` and remembered in ~/.config/niri/tripppin-displays.kdl, which the
//! person's niri config includes (after /etc/niri/desktop.kdl). Brightness is brightnessctl.

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    /// Refresh rate in millihertz, as niri reports it.
    pub refresh: u32,
    pub preferred: bool,
}

impl Mode {
    /// "1920×1080 · 60 hz"
    pub fn label(&self) -> String {
        format!("{}×{} · {} hz", self.width, self.height, (f64::from(self.refresh) / 1000.0).round())
    }
    /// What niri's config and `niri msg output mode` take: "1920x1080@60.000".
    pub fn spec(&self) -> String {
        format!("{}x{}@{:.3}", self.width, self.height, f64::from(self.refresh) / 1000.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub name: String,
    pub title: String,
    pub modes: Vec<Mode>,
    pub current: Option<usize>,
    pub scale: f64,
    pub transform: String,
    /// Width and height in millimetres, when the screen reports them.
    pub physical: Option<(u32, u32)>,
}

/// Parses `niri msg -j outputs`. Modes are listed largest first, one per size and rate.
pub fn parse_outputs(json: &str) -> Vec<Output> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let mut outs: Vec<Output> = map
        .values()
        .map(|o| {
            let modes: Vec<Mode> = o["modes"]
                .as_array()
                .map(|m| {
                    m.iter()
                        .map(|m| Mode {
                            width: m["width"].as_u64().unwrap_or(0) as u32,
                            height: m["height"].as_u64().unwrap_or(0) as u32,
                            refresh: m["refresh_rate"].as_u64().unwrap_or(0) as u32,
                            preferred: m["is_preferred"].as_bool().unwrap_or(false),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let make = o["make"].as_str().unwrap_or("");
            let model = o["model"].as_str().unwrap_or("");
            let title = format!("{make} {model}").trim().to_string();
            Output {
                name: o["name"].as_str().unwrap_or("").to_string(),
                title: if title.is_empty() { "screen".into() } else { title },
                modes,
                current: o["current_mode"].as_u64().map(|i| i as usize),
                scale: o["logical"]["scale"].as_f64().unwrap_or(1.0),
                transform: o["logical"]["transform"].as_str().unwrap_or("Normal").to_lowercase(),
                physical: o["physical_size"]
                    .as_array()
                    .and_then(|p| Some((p.first()?.as_u64()? as u32, p.get(1)?.as_u64()? as u32)))
                    .filter(|&(w, h)| w > 0 && h > 0),
            }
        })
        .collect();
    outs.sort_by(|a, b| a.name.cmp(&b.name));
    outs
}

pub fn outputs() -> Vec<Output> {
    Command::new("niri")
        .args(["msg", "-j", "outputs"])
        .output()
        .ok()
        .map(|o| parse_outputs(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

fn niri(args: &[&str]) -> Result<(), String> {
    let out = Command::new("niri").args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_lowercase()) }
}

/// niri's names for rotations, from the words we show.
pub const ROTATIONS: [(&str, &str); 4] = [("Normal", "normal"), ("90°", "90"), ("180°", "180"), ("270°", "270")];
pub const SCALES: [f64; 6] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.5];

/// The scale that keeps text crisp: a whole number, so every pixel lines up. 2 for
/// high-density screens with room to spare (a 13" 2560×1600 laptop, a 27" 4K or 5K), else
/// 1, which suits ordinary laptops and monitors and keeps the desktop compact. niri's own
/// guess is often fractional (1.25, 1.5), which blurs text and makes everything larger.
pub fn crisp_scale(o: &Output) -> f64 {
    let Some(mode) = o.current.and_then(|i| o.modes.get(i)) else { return 1.0 };
    let dpi = o.physical.map(|(w, _)| f64::from(mode.width) / (f64::from(w) / 25.4));
    match dpi {
        Some(dpi) if dpi >= 150.0 && mode.height >= 1440 => 2.0,
        _ => 1.0,
    }
}

/// Screens the person has set up in Settings → Display keep their choice.
fn remembered(name: &str) -> bool {
    std::fs::read_to_string(config_dir().join("tripppin-displays.kdl"))
        .is_ok_and(|t| t.contains(&format!("output \"{name}\"")))
}

/// At login: every screen the person hasn't set up gets its crisp scale.
pub fn use_crisp_scales() {
    for o in outputs() {
        let scale = crisp_scale(&o);
        if !remembered(&o.name) && (o.scale - scale).abs() > 0.01 {
            let _ = niri(&["msg", "output", &o.name, "scale", &format!("{scale}")]);
        }
    }
}

/// Applies a change now and remembers it for next time.
pub fn apply(o: &Output, mode: &Mode, scale: f64, transform: &str) -> Result<(), String> {
    niri(&["msg", "output", &o.name, "mode", &mode.spec()])?;
    niri(&["msg", "output", &o.name, "scale", &format!("{scale}")])?;
    niri(&["msg", "output", &o.name, "transform", transform])?;
    remember(&o.name, mode, scale, transform).map_err(|e| format!("Applied, but couldn't save it: {e}"))
}

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("niri")
}

/// The block written for one screen.
pub fn output_block(name: &str, mode: &Mode, scale: f64, transform: &str) -> String {
    format!(
        "output \"{name}\" {{\n    mode \"{}\"\n    scale {scale}\n    transform \"{transform}\"\n}}\n",
        mode.spec()
    )
}

/// Replaces (or adds) this screen's block in tripppin-displays.kdl, and makes sure the
/// person's niri config includes the system config and that file.
fn remember(name: &str, mode: &Mode, scale: f64, transform: &str) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("tripppin-displays.kdl");
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    std::fs::write(&file, with_screen(&old, &output_block(name, mode, scale, transform), name))?;

    include_in_niri("tripppin-displays.kdl")
}

/// The displays file with this screen's block replaced (or added). Comments are dropped
/// and the header written once, so saving again never piles up duplicates.
pub fn with_screen(old: &str, block: &str, name: &str) -> String {
    let mut blocks: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in old.lines().filter(|l| !l.trim_start().starts_with("//")) {
        current.push_str(line);
        current.push('\n');
        if line.trim() == "}" {
            blocks.push(std::mem::take(&mut current));
        }
    }
    blocks.retain(|b| !b.trim_start().starts_with(&format!("output \"{name}\"")));
    blocks.push(block.to_string());
    format!("// Screens, written by tripppin settings.\n{}", blocks.join(""))
}

/// Makes sure the person's niri config includes the system desktop, their own pointer
/// settings (seeded from the system defaults) and then `file`, which lives next to it, so
/// settings written there take effect and survive restarts.
pub fn include_in_niri(file: &str) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let pointer = dir.join("tripppin-pointer.kdl");
    if !pointer.exists() {
        std::fs::write(&pointer, crate::pointer::to_kdl(&crate::pointer::Pointer::default()))?;
    }
    let config = dir.join("config.kdl");
    let mut text = std::fs::read_to_string(&config).unwrap_or_default();
    // The whole system config would bring its pointer flags back with it.
    text = text.replace("include \"/etc/niri/config.kdl\"", "include \"/etc/niri/desktop.kdl\"");
    if !text.contains("/etc/niri/desktop.kdl") {
        text = format!(
            "// Your niri settings. The tripppin desktop comes from the system file.\ninclude \"/etc/niri/desktop.kdl\"\n{text}"
        );
    }
    if !text.contains("tripppin-pointer.kdl") {
        text.push_str("include \"tripppin-pointer.kdl\"\n");
    }
    if !text.contains(file) {
        text.push_str(&format!("include \"{file}\"\n"));
    }
    std::fs::write(&config, text)
}

/// Turns niri's window animations off (reduce motion) or back on.
pub fn set_reduce_motion(on: bool) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let body = if on { "animations {\n    off\n}\n" } else { "" };
    std::fs::write(
        dir.join("tripppin-motion.kdl"),
        format!("// Reduce motion, written by tripppin settings.\n{body}"),
    )?;
    include_in_niri("tripppin-motion.kdl")
}

pub fn reduce_motion() -> bool {
    std::fs::read_to_string(config_dir().join("tripppin-motion.kdl")).is_ok_and(|t| t.contains("off"))
}

/// The screen's brightness (0–1), when it has a backlight.
pub fn brightness() -> Option<f64> {
    let out = Command::new("brightnessctl").args(["-m", "-c", "backlight"]).output().ok()?;
    crate::osd::parse_brightnessctl(&String::from_utf8_lossy(&out.stdout))
}

pub fn set_brightness(percent: u32) {
    let _ =
        Command::new("brightnessctl").args(["-q", "-c", "backlight", "set", &format!("{}%", percent.max(5))]).status();
}

pub fn night_light() -> bool {
    Command::new("pgrep").args(["-x", "wlsunset"]).status().is_ok_and(|s| s.success())
}

pub fn set_night_light(on: bool) {
    if on {
        let _ = Command::new("setsid").args(["-f", "wlsunset", "-T", "4500", "-t", "3500"]).status();
    } else {
        let _ = Command::new("pkill").args(["-x", "wlsunset"]).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_niri_outputs() {
        let json = r#"{"Virtual-1":{"name":"Virtual-1","make":"Red Hat, Inc.","model":"QEMU Monitor",
          "modes":[{"width":1280,"height":800,"refresh_rate":59995,"is_preferred":true},
                   {"width":1920,"height":1080,"refresh_rate":60000,"is_preferred":false}],
          "current_mode":0,"logical":{"x":0,"y":0,"width":1280,"height":800,"scale":1.25,"transform":"Normal"}}}"#;
        let outs = parse_outputs(json);
        assert_eq!(outs.len(), 1);
        let o = &outs[0];
        assert_eq!(o.title, "Red Hat, Inc. QEMU Monitor");
        assert_eq!(o.current, Some(0));
        assert_eq!(o.scale, 1.25);
        assert_eq!(o.transform, "normal");
        assert_eq!(o.physical, None);
        assert_eq!(crisp_scale(o), 1.0);
        assert_eq!(o.modes[0].label(), "1280×800 · 60 hz");
        assert_eq!(o.modes[0].spec(), "1280x800@59.995");
        assert_eq!(
            output_block("Virtual-1", &o.modes[1], 1.5, "90"),
            "output \"Virtual-1\" {\n    mode \"1920x1080@60.000\"\n    scale 1.5\n    transform \"90\"\n}\n"
        );
    }

    #[test]
    fn saving_a_screen_replaces_its_block() {
        let old = "// Screens, written by tripppin settings.\n// Screens, written by tripppin settings.\n\
                   output \"Virtual-1\" {\n    scale 1.25\n}\noutput \"HDMI-A-1\" {\n    scale 1\n}\n";
        let new = with_screen(old, "output \"Virtual-1\" {\n    scale 1\n}\n", "Virtual-1");
        assert_eq!(
            new,
            "// Screens, written by tripppin settings.\noutput \"HDMI-A-1\" {\n    scale 1\n}\n\
             output \"Virtual-1\" {\n    scale 1\n}\n"
        );
    }

    #[test]
    fn picks_whole_number_scales() {
        let screen = |w, h, mm: (u32, u32)| Output {
            name: "eDP-1".into(),
            title: "screen".into(),
            modes: vec![Mode { width: w, height: h, refresh: 60000, preferred: true }],
            current: Some(0),
            scale: 1.25,
            transform: "normal".into(),
            physical: Some(mm),
        };
        // A 14" 1920×1200 laptop, a 24" 1080p monitor: 1.
        assert_eq!(crisp_scale(&screen(1920, 1200, (302, 189))), 1.0);
        assert_eq!(crisp_scale(&screen(1920, 1080, (527, 296))), 1.0);
        // A 13" 2560×1600 laptop, a 27" 4K or 5K: 2.
        assert_eq!(crisp_scale(&screen(2560, 1600, (286, 179))), 2.0);
        assert_eq!(crisp_scale(&screen(3840, 2160, (597, 336))), 2.0);
        assert_eq!(crisp_scale(&screen(5120, 2880, (597, 336))), 2.0);
        // A 27" 1440p monitor: 1.
        assert_eq!(crisp_scale(&screen(2560, 1440, (597, 336))), 1.0);
    }
}
