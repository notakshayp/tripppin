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
const MAX_REPLY_TOKENS: u32 = 2048;
const MODELS_DIR: &str = "/var/lib/tripppin/models";
const LLAMA_SERVER: &str = "/usr/bin/llama-server";
const IDLE_UNLOAD_SECS: u32 = 900;
const SYSTEM_PROMPT: &str = "You are the assistant built into Tripppin, a Linux operating system based on Fedora. \
The person types to you in their terminal. You have tools: use exec to run shell commands on their machine, \
and read_file / write_file for files. When they ask you to do something, do it with the tools instead of \
explaining how, check the result, and keep going until it's done or you're truly stuck. \
Then answer briefly and plainly, in a few short lines.

About this system:
- The core system (/usr) is a read-only image; updates replace it whole. /etc, /var and the home folder are writable.
- Install apps with: sudo flatpak install -y flathub <app id> (find the id with: flatpak search <name>).
- `sudo dnf install` changes don't last; for a command-line tool, use `sudo dnf install --transient -y <package>` \
and say it lasts until the next reboot.
- sudo may ask the person for their password in the terminal; that's fine.
- Use non-interactive flags (-y, --noconfirm) and avoid commands that open an editor or a pager.
- There is no graphical desktop yet: Tripppin is text-only for now, so windowed (GUI) apps can't be shown. \
If asked for a GUI, a desktop or a windowed app, say that plainly instead of trying to install one.
- Long commands (big installs, downloads) keep running as background jobs; use job_wait to wait for them \
instead of starting them again.
- If something fails twice the same way, stop and tell the person what's blocking you.";

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
    if installed(&models_dir(), tier) {
        println!("{} is already installed.", tier.model);
        return Ok(());
    }
    let tty = io::stderr().is_terminal();
    let name = tier.model.clone();
    pull_tier(tier, &mut |done, total, rate| {
        if tty {
            eprint!(
                "\r\x1b[2Kdownloading {name}  {:.2} / {:.2} GB  {:>3.0}%  {:.0} MB/s",
                done as f64 / 1e9,
                total as f64 / 1e9,
                100.0 * done as f64 / total as f64,
                rate / 1e6
            );
        }
    })?;
    if tty {
        eprintln!();
    }
    println!(
        "{} installed ({:.1} GB). Start it with: sudo systemctl start tripppin-llm",
        tier.model,
        tier.download_gb()
    );
    Ok(())
}

/// The tier this machine should use, from its hardware.
pub fn recommended() -> Result<Tier> {
    let catalog = Catalog::builtin()?;
    let hw = hwdetect::detect()?;
    catalog.recommend(&hw).cloned().context("this machine has under 16 GB of memory, so there's no local model for it")
}

/// Whether the given tier's model is downloaded.
pub fn is_installed(tier: &Tier) -> bool {
    installed(&models_dir(), tier)
}

/// Downloads `tier`'s model (resuming), checks its checksum and moves it into place.
/// `on_progress` gets bytes done, bytes in all and the rate in bytes per second, a few times a second.
pub fn pull_tier(tier: &Tier, on_progress: &mut dyn FnMut(u64, u64, f64)) -> Result<()> {
    let dir = models_dir();
    fs::create_dir_all(&dir).with_context(|| format!("can't create {}. Try again with sudo.", dir.display()))?;
    let dest = dir.join(&tier.file);
    if installed(&dir, tier) {
        return Ok(());
    }
    let part = dir.join(format!("{}.part", tier.file));
    download(&tier.url, &part, tier.size_bytes, on_progress)?;
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
    Ok(())
}

fn download(url: &str, part: &Path, total: u64, on_progress: &mut dyn FnMut(u64, u64, f64)) -> Result<()> {
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
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if last.elapsed() >= Duration::from_millis(250) {
            last = Instant::now();
            on_progress(done, total, (done - start_bytes) as f64 / start.elapsed().as_secs_f64().max(0.001));
        }
    }
    file.sync_all()?;
    on_progress(done, total, 0.0);
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
        // One person at a time: the whole context goes to one conversation.
        "--parallel".into(),
        "1".into(),
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

/// One message in a conversation, in the OpenAI chat format llama.cpp speaks.
#[derive(Clone, Debug)]
pub struct Message(pub serde_json::Value);

