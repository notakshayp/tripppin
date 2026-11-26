//! The AI's everyday tools beyond running commands: the web, packages, system info, files,
//! services, widgets, the calendar, settings and the model in use.
//!
//! Each is a typed tool with a short schema (small local models cope better with few, plain
//! fields). Their names use underscores on the wire (`web_search`) and are shown as `web.search`.
//! Risky ones (installing, removing, overwriting, services) ask the person first; what asks is a
//! setting (`ask-before`), and "allow always" turns one kind off.

use crate::agent::Ui;
use crate::{apk, paths, sysx, web, widgets};
use serde_json::{Map, Value, json};
use std::sync::atomic::AtomicBool;

/// What a risky tool call wants, for the "needs your ok" box.
#[derive(Clone, Debug, PartialEq)]
pub struct Confirm {
    /// The kind it falls under (`installs`, `deletes`, `writes`, `services`, `settings`).
    pub kind: &'static str,
    pub title: String,
    pub lines: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Allow, and stop asking about this kind.
    AllowAlways,
    Deny,
}

impl Decision {
    pub fn allowed(self) -> bool {
        self != Decision::Deny
    }
}

/// The kinds of action that can ask first, with the words Settings shows.
pub const KINDS: [(&str, &str); 5] = [
    ("installs", "installing"),
    ("deletes", "deleting"),
    ("writes", "overwriting files"),
    ("services", "services"),
    ("settings", "changing settings"),
];

fn ask_file() -> std::path::PathBuf {
    paths::config_dir().join("ask-before")
}

/// The kinds that ask first. Everything, until the person changes it.
pub fn ask_before() -> Vec<String> {
    match std::fs::read_to_string(ask_file()) {
        Ok(t) => t.split_whitespace().map(str::to_string).collect(),
        Err(_) => KINDS.iter().map(|k| k.0.to_string()).collect(),
    }
}

pub fn set_ask_before(kinds: &[String]) {
    let f = ask_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, format!("{}\n", kinds.join(" ")));
}

pub fn allow_always(kind: &str) {
    let k: Vec<String> = ask_before().into_iter().filter(|x| x != kind).collect();
    set_ask_before(&k);
}

/// The tools' JSON schemas, added to the agent's own.
pub fn schemas() -> Vec<Value> {
    let t = |name: &str, desc: &str, props: &[(&str, &str, &str)], req: &[&str]| {
        let p: Map<String, Value> =
            props.iter().map(|(n, ty, d)| (n.to_string(), json!({ "type": ty, "description": d }))).collect();
        json!({"type": "function", "function": {"name": name, "description": desc,
            "parameters": {"type": "object", "properties": p, "required": req}}})
    };
    vec![
        t(
            "web_search",
            "Search the internet. Use this for anything you don't know or that may have changed.",
            &[("query", "string", "What to search for.")],
            &["query"],
        ),
        t(
            "web_read",
            "Open a web page and read its text.",
            &[("url", "string", "A full http(s) address, from web_search.")],
            &["url"],
        ),
        t("pkg_search", "Search installable packages.", &[("query", "string", "Name or what it does.")], &["query"]),
        t("pkg_add", "Install packages (asks first).", &[("names", "string", "Package names, space separated.")], &["names"]),
        t("pkg_remove", "Remove packages (asks first).", &[("names", "string", "Package names, space separated.")], &["names"]),
        t("sys_info", "Memory, cpu, disk, battery, Wi-Fi and the model in use.", &[], &[]),
        t("list_files", "List a folder.", &[("path", "string", "Folder path; ~ is home.")], &["path"]),
        t(
            "service",
            "Check, start, stop or restart a system service (start/stop/restart ask first).",
            &[("name", "string", "Service name."), ("action", "string", "status, start, stop or restart.")],
            &["name", "action"],
        ),
        t(
            "widget_create",
            "Add a widget to the home screen: a shell command whose output is shown as text, refreshed every few seconds.",
            &[
                ("name", "string", "Short title."),
                ("command", "string", "Shell command that prints a few short lines."),
                ("every_secs", "integer", "Refresh interval, at least 5."),
            ],
            &["name", "command"],
        ),
        t("widget_remove", "Remove a home screen widget.", &[("name", "string", "Its title.")], &["name"]),
        t(
            "calendar_add",
            "Add an event to the calendar.",
            &[
                ("title", "string", "What it is."),
                ("when", "string", "Local time as YYYY-MM-DD HH:MM."),
                ("minutes", "integer", "How long, default 60."),
            ],
            &["title", "when"],
        ),
        t(
            "setting",
            "Change a setting: volume (0-100), brightness (0-100), mute (on/off), wifi (value = network name).",
            &[
                ("name", "string", "volume, brightness, mute or wifi."),
                ("value", "string", "The new value."),
                ("password", "string", "Wi-Fi password, if the network has one."),
            ],
            &["name", "value"],
        ),
        t("model_switch", "Use another downloaded model for the next message.", &[("name", "string", "Part of its name.")], &["name"]),
    ]
}

