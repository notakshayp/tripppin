//! The start screen's data: which tiles are pinned, in which groups and sizes, and the pure
//! rules around them (packing, colours, letters). No GTK in here, so it's all unit-tested.

use crate::apps::App;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The tile colours, in the order the hash picks from: the colour tiles of the Bento design.
pub const PALETTE: [&str; 8] = ["#5aa9e6", "#ff8a5a", "#f783ac", "#63e6be", "#ffd43b", "#b197fc", "#7b8cff", "#f1f3f4"];
/// Three more the design uses (graphite, the green and amber tiles); chosen by hand, never by the hash.
pub const EXTRA: [&str; 3] = ["#23272a", "#2b3a33", "#f2b84b"];
pub const COLOURS: usize = PALETTE.len() + EXTRA.len();
pub const COLUMNS: u32 = 4;

/// A stable colour for anything with a name: FNV-1a, so it never changes between runs.
pub fn colour_of(key: &str) -> usize {
    let mut h: u32 = 0x811c_9dc5;
    for b in key.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h as usize % PALETTE.len()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    Small,
    Medium,
    Wide,
    Large,
}

impl Size {
    pub const ALL: [Size; 4] = [Size::Small, Size::Medium, Size::Wide, Size::Large];
    pub fn cells(self) -> (u32, u32) {
        match self {
            Size::Small => (1, 1),
            Size::Medium => (2, 2),
            Size::Wide => (4, 2),
            Size::Large => (4, 4),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Size::Small => "small",
            Size::Medium => "medium",
            Size::Wide => "wide",
            Size::Large => "large",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tile {
    /// `app:<desktop id>`, or `calendar`, `playing`, `ask`.
    pub key: String,
    pub size: Size,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<usize>,
}

impl Tile {
    pub fn new(key: &str, size: Size) -> Tile {
        Tile { key: key.to_string(), size, colour: None }
    }
    pub fn app(id: &str, size: Size) -> Tile {
        Tile::new(&format!("app:{id}"), size)
    }
    pub fn app_id(&self) -> Option<&str> {
        self.key.strip_prefix("app:")
    }
    /// The Ask tile is always a one-row bar.
    pub fn cells(&self) -> (u32, u32) {
        if self.key == "ask" { (COLUMNS, 1) } else { self.size.cells() }
    }
    pub fn sizes(&self) -> &'static [Size] {
        if self.key == "ask" {
            &[]
        } else if self.key == "playing" {
            &[Size::Medium, Size::Wide, Size::Large]
        } else {
            &Size::ALL
        }
    }
    pub fn colour_index(&self) -> usize {
        self.colour.filter(|c| *c < COLOURS).unwrap_or_else(|| colour_of(&self.key))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub tiles: Vec<Tile>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub groups: Vec<Group>,
}

/// Where each tile sits in a group four cells wide, first fit left to right, top to bottom.
/// Returns the (column, row) of each and the number of rows used.
pub fn place(cells: &[(u32, u32)]) -> (Vec<(u32, u32)>, u32) {
    let mut used: Vec<[bool; COLUMNS as usize]> = Vec::new();
    let mut out = Vec::new();
    for &(w, h) in cells {
        let w = w.clamp(1, COLUMNS);
        let mut row = 0u32;
        let spot = 'find: loop {
            while used.len() < (row + h) as usize {
                used.push([false; COLUMNS as usize]);
            }
            for col in 0..=(COLUMNS - w) {
                let free = (row..row + h).all(|r| (col..col + w).all(|c| !used[r as usize][c as usize]));
                if free {
                    break 'find (col, row);
                }
            }
            row += 1;
        };
        for r in spot.1..spot.1 + h {
            for c in spot.0..spot.0 + w {
                used[r as usize][c as usize] = true;
            }
        }
        out.push(spot);
    }
    (out, used.len() as u32)
}

impl Layout {
    pub fn find(&self, key: &str) -> Option<(usize, usize)> {
        self.groups.iter().enumerate().find_map(|(g, grp)| grp.tiles.iter().position(|t| t.key == key).map(|i| (g, i)))
    }
    pub fn is_pinned(&self, key: &str) -> bool {
        self.find(key).is_some()
    }
    pub fn tile(&self, key: &str) -> Option<&Tile> {
        let (g, i) = self.find(key)?;
        self.groups[g].tiles.get(i)
    }
    fn tile_mut(&mut self, key: &str) -> Option<&mut Tile> {
        let (g, i) = self.find(key)?;
        self.groups[g].tiles.get_mut(i)
    }
    fn tidy(&mut self) {
        if self.groups.len() > 1 {
            self.groups.retain(|g| !g.tiles.is_empty());
        }
    }
    pub fn pin(&mut self, tile: Tile, group: usize) {
        if self.is_pinned(&tile.key) {
            return;
        }
        if self.groups.is_empty() {
            self.groups.push(Group { name: "start".into(), tiles: vec![] });
        }
        let g = group.min(self.groups.len() - 1);
        self.groups[g].tiles.push(tile);
    }
    pub fn unpin(&mut self, key: &str) {
        if let Some((g, i)) = self.find(key) {
            self.groups[g].tiles.remove(i);
            self.tidy();
        }
    }
    pub fn resize(&mut self, key: &str, size: Size) {
        if let Some(t) = self.tile_mut(key)
            && t.sizes().contains(&size)
        {
            t.size = size;
        }
    }
    pub fn recolour(&mut self, key: &str, colour: Option<usize>) {
        if let Some(t) = self.tile_mut(key) {
            t.colour = colour;
        }
    }
    pub fn move_to_group(&mut self, key: &str, group: usize) {
        let Some((g, i)) = self.find(key) else { return };
        if g == group || group >= self.groups.len() {
            return;
        }
        let t = self.groups[g].tiles.remove(i);
        self.groups[group].tiles.push(t);
        self.tidy();
    }
    /// Puts `key` just before `target` (in target's group), for drag and drop.
    pub fn move_before(&mut self, key: &str, target: &str) {
        if key == target {
            return;
        }
        let Some((g, i)) = self.find(key) else { return };
        let t = self.groups[g].tiles.remove(i);
        let (tg, ti) = self.find(target).unwrap_or((g, self.groups[g].tiles.len()));
        self.groups[tg].tiles.insert(ti, t);
        self.tidy();
    }
    pub fn new_group(&mut self, name: &str) -> usize {
        self.groups.push(Group { name: name.to_string(), tiles: vec![] });
        self.groups.len() - 1
    }
    pub fn rename_group(&mut self, group: usize, name: &str) {
        if let Some(g) = self.groups.get_mut(group) {
            g.name = name.trim().to_string();
        }
    }
}

pub fn config_file() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("tripppin/start.json")
}

pub fn parse_layout(text: &str) -> Option<Layout> {
    serde_json::from_str::<Layout>(text).ok().filter(|l| !l.groups.is_empty())
}

pub fn load() -> Option<Layout> {
    parse_layout(&std::fs::read_to_string(config_file()).ok()?)
}

/// Writes the layout through a temporary file so a crash never leaves half of it.
pub fn save(json: &str) {
    let path = config_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

pub fn to_json(layout: &Layout) -> String {
    serde_json::to_string_pretty(layout).unwrap_or_default()
}

/// The installed app that is best known by one of `words` (a part of its id, or its name).
fn find_app<'a>(apps: &'a [App], words: &[&str]) -> Option<&'a App> {
    words.iter().find_map(|w| {
        apps.iter().find(|a| {
            let id = a.id.to_lowercase();
            id.contains(w) || a.name.to_lowercase() == *w
        })
    })
}

