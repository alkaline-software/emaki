//! Speech to text on this machine: the composer's dictation.
//!
//! A model is a file downloaded once into `~/.emaki/models`, checked
//! against its SHA-256, and run here by transcribe.cpp (`transcribe-cpp`,
//! ggml's runtime, on the GPU through Metal), which runs Qwen's and
//! Whisper's alike from their files. Nothing said is sent
//! anywhere. What the microphone hears is kept in memory while the person
//! speaks and handed to the model when they stop; the words come back in
//! one piece.
//!
//! Only on a Mac for now (`AVAILABLE`): transcribe.cpp is C++ built by
//! CMake, which the Windows type check made from a Mac cannot build, and
//! nothing here has been tried elsewhere. Elsewhere the models are listed
//! and nothing records.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::paths;

/// Whether this build can record and transcribe.
pub const AVAILABLE: bool = cfg!(target_os = "macos");

/// The sample rate every model offered takes.
pub const RATE: u32 = 16_000;

/// A model that can be downloaded and spoken to.
pub struct Model {
    /// Its key in `config.json` (`app.dictate_model`).
    pub key: &'static str,
    pub name: &'static str,
    /// Who made it, as its row names them; the window draws their mark.
    pub maker: &'static str,
    /// What sets it apart from the others, in a few words that stay on
    /// the name's line.
    pub about: &'static str,
    /// The languages it hears, as said on its row.
    pub languages: &'static str,
    pub license: &'static str,
    /// The file it is kept as in `models_dir`.
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

/// The models offered, the heaviest first. **Each hears many languages,
/// tells by itself which is spoken, and writes two mixed in one sentence**
/// (Chinese with English words in it, as the person talks about code). A
/// model that must be told the language (Cohere Transcribe) or that
/// garbles a mixed sentence (Nemotron 3.5) or hears no Chinese (Parakeet)
/// is not offered. Whisper Large v3 Turbo is whisper.cpp's own five-bit
/// build, which transcribe.cpp reads as it is; the rest are the GGUF
/// builds transcribe.cpp's makers publish and test against the originals,
/// each at the smallest quantization that writes as well as the full one.
/// Each is fetched at a fixed commit, so the hash cannot be moved under it.
pub const MODELS: &[Model] = &[
    Model {
        key: "qwen3-asr-1.7b",
        name: "Qwen3-ASR 1.7B",
        maker: "Qwen",
        about: "Most accurate with mixed speech",
        languages: "30 languages",
        license: "Apache 2.0",
        file: "Qwen3-ASR-1.7B-Q5_K_M.gguf",
        url: "https://huggingface.co/handy-computer/Qwen3-ASR-1.7B-gguf/resolve/3555bd238a8572bbace3ebf60d23b036dc0a5dbe/Qwen3-ASR-1.7B-Q5_K_M.gguf",
        sha256: "034c557fe92ff8fcd9a9c041cbdaad347be0a86a58d3a348f63cf3f0180879d0",
        bytes: 1_517_290_464,
    },
    Model {
        key: "qwen3-asr-0.6b",
        name: "Qwen3-ASR 0.6B",
        maker: "Qwen",
        about: "Nearly as accurate, and faster",
        languages: "30 languages",
        license: "Apache 2.0",
        file: "Qwen3-ASR-0.6B-Q6_K.gguf",
        url: "https://huggingface.co/handy-computer/Qwen3-ASR-0.6B-gguf/resolve/28c66fba077f1359a42b52c55b618a24cdb6e0e7/Qwen3-ASR-0.6B-Q6_K.gguf",
        sha256: "3b051f108f03c0c91bbe1a3b2c1ee15e3ed51e4caec2a48751b01f2a21441cc3",
        bytes: 690_417_824,
    },
    Model {
        key: "whisper-large-v3-turbo",
        name: "Whisper Large v3 Turbo",
        maker: "OpenAI",
        about: "Hears the most languages",
        languages: "99 languages",
        license: "MIT",
        file: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        bytes: 574_041_195,
    },
    Model {
        key: "whisper-small",
        name: "Whisper Small",
        maker: "OpenAI",
        about: "Lightest, weaker with Chinese",
        languages: "99 languages",
        license: "MIT",
        file: "whisper-small-Q6_K.gguf",
        url: "https://huggingface.co/handy-computer/whisper-small-gguf/resolve/a2073177cb69bd74b9ca9460b852d17fbfd5d68c/whisper-small-Q6_K.gguf",
        sha256: "4d14215a972101fff787fc59e30b908ee511c725600317e02c9032cb8799f32d",
        bytes: 212_107_328,
    },
];

/// The model a new install starts with: the middle of the list in weight.
pub const DEFAULT_MODEL: &str = "whisper-large-v3-turbo";

pub fn model(key: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.key == key)
}

