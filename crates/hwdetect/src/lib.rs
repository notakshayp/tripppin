//! Hardware detection for Tripppin.
//!
//! Parsing is kept separate from reading the filesystem, so every parser is
//! tested against captured `/proc` and `/sys` text on any platform. Reading the
//! live system only works on Linux.

mod catalog;
pub mod gguf;
mod parse;
pub mod serving;

pub use catalog::{Catalog, Tier};
pub use parse::{amd_integrated, parse_cpuinfo, parse_meminfo, vendor_from_pci_id};

use serde::Serialize;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// What the machine has, as far as model selection is concerned.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hardware {
    pub memory_bytes: u64,
    pub cpu: Cpu,
    pub gpus: Vec<Gpu>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cpu {
    pub model: String,
    pub threads: usize,
    /// Only the flags that matter for inference speed (avx2, avx512f, …).
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gpu {
    pub vendor: Vendor,
    pub name: Option<String>,
    /// Dedicated video memory. `None` when the driver doesn't report it, and for
    /// integrated GPUs, which share system memory (even when the BIOS sets some aside).
    pub vram_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl Hardware {
    pub fn memory_gib(&self) -> f64 {
        self.memory_bytes as f64 / GIB
    }

    /// The largest dedicated VRAM across all GPUs, in GiB (integrated GPUs have none).
    pub fn max_vram_gib(&self) -> f64 {
        self.gpus.iter().filter_map(|g| g.vram_bytes).max().map_or(0.0, |b| b as f64 / GIB)
    }
}

/// Reads the live system. Linux only.
#[cfg(target_os = "linux")]
pub fn detect() -> anyhow::Result<Hardware> {
    linux::detect()
}

#[cfg(not(target_os = "linux"))]
pub fn detect() -> anyhow::Result<Hardware> {
    anyhow::bail!("hardware detection only runs on Linux")
}

/// How much memory an integrated AMD GPU may map: its BIOS carve-out plus the GTT, the part
/// of system memory the kernel lets it use (half the RAM by default, raised with the
/// `ttm.pages_limit` kernel argument). `None` without one, or off Linux.
pub fn igpu_pool_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        linux::igpu_pool()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{Gpu, Hardware, Vendor, amd_integrated, parse_cpuinfo, parse_meminfo, vendor_from_pci_id};
    use anyhow::{Context, Result};
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    pub fn detect() -> Result<Hardware> {
        let meminfo = fs::read_to_string("/proc/meminfo").context("reading /proc/meminfo")?;
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").context("reading /proc/cpuinfo")?;
        let cpu = parse_cpuinfo(&cpuinfo);
        Ok(Hardware {
            memory_bytes: parse_meminfo(&meminfo).context("MemTotal missing from /proc/meminfo")?,
            gpus: gpus(&cpu.model),
            cpu,
        })
    }

    /// Each AMD card's VRAM and GTT, as amdgpu reports them.
    fn amd_cards() -> Vec<(u64, u64)> {
        let read = |dev: &Path, f: &str| fs::read_to_string(dev.join(f)).ok().and_then(|v| v.trim().parse().ok());
        cards()
            .iter()
            .map(|card| Path::new("/sys/class/drm").join(card).join("device"))
            .filter(|dev| {
                fs::read_to_string(dev.join("vendor")).ok().and_then(|v| vendor_from_pci_id(v.trim()))
                    == Some(Vendor::Amd)
            })
            .map(|dev| (read(&dev, "mem_info_vram_total").unwrap_or(0), read(&dev, "mem_info_gtt_total").unwrap_or(0)))
            .collect()
    }

    pub fn igpu_pool() -> Option<u64> {
        let cpu = parse_cpuinfo(&fs::read_to_string("/proc/cpuinfo").ok()?);
        let cards = amd_cards();
        let vram: Vec<u64> = cards.iter().map(|c| c.0).collect();
        let flags = amd_integrated(&cpu.model, &vram);
        cards.iter().zip(flags).find(|(_, integrated)| *integrated).map(|((v, g), _)| v + g).filter(|&b| b > 0)
    }

    fn cards() -> Vec<String> {
        let Ok(entries) = fs::read_dir("/sys/class/drm") else { return Vec::new() };
        let mut cards: Vec<_> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.strip_prefix("card").is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit())))
            .collect();
        cards.sort();
        cards
    }

    /// One entry per DRM card (`card0`, `card1`, …), skipping connectors like `card0-HDMI-A-1`.
    fn gpus(cpu_model: &str) -> Vec<Gpu> {
        let cards = cards();
        let nvidia = nvidia_smi();
        let amd_vram: Vec<u64> = amd_cards().iter().map(|c| c.0).collect();
        let amd_flags = amd_integrated(cpu_model, &amd_vram);
        let mut amd_index = 0;
        cards
            .iter()
            .map(|card| {
                let dev = Path::new("/sys/class/drm").join(card).join("device");
                let vendor = fs::read_to_string(dev.join("vendor"))
                    .ok()
                    .and_then(|v| vendor_from_pci_id(v.trim()))
                    .unwrap_or(Vendor::Other);
                let mut gpu = Gpu { vendor, name: None, vram_bytes: None };
                match vendor {
                    // amdgpu reports VRAM directly; an APU's is only the BIOS carve-out.
                    Vendor::Amd => {
                        let integrated = amd_flags.get(amd_index).copied().unwrap_or(true);
                        gpu.vram_bytes = amd_vram.get(amd_index).copied().filter(|&b| b > 0 && !integrated);
                        amd_index += 1;
                    }
                    Vendor::Nvidia => {
                        if let Some((name, vram)) = nvidia.first() {
                            gpu.name = Some(name.clone());
                            gpu.vram_bytes = Some(*vram);
                        }
                    }
                    Vendor::Intel | Vendor::Other => {}
                }
                gpu
            })
            .collect()
    }

    /// Name and VRAM per NVIDIA GPU, if the proprietary driver is installed.
    fn nvidia_smi() -> Vec<(String, u64)> {
        let Ok(out) = Command::new("nvidia-smi")
            .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
            .output()
        else {
            return Vec::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let (name, mib) = l.rsplit_once(',')?;
                Some((name.trim().to_string(), mib.trim().parse::<u64>().ok()? * 1024 * 1024))
            })
            .collect()
    }
}
