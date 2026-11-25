//! The Calendar app (`tripppin-ui calendar`), drawn like the Bento design (mock 07): the
//! month is a tile, with an amber TODAY tile and an UPCOMING tile beside it; day and week are
//! one tile each, with chips to switch. The day is a huge number with the weekday and month
//! beside an hour rail of coloured blocks; a
//! "+ add" field takes plain words ("call mum friday 6pm"). Events are iCalendar files
//! (see `cal`). Left/Right step through days, weeks or months; d, w, m change the view.

use crate::cal::{self, Date, Event, Stamp};
use crate::common::label;
use crate::hub::PALETTE;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub fn now() -> Stamp {
    let t = glib::DateTime::now_local().ok();
    match t {
        Some(t) => Stamp {
            date: Date::new(t.year(), t.month() as u32, t.day_of_month() as u32),
            min: t.hour() as u32 * 60 + t.minute() as u32,
        },
        None => Stamp { date: Date::new(2026, 1, 1), min: 0 },
    }
}

/// `tripppin-ui calendar --remind`: announces events that just started. Run every minute by
/// the user timer (tripppin-calendar-remind.timer), so reminders come with the app closed.
pub fn remind_now() {
    let now = now();
    cal::remind(now);
    write_next_event(now);
}

/// Leaves the next event for the lock screen in ~/.local/share/tripppin/next-event.json
/// (`{"title","when","until"}`, `until` in Unix seconds), or removes the file when there's none.
fn write_next_event(now: Stamp) {
    let path = cal::dir().parent().map(|d| d.join("next-event.json"));
    let Some(path) = path else { return };
    let next = cal::load_from(&cal::dir()).into_iter().filter(|e| e.end() > now).min_by_key(|e| e.start);
    let Some(e) = next else {
        let _ = std::fs::remove_file(&path);
        return;
    };
    let day = if e.start.date == now.date {
        "Today".to_string()
    } else if e.start.date == now.date.plus(1) {
        "Tomorrow".to_string()
    } else {
        let w = e.start.date.weekday_name();
        w[..1].to_uppercase() + &w[1..]
    };
    let end = e.end();
    let until = glib::DateTime::new(
        &glib::TimeZone::local(),
        end.date.y,
        end.date.m as i32,
        end.date.d as i32,
        (end.min / 60) as i32,
        (end.min % 60) as i32,
        0.0,
    )
    .map(|t| t.to_unix())
    .unwrap_or(0);
    let when = format!("{day} {} – {}", cal::clock(e.start.min), cal::clock(end.min));
    let json = serde_json::json!({ "title": e.title, "when": when, "until": until });
    let _ = std::fs::write(&path, json.to_string());
}

struct State {
    events: Vec<Event>,
    selected: Date,
    view: usize,
}

type Redo = Rc<dyn Fn()>;
type Shared = Rc<RefCell<State>>;

const VIEWS: [&str; 3] = ["day", "week", "month"];

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("in.trippp.Calendar")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let window: Rc<RefCell<Option<gtk::ApplicationWindow>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, _| {
        let win = window.borrow_mut().get_or_insert_with(|| build(app)).clone();
        win.present();
        glib::ExitCode::SUCCESS
    });
    let args: Vec<String> = std::env::args().take(1).collect();
    app.run_with_args(&args)
}

fn animate(widgets: &[gtk::Widget]) {
    for (i, w) in widgets.iter().enumerate() {
        w.add_css_class("ev-in");
        w.add_css_class(&format!("d{}", i.min(8)));
    }
}

fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

fn block(e: &Event, state: &Shared, rebuild: &Rc<dyn Fn()>, compact: bool) -> gtk::Widget {
    let b = gtk::Button::new();
    b.add_css_class("cal-block");
    b.add_css_class(&format!("c{}", &PALETTE[e.colour()][1..]));
    let inner = gtk::Box::new(gtk::Orientation::Vertical, 0);
    inner.set_halign(gtk::Align::Fill);
    let title = label(&e.title, &[if compact { "cal-title-s" } else { "cal-title" }]);
    title.set_wrap(true);
    title.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    let detail = if compact { cal::clock(e.start.min) } else { e.sub() };
    let sub = label(&detail, &[if compact { "cal-sub-s" } else { "cal-sub" }]);
    inner.append(&title);
    inner.append(&sub);
    b.set_child(Some(&inner));
    b.set_hexpand(true);
    // Click for the one thing you can do to an event.
    let pop = gtk::Popover::new();
    pop.set_parent(&b);
    pop.add_css_class("cal-pop");
    let delete = gtk::Button::with_label("delete");
    delete.add_css_class("cal-delete");
    pop.set_child(Some(&delete));
    {
        let (state, rebuild, uid, pop) = (state.clone(), rebuild.clone(), e.uid.clone(), pop.clone());
        delete.connect_clicked(move |_| {
            pop.popdown();
            cal::delete_in(&cal::dir(), &uid);
            state.borrow_mut().events = cal::load_from(&cal::dir());
            rebuild();
        });
    }
    {
        let pop = pop.clone();
        b.connect_clicked(move |_| pop.popup());
    }
    b.connect_destroy(move |_| pop.unparent());
    b.upcast()
}

