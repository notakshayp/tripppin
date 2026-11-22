//! Settings: one window, a sidebar of sections, each a page in the Tripppin style.
//! `tripppin-ui settings [section]` opens it (on a section); a second call brings it forward.
//! Sections are added here as they're built; each talks to the system service behind it.

use crate::apps;
use crate::bt;
use crate::common::label;
use crate::display;
use crate::nm;
use crate::power;
use crate::snd;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

/// The sections, in sidebar order: (id, title).
const SECTIONS: [(&str, &str); 9] = [
    ("wifi", "wi-fi"),
    ("bluetooth", "bluetooth"),
    ("sound", "sound"),
    ("display", "display"),
    ("appearance", "appearance"),
    ("power", "power & battery"),
    ("ai", "ai"),
    ("notifications", "notifications"),
    ("apps", "apps"),
];

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("in.trippp.Settings")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let window: Rc<RefCell<Option<(gtk::ApplicationWindow, gtk::Stack, gtk::ListBox)>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, cmd| {
        let wanted = cmd.arguments().iter().filter_map(|a| a.to_str().map(str::to_string)).nth(1);
        let mut slot = window.borrow_mut();
        let (win, stack, nav) = slot.get_or_insert_with(|| build(app)).clone();
        drop(slot);
        if let Some(i) = wanted.and_then(|w| SECTIONS.iter().position(|(id, _)| *id == w)) {
            stack.set_visible_child_name(SECTIONS[i].0);
            if let Some(row) = nav.row_at_index(i as i32) {
                nav.select_row(Some(&row));
            }
        }
        win.present();
        glib::ExitCode::SUCCESS
    });
    let args: Vec<String> = std::env::args().take(1).chain(std::env::args().skip(2)).collect();
    app.run_with_args(&args)
}

fn build(app: &gtk::Application) -> (gtk::ApplicationWindow, gtk::Stack, gtk::ListBox) {
    crate::common::load_css();
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("settings")
        .default_width(1040)
        .default_height(660)
        .build();
    window.add_css_class("tripppin");
    window.add_css_class("settings");
    // No title text, as in the design: just the close button.
    let header = gtk::HeaderBar::new();
    header.set_title_widget(Some(&gtk::Label::new(None)));
    header.add_css_class("bare");
    window.set_titlebar(Some(&header));

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_hexpand(true);
    stack.add_named(&wifi_page(), Some("wifi"));
    stack.add_named(&bluetooth_page(), Some("bluetooth"));
    stack.add_named(&sound_page(), Some("sound"));
    stack.add_named(&display_page(), Some("display"));
    stack.add_named(&appearance_page(), Some("appearance"));
    stack.add_named(&power_page(), Some("power"));
    stack.add_named(&ai_page(), Some("ai"));
    stack.add_named(&notifications_page(), Some("notifications"));
    stack.add_named(&apps_page(), Some("apps"));

    let side = gtk::Box::new(gtk::Orientation::Vertical, 10);
    side.add_css_class("sidebar");
    side.set_size_request(260, -1);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("search settings"));
    search.add_css_class("search");
    let nav = gtk::ListBox::new();
    nav.add_css_class("nav");
    for (_, title) in SECTIONS {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("dot");
        dot.set_valign(gtk::Align::Center);
        row.append(&dot);
        row.append(&label(title, &["nav-title"]));
        nav.append(&row);
    }
    {
        let stack = stack.clone();
        nav.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                stack.set_visible_child_name(SECTIONS[row.index() as usize].0);
            }
        });
    }
    {
        let nav = nav.clone();
        search.connect_search_changed(move |s| {
            let q = s.text().to_lowercase();
            let mut i = 0;
            while let Some(row) = nav.row_at_index(i) {
                row.set_visible(q.is_empty() || SECTIONS[i as usize].1.contains(q.as_str()));
                i += 1;
            }
        });
    }
    nav.select_row(nav.row_at_index(0).as_ref());
    side.append(&search);
    side.append(&nav);

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.append(&side);
    root.append(&stack);
    window.set_child(Some(&root));
    (window, stack, nav)
}

/// A page: a big lower-case title, a status line under it, optional control on the right.
fn page(title: &str) -> (gtk::ScrolledWindow, gtk::Box, gtk::Label, gtk::Box) {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 22);
    body.add_css_class("page");
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 4);
    titles.set_hexpand(true);
    titles.append(&label(title, &["page-title"]));
    let status = label("", &["page-sub"]);
    titles.append(&status);
    let right = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    right.set_valign(gtk::Align::Center);
    head.append(&titles);
    head.append(&right);
    body.append(&head);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_child(Some(&body));
    (scroll, body, status, right)
}

/// Runs `work` on a thread and hands its result to `done` on the GTK thread.
fn background<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static) {
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(work());
    });
    glib::spawn_future_local(async move {
        if let Ok(v) = rx.recv().await {
            done(v);
        }
    });
}

/// Signal-strength arcs, green when connected.
fn signal_icon(signal: u8, connected: bool) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(20);
    area.set_content_height(20);
    area.set_valign(gtk::Align::Center);
    let bars = match signal {
        70.. => 3,
        40..=69 => 2,
        _ => 1,
    };
    area.set_draw_func(move |_, cr, _, _| {
        let on = if connected { (0.341, 0.851, 0.604) } else { (0.79, 0.83, 0.81) };
        let off = (0.165, 0.212, 0.196);
        cr.set_line_width(2.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        for (i, r) in [5.0f64, 9.5, 14.0].iter().enumerate() {
            let c = if i < bars { on } else { off };
            cr.set_source_rgb(c.0, c.1, c.2);
            cr.arc(10.0, 17.0, *r, (-135f64).to_radians(), (-45f64).to_radians());
            cr.stroke().ok();
        }
        cr.set_source_rgb(on.0, on.1, on.2);
        cr.arc(10.0, 17.0, 1.4, 0.0, std::f64::consts::TAU);
        cr.fill().ok();
    });
    area
}

fn wifi_page() -> gtk::ScrolledWindow {
    let (scroll, body, status, right) = page("wi-fi");
    let switch = gtk::Switch::new();
    right.append(&switch);
    let list = gtk::ListBox::new();
    list.add_css_class("list-card");
    list.set_selection_mode(gtk::SelectionMode::None);
    let error = label("", &["error"]);
    error.set_visible(false);

    let extras = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    extras.set_homogeneous(true);
    let saved_label = label("none yet", &["card-sub"]);
    let hotspot_label = label("share this connection", &["card-sub"]);
    for (title, sub) in [
        ("hotspot", &hotspot_label),
        ("vpn", &label("wireguard, openvpn", &["card-sub"])),
        ("saved networks", &saved_label),
    ] {
        let card = gtk::Button::new();
        card.add_css_class("small-card");
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 4);
        inner.append(&label(title, &["card-title"]));
        inner.append(sub);
        card.set_child(Some(&inner));
        let hotspot_label = hotspot_label.clone();
        card.connect_clicked(move |_| match title {
            "hotspot" => {
                let l = hotspot_label.clone();
                l.set_text("starting…");
                background(nm::hotspot, move |r| match r {
                    Ok((ssid, psk)) => l.set_text(&format!("on · {ssid} · {psk}")),
                    Err(e) => l.set_text(&e),
                });
            }
            _ => {
                let _ = std::process::Command::new("nm-connection-editor").spawn();
            }
        });
        extras.append(&card);
    }

    body.append(&list);
    body.append(&error);
    body.append(&extras);

    let refresh: Rc<RefCell<Reload>> = Rc::new(RefCell::new(None));
    let open_row: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let refresh_fn: Rc<dyn Fn(bool)> = {
        let (list, status, switch, error, saved_label, refresh, open_row) = (
            list.clone(),
            status.clone(),
            switch.clone(),
            error.clone(),
            saved_label.clone(),
            refresh.clone(),
            open_row.clone(),
        );
        // A hotspot shares the connection over Wi-Fi, so its card goes when there's no card.
        let hotspot_card = hotspot_label.ancestor(gtk::Button::static_type());
        Rc::new(move |rescan: bool| {
            let hotspot_card = hotspot_card.clone();
            let (list, status, switch, error, saved_label, refresh, open_row) = (
                list.clone(),
                status.clone(),
                switch.clone(),
                error.clone(),
                saved_label.clone(),
                refresh.clone(),
                open_row.clone(),
            );
            background(
                move || {
                    (
                        nm::has_wifi(),
                        nm::wired(),
                        nm::wifi_enabled(),
                        nm::networks(rescan),
                        nm::saved(),
                        nm::wifi_address(),
                    )
                },
                move |(has_wifi, wired, enabled, nets, saved, address)| {
                    if let Some(card) = &hotspot_card {
                        card.set_visible(has_wifi);
                    }
                    if !saved.is_empty() {
                        saved_label.set_text(&format!("{} remembered", saved.len()));
                    }
                    if !has_wifi {
                        switch.set_visible(false);
                        list.set_visible(false);
                        status.set_text(if wired {
                            "no wi-fi on this computer · connected by cable"
                        } else {
                            "no wi-fi on this computer"
                        });
                        return;
                    }
                    switch.set_active(enabled);
                    while let Some(child) = list.first_child() {
                        list.remove(&child);
                    }
                    list.set_visible(enabled);
                    if !enabled {
                        status.set_text("wi-fi is off");
                        return;
                    }
                    match nets.iter().find(|n| n.in_use) {
                        Some(n) => status.set_text(&format!(
                            "connected to {} · {} · {}",
                            n.ssid,
                            n.band,
                            nm::strength(n.signal)
                        )),
                        None => status.set_text("not connected"),
                    }
                    for net in &nets {
                        let reload = refresh.borrow().clone();
                        list.append(&network_row(
                            net,
                            saved.contains(&net.ssid),
                            address.as_deref(),
                            &error,
                            &open_row,
                            reload,
                        ));
                    }
                    list.append(&hidden_row(&error, refresh.borrow().clone()));
                },
            );
        })
    };
    *refresh.borrow_mut() = Some(refresh_fn.clone());
    {
        let (refresh, error) = (refresh_fn.clone(), error.clone());
        switch.connect_state_set(move |_, on| {
            let (refresh, error) = (refresh.clone(), error.clone());
            background(
                move || nm::set_wifi(on),
                move |r| {
                    if let Err(e) = r {
                        error.set_text(&e);
                        error.set_visible(true);
                    }
                    refresh(true);
                },
            );
            glib::Propagation::Proceed
        });
    }
    refresh_fn(true);
    // Keep the list fresh while the page is open, without rescanning every time.
    let weak = scroll.downgrade();
    glib::timeout_add_seconds_local(12, move || {
        let Some(scroll) = weak.upgrade() else { return glib::ControlFlow::Break };
        if scroll.is_mapped() {
            refresh_fn(false);
        }
        glib::ControlFlow::Continue
    });
    scroll
}

