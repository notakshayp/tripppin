//! The desktop itself: a transparent layer-shell surface per screen, above the wallpaper and
//! below the windows. Right-click (or long-press) opens a menu; the entries of ~/Desktop show
//! as icons in a grid from the top left, and the folder is watched so changes appear at once.

use crate::apps;
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

const ICON: i32 = 48;
const CELL_W: i32 = 96;
const CELL_H: i32 = 104;
const PAD: i32 = 16;

// ---- pure parts -------------------------------------------------------------------------

/// Where the `index`th icon goes: top-left origin, filling a column from the top, then the
/// next column, with `height` pixels of room below the bar.
pub fn grid_position(index: usize, height: i32) -> (i32, i32) {
    let rows = rows_per_column(height);
    let (col, row) = (index / rows, index % rows);
    (PAD + col as i32 * CELL_W, PAD + row as i32 * CELL_H)
}

pub fn rows_per_column(height: i32) -> usize {
    ((height - PAD) / CELL_H).max(1) as usize
}

/// Name and icon of a .desktop launcher; `None` when it isn't an application or has no name.
pub fn launcher_info(text: &str) -> Option<(String, String)> {
    let (mut name, mut icon, mut in_entry, mut is_app) = (String::new(), String::new(), false, false);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        if !in_entry {
            continue;
        }
        match k.trim() {
            "Name" => name = v.trim().to_string(),
            "Icon" => icon = v.trim().to_string(),
            "Type" => is_app = v.trim() == "Application",
            _ => {}
        }
    }
    (is_app && !name.is_empty()).then_some((name, icon))
}

/// The desktop folder from `xdg-user-dir DESKTOP` output; the home one when it's empty, or
/// is the home folder itself (what xdg-user-dir prints when no desktop folder is set).
pub fn desktop_dir_from(xdg: &str, home: &Path) -> PathBuf {
    let x = xdg.trim();
    if x.is_empty() || Path::new(x) == home { home.join("Desktop") } else { PathBuf::from(x) }
}

/// `base` if `taken` doesn't hold it, else "base 2", "base 3"… (before the extension).
pub fn unique_name(taken: &[String], base: &str) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    let (stem, ext) = match base.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (base, String::new()),
    };
    (2..).map(|n| format!("{stem} {n}{ext}")).find(|c| !taken.contains(c)).unwrap_or_else(|| base.to_string())
}

// ---- the folder ---------------------------------------------------------------------------

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn desktop_dir() -> PathBuf {
    let out = Command::new("xdg-user-dir")
        .arg("DESKTOP")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    desktop_dir_from(&out, &home())
}

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    launcher: bool,
    folder: bool,
    icon: gio::Icon,
}

fn icon_from_name(name: &str) -> gio::Icon {
    if name.starts_with('/') {
        gio::FileIcon::new(&gio::File::for_path(name)).upcast()
    } else if name.is_empty() {
        gio::ThemedIcon::new("application-x-executable").upcast()
    } else {
        gio::ThemedIcon::new(name).upcast()
    }
}

fn entries(dir: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = e.path();
        let file_name = e.file_name().to_string_lossy().into_owned();
        if file_name.starts_with('.') {
            continue;
        }
        let folder = path.is_dir();
        let launcher_data = (!folder && file_name.ends_with(".desktop"))
            .then(|| std::fs::read_to_string(&path).ok().and_then(|t| launcher_info(&t)))
            .flatten();
        let (name, launcher, icon) = match launcher_data {
            Some((n, i)) => (n, true, icon_from_name(&i)),
            None => {
                let icon = gio::File::for_path(&path)
                    .query_info("standard::icon", gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)
                    .ok()
                    .and_then(|i| i.icon())
                    .unwrap_or_else(|| gio::ThemedIcon::new(if folder { "folder" } else { "text-x-generic" }).upcast());
                (file_name, false, icon)
            }
        };
        out.push(Entry { path, name, launcher, folder, icon });
    }
    out.sort_by_key(|e| (!e.folder, e.name.to_lowercase()));
    out
}

