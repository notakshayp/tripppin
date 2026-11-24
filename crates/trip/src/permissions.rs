//! What the AI may do as administrator: a level and, at "Allow what I pick", a set of areas.
//!
//! Settings → AI writes this through `/usr/libexec/tripppin-ai-sudo` (run with pkexec), which
//! keeps a root-owned policy file, `/etc/tripppin/ai-permissions`, and a sudoers rule made from
//! it. sudo enforces the rule; this module reads the policy back, describes it to the agent
//! (`summary`), and reads the activity log the agent's sudo wrapper writes (see `jobs.rs`).
//! See docs/AI-PERMISSIONS.md.

use std::collections::BTreeMap;
use std::path::PathBuf;

pub const POLICY_FILE: &str = "/etc/tripppin/ai-permissions";
/// Before the levels: the groups that ran without a password ("install services updates").
pub const LEGACY_FILE: &str = "/etc/tripppin/ai-sudo";
pub const HELPER: &str = "/usr/libexec/tripppin-ai-sudo";
/// Writes a file under /etc from standard input, for the "System settings files" area.
pub const ETC_WRITER: &str = "/usr/libexec/tripppin-etc-write";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    /// Every administrator command asks for the password in a pop-up.
    #[default]
    Ask,
    /// The picked areas run without a password; everything else asks.
    Pick,
    /// Everything runs without a password, except risky commands, which still ask.
    Full,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Ask, Level::Pick, Level::Full];

    pub fn id(self) -> &'static str {
        match self {
            Level::Ask => "ask",
            Level::Pick => "pick",
            Level::Full => "full",
        }
    }

    pub fn from_id(id: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Level::Ask => "Ask me every time",
            Level::Pick => "Allow what I pick",
            Level::Full => "Full administrator",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Level::Ask => {
                "When the AI needs administrator rights, a pop-up shows the command and asks for your password."
            }
            Level::Pick => "The areas you switch on run without asking. Anything else still asks for your password.",
            Level::Full => {
                "Administrator commands run without asking. Risky ones (deleting system files, formatting disks, changing users, turning off the firewall) still ask for your password."
            }
        }
    }
}

/// A group of administrator commands that can be allowed together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub id: &'static str,
    pub name: &'static str,
    pub about: &'static str,
    /// The commands it covers, in words, for the settings row.
    pub covers: &'static str,
    /// Switched on when someone first picks "Allow what I pick".
    pub default_on: bool,
}

/// The areas, in the order Settings shows them. The commands each one allows are in
/// `/usr/libexec/tripppin-ai-sudo`, which is what sudo enforces.
pub const AREAS: [Area; 9] = [
    Area {
        id: "apps",
        name: "Apps and packages",
        about: "Install, update and remove software.",
        covers: "dnf, flatpak, rpm-ostree",
        default_on: true,
    },
    Area {
        id: "services",
        name: "System services",
        about: "Start, stop and turn services on or off; read system logs.",
        covers: "systemctl, journalctl",
        default_on: true,
    },
    Area {
        id: "updates",
        name: "Updates",
        about: "Check for, apply and roll back system updates.",
        covers: "bootc",
        default_on: true,
    },
    Area {
        id: "network",
        name: "Network and firewall",
        about: "Change connections and open or close firewall ports.",
        covers: "nmcli, firewall-cmd",
        default_on: false,
    },
    Area {
        id: "hardware",
        name: "Hardware and drivers",
        about: "Load drivers and update device firmware.",
        covers: "modprobe, fwupdmgr, dmidecode",
        default_on: false,
    },
    Area {
        id: "disks",
        name: "Disks and storage",
        about: "Look at disks and mount or unmount them. Never formats or erases.",
        covers: "mount, umount, udisksctl mount/unmount, lsblk, blkid",
        default_on: false,
    },
    Area {
        id: "etc",
        name: "System settings files",
        about: "Edit settings files in /etc, keeping a backup. Not passwords, sudo or login files.",
        covers: "tripppin-etc-write",
        default_on: false,
    },
    Area {
        id: "users",
        name: "Users and groups",
        about: "Add, change and remove accounts and groups, and set passwords.",
        covers: "useradd, usermod, userdel, groupadd, passwd",
        default_on: false,
    },
    Area {
        id: "printers",
        name: "Printers",
        about: "Add, set up and remove printers.",
        covers: "lpadmin, cupsenable, cupsaccept",
        default_on: true,
    },
];

pub fn area(id: &str) -> Option<&'static Area> {
    AREAS.iter().find(|a| a.id == id)
}

