//! People on this computer. Reading and changing your own name and picture goes through
//! AccountsService; anything an administrator does (passwords, new people, logging in
//! without a password) runs through `sudo` with the password the person just typed, so no
//! other program can do it quietly.

use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Person {
    pub path: String,
    pub login: String,
    pub name: String,
    pub picture: String,
    pub admin: bool,
}

fn busctl(args: &[&str]) -> Option<String> {
    let out = Command::new("busctl").arg("--system").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The `data` of each line of `busctl --json=short get-property` output.
pub fn parse_properties(out: &str) -> Vec<Value> {
    out.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).map(|v| v["data"].clone()).collect()
}

fn person(path: &str) -> Option<Person> {
    let out = busctl(&[
        "--json=short",
        "get-property",
        "org.freedesktop.Accounts",
        path,
        "org.freedesktop.Accounts.User",
        "UserName",
        "RealName",
        "IconFile",
        "AccountType",
    ])?;
    let p = parse_properties(&out);
    Some(Person {
        path: path.to_string(),
        login: p.first()?.as_str()?.to_string(),
        name: p.get(1).and_then(Value::as_str).unwrap_or("").to_string(),
        picture: p.get(2).and_then(Value::as_str).unwrap_or("").to_string(),
        admin: p.get(3).and_then(Value::as_i64) == Some(1),
    })
}

/// Everyone with a login, you first.
pub fn people() -> Vec<Person> {
    let me = std::env::var("USER").unwrap_or_default();
    let out = busctl(&[
        "--json=short",
        "call",
        "org.freedesktop.Accounts",
        "/org/freedesktop/Accounts",
        "org.freedesktop.Accounts",
        "ListCachedUsers",
    ])
    .unwrap_or_default();
    let paths: Vec<String> = serde_json::from_str::<Value>(out.trim())
        .ok()
        .and_then(|v| v["data"][0].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|p| p.as_str().map(str::to_string))
        .collect();
    let mut all: Vec<Person> = paths.iter().filter_map(|p| person(p)).collect();
    if !all.iter().any(|p| p.login == me)
        && let Some(path) = busctl(&[
            "call",
            "org.freedesktop.Accounts",
            "/org/freedesktop/Accounts",
            "org.freedesktop.Accounts",
            "FindUserByName",
            "s",
            &me,
        ])
        .and_then(|o| o.split('"').nth(1).map(str::to_string))
        && let Some(p) = person(&path)
    {
        all.push(p);
    }
    all.sort_by_key(|p| (p.login != me, p.login.clone()));
    all
}

fn call(path: &str, method: &str, value: &str) -> Result<(), String> {
    let out = Command::new("busctl")
        .args([
            "--system",
            "call",
            "org.freedesktop.Accounts",
            path,
            "org.freedesktop.Accounts.User",
            method,
            "s",
            value,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_lowercase()) }
}

pub fn set_name(p: &Person, name: &str) -> Result<(), String> {
    call(&p.path, "SetRealName", name)
}

pub fn set_picture(p: &Person, file: &str) -> Result<(), String> {
    call(&p.path, "SetIconFile", file)
}

/// Runs `args` as root, with `password` for sudo and `input` on the command's stdin.
fn as_admin(password: &str, args: &[&str], input: &str) -> Result<(), String> {
    let mut child = Command::new("sudo")
        .args(["-k", "-S", "-p", ""])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = write!(stdin, "{password}\n{input}");
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
    Err(if err.contains("incorrect password") || err.contains("try again") {
        "that password isn't right".to_string()
    } else if err.contains("not in the sudoers") || err.contains("not allowed") {
        "only an administrator can do this".to_string()
    } else {
        err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it didn't work").trim().to_string()
    })
}

/// A login name from a full name: "Anu Sharma" → "anu".
pub fn login_from(name: &str) -> String {
    name.split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

pub fn valid_login(login: &str) -> bool {
    login.len() <= 32
        && login.starts_with(|c: char| c.is_ascii_lowercase())
        && login.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub fn set_password(your_password: &str, login: &str, new: &str) -> Result<(), String> {
    if new.contains(['\n', ':']) {
        return Err("a password can't contain a colon or a new line".into());
    }
    as_admin(your_password, &["chpasswd"], &format!("{login}:{new}\n"))
}

pub fn add_person(your_password: &str, name: &str, login: &str, password: &str, admin: bool) -> Result<(), String> {
    if !valid_login(login) {
        return Err("a login is lower-case letters and numbers, starting with a letter".into());
    }
    let mut args = vec!["useradd", "-m", "-c", name];
    if admin {
        args.extend(["-G", "wheel"]);
    }
    args.push(login);
    as_admin(your_password, &args, "")?;
    set_password(your_password, login, password)
}

const GREETD: &str = "/etc/greetd/config.toml";

/// Who logs in without a password, from greetd's [initial_session].
pub fn auto_login_in(config: &str) -> Option<String> {
    let mut inside = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[initial_session]";
        } else if inside && let Some(v) = line.strip_prefix("user") {
            return v.trim_start().strip_prefix('=').map(|u| u.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// greetd's config with (or without) an [initial_session] for `login`.
pub fn with_auto_login(config: &str, login: Option<&str>) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            skipping = t == "[initial_session]";
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    let mut out = out.trim_end().to_string() + "\n";
    if let Some(login) = login {
        out.push_str(&format!("\n[initial_session]\ncommand = \"niri-session\"\nuser = \"{login}\"\n"));
    }
    out
}

pub fn auto_login() -> Option<String> {
    auto_login_in(&std::fs::read_to_string(GREETD).unwrap_or_default())
}

pub fn set_auto_login(your_password: &str, login: Option<&str>) -> Result<(), String> {
    let text = with_auto_login(&std::fs::read_to_string(GREETD).unwrap_or_default(), login);
    as_admin(your_password, &["tee", GREETD], &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        let props = "{\"type\":\"s\",\"data\":\"anu\"}\n{\"type\":\"i\",\"data\":1}\n";
        assert_eq!(parse_properties(props), vec![Value::from("anu"), Value::from(1)]);
        assert_eq!(login_from("Anu Sharma"), "anu");
        assert!(valid_login("anu2") && !valid_login("2anu") && !valid_login("Anu"));
        let base = "[terminal]\nvt = 1\n\n[default_session]\ncommand = \"x\"\nuser = \"greetd\"\n";
        let on = with_auto_login(base, Some("anu"));
        assert_eq!(auto_login_in(&on).as_deref(), Some("anu"));
        assert_eq!(auto_login_in(base), None);
        let off = with_auto_login(&on, None);
        assert_eq!(auto_login_in(&off), None);
        assert!(off.contains("[default_session]") && off.contains("user = \"greetd\""));
    }
}
