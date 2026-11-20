//! The agent loop: the model asks for tools, trip checks each call, runs it and sends the
//! result back, until the model answers in plain text.
//!
//! Every call goes through `verify` first. A call with an unknown tool, arguments that aren't
//! JSON, or missing or mistyped fields isn't run; the model gets the exact problem back and
//! another try, up to `MAX_RETRIES` times in a row.

use crate::model::{self, ContextFull, Message, ToolCall};
use crate::shell::{ACCENT, DIM, RESET};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const MAX_STEPS: usize = 25;
const MAX_RETRIES: usize = 3;
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 1800;
/// How much of a tool's output goes back to the model. The rest is cut from the middle.
const MAX_RESULT_CHARS: usize = 4000;
const MAX_READ_BYTES: u64 = 1 << 20;
/// The same exec command more often than this in one task is refused, so the model can't loop.
const MAX_REPEATS: usize = 2;
/// A rough, deliberately low estimate of characters per token, for budgeting the context.
const CHARS_PER_TOKEN: usize = 3;
/// Context kept free for the tool definitions and the model's reply, in tokens.
const CONTEXT_RESERVE: usize = 3000;
const TRIMMED: &str = "[earlier output removed to save space; run the command again if you need it]";

/// The tools the model can call, as JSON schemas.
pub fn tools() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "exec",
                "description": "Run a bash command on this computer and get its output and exit code. \
    Use it for anything: installing apps, checking the system, downloading, managing files.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "The bash command to run." },
                        "timeout_secs": {
                            "type": "integer",
                            "description": "Stop the command after this many seconds. Default 120; use more for big installs or downloads."
                        }
                    },
                    "required": ["command"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a text file.",
                "parameters": {
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": "Path to the file; ~ is the home folder." } },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Create or replace a text file with the given content. Creates missing folders.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the file; ~ is the home folder." },
                        "content": { "type": "string", "description": "The full new content of the file." }
                    },
                    "required": ["path", "content"]
                }
            }
        }
    ])
}

/// A tool call that passed the checks.
#[derive(Debug, PartialEq)]
pub enum Action {
    Exec { command: String, timeout_secs: u64 },
    ReadFile { path: String },
    WriteFile { path: String, content: String },
}

/// Checks a call's structure. The error is written for the model: what's wrong and what to send.
pub fn verify(call: &ToolCall) -> Result<Action, String> {
    let name = call.name.trim();
    let raw = if call.arguments.trim().is_empty() { "{}" } else { call.arguments.as_str() };
    let args: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            return Err(format!(
                "The arguments for {name} aren't valid JSON ({e}). Send them as one JSON object, for example {}.",
                example(name)
            ));
        }
    };
    let Some(args) = args.as_object() else {
        return Err(format!("The arguments for {name} must be a JSON object, for example {}.", example(name)));
    };
    let text = |key: &str, allow_empty: bool| -> Result<String, String> {
        match args.get(key) {
            None => Err(format!("{name} needs a \"{key}\" field. For example: {}.", example(name))),
            Some(Value::String(s)) if !allow_empty && s.trim().is_empty() => {
                Err(format!("\"{key}\" for {name} is empty; it must say what to use."))
            }
            Some(Value::String(s)) => Ok(s.clone()),
            Some(other) => Err(format!("\"{key}\" for {name} must be a string, but you sent {}.", kind(other))),
        }
    };
    match name {
        "exec" => {
            let command = text("command", false)?;
            let timeout_secs = match args.get("timeout_secs") {
                None | Some(Value::Null) => DEFAULT_TIMEOUT_SECS,
                Some(Value::Number(n)) => match n.as_f64() {
                    Some(t) if t >= 1.0 => (t as u64).min(MAX_TIMEOUT_SECS),
                    _ => return Err("\"timeout_secs\" must be a whole number of seconds, 1 or more.".into()),
                },
                Some(Value::String(s)) => match s.trim().parse::<u64>() {
                    Ok(t) if t >= 1 => t.min(MAX_TIMEOUT_SECS),
                    _ => return Err("\"timeout_secs\" must be a whole number of seconds, 1 or more.".into()),
                },
                Some(other) => return Err(format!("\"timeout_secs\" must be a number, but you sent {}.", kind(other))),
            };
            Ok(Action::Exec { command, timeout_secs })
        }
        "read_file" => Ok(Action::ReadFile { path: text("path", false)? }),
        "write_file" => Ok(Action::WriteFile { path: text("path", false)?, content: text("content", true)? }),
        "" => Err("The tool call has no tool name. The tools are exec, read_file and write_file.".into()),
        other => Err(format!("There's no tool called \"{other}\". The tools are exec, read_file and write_file.")),
    }
}

