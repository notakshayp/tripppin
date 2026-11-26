//! The models view: this machine, the catalog's models with what they're good for, size, measured
//! speed and whether they fit; one-click switch, download with a block progress bar (it
//! resumes), delete; and cloud providers with key entry.

use super::input::Input;
use super::ui::*;
use super::{App, Msg};
use crate::models::{self, LocalModel, Progress};
use crate::{model, paths, providers};
use crossterm::event::{KeyCode, KeyEvent};
use hwdetect::Hardware;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The providers offered as buttons, by id.
pub const CLOUD: [(&str, &str); 4] = [
    ("anthropic", "anthropic"),
    ("openai", "openai"),
    ("openrouter", "openrouter"),
    ("own", "any openai-compatible url"),
];

pub struct Dl {
    pub tier_id: String,
    pub name: String,
    pub done: u64,
    pub total: u64,
    pub speed: f64,
    pub checking: bool,
    pub cancel: Arc<AtomicBool>,
}

pub struct ProviderForm {
    pub idx: usize,
    pub key: Input,
    pub url: Input,
    pub field: usize,
    pub busy: bool,
    pub error: String,
}

pub enum DlEv {
    Progress(Progress),
    Done(Result<(), String>),
}

#[derive(Default)]
pub struct ModelsState {
    pub list: Vec<LocalModel>,
    pub hw: Option<Hardware>,
    pub loaded: bool,
    pub selected: usize,
    pub speeds: HashMap<String, f64>,
    pub bench_running: bool,
    pub dl: Option<Dl>,
    pub form: Option<ProviderForm>,
    pub message: String,
}

fn bench_file() -> std::path::PathBuf {
    paths::cache_dir().join("bench.json")
}

pub fn load_speeds() -> HashMap<String, f64> {
    std::fs::read_to_string(bench_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_speeds(m: &HashMap<String, f64>) {
    let f = bench_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, serde_json::to_string(m).unwrap_or_default());
}

/// Tokens per second from llama.cpp's reply (its own timings, else tokens over wall time).
pub fn parse_bench(json: &str, wall: Duration) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if let Some(t) = v.pointer("/timings/predicted_per_second").and_then(serde_json::Value::as_f64).filter(|t| *t > 0.0)
    {
        return Some(t);
    }
    let n = v.pointer("/usage/completion_tokens").and_then(serde_json::Value::as_f64)?;
    (n > 4.0 && wall.as_secs_f64() > 0.0).then(|| n / wall.as_secs_f64())
}

/// A short generation on the local server, timed. `None` when it isn't up.
pub fn bench_local() -> Option<f64> {
    if model::server_info().loading {
        return None;
    }
    let body = serde_json::json!({
        "messages": [{"role": "user", "content": "Count from 1 to 60 in words."}],
        "max_tokens": 48, "temperature": 0, "stream": false
    });
    let start = std::time::Instant::now();
    let mut r = model::http_agent()
        .post("http://127.0.0.1:8086/v1/chat/completions")
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .ok()?;
    let text = r.body_mut().read_to_string().ok()?;
    parse_bench(&text, start.elapsed())
}

/// How fast a model should run here, from the one measurement we have (speed falls with size).
pub fn estimate(speeds: &HashMap<String, f64>, list: &[LocalModel], m: &LocalModel) -> Option<(f64, bool)> {
    if let Some(s) = speeds.get(&m.tier.id) {
        return Some((*s, false));
    }
    let (id, s) = speeds.iter().find(|(id, _)| list.iter().any(|x| &x.tier.id == *id))?;
    let base = list.iter().find(|x| &x.tier.id == id)?;
    Some((s * base.tier.size_bytes as f64 / m.tier.size_bytes as f64, true))
}

fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

