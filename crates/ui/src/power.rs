//! Power: the battery (UPower), the power mode (power-profiles-daemon) and what happens when
//! the computer is left alone (powerdevil's settings in powerdevilrc, and the lock screen's
//! in kscreenlockerrc).

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
        "charging" if !b.time_left.is_empty() => format!("Charging · {}% · full in {}", b.percent, b.time_left),
        "charging" => format!("Charging · {}%", b.percent),
        "fully-charged" => "Fully charged · plugged in".to_string(),
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

/// powerdevil's profiles that the timings apply to: on mains and on battery.
const PROFILES: [&str; 2] = ["AC", "Battery"];

/// Minutes as powerdevil writes them: seconds, with -1 for never.
pub fn to_secs(minutes: u32) -> i64 {
    if minutes == 0 { -1 } else { i64::from(minutes) * 60 }
}

/// Seconds as powerdevil writes them, back to minutes (0 for never).
pub fn to_minutes(secs: i64) -> u32 {
    if secs <= 0 { 0 } else { (secs / 60) as u32 }
}

/// Reads the timings back: the lock from kscreenlockerrc, the rest from powerdevilrc.
pub fn idle() -> Idle {
    use crate::kde::{read, read_bool, read_num};
    let d = Idle::default();
    let lock = if read_bool("kscreenlockerrc", "Daemon", "Autolock", true) {
        read_num("kscreenlockerrc", "Daemon", "Timeout", d.lock)
    } else {
        0
    };
    let screen_off = read("powerdevilrc", "AC][Display", "TurnOffDisplayIdleTimeoutSec")
        .and_then(|v| v.parse().ok())
        .map_or(d.screen_off, to_minutes);
    let sleep = if read_num("powerdevilrc", "AC][SuspendAndShutdown", "AutoSuspendAction", 1u32) == 0 {
        0
    } else {
        read("powerdevilrc", "AC][SuspendAndShutdown", "AutoSuspendIdleTimeoutSec")
            .and_then(|v| v.parse().ok())
            .map_or(d.sleep, to_minutes)
    };
    Idle { lock, screen_off, sleep }
}

/// What happens when the lid is closed: sleep (true) or nothing.
pub fn lid_sleeps() -> bool {
    crate::kde::read_num("powerdevilrc", "AC][SuspendAndShutdown", "LidAction", 1u32) == 1
}

pub fn set_lid_sleeps(on: bool) -> std::io::Result<()> {
    let mut ok = true;
    for p in PROFILES {
        ok &= crate::kde::write(
            "powerdevilrc",
            &format!("{p}][SuspendAndShutdown"),
            "LidAction",
            if on { "1" } else { "0" },
        );
    }
    reparse();
    if ok { Ok(()) } else { Err(std::io::Error::other("kwriteconfig6 failed")) }
}

/// Saves the timings and tells the lock screen and powerdevil.
pub fn set_idle(idle: &Idle) -> std::io::Result<()> {
    use crate::kde::{write, write_bool};
    let mut ok = write_bool("kscreenlockerrc", "Daemon", "Autolock", idle.lock > 0);
    if idle.lock > 0 {
        ok &= write("kscreenlockerrc", "Daemon", "Timeout", &idle.lock.to_string());
    }
    for p in PROFILES {
        ok &= write(
            "powerdevilrc",
            &format!("{p}][Display"),
            "TurnOffDisplayIdleTimeoutSec",
            &to_secs(idle.screen_off).to_string(),
        );
        let group = format!("{p}][SuspendAndShutdown");
        ok &= write("powerdevilrc", &group, "AutoSuspendAction", if idle.sleep > 0 { "1" } else { "0" });
        if idle.sleep > 0 {
            ok &= write("powerdevilrc", &group, "AutoSuspendIdleTimeoutSec", &to_secs(idle.sleep).to_string());
        }
    }
    reparse();
    if ok { Ok(()) } else { Err(std::io::Error::other("kwriteconfig6 failed")) }
}

/// Makes powerdevil read its settings again.
fn reparse() {
    let _ = crate::kde::call(
        "org.kde.Solid.PowerManagement",
        "/org/kde/Solid/PowerManagement",
        "org.kde.Solid.PowerManagement",
        "reparseConfiguration",
        None,
    );
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
    fn minutes_and_seconds_convert() {
        assert_eq!(to_secs(5), 300);
        assert_eq!(to_secs(0), -1);
        assert_eq!(to_minutes(300), 5);
        assert_eq!(to_minutes(-1), 0);
        assert_eq!(to_minutes(0), 0);
        for m in IDLE_CHOICES {
            assert_eq!(to_minutes(to_secs(m)), m);
        }
    }
}
