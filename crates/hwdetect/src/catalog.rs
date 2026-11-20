use crate::Hardware;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// The model catalog shipped with the OS (`models/catalog.toml`).
#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    pub schema: u32,
    #[serde(rename = "tier")]
    pub tiers: Vec<Tier>,
}

/// One hardware tier and the model it runs. Tiers are listed best first.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Tier {
    pub id: String,
    pub model: String,
    pub download_gb: f64,
    pub license: String,
    #[serde(default)]
    pub min_vram_gib: Option<f64>,
    #[serde(default)]
    pub min_ram_gib: Option<f64>,
}

impl Catalog {
    /// The catalog built into this binary.
    pub fn builtin() -> Result<Self> {
        Self::parse(include_str!("../../../models/catalog.toml"))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let catalog: Self = toml::from_str(text).context("parsing model catalog")?;
        if catalog.schema != 1 {
            bail!("unsupported catalog schema {}", catalog.schema);
        }
        Ok(catalog)
    }

    /// The first tier this machine qualifies for, or `None` when it should use
    /// a cloud key or its own server instead.
    pub fn recommend(&self, hw: &Hardware) -> Option<&Tier> {
        self.tiers.iter().find(|t| {
            t.min_vram_gib.is_none_or(|need| hw.max_vram_gib() >= need)
                && t.min_ram_gib.is_none_or(|need| hw.memory_gib() >= need)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cpu, Gpu, Vendor};

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

    fn pick(ram_gib: f64, vram_gib: Option<u64>) -> Option<String> {
        Catalog::builtin().unwrap().recommend(&machine(ram_gib, vram_gib)).map(|t| t.id.clone())
    }

    #[test]
    fn builtin_catalog_parses() {
        let c = Catalog::builtin().unwrap();
        assert!(!c.tiers.is_empty());
        assert!(c.tiers.iter().all(|t| t.license == "Apache-2.0"));
    }

    #[test]
    fn tiers_follow_the_plan() {
        // A "16 GB" laptop reports about 15.3 GiB.
        assert_eq!(pick(15.3, None).as_deref(), Some("cpu-16"));
        assert_eq!(pick(31.0, None).as_deref(), Some("cpu-32"));
        assert_eq!(pick(15.3, Some(8)).as_deref(), Some("gpu-8"));
        assert_eq!(pick(62.0, Some(24)).as_deref(), Some("gpu-24"));
    }

    #[test]
    fn small_machines_get_no_local_model() {
        assert_eq!(pick(7.6, None), None);
    }

    #[test]
    fn rejects_unknown_schema() {
        assert!(Catalog::parse("schema = 2\ntier = []\n").is_err());
    }
}
