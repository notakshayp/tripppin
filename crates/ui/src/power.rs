//! Power: the battery (UPower), the power mode (power-profiles-daemon) and what happens when
//! the computer is left alone (swayidle, configured by ~/.config/tripppin/idle).

use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Battery {
    pub percent: u8,
    /// charging, discharging, fully-charged, pending-charge…
    pub state: String,
    /// "2.1 hours" style, as UPower words it; empty when unknown.
    pub time_left: String,
    /// Full capacity now versus new, in percent.
    pub health: Option<u8>,
}

/// Parses `upower -i <battery>`.
pub fn parse_upower(out: &str) -> Option<Battery> {
    let mut b = Battery::default();
    let mut seen = false;
    for line in out.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "percentage" => {
                b.percent = value.trim_end_matches('%').trim().parse::<f64>().ok()?.round() as u8;
                seen = true;
            }
            "state" => b.state = value.to_string(),
            "time to empty" | "time to full" => b.time_left = value.to_string(),
            "capacity" => b.health = value.trim_end_matches('%').trim().parse::<f64>().ok().map(|c| c.round() as u8),
            _ => {}
        }
    }
    seen.then_some(b)
}

/// The battery, if the computer has one.
pub fn battery() -> Option<Battery> {
    let list = Command::new("upower").arg("-e").output().ok()?;
    let path = String::from_utf8_lossy(&list.stdout).lines().find(|l| l.contains("battery_BAT"))?.to_string();
    let info = Command::new("upower").args(["-i", &path]).output().ok()?;
    parse_upower(&String::from_utf8_lossy(&info.stdout))
}

/// One line for the page's status: "84% · 2.1 hours left" or "charging · 84%".
pub fn summary(b: &Battery) -> String {
    match b.state.as_str() {
        "charging" if !b.time_left.is_empty() => format!("charging · {}% · full in {}", b.percent, b.time_left),
        "charging" => format!("charging · {}%", b.percent),
        "fully-charged" => "fully charged · plugged in".to_string(),
        _ if !b.time_left.is_empty() => format!("{}% · {} left", b.percent, b.time_left),
        _ => format!("{}%", b.percent),
    }
}

pub const MODES: [(&str, &str); 3] =
    [("power-saver", "saver"), ("balanced", "balanced"), ("performance", "performance")];

/// The current power mode and the ones this machine offers.
pub fn modes() -> Option<(String, Vec<String>)> {
    let get = Command::new("powerprofilesctl").arg("get").output().ok()?;
    if !get.status.success() {
        return None;
    }
    let now = String::from_utf8_lossy(&get.stdout).trim().to_string();
    let list = Command::new("powerprofilesctl").arg("list").output().ok()?;
    let offered = String::from_utf8_lossy(&list.stdout)
        .lines()
        .filter_map(|l| l.trim().trim_start_matches('*').trim().strip_suffix(':').map(str::to_string))
        .collect();
    Some((now, offered))
}

pub fn set_mode(mode: &str) -> Result<(), String> {
    let out = Command::new("powerprofilesctl").args(["set", mode]).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_lowercase()) }
}

/// Minutes until lock, screen off and sleep; 0 means never.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Idle {
    pub lock: u32,
    pub screen_off: u32,
    pub sleep: u32,
}

impl Default for Idle {
    fn default() -> Self {
        Idle { lock: 5, screen_off: 10, sleep: 30 }
    }
}

pub const IDLE_CHOICES: [u32; 7] = [1, 2, 5, 10, 15, 30, 0];

fn idle_file() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("tripppin/idle")
}

/// The swayidle config for these timings. Lock and screen-off happen on battery and mains;
/// "before-sleep" locks first so the computer never wakes up unlocked.
pub fn swayidle_config(idle: &Idle) -> String {
    let mut out = String::from("# When the computer is left alone. Written by tripppin settings.\n");
    if idle.lock > 0 {
        // Fade the screen half a minute first, so locking never comes as a surprise.
        out.push_str(&format!(
            "timeout {} 'tripppin-ui dim' resume 'pkill -f \"^(/usr/bin/)?tripppin-ui dim\"'\n",
            idle.lock * 60 - 30
        ));
        out.push_str(&format!("timeout {} 'tripppin-ui lock'\n", idle.lock * 60));
    }
    if idle.screen_off > 0 {
        out.push_str(&format!(
            "timeout {} 'niri msg action power-off-monitors' resume 'niri msg action power-on-monitors'\n",
            idle.screen_off * 60
        ));
    }
    if idle.sleep > 0 {
        out.push_str(&format!("timeout {} 'systemctl suspend'\n", idle.sleep * 60));
    }
    out.push_str("before-sleep 'tripppin-ui lock'\n");
    out
}

/// Reads the timings back from the file (defaults where it says nothing).
pub fn parse_idle(text: &str) -> Idle {
    let mut idle = Idle { lock: 0, screen_off: 0, sleep: 0 };
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("timeout") {
            continue;
        }
        let secs: u32 = words.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let rest = words.collect::<Vec<_>>().join(" ");
        if rest.contains("dim") {
            continue;
        } else if rest.contains("lock") {
            idle.lock = secs / 60;
        } else if rest.contains("power-off-monitors") {
            idle.screen_off = secs / 60;
        } else if rest.contains("suspend") {
            idle.sleep = secs / 60;
        }
    }
    idle
}

pub fn idle() -> Idle {
    std::fs::read_to_string(idle_file()).map(|t| parse_idle(&t)).unwrap_or_default()
}

/// Saves the timings and restarts swayidle with them.
pub fn set_idle(idle: &Idle) -> std::io::Result<()> {
    let file = idle_file();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&file, swayidle_config(idle))?;
    let _ = Command::new("pkill").args(["-x", "swayidle"]).status();
    let _ = Command::new("niri")
        .args(["msg", "action", "spawn", "--", "swayidle", "-w", "-C", &file.to_string_lossy()])
        .status();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_battery() {
        let out = "  native-path:          BAT0\n  battery\n    state:               discharging\n    \
                   time to empty:       2.1 hours\n    percentage:          84%\n    capacity:            91.4%\n";
        let b = parse_upower(out).unwrap();
        assert_eq!(
            b,
            Battery { percent: 84, state: "discharging".into(), time_left: "2.1 hours".into(), health: Some(91) }
        );
        assert_eq!(summary(&b), "84% · 2.1 hours left");
        assert!(parse_upower("nothing here").is_none());
    }

    #[test]
    fn idle_timings_round_trip() {
        let idle = Idle { lock: 5, screen_off: 10, sleep: 0 };
        let text = swayidle_config(&idle);
        assert!(text.contains("timeout 300 'tripppin-ui lock'"));
        assert!(!text.contains("suspend'\ntimeout"));
        assert_eq!(parse_idle(&text), idle);
    }
}
