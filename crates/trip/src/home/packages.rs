//! The packages view (apk search with plain-word descriptions, install and remove behind the
//! permission box, apps with a window marked ▣ and opened full screen) and a small files view.

use super::input::Input;
use super::ui::*;
use super::{App, Msg};
use crate::apk::{self, Pkg};
use crate::tools::{self, Confirm};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use serde_json::json;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    Install(String),
    Remove(String),
}

impl Job {
    pub fn name(&self) -> &str {
        match self {
            Job::Install(n) | Job::Remove(n) => n,
        }
    }
}

pub struct Modal {
    pub req: Confirm,
    pub job: Job,
}

#[derive(Default)]
pub struct PkgState {
    pub input: Input,
    pub results: Vec<Pkg>,
    pub selected: usize,
    pub searching: bool,
    pub serial: u64,
    pub running: Option<Job>,
    pub message: String,
    pub focus_search: bool,
    pub searched: String,
}

#[derive(Default)]
pub struct FilesState {
    pub dir: PathBuf,
    pub entries: Vec<(String, bool, u64)>,
    pub selected: usize,
    pub preview: Vec<String>,
}

impl App {
    pub fn pkgs_open(&mut self) {
        self.pkgs.focus_search = true;
    }

    pub fn pkg_search(&mut self) {
        let q = self.pkgs.input.text.trim().to_string();
        if q.is_empty() || self.pkgs.searching {
            return;
        }
        self.pkgs.searching = true;
        self.pkgs.serial += 1;
        self.pkgs.searched = q.clone();
        let (tx, serial) = (self.tx.clone(), self.pkgs.serial);
        std::thread::spawn(move || {
            let mut r = apk::search(&q);
            for p in r.iter_mut().take(14) {
                apk::fill_size(p);
            }
            let _ = tx.send(Msg::Pkgs(serial, r));
        });
        self.dirty = true;
    }

    pub fn pkgs_done(&mut self, r: Vec<Pkg>) {
        self.pkgs.searching = false;
        self.pkgs.selected = 0;
        self.pkgs.message =
            if r.is_empty() { format!("nothing found for \"{}\"", self.pkgs.searched) } else { String::new() };
        self.pkgs.results = r;
        self.dirty = true;
    }

    /// Install or remove: asks first (unless "allow always" was chosen), then runs.
    pub fn pkg_go(&mut self, i: usize) {
        let Some(p) = self.pkgs.results.get(i).cloned() else { return };
        if self.pkgs.running.is_some() {
            return;
        }
        let (tool, job) = if p.installed {
            ("pkg_remove", Job::Remove(p.name.clone()))
        } else {
            ("pkg_add", Job::Install(p.name.clone()))
        };
        match tools::needs_ok(tool, &json!({"names": p.name})) {
            Some(req) => self.modal = Some(Modal { req, job }),
            None => self.pkg_run(job),
        }
        self.dirty = true;
    }

