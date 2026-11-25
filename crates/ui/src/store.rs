//! `tripppin-ui store`: the Store, in the Tiles design. A hub (featured, categories,
//! installed, updates) of colour tiles over Flathub's catalogue, an app page with a flat
//! install button and marching-dot progress, and search that runs as you type.
//!
//! Everything slow (parsing the catalogue, `flatpak`, downloading screenshots) runs on
//! threads and reports back through a channel, so the window never waits.

use async_channel::Sender;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use trip::store::{self, App, Catalog, Installed, Progress};

const GRID_COLUMNS: u32 = 6;

enum Msg {
    Catalog(Catalog),
    Installed(Vec<Installed>),
    Updates(Vec<String>),
    Shot(String, Option<PathBuf>),
    Perms(String, Result<Vec<String>, String>),
    Size(String, Option<String>),
    Progress(String, Progress),
    Done(String, Result<(), String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Install,
    Remove,
    Update,
}

impl Op {
    fn ing(self) -> &'static str {
        match self {
            Op::Install => "installing",
            Op::Remove => "removing",
            Op::Update => "updating",
        }
    }
}

/// What the install row shows for the app on the open page.
struct AppPage {
    id: String,
    actions: gtk::Box,
    error: gtk::Label,
    perms: gtk::Box,
    meta: gtk::Label,
    meta_base: String,
    /// The percentage the dots' label is easing towards.
    target: Rc<Cell<f64>>,
}