fn names_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

fn spawn(cmd: &str, args: &[&str]) {
    if let Ok(mut c) = Command::new(cmd).args(args).spawn() {
        std::thread::spawn(move || c.wait());
    }
}

fn open(e: &Entry) {
    let path = e.path.to_string_lossy();
    if e.launcher {
        spawn("gio", &["launch", &path]);
    } else {
        spawn("xdg-open", &[&path]);
    }
}

fn find_desktop_file(id: &str) -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share"));
    [
        data.join("applications"),
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        data.join("flatpak/exports/share/applications"),
    ]
    .into_iter()
    .map(|d| d.join(format!("{id}.desktop")))
    .find(|p| p.is_file())
}

fn add_app(dir: &Path, id: &str) {
    let Some(src) = find_desktop_file(id) else { return };
    let _ = std::fs::create_dir_all(dir);
    let dest = dir.join(unique_name(&names_in(dir), &format!("{id}.desktop")));
    if std::fs::copy(&src, &dest).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
        }
        spawn("gio", &["set", &dest.to_string_lossy(), "metadata::trusted", "true"]);
    }
}

// ---- the surfaces -------------------------------------------------------------------------

struct Surface {
    monitor: gtk::gdk::Monitor,
    window: gtk::Window,
    area: gtk::Fixed,
    popover: gtk::Popover,
    selected: Rc<RefCell<Option<PathBuf>>>,
    icons: RefCell<Vec<gtk::Widget>>,
}

#[derive(Default)]
struct Desk {
    surfaces: Vec<Surface>,
    dir: PathBuf,
}

type Shared = Rc<RefCell<Desk>>;

fn menu_button(text: &str, on: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("menu-item");
    if let Some(l) = b.child().and_downcast::<gtk::Label>() {
        l.set_xalign(0.0);
    }
    b.connect_clicked(move |_| on());
    b
}

fn menu_box() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
    b.set_margin_top(4);
    b.set_margin_bottom(4);
    b.set_margin_start(4);
    b.set_margin_end(4);
    b
}

fn pop_at(pop: &gtk::Popover, x: f64, y: f64, content: &impl IsA<gtk::Widget>) {
    pop.popdown();
    pop.set_child(Some(content));
    pop.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    pop.popup();
}

fn desktop_menu(shared: &Shared, pop: &gtk::Popover, x: f64, y: f64) {
    let b = menu_box();
    let dir = shared.borrow().dir.clone();
    {
        let (pop, dir) = (pop.clone(), dir.clone());
        b.append(&menu_button("New folder", move || {
            pop.popdown();
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::create_dir(dir.join(unique_name(&names_in(&dir), "New folder")));
        }));
    }
    {
        let pop = pop.clone();
        b.append(&menu_button("Add app to desktop…", move || app_picker(&pop, &dir, x, y)));
    }
    {
        let pop = pop.clone();
        b.append(&menu_button("Add widget…", move || widget_menu(&pop, x, y)));
    }
    for (label, cmd, args) in [
        ("Change wallpaper", "tripppin-ui", &["settings", "appearance"][..]),
        ("Display settings", "tripppin-ui", &["settings", "display"][..]),
        ("Open terminal", "foot", &[][..]),
    ] {
        let pop = pop.clone();
        b.append(&menu_button(label, move || {
            pop.popdown();
            spawn(cmd, args);
        }));
    }
    pop_at(pop, x, y, &b);
}

fn widget_menu(pop: &gtk::Popover, x: f64, y: f64) {
    let b = menu_box();
    let c = pop.clone();
    b.append(&menu_button("System widget", move || {
        c.popdown();
        spawn("tripppin-ui", &["widgets", "add-system"]);
    }));
    let c = pop.clone();
    b.append(&menu_button("Ask Tripppin to make one", move || {
        c.popdown();
        spawn("tripppin-ui", &["orb", "--toggle"]);
    }));
    pop_at(pop, x, y, &b);
}

