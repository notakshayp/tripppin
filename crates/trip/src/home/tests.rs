use super::ask::{Item, Pending, Status, ToolRow};
use super::*;
use crate::model::PlanStep;
use crate::tools::Confirm;
use crossterm::event::{KeyEvent, MouseEvent};

fn app() -> App {
    let (tx, _rx) = channel();
    let mut a = App::new(tx, (120, 34));
    a.boot = None;
    a.now = crate::timex::from_unix(crate::timex::days_from_civil(2026, 11, 26) * 86400 + 21 * 3600 + 14 * 60);
    a.snap = Snapshot {
        battery: Some((82, false)),
        battery_hours: Some(4.0),
        cpu_percent: 12,
        mem_used_kib: 3_250_000,
        mem_total_kib: 8_000_000,
        disk_used: 1_200_000_000,
        disk_total: 64_000_000_000,
        load: String::new(),
    };
    a.wifi = Some("home-5g".into());
    a.home.events = vec![
        widgets::Event { time: "10:00".into(), title: "design review".into() },
        widgets::Event { time: "19:00".into(), title: "gym".into() },
    ];
    a.widgets = vec![Widget { name: "weather".into(), command: "echo".into(), every_secs: 600 }];
    a.runner.states.insert(
        "weather".into(),
        widgets::State {
            text: "24°  clear\nbengaluru".into(),
            updated: None,
            updated_at: "21:00".into(),
            running: false,
            last_start: None,
        },
    );
    a.home.order = vec!["clock".into(), "system".into(), "today".into(), "w:weather".into(), "playing".into()];
    a.home.hidden.clear();
    a.playing = Some(Playing {
        title: "slow morning".into(),
        artist: "lo-fi mix".into(),
        playing: true,
        position: 134.0,
        length: 220.0,
    });
    // Open every pane instantly.
    let long_ago = Instant::now() - Duration::from_secs(5);
    for k in ["clock", "system", "today", "playing", "w:weather"] {
        a.home.born.insert(k.to_string(), long_ago);
    }
    a
}

fn mouse(kind: MouseEventKind, x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE })
}

fn click(a: &mut App, x: u16, y: u16) {
    a.render(120, 34);
    a.on_msg(Msg::Term(mouse(MouseEventKind::Down(MouseButton::Left), x, y)));
    a.render(120, 34);
    a.on_msg(Msg::Term(mouse(MouseEventKind::Up(MouseButton::Left), x, y)));
}

fn center_of(a: &App, act: &Act) -> (u16, u16) {
    let h = a.hits.iter().rev().find(|h| &h.act == act).unwrap_or_else(|| panic!("no hit for {act:?}"));
    (h.rect.x + h.rect.width / 2, h.rect.y)
}

fn text(a: &mut App) -> String {
    let b = a.render(120, 34);
    buffer_text(&b)
}

fn dump(name: &str, body: &str) {
    if let Some(dir) = std::env::var_os("TRIP_DUMP_DIR") {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(std::path::Path::new(&dir).join(name), body);
    }
}

#[test]
fn home_screen_has_the_design_parts() {
    let mut a = app();
    let t = text(&mut a);
    dump("home.txt", &t);
    for want in [
        "1 home",
        "2 ask",
        "3 files",
        "┤ now ├",
        "┤ system ├",
        "┤ today ├",
        "┤ weather · made by ai ├",
        "┤ playing ├",
        "[×]",
        "thursday 26 november 2026",
        "battery",
        "wi-fi   ",
        "home-5g",
        "cpu",
        "memory",
        "model",
        "design review",
        "gym",
        "24°  clear",
        "slow morning",
        "✦ ask ›",
        "[ ● mic ]",
        "21:14",
        "[+] add",
    ] {
        assert!(t.contains(want), "missing {want:?} in\n{t}");
    }
    // The logo is braille dots.
    assert!(t.chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)));
    // The pixel clock is blocks.
    assert!(t.contains("████"));
    // Idle home screen shows the model in use on the tab bar and the status line.
    assert!(t.lines().next().unwrap().contains("local") || t.lines().next().unwrap().contains("cloud"));
}