type Reload = Option<Rc<dyn Fn(bool)>>;

fn network_row(
    net: &nm::Network,
    saved: bool,
    address: Option<&str>,
    error: &gtk::Label,
    open_row: &Rc<RefCell<Option<String>>>,
    reload: Reload,
) -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("net-row");
    if net.in_use {
        row.add_css_class("connected");
    }
    row.append(&signal_icon(net.signal, net.in_use));
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&label(&net.ssid, &[if net.in_use { "row-title-strong" } else { "row-title" }]));
    if net.in_use {
        text.append(&label(
            &format!("connected{}", address.map(|a| format!(" · {a}")).unwrap_or_default()),
            &["row-sub", "on"],
        ));
    } else if saved {
        text.append(&label("saved", &["row-sub"]));
    }
    row.append(&text);
    if net.secured && !net.in_use {
        let lock = label("●", &["lock"]);
        lock.set_tooltip_text(Some("needs a password"));
        row.append(&lock);
    }
    let action = gtk::Button::with_label(if net.in_use { "disconnect" } else { "connect" });
    action.add_css_class("link");
    if !net.in_use {
        action.add_css_class("accent");
    }
    row.append(&action);
    outer.append(&row);

    // Inline password, for a secured network this computer hasn't joined before.
    let ask = gtk::Box::new(gtk::Orientation::Vertical, 12);
    ask.add_css_class("ask-password");
    let ask_title = gtk::Label::new(None);
    ask_title.set_markup(&format!("password for <b>{}</b>", glib::markup_escape_text(&net.ssid)));
    ask_title.set_xalign(0.0);
    ask_title.add_css_class("row-title");
    let field_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let pw = gtk::PasswordEntry::new();
    pw.add_css_class("field");
    pw.set_show_peek_icon(true);
    pw.set_hexpand(true);
    let join = gtk::Button::with_label("connect");
    join.add_css_class("pill");
    field_row.append(&pw);
    field_row.append(&join);
    ask.append(&ask_title);
    ask.append(&field_row);
    ask.set_visible(open_row.borrow().as_deref() == Some(net.ssid.as_str()));
    outer.append(&ask);

    let ssid = net.ssid.clone();
    let needs_password = net.secured && !saved;
    let connect = {
        let (ssid, pw, error, reload, action, open_row) =
            (ssid.clone(), pw.clone(), error.clone(), reload.clone(), action.clone(), open_row.clone());
        Rc::new(move || {
            let (ssid2, secret) = (ssid.clone(), pw.text().to_string());
            let (error, reload, open_row) = (error.clone(), reload.clone(), open_row.clone());
            action.set_label("connecting…");
            error.set_visible(false);
            background(
                move || nm::connect(&ssid2, Some(&secret)),
                move |r| {
                    match r {
                        Ok(()) => *open_row.borrow_mut() = None,
                        Err(e) => {
                            error.set_text(&format!("couldn't join: {e}"));
                            error.set_visible(true);
                        }
                    }
                    if let Some(reload) = reload {
                        reload(false);
                    }
                },
            );
        })
    };
    {
        let (connect, ask, pw, open_row, reload, error) =
            (connect.clone(), ask.clone(), pw.clone(), open_row.clone(), reload.clone(), error.clone());
        let in_use = net.in_use;
        action.connect_clicked(move |b| {
            if in_use {
                let (ssid, reload, error) = (ssid.clone(), reload.clone(), error.clone());
                b.set_label("disconnecting…");
                background(
                    move || nm::disconnect(&ssid),
                    move |r| {
                        if let Err(e) = r {
                            error.set_text(&e);
                            error.set_visible(true);
                        }
                        if let Some(reload) = reload {
                            reload(false);
                        }
                    },
                );
            } else if needs_password {
                let show = !ask.is_visible();
                ask.set_visible(show);
                *open_row.borrow_mut() = show.then(|| ssid.clone());
                if show {
                    pw.grab_focus();
                }
            } else {
                connect();
            }
        });
    }
    {
        let connect = connect.clone();
        join.connect_clicked(move |_| connect());
    }
    pw.connect_activate(move |_| connect());

    // Right-click a saved network to forget it.
    if saved {
        let gesture = gtk::GestureClick::new();
        gesture.set_button(3);
        let (ssid, reload) = (net.ssid.clone(), reload.clone());
        gesture.connect_released(move |_, _, _, _| {
            let (ssid, reload) = (ssid.clone(), reload.clone());
            background(
                move || nm::forget(&ssid),
                move |_| {
                    if let Some(reload) = reload {
                        reload(false);
                    }
                },
            );
        });
        row.add_controller(gesture);
        row.set_tooltip_text(Some("right-click to forget this network"));
    }
    outer
}

fn hidden_row(error: &gtk::Label, reload: Reload) -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("net-row");
    let title = label("other network…", &["row-title", "muted"]);
    title.set_hexpand(true);
    row.append(&title);
    row.append(&label("hidden or enterprise", &["row-sub"]));
    let open = gtk::Button::with_label("join");
    open.add_css_class("link");
    open.add_css_class("accent");
    row.append(&open);
    outer.append(&row);
    let form = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    form.add_css_class("ask-password");
    let name = gtk::Entry::new();
    name.add_css_class("field");
    name.set_placeholder_text(Some("network name"));
    name.set_hexpand(true);
    let pw = gtk::PasswordEntry::new();
    pw.add_css_class("field");
    pw.set_hexpand(true);
    let join = gtk::Button::with_label("connect");
    join.add_css_class("pill");
    form.append(&name);
    form.append(&pw);
    form.append(&join);
    form.set_visible(false);
    outer.append(&form);
    {
        let form = form.clone();
        open.connect_clicked(move |_| form.set_visible(!form.is_visible()));
    }
    let error = error.clone();
    join.connect_clicked(move |_| {
        let (ssid, secret) = (name.text().trim().to_string(), pw.text().to_string());
        if ssid.is_empty() {
            return;
        }
        let (error, reload) = (error.clone(), reload.clone());
        background(
            move || nm::connect_hidden(&ssid, &secret),
            move |r| {
                if let Err(e) = r {
                    error.set_text(&format!("couldn't join: {e}"));
                    error.set_visible(true);
                }
                if let Some(reload) = reload {
                    reload(false);
                }
            },
        );
    });
    outer
}

/// A small caption above a list, in DM Mono.
fn caption(text: &str) -> gtk::Label {
    label(text, &["caption"])
}