struct Ui {
    window: gtk::ApplicationWindow,
    root: gtk::Stack,
    home: gtk::Stack,
    pivots: Vec<(gtk::Button, &'static str, gtk::Label)>,
    search: gtk::Entry,
    colours: RefCell<(HashSet<String>, gtk::CssProvider)>,
    catalog: RefCell<Rc<Catalog>>,
    loaded: Cell<bool>,
    installed: RefCell<Vec<Installed>>,
    updates: RefCell<Vec<String>>,
    busy: RefCell<HashMap<String, (Op, Progress)>>,
    tx: Sender<Msg>,
    page: RefCell<Option<AppPage>>,
    shots: RefCell<HashMap<String, Vec<gtk::Picture>>>,
    current: RefCell<&'static str>,
    demo: Option<f64>,
    pending: RefCell<Option<String>>,
}

pub fn run() -> glib::ExitCode {
    let app =
        gtk::Application::builder().application_id("in.trippp.Store").flags(gio::ApplicationFlags::NON_UNIQUE).build();
    app.connect_activate(|app| {
        if let Some(w) = app.active_window() {
            w.present();
            return;
        }
        build(app).present();
    });
    let args: Vec<String> = std::env::args().take(1).collect();
    app.run_with_args(&args)
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

/// An icon file at exactly `px` logical pixels, drawn at twice that so it stays sharp on
/// high-density screens (and an SVG is rendered at that size, not scaled up).
fn icon_widget(path: &std::path::Path, px: i32) -> gtk::Image {
    let paintable = gtk::IconPaintable::for_file(&gio::File::for_path(path), px, 2);
    let img = gtk::Image::from_paintable(Some(&paintable));
    img.set_pixel_size(px);
    img.set_halign(gtk::Align::Center);
    img.set_valign(gtk::Align::Center);
    img
}

fn flat(child: &impl IsA<gtk::Widget>, classes: &[&str]) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_child(Some(child));
    b.set_has_frame(false);
    b.set_focus_on_click(false);
    for c in classes {
        b.add_css_class(c);
    }
    b
}

fn build(app: &gtk::Application) -> gtk::ApplicationWindow {
    let display = gtk::gdk::Display::default();
    if let Some(d) = &display {
        let p = gtk::CssProvider::new();
        p.load_from_string(include_str!("store.css"));
        gtk::style_context_add_provider_for_display(d, &p, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Store")
        .default_width(1000)
        .default_height(625)
        .build();
    window.add_css_class("store");
    window.set_size_request(640, 420);
    // A thin title bar in the window's own ink: it moves and closes the window, nothing more.
    let header = gtk::HeaderBar::new();
    header.add_css_class("st-header");
    header.set_show_title_buttons(true);
    header.set_title_widget(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    window.set_titlebar(Some(&header));
    window.set_default_size(1000, 659);

    let (tx, rx) = async_channel::unbounded::<Msg>();
    let colours = gtk::CssProvider::new();
    if let Some(d) = &display {
        gtk::style_context_add_provider_for_display(d, &colours, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    }

    let root = gtk::Stack::new();
    root.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    root.set_transition_duration(240);
    let home = gtk::Stack::new();
    home.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    home.set_transition_duration(220);
    home.set_vexpand(true);

    let search = gtk::Entry::new();
    search.set_placeholder_text(Some("search apps"));
    search.set_width_chars(24);
    search.add_css_class("st-search");
    search.set_valign(gtk::Align::Start);

    let mut pivots = Vec::new();
    for (name, id) in
        [("featured", "featured"), ("categories", "categories"), ("installed", "installed"), ("updates", "updates")]
    {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&label(name, &["st-pivot-text"]));
        let badge = label("", &["st-badge"]);
        badge.set_valign(gtk::Align::Start);
        badge.set_visible(false);
        row.append(&badge);
        pivots.push((flat(&row, &["st-pivot"]), id, badge));
    }

    let demo = std::env::var("TRIPPPIN_STORE_DEMO").ok().and_then(|v| v.parse::<f64>().ok());
    let ui = Rc::new(Ui {
        window: window.clone(),
        root: root.clone(),
        home: home.clone(),
        pivots,
        search: search.clone(),
        colours: RefCell::new((HashSet::new(), colours)),
        catalog: RefCell::new(Rc::new(Catalog::default())),
        loaded: Cell::new(false),
        installed: RefCell::new(Vec::new()),
        updates: RefCell::new(Vec::new()),
        busy: RefCell::new(HashMap::new()),
        tx: tx.clone(),
        page: RefCell::new(None),
        shots: RefCell::new(HashMap::new()),
        current: RefCell::new("featured"),
        demo,
        pending: RefCell::new(std::env::var("TRIPPPIN_STORE_OPEN").ok()),
    });

    // The home page: STORE, search, pivots, then the pivot's page.
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("st-page");
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let hub = flat(&label("STORE", &["st-hub"]), &["st-back"]);
    hub.set_hexpand(true);
    hub.set_halign(gtk::Align::Start);
    {
        let ui = ui.clone();
        hub.connect_clicked(move |_| go_home(&ui));
    }
    top.append(&hub);
    top.append(&search);
    page.append(&top);
    let piv = gtk::Box::new(gtk::Orientation::Horizontal, 26);
    piv.add_css_class("st-pivots");
    for (b, id, _) in &ui.pivots {
        piv.append(b);
        let ui2 = ui.clone();
        let id = *id;
        b.connect_clicked(move |_| {
            ui2.search.set_text("");
            go_home(&ui2);
            select_pivot(&ui2, id);
        });
    }
    page.append(&piv);
    page.append(&home);
    root.add_named(&page, Some("home"));
    window.set_child(Some(&root));

    {
        let ui = ui.clone();
        search.connect_changed(move |e| {
            let q = e.text().to_string();
            if q.trim().is_empty() {
                let cur = *ui.current.borrow();
                select_pivot(&ui, cur);
            } else {
                show_search(&ui, &q);
            }
        });
    }

    let keys = gtk::EventControllerKey::new();
    {
        let ui = ui.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key != gtk::gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            if ui.root.visible_child_name().as_deref() != Some("home") {
                go_home(&ui);
            } else if !ui.search.text().is_empty() {
                ui.search.set_text("");
            } else {
                ui.window.close();
            }
            glib::Propagation::Stop
        });
    }
    window.add_controller(keys);

    // Messages from the threads.
    {
        let ui = ui.clone();
        glib::spawn_future_local(async move {
            while let Ok(m) = rx.recv().await {
                handle(&ui, m);
            }
        });
    }
    placeholder(&ui, "loading the catalogue…");
    select_pivot(&ui, "featured");
    start_loading(&ui);
    window
}

/// Parses the catalogue and asks flatpak what's installed, off the main thread.
fn start_loading(ui: &Rc<Ui>) {
    let tx = ui.tx.clone();
    std::thread::spawn(move || {
        let mut c = Catalog::load();
        if c.apps.is_empty() && std::env::var_os("TRIPPPIN_STORE_APPSTREAM").is_none() && store::refresh().is_ok() {
            c = Catalog::load();
        }
        let _ = tx.send_blocking(Msg::Catalog(c));
        refresh_lists(&tx);
    });
}

fn refresh_lists(tx: &Sender<Msg>) {
    if std::env::var_os("TRIPPPIN_STORE_DEMO").is_some() {
        let fake = |id: &str, name: &str| Installed {
            id: id.into(),
            name: name.into(),
            version: "1.0".into(),
            size: "40 mb".into(),
        };
        let _ = tx.send_blocking(Msg::Installed(vec![
            fake("org.videolan.VLC", "VLC"),
            fake("org.mozilla.firefox", "Firefox"),
            fake("org.gimp.GIMP", "GIMP"),
        ]));
        let _ = tx.send_blocking(Msg::Updates(vec![
            "org.mozilla.firefox".into(),
            "org.gimp.GIMP".into(),
            "org.videolan.VLC".into(),
        ]));
        return;
    }
    let _ = tx.send_blocking(Msg::Installed(store::installed()));
    let _ = tx.send_blocking(Msg::Updates(store::updates()));
}

fn handle(ui: &Rc<Ui>, m: Msg) {
    match m {
        Msg::Catalog(c) => {
            *ui.catalog.borrow_mut() = Rc::new(c);
            ui.loaded.set(true);
            rebuild_home(ui);
            // For screenshots: `pivot:categories`, `search:video`, or an app id to open.
            if let Some(id) = ui.pending.borrow_mut().take() {
                if let Some(p) = id.strip_prefix("pivot:") {
                    if let Some((_, pid, _)) = ui.pivots.iter().find(|(_, pid, _)| *pid == p) {
                        select_pivot(ui, pid);
                    }
                } else if let Some(q) = id.strip_prefix("search:") {
                    ui.search.set_text(q);
                } else {
                    open_app(ui, &id);
                }
            }
        }
        Msg::Installed(v) => {
            *ui.installed.borrow_mut() = v;
            rebuild_home(ui);
            render_actions(ui);
        }
        Msg::Updates(v) => {
            *ui.updates.borrow_mut() = v;
            rebuild_home(ui);
            render_actions(ui);
        }
        Msg::Shot(url, path) => {
            if let Some(pics) = ui.shots.borrow_mut().remove(&url)
                && let Some(path) = path
            {
                for p in pics {
                    p.set_filename(Some(&path));
                    p.add_css_class("ready");
                }
            }
        }
        Msg::Perms(id, r) => {
            if let Some(p) = ui.page.borrow().as_ref().filter(|p| p.id == id) {
                clear(&p.perms);
                p.perms.append(&label("IT CAN USE", &["st-small"]));
                match r {
                    Ok(lines) => {
                        for l in &lines {
                            p.perms.append(&label(l, &["st-line"]));
                        }
                        let rest = if lines.is_empty() {
                            "nothing · it runs in its own sandbox"
                        } else {
                            "nothing else · it runs in its own sandbox"
                        };
                        p.perms.append(&label(rest, &["st-line", "st-mute"]));
                    }
                    Err(e) => p.perms.append(&label(&format!("couldn't read it: {e}"), &["st-line", "st-mute"])),
                }
            }
        }
        Msg::Size(id, size) => {
            if let (Some(p), Some(size)) = (ui.page.borrow().as_ref().filter(|p| p.id == id), size) {
                p.meta.set_text(&p.meta_base.replace("{size}", &format!(" · {size}")));
            }
        }
        Msg::Progress(id, p) => {
            if let Some(e) = ui.busy.borrow_mut().get_mut(&id) {
                e.1 = p;
            }
            if let Some(page) = ui.page.borrow().as_ref().filter(|pg| pg.id == id) {
                page.target.set(p.fraction() * 100.0);
            }
        }
        Msg::Done(id, r) => {
            ui.busy.borrow_mut().remove(&id);
            if let (Err(e), Some(p)) = (&r, ui.page.borrow().as_ref().filter(|p| p.id == id)) {
                p.error.set_text(e);
                p.error.set_visible(true);
            }
            render_actions(ui);
            let tx = ui.tx.clone();
            std::thread::spawn(move || refresh_lists(&tx));
        }
    }
}

fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

// ---- the home pivots --------------------------------------------------------------------

fn placeholder(ui: &Rc<Ui>, text: &str) {
    replace_page(&ui.home, "message", &label(text, &["st-line", "st-mute", "st-message"]));
    ui.home.set_visible_child_name("message");
}

/// Swaps the page called `name` in a stack for a new one, leaving what's showing alone.
fn replace_page(stack: &gtk::Stack, name: &str, page: &impl IsA<gtk::Widget>) {
    if let Some(old) = stack.child_by_name(name) {
        stack.remove(&old);
    }
    stack.add_named(page, Some(name));
}

fn select_pivot(ui: &Rc<Ui>, id: &'static str) {
    *ui.current.borrow_mut() = id;
    for (b, pid, _) in &ui.pivots {
        if *pid == id {
            b.add_css_class("on");
        } else {
            b.remove_css_class("on");
        }
    }
    if ui.loaded.get() {
        ui.home.set_visible_child_name(id);
    }
}

fn go_home(ui: &Rc<Ui>) {
    if ui.root.visible_child_name().as_deref() == Some("home") {
        return;
    }
    ui.root.set_visible_child_full("home", gtk::StackTransitionType::SlideRight);
    let root = ui.root.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
        for name in ["app", "list"] {
            if root.visible_child_name().as_deref() != Some(name)
                && let Some(c) = root.child_by_name(name)
            {
                root.remove(&c);
            }
        }
    });
    *ui.page.borrow_mut() = None;
}

