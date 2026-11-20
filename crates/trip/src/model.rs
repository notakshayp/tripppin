//! Local models: download, serve and ask.
//!
//! Models live in `/var/lib/tripppin/models` (override with `TRIPPPIN_MODELS`).
//! `tripppin-llm.service` runs `trip model serve`, which starts llama.cpp's server
//! on 127.0.0.1:8086 with the best installed model; `trip ask` talks to it.

use anyhow::{Context, Result, bail};
use hwdetect::{Catalog, Tier};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const PORT: u16 = 8086;
const MODELS_DIR: &str = "/var/lib/tripppin/models";
const LLAMA_SERVER: &str = "/usr/bin/llama-server";
const IDLE_UNLOAD_SECS: u32 = 900;
const SYSTEM_PROMPT: &str = "You are the assistant built into Tripppin, a Linux operating system. \
The person is typing to you in their terminal. Answer briefly and plainly, in a few short lines. \
You can't run commands yet; if something needs a command, give it and say they can run it with /terminal.";

/// An HTTP client that trusts the operating system's certificate store, so CA updates
/// arrive with system updates rather than with a new build of trip.
fn http() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    let tls = TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder().tls_config(tls).build().into()
}

pub fn models_dir() -> PathBuf {
    std::env::var_os("TRIPPPIN_MODELS").map_or_else(|| PathBuf::from(MODELS_DIR), PathBuf::from)
}

fn installed(dir: &Path, tier: &Tier) -> bool {
    fs::metadata(dir.join(&tier.file)).is_ok_and(|m| m.len() == tier.size_bytes)
}

/// The tier to use: the recommended one if it's installed, otherwise the best installed one.
pub fn active_tier<'a>(catalog: &'a Catalog, dir: &Path) -> Option<&'a Tier> {
    let recommended = hwdetect::detect().ok().and_then(|hw| catalog.recommend(&hw));
    recommended.filter(|t| installed(dir, t)).or_else(|| catalog.tiers.iter().find(|t| installed(dir, t)))
}

pub fn list() -> Result<()> {
    let catalog = Catalog::builtin()?;
    let dir = models_dir();
    let recommended = hwdetect::detect().ok().and_then(|hw| catalog.recommend(&hw)).map(|t| t.id.clone());
    for t in &catalog.tiers {
        let mark = if installed(&dir, t) { "installed" } else { "" };
        let rec = if recommended.as_deref() == Some(t.id.as_str()) { "recommended" } else { "" };
        println!("{:<8} {:<18} {:>6.1} GB  {:<10} {}", t.id, t.model, t.download_gb(), mark, rec);
    }
    println!("\nmodels folder: {}", dir.display());
    Ok(())
}

/// Downloads a tier's model, resuming a partial download, and verifies its checksum.
pub fn pull(tier_id: Option<&str>) -> Result<()> {
    let catalog = Catalog::builtin()?;
    let tier = match tier_id {
        Some(id) => catalog.tiers.iter().find(|t| t.id == id).with_context(|| {
            let ids: Vec<_> = catalog.tiers.iter().map(|t| t.id.as_str()).collect();
            format!("no tier called {id}; choose one of: {}", ids.join(", "))
        })?,
        None => {
            let hw = hwdetect::detect()?;
            catalog.recommend(&hw).context(
                "this machine has under 16 GB of memory, so there's no local model for it. Use a cloud key or your own server.",
            )?
        }
    };
    let dir = models_dir();
    fs::create_dir_all(&dir).with_context(|| format!("can't create {}. Try again with sudo.", dir.display()))?;
    let dest = dir.join(&tier.file);
    if installed(&dir, tier) {
        println!("{} is already installed.", tier.model);
        return Ok(());
    }
    let part = dir.join(format!("{}.part", tier.file));
    download(&tier.url, &part, tier.size_bytes, &tier.model)?;
    eprintln!("checking the file…");
    let actual = sha256_file(&part)?;
    if actual != tier.sha256 {
        fs::remove_file(&part).ok();
        bail!(
            "checksum mismatch for {} (expected {}, got {actual}). The download was deleted; run it again.",
            tier.file,
            tier.sha256
        );
    }
    fs::rename(&part, &dest)?;
    println!(
        "{} installed ({:.1} GB). Start it with: sudo systemctl start tripppin-llm",
        tier.model,
        tier.download_gb()
    );
    Ok(())
}

