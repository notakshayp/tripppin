//! The AI shell: the default terminal on Tripppin.
//!
//! Plain text goes to the model as a single question. `/chat` starts a conversation
//! with memory, `/terminal` opens a normal shell, and `!command` runs one command
//! exactly as typed. Every request is saved as a session; `/history` lists them and
//! `trip shell --continue <session>` (the orb's "continue in terminal") picks one up as a chat.
//!
//! It reads like a conversation: what you typed is labelled "You", the answer "Tripppin", and the
//! answer's progress is drawn by [`trip::tui::Terminal`]. Under the prompt a footer shows the
//! model, where it runs and how full a conversation's context is; Tab completes /commands.

use anyhow::Result;
use nu_ansi_term::{Color as Ansi, Style};
use reedline::{
    ColumnarMenu, Completer, CompletionResult, DefaultHinter, Emacs, FileBackedHistory, KeyCode, KeyModifiers,
    MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch, PromptHistorySearchStatus, Reedline, ReedlineEvent,
    ReedlineMenu, Signal, Span, StatusLine, Suggestion, default_emacs_keybindings,
};
use std::borrow::Cow;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use trip::agent::Ui;
use trip::session::{self, Session};
use trip::term::{self, ACCENT, BOLD, DIM, FOREST, PAPER, RED, RESET};
use trip::tui::{self, GUTTER, Mode, Terminal};
use trip::{agent, model, providers};

/// The commands, for /help, Tab and the footer's hints.
pub const COMMANDS: [(&str, &str); 9] = [
    ("/chat", "Start a conversation with memory"),
    ("/exit", "Leave the conversation, or log out"),
    ("/terminal", "Open a normal shell; type exit to come back"),
    ("/history", "What you asked before; /history <n> continues one"),
    ("/wifi", "Connect to a Wi-Fi network"),
    ("/setup", "Download the model for this machine and start it"),
    ("/clear", "Clear the screen"),
    ("/help", "Show this"),
    ("/quit", "The same as /exit"),
];

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
    Clear,
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
        "/clear" | "/cls" => Input::Clear,
        "/help" | "/?" => Input::Help,
        "/exit" | "/quit" | "/logout" => Input::Exit,
        _ if line.starts_with('/') && !line.contains(' ') => Input::Unknown(line),
        _ => Input::Ask(line),
    }
}

/// The commands that start with what's typed, for Tab and the footer.
pub fn matching(typed: &str) -> Vec<(&'static str, &'static str)> {
    if !typed.starts_with('/') || typed.contains(char::is_whitespace) {
        return Vec::new();
    }
    COMMANDS.iter().copied().filter(|(name, _)| name.starts_with(typed)).collect()
}

/// The model the shell talks to, for the header and the footer.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub model: String,
    /// "On this computer", "Via Anthropic", "Your own server".
    pub place: String,
    /// Context size in tokens, once known.
    pub context: usize,
    /// How full the conversation's context was after the last answer, in percent.
    pub fill: Option<usize>,
    pub chat: bool,
    pub ready: bool,
}

impl Info {
    fn detect() -> Info {
        if let Some(e) = model::endpoint() {
            let p = providers::for_url(&e.url);
            let place = if p.id == "own" { "Your own server".to_string() } else { format!("Via {}", p.name) };
            let model = if e.model.is_empty() { "A model".to_string() } else { e.model };
            return Info { model, place, ready: true, ..Info::default() };
        }
        let tier = hwdetect::Catalog::builtin().ok().and_then(|c| {
            let dir = model::models_dir();
            model::active_tier(&c, &dir).cloned()
        });
        match tier {
            Some(t) => Info { model: t.model, place: "On this computer".into(), ready: true, ..Info::default() },
            None => Info { model: "No model yet".into(), place: "/setup downloads one".into(), ..Info::default() },
        }
    }

