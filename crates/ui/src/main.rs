//! `tripppin-ui`: the screens that make the Tripppin desktop its own.
//!
//! - `orb`: the AI, resting in the corner. Super+Space (`orb --toggle`) asks it something.
//! - `greeter`: the login screen, run by greetd.
//! - `lock`: the lock screen.
//! - `first-boot`: fetches the model the first time someone logs in.

mod common;
mod firstboot;
mod greetd;
mod greeter;
mod lock;
mod orb;

use gtk::glib::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("orb") => orb::run(),
        Some("greeter") => greeter::run(),
        Some("lock") => lock::run(),
        Some("first-boot") => firstboot::run(),
        _ => {
            eprintln!("usage: tripppin-ui orb [--toggle] | greeter | lock | first-boot");
            ExitCode::from(2)
        }
    }
}
