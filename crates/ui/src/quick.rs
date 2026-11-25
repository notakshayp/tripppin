//! `tripppin-ui quick`: the one quick panel, opened by the taskbar's status text. Big flat toggle
//! tiles (Wi-Fi, Bluetooth, airplane, do not disturb, night light), volume and brightness
//! sliders, the battery, and buttons for Settings, Lock and Power. A layer-shell popup at the
//! bottom-right above the taskbar; Esc or a click outside closes it, and a second run does too.
//!
//! `tripppin-ui toggle <wifi|bluetooth|airplane|night-light|dnd> [--state]`: the same switches
//! for scripts and the AI (formerly swaync's buttons grid). With `--state` prints `true` or `false` for the
//! tile; without, sets it from `$SWAYNC_TOGGLE_STATE`. In Rust rather than inline shell:
//! swaync runs each command inside `/bin/sh -c "…"`, so any double quote in the config
//! breaks it, and the tiles then never show as on.

use std::process::{Command, Stdio};

/// A command's output, or nothing after two seconds (`bluetoothctl` waits forever for a
/// controller on a computer that has none).
fn output(cmd: &str, args: &[&str]) -> String {
    let child = Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let Ok(mut child) = child else { return String::new() };
    let mut out = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(out) = out.as_mut() {
            let _ = std::io::Read::read_to_string(out, &mut text);
        }
        let _ = tx.send(text);
    });
    let text = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or_default();
    let _ = child.kill();
    let _ = child.wait();
    text
}

fn run(cmd: &str, args: &[&str]) {
    let _ = Command::new(cmd).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

fn notify(title: &str, body: &str, icon: &str) {
    run("notify-send", &["-a", "Quick settings", "-i", icon, title, body]);
}

/// `nmcli -t -f TYPE device` lists a wifi device.
pub fn has_wifi(devices: &str) -> bool {
    devices.lines().any(|l| l.trim() == "wifi")
}

/// `bluetoothctl show` says the adapter is powered.
pub fn bluetooth_powered(show: &str) -> bool {
    show.lines().any(|l| l.trim() == "Powered: yes")
}

/// Do not disturb is on while Plasma's `NotificationsInhibitedUntil` is set.
fn dnd_on() -> bool {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let base =
        std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).unwrap_or_else(|| home.join(".config"));
    std::fs::read_to_string(base.join("plasmanotifyrc")).is_ok_and(|t| dnd_in(&t))
}

pub fn dnd_in(rc: &str) -> bool {
    let mut in_group = false;
    for l in rc.lines().map(str::trim) {
        if l.starts_with('[') {
            in_group = l == "[DoNotDisturb]";
        } else if in_group && let Some(v) = l.strip_prefix("NotificationsInhibitedUntil=") {
            return !v.trim().is_empty();
        }
    }
    false
}

fn state(what: &str) -> bool {
    match what {
        "wifi" => {
            has_wifi(&output("nmcli", &["-t", "-f", "TYPE", "device"]))
                && output("nmcli", &["radio", "wifi"]).trim() == "enabled"
        }
        "bluetooth" => bluetooth_powered(&output("bluetoothctl", &["show"])),
        "airplane" => {
            output("nmcli", &["radio", "wifi"]).trim() == "disabled"
                && output("nmcli", &["radio", "wwan"]).trim() != "enabled"
        }
        "dnd" => dnd_on(),
        "night-light" => {
            Command::new("pgrep").args(["-x", "wlsunset"]).stdout(Stdio::null()).status().is_ok_and(|s| s.success())
        }
        _ => false,
    }
}