fn rebuild_home(ui: &Rc<Ui>) {
    if !ui.loaded.get() {
        return;
    }
    let catalog = ui.catalog.borrow().clone();
    if catalog.apps.is_empty() {
        placeholder(ui, "no app catalogue yet. Connect to the internet and open the Store again.");
        return;
    }
    let updates = ui.updates.borrow().clone();
    let installed = ui.installed.borrow().clone();
    for (_, id, badge) in &ui.pivots {
        let n = match *id {
            "updates" => updates.len(),
            _ => 0,
        };
        badge.set_text(&n.to_string());
        badge.set_visible(n > 0);
    }

    // featured: a live tile for waiting updates, then the curated apps.
    let mut specs = Vec::new();
    if !updates.is_empty() {
        specs.push(TileSpec {
            id: "#updates".into(),
            name: "updates".into(),
            colour: "#2c5aa0".into(),
            icon: None,
            big: Some(updates.len().to_string()),
            sub: "waiting".into(),
        });
    }
    for a in catalog.featured(24) {
        specs.push(spec_of(&catalog, a));
    }
    let wanted = *ui.current.borrow();
    replace_page(&ui.home, "featured", &tile_grid(ui, specs));

    // categories
    let cats: Vec<TileSpec> = store::CATEGORIES
        .iter()
        .enumerate()
        .map(|(i, (id, name))| {
            let n = catalog.apps.iter().filter(|a| a.categories.iter().any(|c| c == id)).count();
            TileSpec {
                id: format!("#cat:{id}"),
                name: (*name).into(),
                colour: store::PALETTE[i % 8].into(),
                icon: None,
                big: None,
                sub: format!("{n} apps"),
            }
        })
        .collect();
    replace_page(&ui.home, "categories", &tile_grid(ui, cats));

    // installed
    let specs: Vec<TileSpec> = installed
        .iter()
        .map(|i| match catalog.get(&i.id) {
            Some(a) => spec_of(&catalog, a),
            None => TileSpec {
                id: i.id.clone(),
                name: i.name.clone(),
                colour: store::PALETTE[4].into(),
                icon: store::installed_icon(&i.id),
                big: None,
                sub: String::new(),
            },
        })
        .collect();
    if specs.is_empty() {
        replace_page(
            &ui.home,
            "installed",
            &label("nothing installed from the store yet", &["st-line", "st-mute", "st-message"]),
        );
    } else {
        replace_page(&ui.home, "installed", &tile_grid(ui, specs));
    }

    replace_page(&ui.home, "updates", &updates_page(ui, &updates, &installed));
    if ui.root.visible_child_name().as_deref() == Some("home") && ui.search.text().is_empty() {
        if ui.home.visible_child_name().as_deref() == Some("message") {
            ui.home.set_transition_type(gtk::StackTransitionType::Crossfade);
            ui.home.set_visible_child_name(wanted);
            ui.home.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
        } else {
            ui.home.set_visible_child_name(wanted);
        }
    }
}

