//! Welcome and tips: a few cards on what Tripppin is and where things are. Shown once at the
//! first login (`tripppin-ui welcome --first-time`, which does nothing after that) and any
//! time from the launcher ("welcome & tips").

use crate::common::{self, label};
use gtk::glib;
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

fn seen_file() -> std::path::PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("tripppin/welcomed")
}

/// (title, words, key combination to show, a button and what it runs)
type Card = (&'static str, &'static str, &'static str, Option<(&'static str, &'static [&'static str])>);

const CARDS: [Card; 4] = [
    (
        "welcome to tripppin",
        "a computer you can talk to. the ai runs here, on this machine: what you ask stays with you.",
        "",
        None,
    ),
    (
        "ask in plain words",
        "press super + space, or click the orb in the corner, and say what you want: \"install vlc\", \"what's using my disk\", \"make the text bigger\". it does the steps and shows you each one.",
        "super + space",
        None,
    ),
    (
        "find your way around",
        "apps are on super + a, or type an app's name after super + space. quick settings and notifications are on super + n. alt + tab switches windows, super + arrows snap them. super + / shows every shortcut.",
        "super + /",
        Some(("show the shortcuts", &["tripppin-ui", "keys"])),
    ),
    (
        "make it yours",
        "wallpaper, wi-fi, sound, displays, accounts and updates are all in settings. new apps come from flathub: ask the orb, or open settings → apps.",
        "",
        Some(("open settings", &["tripppin-ui", "settings"])),
    ),
];

fn online() -> bool {
    crate::nm::wired() || common::wifi_name().is_some()
}

pub fn run() -> glib::ExitCode {
    let first_time = std::env::args().any(|a| a == "--first-time");
    if first_time && seen_file().exists() {
        return glib::ExitCode::SUCCESS;
    }
    let app = gtk::Application::builder().application_id("in.trippp.Welcome").build();
    app.connect_activate(build);
    app.run_with_args(&[] as &[&str])
}

fn build(app: &gtk::Application) {
    common::load_css();
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("welcome")
        .default_width(620)
        .default_height(520)
        .build();
    window.add_css_class("tripppin");
    window.add_css_class("welcome");
    let header = gtk::HeaderBar::new();
    header.set_title_widget(Some(&gtk::Label::new(None)));
    header.add_css_class("bare");
    window.set_titlebar(Some(&header));

    // Offline: the first thing worth doing is getting online, so that card comes first.
    let mut cards: Vec<Card> = Vec::new();
    if !online() && crate::nm::has_wifi() {
        cards.push((
            "connect to the internet",
            "the ai works without it, but updates, new apps and the browser need it.",
            "",
            Some(("choose a wi-fi network", &["tripppin-ui", "settings", "wifi"])),
        ));
    }
    cards.extend(CARDS);
    let cards = Rc::new(cards);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 18);
    body.add_css_class("welcome-body");
    let logo = common::logo(54);
    logo.set_halign(gtk::Align::Start);
    let title = label("", &["welcome-title"]);
    title.set_wrap(true);
    let words = label("", &["welcome-words"]);
    words.set_wrap(true);
    words.set_xalign(0.0);
    let keys = label("", &["keys-combo"]);
    let action = gtk::Button::new();
    action.add_css_class("link");
    action.add_css_class("accent");
    action.set_halign(gtk::Align::Start);
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    let foot = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let dots = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    dots.set_hexpand(true);
    dots.set_valign(gtk::Align::Center);
    let next = gtk::Button::with_label("next");
    next.add_css_class("pill");
    foot.append(&dots);
    foot.append(&next);
    for w in [
        logo.upcast_ref::<gtk::Widget>(),
        title.upcast_ref(),
        words.upcast_ref(),
        keys.upcast_ref(),
        action.upcast_ref(),
        spacer.upcast_ref(),
        foot.upcast_ref(),
    ] {
        body.append(w);
    }
    window.set_child(Some(&body));

    let page = Rc::new(Cell::new(0usize));
    let command: Rc<std::cell::RefCell<Vec<String>>> = Rc::default();
    let show = {
        let (cards, page, command) = (cards.clone(), page.clone(), command.clone());
        let (title, words, keys, action, dots, next) =
            (title.clone(), words.clone(), keys.clone(), action.clone(), dots.clone(), next.clone());
        Rc::new(move || {
            let (t, w, k, a) = cards[page.get()];
            title.set_text(t);
            words.set_text(w);
            keys.set_text(k);
            keys.set_visible(!k.is_empty());
            match a {
                Some((text, cmd)) => {
                    action.set_label(text);
                    action.set_visible(true);
                    *command.borrow_mut() = cmd.iter().map(|s| s.to_string()).collect();
                }
                None => action.set_visible(false),
            }
            while let Some(c) = dots.first_child() {
                dots.remove(&c);
            }
            for i in 0..cards.len() {
                let d = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                d.add_css_class("welcome-dot");
                if i == page.get() {
                    d.add_css_class("on");
                }
                dots.append(&d);
            }
            next.set_label(if page.get() + 1 == cards.len() { "start" } else { "next" });
        })
    };
    show();
    {
        let command = command.clone();
        action.connect_clicked(move |_| {
            let cmd = command.borrow();
            if let Some((first, rest)) = cmd.split_first() {
                let _ = std::process::Command::new(first).args(rest).spawn();
            }
        });
    }
    {
        let (cards, page, show, window) = (cards.clone(), page.clone(), show.clone(), window.clone());
        next.connect_clicked(move |_| {
            if page.get() + 1 < cards.len() {
                page.set(page.get() + 1);
                show();
            } else {
                window.close();
            }
        });
    }
    // Seen once is enough for the first-time showing, however it's closed.
    window.connect_close_request(|_| {
        let file = seen_file();
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, "");
        glib::Propagation::Proceed
    });
    window.present();
}
