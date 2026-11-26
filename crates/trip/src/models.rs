//! Several local models side by side: which catalog models fit this computer, which are
//! downloaded, which one runs, and downloading (with progress and cancel), switching and
//! deleting them. Settings → AI uses this; `model.rs` keeps serving and talking to the model.
//!
//! Models live in the models folder (`model::models_dir`), which administrators can write to.
//! The one in use is named in its `chosen` file; `tripppin-llm` reads it when it (re)starts,
//! and administrators may restart that one service without a password (polkit).

use crate::model;
use anyhow::{Context, Result, bail};
use hwdetect::{Catalog, Hardware, Tier};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const SERVICE: &str = "tripppin-llm.service";

/// One catalog model as this computer sees it.
#[derive(Clone, Debug)]
pub struct LocalModel {
    pub tier: Tier,
    /// Fully downloaded (right size in the models folder).
    pub installed: bool,
    /// Bytes of an unfinished download, which a new download resumes.
    pub partial: Option<u64>,
    /// This computer meets the model's memory or graphics-memory need.
    pub fits: bool,
    /// The one picked for this computer's hardware.
    pub recommended: bool,
    /// The one `tripppin-llm` runs.
    pub active: bool,
}

impl LocalModel {
    /// What it needs, in words: "Needs 16 GB of memory" or "Needs an 8 GB graphics card".
    pub fn needs(&self) -> String {
        need_words(&self.tier)
    }
}

/// Every catalog model, with what this computer makes of it; the recommended one first, then
/// the other tiers, then the other choices.
pub fn list() -> Result<Vec<LocalModel>> {
    let catalog = Catalog::builtin()?;
    let hw = hwdetect::detect().ok();
    Ok(list_in(&catalog, hw.as_ref(), &model::models_dir()))
}

pub fn list_in(catalog: &Catalog, hw: Option<&Hardware>, dir: &Path) -> Vec<LocalModel> {
    let recommended = hw.and_then(|hw| catalog.recommend(hw)).map(|t| t.id.clone());
    let active = model::active_tier(catalog, dir).map(|t| t.id.clone());
    let mut out: Vec<LocalModel> = catalog
        .tiers
        .iter()
        .map(|t| {
            let installed = installed(dir, t);
            let partial = (!installed)
                .then(|| fs::metadata(part_path(dir, t)).ok().map(|m| m.len()))
                .flatten()
                .filter(|n| *n > 0);
            LocalModel {
                tier: t.clone(),
                installed,
                partial,
                fits: hw.is_none_or(|hw| fits(t, hw)),
                recommended: recommended.as_deref() == Some(t.id.as_str()),
                active: installed && active.as_deref() == Some(t.id.as_str()),
            }
        })
        .collect();
    out.sort_by_key(|m| (!m.recommended, m.tier.choice));
    out
}

/// Whether `hw` meets the tier's memory and graphics-memory needs.
pub fn fits(t: &Tier, hw: &Hardware) -> bool {
    t.min_vram_gib.is_none_or(|need| hw.max_vram_gib() >= need)
        && t.min_ram_gib.is_none_or(|need| hw.memory_gib() >= need)
}

/// The size people know a machine by for a threshold in GiB: 14.5 → 16, 30 → 32, 9.5 → 10.
fn marketing_gb(need_gib: f64) -> u32 {
    const SIZES: [u32; 14] = [4, 6, 8, 10, 12, 16, 20, 24, 32, 48, 64, 96, 128, 256];
    SIZES.into_iter().find(|s| f64::from(*s) >= need_gib).unwrap_or(need_gib.ceil() as u32)
}

fn need_words(t: &Tier) -> String {
    match (t.min_vram_gib, t.min_ram_gib) {
        (Some(v), _) => format!("Needs a {} GB graphics card", marketing_gb(v)),
        (None, Some(r)) => format!("Needs {} GB of memory", marketing_gb(r)),
        (None, None) => "Runs on any computer".to_string(),
    }
}

fn installed(dir: &Path, t: &Tier) -> bool {
    fs::metadata(dir.join(&t.file)).is_ok_and(|m| m.len() == t.size_bytes)
}

fn part_path(dir: &Path, t: &Tier) -> PathBuf {
    dir.join(format!("{}.part", t.file))
}

/// Free bytes where the models go, when `df` can tell.
pub fn free_space() -> Option<u64> {
    let dir = model::models_dir();
    let probe = if dir.exists() { dir } else { dir.parent().map(Path::to_path_buf).unwrap_or_default() };
    let out = std::process::Command::new("df").args(["-B1", "--output=avail"]).arg(probe).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().nth(1)?.trim().parse().ok()
}

/// Bytes the downloaded models (and unfinished downloads) take.
pub fn used_space() -> u64 {
    fs::read_dir(model::models_dir())
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name().to_string_lossy().contains(".gguf"))
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// How a download is going.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Progress {
    /// Bytes done, bytes in all, bytes per second.
    Downloading(u64, u64, f64),
    /// Checking the finished file against its checksum.
    Checking,
}

