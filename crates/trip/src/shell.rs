//! The AI shell: the default terminal on Tripppin.
//!
//! Plain text goes to the model as a single question. `/chat` starts a conversation
//! with memory, `/terminal` opens a normal shell, and `!command` runs one command
//! exactly as typed.

use crate::model::{self, Message};
use anyhow::Result;
use reedline::{
    DefaultHinter, FileBackedHistory, Prompt, PromptEditMode, PromptHistorySearch, PromptHistorySearchStatus, Reedline,
    Signal,
};
use std::borrow::Cow;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const ACCENT: &str = "\x1b[38;2;87;217;154m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const HELP: &str = "\
  type anything     ask the model a question
  /chat             start a conversation with memory (/exit to leave it)
  /terminal         open a normal shell (exit to come back)
  !command          run one command exactly as typed
  /setup            download the model for this machine and start it
  /help             show this
  /exit             log out";

/// What a line of input means.
#[derive(Debug, PartialEq)]
pub enum Input<'a> {
    Empty,
    Ask(&'a str),
    Raw(&'a str),
    Chat,
    Terminal,
    Setup,
    Help,
    Exit,
    Unknown(&'a str),
}

pub fn parse(line: &str) -> Input<'_> {
    let line = line.trim();
    if line.is_empty() {
        return Input::Empty;
    }
    if let Some(cmd) = line.strip_prefix('!') {
        return if cmd.trim().is_empty() { Input::Empty } else { Input::Raw(cmd.trim()) };
    }
    match line {
        "/chat" => Input::Chat,
        "/terminal" | "/term" => Input::Terminal,
        "/setup" => Input::Setup,
        "/help" | "/?" => Input::Help,
        "/exit" | "/quit" | "/logout" => Input::Exit,
        _ if line.starts_with('/') && !line.contains(' ') => Input::Unknown(line),
        _ => Input::Ask(line),
    }
}

struct ShellPrompt {
    chat: bool,
}

impl Prompt for ShellPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed(if self.chat { "chat " } else { "" })
    }
    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }
    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("› ")
    }
    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("  ")
    }
    fn render_prompt_history_search_indicator(&self, search: PromptHistorySearch) -> Cow<'_, str> {
        let prefix = if matches!(search.status, PromptHistorySearchStatus::Passing) { "" } else { "no match " };
        Cow::Owned(format!("({prefix}search: {}) ", search.term))
    }
    fn get_prompt_color(&self) -> reedline::Color {
        reedline::Color::Rgb(87, 217, 154)
    }
    fn get_indicator_color(&self) -> reedline::Color {
        reedline::Color::Rgb(87, 217, 154)
    }
}

fn history_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    let dir = base.join("tripppin");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("shell-history.txt"))
}

pub fn run() -> Result<()> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        // Ctrl-C while an answer is streaming stops the answer, not the shell.
        let cancel = cancel.clone();
        ctrlc::set_handler(move || cancel.store(true, Ordering::Relaxed))?;
    }
    let mut editor = Reedline::create().with_hinter(Box::new(DefaultHinter::default()));
    if let Some(path) = history_path()
        && let Ok(history) = FileBackedHistory::with_file(2000, path)
    {
        editor = editor.with_history(Box::new(history));
    }
    println!("{DIM}type anything to ask · /chat for a conversation · /terminal for a normal shell · /help{RESET}");

    let mut chat: Option<Vec<Message>> = None;
    loop {
        let prompt = ShellPrompt { chat: chat.is_some() };
        let line = match editor.read_line(&prompt)? {
            Signal::Success(line) => line,
            Signal::CtrlC(_) => continue,
            Signal::CtrlD => {
                if chat.take().is_some() {
                    println!("{DIM}conversation ended{RESET}");
                    continue;
                }
                return Ok(());
            }
            _ => continue,
        };
        match parse(&line) {
            Input::Empty => {}
            Input::Help => println!("{HELP}"),
            Input::Exit if chat.is_some() => {
                chat = None;
                println!("{DIM}conversation ended{RESET}");
            }
            Input::Exit => return Ok(()),
            Input::Chat => {
                chat = Some(vec![Message::system()]);
                println!("{DIM}conversation started. it remembers what you say until you type /exit.{RESET}");
            }
            Input::Terminal => terminal(),
            Input::Raw(cmd) => raw(cmd),
            Input::Setup => setup(),
            Input::Unknown(cmd) => println!("{DIM}{cmd} isn't a command here. /help lists them.{RESET}"),
            Input::Ask(text) => {
                let messages = match &mut chat {
                    Some(history) => {
                        history.push(Message::user(text));
                        history.clone()
                    }
                    None => vec![Message::system(), Message::user(text)],
                };
                cancel.store(false, Ordering::Relaxed);
                let answer = respond(&messages, &cancel);
                if let (Some(history), Some(answer)) = (&mut chat, answer) {
                    history.push(Message::assistant(&answer));
                }
            }
        }
    }
}

