//! `tripppin-ui`: the screens that make the Tripppin desktop its own.
//!
//! - `orb`: the AI, resting in the corner. Super+Space (`orb --toggle`) asks it something.
//! - `greeter`: the login screen, run by greetd.
//! - `lock`: the lock screen.
//! - `first-boot`: fetches the model the first time someone logs in.
//! - `osd`: the volume, mic and brightness pop-ups.
//! - `settings`: the Settings app.
//! - `askpass`: the password pop-up for administrator commands the AI runs.
//! - `displays`: gives each screen a whole-number scale at login, so text stays sharp.
//! - `corners sharp|default|round`: the corner style everywhere (Settings → Appearance).

mod accounts;
mod apps;
mod askpass;
mod bt;
mod calendar;
mod common;
mod dim;
mod display;
mod firstboot;
mod greetd;
mod greeter;
mod keeper;
mod keyboard;
mod keys;
mod lock;
mod look;
mod nm;
mod orb;
mod osd;
mod pointer;
mod power;
mod printers;
mod privacy;
mod region;
mod settings;
mod snd;
mod storage;
mod switch;
mod system;
mod watch;
mod welcome;

use gtk::glib::ExitCode;

/// Without a GPU that can do 3D (some virtual machines), GTK's OpenGL renderer aborts on
/// start. There, run again with the software renderer (set before GTK starts, by re-exec,
/// since changing our own environment needs unsafe code). Everywhere else the GPU draws:
/// the software renderer is slow and makes every animation stutter.
#[cfg(unix)]
fn use_software_rendering_without_3d() {
    use std::os::unix::process::CommandExt;
    if std::env::var_os("GSK_RENDERER").is_some() {
        return;
    }
    let has_3d = std::fs::read_dir("/dev/dri")
        .is_ok_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("renderD")));
    if !has_3d && let Ok(exe) = std::env::current_exe() {
        let err = std::process::Command::new(exe).args(std::env::args_os().skip(1)).env("GSK_RENDERER", "cairo").exec();
        eprintln!("couldn't restart with software rendering: {err}");
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Not a screen: sets each display's scale at login, before anything draws.
    if args.first().map(String::as_str) == Some("displays") {
        display::use_crisp_scales();
        return ExitCode::SUCCESS;
    }
    // Not a screen either: sets the corner style, for scripts and the AI.
    if args.first().map(String::as_str) == Some("corners") {
        let choice = look::Corners::from_id(args.get(1).map_or("default", String::as_str));
        return match look::set(choice) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("couldn't set the corners: {e}");
                ExitCode::FAILURE
            }
        };
    }
    use_software_rendering_without_3d();
    match args.first().map(String::as_str) {
        Some("orb") => orb::run(),
        Some("greeter") => greeter::run(),
        Some("lock") => lock::run(),
        Some("first-boot") => firstboot::run(),
        Some("osd") => osd::run(),
        Some("settings") => settings::run(),
        Some("calendar") => calendar::run(),
        Some("keys") => keys::run(),
        Some("dim") => dim::run(),
        Some("askpass") => askpass::run(),
        Some("welcome") => welcome::run(),
        Some("switch") => switch::run(),
        Some("snap") => switch::snap(args.get(1).map(String::as_str).unwrap_or("")),
        _ => {
            eprintln!(
                "usage: tripppin-ui orb [--toggle] | greeter | lock | first-boot | osd volume|mic|brightness | settings [section] | calendar | keys | switch | snap <where> | dim | welcome [--first-time] | askpass | displays | corners sharp|default|round"
            );
            ExitCode::from(2)
        }
    }
}