fn updates_page(ui: &Rc<Ui>, updates: &[String], installed: &[Installed]) -> gtk::Widget {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 6);
    col.add_css_class("st-list");
    if updates.is_empty() {
        col.append(&label("everything is up to date", &["st-line", "st-mute"]));
    } else {
        let all = flat(&label("update all", &["st-btn-text"]), &["st-btn"]);
        all.set_halign(gtk::Align::Start);
        all.set_margin_bottom(14);
        let ui2 = ui.clone();
        let ids = updates.to_vec();
        all.connect_clicked(move |_| {
            for id in &ids {
                run_op(&ui2, id, Op::Update);
            }
        });
        col.append(&all);
        for id in updates {
            let name = installed.iter().find(|i| &i.id == id).map_or(id.as_str(), |i| i.name.as_str());
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 18);
            let n = label(name, &["st-row-name"]);
            n.set_hexpand(true);
            row.append(&n);
            let busy = ui.busy.borrow().get(id).map(|b| b.0);
            let b = flat(&label(busy.map_or("update", Op::ing), &["st-link-text"]), &["st-link"]);
            let (ui2, id2) = (ui.clone(), id.clone());
            b.connect_clicked(move |_| run_op(&ui2, &id2, Op::Update));
            let open = flat(&label("open page", &["st-link-text", "st-mute"]), &["st-link"]);
            let (ui2, id2) = (ui.clone(), id.clone());
            open.connect_clicked(move |_| open_app(&ui2, &id2));
            row.append(&open);
            row.append(&b);
            col.append(&row);
        }
    }
    let sw = scroller(&col);
    sw.upcast()
}

