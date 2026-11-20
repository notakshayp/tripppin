use anyhow::Result;
use clap::{Parser, Subcommand};
use hwdetect::{Catalog, Hardware, Tier};
use serde::Serialize;

#[derive(Parser)]
#[command(name = "trip", version, about = "Tripppin: tell your computer what you want.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show this machine's hardware and the model it should run.
    Hw {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Serialize)]
struct HwReport<'a> {
    hardware: &'a Hardware,
    recommended: Option<&'a Tier>,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Hw { json } => hw(json),
    }
}

fn hw(json: bool) -> Result<()> {
    let hardware = hwdetect::detect()?;
    let catalog = Catalog::builtin()?;
    let recommended = catalog.recommend(&hardware);
    if json {
        let report = HwReport { hardware: &hardware, recommended };
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", render(&hardware, recommended));
    }
    Ok(())
}

fn render(hw: &Hardware, tier: Option<&Tier>) -> String {
    let mut out = format!("memory   {:.1} GiB\n", hw.memory_gib());
    let flags = if hw.cpu.flags.is_empty() { String::new() } else { format!(" · {}", hw.cpu.flags.join(" ")) };
    out += &format!("cpu      {} · {} threads{flags}\n", hw.cpu.model, hw.cpu.threads);
    if hw.gpus.is_empty() {
        out += "gpu      none found\n";
    }
    for gpu in &hw.gpus {
        let name = gpu.name.clone().unwrap_or_else(|| format!("{:?}", gpu.vendor).to_lowercase());
        let vram =
            gpu.vram_bytes.map_or("shared memory".to_string(), |b| format!("{:.1} GiB vram", b as f64 / 1073741824.0));
        out += &format!("gpu      {name} · {vram}\n");
    }
    out += &match tier {
        Some(t) => format!("model    {} · {:.1} GB download · on this machine\n", t.model, t.download_gb),
        None => "model    needs a cloud key or your own server (under 16 GB)\n".to_string(),
    };
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwdetect::{Cpu, Gpu, Vendor};

    #[test]
    fn renders_a_laptop() {
        let hw = Hardware {
            memory_bytes: 16 * 1073741824,
            cpu: Cpu { model: "Test CPU".into(), threads: 8, flags: vec!["avx2".into()] },
            gpus: vec![Gpu { vendor: Vendor::Intel, name: None, vram_bytes: None }],
        };
        let catalog = Catalog::builtin().unwrap();
        let text = render(&hw, catalog.recommend(&hw));
        assert!(text.contains("memory   16.0 GiB"));
        assert!(text.contains("cpu      Test CPU · 8 threads · avx2"));
        assert!(text.contains("gpu      intel · shared memory"));
        assert!(text.contains("Qwen3.5 4B"));
    }
}
