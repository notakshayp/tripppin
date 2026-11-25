//! Screens through KWin: resolution, refresh rate, scale, rotation and where each screen sits,
//! changed live with `kscreen-doctor` (Plasma's KScreen remembers each setup by itself).
//! Brightness is brightnessctl; night light is KWin's own, set in kwinrc.

use serde_json::Value;
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    /// kscreen-doctor's id for the mode.
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Refresh rate in millihertz.
    pub refresh: u32,
    pub preferred: bool,
}

impl Mode {
    /// "1920×1080 · 60 hz"
    pub fn label(&self) -> String {
        format!("{}×{} · {} hz", self.width, self.height, (f64::from(self.refresh) / 1000.0).round())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub name: String,
    pub title: String,
    pub modes: Vec<Mode>,
    pub current: Option<usize>,
    pub scale: f64,
    /// One of the words in `ROTATIONS`: normal, left, inverted, right.
    pub transform: String,
    /// Width and height in millimetres, when the screen reports them.
    pub physical: Option<(u32, u32)>,
    /// Top-left corner in the shared desktop space.
    pub pos: (i32, i32),
    pub enabled: bool,
}

/// A friendlier name than the connector's: "eDP-1" is the built-in screen.
pub fn friendly(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.starts_with("edp") || lower.starts_with("lvds") || lower.starts_with("dsi") {
        "Built-in screen".into()
    } else if lower.starts_with("virtual") {
        "Virtual screen".into()
    } else {
        format!("Screen {name}")
    }
}

/// kscreen-doctor's rotation number as a word.
fn rotation_word(n: u64) -> &'static str {
    match n {
        2 => "left",
        4 => "inverted",
        8 => "right",
        _ => "normal",
    }
}

