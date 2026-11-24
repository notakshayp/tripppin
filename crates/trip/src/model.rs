//! Local models: download, serve and ask.
//!
//! Models live in `/var/lib/tripppin/models` (override with `TRIPPPIN_MODELS`).
//! `tripppin-llm.service` runs `trip model serve`, which starts llama.cpp's server
//! on 127.0.0.1:8086 with the best installed model; `trip ask` talks to it.

use anyhow::{Context, Result, bail};
use hwdetect::gguf::{self, ModelShape};
use hwdetect::serving::{self, Accel, Placement, ServePlan};
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
const SYSTEM_PROMPT: &str = "You are the assistant built into Tripppin, a Linux operating system based on Fedora \
with its own desktop. The person asks you from the orb (a pop-up on the desktop) or from the terminal. You have tools: \
exec runs shell commands on their machine, read_file / write_file work with files, plan and done keep your task \
plan, and note writes in your task notebook. When they ask you to do something, do it with the tools instead of \
explaining how, check the result, and keep going until it's done or you're truly stuck. Then answer briefly and \
plainly, in a few short lines.

How to work:
- For a task of more than two steps, first write a plan with plan: short steps, each one thing you can do and check.
- Work on one step at a time. Do it, check it worked, then call done with its number and how you checked. If a \
step can't be done, rewrite the plan or tell the person what's blocking you.
- Note important facts as you find them.
- Your task notebook (the request, your plan, your notes and every step with its result) is shown to you after the \
newest message. Read it before each step; don't redo steps marked ok.
- One command per step; check its output before the next. If something fails twice the same way, stop and tell the \
person what's blocking you.

About this system:
- The core system (/usr) is a read-only image; updates replace it whole. /etc, /var and the home folder are writable.
- For free disk space, check /var (df -h /var): / is the read-only system image and always looks full.
- To install any app (vlc, firefox, spotify, gimp…): flatpak search <name>, then flatpak install -y flathub <app id>. \
No sudo; it works as is. Open apps with: flatpak run <app id> (run it in the background). Never use dnf for apps.
- Only for a command-line tool with no Flatpak: sudo dnf install --transient -y <package>, which lasts until reboot.
- sudo may ask the person for their password in a pop-up, or be allowed without one (Settings → AI). If sudo fails \
with \"a password is required\" or the person cancels, don't retry: tell them the exact command to run themselves.
- Use non-interactive flags (-y, --noconfirm) and avoid commands that open an editor or a pager.
- The desktop is niri (Wayland). Windowed apps open on the desktop; start them in the background.
- Long commands (big installs, downloads) keep running as background jobs; use job_wait to wait for them \
instead of starting them again.";

/// An HTTP client that trusts the operating system's certificate store, so CA updates
/// arrive with system updates rather than with a new build of trip.
pub fn http_agent() -> ureq::Agent {
    http()
}

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

/// The tier to use: the one chosen in Settings if it's installed, else the recommended one if
/// it's installed, otherwise the best installed one.
pub fn active_tier<'a>(catalog: &'a Catalog, dir: &Path) -> Option<&'a Tier> {
    let chosen = fs::read_to_string(dir.join("chosen")).unwrap_or_default();
    let chosen = catalog.tiers.iter().find(|t| t.id == chosen.trim());
    let recommended = hwdetect::detect().ok().and_then(|hw| catalog.recommend(&hw));
    chosen
        .filter(|t| installed(dir, t))
        .or(recommended.filter(|t| installed(dir, t)))
        .or_else(|| catalog.tiers.iter().find(|t| installed(dir, t)))
}

/// Remembers which local model to run (`tripppin-llm` picks it up when it restarts).
pub fn choose(tier: &Tier) -> Result<()> {
    let dir = models_dir();
    fs::write(dir.join("chosen"), &tier.id).with_context(|| format!("can't write to {}", dir.display()))
}