fn clear(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn bluetooth_page() -> gtk::ScrolledWindow {
    let (scroll, body, status, right) = page("bluetooth");
    let switch = gtk::Switch::new();
    right.append(&switch);
    let mine_caption = caption("your devices");
    let mine = gtk::ListBox::new();
    mine.add_css_class("list-card");
    mine.set_selection_mode(gtk::SelectionMode::None);
    let near_head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let near_caption = caption("nearby");
    let spinner = gtk::Spinner::new();
    let again = gtk::Button::with_label("look again");
    again.add_css_class("link");
    again.add_css_class("accent");
    let filler = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    filler.set_hexpand(true);
    near_head.append(&near_caption);
    near_head.append(&spinner);
    near_head.append(&filler);
    near_head.append(&again);
    let near = gtk::ListBox::new();
    near.add_css_class("list-card");
    near.set_selection_mode(gtk::SelectionMode::None);
    let error = label("", &["error"]);
    error.set_visible(false);
    let more = gtk::Button::with_label("more options");
    more.add_css_class("link");
    more.set_halign(gtk::Align::Start);
    more.connect_clicked(|_| {
        let _ = std::process::Command::new("blueman-manager").spawn();
    });
    for w in [
        mine_caption.upcast_ref::<gtk::Widget>(),
        mine.upcast_ref(),
        near_head.upcast_ref(),
        near.upcast_ref(),
        error.upcast_ref(),
        more.upcast_ref(),
    ] {
        body.append(w);
    }
    let parts = [
        mine_caption.clone().upcast::<gtk::Widget>(),
        mine.clone().upcast(),
        near_head.clone().upcast(),
        near.clone().upcast(),
    ];

    let reload: Rc<RefCell<Reload>> = Rc::new(RefCell::new(None));
    let scan: Rc<dyn Fn()> = {
        let (near, spinner, reload, error) = (near.clone(), spinner.clone(), reload.clone(), error.clone());
        Rc::new(move || {
            spinner.start();
            let (near, spinner, reload, error) = (near.clone(), spinner.clone(), reload.clone(), error.clone());
            background(
                || bt::scan(8),
                move |found| {
                    spinner.stop();
                    clear(&near);
                    if found.is_empty() {
                        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        row.add_css_class("net-row");
                        row.append(&label("nothing nearby in pairing mode", &["row-sub"]));
                        near.append(&row);
                    }
                    for d in &found {
                        near.append(&device_row(d, false, &error, reload.borrow().clone()));
                    }
                },
            );
        })
    };
    let load: Rc<dyn Fn(bool)> = {
        let (mine, status, switch, error, reload, scan, parts, more) = (
            mine.clone(),
            status.clone(),
            switch.clone(),
            error.clone(),
            reload.clone(),
            scan.clone(),
            parts.clone(),
            more.clone(),
        );
        Rc::new(move |also_scan: bool| {
            let (mine, status, switch, error, reload, scan, parts, more) = (
                mine.clone(),
                status.clone(),
                switch.clone(),
                error.clone(),
                reload.clone(),
                scan.clone(),
                parts.clone(),
                more.clone(),
            );
            background(
                || (bt::has_adapter(), bt::status(), bt::paired()),
                move |(has, (on, name), devices)| {
                    switch.set_visible(has);
                    more.set_visible(has);
                    for p in &parts {
                        p.set_visible(has && on);
                    }
                    if !has {
                        status.set_text("no bluetooth on this computer");
                        return;
                    }
                    switch.set_active(on);
                    if !on {
                        status.set_text("bluetooth is off");
                        return;
                    }
                    status.set_text(&format!("visible as \"{}\" while this page is open", name.to_lowercase()));
                    clear(&mine);
                    if devices.is_empty() {
                        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        row.add_css_class("net-row");
                        row.append(&label("none yet: pair something from nearby", &["row-sub"]));
                        mine.append(&row);
                    }
                    for d in &devices {
                        mine.append(&device_row(d, true, &error, reload.borrow().clone()));
                    }
                    if also_scan {
                        scan();
                    }
                },
            );
        })
    };
    *reload.borrow_mut() = Some(load.clone());
    {
        let (load, error) = (load.clone(), error.clone());
        switch.connect_state_set(move |_, on| {
            let (load, error) = (load.clone(), error.clone());
            background(
                move || bt::set_power(on),
                move |r| {
                    if let Err(e) = r {
                        error.set_text(&e);
                        error.set_visible(true);
                    }
                    load(on);
                },
            );
            glib::Propagation::Proceed
        });
    }
    {
        let scan = scan.clone();
        again.connect_clicked(move |_| scan());
    }
    // Look for devices each time the page is opened.
    {
        let load = load.clone();
        scroll.connect_map(move |_| load(true));
    }
    scroll
}

fn device_row(d: &bt::Device, paired: bool, error: &gtk::Label, reload: Reload) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("net-row");
    row.add_css_class("dev-row");
    let word = bt::kind_word(&d.kind);
    let icon = gtk::CenterBox::new();
    icon.add_css_class("dev-icon");
    if d.connected {
        icon.add_css_class("on");
    }
    icon.set_valign(gtk::Align::Center);
    icon.set_center_widget(Some(&label(&word[..1], &["dev-letter"])));
    row.append(&icon);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&label(&d.name.to_lowercase(), &[if d.connected { "row-title-strong" } else { "row-title" }]));
    let sub = match (d.connected, d.battery) {
        (true, Some(b)) => format!("connected · battery {b}%"),
        (true, None) => "connected".to_string(),
        (false, _) if paired => format!("{word} · not connected"),
        _ => word.to_string(),
    };
    text.append(&label(&sub, &["row-sub", if d.connected { "on" } else { "plain" }]));
    row.append(&text);
    let verb = if !paired {
        "pair"
    } else if d.connected {
        "disconnect"
    } else {
        "connect"
    };
    let action = gtk::Button::with_label(verb);
    action.add_css_class("link");
    if verb != "disconnect" {
        action.add_css_class("accent");
    }
    row.append(&action);
    let address = d.address.clone();
    let error = error.clone();
    {
        let (address, error, reload) = (address.clone(), error.clone(), reload.clone());
        action.connect_clicked(move |b| {
            b.set_label(match verb {
                "pair" => "pairing…",
                "connect" => "connecting…",
                _ => "disconnecting…",
            });
            let (address, error, reload) = (address.clone(), error.clone(), reload.clone());
            background(
                move || match verb {
                    "pair" => bt::pair(&address),
                    "connect" => bt::connect(&address),
                    _ => bt::disconnect(&address),
                },
                move |r| {
                    if let Err(e) = r {
                        error.set_text(&format!("couldn't {verb}: {e}"));
                        error.set_visible(true);
                    }
                    if let Some(reload) = reload {
                        reload(false);
                    }
                },
            );
        });
    }
    if paired {
        row.set_tooltip_text(Some("right-click to remove this device"));
        let gesture = gtk::GestureClick::new();
        gesture.set_button(3);
        gesture.connect_released(move |_, _, _, _| {
            let (address, reload) = (address.clone(), reload.clone());
            background(
                move || bt::remove(&address),
                move |_| {
                    if let Some(reload) = reload {
                        reload(false);
                    }
                },
            );
        });
        row.add_controller(gesture);
    }
    row
}

/// Whether a device is muted, and how to change that.
type Mute = (bool, Box<dyn Fn(bool)>);

/// A row with a word, a green slider (0–100) and an optional mute switch.
fn level_row(name: &str, value: u32, on_change: impl Fn(u32) + 'static, mute: Option<Mute>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("net-row");
    let word = label(name, &["level-name"]);
    word.set_width_chars(6);
    row.append(&word);
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.add_css_class("level");
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    scale.set_value(f64::from(value.min(100)));
    let shown = label(&value.to_string(), &["level-value"]);
    shown.set_width_chars(3);
    shown.set_xalign(1.0);
    {
        let shown = shown.clone();
        scale.connect_value_changed(move |s| {
            let v = s.value().round() as u32;
            shown.set_text(&v.to_string());
            on_change(v);
        });
    }
    row.append(&scale);
    row.append(&shown);
    if let Some((muted, set)) = mute {
        let switch = gtk::Switch::new();
        switch.set_active(!muted);
        switch.set_valign(gtk::Align::Center);
        switch.set_tooltip_text(Some("on / muted"));
        switch.connect_state_set(move |_, on| {
            set(!on);
            glib::Propagation::Proceed
        });
        row.append(&switch);
    }
    row
}