/// Parses `kscreen-doctor -j`. Modes are listed largest first, then the fastest.
pub fn parse_outputs(json: &str) -> Vec<Output> {
    let Ok(root) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let mut outs: Vec<Output> = root["outputs"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .filter(|o| o["connected"].as_bool().unwrap_or(true))
        .map(|o| {
            let preferred: Vec<&str> = o["preferredModes"]
                .as_array()
                .map(|p| p.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let mut modes: Vec<Mode> = o["modes"]
                .as_array()
                .map(|m| {
                    m.iter()
                        .map(|m| {
                            let id = m["id"].as_str().map(str::to_string).unwrap_or_else(|| m["id"].to_string());
                            Mode {
                                preferred: preferred.contains(&id.as_str()),
                                id,
                                width: m["size"]["width"].as_u64().unwrap_or(0) as u32,
                                height: m["size"]["height"].as_u64().unwrap_or(0) as u32,
                                refresh: (m["refreshRate"].as_f64().unwrap_or(0.0) * 1000.0).round() as u32,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let current_id = o["currentModeId"].as_str().map(str::to_string);
            modes.sort_by_key(|m| std::cmp::Reverse((m.width * m.height, m.refresh)));
            // Several modes can look the same ("1920×1080 · 60 hz" twice): keep the first,
            // and never drop the one in use.
            let mut seen = Vec::new();
            modes.retain(|m| {
                let keep = !seen.contains(&m.label()) || Some(&m.id) == current_id.as_ref();
                seen.push(m.label());
                keep
            });
            let name = o["name"].as_str().unwrap_or("").to_string();
            Output {
                title: friendly(&name),
                current: modes.iter().position(|m| Some(&m.id) == current_id.as_ref()),
                modes,
                scale: o["scale"].as_f64().unwrap_or(1.0),
                transform: rotation_word(o["rotation"].as_u64().unwrap_or(1)).into(),
                physical: physical_from_edid(&name),
                pos: (o["pos"]["x"].as_i64().unwrap_or(0) as i32, o["pos"]["y"].as_i64().unwrap_or(0) as i32),
                enabled: o["enabled"].as_bool().unwrap_or(true),
                name,
            }
        })
        .collect();
    outs.sort_by_key(|o| (o.pos.0, o.pos.1));
    outs
}

pub fn outputs() -> Vec<Output> {
    Command::new("kscreen-doctor")
        .arg("-j")
        .output()
        .ok()
        .map(|o| parse_outputs(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// The screen's size in millimetres from the EDID bytes (centimetres at offsets 21 and 22).
pub fn parse_edid_size(edid: &[u8]) -> Option<(u32, u32)> {
    let (w, h) = (u32::from(*edid.get(21)?), u32::from(*edid.get(22)?));
    (w > 0 && h > 0).then_some((w * 10, h * 10))
}

/// The size of the connector `name` ("eDP-1") from the kernel's copy of its EDID.
fn physical_from_edid(name: &str) -> Option<(u32, u32)> {
    let suffix = format!("-{name}");
    std::fs::read_dir("/sys/class/drm")
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().ends_with(&suffix))
        .and_then(|e| std::fs::read(e.path().join("edid")).ok())
        .and_then(|b| parse_edid_size(&b))
}

/// kscreen-doctor's names for rotations, from the words we show.
pub const ROTATIONS: [(&str, &str); 4] =
    [("Normal", "normal"), ("90°", "left"), ("180°", "inverted"), ("270°", "right")];
pub const SCALES: [f64; 6] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.5];

/// The scale that keeps text crisp: a whole number, so every pixel lines up. 2 for
/// high-density screens with room to spare (a 13" 2560×1600 laptop, a 27" 4K or 5K), else
/// 1, which suits ordinary laptops and monitors and keeps the desktop compact.
pub fn crisp_scale(o: &Output) -> f64 {
    let Some(mode) = o.current.and_then(|i| o.modes.get(i)) else { return 1.0 };
    let dpi = o.physical.map(|(w, _)| f64::from(mode.width) / (f64::from(w) / 25.4));
    match dpi {
        Some(dpi) if dpi >= 150.0 && mode.height >= 1440 => 2.0,
        _ => 1.0,
    }
}

fn marker() -> std::path::PathBuf {
    crate::kde::config_home().join("tripppin/scales-set")
}

/// Once, on the first login: every screen gets its crisp scale. After that KScreen keeps
/// whatever the person chose.
pub fn use_crisp_scales() {
    if marker().exists() {
        return;
    }
    for o in outputs() {
        let scale = crisp_scale(&o);
        if (o.scale - scale).abs() > 0.01 {
            let _ = kscreen(&[format!("output.{}.scale.{scale}", o.name)]);
        }
    }
    if let Some(dir) = marker().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(marker(), "");
}

fn kscreen(args: &[String]) -> Result<(), String> {
    let out = Command::new("kscreen-doctor").args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = if err.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { err };
        Err(err.trim().to_lowercase())
    }
}

/// Applies a change now; KScreen remembers it for this set of screens.
pub fn apply(o: &Output, mode: &Mode, scale: f64, transform: &str) -> Result<(), String> {
    kscreen(&[
        format!("output.{}.mode.{}", o.name, mode.id),
        format!("output.{}.scale.{scale}", o.name),
        format!("output.{}.rotation.{transform}", o.name),
    ])
}

/// Where a screen sits next to another.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Side {
    Right,
    Left,
    Above,
    Below,
}

pub const SIDES: [(Side, &str); 4] =
    [(Side::Right, "Right of"), (Side::Left, "Left of"), (Side::Above, "Above"), (Side::Below, "Below")];

/// A screen's size in the shared space: its pixels over its scale, turned for rotation.
pub fn logical_size(o: &Output) -> (i32, i32) {
    let Some(m) = o.current.and_then(|i| o.modes.get(i)) else { return (0, 0) };
    let (mut w, mut h) = (f64::from(m.width) / o.scale, f64::from(m.height) / o.scale);
    if o.transform == "left" || o.transform == "right" {
        std::mem::swap(&mut w, &mut h);
    }
    (w.round() as i32, h.round() as i32)
}

/// The corner `o` should take to sit on `side` of `anchor`, with their near edges lined up.
pub fn place(o: &Output, anchor: &Output, side: Side) -> (i32, i32) {
    let (ow, oh) = logical_size(o);
    let (aw, ah) = logical_size(anchor);
    let (x, y) = anchor.pos;
    match side {
        Side::Right => (x + aw, y),
        Side::Left => (x - ow, y),
        Side::Above => (x, y - oh),
        Side::Below => (x, y + ah),
    }
}

/// Which side of `anchor` `o` is on now (the nearest one when it isn't flush).
pub fn side_of(o: &Output, anchor: &Output) -> Side {
    SIDES.iter().map(|s| s.0).find(|s| place(o, anchor, *s) == o.pos).unwrap_or_else(|| {
        let (dx, dy) = (o.pos.0 - anchor.pos.0, o.pos.1 - anchor.pos.1);
        match (dx.abs() >= dy.abs(), dx >= 0, dy >= 0) {
            (true, true, _) => Side::Right,
            (true, false, _) => Side::Left,
            (false, _, true) => Side::Below,
            (false, _, false) => Side::Above,
        }
    })
}

pub fn set_position(o: &Output, pos: (i32, i32)) -> Result<(), String> {
    kscreen(&[format!("output.{}.position.{},{}", o.name, pos.0, pos.1)])
}

/// Turns window animations off (reduce motion) or back on, in KWin and Plasma.
pub fn set_reduce_motion(on: bool) -> std::io::Result<()> {
    let ok = if on {
        crate::kde::write("kdeglobals", "KDE", "AnimationDurationFactor", "0")
    } else {
        crate::kde::delete("kdeglobals", "KDE", "AnimationDurationFactor")
    };
    crate::kde::kwin_reconfigure();
    if ok { Ok(()) } else { Err(std::io::Error::other("kwriteconfig6 failed")) }
}

pub fn reduce_motion() -> bool {
    crate::kde::read("kdeglobals", "KDE", "AnimationDurationFactor").is_some_and(|v| v.trim() == "0")
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
    crate::kde::read_bool("kwinrc", "NightColor", "Active", false)
}

/// Night light on or off: KWin warms the screen from 8 pm to 7 am.
pub fn set_night_light(on: bool) {
    if on {
        crate::kde::write("kwinrc", "NightColor", "Mode", "Times");
        crate::kde::write("kwinrc", "NightColor", "EveningBeginFixed", "2000");
        crate::kde::write("kwinrc", "NightColor", "MorningBeginFixed", "0700");
        crate::kde::write("kwinrc", "NightColor", "NightTemperature", "3500");
    }
    crate::kde::write_bool("kwinrc", "NightColor", "Active", on);
    crate::kde::kwin_reconfigure();
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"outputs":[
      {"id":2,"name":"HDMI-A-1","enabled":true,"connected":true,"currentModeId":"5","scale":1.0,"rotation":1,
       "pos":{"x":1280,"y":0},
       "modes":[{"id":"5","name":"1920x1080@60","refreshRate":60.0,"size":{"width":1920,"height":1080}}],
       "preferredModes":["5"]},
      {"id":1,"name":"eDP-1","enabled":true,"connected":true,"currentModeId":"0","scale":1.25,"rotation":1,
       "pos":{"x":0,"y":0},
       "modes":[{"id":"0","name":"1280x800@60","refreshRate":59.995,"size":{"width":1280,"height":800}},
                {"id":"1","name":"1920x1200@60","refreshRate":60.0,"size":{"width":1920,"height":1200}},
                {"id":"2","name":"1920x1200@60","refreshRate":60.0,"size":{"width":1920,"height":1200}},
                {"id":"3","name":"1920x1200@48","refreshRate":48.0,"size":{"width":1920,"height":1200}}],
       "preferredModes":["1"]},
      {"id":3,"name":"DP-2","enabled":false,"connected":false,"modes":[]}],
      "screen":{"currentSize":{"width":3200,"height":1200}}}"#;

    #[test]
    fn reads_kscreen_doctor_json() {
        let outs = parse_outputs(JSON);
        assert_eq!(outs.len(), 2, "the unplugged connector is left out");
        let o = &outs[0];
        assert_eq!(o.name, "eDP-1", "ordered left to right");
        assert_eq!(o.title, "Built-in screen");
        assert_eq!(o.scale, 1.25);
        assert_eq!(o.transform, "normal");
        // Largest first, the duplicate 1920×1200 dropped.
        let labels: Vec<String> = o.modes.iter().map(Mode::label).collect();
        assert_eq!(labels, ["1920×1200 · 60 hz", "1920×1200 · 48 hz", "1280×800 · 60 hz"]);
        assert_eq!(o.current, Some(2));
        assert!(o.modes[0].preferred);
        assert_eq!(o.modes[0].id, "1");
        assert_eq!(outs[1].title, "Screen HDMI-A-1");
        assert_eq!(outs[1].pos, (1280, 0));
        assert!(parse_outputs("not json").is_empty());
    }

    #[test]
    fn reads_rotation_words() {
        assert_eq!(rotation_word(1), "normal");
        assert_eq!(rotation_word(2), "left");
        assert_eq!(rotation_word(4), "inverted");
        assert_eq!(rotation_word(8), "right");
    }

    #[test]
    fn arranges_screens_beside_each_other() {
        let outs = parse_outputs(JSON);
        let (laptop, monitor) = (&outs[0], &outs[1]);
        // The laptop is 1280×800 at 1.25 = 1024×640 in the shared space; the monitor sits at 1280.
        assert_eq!(logical_size(laptop), (1024, 640));
        assert_eq!(place(monitor, laptop, Side::Right), (1024, 0));
        assert_eq!(place(monitor, laptop, Side::Left), (-1920, 0));
        assert_eq!(place(monitor, laptop, Side::Above), (0, -1080));
        assert_eq!(place(monitor, laptop, Side::Below), (0, 640));
        assert_eq!(side_of(monitor, laptop), Side::Right, "a gap still reads as right of");
        let mut below = monitor.clone();
        below.pos = (0, 640);
        assert_eq!(side_of(&below, laptop), Side::Below);
        let mut turned = laptop.clone();
        turned.transform = "left".into();
        assert_eq!(logical_size(&turned), (640, 1024));
    }

    #[test]
    fn reads_screen_size_from_edid() {
        let mut edid = vec![0u8; 128];
        edid[21] = 29;
        edid[22] = 18;
        assert_eq!(parse_edid_size(&edid), Some((290, 180)));
        assert_eq!(parse_edid_size(&[0; 128]), None);
        assert_eq!(parse_edid_size(&[]), None);
    }

    #[test]
    fn picks_whole_number_scales() {
        let screen = |w, h, mm: (u32, u32)| Output {
            name: "eDP-1".into(),
            title: "screen".into(),
            modes: vec![Mode { id: "0".into(), width: w, height: h, refresh: 60000, preferred: true }],
            current: Some(0),
            scale: 1.25,
            transform: "normal".into(),
            physical: Some(mm),
            pos: (0, 0),
            enabled: true,
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
