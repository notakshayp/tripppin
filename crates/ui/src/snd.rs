//! Sound through PipeWire's PulseAudio interface (`pactl -f json`): output and input
//! devices, their volume and mute, the default device, and each app's volume.

use serde_json::Value;
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub name: String,
    pub description: String,
    pub volume: u32,
    pub muted: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    pub index: u64,
    pub app: String,
    pub volume: u32,
}

/// The average of a pactl volume object's channels, in percent.
fn percent(volume: &Value) -> u32 {
    let Some(map) = volume.as_object() else { return 0 };
    let values: Vec<u32> = map
        .values()
        .filter_map(|c| c["value_percent"].as_str())
        .filter_map(|p| p.trim_end_matches('%').trim().parse().ok())
        .collect();
    if values.is_empty() { 0 } else { values.iter().sum::<u32>() / values.len() as u32 }
}

/// Parses `pactl -f json list sinks|sources`, leaving out the monitors of outputs.
pub fn parse_devices(json: &str) -> Vec<Device> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    items
        .iter()
        .filter(|d| !d["name"].as_str().unwrap_or("").ends_with(".monitor"))
        .map(|d| Device {
            name: d["name"].as_str().unwrap_or("").to_string(),
            description: d["description"].as_str().unwrap_or("").to_lowercase(),
            volume: percent(&d["volume"]),
            muted: d["mute"].as_bool().unwrap_or(false),
        })
        .collect()
}

/// Parses `pactl -f json list sink-inputs`: what's playing, and how loud.
pub fn parse_streams(json: &str) -> Vec<Stream> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    items
        .iter()
        .map(|s| Stream {
            index: s["index"].as_u64().unwrap_or(0),
            app: s["properties"]["application.name"].as_str().unwrap_or("an app").to_lowercase(),
            volume: percent(&s["volume"]),
        })
        .collect()
}

fn pactl(args: &[&str]) -> Option<String> {
    let out = Command::new("pactl").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Outputs (speakers, headphones) and the default one's name.
pub fn outputs() -> (Vec<Device>, String) {
    let list = parse_devices(&pactl(&["-f", "json", "list", "sinks"]).unwrap_or_default());
    (list, pactl(&["get-default-sink"]).unwrap_or_default().trim().to_string())
}

/// Inputs (microphones) and the default one's name.
pub fn inputs() -> (Vec<Device>, String) {
    let list = parse_devices(&pactl(&["-f", "json", "list", "sources"]).unwrap_or_default());
    (list, pactl(&["get-default-source"]).unwrap_or_default().trim().to_string())
}

pub fn streams() -> Vec<Stream> {
    parse_streams(&pactl(&["-f", "json", "list", "sink-inputs"]).unwrap_or_default())
}

pub fn set_default(output: bool, name: &str) {
    pactl(&[if output { "set-default-sink" } else { "set-default-source" }, name]);
}

pub fn set_volume(output: bool, name: &str, percent: u32) {
    let v = format!("{}%", percent.min(150));
    pactl(&[if output { "set-sink-volume" } else { "set-source-volume" }, name, &v]);
}

pub fn set_mute(output: bool, name: &str, muted: bool) {
    pactl(&[if output { "set-sink-mute" } else { "set-source-mute" }, name, if muted { "1" } else { "0" }]);
}

pub fn set_stream_volume(index: u64, percent: u32) {
    pactl(&["set-sink-input-volume", &index.to_string(), &format!("{}%", percent.min(150))]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_devices_and_streams() {
        let sinks = r#"[
          {"index":49,"name":"alsa_output.pci.analog-stereo","description":"Built-in Audio Analog Stereo","mute":false,
           "volume":{"front-left":{"value_percent":"60%"},"front-right":{"value_percent":"62%"}}},
          {"index":50,"name":"alsa_output.pci.analog-stereo.monitor","description":"Monitor","mute":false,"volume":{}}
        ]"#;
        assert_eq!(
            parse_devices(sinks),
            vec![Device {
                name: "alsa_output.pci.analog-stereo".into(),
                description: "built-in audio analog stereo".into(),
                volume: 61,
                muted: false
            }]
        );
        let inputs =
            r#"[{"index":7,"properties":{"application.name":"Firefox"},"volume":{"mono":{"value_percent":"80%"}}}]"#;
        assert_eq!(parse_streams(inputs), vec![Stream { index: 7, app: "firefox".into(), volume: 80 }]);
        assert!(parse_devices("not json").is_empty());
    }
}
