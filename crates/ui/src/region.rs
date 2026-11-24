//! Date, time and language through systemd (timedatectl, localectl; polkit lets
//! administrators change them without a password), and the 12/24-hour clock choice in
//! ~/.config/tripppin/clock, which the bar and our screens read.

use std::path::PathBuf;
use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_lowercase())
    }
}

/// `timedatectl show` is KEY=value lines.
pub fn show_value(out: &str, key: &str) -> Option<String> {
    out.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')).map(str::to_string))
}

pub struct Time {
    pub zone: String,
    pub automatic: bool,
}

pub fn time() -> Time {
    let out = run("timedatectl", &["show"]).unwrap_or_default();
    Time {
        zone: show_value(&out, "Timezone").unwrap_or_else(|| "UTC".into()),
        automatic: show_value(&out, "NTP").is_some_and(|v| v == "yes"),
    }
}

pub fn zones() -> Vec<String> {
    run("timedatectl", &["list-timezones"]).map(|o| o.lines().map(str::to_string).collect()).unwrap_or_default()
}

pub fn set_zone(zone: &str) -> Result<(), String> {
    run("timedatectl", &["set-timezone", zone]).map(|_| ())
}

pub fn set_automatic(on: bool) -> Result<(), String> {
    run("timedatectl", &["set-ntp", if on { "true" } else { "false" }]).map(|_| ())
}

/// "Asia/Kolkata" → "kolkata · asia"
pub fn zone_label(zone: &str) -> String {
    match zone.rsplit_once('/') {
        Some((area, city)) => format!("{} · {}", city.replace('_', " "), area.replace('_', " ")).to_lowercase(),
        None => zone.to_lowercase(),
    }
}

fn clock_file() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("tripppin/clock")
}

pub fn twelve_hour() -> bool {
    std::fs::read_to_string(clock_file()).is_ok_and(|t| t.trim() == "12")
}

pub fn set_twelve_hour(on: bool) -> std::io::Result<()> {
    let f = clock_file();
    if let Some(dir) = f.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(f, if on { "12\n" } else { "24\n" })
}

/// Languages people pick from, as (locale, name). Only those installed show up.
pub const LANGUAGES: [(&str, &str); 17] = [
    ("C.UTF-8", "English (basic)"),
    ("en_IN.UTF-8", "English (India)"),
    ("en_US.UTF-8", "English (US)"),
    ("en_GB.UTF-8", "English (UK)"),
    ("hi_IN.UTF-8", "हिन्दी · hindi"),
    ("bn_IN.UTF-8", "বাংলা · bengali"),
    ("ta_IN.UTF-8", "தமிழ் · tamil"),
    ("te_IN.UTF-8", "తెలుగు · telugu"),
    ("mr_IN.UTF-8", "मराठी · marathi"),
    ("kn_IN.UTF-8", "ಕನ್ನಡ · kannada"),
    ("ml_IN.UTF-8", "മലയാളം · malayalam"),
    ("gu_IN.UTF-8", "ગુજરાતી · gujarati"),
    ("pa_IN.UTF-8", "ਪੰਜਾਬੀ · punjabi"),
    ("es_ES.UTF-8", "Español · Spanish"),
    ("fr_FR.UTF-8", "Français · French"),
    ("de_DE.UTF-8", "Deutsch · German"),
    ("pt_BR.UTF-8", "Português · Portuguese"),
];

/// `locale -a` prints "en_US.utf8", or just "hi_IN" for languages only built as UTF-8;
/// the names above use "en_US.UTF-8".
pub fn normalise(locale: &str) -> String {
    if locale.contains('.') || locale.contains('@') || !locale.contains('_') {
        locale.replace(".utf8", ".UTF-8")
    } else {
        format!("{locale}.UTF-8")
    }
}

pub fn installed_languages() -> Vec<(&'static str, &'static str)> {
    let have: Vec<String> = run("locale", &["-a"]).map(|o| o.lines().map(normalise).collect()).unwrap_or_default();
    LANGUAGES.iter().copied().filter(|(l, _)| have.iter().any(|h| h == l)).collect()
}

/// (LANG, LC_TIME) from `localectl status`.
pub fn locale() -> (String, String) {
    let out = run("localectl", &["status"]).unwrap_or_default();
    let get = |key: &str| {
        out.split_whitespace().find_map(|w| w.strip_prefix(key).and_then(|r| r.strip_prefix('=')).map(str::to_string))
    };
    let lang = get("LANG").unwrap_or_else(|| "C.UTF-8".into());
    let time = get("LC_TIME").unwrap_or_else(|| lang.clone());
    (lang, time)
}

/// Sets the language and the region formats (dates, numbers, money) for the whole computer.
/// Takes effect at the next login.
pub fn set_locale(lang: &str, formats: &str) -> Result<(), String> {
    let mut args = vec!["set-locale".to_string(), format!("LANG={lang}")];
    if formats != lang {
        for k in ["LC_TIME", "LC_NUMERIC", "LC_MONETARY", "LC_MEASUREMENT", "LC_PAPER"] {
            args.push(format!("{k}={formats}"));
        }
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run("localectl", &args).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_systemd() {
        let out = "Timezone=Asia/Kolkata\nNTP=yes\nNTPSynchronized=yes\n";
        assert_eq!(show_value(out, "Timezone").as_deref(), Some("Asia/Kolkata"));
        assert_eq!(show_value(out, "NTP").as_deref(), Some("yes"));
        assert_eq!(zone_label("America/New_York"), "new york · america");
        assert_eq!(zone_label("UTC"), "utc");
        assert_eq!(normalise("hi_IN.utf8"), "hi_IN.UTF-8");
        assert_eq!(normalise("hi_IN"), "hi_IN.UTF-8");
        assert_eq!(normalise("C.utf8"), "C.UTF-8");
        assert_eq!(normalise("POSIX"), "POSIX");
    }
}
