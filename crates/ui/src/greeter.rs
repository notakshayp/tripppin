//! The login screen, run by greetd inside a minimal niri session (/etc/tripppin/greeter-niri.kdl).
//! The clock top left; who you are and the password bottom left; network, battery and power
//! bottom right; the faint mark behind it all.

use crate::common::{self, label};
use crate::greetd::{Greetd, Outcome};
use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder().application_id("in.trippp.Greeter").build();
    app.connect_activate(build);
    app.run_with_args(&[] as &[&str])
}

fn build(app: &gtk::Application) {
    common::load_css();
    let window = gtk::Window::builder().application(app).decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("screen");
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-greeter"));
    window.set_layer(Layer::Overlay);
    for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(e, true);
    }
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::Exclusive);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&common::backdrop()));

    // Clock.
    let time = gtk::Box::new(gtk::Orientation::Vertical, 6);
    time.set_halign(gtk::Align::Start);
    time.set_valign(gtk::Align::Start);
    time.set_margin_start(96);
    time.set_margin_top(96);
    let clock = label(&common::clock(), &["clock-big"]);
    let date = label(&common::long_date(), &["date"]);
    time.append(&clock);
    time.append(&date);
    overlay.add_overlay(&time);

    // Who and the password.
    let people = common::people();
    let user = people.first().cloned().unwrap_or_default();
    let form = gtk::Box::new(gtk::Orientation::Vertical, 18);
    form.set_halign(gtk::Align::Start);
    form.set_valign(gtk::Align::End);
    form.set_margin_start(96);
    form.set_margin_bottom(110);
    form.set_size_request(380, -1);
    let who = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    let name = common::display_name(&user);
    let initial = name.chars().next().map(|c| c.to_lowercase().to_string()).unwrap_or_default();
    let avatar = gtk::Label::new(Some(&initial));
    avatar.add_css_class("avatar");
    let names = gtk::Box::new(gtk::Orientation::Vertical, 0);
    names.set_valign(gtk::Align::Center);
    names.append(&label(&name.to_lowercase(), &["who"]));
    names.append(&label("tripppin desktop", &["status"]));
    who.append(&avatar);
    who.append(&names);
    // More than one person: a name field instead of a fixed name.
    let username = gtk::Entry::new();
    username.add_css_class("pw");
    username.set_placeholder_text(Some("user name"));
    let name_pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    name_pill.add_css_class("pw-pill");
    username.set_hexpand(true);
    name_pill.append(&username);
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pill.add_css_class("pw-pill");
    let password = gtk::PasswordEntry::new();
    password.add_css_class("pw");
    password.set_hexpand(true);
    password.set_placeholder_text(Some("password"));
    let go = gtk::Button::new();
    go.add_css_class("go");
    go.set_valign(gtk::Align::Center);
    go.set_child(Some(&common::arrow(false)));
    go.set_tooltip_text(Some("log in"));
    pill.append(&password);
    pill.append(&go);
    let error = label("", &["error"]);
    error.set_visible(false);
    if people.len() == 1 {
        form.append(&who);
    } else {
        form.append(&name_pill);
    }
    form.append(&pill);
    form.append(&error);
    overlay.add_overlay(&form);

    // Network, battery, power.
    let status = gtk::Box::new(gtk::Orientation::Horizontal, 22);
    status.set_halign(gtk::Align::End);
    status.set_valign(gtk::Align::End);
    status.set_margin_end(48);
    status.set_margin_bottom(40);
    if let Some(ssid) = common::wifi_name() {
        status.append(&label(&format!("wi-fi · {}", ssid.to_lowercase()), &["status"]));
    }
    if let Some(b) = common::battery() {
        status.append(&label(&format!("battery {b}%"), &["status"]));
    }
    let power = gtk::MenuButton::new();
    power.set_label("power");
    power.add_css_class("status");
    let menu = gtk::gio::Menu::new();
    menu.append(Some("shut down"), Some("app.poweroff"));
    menu.append(Some("restart"), Some("app.reboot"));
    power.set_menu_model(Some(&menu));
    for (name, verb) in [("poweroff", "poweroff"), ("reboot", "reboot")] {
        let action = gtk::gio::SimpleAction::new(name, None);
        action.connect_activate(move |_, _| {
            let _ = std::process::Command::new("systemctl").arg(verb).spawn();
        });
        app.add_action(&action);
    }
    status.append(&power);
    overlay.add_overlay(&status);
    window.set_child(Some(&overlay));

    glib::timeout_add_seconds_local(5, move || {
        clock.set_text(&common::clock());
        date.set_text(&common::long_date());
        glib::ControlFlow::Continue
    });

    let attempt = {
        let (password, username, error, app) = (password.clone(), username.clone(), error.clone(), app.clone());
        let single = (people.len() == 1).then_some(user);
        move || {
            let who = single.clone().unwrap_or_else(|| username.text().trim().to_string());
            let pw = password.text().to_string();
            if who.is_empty() || pw.is_empty() {
                return;
            }
            let result = Greetd::connect().and_then(|mut g| g.login(&who, &pw, &["niri-session"]));
            match result {
                Ok(Outcome::Started) => app.quit(),
                Ok(Outcome::Refused(why)) => {
                    error.set_text(&why);
                    error.set_visible(true);
                    password.set_text("");
                }
                Err(e) => {
                    error.set_text(&format!("{e:#}").to_lowercase());
                    error.set_visible(true);
                }
            }
        }
    };
    {
        let attempt = attempt.clone();
        password.connect_activate(move |_| attempt());
    }
    go.connect_clicked(move |_| attempt());
    window.present();
    password.grab_focus();
}
