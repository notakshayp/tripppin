//! `tripppin-ui battery-ring`: the bar's battery, a small ring that fills with charge. Writes
//! the ring as an SVG under $XDG_RUNTIME_DIR and prints its path, then the tooltip, which is
//! what waybar's `image` module reads. Prints nothing on a machine with no battery, which
//! hides it.

use std::path::{Path, PathBuf};

/// What the ring shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Charge {
    pub percent: u8,
    /// Charging, or full and plugged in: a bolt inside the ring.
    pub plugged: bool,
    pub charging: bool,
}

/// The machine's own battery (not a mouse's or a headset's, which say `scope=Device`).
pub fn read(root: &Path) -> Option<Charge> {
    let mut supplies: Vec<PathBuf> = std::fs::read_dir(root).ok()?.flatten().map(|e| e.path()).collect();
    supplies.sort();
    let read = |p: &Path, f: &str| std::fs::read_to_string(p.join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
    let on_mains = supplies.iter().any(|p| read(p, "type") == "Mains" && read(p, "online") == "1");
    supplies.iter().find_map(|p| {
        if read(p, "type") != "Battery" || read(p, "scope") == "Device" || read(p, "present") == "0" {
            return None;
        }
        let percent: u8 = read(p, "capacity").parse().ok()?;
        let status = read(p, "status");
        let charging = status == "Charging";
        Some(Charge { percent: percent.min(100), plugged: charging || on_mains, charging })
    })
}

/// Green normally, amber below 20%, red below 10% (unless it's charging back up).
pub fn colour(c: &Charge) -> &'static str {
    match c.percent {
        _ if c.plugged => "#57d99a",
        0..10 => "#ff7a6b",
        10..20 => "#f0b45a",
        _ => "#57d99a",
    }
}

/// The ring: a faint track, the charge drawn clockwise from the top, and a bolt inside when
/// plugged in. 16 units square with round numbers where it matters, so it stays crisp when
/// waybar draws it at 16 px (and 32 px at 2x).
pub fn svg(c: &Charge) -> String {
    const R: f64 = 6.0;
    let circumference = 2.0 * std::f64::consts::PI * R;
    let filled = circumference * f64::from(c.percent) / 100.0;
    let arc = if c.percent >= 100 {
        format!(r##"<circle cx="8" cy="8" r="{R}" fill="none" stroke="{}" stroke-width="2"/>"##, colour(c))
    } else if c.percent == 0 {
        String::new()
    } else {
        format!(
            r##"<circle cx="8" cy="8" r="{R}" fill="none" stroke="{}" stroke-width="2" stroke-linecap="round" stroke-dasharray="{filled:.3} {circumference:.3}" transform="rotate(-90 8 8)"/>"##,
            colour(c)
        )
    };
    let bolt = if c.plugged { r##"<path d="M8.9 3.9 5.4 8.7h2.5l-.8 3.4 3.5-4.8H8.1z" fill="#f2f5ef"/>"## } else { "" };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><circle cx="8" cy="8" r="{R}" fill="none" stroke="#f2f5ef" stroke-opacity="0.2" stroke-width="2"/>{arc}{bolt}</svg>"##
    )
}

pub fn tooltip(c: &Charge) -> String {
    let state = if c.charging {
        " · charging"
    } else if c.plugged {
        " · plugged in"
    } else {
        ""
    };
    format!("Battery {}%{state}", c.percent)
}

pub fn run() -> gtk::glib::ExitCode {
    // `battery-ring 15 charging`: draw that instead, to look at.
    let args: Vec<String> = std::env::args().skip(2).collect();
    let charge = match args.first().and_then(|a| a.parse::<u8>().ok()) {
        Some(percent) => {
            let charging = args.iter().any(|a| a == "charging");
            Some(Charge { percent: percent.min(100), plugged: charging, charging })
        }
        None => read(Path::new("/sys/class/power_supply")),
    };
    let Some(charge) = charge else {
        println!();
        return gtk::glib::ExitCode::SUCCESS;
    };
    let dir = crate::common::runtime_dir();
    // A new name for each look, so waybar never reads a half-written file.
    let name = format!("battery-{}{}.svg", charge.percent, if charge.plugged { "-plugged" } else { "" });
    let path = dir.join(&name);
    if !path.exists() {
        let tmp = dir.join(format!(".{name}.{}", std::process::id()));
        if std::fs::write(&tmp, svg(&charge)).is_err() || std::fs::rename(&tmp, &path).is_err() {
            println!();
            return gtk::glib::ExitCode::FAILURE;
        }
    }
    println!("{}\n{}", path.display(), tooltip(&charge));
    gtk::glib::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(root: &Path, name: &str, files: &[(&str, &str)]) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        for (f, v) in files {
            std::fs::write(dir.join(f), format!("{v}\n")).unwrap();
        }
    }

    #[test]
    fn reads_the_laptop_battery_not_the_mouse() {
        let root = std::env::temp_dir().join(format!("tripppin-battery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        supply(&root, "AC", &[("type", "Mains"), ("online", "0")]);
        supply(&root, "hid-mouse-battery", &[("type", "Battery"), ("scope", "Device"), ("capacity", "40")]);
        assert_eq!(read(&root), None);
        supply(&root, "BAT0", &[("type", "Battery"), ("capacity", "84"), ("status", "Discharging")]);
        assert_eq!(read(&root), Some(Charge { percent: 84, plugged: false, charging: false }));
        supply(&root, "AC", &[("type", "Mains"), ("online", "1")]);
        supply(&root, "BAT0", &[("type", "Battery"), ("capacity", "100"), ("status", "Full")]);
        assert_eq!(read(&root), Some(Charge { percent: 100, plugged: true, charging: false }));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ring_colours_and_shapes() {
        let c = |percent, plugged| Charge { percent, plugged, charging: plugged };
        assert_eq!(colour(&c(50, false)), "#57d99a");
        assert_eq!(colour(&c(19, false)), "#f0b45a");
        assert_eq!(colour(&c(9, false)), "#ff7a6b");
        assert_eq!(colour(&c(9, true)), "#57d99a");
        // Half full: the arc covers half the circle.
        assert!(svg(&c(50, false)).contains("stroke-dasharray=\"18.850 37.699\""));
        assert!(!svg(&c(50, false)).contains("<path"));
        assert!(svg(&c(50, true)).contains("<path"));
        assert!(!svg(&c(0, false)).contains("dasharray"));
        assert!(!svg(&c(100, false)).contains("dasharray"));
        assert_eq!(tooltip(&c(84, false)), "Battery 84%");
        assert_eq!(tooltip(&c(84, true)), "Battery 84% · charging");
    }
}