fn scroller(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let sw = gtk::ScrolledWindow::new();
    sw.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    sw.set_child(Some(child));
    sw.set_vexpand(true);
    sw.add_css_class("st-scroll");
    sw
}

fn show_search(ui: &Rc<Ui>, q: &str) {
    let catalog = ui.catalog.borrow().clone();
    for (b, ..) in &ui.pivots {
        b.remove_css_class("on");
    }
    if ui.root.visible_child_name().as_deref() != Some("home") {
        go_home(ui);
    }
    let hits = catalog.search(q, 36);
    let page: gtk::Widget = if hits.is_empty() {
        label(&format!("nothing found for \"{}\"", q.trim()), &["st-line", "st-mute", "st-message"]).upcast()
    } else {
        tile_grid(ui, hits.iter().map(|a| spec_of(&catalog, a)).collect()).upcast()
    };
    let was = ui.home.visible_child_name();
    replace_page(&ui.home, "search", &page);
    if was.as_deref() != Some("search") {
        ui.home.set_transition_type(gtk::StackTransitionType::Crossfade);
    }
    ui.home.set_visible_child_name("search");
    ui.home.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
}

// ---- tiles ------------------------------------------------------------------------------

struct TileSpec {
    id: String,
    name: String,
    colour: String,
    icon: Option<PathBuf>,
    /// A big light number or word on the tile (a live tile).
    big: Option<String>,
    sub: String,
}

fn spec_of(catalog: &Catalog, a: &App) -> TileSpec {
    TileSpec {
        id: a.id.clone(),
        name: a.name.clone(),
        colour: a.tile_colour(),
        icon: catalog.icon_path(a),
        big: None,
        sub: String::new(),
    }
}

/// Makes `.bg-xxxxxx` mean that background colour, once per colour.
fn colour_class(ui: &Rc<Ui>, hex: &str) -> String {
    let name = format!("bg-{}", hex.trim_start_matches('#'));
    let mut c = ui.colours.borrow_mut();
    if c.0.insert(name.clone()) {
        let css: String = c.0.iter().map(|n| format!(".{n} {{ background: #{}; }}\n", &n[3..])).collect();
        c.1.load_from_string(&css);
    }
    name
}

fn tile(ui: &Rc<Ui>, s: &TileSpec, size: i32, index: usize) -> gtk::Widget {
    let overlay = gtk::Overlay::new();
    overlay.set_size_request(size, size);
    overlay.add_css_class("st-tile-face");
    overlay.add_css_class(&colour_class(ui, &s.colour));
    if let Some(big) = &s.big {
        let b = label(big, &["st-tile-big"]);
        b.set_halign(gtk::Align::Start);
        b.set_valign(gtk::Align::Start);
        overlay.set_child(Some(&b));
    } else if let Some(icon) = &s.icon {
        overlay.set_child(Some(&icon_widget(icon, (f64::from(size) * 0.5).round() as i32)));
    }
    let name = label(&s.name, &["st-tile-name"]);
    name.set_halign(gtk::Align::Start);
    name.set_valign(gtk::Align::End);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.set_max_width_chars(14);
    overlay.add_overlay(&name);
    if !s.sub.is_empty() {
        let sub = label(&s.sub, &["st-tile-sub"]);
        sub.set_halign(gtk::Align::Start);
        sub.set_valign(gtk::Align::Start);
        overlay.add_overlay(&sub);
    }
    let b = flat(&overlay, &["st-tile"]);
    let id = s.id.clone();
    let ui2 = ui.clone();
    b.connect_clicked(move |_| match id.as_str() {
        "#updates" => select_pivot(&ui2, "updates"),
        i if i.starts_with("#cat:") => open_category(&ui2, &i[5..]),
        i => open_app(&ui2, i),
    });
    // Tiles rise in one after another.
    let cell = gtk::Box::new(gtk::Orientation::Vertical, 0);
    cell.add_css_class("st-cell");
    cell.add_css_class(&format!("d{}", index.min(15)));
    cell.append(&b);
    cell.upcast()
}