fn fill_devices(list: &gtk::ListBox, devices: &[snd::Device], default: &str, output: bool, reload: Reload) {
    clear(list);
    if devices.is_empty() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.add_css_class("net-row");
        row.append(&label(
            if output { "no speakers or headphones found" } else { "no microphone found" },
            &["row-sub"],
        ));
        list.append(&row);
        return;
    }
    for d in devices {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("net-row");
        let in_use = d.name == default;
        if in_use {
            row.add_css_class("connected");
        }
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("dot");
        if in_use {
            dot.add_css_class("on");
        }
        dot.set_valign(gtk::Align::Center);
        row.append(&dot);
        let title = label(&d.description, &[if in_use { "row-title-strong" } else { "row-title" }]);
        title.set_hexpand(true);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&title);
        if !in_use {
            let use_it = gtk::Button::with_label("use this");
            use_it.add_css_class("link");
            use_it.add_css_class("accent");
            let (name, reload) = (d.name.clone(), reload.clone());
            use_it.connect_clicked(move |_| {
                let (name, reload) = (name.clone(), reload.clone());
                background(
                    move || snd::set_default(output, &name),
                    move |()| {
                        if let Some(reload) = reload {
                            reload(false);
                        }
                    },
                );
            });
            row.append(&use_it);
        }
        list.append(&row);
    }
    if let Some(d) = devices.iter().find(|d| d.name == default) {
        let (name, name2) = (d.name.clone(), d.name.clone());
        list.append(&level_row(
            if output { "volume" } else { "mic" },
            d.volume,
            move |v| snd::set_volume(output, &name, v),
            Some((d.muted, Box::new(move |m| snd::set_mute(output, &name2, m)))),
        ));
    }
}

fn sound_page() -> gtk::ScrolledWindow {
    let (scroll, body, status, _right) = page("sound");
    let outs = gtk::ListBox::new();
    let ins = gtk::ListBox::new();
    let apps = gtk::ListBox::new();
    for l in [&outs, &ins, &apps] {
        l.add_css_class("list-card");
        l.set_selection_mode(gtk::SelectionMode::None);
    }
    let more = gtk::Button::with_label("more options");
    more.add_css_class("link");
    more.set_halign(gtk::Align::Start);
    more.connect_clicked(|_| {
        let _ = std::process::Command::new("pavucontrol").spawn();
    });
    body.append(&caption("output"));
    body.append(&outs);
    body.append(&caption("input"));
    body.append(&ins);
    body.append(&caption("apps"));
    body.append(&apps);
    body.append(&more);

    let reload: Rc<RefCell<Reload>> = Rc::new(RefCell::new(None));
    let load: Rc<dyn Fn(bool)> = {
        let (outs, ins, apps, status, reload) =
            (outs.clone(), ins.clone(), apps.clone(), status.clone(), reload.clone());
        Rc::new(move |_| {
            let (outs, ins, apps, status, reload) =
                (outs.clone(), ins.clone(), apps.clone(), status.clone(), reload.clone());
            background(
                || (snd::outputs(), snd::inputs(), snd::streams()),
                move |((out_list, out_default), (in_list, in_default), streams)| {
                    let current = out_list.iter().find(|d| d.name == out_default);
                    status.set_text(&match current {
                        Some(d) if d.muted => format!("{} · muted", d.description),
                        Some(d) => format!("{} · {}%", d.description, d.volume),
                        None => "no sound device".to_string(),
                    });
                    fill_devices(&outs, &out_list, &out_default, true, reload.borrow().clone());
                    fill_devices(&ins, &in_list, &in_default, false, reload.borrow().clone());
                    clear(&apps);
                    if streams.is_empty() {
                        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        row.add_css_class("net-row");
                        row.append(&label("nothing is playing", &["row-sub"]));
                        apps.append(&row);
                    }
                    for st in &streams {
                        let index = st.index;
                        apps.append(&level_row(&st.app, st.volume, move |v| snd::set_stream_volume(index, v), None));
                    }
                },
            );
        })
    };
    *reload.borrow_mut() = Some(load.clone());
    scroll.connect_map(move |_| load(false));
    scroll
}

/// A row with a word on the left and a control on the right.
fn setting_row(name: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("net-row");
    let word = label(name, &["row-title"]);
    word.set_hexpand(true);
    row.append(&word);
    control.as_ref().set_valign(gtk::Align::Center);
    row.append(control);
    row
}

fn dropdown(items: &[String], selected: usize) -> gtk::DropDown {
    let strs: Vec<&str> = items.iter().map(String::as_str).collect();
    let d = gtk::DropDown::from_strings(&strs);
    d.add_css_class("choice");
    d.set_selected(selected.min(items.len().saturating_sub(1)) as u32);
    d
}

fn display_page() -> gtk::ScrolledWindow {
    let (scroll, body, status, _right) = page("display");
    let screens = gtk::Box::new(gtk::Orientation::Vertical, 22);
    let light = gtk::ListBox::new();
    light.add_css_class("list-card");
    light.set_selection_mode(gtk::SelectionMode::None);
    let error = label("", &["error"]);
    error.set_visible(false);
    let arrange = gtk::Button::with_label("arrange screens");
    arrange.add_css_class("link");
    arrange.set_halign(gtk::Align::Start);
    arrange.connect_clicked(|_| {
        let _ = std::process::Command::new("wdisplays").spawn();
    });
    body.append(&screens);
    body.append(&caption("light"));
    body.append(&light);
    body.append(&error);
    body.append(&arrange);

    let load: Rc<dyn Fn()> = {
        let (screens, light, status, error) = (screens.clone(), light.clone(), status.clone(), error.clone());
        Rc::new(move || {
            let (screens, light, status, error) = (screens.clone(), light.clone(), status.clone(), error.clone());
            background(
                || (display::outputs(), display::brightness(), display::night_light()),
                move |(outs, bright, night)| {
                    while let Some(child) = screens.first_child() {
                        screens.remove(&child);
                    }
                    match outs.first() {
                        Some(o) => {
                            let mode =
                                o.current.and_then(|i| o.modes.get(i)).map(display::Mode::label).unwrap_or_default();
                            status.set_text(&format!("{} · {mode} · scale {:.0}%", o.title, o.scale * 100.0));
                        }
                        None => status.set_text("no screen found"),
                    }
                    for o in &outs {
                        screens.append(&screen_card(o, &error));
                    }
                    clear(&light);
                    if let Some(b) = bright {
                        light.append(&level_row(
                            "brightness",
                            (b * 100.0).round() as u32,
                            display::set_brightness,
                            None,
                        ));
                    }
                    let night_switch = gtk::Switch::new();
                    night_switch.set_active(night);
                    night_switch.connect_state_set(|_, on| {
                        display::set_night_light(on);
                        glib::Propagation::Proceed
                    });
                    light.append(&setting_row("night light: warmer colours after sunset", &night_switch));
                },
            );
        })
    };
    scroll.connect_map(move |_| load());
    scroll
}

fn screen_card(o: &display::Output, error: &gtk::Label) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    card.append(&caption(&o.title));
    let list = gtk::ListBox::new();
    list.add_css_class("list-card");
    list.set_selection_mode(gtk::SelectionMode::None);
    let mode_names: Vec<String> =
        o.modes.iter().map(|m| format!("{}{}", m.label(), if m.preferred { " · best" } else { "" })).collect();
    let modes = dropdown(&mode_names, o.current.unwrap_or(0));
    let scale_names: Vec<String> = display::SCALES.iter().map(|s| format!("{:.0}%", s * 100.0)).collect();
    let scale_now = display::SCALES.iter().position(|s| (s - o.scale).abs() < 0.01).unwrap_or(0);
    let scales = dropdown(&scale_names, scale_now);
    let rot_names: Vec<String> = display::ROTATIONS.iter().map(|r| r.0.to_string()).collect();
    let rot_now = display::ROTATIONS
        .iter()
        .position(|r| o.transform == r.1 || (o.transform == "normal" && r.1 == "normal"))
        .unwrap_or(0);
    let rotations = dropdown(&rot_names, rot_now);
    list.append(&setting_row("resolution", &modes));
    list.append(&setting_row("scale", &scales));
    list.append(&setting_row("rotation", &rotations));
    card.append(&list);
    let apply = {
        let (o, modes, scales, rotations, error) =
            (o.clone(), modes.clone(), scales.clone(), rotations.clone(), error.clone());
        Rc::new(move || {
            let Some(mode) = o.modes.get(modes.selected() as usize).cloned() else { return };
            let scale = display::SCALES[scales.selected() as usize];
            let transform = display::ROTATIONS[rotations.selected() as usize].1;
            let (o, error) = (o.clone(), error.clone());
            background(
                move || display::apply(&o, &mode, scale, transform),
                move |r| match r {
                    Ok(()) => error.set_visible(false),
                    Err(e) => {
                        error.set_text(&format!("couldn't change the screen: {e}"));
                        error.set_visible(true);
                    }
                },
            );
        })
    };
    for d in [&modes, &scales, &rotations] {
        let apply = apply.clone();
        d.connect_selected_notify(move |_| apply());
    }
    card
}

