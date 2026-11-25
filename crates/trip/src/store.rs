//! The Store's catalogue: Flathub's apps, read from the AppStream data Flatpak already keeps
//! on this computer, searched locally, and installed through `flatpak`.
//!
//! The command line (`trip store …`), the AI's tools and the Store window
//! (`tripppin-ui store`) all use this module. Installs are system-wide: administrators need
//! no password (Flatpak's polkit rule for the wheel group, restated in the image's rules).

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The tile colours, from the design: an app without a brand colour gets one of these.
pub const PALETTE: [&str; 8] = ["#1f6b45", "#0f7c8c", "#c2502e", "#6b3f8f", "#3a4a5a", "#b8862b", "#2c5aa0", "#3a2a22"];

/// One app in the catalogue.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    /// The cached icon's file name (in the catalogue's icons folder).
    pub icon: Option<String>,
    pub screenshots: Vec<String>,
    pub licence: String,
    pub developer: String,
    pub verified: bool,
    /// Download size in bytes, when the catalogue says.
    pub size: Option<u64>,
    /// The app's own colour (`#rrggbb`), when it has one that suits a dark tile.
    pub brand: Option<String>,
}

impl App {
    /// The tile colour: the brand's when it's dark enough for a white label, else a palette
    /// colour that's always the same for this app.
    pub fn tile_colour(&self) -> String {
        if let Some(b) = &self.brand
            && luminance(b).is_some_and(|l| l < 0.45)
        {
            return b.clone();
        }
        let h = self.id.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
        PALETTE[h as usize % PALETTE.len()].to_string()
    }

    /// "GPL-2.0+" from "GPL-2.0-or-later"; "proprietary" for a custom licence.
    pub fn licence_words(&self) -> String {
        let l = self.licence.split('=').next().unwrap_or("");
        match l {
            "" => String::new(),
            l if l.starts_with("LicenseRef-proprietary") => "proprietary".into(),
            l if l.starts_with("LicenseRef") => "other licence".into(),
            l => l.replace("-only", "").replace("-or-later", "+"),
        }
    }
}

fn luminance(hex: &str) -> Option<f64> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|v| f64::from(v) / 255.0);
    Some(0.2126 * c(0)? + 0.7152 * c(2)? + 0.0722 * c(4)?)
}

// ---- reading AppStream ------------------------------------------------------------------

