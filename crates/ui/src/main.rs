//! `tripppin-ui`: the screens that make the Tripppin desktop its own.
//!
//! - `orb`: the AI, resting in the corner. Super+Space (`orb --toggle`) asks it something.
//! - `greeter`: the login screen, run by greetd.
//! - `lock`: the lock screen.
//! - `first-boot`: fetches the model the first time someone logs in.
//! - `osd`: the volume, mic and brightness pop-ups.
//! - `settings`: the Settings app.

mod accounts;
mod apps;
mod bt;
mod common;
mod display;
mod firstboot;
mod greetd;
mod greeter;
mod keyboard;
mod lock;
mod nm;
mod orb;
mod osd;
mod pointer;
mod power;
mod region;
mod settings;
mod snd;
mod storage;
mod system;

use gtk::glib::ExitCode;

/// Virtual machines often have a GPU without 3D, where GTK's OpenGL renderer aborts on
/// start. There, run again with the software renderer (set before GTK starts, by re-exec,
/// since changing our own environment needs unsafe code).
#[cfg(unix)]
fn use_software_rendering_in_vms() {
    use std::os::unix::process::CommandExt;
    if std::env::var_os("GSK_RENDERER").is_some() {
        return;
    }
    let in_vm =
        std::process::Command::new("systemd-detect-virt").args(["--vm", "--quiet"]).status().is_ok_and(|s| s.success());
    if in_vm && let Ok(exe) = std::env::current_exe() {
        let err = std::process::Command::new(exe).args(std::env::args_os().skip(1)).env("GSK_RENDERER", "cairo").exec();
        eprintln!("couldn't restart with software rendering: {err}");
    }
}

fn main() -> ExitCode {
    use_software_rendering_in_vms();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("orb") => orb::run(),
        Some("greeter") => greeter::run(),
        Some("lock") => lock::run(),
        Some("first-boot") => firstboot::run(),
        Some("osd") => osd::run(),
        Some("settings") => settings::run(),
        _ => {
            eprintln!(
                "usage: tripppin-ui orb [--toggle] | greeter | lock | first-boot | osd volume|mic|brightness | settings [section]"
            );
            ExitCode::from(2)
        }
    }
}