/// Why a download stopped early.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the download was cancelled")
    }
}

impl std::error::Error for Cancelled {}

fn not_allowed(e: std::io::Error, dir: &Path) -> anyhow::Error {
    if e.kind() == ErrorKind::PermissionDenied {
        anyhow::anyhow!("Only an administrator can add or remove models (in {}).", dir.display())
    } else {
        anyhow::Error::new(e).context(format!("can't write to {}", dir.display()))
    }
}

/// Downloads a model, resuming an unfinished download, and checks its checksum. Setting
/// `cancel` stops it and keeps what was downloaded, so the next download resumes; the error is
/// then `Cancelled`.
pub fn download(t: &Tier, cancel: &AtomicBool, on_progress: &mut dyn FnMut(Progress)) -> Result<()> {
    let dir = model::models_dir();
    fs::create_dir_all(&dir).map_err(|e| not_allowed(e, &dir))?;
    if installed(&dir, t) {
        return Ok(());
    }
    let part = part_path(&dir, t);
    let have = fs::metadata(&part).map_or(0, |m| m.len());
    if let Some(free) = free_space() {
        let need = t.size_bytes.saturating_sub(have);
        // Leave a little room for everything else.
        if free < need + 1_000_000_000 {
            bail!(
                "Not enough space: it needs {:.1} GB free and there's {:.1} GB. Delete a model or other files first.",
                need as f64 / 1e9,
                free as f64 / 1e9
            );
        }
    }
    fetch(&t.url, &part, have, t.size_bytes, cancel, on_progress)?;
    on_progress(Progress::Checking);
    let actual = sha256_file(&part, cancel)?;
    if actual != t.sha256 {
        fs::remove_file(&part).ok();
        bail!("The download was damaged (its checksum didn't match), so it was deleted. Try again.");
    }
    fs::rename(&part, dir.join(&t.file)).map_err(|e| not_allowed(e, &dir))?;
    // Readable by the model service.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.join(&t.file), fs::Permissions::from_mode(0o664)).ok();
    }
    Ok(())
}

fn fetch(
    url: &str,
    part: &Path,
    have: u64,
    total: u64,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<()> {
    if have >= total {
        return Ok(());
    }
    let mut req = model::http_agent().get(url);
    if have > 0 {
        req = req.header("Range", &format!("bytes={have}-"));
    }
    let mut resp = req.call().context("Couldn't reach the download server. Check that this computer is online")?;
    // A server that ignores Range sends the whole file again; start over in that case.
    let resumed = resp.status().as_u16() == 206;
    let dir = part.parent().unwrap_or(Path::new("."));
    let mut file = if resumed { OpenOptions::new().append(true).open(part) } else { File::create(part) }
        .map_err(|e| not_allowed(e, dir))?;
    let mut done = if resumed { have } else { 0 };
    let start_bytes = done;
    let mut reader = resp.body_mut().as_reader();
    let mut buf = vec![0u8; 1 << 20];
    let (start, mut last) = (Instant::now(), Instant::now() - Duration::from_secs(1));
    loop {
        if cancel.load(Ordering::Relaxed) {
            file.sync_all().ok();
            return Err(Cancelled.into());
        }
        let n = reader.read(&mut buf).context("The download broke off; download again to resume")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if last.elapsed() >= Duration::from_millis(250) {
            last = Instant::now();
            let rate = (done - start_bytes) as f64 / start.elapsed().as_secs_f64().max(0.001);
            on_progress(Progress::Downloading(done, total, rate));
        }
    }
    file.sync_all()?;
    on_progress(Progress::Downloading(done, total, 0.0));
    if done != total {
        bail!(
            "The download stopped at {:.1} of {:.1} GB; download again to resume.",
            done as f64 / 1e9,
            total as f64 / 1e9
        );
    }
    Ok(())
}

fn sha256_file(path: &Path, cancel: &AtomicBool) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Makes `t` the model in use and restarts the model service so it loads it.
pub fn activate(t: &Tier) -> Result<()> {
    let dir = model::models_dir();
    if !installed(&dir, t) {
        bail!("{} isn't downloaded yet.", t.model);
    }
    write_chosen(&dir, &t.id)?;
    restart_service()
}

/// Writes the `chosen` file by replacing it, so any administrator can change it.
fn write_chosen(dir: &Path, id: &str) -> Result<()> {
    let tmp = dir.join(".chosen.new");
    fs::write(&tmp, id).map_err(|e| not_allowed(e, dir))?;
    fs::rename(&tmp, dir.join("chosen")).map_err(|e| not_allowed(e, dir))?;
    Ok(())
}

/// Deletes a model's file and any unfinished download of it. When it was the one in use, the
/// service moves to another downloaded model, or stops when none is left.
pub fn delete(t: &Tier) -> Result<()> {
    let catalog = Catalog::builtin()?;
    let dir = model::models_dir();
    let was_active = model::active_tier(&catalog, &dir).is_some_and(|a| a.id == t.id);
    for path in [dir.join(&t.file), part_path(&dir, t)] {
        match fs::remove_file(&path) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(not_allowed(e, &dir)),
            _ => {}
        }
    }
    if !was_active {
        return Ok(());
    }
    match model::active_tier(&catalog, &dir) {
        Some(next) => {
            write_chosen(&dir, &next.id)?;
            restart_service()
        }
        None => {
            fs::remove_file(dir.join("chosen")).ok();
            systemctl("stop")
        }
    }
}

