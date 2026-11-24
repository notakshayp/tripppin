//! The splash between logging in and the desktop: ink over every screen with the mark playing
//! its intro (the ring draws itself, the × spins in), started first thing in the niri session
//! (/etc/niri/desktop.kdl). It fades away once the bar and the orb are up, or after a few
//! seconds whatever happens, so the desktop arrives whole instead of piece by piece.

use crate::common;
use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::time::{Duration, Instant};

/// The ink behind it: the login screen's and the wallpaper's, so nothing changes colour.
const INK_BG: (f64, f64, f64) = (0.043, 0.063, 0.059);
/// Shown at least this long, so the intro always finishes.
const AT_LEAST: Duration = Duration::from_millis(1500);
/// Gone by this long, even if the bar or the orb never appears.
const AT_MOST: Duration = Duration::from_secs(6);
const FADE: Duration = Duration::from_millis(400);
/// The layer surfaces that mean the desktop is ready.
const READY: [&str; 2] = ["waybar", "tripppin-orb"];

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder().application_id("in.trippp.Splash").build();
    app.connect_activate(build);
    app.run_with_args(&[] as &[&str])
}

fn build(app: &gtk::Application) {
    let began = Instant::now();
    common::load_css();
    let window = gtk::Window::builder().application(app).decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("clear");
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-splash"));
    window.set_layer(Layer::Overlay);
    for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(e, true);
    }
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::None);

    let overlay = gtk::Overlay::new();
    let ink = gtk::DrawingArea::new();
    ink.set_hexpand(true);
    ink.set_vexpand(true);
    ink.set_draw_func(|_, cr, _, _| {
        cr.set_source_rgb(INK_BG.0, INK_BG.1, INK_BG.2);
        cr.paint().ok();
    });
    overlay.set_child(Some(&ink));
    let mark = common::animated_logo(112, true);
    // A touch above centre, where the eye lands.
    mark.set_margin_bottom(48);
    overlay.add_overlay(&mark);
    window.set_child(Some(&overlay));
    // Clicks go through to the desktop underneath as soon as it's there.
    window.set_can_target(false);
    window.present();

    // Ask niri, off the main loop, until the bar and the orb have their surfaces.
    let (tx, rx) = async_channel::bounded::<()>(1);
    std::thread::spawn(move || {
        while began.elapsed() < AT_MOST && !desktop_ready() {
            std::thread::sleep(Duration::from_millis(150));
        }
        let _ = tx.send_blocking(());
    });
    let (window, app) = (window.clone(), app.clone());
    glib::spawn_future_local(async move {
        let _ = rx.recv().await;
        if let Some(rest) = AT_LEAST.checked_sub(began.elapsed()) {
            glib::timeout_future(rest).await;
        }
        fade_out(&window, &app);
    });
}

/// Whether niri lists the bar's and the orb's layer surfaces.
fn desktop_ready() -> bool {
    std::process::Command::new("niri")
        .args(["msg", "--json", "layers"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| has_namespaces(&String::from_utf8_lossy(&o.stdout), &READY))
}

fn has_namespaces(json: &str, wanted: &[&str]) -> bool {
    let Ok(serde_json::Value::Array(layers)) = serde_json::from_str::<serde_json::Value>(json) else {
        return false;
    };
    wanted.iter().all(|w| layers.iter().any(|l| l["namespace"].as_str() == Some(w)))
}

fn fade_out(window: &gtk::Window, app: &gtk::Application) {
    let start = Instant::now();
    let app = app.clone();
    window.add_tick_callback(move |w, _| {
        let t = (start.elapsed().as_secs_f64() / FADE.as_secs_f64()).min(1.0);
        w.set_opacity(1.0 - common::ease_out(t));
        if t >= 1.0 {
            app.quit();
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_once_both_are_up() {
        let both =
            r#"[{"namespace":"wallpaper","output":"eDP-1"},{"namespace":"waybar"},{"namespace":"tripppin-orb"}]"#;
        assert!(has_namespaces(both, &READY));
        assert!(!has_namespaces(r#"[{"namespace":"waybar"}]"#, &READY));
        assert!(!has_namespaces("not json", &READY));
    }
}
