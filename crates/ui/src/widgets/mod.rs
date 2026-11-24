//! Desktop widgets anyone (or the AI) can make: a folder with a `widget.toml` and a script in
//! any language that prints a little JSON. `tripppin-ui widgets` is the host that draws them
//! on the desktop in our own style; the other subcommands manage them. docs/WIDGETS.md has
//! the schema and examples (installed as /usr/share/doc/tripppin/WIDGETS.md).
//!
//! Widgets live in ~/.local/share/tripppin/widgets/<name>/. Where one was dragged or pinned
//! is kept apart, in ~/.config/tripppin/widgets.json, so a widget's own files are never
//! rewritten.

mod host;
pub mod spec;
mod system;

use gtk::glib::ExitCode;
use spec::{Config, Placement};
use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// Where widget folders live.
pub fn base_dir() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME").map_or_else(|| home().join(".local/share"), PathBuf::from);
    data.join("tripppin/widgets")
}

fn placements_path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home().join(".config"), PathBuf::from);
    config.join("tripppin/widgets.json")
}

pub fn load_placements() -> BTreeMap<String, Placement> {
    std::fs::read_to_string(placements_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_placements(p: &BTreeMap<String, Placement>) {
    let path = placements_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    if let Ok(text) = serde_json::to_string_pretty(p) {
        std::fs::write(path, text).ok();
    }
}

/// Each widget folder's name and its widget.toml, read (or why it couldn't be).
pub fn installed() -> Vec<(String, PathBuf, Result<Config, String>)> {
    let mut out: Vec<_> = std::fs::read_dir(base_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("widget.toml").is_file())
        .map(|e| {
            let dir = e.path();
            let config = std::fs::read_to_string(dir.join("widget.toml"))
                .map_err(|e| format!("widget.toml: {e}"))
                .and_then(|t| Config::parse(&t));
            (e.file_name().to_string_lossy().into_owned(), dir, config)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The last result of each widget, written by the host, so `list` can say how it's doing.
fn status_path(name: &str) -> PathBuf {
    let dir = crate::common::runtime_dir().join("widgets");
    std::fs::create_dir_all(&dir).ok();
    dir.join(format!("{name}.txt"))
}

pub fn write_status(name: &str, line: &str) {
    std::fs::write(status_path(name), line).ok();
}

/// Runs a widget's command once, as the host does, and reads what it printed.
pub fn run_once(dir: &Path, config: &Config) -> Result<spec::Shown, String> {
    let mut child = Command::new("sh")
        .args(["-c", &config.command])
        .current_dir(dir)
        .env("TRIPPPIN_WIDGET_DIR", dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("couldn't start: {e}"))?;
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = out.as_mut() {
            o.read_to_string(&mut s).ok();
        }
        s
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err.as_mut() {
            e.read_to_string(&mut s).ok();
        }
        s
    });
    let deadline = Instant::now() + Duration::from_secs(config.timeout_secs());
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                // The whole group: sh and whatever it started (curl, python…).
                Command::new("kill").args(["-KILL", "--", &format!("-{}", child.id())]).status().ok();
                child.kill().ok();
                child.wait().ok();
                return Err(format!("took longer than {} s", config.timeout_secs()));
            }
        }
    };
    let stdout = reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    result(status.success(), status.code(), &stdout, &stderr)
}

