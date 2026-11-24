//! The AI shell: the default terminal on Tripppin.
//!
//! Plain text goes to the model as a single question. `/chat` starts a conversation
//! with memory, `/terminal` opens a normal shell, and `!command` runs one command
//! exactly as typed. Every request is saved as a session; `/history` lists them and
//! `trip shell --continue <session>` (the orb's "continue in terminal") picks one up as a chat.

use anyhow::Result;
use reedline::{
    DefaultHinter, FileBackedHistory, Prompt, PromptEditMode, PromptHistorySearch, PromptHistorySearchStatus, Reedline,
    Signal,
};
use std::borrow::Cow;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use trip::agent;
use trip::session::{self, Session};
use trip::term::{ACCENT, DIM, RESET};

const HELP: &str = "\
  type anything     ask, or tell it what to do; it can run commands
  /chat             start a conversation with memory (/exit to leave it)
  /terminal         open a normal shell (exit to come back)
  /history          what you asked before, and the steps it took
  /history <n>      show one in full, and continue it as a chat
  !command          run one command exactly as typed
  /wifi             connect to a wi-fi network
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
    History(Option<usize>),
    Wifi,
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
        "/history" => Input::History(None),
        _ if line.starts_with("/history ") => match line["/history ".len()..].trim().parse() {
            Ok(n) => Input::History(Some(n)),
            Err(_) => Input::Unknown(line),
        },
        "/wifi" => Input::Wifi,
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

pub fn run(resume: Option<&str>) -> Result<()> {
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

    let mut chat: Option<Session> = resume.and_then(continued);
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
                chat = Some(Session::new("terminal"));
                println!("{DIM}conversation started. it remembers what you say until you type /exit.{RESET}");
            }
            Input::Terminal => terminal(),
            Input::History(None) => history(),
            Input::History(Some(n)) => {
                if let Some(s) = pick(n) {
                    chat = Some(s);
                }
            }
            Input::Raw(cmd) => raw(cmd),
            Input::Wifi => wifi(),
            Input::Setup => setup(),
            Input::Unknown(cmd) => println!("{DIM}{cmd} isn't a command here. /help lists them.{RESET}"),
            Input::Ask(text) => {
                cancel.store(false, Ordering::Relaxed);
                match &mut chat {
                    Some(session) => {
                        session.ask(text);
                        respond(session, &cancel);
                    }
                    None => {
                        let mut session = Session::new("terminal");
                        session.ask(text);
                        respond(&mut session, &cancel);
                    }
                }
            }
        }
    }
}

/// A session from the history, shown as a recap and kept as a chat so follow-ups know
/// what happened.
fn continued(id: &str) -> Option<Session> {
    let Some(s) = session::load(id) else {
        println!("{DIM}couldn't find that conversation; starting fresh{RESET}");
        return None;
    };
    println!("{DIM}continuing · it remembers this until /exit{RESET}");
    recap(&s);
    Some(s)
}

/// Every request in a session with its steps (and how each went) and the answer.
fn recap(s: &Session) {
    for t in &s.turns {
        println!("\n{DIM}you asked{RESET}  {}", t.request);
        for step in &t.steps {
            let mark = match step.ok {
                Some(true) => format!("{ACCENT}✓{RESET}"),
                Some(false) => "\x1b[31m✗\x1b[0m".to_string(),
                None => format!("{DIM}·{RESET}"),
            };
            let detail: String = step.detail.lines().next().unwrap_or("").chars().take(90).collect();
            println!("  {mark} {DIM}{}{RESET} {detail}", step.tool);
        }
        if !t.answer.is_empty() {
            // The answer was written for the orb's light Markdown; plain text reads better here.
            println!("\n{}", t.answer.replace("**", "").replace('`', ""));
        } else if !t.error.is_empty() {
            println!("\n{DIM}stopped: {}{RESET}", t.error);
        }
    }
    println!();
}

/// The recent sessions, numbered for `/history <n>`.
fn history() {
    let list = session::list(15);
    if list.is_empty() {
        println!("{DIM}nothing yet. what you ask here and in the orb shows up in this list.{RESET}");
        return;
    }
    let now = session::now_secs();
    for (i, s) in list.iter().enumerate() {
        let mark = match s.ok {
            Some(true) => format!("{ACCENT}✓{RESET}"),
            Some(false) => "\x1b[31m✗\x1b[0m".to_string(),
            None => " ".to_string(),
        };
        let title: String = s.title.chars().take(56).collect();
        println!(
            "{DIM}{:>3}{RESET} {mark} {title}  {DIM}{} · {} · {} step{}{RESET}",
            i + 1,
            session::when(s.started, now),
            s.source,
            s.steps,
            if s.steps == 1 { "" } else { "s" }
        );
    }
    println!("{DIM}/history <number> shows one and continues it as a chat{RESET}");
}

fn pick(n: usize) -> Option<Session> {
    let list = session::list(n.max(15));
    let Some(s) = n.checked_sub(1).and_then(|i| list.get(i)).and_then(|s| session::load(&s.id)) else {
        println!("{DIM}there's no number {n}. /history lists them.{RESET}");
        return None;
    };
    recap(&s);
    println!("{DIM}continuing this conversation · /exit to leave it{RESET}");
    Some(s)
}

/// Works on one request with the agent, which streams its answer and shows each step.
fn respond(session: &mut Session, cancel: &AtomicBool) {
    match agent::run(session, cancel, &mut agent::Terminal) {
        Ok(done) => {
            if let Some(stats) = done.stats {
                println!("{DIM}{stats}{RESET}");
            }
        }
        Err(e) if e.to_string() == "stopped" => println!("{DIM}stopped{RESET}"),
        Err(e) => println!("{ACCENT}!{RESET} {e:#}"),
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

/// NetworkManager's menu for picking a network and typing its password.
fn wifi() {
    match Command::new("nmtui").arg("connect").status() {
        Ok(_) => println!("{DIM}back in tripppin{RESET}"),
        Err(e) => println!("{ACCENT}!{RESET} couldn't open the wi-fi menu: {e}"),
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
                println!(
                    "{ACCENT}!{RESET} `{}` didn't finish. no internet? try /wifi, then /setup again.",
                    step.join(" ")
                );
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
        assert_eq!(parse("/wifi"), Input::Wifi);
        assert_eq!(parse("/exit"), Input::Exit);
        assert_eq!(parse("/history"), Input::History(None));
        assert_eq!(parse("/history 3"), Input::History(Some(3)));
        assert_eq!(parse("/history x"), Input::Unknown("/history x"));
        assert_eq!(parse("/nope"), Input::Unknown("/nope"));
    }

    #[test]
    fn bang_runs_a_command() {
        assert_eq!(parse("!df -h /"), Input::Raw("df -h /"));
        assert_eq!(parse("!"), Input::Empty);
        assert_eq!(parse(""), Input::Empty);
    }
}
