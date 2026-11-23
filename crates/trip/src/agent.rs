//! The agent loop: the model asks for tools, trip checks each call, runs it and sends the
//! result back, until the model answers in plain text.
//!
//! The work is recorded in a [`Session`]: each step, its outcome and the end of its output.
//! The session's notebook (the request, the model's own notes and one line per step) is shown to
//! the model at every step, so small models with small context windows keep track of the task
//! even after old messages are dropped to make room.
//!
//! Every call goes through `verify` first. A call with an unknown tool, arguments that aren't
//! JSON, or missing or mistyped fields isn't run; the model gets the exact problem back and
//! another try, up to `MAX_RETRIES` times in a row.

use crate::jobs;
use crate::model::{self, ContextFull, Message, ToolCall};
use crate::session::Session;
use crate::term::{ACCENT, DIM, RESET};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const MAX_STEPS: usize = 40;
const MAX_RETRIES: usize = 3;
/// How long exec waits before a command carries on in the background.
const DEFAULT_WAIT_SECS: u64 = 60;
/// How long job_wait waits by default.
const DEFAULT_JOB_WAIT_SECS: u64 = 120;
const MAX_WAIT_SECS: u64 = 1800;
/// How much of a file goes back to the model. The rest is cut from the middle.
const MAX_RESULT_CHARS: usize = 4000;
const MAX_READ_BYTES: u64 = 1 << 20;
/// The same exec command more often than this in one task is refused, so the model can't loop.
const MAX_REPEATS: usize = 2;
/// A rough, deliberately low estimate of characters per token, for budgeting the context.
const CHARS_PER_TOKEN: usize = 3;
/// Context kept free for the tool definitions and the model's reply, in tokens.
const CONTEXT_RESERVE: usize = 3000;
/// Room for the notebook, which is added to what the model sees, in characters.
const NOTEBOOK_RESERVE: usize = 3000;
const TRIMMED: &str = "[earlier output removed to save space; run the command again if you need it]";

