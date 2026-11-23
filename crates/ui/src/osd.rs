//! Volume, brightness, keyboard light, mic and caps-lock pop-ups: a small ink pill near the bottom of the screen with a
//! green bar, shown when a key changes the level and gone two seconds later. niri runs the
//! change itself (wpctl, brightnessctl) and then `tripppin-ui osd <kind>`; a second call while
//! the pill is up updates it instead of opening another.

use crate::common::{self, icons, label};
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::{Cell, RefCell};
use std::process::Command;
use std::rc::Rc;

const SHOW_FOR_MS: u32 = 1800;

/// What a pop-up shows: a word, a level from 0 to 1 and whether it's muted.
#[derive(Debug, PartialEq)]
pub struct Level {
    pub name: &'static str,
    pub value: f64,
    pub muted: bool,
    /// A word to show instead of the percentage ("on", "off").
    pub word: Option<&'static str>,
}

/// Whether caps lock is on, from the keyboards' caps-lock lights; None without one.
pub fn caps_lock() -> Option<bool> {
    let dirs = std::fs::read_dir("/sys/class/leds").ok()?;
    let mut found = None;
    for d in dirs.flatten() {
        if d.file_name().to_string_lossy().ends_with("::capslock") {
            let on = std::fs::read_to_string(d.path().join("brightness")).is_ok_and(|b| b.trim() != "0");
            found = Some(found.unwrap_or(false) || on);
        }
    }
    found
}

/// `wpctl get-volume` prints "Volume: 0.40" or "Volume: 0.40 [MUTED]".
pub fn parse_wpctl(out: &str) -> Option<(f64, bool)> {
    let rest = out.trim().strip_prefix("Volume:")?.trim();
    let value: f64 = rest.split_whitespace().next()?.parse().ok()?;
    Some((value, rest.contains("[MUTED]")))
}

/// `brightnessctl -m` prints "device,class,current,percent%,max".
pub fn parse_brightnessctl(out: &str) -> Option<f64> {
    let line = out.lines().next()?;
    let mut fields = line.split(',');
    let current: f64 = fields.nth(2)?.parse().ok()?;
    let max: f64 = fields.nth(1)?.parse().ok()?;
    (max > 0.0).then(|| current / max)
}