fn example(tool: &str) -> &'static str {
    match tool {
        "read_file" => r#"{"path": "~/notes.txt"}"#,
        "write_file" => r#"{"path": "~/notes.txt", "content": "hello"}"#,
        _ => r#"{"command": "ls -la ~"}"#,
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "true/false",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// A reply that tried to call a tool but wrote the call as plain text, so llama.cpp couldn't parse it.
fn looks_like_text_call(text: &str) -> bool {
    let t = text.trim_start();
    t.contains("<tool_call>") || t.contains("<function=") || (t.starts_with('{') && t.contains("\"name\""))
}

const FIX_TEXT_CALL: &str = "Your last reply looks like a tool call written as text, so nothing ran. \
Call the tool through the tool interface with valid JSON arguments, or answer in plain words if you're done.";

/// How a task ended.
pub struct Done {
    pub stats: Option<String>,
}

/// Works on the conversation in `history` until the model answers without calling a tool.
/// The model's text streams to the terminal; each tool call shows as a line, with its output.
pub fn run(history: &mut Vec<Message>, cancel: &AtomicBool) -> Result<Done> {
    let tools = tools();
    let budget = model::context_tokens().saturating_sub(CONTEXT_RESERVE).max(2000) * CHARS_PER_TOKEN;
    let mut runs: HashMap<String, usize> = HashMap::new();
    let mut retries = 0;
    for step in 0..MAX_STEPS {
        compact(history, budget);
        let reply = match ask(history, &tools, cancel) {
            Err(e) if e.downcast_ref::<ContextFull>().is_some() => {
                // The estimate was off; keep only the newest output and try once more.
                note("the conversation got too long for the model's memory; trimming old output");
                compact(history, 0);
                ask(history, &tools, cancel).map_err(|e| match e.downcast_ref::<ContextFull>() {
                    Some(_) => anyhow::anyhow!("this task got too long for the model's memory. try a smaller step."),
                    None => e,
                })?
            }
            other => other?,
        };
        if !reply.text.is_empty() && !reply.text.ends_with('\n') {
            println!();
        }
        if reply.cancelled {
            bail!("stopped");
        }
        if reply.calls.is_empty() {
            history.push(Message::assistant(&reply.text));
            if looks_like_text_call(&reply.text) {
                retries += 1;
                if retries > MAX_RETRIES {
                    bail!("the model kept writing tool calls it couldn't send properly. try rephrasing.");
                }
                note(&format!("that tool call came out malformed; asking for a fixed one ({retries}/{MAX_RETRIES})"));
                history.push(Message::user(FIX_TEXT_CALL));
                continue;
            }
            return Ok(Done { stats: reply.stats() });
        }
        let mut calls = reply.calls;
        for (i, call) in calls.iter_mut().enumerate() {
            if call.id.is_empty() {
                call.id = format!("call_{step}_{i}");
            }
        }
        history.push(Message::assistant_calls(&reply.text, &calls));
        let mut broken = false;
        for call in &calls {
            let result = match verify(call) {
                Ok(Action::Exec { command, .. }) if bump(&mut runs, &command) > MAX_REPEATS => {
                    note(&format!("not running `{}` again; it already ran {MAX_REPEATS} times", one_line(&command)));
                    format!(
                        "Not run: you've already run this exact command {MAX_REPEATS} times in this task. \
Don't repeat it. Try a different approach, or tell the person what's blocking you."
                    )
                }
                Ok(action) => perform(&action, cancel),
                Err(problem) => {
                    broken = true;
                    note(&format!("bad {} call: {problem}", if call.name.is_empty() { "tool" } else { &call.name }));
                    format!("Error: this call was not run. {problem} Fix the call and send it again.")
                }
            };
            history.push(Message::tool(&call.id, &result));
            if cancel.load(Ordering::Relaxed) {
                bail!("stopped");
            }
        }
        if broken {
            retries += 1;
            if retries > MAX_RETRIES {
                bail!("the model kept sending broken tool calls ({MAX_RETRIES} retries). try rephrasing.");
            }
            note(&format!("asking the model to fix its call ({retries}/{MAX_RETRIES})"));
        } else {
            retries = 0;
        }
    }
    bail!("stopped after {MAX_STEPS} steps without finishing. ask again to keep going.")
}

/// One model turn, streamed to the terminal.
fn ask(history: &[Message], tools: &Value, cancel: &AtomicBool) -> Result<model::Reply> {
    let mut out = io::stdout();
    model::stream(history, Some(tools), cancel, |piece| {
        let _ = write!(out, "{piece}");
        let _ = out.flush();
    })
}

fn bump(runs: &mut HashMap<String, usize>, command: &str) -> usize {
    let n = runs.entry(command.trim().to_string()).or_default();
    *n += 1;
    *n
}

fn one_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("");
    if line.chars().count() > 60 {
        format!("{}…", line.chars().take(60).collect::<String>())
    } else {
        line.to_string()
    }
}