#[test]
fn clicking_tabs_and_closing_panes_works() {
    let mut a = app();
    a.render(120, 34);
    let (x, y) = center_of(&a, &Act::View(View::Ask));
    click(&mut a, x, y);
    assert_eq!(a.view, View::Ask);
    let (x, y) = center_of(&a, &Act::View(View::Home));
    click(&mut a, x, y);
    assert_eq!(a.view, View::Home);
    a.render(120, 34);
    let (x, y) = center_of(&a, &Act::Close(Pane::Today));
    click(&mut a, x, y);
    let t = text(&mut a);
    assert!(!t.contains("┤ today ├"));
    assert!(a.home.hidden.contains(&"today".to_string()));
    // [+] reopens it from the chooser.
    a.render(120, 34);
    let (x, y) = center_of(&a, &Act::AddPane);
    click(&mut a, x, y);
    assert!(text(&mut a).contains("show today"));
    let (x, y) = center_of(&a, &Act::PickPane(0));
    click(&mut a, x, y);
    a.home.born.insert("today".into(), Instant::now() - Duration::from_secs(5));
    assert!(text(&mut a).contains("┤ today ├"));
}

#[test]
fn hover_and_press_change_the_look() {
    let mut a = app();
    let before = a.render(120, 34);
    let (x, y) = center_of(&a, &Act::Mic);
    a.on_msg(Msg::Term(mouse(MouseEventKind::Moved, x, y)));
    assert_eq!(a.hover, Some(Act::Mic));
    assert!(a.dirty);
    let hovered = a.render(120, 34);
    assert_ne!(before[(x, y)].style(), hovered[(x, y)].style());
    a.on_msg(Msg::Term(mouse(MouseEventKind::Down(MouseButton::Left), x, y)));
    let pressed = a.render(120, 34);
    assert_ne!(hovered[(x, y)].style(), pressed[(x, y)].style());
}

#[test]
fn dragging_a_title_onto_another_pane_swaps_them() {
    let mut a = app();
    a.render(120, 34);
    let first = a.home.order[0].clone();
    let second = a.home.order[1].clone();
    let (tx, ty) = center_of(&a, &Act::Title(Pane::Clock));
    let (_, r) = a.home.pane_rects.iter().find(|(p, _)| *p == Pane::System).copied().unwrap();
    a.on_msg(Msg::Term(mouse(MouseEventKind::Down(MouseButton::Left), tx, ty)));
    a.on_msg(Msg::Term(mouse(MouseEventKind::Drag(MouseButton::Left), r.x + 5, r.y + 3)));
    assert_eq!(a.hover_pane, Some(Pane::System));
    a.on_msg(Msg::Term(mouse(MouseEventKind::Up(MouseButton::Left), r.x + 5, r.y + 3)));
    assert_eq!(a.home.order[0], second);
    assert_eq!(a.home.order[1], first);
}

#[test]
fn sliders_set_values_from_the_mouse() {
    let mut a = app();
    a.view = View::Settings;
    a.settings.tab = 1;
    a.settings.volume = Some((10, false));
    a.render(120, 34);
    let h = a.hits.iter().find(|h| h.act == Act::Slider(Slider::Volume)).cloned().unwrap();
    // The slider math is covered by `slider_value`; here, clicking must register a drag target.
    a.on_msg(Msg::Term(mouse(MouseEventKind::Down(MouseButton::Left), h.rect.x + h.rect.width / 2, h.rect.y)));
    assert!(a.slider.is_some());
    a.on_msg(Msg::Term(mouse(MouseEventKind::Up(MouseButton::Left), h.rect.x, h.rect.y)));
    assert!(a.slider.is_none());
}