impl Message {
    pub fn system() -> Self {
        Message(serde_json::json!({ "role": "system", "content": SYSTEM_PROMPT }))
    }
    pub fn user(text: &str) -> Self {
        Message(serde_json::json!({ "role": "user", "content": text }))
    }
    pub fn assistant(text: &str) -> Self {
        Message(serde_json::json!({ "role": "assistant", "content": text }))
    }
    /// The model's turn when it asked for tools.
    pub fn assistant_calls(text: &str, calls: &[ToolCall]) -> Self {
        let calls: Vec<_> = calls
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments }
                })
            })
            .collect();
        Message(serde_json::json!({ "role": "assistant", "content": text, "tool_calls": calls }))
    }
    /// The result of one tool call.
    pub fn tool(id: &str, content: &str) -> Self {
        Message(serde_json::json!({ "role": "tool", "tool_call_id": id, "content": content }))
    }
}

/// A tool call as the model sent it, before it's been checked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// What came back from one streamed reply.
pub struct Reply {
    pub text: String,
    /// The reply hit the length cap before the model finished.
    pub cut_off: bool,
    pub calls: Vec<ToolCall>,
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

/// Streams a reply to `messages`, calling `on_text` for each piece of text. With `tools`, the model
/// may answer with tool calls instead, which come back in `Reply::calls`. Stops early when `cancel` is set.
pub fn stream(
    messages: &[Message],
    tools: Option<&serde_json::Value>,
    cancel: &AtomicBool,
    mut on_text: impl FnMut(&str),
) -> Result<Reply> {
    let msgs: Vec<_> = messages.iter().map(|m| &m.0).collect();
    let mut body = serde_json::json!({
        "model": "tripppin",
        "stream": true,
        "messages": msgs,
        // Answers and tool calls don't need the model's long thinking pass.
        "chat_template_kwargs": { "enable_thinking": false },
        // Qwen's recommended sampling without thinking; the presence penalty keeps small models
        // from repeating themselves.
        "temperature": 0.7,
        "top_p": 0.8,
        "top_k": 20,
        "min_p": 0.0,
        "presence_penalty": 1.5,
        // A reply is a few lines or a tool call; anything longer has gone wrong.
        "max_tokens": MAX_REPLY_TOKENS
    });
    if let Some(tools) = tools {
        body["tools"] = tools.clone();
    }
    let url = format!("http://127.0.0.1:{PORT}/v1/chat/completions");
    let start = Instant::now();
    let request = http().post(&url).config().http_status_as_error(false).build();
    let mut resp = match request.header("Content-Type", "application/json").send(body.to_string()) {
        Ok(r) => r,
        Err(ureq::Error::Io(_)) | Err(ureq::Error::ConnectionFailed) => return Err(unavailable()),
        Err(e) => return Err(e).context("couldn't reach the model"),
    };
    match resp.status().as_u16() {
        200 => {}
        503 => bail!("the model is still loading. try again in a few seconds."),
        status => {
            let text = resp.body_mut().read_to_string().unwrap_or_default();
            return Err(server_error(status, &text));
        }
    }
    let reader = BufReader::new(resp.body_mut().as_reader());
    let (mut text, mut first, mut pieces, mut cancelled) = (String::new(), None, 0u32, false);
    let mut calls: Vec<ToolCall> = Vec::new();
    let mut cut_off = false;
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
        let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) else { continue };
        if chunk.get("error").is_some() {
            return Err(server_error(200, data));
        }
        if chunk.pointer("/choices/0/finish_reason").and_then(|r| r.as_str()) == Some("length") {
            cut_off = true;
        }
        if let Some(piece) = delta_text(&chunk) {
            first.get_or_insert_with(|| start.elapsed());
            pieces += 1;
            on_text(&piece);
            text.push_str(&piece);
        }
        if merge_tool_calls(&chunk, &mut calls) {
            first.get_or_insert_with(|| start.elapsed());
            pieces += 1;
        }
    }
    let tokens_per_sec =
        first.map_or(0.0, |t: Duration| f64::from(pieces) / start.elapsed().saturating_sub(t).as_secs_f64().max(0.001));
    Ok(Reply { text, cut_off, calls, first_word: first, tokens_per_sec, cancelled })
}

/// The conversation no longer fits in the model's context; drop old material and try again.
#[derive(Debug)]
pub struct ContextFull;

impl std::fmt::Display for ContextFull {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the conversation is too long for the model's memory")
    }
}