    /// The footer under the prompt, cut to `width` columns.
    pub fn footer(&self, width: usize) -> String {
        let mut parts = Vec::new();
        if self.chat {
            parts.push("Chat".to_string());
        }
        parts.push(self.model.clone());
        parts.push(self.place.clone());
        match (self.chat, self.fill) {
            (true, Some(fill)) => parts.push(format!("Context {fill}% full")),
            _ if self.context > 0 => parts.push(format!("{} context", tui::context_size(self.context))),
            _ => {}
        }
        parts.push(if self.chat { "/exit ends it" } else { "/help" }.to_string());
        term::truncate(&parts.join(" · "), width.saturating_sub(1))
    }
}

/// The prompt: the mode in the gutter, then the marker.
struct ShellPrompt {
    chat: bool,
}

impl Prompt for ShellPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{:<w$}", if self.chat { "Chat" } else { "AI" }, w = GUTTER - 2))
    }
    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }
    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("› ")
    }
    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Owned(" ".repeat(GUTTER))
    }
    fn render_prompt_history_search_indicator(&self, search: PromptHistorySearch) -> Cow<'_, str> {
        let prefix = if matches!(search.status, PromptHistorySearchStatus::Passing) { "" } else { "No match " };
        Cow::Owned(format!("({prefix}Search: {}) ", search.term))
    }
    fn get_prompt_color(&self) -> reedline::Color {
        reedline::Color::Rgb(70, 160, 118)
    }
    fn get_indicator_color(&self) -> reedline::Color {
        reedline::Color::Rgb(87, 217, 154)
    }
}

/// What a sent line turns into, so the conversation reads "you … / trip …".
struct SentPrompt;

impl Prompt for SentPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed("You")
    }
    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }
    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Owned(" ".repeat(GUTTER - 3))
    }
    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Owned(" ".repeat(GUTTER))
    }
    fn render_prompt_history_search_indicator(&self, _search: PromptHistorySearch) -> Cow<'_, str> {
        Cow::Borrowed("")
    }
    fn get_prompt_color(&self) -> reedline::Color {
        reedline::Color::Rgb(140, 150, 145)
    }
}

/// Tab completes /commands, with what each does beside it.
struct Commands;

impl Completer for Commands {
    fn complete(&mut self, line: &str, pos: usize) -> CompletionResult {
        let typed = &line[..pos.min(line.len())];
        let suggestions: Vec<Suggestion> = matching(typed)
            .into_iter()
            .map(|(name, about)| Suggestion {
                value: name.to_string(),
                description: Some(about.to_string()),
                span: Span { start: 0, end: pos },
                append_whitespace: false,
                ..Suggestion::default()
            })
            .collect();
        CompletionResult::fresh(suggestions)
    }
}

/// The row under the prompt: what a /command does while one is typed, the footer otherwise.
struct Footer(Arc<Mutex<Info>>);

impl StatusLine for Footer {
    fn status(&mut self, line: &str, _pos: usize, columns: u16, color: bool) -> String {
        let width = usize::from(columns);
        let text = match matching(line.trim_end()).as_slice() {
            [] if line.starts_with('/') && !line.contains(' ') => "No such command · /help lists them".to_string(),
            [] => self.0.lock().map(|i| i.footer(width)).unwrap_or_default(),
            [(name, about)] => format!("{name}  {about}"),
            many => format!("Tab: {}", many.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(" ")),
        };
        let text = term::truncate(&text, width.saturating_sub(1));
        if color { format!("{DIM}{text}{RESET}") } else { text }
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

fn editor(info: Arc<Mutex<Info>>) -> Reedline {
    let mut keys = default_emacs_keybindings();
    keys.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![ReedlineEvent::Menu("commands".into()), ReedlineEvent::MenuNext]),
    );
    let green = Ansi::Rgb(87, 217, 154);
    let menu = ColumnarMenu::default()
        .with_name("commands")
        .with_columns(1)
        .with_marker(&" ".repeat(GUTTER))
        .with_text_style(Style::new().fg(green))
        .with_selected_text_style(Style::new().fg(green).bold().underline())
        .with_description_text_style(Style::new().fg(Ansi::Rgb(140, 150, 145)));
    let mut editor = Reedline::create()
        .with_hinter(Box::new(DefaultHinter::default().with_style(Style::new().fg(Ansi::Rgb(90, 100, 95)))))
        .with_completer(Box::new(Commands))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(menu)))
        .with_edit_mode(Box::new(Emacs::new(keys)))
        .with_transient_prompt(Box::new(SentPrompt));
    if Mode::detect().color {
        editor = editor.with_status_line(Box::new(Footer(info)));
    }
    if let Some(path) = history_path()
        && let Ok(history) = FileBackedHistory::with_file(2000, path)
    {
        editor = editor.with_history(Box::new(history));
    }
    editor
}

