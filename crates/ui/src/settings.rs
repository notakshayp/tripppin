//! Settings: one window, a sidebar of sections, each a page in the Tripppin style.
//! `tripppin-ui settings [section]` opens it (on a section); a second call brings it forward.
//! Sections are added here as they're built; each talks to the system service behind it.

use crate::bt;
use crate::common::label;
use crate::nm;
use crate::snd;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

/// The sections, in sidebar order: (id, title).
const SECTIONS: [(&str, &str); 3] = [("wifi", "wi-fi"), ("bluetooth", "bluetooth"), ("sound", "sound")];

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
