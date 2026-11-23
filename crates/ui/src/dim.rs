//! The screen fading down before it locks (swayidle runs `tripppin-ui dim` half a minute
//! before the lock and stops it when you're back). An overlay rather than the backlight, so
//! it works on every screen; it takes no input, so the first touch goes where you meant it.

use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder().application_id("in.trippp.Dim").build();
    app.connect_activate(|app| {
        crate::common::load_css();
        let window = gtk::Window::builder().application(app).decorated(false).build();
        window.add_css_class("dim");
        window.init_layer_shell();
        window.set_namespace(Some("tripppin-dim"));
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::None);
        for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(e, true);
        }
        window.set_exclusive_zone(-1);
        window.set_opacity(0.0);
        window.connect_map(|w| {
            if let Some(surface) = w.surface() {
                surface.set_input_region(Some(&gtk::cairo::Region::create()));
            }
        });
        window.present();
        // Fade to 60% black over about two seconds.
        let step = std::cell::Cell::new(0);
        glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
            step.set(step.get() + 1);
            window.set_opacity(f64::from(step.get()) / 50.0 * 0.6);
            if step.get() >= 50 { glib::ControlFlow::Break } else { glib::ControlFlow::Continue }
        });
    });
    app.run_with_args(&[] as &[&str])
}