/// Reads Flathub's AppStream XML into apps (desktop applications only). Text with an
/// `xml:lang` is a translation and is skipped.
pub fn parse_appstream(source: impl Read) -> Vec<App> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_reader(BufReader::new(source));
    let mut buf = Vec::new();
    let mut apps = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut skip: Option<usize> = None;
    let mut cur: Option<App> = None;
    let mut text = String::new();
    let mut desc = String::new();
    // Attributes of the element being read.
    let (mut icon_kind, mut icon_width, mut image_kind, mut image_width, mut value_key) =
        (String::new(), 0u32, String::new(), 0u32, String::new());
    let (mut thumb, mut source_url) = (None::<(u32, String)>, None::<String>);
    let mut colour_dark = false;
    let mut brand: Option<String> = None;
    let mut icon_best = 0u32;

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let mut attrs = std::collections::HashMap::new();
                for a in e.attributes().flatten() {
                    attrs.insert(
                        String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                        a.unescape_value().map(|v| v.into_owned()).unwrap_or_default(),
                    );
                }
                stack.push(name.clone());
                if skip.is_none() && attrs.contains_key("xml:lang") {
                    skip = Some(stack.len());
                }
                text.clear();
                match name.as_str() {
                    "component" => {
                        cur = (attrs.get("type").map(String::as_str) == Some("desktop-application")).then(App::default);
                        desc.clear();
                        icon_best = 0;
                        brand = None;
                    }
                    "icon" => {
                        icon_kind = attrs.get("type").cloned().unwrap_or_default();
                        icon_width = attrs.get("width").and_then(|w| w.parse().ok()).unwrap_or(0);
                    }
                    "screenshot" => {
                        thumb = None;
                        source_url = None;
                    }
                    "image" => {
                        image_kind = attrs.get("type").cloned().unwrap_or_default();
                        image_width = attrs.get("width").and_then(|w| w.parse().ok()).unwrap_or(0);
                    }
                    "value" => value_key = attrs.get("key").cloned().unwrap_or_default(),
                    "color" => {
                        colour_dark = attrs.get("type").map(String::as_str) == Some("primary")
                            && attrs.get("scheme_preference").map(String::as_str) != Some("light");
                    }
                    "size" => {
                        value_key = if attrs.get("type").map(String::as_str) == Some("download") {
                            "download-size".into()
                        } else {
                            String::new()
                        };
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if skip.is_none()
                    && let Ok(t) = t.unescape()
                {
                    if stack.iter().any(|s| s == "description") {
                        desc.push_str(&t);
                    }
                    text.push_str(&t);
                }
            }
            Ok(Event::End(_)) => {
                let name = stack.last().cloned().unwrap_or_default();
                let parent = stack.len().checked_sub(2).and_then(|i| stack.get(i)).cloned().unwrap_or_default();
                if skip.is_none()
                    && let Some(app) = cur.as_mut()
                {
                    let t = text.trim().to_string();
                    match (name.as_str(), parent.as_str()) {
                        ("id", "component") => app.id = t.trim_end_matches(".desktop").to_string(),
                        ("name", "component") => app.name = t,
                        ("summary", "component") => app.summary = t,
                        ("project_license", _) => app.licence = t,
                        ("developer_name", _) => app.developer = t,
                        ("name", "developer") => app.developer = t,
                        ("category", "categories") if !t.is_empty() => app.categories.push(t),
                        ("keyword", "keywords") if !t.is_empty() => app.keywords.push(t.to_lowercase()),
                        ("icon", "component") if icon_kind == "cached" && !t.is_empty() => {
                            // Prefer 128 px; any cached icon is better than none.
                            let score = if icon_width == 128 { 1000 } else { icon_width };
                            if score >= icon_best {
                                icon_best = score;
                                app.icon = Some(t);
                            }
                        }
                        ("image", "screenshot") if !t.is_empty() => {
                            if image_kind == "source" {
                                source_url = Some(t);
                            } else if image_kind == "thumbnail"
                                && image_width <= 800
                                && thumb.as_ref().is_none_or(|(w, _)| image_width > *w)
                            {
                                thumb = Some((image_width, t));
                            }
                        }
                        ("screenshot", "screenshots") => {
                            if let Some(url) = thumb.take().map(|(_, u)| u).or(source_url.take()) {
                                app.screenshots.push(url);
                            }
                        }
                        ("value", _) if value_key == "flathub::verification::verified" => app.verified = t == "true",
                        ("size", _) if value_key == "download-size" => app.size = t.parse().ok(),
                        ("color", _) if colour_dark && t.starts_with('#') => brand = Some(t),
                        ("li", _) | ("p", "description") => desc.push('\n'),
                        _ => {}
                    }
                }
                if name == "component"
                    && skip.is_none()
                    && let Some(mut app) = cur.take()
                {
                    app.brand = brand.take();
                    app.description = tidy_description(&desc);
                    if !app.id.is_empty() && !app.name.is_empty() {
                        apps.push(app);
                    }
                }
                if skip == Some(stack.len()) {
                    skip = None;
                }
                stack.pop();
                text.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    apps
}

fn tidy_description(raw: &str) -> String {
    let paras: Vec<String> =
        raw.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect();
    let mut out = paras.join("\n");
    if out.chars().count() > 1200 {
        out = out.chars().take(1200).collect::<String>() + "…";
    }
    out
}

// ---- the catalogue ----------------------------------------------------------------------

/// Apps shown first on the Store's home, when they're in the catalogue.
pub const FEATURED: [&str; 24] = [
    "org.mozilla.firefox",
    "org.videolan.VLC",
    "org.gimp.GIMP",
    "org.libreoffice.LibreOffice",
    "md.obsidian.Obsidian",
    "com.bitwarden.desktop",
    "org.mozilla.Thunderbird",
    "com.discordapp.Discord",
    "com.spotify.Client",
    "com.visualstudio.code",
    "org.kde.krita",
    "org.inkscape.Inkscape",
    "org.blender.Blender",
    "com.obsproject.Studio",
    "org.audacityteam.Audacity",
    "org.telegram.desktop",
    "org.signal.Signal",
    "com.slack.Slack",
    "org.kde.kdenlive",
    "io.github.celluloid_player.Celluloid",
    "org.gnome.Calculator",
    "com.google.Chrome",
    "org.qbittorrent.qBittorrent",
    "com.valvesoftware.Steam",
];

/// The categories the Store lists, as (AppStream name, what people call it).
pub const CATEGORIES: [(&str, &str); 10] = [
    ("Network", "internet"),
    ("AudioVideo", "music & video"),
    ("Graphics", "graphics"),
    ("Office", "office"),
    ("Development", "develop"),
    ("Game", "games"),
    ("Education", "learn"),
    ("Utility", "tools"),
    ("System", "system"),
    ("Science", "science"),
];

#[derive(Debug, Default)]
pub struct Catalog {
    pub apps: Vec<App>,
    icons: Vec<PathBuf>,
    /// Lowercased name, id and summary per app, for search.
    index: Vec<(String, String, String)>,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    source: String,
    mtime: u64,
    len: u64,
    apps: Vec<App>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map_or_else(|| home().join(".local/share"), PathBuf::from)
}

/// Where Flatpak keeps Flathub's AppStream data: the system installation, then the person's.
/// `TRIPPPIN_STORE_APPSTREAM` (a file) and `TRIPPPIN_STORE_ICONS` (a folder) override it, for tests.
fn sources() -> Vec<(PathBuf, Vec<PathBuf>)> {
    if let Some(f) = std::env::var_os("TRIPPPIN_STORE_APPSTREAM") {
        let icons = std::env::var_os("TRIPPPIN_STORE_ICONS").map(PathBuf::from).into_iter().collect();
        return vec![(PathBuf::from(f), icons)];
    }
    [PathBuf::from("/var/lib/flatpak"), data_home().join("flatpak")]
        .into_iter()
        .map(|root| {
            let active = root.join("appstream/flathub").join(std::env::consts::ARCH).join("active");
            (active.join("appstream.xml.gz"), vec![active.join("icons/128x128"), active.join("icons/flathub/128x128")])
        })
        .collect()
}

fn cache_file() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home().join(".cache"), PathBuf::from);
    base.join("tripppin/store-index.json")
}