/// Commands that still ask at "Full administrator" (sudo enforces this list; the pop-up
/// warns about them). Words as they'd appear at the start of the command sudo runs.
pub const RISKY: [&str; 22] = [
    "rm",
    "shred",
    "dd",
    "wipefs",
    "mkfs",
    "fdisk",
    "sfdisk",
    "gdisk",
    "sgdisk",
    "parted",
    "blkdiscard",
    "cryptsetup",
    "useradd",
    "usermod",
    "userdel",
    "groupadd",
    "groupmod",
    "groupdel",
    "gpasswd",
    "passwd",
    "chpasswd",
    "visudo",
];

/// Whether a command is one of the risky kind, as far as its words tell: one of `RISKY`, a
/// `mkfs.*`, formatting through udisksctl, or turning the firewall off.
pub fn is_risky(command: &str) -> bool {
    let words: Vec<&str> = command.split(|c: char| c.is_whitespace() || c == ';' || c == '&' || c == '|').collect();
    // A program, by its name or path (a path to a data file, like /etc/passwd, isn't one).
    let names: Vec<String> = words
        .iter()
        .filter(|w| !w.contains('/') || w.contains("bin/"))
        .map(|w| w.rsplit('/').next().unwrap_or(w).to_string())
        .collect();
    let has = |w: &str| names.iter().any(|n| n == w);
    names.iter().any(|n| RISKY.contains(&n.as_str()) || n.starts_with("mkfs."))
        || (has("udisksctl") && has("format"))
        || (has("systemctl") && (has("stop") || has("disable") || has("mask")) && command.contains("firewalld"))
        || (has("firewall-cmd") && (command.contains("--panic-on") || command.contains("zone=trusted")))
}

/// One person's permissions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Permissions {
    pub level: Level,
    /// Area ids, used at `Level::Pick`.
    pub areas: Vec<String>,
}

impl Permissions {
    pub fn allows(&self, area: &str) -> bool {
        match self.level {
            Level::Ask => false,
            Level::Pick => self.areas.iter().any(|a| a == area),
            // Users and groups are on the risky list, so they still ask.
            Level::Full => area != "users",
        }
    }
}

#[derive(serde::Deserialize, Default)]
struct PolicyFile {
    #[serde(default)]
    user: BTreeMap<String, UserEntry>,
}

#[derive(serde::Deserialize)]
struct UserEntry {
    level: String,
    #[serde(default)]
    areas: Vec<String>,
}

/// Reads one user's permissions from the policy file's text.
pub fn parse_policy(text: &str, user: &str) -> Option<Permissions> {
    let file: PolicyFile = toml::from_str(text).ok()?;
    let entry = file.user.get(user)?;
    Some(Permissions {
        level: Level::from_id(&entry.level).unwrap_or_default(),
        areas: entry.areas.iter().filter(|a| area(a).is_some()).cloned().collect(),
    })
}

/// Reads the old groups file: "install services updates" became areas at "Allow what I pick".
pub fn parse_legacy(text: &str) -> Permissions {
    let areas: Vec<String> = text
        .split_whitespace()
        .filter_map(|g| match g {
            "install" => Some("apps"),
            "services" => Some("services"),
            "updates" => Some("updates"),
            _ => None,
        })
        .map(String::from)
        .collect();
    Permissions { level: if areas.is_empty() { Level::Ask } else { Level::Pick }, areas }
}

/// The person running this program.
pub fn current_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .ok()
        .filter(|u| !u.is_empty())
        .or_else(|| {
            let out = std::process::Command::new("id").arg("-un").output().ok()?;
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        })
        .unwrap_or_default()
}

/// The current person's permissions: from the policy file, else the old groups file, else
/// "Ask me every time".
pub fn load() -> Permissions {
    let user = current_user();
    if let Ok(text) = std::fs::read_to_string(POLICY_FILE) {
        return parse_policy(&text, &user).unwrap_or_default();
    }
    std::fs::read_to_string(LEGACY_FILE).map(|t| parse_legacy(&t)).unwrap_or_default()
}

