//! The built-in System widget's script: `tripppin-ui widgets system-json` prints CPU, memory,
//! GPU, temperature and the local AI model in the widget schema, so it renders like any
//! other widget (and shows the schema in use).

use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

/// Busy and total jiffies from /proc/stat's first line.
pub fn parse_stat(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().next()?;
    let nums: Vec<u64> = line.split_whitespace().skip(1).filter_map(|n| n.parse().ok()).collect();
    if nums.len() < 4 {
        return None;
    }
    let total: u64 = nums.iter().take(8).sum();
    // idle + iowait
    let idle = nums[3] + nums.get(4).copied().unwrap_or(0);
    Some((total - idle, total))
}

pub fn cpu_percent(before: (u64, u64), now: (u64, u64)) -> f64 {
    let busy = now.0.saturating_sub(before.0) as f64;
    let total = now.1.saturating_sub(before.1) as f64;
    if total <= 0.0 { 0.0 } else { (busy / total * 100.0).clamp(0.0, 100.0) }
}

/// A `Key:   123 kB` value from /proc/meminfo or /proc/<pid>/status, in kB.
pub fn kb(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':')?.split_whitespace().next()?.parse().ok())
}

/// Generated tokens per second, from llama.cpp's /metrics (when it runs with --metrics).
pub fn tokens_per_second(metrics: &str) -> Option<f64> {
    metrics.lines().find_map(|l| l.strip_prefix("llamacpp:predicted_tokens_seconds ")?.trim().parse().ok())
}

pub fn gb(kb: u64) -> String {
    let g = kb as f64 / 1024.0 / 1024.0;
    if g >= 10.0 { format!("{g:.0} GB") } else { format!("{g:.1} GB") }
}

fn read(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// A plain HTTP GET to the local model, with short timeouts: (status code, body).
fn local_get(path: &str) -> Option<(u16, String)> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], trip::model::PORT));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(300)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(800))).ok()?;
    write!(s, "GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").ok()?;
    let mut text = String::new();
    s.read_to_string(&mut text).ok()?;
    let code = text.split_whitespace().nth(1)?.parse().ok()?;
    let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
    Some((code, body))
}

/// CPU use since the last run (or over a quarter second the first time), and the last 40.
fn cpu(state_dir: &Path) -> Option<(f64, Vec<f64>)> {
    let now = parse_stat(&read("/proc/stat")?)?;
    let file = state_dir.join("widget-cpu.json");
    let saved: Value = read(&file).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let before = match (saved["busy"].as_u64(), saved["total"].as_u64()) {
        (Some(b), Some(t)) if t < now.1 => (b, t),
        _ => {
            std::thread::sleep(Duration::from_millis(250));
            let later = parse_stat(&read("/proc/stat")?)?;
            let p = cpu_percent(now, later);
            std::fs::write(&file, json!({"busy": later.0, "total": later.1, "history": [p]}).to_string()).ok();
            return Some((p, vec![p]));
        }
    };
    let p = cpu_percent(before, now);
    let mut history: Vec<f64> = saved["history"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
    history.push(p);
    if history.len() > 40 {
        history.drain(..history.len() - 40);
    }
    std::fs::write(&file, json!({"busy": now.0, "total": now.1, "history": history}).to_string()).ok();
    Some((p, history))
}

/// The hottest CPU sensor we can find, in °C.
fn temperature() -> Option<f64> {
    let preferred = ["k10temp", "coretemp", "zenpower", "cpu_thermal", "soc_thermal", "acpitz"];
    let mut found: Vec<(usize, f64)> = Vec::new();
    for e in std::fs::read_dir("/sys/class/hwmon").ok()?.flatten() {
        let name = read(e.path().join("name")).unwrap_or_default();
        let Some(rank) = preferred.iter().position(|p| *p == name.trim()) else { continue };
        if let Some(t) = read(e.path().join("temp1_input")).and_then(|t| t.trim().parse::<f64>().ok()) {
            found.push((rank, t / 1000.0));
        }
    }
    found.sort_by_key(|a| a.0);
    found.first().map(|f| f.1)
}

/// GPU busy % and video memory (used, total kB).
type Gpu = (Option<f64>, Option<(u64, u64)>);

/// The GPU's load: AMD through sysfs, NVIDIA through nvidia-smi.
fn gpu() -> Option<Gpu> {
    for e in std::fs::read_dir("/sys/class/drm").ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let dev = e.path().join("device");
        let busy = read(dev.join("gpu_busy_percent")).and_then(|t| t.trim().parse::<f64>().ok());
        let used = read(dev.join("mem_info_vram_used")).and_then(|t| t.trim().parse::<u64>().ok());
        let total = read(dev.join("mem_info_vram_total")).and_then(|t| t.trim().parse::<u64>().ok());
        let vram = used.zip(total).map(|(u, t)| (u / 1024, t / 1024));
        if busy.is_some() || vram.is_some() {
            return Some((busy, vram));
        }
    }
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=utilization.gpu,memory.used,memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let nums: Vec<f64> = text.lines().next()?.split(',').filter_map(|n| n.trim().parse().ok()).collect();
    let [busy, used, total] = nums[..] else { return None };
    Some((Some(busy), Some(((used * 1024.0) as u64, (total * 1024.0) as u64))))
}