fn open_xml(path: &Path) -> Option<Box<dyn Read>> {
    let f = std::fs::File::open(path).ok()?;
    Some(if path.extension().is_some_and(|e| e == "gz") {
        Box::new(flate2::read::MultiGzDecoder::new(f))
    } else {
        Box::new(f)
    })
}

/// The icon of an installed app, the biggest Flatpak exported (scalable first): sharp at any scale.
pub fn installed_icon(id: &str) -> Option<PathBuf> {
    let roots = [
        PathBuf::from("/var/lib/flatpak/exports/share/icons/hicolor"),
        data_home().join("flatpak/exports/share/icons/hicolor"),
    ];
    let sizes = ["scalable", "512x512", "256x256", "128x128", "64x64"];
    for root in &roots {
        for size in sizes {
            for ext in ["svg", "png"] {
                let p = root.join(size).join("apps").join(format!("{id}.{ext}"));
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

impl Catalog {
    pub fn from_apps(apps: Vec<App>, icons: Vec<PathBuf>) -> Catalog {
        let index =
            apps.iter().map(|a| (a.name.to_lowercase(), a.id.to_lowercase(), a.summary.to_lowercase())).collect();
        Catalog { apps, icons, index }
    }

    /// Loads the catalogue Flatpak has downloaded; an empty one if it hasn't (run `refresh`).
    /// The parsed result is kept in the cache folder until the source file changes. Parsing
    /// a fresh catalogue takes a moment: windows call this from a thread.
    pub fn load() -> Catalog {
        for (path, icons) in sources() {
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            let key = path.to_string_lossy().into_owned();
            if let Ok(text) = std::fs::read_to_string(cache_file())
                && let Ok(c) = serde_json::from_str::<Cached>(&text)
                && c.source == key
                && c.mtime == mtime
                && c.len == meta.len()
            {
                return Catalog::from_apps(c.apps, icons);
            }
            let Some(src) = open_xml(&path) else { continue };
            let apps = parse_appstream(src);
            if !apps.is_empty() {
                let cache = cache_file();
                if let Some(dir) = cache.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let cached = Cached { source: key, mtime, len: meta.len(), apps };
                if let Ok(json) = serde_json::to_string(&cached) {
                    let _ = std::fs::write(&cache, json);
                }
                return Catalog::from_apps(cached.apps, icons);
            }
        }
        Catalog::default()
    }

    pub fn get(&self, id: &str) -> Option<&App> {
        self.apps.iter().find(|a| a.id == id)
    }

    /// The app's icon on disk: the installed app's own (scalable) one, else the catalogue's
    /// 128 px one.
    pub fn icon_path(&self, app: &App) -> Option<PathBuf> {
        installed_icon(&app.id).or_else(|| {
            let file = app.icon.as_ref()?;
            self.icons.iter().map(|d| d.join(file)).find(|p| p.exists())
        })
    }

    /// Apps matching every word, best first: a name match beats the summary, which beats
    /// keywords and categories. Ties go to verified apps, then alphabetical.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&App> {
        let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(u32, &App)> = self
            .apps
            .iter()
            .zip(&self.index)
            .filter_map(|(app, (name, id, summary))| {
                let mut total = 0;
                for w in &words {
                    let score = if name == w {
                        100
                    } else if name.starts_with(w.as_str()) {
                        70
                    } else if name.split(|c: char| !c.is_alphanumeric()).any(|p| p == w) {
                        55
                    } else if name.contains(w.as_str()) {
                        40
                    } else if id.contains(w.as_str()) {
                        25
                    } else if summary.contains(w.as_str()) {
                        15
                    } else if app.keywords.iter().any(|k| k.contains(w.as_str())) {
                        10
                    } else if app.categories.iter().any(|c| c.to_lowercase().contains(w.as_str())) {
                        4
                    } else {
                        return None;
                    };
                    total += score;
                }
                Some((total + u32::from(app.verified), app))
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase())));
        hits.into_iter().take(limit).map(|(_, a)| a).collect()
    }

    /// The home page's apps: the curated list that's in the catalogue, then verified apps
    /// with screenshots to fill up to `limit`.
    pub fn featured(&self, limit: usize) -> Vec<&App> {
        let mut out: Vec<&App> = FEATURED.iter().filter_map(|id| self.get(id)).collect();
        for a in &self.apps {
            if out.len() >= limit {
                break;
            }
            if a.verified && !a.screenshots.is_empty() && a.icon.is_some() && !out.iter().any(|o| o.id == a.id) {
                out.push(a);
            }
        }
        out.truncate(limit);
        out
    }

    pub fn in_category(&self, category: &str, limit: usize) -> Vec<&App> {
        let mut v: Vec<&App> = self.apps.iter().filter(|a| a.categories.iter().any(|c| c == category)).collect();
        v.sort_by_key(|a| (!a.verified, a.icon.is_none(), a.name.to_lowercase()));
        v.truncate(limit);
        v
    }

    /// Apps that share categories with this one (verified and well described first).
    pub fn related(&self, app: &App, limit: usize) -> Vec<&App> {
        let mut v: Vec<&App> = self
            .apps
            .iter()
            .filter(|a| a.id != app.id && a.categories.iter().any(|c| app.categories.contains(c)))
            .collect();
        let shared = |a: &App| a.categories.iter().filter(|c| app.categories.contains(c)).count();
        v.sort_by_key(|a| (std::cmp::Reverse(shared(a)), !a.verified, a.icon.is_none(), a.name.to_lowercase()));
        v.truncate(limit);
        v
    }

    /// An app by id, or the best search match for what was typed.
    pub fn find(&self, id_or_words: &str) -> Option<&App> {
        self.get(id_or_words).or_else(|| self.search(id_or_words, 1).into_iter().next())
    }
}

// ---- flatpak ----------------------------------------------------------------------------

fn flatpak(args: &[&str]) -> Result<String, String> {
    let out = Command::new("flatpak").args(args).output().map_err(|e| format!("can't run flatpak: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("flatpak failed").trim().to_string())
    }
}

/// Fetches the newest catalogue from Flathub.
pub fn refresh() -> Result<(), String> {
    flatpak(&["update", "--appstream", "--noninteractive"]).map(|_| ())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub size: String,
}

pub fn parse_installed(out: &str) -> Vec<Installed> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 4 && !f[0].is_empty()).then(|| Installed {
                id: f[0].into(),
                name: f[1].into(),
                version: f[2].into(),
                size: f[3].to_lowercase(),
            })
        })
        .collect()
}