fn set(what: &str, on: bool) {
    let word = if on { "on" } else { "off" };
    match what {
        "wifi" => {
            if !has_wifi(&output("nmcli", &["-t", "-f", "TYPE", "device"])) {
                notify(
                    "No Wi-Fi on this computer",
                    "There's no Wi-Fi adapter, so it uses a wired connection.",
                    "network-wireless-disabled",
                );
                return;
            }
            run("nmcli", &["radio", "wifi", word]);
        }
        "bluetooth" => {
            if !output("bluetoothctl", &["list"]).contains("Controller") {
                notify("No Bluetooth on this computer", "There's no Bluetooth adapter.", "bluetooth-disabled");
                return;
            }
            run("bluetoothctl", &["power", word]);
        }
        "dnd" => {
            // Plasma's notification server watches this key (kwriteconfig6 tells it).
            let base = ["--file", "plasmanotifyrc", "--group", "DoNotDisturb", "--key", "NotificationsInhibitedUntil"];
            if on {
                run("kwriteconfig6", &[base[0], base[1], base[2], base[3], base[4], base[5], "2099-01-01T00:00:00"]);
            } else {
                run("kwriteconfig6", &[base[0], base[1], base[2], base[3], base[4], base[5], "--delete"]);
            }
        }
        // Airplane mode on turns every radio off.
        "airplane" => run("nmcli", &["radio", "all", if on { "off" } else { "on" }]),
        "night-light" => {
            if on {
                if !state("night-light") {
                    use std::os::unix::process::CommandExt;
                    let _ = Command::new("wlsunset")
                        .args(["-T", "4500", "-t", "3500"])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .process_group(0)
                        .spawn();
                }
            } else {
                run("pkill", &["-x", "wlsunset"]);
            }
        }
        _ => {}
    }
}

pub fn main(args: &[String]) -> gtk::glib::ExitCode {
    let what = args.first().map(String::as_str).unwrap_or("");
    if !["wifi", "bluetooth", "airplane", "night-light", "dnd"].contains(&what) {
        eprintln!("usage: tripppin-ui toggle wifi|bluetooth|airplane|night-light|dnd [--state]");
        return gtk::glib::ExitCode::from(2);
    }
    if args.iter().any(|a| a == "--state") {
        println!("{}", state(what));
    } else {
        let on = std::env::var("SWAYNC_TOGGLE_STATE").is_ok_and(|v| v == "true");
        set(what, on);
    }
    gtk::glib::ExitCode::SUCCESS
}

// ---- the panel ----------------------------------------------------------------------

use crate::common;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The taskbar's height: the panel sits on top of it.
const TASKBAR: i32 = 40;

/// The switches, in the order they are drawn: id and the name on the tile.
const TILES: [(&str, &str); 5] = [
    ("wifi", "wi-fi"),
    ("bluetooth", "bluetooth"),
    ("airplane", "airplane"),
    ("dnd", "do not disturb"),
    ("night-light", "night light"),
];