/// Prints a line in the answer's column: dim, with a mark in the gutter's place.
fn say(text: &str) {
    let text = term::sentence(text);
    if Mode::detect().color {
        println!("{}{FOREST}·{RESET} {DIM}{text}{RESET}", " ".repeat(GUTTER));
    } else {
        println!("· {text}");
    }
}

fn warn(text: &str) {
    let text = term::sentence(text);
    if Mode::detect().color {
        println!("{}{RED}!{RESET} {text}", " ".repeat(GUTTER));
    } else {
        println!("! {text}");
    }
}

/// The power spark, three rows tall, for the header.
fn mark(mode: Mode) -> [String; 3] {
    let ring = |s: &str| if mode.color { format!("{PAPER}{s}{RESET}") } else { s.to_string() };
    let spark = if mode.color { format!("{ACCENT}{BOLD}✕{RESET}") } else { "x".to_string() };
    [format!(" {} {spark} {} ", ring("▄▀"), ring("▀▄")), ring("█       █"), ring(" ▀▄▄▄▄▄▀ ")]
}

/// The welcome header: the mark, the model and where it runs, and how to get going.
pub fn header(info: &Info, mode: Mode, width: usize) -> Vec<String> {
    let paint = |code: &str, s: &str| if mode.color { format!("{code}{s}{RESET}") } else { s.to_string() };
    let mut model = vec![info.model.clone(), info.place.clone()];
    if info.context > 0 {
        model.push(format!("{} context", tui::context_size(info.context)));
    }
    let hint = ["Type anything to ask", "/chat", "/terminal", "/help"].map(String::from);
    if !mode.unicode || width < 40 {
        let mut rows = vec![paint(BOLD, "Tripppin")];
        rows.extend(tui::fit(&model, width.saturating_sub(1)).iter().map(|r| paint(DIM, r)));
        rows.extend(tui::fit(&hint, width.saturating_sub(1)).iter().map(|r| paint(DIM, r)));
        return rows;
    }
    let room = width.saturating_sub(13);
    let mut text = vec![format!("{}  {}", paint(BOLD, "Tripppin"), paint(DIM, "Tell your computer what you want"))];
    if term::width(&text[0]) > room {
        text[0] = paint(BOLD, "Tripppin");
    }
    text.push(paint(PAPER, &term::truncate(&model.join(" · "), room)));
    let hint_rows = tui::fit(&hint, room);
    text.extend(hint_rows.iter().map(|r| paint(DIM, r)));
    let mark = mark(mode);
    let blank = " ".repeat(9);
    text.iter().enumerate().map(|(i, t)| format!(" {}  {t}", mark.get(i).unwrap_or(&blank))).collect()
}