pub fn installed() -> Vec<Installed> {
    flatpak(&["list", "--app", "--columns=application,name,version,size"])
        .map(|o| parse_installed(&o))
        .unwrap_or_default()
}

/// Ids of apps with an update waiting.
pub fn updates() -> Vec<String> {
    flatpak(&["remote-ls", "--updates", "--app", "--columns=application"])
        .map(|o| o.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

/// How far a `flatpak` run has got: the step ("Installing 2/3") and the percentage of it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
    pub step: u32,
    pub steps: u32,
    pub percent: u32,
}

impl Progress {
    /// Overall progress, 0.0 to 1.0, across the steps (an app and the runtimes it needs).
    pub fn fraction(&self) -> f64 {
        let steps = f64::from(self.steps.max(1));
        ((f64::from(self.step.max(1)) - 1.0) + f64::from(self.percent.min(100)) / 100.0) / steps
    }
}

/// Reads one line of `flatpak install`'s output ("Installing 2/3… ████ 45%") into `p`.
/// Returns whether the line said anything about progress.
pub fn parse_progress(line: &str, p: &mut Progress) -> bool {
    let mut seen = false;
    for tok in line.split_whitespace() {
        let t = tok.trim_matches(|c: char| !c.is_ascii_digit() && c != '/' && c != '%');
        if let Some(num) = t.strip_suffix('%')
            && let Ok(n) = num.parse::<u32>()
            && n <= 100
        {
            p.percent = n;
            seen = true;
        } else if let Some((a, b)) = t.split_once('/')
            && let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>())
            && a >= 1
            && a <= b
        {
            if a != p.step {
                p.percent = 0;
            }
            p.step = a;
            p.steps = b;
            seen = true;
        }
    }
    seen
}