/// Keeps the conversation under `budget` characters by replacing the oldest tool outputs with a
/// short note. The newest tool result always stays, so the model can see what just happened.
fn compact(history: &mut [Message], budget: usize) {
    let size = |h: &[Message]| h.iter().map(|m| m.0.to_string().len()).sum::<usize>();
    let mut total = size(history);
    if total <= budget {
        return;
    }
    let newest = history.iter().rposition(|m| m.0["role"] == "tool");
    for (i, msg) in history.iter_mut().enumerate() {
        if total <= budget {
            break;
        }
        let m = &mut msg.0;
        if m["role"] != "tool" || Some(i) == newest || m["content"] == TRIMMED {
            continue;
        }
        let before = m.to_string().len();
        m["content"] = Value::String(TRIMMED.into());
        total = total - before + m.to_string().len();
    }
}

fn note(text: &str) {
    println!("{DIM}{text}{RESET}");
}

/// Shows the step on one line, cut to a readable length.
fn show_step(tool: &str, detail: &str) {
    let line = detail.lines().next().unwrap_or("");
    let more = line.chars().count() > 100 || detail.lines().nth(1).is_some();
    let line: String = line.chars().take(100).collect();
    println!("{ACCENT}▸{RESET} {DIM}{tool}{RESET} {line}{}", if more { "…" } else { "" });
}

fn perform(action: &Action, cancel: &AtomicBool) -> String {
    match action {
        Action::Exec { command, timeout_secs } => {
            show_step("exec", command);
            exec(command, Duration::from_secs(*timeout_secs), cancel)
        }
        Action::ReadFile { path } => {
            show_step("read", path);
            read_file(path)
        }
        Action::WriteFile { path, content } => {
            show_step("write", path);
            write_file(path, content)
        }
    }
}

fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ if path == "~" => std::env::var_os("HOME").map_or_else(|| PathBuf::from(path), PathBuf::from),
        _ => PathBuf::from(path),
    }
}

fn read_file(path: &str) -> String {
    let p = expand(path);
    match std::fs::metadata(&p) {
        Err(e) => return format!("Error: can't read {path}: {e}"),
        Ok(m) if m.is_dir() => return format!("Error: {path} is a folder; use exec with ls to list it."),
        Ok(m) if m.len() > MAX_READ_BYTES => {
            return format!(
                "Error: {path} is {} bytes, too big to read whole; use exec with head, tail or grep.",
                m.len()
            );
        }
        Ok(_) => {}
    }
    match std::fs::read(&p) {
        Ok(bytes) => clip(&String::from_utf8_lossy(&bytes)),
        Err(e) => format!("Error: can't read {path}: {e}"),
    }
}

fn write_file(path: &str, content: &str) -> String {
    let p = expand(path);
    if let Some(parent) = p.parent().filter(|d| !d.as_os_str().is_empty())
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return format!("Error: can't create the folder {}: {e}", parent.display());
    }
    match std::fs::write(&p, content) {
        Ok(()) => format!("Wrote {} bytes to {path}.", content.len()),
        Err(e) => format!("Error: can't write {path}: {e}"),
    }
}