pub fn models_dir() -> PathBuf {
    paths::root().join("models")
}

pub fn path_of(m: &Model) -> PathBuf {
    models_dir().join(m.file)
}

/// Whether the model is here, whole: a download still under way is a
/// `.part` beside it.
pub fn installed(m: &Model) -> bool {
    fs::metadata(path_of(m)).is_ok_and(|st| st.len() == m.bytes)
}

/// The model to use: the one chosen when it is here, else the first one
/// that is. None when no model has been downloaded.
pub fn chosen(key: &str) -> Option<&'static Model> {
    model(key).filter(|m| installed(m)).or_else(|| MODELS.iter().find(|m| installed(m)))
}

/// Take the model off the disk, and out of memory.
pub fn remove(m: &Model) -> Result<(), String> {
    unload();
    let _ = fs::remove_file(path_of(m).with_extension("part"));
    match fs::remove_file(path_of(m)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Download the model, `got` counting the bytes as they come. A download
/// stopped (`stop`) or cut off goes on from where it was the next time.
/// The file takes its name only once its SHA-256 is the one expected.
pub fn download(m: &Model, got: &AtomicU64, stop: &AtomicBool) -> Result<(), String> {
    if installed(m) {
        return Ok(());
    }
    fs::create_dir_all(models_dir()).map_err(|e| e.to_string())?;
    let part = path_of(m).with_extension("part");
    let mut hash = Sha256::new();
    let mut have = 0u64;
    // What a download cut off left is hashed again first: the hash is of
    // the whole file.
    if let Ok(mut f) = fs::File::open(&part) {
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            have += n as u64;
        }
    }
    if have > m.bytes {
        let _ = fs::remove_file(&part);
        return download(m, got, stop);
    }
    got.store(have, Ordering::Relaxed);
    if have < m.bytes {
        let agent = ureq::AgentBuilder::new().redirects(8).timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).user_agent(&format!("emaki/{}", env!("CARGO_PKG_VERSION"))).build();
        let mut req = agent.get(m.url);
        if have > 0 {
            req = req.set("Range", &format!("bytes={have}-"));
        }
        let response = req.call().map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("the server answered {code}"),
            ureq::Error::Transport(t) => match t.kind() {
                ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Io => "no network".to_string(),
                _ => t.to_string(),
            },
        })?;
        // A server that sends the whole file again is taken from the start.
        if have > 0 && response.status() != 206 {
            hash = Sha256::new();
            have = 0;
            got.store(0, Ordering::Relaxed);
            let _ = fs::remove_file(&part);
        }
        let mut out = fs::OpenOptions::new().create(true).append(true).open(&part).map_err(|e| e.to_string())?;
        let mut body = response.into_reader();
        let mut buf = vec![0u8; 1 << 18];
        loop {
            if stop.load(Ordering::Relaxed) {
                return Err("stopped".into());
            }
            let n = body.read(&mut buf).map_err(|_| "the download was cut off".to_string())?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            hash.update(&buf[..n]);
            have += n as u64;
            got.store(have, Ordering::Relaxed);
        }
        out.sync_all().map_err(|e| e.to_string())?;
    }
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if have != m.bytes || digest != m.sha256 {
        let _ = fs::remove_file(&part);
        return Err("the file that came was not the model".into());
    }
    fs::rename(&part, path_of(m)).map_err(|e| e.to_string())
}

/// Interleaved samples at any rate, as one channel at `RATE`: the
/// channels averaged, then taken at the new rate by straight lines
/// between the old samples. Speech has nothing above 8 kHz worth the
/// cost of a better filter.
pub fn to_whisper(samples: &[f32], channels: usize, rate: u32) -> Vec<f32> {
    let channels = channels.max(1);
    let mono: Vec<f32> = samples.chunks(channels).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect();
    if rate == RATE || mono.is_empty() {
        return mono;
    }
    let step = rate as f64 / RATE as f64;
    let n = (mono.len() as f64 / step).floor() as usize;
    (0..n)
        .map(|i| {
            let at = i as f64 * step;
            let (j, t) = (at.floor() as usize, (at - at.floor()) as f32);
            let a = mono[j];
            let b = *mono.get(j + 1).unwrap_or(&a);
            a + (b - a) * t
        })
        .collect()
}