fn tile_grid(ui: &Rc<Ui>, specs: Vec<TileSpec>) -> gtk::ScrolledWindow {
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_min_children_per_line(2);
    grid.set_max_children_per_line(GRID_COLUMNS);
    grid.set_column_spacing(5);
    grid.set_row_spacing(5);
    grid.set_valign(gtk::Align::Start);
    grid.set_halign(gtk::Align::Start);
    grid.add_css_class("st-grid");
    for (i, s) in specs.iter().enumerate() {
        grid.append(&tile(ui, s, 136, i));
    }
    scroller(&grid)
}

// ---- pages: a category, an app ----------------------------------------------------------

/// A page over the home page: STORE to go back, the title as a big pivot, then the content.
fn push_page(
    ui: &Rc<Ui>,
    name: &str,
    pivots: &[(&str, bool)],
    on_pivot: impl Fn(usize) + 'static,
    content: &impl IsA<gtk::Widget>,
) {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("st-page");
    let hub = flat(&label("STORE", &["st-hub"]), &["st-back"]);
    hub.set_halign(gtk::Align::Start);
    let ui2 = ui.clone();
    hub.connect_clicked(move |_| go_home(&ui2));
    page.append(&hub);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 26);
    row.add_css_class("st-pivots");
    let buttons: Rc<Vec<gtk::Button>> = Rc::new(
        pivots
            .iter()
            .map(|(t, on)| {
                let b = flat(&label(t, &["st-pivot-text"]), &["st-pivot"]);
                if *on {
                    b.add_css_class("on");
                }
                row.append(&b);
                b
            })
            .collect(),
    );
    let on_pivot = Rc::new(on_pivot);
    for (i, b) in buttons.iter().enumerate() {
        let (buttons, on_pivot) = (buttons.clone(), on_pivot.clone());
        b.connect_clicked(move |_| {
            for (j, o) in buttons.iter().enumerate() {
                if j == i {
                    o.add_css_class("on");
                } else {
                    o.remove_css_class("on");
                }
            }
            on_pivot(i);
        });
    }
    page.append(&row);
    page.append(content);
    replace_page(&ui.root, name, &page);
    ui.root.set_visible_child_full(name, gtk::StackTransitionType::SlideLeft);
}

fn open_category(ui: &Rc<Ui>, id: &str) {
    let catalog = ui.catalog.borrow().clone();
    let title = store::CATEGORIES.iter().find(|(c, _)| *c == id).map_or(id, |(_, n)| *n);
    let grid = tile_grid(ui, catalog.in_category(id, 60).iter().map(|a| spec_of(&catalog, a)).collect());
    push_page(ui, "list", &[(title, true)], |_| {}, &grid);
}