/// The "+ add" field: plain words in, an event out.
fn add_field(state: &Shared, rebuild: &Rc<dyn Fn()>, note: &gtk::Label) -> gtk::Entry {
    let entry = gtk::Entry::new();
    entry.add_css_class("cal-add");
    entry.set_placeholder_text(Some("+ add \"call mum friday 6pm\""));
    let (state, rebuild, note) = (state.clone(), rebuild.clone(), note.clone());
    entry.connect_activate(move |e| {
        let text = e.text().to_string();
        if text.trim().is_empty() {
            return;
        }
        match cal::parse_event(&text, now()) {
            Some(p) => {
                let ev = Event {
                    uid: cal::new_uid(),
                    title: p.title,
                    start: p.start,
                    minutes: p.minutes,
                    detail: String::new(),
                };
                if let Err(err) = cal::save_in(&cal::dir(), &ev) {
                    note.set_text(&format!("couldn't save it: {err}"));
                    return;
                }
                note.set_text(&format!(
                    "added {} · {} {} {}, {}",
                    ev.title.to_lowercase(),
                    ev.start.date.weekday_name(),
                    ev.start.date.d,
                    ev.start.date.month_name(),
                    cal::clock(ev.start.min)
                ));
                e.set_text("");
                let mut s = state.borrow_mut();
                s.selected = ev.start.date;
                s.events = cal::load_from(&cal::dir());
                drop(s);
                rebuild();
            }
            None => note.set_text("what is it called? try: call mum friday 6pm"),
        }
    });
    entry
}

fn day_view(s: &State, state: &Shared, rebuild: &Rc<dyn Fn()>, note: &gtk::Label) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 40);
    root.add_css_class("cal-tile");
    let d = s.selected;
    let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
    left.set_size_request(220, -1);
    left.set_valign(gtk::Align::Start);
    left.append(&label(&d.d.to_string(), &["cal-daynum"]));
    left.append(&label(d.weekday_name(), &["cal-weekday"]));
    left.append(&label(&format!("{} {}", d.month_name(), d.y), &["cal-month"]));
    root.append(&left);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 5);
    let todays = cal::on(&s.events, d);
    let mut animated: Vec<gtk::Widget> = Vec::new();
    let rail = |text: &str| {
        let t = label(text, &["cal-time"]);
        t.set_size_request(50, -1);
        t.set_valign(gtk::Align::Start);
        t
    };
    for (i, e) in todays.iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        row.append(&rail(&cal::clock(e.start.min)));
        row.append(&block(e, state, rebuild, false));
        list.append(&row);
        animated.push(row.upcast());
        if let Some(next) = todays.get(i + 1)
            && next.start > e.end()
            && e.end().date == d
        {
            let gap = gtk::Box::new(gtk::Orientation::Horizontal, 14);
            gap.append(&rail(&cal::clock(e.end().min)));
            let line = gtk::Box::new(gtk::Orientation::Vertical, 0);
            line.add_css_class("cal-rule");
            line.set_hexpand(true);
            line.set_valign(gtk::Align::Start);
            line.set_margin_top(14);
            gap.append(&line);
            list.append(&gap);
        }
    }
    if todays.is_empty() {
        list.append(&label("nothing planned", &["cal-empty"]));
    }
    let add_row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    add_row.set_margin_top(10);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_size_request(50, -1);
    add_row.append(&spacer);
    let entry = add_field(state, rebuild, note);
    entry.set_hexpand(true);
    add_row.append(&entry);
    list.append(&add_row);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .vexpand(true)
        .build();
    animate(&animated);
    let right = gtk::Box::new(gtk::Orientation::Vertical, 6);
    right.append(&scroll);
    right.append(note);
    root.append(&right);
    root
}

