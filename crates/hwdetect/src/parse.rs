use crate::{Cpu, Vendor};

/// CPU flags worth reporting: they change which llama.cpp kernels run.
const INTERESTING_FLAGS: &[&str] = &["avx2", "avx512f", "avx512_vnni", "avx_vnni", "f16c", "fma"];

/// Total memory in bytes from `/proc/meminfo` (`MemTotal:  32768000 kB`).
pub fn parse_meminfo(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// Model name, thread count and the flags we care about from `/proc/cpuinfo`.
///
/// x86 lists `model name` and `flags`; ARM lists neither reliably, so the model
/// falls back to `unknown` and flags come from `Features` when present.
pub fn parse_cpuinfo(text: &str) -> Cpu {
    let field = |key: &str| {
        text.lines()
            .find(|l| l.split(':').next().is_some_and(|k| k.trim() == key))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().to_string())
    };
    let threads = text.lines().filter(|l| l.split(':').next().is_some_and(|k| k.trim() == "processor")).count();
    let flags = field("flags")
        .or_else(|| field("Features"))
        .map(|all| {
            INTERESTING_FLAGS
                .iter()
                .filter(|f| all.split_whitespace().any(|have| have == **f))
                .map(|f| f.to_string())
                .collect()
        })
        .unwrap_or_default();
    Cpu { model: field("model name").unwrap_or_else(|| "unknown".to_string()), threads: threads.max(1), flags }
}

/// Maps a PCI vendor ID as written in sysfs (`0x10de`) to a vendor.
pub fn vendor_from_pci_id(id: &str) -> Option<Vendor> {
    let hex = id.trim().trim_start_matches("0x");
    match u16::from_str_radix(hex, 16).ok()? {
        0x10de => Some(Vendor::Nvidia),
        0x1002 => Some(Vendor::Amd),
        0x8086 => Some(Vendor::Intel),
        _ => Some(Vendor::Other),
    }
}

/// Which AMD cards are integrated (an APU's GPU), given each card's reported VRAM in the order
/// found. An APU reports only its BIOS carve-out: usually under 2 GiB, but some laptops set
/// aside 4 or 8. So a bigger card still counts as integrated when the CPU has Radeon graphics
/// built in ("AMD Ryzen 7 7840U w/ Radeon 780M Graphics") and it's the smallest AMD card there.
pub fn amd_integrated(cpu_model: &str, vram: &[u64]) -> Vec<bool> {
    const SMALL: u64 = 2 * 1024 * 1024 * 1024;
    let apu = cpu_model.to_lowercase().contains("radeon");
    let smallest = vram.iter().copied().min().unwrap_or(0);
    // Only one APU per machine: a small card already is it.
    let mut taken = vram.iter().any(|&v| v < SMALL);
    vram.iter()
        .map(|&v| {
            if v < SMALL {
                return true;
            }
            if apu && !taken && v == smallest {
                taken = true;
                return true;
            }
            false
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_total() {
        let text = include_str!("../tests/fixtures/meminfo.txt");
        assert_eq!(parse_meminfo(text), Some(32_559_268 * 1024));
    }

    #[test]
    fn meminfo_missing() {
        assert_eq!(parse_meminfo("MemFree: 1 kB\n"), None);
    }

    #[test]
    fn cpuinfo_x86() {
        let cpu = parse_cpuinfo(include_str!("../tests/fixtures/cpuinfo-x86.txt"));
        assert_eq!(cpu.model, "AMD Ryzen 7 7840U w/ Radeon 780M Graphics");
        assert_eq!(cpu.threads, 2);
        assert_eq!(cpu.flags, vec!["avx2", "avx512f", "f16c", "fma"]);
    }

    #[test]
    fn cpuinfo_arm() {
        let cpu = parse_cpuinfo("processor\t: 0\nFeatures\t: fp asimd\n\nprocessor\t: 1\nFeatures\t: fp asimd\n");
        assert_eq!(cpu.model, "unknown");
        assert_eq!(cpu.threads, 2);
        assert!(cpu.flags.is_empty());
    }

    #[test]
    fn tells_apus_from_graphics_cards() {
        const G: u64 = 1024 * 1024 * 1024;
        let apu = "AMD Ryzen 7 7840U w/ Radeon 780M Graphics";
        assert_eq!(amd_integrated(apu, &[512 << 20]), [true]);
        // Some laptops' BIOS sets 8 GB aside for the APU.
        assert_eq!(amd_integrated(apu, &[8 * G]), [true]);
        // An APU and a Radeon card: the small one is the APU.
        assert_eq!(amd_integrated(apu, &[8 * G, G / 2]), [false, true]);
        assert_eq!(amd_integrated(apu, &[16 * G, 4 * G]), [false, true]);
        // A desktop CPU without graphics and a Radeon card.
        assert_eq!(amd_integrated("AMD Ryzen 7 7700X 8-Core Processor", &[16 * G]), [false]);
        assert!(amd_integrated(apu, &[]).is_empty());
    }

    #[test]
    fn pci_vendors() {
        assert_eq!(vendor_from_pci_id("0x10de\n"), Some(Vendor::Nvidia));
        assert_eq!(vendor_from_pci_id("0x1002"), Some(Vendor::Amd));
        assert_eq!(vendor_from_pci_id("0x8086"), Some(Vendor::Intel));
        assert_eq!(vendor_from_pci_id("0x1af4"), Some(Vendor::Other));
        assert_eq!(vendor_from_pci_id("nope"), None);
    }
}