/// The tools the model can call, as JSON schemas.
pub fn tools() -> Value {
    let job_id = json!({ "type": "integer", "description": "The job number exec gave you." });
    json!([
        {
            "type": "function",
            "function": {
                "name": "exec",
                "description": "Run a bash command on this computer. Waits up to wait_secs (default 60) for it to \
    finish and returns the exit code and the newest output. If it's still running then, it carries on as a background \
    job and you get its job number; use job_wait, job_output or job_stop with it. Long output is cut to the newest \
    lines; the full log's path is given so you can grep it.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "The bash command to run." },
                        "wait_secs": {
                            "type": "integer",
                            "description": "How long to wait before letting it run in the background. Default 60."
                        },
                        "background": {
                            "type": "boolean",
                            "description": "Start it in the background right away and return its job number, for things \
    that run a long time or forever (servers, big downloads). It gets no keyboard input."
                        }
                    },
                    "required": ["command"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "job_wait",
                "description": "Wait for a background job to finish (up to wait_secs, default 120) and get its new output.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "job": job_id,
                        "wait_secs": { "type": "integer", "description": "The most to wait. Default 120." }
                    },
                    "required": ["job"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "job_output",
                "description": "Check on a background job without waiting: whether it's running and its new output.",
                "parameters": { "type": "object", "properties": { "job": job_id }, "required": ["job"] }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "job_stop",
                "description": "Stop a background job.",
                "parameters": { "type": "object", "properties": { "job": job_id }, "required": ["job"] }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "note",
                "description": "Write a short note in your task notebook: your plan, a fact you found, what worked or \
    what failed. The notebook is shown to you at every step, even after old messages are dropped, so note anything \
    you'll need later. Start a task of more than two steps by noting your plan.",
                "parameters": {
                    "type": "object",
                    "properties": { "text": { "type": "string", "description": "The note, one or two lines." } },
                    "required": ["text"]
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

const TOOL_NAMES: &str = "exec, job_wait, job_output, job_stop, note, read_file and write_file";

/// A tool call that passed the checks.
#[derive(Debug, PartialEq)]
pub enum Action {
    Exec { command: String, wait_secs: u64, background: bool },
    JobWait { job: usize, wait_secs: u64 },
    JobOutput { job: usize },
    JobStop { job: usize },
    Note { text: String },
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
    let seconds = |key: &str, default: u64| -> Result<u64, String> {
        let bad = || format!("\"{key}\" must be a whole number of seconds, 1 or more.");
        match args.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(Value::Number(n)) => match n.as_f64() {
                Some(t) if t >= 1.0 => Ok((t as u64).min(MAX_WAIT_SECS)),
                _ => Err(bad()),
            },
            Some(Value::String(s)) => match s.trim().parse::<u64>() {
                Ok(t) if t >= 1 => Ok(t.min(MAX_WAIT_SECS)),
                _ => Err(bad()),
            },
            Some(other) => Err(format!("\"{key}\" must be a number, but you sent {}.", kind(other))),
        }
    };
    let job = || -> Result<usize, String> {
        let bad = |what: &str| format!("\"job\" for {name} must be the job number exec gave you, but you sent {what}.");
        match args.get("job") {
            None => Err(format!("{name} needs a \"job\" field with the job number, for example {{\"job\": 1}}.")),
            Some(Value::Number(n)) => {
                n.as_u64().filter(|&n| n > 0).map(|n| n as usize).ok_or_else(|| bad("that number"))
            }
            Some(Value::String(s)) => {
                s.trim().trim_start_matches(['#', 'j', 'o', 'b', ' ']).parse::<usize>().map_err(|_| bad("text"))
            }
            Some(other) => Err(bad(kind(other))),
        }
    };
    match name {
        "exec" => {
            let command = text("command", false)?;
            // timeout_secs is what the first version called wait_secs; small models remember it.
            let wait_secs = if args.contains_key("wait_secs") {
                seconds("wait_secs", DEFAULT_WAIT_SECS)?
            } else {
                seconds("timeout_secs", DEFAULT_WAIT_SECS)?
            };
            let background = match args.get("background") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(Value::String(s)) if s == "true" || s == "false" => s == "true",
                Some(other) => {
                    return Err(format!("\"background\" must be true or false, but you sent {}.", kind(other)));
                }
            };
            Ok(Action::Exec { command, wait_secs, background })
        }
        "job_wait" => Ok(Action::JobWait { job: job()?, wait_secs: seconds("wait_secs", DEFAULT_JOB_WAIT_SECS)? }),
        "job_output" => Ok(Action::JobOutput { job: job()? }),
        "job_stop" => Ok(Action::JobStop { job: job()? }),
        "note" => Ok(Action::Note { text: text("text", false)? }),
        "read_file" => Ok(Action::ReadFile { path: text("path", false)? }),
        "write_file" => Ok(Action::WriteFile { path: text("path", false)?, content: text("content", true)? }),
        "" => Err(format!("The tool call has no tool name. The tools are {TOOL_NAMES}.")),
        other => Err(format!("There's no tool called \"{other}\". The tools are {TOOL_NAMES}.")),
    }
}

fn example(tool: &str) -> &'static str {
    match tool {
        "read_file" => r#"{"path": "~/notes.txt"}"#,
        "write_file" => r#"{"path": "~/notes.txt", "content": "hello"}"#,
        "job_wait" | "job_output" | "job_stop" => r#"{"job": 1}"#,
        "note" => r#"{"text": "plan: 1. find the package 2. install it 3. check it runs"}"#,
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

/// Where the agent's progress goes: the terminal, or the orb's window.
pub trait Ui {
    /// A piece of the model's answer, as it streams.
    fn text(&mut self, piece: &str);
    /// A tool call starting: what kind, and what it works on (one line).
    fn step(&mut self, label: &str, detail: &str);
    /// A line of output from a running command.
    fn output(&mut self, line: &str);
    /// Something the person should know that isn't the answer.
    fn note(&mut self, text: &str);
    /// The step that just ran finished, well or not. Not called while a job keeps running.
    fn step_done(&mut self, _ok: bool) {}
    /// The model is working out its next move (no output to show yet).
    fn thinking(&mut self) {}
}

/// Progress printed to the terminal, the way the AI shell shows it.
pub struct Terminal;

impl Ui for Terminal {
    fn text(&mut self, piece: &str) {
        let mut out = io::stdout();
        let _ = write!(out, "{piece}");
        let _ = out.flush();
    }
    fn step(&mut self, label: &str, detail: &str) {
        println!("{ACCENT}▸{RESET} {DIM}{label}{RESET} {detail}");
    }
    fn output(&mut self, line: &str) {
        println!("{DIM}  │ {line}{RESET}");
    }
    fn note(&mut self, text: &str) {
        println!("{DIM}{text}{RESET}");
    }
}

/// How a task ended.
pub struct Done {
    pub stats: Option<String>,
    /// The model's final answer.
    pub answer: String,
}

/// Works on the session's newest request until the model answers without calling a tool.
/// The model's text streams to `ui`; each tool call shows as a step, with its output. The
/// session records every step and is saved as it goes; a request that fails leaves no
/// half-finished tool turns in the conversation.
pub fn run(session: &mut Session, cancel: &AtomicBool, ui: &mut dyn Ui) -> Result<Done> {
    let result = work(session, cancel, ui);
    match &result {
        Ok(done) => session.finish(Ok(&done.answer)),
        Err(e) => {
            session.finish(Err(&format!("{e:#}")));
            session.forget_unfinished();
            session.save();
        }
    }
    result
}

fn work(session: &mut Session, cancel: &AtomicBool, ui: &mut dyn Ui) -> Result<Done> {
    let tools = tools();
    let budget = (model::context_tokens().saturating_sub(CONTEXT_RESERVE).max(2000) * CHARS_PER_TOKEN)
        .saturating_sub(NOTEBOOK_RESERVE);
    let mut runs: HashMap<String, usize> = HashMap::new();
    let mut retries = 0;
    for step in 0..MAX_STEPS {
        if compact(session, budget) {
            ui.note("memory nearly full: older steps were dropped; the task notebook keeps track of them");
        }
        let reply = match ask(session, &tools, cancel, ui) {
            Err(e) if e.downcast_ref::<ContextFull>().is_some() => {
                // The estimate was off; keep only the newest steps and try once more.
                ui.note("the conversation got too long for the model's memory; dropping older steps");
                compact(session, 0);
                ask(session, &tools, cancel, ui).map_err(|e| match e.downcast_ref::<ContextFull>() {
                    Some(_) => anyhow::anyhow!("this task got too long for the model's memory. try a smaller step."),
                    None => e,
                })?
            }
            other => other?,
        };
        if !reply.text.is_empty() && !reply.text.ends_with('\n') {
            ui.text("\n");
        }
        if reply.cancelled {
            bail!("stopped");
        }
        if reply.cut_off {
            // A runaway reply; its half-written tool calls can't be trusted.
            bail!("the model's reply ran on too long, so it was cut off. try asking again, more specifically.");
        }
        let history = &mut session.history;
        if reply.calls.is_empty() {
            history.push(Message::assistant(&reply.text));
            if looks_like_text_call(&reply.text) {
                retries += 1;
                if retries > MAX_RETRIES {
                    bail!("the model kept writing tool calls it couldn't send properly. try rephrasing.");
                }
                ui.note(&format!(
                    "that tool call came out malformed; asking for a fixed one ({retries}/{MAX_RETRIES})"
                ));
                history.push(Message::user(FIX_TEXT_CALL));
                continue;
            }
            return Ok(Done { stats: reply.stats(), answer: reply.text });
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
                    ui.note(&format!("not running `{}` again; it already ran {MAX_REPEATS} times", one_line(&command)));
                    format!(
                        "Not run: you've already run this exact command {MAX_REPEATS} times in this task. \
Don't repeat it. Try a different approach, or tell the person what's blocking you."
                    )
                }
                Ok(Action::Note { text }) => {
                    show_step(ui, "note", &text);
                    session.step_started("note", &text);
                    session.step_finished(Some(true), "");
                    session.notes.push(one_line_full(&text));
                    ui.step_done(true);
                    "Noted. It's in your notebook.".to_string()
                }
                Ok(action) => {
                    let (label, detail) = describe_action(&action);
                    session.step_started(&label, &detail);
                    session.save();
                    let result = perform(&action, cancel, ui);
                    let ok = outcome(&result);
                    if let Some(ok) = ok {
                        ui.step_done(ok);
                    }
                    session.step_finished(ok, &result);
                    result
                }
                Err(problem) => {
                    broken = true;
                    ui.note(&format!("bad {} call: {problem}", if call.name.is_empty() { "tool" } else { &call.name }));
                    format!("Error: this call was not run. {problem} Fix the call and send it again.")
                }
            };
            session.history.push(Message::tool(&call.id, &result));
            session.save();
            if cancel.load(Ordering::Relaxed) {
                bail!("stopped");
            }
        }
        if broken {
            retries += 1;
            if retries > MAX_RETRIES {
                bail!("the model kept sending broken tool calls ({MAX_RETRIES} retries). try rephrasing.");
            }
            ui.note(&format!("asking the model to fix its call ({retries}/{MAX_RETRIES})"));
        } else {
            retries = 0;
        }
    }
    bail!("stopped after {MAX_STEPS} steps without finishing. ask again to keep going.")
}

/// One model turn, streamed to the terminal.
/// Whether a tool result means the step worked: `None` while its job is still running.
fn outcome(result: &str) -> Option<bool> {
    if result.contains("is still running") {
        None
    } else if result.starts_with("Error") || result.contains("was stopped") || result.contains("ended by a signal") {
        Some(false)
    } else if let Some(rest) = result.split("exit code ").nth(1) {
        Some(rest.starts_with("0 ") || rest.starts_with("0."))
    } else {
        Some(true)
    }
}

/// Sends the conversation with the notebook added to its newest message, so the model always
/// sees its request, notes and steps right before it decides (the notebook isn't kept in the
/// history, so the conversation itself stays unchanged and cacheable).
fn ask(session: &Session, tools: &Value, cancel: &AtomicBool, ui: &mut dyn Ui) -> Result<model::Reply> {
    ui.thinking();
    let mut messages = session.history.clone();
    let has_work = session.turns.last().is_some_and(|t| !t.steps.is_empty()) || !session.notes.is_empty();
    if has_work && let Some(last) = messages.last_mut() {
        let content = last.0["content"].as_str().unwrap_or("").to_string();
        last.0["content"] = Value::String(format!("{content}\n\n---\n{}", session.notebook()));
    }
    model::stream(&messages, Some(tools), cancel, |piece| ui.text(piece))
}

fn bump(runs: &mut HashMap<String, usize>, command: &str) -> usize {
    let n = runs.entry(command.trim().to_string()).or_default();
    *n += 1;
    *n
}

/// The first line, whole (a note).
fn one_line_full(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn one_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("");
    if line.chars().count() > 60 {
        format!("{}…", line.chars().take(60).collect::<String>())
    } else {
        line.to_string()
    }
}

/// Keeps the conversation under `budget` characters, in three passes, stopping as soon as it fits:
/// 1. earlier requests keep only what was asked and answered (their steps go);
/// 2. old command output is replaced with a short note (the newest result always stays);
/// 3. the current request's oldest steps go altogether, keeping the newest two.
///
/// Every step stays in the session's notebook, which the model sees each time. Returns whether
/// whole steps were dropped (passes 1 and 3).
fn compact(session: &mut Session, budget: usize) -> bool {
    let size = |h: &[Message]| h.iter().map(|m| m.0.to_string().len()).sum::<usize>();
    if size(&session.history) <= budget {
        return false;
    }
    let start = session.turns.last().map_or(1, |t| t.start).min(session.history.len());
    let mut dropped = false;

    // 1. Earlier requests: only the questions and the final answers.
    let before = session.history.len();
    let mut kept = Vec::with_capacity(before);
    for (i, m) in session.history.drain(..).enumerate() {
        let role = m.0["role"].as_str().unwrap_or("");
        if i == 0 || i >= start || role == "user" || (role == "assistant" && m.0.get("tool_calls").is_none()) {
            kept.push(m);
        }
    }
    let removed = before - kept.len();
    session.history = kept;
    let start = start - removed;
    if let Some(t) = session.turns.last_mut() {
        t.start = start;
    }
    dropped |= removed > 0;

    // 2. Old output.
    let history = &mut session.history;
    let mut total = size(history);
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

    // 3. The current request's oldest steps (a call and its results go together).
    while total > budget {
        let groups: Vec<usize> = (start + 1..history.len())
            .filter(|&i| history[i].0["role"] == "assistant" && history[i].0.get("tool_calls").is_some())
            .collect();
        if groups.len() <= 2 {
            break;
        }
        let from = groups[0];
        let to = groups[1];
        let gone: usize = history[from..to].iter().map(|m| m.0.to_string().len()).sum();
        history.drain(from..to);
        total -= gone;
        dropped = true;
    }
    if dropped {
        session.compactions += 1;
    }
    dropped
}

/// Shows the step on one line, cut to a readable length.
fn show_step(ui: &mut dyn Ui, tool: &str, detail: &str) {
    let line = detail.lines().next().unwrap_or("");
    let more = line.chars().count() > 100 || detail.lines().nth(1).is_some();
    let line: String = line.chars().take(100).collect();
    ui.step(tool, &format!("{line}{}", if more { "…" } else { "" }));
}

/// What a step is and what it works on, as the history shows it.
fn describe_action(action: &Action) -> (String, String) {
    match action {
        Action::Exec { command, background, .. } => {
            ((if *background { "exec in the background" } else { "exec" }).into(), command.clone())
        }
        Action::JobWait { job, .. } => (format!("waiting on job {job}"), jobs::command(*job).unwrap_or_default()),
        Action::JobOutput { job } => (format!("checking job {job}"), jobs::command(*job).unwrap_or_default()),
        Action::JobStop { job } => (format!("stopping job {job}"), jobs::command(*job).unwrap_or_default()),
        Action::Note { text } => ("note".into(), text.clone()),
        Action::ReadFile { path } => ("read".into(), path.clone()),
        Action::WriteFile { path, .. } => ("write".into(), path.clone()),
    }
}

fn perform(action: &Action, cancel: &AtomicBool, ui: &mut dyn Ui) -> String {
    match action {
        Action::Note { .. } => "Noted.".into(),
        Action::Exec { command, wait_secs, background } => {
            show_step(ui, if *background { "exec in the background" } else { "exec" }, command);
            let id = match jobs::start(command, !*background) {
                Ok(id) => id,
                Err(e) => return format!("Error: {e}"),
            };
            let wait = if *background { Duration::ZERO } else { Duration::from_secs(*wait_secs) };
            let result = jobs::wait(id, wait, cancel, &mut |line| ui.output(line));
            if !jobs::finished(id) && !*background {
                ui.note(&format!("  still running; carrying on in the background as job {id}"));
            }
            result
        }
        Action::JobWait { job, wait_secs } => {
            show_step(ui, &format!("waiting on job {job}"), &jobs::command(*job).unwrap_or_default());
            jobs::wait(*job, Duration::from_secs(*wait_secs), cancel, &mut |line| ui.output(line))
        }
        Action::JobOutput { job } => {
            show_step(ui, &format!("checking job {job}"), &jobs::command(*job).unwrap_or_default());
            jobs::output(*job)
        }
        Action::JobStop { job } => {
            show_step(ui, &format!("stopping job {job}"), &jobs::command(*job).unwrap_or_default());
            jobs::stop(*job)
        }
        Action::ReadFile { path } => {
            show_step(ui, "read", path);
            read_file(path)
        }
        Action::WriteFile { path, content } => {
            show_step(ui, "write", path);
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
    let mut session = Session::new("terminal");
    session.ask(task);
    let done = run(&mut session, &cancel, &mut Terminal)?;
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
            Ok(Action::Exec { command: "ls".into(), wait_secs: DEFAULT_WAIT_SECS, background: false })
        );
        assert_eq!(
            verify(&call("exec", r#"{"command":"sleep 1","timeout_secs":"30"}"#)),
            Ok(Action::Exec { command: "sleep 1".into(), wait_secs: 30, background: false })
        );
        assert_eq!(
            verify(&call("exec", r#"{"command":"x","wait_secs":99999,"background":true}"#)),
            Ok(Action::Exec { command: "x".into(), wait_secs: MAX_WAIT_SECS, background: true })
        );
        assert_eq!(verify(&call("job_wait", r#"{"job":2}"#)), Ok(Action::JobWait { job: 2, wait_secs: 120 }));
        assert_eq!(verify(&call("job_output", r##"{"job":"#3"}"##)), Ok(Action::JobOutput { job: 3 }));
        assert_eq!(verify(&call("job_stop", r#"{"job":"job 4"}"#)), Ok(Action::JobStop { job: 4 }));
        assert_eq!(verify(&call("read_file", r#"{"path":"~/a"}"#)), Ok(Action::ReadFile { path: "~/a".into() }));
        assert_eq!(verify(&call("note", r#"{"text":"plan"}"#)), Ok(Action::Note { text: "plan".into() }));
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
        assert!(err("job_wait", "{}").contains("needs a \"job\" field"));
        assert!(err("job_stop", r#"{"job": 0}"#).contains("job number"));
        assert!(err("exec", r#"{"command": "ls", "background": 1}"#).contains("true or false"));
        assert!(err("note", "{}").contains("needs a \"text\" field"));
    }

    #[test]
    fn tells_success_from_failure() {
        assert_eq!(outcome("Job 1 finished with exit code 0 after 2 s. Output:\nok"), Some(true));
        assert_eq!(
            outcome("Job 1 finished with exit code 1 after 0 s. Output:\nsudo: a password is required"),
            Some(false)
        );
        assert_eq!(outcome("Job 2 is still running (61 s so far)."), None);
        assert_eq!(outcome("Error: can't read /x"), Some(false));
        assert_eq!(outcome("Wrote 5 bytes to a."), Some(true));
    }

    fn session_with(messages: Vec<Message>) -> Session {
        let mut s = Session::new("test");
        s.history = messages;
        s.turns.push(crate::session::Turn { request: "task".into(), start: 1, ..Default::default() });
        s
    }

    #[test]
    fn compacts_old_tool_output_first() {
        let big = "x".repeat(5000);
        let mut s = session_with(vec![
            Message::system(),
            Message::user("task"),
            Message::tool("a", &big),
            Message::tool("b", &big),
            Message::tool("c", &big),
        ]);
        assert!(!compact(&mut s, 8000), "only output was trimmed");
        let h = &s.history;
        assert_eq!(h[2].0["content"], TRIMMED);
        assert_eq!(h[3].0["content"], TRIMMED);
        assert_eq!(h[4].0["content"], big.as_str(), "the newest output stays");
        assert_eq!(h[1].0["content"], "task", "the request stays");
        let mut small = session_with(vec![Message::user("hi"), Message::tool("a", "ok")]);
        compact(&mut small, 10_000);
        assert_eq!(small.history[1].0["content"], "ok");
    }

    #[test]
    fn compacts_whole_steps_when_output_is_not_enough() {
        let call = |id: &str| ToolCall { id: id.into(), name: "exec".into(), arguments: "x".repeat(3000) };
        let mut h = vec![
            Message::system(),
            Message::user("earlier question"),
            Message::assistant_calls("", &[call("old")]),
            Message::tool("old", "old output"),
            Message::assistant("earlier answer"),
            Message::user("task"),
        ];
        for id in ["a", "b", "c", "d"] {
            h.push(Message::assistant_calls("", &[call(id)]));
            h.push(Message::tool(id, "ok"));
        }
        let mut s = session_with(h);
        s.turns[0].start = 5;
        assert!(compact(&mut s, 8000));
        let roles: Vec<String> = s.history.iter().map(|m| m.0["role"].as_str().unwrap().to_string()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user", "assistant", "tool", "assistant", "tool"]);
        assert_eq!(s.history[3].0["content"], "task");
        assert_eq!(s.turns[0].start, 3);
        assert_eq!(s.history[5].0["tool_call_id"], "c", "the newest two steps stay");
        assert_eq!(s.compactions, 1);
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
