//! The first time someone logs in and there's no model yet: a card that explains the AI runs
//! on this computer, names the model picked for its hardware, and downloads it with a progress
//! bar. Closing it lets the download carry on; a notification says when the AI is ready.

use crate::common::{self, label};
use gtk::glib;
use gtk::prelude::*;
use hwdetect::Tier;

pub fn run() -> glib::ExitCode {
    let Ok(tier) = trip::model::recommended() else { return glib::ExitCode::SUCCESS };
    if trip::model::is_installed(&tier) {
        return glib::ExitCode::SUCCESS;
    }
    let app = gtk::Application::builder().application_id("in.trippp.FirstBoot").build();
    app.connect_activate(move |app| build(app, tier.clone()));
    app.run_with_args(&[] as &[&str])
}

fn machine() -> String {
    let Ok(hw) = hwdetect::detect() else { return "this computer".into() };
    // Round to the marketing size: 15.3 GiB of usable memory is a "16 gb" machine.
    let mem = format!("{:.0} gb", (hw.memory_bytes as f64 / 1e9).ceil());
    match hw.gpus.iter().find(|g| g.vram_bytes.unwrap_or(0) > 0).or(hw.gpus.first()) {
        Some(g) => format!("{mem} · {}", format!("{:?}", g.vendor).to_lowercase()),
        None => mem,
    }
}

enum Progress {
    Bytes(u64, u64, f64),
    Done(Result<(), String>),
}

fn build(app: &gtk::Application, tier: Tier) {
    common::load_css();
    let window = gtk::Window::builder().application(app).title("welcome").default_width(620).build();
    window.add_css_class("tripppin");
    window.add_css_class("fb");
    // A plain title bar of ours, so the close button has its own row above the card.
    let header = gtk::HeaderBar::new();
    header.set_title_widget(Some(&gtk::Label::new(None)));
    window.set_titlebar(Some(&header));
    let card = gtk::Box::new(gtk::Orientation::Vertical, 22);
    card.add_css_class("fb-card");
    card.set_size_request(620, -1);

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    let orb = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    orb.add_css_class("ask-orb");
    orb.set_size_request(48, 48);
    let s = common::logo(19);
    s.set_hexpand(true);
    s.set_halign(gtk::Align::Center);
    s.set_valign(gtk::Align::Center);
    orb.append(&s);
    let name = common::display_name(&common::current_user()).to_lowercase();
    head.append(&orb);
    head.append(&label(&format!("welcome, {name}."), &["fb-title"]));

    let body = label(
        "one last thing: the ai. it runs on this computer, not in the cloud, so it needs a model sized for your hardware.",
        &["body"],
    );
    body.set_wrap(true);
    body.set_max_width_chars(60);

    let stats = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    stats.set_homogeneous(true);
    for (k, v) in [
        ("this computer", machine()),
        ("the model", tier.model.to_lowercase()),
        ("download", format!("{:.1} gb", tier.download_gb())),
    ] {
        let stat = gtk::Box::new(gtk::Orientation::Vertical, 4);
        stat.add_css_class("stat");
        stat.append(&label(k, &["caption"]));
        stat.append(&label(&v, &["stat-value"]));
        stats.append(&stat);
    }

    let bar = gtk::ProgressBar::new();
    let notes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let left = label("starting…", &["progress-note"]);
    left.set_hexpand(true);
    let right = label("", &["progress-note"]);
    notes.append(&left);
    notes.append(&right);
    let progress = gtk::Box::new(gtk::Orientation::Vertical, 8);
    progress.append(&bar);
    progress.append(&notes);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let cloud = gtk::Button::with_label("use a cloud key instead");
    cloud.add_css_class("link");
    cloud.set_hexpand(true);
    cloud.set_halign(gtk::Align::Start);
    {
        let left = left.clone();
        cloud.connect_clicked(move |b| {
            b.set_sensitive(false);
            left.set_text("cloud keys arrive in a later update; the download keeps going.");
        });
    }
    let hide = gtk::Button::with_label("keep going in the background");
    hide.add_css_class("secondary");
    {
        let window = window.clone();
        hide.connect_clicked(move |_| window.set_visible(false));
    }
    actions.append(&cloud);
    actions.append(&hide);

    card.append(&head);
    card.append(&body);
    card.append(&stats);
    card.append(&progress);
    card.append(&actions);
    window.set_child(Some(&card));
    // Closing it is the same as "keep going in the background".
    window.connect_close_request(|w| {
        w.set_visible(false);
        glib::Propagation::Stop
    });
    let hold = app.hold();

    let (tx, rx) = async_channel::unbounded::<Progress>();
    {
        let tier = tier.clone();
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let result = trip::model::pull_tier(&tier, &mut |done, total, rate| {
                let _ = progress_tx.send_blocking(Progress::Bytes(done, total, rate));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send_blocking(Progress::Done(result));
        });
    }
    let app = app.clone();
    let shown = window.clone();
    glib::spawn_future_local(async move {
        let _hold = hold;
        while let Ok(p) = rx.recv().await {
            match p {
                Progress::Bytes(done, total, rate) => {
                    bar.set_fraction(done as f64 / total.max(1) as f64);
                    let mut text = format!("{:.1} of {:.1} gb", done as f64 / 1e9, total as f64 / 1e9);
                    if rate > 0.0 {
                        text.push_str(&format!(" · {:.0} mb/s", rate / 1e6));
                        let secs = (total.saturating_sub(done)) as f64 / rate;
                        right.set_text(&if secs < 90.0 {
                            format!("about {:.0} s", secs.max(1.0))
                        } else {
                            format!("about {:.0} min", (secs / 60.0).ceil())
                        });
                    }
                    left.set_text(&text);
                }
                Progress::Done(Ok(())) => {
                    left.set_text("checked and installed. starting the ai…");
                    right.set_text("");
                    let _ = std::process::Command::new("systemctl").args(["restart", "tripppin-llm.service"]).status();
                    let _ = std::process::Command::new("notify-send")
                        .args([
                            "-a",
                            "tripppin",
                            "-i",
                            "tripppin",
                            "the ai is ready",
                            "press super + space and ask anything.",
                        ])
                        .status();
                    app.quit();
                }
                Progress::Done(Err(e)) => {
                    left.set_text(&format!("the download stopped: {}", e.to_lowercase()));
                    right.set_text("");
                    shown.present();
                }
            }
        }
    });
    window.present();
}