/// Runs a flatpak command, calling `on_progress` as it reports progress (Flatpak redraws
/// its bar with carriage returns, so output is split on both those and newlines).
fn run_with_progress(args: &[&str], on_progress: &mut dyn FnMut(Progress)) -> Result<(), String> {
    let mut child = Command::new("flatpak")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("can't run flatpak: {e}"))?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let mut readers = Vec::new();
    let streams: [Box<dyn Read + Send>; 2] =
        [Box::new(child.stdout.take().ok_or("no output")?), Box::new(child.stderr.take().ok_or("no output")?)];
    for stream in streams {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            let mut r = BufReader::new(stream);
            let mut chunk = Vec::new();
            while let Ok(n) = r.read_until(b'\r', &mut chunk) {
                if n == 0 {
                    break;
                }
                for line in String::from_utf8_lossy(&chunk).split('\n') {
                    if tx.send(line.trim_matches('\r').to_string()).is_err() {
                        return;
                    }
                }
                chunk.clear();
            }
        }));
    }
    drop(tx);
    let (mut p, mut last_error) = (Progress::default(), String::new());
    for line in rx {
        if parse_progress(&line, &mut p) {
            on_progress(p);
        } else if !line.trim().is_empty() {
            last_error = line.trim().to_string();
        }
    }
    for r in readers {
        let _ = r.join();
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        on_progress(Progress { step: 1, steps: 1, percent: 100 });
        Ok(())
    } else {
        Err(last_error.trim_start_matches("error: ").to_string())
    }
}