const CSS: &str = r#"
window.quick-win { background: transparent; color: #fff; font-family: Inter, sans-serif; }
window.quick-win * { border-radius: 0; }
.quick-panel { background: #101418; padding: 5px; }
.quick-panel.leave { animation: quick-out 150ms ease-out forwards; }
.quick-panel.rise-a, .quick-panel.rise-b { animation-duration: 200ms; animation-timing-function: ease-out; }
@keyframes quick-out { to { opacity: 0; transform: translateY(8px); } }
.quick-panel button { border: none; box-shadow: none; padding: 0; transition: background 150ms ease-out, opacity 150ms ease-out; }
button.qtile { background: #1a2025; color: #9aa4ab; min-height: 84px; padding: 12px 14px; }
button.qtile:hover { background: #222a30; }
button.qtile.on { background: #1f6b45; color: #fff; }
button.qtile.on:hover { background: #247a4f; }
button.qtile:active { opacity: 0.85; }
.qtile .qname { font-size: 17px; font-weight: 300; }
.qtile .qstate { font-size: 11px; font-weight: 600; letter-spacing: 0.04em; opacity: 0.8; }
.qrow { background: #1a2025; padding: 14px 16px; }
.qrow .qlabel { font-size: 11px; font-weight: 600; letter-spacing: 0.04em; color: #9aa4ab; }
.qrow .qvalue { font-size: 11px; color: #9aa4ab; }
.quick-panel scale trough { background: #2a343b; background-image: none; border: none; box-shadow: none; outline: none; min-height: 4px; }
.quick-panel scale highlight { background: #3fae7a; background-image: none; border: none; min-height: 4px; }
.quick-panel scale slider { background: #fff; background-image: none; min-width: 10px; min-height: 22px; border: none; box-shadow: none; outline: none; }
.quick-panel scale { padding: 0; min-height: 22px; }
.qbattery { background: #1a2025; padding: 12px 16px; font-size: 13px; color: #9aa4ab; }
button.qact { background: #3a4a5a; color: #fff; min-height: 48px; font-size: 12px; font-weight: 600; letter-spacing: 0.04em; }
button.qact:hover { background: #46586a; }
button.qact.power { background: #c2502e; }
button.qact.power:hover { background: #d05d3a; }
button.qopt { background: #1a2025; color: #fff; min-height: 44px; font-size: 14px; font-weight: 300; }
button.qopt:hover { background: #222a30; }
"#;

fn spawn(cmd: &str, args: &[&str]) {
    if let Ok(mut c) =
        Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    {
        std::thread::spawn(move || c.wait());
    }
}

/// Everything the panel shows, read off the UI thread.
struct Snapshot {
    toggles: Vec<bool>,
    volume: Option<f64>,
    brightness: Option<f64>,
    battery: Option<crate::battery::Charge>,
}

fn read_all() -> Snapshot {
    let toggles = std::thread::scope(|s| {
        let handles: Vec<_> = TILES.iter().map(|(id, _)| s.spawn(move || state(id))).collect();
        handles.into_iter().map(|h| h.join().unwrap_or(false)).collect()
    });
    let volume = crate::osd::parse_wpctl(&output("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])).map(|(v, _)| v);
    let brightness = crate::osd::parse_brightnessctl(&output("brightnessctl", &["-m", "-c", "backlight"]));
    let battery = crate::battery::read(std::path::Path::new("/sys/class/power_supply"));
    Snapshot { toggles, volume, brightness, battery }
}

fn battery_line(c: &crate::battery::Charge) -> String {
    let state = if c.charging {
        " · charging"
    } else if c.plugged {
        " · plugged in"
    } else {
        ""
    };
    format!("battery {}%{state}", c.percent)
}

struct Panel {
    app: gtk::Application,
    window: gtk::Window,
    root: gtk::Box,
    panel: gtk::Box,
    tiles: Vec<(gtk::Button, gtk::Label)>,
    volume: gtk::Scale,
    volume_value: gtk::Label,
    brightness: gtk::Scale,
    brightness_row: gtk::Box,
    battery: gtk::Label,
    closing: Cell<bool>,
    /// A slider moved by the code, not by a hand: don't send it back.
    quiet: Cell<bool>,
}

fn slider() -> gtk::Scale {
    let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    s.set_draw_value(false);
    s.set_hexpand(true);
    s
}

/// A slider row: a small label and its value above the bar.
fn slider_row(name: &str, value: &gtk::Label, scale: &gtk::Scale) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("qrow");
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let l = common::label(name, &["qlabel"]);
    l.set_hexpand(true);
    head.append(&l);
    head.append(value);
    row.append(&head);
    row.append(scale);
    row
}

/// Runs `apply` with the newest value, at most every 60 ms, so dragging a slider doesn't start a
/// process per pixel.
fn debounced(apply: impl Fn(f64) + 'static) -> impl Fn(f64) + 'static {
    let pending: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    let apply = Rc::new(apply);
    move |v| {
        let idle = pending.replace(Some(v)).is_none();
        if idle {
            let (pending, apply) = (pending.clone(), apply.clone());
            glib::timeout_add_local_once(std::time::Duration::from_millis(60), move || {
                if let Some(v) = pending.take() {
                    apply(v);
                }
            });
        }
    }
}

impl Panel {
    fn open(app: &gtk::Application, window_mode: bool) -> Rc<Panel> {
        common::load_css();
        if let Some(display) = gdk::Display::default() {
            let provider = gtk::CssProvider::new();
            provider.load_from_string(CSS);
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
            );
        }
        let window = gtk::Window::builder().application(app).decorated(false).build();
        window.add_css_class("tripppin");
        window.add_css_class("quick-win");
        if window_mode {
            window.set_default_size(900, 700);
        } else {
            window.init_layer_shell();
            window.set_namespace(Some("tripppin-quick"));
            window.set_layer(Layer::Overlay);
            for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                window.set_anchor(e, true);
            }
            window.set_exclusive_zone(-1);
            window.set_keyboard_mode(KeyboardMode::OnDemand);
        }

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 5);
        panel.add_css_class("quick-panel");
        panel.set_size_request(380, -1);
        panel.set_halign(gtk::Align::End);
        panel.set_valign(gtk::Align::End);
        panel.set_margin_bottom(if window_mode { 0 } else { TASKBAR });

        // The toggles: three above, two below, big and flat.
        let grid = gtk::Grid::new();
        grid.set_row_spacing(5);
        grid.set_column_spacing(5);
        grid.set_column_homogeneous(true);
        let mut tiles = Vec::new();
        for (i, (_, name)) in TILES.iter().enumerate() {
            let b = gtk::Button::new();
            b.add_css_class("qtile");
            let inner = gtk::Box::new(gtk::Orientation::Vertical, 4);
            inner.set_valign(gtk::Align::End);
            let n = common::label(name, &["qname"]);
            n.set_wrap(true);
            let st = common::label("off", &["qstate"]);
            inner.append(&n);
            inner.append(&st);
            b.set_child(Some(&inner));
            let (col, row, span) = if i < 3 { (i as i32 * 2, 0, 2) } else { ((i as i32 - 3) * 3, 1, 3) };
            grid.attach(&b, col, row, span, 1);
            tiles.push((b, st));
        }
        panel.append(&grid);

        let volume = slider();
        let volume_value = common::label("", &["qvalue"]);
        let volume_row = slider_row("volume", &volume_value, &volume);
        panel.append(&volume_row);
        let brightness = slider();
        let brightness_value = common::label("", &["qvalue"]);
        let brightness_row = slider_row("brightness", &brightness_value, &brightness);
        brightness_row.set_visible(false);
        panel.append(&brightness_row);
        {
            let v = brightness_value.clone();
            brightness.connect_value_changed(move |s| v.set_text(&format!("{}%", s.value().round() as i32)));
        }

        let battery = common::label("", &["qbattery"]);
        battery.set_visible(false);
        panel.append(&battery);

        // Settings, lock, power. Power opens a row of its own choices.
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        actions.set_homogeneous(true);
        let settings = gtk::Button::with_label("settings");
        let lock = gtk::Button::with_label("lock");
        let power = gtk::Button::with_label("power");
        for (b, extra) in [(&settings, ""), (&lock, ""), (&power, "power")] {
            b.add_css_class("qact");
            if !extra.is_empty() {
                b.add_css_class(extra);
            }
            actions.append(b);
        }
        panel.append(&actions);
        let options = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        options.set_homogeneous(true);
        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
        revealer.set_transition_duration(160);
        revealer.set_child(Some(&options));
        panel.append(&revealer);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.append(&panel);
        window.set_child(Some(&root));

        let p = Rc::new(Panel {
            app: app.clone(),
            window: window.clone(),
            root: root.clone(),
            panel: panel.clone(),
            tiles,
            volume: volume.clone(),
            volume_value: volume_value.clone(),
            brightness: brightness.clone(),
            brightness_row: brightness_row.clone(),
            battery: battery.clone(),
            closing: Cell::new(false),
            quiet: Cell::new(false),
        });
        p.wire(&settings, &lock, &power, &revealer, &options);
        common::play(&panel, "rise");
        window.present();
        common::snapshot_hook(&panel);
        p.refresh();
        p
    }

    fn wire(
        self: &Rc<Self>,
        settings: &gtk::Button,
        lock: &gtk::Button,
        power: &gtk::Button,
        revealer: &gtk::Revealer,
        options: &gtk::Box,
    ) {
        // Click outside the panel, or Esc: back to the desktop.
        let outside = gtk::GestureClick::new();
        let u = Rc::clone(self);
        outside.connect_released(move |_, _, _, _| u.close());
        self.root.add_controller(outside);
        let swallow = gtk::GestureClick::new();
        swallow.connect_released(|g, _, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
        });
        self.panel.add_controller(swallow);
        let keys = gtk::EventControllerKey::new();
        let u = Rc::clone(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                u.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.window.add_controller(keys);

        for (i, (id, _)) in TILES.iter().enumerate() {
            let (u, id) = (Rc::clone(self), *id);
            self.tiles[i].0.connect_clicked(move |b| {
                let on = !b.has_css_class("on");
                u.show_toggle(id, on);
                std::thread::spawn(move || set(id, on));
                // The others can change with it (airplane turns Wi-Fi and Bluetooth off).
                let u = Rc::clone(&u);
                glib::timeout_add_local_once(std::time::Duration::from_millis(900), move || u.refresh());
            });
        }

        let u = Rc::clone(self);
        let apply = debounced(move |v| {
            let _ = &u;
            spawn("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", v / 100.0)]);
        });
        let u = Rc::clone(self);
        self.volume.connect_value_changed(move |s| {
            u.volume_value.set_text(&format!("{}%", s.value().round() as i32));
            if !u.quiet.get() {
                apply(s.value());
            }
        });
        let u = Rc::clone(self);
        let apply = debounced(move |v| {
            let _ = &u;
            spawn("brightnessctl", &["-c", "backlight", "set", &format!("{}%", v.round() as i32)]);
        });
        let u = Rc::clone(self);
        self.brightness.connect_value_changed(move |s| {
            if !u.quiet.get() {
                apply(s.value());
            }
        });

        let u = Rc::clone(self);
        settings.connect_clicked(move |_| {
            spawn("tripppin-ui", &["settings"]);
            u.close();
        });
        let u = Rc::clone(self);
        lock.connect_clicked(move |_| {
            spawn("loginctl", &["lock-session"]);
            u.close();
        });
        for (label, cmd, args) in [
            ("sleep", "systemctl", &["suspend"][..]),
            ("restart", "systemctl", &["reboot"][..]),
            ("shut down", "systemctl", &["poweroff"][..]),
            ("log out", "sh", &["-c", "loginctl terminate-session \"$XDG_SESSION_ID\""][..]),
        ] {
            let b = gtk::Button::with_label(label);
            b.add_css_class("qopt");
            let u = Rc::clone(self);
            b.connect_clicked(move |_| {
                spawn(cmd, args);
                u.close();
            });
            options.append(&b);
        }
        let revealer = revealer.clone();
        power.connect_clicked(move |_| revealer.set_reveal_child(!revealer.reveals_child()));
    }

    fn show_toggle(&self, id: &str, on: bool) {
        let Some(i) = TILES.iter().position(|(t, _)| *t == id) else { return };
        let (b, st) = &self.tiles[i];
        if on {
            b.add_css_class("on");
        } else {
            b.remove_css_class("on");
        }
        st.set_text(if on { "on" } else { "off" });
    }

    /// Reads the real state of everything and shows it.
    fn refresh(self: &Rc<Self>) {
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = tx.send_blocking(read_all());
        });
        let u = Rc::clone(self);
        glib::spawn_future_local(async move {
            let Ok(snap) = rx.recv().await else { return };
            for (i, on) in snap.toggles.iter().enumerate() {
                u.show_toggle(TILES[i].0, *on);
            }
            u.quiet.set(true);
            if let Some(v) = snap.volume {
                u.volume.set_value((v * 100.0).round());
            }
            if let Some(b) = snap.brightness {
                u.brightness.set_value((b * 100.0).round());
            }
            u.brightness_row.set_visible(snap.brightness.is_some());
            u.quiet.set(false);
            match snap.battery {
                Some(c) => {
                    u.battery.set_text(&battery_line(&c));
                    u.battery.set_visible(true);
                }
                None => u.battery.set_visible(false),
            }
        });
    }

    fn close(self: &Rc<Self>) {
        if self.closing.replace(true) {
            return;
        }
        self.panel.add_css_class("leave");
        let u = Rc::clone(self);
        glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
            u.window.close();
            u.app.quit();
        });
    }
}

/// `tripppin-ui quick [--window]`. A second run closes the open panel.
pub fn panel() -> glib::ExitCode {
    let window_mode = std::env::args().any(|a| a == "--window");
    let app = gtk::Application::builder()
        .application_id("in.trippp.Quick")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let open: Rc<RefCell<Option<Rc<Panel>>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, _| {
        let existing = open.borrow().clone();
        match existing {
            Some(p) => p.close(),
            None => *open.borrow_mut() = Some(Panel::open(app, window_mode)),
        }
        glib::ExitCode::SUCCESS
    });
    let args: Vec<String> = std::env::args().take(1).collect();
    app.run_with_args(&args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_do_not_disturb() {
        assert!(dnd_in("[DoNotDisturb]\nNotificationsInhibitedUntil=2099-01-01T00:00:00\n"));
        assert!(!dnd_in("[DoNotDisturb]\nWhenScreensMirrored=true\n"));
        assert!(!dnd_in("[Other]\nNotificationsInhibitedUntil=x\n"));
    }

    #[test]
    fn reads_nmcli_and_bluetoothctl() {
        assert!(has_wifi("ethernet\nwifi\nloopback\n"));
        assert!(!has_wifi("ethernet\nwifi-p2p\nloopback\n"));
        assert!(bluetooth_powered("Controller 00:11 (public)\n\tName: x\n\tPowered: yes\n"));
        assert!(!bluetooth_powered("Controller 00:11 (public)\n\tPowered: no\n"));
    }
}