/// What a first run shows: only apps that are really installed, plus our own tiles.
pub fn default_layout(apps: &[App]) -> Layout {
    let app = |words: &[&str], size: Size, colour: usize| {
        find_app(apps, words).map(|a| Tile { key: format!("app:{}", a.id), size, colour: Some(colour) })
    };
    let own = |key: &str, size: Size, colour: Option<usize>| Tile { key: key.to_string(), size, colour };
    let mut everyday = vec![own("calendar", Size::Medium, Some(9))];
    let mut work = Vec::new();
    everyday.extend(app(&["thunderbird", "geary", "evolution", "kmail"], Size::Medium, 6));
    everyday.push(own("playing", Size::Wide, None));
    everyday.extend(app(&["firefox"], Size::Medium, 1));
    everyday.extend(app(&["chrome", "chromium"], Size::Small, 3));
    everyday.extend(app(&["obsidian"], Size::Small, 4));
    everyday.extend(app(&["slack"], Size::Small, 2));
    everyday.extend(app(&["bitwarden"], Size::Small, 5));
    work.extend(app(&["visualstudio.code", "vscode", "code-oss"], Size::Medium, 0));
    work.extend(app(&["claude"], Size::Medium, 4));
    work.extend(app(&["dolphin", "nautilus", "thunar", "files"], Size::Medium, 0));
    work.extend(app(&["konsole", "foot", "terminal", "alacritty"], Size::Medium, 7));
    work.push(own("ask", Size::Wide, Some(10)));
    Layout {
        groups: vec![Group { name: "everyday".into(), tiles: everyday }, Group { name: "work".into(), tiles: work }],
    }
}