/// Restarts the model service (administrators may, without a password).
pub fn restart_service() -> Result<()> {
    systemctl("restart")
}

fn systemctl(verb: &str) -> Result<()> {
    // systemd where there is one; OpenRC (Alpine) otherwise.
    let (program, args): (&str, Vec<&str>) = if std::path::Path::new("/run/systemd/system").exists() {
        ("systemctl", vec![verb, SERVICE])
    } else {
        ("rc-service", vec![SERVICE.trim_end_matches(".service"), verb])
    };
    let out = std::process::Command::new(program)
        .args(&args)
        .output()
        .with_context(|| format!("couldn't {verb} the model service"))?;
    if out.status.success() {
        Ok(())
    } else {
        bail!("Couldn't {verb} the model service: {}", String::from_utf8_lossy(&out.stderr).trim())
    }
}

/// Which model answers right now, in a few words, and whether it runs on this computer:
/// ("qwen3.5 4b", true), ("claude sonnet", false) or ("no model", true).
pub fn model_label() -> (String, bool) {
    if let Some(e) = model::endpoint() {
        let p = crate::providers::for_url(&e.url);
        let name = if e.model.is_empty() { p.name.to_lowercase() } else { e.model.clone() };
        return (name.to_lowercase(), false);
    }
    let Ok(catalog) = Catalog::builtin() else { return ("no model".into(), true) };
    match model::active_tier(&catalog, &model::models_dir()) {
        Some(t) => (t.model.to_lowercase(), true),
        None => ("no model".into(), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwdetect::{Cpu, Gpu, Vendor};

    const GIB: u64 = 1024 * 1024 * 1024;

    fn machine(ram_gib: f64, vram_gib: Option<u64>) -> Hardware {
        Hardware {
            memory_bytes: (ram_gib * GIB as f64) as u64,
            cpu: Cpu { model: "test".into(), threads: 8, flags: vec![] },
            gpus: vram_gib
                .map(|v| vec![Gpu { vendor: Vendor::Nvidia, name: None, vram_bytes: Some(v * GIB) }])
                .unwrap_or_default(),
        }
    }

    #[test]
    fn says_what_a_model_needs() {
        assert_eq!(marketing_gb(14.5), 16);
        assert_eq!(marketing_gb(30.0), 32);
        assert_eq!(marketing_gb(9.5), 10);
        assert_eq!(marketing_gb(22.0), 24);
        let c = Catalog::builtin().unwrap();
        let t = c.tiers.iter().find(|t| t.min_vram_gib.is_none() && t.min_ram_gib.is_some()).unwrap();
        assert!(need_words(t).ends_with("GB of memory"));
    }

    #[test]
    fn lists_what_fits_and_whats_downloaded() {
        let dir = std::env::temp_dir().join(format!("trip-models-list-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let c = Catalog::builtin().unwrap();
        let laptop = machine(15.3, None);
        let list = list_in(&c, Some(&laptop), &dir);
        assert_eq!(list.len(), c.tiers.len());
        assert!(list[0].recommended && list[0].fits);
        assert!(list.iter().any(|m| !m.fits));
        assert!(list.iter().all(|m| !m.installed && !m.active));
        // A sparse file of the right size counts as downloaded, and becomes the one in use.
        let t = &list[0].tier;
        File::create(dir.join(&t.file)).unwrap().set_len(t.size_bytes).unwrap();
        fs::write(part_path(&dir, &list[1].tier), b"abc").unwrap();
        let list = list_in(&c, Some(&laptop), &dir);
        assert!(list[0].installed && list[0].active);
        assert_eq!(list.iter().find(|m| m.tier.id == list[1].tier.id).unwrap().partial, Some(3));
        write_chosen(&dir, &t.id).unwrap();
        assert_eq!(fs::read_to_string(dir.join("chosen")).unwrap(), t.id);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn checksum_stops_when_cancelled() {
        let dir = std::env::temp_dir().join(format!("trip-models-sum-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("abc");
        fs::write(&f, b"abc").unwrap();
        let go = AtomicBool::new(false);
        assert_eq!(sha256_file(&f, &go).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let stop = AtomicBool::new(true);
        assert!(sha256_file(&f, &stop).unwrap_err().is::<Cancelled>());
        fs::remove_dir_all(&dir).ok();
    }
}
