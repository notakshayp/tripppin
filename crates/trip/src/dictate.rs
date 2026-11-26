//! Dictation: `trip dictate` (the compositor runs it on Super+M). Speak, press the key again,
//! and the words are typed where you were.
//!
//! Everything stays on this computer: `pw-record` (PipeWire) streams 16 kHz mono samples that
//! we read ourselves (for the level meter and the WAV), `whisper-cli` (whisper.cpp, run once per
//! utterance as its own process) turns them into text. If the full-screen interface has focus it
//! shows the pill inside itself and takes the text into its ask bar (over a socket it listens
//! on, so it costs nothing while idle); otherwise the text is typed with `wtype`.
//!
//! A second call while one is recording finishes it (`trip dictate cancel` throws it away).
//! It also finishes by itself after two seconds of quiet once speech was heard, and at two minutes.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

const SAMPLE_RATE: u32 = 16_000;
/// Samples per chunk read from the microphone: 100 ms.
const CHUNK: usize = 1600;
const CHUNK_MS: u32 = 100;
/// Louder than this (RMS, 0 to 1) counts as speech.
const SPEECH_RMS: f32 = 0.015;
const SILENCE_STOP_MS: u32 = 2000;
const CAP_MS: u32 = 120_000;
/// Shorter than this is a stray key press, not speech.
const MIN_SAMPLES: usize = SAMPLE_RATE as usize * 4 / 10;
const WHISPER_DIR: &str = "/usr/share/tripppin/whisper";

// ---- the pure parts ---------------------------------------------------------------------

/// The 44-byte header of a 16-bit mono PCM WAV with `samples` samples.
pub fn wav_header(samples: usize) -> Vec<u8> {
    let data = (samples * 2) as u32;
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    h.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    h.extend_from_slice(&2u16.to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    h
}

fn write_wav(path: &Path, samples: &[i16]) -> std::io::Result<()> {
    let mut bytes = wav_header(samples.len());
    bytes.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
    std::fs::write(path, bytes)
}

/// Loudness of a chunk, 0 to 1.
pub fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (f64::from(s) / 32768.0).powi(2)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

#[derive(Debug, PartialEq)]
pub enum Stop {
    Silence,
    Cap,
}

/// Decides when to stop listening: after two quiet seconds once speech was heard, or at the cap.
#[derive(Default)]
pub struct Endpointer {
    heard: bool,
    quiet_ms: u32,
    total_ms: u32,
}

impl Endpointer {
    pub fn feed(&mut self, rms: f32, ms: u32) -> Option<Stop> {
        self.total_ms += ms;
        if rms >= SPEECH_RMS {
            self.heard = true;
            self.quiet_ms = 0;
        } else {
            self.quiet_ms += ms;
        }
        if self.total_ms >= CAP_MS {
            Some(Stop::Cap)
        } else if self.heard && self.quiet_ms >= SILENCE_STOP_MS {
            Some(Stop::Silence)
        } else {
            None
        }
    }
}

/// Whisper's text without its stage directions: `[BLANK_AUDIO]`, `(music)`, ♪ and line breaks.
pub fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' || c == '(' {
            let close = if c == '[' { ']' } else { ')' };
            let rest: String = chars.clone().take(41).collect();
            if let Some(n) = rest.find(close).filter(|&n| rest[..n].chars().count() <= 40) {
                for _ in 0..rest[..n].chars().count() + 1 {
                    chars.next();
                }
                continue;
            }
        }
        match c {
            '♪' | '♫' => {}
            '\n' | '\r' | '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Physical cores from /proc/cpuinfo (whisper is fastest with one thread per core).
pub fn physical_cores(cpuinfo: &str) -> Option<usize> {
    let mut seen = std::collections::HashSet::new();
    let (mut package, mut core) = (String::new(), None::<String>);
    for line in cpuinfo.lines().chain(std::iter::once("")) {
        let value = line.split_once(':').map(|(_, v)| v.trim().to_string());
        match line.split(':').next().map(str::trim) {
            Some("physical id") => package = value.unwrap_or_default(),
            Some("core id") => core = value,
            Some("") => {
                if let Some(c) = core.take() {
                    seen.insert((package.clone(), c));
                }
            }
            _ => {}
        }
    }
    (!seen.is_empty()).then_some(seen.len())
}

fn threads() -> usize {
    let cores = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|t| physical_cores(&t));
    cores.unwrap_or_else(|| std::thread::available_parallelism().map_or(2, |n| n.get() / 2)).max(1)
}

/// The speech model file: `TRIPPPIN_WHISPER`, else the first ggml file in the image's folder.
pub fn model_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("TRIPPPIN_WHISPER").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    for dir in [PathBuf::from(WHISPER_DIR), crate::model::models_dir().join("whisper")] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "bin")).collect())
            .unwrap_or_default();
        files.sort();
        if let Some(f) = files.into_iter().next() {
            return Some(f);
        }
    }
    None
}