#[test]
fn keyboard_reaches_everything() {
    let mut a = app();
    let key = |c: KeyCode, m: KeyModifiers| Msg::Term(Event::Key(KeyEvent::new(c, m)));
    a.on_msg(key(KeyCode::F(4), KeyModifiers::NONE));
    assert_eq!(a.view, View::Models);
    a.on_msg(key(KeyCode::Char('3'), KeyModifiers::ALT));
    assert_eq!(a.view, View::Files);
    a.on_msg(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(a.view, View::Home);
    for c in "hello".chars() {
        a.on_msg(key(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(a.home.input.text, "hello");
    a.on_msg(key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(a.view, View::Home, "tab only cycles when the ask bar is empty");
    a.home.input.take();
    a.on_msg(key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(a.view, View::Ask);
    a.on_msg(key(KeyCode::Char('q'), KeyModifiers::CONTROL));
    assert!(a.quit);
}

fn tool(label: &str, detail: &str, status: Status, output: &str) -> Item {
    Item::Tool(ToolRow {
        label: label.into(),
        detail: detail.into(),
        full: detail.into(),
        status,
        output: output.into(),
        expanded: false,
        started: Instant::now(),
        took: Some(Duration::from_millis(900)),
    })
}

fn working_ask(a: &mut App) {
    a.view = View::Ask;
    a.ask.running = true;
    a.ask.items = vec![
        Item::You(
            "set up a python project for scraping prices from that store and tell me which library is best".into(),
        ),
        tool(
            "web.search",
            "\"python web scraping library 2026 comparison\"",
            Status::Ok,
            "1. httpx + selectolax\n2. playwright\n3. scrapy",
        ),
        tool("web.read", "playwright.dev/python/docs/intro", Status::Ok, "Playwright for Python\nInstallation"),
        tool("pkg.add", "python3 py3-pip", Status::Waiting, ""),
    ];
    a.ask.plan = vec![
        PlanStep { text: "find the best scraping library".into(), done: true },
        PlanStep { text: "check python is installed".into(), done: true },
        PlanStep { text: "install python and the library".into(), done: false },
        PlanStep { text: "make the project folder and a starter script".into(), done: false },
    ];
    a.ask.pending = Some(Pending {
        req: Confirm {
            kind: "installs",
            title: "Install 2 packages?".into(),
            lines: vec![("packages".into(), "python3 py3-pip".into()), ("from".into(), "alpine, signed".into())],
        },
    });
}

#[test]
fn ask_view_shows_plan_tool_rows_and_the_ok_box() {
    let mut a = app();
    working_ask(&mut a);
    let t = text(&mut a);
    dump("ask.txt", &t);
    for want in [
        "you ›",
        "plan",
        "[x] find the best scraping library",
        "[ ] make the project folder",
        "▸ web.search",
        "▸ web.read",
        "pkg.add",
        "needs your ok",
        "Install 2 packages?",
        "[ allow ]",
        "[ no ]",
        "allow always for installing",
        "y / n / a",
        "context",
        "tokens",
        "tools used",
        "┤ set up a python project",
    ] {
        assert!(t.contains(want), "missing {want:?} in\n{t}");
    }
    assert!(t.contains("[~] install python"));
    // Clicking a row opens it and shows the exact query and the output.
    let (x, y) = center_of(&a, &Act::Toggle(1));
    click(&mut a, x, y);
    let t = text(&mut a);
    assert!(t.contains("▾ web.search"));
    assert!(t.contains("python web scraping library 2026 comparison"));
    assert!(t.contains("│ 2. playwright"));
}

#[test]
fn the_ok_box_buttons_answer_the_agent() {
    let mut a = app();
    working_ask(&mut a);
    let (tx, rx) = channel();
    a.ask.reply = Some(tx);
    a.render(120, 34);
    let (x, y) = center_of(&a, &Act::Allow);
    click(&mut a, x, y);
    assert_eq!(rx.try_recv(), Ok(Decision::Allow));
    assert!(a.ask.pending.is_none());
    working_ask(&mut a);
    a.ask.reply = Some(channel().0);
    let (tx, rx) = channel();
    a.ask.reply = Some(tx);
    a.on_msg(Msg::Term(Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))));
    assert_eq!(rx.try_recv(), Ok(Decision::Deny));
}

#[test]
fn agent_events_build_the_transcript() {
    let mut a = app();
    a.ask.running = true;
    a.ask_event(ask::Ev::Text("looking".into()));
    a.ask_event(ask::Ev::Step { label: "web.search".into(), detail: "\"x\"".into(), full: "\"x\"".into() });
    a.ask_event(ask::Ev::Result("1. a\n2. b".into()));
    a.ask_event(ask::Ev::StepDone(true));
    a.ask_event(ask::Ev::Confirm {
        label: "pkg.add".into(),
        detail: "git".into(),
        req: Confirm { kind: "installs", title: "Install git?".into(), lines: vec![] },
    });
    // The step that follows an approval reuses the waiting row.
    a.ask_event(ask::Ev::Step { label: "exec".into(), detail: "apk add git".into(), full: "doas apk add git".into() });
    let tools: Vec<&ToolRow> =
        a.ask.items.iter().filter_map(|i| if let Item::Tool(t) = i { Some(t) } else { None }).collect();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[1].label, "pkg.add");
    assert_eq!(tools[1].status, Status::Running);
    assert_eq!(tools[1].full, "doas apk add git");
    assert_eq!(tools[0].output, "1. a\n2. b");
    assert_eq!(ask::summary_of("1. a\n2. b"), "1. a · 2 lines");
}

#[test]
fn every_view_renders_without_panicking_at_small_and_large_sizes() {
    let mut a = app();
    for (w, h) in [(120, 34), (80, 24), (60, 16), (40, 12), (200, 60)] {
        for v in View::ALL {
            a.view = v;
            let b = a.render(w, h);
            assert_eq!(b.area.width, w);
        }
    }
    a.view = View::Models;
    let t = text(&mut a);
    dump("models.txt", &t);
    assert!(t.contains("┤ models ├") && t.contains("good for") && t.contains("cloud (optional"));
    a.view = View::Settings;
    for tab in 0..settings::TABS.len() {
        a.settings.tab = tab;
        let t = text(&mut a);
        assert!(t.contains("┤ settings ├") && t.contains("network"));
        if tab == 0 {
            dump("settings.txt", &t);
        }
    }
    a.view = View::Packages;
    a.pkgs.results = vec![
        Pkg {
            name: "firefox".into(),
            version: "1".into(),
            desc: "web browser".into(),
            installed: true,
            gui: true,
            size: Some(82_000_000),
        },
        Pkg {
            name: "w3m".into(),
            version: "1".into(),
            desc: "browse the web as text, right here".into(),
            installed: false,
            gui: false,
            size: Some(2_000_000),
        },
    ];
    let t = text(&mut a);
    dump("packages.txt", &t);
    assert!(t.contains("▣") && t.contains("[ open ]") && t.contains("[ install ]"));
}

#[test]
fn boot_animation_draws_the_logo_then_the_name() {
    let mut a = app();
    let mut frames = Vec::new();
    for t in [0.05, 0.2, 0.5, 1.19] {
        a.boot = Some(Instant::now() - Duration::from_secs_f64(t));
        let b = a.render(80, 30);
        frames.push(buffer_text(&b));
    }
    dump("boot.txt", &frames.join("\n=====\n"));
    assert!(!frames[0].contains("t r i"));
    assert!(frames[3].contains("t r i p p p i n"));
    let dots = |s: &str| s.chars().filter(|c| ('\u{2801}'..='\u{28ff}').contains(c)).count();
    assert!(dots(&frames[0]) < dots(&frames[3]));
    // A key press skips it.
    a.boot = Some(Instant::now());
    a.on_msg(Msg::Term(Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))));
    assert!(a.boot.is_none());
}

#[test]
fn idle_loop_sleeps_until_the_next_minute() {
    let mut a = app();
    a.home.born.clear();
    for k in ["clock", "system", "today", "playing", "w:weather"] {
        a.home.born.insert(k.to_string(), Instant::now() - Duration::from_secs(5));
    }
    a.playing = None;
    a.widgets.clear();
    assert!(a.animating().is_none());
    assert!(a.next_wake() > Duration::from_secs(1));
    a.ask.running = true;
    assert_eq!(a.animating(), Some(Duration::from_millis(125)));
    // Out of focus, nothing animates.
    a.focused.store(false, Ordering::Relaxed);
    assert!(a.animating().is_none());
}