/// Another server to talk to instead of the local model: any OpenAI-style chat API (a cloud
/// provider with a key, or the person's own llama.cpp / Ollama box). Kept in
/// ~/.config/tripppin/ai, readable only by its owner.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Endpoint {
    /// Base URL ending before /chat/completions, e.g. https://api.example.com/v1
    pub url: String,
    pub model: String,
    pub key: String,
}

fn endpoint_file() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("tripppin/ai")
}

pub fn parse_endpoint(text: &str) -> Option<Endpoint> {
    let mut e = Endpoint::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim().to_string();
        match k.trim() {
            "url" => e.url = v.trim_end_matches('/').to_string(),
            "model" => e.model = v,
            "key" => e.key = v,
            _ => {}
        }
    }
    (!e.url.is_empty()).then_some(e)
}

/// The other server, when one is set; `None` means the model on this computer.
pub fn endpoint() -> Option<Endpoint> {
    parse_endpoint(&fs::read_to_string(endpoint_file()).ok()?)
}

/// Saves (or, with `None`, forgets) the other server.
pub fn set_endpoint(e: Option<&Endpoint>) -> Result<()> {
    let file = endpoint_file();
    let Some(e) = e else {
        return match fs::remove_file(&file) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err.into()),
            _ => Ok(()),
        };
    };
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = format!(
        "# Where the AI runs. Written by tripppin settings.\nurl={}\nmodel={}\nkey={}\n",
        e.url, e.model, e.key
    );
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    opts.open(&file)?.write_all(text.as_bytes())?;
    Ok(())
}