// ---- what the pill shows ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Pill {
    /// Recording, with the loudness of the latest chunk.
    Listening(f32),
    Transcribing,
    /// The words, to type or take into the ask bar.
    Text(String),
    Failed(String),
}

impl Pill {
    pub fn to_line(&self) -> String {
        match self {
            Pill::Listening(l) => format!("listening {l:.2}"),
            Pill::Transcribing => "transcribing".into(),
            Pill::Text(t) => format!("text {t}"),
            Pill::Failed(e) => format!("failed {e}"),
        }
    }

    pub fn from_line(l: &str) -> Option<Pill> {
        let l = l.trim_end_matches('\n');
        let (k, rest) = l.split_once(' ').unwrap_or((l, ""));
        Some(match k {
            "listening" => Pill::Listening(rest.parse().unwrap_or(0.0)),
            "transcribing" => Pill::Transcribing,
            "text" => Pill::Text(rest.to_string()),
            "failed" => Pill::Failed(rest.to_string()),
            _ => return None,
        })
    }
}

/// The bar graph for the pill: `▁▃▅▇▆▄▂` scaled from the recent levels.
pub fn bars(levels: &[f32]) -> String {
    const B: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    levels.iter().map(|l| B[((l * 12.0).sqrt().clamp(0.0, 1.0) * 7.0).round() as usize]).collect()
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

pub fn socket_path() -> PathBuf {
    runtime_dir().join("tripppin-tui.sock")
}

// ---- the interface's end: listen for a dictation ------------------------------------------

/// Listens for `trip dictate` on a socket and forwards what it sends. `focused` says whether the
/// interface has the keyboard; it answers "focused" or "away" to the first line it's sent.
pub fn listen<T: Send + 'static>(focused: std::sync::Arc<AtomicBool>, tx: Sender<T>, wrap: fn(Pill) -> T) {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else { return };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut w = match stream.try_clone() {
                Ok(w) => w,
                Err(_) => continue,
            };
            let mut lines = BufReader::new(stream).lines();
            if lines.next().is_none() {
                continue;
            }
            let is_focused = focused.load(Ordering::Relaxed);
            let _ = writeln!(w, "{}", if is_focused { "focused" } else { "away" });
            if !is_focused {
                continue;
            }
            for line in lines.map_while(Result::ok) {
                if let Some(p) = Pill::from_line(&line)
                    && tx.send(wrap(p)).is_err()
                {
                    return;
                }
            }
        }
    });
}

// ---- `trip dictate` -------------------------------------------------------------------------

trait Sink {
    fn pill(&mut self, p: &Pill);
}

struct Tui(UnixStream);

impl Sink for Tui {
    fn pill(&mut self, p: &Pill) {
        let _ = writeln!(self.0, "{}", p.to_line());
    }
}

struct Typer;

impl Sink for Typer {
    fn pill(&mut self, p: &Pill) {
        match p {
            Pill::Text(t) => {
                // wtype types into the focused Wayland window; a trailing space helps the next word.
                let _ = Command::new("wtype").arg("--").arg(format!("{t} ")).status();
            }
            Pill::Failed(e) => {
                let _ = Command::new("notify-send").args(["dictation", e]).status();
            }
            _ => {}
        }
    }
}

/// Connects to the interface if it's running and focused.
fn focused_tui() -> Option<Tui> {
    let mut s = UnixStream::connect(socket_path()).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    writeln!(s, "?").ok()?;
    let mut answer = String::new();
    BufReader::new(s.try_clone().ok()?).read_line(&mut answer).ok()?;
    (answer.trim() == "focused").then_some(Tui(s))
}

fn control_file(name: &str) -> PathBuf {
    runtime_dir().join(format!("tripppin-dictate.{name}"))
}

fn alive(pid: &str) -> bool {
    pid.trim().parse::<u32>().is_ok() && Path::new("/proc").join(pid.trim()).exists()
}

/// The command: starts recording, or finishes (or with `cancel`, drops) the one that is.
pub fn run(cancel: bool) -> anyhow::Result<()> {
    let pid_file = control_file("pid");
    if let Ok(pid) = std::fs::read_to_string(&pid_file)
        && alive(&pid)
    {
        std::fs::write(control_file(if cancel { "cancel" } else { "stop" }), "")?;
        return Ok(());
    }
    if cancel {
        return Ok(());
    }
    std::fs::write(&pid_file, std::process::id().to_string())?;
    let _ = std::fs::remove_file(control_file("stop"));
    let _ = std::fs::remove_file(control_file("cancel"));
    let mut sink: Box<dyn Sink> = match focused_tui() {
        Some(t) => Box::new(t),
        None => Box::new(Typer),
    };
    let result = session(sink.as_mut());
    let _ = std::fs::remove_file(&pid_file);
    if let Err(e) = &result {
        sink.pill(&Pill::Failed(format!("{e:#}")));
    }
    Ok(())
}