fn open_app(ui: &Rc<Ui>, id: &str) {
    let catalog = ui.catalog.borrow().clone();
    let app = catalog.get(id).cloned().unwrap_or_else(|| {
        let inst = ui.installed.borrow();
        let name = inst.iter().find(|i| i.id == id).map_or(id, |i| i.name.as_str()).to_string();
        App { id: id.to_string(), name, ..App::default() }
    });
    let icon = catalog.icon_path(&app);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(220);
    stack.set_vexpand(true);

    // ---- the app pane
    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
    col.add_css_class("st-appcol");
    let hero = gtk::Box::new(gtk::Orientation::Horizontal, 30);
    let tile = gtk::Overlay::new();
    tile.set_size_request(200, 200);
    tile.set_hexpand(false);
    tile.set_vexpand(false);
    tile.set_valign(gtk::Align::Start);
    tile.add_css_class("st-hero");
    tile.add_css_class(&colour_class(ui, &app.tile_colour()));
    if let Some(icon) = &icon {
        tile.set_child(Some(&icon_widget(icon, 120)));
    }
    hero.append(&tile);
    let info = gtk::Box::new(gtk::Orientation::Vertical, 10);
    info.set_hexpand(true);
    info.append(&label(&app.name, &["st-app-name"]));
    let mut facts: Vec<String> = Vec::new();
    for f in [&app.summary, &app.licence_words()] {
        if !f.is_empty() {
            facts.push(f.clone());
        }
    }
    let meta_base =
        format!("{}{{size}} · {}", facts.join(" · "), if app.verified { "Flathub, verified" } else { "Flathub" });
    let size = app.size.map(|s| format!(" · {}", store::size_words(s))).unwrap_or_default();
    let meta = label(&meta_base.replace("{size}", &size), &["st-meta"]);
    meta.set_wrap(true);
    info.append(&meta);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    actions.add_css_class("st-actions");
    info.append(&actions);
    let error = label("", &["st-line", "st-error"]);
    error.set_visible(false);
    info.append(&error);
    let perms = gtk::Box::new(gtk::Orientation::Vertical, 0);
    perms.add_css_class("st-perms");
    perms.append(&label("IT CAN USE", &["st-small"]));
    perms.append(&label("reading what it needs…", &["st-line", "st-mute"]));
    info.append(&perms);
    hero.append(&info);
    col.append(&hero);

    if !app.screenshots.is_empty() {
        let shots = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        shots.add_css_class("st-shots");
        for url in app.screenshots.iter().take(8) {
            let holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            holder.add_css_class("st-shot-holder");
            holder.set_size_request(356, 200);
            let pic = gtk::Picture::new();
            pic.set_can_shrink(true);
            pic.set_content_fit(gtk::ContentFit::Cover);
            pic.set_hexpand(true);
            pic.add_css_class("st-shot");
            holder.append(&pic);
            shots.append(&holder);
            ui.shots.borrow_mut().entry(url.clone()).or_default().push(pic);
            let (tx, url) = (ui.tx.clone(), url.clone());
            std::thread::spawn(move || {
                let path = fetch_shot(&url);
                let _ = tx.send_blocking(Msg::Shot(url, path));
            });
        }
        let sw = gtk::ScrolledWindow::new();
        sw.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
        sw.set_child(Some(&shots));
        sw.set_margin_top(30);
        col.append(&sw);
    }
    if !app.description.is_empty() {
        let d = label(&app.description, &["st-desc"]);
        d.set_wrap(true);
        d.set_max_width_chars(90);
        d.set_margin_top(26);
        col.append(&d);
    }
    stack.add_named(&scroller(&col), Some("app"));

    // ---- related
    let related = catalog.related(&app, 36);
    let rel: gtk::Widget = if related.is_empty() {
        label("nothing related", &["st-line", "st-mute", "st-message"]).upcast()
    } else {
        tile_grid(ui, related.iter().map(|a| spec_of(&catalog, a)).collect()).upcast()
    };
    stack.add_named(&rel, Some("related"));

    let st = stack.clone();
    let title = app.name.to_lowercase();
    push_page(
        ui,
        "app",
        &[(title.as_str(), true), ("related", false)],
        move |i| st.set_visible_child_name(if i == 0 { "app" } else { "related" }),
        &stack,
    );

    let target = Rc::new(Cell::new(0.0));
    *ui.page.borrow_mut() = Some(AppPage { id: app.id.clone(), actions, error, perms, meta, meta_base, target });
    render_actions(ui);
    if let Some(p) = ui.demo
        && ui.busy.borrow().get(&app.id).is_none()
    {
        let prog = Progress { step: 1, steps: 1, percent: p as u32 };
        ui.busy.borrow_mut().insert(app.id.clone(), (Op::Install, prog));
        if let Some(pg) = ui.page.borrow().as_ref() {
            pg.target.set(p);
        }
        render_actions(ui);
    }

    // Permissions and size come from the remote; both take a moment.
    let (tx, id) = (ui.tx.clone(), app.id.clone());
    let need_size = app.size.is_none();
    std::thread::spawn(move || {
        let perms = if std::env::var_os("TRIPPPIN_STORE_DEMO").is_some() {
            Ok(vec!["the internet, to sync your vault".to_string(), "notifications".to_string()])
        } else {
            store::permissions(&id)
        };
        let _ = tx.send_blocking(Msg::Perms(id.clone(), perms));
        if need_size {
            let size = if std::env::var_os("TRIPPPIN_STORE_DEMO").is_some() {
                Some("92 MB".into())
            } else {
                store::download_size(&id)
            };
            let _ = tx.send_blocking(Msg::Size(id, size));
        }
    });
}

/// The install row: a flat accent button, and while it works, marching dots and a percentage.
fn render_actions(ui: &Rc<Ui>) {
    let page = ui.page.borrow();
    let Some(p) = page.as_ref() else { return };
    clear(&p.actions);
    let id = p.id.clone();
    let busy = ui.busy.borrow().get(&id).map(|b| b.0);
    let have = ui.installed.borrow().iter().any(|i| i.id == id);
    let waiting = ui.updates.borrow().contains(&id);
    let button = |text: &str, accent: bool| {
        let b = flat(&label(text, &["st-btn-text"]), &["st-btn"]);
        if !accent {
            b.add_css_class("quiet");
        }
        b
    };
    if let Some(op) = busy {
        p.actions.append(&button(op.ing(), true));
        let shown = Rc::new(Cell::new(0.0f64));
        let pct = label("0%", &["st-meta"]);
        let dots = marching_dots(p.target.clone(), shown, pct.clone());
        p.actions.append(&dots);
        p.actions.append(&pct);
        p.error.set_visible(false);
        return;
    }
    if have {
        let open = button("open", true);
        let id2 = id.clone();
        open.connect_clicked(move |_| {
            let _ = std::process::Command::new("flatpak").args(["run", &id2]).spawn();
        });
        p.actions.append(&open);
        if waiting {
            let up = button("update", true);
            let (ui2, id2) = (ui.clone(), id.clone());
            up.connect_clicked(move |_| run_op(&ui2, &id2, Op::Update));
            p.actions.append(&up);
        }
        let rm = button("remove", false);
        let (ui2, id2) = (ui.clone(), id.clone());
        rm.connect_clicked(move |_| run_op(&ui2, &id2, Op::Remove));
        p.actions.append(&rm);
    } else {
        let b = button("install", true);
        let (ui2, id2) = (ui.clone(), id.clone());
        b.connect_clicked(move |_| run_op(&ui2, &id2, Op::Install));
        p.actions.append(&b);
    }
}