/// Asks the other server for one word, to check the address, model and key work.
pub fn test_endpoint(e: &Endpoint) -> Result<()> {
    let body = serde_json::json!({
        "model": e.model,
        "messages": [{ "role": "user", "content": "say ok" }],
        "max_tokens": 5
    });
    let mut req = http().post(&format!("{}/chat/completions", e.url)).config().http_status_as_error(false).build();
    req = req.header("Content-Type", "application/json");
    if !e.key.is_empty() {
        req = req.header("Authorization", &format!("Bearer {}", e.key));
    }
    let mut resp = req.send(body.to_string()).with_context(|| format!("couldn't reach {}", e.url))?;
    match resp.status().as_u16() {
        200 => Ok(()),
        401 | 403 => bail!("the server turned the key down"),
        404 => bail!("nothing answered at {}/chat/completions; check the address and model", e.url),
        status => {
            let text = resp.body_mut().read_to_string().unwrap_or_default();
            Err(server_error(status, &text))
        }
    }
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

/// How to run `tier` on this machine: placement, context and caches sized from its memory
/// and GPU and from the model file's own shape (see `hwdetect::serving`).
pub fn serve_plan(tier: &Tier, model_path: &Path) -> ServePlan {
    let (memory, accel) = match hwdetect::detect() {
        Ok(hw) => (hw.memory_bytes, Accel::from_hardware(&hw, hwdetect::igpu_pool_bytes())),
        // Off Linux (tests, a Mac): assume a 16 GB machine without a GPU.
        Err(_) => (16 << 30, Accel::None),
    };
    let shape = gguf::read_shape(model_path).unwrap_or_else(|e| {
        eprintln!("can't read the model's shape ({e:#}); assuming a large KV cache");
        fallback_shape(tier)
    });
    serving::plan(memory, accel, &shape, tier.size_bytes, tier.context)
}

/// A model we can't read: plain attention on every layer at 160 KiB per token (an 8B model),
/// so the context is sized on the safe side.
fn fallback_shape(tier: &Tier) -> ModelShape {
    ModelShape {
        arch: "unknown".into(),
        layers: 40,
        train_context: tier.context,
        kv_values_full: 40 * 8 * 256,
        ..Default::default()
    }
}

/// The llama.cpp server arguments for a model. Kept separate so it can be tested.
pub fn server_args(model_path: &Path, plan: &ServePlan) -> Vec<String> {
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
        plan.context.to_string(),
        "--jinja".into(),
        // One person at a time: the whole context goes to one conversation.
        "--parallel".into(),
        "1".into(),
        // A q8_0 KV cache takes half the memory of f16; quantizing V needs flash attention.
        "--flash-attn".into(),
        "on".into(),
        "--cache-type-k".into(),
        plan.cache.name().into(),
        "--cache-type-v".into(),
        plan.cache.name().into(),
        // Prompt caching: each turn reads only what's new. Checkpoints let models with recurrent
        // or sliding-window layers (Qwen3.5, Gemma) go back to where the new part starts;
        // cache reuse shifts matching chunks after the harness drops old messages (plain
        // attention only; llama.cpp turns it off where it can't work).
        "--ctx-checkpoints".into(),
        plan.checkpoints.to_string(),
        "--cache-reuse".into(),
        "256".into(),
        "--cache-ram".into(),
        plan.cache_ram_mib.to_string(),
        // The harness compacts the conversation itself; shifting would cut tool calls in half.
        "--no-context-shift".into(),
        // Free the memory when nobody has asked anything for a while.
        "--sleep-idle-seconds".into(),
        IDLE_UNLOAD_SECS.to_string(),
    ];
    match plan.placement {
        // Leave every GPU alone (llvmpipe, Mesa's software Vulkan, would otherwise count as one).
        Placement::Cpu => args.extend(["--device", "none", "--n-gpu-layers", "0", "--fit", "off"].map(String::from)),
        // llama.cpp measures the GPU's free memory and puts as many layers there as fit,
        // leaving the margin free for the desktop.
        Placement::Gpu { margin_mib } => {
            args.extend(["--fit".to_string(), "on".into(), "--fit-target".into(), margin_mib.to_string()])
        }
    }
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
    let path = dir.join(&tier.file);
    let plan = serve_plan(tier, &path);
    let args = server_args(&path, &plan);
    let bin = std::env::var("TRIPPPIN_LLAMA_SERVER").unwrap_or_else(|_| LLAMA_SERVER.to_string());
    eprintln!("starting {} with {}: {}", bin, tier.model, plan.why);
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
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
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

/// Token counts for one model reply, and how full the context is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    /// Tokens the model read for this reply (the whole conversation sent).
    pub prompt: usize,
    /// Of those, how many came from the server's cache rather than being read again.
    pub cached: usize,
    /// Tokens the model wrote.
    pub completion: usize,
    /// The model's context size, when known (0 otherwise).
    pub context: usize,
    /// Writing speed.
    pub tokens_per_sec: f64,
    /// Running totals for the whole session.
    pub session_prompt: usize,
    pub session_completion: usize,
}

/// One step of the task plan the agent keeps in its notebook.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlanStep {
    pub text: String,
    pub done: bool,
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
    /// Token counts from the server (zero when it sent none). `context` and the session
    /// totals are left for the agent to fill in.
    pub usage: Usage,
}

/// What happens while a reply streams in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event<'a> {
    /// The server is reading the prompt: `total` tokens, `cached` of them from its cache,
    /// `done` read so far (llama.cpp only).
    Reading { total: usize, cached: usize, done: usize },
    /// The first token of the answer (or of a tool call) arrived.
    Writing,
    /// A piece of the answer.
    Text(&'a str),
}

/// The local model server's state, from `/props` (which doesn't wake a sleeping server).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServerInfo {
    /// The next request has to load the model first (starting up, or asleep after being idle).
    pub loading: bool,
    /// The context size in tokens.
    pub context: usize,
}

/// Asks the local server how it is. Another server (a cloud key) is assumed ready, with a
/// 32k-token budget: longer prompts cost more and help little.
pub fn server_info() -> ServerInfo {
    if endpoint().is_some() {
        return ServerInfo { loading: false, context: 32768 };
    }
    let url = format!("http://127.0.0.1:{PORT}/props");
    let resp = http()
        .get(&url)
        .config()
        .timeout_global(Some(Duration::from_secs(3)))
        .http_status_as_error(false)
        .build()
        .call();
    let Ok(mut resp) = resp else { return ServerInfo { loading: true, context: 8192 } };
    if resp.status().as_u16() != 200 {
        return ServerInfo { loading: true, context: 8192 };
    }
    let props: serde_json::Value =
        resp.body_mut().read_to_string().ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    parse_props(&props)
}

