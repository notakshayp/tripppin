//! `tripppin-ui`: the screens that make the Tripppin desktop its own.
//!
//! - `orb`: the AI, resting in the corner. Super+Space (`orb --toggle`) asks it something.
//! - `greeter`: the login screen, run by greetd.
//! - `splash`: the logo on ink between logging in and the desktop being ready.
//! - `lock`: the lock screen.
//! - `first-boot`: fetches the model the first time someone logs in.
//! - `osd`: the volume, mic and brightness pop-ups.
//! - `dictate`: speak and it's typed (press again to finish); runs whisper.cpp on the device.
//! - `settings`: the Settings app.
//! - `calendar`: the Calendar app (`--popover`: the small one that drops from the clock; `--remind`: announce events that just started).
//! - `askpass`: the password pop-up for administrator commands the AI runs.
//! - `displays`: gives each screen a whole-number scale at login, so text stays sharp.
//! - `corners sharp|default|round`: the corner style everywhere (Settings → Appearance).
//! - `battery-ring`: the bar's battery ring, as an SVG for waybar.
//! - `toggle`: the on/off tiles in quick settings.
//! - `snap`: puts the focused window in a half, a quarter, or fills the screen.
//! - `bridge` (also `start`): the start screen: tiles, live content, all apps. Run again to close.
//! - `quick`: the quick panel from the taskbar status: toggles, volume, brightness, power.
//! - `store`: the Store, apps from Flathub in the Tiles design.
//! - `desktop`: right-click menu and the icons of ~/Desktop, on the background layer.
//! - `widgets`: the desktop widgets host, and `widgets add/list/test/remove/pin` to manage them.

mod accounts;
mod apps;
mod askpass;
mod battery;
mod bt;
mod cal;
mod calendar;
mod calendar_app;
mod common;
mod desktop;
mod dictate;
mod dim;
mod display;
mod firstboot;
mod glyph;
mod greetd;
mod greeter;
mod hub;
mod kde;
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
mod quick;
mod region;
mod settings;
mod snap;
mod snd;
mod splash;
mod start;
mod storage;
mod store;
mod switch;
mod system;
mod watch;
mod welcome;
mod widgets;

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
    // Not a screen either: run by waybar every 30 seconds.
    // Not a screen: `tripppin-ui dnd on|off`, for the quiet-hours timers.
    if args.first().map(String::as_str) == Some("dnd") {
        apps::set_dnd(args.get(1).map(String::as_str) == Some("on"));
        return ExitCode::SUCCESS;
    }
    if args.first().map(String::as_str) == Some("battery-ring") {
        return battery::run();
    }
    if args.first().map(String::as_str) == Some("toggle") {
        return quick::main(&args[1..]);
    }
    // Not a screen: announces calendar events that just started (the user timer runs it).
    if args.first().map(String::as_str) == Some("calendar") && args.get(1).map(String::as_str) == Some("--remind") {
        calendar_app::remind_now();
        return ExitCode::SUCCESS;
    }
    use_software_rendering_without_3d();
    match args.first().map(String::as_str) {
        Some("orb") => orb::run(),
        Some("greeter") => greeter::run(),
        Some("lock") => lock::run(),
        Some("first-boot") => firstboot::run(),
        Some("osd") => osd::run(),
        Some("dictate") => dictate::run(),
        Some("settings") => settings::run(),
        Some("calendar") if args.get(1).map(String::as_str) == Some("--popover") => calendar::run(),
        Some("calendar") => calendar_app::run(),
        Some("keys") => keys::run(),
        Some("dim") => dim::run(),
        Some("askpass") => askpass::run(),
        Some("welcome") => welcome::run(),
        Some("splash") => splash::run(),
        Some("switch") => switch::run(),
        Some("snap") => snap::run(args.get(1).map(String::as_str).unwrap_or("")),
        Some("desktop") => desktop::run(),
        Some("bridge") | Some("start") => start::run(),
        Some("quick") => quick::panel(),
        Some("store") => store::run(),
        Some("widgets") => widgets::cli(&args[1..]),
        _ => {
            eprintln!(
                "usage: tripppin-ui orb [--toggle] | greeter | lock [--release] | first-boot | osd volume|mic|brightness | dictate [cancel] | settings [section] | calendar [--popover|--remind] | keys | switch | snap <where> | bridge | quick | desktop | store | widgets [add|list|test|remove|pin|…] | dim | welcome [--first-time] | splash | askpass | displays | battery-ring | toggle <what> [--state] | corners sharp|default|round"
            );
            ExitCode::from(2)
        }
    }
}
