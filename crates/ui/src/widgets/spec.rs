//! What a widget is: its `widget.toml`, where it sits, and what its script prints. Kept free
//! of GTK so it can be tested, and small enough that a small local model writes it right.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The corner a widget is measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    TopLeft,
    #[default]
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Anchor {
    pub fn top(self) -> bool {
        matches!(self, Anchor::TopLeft | Anchor::TopRight)
    }

    pub fn left(self) -> bool {
        matches!(self, Anchor::TopLeft | Anchor::BottomLeft)
    }

    pub fn name(self) -> &'static str {
        match self {
            Anchor::TopLeft => "top-left",
            Anchor::TopRight => "top-right",
            Anchor::BottomLeft => "bottom-left",
            Anchor::BottomRight => "bottom-right",
        }
    }
}

fn default_every() -> u64 {
    60
}

fn default_margin() -> i32 {
    24
}

fn default_timeout() -> u64 {
    20
}

/// `widget.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The title, when the script doesn't print one.
    #[serde(default)]
    pub name: Option<String>,
    /// Run with `sh -c` in the widget's folder; prints JSON (or plain text) to stdout.
    pub command: String,
    /// Seconds between runs.
    #[serde(default = "default_every")]
    pub every: u64,
    /// The screen it's on, by connector name (`eDP-1`, `HDMI-A-1`); empty for the first.
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub anchor: Anchor,
    /// Distance from the anchor corner, in logical pixels (below the bar at the top).
    #[serde(default = "default_margin")]
    pub x: i32,
    #[serde(default = "default_margin")]
    pub y: i32,
    /// Width in logical pixels; 260 when not given.
    #[serde(default)]
    pub width: Option<i32>,
    /// Seconds a run may take before it's stopped.
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let c: Config = toml::from_str(text).map_err(|e| format!("widget.toml: {}", e.message()))?;
        if c.command.trim().is_empty() {
            return Err("widget.toml: command is empty".into());
        }
        Ok(c)
    }

    /// Never more often than every 2 seconds.
    pub fn every_secs(&self) -> u64 {
        self.every.max(2)
    }

    pub fn timeout_secs(&self) -> u64 {
        self.timeout.clamp(1, 300)
    }
}

/// Where a widget was dragged or pinned to: overrides its widget.toml, kept in
/// ~/.config/tripppin/widgets.json so the widget's own files are never rewritten.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
}

/// Where a widget goes: its output (None for the first screen), corner and distances.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub output: Option<String>,
    pub anchor: Anchor,
    pub x: i32,
    pub y: i32,
}

pub fn place(c: &Config, p: Option<&Placement>) -> Place {
    let p = p.cloned().unwrap_or_default();
    Place {
        output: p.output.or_else(|| c.output.clone()).filter(|o| !o.trim().is_empty()),
        anchor: p.anchor.unwrap_or(c.anchor),
        x: p.x.unwrap_or(c.x).max(0),
        y: p.y.unwrap_or(c.y).max(0),
    }
}

/// A folder name usable as a widget's name: lowercase letters, digits and dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

// ---- what a script prints -------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Ok,
    Warn,
    Error,
}

impl Status {
    fn parse(v: &Value) -> Status {
        match v.as_str().map(str::to_lowercase).as_deref() {
            Some("warn" | "warning") => Status::Warn,
            Some("error" | "bad" | "fail") => Status::Error,
            _ => Status::Ok,
        }
    }