fn parse_props(props: &serde_json::Value) -> ServerInfo {
    ServerInfo {
        loading: props.get("is_sleeping").and_then(|v| v.as_bool()).unwrap_or(false),
        context: props
            .pointer("/default_generation_settings/n_ctx")
            .and_then(|n| n.as_u64())
            .map_or(8192, |n| n as usize),
    }
}

/// Token counts in a streamed chunk: llama.cpp's `timings` (with its own cache count) or the
/// OpenAI-style `usage` other servers send at the end.
fn read_usage(chunk: &serde_json::Value, usage: &mut Usage) -> bool {
    let n = |v: &serde_json::Value, p: &str| v.pointer(p).and_then(|x| x.as_u64()).map(|x| x as usize);
    let mut found = false;
    if let Some(u) = chunk.get("usage").filter(|u| u.is_object()) {
        usage.prompt = n(u, "/prompt_tokens").unwrap_or(usage.prompt);
        usage.completion = n(u, "/completion_tokens").unwrap_or(usage.completion);
        usage.cached = n(u, "/prompt_tokens_details/cached_tokens").unwrap_or(usage.cached);
        found = true;
    }
    if let Some(t) = chunk.get("timings").filter(|t| t.is_object()) {
        if let (Some(cache), Some(read)) = (n(t, "/cache_n"), n(t, "/prompt_n")) {
            usage.cached = cache;
            usage.prompt = usage.prompt.max(cache + read);
        }
        usage.completion = n(t, "/predicted_n").unwrap_or(usage.completion);
        if let Some(tps) = t.get("predicted_per_second").and_then(|x| x.as_f64()).filter(|x| x.is_finite() && *x > 0.0)
        {
            usage.tokens_per_sec = tps;
        }
        found = true;
    }
    found
}