pub const NAMES: &[&str] = &[
    "web_search",
    "web_read",
    "pkg_search",
    "pkg_add",
    "pkg_remove",
    "sys_info",
    "list_files",
    "service",
    "widget_create",
    "widget_remove",
    "calendar_add",
    "setting",
    "model_switch",
];

pub fn is_ext(name: &str) -> bool {
    NAMES.contains(&name)
}

pub fn example(name: &str) -> &'static str {
    match name {
        "web_search" => r#"{"query": "python scraping library comparison"}"#,
        "web_read" => r#"{"url": "https://example.com/docs"}"#,
        "pkg_search" => r#"{"query": "browser"}"#,
        "pkg_add" | "pkg_remove" => r#"{"names": "python3 py3-pip"}"#,
        "list_files" => r#"{"path": "~/Downloads"}"#,
        "service" => r#"{"name": "sshd", "action": "status"}"#,
        "widget_create" => r#"{"name": "uptime", "command": "uptime -p", "every_secs": 60}"#,
        "widget_remove" => r#"{"name": "uptime"}"#,
        "calendar_add" => r#"{"title": "dentist", "when": "2026-12-01 15:30"}"#,
        "setting" => r#"{"name": "volume", "value": "40"}"#,
        "model_switch" => r#"{"name": "9b"}"#,
        _ => "{}",
    }
}

/// Checks an extension call's fields. Returns the arguments cleaned up, or what to tell the model.
pub fn check(name: &str, args: &Map<String, Value>) -> Result<Value, String> {
    let text = |k: &str| -> Result<String, String> {
        match args.get(k) {
            Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.trim().to_string()),
            Some(Value::String(_)) => Err(format!("\"{k}\" for {name} is empty.")),
            Some(Value::Array(a)) if k == "names" => {
                Ok(a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))
            }
            Some(Value::Number(n)) => Ok(n.to_string()),
            Some(_) => Err(format!("\"{k}\" for {name} must be a string.")),
            None => Err(format!("{name} needs a \"{k}\" field. For example: {}.", example(name))),
        }
    };
    let opt_num = |k: &str, default: u64| -> Result<u64, String> {
        match args.get(k) {
            None | Some(Value::Null) => Ok(default),
            Some(Value::Number(n)) => n.as_f64().filter(|v| *v >= 0.0).map(|v| v as u64).ok_or(format!("\"{k}\" must be a positive number.")),
            Some(Value::String(s)) => s.trim().parse().map_err(|_| format!("\"{k}\" must be a number.")),
            Some(_) => Err(format!("\"{k}\" must be a number.")),
        }
    };
    Ok(match name {
        "web_search" => json!({"query": text("query")?}),
        "web_read" => {
            let url = text("url")?;
            web::check_url(&url).map_err(|e| format!("{e} Use an address from web_search."))?;
            json!({"url": url})
        }
        "pkg_search" => json!({"query": text("query")?}),
        "pkg_add" | "pkg_remove" => {
            let names = text("names")?.replace(',', " ");
            let list: Vec<&str> = names.split_whitespace().collect();
            if list.is_empty() || list.len() > 12 || !list.iter().all(|n| apk::valid_name(n)) {
                return Err("\"names\" must be 1 to 12 package names, like python3 py3-pip.".into());
            }
            json!({"names": list.join(" ")})
        }
        "sys_info" => json!({}),
        "list_files" => json!({"path": text("path")?}),
        "service" => {
            let (n, a) = (text("name")?, text("action")?.to_lowercase());
            if !["status", "start", "stop", "restart"].contains(&a.as_str()) {
                return Err("\"action\" must be status, start, stop or restart.".into());
            }
            if !apk::valid_name(&n) {
                return Err("\"name\" must be a service name like sshd.".into());
            }
            json!({"name": n, "action": a})
        }
        "widget_create" => {
            let n = text("name")?;
            if !widgets::valid_name(&n) {
                return Err("the widget \"name\" must be letters, numbers, spaces or dashes.".into());
            }
            json!({"name": n, "command": text("command")?, "every_secs": opt_num("every_secs", 60)?})
        }
        "widget_remove" => json!({"name": text("name")?}),
        "calendar_add" => json!({"title": text("title")?, "when": text("when")?, "minutes": opt_num("minutes", 60)?}),
        "setting" => {
            let n = text("name")?.to_lowercase();
            if !["volume", "brightness", "mute", "wifi"].contains(&n.as_str()) {
                return Err("\"name\" must be volume, brightness, mute or wifi.".into());
            }
            let v = text("value")?;
            if matches!(n.as_str(), "volume" | "brightness") && v.trim_end_matches('%').parse::<u8>().is_err() {
                return Err(format!("\"value\" for {n} must be a number from 0 to 100."));
            }
            json!({"name": n, "value": v, "password": args.get("password").and_then(Value::as_str).unwrap_or("")})
        }
        "model_switch" => json!({"name": text("name")?}),
        _ => return Err(format!("There's no tool called \"{name}\".")),
    })
}