fn download(url: &str, part: &Path, total: u64, name: &str) -> Result<()> {
    let have = fs::metadata(part).map_or(0, |m| m.len());
    if have >= total {
        return Ok(());
    }
    let mut req = http().get(url);
    if have > 0 {
        req = req.header("Range", &format!("bytes={have}-"));
    }
    let mut resp = req.call().with_context(|| format!("downloading {url}"))?;
    // A server that ignores Range sends the whole file again; start over in that case.
    let resumed = resp.status().as_u16() == 206;
    let mut file = if resumed { OpenOptions::new().append(true).open(part)? } else { File::create(part)? };
    let mut done = if resumed { have } else { 0 };
    let mut reader = resp.body_mut().as_reader();
    let mut buf = vec![0u8; 1 << 20];
    let (start, mut last) = (Instant::now(), Instant::now() - Duration::from_secs(1));
    let start_bytes = done;
    let tty = io::stderr().is_terminal();
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if tty && last.elapsed() >= Duration::from_millis(250) {
            last = Instant::now();
            let rate = (done - start_bytes) as f64 / start.elapsed().as_secs_f64().max(0.001) / 1e6;
            eprint!(
                "\r\x1b[2Kdownloading {name}  {:.2} / {:.2} GB  {:>3.0}%  {rate:.0} MB/s",
                done as f64 / 1e9,
                total as f64 / 1e9,
                100.0 * done as f64 / total as f64
            );
        }
    }
    file.sync_all()?;
    if tty {
        eprintln!();
    }
    if done != total {
        bail!("download stopped at {done} of {total} bytes; run the command again to resume");
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The llama.cpp server arguments for a model. Kept separate so it can be tested.
pub fn server_args(tier: &Tier, model_path: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--model".into(),
        model_path.display().to_string(),
        "--alias".into(),
        "tripppin".into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        PORT.to_string(),
        "--ctx-size".into(),
        tier.context.to_string(),
        "--jinja".into(),
        // Put every layer on the GPU when there is one; llama.cpp falls back to the CPU otherwise.
        "--n-gpu-layers".into(),
        "99".into(),
        // Free the memory when nobody has asked anything for a while.
        "--sleep-idle-seconds".into(),
        IDLE_UNLOAD_SECS.to_string(),
    ];
    if let Ok(extra) = std::env::var("TRIPPPIN_LLAMA_ARGS") {
        args.extend(extra.split_whitespace().map(String::from));
    }
    args
}

/// Replaces this process with llama.cpp's server. Used by `tripppin-llm.service`.
pub fn serve() -> Result<()> {
    let catalog = Catalog::builtin()?;
    let dir = models_dir();
    let tier = active_tier(&catalog, &dir).context("no model is installed yet. Run: sudo trip model pull")?;
    let args = server_args(tier, &dir.join(&tier.file));
    let bin = std::env::var("TRIPPPIN_LLAMA_SERVER").unwrap_or_else(|_| LLAMA_SERVER.to_string());
    eprintln!("starting {} with {}", bin, tier.model);
    exec(&bin, &args)
}

#[cfg(unix)]
fn exec(bin: &str, args: &[String]) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(bin).args(args).exec();
    Err(err).with_context(|| format!("starting {bin}"))
}

#[cfg(not(unix))]
fn exec(bin: &str, _args: &[String]) -> Result<()> {
    bail!("{bin} can only be started on Linux")
}

/// One message in a conversation.
#[derive(Clone, Debug)]
pub struct Message {
    pub role: &'static str,
    pub content: String,
}

impl Message {
    pub fn system() -> Self {
        Message { role: "system", content: SYSTEM_PROMPT.to_string() }
    }
    pub fn user(text: &str) -> Self {
        Message { role: "user", content: text.to_string() }
    }
    pub fn assistant(text: &str) -> Self {
        Message { role: "assistant", content: text.to_string() }
    }
}

/// What came back from one streamed reply.
pub struct Reply {
    pub text: String,
    pub first_word: Option<Duration>,
    pub tokens_per_sec: f64,
    pub cancelled: bool,
}

impl Reply {
    pub fn stats(&self) -> Option<String> {
        self.first_word.map(|t| format!("first word {:.1} s · {:.0} tokens/s", t.as_secs_f64(), self.tokens_per_sec))
    }
}

/// Explains why the model can't be reached, and what to do about it.
fn unavailable() -> anyhow::Error {
    let installed = Catalog::builtin().ok().is_some_and(|c| active_tier(&c, &models_dir()).is_some());
    if installed {
        anyhow::anyhow!("the model service isn't running yet. type /setup to start it.")
    } else {
        anyhow::anyhow!("no model is downloaded yet. type /setup to download the right one for this machine.")
    }
}

