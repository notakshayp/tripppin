//! The system image: which version runs, whether an update is waiting, going back to the
//! previous one (rpm-ostree, readable without root), checking for updates (the
//! bootc-fetch-apply-updates service, which only downloads here), and About's facts.

use serde_json::Value;
use std::process::Command;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deployment {
    pub version: String,
    /// Seconds since 1970.
    pub timestamp: i64,
    pub image: String,
    pub booted: bool,
    pub staged: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub booted: Option<Deployment>,
    /// Downloaded and waiting for a restart.
    pub staged: Option<Deployment>,
    /// The one before, to go back to.
    pub previous: Option<Deployment>,
}

/// Parses `rpm-ostree status --json`.
pub fn parse_status(json: &str) -> Status {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return Status::default() };
    let all: Vec<Deployment> = v["deployments"]
        .as_array()
        .map(|d| {
            d.iter()
                .map(|d| Deployment {
                    version: d["version"].as_str().unwrap_or("").to_string(),
                    timestamp: d["timestamp"].as_i64().unwrap_or(0),
                    image: d["container-image-reference"].as_str().unwrap_or("").to_string(),
                    booted: d["booted"].as_bool().unwrap_or(false),
                    staged: d["staged"].as_bool().unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();
    Status {
        booted: all.iter().find(|d| d.booted).cloned(),
        staged: all.iter().find(|d| d.staged).cloned(),
        previous: all.iter().find(|d| !d.booted && !d.staged).cloned(),
    }
}

pub fn status() -> Status {
    Command::new("rpm-ostree")
        .args(["status", "--json"])
        .output()
        .ok()
        .map(|o| parse_status(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// "nightly" from "ostree-image-signed:docker://ghcr.io/notakshayp/tripppin:nightly".
pub fn channel(image: &str) -> String {
    if image.contains("oci-archive:") {
        return "A local test build".to_string();
    }
    image
        .rsplit_once(':')
        .map(|(_, tag)| tag.to_string())
        .filter(|t| !t.contains('/'))
        .unwrap_or_else(|| "latest".into())
}

pub fn date(timestamp: i64) -> String {
    gtk::glib::DateTime::from_unix_local(timestamp)
        .ok()
        .and_then(|t| t.format("%-d %B %Y").ok())
        .map(|s| s.to_lowercase())
        .unwrap_or_default()
}

const UPDATE_SERVICE: &str = "bootc-fetch-apply-updates.service";

/// Starts a check now; it downloads any update and stages it for the next restart.
pub fn check() -> Result<(), String> {
    systemctl(&["start", "--no-block", UPDATE_SERVICE])
}

pub fn checking() -> bool {
    Command::new("systemctl").args(["is-active", "--quiet", UPDATE_SERVICE]).status().is_ok_and(|s| s.success())
}

/// Makes the previous version the one that starts next.
pub fn roll_back() -> Result<(), String> {
    let out = Command::new("rpm-ostree").arg("rollback").output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_lowercase()) }
}

pub fn restart() -> Result<(), String> {
    systemctl(&["reboot"])
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("systemctl").args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_lowercase()) }
}

// ---- about ---------------------------------------------------------------------------------

/// A value from os-release text.
pub fn os_release_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=').map(|v| v.trim_matches('"').to_string()))
}

pub fn pretty_name() -> String {
    let text = std::fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    os_release_value(&text, "PRETTY_NAME").unwrap_or_else(|| "Tripppin".into())
}

pub fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .or_else(|| gtk::glib::host_name().split('.').next().map(str::to_string))
        .unwrap_or_default()
}

/// A hostname is letters, digits and dashes, not starting or ending with a dash.
pub fn valid_hostname(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub fn set_hostname(name: &str) -> Result<(), String> {
    let out = Command::new("hostnamectl").args(["set-hostname", name]).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_lowercase()) }
}

/// Size and free space of /var (where everything people keep lives), in bytes.
pub fn disk() -> Option<(u64, u64)> {
    let out = Command::new("df").args(["-B1", "--output=size,avail", "/var"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut nums = text.lines().nth(1)?.split_whitespace().filter_map(|n| n.parse::<u64>().ok());
    Some((nums.next()?, nums.next()?))
}

pub fn package_count() -> usize {
    Command::new("rpm").args(["-qa"]).output().map(|o| String::from_utf8_lossy(&o.stdout).lines().count()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rpm_ostree_status() {
        let json = r#"{"deployments":[
          {"booted":false,"staged":true,"version":"45.2","timestamp":2,"container-image-reference":"ostree-image-signed:docker://ghcr.io/x/tripppin:nightly"},
          {"booted":true,"staged":false,"version":"45.1","timestamp":1,"container-image-reference":"ostree-image-signed:docker://ghcr.io/x/tripppin:nightly"},
          {"booted":false,"staged":false,"version":"45.0","timestamp":0,"container-image-reference":"x"}]}"#;
        let s = parse_status(json);
        assert_eq!(s.booted.unwrap().version, "45.1");
        assert_eq!(s.staged.unwrap().version, "45.2");
        assert_eq!(s.previous.unwrap().version, "45.0");
        assert_eq!(channel("ostree-image-signed:docker://ghcr.io/x/tripppin:nightly"), "nightly");
        assert_eq!(channel("ostree-unverified-image:oci-archive:/var/tmp/t.tar"), "A local test build");
        assert_eq!(
            os_release_value("NAME=\"Tripppin\"\nPRETTY_NAME=\"Tripppin 0.1\"\n", "PRETTY_NAME").as_deref(),
            Some("Tripppin 0.1")
        );
        assert!(valid_hostname("akshays-laptop"));
        assert!(!valid_hostname("-bad") && !valid_hostname("no spaces") && !valid_hostname(""));
    }
}
