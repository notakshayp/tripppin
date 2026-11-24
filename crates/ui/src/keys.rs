//! The shortcuts cheat sheet (Super + /): every shortcut on a dimmed screen, in three
//! columns. Any key, a click, or Super + / again closes it.

use crate::common::{SHORTCUTS, label};
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::RefCell;
use std::rc::Rc;

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("in.trippp.Keys")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let window: Rc<RefCell<Option<gtk::Window>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, _| {
        let mut slot = window.borrow_mut();
        if slot.is_some() {
            app.quit();
        } else {
            let w = build(app);
            w.present();
            if let Some(c) = w.child() {
                crate::common::play(&c, "pop");
            }
            *slot = Some(w);
        }
        glib::ExitCode::SUCCESS
    });
    let args: Vec<String> = std::env::args().take(1).collect();
    app.run_with_args(&args)
}

fn build(app: &gtk::Application) -> gtk::Window {
    crate::common::load_css();
    let window = gtk::Window::builder().application(app).decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("clear");
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-keys"));
    window.set_layer(Layer::Overlay);
    for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(e, true);
    }
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::Exclusive);

    let shade = gtk::Box::new(gtk::Orientation::Vertical, 0);
    shade.add_css_class("backdrop");
    let card = gtk::Box::new(gtk::Orientation::Vertical, 22);
    card.add_css_class("keys-card");
    card.set_halign(gtk::Align::Center);
    card.set_valign(gtk::Align::Center);
    card.set_vexpand(true);
    card.append(&label("Shortcuts", &["keys-title"]));
    let columns = gtk::Box::new(gtk::Orientation::Horizontal, 40);
    for (group, list) in SHORTCUTS {
        let col = gtk::Box::new(gtk::Orientation::Vertical, 12);
        col.append(&label(group, &["caption"]));
        for (what, combo) in list {
            let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
            row.append(&label(combo, &["keys-combo"]));
            row.append(&label(what, &["keys-what"]));
            col.append(&row);
        }
        columns.append(&col);
    }
    card.append(&columns);
    card.append(&label("Press any key to close", &["hint"]));
    shade.append(&card);
    window.set_child(Some(&shade));

    let close = {
        let app = app.clone();
        move || app.quit()
    };
    let keys = gtk::EventControllerKey::new();
    {
        let close = close.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            // Super on its own is the start of Super + / again; let that toggle instead.
            if !matches!(key, gtk::gdk::Key::Super_L | gtk::gdk::Key::Super_R) {
                close();
            }
            glib::Propagation::Stop
        });
    }
    window.add_controller(keys);
    let click = gtk::GestureClick::new();
    click.connect_released(move |_, _, _, _| close());
    window.add_controller(click);
    window
}
