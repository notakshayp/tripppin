//! The machine as the home screen shows it: cheap reads of /proc and /sys (battery, memory,
//! cpu, disk), and the small settings backends (Wi-Fi through iwd, sound through wpctl,
//! brightness, power profiles). Parsers are pure so they can be tested.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// Battery percent and whether it's charging, when there is a battery.
    pub battery: Option<(u8, bool)>,
    pub battery_hours: Option<f32>,
    pub cpu_percent: u8,
    pub mem_used_kib: u64,
    pub mem_total_kib: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub load: String,
}

/// Previous cpu counters, to turn two readings into a percentage.
#[derive(Clone, Copy, Debug, Default)]
pub struct CpuCounters {
    pub busy: u64,
    pub total: u64,
}

pub fn parse_cpu(stat: &str) -> Option<CpuCounters> {
    let l = stat.lines().next()?.strip_prefix("cpu")?;
    let v: Vec<u64> = l.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    if v.len() < 4 {
        return None;
    }
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    let total: u64 = v.iter().take(8).sum();
    Some(CpuCounters { busy: total - idle, total })
}

pub fn cpu_percent(prev: CpuCounters, now: CpuCounters) -> u8 {
    let d = now.total.saturating_sub(prev.total);
    (now.busy.saturating_sub(prev.busy) * 100).checked_div(d).map_or(0, |v| v.min(100) as u8)
}

/// (used, total) in KiB from /proc/meminfo; used excludes reclaimable cache.
pub fn parse_mem(info: &str) -> (u64, u64) {
    let get = |k: &str| {
        info.lines()
            .find_map(|l| {
                l.strip_prefix(k).and_then(|v| v.trim_start_matches(':').split_whitespace().next()?.parse::<u64>().ok())
            })
            .unwrap_or(0)
    };
    let total = get("MemTotal");
    (total.saturating_sub(get("MemAvailable")), total)
}

fn read(p: &str) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn battery_dir() -> Option<PathBuf> {
    fs::read_dir("/sys/class/power_supply").ok()?.flatten().map(|e| e.path()).find(|p| {
        fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Battery") && p.join("capacity").exists()
    })
}

/// Percent, charging, and hours left at the current draw.
pub fn battery() -> Option<(u8, bool, Option<f32>)> {
    let d = battery_dir()?;
    let get = |f: &str| fs::read_to_string(d.join(f)).ok().map(|s| s.trim().to_string());
    let pct: u8 = get("capacity")?.parse().ok()?;
    let status = get("status").unwrap_or_default();
    let charging = status == "Charging" || status == "Full";
    let hours = (|| {
        let now: f64 = get("energy_now").or_else(|| get("charge_now"))?.parse().ok()?;
        let draw: f64 = get("power_now").or_else(|| get("current_now"))?.parse().ok()?;
        (draw > 0.0 && !charging).then(|| (now / draw) as f32)
    })();
    Some((pct, charging, hours))
}

/// (used, total) bytes of the root filesystem, from `df`.
pub fn disk() -> (u64, u64) {
    let out = Command::new("df")
        .args(["-k", "/"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let f: Vec<&str> = out.lines().nth(1).unwrap_or("").split_whitespace().collect();
    let n = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1024;
    (n(2), n(1))
}

/// The cheap part, read every second while the home screen is showing.
pub fn read_fast(prev: &mut CpuCounters, base: &Snapshot) -> Snapshot {
    let mut s = base.clone();
    if let Some(now) = read("/proc/stat").and_then(|t| parse_cpu(&t)) {
        s.cpu_percent = cpu_percent(*prev, now);
        *prev = now;
    }
    if let Some(m) = read("/proc/meminfo") {
        (s.mem_used_kib, s.mem_total_kib) = parse_mem(&m);
    }
    if let Some(l) = read("/proc/loadavg") {
        s.load = l.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
    }
    s
}

/// The slower part (battery, disk): every 30 s.
pub fn read_slow(s: &mut Snapshot) {
    match battery() {
        Some((p, c, h)) => {
            s.battery = Some((p, c));
            s.battery_hours = h;
        }
        None => s.battery = None,
    }
    (s.disk_used, s.disk_total) = disk();
}

// ---- Wi-Fi (iwd) ------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Network {
    pub name: String,
    pub secured: bool,
    /// 1 to 4 bars.
    pub bars: u8,
    pub connected: bool,
}

/// Removes terminal colour codes (iwctl colours its tables).
pub fn strip_ansi(s: &str) -> String {
    crate::term::strip_ansi(s)
}

/// `iwctl station wlan0 get-networks` output.
pub fn parse_networks(out: &str) -> Vec<Network> {
    strip_ansi(out)
        .lines()
        .filter_map(|l| {
            let l = l.trim_end();
            let connected = l.trim_start().starts_with('>');
            let body = l.trim_start().trim_start_matches('>').trim_start();
            // "name   psk   ****" with at least two spaces between columns.
            let stars = body.rfind('*').map(|i| body[..=i].chars().rev().take_while(|c| *c == '*').count())?;
            let cols: Vec<&str> = body.split("  ").map(str::trim).filter(|c| !c.is_empty()).collect();
            if cols.len() < 3 {
                return None;
            }
            let (name, sec) = (cols[0], cols[1]);
            if name == "Network name" || name.starts_with('-') {
                return None;
            }
            Some(Network { name: name.to_string(), secured: sec != "open", bars: stars.clamp(1, 4) as u8, connected })
        })
        .collect()
}

pub fn wifi_device() -> Option<String> {
    let d = fs::read_dir("/sys/class/net").ok()?;
    d.flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| PathBuf::from(format!("/sys/class/net/{n}/wireless")).exists())
}