/// The llama.cpp server's resident memory, in kB.
fn model_rss() -> Option<u64> {
    std::fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
        let comm = read(e.path().join("comm"))?;
        if comm.trim() != "llama-server" {
            return None;
        }
        kb(&read(e.path().join("status"))?, "VmRSS")
    })
}

fn llm_state() -> String {
    let out = std::process::Command::new("systemctl").args(["is-active", "tripppin-llm.service"]).output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

fn row(label: &str, value: String, bar: Option<f64>, status: &str) -> Value {
    let mut v = json!({"label": label, "value": value});
    if let Some(b) = bar {
        v["bar"] = json!((b * 100.0).round() / 100.0);
    }
    if status != "ok" {
        v["status"] = json!(status);
    }
    v
}

pub fn report() -> Value {
    let state = crate::common::runtime_dir();
    let mut items = Vec::new();
    let mut worst = "ok";
    let mut flag = |s: &'static str| {
        if s == "error" || (s == "warn" && worst == "ok") {
            worst = s;
        }
        s
    };
    if let Some((p, history)) = cpu(&state) {
        items.push(row("CPU", format!("{p:.0}%"), Some(p / 100.0), "ok"));
        if history.len() > 2 {
            items.push(json!({"spark": history.iter().map(|v| v.round()).collect::<Vec<_>>()}));
        }
    }
    if let Some(mem) = read("/proc/meminfo") {
        let total = kb(&mem, "MemTotal").unwrap_or(0);
        let avail = kb(&mem, "MemAvailable").unwrap_or(0);
        if total > 0 {
            let used = total.saturating_sub(avail);
            let f = used as f64 / total as f64;
            let s = flag(if f > 0.92 { "warn" } else { "ok" });
            items.push(row("Memory", format!("{} of {}", gb(used), gb(total)), Some(f), s));
        }
    }
    if let Some((busy, vram)) = gpu() {
        if let Some(b) = busy {
            items.push(row("GPU", format!("{b:.0}%"), Some(b / 100.0), "ok"));
        }
        if let Some((used, total)) = vram.filter(|v| v.1 > 0) {
            items.push(row(
                "Video memory",
                format!("{} of {}", gb(used), gb(total)),
                Some(used as f64 / total as f64),
                "ok",
            ));
        }
    }
    if let Some(t) = temperature() {
        let s = flag(if t >= 95.0 {
            "error"
        } else if t >= 85.0 {
            "warn"
        } else {
            "ok"
        });
        items.push(row("Temperature", format!("{t:.0} °C"), None, s));
    }
    let llm = llm_state();
    let model = match llm.as_str() {
        "active" | "reloading" => {
            let health = local_get("/health").map(|(code, _)| code);
            let busy = local_get("/slots")
                .and_then(|(_, b)| serde_json::from_str::<Value>(&b).ok())
                .and_then(|v| v.as_array().map(|a| a.iter().any(|s| s["is_processing"].as_bool() == Some(true))));
            let what = match (health, busy) {
                (Some(503), _) => "loading",
                (_, Some(true)) => "working",
                (Some(200), _) => "ready",
                _ => "starting",
            };
            match model_rss() {
                Some(rss) => format!("{what}, {}", gb(rss)),
                None => what.to_string(),
            }
        }
        "failed" => {
            flag("warn");
            "stopped with an error".into()
        }
        "activating" => "starting".into(),
        _ => "off".into(),
    };
    items.push(row("AI model", model, None, if llm == "failed" { "warn" } else { "ok" }));
    if let Some(tps) = local_get("/metrics").filter(|(c, _)| *c == 200).and_then(|(_, b)| tokens_per_second(&b)) {
        items.push(row("Speed", format!("{tps:.0} tokens/s"), None, "ok"));
    }
    json!({"title": "System", "status": worst, "items": items})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_from_proc_stat() {
        let a = parse_stat("cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 2 3 4").unwrap();
        assert_eq!(a, (200, 1000));
        let b = parse_stat("cpu  150 0 150 750 150 0 0 0 0 0").unwrap();
        assert_eq!(cpu_percent(a, b), 50.0);
        assert_eq!(cpu_percent(a, a), 0.0);
    }

    #[test]
    fn memory_and_metrics() {
        let mem = "MemTotal:       16303428 kB\nMemFree:         1234 kB\nMemAvailable:    8151714 kB\n";
        assert_eq!(kb(mem, "MemTotal"), Some(16303428));
        assert_eq!(kb(mem, "MemAvailable"), Some(8151714));
        assert_eq!(kb(mem, "Mem"), None);
        assert_eq!(gb(16303428), "16 GB");
        assert_eq!(gb(8151714), "7.8 GB");
        let metrics = "# HELP llamacpp:predicted_tokens_seconds Average generation throughput\nllamacpp:predicted_tokens_seconds 23.5\n";
        assert_eq!(tokens_per_second(metrics), Some(23.5));
    }

    #[test]
    fn the_report_fits_the_schema() {
        let shown = super::super::spec::parse_output(&report().to_string()).unwrap();
        assert_eq!(shown.title.as_deref(), Some("System"));
        assert!(!shown.items.is_empty());
    }
}
