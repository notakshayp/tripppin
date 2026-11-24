//! `tripppin-ui toggle <wifi|bluetooth|airplane|night-light> [--state]`: the switches in
//! quick settings (swaync's buttons grid). With `--state` prints `true` or `false` for the
//! tile; without, sets it from `$SWAYNC_TOGGLE_STATE`. In Rust rather than inline shell:
//! swaync runs each command inside `/bin/sh -c "…"`, so any double quote in the config
//! breaks it, and the tiles then never show as on.

use std::process::{Command, Stdio};

/// A command's output, or nothing after two seconds (`bluetoothctl` waits forever for a
/// controller on a computer that has none).
fn output(cmd: &str, args: &[&str]) -> String {
    let child = Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let Ok(mut child) = child else { return String::new() };
    let mut out = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(out) = out.as_mut() {
            let _ = std::io::Read::read_to_string(out, &mut text);
        }
        let _ = tx.send(text);
    });
    let text = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or_default();
    let _ = child.kill();
    let _ = child.wait();
    text
}

fn run(cmd: &str, args: &[&str]) {
    let _ = Command::new(cmd).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

fn notify(title: &str, body: &str, icon: &str) {
    run("notify-send", &["-a", "Quick settings", "-i", icon, title, body]);
}

/// `nmcli -t -f TYPE device` lists a wifi device.
pub fn has_wifi(devices: &str) -> bool {
    devices.lines().any(|l| l.trim() == "wifi")
}

/// `bluetoothctl show` says the adapter is powered.
pub fn bluetooth_powered(show: &str) -> bool {
    show.lines().any(|l| l.trim() == "Powered: yes")
}

fn state(what: &str) -> bool {
    match what {
        "wifi" => {
            has_wifi(&output("nmcli", &["-t", "-f", "TYPE", "device"]))
                && output("nmcli", &["radio", "wifi"]).trim() == "enabled"
        }
        "bluetooth" => bluetooth_powered(&output("bluetoothctl", &["show"])),
        "airplane" => {
            output("nmcli", &["radio", "wifi"]).trim() == "disabled"
                && output("nmcli", &["radio", "wwan"]).trim() != "enabled"
        }
        "night-light" => {
            Command::new("pgrep").args(["-x", "wlsunset"]).stdout(Stdio::null()).status().is_ok_and(|s| s.success())
        }
        _ => false,
    }
}

fn set(what: &str, on: bool) {
    let word = if on { "on" } else { "off" };
    match what {
        "wifi" => {
            if !has_wifi(&output("nmcli", &["-t", "-f", "TYPE", "device"])) {
                notify(
                    "No Wi-Fi on this computer",
                    "There's no Wi-Fi adapter, so it uses a wired connection.",
                    "network-wireless-disabled",
                );
                return;
            }
            run("nmcli", &["radio", "wifi", word]);
        }
        "bluetooth" => {
            if !output("bluetoothctl", &["list"]).contains("Controller") {
                notify("No Bluetooth on this computer", "There's no Bluetooth adapter.", "bluetooth-disabled");
                return;
            }
            run("bluetoothctl", &["power", word]);
        }
        // Airplane mode on turns every radio off.
        "airplane" => run("nmcli", &["radio", "all", if on { "off" } else { "on" }]),
        "night-light" => {
            if on {
                if !state("night-light") {
                    use std::os::unix::process::CommandExt;
                    let _ = Command::new("wlsunset")
                        .args(["-T", "4500", "-t", "3500"])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .process_group(0)
                        .spawn();
                }
            } else {
                run("pkill", &["-x", "wlsunset"]);
            }
        }
        _ => {}
    }
}

pub fn main(args: &[String]) -> gtk::glib::ExitCode {
    let what = args.first().map(String::as_str).unwrap_or("");
    if !["wifi", "bluetooth", "airplane", "night-light"].contains(&what) {
        eprintln!("usage: tripppin-ui toggle wifi|bluetooth|airplane|night-light [--state]");
        return gtk::glib::ExitCode::from(2);
    }
    if args.iter().any(|a| a == "--state") {
        println!("{}", state(what));
    } else {
        let on = std::env::var("SWAYNC_TOGGLE_STATE").is_ok_and(|v| v == "true");
        set(what, on);
    }
    gtk::glib::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nmcli_and_bluetoothctl() {
        assert!(has_wifi("ethernet\nwifi\nloopback\n"));
        assert!(!has_wifi("ethernet\nwifi-p2p\nloopback\n"));
        assert!(bluetooth_powered("Controller 00:11 (public)\n\tName: x\n\tPowered: yes\n"));
        assert!(!bluetooth_powered("Controller 00:11 (public)\n\tPowered: no\n"));
    }
}
