//! Storage: what's using the disk, cleaning up, and external drives (lsblk, udisksctl).

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Bytes under `path` (0 if it can't be read).
fn du(path: &Path) -> u64 {
    Command::new("du")
        .args(["-sb", &path.to_string_lossy()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or(0)
}

#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub total: u64,
    pub used: u64,
    /// (what, bytes), largest first; "the system" is what's left of `used`.
    pub parts: Vec<(&'static str, u64)>,
    pub trash: u64,
    pub cache: u64,
}

pub fn usage() -> Usage {
    let (total, used) = Command::new("df")
        .args(["-B1", "--output=size,used", "/var"])
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).into_owned();
            let mut n = text.lines().nth(1)?.split_whitespace().filter_map(|x| x.parse::<u64>().ok());
            Some((n.next()?, n.next()?))
        })
        .unwrap_or((0, 0));
    let h = home();
    let trash = du(&h.join(".local/share/Trash"));
    let cache = du(&h.join(".cache"));
    let mut parts = vec![
        ("your files", du(&h).saturating_sub(trash + cache)),
        ("apps", du(Path::new("/var/lib/flatpak")) + du(&h.join(".local/share/flatpak"))),
        ("ai models", du(&trip::model::models_dir())),
        ("trash and caches", trash + cache),
    ];
    let counted: u64 = parts.iter().map(|p| p.1).sum();
    parts.push(("the system and its previous version", used.saturating_sub(counted)));
    parts.sort_by_key(|p| std::cmp::Reverse(p.1));
    Usage { total, used, parts, trash, cache }
}

pub fn empty_trash() -> std::io::Result<()> {
    let t = home().join(".local/share/Trash");
    for sub in ["files", "info"] {
        let dir = t.join(sub);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
            std::fs::create_dir_all(&dir)?;
        }
    }
    Ok(())
}

/// Removes ~/.cache's contents: apps rebuild what they need.
pub fn clear_cache() -> std::io::Result<()> {
    let c = home().join(".cache");
    let Ok(entries) = std::fs::read_dir(&c) else { return Ok(()) };
    for e in entries.flatten() {
        let p = e.path();
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    }
    Ok(())
}

/// Removes Flatpak runtimes no app needs any more.
pub fn remove_unused() -> Result<(), String> {
    let out = Command::new("flatpak")
        .args(["uninstall", "--unused", "-y", "--noninteractive"])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_lowercase()) }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drive {
    /// The disk, e.g. /dev/sdb (what gets powered off).
    pub disk: String,
    pub name: String,
    pub size: u64,
    /// (partition device, where it's mounted)
    pub mounted: Vec<(String, String)>,
}

/// External drives from `lsblk -J -b -o PATH,RM,HOTPLUG,TRAN,TYPE,MOUNTPOINT,LABEL,MODEL,SIZE`.
pub fn parse_drives(json: &str) -> Vec<Drive> {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let mut drives = Vec::new();
    for d in v["blockdevices"].as_array().into_iter().flatten() {
        let external = d["rm"].as_bool() == Some(true)
            || d["hotplug"].as_bool() == Some(true)
            || d["tran"].as_str() == Some("usb");
        if !external || d["type"].as_str() != Some("disk") {
            continue;
        }
        let mut mounted = Vec::new();
        let mut label = None;
        let mut visit = |p: &Value| {
            if let Some(m) = p["mountpoint"].as_str() {
                mounted.push((p["path"].as_str().unwrap_or("").to_string(), m.to_string()));
            }
            if label.is_none() {
                label = p["label"].as_str().map(str::to_string);
            }
        };
        visit(d);
        for c in d["children"].as_array().into_iter().flatten() {
            visit(c);
        }
        let model = d["model"].as_str().unwrap_or("drive").trim().to_string();
        drives.push(Drive {
            disk: d["path"].as_str().unwrap_or("").to_string(),
            name: label.unwrap_or(model).to_lowercase(),
            size: d["size"].as_u64().unwrap_or(0),
            mounted,
        });
    }
    drives
}

pub fn drives() -> Vec<Drive> {
    Command::new("lsblk")
        .args(["-J", "-b", "-o", "PATH,RM,HOTPLUG,TRAN,TYPE,MOUNTPOINT,LABEL,MODEL,SIZE"])
        .output()
        .ok()
        .map(|o| parse_drives(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Unmounts every partition, then powers the drive off so it's safe to pull out.
pub fn eject(d: &Drive) -> Result<(), String> {
    for (dev, _) in &d.mounted {
        let out = Command::new("udisksctl").args(["unmount", "-b", dev]).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
            return Err(if err.contains("busy") {
                "something is still using it; close its files and try again".into()
            } else {
                err.trim().to_string()
            });
        }
    }
    let _ = Command::new("udisksctl").args(["power-off", "-b", &d.disk]).output();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_external_drives() {
        let json = r#"{"blockdevices":[
          {"path":"/dev/vda","rm":false,"hotplug":false,"tran":"virtio","type":"disk","mountpoint":null,"label":null,"model":null,"size":42949672960,
           "children":[{"path":"/dev/vda3","rm":false,"hotplug":false,"tran":"virtio","type":"part","mountpoint":"/sysroot","label":"root","model":null,"size":1}]},
          {"path":"/dev/sdb","rm":true,"hotplug":true,"tran":"usb","type":"disk","mountpoint":null,"label":null,"model":"SanDisk Ultra ","size":32000000000,
           "children":[{"path":"/dev/sdb1","rm":true,"hotplug":true,"tran":null,"type":"part","mountpoint":"/run/media/a/PHOTOS","label":"PHOTOS","model":null,"size":31999000000}]}]}"#;
        let d = parse_drives(json);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].disk, "/dev/sdb");
        assert_eq!(d[0].name, "photos");
        assert_eq!(d[0].mounted, vec![("/dev/sdb1".to_string(), "/run/media/a/PHOTOS".to_string())]);
    }
}