/// Installs an app from Flathub for everyone on the computer.
pub fn install(id: &str, on_progress: &mut dyn FnMut(Progress)) -> Result<(), String> {
    check_id(id)?;
    run_with_progress(&["install", "-y", "--noninteractive", "--system", "flathub", id], on_progress)
}

pub fn remove(id: &str, on_progress: &mut dyn FnMut(Progress)) -> Result<(), String> {
    check_id(id)?;
    run_with_progress(&["uninstall", "-y", "--noninteractive", id], on_progress)
}

/// Updates one app, or everything when `id` is `None`.
pub fn update(id: Option<&str>, on_progress: &mut dyn FnMut(Progress)) -> Result<(), String> {
    let mut args = vec!["update", "-y", "--noninteractive"];
    if let Some(id) = id {
        check_id(id)?;
        args.push(id);
    }
    run_with_progress(&args, on_progress)
}

/// An application id is dotted words; refuse anything else so it can't pass for an option.
pub fn check_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && !id.starts_with(['-', '.'])
        && id.contains('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if ok { Ok(()) } else { Err(format!("\"{id}\" isn't an app id (they look like org.mozilla.firefox)")) }
}

/// The text of `flatpak remote-info`'s line for one field, such as "Download".
fn info_field(out: &str, field: &str) -> Option<String> {
    out.lines().find_map(|l| {
        l.trim().strip_prefix(&format!("{field}:")).map(|v| v.trim().trim_start_matches(['<', '≤']).trim().to_string())
    })
}

/// The download size as Flatpak words it ("92.3 MB"), from the remote.
pub fn download_size(id: &str) -> Option<String> {
    check_id(id).ok()?;
    let out = flatpak(&["remote-info", "--system", "flathub", id]).ok()?;
    info_field(&out, "Download")
}

/// What an app is allowed to do, in plain words, from its finish-args.
pub fn permissions(id: &str) -> Result<Vec<String>, String> {
    check_id(id)?;
    let meta = flatpak(&["remote-info", "--system", "--show-metadata", "flathub", id])?;
    Ok(permission_words(&meta))
}