const WALLPAPERS: [(&str, &str); 4] = [("ink", "ink"), ("night", "night"), ("forest", "forest"), ("paper", "paper")];
const TEXT_SIZES: [(&str, f64); 4] = [("small", 0.9), ("default", 1.0), ("large", 1.15), ("larger", 1.3)];

fn tripppin_config() -> std::path::PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("tripppin")
}

/// Copies an image to ~/.config/tripppin/wallpaper and restarts swaybg with it.
fn set_wallpaper(from: &std::path::Path, name: &str) -> std::io::Result<()> {
    let dir = tripppin_config();
    std::fs::create_dir_all(&dir)?;
    std::fs::copy(from, dir.join("wallpaper"))?;
    std::fs::write(dir.join("wallpaper-name"), name)?;
    let _ = std::process::Command::new("pkill").args(["-x", "swaybg"]).status();
    let wall = dir.join("wallpaper");
    let _ = std::process::Command::new("niri")
        .args(["msg", "action", "spawn", "--", "swaybg", "-i", &wall.to_string_lossy(), "-m", "fill"])
        .status();
    Ok(())
}

fn text_scale() -> f64 {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "text-scaling-factor"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(1.0)
}

fn gsettings_set(key: &str, value: &str) {
    let _ = std::process::Command::new("gsettings").args(["set", "org.gnome.desktop.interface", key, value]).status();
}

fn appearance_page() -> gtk::ScrolledWindow {
    let (scroll, body, status, _right) = page("appearance");
    let current = std::fs::read_to_string(tripppin_config().join("wallpaper-name")).unwrap_or_else(|_| "ink".into());
    status.set_text(&format!("dark · {} wallpaper", current.trim()));
    body.append(&caption("wallpaper"));
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_max_children_per_line(4);
    grid.set_min_children_per_line(2);
    grid.set_column_spacing(12);
    grid.set_row_spacing(12);
    let tiles: Rc<RefCell<Vec<gtk::Button>>> = Rc::new(RefCell::new(Vec::new()));
    for (id, name) in WALLPAPERS {
        let path = std::path::PathBuf::from(format!("/usr/share/tripppin/wallpapers/{id}.png"));
        let tile = gtk::Button::new();
        tile.add_css_class("wall-tile");
        if current.trim() == id {
            tile.add_css_class("selected");
        }
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let pic = gtk::Picture::for_filename(&path);
        pic.set_content_fit(gtk::ContentFit::Cover);
        pic.set_size_request(184, 115);
        pic.add_css_class("wall-pic");
        inner.append(&pic);
        inner.append(&label(name, &["card-sub"]));
        tile.set_child(Some(&inner));
        let (tiles2, status) = (tiles.clone(), status.clone());
        tile.connect_clicked(move |b| {
            if set_wallpaper(&path, id).is_ok() {
                for t in tiles2.borrow().iter() {
                    t.remove_css_class("selected");
                }
                b.add_css_class("selected");
                status.set_text(&format!("dark · {id} wallpaper"));
            }
        });
        tiles.borrow_mut().push(tile.clone());
        grid.insert(&tile, -1);
    }
    body.append(&grid);
    let own = gtk::Button::with_label("choose your own…");
    own.add_css_class("link");
    own.add_css_class("accent");
    own.set_halign(gtk::Align::Start);
    {
        let (status, tiles) = (status.clone(), tiles.clone());
        own.connect_clicked(move |b| {
            let filter = gtk::FileFilter::new();
            filter.add_mime_type("image/*");
            filter.set_name(Some("images"));
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let dialog = gtk::FileDialog::builder().title("choose a wallpaper").filters(&filters).modal(true).build();
            let window = b.root().and_downcast::<gtk::Window>();
            let (status, tiles) = (status.clone(), tiles.clone());
            dialog.open(window.as_ref(), gio::Cancellable::NONE, move |r| {
                if let Ok(file) = r
                    && let Some(path) = file.path()
                    && set_wallpaper(&path, "your own").is_ok()
                {
                    for t in tiles.borrow().iter() {
                        t.remove_css_class("selected");
                    }
                    status.set_text("dark · your own wallpaper");
                }
            });
        });
    }
    body.append(&own);

    body.append(&caption("look"));
    let look = gtk::ListBox::new();
    look.add_css_class("list-card");
    look.set_selection_mode(gtk::SelectionMode::None);
    look.append(&setting_row("mode", &label("dark · light mode comes in a later update", &["row-sub"])));
    let sizes: Vec<String> = TEXT_SIZES.iter().map(|s| s.0.to_string()).collect();
    let now = text_scale();
    let size_now = TEXT_SIZES.iter().position(|s| (s.1 - now).abs() < 0.02).unwrap_or(1);
    let size = dropdown(&sizes, size_now);
    size.connect_selected_notify(|d| {
        gsettings_set("text-scaling-factor", &TEXT_SIZES[d.selected() as usize].1.to_string())
    });
    look.append(&setting_row("text size", &size));
    let motion = gtk::Switch::new();
    motion.set_active(display::reduce_motion());
    motion.connect_state_set(|_, on| {
        let _ = display::set_reduce_motion(on);
        gsettings_set("enable-animations", if on { "false" } else { "true" });
        glib::Propagation::Proceed
    });
    look.append(&setting_row("reduce motion", &motion));
    body.append(&look);
    scroll
}

fn minutes(m: u32) -> String {
    match m {
        0 => "never".to_string(),
        1 => "1 minute".to_string(),
        m => format!("{m} minutes"),
    }
}

fn power_page() -> gtk::ScrolledWindow {
    let (scroll, body, status, _right) = page("power & battery");
    let battery_caption = caption("battery");
    let battery = gtk::ListBox::new();
    battery.add_css_class("list-card");
    battery.set_selection_mode(gtk::SelectionMode::None);
    let modes_caption = caption("power mode");
    let modes = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    modes.set_homogeneous(true);
    let error = label("", &["error"]);
    error.set_visible(false);
    body.append(&battery_caption);
    body.append(&battery);
    body.append(&modes_caption);
    body.append(&modes);
    body.append(&caption("when you're away"));
    let idle_list = gtk::ListBox::new();
    idle_list.add_css_class("list-card");
    idle_list.set_selection_mode(gtk::SelectionMode::None);
    let idle = power::idle();
    let names: Vec<String> = power::IDLE_CHOICES.iter().map(|m| minutes(*m)).collect();
    let pick = |now: u32| power::IDLE_CHOICES.iter().position(|m| *m == now).unwrap_or(2);
    let lock = dropdown(&names, pick(idle.lock));
    let screen = dropdown(&names, pick(idle.screen_off));
    let sleep = dropdown(&names, pick(idle.sleep));
    idle_list.append(&setting_row("lock the screen after", &lock));
    idle_list.append(&setting_row("turn the screen off after", &screen));
    idle_list.append(&setting_row("sleep after", &sleep));
    let save = {
        let (lock, screen, sleep, error) = (lock.clone(), screen.clone(), sleep.clone(), error.clone());
        Rc::new(move || {
            let pick = |d: &gtk::DropDown| power::IDLE_CHOICES[d.selected() as usize];
            let idle = power::Idle { lock: pick(&lock), screen_off: pick(&screen), sleep: pick(&sleep) };
            if let Err(e) = power::set_idle(&idle) {
                error.set_text(&format!("couldn't save: {e}"));
                error.set_visible(true);
            }
        })
    };
    for d in [&lock, &screen, &sleep] {
        let save = save.clone();
        d.connect_selected_notify(move |_| save());
    }
    body.append(&idle_list);
    body.append(&error);

    let load = {
        let (battery, battery_caption, modes, modes_caption, status, error) = (
            battery.clone(),
            battery_caption.clone(),
            modes.clone(),
            modes_caption.clone(),
            status.clone(),
            error.clone(),
        );
        move || {
            let (battery, battery_caption, modes, modes_caption, status, error) = (
                battery.clone(),
                battery_caption.clone(),
                modes.clone(),
                modes_caption.clone(),
                status.clone(),
                error.clone(),
            );
            background(
                || (power::battery(), power::modes()),
                move |(bat, mode)| {
                    clear(&battery);
                    battery.set_visible(bat.is_some());
                    battery_caption.set_visible(bat.is_some());
                    match &bat {
                        Some(b) => {
                            status.set_text(&power::summary(b));
                            battery.append(&setting_row("charge", &label(&format!("{}%", b.percent), &["row-value"])));
                            battery.append(&setting_row("state", &label(&b.state.replace('-', " "), &["row-value"])));
                            if let Some(h) = b.health {
                                battery.append(&setting_row("health", &label(&format!("{h}% of new"), &["row-value"])));
                            }
                        }
                        None => status.set_text("no battery · on mains power"),
                    }
                    while let Some(child) = modes.first_child() {
                        modes.remove(&child);
                    }
                    modes.set_visible(mode.is_some());
                    modes_caption.set_visible(mode.is_some());
                    let Some((now, offered)) = mode else { return };
                    let buttons: Rc<RefCell<Vec<gtk::Button>>> = Rc::new(RefCell::new(Vec::new()));
                    for (id, word) in power::MODES {
                        if !offered.iter().any(|o| o == id) {
                            continue;
                        }
                        let b = gtk::Button::with_label(word);
                        b.add_css_class("segment");
                        if now == id {
                            b.add_css_class("selected");
                        }
                        let (buttons2, error) = (buttons.clone(), error.clone());
                        b.connect_clicked(move |b| match power::set_mode(id) {
                            Ok(()) => {
                                for x in buttons2.borrow().iter() {
                                    x.remove_css_class("selected");
                                }
                                b.add_css_class("selected");
                            }
                            Err(e) => {
                                error.set_text(&format!("couldn't change the power mode: {e}"));
                                error.set_visible(true);
                            }
                        });
                        buttons.borrow_mut().push(b.clone());
                        modes.append(&b);
                    }
                },
            );
        }
    };
    scroll.connect_map(move |_| load());
    scroll
}

