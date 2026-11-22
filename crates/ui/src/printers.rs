//! Printers through CUPS's command-line tools. Network and USB printers that speak IPP
//! Everywhere appear by themselves (CUPS's own discovery makes temporary queues for them,
//! which only `lpstat -e` lists; ipp-usb does the same for USB); the default is per person
//! (`lpoptions -d`, kept in ~/.cups/lpoptions), so it needs no administrator.

use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct Printer {
    pub name: String,
    /// "idle", "printing", "disabled"…
    pub state: String,
    pub default: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: String,
    pub printer: String,
    pub size: String,
}

/// Parses `LC_ALL=C lpstat -p -d`.
pub fn parse_printers(out: &str) -> Vec<Printer> {
    let default = out.lines().find_map(|l| l.strip_prefix("system default destination: ")).map(str::trim);
    out.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("printer ")?;
            let (name, rest) = rest.split_once(' ')?;
            let state = if rest.contains("disabled") {
                "turned off"
            } else if rest.contains("now printing") {
                "printing"
            } else {
                "ready"
            };
            Some(Printer { name: name.to_string(), state: state.to_string(), default: Some(name) == default })
        })
        .collect()
}

/// Parses `LC_ALL=C lpstat -o`: "Office-123  anu  4096  Mon 23 Nov…".
pub fn parse_jobs(out: &str) -> Vec<Job> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let id = *f.first()?;
            let printer = id.rsplit_once('-')?.0;
            let bytes: u64 = f.get(2)?.parse().ok()?;
            Some(Job {
                id: id.to_string(),
                printer: printer.to_string(),
                size: format!("{:.0} kb", bytes as f64 / 1e3),
            })
        })
        .collect()
}

fn lpstat(args: &[&str]) -> String {
    Command::new("lpstat")
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Adds the printers only `lpstat -e` knows (discovered, not yet used) as ready.
pub fn with_discovered(mut list: Vec<Printer>, every: &str) -> Vec<Printer> {
    for name in every.lines().map(str::trim).filter(|n| !n.is_empty()) {
        if !list.iter().any(|p| p.name == name) {
            list.push(Printer { name: name.to_string(), state: "ready".into(), default: false });
        }
    }
    list
}

/// None when CUPS isn't installed.
pub fn printers() -> Option<Vec<Printer>> {
    if !std::path::Path::new("/usr/bin/lpstat").exists() {
        return None;
    }
    Some(with_discovered(parse_printers(&lpstat(&["-p", "-d"])), &lpstat(&["-e"])))
}

pub fn jobs() -> Vec<Job> {
    parse_jobs(&lpstat(&["-o"]))
}

fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_lowercase()) }
}

pub fn set_default(name: &str) -> Result<(), String> {
    run("lpoptions", &["-d", name])
}

pub fn test_page(name: &str) -> Result<(), String> {
    run("lp", &["-d", name, "/usr/share/cups/data/testprint"])
}

pub fn cancel(job: &str) -> Result<(), String> {
    run("cancel", &[job])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lpstat() {
        let p = "printer Office_HP is idle.  enabled since Mon 23 Nov 2026\nprinter Brother is now printing Brother-7.  enabled since…\nprinter Old disabled since Sun…\nsystem default destination: Brother\n";
        let list = parse_printers(p);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0], Printer { name: "Office_HP".into(), state: "ready".into(), default: false });
        assert_eq!(list[1].state, "printing");
        assert!(list[1].default);
        assert_eq!(list[2].state, "turned off");
        let all = with_discovered(list.clone(), "Office_HP\nHP_LaserJet_M110\n");
        assert_eq!(all.len(), 4);
        assert_eq!(all[3].name, "HP_LaserJet_M110");
        let jobs = parse_jobs("Brother-7               anu            4096   Mon 23 Nov 2026 04:00:00\n");
        assert_eq!(jobs, vec![Job { id: "Brother-7".into(), printer: "Brother".into(), size: "4 kb".into() }]);
    }
}