fn read(kind: &str) -> Option<Level> {
    let run = |cmd: &str, args: &[&str]| -> Option<String> {
        let out = Command::new(cmd).args(args).output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    match kind {
        "volume" => {
            let (value, muted) = parse_wpctl(&run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?)?;
            Some(Level { name: "vol", value, muted, word: None })
        }
        "mic" => {
            let (value, muted) = parse_wpctl(&run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SOURCE@"])?)?;
            Some(Level { name: "mic", value, muted, word: None })
        }
        "brightness" => {
            let value = parse_brightnessctl(&run("brightnessctl", &["-m", "-c", "backlight"])?)?;
            Some(Level { name: "light", value, muted: false, word: None })
        }
        "keyboard" => {
            let value = parse_brightnessctl(&run("brightnessctl", &["-m", "-d", "*::kbd_backlight"])?)?;
            Some(Level { name: "keys", value, muted: false, word: None })
        }
        "caps" => {
            let on = caps_lock()?;
            Some(Level {
                name: "caps",
                value: if on { 1.0 } else { 0.0 },
                muted: !on,
                word: Some(if on { "on" } else { "off" }),
            })
        }
        _ => None,
    }
}

/// The icon for a level: a speaker, a mic, the sun, a keyboard, caps lock's arrow.
fn icon_for(level: &Level) -> char {
    match (level.name, level.muted) {
        ("vol", false) => icons::SOUND,
        ("vol", true) => icons::MUTED,
        ("mic", false) => icons::MIC,
        ("mic", true) => icons::MIC_OFF,
        ("light", _) => icons::SUN,
        ("caps", _) => icons::CAPS,
        _ => icons::KEYBOARD,
    }
}

struct Osd {
    window: gtk::Window,
    pill: gtk::Box,
    name: gtk::Label,
    bar: gtk::ProgressBar,
    value: gtk::Label,
    /// Bumped on every update; a hide timer only acts if nothing came after it.
    generation: Cell<u64>,
}

impl Osd {
    fn show(self: &Rc<Self>, app: &gtk::Application, level: &Level) {
        self.name.set_text(&icon_for(level).to_string());
        // The bar glides to the new level rather than jumping.
        let (from, to) = (self.bar.fraction(), level.value.clamp(0.0, 1.0));
        let begun = Cell::new(None::<i64>);
        self.bar.add_tick_callback(move |bar, clock| {
            let now = clock.frame_time();
            let t = (now - *begun.get().get_or_insert(now)) as f64 / 160_000.0;
            bar.set_fraction(from + (to - from) * common::ease_out(t));
            if t >= 1.0 { glib::ControlFlow::Break } else { glib::ControlFlow::Continue }
        });
        match level.word {
            Some(w) => self.value.set_text(w),
            None => self.value.set_text(&format!("{:.0}", (level.value * 100.0).round())),
        }
        if level.muted {
            self.bar.add_css_class("muted");
        } else {
            self.bar.remove_css_class("muted");
        }
        if !self.window.is_visible() {
            self.window.present();
            common::play(&self.pill, "pop");
        }
        self.pill.remove_css_class("leave");
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let (osd, app) = (self.clone(), app.clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(u64::from(SHOW_FOR_MS)), move || {
            if osd.generation.get() == generation {
                osd.pill.add_css_class("leave");
                glib::timeout_add_local_once(std::time::Duration::from_millis(170), move || {
                    if osd.generation.get() == generation {
                        osd.window.set_visible(false);
                        app.quit();
                    }
                });
            }
        });
    }
}

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("in.trippp.Osd")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let osd: Rc<RefCell<Option<Rc<Osd>>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, cmd| {
        let kind = cmd.arguments().iter().rev().find_map(|a| a.to_str().map(str::to_string)).unwrap_or_default();
        let Some(level) = read(&kind) else { return glib::ExitCode::FAILURE };
        let mut slot = osd.borrow_mut();
        let o = slot.get_or_insert_with(|| build(app)).clone();
        drop(slot);
        o.show(app, &level);
        glib::ExitCode::SUCCESS
    });
    // `tripppin-ui osd <kind>`: drop the "osd" word for GTK's own argument parsing.
    let args: Vec<String> = std::env::args().take(1).chain(std::env::args().skip(2)).collect();
    app.run_with_args(&args)
}

fn build(app: &gtk::Application) -> Rc<Osd> {
    crate::common::load_css();
    let window = gtk::Window::builder().application(app).decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("clear");
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-osd"));
    window.set_layer(Layer::Overlay);
    window.set_anchor(Edge::Bottom, true);
    window.set_margin(Edge::Bottom, 72);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_exclusive_zone(-1);
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    pill.add_css_class("osd");
    pill.set_margin_top(24);
    pill.set_margin_bottom(24);
    pill.set_margin_start(24);
    pill.set_margin_end(24);
    let name = common::icon(icons::SOUND, &["osd-icon"]);
    name.set_width_chars(2);
    let bar = gtk::ProgressBar::new();
    bar.add_css_class("osd-bar");
    bar.set_hexpand(true);
    bar.set_valign(gtk::Align::Center);
    let value = label("0", &["osd-value"]);
    value.set_width_chars(3);
    value.set_xalign(1.0);
    pill.append(&name);
    pill.append(&bar);
    pill.append(&value);
    window.set_child(Some(&pill));
    // Never takes clicks: the pop-up is information, not a control.
    window.connect_map(|w| {
        if let Some(surface) = w.surface() {
            surface.set_input_region(Some(&gtk::cairo::Region::create()));
        }
    });
    Rc::new(Osd { window, pill, name, bar, value, generation: Cell::new(0) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_volume_and_brightness() {
        assert_eq!(parse_wpctl("Volume: 0.40\n"), Some((0.40, false)));
        assert_eq!(parse_wpctl("Volume: 0.75 [MUTED]\n"), Some((0.75, true)));
        assert_eq!(parse_wpctl("nothing"), None);
        assert_eq!(parse_brightnessctl("amdgpu_bl1,backlight,128,50%,255\n"), Some(128.0 / 255.0));
        assert_eq!(parse_brightnessctl("x,backlight,0,0%,0"), None);
    }
}
