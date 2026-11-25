//! Dictation: press the shortcut anywhere, speak, press it again, and the words are typed into
//! whatever had focus. `tripppin-ui dictate` opens a small pill ("Listening" with a level meter)
//! at the bottom of the screen; a second call while it's up finishes (`dictate cancel` throws
//! the recording away). It also finishes by itself after two seconds of quiet once speech was
//! heard, and at two minutes.
//!
//! Everything stays on the computer: `pw-record` (PipeWire) streams 16 kHz mono samples that we
//! read ourselves (for the meter, and to write the WAV), `whisper-cli` (whisper.cpp, run once
//! per utterance as its own process) turns them into text, and the text is typed through the
//! RemoteDesktop portal (works on KDE and GNOME), or `wtype` where the portal isn't there.
//! The pill never takes keyboard focus, so the target app keeps it.

use crate::common::{self, icons, label};
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;

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
const MODEL: &str = "/usr/share/tripppin/whisper/ggml-base-q5_1.bin";

// ---- the pure parts ----------------------------------------------------------

/// The 44-byte header of a 16-bit mono PCM WAV with `samples` samples.
pub fn wav_header(samples: usize) -> Vec<u8> {
    let data = (samples * 2) as u32;
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes()); // PCM
    h.extend_from_slice(&1u16.to_le_bytes()); // mono
    h.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    h.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    h.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
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

/// Why recording should stop by itself.
#[derive(Debug, PartialEq)]
pub enum Stop {
    Silence,
    Cap,
}

/// Watches the loudness of each chunk: stops after `SILENCE_STOP_MS` of quiet once speech was
/// heard (quiet before anyone speaks never stops it), or at the cap.
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

/// Whisper's non-speech notes ("[BLANK_AUDIO]", "(music)", "♪") and stray spaces removed.
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

/// How many physical cores `/proc/cpuinfo` lists (distinct package and core pairs), if it says.
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

/// The X11 keysym for a character: Latin-1 is its own code, Unicode is 0x01000000 + code.
pub fn keysym(c: char) -> i32 {
    match c {
        '\n' => 0xff0d,
        '\t' => 0xff09,
        c if (c as u32) < 0x100 => c as i32,
        c => 0x0100_0000 + c as i32,
    }
}

// ---- turning speech into text ------------------------------------------------

fn transcribe(wav: &Path) -> Result<String, String> {
    let out = Command::new("whisper-cli")
        .args(["-m", MODEL, "-l", "auto", "-nt", "-np", "-t"])
        .arg(threads().to_string())
        .arg("-f")
        .arg(wav)
        .output()
        .map_err(|e| format!("whisper-cli didn't start: {e}"))?;
    if !out.status.success() {
        return Err("The speech model failed".into());
    }
    Ok(clean(&String::from_utf8_lossy(&out.stdout)))
}

// ---- typing the text ---------------------------------------------------------

/// Something that can type text into the focused app. Another backend (another compositor)
/// is one more impl in `type_text`.
trait Typer {
    fn type_text(&self, text: &str) -> anyhow::Result<()>;
}

/// The RemoteDesktop portal: a keyboard device, one key press per character. The permission
/// is asked once; its token is kept in ~/.local/state/tripppin.
struct Portal;

fn token_file() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    state.join("tripppin/remote-desktop-token")
}

impl Typer for Portal {
    fn type_text(&self, text: &str) -> anyhow::Result<()> {
        use ashpd::desktop::PersistMode;
        use ashpd::desktop::remote_desktop::{DeviceType, KeyState, RemoteDesktop, SelectDevicesOptions};
        glib::MainContext::new().block_on(async {
            let rd = RemoteDesktop::new().await?;
            let session = rd.create_session(Default::default()).await?;
            let token = std::fs::read_to_string(token_file()).ok().map(|t| t.trim().to_string());
            rd.select_devices(
                &session,
                SelectDevicesOptions::default()
                    .set_devices(DeviceType::KEYBOARD)
                    .set_persist_mode(PersistMode::ExplicitlyRevoked)
                    .set_restore_token(token.as_deref().filter(|t| !t.is_empty())),
            )
            .await?
            .response()?;
            let started = rd.start(&session, None, Default::default()).await?.response()?;
            if let Some(t) = started.restore_token() {
                let file = token_file();
                if let Some(dir) = file.parent() {
                    std::fs::create_dir_all(dir).ok();
                }
                std::fs::write(file, t).ok();
            }
            for c in text.chars() {
                let sym = keysym(c);
                rd.notify_keyboard_keysym(&session, sym, KeyState::Pressed, Default::default()).await?;
                rd.notify_keyboard_keysym(&session, sym, KeyState::Released, Default::default()).await?;
            }
            session.close().await?;
            Ok(())
        })
    }
}

/// wtype, for compositors with the virtual-keyboard protocol (niri, sway) and no portal.
struct Wtype;