fn week_view(s: &State, state: &Shared, rebuild: &Rc<dyn Fn()>, go_day: &Rc<dyn Fn(Date)>) -> gtk::Widget {
    let grid = gtk::Grid::new();
    grid.add_css_class("cal-week");
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(12);
    let today = now().date;
    let start = s.selected.week_start();
    let mut animated: Vec<gtk::Widget> = Vec::new();
    for i in 0..7 {
        let d = start.plus(i);
        let col = gtk::Box::new(gtk::Orientation::Vertical, 5);
        let head = gtk::Button::new();
        head.add_css_class("cal-head");
        if d == today {
            head.add_css_class("today");
        }
        let hb = gtk::Box::new(gtk::Orientation::Vertical, 0);
        hb.append(&label(&d.weekday_name()[..3], &["cal-head-day"]));
        hb.append(&label(&d.d.to_string(), &["cal-head-num"]));
        head.set_child(Some(&hb));
        let go = go_day.clone();
        head.connect_clicked(move |_| go(d));
        col.append(&head);
        for e in cal::on(&s.events, d) {
            let b = block(e, state, rebuild, true);
            animated.push(b.clone());
            col.append(&b);
        }
        col.set_valign(gtk::Align::Start);
        grid.attach(&col, i as i32, 0, 1, 1);
    }
    animate(&animated);
    gtk::ScrolledWindow::builder().child(&grid).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build().upcast()
}

/// The cells of a month: weeks of seven dates, from the Monday on or before the 1st to the
/// Sunday on or after the last day.
pub fn month_cells(d: Date) -> Vec<Date> {
    let first = Date::new(d.y, d.m, 1);
    let last = Date::new(d.y, d.m, Date::days_in_month(d.y, d.m));
    let start = first.week_start();
    let weeks = (last.days() - start.days()) / 7 + 1;
    (0..weeks * 7).map(|i| start.plus(i)).collect()
}

fn month_view(s: &State, go_day: &Rc<dyn Fn(Date)>) -> gtk::Box {
    let board = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.add_css_class("cal-tile");
    root.set_hexpand(true);
    board.append(&root);
    board.append(&side_tiles(s));
    let title = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let name = s.selected.month_name();
    title.append(&label(&(name[..1].to_uppercase() + &name[1..]), &["cal-month-title"]));
    let year = label(&s.selected.y.to_string(), &["cal-year"]);
    year.set_valign(gtk::Align::End);
    year.set_margin_bottom(6);
    title.append(&year);
    root.append(&title);
    let grid = gtk::Grid::new();
    grid.set_column_homogeneous(true);
    grid.set_row_homogeneous(true);
    grid.set_column_spacing(5);
    grid.set_row_spacing(5);
    grid.set_vexpand(true);
    let today = now().date;
    let dow = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    dow.set_homogeneous(true);
    for name in ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] {
        dow.append(&label(name, &["cal-dow"]));
    }
    root.append(&dow);
    let mut animated: Vec<gtk::Widget> = Vec::new();
    for (i, d) in month_cells(s.selected).into_iter().enumerate() {
        let cell = gtk::Button::new();
        cell.add_css_class("cal-cell");
        if d.m != s.selected.m {
            cell.add_css_class("other");
        }
        if d == today {
            cell.add_css_class("today");
        }
        if d == s.selected {
            cell.add_css_class("picked");
        }
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 2);
        inner.append(&label(&d.d.to_string(), &["cal-cell-num"]));
        let evs = cal::on(&s.events, d);
        for e in evs.iter().take(3) {
            let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
            bar.add_css_class("cal-bar");
            bar.add_css_class(&format!("c{}", &PALETTE[e.colour()][1..]));
            bar.set_size_request(8, 8);
            bar.set_halign(gtk::Align::Start);
            inner.append(&bar);
        }
        if evs.len() > 3 {
            inner.append(&label(&format!("+{}", evs.len() - 3), &["cal-more"]));
        }
        cell.set_child(Some(&inner));
        let go = go_day.clone();
        cell.connect_clicked(move |_| go(d));
        grid.attach(&cell, (i % 7) as i32, (i / 7) as i32, 1, 1);
        animated.push(cell.upcast());
    }
    animate(&animated);
    root.append(&grid);
    board
}

fn short_day(d: Date) -> String {
    let w = d.weekday_name();
    format!("{}{}", w[..1].to_uppercase(), &w[1..3])
}

