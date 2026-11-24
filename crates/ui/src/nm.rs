//! NetworkManager through `nmcli`: Wi-Fi networks, connecting, forgetting, the radio.
//! nmcli's terse mode (`-t`) separates fields with `:` and escapes `:` and `\` inside them.

use std::process::Command;

/// One Wi-Fi network in range.
#[derive(Clone, Debug, PartialEq)]
pub struct Network {
    pub ssid: String,
    pub signal: u8,
    pub secured: bool,
    pub in_use: bool,
    pub band: &'static str,
}

/// Splits one line of `nmcli -t` output into its fields, undoing the escapes.
pub fn split_terse(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    fields.last_mut().expect("one field").push(next);
                }
            }
            ':' => fields.push(String::new()),
            _ => fields.last_mut().expect("one field").push(c),
        }
    }
    fields
}

/// Parses `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY,FREQ dev wifi list`: one entry per network
/// name (the strongest access point), the connected one first, then by signal.
pub fn parse_networks(out: &str) -> Vec<Network> {
    let mut nets: Vec<Network> = Vec::new();
    for line in out.lines() {
        let f = split_terse(line);
        if f.len() < 5 || f[1].is_empty() {
            continue;
        }
        let mhz: u32 = f[4].split_whitespace().next().and_then(|m| m.parse().ok()).unwrap_or(0);
        let net = Network {
            ssid: f[1].clone(),
            signal: f[2].parse().unwrap_or(0),
            secured: !f[3].trim().is_empty() && f[3].trim() != "--",
            in_use: f[0].trim() == "*",
            band: if mhz >= 5900 {
                "6 ghz"
            } else if mhz >= 4900 {
                "5 ghz"
            } else {
                "2.4 ghz"
            },
        };
        match nets.iter_mut().find(|n| n.ssid == net.ssid) {
            Some(existing) => {
                if net.in_use || (!existing.in_use && net.signal > existing.signal) {
                    *existing = net;
                }
            }
            None => nets.push(net),
        }
    }
    nets.sort_by(|a, b| b.in_use.cmp(&a.in_use).then(b.signal.cmp(&a.signal)));
    nets
}

/// "strong", "good", "weak" for a signal percentage.
pub fn strength(signal: u8) -> &'static str {
    match signal {
        70.. => "strong",
        40..=69 => "good",
        _ => "weak",
    }
}