impl App {
    pub fn models_open(&mut self) {
        let m = &mut self.models;
        if !m.loaded {
            m.loaded = true;
            m.speeds = load_speeds();
            m.hw = hwdetect::detect().ok();
        }
        self.models_reload();
        // Measure the model in use once, the first time, and keep the answer.
        let active = self.models.list.iter().find(|x| x.active).map(|x| x.tier.id.clone());
        if let Some(id) = active
            && !self.models.speeds.contains_key(&id)
            && !self.models.bench_running
            && model::endpoint().is_none()
        {
            self.models.bench_running = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Msg::Bench(bench_local().map(|s| (id, s))));
            });
        }
    }

    pub fn models_reload(&mut self) {
        self.models.list = models::list().unwrap_or_default();
        self.models.selected = self.models.selected.min(self.models.list.len().saturating_sub(1));
        self.dirty = true;
    }

    pub fn bench_done(&mut self, r: Option<(String, f64)>) {
        self.models.bench_running = false;
        if let Some((id, s)) = r {
            self.models.speeds.insert(id, s);
            save_speeds(&self.models.speeds);
        }
        self.dirty = true;
    }

    pub fn model_use(&mut self, i: usize) {
        let Some(m) = self.models.list.get(i).cloned() else { return };
        if !m.installed {
            return self.model_download(i);
        }
        if model::endpoint().is_some() {
            let _ = model::set_endpoint(None);
        }
        self.models.message = match models::activate(&m.tier) {
            Ok(()) => format!("now using {} for the next message", m.tier.model.to_lowercase()),
            Err(e) => format!("{e:#}"),
        };
        self.models_reload();
    }

    pub fn model_download(&mut self, i: usize) {
        let Some(m) = self.models.list.get(i).cloned() else { return };
        if m.installed || self.models.dl.is_some() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.models.dl = Some(Dl {
            tier_id: m.tier.id.clone(),
            name: m.tier.model.to_lowercase(),
            done: m.partial.unwrap_or(0),
            total: m.tier.size_bytes,
            speed: 0.0,
            checking: false,
            cancel: cancel.clone(),
        });
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = models::download(&m.tier, &cancel, &mut |p| {
                let _ = tx.send(Msg::Dl(DlEv::Progress(p)));
            });
            let r = r.map_err(|e| if e.is::<models::Cancelled>() { "paused".to_string() } else { format!("{e:#}") });
            let _ = tx.send(Msg::Dl(DlEv::Done(r)));
        });
        self.dirty = true;
    }

    pub fn dl_event(&mut self, ev: DlEv) {
        match ev {
            DlEv::Progress(Progress::Downloading(done, total, speed)) => {
                if let Some(d) = &mut self.models.dl {
                    (d.done, d.total, d.speed, d.checking) = (done, total, speed, false);
                }
            }
            DlEv::Progress(Progress::Checking) => {
                if let Some(d) = &mut self.models.dl {
                    d.checking = true;
                }
            }
            DlEv::Done(r) => {
                let name = self.models.dl.take().map(|d| d.name).unwrap_or_default();
                self.models.message = match r {
                    Ok(()) => format!("{name} is ready; press enter to use it"),
                    Err(e) if e == "paused" => format!("{name} paused; download again to resume"),
                    Err(e) => e,
                };
                self.models_reload();
            }
        }
        self.dirty = true;
    }

    pub fn model_delete(&mut self, i: usize) {
        let Some(m) = self.models.list.get(i).cloned() else { return };
        if !m.installed && m.partial.is_none() {
            return;
        }
        self.models.message = match models::delete(&m.tier) {
            Ok(()) => format!("deleted {}", m.tier.model.to_lowercase()),
            Err(e) => format!("{e:#}"),
        };
        self.models_reload();
    }

    pub fn provider_open(&mut self, i: usize) {
        let (id, _) = CLOUD[i];
        let url = providers::by_id(id).map(|p| p.url).unwrap_or("");
        self.models.form = Some(ProviderForm {
            idx: i,
            key: Input::secret(),
            url: Input::new(url),
            field: usize::from(id != "own"),
            busy: false,
            error: String::new(),
        });
        self.dirty = true;
    }

    pub fn provider_save(&mut self) {
        let Some(f) = &mut self.models.form else { return };
        if f.busy {
            return;
        }
        let (id, _) = CLOUD[f.idx];
        let url = f.url.text.trim().trim_end_matches('/').to_string();
        if url.is_empty() {
            f.error = "type the server's address first".into();
            return;
        }
        f.busy = true;
        f.error.clear();
        let e = model::Endpoint { url, model: String::new(), key: f.key.text.trim().to_string() };
        let tx = self.tx.clone();
        let own = id == "own";
        std::thread::spawn(move || {
            let r = (|| {
                let list = providers::list_models(&e).map_err(|x| format!("{x:#}"))?;
                let p = providers::for_url(&e.url);
                let pick =
                    providers::default_model(p, &list).and_then(|i| list.get(i)).ok_or("the server lists no models")?;
                let e = model::Endpoint { model: pick.id.clone(), ..e };
                if own {
                    model::test_endpoint(&e).map_err(|x| format!("{x:#}"))?;
                }
                model::set_endpoint(Some(&e)).map_err(|x| format!("{x:#}"))?;
                Ok::<_, String>(e)
            })();
            let _ = tx.send(Msg::Provider(r));
        });
        self.dirty = true;
    }

    pub fn provider_done(&mut self, r: Result<model::Endpoint, String>) {
        match r {
            Ok(e) => {
                self.models.form = None;
                self.models.message = format!("now using {} (cloud) for the next message", e.model);
            }
            Err(e) => {
                if let Some(f) = &mut self.models.form {
                    f.busy = false;
                    f.error = e;
                }
            }
        }
        self.dirty = true;
    }

    pub fn models_key(&mut self, k: KeyEvent) {
        if let Some(f) = &mut self.models.form {
            match k.code {
                KeyCode::Esc => self.models.form = None,
                KeyCode::Enter => self.provider_save(),
                KeyCode::Tab | KeyCode::Down | KeyCode::Up => f.field = 1 - f.field.min(1),
                _ => {
                    let field = if f.field == 0 { &mut f.url } else { &mut f.key };
                    field.key(k);
                }
            }
            self.dirty = true;
            return;
        }
        let n = self.models.list.len();
        match k.code {
            KeyCode::Up => self.models.selected = self.models.selected.saturating_sub(1),
            KeyCode::Down => self.models.selected = (self.models.selected + 1).min(n.saturating_sub(1)),
            KeyCode::Enter => self.model_use(self.models.selected),
            KeyCode::Char('d') => self.model_download(self.models.selected),
            KeyCode::Char('x') => self.model_delete(self.models.selected),
            KeyCode::Char('p') => {
                if let Some(d) = &self.models.dl {
                    d.cancel.store(true, Ordering::Relaxed);
                }
            }
            KeyCode::Char('c') => self.provider_open(0),
            _ => {}
        }
        self.dirty = true;
    }

    pub fn draw_models(&mut self, p: &mut Painter, area: Rect) {
        let area = Rect::new(area.x + 2.min(area.width / 8), area.y, area.width - 4.min(area.width / 4), area.height);
        let Some(inner) = p.pane(area, "models", true, None, Some(Act::View(View::Home)), None, 1.0) else { return };
        let m = &self.models;
        let (label, local) = crate::models::model_label();
        let mut y = inner.y;
        let hw = m.hw.as_ref().map_or("this machine".to_string(), |hw| {
            let gpu = hw.gpus.first().map_or("no gpu".to_string(), |g| {
                format!("{} (gpu)", g.name.clone().unwrap_or_else(|| format!("{:?}", g.vendor).to_lowercase()))
            });
            format!("this machine: {} · {gpu} · {:.0} GB memory", hw.cpu.model, hw.memory_gib())
        });
        p.text(inner.x, y, &hw, fg(MUTED));
        y += 1;
        let w = p.text(inner.x, y, "in use: ", fg(MUTED));
        let w2 = p.text(inner.x + w, y, &label, fg(GREEN).add_modifier(Modifier::BOLD));
        p.text(inner.x + w + w2, y, if local { " · local, on this computer" } else { " · cloud" }, fg(MUTED));
        if !local {
            p.button(inner.x + inner.width.saturating_sub(16), y, "use local", Act::LocalAgain, false);
        }
        y += 2;
        let iw = inner.width as usize;
        let for_w = iw.saturating_sub(2 + 20 + 9 + 14 + 6 + 14).clamp(10, 40);
        let hdr = format!("  {:<20}{:<for_w$} {:<9}{:<14}{:<6}", "model", "good for", "size", "speed here", "fits");
        p.text(inner.x, y, &hdr, fg(MUTED));
        y += 1;
        let rows_h = inner.height.saturating_sub(12) as usize;
        let rows_h = rows_h.max(3);
        p.scroll_area(Rect::new(inner.x, y, inner.width, rows_h as u16), ScrollId::Models);
        let off = self.models.selected.saturating_sub(rows_h.saturating_sub(1));
        for (i, mm) in m.list.iter().enumerate().skip(off).take(rows_h) {
            let r = Rect::new(inner.x, y, inner.width, 1);
            p.row(r, Act::ModelRow(i), i == m.selected);
            let cur = if mm.active && local { "●" } else { " " };
            p.text(inner.x, y, cur, fg(GREEN));
            p.text(inner.x + 2, y, &mm.tier.model.to_lowercase(), fg(BRIGHT));
            let about = if mm.tier.about.is_empty() { mm.needs() } else { mm.tier.about.clone() };
            let about: String = about.chars().take(for_w - 1).collect();
            p.text(inner.x + 22, y, &about, fg(MUTED));
            let c1 = inner.x + 22 + for_w as u16 + 1;
            p.text(c1, y, &gb(mm.tier.size_bytes), fg(TEXT));
            let (speed, scol) = match estimate(&m.speeds, &m.list, mm) {
                _ if !mm.fits => ("too slow".to_string(), RED),
                Some((s, est)) => {
                    (format!("{}{s:.0} tok/s", if est { "~" } else { "" }), if s >= 15.0 { GREEN } else { AMBER })
                }
                None if m.bench_running && mm.active => {
                    (format!("{} measuring", spinner(self.started.elapsed().as_millis())), AMBER)
                }
                None => ("—".to_string(), MUTED),
            };
            p.text(c1 + 9, y, &speed, fg(scol));
            p.text(c1 + 23, y, if mm.fits { "yes" } else { "no" }, fg(if mm.fits { GREEN } else { RED }));
            let (btn, act, lit) = match (mm.installed, mm.active && local, &m.dl) {
                (_, true, _) => ("in use", Act::Noop, true),
                (true, _, _) => ("use", Act::ModelUse(i), false),
                (false, _, Some(d)) if d.tier_id == mm.tier.id => ("downloading", Act::ModelPause, false),
                _ if !mm.fits => ("cloud only", Act::Noop, false),
                _ if mm.partial.is_some() => ("resume", Act::ModelDownload(i), false),
                _ => ("download", Act::ModelDownload(i), false),
            };
            let bw = btn.len() as u16 + 4;
            p.button(inner.x + inner.width - bw, y, btn, act, lit);
            if mm.recommended {
                p.text(inner.x + inner.width - bw - 14, y, "recommended", fg(BLUE));
            }
            y += 1;
        }
        y = inner.y + 4 + rows_h as u16 + 1;
        p.text(inner.x, y, "cloud (optional, your key)", fg(MUTED));
        y += 1;
        match &m.form {
            None => {
                let mut x = inner.x;
                for (i, (_, name)) in CLOUD.iter().enumerate() {
                    x += p.button(x, y, name, Act::Provider(i), false) + 1;
                }
            }
            Some(f) => {
                let (id, name) = CLOUD[f.idx];
                p.text(inner.x, y, name, fg(BRIGHT));
                y += 1;
                if id == "own" || f.field == 0 {
                    p.text(inner.x, y, "address ", fg(MUTED));
                    p.text(inner.x + 8, y, &f.url.shown(), fg(if f.field == 0 { BRIGHT } else { TEXT }));
                    p.hit(Rect::new(inner.x, y, inner.width, 1), Act::Field(0));
                    if f.field == 0 {
                        self.cursor = Some((inner.x + 8 + f.url.cursor as u16, y));
                    }
                    y += 1;
                }
                p.text(inner.x, y, "key     ", fg(MUTED));
                p.text(inner.x + 8, y, &f.key.shown(), fg(if f.field == 1 { BRIGHT } else { TEXT }));
                p.hit(Rect::new(inner.x, y, inner.width, 1), Act::Field(1));
                if f.field == 1 {
                    self.cursor = Some((inner.x + 8 + f.key.cursor as u16, y));
                }
                y += 1;
                let w = p.button(inner.x, y, if f.busy { "checking…" } else { "save & use" }, Act::ProviderSave, true);
                p.button(inner.x + w + 1, y, "cancel", Act::ProviderCancel, false);
                if let Some(pr) = providers::by_id(id)
                    && !pr.keys.is_empty()
                {
                    p.text(inner.x + w + 14, y, &format!("make a key at {}", pr.keys), fg(MUTED));
                }
                if !f.error.is_empty() {
                    p.text(inner.x, y + 1, &f.error, fg(RED));
                }
            }
        }
        let by = inner.y + inner.height - 3;
        if let Some(d) = &m.dl {
            let frac = d.done as f32 / d.total.max(1) as f32;
            let label = format!("downloading {}  ", d.name);
            let w = p.text(inner.x, by, &label, fg(TEXT));
            let bw = inner.width.saturating_sub(w + 34).max(8);
            p.bar(inner.x + w, by, bw, frac, GREEN);
            let tail = if d.checking {
                " checking…".to_string()
            } else {
                format!(" {:.0}%  {} / {}  {:.1} MB/s", frac * 100.0, gb(d.done), gb(d.total), d.speed / 1e6)
            };
            let tw = p.text(inner.x + w + bw, by, &tail, fg(TEXT));
            p.button(inner.x + w + bw + tw + 1, by, "pause", Act::ModelPause, false);
        } else if !m.message.is_empty() {
            p.text(inner.x, by, &m.message, fg(AMBER));
        }
        let hint = "↑↓ choose · enter use · d download · x delete · p pause · c cloud";
        p.text(
            inner.x + inner.width.saturating_sub(hint.chars().count() as u16),
            inner.y + inner.height - 1,
            hint,
            fg(MUTED),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwdetect::Catalog;

    #[test]
    fn reads_the_server_timings() {
        let j = r#"{"timings":{"predicted_per_second": 38.2},"usage":{"completion_tokens":48}}"#;
        assert_eq!(parse_bench(j, Duration::from_secs(2)), Some(38.2));
        let j = r#"{"usage":{"completion_tokens":48}}"#;
        assert_eq!(parse_bench(j, Duration::from_secs(2)), Some(24.0));
        assert_eq!(parse_bench("{}", Duration::from_secs(2)), None);
    }

    #[test]
    fn estimates_other_models_from_one_measurement() {
        let c = Catalog::builtin().unwrap();
        let dir = std::env::temp_dir().join("trip-est-none");
        let list = models::list_in(&c, None, &dir);
        let base = &list[0];
        let other = list.iter().find(|m| m.tier.size_bytes != base.tier.size_bytes).unwrap();
        let mut speeds = HashMap::new();
        assert!(estimate(&speeds, &list, base).is_none());
        speeds.insert(base.tier.id.clone(), 30.0);
        assert_eq!(estimate(&speeds, &list, base), Some((30.0, false)));
        let (s, est) = estimate(&speeds, &list, other).unwrap();
        assert!(est);
        assert_eq!(s > 30.0, other.tier.size_bytes < base.tier.size_bytes);
    }
}