/// The amber TODAY tile and the UPCOMING tile beside the month.
fn side_tiles(s: &State) -> gtk::Box {
    let side = gtk::Box::new(gtk::Orientation::Vertical, 12);
    side.set_size_request(250, -1);
    let today = now().date;
    let tile = gtk::Box::new(gtk::Orientation::Vertical, 6);
    tile.add_css_class("cal-tile");
    tile.add_css_class("today");
    tile.append(&label("TODAY", &["cal-k"]));
    tile.append(&label(&format!("{} {}", short_day(today), today.d), &["cal-today-num"]));
    let todays = cal::on(&s.events, today);
    for e in todays.iter().take(5) {
        let l = label(&format!("{} {}", cal::clock(e.start.min), e.title), &["cal-line"]);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        tile.append(&l);
    }
    if todays.is_empty() {
        tile.append(&label("Nothing planned", &["cal-nothing"]));
    }
    side.append(&tile);
    let up = gtk::Box::new(gtk::Orientation::Vertical, 6);
    up.add_css_class("cal-tile");
    up.set_vexpand(true);
    up.append(&label("UPCOMING", &["cal-k"]));
    let mut later: Vec<&Event> = s.events.iter().filter(|e| e.start.date.days() > today.days()).collect();
    later.sort_by_key(|e| e.start);
    for e in later.iter().take(5) {
        let l = label(&format!("{}  {} {}", short_day(e.start.date), e.title, cal::clock(e.start.min)), &["cal-line"]);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        up.append(&l);
    }
    if later.is_empty() {
        up.append(&label("Nothing coming up", &["cal-nothing"]));
    }
    side.append(&up);
    side
}