/// Saves new permissions for the current person. pkexec asks for an administrator's password.
pub fn apply(p: &Permissions) -> Result<(), String> {
    let mut args = vec![HELPER.to_string(), "--level".into(), p.level.id().into()];
    if p.level == Level::Pick {
        args.extend(p.areas.iter().filter(|a| area(a).is_some()).cloned());
    }
    let out = std::process::Command::new("pkexec").args(&args).output().map_err(|e| format!("Couldn't ask: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(()),
        // pkexec: 126 is a cancelled or refused password, 127 a failed check.
        Some(126 | 127) => Err("Not changed: the password wasn't given.".into()),
        _ => Err(format!("Couldn't change it: {}", String::from_utf8_lossy(&out.stderr).trim())),
    }
}

/// A few lines for the agent's system prompt: what it may run as administrator, and how.
pub fn summary() -> String {
    summary_for(&load())
}

pub fn summary_for(p: &Permissions) -> String {
    let mut s = String::from("Administrator commands (sudo): ");
    match p.level {
        Level::Ask => s.push_str(
            "every sudo command shows the person a pop-up with the command and asks for their password. \
Use sudo only when needed, one command at a time, and say why before you run it.",
        ),
        Level::Pick => {
            let on: Vec<String> = AREAS
                .iter()
                .filter(|a| p.allows(a.id))
                .map(|a| format!("{} ({})", a.name.to_lowercase(), a.covers))
                .collect();
            if on.is_empty() {
                s.push_str("every sudo command asks the person for their password in a pop-up.");
            } else {
                s.push_str(&format!(
                    "these run without asking: {}. Any other sudo command shows the person a pop-up and asks for their password.",
                    on.join("; ")
                ));
            }
        }
        Level::Full => s.push_str(
            "sudo runs without asking, except risky commands (rm, dd, mkfs, wipefs, partitioning, user and \
password changes, turning off the firewall), which ask the person for their password. Be careful: \
prefer the smallest change that does the job.",
        ),
    }
    if p.allows("etc") {
        s.push_str(&format!(
            " To change a file in /etc, pipe the new contents to: sudo {ETC_WRITER} /etc/<path> (it keeps a backup)."
        ));
    }
    s.push_str(
        " If the person says no or sudo is refused, don't try another way around it; tell them what you \
needed and that Settings → AI → Permissions can allow it.",
    );
    s
}

/// What happened to one administrator command the AI ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Ran without asking, because the permissions allow it.
    Allowed,
    /// The person typed their password in the pop-up, and it ran.
    Asked,
    /// The person cancelled the pop-up.
    Denied,
    /// It asked, or was allowed, but didn't succeed (wrong password, or the command failed).
    Failed,
    /// It ran in a terminal, where sudo asks there.
    Terminal,
}

impl Outcome {
    pub fn words(self) -> &'static str {
        match self {
            Outcome::Allowed => "Allowed by your settings",
            Outcome::Asked => "You allowed it",
            Outcome::Denied => "You said no",
            Outcome::Failed => "Didn't succeed",
            Outcome::Terminal => "In the terminal",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    /// Seconds since 1970.
    pub time: u64,
    pub command: String,
    pub status: i32,
    pub outcome: Outcome,
}

/// Where the agent's sudo wrapper appends a line per administrator command.
pub fn activity_log() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("tripppin/admin.log"))
}

/// One log line: seconds, exit status, what the pop-up recorded, the command; tab-separated.
pub fn parse_activity_line(line: &str) -> Option<Activity> {
    let mut parts = line.splitn(4, '\t');
    let time = parts.next()?.trim().parse().ok()?;
    let status: i32 = parts.next()?.trim().parse().ok()?;
    let mark = parts.next()?.trim();
    let command = parts.next()?.trim().to_string();
    let outcome = match (mark, status) {
        ("cancelled", _) => Outcome::Denied,
        ("terminal", _) => Outcome::Terminal,
        ("asked", 0) => Outcome::Asked,
        ("asked", _) => Outcome::Failed,
        (_, 0) => Outcome::Allowed,
        _ => Outcome::Failed,
    };
    Some(Activity { time, command, status, outcome })
}

/// The newest administrator commands, newest first.
pub fn activity(limit: usize) -> Vec<Activity> {
    let Some(text) = activity_log().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return Vec::new();
    };
    text.lines().rev().filter_map(parse_activity_line).take(limit).collect()
}

