//! The login screen, run by greetd inside a minimal KWin session. It matches the lock screen
//! (the "Tiles" design, mock section 01): flat teal with a darkening towards the bottom, the
//! big light time bottom-left with the date, who you are and the password under them, Wi-Fi
//! and battery top-right, power bottom-right.

use crate::common::{self, label};
use crate::greetd::{Greetd, Outcome};
use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

/// The desktop session greetd starts: it keeps the text console blank and ink, then runs
/// niri-session (os/files/usr/libexec/tripppin-session).
const SESSION: &str = "/usr/libexec/tripppin-session";

/// The Tiles look for the login screen, on top of the shared stylesheet so it needs no change
/// there: teal #0f3d3a darkening towards the bottom, the time in Inter 200 at 140 px, square
/// everything, the password box in the lock screen's style (raised, 2 px outline, accent
/// when focused).
const TILES_CSS: &str = r#"
window.tiles-greeter.screen {
  background-color: #0f3d3a;
  background-image: linear-gradient(to bottom, rgba(0,0,0,0) 40%, rgba(0,0,0,0.45));
  color: #ffffff;
}
window.tiles-greeter .clock-big {
  font-family: Inter; font-weight: 200; font-size: 140px; letter-spacing: -2.8px;
  color: #ffffff; margin: 0; padding: 0; line-height: 1;
}
window.tiles-greeter .date {
  font-family: Inter; font-weight: 300; font-size: 30px; color: #ffffff; margin-top: 4px;
}
window.tiles-greeter .tiles-top { font-size: 13px; color: #ffffff; }
window.tiles-greeter .pw-pill {
  background: rgba(16,20,24,0.63); border: 2px solid #3a444b; border-radius: 0; box-shadow: none;
  padding: 0; min-height: 52px;
}
window.tiles-greeter .pw-pill:focus-within { border-color: #3fae7a; }
window.tiles-greeter .pw-pill entry, window.tiles-greeter .pw-pill text {
  font-family: Inter; font-weight: 300; font-size: 20px; color: #ffffff; background: none;
  border: none; box-shadow: none; outline: none; border-radius: 0; padding-left: 14px;
}
window.tiles-greeter button.go {
  background: #1f6b45; color: #ffffff; border-radius: 0; border: none; box-shadow: none;
  min-width: 52px; min-height: 52px; margin: -2px; padding: 0;
}
window.tiles-greeter button.go:hover { background: #2a7b54; }
window.tiles-greeter .avatar { border-radius: 0; background: #c2502e; color: #ffffff; }
window.tiles-greeter .who { font-family: Inter; font-weight: 300; font-size: 20px; color: #ffffff; }
window.tiles-greeter .status { color: #9aa4ab; font-family: Inter; font-size: 13px; }
window.tiles-greeter .tiles-top.status { color: #ffffff; }
window.tiles-greeter button, window.tiles-greeter popover contents { border-radius: 0; }
window.tiles-greeter button.person { border-radius: 0; background: rgba(16,20,24,0.5); border: 2px solid #3a444b; box-shadow: none; }
window.tiles-greeter button.person.selected { border-color: #3fae7a; }
window.tiles-greeter .error { color: #e8825c; }
"#;

fn load_tiles_css() {
    let Some(display) = gtk::gdk::Display::default() else { return };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(TILES_CSS);
    // Above the shared stylesheet and the corner style (which sit at APPLICATION and +1).
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 5);
}

/// Once logged in: everything on the login screen fades, leaving plain ink, and then the
/// greeter quits. greetd starts the desktop only after that, and the desktop opens on the
/// same ink with the splash (`tripppin-ui splash`), so ink is all there is in between.
fn fade_to_ink(app: &gtk::Application) {
    let Some(window) = app.active_window() else {
        app.quit();
        return;
    };
    let Some(content) = window.child() else {
        app.quit();
        return;
    };
    let start = std::time::Instant::now();
    let app = app.clone();
    content.add_tick_callback(move |c, _| {
        let t = (start.elapsed().as_secs_f64() / 0.25).min(1.0);
        c.set_opacity(1.0 - common::ease_out(t));
        if t >= 1.0 {
            app.quit();
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
}

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
    window.add_css_class("tiles-greeter");
    load_tiles_css();
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-greeter"));
    window.set_layer(Layer::Overlay);
    for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(e, true);
    }
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::Exclusive);

    let overlay = gtk::Overlay::new();
    let flat = gtk::Box::new(gtk::Orientation::Vertical, 0);
    flat.set_hexpand(true);
    flat.set_vexpand(true);
    overlay.set_child(Some(&flat));

    // Everything bottom-left in one column: the time, the date, then who and the password.
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_halign(gtk::Align::Start);
    column.set_valign(gtk::Align::End);
    column.set_margin_start(48);
    column.set_margin_bottom(60);

    // Clock.
    let time = gtk::Box::new(gtk::Orientation::Vertical, 0);
    time.set_halign(gtk::Align::Start);
    let clock = label(&common::clock(), &["clock-big"]);
    let date = label(&common::long_date(), &["date"]);
    time.append(&clock);
    time.append(&date);
    column.append(&time);

    // Who and the password.
    let people = common::people();
    let user = people.first().cloned().unwrap_or_default();
    let form = gtk::Box::new(gtk::Orientation::Vertical, 18);
    form.set_halign(gtk::Align::Start);
    form.set_margin_top(28);
    form.set_size_request(380, -1);
    let who = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    let name = common::display_name(&user);
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let avatar = gtk::Label::new(Some(&initial));
    avatar.add_css_class("avatar");
    let names = gtk::Box::new(gtk::Orientation::Vertical, 0);
    names.set_valign(gtk::Align::Center);
    names.append(&label(&name, &["who"]));
    names.append(&label("Tripppin desktop", &["status"]));
    who.append(&avatar);
    who.append(&names);
    // More than one person: a name field instead of a fixed name.
    let username = gtk::Entry::new();
    username.add_css_class("pw");
    username.set_placeholder_text(Some("User name"));
    let name_pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    name_pill.add_css_class("pw-pill");
    username.set_hexpand(true);
    name_pill.append(&username);
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pill.add_css_class("pw-pill");
    let password = gtk::PasswordEntry::new();
    password.add_css_class("pw");
    password.set_hexpand(true);
    password.set_placeholder_text(Some("Password"));
    let go = gtk::Button::new();
    go.add_css_class("go");
    go.set_valign(gtk::Align::Center);
    go.set_child(Some(&common::arrow(false)));
    go.set_tooltip_text(Some("Log in"));
    pill.append(&password);
    pill.append(&go);
    let error = label("", &["error"]);
    error.set_visible(false);
    // Who's logging in: the one person, or a chip each (and "other" for a name not listed).
    let chosen = std::rc::Rc::new(std::cell::RefCell::new(user.clone()));
    if people.len() == 1 {
        form.append(&who);
    } else {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let chips: std::rc::Rc<std::cell::RefCell<Vec<gtk::Button>>> = std::rc::Rc::default();
        let add_chip = |login: Option<&str>| {
            let shown = login.map(common::display_name).unwrap_or_else(|| "Other".into());
            let chip = gtk::Button::new();
            chip.add_css_class("person");
            let inner = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            let letter = shown.chars().next().map(|c| c.to_string()).unwrap_or_default();
            let a = label(&letter, &["avatar", "small"]);
            a.set_xalign(0.5);
            inner.append(&a);
            inner.append(&label(&shown, &["who-small"]));
            chip.set_child(Some(&inner));
            if login == Some(user.as_str()) {
                chip.add_css_class("selected");
            }
            let (chosen, chips2, name_pill, password) =
                (chosen.clone(), chips.clone(), name_pill.clone(), password.clone());
            let login = login.map(str::to_string);
            chip.connect_clicked(move |b| {
                for c in chips2.borrow().iter() {
                    c.remove_css_class("selected");
                }
                b.add_css_class("selected");
                *chosen.borrow_mut() = login.clone().unwrap_or_default();
                name_pill.set_visible(login.is_none());
                password.grab_focus();
            });
            chips.borrow_mut().push(chip.clone());
            row.append(&chip);
        };
        for p in &people {
            add_chip(Some(p));
        }
        add_chip(None);
        name_pill.set_visible(false);
        form.append(&row);
        form.append(&name_pill);
    }
    form.append(&pill);
    // Caps lock: the keyboard's light says so, checked a few times a second.
    let caps = label("Caps lock is on", &["caps-warning"]);
    caps.set_visible(crate::osd::caps_lock() == Some(true));
    {
        let caps = caps.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            caps.set_visible(crate::osd::caps_lock() == Some(true));
            glib::ControlFlow::Continue
        });
    }
    form.append(&caps);
    form.append(&error);
    let forgot = gtk::Button::with_label("Forgot your password?");
    forgot.add_css_class("link");
    forgot.set_halign(gtk::Align::Start);
    let help = label(
        "Another administrator on this computer can set a new one in Settings → accounts. If you're the only one, the password can't be recovered: your files stay safe, but Tripppin has to be installed again.",
        &["status"],
    );
    help.set_wrap(true);
    help.set_xalign(0.0);
    help.set_max_width_chars(46);
    help.set_visible(false);
    {
        let help = help.clone();
        forgot.connect_clicked(move |_| help.set_visible(!help.is_visible()));
    }
    form.append(&forgot);
    form.append(&help);
    column.append(&form);
    overlay.add_overlay(&column);

    // Wi-Fi and battery, top-right: "Wi-Fi   82%", as on the lock screen.
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    top.set_halign(gtk::Align::End);
    top.set_valign(gtk::Align::Start);
    top.set_margin_end(40);
    top.set_margin_top(30);
    if common::wifi_name().is_some() {
        top.append(&label("Wi-Fi", &["status", "tiles-top"]));
    }
    if let Some(b) = common::battery() {
        top.append(&label(&format!("{b}%"), &["status", "tiles-top"]));
    }
    overlay.add_overlay(&top);

    // Power, bottom-right, where the lock screen has its hint.
    let status = gtk::Box::new(gtk::Orientation::Horizontal, 22);
    status.set_halign(gtk::Align::End);
    status.set_valign(gtk::Align::End);
    status.set_margin_end(40);
    status.set_margin_bottom(60);
    let power = gtk::MenuButton::new();
    power.set_label("Power");
    power.set_always_show_arrow(false);
    power.add_css_class("status");
    let menu = gtk::gio::Menu::new();
    menu.append(Some("Shut down"), Some("app.poweroff"));
    menu.append(Some("Restart"), Some("app.reboot"));
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
        let chosen = chosen.clone();
        move || {
            let picked = chosen.borrow().clone();
            let who = if picked.is_empty() { username.text().trim().to_string() } else { picked };
            let pw = password.text().to_string();
            if who.is_empty() || pw.is_empty() {
                return;
            }
            let result = Greetd::connect().and_then(|mut g| g.login(&who, &pw, &[SESSION]));
            match result {
                Ok(Outcome::Started) => fade_to_ink(&app),
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
    if let Some(c) = window.child() {
        crate::common::play(&c, "fade");
    }
    password.grab_focus();
}