/// Streams a reply to `messages`, calling `on_text` for each piece. Stops early when `cancel` is set.
pub fn stream(messages: &[Message], cancel: &AtomicBool, mut on_text: impl FnMut(&str)) -> Result<Reply> {
    let msgs: Vec<_> = messages.iter().map(|m| serde_json::json!({ "role": m.role, "content": m.content })).collect();
    let body = serde_json::json!({
        "model": "tripppin",
        "stream": true,
        "messages": msgs,
        // Plain answers don't need the model's long thinking pass.
        "chat_template_kwargs": { "enable_thinking": false }
    });
    let url = format!("http://127.0.0.1:{PORT}/v1/chat/completions");
    let start = Instant::now();
    let mut resp = match http().post(&url).header("Content-Type", "application/json").send(body.to_string()) {
        Ok(r) => r,
        Err(ureq::Error::Io(_)) | Err(ureq::Error::ConnectionFailed) => return Err(unavailable()),
        Err(ureq::Error::StatusCode(503)) => bail!("the model is still loading. try again in a few seconds."),
        Err(e) => return Err(e).context("asking the model"),
    };
    let reader = BufReader::new(resp.body_mut().as_reader());
    let (mut text, mut first, mut pieces, mut cancelled) = (String::new(), None, 0u32, false);
    for line in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let line = line?;
        let Some(data) = line.strip_prefix("data: ") else { continue };
        if data.trim() == "[DONE]" {
            break;
        }
        if let Some(piece) = delta_text(data) {
            first.get_or_insert_with(|| start.elapsed());
            pieces += 1;
            on_text(&piece);
            text.push_str(&piece);
        }
    }
    let tokens_per_sec =
        first.map_or(0.0, |t: Duration| f64::from(pieces) / start.elapsed().saturating_sub(t).as_secs_f64().max(0.001));
    Ok(Reply { text, first_word: first, tokens_per_sec, cancelled })
}

/// Sends one question and prints the streamed answer (`trip ask`, for scripts and testing).
pub fn ask(question: &str) -> Result<()> {
    let mut out = io::stdout().lock();
    let reply = stream(&[Message::system(), Message::user(question)], &AtomicBool::new(false), |t| {
        let _ = write!(out, "{t}");
        let _ = out.flush();
    })?;
    writeln!(out)?;
    if let Some(note) = reply.stats() {
        if io::stderr().is_terminal() {
            eprintln!("\x1b[2m{note}\x1b[0m");
        } else {
            eprintln!("{note}");
        }
    }
    Ok(())
}

/// The text in one streamed chunk, if any.
fn delta_text(data: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    let s = v.pointer("/choices/0/delta/content")?.as_str()?;
    (!s.is_empty()).then(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_streamed_chunks() {
        let chunk = r#"{"choices":[{"delta":{"content":"hel"},"index":0}]}"#;
        assert_eq!(delta_text(chunk).as_deref(), Some("hel"));
        assert_eq!(delta_text(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#), None);
        assert_eq!(delta_text("not json"), None);
    }

    #[test]
    fn server_stays_local_and_uses_the_tier_context() {
        let catalog = Catalog::builtin().unwrap();
        let tier = &catalog.tiers[0];
        let args = server_args(tier, Path::new("/m/x.gguf"));
        let joined = args.join(" ");
        assert!(joined.contains("--host 127.0.0.1"));
        assert!(joined.contains(&format!("--ctx-size {}", tier.context)));
        assert!(joined.contains("--model /m/x.gguf"));
    }

    #[test]
    fn checksum_of_a_known_file() {
        let dir = std::env::temp_dir().join(format!("trip-sha-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("abc");
        fs::write(&f, b"abc").unwrap();
        assert_eq!(sha256_file(&f).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn picks_an_installed_model() {
        let dir = std::env::temp_dir().join(format!("trip-models-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let catalog = Catalog::builtin().unwrap();
        assert!(active_tier(&catalog, &dir).is_none());
        // A sparse file of the right size counts as installed.
        let t = catalog.tiers.last().unwrap();
        File::create(dir.join(&t.file)).unwrap().set_len(t.size_bytes).unwrap();
        assert_eq!(active_tier(&catalog, &dir).map(|t| t.id.as_str()), Some(t.id.as_str()));
        fs::remove_dir_all(&dir).ok();
    }
}