/// What a finished run means: its output read, or the error to show in the widget.
pub fn result(success: bool, code: Option<i32>, stdout: &str, stderr: &str) -> Result<spec::Shown, String> {
    if !success {
        let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        let code = code.map_or_else(|| "was stopped".to_string(), |c| format!("exited with {c}"));
        return Err(if last.is_empty() { format!("the script {code}") } else { format!("the script {code}: {last}") });
    }
    spec::parse_output(stdout)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// connector name, and a description, of each screen niri has.
fn outputs() -> Vec<(String, String)> {
    let out = Command::new("niri").args(["msg", "-j", "outputs"]).output().ok();
    let v: serde_json::Value = out.and_then(|o| serde_json::from_slice(&o.stdout).ok()).unwrap_or_default();
    let mut list: Vec<(String, String)> = v
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, o)| {
            let make = o["make"].as_str().unwrap_or("");
            let model = o["model"].as_str().unwrap_or("");
            let size = format!(
                "{}x{}",
                o["logical"]["width"].as_i64().unwrap_or(0),
                o["logical"]["height"].as_i64().unwrap_or(0)
            );
            (name.clone(), format!("{make} {model}, {size}").trim().to_string())
        })
        .collect();
    list.sort();
    list
}

fn find(name: &str) -> Option<(PathBuf, Result<Config, String>)> {
    installed().into_iter().find(|(n, _, _)| n == name).map(|(_, d, c)| (d, c))
}

fn check_line(dir: &Path, config: &Result<Config, String>) -> String {
    match config {
        Err(e) => format!("broken: {e}"),
        Ok(c) => match run_once(dir, c) {
            Ok(s) => format!("works ({})", spec::summary(&s)),
            Err(e) => format!("error: {e}"),
        },
    }
}

const SYSTEM_TOML: &str = "# The built-in System widget: CPU, memory, GPU, temperature and the local AI model.\n\
name = \"System\"\n\
command = \"tripppin-ui widgets system-json\"\n\
every = 3\n\
anchor = \"top-right\"\n\
x = 24\n\
y = 24\n";

const USAGE: &str = "usage: tripppin-ui widgets [run] | add <folder> | add-system | list | test <name or folder> | remove <name> | pin <name> <output> | outputs | system-json | guide";