/// Streams one answer to the terminal. Returns the text if it completed.
fn respond(messages: &[Message], cancel: &AtomicBool) -> Option<String> {
    let mut out = io::stdout().lock();
    let result = model::stream(messages, cancel, |piece| {
        let _ = write!(out, "{piece}");
        let _ = out.flush();
    });
    match result {
        Ok(reply) => {
            let _ = writeln!(out);
            if reply.cancelled {
                let _ = writeln!(out, "{DIM}stopped{RESET}");
                return None;
            }
            if let Some(stats) = reply.stats() {
                let _ = writeln!(out, "{DIM}{stats}{RESET}");
            }
            Some(reply.text)
        }
        Err(e) => {
            let _ = writeln!(out, "{ACCENT}!{RESET} {e}");
            None
        }
    }
}

fn shell_program() -> String {
    std::env::var("TRIPPPIN_CLASSIC_SHELL").unwrap_or_else(|_| "bash".to_string())
}

/// A normal interactive shell. Marked so the login hook doesn't start the AI shell inside it.
fn terminal() {
    println!("{DIM}normal shell. type exit to come back.{RESET}");
    let status = Command::new(shell_program()).arg("-i").env("TRIPPPIN_IN_SHELL", "1").status();
    if let Err(e) = status {
        println!("{ACCENT}!{RESET} couldn't open a shell: {e}");
    }
    println!("{DIM}back in tripppin{RESET}");
}

fn raw(cmd: &str) {
    if let Err(e) = Command::new(shell_program()).arg("-c").arg(cmd).env("TRIPPPIN_IN_SHELL", "1").status() {
        println!("{ACCENT}!{RESET} couldn't run it: {e}");
    }
}

/// Downloads the recommended model and starts the service; both need admin rights.
fn setup() {
    println!("{DIM}downloading the model for this machine. you may be asked for your password.{RESET}");
    let steps: [&[&str]; 2] = [&["sudo", "trip", "model", "pull"], &["sudo", "systemctl", "restart", "tripppin-llm"]];
    for step in steps {
        match Command::new(step[0]).args(&step[1..]).status() {
            Ok(s) if s.success() => {}
            Ok(_) => {
                println!("{ACCENT}!{RESET} `{}` didn't finish. you can run /setup again.", step.join(" "));
                return;
            }
            Err(e) => {
                println!("{ACCENT}!{RESET} couldn't run `{}`: {e}", step.join(" "));
                return;
            }
        }
    }
    println!("{DIM}ready. ask anything.{RESET}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_a_question() {
        assert_eq!(parse("what is my ip"), Input::Ask("what is my ip"));
        assert_eq!(parse("  ls -la  "), Input::Ask("ls -la"));
        assert_eq!(parse("/usr/bin is full of what?"), Input::Ask("/usr/bin is full of what?"));
    }

    #[test]
    fn slash_commands() {
        assert_eq!(parse("/chat"), Input::Chat);
        assert_eq!(parse("/terminal"), Input::Terminal);
        assert_eq!(parse("/term"), Input::Terminal);
        assert_eq!(parse("/setup"), Input::Setup);
        assert_eq!(parse("/exit"), Input::Exit);
        assert_eq!(parse("/nope"), Input::Unknown("/nope"));
    }

    #[test]
    fn bang_runs_a_command() {
        assert_eq!(parse("!df -h /"), Input::Raw("df -h /"));
        assert_eq!(parse("!"), Input::Empty);
        assert_eq!(parse(""), Input::Empty);
    }
}