/// Five accent squares with a wave of light marching along them; the percentage beside them
/// eases towards the real one, so it moves smoothly however flatpak reports.
fn marching_dots(target: Rc<Cell<f64>>, shown: Rc<Cell<f64>>, pct: gtk::Label) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(65);
    area.set_content_height(5);
    area.set_valign(gtk::Align::Center);
    area.set_draw_func(|a, cr, _, _| {
        let animate = gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations());
        let t = if animate { a.frame_clock().map_or(0.0, |c| c.frame_time() as f64 / 1_000_000.0) } else { 0.0 };
        for i in 0..5 {
            let wave = (((t * 1.5 - f64::from(i) * 0.16) * std::f64::consts::TAU).cos() * 0.5 + 0.5).powi(3);
            cr.set_source_rgba(0.247, 0.682, 0.478, 0.25 + 0.75 * wave);
            cr.rectangle(f64::from(i) * 15.0, 0.0, 5.0, 5.0);
            cr.fill().ok();
        }
    });
    let last = Cell::new(0.0f64);
    area.add_tick_callback(move |a, clock| {
        a.queue_draw();
        let now = clock.frame_time() as f64 / 1_000_000.0;
        let dt = (now - last.replace(now)).clamp(0.0, 0.1);
        let (cur, goal) = (shown.get(), target.get());
        // Ease out: close a good part of the gap each frame, at any frame rate.
        let next = cur + (goal - cur) * (1.0 - (-dt * 8.0).exp());
        shown.set(next);
        pct.set_text(&format!("{}%", next.round() as u32));
        glib::ControlFlow::Continue
    });
    area
}

fn run_op(ui: &Rc<Ui>, id: &str, op: Op) {
    if ui.busy.borrow().contains_key(id) {
        return;
    }
    ui.busy.borrow_mut().insert(id.to_string(), (op, Progress::default()));
    if let Some(p) = ui.page.borrow().as_ref().filter(|p| p.id == id) {
        p.target.set(0.0);
    }
    render_actions(ui);
    rebuild_home(ui);
    let (tx, id) = (ui.tx.clone(), id.to_string());
    std::thread::spawn(move || {
        let t2 = tx.clone();
        let id2 = id.clone();
        let mut report = move |p: Progress| {
            let _ = t2.send_blocking(Msg::Progress(id2.clone(), p));
        };
        let r = match op {
            Op::Install => store::install(&id, &mut report),
            Op::Remove => store::remove(&id, &mut report),
            Op::Update => store::update(Some(&id), &mut report),
        };
        let _ = tx.send_blocking(Msg::Done(id, r));
    });
}

/// Downloads a screenshot into the cache (once) and returns where it is. A path that
/// already is a file (the demo catalogue) is used as it is.
fn fetch_shot(url: &str) -> Option<PathBuf> {
    if url.starts_with('/') {
        return Some(PathBuf::from(url));
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    let base = match std::env::var_os("XDG_CACHE_HOME") {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".cache"),
    };
    let dir = base.join("tripppin/store-shots");
    let path = dir.join(format!("{:016x}.img", h.finish()));
    if path.exists() {
        return Some(path);
    }
    std::fs::create_dir_all(&dir).ok()?;
    let mut resp = ureq::get(url).call().ok()?;
    let bytes = resp.body_mut().with_config().limit(12_000_000).read_to_vec().ok()?;
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes).ok()?;
    std::fs::rename(&tmp, &path).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_stylesheet_names_the_design_tokens() {
        let css = include_str!("store.css");
        for token in ["#101418", "#1a2025", "#9aa4ab", "#3fae7a", "#4b555c"] {
            assert!(css.contains(token), "missing {token}");
        }
    }
}