/// Runs a command with bash. Output shows in the terminal as it happens and is collected for the
/// model. The keyboard stays connected, so sudo can ask for a password.
fn exec(command: &str, timeout: Duration, cancel: &AtomicBool) -> String {
    let child = Command::new("bash")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TRIPPPIN_IN_SHELL", "1")
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return format!("Error: couldn't start bash: {e}"),
    };
    let (tx, rx) = mpsc::channel::<String>();
    let readers: Vec<Box<dyn Read + Send>> = [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .collect();
    for stream in readers {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            let mut buf = Vec::new();
            while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string();
                if tx.send(line).is_err() {
                    break;
                }
                buf.clear();
            }
        });
    }
    drop(tx);

    let start = Instant::now();
    let mut output = String::new();
    let mut stopped: Option<&str> = None;
    let mut killed_at: Option<Instant> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                println!("{DIM}  │ {line}{RESET}");
                // Keep memory bounded on very chatty commands; the middle gets cut anyway.
                if output.len() < 4 * MAX_RESULT_CHARS * 16 {
                    output.push_str(&line);
                    output.push('\n');
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if stopped.is_none() {
            if cancel.load(Ordering::Relaxed) {
                stopped = Some("stopped by the person (Ctrl-C)");
            } else if start.elapsed() > timeout {
                stopped = Some("timed out");
            }
            if stopped.is_some() {
                let _ = child.kill();
                killed_at = Some(Instant::now());
            }
        }
        // Something it started in the background can keep the output open; don't wait on it forever.
        if killed_at.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            break;
        }
        if stopped.is_none() && child.try_wait().ok().flatten().is_some() && rx_idle(&rx, &mut output) {
            break;
        }
    }
    let status = child.wait();
    let head = match (stopped, status) {
        (Some("timed out"), _) => format!("The command timed out after {} s and was stopped.", timeout.as_secs()),
        (Some(why), _) => format!("The command was {why}."),
        (None, Ok(s)) => match s.code() {
            Some(code) => format!("Exit code {code}."),
            None => "The command was ended by a signal.".to_string(),
        },
        (None, Err(e)) => format!("Couldn't get the command's result: {e}"),
    };
    if output.trim().is_empty() { format!("{head} No output.") } else { format!("{head} Output:\n{}", clip(&output)) }
}

/// After the command exits, drains what's left. Returns true once the output is closed or quiet.
fn rx_idle(rx: &mpsc::Receiver<String>, output: &mut String) -> bool {
    loop {
        match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(line) => {
                println!("{DIM}  │ {line}{RESET}");
                output.push_str(&line);
                output.push('\n');
            }
            Err(_) => return true,
        }
    }
}

/// Cuts long text from the middle, keeping the start and the end (where errors usually are).
fn clip(text: &str) -> String {
    let count = text.chars().count();
    if count <= MAX_RESULT_CHARS {
        return text.to_string();
    }
    let head_n = MAX_RESULT_CHARS / 3;
    let tail_n = MAX_RESULT_CHARS - head_n;
    let head: String = text.chars().take(head_n).collect();
    let tail: String = text.chars().skip(count - tail_n).collect();
    format!("{head}\n… ({} characters cut) …\n{tail}", count - head_n - tail_n)
}