/// A WAV file's samples (PCM, 16 bits), its channels and its rate: for
/// a recording handed in in place of the microphone (`emaki-core dictate
/// file`, the probe's `dictate:file:`).
pub fn wav_samples(bytes: &[u8]) -> Option<(Vec<f32>, usize, u32)> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let (mut at, mut format) = (12, None);
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let body = bytes.get(at + 8..(at + 8 + len).min(bytes.len()))?;
        if id == b"fmt " && body.len() >= 16 {
            let channels = u16::from_le_bytes([body[2], body[3]]) as usize;
            let rate = u32::from_le_bytes(body[4..8].try_into().ok()?);
            let bits = u16::from_le_bytes([body[14], body[15]]);
            format = (bits == 16).then_some((channels, rate));
        } else if id == b"data" {
            let (channels, rate) = format?;
            let samples = body.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / i16::MAX as f32).collect();
            return Some((samples, channels, rate));
        }
        at += 8 + len + (len & 1);
    }
    None
}

/// How loud a stretch is: its root mean square, about 0.01 for a quiet
/// room and 0.1 for speech close by.
pub fn loudness(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Where in a recording someone speaks: the stretch from the first
/// moment loud enough to be a voice to the last, with a little room each
/// side, or None when there is none. A model must never be handed a
/// recording without a voice: Whisper was taught on videos' subtitles, and
/// silence or a room's hum it writes down as the line such a video ends
/// on ("Thank you.", "ご視聴ありがとうございました").
///
/// A voice is a stretch well above the room's own level (the quietest
/// tenth of the recording) and above `QUIET` whatever the room, lasting
/// `SPOKEN` in all: a key pressed or a cough is shorter.
pub fn speech_span(audio: &[f32]) -> Option<std::ops::Range<usize>> {
    let frame = RATE as usize * 3 / 100;
    let levels: Vec<f32> = audio.chunks(frame).map(loudness).collect();
    if levels.is_empty() {
        return None;
    }
    let mut sorted = levels.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let room = sorted[sorted.len() / 10];
    let over = (room * 2.5).max(QUIET);
    let loud: Vec<usize> = levels.iter().enumerate().filter(|(_, l)| **l > over).map(|(i, _)| i).collect();
    if (loud.len() * frame) < (SPOKEN * RATE as f32) as usize {
        return None;
    }
    let pad = RATE as usize * 3 / 10;
    let start = (loud[0] * frame).saturating_sub(pad);
    let end = ((loud[loud.len() - 1] + 1) * frame + pad).min(audio.len());
    Some(start..end)
}

/// Under this no moment is a voice, however quiet the room.
pub const QUIET: f32 = 0.008;
/// How long a voice is loud, in seconds, before it is taken for one.
pub const SPOKEN: f32 = 0.25;


#[cfg(target_os = "macos")]
pub use engine::{release_idle, transcribe, unload, Recorder};

#[cfg(not(target_os = "macos"))]
pub fn release_idle(_after: Duration) {}

#[cfg(not(target_os = "macos"))]
pub fn unload() {}

#[cfg(target_os = "macos")]
mod engine {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    use super::{loudness, path_of, pieces, speech_span, to_whisper, Model, RATE};

    /// The microphone, listened to until `finish`. The stream lives on a
    /// thread of its own, as the system's audio object must stay on the
    /// thread that made it.
    pub struct Recorder {
        stop: mpsc::Sender<()>,
        done: mpsc::Receiver<(Vec<f32>, usize, u32)>,
        level: Arc<AtomicU32>,
    }

    impl Recorder {
        /// Start listening. The first time, the system asks the person
        /// whether Emaki may use the microphone.
        pub fn start() -> Result<Recorder, String> {
            let (stop_tx, stop_rx) = mpsc::channel::<()>();
            let (done_tx, done_rx) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
            let level = Arc::new(AtomicU32::new(0));
            let meter = Arc::clone(&level);
            thread::Builder::new()
                .name("emaki-mic".into())
                .spawn(move || {
                    let heard: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
                    let open = || -> Result<(cpal::Stream, usize, u32), String> {
                        let device = cpal::default_host().default_input_device().ok_or("no microphone")?;
                        let config = device.default_input_config().map_err(|e| e.to_string())?;
                        let (channels, rate) = (config.channels() as usize, config.sample_rate());
                        let into = Arc::clone(&heard);
                        let meter = Arc::clone(&meter);
                        let take = move |data: &[f32]| {
                            meter.store(loudness(data).to_bits(), Ordering::Relaxed);
                            into.lock().unwrap().extend_from_slice(data);
                        };
                        let err = |e: cpal::Error| eprintln!("emaki: the microphone: {e}");
                        let stream = match config.sample_format() {
                            cpal::SampleFormat::F32 => device.build_input_stream(config.config(), move |d: &[f32], _: &_| take(d), err, None),
                            cpal::SampleFormat::I16 => device.build_input_stream(config.config(), move |d: &[i16], _: &_| take(&d.iter().map(|s| *s as f32 / i16::MAX as f32).collect::<Vec<_>>()), err, None),
                            cpal::SampleFormat::I32 => device.build_input_stream(config.config(), move |d: &[i32], _: &_| take(&d.iter().map(|s| *s as f32 / i32::MAX as f32).collect::<Vec<_>>()), err, None),
                            other => return Err(format!("the microphone gives {other} samples")),
                        }
                        .map_err(|e| e.to_string())?;
                        stream.play().map_err(|e| e.to_string())?;
                        Ok((stream, channels, rate))
                    };
                    match open() {
                        Ok((stream, channels, rate)) => {
                            let _ = ready_tx.send(Ok(()));
                            let _ = stop_rx.recv();
                            drop(stream);
                            let samples = std::mem::take(&mut *heard.lock().unwrap());
                            let _ = done_tx.send((samples, channels, rate));
                        }
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            ready_rx.recv().map_err(|_| "the microphone did not start".to_string())??;
            Ok(Recorder { stop: stop_tx, done: done_rx, level })
        }

        /// A recorder with no microphone behind it, for a probe of the
        /// window: its loudness rises and falls as a voice's does, and it
        /// hears nothing.
        pub fn pretend() -> Recorder {
            let (stop, stop_rx) = mpsc::channel::<()>();
            let (done_tx, done) = mpsc::channel();
            let level = Arc::new(AtomicU32::new(0));
            let meter = Arc::clone(&level);
            thread::spawn(move || {
                let at = Instant::now();
                while stop_rx.recv_timeout(Duration::from_millis(40)).is_err() {
                    let t = at.elapsed().as_secs_f32();
                    meter.store((0.045 * ((t * 7.0).sin().abs() * (t * 1.3).sin().abs())).to_bits(), Ordering::Relaxed);
                }
                let _ = done_tx.send((Vec::new(), 1, RATE));
            });
            Recorder { stop, done, level }
        }

        /// How loud the last moment was (`loudness`).
        pub fn level(&self) -> f32 {
            f32::from_bits(self.level.load(Ordering::Relaxed))
        }

        /// Stop listening: what was heard, as the models take it.
        pub fn finish(self) -> Vec<f32> {
            let _ = self.stop.send(());
            self.done.recv_timeout(Duration::from_secs(5)).map(|(s, c, r)| to_whisper(&s, c, r)).unwrap_or_default()
        }
    }

    /// The model loaded, by its file, and when it was last used. Loading
    /// takes a second or two, so it stays in memory between dictations
    /// and is let go after a while unused (`release_idle`).
    static LOADED: Mutex<Option<(String, transcribe_cpp::Model, Instant)>> = Mutex::new(None);

    /// Let the model go now. Before the process exits, always: Metal's
    /// device is torn down at exit, and ggml aborts the process if a model
    /// still holds its memory then.
    pub fn unload() {
        *LOADED.lock().unwrap() = None;
    }

    /// Let the model go when it has not been used for `after`.
    pub fn release_idle(after: Duration) {
        let mut g = LOADED.lock().unwrap();
        if g.as_ref().is_some_and(|(_, _, at)| at.elapsed() > after) {
            *g = None;
        }
    }

    fn loaded(m: &Model) -> Result<transcribe_cpp::Model, String> {
        let mut g = LOADED.lock().unwrap();
        if let Some((file, model, at)) = g.as_mut() {
            if file == m.file {
                *at = Instant::now();
                return Ok(model.clone());
            }
        }
        // The library says a great deal on stderr; this sends it nowhere.
        transcribe_cpp::disable_logging();
        *g = None;
        let model = transcribe_cpp::Model::load(path_of(m)).map_err(|e| format!("the model would not load: {e}"))?;
        *g = Some((m.file.to_string(), model.clone(), Instant::now()));
        Ok(model)
    }

    /// The words in `audio` (one channel at `RATE`), in the languages they
    /// were spoken in, told apart by the model. Empty when nothing was
    /// said. Blocking: a second or two for a minute of speech on Apple
    /// silicon.
    pub fn transcribe(m: &Model, audio: &[f32]) -> Result<String, String> {
        let Some(span) = speech_span(audio) else {
            return Ok(String::new());
        };
        // Only where someone spoke, and no less than a second, which is
        // the least some models take.
        let mut audio = audio[span].to_vec();
        if audio.len() < RATE as usize + RATE as usize / 10 {
            audio.resize(RATE as usize + RATE as usize / 10, 0.0);
        }
        let model = loaded(m)?;
        let longest = model.capabilities().map(|c| c.max_audio_ms).unwrap_or(0);
        let mut session = model.session().map_err(|e| e.to_string())?;
        let options = transcribe_cpp::RunOptions::default();
        // A model that takes no more than so long at a time is handed the
        // speech in pieces cut where it is quietest.
        let mut words = String::new();
        for piece in pieces(&audio, longest) {
            let text = session.run(piece, &options).map_err(|e| e.to_string())?.text;
            let text = text.trim();
            if !text.is_empty() {
                if !words.is_empty() && !text.starts_with(|c: char| c > '\u{2fff}') {
                    words.push(' ');
                }
                words.push_str(text);
            }
        }
        Ok(words)
    }
}

/// The speech cut into pieces no longer than `longest_ms` (none when it is
/// 0), each cut at the quietest moment of the last few seconds before the
/// limit, so no word is split.
pub fn pieces(audio: &[f32], longest_ms: i64) -> Vec<&[f32]> {
    let most = if longest_ms > 0 { (longest_ms as usize * RATE as usize / 1000).saturating_sub(RATE as usize / 2) } else { usize::MAX };
    let mut out = Vec::new();
    let mut at = 0;
    while audio.len() - at > most {
        let window = RATE as usize * 4;
        let frame = RATE as usize / 50;
        let from = at + most.saturating_sub(window);
        let cut = (from..at + most - frame).step_by(frame).min_by(|a, b| loudness(&audio[*a..*a + frame]).total_cmp(&loudness(&audio[*b..*b + frame]))).unwrap_or(at + most);
        out.push(&audio[at..cut]);
        at = cut;
    }
    out.push(&audio[at..]);
    out
}

/// What a dictation key that is modifiers alone is kept as in the
/// config, before the tap's key: `tap:right-cmd`. Any other key is a
/// binding gpui takes (`secondary-shift-d`).
pub const TAP_PREFIX: &str = "tap:";

/// A modifier key tapped alone as dictation's key: one of the right
/// side's (the left ones are in every shortcut), Fn, or two together.
/// gpui binds a key with modifiers, never modifiers alone, so these are
/// chosen from a list and watched for by `TapWatch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tap {
    /// As kept in the config, after `TAP_PREFIX`.
    pub key: &'static str,
    /// As the window writes it, in the Mac's signs (🌐 is Fn's), a pair
    /// with a "+" between.
    pub signs: &'static str,
    /// What has to have been held, and nothing else.
    want: Want,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Want {
    /// These keys, on this side.
    Keys(u16),
    /// These kinds of key, on either side.
    Kinds(u8),
}

/// The modifiers held, a bit a key: `held_from_mac` reads them.
pub const L_CTRL: u16 = 1;
pub const R_CTRL: u16 = 1 << 1;
pub const L_ALT: u16 = 1 << 2;
pub const R_ALT: u16 = 1 << 3;
pub const L_SHIFT: u16 = 1 << 4;
pub const R_SHIFT: u16 = 1 << 5;
pub const L_CMD: u16 = 1 << 6;
pub const R_CMD: u16 = 1 << 7;
pub const FN: u16 = 1 << 8;

const CTRL: u8 = 1;
const ALT: u8 = 1 << 1;
const SHIFT: u8 = 1 << 2;
const CMD: u8 = 1 << 3;
const FN_KIND: u8 = 1 << 4;

/// The taps offered, as the Mac orders its signs (⌃⌥⇧⌘): the keys
/// alone, then the pairs.
pub const TAPS: &[Tap] = &[
    Tap { key: "right-cmd", signs: "Right ⌘", want: Want::Keys(R_CMD) },
    Tap { key: "right-alt", signs: "Right ⌥", want: Want::Keys(R_ALT) },
    Tap { key: "right-shift", signs: "Right ⇧", want: Want::Keys(R_SHIFT) },
    Tap { key: "right-ctrl", signs: "Right ⌃", want: Want::Keys(R_CTRL) },
    Tap { key: "fn", signs: "Fn 🌐", want: Want::Keys(FN) },
    Tap { key: "alt-cmd", signs: "⌥ + ⌘", want: Want::Kinds(ALT | CMD) },
    Tap { key: "ctrl-cmd", signs: "⌃ + ⌘", want: Want::Kinds(CTRL | CMD) },
    Tap { key: "ctrl-alt", signs: "⌃ + ⌥", want: Want::Kinds(CTRL | ALT) },
    Tap { key: "shift-cmd", signs: "⇧ + ⌘", want: Want::Kinds(SHIFT | CMD) },
    Tap { key: "alt-shift", signs: "⌥ + ⇧", want: Want::Kinds(ALT | SHIFT) },
    Tap { key: "ctrl-shift", signs: "⌃ + ⇧", want: Want::Kinds(CTRL | SHIFT) },
];

impl Tap {
    /// Whether these keys, held together at most, are this tap.
    pub fn is(&self, held: u16) -> bool {
        match self.want {
            Want::Keys(keys) => held == keys,
            Want::Kinds(kinds) => kinds_of(held) == kinds,
        }
    }

    /// Two keys together, not one alone.
    pub fn pair(&self) -> bool {
        matches!(self.want, Want::Kinds(_))
    }
}

/// The tap a dictation key names, when it names one.
pub fn tap(key: &str) -> Option<&'static Tap> {
    let key = key.strip_prefix(TAP_PREFIX)?;
    TAPS.iter().find(|t| t.key == key)
}

fn kinds_of(held: u16) -> u8 {
    let mut kinds = 0;
    for (keys, kind) in [(L_CTRL | R_CTRL, CTRL), (L_ALT | R_ALT, ALT), (L_SHIFT | R_SHIFT, SHIFT), (L_CMD | R_CMD, CMD), (FN, FN_KIND)] {
        if held & keys != 0 {
            kinds |= kind;
        }
    }
    kinds
}

/// The modifiers held, from the flags of a Mac's key event
/// (`NSEvent.modifierFlags`). The flags for a kind of key say it is
/// held and the device's bits say on which side; a keyboard that gives
/// no side has it count as the left, so it never makes a right-hand tap.
/// Caps Lock is not a modifier here.
pub fn held_from_mac(flags: u64) -> u16 {
    // NSEventModifierFlag* and the NX_DEVICE*KEYMASK bits.
    const SIDES: [(u64, u64, u64, u16, u16); 4] = [
        (1 << 18, 0x0001, 0x2000, L_CTRL, R_CTRL),
        (1 << 19, 0x0020, 0x0040, L_ALT, R_ALT),
        (1 << 17, 0x0002, 0x0004, L_SHIFT, R_SHIFT),
        (1 << 20, 0x0008, 0x0010, L_CMD, R_CMD),
    ];
    let mut held = 0;
    for (kind, left, right, l, r) in SIDES {
        if flags & kind == 0 {
            continue;
        }
        if flags & right != 0 {
            held |= r;
        }
        if flags & left != 0 || flags & right == 0 {
            held |= l;
        }
    }
    if flags & (1 << 23) != 0 {
        held |= FN;
    }
    held
}

/// Watches the modifiers for a tap: keys pressed with nothing held and
/// let go with no other key or click among them. It goes on the keys
/// held at most, so ⌥ then ⌘, let go in any order, is ⌥⌘, and ⌘ that was
/// part of ⌘C is no tap.
#[derive(Debug, Default, Clone)]
pub struct TapWatch {
    held: u16,
    most: u16,
    spoiled: bool,
}

impl TapWatch {
    /// The modifiers held now. True when this lets go of a tap of `tap`.
    pub fn flags(&mut self, held: u16, tap: &Tap) -> bool {
        if held != 0 {
            self.held = held;
            self.most |= held;
            return false;
        }
        let hit = !self.spoiled && tap.is(self.most);
        *self = Self::default();
        hit
    }

    /// A key or a click: with modifiers held it was a shortcut, not a tap.
    pub fn other(&mut self) {
        if self.held != 0 {
            self.spoiled = true;
        }
    }

    /// Forget what is held: the app was left, and a key let go elsewhere
    /// is never seen.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
