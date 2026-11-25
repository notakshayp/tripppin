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
use crate::session::{Dropped, Session};
use crate::tui::Terminal;
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
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
/// Identical calls of other tools (read the same file, the same note) allowed before refusing.
const MAX_SAME_CALL: usize = 3;
/// Steps one request may take with a long plan.
const MAX_PLAN_TOTAL: usize = 150;
/// Characters per token before the server has counted a prompt: deliberately low.
const START_CHARS_PER_TOKEN: f64 = 3.0;
/// Compaction starts when the prompt fills this share of the room left after the reply…
const COMPACT_AT_PERCENT: usize = 85;
/// …and goes down to this share, so it runs rarely and the server's cache stays useful.
const COMPACT_TO_PERCENT: usize = 55;
/// Steps without the plan moving before the model is reminded; twice this and it must stop.
const STALL_STEPS: usize = 8;
/// The newest output is cut to this many characters when nothing else frees enough room.
const SHORT_RESULT_CHARS: usize = 1500;
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
                "name": "plan",
                "description": "Write the plan for a task of more than two steps: short steps, each one thing you can \
    do and check. Your notebook shows the plan and which step you're on. Call it again to rewrite the plan when it \
    isn't working; steps you keep word for word stay done.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "steps": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "The steps in order, a few words each. At most 12."
                        }
                    },
                    "required": ["steps"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "done",
                "description": "Mark a plan step done, once you've checked it worked (for example by reading the file \
    back or running a command that shows the change). Then go on to the next step.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "step": { "type": "integer", "description": "The step's number in the plan." },
                        "check": { "type": "string", "description": "How you checked it worked, in one line." }
                    },
                    "required": ["step", "check"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "note",
                "description": "Write a short note in your task notebook: a fact you found, what worked or what \
    failed. The notebook is shown to you at every step, even after old messages are dropped, so note anything \
    you'll need later.",
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

const TOOL_NAMES: &str = "exec, job_wait, job_output, job_stop, plan, done, note, read_file and write_file";

/// A tool call that passed the checks.
#[derive(Debug, PartialEq)]
pub enum Action {
    Exec { command: String, wait_secs: u64, background: bool },
    JobWait { job: usize, wait_secs: u64 },
    JobOutput { job: usize },
    JobStop { job: usize },
    Plan { steps: Vec<String> },
    Done { step: usize, check: String },
    Note { text: String },
    ReadFile { path: String },
    WriteFile { path: String, content: String },
}

/// Plan steps as a small model may send them: a list, or one string with a step per line
/// ("1. find it\n2. fix it"). Numbering and bullets are dropped.
fn plan_steps(value: Option<&Value>) -> Result<Vec<String>, String> {
    let clean = |s: &str| {
        let s = s.trim();
        let digits = s.trim_start_matches(|c: char| c.is_ascii_digit());
        let s = match digits.strip_prefix(['.', ')']) {
            Some(rest) if digits.len() < s.len() => rest,
            _ => s.strip_prefix(['-', '*', '•']).unwrap_or(s),
        };
        one_line_full(s)
    };
    let steps: Vec<String> = match value {
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::String(s) => Ok(clean(s)),
                other => Err(format!("each step must be a string, but one was {}.", kind(other))),
            })
            .collect::<Result<_, _>>()?,
        Some(Value::String(s)) => s.lines().map(clean).collect(),
        None => return Err(format!("plan needs a \"steps\" field. For example: {}.", example("plan"))),
        Some(other) => return Err(format!("\"steps\" must be a list of strings, but you sent {}.", kind(other))),
    };
    let steps: Vec<String> = steps.into_iter().filter(|s| !s.is_empty()).collect();
    if steps.is_empty() {
        return Err(format!("the plan has no steps. For example: {}.", example("plan")));
    }
    if steps.len() > crate::session::MAX_PLAN_STEPS {
        return Err(format!(
            "that's {} steps; keep the plan to {} or fewer bigger ones.",
            steps.len(),
            crate::session::MAX_PLAN_STEPS
        ));
    }
    Ok(steps.into_iter().map(|s| s.chars().take(160).collect()).collect())
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
        "plan" => Ok(Action::Plan { steps: plan_steps(args.get("steps"))? }),
        "done" => {
            let bad =
                |what: &str| format!("\"step\" for done must be the step's number in the plan, but you sent {what}.");
            let step = match args.get("step") {
                None => return Err(format!("done needs a \"step\" field. For example: {}.", example("done"))),
                Some(Value::Number(n)) => {
                    n.as_u64().filter(|&n| n > 0).map(|n| n as usize).ok_or_else(|| bad("that number"))?
                }
                Some(Value::String(s)) => {
                    s.trim().trim_start_matches("step").trim().parse::<usize>().map_err(|_| bad("text"))?
                }
                Some(other) => return Err(bad(kind(other))),
            };
            Ok(Action::Done { step, check: text("check", false)? })
        }
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
        "note" => r#"{"text": "the theme is set in ~/.config/gtk-4.0/settings.ini"}"#,
        "plan" => r#"{"steps": ["find the package", "install it", "check it runs"]}"#,
        "done" => r#"{"step": 1, "check": "flatpak list shows org.videolan.VLC"}"#,
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
    /// What the agent is waiting on right now, in a few words ("loading the model",
    /// "reading 3,200 tokens", "writing"), so a quiet moment never looks like a hang.
    fn waiting(&mut self, _what: &str) {}
    /// Token counts after each model reply.
    fn usage(&mut self, _usage: &crate::model::Usage) {}
    /// The task plan changed: the steps and which are done.
    fn plan(&mut self, _steps: &[crate::model::PlanStep]) {}
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
    let tools_chars = tools.to_string().len();
    let mut budget = Budget::new(model::server_info().context);
    let mut watch = Watch::default();
    let mut runs: HashMap<String, usize> = HashMap::new();
    let mut retries = 0;
    if !session.plan.is_empty() {
        ui.plan(&session.plan);
    }
    let mut step = 0;
    while step < max_steps(session) {
        step += 1;
        let answer_now = watch.must_answer();
        if answer_now && !watch.told_to_stop {
            watch.told_to_stop = true;
            ui.note("the model seems stuck; asking it to say where it got to");
        }
        let hints = watch.hints(session);
        let offered = (!answer_now).then_some(&tools);
        let fixed = prompt_chars(&prompt(session, &hints)) - prompt_chars(&session.history)
            + offered.map_or(0, |_| tools_chars);
        let estimate = budget.tokens(prompt_chars(&session.history) + fixed);
        if estimate > budget.compact_at() {
            shrink(session, ui, &budget, budget.compact_to(), fixed);
        }
        let reply = match ask(session, &hints, offered, cancel, ui, &mut budget) {
            Err(e) if e.downcast_ref::<ContextFull>().is_some() => {
                // The estimate was off; keep only the newest steps and try once more.
                ui.note("the conversation got too long for the model's memory; dropping older steps");
                shrink(session, ui, &budget, 0, fixed);
                ask(session, &hints, offered, cancel, ui, &mut budget).map_err(|e| {
                    match e.downcast_ref::<ContextFull>() {
                        Some(_) => {
                            anyhow::anyhow!("this task got too long for the model's memory. try a smaller step.")
                        }
                        None => e,
                    }
                })?
            }
            other => other?,
        };
        report_usage(session, ui, &budget, &reply);
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
            if looks_like_text_call(&reply.text) && !answer_now {
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
                    watch.step(Some(false));
                    format!(
                        "Not run: you've already run this exact command {MAX_REPEATS} times in this task. \
Don't repeat it. Try a different approach, or tell the person what's blocking you."
                    )
                }
                Ok(action) if repeats(&action) && bump(&mut runs, &signature(call)) > MAX_SAME_CALL => {
                    ui.note(&format!("not repeating {} again; the model is going round in circles", call.name));
                    watch.step(Some(false));
                    format!(
                        "Not done: you've made this exact {} call {MAX_SAME_CALL} times already. It won't help \
again. Take the next step of your plan, rewrite the plan, or tell the person what's blocking you.",
                        call.name
                    )
                }
                Ok(Action::Plan { steps }) => {
                    show_step(ui, "plan", &steps.join(" · "));
                    session.step_started("plan", &steps.join("; "));
                    if session.set_plan(&steps) {
                        watch.progress();
                        ui.plan(&session.plan);
                    }
                    session.step_finished(Some(true), "");
                    ui.step_done(true);
                    match session.current_step() {
                        Some((n, s)) => format!(
                            "Plan saved ({} steps). Now step {n}: {}. Do just that, check it worked, then call done \
with {n}.",
                            session.plan.len(),
                            s.text
                        ),
                        None => "Plan saved; every step in it is already done. Check the result, then answer.".into(),
                    }
                }
                Ok(Action::Done { step: n, check }) => {
                    show_step(ui, "done", &format!("step {n}: {check}"));
                    session.step_started("done", &format!("step {n}: {check}"));
                    let result = session.finish_step(n, &check);
                    session.step_finished(Some(result.is_ok()), result.as_ref().err().map_or("", |e| e.as_str()));
                    ui.step_done(result.is_ok());
                    match result {
                        Ok(next) => {
                            watch.progress();
                            ui.plan(&session.plan);
                            next
                        }
                        Err(problem) => format!("Error: {problem}"),
                    }
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
                    watch.step(ok);
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
    bail!("stopped after {step} steps without finishing. ask again to keep going.")
}

/// Steps one request may take: 40, or more for a long plan (12 a step, up to 150).
fn max_steps(session: &Session) -> usize {
    if session.plan.is_empty() { MAX_STEPS } else { (session.plan.len() * 12 + 10).clamp(MAX_STEPS, MAX_PLAN_TOTAL) }
}

/// Calls that are pointless to repeat word for word (waiting on a job is not).
fn repeats(action: &Action) -> bool {
    !matches!(action, Action::JobWait { .. } | Action::JobOutput { .. } | Action::Exec { .. })
}

/// A call's name and arguments, the same however the JSON was spaced.
fn signature(call: &ToolCall) -> String {
    let args =
        serde_json::from_str::<Value>(&call.arguments).map_or_else(|_| call.arguments.clone(), |v| v.to_string());
    format!("{} {args}", call.name.trim())
}

/// How full the model's context is allowed to get, in the server's own tokens.
struct Budget {
    /// The server's context size.
    context: usize,
    /// Characters (of the JSON we send) per token, learnt from the server's counts.
    chars_per_token: f64,
    /// The last prompt's size as the server counted it.
    last_prompt: usize,
}

impl Budget {
    fn new(context: usize) -> Self {
        Budget { context, chars_per_token: START_CHARS_PER_TOKEN, last_prompt: 0 }
    }

    /// Room for the prompt, after keeping enough free for the longest reply.
    fn room(&self) -> usize {
        self.context.saturating_sub(model::REPLY_TOKENS + 256).max(2000)
    }

    /// Compaction starts above this many tokens…
    fn compact_at(&self) -> usize {
        self.room() * COMPACT_AT_PERCENT / 100
    }

    /// …and goes down to this many, so it doesn't run again (and throw away the server's
    /// cache) on the very next step.
    fn compact_to(&self) -> usize {
        self.room() * COMPACT_TO_PERCENT / 100
    }

    fn tokens(&self, chars: usize) -> usize {
        (chars as f64 / self.chars_per_token).ceil() as usize
    }

    fn chars(&self, tokens: usize) -> usize {
        (tokens as f64 * self.chars_per_token) as usize
    }

    /// Learns the ratio from a prompt of `chars` that the server counted as `tokens`.
    fn learn(&mut self, chars: usize, tokens: usize) {
        if tokens >= 200 {
            self.chars_per_token = (chars as f64 / tokens as f64).clamp(1.5, 8.0);
        }
        self.last_prompt = tokens;
    }
}

/// Signs that the model is stuck, and what to tell it.
#[derive(Default)]
struct Watch {
    /// Steps that ran something since the plan last moved (written, rewritten, a step done).
    since_progress: usize,
    /// Steps that ran something, in all.
    steps: usize,
    failures_in_row: usize,
    told_to_stop: bool,
}

impl Watch {
    fn step(&mut self, ok: Option<bool>) {
        self.steps += 1;
        self.since_progress += 1;
        match ok {
            Some(false) => self.failures_in_row += 1,
            Some(true) => self.failures_in_row = 0,
            None => {}
        }
    }

    fn progress(&mut self) {
        self.since_progress = 0;
    }

    /// Stuck for good: the next reply gets no tools, so it has to explain.
    fn must_answer(&self) -> bool {
        self.since_progress >= 2 * STALL_STEPS || self.failures_in_row >= 5
    }

    /// Reminders for the notebook, from what's happened so far.
    fn hints(&self, session: &Session) -> Vec<String> {
        let mut out = Vec::new();
        if self.must_answer() {
            out.push(
                "Stop here; no more tools. Tell the person in a few lines what you did, what's left and what's \
blocking you, and ask how to go on."
                    .to_string(),
            );
            return out;
        }
        if session.plan.is_empty() && self.steps >= 3 {
            out.push(
                "This task is taking several steps. Write a short plan with the plan tool, then work through it one \
step at a time."
                    .to_string(),
            );
        }
        if let Some((n, s)) = session.current_step().filter(|_| self.since_progress >= STALL_STEPS) {
            out.push(format!(
                "You've taken {} steps since the plan last moved, and step {n} ({}) isn't done. If it is done, call \
done with how you checked. If it's stuck, rewrite the plan with plan, or stop and tell the person what's blocking \
you.",
                self.since_progress, s.text
            ));
        }
        if self.failures_in_row >= 3 {
            out.push(format!(
                "The last {} steps failed. Don't try the same thing again: find another way (rewrite the plan) or stop \
and ask the person.",
                self.failures_in_row
            ));
        }
        out
    }
}

/// Tells the UI the server's token counts, with the context size and the session's totals.
fn report_usage(session: &mut Session, ui: &mut dyn Ui, budget: &Budget, reply: &model::Reply) {
    session.prompt_tokens += reply.usage.prompt as u64;
    session.completion_tokens += reply.usage.completion as u64;
    ui.usage(&model::Usage {
        context: budget.context,
        session_prompt: session.prompt_tokens as usize,
        session_completion: session.completion_tokens as usize,
        ..reply.usage.clone()
    });
}

/// Compacts the conversation down to `target` tokens (plus `fixed` characters of tools and
/// notebook) and records what went.
fn shrink(session: &mut Session, ui: &mut dyn Ui, budget: &Budget, target: usize, fixed: usize) {
    let before = budget.tokens(prompt_chars(&session.history) + fixed);
    let Some(mut dropped) = compact(session, budget.chars(target).saturating_sub(fixed)) else { return };
    dropped.tokens_before = before;
    dropped.tokens_after = budget.tokens(prompt_chars(&session.history) + fixed);
    let mut what = Vec::new();
    if !dropped.steps.is_empty() {
        what.push(format!("{} old steps", dropped.steps.len()));
    }
    if !dropped.requests.is_empty() {
        what.push(format!("{} earlier requests", dropped.requests.len()));
    }
    if dropped.outputs_trimmed > 0 {
        what.push(format!("{} old outputs", dropped.outputs_trimmed));
    }
    ui.note(&format!(
        "memory nearly full ({} of {} tokens): dropped {}; the task notebook keeps track of them",
        before,
        budget.context,
        what.join(", ")
    ));
    session.dropped.push(dropped);
    session.save();
}

/// The conversation's size as sent, in characters of JSON.
fn prompt_chars(messages: &[Message]) -> usize {
    messages.iter().map(|m| m.0.to_string().len()).sum()
}

/// What the model is sent: the conversation, then the notebook as a message of its own, so
/// the conversation before it is the same from one step to the next and the server reads only
/// what's new (with a hybrid model like Qwen3.5 it can only go back to where a message
/// starts). Right after a request, the notebook joins that message so roles still alternate.
fn prompt(session: &Session, hints: &[String]) -> Vec<Message> {
    let mut messages = session.history.clone();
    let has_work = session.turns.last().is_some_and(|t| !t.steps.is_empty())
        || !session.notes.is_empty()
        || !session.plan.is_empty()
        || !hints.is_empty();
    if !has_work {
        return messages;
    }
    let notebook = session.notebook_with(hints);
    match messages.last_mut() {
        Some(last) if last.0["role"] == "user" => {
            let content = last.0["content"].as_str().unwrap_or("").to_string();
            last.0["content"] = Value::String(format!("{content}\n\n---\n{notebook}"));
        }
        _ => messages.push(Message::user(&notebook)),
    }
    messages
}

/// One model turn: says what it's waiting on, streams the text to `ui` and learns the
/// characters-per-token ratio from the server's count.
fn ask(
    session: &Session,
    hints: &[String],
    tools: Option<&Value>,
    cancel: &AtomicBool,
    ui: &mut dyn Ui,
    budget: &mut Budget,
) -> Result<model::Reply> {
    ui.thinking();
    let messages = prompt(session, hints);
    let chars = prompt_chars(&messages) + tools.map_or(0, |t| t.to_string().len());
    if model::server_info().loading {
        ui.waiting("loading the model");
    } else {
        // Roughly what's new since the last request; the server's progress says exactly.
        let new = budget.tokens(chars).saturating_sub(budget.last_prompt).max(1);
        ui.waiting(&format!("reading {} tokens", thousands(new)));
    }
    let reply = model::stream(&messages, tools, cancel, |event| match event {
        model::Event::Reading { total, cached, done } => {
            let todo = total.saturating_sub(cached).max(1);
            let read = done.saturating_sub(cached).min(todo);
            ui.waiting(&if read == 0 {
                format!("reading {} tokens", thousands(todo))
            } else {
                format!("reading {} tokens ({}%)", thousands(todo), read * 100 / todo)
            });
        }
        model::Event::Writing => ui.waiting("writing"),
        model::Event::Text(piece) => ui.text(piece),
    })?;
    if reply.usage.prompt > 0 {
        budget.learn(chars, reply.usage.prompt);
    }
    Ok(reply)
}

/// 3200 → "3,200".
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

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

/// One line per tool call in an assistant message ("exec ls -la"), for the record of what was dropped.
fn describe_calls(message: &Message) -> Vec<String> {
    let calls = message.0.get("tool_calls").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    calls
        .iter()
        .map(|c| {
            let name = c.pointer("/function/name").and_then(|n| n.as_str()).unwrap_or("?");
            let args = c.pointer("/function/arguments").and_then(|a| a.as_str()).unwrap_or("");
            let parsed: Value = serde_json::from_str(args).unwrap_or_default();
            let detail = ["command", "path", "text", "job", "step"]
                .iter()
                .find_map(|k| parsed.get(*k).map(|v| v.as_str().map_or_else(|| v.to_string(), String::from)))
                .unwrap_or_default();
            format!("{name} {}", one_line(&detail)).trim().to_string()
        })
        .collect()
}

/// Keeps the conversation under `budget` characters, in steps, stopping as soon as it fits:
/// 1. earlier requests keep only what was asked and answered (their steps go);
/// 2. old command output is replaced with a short note (the newest result always stays);
/// 3. the current request's oldest steps go altogether, keeping the newest two;
/// 4. earlier requests go altogether, oldest first (the notebook lists the last few);
/// 5. the newest output is cut short.
///
/// Every step stays in the session's notebook, which the model sees each time. Returns what
/// went, or `None` when nothing needed to.
fn compact(session: &mut Session, budget: usize) -> Option<Dropped> {
    let size = |h: &[Message]| h.iter().map(|m| m.0.to_string().len()).sum::<usize>();
    if size(&session.history) <= budget {
        return None;
    }
    let mut report = Dropped { turn: session.turns.len().saturating_sub(1), ..Default::default() };
    let mut start = session.turns.last().map_or(1, |t| t.start).min(session.history.len());

    // 1. Earlier requests: only the questions and the final answers.
    let before = session.history.len();
    let mut kept = Vec::with_capacity(before);
    for (i, m) in session.history.drain(..).enumerate() {
        let role = m.0["role"].as_str().unwrap_or("");
        if i == 0 || i >= start || role == "user" || (role == "assistant" && m.0.get("tool_calls").is_none()) {
            kept.push(m);
        } else {
            report.steps.extend(describe_calls(&m));
        }
    }
    start -= before - kept.len();
    session.history = kept;

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
        report.outputs_trimmed += 1;
    }

    // 3. The current request's oldest steps (a call and its results go together).
    while total > budget {
        let groups: Vec<usize> = (start + 1..history.len())
            .filter(|&i| history[i].0["role"] == "assistant" && history[i].0.get("tool_calls").is_some())
            .collect();
        if groups.len() <= 2 {
            break;
        }
        let (from, to) = (groups[0], groups[1]);
        report.steps.extend(describe_calls(&history[from]));
        total -= history.drain(from..to).map(|m| m.0.to_string().len()).sum::<usize>();
    }

    // 4. Earlier requests, oldest first: a question and everything up to the next one.
    while total > budget && start > 1 {
        let end = (2..start).find(|&i| history[i].0["role"] == "user").unwrap_or(start);
        let request = history[1].0["content"].as_str().unwrap_or("").to_string();
        report.requests.push(one_line(&request));
        total -= history.drain(1..end).map(|m| m.0.to_string().len()).sum::<usize>();
        start -= end - 1;
        session.forgotten += 1;
    }

    // 5. The newest output, cut short.
    if total > budget
        && let Some(last) = history.iter_mut().rev().find(|m| m.0["role"] == "tool")
    {
        let content = last.0["content"].as_str().unwrap_or("").to_string();
        if content.chars().count() > SHORT_RESULT_CHARS {
            let tail: String = content.chars().skip(content.chars().count() - SHORT_RESULT_CHARS).collect();
            last.0["content"] = Value::String(format!("[cut to its end to save space]\n…{tail}"));
            report.outputs_trimmed += 1;
        }
    }

    if let Some(t) = session.turns.last_mut() {
        t.start = start;
    }
    if report.steps.is_empty() && report.requests.is_empty() && report.outputs_trimmed == 0 {
        return None;
    }
    session.compactions += 1;
    Some(report)
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
        Action::Plan { steps } => ("plan".into(), steps.join("; ")),
        Action::Done { step, check } => ("done".into(), format!("step {step}: {check}")),
        Action::ReadFile { path } => ("read".into(), path.clone()),
        Action::WriteFile { path, .. } => ("write".into(), path.clone()),
    }
}