/// `trip do <task>`: one task without the interactive shell, for scripts and testing.
pub fn once(task: &str) -> Result<()> {
    let cancel = AtomicBool::new(false);
    let mut history = vec![Message::system(), Message::user(task)];
    let done = run(&mut history, &cancel)?;
    if let Some(stats) = done.stats {
        eprintln!("{DIM}{stats}{RESET}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &str) -> ToolCall {
        ToolCall { id: "c".into(), name: name.into(), arguments: args.into() }
    }

    #[test]
    fn accepts_well_formed_calls() {
        assert_eq!(
            verify(&call("exec", r#"{"command":"ls"}"#)),
            Ok(Action::Exec { command: "ls".into(), timeout_secs: DEFAULT_TIMEOUT_SECS })
        );
        assert_eq!(
            verify(&call("exec", r#"{"command":"sleep 1","timeout_secs":"30"}"#)),
            Ok(Action::Exec { command: "sleep 1".into(), timeout_secs: 30 })
        );
        assert_eq!(
            verify(&call("exec", r#"{"command":"x","timeout_secs":99999}"#)),
            Ok(Action::Exec { command: "x".into(), timeout_secs: MAX_TIMEOUT_SECS })
        );
        assert_eq!(verify(&call("read_file", r#"{"path":"~/a"}"#)), Ok(Action::ReadFile { path: "~/a".into() }));
        assert_eq!(
            verify(&call("write_file", r#"{"path":"a","content":""}"#)),
            Ok(Action::WriteFile { path: "a".into(), content: String::new() })
        );
    }

    #[test]
    fn explains_broken_calls() {
        let err = |name, args| verify(&call(name, args)).unwrap_err();
        assert!(err("exec", r#"{"command": "ls""#).contains("aren't valid JSON"));
        assert!(err("exec", "[1]").contains("must be a JSON object"));
        assert!(err("exec", "{}").contains("needs a \"command\" field"));
        assert!(err("exec", r#"{"command": 5}"#).contains("must be a string, but you sent a number"));
        assert!(err("exec", r#"{"command": "  "}"#).contains("is empty"));
        assert!(err("exec", r#"{"command": "ls", "timeout_secs": -3}"#).contains("whole number"));
        assert!(err("run_shell", r#"{"command": "ls"}"#).contains("no tool called \"run_shell\""));
        assert!(err("", "{}").contains("no tool name"));
        assert!(err("write_file", r#"{"path": "a"}"#).contains("needs a \"content\" field"));
    }

    #[test]
    fn compacts_old_tool_output_first() {
        let big = "x".repeat(5000);
        let mut h = vec![
            Message::system(),
            Message::user("task"),
            Message::tool("a", &big),
            Message::tool("b", &big),
            Message::tool("c", &big),
        ];
        compact(&mut h, 8000);
        assert_eq!(h[2].0["content"], TRIMMED);
        assert_eq!(h[3].0["content"], TRIMMED);
        assert_eq!(h[4].0["content"], big.as_str(), "the newest output stays");
        assert_eq!(h[1].0["content"], "task", "the request stays");
        let mut small = vec![Message::user("hi"), Message::tool("a", "ok")];
        compact(&mut small, 10_000);
        assert_eq!(small[1].0["content"], "ok");
    }

    #[test]
    fn counts_repeated_commands() {
        let mut runs = HashMap::new();
        assert_eq!(bump(&mut runs, "ls"), 1);
        assert_eq!(bump(&mut runs, " ls "), 2);
        assert_eq!(bump(&mut runs, "ls"), 3);
        assert_eq!(bump(&mut runs, "pwd"), 1);
    }

    #[test]
    fn spots_tool_calls_written_as_text() {
        assert!(looks_like_text_call("<tool_call>\n{\"name\": \"exec\"}"));
        assert!(looks_like_text_call(r#"{"name": "exec", "arguments": {"command": "ls"}}"#));
        assert!(!looks_like_text_call("firefox is installed."));
    }

    #[test]
    fn clips_long_output_from_the_middle() {
        let long = format!("start{}end", "x".repeat(MAX_RESULT_CHARS * 2));
        let clipped = clip(&long);
        assert!(clipped.starts_with("start") && clipped.ends_with("end"));
        assert!(clipped.contains("characters cut"));
        assert_eq!(clip("short"), "short");
    }

    #[cfg(unix)]
    #[test]
    fn runs_commands_and_reports_exit_codes() {
        let cancel = AtomicBool::new(false);
        let ok = exec("echo hi; echo oops >&2", Duration::from_secs(10), &cancel);
        assert!(ok.starts_with("Exit code 0.") && ok.contains("hi") && ok.contains("oops"), "{ok}");
        let fail = exec("exit 3", Duration::from_secs(10), &cancel);
        assert_eq!(fail, "Exit code 3. No output.");
        let slow = exec("sleep 5", Duration::from_secs(1), &cancel);
        assert!(slow.contains("timed out"), "{slow}");
    }

    #[test]
    fn reads_and_writes_files() {
        let dir = std::env::temp_dir().join(format!("trip-agent-{}", std::process::id()));
        let path = dir.join("sub/note.txt");
        let path = path.to_str().unwrap();
        assert!(write_file(path, "hello").starts_with("Wrote 5 bytes"));
        assert_eq!(read_file(path), "hello");
        assert!(read_file(dir.to_str().unwrap()).contains("is a folder"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
