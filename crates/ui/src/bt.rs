//! Bluetooth through BlueZ's `bluetoothctl`: the adapter, paired and nearby devices,
//! pairing, connecting and removing (devices that show a confirmation code pair on their own).

use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub address: String,
    pub name: String,
    pub connected: bool,
    pub battery: Option<u8>,
    /// BlueZ's icon name: audio-headphones, input-mouse, phone…
    pub kind: String,
}

/// `bluetoothctl devices …` prints "Device AA:BB:CC:DD:EE:FF Name with spaces".
pub fn parse_devices(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("Device ")?;
            let (addr, name) = rest.split_once(' ').unwrap_or((rest, ""));
            (addr.len() == 17)
                .then(|| (addr.to_string(), if name.is_empty() { addr.to_string() } else { name.to_string() }))
        })
        .collect()
}

/// From `bluetoothctl info <addr>`: whether it's connected, its battery and its icon.
pub fn parse_info(out: &str) -> (bool, Option<u8>, String) {
    let mut connected = false;
    let mut battery = None;
    let mut icon = String::new();
    for line in out.lines().map(str::trim) {
        if let Some(v) = line.strip_prefix("Connected:") {
            connected = v.trim() == "yes";
        } else if let Some(v) = line.strip_prefix("Battery Percentage:") {
            // "0x48 (72)"
            battery = v.split('(').nth(1).and_then(|s| s.trim_end_matches(')').trim().parse().ok());
        } else if let Some(v) = line.strip_prefix("Icon:") {
            icon = v.trim().to_string();
        }
    }
    (connected, battery, icon)
}

/// A short word for what kind of device it is.
pub fn kind_word(icon: &str) -> &'static str {
    match icon {
        i if i.starts_with("audio-headphones") || i.starts_with("audio-headset") => "headphones",
        i if i.starts_with("audio") => "speaker",
        "input-mouse" => "mouse",
        "input-keyboard" => "keyboard",
        "input-gaming" => "controller",
        i if i.starts_with("phone") => "phone",
        i if i.starts_with("computer") => "computer",
        _ => "device",
    }
}

fn ctl(args: &[&str]) -> Result<String, String> {
    let out =
        Command::new("bluetoothctl").args(args).output().map_err(|e| format!("Couldn't run bluetoothctl: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() && !text.contains("Failed") && !text.contains("not available") {
        Ok(text)
    } else {
        let line = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("Bluetooth failed");
        Err(line.trim().to_lowercase())
    }
}

pub fn has_adapter() -> bool {
    ctl(&["list"]).is_ok_and(|s| s.lines().any(|l| l.starts_with("Controller")))
}

/// Whether the adapter is on, and the name other devices see.
pub fn status() -> (bool, String) {
    let out = ctl(&["show"]).unwrap_or_default();
    let on = out.lines().any(|l| l.trim() == "Powered: yes");
    let name = out.lines().find_map(|l| l.trim().strip_prefix("Alias: ").map(str::to_string)).unwrap_or_default();
    (on, name)
}

pub fn set_power(on: bool) -> Result<(), String> {
    ctl(&["power", if on { "on" } else { "off" }]).map(|_| ())
}

/// The devices this computer has paired with.
pub fn paired() -> Vec<Device> {
    let list = parse_devices(&ctl(&["devices", "Paired"]).unwrap_or_default());
    list.into_iter()
        .map(|(address, name)| {
            let (connected, battery, icon) = parse_info(&ctl(&["info", &address]).unwrap_or_default());
            Device { address, name, connected, battery, kind: icon }
        })
        .collect()
}

/// Looks for nearby devices for a few seconds; returns those not yet paired.
pub fn scan(seconds: u32) -> Vec<Device> {
    let _ = ctl(&["--timeout", &seconds.to_string(), "scan", "on"]);
    let paired: Vec<String> =
        parse_devices(&ctl(&["devices", "Paired"]).unwrap_or_default()).into_iter().map(|d| d.0).collect();
    parse_devices(&ctl(&["devices"]).unwrap_or_default())
        .into_iter()
        .filter(|(addr, name)| !paired.contains(addr) && name != addr)
        .map(|(address, name)| Device { address, name, connected: false, battery: None, kind: String::new() })
        .collect()
}

/// Pairs, trusts (so it reconnects by itself) and connects.
pub fn pair(address: &str) -> Result<(), String> {
    ctl(&["--timeout", "20", "pair", address])?;
    ctl(&["trust", address])?;
    ctl(&["--timeout", "15", "connect", address]).map(|_| ())
}

pub fn connect(address: &str) -> Result<(), String> {
    ctl(&["--timeout", "15", "connect", address]).map(|_| ())
}

pub fn disconnect(address: &str) -> Result<(), String> {
    ctl(&["disconnect", address]).map(|_| ())
}

pub fn remove(address: &str) -> Result<(), String> {
    ctl(&["remove", address]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_devices_and_info() {
        let devices = "Device 11:22:33:44:55:66 Sony WH-1000XM5\nDevice AA:BB:CC:DD:EE:FF\nnoise\n";
        assert_eq!(
            parse_devices(devices),
            vec![
                ("11:22:33:44:55:66".to_string(), "Sony WH-1000XM5".to_string()),
                ("AA:BB:CC:DD:EE:FF".to_string(), "AA:BB:CC:DD:EE:FF".to_string())
            ]
        );
        let info =
            "Device 11:22\n\tName: Sony\n\tIcon: audio-headphones\n\tConnected: yes\n\tBattery Percentage: 0x48 (72)\n";
        assert_eq!(parse_info(info), (true, Some(72), "audio-headphones".to_string()));
        assert_eq!(kind_word("audio-headphones"), "headphones");
        assert_eq!(kind_word("input-mouse"), "mouse");
        assert_eq!(kind_word("weird"), "device");
    }
}