fn llm_running() -> bool {
    std::process::Command::new("systemctl")
        .args(["is-active", "--quiet", "tripppin-llm.service"])
        .status()
        .is_ok_and(|s| s.success())
}

fn restart_llm() -> Result<(), String> {
    let out = std::process::Command::new("systemctl")
        .args(["restart", "tripppin-llm.service"])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_lowercase()) }
}

/// Filled in once the page's loader exists, so its own buttons can call it again.
type LoadSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// What the AI page shows: the models, which one is downloaded, recommended and in use.
struct AiState {
    tiers: Vec<hwdetect::Tier>,
    installed: Vec<bool>,
    recommended: Option<String>,
    active: Option<String>,
    running: bool,
    endpoint: Option<trip::model::Endpoint>,
}

fn ai_state() -> AiState {
    let catalog = hwdetect::Catalog::builtin().ok();
    let tiers = catalog.as_ref().map(|c| c.tiers.clone()).unwrap_or_default();
    let installed = tiers.iter().map(trip::model::is_installed).collect();
    let active =
        catalog.as_ref().and_then(|c| trip::model::active_tier(c, &trip::model::models_dir())).map(|t| t.id.clone());
    AiState {
        tiers,
        installed,
        recommended: trip::model::recommended().ok().map(|t| t.id),
        active,
        running: llm_running(),
        endpoint: trip::model::endpoint(),
    }
}

fn ai_page() -> gtk::ScrolledWindow {
    let (scroll, body, status, _right) = page("ai");
    body.append(&caption("where it runs"));
    let modes = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    modes.set_homogeneous(true);
    let here = gtk::Button::with_label("this computer");
    let there = gtk::Button::with_label("another server");
    for b in [&here, &there] {
        b.add_css_class("segment");
        modes.append(b);
    }
    body.append(&modes);
    let error = label("", &["error"]);
    error.set_visible(false);

    // This computer: the models and the service that runs them.
    let local = gtk::Box::new(gtk::Orientation::Vertical, 22);
    local.append(&caption("models"));
    let models = gtk::ListBox::new();
    models.add_css_class("list-card");
    models.set_selection_mode(gtk::SelectionMode::None);
    local.append(&models);
    let service = gtk::ListBox::new();
    service.add_css_class("list-card");
    service.set_selection_mode(gtk::SelectionMode::None);
    local.append(&service);

    // Another server: any OpenAI-style chat API.
    let remote = gtk::Box::new(gtk::Orientation::Vertical, 12);
    remote.append(&caption("another server"));
    remote.append(&label(
        "a cloud provider with a key, or your own llama.cpp or ollama machine. anything that speaks the openai chat api.",
        &["row-sub"],
    ));
    let url = gtk::Entry::builder().placeholder_text("address, like https://api.example.com/v1").build();
    let model = gtk::Entry::builder().placeholder_text("model name").build();
    let key = gtk::PasswordEntry::builder()
        .placeholder_text("key (leave empty for your own server)")
        .show_peek_icon(true)
        .build();
    url.add_css_class("field");
    model.add_css_class("field");
    key.add_css_class("field");
    let save = gtk::Button::with_label("test and use");
    save.add_css_class("pill");
    save.set_halign(gtk::Align::Start);
    remote.append(&url);
    remote.append(&model);
    remote.append(&key);
    remote.append(&save);
    body.append(&local);
    body.append(&remote);
    body.append(&error);

    body.append(&caption("what it may do alone"));
    let alone = gtk::ListBox::new();
    alone.add_css_class("list-card");
    alone.set_selection_mode(gtk::SelectionMode::None);
    alone.append(&setting_row("run commands and change files", &label("yes, and it checks its work", &["row-sub"])));
    alone.append(&setting_row("ask before risky steps", &label("comes in a later update", &["row-sub"])));
    body.append(&alone);

    let show_remote = {
        let (here, there, local, remote) = (here.clone(), there.clone(), local.clone(), remote.clone());
        Rc::new(move |on: bool| {
            local.set_visible(!on);
            remote.set_visible(on);
            if on {
                here.remove_css_class("selected");
                there.add_css_class("selected");
            } else {
                there.remove_css_class("selected");
                here.add_css_class("selected");
            }
        })
    };

    let load: LoadSlot = Rc::new(RefCell::new(None));
    let reload = {
        let load = load.clone();
        move || {
            if let Some(f) = load.borrow().clone() {
                f();
            }
        }
    };
    {
        let (models, service, status, error, show_remote, url, model, key) = (
            models.clone(),
            service.clone(),
            status.clone(),
            error.clone(),
            show_remote.clone(),
            url.clone(),
            model.clone(),
            key.clone(),
        );
        let reload = reload.clone();
        *load.borrow_mut() = Some(Rc::new(move || {
            let (models, service, status, error, show_remote, url, model, key) = (
                models.clone(),
                service.clone(),
                status.clone(),
                error.clone(),
                show_remote.clone(),
                url.clone(),
                model.clone(),
                key.clone(),
            );
            let reload = reload.clone();
            background(ai_state, move |st| {
                if let Some(e) = &st.endpoint {
                    let host = e.url.split("://").nth(1).unwrap_or(&e.url).split('/').next().unwrap_or("").to_string();
                    status.set_text(&format!("{} · on {host}", e.model.to_lowercase()));
                    url.set_text(&e.url);
                    model.set_text(&e.model);
                    key.set_text(&e.key);
                } else {
                    let name = st
                        .active
                        .as_ref()
                        .and_then(|id| st.tiers.iter().find(|t| &t.id == id))
                        .map(|t| t.model.to_lowercase());
                    status.set_text(&match (name, st.running) {
                        (Some(n), true) => format!("{n} · on this computer · ready"),
                        (Some(n), false) => format!("{n} · on this computer · stopped"),
                        (None, _) => "no model downloaded yet".to_string(),
                    });
                }
                show_remote(st.endpoint.is_some());
                clear(&models);
                for (i, t) in st.tiers.iter().enumerate() {
                    models.append(&model_row(t, &st, st.installed[i], &error, reload.clone()));
                }
                clear(&service);
                let restart = gtk::Button::with_label(if st.running { "restart" } else { "start" });
                restart.add_css_class("link");
                restart.add_css_class("accent");
                let (error, reload2) = (error.clone(), reload.clone());
                restart.connect_clicked(move |b| {
                    b.set_sensitive(false);
                    let (error, reload) = (error.clone(), reload2.clone());
                    background(restart_llm, move |r| {
                        if let Err(e) = r {
                            error.set_text(&format!("couldn't start the model: {e}"));
                            error.set_visible(true);
                        }
                        reload();
                    });
                });
                let row = setting_row(
                    if st.running { "model service · running" } else { "model service · stopped" },
                    &restart,
                );
                service.append(&row);
            });
        }));
    }
    {
        let (show_remote, error) = (show_remote.clone(), error.clone());
        let reload = reload.clone();
        here.connect_clicked(move |_| match trip::model::set_endpoint(None) {
            Ok(()) => {
                show_remote(false);
                reload();
            }
            Err(e) => {
                error.set_text(&format!("couldn't switch back: {e:#}"));
                error.set_visible(true);
            }
        });
    }
    {
        let show_remote = show_remote.clone();
        there.connect_clicked(move |_| show_remote(true));
    }
    {
        let (url, model, key, error) = (url.clone(), model.clone(), key.clone(), error.clone());
        let reload = reload.clone();
        save.connect_clicked(move |b| {
            let e = trip::model::Endpoint {
                url: url.text().trim().trim_end_matches('/').to_string(),
                model: model.text().trim().to_string(),
                key: key.text().trim().to_string(),
            };
            if e.url.is_empty() || e.model.is_empty() {
                error.set_text("fill in the address and the model name");
                error.set_visible(true);
                return;
            }
            b.set_sensitive(false);
            b.set_label("testing…");
            let (b, error, reload) = (b.clone(), error.clone(), reload.clone());
            background(
                move || {
                    trip::model::test_endpoint(&e)
                        .and_then(|()| trip::model::set_endpoint(Some(&e)))
                        .map_err(|e| format!("{e:#}"))
                },
                move |r| {
                    b.set_sensitive(true);
                    b.set_label("test and use");
                    match r {
                        Ok(()) => {
                            error.set_visible(false);
                            reload();
                        }
                        Err(e) => {
                            error.set_text(&e.to_lowercase());
                            error.set_visible(true);
                        }
                    }
                },
            );
        });
    }
    scroll.connect_map(move |_| reload());
    scroll
}