/// /help: every command with what it does, then the keys.
pub fn help(mode: Mode) -> Vec<String> {
    let paint = |code: &str, s: &str| if mode.color { format!("{code}{s}{RESET}") } else { s.to_string() };
    let mut rows = vec![
        format!(
            "{}  {}",
            paint(ACCENT, &format!("{:<11}", "anything")),
            "Ask, or tell it what to do; it can run commands"
        ),
        format!("{}  {}", paint(ACCENT, &format!("{:<11}", "!command")), "Run one command exactly as typed"),
    ];
    for (name, about) in COMMANDS.iter().filter(|(n, _)| *n != "/quit") {
        rows.push(format!("{}  {about}", paint(ACCENT, &format!("{name:<11}"))));
    }
    rows.push(String::new());
    rows.push(paint(DIM, "Tab completes · Up and Ctrl+R search what you typed before"));
    rows.push(paint(DIM, "Ctrl+C stops the current task · Ctrl+D leaves a chat, or logs out"));
    rows
}

fn print_rows(rows: &[String]) {
    let pad = if Mode::detect().color { " ".repeat(GUTTER) } else { String::new() };
    for r in rows {
        if r.is_empty() {
            println!();
        } else {
            println!("{pad}{r}");
        }
    }
}

/// Finds the context size without holding up the prompt: the local server may be asleep.
fn context_size(info: &Info) -> usize {
    if !info.ready {
        return 0;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let info = model::server_info();
        let _ = tx.send(if info.loading { 0 } else { info.context });
    });
    rx.recv_timeout(Duration::from_millis(400)).unwrap_or(0)
}