impl Typer for Wtype {
    fn type_text(&self, text: &str) -> anyhow::Result<()> {
        let status = Command::new("wtype").arg("--").arg(text).status()?;
        anyhow::ensure!(status.success(), "wtype failed");
        Ok(())
    }
}

fn type_text(text: &str) -> Result<(), String> {
    let first = Portal.type_text(text);
    let Err(portal) = first else { return Ok(()) };
    Wtype.type_text(text).map_err(|e| format!("Couldn't type: {portal}; {e}"))
}

// ---- settings ----------------------------------------------------------------

fn off_file() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    config.join("tripppin/dictation-off")
}

pub fn enabled() -> bool {
    !off_file().exists()
}

pub fn set_enabled(on: bool) {
    let file = off_file();
    if on {
        let _ = std::fs::remove_file(file);
    } else {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, "");
    }
}

// ---- the pill ----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Listening,
    Working,
}

struct Dictation {
    app: gtk::Application,
    window: gtk::Window,
    pill: gtk::Box,
    icon: gtk::Label,
    text: gtk::Label,
    bar: gtk::ProgressBar,
    phase: Cell<Phase>,
    samples: RefCell<Vec<i16>>,
    end: RefCell<Endpointer>,
    child: RefCell<Option<Child>>,
}

impl Dictation {
    fn start(self: &Rc<Self>) {
        let child = Command::new("pw-record")
            .args(["--raw", "--rate", "16000", "--channels", "1", "--format", "s16", "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(_) => return self.fail("No microphone"),
        };
        let Some(mut stdout) = child.stdout.take() else { return self.fail("No microphone") };
        *self.child.borrow_mut() = Some(child);
        let (tx, rx) = async_channel::unbounded::<Vec<i16>>();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; CHUNK * 2];
            while stdout.read_exact(&mut buf).is_ok() {
                let chunk = buf.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
                if tx.send_blocking(chunk).is_err() {
                    break;
                }
            }
        });
        self.window.present();
        common::play(&self.pill, "pop");
        let d = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(chunk) = rx.recv().await {
                if d.phase.get() != Phase::Listening {
                    break;
                }
                // The meter: quiet speech should still move it, so it follows the square root.
                let level = f64::from(rms(&chunk)).sqrt() * 1.6;
                d.bar.set_fraction(level.clamp(0.0, 1.0));
                d.samples.borrow_mut().extend_from_slice(&chunk);
                if d.end.borrow_mut().feed(rms(&chunk), CHUNK_MS).is_some() {
                    d.finish(true);
                    break;
                }
            }
            // The microphone stream ended on its own (PipeWire went away).
            if d.phase.get() == Phase::Listening {
                d.finish(true);
            }
        });
    }

    /// Stops listening; with `keep`, transcribes and types what was said.
    fn finish(self: &Rc<Self>, keep: bool) {
        if self.phase.get() != Phase::Listening {
            return;
        }
        self.phase.set(Phase::Working);
        if let Some(mut child) = self.child.borrow_mut().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let samples = std::mem::take(&mut *self.samples.borrow_mut());
        if !keep {
            return self.leave();
        }
        if samples.len() < MIN_SAMPLES || !self.end.borrow().heard {
            return self.fail("Didn't catch that");
        }
        self.icon.set_text(&icons::SPARKLES.to_string());
        self.text.set_text("Transcribing…");
        self.bar.set_visible(false);
        let (tx, rx) = async_channel::bounded::<Result<(), String>>(1);
        std::thread::spawn(move || {
            let wav = common::runtime_dir().join("dictation.wav");
            let result = write_wav(&wav, &samples)
                .map_err(|e| format!("Couldn't save the recording: {e}"))
                .and_then(|()| transcribe(&wav))
                .and_then(|text| if text.is_empty() { Err("Didn't catch that".into()) } else { type_text(&text) });
            // The recording is private: gone as soon as it's been read.
            let _ = std::fs::remove_file(&wav);
            let _ = tx.send_blocking(result);
        });
        let d = self.clone();
        glib::spawn_future_local(async move {
            match rx.recv().await {
                Ok(Ok(())) => d.leave(),
                Ok(Err(e)) => d.fail(&e),
                Err(_) => d.leave(),
            }
        });
    }

    fn fail(self: &Rc<Self>, message: &str) {
        self.phase.set(Phase::Working);
        if let Some(mut child) = self.child.borrow_mut().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.icon.set_text(&icons::MIC_OFF.to_string());
        self.text.set_text(message);
        self.bar.set_visible(false);
        if !self.window.is_visible() {
            self.window.present();
            common::play(&self.pill, "pop");
        }
        let d = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(2200), move || d.leave());
    }

    fn leave(self: &Rc<Self>) {
        self.pill.add_css_class("leave");
        let d = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(170), move || {
            d.window.set_visible(false);
            d.app.quit();
        });
    }
}

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("in.trippp.Dictate")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let current: Rc<RefCell<Option<Rc<Dictation>>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, cmd| {
        let cancel = cmd.arguments().iter().any(|a| a.to_str() == Some("cancel"));
        let running = current.borrow().clone();
        match running {
            // A second press: finish (or throw away).
            Some(d) => d.finish(!cancel),
            None if cancel || !enabled() => {}
            None => {
                let d = build(app);
                *current.borrow_mut() = Some(d.clone());
                d.start();
            }
        }
        glib::ExitCode::SUCCESS
    });
    // `tripppin-ui dictate [cancel]`: drop the "dictate" word for GTK's own argument parsing.
    let args: Vec<String> = std::env::args().take(1).chain(std::env::args().skip(2)).collect();
    app.run_with_args(&args)
}