enum Pull {
    Bytes(u64, u64),
    Done(Result<(), String>),
}

fn model_row(
    t: &hwdetect::Tier,
    st: &AiState,
    installed: bool,
    error: &gtk::Label,
    reload: impl Fn() + Clone + 'static,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("net-row");
    let in_use = st.endpoint.is_none() && st.active.as_deref() == Some(t.id.as_str());
    if in_use {
        row.add_css_class("connected");
    }
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&label(&t.model.to_lowercase(), &[if in_use { "row-title-strong" } else { "row-title" }]));
    let mut words = vec![format!("{:.1} gb", t.download_gb())];
    if st.recommended.as_deref() == Some(t.id.as_str()) {
        words.push("best for this computer".into());
    }
    if in_use {
        words.push("in use".into());
    } else if installed {
        words.push("downloaded".into());
    }
    let sub = label(&words.join(" · "), if in_use { &["row-sub", "on"][..] } else { &["row-sub"][..] });
    text.append(&sub);
    row.append(&text);
    if in_use {
        return row;
    }
    let action = gtk::Button::with_label(if installed { "use" } else { "download" });
    action.add_css_class("link");
    action.add_css_class("accent");
    let (tier, error) = (t.clone(), error.clone());
    action.connect_clicked(move |b| {
        b.set_sensitive(false);
        let (error, reload) = (error.clone(), reload.clone());
        if trip::model::is_installed(&tier) {
            let tier = tier.clone();
            background(
                move || trip::model::choose(&tier).map_err(|e| format!("{e:#}")).and_then(|()| restart_llm()),
                move |r| {
                    if let Err(e) = r {
                        error.set_text(&format!("couldn't switch models: {e}"));
                        error.set_visible(true);
                    }
                    reload();
                },
            );
            return;
        }
        let (tx, rx) = async_channel::unbounded::<Pull>();
        let tier = tier.clone();
        std::thread::spawn(move || {
            let progress = tx.clone();
            let r = trip::model::pull_tier(&tier, &mut |done, total, _| {
                let _ = progress.send_blocking(Pull::Bytes(done, total));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send_blocking(Pull::Done(r));
        });
        let (b, sub) = (b.clone(), sub.clone());
        glib::spawn_future_local(async move {
            while let Ok(p) = rx.recv().await {
                match p {
                    Pull::Bytes(done, total) => {
                        b.set_label(&format!("{:.0}%", 100.0 * done as f64 / total.max(1) as f64));
                        sub.set_text(&format!(
                            "downloading · {:.1} of {:.1} gb",
                            done as f64 / 1e9,
                            total as f64 / 1e9
                        ));
                    }
                    Pull::Done(r) => {
                        if let Err(e) = r {
                            error.set_text(&format!("the download stopped: {}", e.to_lowercase()));
                            error.set_visible(true);
                        }
                        reload();
                        break;
                    }
                }
            }
        });
    });
    row.append(&action);
    row
}

fn notifications_page() -> gtk::ScrolledWindow {
    let (scroll, body, status, right) = page("notifications");
    let dnd = gtk::Switch::new();
    right.append(&label("do not disturb", &["row-sub"]));
    right.append(&dnd);
    right.set_spacing(12);
    let error = label("", &["error"]);
    error.set_visible(false);

    body.append(&caption("quiet hours"));
    let quiet = gtk::ListBox::new();
    quiet.add_css_class("list-card");
    quiet.set_selection_mode(gtk::SelectionMode::None);
    let hours: Vec<String> = (0..24).map(|h| format!("{h:02}:00")).collect();
    let on = gtk::Switch::new();
    let from = dropdown(&hours, 22);
    let to = dropdown(&hours, 7);
    quiet.append(&setting_row("turn on do not disturb every day", &on));
    let from_row = setting_row("from", &from);
    let to_row = setting_row("until", &to);
    quiet.append(&from_row);
    quiet.append(&to_row);
    body.append(&quiet);

    body.append(&caption("muted apps"));
    body.append(&label("their notifications are dropped quietly.", &["row-sub"]));
    let muted = gtk::ListBox::new();
    muted.add_css_class("list-card");
    muted.set_selection_mode(gtk::SelectionMode::None);
    body.append(&muted);
    body.append(&error);

    let set_status = {
        let status = status.clone();
        move |dnd: bool, quiet: Option<&(String, String)>, muted: usize| {
            let mut words =
                vec![if dnd { "do not disturb is on".to_string() } else { "showing notifications".to_string() }];
            if let Some((a, b)) = quiet {
                words.push(format!("quiet {a}–{b}"));
            }
            if muted > 0 {
                words.push(format!("{muted} muted"));
            }
            status.set_text(&words.join(" · "));
        }
    };

    // Each change saves at once; `loading` stops the controls' own signals while filling them in.
    let loading = Rc::new(std::cell::Cell::new(true));
    {
        let loading = loading.clone();
        dnd.connect_state_set(move |_, on| {
            if !loading.get() {
                apps::set_dnd(on);
            }
            glib::Propagation::Proceed
        });
    }
    let save_quiet = {
        let (on, from, to, from_row, to_row, error, loading, hours) = (
            on.clone(),
            from.clone(),
            to.clone(),
            from_row.clone(),
            to_row.clone(),
            error.clone(),
            loading.clone(),
            hours.clone(),
        );
        Rc::new(move || {
            from_row.set_sensitive(on.is_active());
            to_row.set_sensitive(on.is_active());
            if loading.get() {
                return;
            }
            let range = on
                .is_active()
                .then(|| (hours[from.selected() as usize].clone(), hours[to.selected() as usize].clone()));
            let error = error.clone();
            background(
                move || {
                    apps::set_quiet_hours(range.as_ref().map(|(a, b)| (a.as_str(), b.as_str())))
                        .map_err(|e| e.to_string())
                },
                move |r| {
                    if let Err(e) = r {
                        error.set_text(&format!("couldn't save quiet hours: {e}"));
                        error.set_visible(true);
                    }
                },
            );
        })
    };
    {
        let save = save_quiet.clone();
        on.connect_active_notify(move |_| save());
    }
    for d in [&from, &to] {
        let save = save_quiet.clone();
        d.connect_selected_notify(move |_| save());
    }

    let fill_muted: LoadSlot = Rc::new(RefCell::new(None));
    {
        let (muted_list, error, slot) = (muted.clone(), error.clone(), fill_muted.clone());
        *fill_muted.borrow_mut() = Some(Rc::new(move || {
            let (muted_list, error, slot) = (muted_list.clone(), error.clone(), slot.clone());
            background(
                || (apps::muted(), apps::installed()),
                move |(names, installed)| {
                    clear(&muted_list);
                    let names = Rc::new(names);
                    let save = {
                        let (error, slot) = (error.clone(), slot.clone());
                        move |next: Vec<String>| {
                            let (error, slot) = (error.clone(), slot.clone());
                            background(
                                move || apps::set_muted(&next).map_err(|e| e.to_string()),
                                move |r| {
                                    if let Err(e) = r {
                                        error.set_text(&format!("couldn't save: {e}"));
                                        error.set_visible(true);
                                    }
                                    if let Some(f) = slot.borrow().clone() {
                                        f();
                                    }
                                },
                            );
                        }
                    };
                    for n in names.iter() {
                        let unmute = gtk::Button::with_label("unmute");
                        unmute.add_css_class("link");
                        unmute.add_css_class("accent");
                        let (n, names, save) = (n.clone(), names.clone(), save.clone());
                        let row = setting_row(&n.to_lowercase(), &unmute);
                        unmute.connect_clicked(move |b| {
                            b.set_sensitive(false);
                            save(names.iter().filter(|x| **x != n).cloned().collect())
                        });
                        muted_list.append(&row);
                    }
                    let choices: Vec<String> =
                        installed.iter().map(|a| a.name.clone()).filter(|n| !names.contains(n)).collect();
                    if choices.is_empty() {
                        return;
                    }
                    // Short labels, so a long app name can't widen the window.
                    let lower: Vec<String> = choices
                        .iter()
                        .map(|c| {
                            let c = c.to_lowercase();
                            if c.chars().count() > 26 {
                                format!("{}…", c.chars().take(25).collect::<String>())
                            } else {
                                c
                            }
                        })
                        .collect();
                    let pick = dropdown(&lower, 0);
                    pick.set_enable_search(true);
                    let mute = gtk::Button::with_label("mute");
                    mute.add_css_class("link");
                    mute.add_css_class("accent");
                    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 14);
                    controls.append(&pick);
                    controls.append(&mute);
                    {
                        let (pick, names) = (pick.clone(), names.clone());
                        mute.connect_clicked(move |_| {
                            let mut next: Vec<String> = names.to_vec();
                            next.push(choices[pick.selected() as usize].clone());
                            save(next);
                        });
                    }
                    muted_list.append(&setting_row("mute an app", &controls));
                },
            );
        }));
    }

    {
        let (dnd, on, from, to, loading, fill_muted) =
            (dnd.clone(), on.clone(), from.clone(), to.clone(), loading.clone(), fill_muted.clone());
        scroll.connect_map(move |_| {
            let (dnd, on, from, to, loading, set_status, save_quiet) = (
                dnd.clone(),
                on.clone(),
                from.clone(),
                to.clone(),
                loading.clone(),
                set_status.clone(),
                save_quiet.clone(),
            );
            loading.set(true);
            background(
                || (apps::dnd(), apps::quiet_hours(), apps::muted().len()),
                move |(is_dnd, quiet, muted)| {
                    dnd.set_active(is_dnd);
                    on.set_active(quiet.is_some());
                    if let Some((a, b)) = &quiet {
                        let hour = |t: &str| t.split(':').next().and_then(|h| h.parse::<u32>().ok()).unwrap_or(0);
                        from.set_selected(hour(a));
                        to.set_selected(hour(b));
                    }
                    set_status(is_dnd, quiet.as_ref(), muted);
                    save_quiet();
                    loading.set(false);
                },
            );
            if let Some(f) = fill_muted.borrow().clone() {
                f();
            }
        });
    }
    scroll
}