fn app_picker(pop: &gtk::Popover, dir: &Path, x: f64, y: f64) {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
    b.set_margin_top(6);
    b.set_margin_bottom(6);
    b.set_margin_start(6);
    b.set_margin_end(6);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search apps"));
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    for a in &apps::installed() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let img = gtk::Image::from_gicon(&icon_from_name(&a.icon));
        img.set_pixel_size(24);
        let name = gtk::Label::new(Some(&a.name));
        name.set_xalign(0.0);
        row.append(&img);
        row.append(&name);
        let r = gtk::ListBoxRow::new();
        r.set_child(Some(&row));
        r.set_widget_name(&a.name.to_lowercase());
        list.append(&r);
        let click = gtk::GestureClick::new();
        let (pop, dir, id) = (pop.clone(), dir.to_path_buf(), a.id.clone());
        click.connect_released(move |_, _, _, _| {
            add_app(&dir, &id);
            pop.popdown();
        });
        r.add_controller(click);
    }
    let q = Rc::new(RefCell::new(String::new()));
    {
        let q = q.clone();
        list.set_filter_func(move |r| r.widget_name().contains(q.borrow().as_str()));
    }
    {
        let list = list.clone();
        search.connect_search_changed(move |s| {
            *q.borrow_mut() = s.text().to_lowercase();
            list.invalidate_filter();
        });
    }
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_min_content_height(320);
    scroll.set_min_content_width(300);
    b.append(&search);
    b.append(&scroll);
    pop_at(pop, x, y, &b);
    search.grab_focus();
}

fn icon_menu(shared: &Shared, pop: &gtk::Popover, e: &Entry, x: f64, y: f64) {
    let b = menu_box();
    let (c, en) = (pop.clone(), e.clone());
    b.append(&menu_button("Open", move || {
        c.popdown();
        open(&en);
    }));
    if !e.launcher {
        let (pop, en, dir) = (pop.clone(), e.clone(), shared.borrow().dir.clone());
        b.append(&menu_button("Rename", move || rename_box(&dir, &pop, &en, x, y)));
    }
    let (c, en) = (pop.clone(), e.clone());
    b.append(&menu_button("Remove from desktop", move || {
        c.popdown();
        let _ = gio::File::for_path(&en.path).trash(gio::Cancellable::NONE);
    }));
    pop_at(pop, x, y, &b);
}

fn rename_box(dir: &Path, pop: &gtk::Popover, e: &Entry, x: f64, y: f64) {
    let entry = gtk::Entry::new();
    entry.set_text(&e.name);
    entry.set_margin_top(6);
    entry.set_margin_bottom(6);
    entry.set_margin_start(6);
    entry.set_margin_end(6);
    let (pop2, path, dir) = (pop.clone(), e.path.clone(), dir.to_path_buf());
    entry.connect_activate(move |en| {
        let new = en.text().trim().to_string();
        if !new.is_empty() && !new.contains('/') && Some(new.as_str()) != path.file_name().and_then(|n| n.to_str()) {
            let target = dir.join(unique_name(&names_in(&dir), &new));
            let _ = std::fs::rename(&path, target);
        }
        pop2.popdown();
    });
    pop_at(pop, x, y, &entry);
    entry.grab_focus();
}