fn perform(action: &Action, cancel: &AtomicBool, ui: &mut dyn Ui) -> String {
    match action {
        // Handled by the agent loop itself, which keeps the notebook.
        Action::Note { .. } | Action::Plan { .. } | Action::Done { .. } => "Noted.".into(),
        Action::Exec { command, wait_secs, background } => {
            show_step(ui, if *background { "exec in the background" } else { "exec" }, command);
            ui.waiting(&format!("running {}", one_line(command)));
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
            ui.waiting(&format!("running {}", one_line(&jobs::command(*job).unwrap_or_else(|| format!("job {job}")))));
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
    let mut ui = Terminal::new(None);
    let done = run(&mut session, &cancel, &mut ui);
    ui.finish(done.as_ref().ok().and_then(|d| d.stats.as_deref()));
    done.map(|_| ())
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
            Message(serde_json::json!({ "role": "system", "content": "s".repeat(4000) })),
            Message::user("task"),
            Message::tool("a", &big),
            Message::tool("b", &big),
            Message::tool("c", &big),
        ]);
        let d = compact(&mut s, 11000).unwrap();
        assert_eq!((d.outputs_trimmed, d.steps.len()), (2, 0), "only output was trimmed");
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
        let d = compact(&mut s, 10_500).unwrap();
        assert_eq!(d.steps, ["exec", "exec", "exec"], "the earlier request's step and the two oldest current ones");
        let roles: Vec<String> = s.history.iter().map(|m| m.0["role"].as_str().unwrap().to_string()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user", "assistant", "tool", "assistant", "tool"]);
        assert_eq!(s.history[3].0["content"], "task");
        assert_eq!(s.turns[0].start, 3);
        assert_eq!(s.history[5].0["tool_call_id"], "c", "the newest two steps stay");
        assert_eq!(s.compactions, 1);
    }

    #[test]
    fn drops_whole_earlier_requests_last() {
        let mut h = vec![Message::system()];
        for (q, a) in [("first question", "x".repeat(4000)), ("second question", "y".repeat(4000))] {
            h.push(Message::user(q));
            h.push(Message::assistant(&a));
        }
        h.push(Message::user("task"));
        let mut s = session_with(h);
        s.turns[0].start = 5;
        let d = compact(&mut s, 11000).unwrap();
        assert_eq!(d.requests, ["first question"]);
        assert_eq!(s.forgotten, 1);
        assert_eq!(s.history[1].0["content"], "second question");
        assert_eq!(s.turns[0].start, 3);
        assert_eq!(s.history[3].0["content"], "task");
        assert!(compact(&mut s, 1_000_000).is_none());
    }

    #[test]
    fn describes_dropped_steps() {
        let m = Message::assistant_calls("", &[call("exec", r#"{"command":"ls -la"}"#), call("done", r#"{"step":2}"#)]);
        assert_eq!(describe_calls(&m), ["exec ls -la", "done 2"]);
    }

    #[test]
    fn the_notebook_follows_the_conversation_so_the_cache_holds() {
        let mut s = Session::new("test");
        s.ask("make the desktop dark");
        assert_eq!(prompt(&s, &[]).len(), 2, "no work yet: just the conversation");
        s.set_plan(&["find the setting".into()]);
        let p = prompt(&s, &[]);
        assert_eq!(p.len(), 2, "right after the request the notebook joins it");
        assert!(p[1].0["content"].as_str().unwrap().contains("TASK NOTEBOOK"));
        s.history.push(Message::assistant_calls("", &[call("exec", r#"{"command":"ls"}"#)]));
        s.history.push(Message::tool("c", "ok"));
        let p = prompt(&s, &[]);
        assert_eq!(p.len(), 5);
        assert_eq!(p[3].0, s.history[3].0, "the conversation is sent unchanged");
        assert_eq!(p[4].0["role"], "user");
    }

    #[test]
    fn budgets_with_the_servers_token_counts() {
        let mut b = Budget::new(32768);
        assert_eq!(b.room(), 32768 - model::REPLY_TOKENS - 256);
        assert!(b.compact_to() < b.compact_at() && b.compact_at() < b.room());
        assert_eq!(b.tokens(3000), 1000);
        b.learn(40_000, 10_000);
        assert_eq!(b.tokens(4000), 1000);
        assert_eq!(b.last_prompt, 10_000);
        b.learn(100, 10);
        assert_eq!(b.chars_per_token, 4.0, "too small a sample to learn from");
        assert_eq!(Budget::new(1000).room(), 2000);
    }

    #[test]
    fn notices_a_stuck_model() {
        let mut s = Session::new("test");
        s.ask("x");
        let mut w = Watch::default();
        for _ in 0..3 {
            w.step(Some(true));
        }
        assert!(w.hints(&s)[0].contains("Write a short plan"));
        s.set_plan(&["a".into(), "b".into()]);
        w.progress();
        assert!(w.hints(&s).is_empty());
        for _ in 0..STALL_STEPS {
            w.step(Some(true));
        }
        assert!(w.hints(&s)[0].contains("step 1 (a) isn't done"));
        for _ in 0..3 {
            w.step(Some(false));
        }
        assert!(w.hints(&s).iter().any(|h| h.contains("The last 3 steps failed")));
        assert!(!w.must_answer());
        for _ in 0..2 {
            w.step(Some(false));
        }
        assert!(w.must_answer());
        assert!(w.hints(&s)[0].starts_with("Stop here; no more tools"));
    }

    #[test]
    fn a_long_plan_gets_more_steps() {
        let mut s = Session::new("test");
        assert_eq!(max_steps(&s), MAX_STEPS);
        s.set_plan(&(0..10).map(|i| format!("step {i}")).collect::<Vec<_>>());
        assert_eq!(max_steps(&s), 130);
    }

    #[test]
    fn reads_plans_however_they_come() {
        assert_eq!(
            verify(&call("plan", r#"{"steps":["1. find it","- fix it","3D print"]}"#)),
            Ok(Action::Plan { steps: vec!["find it".into(), "fix it".into(), "3D print".into()] })
        );
        assert_eq!(
            verify(&call("plan", r#"{"steps":"1) look\n2) change\n\n"}"#)),
            Ok(Action::Plan { steps: vec!["look".into(), "change".into()] })
        );
        assert!(verify(&call("plan", r#"{"steps":[]}"#)).unwrap_err().contains("no steps"));
        assert!(verify(&call("plan", r#"{"steps":[1]}"#)).unwrap_err().contains("must be a string"));
        assert_eq!(
            verify(&call("done", r#"{"step":"step 2","check":"ls shows it"}"#)),
            Ok(Action::Done { step: 2, check: "ls shows it".into() })
        );
        assert!(verify(&call("done", r#"{"step":1}"#)).unwrap_err().contains("\"check\""));
        assert_eq!(signature(&call("note", r#"{ "text" : "a" }"#)), signature(&call("note", r#"{"text":"a"}"#)));
        assert_eq!(thousands(3200), "3,200");
        assert_eq!(thousands(1234567), "1,234,567");
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
