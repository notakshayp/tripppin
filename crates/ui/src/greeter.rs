//! The login screen, run by greetd inside a minimal KWin session. It matches the lock screen
//! (the "Bento" design, mock section 01): a board of rounded tiles on #16181a, the big time
//! tile (Manrope 800, 120 px) with the date over the amber sign-in tile (avatar circle, name,
//! the password field, an arrow), and on the right the battery and connection when there is
//! something to say, and the power menu. No curtain: typing goes straight into the field.
//! (The next event, what is playing and the unread count belong to the person, so only the
//! lock screen, which runs as them, shows those.)

use crate::common::{self, label};
use crate::greetd::{Greetd, Outcome};
use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

/// The desktop session greetd starts: it keeps the text console blank and ink, then runs
/// niri-session (os/files/usr/libexec/tripppin-session).
const SESSION: &str = "/usr/libexec/tripppin-session";

/// The Bento look for the login screen, on top of the shared stylesheet so it needs no change
/// there: #16181a, 20 px tiles 12 px apart, the time in Manrope 800 at 120 px, the amber
/// sign-in tile with a darker pill for the password.
const BENTO_CSS: &str = r#"
window.bento.screen { background: #16181a; color: #f1f3f4; font-family: Manrope; }
window.bento .tile { background: #23272a; border-radius: 20px; padding: 16px; }
window.bento .tile-time { background: #2b3a33; }
window.bento .tile-label { font-family: Manrope; font-weight: 800; font-size: 13px; color: #9aa3a8; }
window.bento .tile-big { font-family: Manrope; font-weight: 800; font-size: 40px; letter-spacing: -1.2px; color: #f1f3f4; }
window.bento .clock-big {
  font-family: Manrope; font-weight: 800; font-size: 120px; letter-spacing: -3.6px;
  color: #f1f3f4; margin: 0; padding: 0; line-height: 1;
}
window.bento .date { font-family: Manrope; font-weight: 600; font-size: 18px; color: #9aa3a8; margin-top: 6px; }
window.bento .tile-signin { background: #f2b84b; color: #16181a; }
window.bento .bento-avatar {
  min-width: 52px; min-height: 52px; border-radius: 999px; background: #16181a; color: #f2b84b;
  font-family: Manrope; font-weight: 800; font-size: 20px;
}
window.bento .bento-name { font-family: Manrope; font-weight: 800; font-size: 16px; color: #16181a; }
window.bento .pw-pill {
  background: rgba(22,24,26,0.12); border: none; border-radius: 999px; box-shadow: none;
  padding: 0; min-height: 32px;
}
window.bento .pw-pill:focus-within { background: rgba(22,24,26,0.2); }
window.bento .pw-pill entry, window.bento .pw-pill text {
  font-family: Manrope; font-weight: 600; font-size: 14px; color: #16181a; background: none;
  border: none; box-shadow: none; outline: none; border-radius: 0; padding: 0 14px; min-height: 32px;
}
window.bento .pw-pill text placeholder, window.bento .pw-pill entry placeholder { color: rgba(22,24,26,0.7); }
window.bento button.go {
  background: #16181a; border-radius: 999px; border: none; box-shadow: none;
  min-width: 40px; min-height: 40px; margin: 0; padding: 0; transition: transform 150ms ease-out;
}
window.bento button.go:hover { transform: scale(1.06); }
window.bento button.go:active { transform: scale(0.94); }
window.bento .status { color: #9aa3a8; font-family: Manrope; font-size: 13px; }
window.bento .tile-signin .status { color: rgba(22,24,26,0.7); }
window.bento button.power-tile {
  background: #23272a; color: #f1f3f4; border: none; border-radius: 20px; box-shadow: none;
  font-family: Manrope; font-weight: 800; font-size: 14px; padding: 14px 20px;
}
window.bento button.power-tile:hover { background: #2e3337; }
window.bento button.person {
  background: rgba(22,24,26,0.12); color: #16181a; border: none; border-radius: 999px; box-shadow: none;
  font-family: Manrope; font-weight: 700; padding: 2px 10px;
}
window.bento button.person.selected { background: #16181a; color: #f2b84b; }
window.bento button.link { color: #16181a; font-family: Manrope; font-weight: 600; }
window.bento .caps-warning, window.bento .error { color: #7a1f12; font-family: Manrope; font-weight: 700; font-size: 13px; }
window.bento button, window.bento popover contents { border-radius: 12px; }
"#;

fn load_bento_css() {
    let Some(display) = gtk::gdk::Display::default() else { return };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(BENTO_CSS);
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

fn tile(classes: &[&str]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
    b.add_css_class("tile");
    for c in classes {
        b.add_css_class(c);
    }
    b
}

/// The sign-in tile's arrow: an amber 2.2 px line arrow on the dark circle, drawn as a vector.
fn amber_arrow() -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(22);
    area.set_content_height(22);
    area.set_can_target(false);
    area.set_draw_func(|_, cr, w, h| {
        let k = f64::from(w.min(h)) / 24.0;
        cr.set_source_rgb(0.949, 0.722, 0.294);
        cr.set_line_width(2.2 * k);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        cr.move_to(5.0 * k, 12.0 * k);
        cr.line_to(19.0 * k, 12.0 * k);
        cr.move_to(13.0 * k, 6.0 * k);
        cr.line_to(19.0 * k, 12.0 * k);
        cr.line_to(13.0 * k, 18.0 * k);
        let _ = cr.stroke();
    });
    area
}

fn build(app: &gtk::Application) {
    common::load_css();
    let window = gtk::Window::builder().application(app).decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("screen");
    window.add_css_class("bento");
    load_bento_css();
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-greeter"));
    window.set_layer(Layer::Overlay);
    for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(e, true);
    }
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::Exclusive);

    // The board: two equal columns, 12 px apart, 28 px from the screen's edge.
    let board = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    board.set_homogeneous(true);
    board.set_margin_top(28);
    board.set_margin_bottom(28);
    board.set_margin_start(28);
    board.set_margin_end(28);
    let left = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let right = gtk::Box::new(gtk::Orientation::Vertical, 12);
    board.append(&left);
    board.append(&right);

    // The time tile: the big time and the date, bottom-left of the tile.
    let time_tile = tile(&["tile-time"]);
    time_tile.set_vexpand(true);
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    time_tile.append(&spacer);
    let clock = label(&common::clock(), &["clock-big"]);
    clock.set_halign(gtk::Align::Start);
    let date = label(&common::long_date(), &["date"]);
    date.set_halign(gtk::Align::Start);
    time_tile.append(&clock);
    time_tile.append(&date);
    left.append(&time_tile);

    // The sign-in tile: avatar circle, name, the password field, the arrow.
    let people = common::people();
    let user = people.first().cloned().unwrap_or_default();
    let signin = tile(&["tile-signin"]);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    let name = common::display_name(&user);
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let avatar = gtk::Label::new(Some(&initial));
    avatar.add_css_class("bento-avatar");
    avatar.set_valign(gtk::Align::Center);
    row.append(&avatar);
    let middle = gtk::Box::new(gtk::Orientation::Vertical, 4);
    middle.set_hexpand(true);
    middle.set_valign(gtk::Align::Center);
    let name_label = label(&name, &["bento-name"]);
    name_label.set_xalign(0.0);
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
    password.set_placeholder_text(Some("Type your password"));
    pill.append(&password);
    let go = gtk::Button::new();
    go.add_css_class("go");
    go.set_valign(gtk::Align::Center);
    go.set_child(Some(&amber_arrow()));
    go.set_tooltip_text(Some("Log in"));
    let error = label("", &["error"]);
    error.set_visible(false);
    error.set_xalign(0.0);
    // Who's logging in: the one person, or a chip each (and "other" for a name not listed).
    let chosen = std::rc::Rc::new(std::cell::RefCell::new(user.clone()));
    if people.len() == 1 {
        middle.append(&name_label);
    } else {
        let chips_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let chips: std::rc::Rc<std::cell::RefCell<Vec<gtk::Button>>> = std::rc::Rc::default();
        let add_chip = |login: Option<&str>| {
            let shown = login.map(common::display_name).unwrap_or_else(|| "Other".into());
            let chip = gtk::Button::with_label(&shown);
            chip.add_css_class("person");
            if login == Some(user.as_str()) {
                chip.add_css_class("selected");
            }
            let (chosen, chips2, name_pill, password, avatar) =
                (chosen.clone(), chips.clone(), name_pill.clone(), password.clone(), avatar.clone());
            let login = login.map(str::to_string);
            chip.connect_clicked(move |b| {
                for c in chips2.borrow().iter() {
                    c.remove_css_class("selected");
                }
                b.add_css_class("selected");
                *chosen.borrow_mut() = login.clone().unwrap_or_default();
                let initial = login
                    .as_deref()
                    .map(common::display_name)
                    .and_then(|n| n.chars().next().map(|c| c.to_uppercase().to_string()))
                    .unwrap_or_else(|| "?".into());
                avatar.set_text(&initial);
                name_pill.set_visible(login.is_none());
                password.grab_focus();
            });
            chips.borrow_mut().push(chip.clone());
            chips_row.append(&chip);
        };
        for p in &people {
            add_chip(Some(p));
        }
        add_chip(None);
        name_pill.set_visible(false);
        middle.append(&chips_row);
        middle.append(&name_pill);
    }
    middle.append(&pill);
    // Caps lock: the keyboard's light says so, checked a few times a second.
    let caps = label("Caps lock is on", &["caps-warning"]);
    caps.set_xalign(0.0);
    caps.set_visible(crate::osd::caps_lock() == Some(true));
    {
        let caps = caps.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            caps.set_visible(crate::osd::caps_lock() == Some(true));
            glib::ControlFlow::Continue
        });
    }
    middle.append(&caps);
    middle.append(&error);
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
    middle.append(&forgot);
    middle.append(&help);
    row.append(&middle);
    row.append(&go);
    signin.append(&row);
    left.append(&signin);

    // Right column: battery and connection (only what is real), then the power menu.
    let wifi = common::wifi_name().is_some();
    let battery = common::battery();
    if wifi || battery.is_some() {
        let status = tile(&[]);
        status.set_valign(gtk::Align::Start);
        status.append(&label(if battery.is_some() { "Battery" } else { "Network" }, &["tile-label"]));
        let big = match battery {
            Some(b) => format!("{b}%"),
            None => "Wi-Fi".to_string(),
        };
        let big = label(&big, &["tile-big"]);
        big.set_halign(gtk::Align::Start);
        status.append(&big);
        if wifi && battery.is_some() {
            let net = label("Wi-Fi", &["status"]);
            net.set_halign(gtk::Align::Start);
            status.append(&net);
        }
        right.append(&status);
    }
    let fill = gtk::Box::new(gtk::Orientation::Vertical, 0);
    fill.set_vexpand(true);
    right.append(&fill);
    let power = gtk::MenuButton::new();
    power.set_label("Power");
    power.set_always_show_arrow(false);
    power.add_css_class("power-tile");
    power.set_halign(gtk::Align::End);
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
    right.append(&power);

    window.set_child(Some(&board));
    // The field has the focus from the start: typing goes straight into it.
    password.grab_focus();

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
}
