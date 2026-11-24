//! `trip shell --demo`: a canned session replayed through the real terminal display, with
//! made-up steps and token counts. It needs no model, so it works anywhere (macOS too) and is
//! what the screenshots in the docs are taken from. In a terminal it plays at reading speed;
//! piped to a file it prints the finished screen at once (`CLICOLOR_FORCE=1` keeps the colours).

use crate::shell::{self, Info};
use std::io::Write;
use std::thread::sleep;
use std::time::Duration;
use trip::agent::Ui;
use trip::model::PlanStep;
use trip::term::{DIM, RESET};
use trip::tui::{GUTTER, Mode, Terminal};

/// One thing that happens in the replay.
enum Event {
    /// The person types a line at the prompt (in AI or chat mode).
    Type(&'static str, bool),
    /// A dim line from the shell itself.
    Say(&'static str),
    Thinking,
    Waiting(&'static str),
    Plan(&'static [(&'static str, bool)]),
    Step(&'static str, &'static str),
    Output(&'static str),
    Done(bool),
    /// The answer, streamed a few characters at a time.
    Text(&'static str),
    Usage(usize, usize, usize, f64),
    /// The task ends: the token line.
    Finish,
    Pause(u64),
}

use Event::*;

const ANSWER_1: &str = "Your home folder uses **41 GB**. Most of it is in three places:

| Folder | Size |
|---|---:|
| ~/Downloads | 18 GB |
| ~/.local/share/flatpak | 9.4 GB |
| ~/.cache | 6.2 GB |

- **Downloads** has two Fedora ISOs (`Fedora-45-x86_64.iso`, 2.4 GB each) you could delete.
- The **cache** is safe to clear; apps rebuild it as they need it:

```bash
rm -rf ~/.cache/thumbnails ~/.cache/pip
```

Want me to clear the cache and remove unused Flatpak runtimes?
";

const ANSWER_2: &str = "Done. The cache is cleared, which freed **6.2 GB**. Removing the unused \
Flatpak runtimes failed because another update was running; try again in a minute, or run \
`flatpak uninstall --unused` yourself.
";

const SCRIPT: &[Event] = &[
    Type("what's using my disk space?", false),
    Thinking,
    Waiting("loading the model"),
    Pause(1600),
    Waiting("reading 1,240 tokens"),
    Pause(900),
    Usage(1240, 0, 64, 18.0),
    Plan(&[("Find the biggest folders", false), ("Suggest what's safe to clear", false)]),
    Step("exec", "du -xh ~ --max-depth=1 2>/dev/null | sort -rh | head -6"),
    Pause(500),
    Output("41G\t/home/anu"),
    Pause(250),
    Output("18G\t/home/anu/Downloads"),
    Pause(250),
    Output("9.4G\t/home/anu/.local"),
    Pause(250),
    Output("6.2G\t/home/anu/.cache"),
    Pause(250),
    Output("3.1G\t/home/anu/Videos"),
    Pause(400),
    Done(true),
    Thinking,
    Pause(1300),
    Usage(1610, 1240, 41, 18.0),
    Step("exec", "ls -lhS ~/Downloads | head -4"),
    Pause(350),
    Output("-rw-r--r--. 1 anu anu 2.4G Nov 20 Fedora-45-x86_64.iso"),
    Output("-rw-r--r--. 1 anu anu 2.4G Nov 12 Fedora-45-x86_64 (1).iso"),
    Pause(300),
    Done(true),
    Plan(&[("Find the biggest folders", true), ("Suggest what's safe to clear", false)]),
    Thinking,
    Pause(1100),
    Text(ANSWER_1),
    Usage(1890, 1610, 236, 17.6),
    Finish,
    Type("/chat", false),
    Say("Conversation started. It remembers what you say until /exit."),
    Type("yes, clear them", true),
    Thinking,
    Waiting("reading 2,310 tokens"),
    Pause(1400),
    Usage(2310, 1890, 52, 18.2),
    Step("exec", "rm -rf ~/.cache/thumbnails ~/.cache/pip && du -sh ~/.cache"),
    Pause(900),
    Output("41M\t/home/anu/.cache"),
    Done(true),
    Step("exec", "flatpak uninstall --unused -y"),
    Pause(700),
    Output("Looking for unused refs…"),
    Pause(500),
    Output("error: Ongoing operation on org.gnome.Platform//47; try again later"),
    Done(false),
    Thinking,
    Pause(1000),
    Text(ANSWER_2),
    Usage(2480, 2310, 61, 18.1),
    Plan(&[("Find the biggest folders", true), ("Suggest what's safe to clear", true)]),
    Finish,
];

pub fn run() -> anyhow::Result<()> {
    let mode = Mode::detect();
    let width = trip::tui::terminal_width();
    // Live in a terminal; at once (no pauses) when recorded to a file.
    let pause = |ms: u64| {
        if mode.live {
            sleep(Duration::from_millis(ms));
        }
    };
    let info = Info {
        model: "Qwen3.5 4B".into(),
        place: "On this computer".into(),
        context: 32768,
        ready: true,
        ..Info::default()
    };
    println!();
    for row in shell::header(&info, mode, width) {
        println!("{row}");
    }
    println!();

    let mut ui: Option<Terminal> = None;
    let mut session = (0usize, 0usize);
    for event in SCRIPT {
        match event {
            Type(line, chat) => {
                type_line(line, *chat, mode, &pause);
                if !line.starts_with('/') {
                    println!();
                    ui = Some(Terminal::new(None));
                }
            }
            Say(text) => {
                if mode.color {
                    println!("{}{DIM}· {text}{RESET}\n", " ".repeat(GUTTER));
                } else {
                    println!("· {text}\n");
                }
            }
            Pause(ms) => pause(*ms),
            Finish => {
                if let Some(mut t) = ui.take() {
                    t.finish(None);
                }
                println!();
                pause(900);
            }
            other => {
                let Some(t) = ui.as_mut() else { continue };
                match other {
                    Thinking => t.thinking(),
                    Waiting(what) => t.waiting(what),
                    Plan(steps) => {
                        let steps: Vec<PlanStep> =
                            steps.iter().map(|(text, done)| PlanStep { text: text.to_string(), done: *done }).collect();
                        t.plan(&steps);
                    }
                    Step(label, detail) => t.step(label, detail),
                    Output(line) => {
                        t.output(line);
                        pause(120);
                    }
                    Done(ok) => t.step_done(*ok),
                    Text(text) => stream(t, text, &pause),
                    Usage(prompt, cached, completion, tps) => {
                        session.0 += prompt;
                        session.1 += completion;
                        t.usage(&trip::model::Usage {
                            prompt: *prompt,
                            cached: *cached,
                            completion: *completion,
                            context: 32768,
                            tokens_per_sec: *tps,
                            session_prompt: session.0,
                            session_completion: session.1,
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    let chat = Info { chat: true, fill: Some(8), ..info };
    if mode.color {
        println!("{DIM}{}{RESET}", chat.footer(width));
    } else {
        println!("{}", chat.footer(width));
    }
    Ok(())
}

/// The prompt, the line typed out, then the line as it stays in the scrollback ("You …").
fn type_line(line: &str, chat: bool, mode: Mode, pause: &dyn Fn(u64)) {
    let mut out = std::io::stdout();
    let label = format!("{:<w$}", if chat { "Chat" } else { "AI" }, w = GUTTER - 2);
    if mode.live {
        let _ = write!(out, "\x1b[38;2;70;160;118m{label}\x1b[38;2;87;217;154m› {RESET}");
        let _ = out.flush();
        pause(500);
        for c in line.chars() {
            let _ = write!(out, "{c}");
            let _ = out.flush();
            pause(45);
        }
        pause(350);
        let _ = write!(out, "\r\x1b[2K");
    }
    if mode.color {
        println!("{DIM}You{RESET}{}{line}", " ".repeat(GUTTER - 3));
    } else {
        println!("You: {line}");
    }
}

/// Sends the text in small pieces, the way a model streams it.
fn stream(t: &mut Terminal, text: &str, pause: &dyn Fn(u64)) {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut n = 0;
    while i < chars.len() {
        let len = 1 + (n * 7 % 4);
        let piece: String = chars[i..(i + len).min(chars.len())].iter().collect();
        t.text(&piece);
        i += len;
        n += 1;
        pause(22);
    }
}