fn s<'a>(a: &'a Value, k: &str) -> &'a str {
    a[k].as_str().unwrap_or("")
}

/// The label and one-line detail shown for a call.
pub fn describe(name: &str, a: &Value) -> (String, String) {
    let label = name.replacen('_', ".", 1);
    let detail = match name {
        "web_search" | "pkg_search" => format!("\"{}\"", s(a, "query")),
        "web_read" => s(a, "url").to_string(),
        "pkg_add" | "pkg_remove" => s(a, "names").to_string(),
        "list_files" => s(a, "path").to_string(),
        "service" => format!("{} {}", s(a, "name"), s(a, "action")),
        "widget_create" => format!("{} every {}s: {}", s(a, "name"), a["every_secs"], s(a, "command")),
        "widget_remove" | "model_switch" => s(a, "name").to_string(),
        "calendar_add" => format!("{} at {}", s(a, "title"), s(a, "when")),
        "setting" => format!("{} {}", s(a, "name"), s(a, "value")),
        _ => String::new(),
    };
    (label, detail)
}

/// The shell command a call runs, for the tools that are one (they go through the agent's exec).
pub fn to_exec(name: &str, a: &Value) -> Option<String> {
    let names: Vec<String> = s(a, "names").split_whitespace().map(str::to_string).collect();
    match name {
        "pkg_add" => Some(apk::add_command(&names)),
        "pkg_remove" => Some(apk::del_command(&names)),
        "service" if s(a, "action") != "status" => {
            Some(format!("{}rc-service {} {}", apk::privilege(), s(a, "name"), s(a, "action")))
        }
        _ => None,
    }
}

/// Whether a tool call asks first, and what to show.
pub fn needs_ok(name: &str, a: &Value) -> Option<Confirm> {
    let c = match name {
        "pkg_add" => Confirm {
            kind: "installs",
            title: format!("Install {} package{}?", s(a, "names").split_whitespace().count(), plural(s(a, "names"))),
            lines: vec![("packages".into(), s(a, "names").into()), ("from".into(), "alpine, signed".into()), ("undo".into(), "pkg.remove, any time".into())],
        },
        "pkg_remove" => Confirm {
            kind: "deletes",
            title: format!("Remove {}?", s(a, "names")),
            lines: vec![("packages".into(), s(a, "names").into())],
        },
        "service" if s(a, "action") != "status" => Confirm {
            kind: "services",
            title: format!("{} {}?", capital(s(a, "action")), s(a, "name")),
            lines: vec![("service".into(), s(a, "name").into()), ("action".into(), s(a, "action").into())],
        },
        "setting" if s(a, "name") == "wifi" => Confirm {
            kind: "settings",
            title: format!("Connect to {}?", s(a, "value")),
            lines: vec![("network".into(), s(a, "value").into())],
        },
        _ => return None,
    };
    ask_before().iter().any(|k| k == c.kind).then_some(c)
}