/// Translates an app's metadata (a key file with a [Context] section) into plain words.
pub fn permission_words(metadata: &str) -> Vec<String> {
    let mut section = String::new();
    let (mut shared, mut sockets, mut devices, mut features, mut files, mut bus) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for line in metadata.lines().map(str::trim) {
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = s.to_string();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let list = || v.split(';').map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect::<Vec<_>>();
        match (section.as_str(), k.trim()) {
            ("Context", "shared") => shared = list(),
            ("Context", "sockets") => sockets = list(),
            ("Context", "devices") => devices = list(),
            ("Context", "features") => features = list(),
            ("Context", "filesystems") => files = list(),
            ("Session Bus Policy", name) if v.trim() == "talk" || v.trim() == "own" => bus.push(name.to_string()),
            _ => {}
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut add = |s: &str| {
        if !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    };
    if shared.iter().any(|s| s == "network") {
        add("the internet");
    }
    let mut folders: Vec<String> = Vec::new();
    for f in files.iter().filter(|f| !f.starts_with('!')) {
        let (path, ro) = match f.rsplit_once(':') {
            Some((p, "ro")) => (p, true),
            Some((p, "rw" | "create")) => (p, false),
            _ => (f.as_str(), false),
        };
        let name = match path {
            "host" => "all your files".to_string(),
            "host-os" | "host-etc" => "the system's files".into(),
            "home" => "your whole home folder".into(),
            "xdg-documents" => "your Documents".into(),
            "xdg-download" => "your Downloads".into(),
            "xdg-pictures" => "your Pictures".into(),
            "xdg-music" => "your Music".into(),
            "xdg-videos" => "your Videos".into(),
            "xdg-desktop" => "your Desktop".into(),
            p if p.starts_with("xdg-") => continue,
            p if p.starts_with('/') || p.starts_with('~') => format!("the folder {p}"),
            _ => continue,
        };
        folders.push(if ro { format!("{name}, read only") } else { name });
    }
    if folders.iter().any(|f| f == "all your files") {
        folders.retain(|f| !f.starts_with("your ") && f != "all your files, read only");
    }
    for f in folders {
        add(&f);
    }
    for s in &sockets {
        match s.as_str() {
            "pulseaudio" => add("sound, and the microphone"),
            "ssh-auth" => add("your SSH keys"),
            "gpg-agent" => add("your signing keys"),
            "system-bus" => add("the computer's system services"),
            "session-bus" => add("everything on your desktop (it is not sandboxed from it)"),
            "cups" => add("your printers"),
            "pcsc" => add("smart card readers"),
            _ => {}
        }
    }
    if sockets.iter().any(|s| s == "x11") && !sockets.iter().any(|s| s == "fallback-x11") {
        add("the older X11 display system (less isolated)");
    }
    for d in &devices {
        match d.as_str() {
            "all" => add("all your devices, cameras and drives included"),
            "dri" => add("the graphics card"),
            "kvm" => add("virtual machine hardware"),
            "input" => add("game controllers and input devices"),
            _ => {}
        }
    }
    for f in &features {
        match f.as_str() {
            "bluetooth" => add("Bluetooth"),
            "devel" => add("other programs' internals (for debugging)"),
            _ => {}
        }
    }
    for b in &bus {
        match b.as_str() {
            "org.freedesktop.Notifications" => add("notifications"),
            "org.freedesktop.secrets" => add("your saved passwords"),
            "org.freedesktop.FileManager1" => add("showing files in the file manager"),
            "org.freedesktop.ScreenSaver" => add("keeping the screen awake"),
            "org.kde.StatusNotifierWatcher" => add("a tray icon"),
            _ => {}
        }
    }
    out
}

/// A size in bytes as people say it ("92 MB").
pub fn size_words(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} kB", (b / 1e3).max(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<components version="0.16" origin="flathub">
  <component type="desktop-application">
    <id>org.videolan.VLC</id>
    <name>VLC</name>
    <name xml:lang="de">VLC Deutsch</name>
    <summary>Play videos &amp; music</summary>
    <summary xml:lang="de">Spielt Videos</summary>
    <description><p>The best player.</p><p xml:lang="de">Der beste</p><ul><li>plays everything</li></ul></description>
    <project_license>GPL-2.0-or-later</project_license>
    <developer_name>VideoLAN</developer_name>
    <categories><category>AudioVideo</category><category>Player</category></categories>
    <keywords><keyword>Media</keyword></keywords>
    <icon type="cached" width="64" height="64">org.videolan.VLC.png</icon>
    <icon type="cached" width="128" height="128">org.videolan.VLC.128.png</icon>
    <screenshots>
      <screenshot type="default">
        <image type="source" width="1600" height="900">https://x/full.png</image>
        <image type="thumbnail" width="224" height="126">https://x/224.png</image>
        <image type="thumbnail" width="752" height="423">https://x/752.png</image>
      </screenshot>
      <screenshot><image type="source">https://x/only.png</image></screenshot>
    </screenshots>
    <custom><value key="flathub::verification::verified">true</value></custom>
    <branding><color type="primary" scheme_preference="dark">#8a3b00</color></branding>
  </component>
  <component type="runtime"><id>org.x.Platform</id><name>Platform</name></component>
  <component type="desktop-application">
    <id>org.gnome.Calculator.desktop</id>
    <name>Calculator</name>
    <summary>Do sums and play video of numbers</summary>
    <keywords><keyword>maths</keyword></keywords>
  </component>
</components>"##;

    fn catalog() -> Catalog {
        Catalog::from_apps(parse_appstream(FIXTURE.as_bytes()), vec![])
    }

    #[test]
    fn parses_apps_and_skips_translations_and_runtimes() {
        let apps = parse_appstream(FIXTURE.as_bytes());
        assert_eq!(apps.len(), 2);
        let v = &apps[0];
        assert_eq!((v.id.as_str(), v.name.as_str()), ("org.videolan.VLC", "VLC"));
        assert_eq!(v.summary, "Play videos & music");
        assert_eq!(v.description, "The best player.\nplays everything");
        assert_eq!(v.licence_words(), "GPL-2.0+");
        assert_eq!(v.developer, "VideoLAN");
        assert_eq!(v.categories, ["AudioVideo", "Player"]);
        assert_eq!(v.keywords, ["media"]);
        assert_eq!(v.icon.as_deref(), Some("org.videolan.VLC.128.png"));
        assert_eq!(v.screenshots, ["https://x/752.png", "https://x/only.png"]);
        assert!(v.verified);
        assert_eq!(v.tile_colour(), "#8a3b00");
        assert_eq!(apps[1].id, "org.gnome.Calculator");
        assert!(!apps[1].verified);
        assert!(PALETTE.contains(&apps[1].tile_colour().as_str()));
    }

    #[test]
    fn name_beats_summary_and_all_words_must_match() {
        let c = catalog();
        let hits: Vec<_> = c.search("video", 10).iter().map(|a| a.name.clone()).collect();
        assert_eq!(hits, ["VLC", "Calculator"]);
        assert_eq!(c.search("vlc", 5).len(), 1);
        assert_eq!(c.search("maths sums", 5).len(), 1);
        assert!(c.search("vlc sums", 5).is_empty());
        assert!(c.search("", 5).is_empty());
        assert_eq!(c.find("org.videolan.VLC").unwrap().name, "VLC");
        assert_eq!(c.find("calc").unwrap().name, "Calculator");
    }

    #[test]
    fn category_and_related() {
        let c = catalog();
        assert_eq!(c.in_category("AudioVideo", 5).len(), 1);
        assert!(c.related(c.get("org.videolan.VLC").unwrap(), 5).is_empty());
    }

    #[test]
    fn metadata_becomes_plain_words() {
        let meta = "[Application]\nname=org.x.App\n\n[Context]\nshared=network;ipc;\nsockets=x11;wayland;pulseaudio;fallback-x11;\ndevices=dri;\nfilesystems=xdg-download;xdg-documents:ro;~/Notes;!xdg-run/x;xdg-config/foo:create;\nfeatures=bluetooth;\n\n[Session Bus Policy]\norg.freedesktop.Notifications=talk\norg.freedesktop.portal.Foo=talk\n";
        let w = permission_words(meta);
        assert_eq!(w[0], "the internet");
        for want in [
            "your Downloads",
            "your Documents, read only",
            "the folder ~/Notes",
            "sound, and the microphone",
            "the graphics card",
            "Bluetooth",
            "notifications",
        ] {
            assert!(w.iter().any(|x| x == want), "missing {want}: {w:?}");
        }
        assert!(!w.iter().any(|x| x.contains("X11")));
        assert!(permission_words("[Context]\nshared=ipc;\n").is_empty());
        let wide = permission_words("[Context]\nfilesystems=host;xdg-download;\n");
        assert_eq!(wide, ["all your files"]);
    }

    #[test]
    fn reads_flatpak_progress() {
        let mut p = Progress::default();
        assert!(parse_progress("Installing 2/3… ██████░░░░░░ 45%", &mut p));
        assert_eq!(p, Progress { step: 2, steps: 3, percent: 45 });
        assert!(parse_progress("Installing… 80%", &mut p));
        assert_eq!(p.percent, 80);
        assert!((p.fraction() - (1.0 + 0.8) / 3.0).abs() < 1e-9);
        assert!(parse_progress("Installing 3/3…", &mut p));
        assert_eq!(p.percent, 0);
        assert!(!parse_progress("app/org.x.App/x86_64/stable", &mut p));
        assert!(!parse_progress("Looking for matches…", &mut p));
    }

    #[test]
    fn rejects_odd_ids_and_parses_lists() {
        assert!(check_id("org.mozilla.firefox").is_ok());
        assert!(check_id("--user").is_err());
        assert!(check_id("firefox").is_err());
        let l = parse_installed("org.x.App\tApp\t1.0\t12.3 MB\n\n");
        assert_eq!(
            l[0],
            Installed { id: "org.x.App".into(), name: "App".into(), version: "1.0".into(), size: "12.3 mb".into() }
        );
        assert_eq!(info_field("  Download: < 92.3 MB\n", "Download").as_deref(), Some("92.3 MB"));
        assert_eq!(size_words(92_300_000), "92 MB");
    }
}