/// `tripppin-ui widgets …`.
pub fn cli(args: &[String]) -> ExitCode {
    let arg = |i: usize| args.get(i).map(String::as_str);
    match arg(0) {
        None | Some("run") => host::run(),
        Some("system-json") => {
            println!("{}", system::report());
            ExitCode::SUCCESS
        }
        Some("add") => {
            let Some(from) = arg(1).map(PathBuf::from) else { return usage() };
            let from = from.canonicalize().unwrap_or(from);
            let config = match std::fs::read_to_string(from.join("widget.toml")) {
                Ok(t) => Config::parse(&t),
                Err(e) => {
                    eprintln!("no widget.toml in {}: {e}", from.display());
                    return ExitCode::FAILURE;
                }
            };
            if let Err(e) = &config {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
            let name = spec::slug(&from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            if name.is_empty() {
                eprintln!("the folder needs a name made of letters, digits or dashes");
                return ExitCode::FAILURE;
            }
            let to = base_dir().join(&name);
            if to != from {
                std::fs::remove_dir_all(&to).ok();
                if let Err(e) = copy_dir(&from, &to) {
                    eprintln!("couldn't copy the widget: {e}");
                    return ExitCode::FAILURE;
                }
            }
            println!("added {name}: {}", check_line(&to, &config));
            println!("it shows on the desktop within a few seconds (tripppin-ui widgets runs from the session)");
            ExitCode::SUCCESS
        }
        Some("add-system") => {
            let dir = base_dir().join("system");
            if std::fs::create_dir_all(&dir)
                .and_then(|()| std::fs::write(dir.join("widget.toml"), SYSTEM_TOML))
                .is_err()
            {
                eprintln!("couldn't write {}", dir.display());
                return ExitCode::FAILURE;
            }
            println!("added system");
            ExitCode::SUCCESS
        }
        Some("list") => {
            let placements = load_placements();
            let all = installed();
            if all.is_empty() {
                println!("no widgets yet (folders go in {})", base_dir().display());
            }
            for (name, _, config) in all {
                match config {
                    Err(e) => println!("{name}: broken: {e}"),
                    Ok(c) => {
                        let p = spec::place(&c, placements.get(&name));
                        let last = std::fs::read_to_string(status_path(&name)).unwrap_or_else(|_| "not run yet".into());
                        println!(
                            "{name}: on {}, {} corner ({} and {} px in), every {} s; last run: {}",
                            p.output.as_deref().unwrap_or("the first screen"),
                            p.anchor.name(),
                            p.x,
                            p.y,
                            c.every_secs(),
                            last.trim()
                        );
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Some("test") => {
            let Some(what) = arg(1) else { return usage() };
            let dir = if Path::new(what).join("widget.toml").is_file() {
                PathBuf::from(what)
            } else if let Some((d, _)) = find(what) {
                d
            } else {
                eprintln!("no widget called {what}");
                return ExitCode::FAILURE;
            };
            let config = std::fs::read_to_string(dir.join("widget.toml"))
                .map_err(|e| e.to_string())
                .and_then(|t| Config::parse(&t));
            let shown = config.and_then(|c| run_once(&dir, &c));
            match shown {
                Ok(s) => {
                    println!("works ({})", spec::summary(&s));
                    if let Some(t) = &s.title {
                        println!("title: {t}");
                    }
                    for i in &s.items {
                        println!("  {i:?}");
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("remove") => {
            let Some(name) = arg(1) else { return usage() };
            let Some((dir, _)) = find(name) else {
                eprintln!("no widget called {name}");
                return ExitCode::FAILURE;
            };
            std::fs::remove_dir_all(dir).ok();
            let mut p = load_placements();
            if p.remove(name).is_some() {
                save_placements(&p);
            }
            println!("removed {name}");
            ExitCode::SUCCESS
        }
        Some("pin") => {
            let (Some(name), Some(output)) = (arg(1), arg(2)) else { return usage() };
            if find(name).is_none() {
                eprintln!("no widget called {name}");
                return ExitCode::FAILURE;
            }
            let known = outputs();
            if !known.is_empty() && !known.iter().any(|(n, _)| n == output) {
                let names: Vec<&str> = known.iter().map(|(n, _)| n.as_str()).collect();
                eprintln!("no screen called {output}; the screens are: {}", names.join(", "));
                return ExitCode::FAILURE;
            }
            let mut p = load_placements();
            p.entry(name.to_string()).or_default().output = Some(output.to_string());
            save_placements(&p);
            println!("{name} is on {output}");
            ExitCode::SUCCESS
        }
        Some("outputs") => {
            for (name, about) in outputs() {
                println!("{name}: {about}");
            }
            ExitCode::SUCCESS
        }
        Some("guide") => {
            match std::fs::read_to_string("/usr/share/doc/tripppin/WIDGETS.md") {
                Ok(t) => print!("{t}"),
                Err(_) => println!("see docs/WIDGETS.md in the tripppin repository"),
            }
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_run_says_why() {
        assert_eq!(
            result(false, Some(1), "", "Traceback…\nNameError: x\n\n").unwrap_err(),
            "the script exited with 1: NameError: x"
        );
        assert_eq!(result(false, None, "", "").unwrap_err(), "the script was stopped");
        assert!(result(true, Some(0), "{\"title\":\"a\"}", "").is_ok());
    }

    #[test]
    fn runs_a_script_once() {
        let dir = std::env::temp_dir().join(format!("tripppin-widget-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("w.sh"), "echo '{\"title\":\"hi\",\"items\":[{\"label\":\"a\",\"value\":1}]}'")
            .unwrap();
        let c = Config::parse("command = \"sh w.sh\"").unwrap();
        let s = run_once(&dir, &c).unwrap();
        assert_eq!(s.title.as_deref(), Some("hi"));
        let slow = Config::parse("command = \"sleep 5\"\ntimeout = 1").unwrap();
        assert_eq!(run_once(&dir, &slow).unwrap_err(), "took longer than 1 s");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_system_widget_toml_is_valid() {
        let c = Config::parse(SYSTEM_TOML).unwrap();
        assert_eq!(c.every_secs(), 3);
    }
}