/// Prompt progress in a streamed chunk (llama.cpp with `return_progress`).
fn read_progress(chunk: &serde_json::Value) -> Option<(usize, usize, usize)> {
    let p = chunk.get("prompt_progress")?;
    let n = |k: &str| p.get(k).and_then(|x| x.as_u64()).map(|x| x as usize);
    Some((n("total")?, n("cache").unwrap_or(0), n("processed").unwrap_or(0)))
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

/// The most tokens one reply may take; the agent keeps this much of the context free.
pub const REPLY_TOKENS: usize = MAX_REPLY_TOKENS as usize;

/// Streams a reply to `messages`, telling `on_event` as the prompt is read and each piece of
/// text arrives. With `tools`, the model may answer with tool calls instead, which come back in
/// `Reply::calls`. Stops early when `cancel` is set.
pub fn stream(
    messages: &[Message],
    tools: Option<&serde_json::Value>,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(Event),
) -> Result<Reply> {
    let msgs: Vec<_> = messages.iter().map(|m| &m.0).collect();
    let remote = endpoint();
    let mut body = serde_json::json!({
        "model": "tripppin",
        "stream": true,
        // Token counts at the end: llama.cpp adds its timings and cache hits.
        "stream_options": { "include_usage": true },
        "messages": msgs,
        // Answers and tool calls don't need the model's long thinking pass.
        "chat_template_kwargs": { "enable_thinking": false },
        // A reply is a few lines or a tool call; anything longer has gone wrong.
        "max_tokens": MAX_REPLY_TOKENS,
        // llama.cpp: report progress while it reads the prompt, and reuse its cache.
        "return_progress": true,
        "cache_prompt": true
    });
    if remote.is_none() {
        let family = Catalog::builtin().ok().and_then(|c| active_tier(&c, &models_dir()).map(|t| t.model.clone()));
        for (k, v) in sampling(family.as_deref().unwrap_or("")) {
            body[k] = v.into();
        }
    }
    if let Some(tools) = tools {
        body["tools"] = tools.clone();
    }
    let url = match &remote {
        // Other servers get the plain OpenAI request; llama.cpp's extra knobs can be refused.
        Some(e) => {
            if let Some(map) = body.as_object_mut() {
                for k in ["chat_template_kwargs", "top_k", "min_p", "return_progress", "cache_prompt"] {
                    map.remove(k);
                }
            }
            body["model"] = e.model.clone().into();
            format!("{}/chat/completions", e.url)
        }
        None => format!("http://127.0.0.1:{PORT}/v1/chat/completions"),
    };
    let start = Instant::now();
    let mut request = http().post(&url).config().http_status_as_error(false).build();
    if let Some(e) = remote.as_ref().filter(|e| !e.key.is_empty()) {
        request = request.header("Authorization", &format!("Bearer {}", e.key));
    }
    let mut resp = match request.header("Content-Type", "application/json").send(body.to_string()) {
        Ok(r) => r,
        Err(ureq::Error::Io(_)) | Err(ureq::Error::ConnectionFailed) if remote.is_none() => return Err(unavailable()),
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
    let mut usage = Usage::default();
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
        if first.is_none()
            && let Some((total, cached, done)) = read_progress(&chunk)
        {
            on_event(Event::Reading { total, cached, done });
        }
        read_usage(&chunk, &mut usage);
        let piece = delta_text(&chunk);
        let called = merge_tool_calls(&chunk, &mut calls);
        if (piece.is_some() || called) && first.is_none() {
            first = Some(start.elapsed());
            on_event(Event::Writing);
        }
        if let Some(piece) = piece {
            pieces += 1;
            on_event(Event::Text(&piece));
            text.push_str(&piece);
        }
        if called {
            pieces += 1;
        }
    }
    let measured =
        first.map_or(0.0, |t: Duration| f64::from(pieces) / start.elapsed().saturating_sub(t).as_secs_f64().max(0.001));
    let tokens_per_sec = if usage.tokens_per_sec > 0.0 { usage.tokens_per_sec } else { measured };
    usage.tokens_per_sec = tokens_per_sec;
    Ok(Reply { text, cut_off, calls, first_word: first, tokens_per_sec, cancelled, usage })
}

/// Each model family's recommended sampling for answers without a thinking pass. Qwen's presence
/// penalty keeps small models from repeating themselves; Gemma and Granite do better without.
pub fn sampling(model: &str) -> Vec<(&'static str, f64)> {
    let m = model.to_lowercase();
    if m.starts_with("gemma") {
        vec![("temperature", 1.0), ("top_p", 0.95), ("top_k", 64.0), ("min_p", 0.0)]
    } else if m.starts_with("granite") {
        vec![("temperature", 0.0)]
    } else {
        vec![("temperature", 0.7), ("top_p", 0.8), ("top_k", 20.0), ("min_p", 0.0), ("presence_penalty", 1.5)]
    }
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

/// Sends one question and prints the streamed answer (`trip ask`, for scripts and testing).
pub fn ask(question: &str) -> Result<()> {
    let mut out = io::stdout().lock();
    let reply = stream(&[Message::system(), Message::user(question)], None, &AtomicBool::new(false), |e| {
        if let Event::Text(t) = e {
            let _ = write!(out, "{t}");
            let _ = out.flush();
        }
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
    fn samples_each_family_its_own_way() {
        assert!(sampling("Qwen3.5 4B").contains(&("presence_penalty", 1.5)));
        assert!(sampling("Gemma 4 E4B").contains(&("top_k", 64.0)));
        assert_eq!(sampling("Granite 4.1 8B"), [("temperature", 0.0)]);
    }

    #[test]
    fn reads_an_endpoint() {
        let e = parse_endpoint("# x\nurl=https://api.example.com/v1/\nmodel=big\nkey=sk-1\n").unwrap();
        assert_eq!(e, Endpoint { url: "https://api.example.com/v1".into(), model: "big".into(), key: "sk-1".into() });
        assert!(parse_endpoint("model=big").is_none());
    }

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

    fn plan_for(placement: Placement, context: u32) -> ServePlan {
        ServePlan {
            placement,
            context,
            cache: serving::CacheType::Q8_0,
            cache_ram_mib: 1024,
            checkpoints: 8,
            estimate_bytes: 0,
            why: String::new(),
        }
    }

    #[test]
    fn server_stays_local_and_uses_the_planned_context() {
        let args = server_args(Path::new("/m/x.gguf"), &plan_for(Placement::Gpu { margin_mib: 2048 }, 32768));
        let joined = args.join(" ");
        assert!(joined.contains("--host 127.0.0.1"));
        assert!(joined.contains("--ctx-size 32768"));
        assert!(joined.contains("--model /m/x.gguf"));
        assert!(joined.contains("--flash-attn on --cache-type-k q8_0 --cache-type-v q8_0"));
        assert!(joined.contains("--ctx-checkpoints 8 --cache-reuse 256 --cache-ram 1024"));
        assert!(joined.contains("--fit on --fit-target 2048"));
        assert!(!joined.contains("--n-gpu-layers"), "llama.cpp fits the layers on a GPU");
        let cpu = server_args(Path::new("/m/x.gguf"), &plan_for(Placement::Cpu, 16384)).join(" ");
        assert!(cpu.contains("--device none --n-gpu-layers 0 --fit off"));
    }

    #[test]
    fn the_tier_context_is_the_most_a_plan_asks_for() {
        let catalog = Catalog::builtin().unwrap();
        let tier = catalog.tiers.iter().find(|t| t.id == "cpu-16").unwrap();
        // No model file here, so the safe fallback shape is used.
        let plan = serve_plan(tier, Path::new("/nonexistent.gguf"));
        assert!(plan.context <= tier.context && plan.context >= serving::MIN_CONTEXT);
    }

    #[test]
    fn reads_usage_from_llama_cpp_and_cloud_servers() {
        let mut u = Usage::default();
        let llama = serde_json::json!({
            "choices": [],
            "usage": { "prompt_tokens": 5000, "completion_tokens": 40, "prompt_tokens_details": { "cached_tokens": 4800 } },
            "timings": { "cache_n": 4800, "prompt_n": 200, "predicted_n": 40, "predicted_per_second": 18.5 }
        });
        assert!(read_usage(&llama, &mut u));
        assert_eq!((u.prompt, u.cached, u.completion, u.tokens_per_sec), (5000, 4800, 40, 18.5));
        let mut u = Usage::default();
        let cloud = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 900, "completion_tokens": 12 } });
        assert!(read_usage(&cloud, &mut u));
        assert_eq!((u.prompt, u.cached, u.completion), (900, 0, 12));
        assert!(!read_usage(&serde_json::json!({"choices":[{"delta":{"content":"x"}}]}), &mut u));
        assert!(!read_usage(&serde_json::json!({"usage": null}), &mut u));
    }

    #[test]
    fn reads_prompt_progress_and_props() {
        let chunk = serde_json::json!({"choices":[],"prompt_progress":{"total":3000,"cache":2000,"processed":2500,"time_ms":10}});
        assert_eq!(read_progress(&chunk), Some((3000, 2000, 2500)));
        assert_eq!(read_progress(&serde_json::json!({"choices":[]})), None);
        let props = serde_json::json!({"default_generation_settings":{"n_ctx":32768},"is_sleeping":true});
        assert_eq!(parse_props(&props), ServerInfo { loading: true, context: 32768 });
        assert_eq!(parse_props(&serde_json::json!({})), ServerInfo { loading: false, context: 8192 });
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