    pub fn class(self) -> Option<&'static str> {
        match self {
            Status::Ok => None,
            Status::Warn => Some("warn"),
            Status::Error => Some("error"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A label and a value on one line, with an optional bar (0 to 1) under it.
    Row { label: String, value: String, bar: Option<f64>, status: Status },
    /// A line or paragraph of text.
    Text(String),
    /// A big number or score, with a small caption.
    Big { value: String, label: Option<String> },
    /// A small line chart of the numbers, oldest first.
    Spark { values: Vec<f64>, label: Option<String> },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    pub title: Option<String>,
    pub status: Status,
    pub items: Vec<Item>,
    pub footer: Option<String>,
}

const MAX_ITEMS: usize = 20;
const MAX_TEXT: usize = 400;
const MAX_SPARK: usize = 120;

fn text_of(v: &Value) -> Option<String> {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => return None,
    };
    Some(clip(s.trim()))
}

fn clip(s: &str) -> String {
    if s.chars().count() > MAX_TEXT {
        let mut c: String = s.chars().take(MAX_TEXT - 1).collect();
        c.push('…');
        c
    } else {
        s.to_string()
    }
}

/// A bar's fill: 0 to 1, or a percentage (23 means 0.23).
fn fraction(v: &Value) -> Option<f64> {
    let n = v.as_f64().or_else(|| v.as_str()?.trim().trim_end_matches('%').parse().ok())?;
    let n = if n > 1.0 { n / 100.0 } else { n };
    n.is_finite().then(|| n.clamp(0.0, 1.0))
}

fn item(v: &Value) -> Option<Item> {
    if let Some(s) = v.as_str() {
        return Some(Item::Text(clip(s)));
    }
    let label = v.get("label").and_then(text_of);
    if let Some(spark) = v.get("spark").and_then(Value::as_array) {
        let mut values: Vec<f64> = spark.iter().filter_map(Value::as_f64).filter(|n| n.is_finite()).collect();
        if values.len() > MAX_SPARK {
            values.drain(..values.len() - MAX_SPARK);
        }
        return Some(Item::Spark { values, label });
    }
    if let Some(value) = v.get("big").and_then(text_of) {
        return Some(Item::Big { value, label });
    }
    if let Some(text) = v.get("text").and_then(text_of) {
        return Some(Item::Text(text));
    }
    let value = v.get("value").and_then(text_of);
    let bar = v.get("bar").and_then(fraction);
    if label.is_none() && value.is_none() && bar.is_none() {
        return None;
    }
    Some(Item::Row {
        label: label.unwrap_or_default(),
        value: value.unwrap_or_default(),
        bar,
        status: v.get("status").map_or(Status::Ok, Status::parse),
    })
}

/// Reads what a widget's script printed: the JSON schema in docs/WIDGETS.md, or else plain
/// text, one line per item. Anything it can't use is left out rather than failing the widget.
pub fn parse_output(stdout: &str) -> Result<Shown, String> {
    let text = stdout.trim();
    if text.is_empty() {
        return Err("the script printed nothing".into());
    }
    if !text.starts_with('{') {
        let items = text.lines().filter(|l| !l.trim().is_empty()).take(MAX_ITEMS).map(|l| Item::Text(clip(l.trim())));
        return Ok(Shown { items: items.collect(), ..Default::default() });
    }
    // Some scripts print a line per run; the last complete object is the current one.
    let v: Value = serde_json::from_str(text)
        .or_else(|e| text.lines().rev().find_map(|l| serde_json::from_str(l.trim()).ok()).ok_or(e))
        .map_err(|e| format!("not valid JSON: {e}"))?;
    let Value::Object(_) = v else {
        return Err("the JSON must be an object: {\"title\": …, \"items\": […]}".into());
    };
    let items = match v.get("items") {
        Some(Value::Array(a)) => a.iter().filter_map(item).take(MAX_ITEMS).collect(),
        Some(_) => return Err("\"items\" must be a list".into()),
        None => Vec::new(),
    };
    Ok(Shown {
        title: v.get("title").and_then(text_of).filter(|t| !t.is_empty()),
        status: v.get("status").map_or(Status::Ok, Status::parse),
        items,
        footer: v.get("footer").and_then(text_of).filter(|t| !t.is_empty()),
    })
}

/// One line about a result, for `widgets list` and `widgets test`.
pub fn summary(s: &Shown) -> String {
    let n = s.items.len();
    let status = match s.status {
        Status::Ok => "ok",
        Status::Warn => "warn",
        Status::Error => "error",
    };
    format!("status {status}, {n} item{}", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_widget_toml() {
        let c = Config::parse(
            "command = \"python3 score.py\"\nevery = 30\noutput = \"HDMI-A-1\"\nanchor = \"bottom-left\"\n",
        )
        .unwrap();
        assert_eq!(c.every_secs(), 30);
        assert_eq!(c.anchor, Anchor::BottomLeft);
        assert_eq!((c.x, c.y, c.width), (24, 24, None));
        assert!(Config::parse("every = 3").is_err());
        assert!(Config::parse("command = \"x\"\nanchr = \"top-left\"").unwrap_err().contains("anchr"));
        assert_eq!(Config::parse("command = \"x\"\nevery = 0").unwrap().every_secs(), 2);
    }

    #[test]
    fn placement_overrides_the_toml() {
        let c = Config::parse("command = \"x\"\noutput = \"eDP-1\"").unwrap();
        let p = Placement { output: Some("HDMI-A-1".into()), x: Some(-5), ..Default::default() };
        assert_eq!(
            place(&c, Some(&p)),
            Place { output: Some("HDMI-A-1".into()), anchor: Anchor::TopRight, x: 0, y: 24 }
        );
        assert_eq!(place(&c, None).output.as_deref(), Some("eDP-1"));
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Cricket score!"), "cricket-score");
        assert_eq!(slug("  ../etc "), "etc");
    }

    #[test]
    fn reads_the_schema() {
        let out = r#"{"title":"System","status":"warn","items":[
            {"label":"CPU","value":"23%","bar":0.23},
            {"label":"Memory","value":7.5,"bar":"81%","status":"warn"},
            {"text":"all good"},
            {"big":"245/3","label":"India, 41.2 overs"},
            {"spark":[1,2,3]},
            "a plain line",
            {"nothing":"here"}
        ],"footer":"updated 12:03"}"#;
        let s = parse_output(out).unwrap();
        assert_eq!(s.title.as_deref(), Some("System"));
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.items.len(), 6);
        assert_eq!(
            s.items[1],
            Item::Row { label: "Memory".into(), value: "7.5".into(), bar: Some(0.81), status: Status::Warn }
        );
        assert_eq!(s.items[3], Item::Big { value: "245/3".into(), label: Some("India, 41.2 overs".into()) });
        assert_eq!(s.items[5], Item::Text("a plain line".into()));
        assert_eq!(summary(&s), "status warn, 6 items");
    }

    #[test]
    fn plain_text_and_mistakes() {
        let s = parse_output("hello\n\nworld\n").unwrap();
        assert_eq!(s.items, [Item::Text("hello".into()), Item::Text("world".into())]);
        assert!(parse_output("  ").is_err());
        assert!(parse_output("{\"title\": ").unwrap_err().starts_with("not valid JSON"));
        assert!(parse_output("{\"items\": 3}").is_err());
        // A script that prints a line per update: the last one counts.
        let s = parse_output("{\"title\":\"a\"}\n{\"title\":\"b\"}").unwrap();
        assert_eq!(s.title.as_deref(), Some("b"));
    }
}
