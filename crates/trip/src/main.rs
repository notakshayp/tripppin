use anyhow::Result;
use clap::{Parser, Subcommand};
use hwdetect::{Catalog, Hardware, Tier};
use serde::Serialize;

mod logo;
mod shell;

use trip::{agent, model};

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
    /// The AI shell: type anything to ask. Starts automatically in a terminal.
    Shell,
    /// Ask one question and print the answer (for scripts and testing).
    #[command(hide = true)]
    Ask {
        /// The question, in plain words.
        #[arg(required = true, trailing_var_arg = true)]
        words: Vec<String>,
    },
    /// Do one task with tools and print the result (for scripts and testing).
    #[command(hide = true, name = "do")]
    Do {
        /// The task, in plain words.
        #[arg(required = true, trailing_var_arg = true)]
        words: Vec<String>,
    },
    /// Manage local models.
    #[command(subcommand)]
    Model(ModelCommand),
    /// Show the tripppin logo with a short system summary.
    Logo {
        /// Draw it once without animating.
        #[arg(long = "static")]
        still: bool,
        /// Only the logo, without the system summary (used for the login banner).
        #[arg(long)]
        bare: bool,
    },
}

#[derive(Subcommand)]
enum ModelCommand {
    /// List the catalog and what's installed.
    List,
    /// Download a model (the one recommended for this machine unless you name a tier).
    Pull {
        /// A tier id from `trip model list`, such as cpu-16.
        tier: Option<String>,
    },
    /// Run the model server (used by tripppin-llm.service).
    Serve,
}

#[derive(Serialize)]
struct HwReport<'a> {
    hardware: &'a Hardware,
    recommended: Option<&'a Tier>,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Hw { json } => hw(json),
        Command::Shell => shell::run(),
        Command::Ask { words } => model::ask(&words.join(" ")),
        Command::Do { words } => agent::once(&words.join(" ")),
        Command::Model(ModelCommand::List) => model::list(),
        Command::Model(ModelCommand::Pull { tier }) => model::pull(tier.as_deref()),
        Command::Model(ModelCommand::Serve) => model::serve(),
        Command::Logo { still, bare } => {
            let info = if bare { Vec::new() } else { summary() };
            Ok(logo::show(&info, !still)?)
        }
    }
}

/// The lines printed beside the logo. Each piece is skipped if it can't be read.
fn summary() -> Vec<String> {
    let mut lines = vec!["tripppin".to_string(), "a linux with an ai inside".to_string(), String::new()];
    let os = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string())));
    if let Some(os) = os {
        lines.push(format!("system   {os}"));
    }
    if let Ok(kernel) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        lines.push(format!("kernel   {}", kernel.trim()));
    }
    if let Ok(hw) = hwdetect::detect() {
        lines.push(format!("memory   {:.1} GiB", hw.memory_gib()));
        if let Ok(catalog) = Catalog::builtin() {
            lines.push(match catalog.recommend(&hw) {
                Some(t) if model::active_tier(&catalog, &model::models_dir()).is_some_and(|a| a.id == t.id) => {
                    format!("model    {} · ready", t.model)
                }
                Some(t) => format!("model    {} · not downloaded (sudo trip model pull)", t.model),
                None => "model    cloud key or your own server".to_string(),
            });
        }
    }
    lines
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
        Some(t) => format!("model    {} · {:.1} GB download · on this machine\n", t.model, t.download_gb()),
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