fn plural(names: &str) -> &'static str {
    if names.split_whitespace().count() == 1 { "" } else { "s" }
}

fn capital(w: &str) -> String {
    let mut c = w.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// Confirmation for the agent's older tools: exec (by what the command does) and write_file.
pub fn needs_ok_exec(command: &str) -> Option<Confirm> {
    let words: Vec<&str> = command.split(|c: char| c.is_whitespace() || ";&|".contains(c)).filter(|w| !w.is_empty()).collect();
    let has = |w: &str| words.contains(&w);
    let kind = if (has("apk") && (has("add") || has("upgrade") || has("fix"))) || has("pip") && has("install") {
        "installs"
    } else if has("rm") || has("rmdir") || has("shred") || (has("apk") && has("del")) || crate::permissions::is_risky(command) {
        "deletes"
    } else if has("rc-service") || has("rc-update") || has("reboot") || has("poweroff") || has("shutdown") {
        "services"
    } else if has("mv") || has("dd") || has("tee") {
        "writes"
    } else {
        return None;
    };
    let c = Confirm {
        kind,
        title: match kind {
            "installs" => "Install software?",
            "deletes" => "Delete something?",
            "services" => "Change a service?",
            _ => "Move or overwrite files?",
        }
        .into(),
        lines: vec![("command".into(), command.lines().next().unwrap_or("").chars().take(100).collect())],
    };
    ask_before().iter().any(|k| k == kind).then_some(c)
}

pub fn needs_ok_write(path: &str) -> Option<Confirm> {
    let p = paths::expand(path);
    let outside = std::env::var_os("HOME").is_some_and(|h| !p.starts_with(h));
    if !(p.exists() || outside) {
        return None;
    }
    let c = Confirm {
        kind: "writes",
        title: "Overwrite a file?".into(),
        lines: vec![("file".into(), path.into()), ("exists".into(), if p.exists() { "yes, it's replaced".into() } else { "no, new file".into() })],
    };
    ask_before().iter().any(|k| k == c.kind).then_some(c)
}

/// Runs the tools that aren't a shell command.
pub fn run(name: &str, a: &Value, _cancel: &AtomicBool, _ui: &mut dyn Ui) -> String {
    match name {
        "web_search" => match web::search(s(a, "query"), 8) {
            Ok(h) if h.is_empty() => format!("No results for \"{}\". Try different words.", s(a, "query")),
            Ok(h) => web::format_hits(&h),
            Err(e) => format!("Error: search failed: {e:#}. Check the internet connection."),
        },
        "web_read" => match web::read(s(a, "url")) {
            Ok(t) if t.trim().is_empty() => "The page has no readable text.".into(),
            Ok(t) => t,
            Err(e) => format!("Error: {e:#}"),
        },
        "pkg_search" => {
            let hits = apk::search(s(a, "query"));
            if hits.is_empty() {
                return format!("No packages found for \"{}\".", s(a, "query"));
            }
            hits.iter()
                .take(12)
                .map(|p| format!("{}{}{}: {}", p.name, if p.gui { " [app]" } else { "" }, if p.installed { " [installed]" } else { "" }, p.desc))
                .collect::<Vec<_>>()
                .join("\n")
        }
        "sys_info" => sys_info(),
        "list_files" => list_files(s(a, "path")),
        "service" => {
            let out = std::process::Command::new("rc-service").args([s(a, "name"), "status"]).output();
            match out {
                Ok(o) => format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)).trim().to_string(),
                Err(e) => format!("Error: rc-service didn't start: {e}"),
            }
        }
        "widget_create" => match widgets::create(s(a, "name"), s(a, "command"), a["every_secs"].as_u64().unwrap_or(60)) {
            Ok(w) => format!("Added the widget \"{}\" to the home screen; it refreshes every {} seconds.", w.name, w.every_secs),
            Err(e) => format!("Error: {e}"),
        },
        "widget_remove" => {
            if widgets::remove(s(a, "name")) {
                format!("Removed the widget \"{}\".", s(a, "name"))
            } else {
                format!("Error: no widget called \"{}\".", s(a, "name"))
            }
        }
        "calendar_add" => widgets::add_event(s(a, "title"), s(a, "when"), a["minutes"].as_u64().unwrap_or(60) as u32)
            .unwrap_or_else(|e| format!("Error: {e}")),
        "setting" => setting(a),
        "model_switch" => model_switch(s(a, "name")),
        _ => format!("Error: no tool called {name}."),
    }
}