fn apps_page() -> gtk::ScrolledWindow {
    let (scroll, body, status, right) = page("apps");
    let update = gtk::Button::with_label("update all");
    update.add_css_class("pill");
    update.set_visible(false);
    right.append(&update);
    let error = label("", &["error"]);
    error.set_visible(false);
    body.append(&caption("open things with"));
    let defaults = gtk::ListBox::new();
    defaults.add_css_class("list-card");
    defaults.set_selection_mode(gtk::SelectionMode::None);
    body.append(&defaults);
    body.append(&caption("your apps"));
    let mine = gtk::ListBox::new();
    mine.add_css_class("list-card");
    mine.set_selection_mode(gtk::SelectionMode::None);
    body.append(&mine);
    body.append(&error);
    let more = gtk::Button::with_label("get more apps: ask tripppin (super + space)");
    more.add_css_class("link");
    more.add_css_class("accent");
    more.set_halign(gtk::Align::Start);
    more.connect_clicked(|_| {
        let _ = std::process::Command::new("tripppin-ui").args(["orb", "--toggle"]).spawn();
    });
    body.append(&more);
    body.append(&caption("part of tripppin"));
    let system = label("", &["row-sub"]);
    system.set_wrap(true);
    system.set_xalign(0.0);
    body.append(&system);

    let load: LoadSlot = Rc::new(RefCell::new(None));
    {
        let (slot, status, update, defaults, mine, system, error) = (
            load.clone(),
            status.clone(),
            update.clone(),
            defaults.clone(),
            mine.clone(),
            system.clone(),
            error.clone(),
        );
        *load.borrow_mut() = Some(Rc::new(move || {
            let (slot, status, update, defaults, mine, system, error) = (
                slot.clone(),
                status.clone(),
                update.clone(),
                defaults.clone(),
                mine.clone(),
                system.clone(),
                error.clone(),
            );
            background(
                || (apps::installed(), apps::flatpaks(), apps::updates()),
                move |(installed, flatpaks, updates)| {
                    let reload = {
                        let slot = slot.clone();
                        move || {
                            if let Some(f) = slot.borrow().clone() {
                                f();
                            }
                        }
                    };
                    status.set_text(&format!(
                        "{} apps · {}",
                        installed.len(),
                        match updates.len() {
                            0 => "up to date".to_string(),
                            1 => "1 update".to_string(),
                            n => format!("{n} updates"),
                        }
                    ));
                    update.set_visible(!updates.is_empty());

                    clear(&defaults);
                    for (what, mimes) in apps::DEFAULTS {
                        let can: Vec<&apps::App> =
                            installed.iter().filter(|a| a.mime.iter().any(|m| mimes.contains(&m.as_str()))).collect();
                        if can.is_empty() {
                            defaults.append(&setting_row(what, &label("no app for this yet", &["row-sub"])));
                            continue;
                        }
                        let current = apps::default_for(mimes);
                        let names: Vec<String> = can.iter().map(|a| a.name.to_lowercase()).collect();
                        let now = can.iter().position(|a| Some(&a.id) == current.as_ref()).unwrap_or(0);
                        let pick = dropdown(&names, now);
                        let ids: Vec<String> = can.iter().map(|a| a.id.clone()).collect();
                        let error = error.clone();
                        pick.connect_selected_notify(move |d| {
                            if let Err(e) = apps::set_default(mimes, &ids[d.selected() as usize]) {
                                error.set_text(&format!("couldn't save: {e}"));
                                error.set_visible(true);
                            }
                        });
                        defaults.append(&setting_row(what, &pick));
                    }

                    clear(&mine);
                    if flatpaks.is_empty() {
                        mine.append(&setting_row("nothing installed yet", &label("", &["row-sub"])));
                    }
                    for app in &flatpaks {
                        let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
                        row.add_css_class("net-row");
                        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
                        text.set_hexpand(true);
                        text.set_valign(gtk::Align::Center);
                        text.append(&label(&app.name.to_lowercase(), &["row-title"]));
                        let mut sub = format!("{} · {}", app.version, app.size);
                        if updates.contains(&app.id) {
                            sub.push_str(" · update waiting");
                        }
                        text.append(&label(&sub, &["row-sub"]));
                        row.append(&text);
                        let remove = gtk::Button::with_label("remove");
                        remove.add_css_class("link");
                        let (app, error, reload) = (app.clone(), error.clone(), reload.clone());
                        remove.connect_clicked(move |b| {
                            // A second click confirms.
                            if b.label().as_deref() != Some("sure? remove") {
                                b.set_label("sure? remove");
                                b.add_css_class("danger");
                                return;
                            }
                            b.set_sensitive(false);
                            b.set_label("removing…");
                            let (app, error, reload) = (app.clone(), error.clone(), reload.clone());
                            background(
                                move || apps::remove(&app),
                                move |r| {
                                    if let Err(e) = r {
                                        error.set_text(&format!("couldn't remove it: {e}"));
                                        error.set_visible(true);
                                    }
                                    reload();
                                },
                            );
                        });
                        row.append(&remove);
                        mine.append(&row);
                    }

                    let flat_ids: Vec<&str> = flatpaks.iter().map(|f| f.id.as_str()).collect();
                    let builtin: Vec<String> = installed
                        .iter()
                        .filter(|a| !a.flatpak && !flat_ids.contains(&a.id.as_str()))
                        .map(|a| a.name.to_lowercase())
                        .collect();
                    system.set_text(&format!("{}. these update with the system.", builtin.join(" · ")));
                },
            );
        }));
    }
    {
        let (slot, error) = (load.clone(), error.clone());
        update.connect_clicked(move |b| {
            b.set_sensitive(false);
            b.set_label("updating…");
            let (b, slot, error) = (b.clone(), slot.clone(), error.clone());
            background(apps::update_all, move |r| {
                b.set_sensitive(true);
                b.set_label("update all");
                if let Err(e) = r {
                    error.set_text(&format!("couldn't update: {e}"));
                    error.set_visible(true);
                }
                if let Some(f) = slot.borrow().clone() {
                    f();
                }
            });
        });
    }
    scroll.connect_map(move |_| {
        if let Some(f) = load.borrow().clone() {
            f();
        }
    });
    scroll
}
