//! The calendar that drops from the clock: today's date and the month. Clicking the clock
//! again (a second `tripppin-ui calendar`) or pressing Esc closes it.

use crate::common::label;
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::RefCell;
use std::rc::Rc;

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("in.trippp.Calendar")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let window: Rc<RefCell<Option<gtk::Window>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, _| {
        let mut slot = window.borrow_mut();
        match slot.as_ref() {
            // Already open: this click is the one that closes it.
            Some(w) => {
                w.close();
                app.quit();
            }
            None => {
                let w = build(app);
                w.present();
                if let Some(c) = w.child() {
                    crate::common::play(&c, "drop");
                }
                *slot = Some(w);
            }
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
    window.set_namespace(Some("tripppin-calendar"));
    window.set_layer(Layer::Top);
    window.set_anchor(Edge::Top, true);
    window.set_margin(Edge::Top, 52);
    window.set_keyboard_mode(KeyboardMode::OnDemand);

    let card = gtk::Box::new(gtk::Orientation::Vertical, 14);
    card.add_css_class("calendar-card");
    let now = glib::DateTime::now_local().ok();
    let day = now.as_ref().and_then(|t| t.format("%A").ok()).map(|s| s.to_lowercase()).unwrap_or_default();
    let date = now.as_ref().and_then(|t| t.format("%-d %B %Y").ok()).map(|s| s.to_lowercase()).unwrap_or_default();
    card.append(&label(&day, &["calendar-day"]));
    card.append(&label(&date, &["calendar-date"]));
    let month = gtk::Calendar::new();
    month.add_css_class("month");
    card.append(&month);
    let events = label("no events · connecting a calendar comes in a later update", &["row-sub"]);
    events.set_wrap(true);
    events.set_xalign(0.0);
    events.set_max_width_chars(34);
    card.append(&events);
    window.set_child(Some(&card));

    let keys = gtk::EventControllerKey::new();
    {
        let (window, app) = (window.clone(), app.clone());
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                window.close();
                app.quit();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    window.add_controller(keys);
    window
}