fn setting(a: &Value) -> String {
    let v = s(a, "value");
    let pct = || v.trim_end_matches('%').parse::<u8>().unwrap_or(50).min(100);
    let ok = |done: bool, what: String| if done { what } else { format!("Error: couldn't change {}.", s(a, "name")) };
    match s(a, "name") {
        "volume" => ok(sysx::set_volume(pct()), format!("Volume is now {}%.", pct())),
        "brightness" => ok(sysx::set_brightness(pct()), format!("Brightness is now {}%.", pct())),
        "mute" => {
            let on = matches!(v, "on" | "true" | "1" | "yes");
            ok(sysx::set_mute(on), format!("Sound is {}.", if on { "muted" } else { "on" }))
        }
        "wifi" => match sysx::wifi_connect(v, Some(s(a, "password"))) {
            Ok(()) => format!("Connected to {v}."),
            Err(e) => format!("Error: couldn't connect to {v}: {e}"),
        },
        _ => "Error: unknown setting.".into(),
    }
}

fn model_switch(name: &str) -> String {
    let want = name.to_lowercase();
    let Ok(models) = crate::models::list() else { return "Error: can't read the model list.".into() };
    let Some(m) = models.iter().find(|m| m.installed && m.tier.model.to_lowercase().contains(&want)) else {
        let have: Vec<String> = models.iter().filter(|m| m.installed).map(|m| m.tier.model.clone()).collect();
        return format!("Error: no downloaded model matches \"{name}\". Downloaded: {}.", if have.is_empty() { "none".into() } else { have.join(", ") });
    };
    match crate::models::activate(&m.tier) {
        Ok(()) => format!("Now using {} (applies to the next message).", m.tier.model),
        Err(e) => format!("Error: {e:#}"),
    }
}

fn list_files(path: &str) -> String {
    let p = paths::expand(path);
    let Ok(rd) = std::fs::read_dir(&p) else { return format!("Error: can't open {path}.") };
    let mut rows: Vec<(bool, String, u64)> = rd
        .flatten()
        .map(|e| {
            let m = e.metadata().ok();
            (m.as_ref().is_some_and(|m| m.is_dir()), e.file_name().to_string_lossy().into_owned(), m.map_or(0, |m| m.len()))
        })
        .collect();
    rows.sort_by(|a, b| (!a.0, a.1.to_lowercase()).cmp(&(!b.0, b.1.to_lowercase())));
    let total = rows.len();
    let mut out: Vec<String> = rows
        .iter()
        .take(60)
        .map(|(d, n, sz)| if *d { format!("{n}/") } else { format!("{n}  {}", apk::size_words(*sz)) })
        .collect();
    if total > 60 {
        out.push(format!("… and {} more", total - 60));
    }
    if out.is_empty() { "(empty folder)".into() } else { out.join("\n") }
}