fn nmcli(args: &[&str]) -> Result<String, String> {
    let out = Command::new("nmcli").args(args).output().map_err(|e| format!("Couldn't run nmcli: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim().trim_start_matches("Error: ");
        Err(if err.is_empty() { "Nmcli failed".to_string() } else { err.to_lowercase() })
    }
}

/// Whether this computer has a Wi-Fi card at all.
pub fn has_wifi() -> bool {
    nmcli(&["-t", "-f", "TYPE", "dev"]).is_ok_and(|s| s.lines().any(|t| t.trim() == "wifi"))
}

/// Whether a cable (Ethernet) connection is up.
pub fn wired() -> bool {
    nmcli(&["-t", "-f", "TYPE,STATE", "dev"]).is_ok_and(|s| s.lines().any(|l| l.starts_with("ethernet:connected")))
}

pub fn wifi_enabled() -> bool {
    nmcli(&["radio", "wifi"]).is_ok_and(|s| s.trim() == "enabled")
}

pub fn set_wifi(on: bool) -> Result<(), String> {
    nmcli(&["radio", "wifi", if on { "on" } else { "off" }]).map(|_| ())
}

/// Networks in range; `rescan` asks the card to look again (slower).
pub fn networks(rescan: bool) -> Vec<Network> {
    let rescan = if rescan { "yes" } else { "auto" };
    nmcli(&["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY,FREQ", "dev", "wifi", "list", "--rescan", rescan])
        .map(|out| parse_networks(&out))
        .unwrap_or_default()
}

/// Connects to `ssid`, with a password for secured networks. Saved networks need none.
pub fn connect(ssid: &str, password: Option<&str>) -> Result<(), String> {
    match password {
        Some(pw) if !pw.is_empty() => nmcli(&["dev", "wifi", "connect", ssid, "password", pw]).map(|_| ()),
        _ => nmcli(&["dev", "wifi", "connect", ssid]).map(|_| ()),
    }
}

/// Connects to a network that doesn't broadcast its name.
pub fn connect_hidden(ssid: &str, password: &str) -> Result<(), String> {
    let mut args = vec!["dev", "wifi", "connect", ssid, "hidden", "yes"];
    if !password.is_empty() {
        args.extend(["password", password]);
    }
    nmcli(&args).map(|_| ())
}

pub fn disconnect(ssid: &str) -> Result<(), String> {
    nmcli(&["connection", "down", "id", ssid]).map(|_| ())
}

pub fn forget(ssid: &str) -> Result<(), String> {
    nmcli(&["connection", "delete", "id", ssid]).map(|_| ())
}

/// Names of the Wi-Fi networks this computer remembers.
pub fn saved() -> Vec<String> {
    nmcli(&["-t", "-f", "NAME,TYPE", "connection", "show"])
        .map(|out| {
            out.lines()
                .map(split_terse)
                .filter(|f| f.get(1).is_some_and(|t| t == "802-11-wireless"))
                .map(|f| f[0].clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The Wi-Fi card's IPv4 address, if it has one.
pub fn wifi_address() -> Option<String> {
    let devices = nmcli(&["-t", "-f", "DEVICE,TYPE", "dev"]).ok()?;
    let dev = devices.lines().map(split_terse).find(|f| f.get(1).is_some_and(|t| t == "wifi"))?[0].clone();
    let out = nmcli(&["-t", "-f", "IP4.ADDRESS", "dev", "show", &dev]).ok()?;
    out.lines().find_map(|l| l.split_once(':').map(|(_, v)| v.split('/').next().unwrap_or(v).to_string()))
}

/// Shares this computer's connection as a Wi-Fi hotspot; returns its name and password.
pub fn hotspot() -> Result<(String, String), String> {
    nmcli(&["dev", "wifi", "hotspot"])?;
    let show = nmcli(&[
        "-s",
        "-t",
        "-f",
        "802-11-wireless.ssid,802-11-wireless-security.psk",
        "connection",
        "show",
        "Hotspot",
    ])?;
    let mut ssid = String::new();
    let mut psk = String::new();
    for line in show.lines() {
        if let Some((k, v)) = line.split_once(':') {
            match k {
                "802-11-wireless.ssid" => ssid = v.to_string(),
                "802-11-wireless-security.psk" => psk = v.to_string(),
                _ => {}
            }
        }
    }
    Ok((ssid, psk))
}

/// A connected device and its addresses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Link {
    pub device: String,
    /// ethernet, wifi, wireguard, vpn…
    pub kind: String,
    pub connection: String,
    pub hardware: String,
    pub ipv4: Vec<String>,
    pub gateway: String,
    pub dns: Vec<String>,
    pub ipv6: Vec<String>,
}

/// Parses `nmcli -t -f GENERAL.HWADDR,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,IP6.ADDRESS device show <dev>`.
pub fn parse_details(out: &str, link: &mut Link) {
    for line in out.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.replace("\\:", ":");
        let key = key.split('[').next().unwrap_or(key);
        match key {
            "GENERAL.HWADDR" => link.hardware = value.to_lowercase(),
            "IP4.ADDRESS" => link.ipv4.push(value),
            "IP4.GATEWAY" => link.gateway = value,
            "IP4.DNS" => link.dns.push(value),
            // Link-local addresses (fe80::) aren't worth showing.
            "IP6.ADDRESS" if !value.starts_with("fe80") => link.ipv6.push(value),
            _ => {}
        }
    }
}

/// The devices that are connected, with their addresses (not the loopback).
pub fn links() -> Vec<Link> {
    let list = nmcli(&["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"]).unwrap_or_default();
    list.lines()
        .map(split_terse)
        .filter(|f| f.len() >= 4 && f[2] == "connected" && f[1] != "loopback")
        .map(|f| {
            let mut link =
                Link { device: f[0].clone(), kind: f[1].clone(), connection: f[3].clone(), ..Link::default() };
            let fields = "GENERAL.HWADDR,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,IP6.ADDRESS";
            parse_details(&nmcli(&["-t", "-f", fields, "device", "show", &f[0]]).unwrap_or_default(), &mut link);
            link
        })
        .collect()
}

/// Saved VPNs: (name, type, active).
pub fn parse_vpns(out: &str) -> Vec<(String, String, bool)> {
    out.lines()
        .map(split_terse)
        .filter(|f| f.len() >= 3 && (f[1] == "vpn" || f[1] == "wireguard"))
        .map(|f| (f[0].clone(), f[1].clone(), f[2] == "yes"))
        .collect()
}

pub fn vpns() -> Vec<(String, String, bool)> {
    parse_vpns(&nmcli(&["-t", "-f", "NAME,TYPE,ACTIVE", "connection", "show"]).unwrap_or_default())
}

pub fn vpn_up(name: &str, up: bool) -> Result<(), String> {
    nmcli(&["connection", if up { "up" } else { "down" }, "id", name]).map(|_| ())
}

/// Imports a WireGuard .conf or an OpenVPN .ovpn file as a new VPN.
pub fn import_vpn(path: &str) -> Result<(), String> {
    let kind = if path.ends_with(".conf") { "wireguard" } else { "openvpn" };
    nmcli(&["connection", "import", "type", kind, "file", path]).map(|_| ())
}

pub fn remove_connection(name: &str) -> Result<(), String> {
    nmcli(&["connection", "delete", "id", name]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_details_and_vpns() {
        let out = "GENERAL.HWADDR:86\\:E4\\:1C\\:22\\:F4\\:CB\nIP4.ADDRESS[1]:192.168.64.3/24\nIP4.GATEWAY:192.168.64.1\nIP4.DNS[1]:192.168.64.1\nIP6.ADDRESS[1]:fd94\\:a55\\:\\:1/64\nIP6.ADDRESS[2]:fe80\\:\\:1/64\n";
        let mut l = Link::default();
        parse_details(out, &mut l);
        assert_eq!(l.hardware, "86:e4:1c:22:f4:cb");
        assert_eq!(l.ipv4, vec!["192.168.64.3/24"]);
        assert_eq!(l.gateway, "192.168.64.1");
        assert_eq!(l.dns, vec!["192.168.64.1"]);
        assert_eq!(l.ipv6, vec!["fd94:a55::1/64"]);
        let vpns = parse_vpns("Wired connection 1:802-3-ethernet:yes\nwork:vpn:no\nhome wg:wireguard:yes\n");
        assert_eq!(vpns, vec![("work".into(), "vpn".into(), false), ("home wg".into(), "wireguard".into(), true)]);
    }

    #[test]
    fn splits_escaped_fields() {
        assert_eq!(split_terse(r"*:my\:net:80:WPA2:5180 MHz"), vec!["*", "my:net", "80", "WPA2", "5180 MHz"]);
        assert_eq!(split_terse(r"a\\b:c"), vec![r"a\b", "c"]);
    }

    #[test]
    fn lists_each_network_once_connected_first() {
        let out = " :cafe guest:40::2437 MHz\n\
                   *:akshay home:72:WPA2:5180 MHz\n \
                   :jiofiber-5g:65:WPA2 WPA3:5745 MHz\n \
                   :jiofiber-5g:30:WPA2 WPA3:2412 MHz\n \
                   :::--:2412 MHz\n";
        let nets = parse_networks(out);
        assert_eq!(nets.len(), 3);
        assert_eq!(
            nets[0],
            Network { ssid: "akshay home".into(), signal: 72, secured: true, in_use: true, band: "5 ghz" }
        );
        assert_eq!(nets[1].ssid, "jiofiber-5g");
        assert_eq!(nets[1].signal, 65);
        assert!(!nets[2].secured);
        assert_eq!(strength(72), "strong");
        assert_eq!(strength(45), "good");
        assert_eq!(strength(10), "weak");
    }
}