fn entry_widget(shared: &Shared, s: &Surface, e: &Entry) -> gtk::Box {
    let cell = gtk::Box::new(gtk::Orientation::Vertical, 4);
    cell.add_css_class("desktop-icon");
    cell.set_size_request(CELL_W - 8, CELL_H - 8);
    if s.selected.borrow().as_deref() == Some(e.path.as_path()) {
        cell.add_css_class("selected");
    }
    let img = gtk::Image::from_gicon(&e.icon);
    img.set_pixel_size(ICON);
    let name = gtk::Label::new(Some(&e.name));
    name.add_css_class("desktop-label");
    name.set_wrap(true);
    name.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    name.set_lines(2);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.set_justify(gtk::Justification::Center);
    name.set_max_width_chars(11);
    cell.append(&img);
    cell.append(&name);

    let left = gtk::GestureClick::new();
    left.set_button(1);
    let (en, sel, shared2) = (e.clone(), s.selected.clone(), shared.clone());
    left.connect_pressed(move |g, n, _, _| {
        g.set_state(gtk::EventSequenceState::Claimed);
        if n >= 2 {
            open(&en);
        } else {
            *sel.borrow_mut() = Some(en.path.clone());
            let sh = shared2.clone();
            glib::idle_add_local_once(move || restyle(&sh));
        }
    });
    cell.add_controller(left);

    let right = gtk::GestureClick::new();
    right.set_button(3);
    let (en, pop, shared2, cellw, area) = (e.clone(), s.popover.clone(), shared.clone(), cell.clone(), s.area.clone());
    right.connect_pressed(move |g, _, x, y| {
        g.set_state(gtk::EventSequenceState::Claimed);
        let (ax, ay) = cellw
            .compute_point(&area, &gtk::graphene::Point::new(x as f32, y as f32))
            .map_or((x, y), |p| (f64::from(p.x()), f64::from(p.y())));
        icon_menu(&shared2, &pop, &en, ax, ay);
    });
    cell.add_controller(right);
    cell
}

/// Redraws every screen's icons (cheap: a handful of widgets).
fn restyle(shared: &Shared) {
    let d = shared.borrow();
    let list = entries(&d.dir);
    for s in &d.surfaces {
        for w in s.icons.borrow_mut().drain(..) {
            s.area.remove(&w);
        }
        let h = s.monitor.geometry().height() - crate::snap::BAR;
        for (i, e) in list.iter().enumerate() {
            let (x, y) = grid_position(i, h);
            let w = entry_widget(shared, s, e);
            s.area.put(&w, x as f64, y as f64);
            s.icons.borrow_mut().push(w.upcast());
        }
    }
}

fn make_surface(app: &gtk::Application, shared: &Shared, monitor: gtk::gdk::Monitor) -> Surface {
    let window = gtk::Window::builder().application(app).decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("clear");
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-desktop"));
    window.set_layer(Layer::Background);
    window.set_exclusive_zone(0);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    window.set_margin(Edge::Top, crate::snap::BAR);
    window.set_monitor(Some(&monitor));

    let area = gtk::Fixed::new();
    area.add_css_class("desktop-area");
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_parent(&area);
    window.set_child(Some(&area));

    let selected: Rc<RefCell<Option<PathBuf>>> = Rc::default();
    // Empty desktop: right-click or long-press for the menu; a click clears the selection.
    let right = gtk::GestureClick::new();
    right.set_button(3);
    let (sh, pop) = (shared.clone(), popover.clone());
    right.connect_pressed(move |_, _, x, y| desktop_menu(&sh, &pop, x, y));
    area.add_controller(right);
    let long = gtk::GestureLongPress::new();
    long.set_touch_only(true);
    let (sh, pop) = (shared.clone(), popover.clone());
    long.connect_pressed(move |_, x, y| desktop_menu(&sh, &pop, x, y));
    area.add_controller(long);
    let left = gtk::GestureClick::new();
    left.set_button(1);
    let (sel, sh) = (selected.clone(), shared.clone());
    left.connect_pressed(move |_, _, _, _| {
        if sel.borrow_mut().take().is_some() {
            let sh = sh.clone();
            glib::idle_add_local_once(move || restyle(&sh));
        }
    });
    area.add_controller(left);

    window.present();
    Surface { monitor, window, area, popover, selected, icons: RefCell::default() }
}

