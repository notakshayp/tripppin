//! Things the system tells you about without being asked, run from the orb (which is always
//! there): the battery getting low, an update ready for the next restart, and an app that
//! quit unexpectedly (with a button to open it again).

use gtk::glib;
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::process::Command;

fn notify(app: &str, title: &str, body: &str, urgency: &str) {
    let _ = Command::new("notify-send").args(["-a", app, "-u", urgency, title, body]).spawn();
}

/// What to say when the battery reaches `percent` while discharging, given the last warning.
pub fn battery_warning(percent: u8, discharging: bool, warned_at: Option<u8>) -> Option<(u8, &'static str)> {
    if !discharging {
        return None;
    }
    let level = match percent {
        0..=5 => 5,
        6..=10 => 10,
        _ => return None,
    };
    (warned_at.is_none_or(|w| level < w))
        .then_some((level, if level == 5 { "plug in now: the computer sleeps soon" } else { "battery low: 10% left" }))
}

/// Apps that crashed after `since` (microseconds since 1970), from `coredumpctl --json=short`:
/// (pid, executable).
pub fn parse_crashes(json: &str, uid: u32, since: u64) -> Vec<(u64, String)> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    items
        .iter()
        .filter(|c| c["uid"].as_u64() == Some(u64::from(uid)) && c["time"].as_u64().unwrap_or(0) > since)
        .filter_map(|c| Some((c["pid"].as_u64()?, c["exe"].as_str()?.to_string())))
        .collect()
}

fn now_us() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_micros() as u64)
}

pub fn start() {
    let warned: Cell<Option<u8>> = Cell::new(None);
    let staged: RefCell<String> = RefCell::new(String::new());
    let since = Cell::new(now_us());
    let seen: RefCell<Vec<u64>> = RefCell::new(Vec::new());
    let uid = Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(1000);
    glib::timeout_add_seconds_local(60, move || {
        // Battery.
        if let Some(b) = crate::power::battery() {
            let discharging = b.state == "discharging";
            if !discharging {
                warned.set(None);
            } else if let Some((level, text)) = battery_warning(b.percent, true, warned.get()) {
                warned.set(Some(level));
                notify("battery", text, "", if level == 5 { "critical" } else { "normal" });
            }
        }
        // An update downloaded and waiting.
        let st = crate::system::status();
        if let Some(next) = st.staged {
            if *staged.borrow() != next.version {
                *staged.borrow_mut() = next.version.clone();
                notify("updates", "an update is ready", "it starts the next time you restart", "normal");
            }
        }
        // Crashes since we last looked.
        let out = Command::new("coredumpctl")
            .args(["list", "--json=short", "--no-pager", "--since=-10min"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        for (pid, exe) in parse_crashes(&out, uid, since.get()) {
            if seen.borrow().contains(&pid) {
                continue;
            }
            seen.borrow_mut().push(pid);
            let name = exe.rsplit('/').next().unwrap_or(&exe).to_string();
            // Wait for the person's answer off the main loop; "reopen" starts it again.
            std::thread::spawn(move || {
                let answer = Command::new("notify-send")
                    .args(["-a", "tripppin", "-A", "reopen=open it again", "--wait"])
                    .arg(format!("{name} quit unexpectedly"))
                    .arg("its work since the last save may be lost")
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();
                if answer == "reopen" {
                    let _ = Command::new(&exe).spawn();
                }
            });
        }
        since.set(now_us() - 60_000_000);
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_once_per_level() {
        assert_eq!(battery_warning(50, true, None), None);
        assert_eq!(battery_warning(9, true, None).map(|w| w.0), Some(10));
        assert_eq!(battery_warning(8, true, Some(10)), None);
        assert_eq!(battery_warning(4, true, Some(10)).map(|w| w.0), Some(5));
        assert_eq!(battery_warning(4, false, None), None);
        let json = r#"[{"time":2000,"pid":42,"uid":1000,"exe":"/usr/bin/foot","sig":11},
                      {"time":500,"pid":7,"uid":1000,"exe":"/usr/bin/old"},
                      {"time":3000,"pid":9,"uid":0,"exe":"/usr/sbin/root"}]"#;
        assert_eq!(parse_crashes(json, 1000, 1000), vec![(42, "/usr/bin/foot".to_string())]);
    }
}