/// The bash function the agent's commands get, so every sudo is logged and never reuses a
/// password typed earlier (`-k`). With `askpass`, sudo asks in the pop-up (`-A`), which
/// records "asked" or "cancelled" in the file named by `TRIPPPIN_SUDO_MARK`.
pub fn sudo_wrapper(askpass: bool) -> String {
    let (flags, mark) = if askpass { ("-k -A", "") } else { ("", "terminal") };
    format!(
        r#"sudo() {{
    local __m __s __log="${{TRIPPPIN_ADMIN_LOG:-}}"
    __m=$(mktemp "${{XDG_RUNTIME_DIR:-/tmp}}/tripppin-sudo.XXXXXX" 2>/dev/null) || __m=/dev/null
    TRIPPPIN_SUDO_MARK="$__m" command sudo {flags} "$@"; __s=$?
    if [ -n "$__log" ]; then
        local __c="$*"; __c=${{__c//$'\t'/ }}; __c=${{__c//$'\n'/ }}
        local __k; __k=$(cat "$__m" 2>/dev/null); [ -n "{mark}" ] && __k="{mark}"
        printf '%s\t%s\t%s\t%s\n' "$(date +%s)" "$__s" "${{__k:-none}}" "${{__c:0:500}}" >> "$__log" 2>/dev/null
    fi
    [ "$__m" != /dev/null ] && rm -f "$__m"
    return $__s
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_policy_for_one_person() {
        let text = r#"
# Written by Settings → AI → Permissions.
[user."akshay"]
level = "pick"
areas = ["apps", "printers", "nonsense"]

[user."guest"]
level = "full"
"#;
        let p = parse_policy(text, "akshay").unwrap();
        assert_eq!(p.level, Level::Pick);
        assert_eq!(p.areas, ["apps", "printers"]);
        assert!(p.allows("apps") && !p.allows("users"));
        let g = parse_policy(text, "guest").unwrap();
        assert!(g.allows("disks") && !g.allows("users"));
        assert!(parse_policy(text, "nobody").is_none());
        assert!(parse_policy("not toml [", "akshay").is_none());
    }

    #[test]
    fn reads_the_old_groups() {
        let p = parse_legacy("install updates\n");
        assert_eq!(p.level, Level::Pick);
        assert_eq!(p.areas, ["apps", "updates"]);
        assert_eq!(parse_legacy(""), Permissions::default());
    }

    #[test]
    fn spots_risky_commands() {
        assert!(is_risky("/usr/bin/rm -rf /etc/foo"));
        assert!(is_risky("mkfs.ext4 /dev/sdb1"));
        assert!(is_risky("sudo systemctl disable --now firewalld"));
        assert!(is_risky("udisksctl format -b /dev/sdb"));
        assert!(!is_risky("dnf install -y htop"));
        assert!(!is_risky("systemctl restart sshd"));
        assert!(!is_risky("firmware-update"));
        assert!(!is_risky("cat /etc/passwd"));
    }

    #[test]
    fn describes_itself_to_the_agent() {
        assert!(summary_for(&Permissions::default()).contains("asks for their password"));
        let pick = Permissions { level: Level::Pick, areas: vec!["apps".into(), "etc".into()] };
        let s = summary_for(&pick);
        assert!(s.contains("dnf") && s.contains(ETC_WRITER) && !s.contains("lpadmin"));
        assert!(summary_for(&Permissions { level: Level::Full, areas: vec![] }).contains("risky"));
    }

    #[test]
    fn reads_the_activity_log() {
        let a = parse_activity_line("1700000000\t0\tnone\tdnf install -y htop").unwrap();
        assert_eq!(a.outcome, Outcome::Allowed);
        assert_eq!(a.command, "dnf install -y htop");
        assert_eq!(parse_activity_line("1\t1\tcancelled\trm x").unwrap().outcome, Outcome::Denied);
        assert_eq!(parse_activity_line("1\t0\tasked\tbootc upgrade").unwrap().outcome, Outcome::Asked);
        assert_eq!(parse_activity_line("1\t1\tasked\tbootc upgrade").unwrap().outcome, Outcome::Failed);
        assert!(parse_activity_line("garbage").is_none());
    }

    #[test]
    fn wrapper_logs_and_ignores_cached_passwords() {
        let w = sudo_wrapper(true);
        assert!(w.contains("command sudo -k -A \"$@\""));
        assert!(sudo_wrapper(false).contains("terminal"));
    }

    #[cfg(unix)]
    #[test]
    fn wrapper_writes_the_activity_log() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("trip-sudo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A stand-in sudo that "asks" (writes the mark like the pop-up) and fails on "nope".
        let fake = dir.join("sudo");
        std::fs::write(
            &fake,
            "#!/bin/sh\nwhile [ \"${1#-}\" != \"$1\" ]; do shift; done\necho asked > \"$TRIPPPIN_SUDO_MARK\"\n[ \"$1\" != nope ]\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let log = dir.join("admin.log");
        let script = format!("{}sudo dnf install 'a\tb'; sudo nope; true", sudo_wrapper(true));
        let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default());
        let ok = std::process::Command::new("bash")
            .arg("-c")
            .arg(script)
            .env("PATH", path)
            .env("TRIPPPIN_ADMIN_LOG", &log)
            .env("XDG_RUNTIME_DIR", &dir)
            .status()
            .unwrap();
        assert!(ok.success());
        let text = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<Activity> = text.lines().filter_map(parse_activity_line).collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert_eq!(lines[0].outcome, Outcome::Asked);
        assert_eq!(lines[0].command, "dnf install a b");
        assert_eq!(lines[1].outcome, Outcome::Failed);
        std::fs::remove_dir_all(&dir).ok();
    }
}