pub fn run(resume: Option<&str>) -> Result<()> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        // Ctrl-C while an answer is streaming stops the answer, not the shell.
        let cancel = cancel.clone();
        ctrlc::set_handler(move || cancel.store(true, Ordering::Relaxed))?;
    }
    let mut info = Info::detect();
    info.context = context_size(&info);
    let mode = Mode::detect();
    let width = tui::terminal_width();
    println!();
    for row in header(&info, mode, width) {
        println!("{row}");
    }
    println!();
    let info = Arc::new(Mutex::new(info));
    let mut editor = editor(info.clone());

    let mut chat: Option<Session> = resume.and_then(continued);
    loop {
        if let Ok(mut i) = info.lock() {
            i.chat = chat.is_some();
            if !i.chat {
                i.fill = None;
            }
        }
        let prompt = ShellPrompt { chat: chat.is_some() };
        let line = match editor.read_line(&prompt)? {
            Signal::Success(line) => line,
            Signal::CtrlC(_) => continue,
            Signal::CtrlD => {
                if chat.take().is_some() {
                    say("Conversation ended");
                    continue;
                }
                return Ok(());
            }
            _ => continue,
        };
        match parse(&line) {
            Input::Empty => {}
            Input::Help => {
                println!();
                print_rows(&help(mode));
                println!();
            }
            Input::Clear => {
                let _ = editor.clear_screen();
            }
            Input::Exit if chat.is_some() => {
                chat = None;
                say("Conversation ended");
            }
            Input::Exit => return Ok(()),
            Input::Chat => {
                chat = Some(Session::new("terminal"));
                say("Conversation started. It remembers what you say until /exit.");
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
            Input::Unknown(cmd) => say(&format!("{cmd} isn't a command here. /help lists them.")),
            Input::Ask(text) => {
                cancel.store(false, Ordering::Relaxed);
                let usage = match &mut chat {
                    Some(session) => {
                        session.ask(text);
                        respond(session, &cancel)
                    }
                    None => {
                        let mut session = Session::new("terminal");
                        session.ask(text);
                        respond(&mut session, &cancel)
                    }
                };
                if let (Some(u), Ok(mut i)) = (usage, info.lock()) {
                    if u.context > 0 {
                        i.context = u.context;
                        i.fill = Some(tui::context_fill(&u));
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
        say("Couldn't find that conversation; starting fresh");
        return None;
    };
    recap(&s);
    say("Continuing · it remembers this until /exit");
    Some(s)
}

/// Every request in a session with its steps (and how each went) and the answer, laid out like
/// the conversation was.
fn recap(s: &Session) {
    let mode = Mode::detect();
    for t in &s.turns {
        if mode.color {
            println!("\n{DIM}You{RESET}{}{}", " ".repeat(GUTTER - 3), t.request);
        } else {
            println!("\nYou: {}", t.request);
        }
        println!();
        let mut ui = Terminal::with_output(Box::new(std::io::stdout()), Mode { live: false, ..mode }, None, None);
        for step in &t.steps {
            ui.step(&step.tool, step.detail.lines().next().unwrap_or(""));
            if let Some(ok) = step.ok {
                ui.step_done(ok);
            }
        }
        if !t.answer.is_empty() {
            ui.text(&t.answer);
            ui.text("\n");
        } else if !t.error.is_empty() {
            ui.note(&format!("Stopped: {}", t.error));
        }
        ui.finish(None);
    }
    println!();
}

/// The recent sessions, numbered for `/history <n>`.
fn history() {
    let list = session::list(15);
    if list.is_empty() {
        say("Nothing yet. What you ask here and in the orb shows up in this list.");
        return;
    }
    let now = session::now_secs();
    let pad = " ".repeat(GUTTER - 4);
    println!();
    for (i, s) in list.iter().enumerate() {
        let mark = match s.ok {
            Some(true) => format!("{ACCENT}✓{RESET}"),
            Some(false) => format!("{RED}✗{RESET}"),
            None => " ".to_string(),
        };
        let title = term::truncate(&s.title, 50);
        println!(
            "{pad}{DIM}{:>3}{RESET} {mark} {title}  {DIM}{} · {} · {} step{}{RESET}",
            i + 1,
            session::when(s.started, now),
            s.source,
            s.steps,
            if s.steps == 1 { "" } else { "s" }
        );
    }
    println!();
    say("/history <number> shows one and continues it as a chat");
}

fn pick(n: usize) -> Option<Session> {
    let list = session::list(n.max(15));
    let Some(s) = n.checked_sub(1).and_then(|i| list.get(i)).and_then(|s| session::load(&s.id)) else {
        say(&format!("There's no number {n}. /history lists them."));
        return None;
    };
    recap(&s);
    say("Continuing this conversation · /exit to leave it");
    Some(s)
}

/// Works on one request with the agent, which streams its answer and shows each step.
/// Returns the newest token counts, for the footer.
fn respond(session: &mut Session, cancel: &Arc<AtomicBool>) -> Option<model::Usage> {
    println!();
    let mut ui = Terminal::new(Some(cancel.clone()));
    let result = agent::run(session, cancel, &mut ui);
    ui.finish(result.as_ref().ok().and_then(|d| d.stats.as_deref()));
    match result {
        Ok(_) => {}
        Err(e) if e.to_string() == "stopped" => say("Stopped"),
        Err(e) => warn(&format!("{e:#}")),
    }
    println!();
    ui.last_usage()
}

fn shell_program() -> String {
    std::env::var("TRIPPPIN_CLASSIC_SHELL").unwrap_or_else(|_| "bash".to_string())
}

/// A bash start-up file that loads the person's own and then marks the prompt, so it's clear
/// this is the normal shell inside tripppin.
fn marked_rcfile() -> Option<PathBuf> {
    let dir = history_path()?.parent()?.to_path_buf();
    let path = dir.join("terminal.bashrc");
    let rc = "[ -f ~/.bashrc ] && . ~/.bashrc\nPS1=\"\\[\\e[38;2;70;160;118m\\]Terminal\\[\\e[0m\\] $PS1\"\n";
    std::fs::write(&path, rc).ok()?;
    Some(path)
}

/// A normal interactive shell. Marked so the login hook doesn't start the AI shell inside it.
fn terminal() {
    say("Normal shell. Type exit to come back.");
    let program = shell_program();
    let mut cmd = Command::new(&program);
    if program == "bash"
        && let Some(rc) = marked_rcfile()
    {
        cmd.arg("--rcfile").arg(rc);
    }
    let status = cmd.arg("-i").env("TRIPPPIN_IN_SHELL", "1").status();
    if let Err(e) = status {
        warn(&format!("Couldn't open a shell: {e}"));
    }
    say("Back in Tripppin");
}

fn raw(cmd: &str) {
    if let Err(e) = Command::new(shell_program()).arg("-c").arg(cmd).env("TRIPPPIN_IN_SHELL", "1").status() {
        warn(&format!("Couldn't run it: {e}"));
    }
}

/// NetworkManager's menu for picking a network and typing its password.
fn wifi() {
    match Command::new("nmtui").arg("connect").status() {
        Ok(_) => say("Back in Tripppin"),
        Err(e) => warn(&format!("Couldn't open the Wi-Fi menu: {e}")),
    }
}

/// Downloads the recommended model and starts the service; both need admin rights.
fn setup() {
    say("Downloading the model for this machine. You may be asked for your password.");
    let steps: [&[&str]; 2] = [&["sudo", "trip", "model", "pull"], &["sudo", "systemctl", "restart", "tripppin-llm"]];
    for step in steps {
        match Command::new(step[0]).args(&step[1..]).status() {
            Ok(s) if s.success() => {}
            Ok(_) => {
                warn(&format!("`{}` didn't finish. No internet? Try /wifi, then /setup again.", step.join(" ")));
                return;
            }
            Err(e) => {
                warn(&format!("Couldn't run `{}`: {e}", step.join(" ")));
                return;
            }
        }
    }
    say("Ready. Ask anything.");
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
        assert_eq!(parse("/clear"), Input::Clear);
        assert_eq!(parse("/exit"), Input::Exit);
        assert_eq!(parse("/quit"), Input::Exit);
        assert_eq!(parse("/help"), Input::Help);
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

    #[test]
    fn every_command_parses() {
        for (name, _) in COMMANDS {
            assert!(!matches!(parse(name), Input::Unknown(_) | Input::Ask(_)), "{name}");
        }
    }

    #[test]
    fn tab_completes_commands() {
        assert_eq!(matching("/c").iter().map(|(n, _)| *n).collect::<Vec<_>>(), ["/chat", "/clear"]);
        assert_eq!(matching("/te").len(), 1);
        assert!(matching("hello").is_empty());
        assert!(matching("/history 2").is_empty());
        let mut c = Commands;
        let result = c.complete("/se", 3);
        let s = result.suggestions();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].value, "/setup");
        assert_eq!(s[0].span, Span { start: 0, end: 3 });
    }

    #[test]
    fn footer_shows_model_and_context() {
        let mut info =
            Info { model: "Qwen3.5 4B".into(), place: "On this computer".into(), context: 32768, ..Info::default() };
        assert_eq!(info.footer(80), "Qwen3.5 4B · On this computer · 32k context · /help");
        info.chat = true;
        info.fill = Some(38);
        assert_eq!(info.footer(80), "Chat · Qwen3.5 4B · On this computer · Context 38% full · /exit ends it");
        assert!(term::width(&info.footer(30)) <= 29);
    }

    #[test]
    fn header_fits_narrow_terminals() {
        let info = Info { model: "Qwen3.5 4B".into(), place: "On this computer".into(), ..Info::default() };
        let mode = Mode { live: true, color: true, unicode: true };
        for width in [40, 80, 120] {
            let rows = header(&info, mode, width);
            assert!(rows.len() >= 3);
            assert!(rows.iter().all(|r| term::width(r) < width), "{width}: {rows:?}");
        }
        let plain = header(&info, Mode::PLAIN, 80);
        assert!(plain.iter().all(|r| !r.contains('\x1b')));
        assert!(plain.join("\n").contains("Qwen3.5 4B · On this computer"));
    }

    #[test]
    fn help_lists_every_command() {
        let text = help(Mode::PLAIN).join("\n");
        for (name, about) in COMMANDS.iter().filter(|(n, _)| *n != "/quit") {
            assert!(text.contains(name) && text.contains(about), "{name}");
        }
        assert!(help(Mode::PLAIN).iter().all(|r| term::width(r) < 80 - GUTTER));
    }
}