fn build(app: &gtk::Application) -> Rc<Dictation> {
    common::load_css();
    let window = gtk::Window::builder().application(app).decorated(false).build();
    window.add_css_class("tripppin");
    window.add_css_class("clear");
    window.init_layer_shell();
    window.set_namespace(Some("tripppin-dictate"));
    window.set_layer(Layer::Overlay);
    window.set_anchor(Edge::Bottom, true);
    window.set_margin(Edge::Bottom, 72);
    // No keyboard focus: the app being dictated into keeps it.
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_exclusive_zone(-1);
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    pill.add_css_class("osd");
    pill.add_css_class("dictate");
    pill.set_margin_top(24);
    pill.set_margin_bottom(24);
    pill.set_margin_start(24);
    pill.set_margin_end(24);
    let icon = common::icon(icons::MIC, &["osd-icon"]);
    icon.set_width_chars(2);
    let text = label("Listening", &["osd-name"]);
    let bar = gtk::ProgressBar::new();
    bar.add_css_class("osd-bar");
    bar.set_hexpand(true);
    bar.set_valign(gtk::Align::Center);
    pill.append(&icon);
    pill.append(&text);
    pill.append(&bar);
    window.set_child(Some(&pill));
    // Never takes clicks either.
    window.connect_map(|w| {
        if let Some(surface) = w.surface() {
            surface.set_input_region(Some(&gtk::cairo::Region::create()));
        }
    });
    Rc::new(Dictation {
        app: app.clone(),
        window,
        pill,
        icon,
        text,
        bar,
        phase: Cell::new(Phase::Listening),
        samples: RefCell::new(Vec::new()),
        end: RefCell::new(Endpointer::default()),
        child: RefCell::new(None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_whisper_notes() {
        assert_eq!(clean(" [BLANK_AUDIO]\n"), "");
        assert_eq!(clean(" (music) Hello there.\n"), "Hello there.");
        assert_eq!(clean("\n Hello,\n world ♪"), "Hello, world");
        assert_eq!(clean("a [ Silence ] b"), "a b");
        // An unclosed bracket is just text.
        assert_eq!(clean("smile :( ok"), "smile :( ok");
    }

    #[test]
    fn wav_header_is_right() {
        let h = wav_header(16000);
        assert_eq!(h.len(), 44);
        assert_eq!(&h[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(h[4..8].try_into().unwrap()), 36 + 32000);
        assert_eq!(u32::from_le_bytes(h[24..28].try_into().unwrap()), 16000);
        assert_eq!(u32::from_le_bytes(h[40..44].try_into().unwrap()), 32000);
    }

    #[test]
    fn stops_after_silence_once_speech_was_heard() {
        let mut e = Endpointer::default();
        // Quiet at the start never stops it.
        for _ in 0..50 {
            assert_eq!(e.feed(0.0, 100), None);
        }
        assert_eq!(e.feed(0.1, 100), None);
        for _ in 0..19 {
            assert_eq!(e.feed(0.0, 100), None);
        }
        assert_eq!(e.feed(0.0, 100), Some(Stop::Silence));
    }

    #[test]
    fn stops_at_the_cap() {
        let mut e = Endpointer::default();
        let mut stop = None;
        for _ in 0..1200 {
            stop = e.feed(0.1, 100);
        }
        assert_eq!(stop, Some(Stop::Cap));
    }

    #[test]
    fn loudness() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0; 100]), 0.0);
        assert!((rms(&[16384; 100]) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn keysyms() {
        assert_eq!(keysym('a'), 0x61);
        assert_eq!(keysym('é'), 0xe9);
        assert_eq!(keysym('€'), 0x0100_20ac);
        assert_eq!(keysym('न'), 0x0100_0928);
    }

    #[test]
    fn counts_physical_cores() {
        let info = "processor : 0\nphysical id : 0\ncore id : 0\n\nprocessor : 1\nphysical id : 0\ncore id : 0\n\nprocessor : 2\nphysical id : 0\ncore id : 1\n\n";
        assert_eq!(physical_cores(info), Some(2));
        assert_eq!(physical_cores("processor : 0\nBogoMIPS : 50\n\n"), None);
    }
}