fn sync_monitors(app: &gtk::Application, shared: &Shared) {
    let Some(display) = gtk::gdk::Display::default() else { return };
    let list = display.monitors();
    let current: Vec<gtk::gdk::Monitor> = (0..list.n_items()).filter_map(|i| list.item(i).and_downcast()).collect();
    shared.borrow_mut().surfaces.retain(|s| {
        let keep = s.monitor.is_valid() && current.contains(&s.monitor);
        if !keep {
            s.popover.unparent();
            s.window.destroy();
        }
        keep
    });
    for m in current {
        if !shared.borrow().surfaces.iter().any(|s| s.monitor == m) {
            let s = make_surface(app, shared, m);
            shared.borrow_mut().surfaces.push(s);
        }
    }
    restyle(shared);
}

pub fn run() -> glib::ExitCode {
    let app =
        gtk::Application::builder().application_id("in.trippp.Desktop").flags(gio::ApplicationFlags::default()).build();
    let started: Rc<RefCell<Option<gio::ApplicationHoldGuard>>> = Rc::default();
    let keep: Rc<RefCell<Option<gio::FileMonitor>>> = Rc::default();
    app.connect_activate(move |app| {
        if started.borrow().is_some() {
            return;
        }
        *started.borrow_mut() = Some(app.hold());
        crate::common::load_css();
        let dir = desktop_dir();
        let _ = std::fs::create_dir_all(&dir);
        let shared: Shared = Rc::new(RefCell::new(Desk { surfaces: Vec::new(), dir: dir.clone() }));
        sync_monitors(app, &shared);

        if let Some(display) = gtk::gdk::Display::default() {
            let (app, shared) = (app.clone(), shared.clone());
            display.monitors().connect_items_changed(move |_, _, _, _| sync_monitors(&app, &shared));
        }
        if let Ok(mon) =
            gio::File::for_path(&dir).monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        {
            let pending: Rc<RefCell<bool>> = Rc::default();
            let shared = shared.clone();
            mon.connect_changed(move |_, _, _, _| {
                if pending.replace(true) {
                    return;
                }
                let (pending, shared) = (pending.clone(), shared.clone());
                glib::timeout_add_local_once(Duration::from_millis(150), move || {
                    *pending.borrow_mut() = false;
                    restyle(&shared);
                });
            });
            *keep.borrow_mut() = Some(mon);
        }
    });
    let args: Vec<String> = std::env::args().take(1).collect();
    app.run_with_args(&args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_fills_columns_top_down() {
        assert_eq!(rows_per_column(600), 5);
        assert_eq!(grid_position(0, 600), (PAD, PAD));
        assert_eq!(grid_position(1, 600), (PAD, PAD + CELL_H));
        assert_eq!(grid_position(5, 600), (PAD + CELL_W, PAD));
        assert_eq!(rows_per_column(10), 1);
    }

    #[test]
    fn launcher_parsing() {
        let t = "[Desktop Entry]\nType=Application\nName=Files\nIcon=folder\n[Desktop Action x]\nName=Other\n";
        assert_eq!(launcher_info(t), Some(("Files".into(), "folder".into())));
        assert_eq!(launcher_info("[Desktop Entry]\nType=Link\nName=x\n"), None);
        assert_eq!(launcher_info("Name=x\nType=Application\n"), None);
    }

    #[test]
    fn desktop_dir_choice() {
        let home = Path::new("/home/a");
        assert_eq!(desktop_dir_from("/home/a/Skrivbord\n", home), PathBuf::from("/home/a/Skrivbord"));
        assert_eq!(desktop_dir_from("/home/a\n", home), PathBuf::from("/home/a/Desktop"));
        assert_eq!(desktop_dir_from("", home), PathBuf::from("/home/a/Desktop"));
    }

    #[test]
    fn unique_names() {
        let t = vec!["New folder".to_string(), "a.desktop".to_string(), "a 2.desktop".to_string()];
        assert_eq!(unique_name(&t, "New folder"), "New folder 2");
        assert_eq!(unique_name(&t, "a.desktop"), "a 3.desktop");
        assert_eq!(unique_name(&t, "b"), "b");
    }
}
