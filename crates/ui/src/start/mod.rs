//! `tripppin-ui bridge` (also `start`): the bridge, the start screen. A full-screen layer over everything: named groups of
//! live tiles you size, colour and drag, and an all-apps list with a letter jump grid. Run it
//! again (the taskbar button, the Meta key) to close it; Esc closes it; typing searches.
//! `--window` opens an ordinary window instead of a layer, for screenshots and development.

mod live;
mod model;

use crate::apps::{self, App};
use crate::common::{self, icons};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use live::Live;
use model::{Layout, Size, Tile};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const UNIT: i32 = 72;
const GAP: i32 = 12;

fn px(cells: u32) -> i32 {
    cells as i32 * UNIT + (cells as i32 - 1) * GAP
}

fn lbl(text: &str, classes: &[&str]) -> gtk::Label {
    common::label(text, classes)
}

fn spawn(cmd: &str, args: &[&str]) {
    if let Ok(mut c) = std::process::Command::new(cmd).args(args).spawn() {
        std::thread::spawn(move || c.wait());
    }
}

pub fn run() -> glib::ExitCode {
    let window_mode = std::env::args().any(|a| a == "--window");
    let app = gtk::Application::builder()
        .application_id("in.trippp.Start")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let open: Rc<RefCell<Option<Rc<Ui>>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, _| {
        let existing = open.borrow().clone();
        match existing {
            // Already open: this press is the one that closes it.
            Some(ui) => ui.close(),
            None => *open.borrow_mut() = Some(Ui::open(app, window_mode)),
        }
        glib::ExitCode::SUCCESS
    });
    let args: Vec<String> = std::env::args().take(1).collect();
    app.run_with_args(&args)
}

type Updater = Box<dyn Fn(&Live)>;

struct Ui {
    app: gtk::Application,
    window: gtk::Window,
    stack: gtk::Stack,
    title: gtk::Label,
    search: gtk::Entry,
    scroller: gtk::ScrolledWindow,
    groups_box: gtk::Box,
    list_scroll: gtk::ScrolledWindow,
    list: gtk::Box,
    layout: RefCell<Layout>,
    apps: RefCell<Vec<App>>,
    live: RefCell<Live>,
    updaters: RefCell<Vec<Updater>>,
    animate_in: Cell<bool>,
    closing: Cell<bool>,
    save: async_channel::Sender<String>,
}

fn load_css() {
    common::load_css();
    let Some(display) = gdk::Display::default() else { return };
    let mut css = include_str!("start.css").to_string();
    for i in 0..16 {
        css.push_str(&format!(".st{i} {{ animation-delay: {}ms; }}\n", i * 22));
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css);
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2);
}

impl Ui {
    fn open(app: &gtk::Application, window_mode: bool) -> Rc<Ui> {
        load_css();
        let window = gtk::Window::builder().application(app).decorated(false).build();
        window.add_css_class("tripppin");
        window.add_css_class("start-win");
        if window_mode {
            window.set_default_size(1280, 800);
            window.maximize();
        } else {
            window.init_layer_shell();
            window.set_namespace(Some("tripppin-start"));
            window.set_layer(Layer::Overlay);
            for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                window.set_anchor(e, true);
            }
            window.set_exclusive_zone(-1);
            window.set_keyboard_mode(KeyboardMode::Exclusive);
        }

        let root = gtk::Overlay::new();
        root.add_css_class("start-root");
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(180);
        root.set_child(Some(&stack));

        // The header both pages share: the hub title, search, and who is logged in.
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        header.set_valign(gtk::Align::Start);
        header.set_margin_start(36);
        header.set_margin_end(30);
        header.set_margin_top(26);
        let title = lbl("Bridge", &["hub"]);
        title.set_valign(gtk::Align::Start);
        header.append(&title);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        header.append(&spacer);
        let search = gtk::Entry::new();
        search.add_css_class("search");
        search.set_placeholder_text(Some("Search apps"));
        search.set_valign(gtk::Align::Start);
        search.set_margin_top(4);
        search.set_visible(false);
        header.append(&search);
        let user = common::current_user();
        let who = lbl(&common::display_name(&user), &["who"]);
        who.set_valign(gtk::Align::Center);
        who.set_margin_top(4);
        let avatar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        avatar.add_css_class("avatar");
        avatar.add_css_class(&format!("tc{}", model::colour_of(&user)));
        avatar.set_valign(gtk::Align::Start);
        avatar.set_margin_top(4);
        // Name and avatar are one button: it opens Settings at the account and closes the bridge.
        let me = gtk::Button::new();
        me.add_css_class("plain");
        me.add_css_class("me");
        me.set_valign(gtk::Align::Start);
        me.set_tooltip_text(Some("your account"));
        let me_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        me_row.append(&who);
        me_row.append(&avatar);
        me.set_child(Some(&me_row));
        header.append(&me);
        root.add_overlay(&header);