/// The names of the category tiles on the all-apps board, in the order ties are laid out.
pub const CATEGORY_NAMES: [&str; 6] = ["Internet", "Create", "Work", "Play", "System", "Other"];

/// Which category tile an app belongs to, from its .desktop `Categories`.
pub fn category_of(categories: &[String]) -> &'static str {
    let has = |words: &[&str]| categories.iter().any(|c| words.contains(&c.as_str()));
    if has(&["Network", "WebBrowser", "Email", "InstantMessaging", "Chat"]) {
        "Internet"
    } else if has(&["Graphics", "AudioVideo", "Audio", "Video", "Photography", "Music"]) {
        "Create"
    } else if has(&["Office", "Development", "Education", "Science", "Finance", "ProjectManagement"]) {
        "Work"
    } else if has(&["Game"]) {
        "Play"
    } else if has(&["System", "Settings", "Utility", "Accessibility", "Core", "TerminalEmulator", "FileManager"]) {
        "System"
    } else {
        "Other"
    }
}

/// The category tiles for these apps: (name, indexes into `apps`), the fullest first, empty ones left out.
pub fn category_tiles(apps: &[App]) -> Vec<(&'static str, Vec<usize>)> {
    let mut out: Vec<(&'static str, Vec<usize>)> = CATEGORY_NAMES.iter().map(|n| (*n, Vec::new())).collect();
    for (i, a) in apps.iter().enumerate() {
        let name = category_of(&a.categories);
        if let Some(t) = out.iter_mut().find(|t| t.0 == name) {
            t.1.push(i);
        }
    }
    out.retain(|t| !t.1.is_empty());
    out.sort_by_key(|t| std::cmp::Reverse(t.1.len()));
    out
}

/// How many cells (columns, rows) a category tile takes: the ones you use most grow bigger.
pub fn category_span(rank: usize, count: usize) -> (u32, u32) {
    match (rank, count) {
        (0, n) if n >= 6 => (2, 2),
        (_, n) if n >= 5 => (2, 1),
        _ => (1, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str) -> App {
        App { id: id.into(), name: name.into(), icon: String::new(), flatpak: false, mime: vec![], categories: vec![] }
    }

    #[test]
    fn colour_is_stable_and_in_range() {
        assert_eq!(colour_of("app:org.mozilla.firefox"), colour_of("app:org.mozilla.firefox"));
        assert!(colour_of("x") < PALETTE.len());
        // FNV-1a of "a" is 0xe40c292c: a fixed point, so colours never change between releases.
        assert_eq!(colour_of("a"), 0xe40c_292c_usize % 8);
        let distinct: std::collections::HashSet<_> = (0..40).map(|i| colour_of(&format!("app:{i}"))).collect();
        assert!(distinct.len() >= 5);
    }

    #[test]
    fn override_wins_and_bad_index_falls_back() {
        let mut t = Tile::new("app:a", Size::Small);
        assert_eq!(t.colour_index(), colour_of("app:a"));
        t.colour = Some(9);
        assert_eq!(t.colour_index(), 9);
        t.colour = Some(99);
        assert_eq!(t.colour_index(), colour_of("app:a"));
    }

    #[test]
    fn packs_like_the_design() {
        let cells = [(2, 2), (2, 2), (4, 2), (2, 2), (1, 1), (1, 1), (1, 1), (1, 1)];
        let (at, rows) = place(&cells);
        assert_eq!(at, vec![(0, 0), (2, 0), (0, 2), (0, 4), (2, 4), (3, 4), (2, 5), (3, 5)]);
        assert_eq!(rows, 6);
        let (at, rows) = place(&[(2, 2), (2, 2), (2, 2), (2, 2), (4, 1)]);
        assert_eq!(at[4], (0, 4));
        assert_eq!(rows, 5);
    }

    #[test]
    fn layout_edits_persist_round_trip() {
        let mut l = default_layout(&[app("org.mozilla.firefox", "Firefox"), app("md.obsidian.Obsidian", "Obsidian")]);
        l.resize("app:org.mozilla.firefox", Size::Large);
        l.recolour("app:md.obsidian.Obsidian", Some(5));
        l.rename_group(0, " daily ");
        let back = parse_layout(&to_json(&l)).unwrap();
        assert_eq!(back, l);
        assert_eq!(back.groups[0].name, "daily");
        assert_eq!(back.tile("app:org.mozilla.firefox").map(|t| t.size), Some(Size::Large));
        assert_eq!(back.tile("app:md.obsidian.Obsidian").and_then(|t| t.colour), Some(5));
    }

    #[test]
    fn broken_files_are_ignored() {
        assert!(parse_layout("not json").is_none());
        assert!(parse_layout("{\"groups\":[]}").is_none());
    }

    #[test]
    fn default_has_only_installed_apps() {
        let l = default_layout(&[app("org.mozilla.firefox", "Firefox"), app("org.kde.konsole", "Konsole")]);
        let keys: Vec<_> = l.groups.iter().flat_map(|g| g.tiles.iter().map(|t| t.key.as_str())).collect();
        assert_eq!(keys, ["calendar", "playing", "app:org.mozilla.firefox", "app:org.kde.konsole", "ask"]);
        let empty = default_layout(&[]);
        assert!(empty.groups.iter().flat_map(|g| &g.tiles).all(|t| t.app_id().is_none()));
    }

    #[test]
    fn pin_move_unpin_resize_and_reorder() {
        let mut l = default_layout(&[app("a.firefox", "Firefox"), app("b.slack", "Slack")]);
        l.pin(Tile::app("c.new", Size::Small), 1);
        l.pin(Tile::app("c.new", Size::Small), 0);
        assert_eq!(l.find("app:c.new").map(|p| p.0), Some(1));
        l.move_to_group("app:c.new", 0);
        assert_eq!(l.find("app:c.new").map(|p| p.0), Some(0));
        l.move_before("app:c.new", "calendar");
        assert_eq!(l.groups[0].tiles[0].key, "app:c.new");
        l.resize("ask", Size::Small);
        assert_eq!(l.tile("ask").map(Tile::cells), Some((4, 1)));
        l.resize("playing", Size::Small);
        assert_eq!(l.tile("playing").map(|t| t.size), Some(Size::Wide));
        l.unpin("app:c.new");
        assert!(!l.is_pinned("app:c.new"));
        let g = l.new_group("fun");
        l.move_to_group("ask", g);
        assert_eq!(l.groups.len(), 2, "the emptied group goes away");
        assert_eq!(l.find("ask").map(|p| l.groups[p.0].name.as_str()), Some("fun"));
        l.unpin("ask");
        assert!(l.groups.iter().all(|g| g.name != "fun"));
    }

    #[test]
    fn apps_fall_into_category_tiles() {
        let with =
            |id: &str, cats: &[&str]| App { categories: cats.iter().map(|c| c.to_string()).collect(), ..app(id, id) };
        let apps = [
            with("a", &["Network", "WebBrowser"]),
            with("b", &["Graphics"]),
            with("c", &["Graphics", "Photography"]),
            with("d", &["Game"]),
            with("e", &[]),
        ];
        assert_eq!(category_of(&apps[0].categories), "Internet");
        assert_eq!(category_of(&apps[4].categories), "Other");
        let tiles = category_tiles(&apps);
        assert_eq!(tiles[0], ("Create", vec![1, 2]), "the fullest comes first");
        assert_eq!(category_span(0, 7), (2, 2));
        assert_eq!(category_span(1, 5), (2, 1));
        assert_eq!(category_span(2, 2), (1, 1));
    }
}