    pub fn pkg_run(&mut self, job: Job) {
        let cmd = match &job {
            Job::Install(n) => apk::add_command(std::slice::from_ref(n)),
            Job::Remove(n) => apk::del_command(std::slice::from_ref(n)),
        };
        self.pkgs.running = Some(job.clone());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let out = Command::new("sh").arg("-c").arg(format!("{cmd} 2>&1")).output();
            let (ok, text) = match out {
                Ok(o) => {
                    (o.status.success(), String::from_utf8_lossy(&o.stdout).lines().last().unwrap_or("").to_string())
                }
                Err(e) => (false, e.to_string()),
            };
            let _ = tx.send(Msg::PkgJob(job, ok, text));
        });
    }

    pub fn pkg_job_done(&mut self, job: Job, ok: bool, text: String) {
        self.pkgs.running = None;
        let have = apk::installed_names();
        for p in &mut self.pkgs.results {
            p.installed = have.contains(&p.name);
        }
        self.pkgs.message = match (ok, &job) {
            (true, Job::Install(n)) => format!("{n} installed; no reboot needed"),
            (true, Job::Remove(n)) => format!("{n} removed"),
            (false, _) => format!("didn't work: {text}"),
        };
        self.dirty = true;
    }

    pub fn pkg_open(&mut self, i: usize) {
        let Some(p) = self.pkgs.results.get(i) else { return };
        let cmd = apk::desktop_exec(&p.name).unwrap_or_else(|| p.name.clone());
        launch(&cmd);
        self.pkgs.message = format!("opening {}; super+esc comes back here", p.name);
        self.dirty = true;
    }

    pub fn pkgs_key(&mut self, k: KeyEvent) {
        let n = self.pkgs.results.len();
        match k.code {
            KeyCode::Esc => self.pkgs.focus_search = false,
            KeyCode::Enter if self.pkgs.focus_search => self.pkg_search(),
            KeyCode::Down | KeyCode::Tab => {
                self.pkgs.focus_search = false;
                self.pkgs.selected = (self.pkgs.selected + 1).min(n.saturating_sub(1));
            }
            KeyCode::Up | KeyCode::BackTab => {
                if self.pkgs.selected == 0 {
                    self.pkgs.focus_search = true;
                }
                self.pkgs.selected = self.pkgs.selected.saturating_sub(1);
            }
            KeyCode::Char('/') if !self.pkgs.focus_search => self.pkgs.focus_search = true,
            KeyCode::Enter => {
                let i = self.pkgs.selected;
                match self.pkgs.results.get(i) {
                    Some(p) if p.installed && p.gui => self.pkg_open(i),
                    Some(_) => self.pkg_go(i),
                    None => {}
                }
            }
            KeyCode::Char('r') if !self.pkgs.focus_search => {
                let i = self.pkgs.selected;
                if self.pkgs.results.get(i).is_some_and(|p| p.installed) {
                    self.pkg_go(i);
                }
            }
            _ if self.pkgs.focus_search => {
                self.pkgs.input.key(k);
            }
            _ => {}
        }
        self.dirty = true;
    }

    pub fn draw_packages(&mut self, p: &mut Painter, area: Rect) {
        let area = Rect::new(area.x + 2.min(area.width / 8), area.y, area.width - 4.min(area.width / 4), area.height);
        let Some(inner) = p.pane(area, "packages", true, None, Some(Act::View(View::Home)), None, 1.0) else { return };
        let ms = self.started.elapsed().as_millis();
        p.text(inner.x, inner.y, "/ ", fg(GREEN));
        let q = self.pkgs.input.shown();
        if q.is_empty() && !self.pkgs.focus_search {
            p.text(inner.x + 2, inner.y, "search for an app or tool, then press enter", fg(MUTED));
        } else {
            p.text(inner.x + 2, inner.y, &q, fg(BRIGHT));
        }
        p.hit(Rect::new(inner.x, inner.y, inner.width, 1), Act::PkgFocusSearch);
        if self.pkgs.focus_search {
            self.cursor = Some((inner.x + 2 + self.pkgs.input.cursor as u16, inner.y));
        }
        if self.pkgs.searching {
            p.text(inner.x + inner.width - 12, inner.y, &format!("{} searching", spinner(ms)), fg(AMBER));
        }
        let rows = (inner.height.saturating_sub(5)) as usize;
        let list_y = inner.y + 2;
        p.scroll_area(Rect::new(inner.x, list_y, inner.width, rows as u16), ScrollId::Packages);
        let off = self.pkgs.selected.saturating_sub(rows.saturating_sub(1));
        let desc_w = inner.width.saturating_sub(2 + 22 + 8 + 14) as usize;
        for (row, (i, pk)) in self.pkgs.results.iter().enumerate().skip(off).take(rows).enumerate() {
            let y = list_y + row as u16;
            p.row(Rect::new(inner.x, y, inner.width, 1), Act::PkgRow(i), i == self.pkgs.selected);
            p.text(inner.x, y, if pk.gui { "▣" } else { " " }, fg(BLUE));
            p.text(inner.x + 2, y, &pk.name.chars().take(20).collect::<String>(), fg(BRIGHT));
            p.text(inner.x + 24, y, &pk.desc.chars().take(desc_w).collect::<String>(), fg(MUTED));
            p.text(inner.x + inner.width - 22, y, &pk.size.map(apk::size_words).unwrap_or_default(), fg(TEXT));
            let busy = self.pkgs.running.as_ref().is_some_and(|j| j.name() == pk.name);
            let (label, act) = if busy {
                (format!("{} working", spinner(ms)), Act::Noop)
            } else if pk.installed && pk.gui {
                ("open".to_string(), Act::PkgOpen(i))
            } else if pk.installed {
                ("remove".to_string(), Act::PkgGo(i))
            } else {
                ("install".to_string(), Act::PkgGo(i))
            };
            let w = label.chars().count() as u16 + 4;
            p.button(inner.x + inner.width - w, y, &label, act, pk.installed && pk.gui);
        }
        let by = inner.y + inner.height - 3;
        if !self.pkgs.message.is_empty() {
            p.text(inner.x, by, &self.pkgs.message, fg(AMBER));
        }
        p.text(inner.x, by + 1, "▣ opens as a full-screen app · installs never need a reboot", fg(MUTED));
        let hint = "/ search · ↑↓ choose · enter install or open · r remove";
        p.text(inner.x + inner.width.saturating_sub(hint.chars().count() as u16), by + 2, hint, fg(MUTED));
    }

    // ---- files -----------------------------------------------------------------------

    pub fn files_open(&mut self) {
        if self.files.dir.as_os_str().is_empty() {
            self.files.dir = crate::paths::expand("~");
        }
        self.files_load();
    }

    pub fn files_load(&mut self) {
        let mut rows: Vec<(String, bool, u64)> = std::fs::read_dir(&self.files.dir)
            .map(|d| {
                d.flatten()
                    .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                    .map(|e| {
                        let m = e.metadata().ok();
                        (
                            e.file_name().to_string_lossy().into_owned(),
                            m.as_ref().is_some_and(|m| m.is_dir()),
                            m.map_or(0, |m| m.len()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        rows.sort_by_key(|a| (!a.1, a.0.to_lowercase()));
        self.files.entries = rows;
        self.files.selected = 0;
        self.files_preview();
        self.dirty = true;
    }

    fn files_preview(&mut self) {
        self.files.preview.clear();
        let Some((name, is_dir, size)) = self.files.entries.get(self.files.selected).cloned() else { return };
        if is_dir || size > 64 * 1024 {
            return;
        }
        if let Ok(t) = std::fs::read_to_string(self.files.dir.join(name)) {
            self.files.preview = t.lines().take(60).map(|l| l.chars().take(200).collect()).collect();
        }
    }

    pub fn file_enter(&mut self, i: usize) {
        let Some((name, is_dir, _)) = self.files.entries.get(i).cloned() else { return };
        if is_dir {
            self.files.dir.push(name);
            self.files_load();
        } else {
            self.files.selected = i;
            self.files_preview();
        }
        self.dirty = true;
    }

    pub fn files_up(&mut self) {
        if self.files.dir.pop() {
            self.files_load();
        }
    }

    pub fn files_key(&mut self, k: KeyEvent) {
        let n = self.files.entries.len();
        match k.code {
            KeyCode::Up => self.files.selected = self.files.selected.saturating_sub(1),
            KeyCode::Down => self.files.selected = (self.files.selected + 1).min(n.saturating_sub(1)),
            KeyCode::Enter | KeyCode::Right => return self.file_enter(self.files.selected),
            KeyCode::Backspace | KeyCode::Left => return self.files_up(),
            _ => {}
        }
        self.files_preview();
        self.dirty = true;
    }

    pub fn draw_files(&mut self, p: &mut Painter, area: Rect) {
        let split = area.width * 5 / 9;
        let left = Rect::new(area.x, area.y, split, area.height);
        let right = Rect::new(area.x + split + 1, area.y, area.width - split - 1, area.height);
        let title = self.files.dir.display().to_string();
        if let Some(inner) = p.pane(left, &title, true, None, None, None, 1.0) {
            p.row(Rect::new(inner.x, inner.y, inner.width, 1), Act::FileRow(usize::MAX), false);
            p.text(inner.x, inner.y, "↰ ..", fg(MUTED));
            let rows = inner.height.saturating_sub(1) as usize;
            p.scroll_area(Rect::new(inner.x, inner.y + 1, inner.width, rows as u16), ScrollId::Files);
            let off = self.files.selected.saturating_sub(rows.saturating_sub(1));
            for (r, (i, (name, dir, size))) in self.files.entries.iter().enumerate().skip(off).take(rows).enumerate() {
                let y = inner.y + 1 + r as u16;
                p.row(Rect::new(inner.x, y, inner.width, 1), Act::FileRow(i), i == self.files.selected);
                p.text(
                    inner.x,
                    y,
                    &format!("{}{}", name, if *dir { "/" } else { "" }),
                    fg(if *dir { BLUE } else { TEXT }),
                );
                if !dir {
                    let s = apk::size_words(*size);
                    p.text(inner.x + inner.width - s.len() as u16, y, &s, fg(MUTED));
                }
            }
        }
        let name = self.files.entries.get(self.files.selected).map_or("preview".to_string(), |e| e.0.clone());
        if let Some(inner) = p.pane(right, &name, false, None, None, None, 1.0) {
            if self.files.preview.is_empty() {
                p.text(inner.x, inner.y, "no preview", fg(MUTED));
            }
            for (i, l) in self.files.preview.iter().take(inner.height as usize).enumerate() {
                p.text(inner.x, inner.y + i as u16, l, fg(TEXT));
            }
        }
        let _ = Modifier::BOLD;
    }
}

/// Starts a program detached from the interface (it opens as its own full-screen window).
pub fn launch(cmd: &str) {
    let script = format!("exec {cmd}");
    let started = Command::new("setsid")
        .args(["-f", "sh", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if started.is_err() {
        let _ = Command::new("sh")
            .arg("-c")
            .arg(format!("{script} &"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}