        // Page one: the groups, sideways.
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
        scroller.set_margin_top(96);
        scroller.set_hexpand(true);
        scroller.set_vexpand(true);
        let groups_box = gtk::Box::new(gtk::Orientation::Horizontal, 36);
        groups_box.set_margin_start(36);
        groups_box.set_margin_end(36);
        groups_box.set_valign(gtk::Align::Start);
        scroller.set_child(Some(&groups_box));
        let start_page = gtk::Overlay::new();
        start_page.set_child(Some(&scroller));
        let all = gtk::Button::with_label("All apps \u{203a}");
        all.add_css_class("plain");
        all.add_css_class("link");
        all.set_halign(gtk::Align::Start);
        all.set_valign(gtk::Align::End);
        all.set_margin_start(36);
        all.set_margin_bottom(30);
        start_page.add_overlay(&all);
        stack.add_named(&start_page, Some("start"));

        // Page two: all apps, a vertical list, and the jump grid over the right side.
        let list_scroll = gtk::ScrolledWindow::new();
        list_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        list_scroll.set_margin_top(96);
        list_scroll.set_margin_bottom(64);
        list_scroll.set_hexpand(true);
        list_scroll.set_vexpand(true);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 12);
        list.set_hexpand(true);
        list.set_margin_start(36);
        list.set_margin_end(36);
        list.set_margin_bottom(8);
        list_scroll.set_child(Some(&list));
        let apps_page = gtk::Overlay::new();
        apps_page.set_child(Some(&list_scroll));
        let back = gtk::Button::with_label("\u{2039} Bridge");
        back.add_css_class("plain");
        back.add_css_class("link");
        back.set_halign(gtk::Align::Start);
        back.set_valign(gtk::Align::End);
        back.set_margin_start(36);
        back.set_margin_bottom(30);
        apps_page.add_overlay(&back);
        stack.add_named(&apps_page, Some("apps"));
        window.set_child(Some(&root));

        let (save, save_rx) = async_channel::unbounded::<String>();
        std::thread::spawn(move || {
            while let Ok(json) = save_rx.recv_blocking() {
                model::save(&json);
            }
        });

        let ui = Rc::new(Ui {
            app: app.clone(),
            window: window.clone(),
            stack,
            title,
            search,
            scroller,
            groups_box,
            list_scroll,
            list,
            layout: RefCell::new(Layout::default()),
            apps: RefCell::new(Vec::new()),
            live: RefCell::new(Live::default()),
            updaters: RefCell::new(Vec::new()),
            animate_in: Cell::new(true),
            closing: Cell::new(false),
            save,
        });
        ui.wire(&all, &back, &me);
        window.present();
        common::snapshot_hook(&window);

        // Reading the .desktop files and the saved layout happens off the UI thread.
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let installed = apps::installed();
            let saved = model::load();
            let _ = tx.send_blocking((installed, saved));
        });
        let u = Rc::clone(&ui);
        glib::spawn_future_local(async move {
            if let Ok((installed, saved)) = rx.recv().await {
                let fresh = saved.is_none();
                let layout = saved.unwrap_or_else(|| model::default_layout(&installed));
                *u.apps.borrow_mut() = installed;
                *u.layout.borrow_mut() = layout;
                if fresh {
                    u.persist();
                }
                u.rebuild_groups();
                u.rebuild_list();
            }
        });
        let u = Rc::clone(&ui);
        let live_rx = live::watch();
        glib::spawn_future_local(async move {
            while let Ok(l) = live_rx.recv().await {
                *u.live.borrow_mut() = l;
                u.push_live();
            }
        });
        ui
    }

    fn wire(self: &Rc<Self>, all: &gtk::Button, back: &gtk::Button, me: &gtk::Button) {
        let u = Rc::clone(self);
        all.connect_clicked(move |_| u.show_page("apps"));
        let u = Rc::clone(self);
        back.connect_clicked(move |_| u.show_page("start"));
        let u = Rc::clone(self);
        me.connect_clicked(move |_| {
            spawn("tripppin-ui", &["settings", "account"]);
            u.close();
        });

        // A click on empty space (not a tile, button, list row or the search box) closes the bridge.
        let empty = gtk::GestureClick::new();
        empty.set_propagation_phase(gtk::PropagationPhase::Bubble);
        let u = Rc::clone(self);
        empty.connect_released(move |g, _, x, y| {
            let Some(root) = u.window.child() else { return };
            let mut w = root.pick(x, y, gtk::PickFlags::DEFAULT);
            while let Some(cur) = w {
                if cur.is::<gtk::Button>()
                    || cur.is::<gtk::Editable>()
                    || cur.has_css_class("tile")
                    || cur.has_css_class("cat")
                    || cur.has_css_class("row")
                    || cur.has_css_class("groupname")
                    || cur.has_css_class("hub")
                {
                    return;
                }
                w = cur.parent();
            }
            g.set_state(gtk::EventSequenceState::Claimed);
            u.close();
        });
        self.window.child().unwrap().add_controller(empty);

        let u = Rc::clone(self);
        self.search.connect_changed(move |s| {
            if !s.text().is_empty() {
                u.show_page("apps");
            }
            u.rebuild_list();
        });
        let u = Rc::clone(self);
        self.search.connect_activate(move |s| {
            let q = s.text().to_lowercase();
            let first = u.matches(&q).into_iter().next();
            if let Some(app) = first {
                u.launch(&app.id);
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let u = Rc::clone(self);
        keys.connect_key_pressed(move |_, key, _, state| u.key(key, state));
        self.window.add_controller(keys);

        // The wheel moves the groups sideways; with nothing to scroll to, down opens all apps.
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
        let u = Rc::clone(self);
        wheel.connect_scroll(move |_, _, dy| {
            let adj = u.scroller.hadjustment();
            if adj.upper() > adj.page_size() + 1.0 {
                adj.set_value((adj.value() + dy * 70.0).clamp(adj.lower(), adj.upper() - adj.page_size()));
                glib::Propagation::Stop
            } else if dy > 0.0 {
                u.show_page("apps");
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.scroller.add_controller(wheel);
    }

    fn page(&self) -> String {
        self.stack.visible_child_name().map(|n| n.to_string()).unwrap_or_default()
    }

    fn show_page(&self, name: &str) {
        self.stack.set_visible_child_name(name);
        self.title.set_text(if name == "start" { "Bridge" } else { "All apps" });
        self.search.set_visible(name == "apps");
        if name == "apps" {
            self.list_scroll.vadjustment().set_value(0.0);
        }
        if name == "start" {
            self.search.set_text("");
            GtkWindowExt::set_focus(&self.window, None::<&gtk::Widget>);
        }
    }

    fn key(self: &Rc<Self>, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        if key == gdk::Key::Escape {
            if !self.search.text().is_empty() {
                self.search.set_text("");
            } else {
                self.close();
            }
            return glib::Propagation::Stop;
        }
        let editing = GtkWindowExt::focus(&self.window).is_some_and(|f| f.downcast_ref::<gtk::Editable>().is_some());
        if editing {
            return glib::Propagation::Proceed;
        }
        let page = self.page();
        match key {
            gdk::Key::Down | gdk::Key::Page_Down if page == "start" => {
                self.show_page("apps");
                return glib::Propagation::Stop;
            }
            gdk::Key::Left | gdk::Key::Home if page == "apps" => {
                self.show_page("start");
                return glib::Propagation::Stop;
            }
            _ => {}
        }
        let held = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK;
        if let Some(c) = key.to_unicode().filter(|c| !c.is_control() && !c.is_whitespace())
            && !state.intersects(held)
        {
            self.show_page("apps");
            self.search.grab_focus();
            self.search.set_text(&c.to_string());
            self.search.set_position(-1);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    }

    fn close(self: &Rc<Self>) {
        if self.closing.replace(true) {
            return;
        }
        if let Some(root) = self.window.child() {
            root.add_css_class("closing");
        }
        let u = Rc::clone(self);
        glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
            u.window.close();
            u.app.quit();
        });
    }

    fn launch(self: &Rc<Self>, id: &str) {
        spawn("gtk4-launch", &[id]);
        self.close();
    }

    fn persist(&self) {
        let _ = self.save.try_send(model::to_json(&self.layout.borrow()));
    }

    /// After an edit: save, and redraw the groups once the current event is done with.
    fn changed(self: &Rc<Self>) {
        self.persist();
        let u = Rc::clone(self);
        glib::idle_add_local_once(move || u.rebuild_groups());
    }

    fn push_live(&self) {
        let live = self.live.borrow().clone();
        for f in self.updaters.borrow().iter() {
            f(&live);
        }
    }

    // ---- icons -----------------------------------------------------------------------------

    /// The app's own icon through the icon theme. GtkImage looks it up again at the screen's
    /// scale, so scalable icons stay sharp and rasters use the largest size that fits.
    fn icon_widget(&self, icon: &str, size: i32, name: &str) -> gtk::Widget {
        let image = if icon.starts_with('/') && std::path::Path::new(icon).exists() {
            Some(gtk::Image::from_file(icon))
        } else {
            let theme = gdk::Display::default().map(|d| gtk::IconTheme::for_display(&d));
            theme.filter(|t| !icon.is_empty() && t.has_icon(icon)).map(|_| gtk::Image::from_icon_name(icon))
        };
        match image {
            Some(img) => {
                img.set_pixel_size(size);
                img.upcast()
            }
            None => {
                let l = lbl(
                    &name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(),
                    &["letter-fallback"],
                );
                l.set_xalign(0.5);
                l.upcast()
            }
        }
    }

    // ---- the start page --------------------------------------------------------------------

    fn rebuild_groups(self: &Rc<Self>) {
        while let Some(c) = self.groups_box.first_child() {
            self.groups_box.remove(&c);
        }
        self.updaters.borrow_mut().clear();
        let layout = self.layout.borrow().clone();
        let animate = self.animate_in.replace(false);
        let mut n = 0usize;
        for (gi, group) in layout.groups.iter().enumerate() {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
            col.set_valign(gtk::Align::Start);
            let name = gtk::Entry::new();
            name.add_css_class("groupname");
            name.set_text(&group.name);
            name.set_halign(gtk::Align::Start);
            name.set_width_chars(group.name.chars().count().max(6) as i32);
            name.set_max_width_chars(24);
            let u = Rc::clone(self);
            name.connect_changed(move |e| {
                u.layout.borrow_mut().rename_group(gi, &e.text());
                u.persist();
            });
            col.append(&name);

            let tiles: Vec<Tile> = group
                .tiles
                .iter()
                .filter(|t| t.app_id().is_none_or(|id| self.apps.borrow().iter().any(|a| a.id == id)))
                .cloned()
                .collect();
            let cells: Vec<(u32, u32)> = tiles.iter().map(Tile::cells).collect();
            let (at, rows) = model::place(&cells);
            let fixed = gtk::Fixed::new();
            fixed.set_size_request(px(model::COLUMNS), px(rows.max(1)));
            for (t, (c, r)) in tiles.iter().zip(at) {
                let w = self.tile_widget(t);
                if animate {
                    w.add_css_class("tile-in");
                    w.add_css_class(&format!("st{}", n.min(15)));
                }
                n += 1;
                fixed.put(&w, f64::from(c as i32 * (UNIT + GAP)), f64::from(r as i32 * (UNIT + GAP)));
            }
            // Dropping on the group's own space sends the tile to its end.
            let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
            let u = Rc::clone(self);
            drop.connect_drop(move |_, value, _, _| {
                if let Ok(key) = value.get::<String>() {
                    u.layout.borrow_mut().move_to_group(&key, gi);
                    u.changed();
                }
                true
            });
            fixed.add_controller(drop);
            col.append(&fixed);
            self.groups_box.append(&col);
        }
        self.push_live();
    }

    fn tile_widget(self: &Rc<Self>, t: &Tile) -> gtk::Widget {
        let (cols, rows) = t.cells();
        let (w, h) = (px(cols), px(rows));
        let ov = gtk::Overlay::new();
        ov.add_css_class("tile");
        ov.set_overflow(gtk::Overflow::Hidden);
        ov.set_size_request(w, h);
        ov.set_halign(gtk::Align::Start);
        ov.set_valign(gtk::Align::Start);
        if t.key == "playing" && t.colour.is_none() {
            ov.add_css_class("tc-play");
        } else {
            ov.add_css_class(&format!("tc{}", t.colour_index()));
        }
        match t.key.as_str() {
            "calendar" => self.fill_calendar(&ov, t, w),
            "playing" => self.fill_playing(&ov, t, w, h),
            "ask" => {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                row.set_valign(gtk::Align::Center);
                row.set_margin_start(10);
                row.append(&common::icon(icons::SPARKLES, &["ask"]));
                row.append(&lbl("Ask Tripppin anything", &["ask"]));
                ov.set_child(Some(&row));
            }
            _ => self.fill_app(&ov, t),
        }
        self.tile_gestures(&ov, t);
        ov.upcast()
    }

    fn fill_app(&self, ov: &gtk::Overlay, t: &Tile) {
        let apps = self.apps.borrow();
        let Some(app) = t.app_id().and_then(|id| apps.iter().find(|a| a.id == id)) else { return };
        let size = match t.size {
            Size::Small => 30,
            Size::Large => 64,
            _ => 44,
        };
        let icon = self.icon_widget(&app.icon, size, &app.name);
        let chip = gtk::Box::new(gtk::Orientation::Vertical, 0);
        chip.add_css_class("icon-chip");
        chip.set_halign(gtk::Align::Center);
        chip.set_valign(gtk::Align::Center);
        chip.append(&icon);
        ov.set_child(Some(&chip));
        if t.size != Size::Small {
            let nm = lbl(&app.name, &["nm"]);
            nm.set_halign(gtk::Align::Start);
            nm.set_valign(gtk::Align::End);
            nm.set_ellipsize(gtk::pango::EllipsizeMode::End);
            ov.add_overlay(&nm);
        }
    }

    fn fill_calendar(&self, ov: &gtk::Overlay, t: &Tile, w: i32) {
        let small = t.size == Size::Small;
        let day = glib::DateTime::now_local()
            .ok()
            .and_then(|d| d.format("%-d").ok())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let big = lbl(&day, &["big"]);
        big.set_halign(if small { gtk::Align::Center } else { gtk::Align::End });
        big.set_valign(if small { gtk::Align::Center } else { gtk::Align::End });
        if small {
            big.add_css_class("small");
        }
        ov.add_overlay(&big);
        if small {
            return;
        }
        let nm = lbl("Calendar", &["nm"]);
        nm.set_halign(gtk::Align::Start);
        nm.set_valign(gtk::Align::End);
        ov.add_overlay(&nm);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text.set_halign(gtk::Align::Start);
        text.set_valign(gtk::Align::Start);
        text.set_margin_start(10);
        text.set_margin_top(10);
        let (when, ttl, time) = (lbl("", &["when"]), lbl("", &["ttl"]), lbl("", &["sub"]));
        ttl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        ttl.set_max_width_chars(w / 11);
        ttl.set_width_chars(1);
        for l in [&when, &ttl, &time] {
            text.append(l);
        }
        ov.add_overlay(&text);
        let update = move |live: &Live| {
            text.set_visible(live.event.is_some());
            if let Some(e) = &live.event {
                when.set_text(&e.when);
                ttl.set_text(&e.title);
                time.set_text(&e.time);
            }
        };
        update(&self.live.borrow());
        self.updaters.borrow_mut().push(Box::new(update));
    }

    fn fill_playing(&self, ov: &gtk::Overlay, t: &Tile, w: i32, h: i32) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.set_valign(gtk::Align::Center);
        row.set_margin_start(10);
        row.set_margin_end(10);
        let art_size = h - 27;
        let art = gtk::Box::new(gtk::Orientation::Vertical, 0);
        art.add_css_class("art-ph");
        art.set_size_request(art_size, art_size);
        art.set_valign(gtk::Align::Center);
        art.set_overflow(gtk::Overflow::Hidden);
        let pic = gtk::Picture::new();
        pic.set_content_fit(gtk::ContentFit::Cover);
        pic.set_size_request(art_size, art_size);
        pic.set_visible(false);
        art.append(&pic);
        art.set_visible(t.size != Size::Medium);
        row.append(&art);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text.set_valign(gtk::Align::Center);
        let (head, ttl, artist) = (lbl("Now playing", &["when"]), lbl("", &["ttl"]), lbl("", &["sub", "mute"]));
        for l in [&ttl, &artist] {
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            l.set_width_chars(1);
            l.set_max_width_chars((w - art_size - 40) / 11);
        }
        for l in [&head, &ttl, &artist] {
            text.append(l);
        }
        row.append(&text);
        ov.set_child(Some(&row));
        let badge = gtk::Image::new();
        badge.set_pixel_size(22);
        badge.set_halign(gtk::Align::End);
        badge.set_valign(gtk::Align::End);
        badge.set_margin_end(10);
        badge.set_margin_bottom(10);
        badge.set_visible(false);
        ov.add_overlay(&badge);
        let update = move |live: &Live| match &live.playing {
            Some(p) => {
                head.set_text(if p.playing { "Now playing" } else { "Paused" });
                ttl.set_text(&p.title);
                ttl.remove_css_class("mute");
                artist.set_text(&p.artist);
                pic.set_filename(p.art.as_deref());
                pic.set_visible(p.art.is_some());
                let theme = gdk::Display::default().map(|d| gtk::IconTheme::for_display(&d));
                let known = theme.is_some_and(|t| !p.player.is_empty() && t.has_icon(&p.player));
                badge.set_visible(known);
                if known {
                    badge.set_icon_name(Some(&p.player));
                }
            }
            None => {
                head.set_text("");
                ttl.set_text("Music");
                ttl.remove_css_class("mute");
                artist.set_text("Nothing playing");
                pic.set_visible(false);
                badge.set_visible(false);
            }
        };
        update(&self.live.borrow());
        self.updaters.borrow_mut().push(Box::new(update));
    }

    fn activate(self: &Rc<Self>, key: &str) {
        match key {
            "calendar" => {
                spawn("tripppin-ui", &["calendar"]);
                self.close();
            }
            "ask" => {
                spawn("tripppin-ui", &["orb", "--toggle"]);
                self.close();
            }
            "playing" => spawn("playerctl", &["play-pause"]),
            _ => {
                if let Some(id) = key.strip_prefix("app:") {
                    self.launch(id);
                }
            }
        }
    }

    fn tile_gestures(self: &Rc<Self>, ov: &gtk::Overlay, t: &Tile) {
        let key = t.key.clone();
        let click = gtk::GestureClick::new();
        click.set_button(1);
        let (u, k) = (Rc::clone(self), key.clone());
        click.connect_released(move |_, _, _, _| u.activate(&k));
        ov.add_controller(click);

        let right = gtk::GestureClick::new();
        right.set_button(3);
        let (u, k, o) = (Rc::clone(self), key.clone(), ov.clone());
        right.connect_pressed(move |g, _, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            u.tile_menu(&o, &k);
        });
        ov.add_controller(right);

        let long = gtk::GestureLongPress::new();
        long.set_touch_only(true);
        let (u, k, o) = (Rc::clone(self), key.clone(), ov.clone());
        long.connect_pressed(move |g, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            u.tile_menu(&o, &k);
        });
        ov.add_controller(long);

        let src = gtk::DragSource::new();
        src.set_actions(gdk::DragAction::MOVE);
        let k = key.clone();
        src.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&k.to_value())));
        let o = ov.clone();
        src.connect_drag_begin(move |s, _| {
            o.add_css_class("dragging");
            s.set_icon(Some(&gtk::WidgetPaintable::new(Some(&o))), 20, 20);
        });
        let o = ov.clone();
        src.connect_drag_end(move |_, _, _| o.remove_css_class("dragging"));
        ov.add_controller(src);

        let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
        let o = ov.clone();
        drop.connect_enter(move |_, _, _| {
            o.add_css_class("drop");
            gdk::DragAction::MOVE
        });
        let o = ov.clone();
        drop.connect_leave(move |_| o.remove_css_class("drop"));
        let (u, k) = (Rc::clone(self), key);
        drop.connect_drop(move |_, value, _, _| {
            if let Ok(dragged) = value.get::<String>() {
                u.layout.borrow_mut().move_before(&dragged, &k);
                u.changed();
            }
            true
        });
        ov.add_controller(drop);
    }

    // ---- menus -----------------------------------------------------------------------------

    fn menu(&self, anchor: &impl IsA<gtk::Widget>) -> (gtk::Popover, gtk::Box) {
        let pop = gtk::Popover::new();
        pop.add_css_class("start-menu");
        pop.set_has_arrow(false);
        pop.set_position(gtk::PositionType::Bottom);
        pop.set_parent(anchor);
        let v = gtk::Box::new(gtk::Orientation::Vertical, 0);
        pop.set_child(Some(&v));
        pop.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        (pop, v)
    }

    fn item(self: &Rc<Self>, v: &gtk::Box, pop: &gtk::Popover, text: &str, on: bool, act: impl Fn(&Rc<Ui>) + 'static) {
        let b = gtk::Button::with_label(text);
        if let Some(l) = b.child().and_downcast::<gtk::Label>() {
            l.set_xalign(0.0);
        }
        if on {
            b.add_css_class("on");
        }
        let (u, p) = (Rc::clone(self), pop.clone());
        b.connect_clicked(move |_| {
            p.popdown();
            act(&u);
        });
        v.append(&b);
    }

    fn tile_menu(self: &Rc<Self>, anchor: &gtk::Overlay, key: &str) {
        let Some(tile) = self.layout.borrow().tile(key).cloned() else { return };
        let (pop, v) = self.menu(anchor);
        if !tile.sizes().is_empty() {
            v.append(&lbl("SIZE", &["cap"]));
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            for s in tile.sizes() {
                let (k, s) = (key.to_string(), *s);
                self.item(&row, &pop, s.label(), s == tile.size, move |u| {
                    u.layout.borrow_mut().resize(&k, s);
                    u.changed();
                });
            }
            v.append(&row);
        }
        v.append(&lbl("COLOUR", &["cap"]));
        let swatches = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        for i in 0..model::COLOURS {
            let b = gtk::Button::new();
            b.add_css_class("swatch");
            b.add_css_class(&format!("tc{i}"));
            if i == tile.colour_index() {
                b.add_css_class("on");
            }
            let (u, p, k) = (Rc::clone(self), pop.clone(), key.to_string());
            b.connect_clicked(move |_| {
                p.popdown();
                u.layout.borrow_mut().recolour(&k, Some(i));
                u.changed();
            });
            swatches.append(&b);
        }
        v.append(&swatches);
        let groups: Vec<String> = self.layout.borrow().groups.iter().map(|g| g.name.clone()).collect();
        let here = self.layout.borrow().find(key).map(|p| p.0);
        v.append(&lbl("MOVE TO GROUP", &["cap"]));
        for (gi, name) in groups.iter().enumerate().filter(|(gi, _)| Some(*gi) != here) {
            let k = key.to_string();
            self.item(&v, &pop, name, false, move |u| {
                u.layout.borrow_mut().move_to_group(&k, gi);
                u.changed();
            });
        }
        let k = key.to_string();
        self.item(&v, &pop, "new group", false, move |u| {
            let gi = u.layout.borrow_mut().new_group("new group");
            u.layout.borrow_mut().move_to_group(&k, gi);
            u.changed();
        });
        let k = key.to_string();
        self.item(&v, &pop, "Unpin from start", false, move |u| {
            u.layout.borrow_mut().unpin(&k);
            u.changed();
        });
        pop.popup();
    }

    fn app_menu(self: &Rc<Self>, anchor: &gtk::Button, app: &App) {
        let key = format!("app:{}", app.id);
        let (pop, v) = self.menu(anchor);
        if self.layout.borrow().is_pinned(&key) {
            self.item(&v, &pop, "Unpin from start", false, move |u| {
                u.layout.borrow_mut().unpin(&key);
                u.changed();
            });
        } else {
            let groups: Vec<String> = self.layout.borrow().groups.iter().map(|g| g.name.clone()).collect();
            for (gi, name) in groups.iter().enumerate() {
                let (key, id) = (key.clone(), app.id.clone());
                self.item(&v, &pop, &format!("Pin to {name}"), false, move |u| {
                    let _ = &key;
                    u.layout.borrow_mut().pin(Tile::app(&id, Size::Small), gi);
                    u.changed();
                });
            }
        }
        pop.popup();
    }

    // ---- all apps --------------------------------------------------------------------------

    fn matches(&self, q: &str) -> Vec<App> {
        let apps = self.apps.borrow();
        if q.is_empty() {
            return apps.clone();
        }
        let mut hits: Vec<App> = apps
            .iter()
            .filter(|a| a.name.to_lowercase().contains(q) || a.id.to_lowercase().contains(q))
            .cloned()
            .collect();
        hits.sort_by_key(|a| !a.name.to_lowercase().starts_with(q));
        hits
    }

    fn app_row(self: &Rc<Self>, app: &App) -> gtk::Button {
        let b = gtk::Button::new();
        b.add_css_class("plain");
        b.add_css_class("app-row");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let tile = gtk::CenterBox::new();
        tile.set_size_request(40, 40);
        tile.set_halign(gtk::Align::Start);
        tile.add_css_class("app-icon");
        tile.add_css_class(&format!("tc{}", model::colour_of(&format!("app:{}", app.id))));
        let icon = self.icon_widget(&app.icon, 24, &app.name);
        tile.set_center_widget(Some(&icon));
        row.append(&tile);
        let nm = lbl(&app.name, &["nm"]);
        nm.set_valign(gtk::Align::Center);
        nm.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&nm);
        b.set_child(Some(&row));
        self.wire_app(&b, app);
        b
    }

    /// Click launches; right-click or long-press offers to pin it.
    fn wire_app(self: &Rc<Self>, b: &gtk::Button, app: &App) {
        let (u, id) = (Rc::clone(self), app.id.clone());
        b.connect_clicked(move |_| u.launch(&id));
        let right = gtk::GestureClick::new();
        right.set_button(3);
        let (u, a, anchor) = (Rc::clone(self), app.clone(), b.clone());
        right.connect_pressed(move |g, _, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            u.app_menu(&anchor, &a);
        });
        b.add_controller(right);
        let long = gtk::GestureLongPress::new();
        long.set_touch_only(true);
        let (u, a, anchor) = (Rc::clone(self), app.clone(), b.clone());
        long.connect_pressed(move |g, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            u.app_menu(&anchor, &a);
        });
        b.add_controller(long);
    }

    /// One app inside a category tile: its icon on a graphite chip, its name under it.
    fn app_cell(self: &Rc<Self>, app: &App) -> gtk::Button {
        let b = gtk::Button::new();
        b.add_css_class("plain");
        b.add_css_class("app-cell");
        let col = gtk::Box::new(gtk::Orientation::Vertical, 5);
        col.set_size_request(62, -1);
        let chip = gtk::CenterBox::new();
        chip.add_css_class("app-chip-icon");
        chip.set_halign(gtk::Align::Center);
        chip.set_center_widget(Some(&self.icon_widget(&app.icon, 32, &app.name)));
        col.append(&chip);
        let nm = lbl(&app.name, &["nm"]);
        nm.set_xalign(0.5);
        nm.set_ellipsize(gtk::pango::EllipsizeMode::End);
        nm.set_max_width_chars(9);
        col.append(&nm);
        b.set_child(Some(&col));
        self.wire_app(&b, app);
        b
    }

    fn rebuild_list(self: &Rc<Self>) {
        while let Some(c) = self.list.first_child() {
            self.list.remove(&c);
        }
        let q = self.search.text().to_lowercase();
        let hits = self.matches(&q);
        if !q.is_empty() {
            if hits.is_empty() {
                self.list.append(&lbl("no apps match", &["empty"]));
            }
            for a in &hits {
                self.list.append(&self.app_row(a));
            }
        } else {
            // Categories are tiles of different sizes; the fullest grows biggest.
            let tiles = model::category_tiles(&hits);
            let spans: Vec<(u32, u32)> =
                tiles.iter().enumerate().map(|(i, t)| model::category_span(i, t.1.len())).collect();
            let (at, _) = model::place(&spans);
            let grid = gtk::Grid::new();
            grid.set_column_homogeneous(true);
            grid.set_row_spacing(GAP as u32);
            grid.set_column_spacing(GAP as u32);
            grid.set_valign(gtk::Align::Start);
            for (i, ((name, idxs), (c, r))) in tiles.iter().zip(at).enumerate() {
                let tile = gtk::Box::new(gtk::Orientation::Vertical, 8);
                tile.add_css_class("cat");
                if i == 0 {
                    tile.add_css_class("first");
                }
                let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let n = lbl(name, &["cat-name"]);
                n.set_hexpand(true);
                head.append(&n);
                head.append(&lbl(&idxs.len().to_string(), &["cat-count"]));
                tile.append(&head);
                let flow = gtk::FlowBox::new();
                flow.set_selection_mode(gtk::SelectionMode::None);
                flow.set_homogeneous(true);
                flow.set_max_children_per_line(if spans[i].0 == 2 { 8 } else { 4 });
                flow.set_column_spacing(4);
                flow.set_row_spacing(4);
                flow.set_valign(gtk::Align::Start);
                for &k in idxs {
                    flow.append(&self.app_cell(&hits[k]));
                }
                tile.append(&flow);
                grid.attach(&tile, c as i32, r as i32, spans[i].0 as i32, spans[i].1 as i32);
            }
            self.list.append(&grid);
        }
        let adj = self.list_scroll.vadjustment();
        adj.set_value(0.0);
        glib::idle_add_local_once(move || adj.set_value(0.0));
    }
}