fn session(sink: &mut dyn Sink) -> anyhow::Result<()> {
    let model = model_path().ok_or_else(|| anyhow::anyhow!("no speech model installed"))?;
    let samples = record(sink)?;
    if control_file("cancel").exists() {
        let _ = std::fs::remove_file(control_file("cancel"));
        return Ok(());
    }
    if samples.len() < MIN_SAMPLES {
        return Ok(());
    }
    sink.pill(&Pill::Transcribing);
    let wav = runtime_dir().join(format!("tripppin-dictate-{}.wav", std::process::id()));
    write_wav(&wav, &samples)?;
    let text = transcribe(&model, &wav);
    let _ = std::fs::remove_file(&wav);
    let text = text?;
    if !text.is_empty() {
        sink.pill(&Pill::Text(text));
    }
    Ok(())
}

fn record(sink: &mut dyn Sink) -> anyhow::Result<Vec<i16>> {
    let mut child = Command::new("pw-record")
        .args(["--raw", "--rate", "16000", "--channels", "1", "--format", "s16", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("the microphone recorder (pw-record) didn't start: {e}"))?;
    let mut out = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no audio stream"))?;
    let mut samples: Vec<i16> = Vec::new();
    let mut end = Endpointer::default();
    let mut buf = vec![0u8; CHUNK * 2];
    while out.read_exact(&mut buf).is_ok() {
        let chunk: Vec<i16> = buf.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        let level = rms(&chunk);
        sink.pill(&Pill::Listening(level));
        samples.extend_from_slice(&chunk);
        if control_file("stop").exists() || control_file("cancel").exists() || end.feed(level, CHUNK_MS).is_some() {
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(control_file("stop"));
    Ok(samples)
}

fn transcribe(model: &Path, wav: &Path) -> anyhow::Result<String> {
    let out = Command::new("whisper-cli")
        .args(["-m"])
        .arg(model)
        .args(["-l", "auto", "-nt", "-np", "-t"])
        .arg(threads().to_string())
        .arg("-f")
        .arg(wav)
        .output()
        .map_err(|e| anyhow::anyhow!("whisper-cli didn't start: {e}"))?;
    if !out.status.success() {
        anyhow::bail!("the speech model failed");
    }
    Ok(clean(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_right() {
        let h = wav_header(16000);
        assert_eq!(h.len(), 44);
        assert_eq!(&h[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(h[40..44].try_into().unwrap()), 32000);
    }

    #[test]
    fn stops_after_quiet_once_speech_was_heard() {
        let mut e = Endpointer::default();
        for _ in 0..30 {
            assert_eq!(e.feed(0.0, 100), None, "silence alone never stops it");
        }
        assert_eq!(e.feed(0.2, 100), None);
        for _ in 0..19 {
            assert_eq!(e.feed(0.0, 100), None);
        }
        assert_eq!(e.feed(0.0, 100), Some(Stop::Silence));
        let mut e = Endpointer::default();
        assert_eq!((0..1200).filter_map(|_| e.feed(0.5, 100)).next(), Some(Stop::Cap));
    }

    #[test]
    fn cleans_whisper_text() {
        assert_eq!(clean(" [BLANK_AUDIO]\n"), "");
        assert_eq!(clean("hello (music) there\n♪ la ♪"), "hello there la");
        assert_eq!(clean("a [b] c (d e)"), "a c");
        assert_eq!(rms(&[0; 100]), 0.0);
        assert!(rms(&[16384; 100]) > 0.4);
    }

    #[test]
    fn pill_lines_round_trip() {
        for p in [
            Pill::Listening(0.25),
            Pill::Transcribing,
            Pill::Text("open the pod bay doors".into()),
            Pill::Failed("no mic".into()),
        ] {
            assert_eq!(Pill::from_line(&p.to_line()), Some(p));
        }
        assert_eq!(Pill::from_line("junk"), None);
        assert_eq!(bars(&[0.0, 0.5, 1.0]).chars().count(), 3);
    }

    #[test]
    fn counts_physical_cores() {
        let info = "physical id\t: 0\ncore id\t\t: 0\n\nphysical id\t: 0\ncore id\t\t: 0\n\nphysical id\t: 0\ncore id\t\t: 1\n";
        assert_eq!(physical_cores(info), Some(2));
        assert_eq!(physical_cores("model name: x"), None);
    }
}