pub fn wifi_networks() -> Vec<Network> {
    let Some(dev) = wifi_device() else { return Vec::new() };
    let _ = Command::new("iwctl").args(["station", &dev, "scan"]).output();
    Command::new("iwctl")
        .args(["station", &dev, "get-networks"])
        .output()
        .map(|o| parse_networks(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// The connected network's name and signal bars, from `iwctl station dev show`.
pub fn parse_wifi_status(out: &str) -> Option<String> {
    strip_ansi(out).lines().find_map(|l| {
        let l = l.trim();
        l.strip_prefix("Connected network").map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
    })
}

pub fn wifi_status() -> Option<String> {
    let dev = wifi_device()?;
    let out = Command::new("iwctl").args(["station", &dev, "show"]).output().ok()?;
    parse_wifi_status(&String::from_utf8_lossy(&out.stdout))
}

pub fn wifi_connect(ssid: &str, password: Option<&str>) -> Result<(), String> {
    let dev = wifi_device().ok_or("no Wi-Fi adapter found")?;
    let mut c = Command::new("iwctl");
    if let Some(p) = password.filter(|p| !p.is_empty()) {
        c.args(["--passphrase", p]);
    }
    let out = c.args(["station", &dev, "connect", ssid]).output().map_err(|e| format!("iwctl didn't start: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(strip_ansi(&String::from_utf8_lossy(&out.stdout)).trim().to_string())
    }
}

pub fn wifi_forget(ssid: &str) -> bool {
    Command::new("iwctl").args(["known-networks", ssid, "forget"]).status().is_ok_and(|s| s.success())
}

// ---- sound ------------------------------------------------------------------------------

/// "Volume: 0.60 [MUTED]" → (60, muted).
pub fn parse_volume(out: &str) -> Option<(u8, bool)> {
    let v: f32 = out.split_whitespace().nth(1)?.parse().ok()?;
    Some(((v * 100.0).round().clamp(0.0, 150.0) as u8, out.contains("MUTED")))
}

pub fn volume() -> Option<(u8, bool)> {
    let o = Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    parse_volume(&String::from_utf8_lossy(&o.stdout))
}

pub fn set_volume(percent: u8) -> bool {
    Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", f32::from(percent.min(100)) / 100.0)])
        .status()
        .is_ok_and(|s| s.success())
}

pub fn set_mute(mute: bool) -> bool {
    Command::new("wpctl")
        .args(["set-mute", "@DEFAULT_AUDIO_SINK@", if mute { "1" } else { "0" }])
        .status()
        .is_ok_and(|s| s.success())
}

// ---- display ----------------------------------------------------------------------------

fn backlight() -> Option<PathBuf> {
    fs::read_dir("/sys/class/backlight").ok()?.flatten().map(|e| e.path()).next()
}

/// Brightness in percent.
pub fn brightness() -> Option<u8> {
    let d = backlight()?;
    let cur: f64 = fs::read_to_string(d.join("brightness")).ok()?.trim().parse().ok()?;
    let max: f64 = fs::read_to_string(d.join("max_brightness")).ok()?.trim().parse().ok()?;
    (max > 0.0).then(|| (cur * 100.0 / max).round() as u8)
}

pub fn set_brightness(percent: u8) -> bool {
    let p = percent.clamp(1, 100);
    if Command::new("brightnessctl").args(["set", &format!("{p}%")]).output().is_ok_and(|o| o.status.success()) {
        return true;
    }
    let Some(d) = backlight() else { return false };
    let max: u64 = fs::read_to_string(d.join("max_brightness")).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    max > 0 && fs::write(d.join("brightness"), (max * u64::from(p) / 100).to_string()).is_ok()
}

// ---- power ------------------------------------------------------------------------------

pub fn power_profile() -> Option<String> {
    let o = Command::new("powerprofilesctl").arg("get").output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

pub fn set_power_profile(p: &str) -> bool {
    Command::new("powerprofilesctl").args(["set", p]).status().is_ok_and(|s| s.success())
}

// ---- keyboard ---------------------------------------------------------------------------

/// The keyboard layout the compositor was started with (XKB_DEFAULT_LAYOUT), else "us".
pub fn keyboard_layout() -> String {
    std::env::var("XKB_DEFAULT_LAYOUT").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "us".into())
}

// ---- playing ----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Playing {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub position: f64,
    pub length: f64,
}

/// `playerctl metadata --format` with tab-separated fields: status, title, artist, position (s), length (s).
pub fn parse_playing(out: &str) -> Option<Playing> {
    let f: Vec<&str> = out.trim_end_matches('\n').split('\t').collect();
    if f.len() < 5 || f[0].is_empty() {
        return None;
    }
    let micros = |s: &str| s.trim().parse::<f64>().unwrap_or(0.0) / 1e6;
    Some(Playing {
        playing: f[0] == "Playing",
        title: f[1].into(),
        artist: f[2].into(),
        position: micros(f[3]),
        length: micros(f[4]),
    })
}

pub fn playing() -> Option<Playing> {
    let o = Command::new("playerctl")
        .args(["metadata", "--format", "{{status}}\t{{title}}\t{{artist}}\t{{position}}\t{{mpris:length}}"])
        .output()
        .ok()?;
    o.status.success().then(|| parse_playing(&String::from_utf8_lossy(&o.stdout))).flatten()
}

pub fn player(cmd: &str) {
    let _ = Command::new("playerctl").arg(cmd).output();
}

/// "2:14" from seconds.
pub fn mmss(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn hostname() -> String {
    read("/proc/sys/kernel/hostname").or_else(|| read("/etc/hostname")).unwrap_or_else(|| "tripppin".into())
}

pub fn user() -> String {
    std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_else(|_| "you".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_percent_from_two_readings() {
        let a = parse_cpu("cpu  100 0 100 800 0 0 0 0\ncpu0 1 2 3 4").unwrap();
        let b = parse_cpu("cpu  150 0 150 900 0 0 0 0").unwrap();
        assert_eq!(cpu_percent(a, b), 50);
        assert_eq!(cpu_percent(a, a), 0);
    }

    #[test]
    fn memory_uses_available() {
        let (used, total) = parse_mem("MemTotal:  8000000 kB\nMemFree: 100 kB\nMemAvailable: 5000000 kB\n");
        assert_eq!((used, total), (3000000, 8000000));
    }

    #[test]
    fn parses_iwctl_networks() {
        let out = "\x1b[1;90m                              Available networks\x1b[0m\n\
--------------------------------------------------------------------------------\n\
\x1b[1;90m    Network name                    Security            Signal\x1b[0m\n\
--------------------------------------------------------------------------------\n\
\x1b[1;90m  > \x1b[0mhome-5g                          psk                 ****\n\
    cafe-guest                      open                **  \n\
    office                          psk                 ***\n";
        let n = parse_networks(out);
        assert_eq!(n.len(), 3);
        assert_eq!(n[0], Network { name: "home-5g".into(), secured: true, bars: 4, connected: true });
        assert_eq!(n[1], Network { name: "cafe-guest".into(), secured: false, bars: 2, connected: false });
        assert_eq!(n[2].bars, 3);
    }

    #[test]
    fn parses_status_volume_and_player() {
        assert_eq!(
            parse_wifi_status("  State   connected\n  Connected network   home-5g\n").as_deref(),
            Some("home-5g")
        );
        assert_eq!(parse_wifi_status("State disconnected"), None);
        assert_eq!(parse_volume("Volume: 0.60"), Some((60, false)));
        assert_eq!(parse_volume("Volume: 0.35 [MUTED]"), Some((35, true)));
        let p = parse_playing("Playing\tslow morning\tlo-fi mix\t134000000\t220000000\n").unwrap();
        assert!(p.playing);
        assert_eq!((mmss(p.position), mmss(p.length)), ("2:14".to_string(), "3:40".to_string()));
        assert!(parse_playing("").is_none());
    }
}