fn build(app: &gtk::Application) -> gtk::ApplicationWindow {
    crate::common::load_css();
    crate::hub::load_css();
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Calendar")
        .default_width(1000)
        .default_height(625)
        .build();
    window.add_css_class("tripppin");
    window.add_css_class("calendar-app");
    window.set_size_request(640, 420);

    let t = now();
    let state: Shared = Rc::new(RefCell::new(State { events: cal::load_from(&cal::dir()), selected: t.date, view: 2 }));
    let note = label("", &["cal-note"]);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(if std::env::var_os("TRIPPPIN_SHOT").is_some() { 0 } else { 220 });
    stack.set_vexpand(true);
    let holders: Vec<gtk::Box> = VIEWS.iter().map(|_| gtk::Box::new(gtk::Orientation::Vertical, 0)).collect();
    for (name, h) in VIEWS.iter().zip(&holders) {
        h.set_vexpand(true);
        stack.add_named(h, Some(name));
    }

    // Filling the open view; `rebuild` is handed to everything that changes the data.
    let rebuild_cell: Rc<RefCell<Option<Redo>>> = Rc::new(RefCell::new(None));
    let rebuild: Rc<dyn Fn()> = {
        let cell = rebuild_cell.clone();
        Rc::new(move || {
            if let Some(f) = cell.borrow().clone() {
                f();
            }
        })
    };
    let go_day: Rc<dyn Fn(Date)> = {
        let (state, rebuild, cell) = (state.clone(), rebuild.clone(), rebuild_cell.clone());
        let _ = cell;
        Rc::new(move |d| {
            {
                let mut s = state.borrow_mut();
                s.selected = d;
                s.view = 0;
            }
            rebuild();
        })
    };

    let buttons: Rc<Vec<gtk::Button>> = Rc::new(
        VIEWS
            .iter()
            .map(|v| {
                let b = gtk::Button::new();
                b.add_css_class("cal-pivot");
                b.set_child(Some(&label(v, &[])));
                b
            })
            .collect(),
    );
    let fill = {
        let (state, holders, stack, buttons) = (state.clone(), holders.clone(), stack.clone(), buttons.clone());
        let (rebuild, go_day, note) = (rebuild.clone(), go_day.clone(), note.clone());
        move || {
            let view = state.borrow().view;
            let holder = &holders[view];
            clear(holder);
            // The note belongs to the day view only; take it out of any old parent first.
            if let Some(p) = note.parent().and_then(|p| p.downcast::<gtk::Box>().ok()) {
                p.remove(&note);
            }
            {
                let s = state.borrow();
                match view {
                    0 => holder.append(&day_view(&s, &state, &rebuild, &note)),
                    1 => holder.append(&week_view(&s, &state, &rebuild, &go_day)),
                    _ => holder.append(&month_view(&s, &go_day)),
                }
            }
            stack.set_visible_child_name(VIEWS[view]);
            for (i, b) in buttons.iter().enumerate() {
                if i == view {
                    b.add_css_class("on");
                } else {
                    b.remove_css_class("on");
                }
            }
        }
    };
    *rebuild_cell.borrow_mut() = Some(Rc::new(fill));
    for (i, b) in buttons.iter().enumerate() {
        let (state, rebuild) = (state.clone(), rebuild.clone());
        b.connect_clicked(move |_| {
            state.borrow_mut().view = i;
            rebuild();
        });
    }

    // Header: the label, and ‹ today › on the right.
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let app_label = label("Calendar", &["hub-app"]);
    app_label.set_hexpand(true);
    top.append(&app_label);
    let shift: Rc<dyn Fn(i32)> = {
        let (state, rebuild) = (state.clone(), rebuild.clone());
        Rc::new(move |by| {
            {
                let mut s = state.borrow_mut();
                s.selected = match s.view {
                    0 => s.selected.plus(i64::from(by)),
                    1 => s.selected.plus(7 * i64::from(by)),
                    _ => s.selected.add_months(by),
                };
            }
            rebuild();
        })
    };
    for (text, by) in [("\u{2039}", -1), ("today", 0), ("\u{203a}", 1)] {
        let b = gtk::Button::with_label(text);
        b.add_css_class("cal-nav");
        let (shift, state, rebuild) = (shift.clone(), state.clone(), rebuild.clone());
        b.connect_clicked(move |_| {
            if by == 0 {
                state.borrow_mut().selected = now().date;
                rebuild();
            } else {
                shift(by);
            }
        });
        top.append(&b);
    }
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.add_css_class("cal-pivots");
    for b in buttons.iter() {
        bar.append(b);
    }

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.set_margin_start(28);
    body.set_margin_end(28);
    body.set_margin_bottom(26);
    body.append(&top);
    body.append(&bar);
    let gap = gtk::Box::new(gtk::Orientation::Vertical, 0);
    gap.set_size_request(-1, 14);
    body.append(&gap);
    body.append(&stack);

    let controls = gtk::WindowControls::new(gtk::PackType::End);
    let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    titlebar.add_css_class("titlebar-row");
    let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    space.set_hexpand(true);
    titlebar.append(&space);
    titlebar.append(&controls);
    let handle = gtk::WindowHandle::new();
    handle.set_child(Some(&titlebar));
    window.set_titlebar(Some(&handle));
    window.set_child(Some(&body));

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (state, rebuild, shift) = (state.clone(), rebuild.clone(), shift.clone());
        let window = window.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            use gtk::gdk::Key;
            if gtk::prelude::GtkWindowExt::focus(&window).is_some_and(|w| w.is::<gtk::Text>()) {
                return glib::Propagation::Proceed;
            }
            let view = match key {
                Key::d => Some(0),
                Key::w => Some(1),
                Key::m => Some(2),
                _ => None,
            };
            match key {
                Key::Left => shift(-1),
                Key::Right => shift(1),
                _ if view.is_some() => {
                    state.borrow_mut().view = view.unwrap_or(0);
                    rebuild();
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
    }
    window.add_controller(keys);

    rebuild();
    gtk::prelude::GtkWindowExt::set_focus(&window, Some(&buttons[0]));

    // Reminders while the app is open (the user timer does the same when it isn't), and a
    // fresh look at the folder now and then, in case the AI or another program added events.
    {
        let (state, rebuild, ticks) = (state.clone(), rebuild.clone(), Rc::new(Cell::new(0u32)));
        glib::timeout_add_seconds_local(30, move || {
            cal::remind(now());
            ticks.set(ticks.get() + 1);
            if ticks.get() % 2 == 0 {
                let fresh = cal::load_from(&cal::dir());
                if fresh != state.borrow().events {
                    state.borrow_mut().events = fresh;
                    rebuild();
                }
            }
            glib::ControlFlow::Continue
        });
    }
    if let Ok(spec) = std::env::var("TRIPPPIN_CAL") {
        // development: "view,YYYY-MM-DD" to open on another day or view
        let mut it = spec.split(',');
        let view = it.next().and_then(|v| VIEWS.iter().position(|n| *n == v)).unwrap_or(0);
        let day = it.next().and_then(|d| {
            let p: Vec<u32> = d.split('-').filter_map(|x| x.parse().ok()).collect();
            (p.len() == 3).then(|| Date::new(p[0] as i32, p[1], p[2]))
        });
        {
            let mut s = state.borrow_mut();
            s.view = view;
            if let Some(d) = day {
                s.selected = d;
            }
        }
        rebuild();
    }
    crate::common::snapshot_hook(&window);
    window
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_month_is_whole_weeks_from_monday() {
        // November 2026 starts on a Sunday and ends on a Monday: six weeks.
        let cells = month_cells(Date::new(2026, 11, 25));
        assert_eq!(cells.len(), 42);
        assert_eq!(cells[0], Date::new(2026, 10, 26));
        assert_eq!(cells[0].weekday(), 0);
        assert_eq!(cells[41], Date::new(2026, 12, 6));
        // February 2027 starts on a Monday and is exactly four weeks.
        assert_eq!(month_cells(Date::new(2027, 2, 10)).len(), 28);
    }
}