pub fn sys_info() -> String {
    let mut prev = sysx::CpuCounters::default();
    let mut snap = sysx::Snapshot::default();
    snap = sysx::read_fast(&mut prev, &snap);
    sysx::read_slow(&mut snap);
    let (model, local) = crate::models::model_label();
    let mut out = vec![
        format!("host     {}", sysx::hostname()),
        format!("memory   {:.1} of {:.1} GB used", snap.mem_used_kib as f64 / 1048576.0, snap.mem_total_kib as f64 / 1048576.0),
        format!("load     {}", snap.load),
        format!("disk     {} of {} used", apk::size_words(snap.disk_used), apk::size_words(snap.disk_total)),
    ];
    if let Some((p, c)) = snap.battery {
        out.push(format!("battery  {p}%{}", if c { " charging" } else { "" }));
    }
    if let Some(w) = sysx::wifi_status() {
        out.push(format!("wi-fi    {w}"));
    }
    out.push(format!("model    {model} ({})", if local { "on this computer" } else { "cloud" }));
    if let Ok(k) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        out.push(format!("kernel   {}", k.trim()));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn every_tool_has_a_schema_and_an_example_that_passes() {
        let tools = schemas();
        assert_eq!(tools.len(), NAMES.len());
        for n in NAMES {
            assert!(tools.iter().any(|t| t["function"]["name"] == *n), "{n} has no schema");
            let ex: Value = serde_json::from_str(example(n)).unwrap();
            assert!(check(n, ex.as_object().unwrap()).is_ok(), "example for {n} fails its own check");
        }
        // Compact: all extra schemas together stay small for small models.
        assert!(Value::Array(tools).to_string().len() < 5500);
    }

    #[test]
    fn checks_arguments() {
        assert!(check("web_search", &obj(json!({}))).unwrap_err().contains("needs a \"query\""));
        assert!(check("web_read", &obj(json!({"url": "file:///etc/passwd"}))).is_err());
        assert!(check("pkg_add", &obj(json!({"names": "-rf /"}))).is_err());
        assert_eq!(check("pkg_add", &obj(json!({"names": ["a", "b"]}))).unwrap(), json!({"names": "a b"}));
        assert_eq!(check("pkg_add", &obj(json!({"names": "a, b"}))).unwrap(), json!({"names": "a b"}));
        assert!(check("service", &obj(json!({"name": "sshd", "action": "explode"}))).is_err());
        assert!(check("setting", &obj(json!({"name": "volume", "value": "loud"}))).is_err());
        assert_eq!(check("widget_create", &obj(json!({"name": "up", "command": "uptime"}))).unwrap()["every_secs"], 60);
        assert!(check("widget_create", &obj(json!({"name": "../x", "command": "uptime"}))).is_err());
    }

    #[test]
    fn labels_use_dots() {
        let (l, d) = describe("web_search", &json!({"query": "rust"}));
        assert_eq!((l.as_str(), d.as_str()), ("web.search", "\"rust\""));
        assert_eq!(describe("pkg_add", &json!({"names": "a b"})).0, "pkg.add");
        assert_eq!(describe("sys_info", &json!({})).0, "sys.info");
    }

    #[test]
    fn installs_and_removals_ask_and_are_commands() {
        let a = json!({"names": "python3 py3-pip"});
        let c = needs_ok("pkg_add", &a).unwrap();
        assert_eq!(c.kind, "installs");
        assert_eq!(c.title, "Install 2 packages?");
        assert!(to_exec("pkg_add", &a).unwrap().contains("apk add --no-progress python3 py3-pip"));
        assert_eq!(needs_ok("pkg_remove", &a).unwrap().kind, "deletes");
        assert!(needs_ok("service", &json!({"name": "sshd", "action": "status"})).is_none());
        assert_eq!(needs_ok("service", &json!({"name": "sshd", "action": "stop"})).unwrap().kind, "services");
        assert!(needs_ok("web_search", &json!({"query": "x"})).is_none());
        assert!(to_exec("service", &json!({"name": "sshd", "action": "status"})).is_none());
    }

    #[test]
    fn shell_commands_are_classified() {
        let kind = |c: &str| needs_ok_exec(c).map(|c| c.kind);
        assert_eq!(kind("apk add git"), Some("installs"));
        assert_eq!(kind("doas apk del git"), Some("deletes"));
        assert_eq!(kind("rm -rf ~/x"), Some("deletes"));
        assert_eq!(kind("rc-service sshd restart"), Some("services"));
        assert_eq!(kind("mv a b"), Some("writes"));
        assert_eq!(kind("ls -la"), None);
        assert_eq!(kind("grep apk notes.txt"), None);
    }

    #[test]
    fn lists_a_folder() {
        let out = list_files("/");
        assert!(out.lines().count() > 3);
        assert!(list_files("/definitely/not/here").starts_with("Error"));
    }
}