impl std::error::Error for ContextFull {}

/// Turns an error reply from llama.cpp into something readable, keeping "context full" distinct.
fn server_error(status: u16, body: &str) -> anyhow::Error {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let err = v.get("error").unwrap_or(&v);
    let message = err.get("message").and_then(|m| m.as_str()).unwrap_or(body).trim().to_string();
    let kind = err.get("type").and_then(|t| t.as_str()).unwrap_or("");
    if kind == "exceed_context_size_error" || message.contains("context size") || message.contains("context length") {
        return ContextFull.into();
    }
    let message = if message.is_empty() { "no details".to_string() } else { message };
    anyhow::anyhow!("the model server said no ({status}): {message}")
}

/// How many tokens the running model can hold, from llama.cpp's /props. 8192 if it can't say.
pub fn context_tokens() -> usize {
    let url = format!("http://127.0.0.1:{PORT}/props");
    http()
        .get(&url)
        .call()
        .ok()
        .and_then(|mut r| r.body_mut().read_to_string().ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.pointer("/default_generation_settings/n_ctx").and_then(|n| n.as_u64()))
        .map_or(8192, |n| n as usize)
}

/// Sends one question and prints the streamed answer (`trip ask`, for scripts and testing).
pub fn ask(question: &str) -> Result<()> {
    let mut out = io::stdout().lock();
    let reply = stream(&[Message::system(), Message::user(question)], None, &AtomicBool::new(false), |t| {
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
fn delta_text(chunk: &serde_json::Value) -> Option<String> {
    let s = chunk.pointer("/choices/0/delta/content")?.as_str()?;
    (!s.is_empty()).then(|| s.to_string())
}

/// Adds the tool-call pieces in one streamed chunk to `calls`. Each call arrives in parts keyed by
/// `index`: the id and name first, then the arguments a few characters at a time.
fn merge_tool_calls(chunk: &serde_json::Value, calls: &mut Vec<ToolCall>) -> bool {
    let Some(parts) = chunk.pointer("/choices/0/delta/tool_calls").and_then(|v| v.as_array()) else { return false };
    for part in parts {
        let index = part.get("index").and_then(|i| i.as_u64()).map_or(calls.len().saturating_sub(1), |i| i as usize);
        while calls.len() <= index {
            calls.push(ToolCall::default());
        }
        let call = &mut calls[index];
        if let Some(id) = part.get("id").and_then(|v| v.as_str()) {
            call.id.push_str(id);
        }
        if let Some(name) = part.pointer("/function/name").and_then(|v| v.as_str()) {
            call.name.push_str(name);
        }
        if let Some(args) = part.pointer("/function/arguments").and_then(|v| v.as_str()) {
            call.arguments.push_str(args);
        }
    }
    !parts.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_streamed_chunks() {
        let chunk = serde_json::json!({"choices":[{"delta":{"content":"hel"},"index":0}]});
        assert_eq!(delta_text(&chunk).as_deref(), Some("hel"));
        assert_eq!(delta_text(&serde_json::json!({"choices":[{"delta":{"role":"assistant"}}]})), None);
    }

    #[test]
    fn recognises_a_full_context() {
        let body = r#"{"error":{"code":400,"message":"the request exceeds the available context size, try increasing it","type":"exceed_context_size_error"}}"#;
        assert!(server_error(400, body).downcast_ref::<ContextFull>().is_some());
        let other = server_error(500, r#"{"error":{"message":"boom"}}"#);
        assert!(other.downcast_ref::<ContextFull>().is_none());
        assert_eq!(other.to_string(), "the model server said no (500): boom");
    }

    #[test]
    fn assembles_tool_calls_from_pieces() {
        let pieces = [
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"exec","arguments":""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"command\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"ls\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"c2","function":{"name":"read_file","arguments":"{}"}}]}}]}"#,
        ];
        let mut calls = Vec::new();
        for p in pieces {
            assert!(merge_tool_calls(&serde_json::from_str(p).unwrap(), &mut calls));
        }
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0],
            ToolCall { id: "c1".into(), name: "exec".into(), arguments: r#"{"command":"ls"}"#.into() }
        );
        assert_eq!(calls[1].name, "read_file");
        assert!(!merge_tool_calls(&serde_json::json!({"choices":[{"delta":{"content":"x"}}]}), &mut calls));
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
