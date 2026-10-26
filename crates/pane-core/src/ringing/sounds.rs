//! What a ringing plays, as stereo 32-bit float frames: the three sounds
//! Pane ships, made for Pane by the code below (no recording, so no
//! licence beyond Pane's own), a WAV file read here on every system, an
//! MP3 decoded by Media Foundation on Windows (and a WAV of another
//! encoding) and by Symphonia elsewhere, an Ogg Vorbis file decoded by
//! Symphonia everywhere (`compressed`), and the system's notification
//! sound: the sound scheme's on Windows, the alert sound on macOS (an AIFF
//! file, read here too), and the freedesktop sound theme's on Linux.

use std::f32::consts::TAU;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use super::{MAX_FILE_BYTES, RingError, Sound};

/// A sound Pane ships, the same on every system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bundled {
    /// Insistent, for an alarm: four quick beeps, then a pause.
    Alarm,
    /// A struck bell, for a timer or a reminder.
    Bell,
    /// Gentle, for a reminder: two falling notes.
    Chime,
}

/// The rate the bundled sounds are made at.
const BUNDLED_RATE: u32 = 48_000;

/// The loudest a bundled sound gets, a little under full scale.
const BUNDLED_PEAK: f32 = 0.7;

/// A sound being played: frames out, from the start again. Opened and
/// played on the ringing's own thread.
pub(crate) trait Source {
    /// Frames a second.
    fn rate(&self) -> u32;

    /// Fills `out` (interleaved stereo) with the next frames, answering how
    /// many; 0 once the sound ended.
    fn fill(&mut self, out: &mut [f32]) -> Result<usize, String>;

    /// Goes back to the start, to play the sound again.
    fn rewind(&mut self) -> Result<(), String>;
}

/// Opens `sound` for playing, or answers why it cannot: a file that cannot
/// be read or decoded is `failed`; a sound this system cannot reach is
/// `not-available`.
pub(crate) fn open(sound: &Sound) -> Result<Box<dyn Source>, RingError> {
    match sound {
        Sound::Bundled(bundled) => Ok(Box::new(Frames::new(BUNDLED_RATE, made(*bundled)))),
        Sound::System => {
            let path = system_sound()?;
            open_system_file(&path).map_err(|why| {
                RingError::NotAvailable(format!(
                    "the system's notification sound cannot be played: {why}"
                ))
            })
        }
        Sound::File(path) => open_file(path).map_err(RingError::Failed),
    }
}

/// The file the system's sound scheme plays for notifications.
fn system_sound() -> Result<std::path::PathBuf, RingError> {
    #[cfg(windows)]
    {
        super::windows::notification_sound().map_err(RingError::NotAvailable)
    }
    #[cfg(target_os = "macos")]
    {
        Ok(super::macos::alert_sound())
    }
    #[cfg(target_os = "linux")]
    {
        theme_sound(&sound_folders()).map_err(RingError::NotAvailable)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Err(RingError::NotAvailable(
            "the system's notification sound is not available on this system".into(),
        ))
    }
}

/// The freedesktop sound theme's names for a notification's sound, the
/// most fitting first.
#[cfg(any(target_os = "linux", test))]
const THEME_SOUNDS: [&str; 3] = ["message-new-instant", "message", "bell"];

/// The folders the freedesktop sound theme specification looks for themes
/// in: `$XDG_DATA_HOME/sounds`, then each of `$XDG_DATA_DIRS`' `sounds`.
#[cfg(target_os = "linux")]
fn sound_folders() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let home = std::env::var_os("XDG_DATA_HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(dirs.split(':').map(PathBuf::from))
        .map(|folder| folder.join("sounds"))
        .collect()
}

/// The `freedesktop` theme's sound for a notification under `folders`, the
/// theme every desktop falls back to: Ogg Vorbis (`.oga`) on most
/// distributions, which Pane decodes, or WAV.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn theme_sound(folders: &[std::path::PathBuf]) -> Result<std::path::PathBuf, String> {
    for name in THEME_SOUNDS {
        for folder in folders {
            let stereo = folder.join("freedesktop/stereo");
            let found = ["oga", "ogg", "wav"]
                .iter()
                .map(|extension| stereo.join(format!("{name}.{extension}")))
                .find(|path| path.is_file());
            if let Some(found) = found {
                return Ok(found);
            }
        }
    }
    Err(
        "the desktop's sound theme has no notification sound (the freedesktop sound theme is \
         not installed)"
            .into(),
    )
}

/// Opens the system's sound file `path`: a WAV, an AIFF (macOS's sounds
/// are), an Ogg Vorbis file (Linux's are), else whatever the system's
/// decoder reads.
fn open_system_file(path: &Path) -> Result<Box<dyn Source>, String> {
    let shown = path.display();
    let bytes = read_sound(path)?;
    match Wav::read_aiff(&bytes) {
        Ok(Some(aiff)) => return Ok(Box::new(aiff)),
        Ok(None) => {}
        Err(why) => return Err(format!("{shown} is not an AIFF file Pane can play: {why}")),
    }
    open_read(path, bytes)
}

/// The bytes of the sound file `path`, at most [`MAX_FILE_BYTES`].
fn read_sound(path: &Path) -> Result<Vec<u8>, String> {
    let shown = path.display();
    let size = std::fs::metadata(path)
        .map_err(|error| format!("{shown} cannot be read: {error}"))?
        .len();
    if size > MAX_FILE_BYTES {
        return Err(format!(
            "{shown} is {} MiB; Pane plays sound files of at most 10 MiB",
            size.div_ceil(1024 * 1024)
        ));
    }
    std::fs::read(path).map_err(|error| format!("{shown} cannot be read: {error}"))
}

/// Opens the sound file `path`: a WAV Pane reads itself, else an MP3 or an
/// Ogg Vorbis file.
fn open_file(path: &Path) -> Result<Box<dyn Source>, String> {
    open_read(path, read_sound(path)?)
}

/// Opens `bytes`, read from `path`, as [`open_file`] does.
fn open_read(path: &Path, bytes: Vec<u8>) -> Result<Box<dyn Source>, String> {
    if !(bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE") {
        return decoded(path, bytes);
    }
    match Wav::read(bytes) {
        Ok(Some(wav)) => Ok(Box::new(wav)),
        // A WAV of an encoding Pane does not read itself: Media Foundation
        // reads it from the file on Windows.
        Ok(None) => decoded(path, Vec::new()),
        Err(why) => Err(format!(
            "{} is not a WAV file Pane can play: {why}",
            path.display()
        )),
    }
}

/// A file Pane does not read itself: an MP3 or an Ogg Vorbis file, decoded
/// by Symphonia, but for Windows' MP3s (and compressed WAVs), which Media
/// Foundation decodes there.
fn decoded(path: &Path, bytes: Vec<u8>) -> Result<Box<dyn Source>, String> {
    #[cfg(windows)]
    if !bytes.starts_with(b"OggS") {
        return super::windows::Decoded::open(path)
            .map(|decoded| Box::new(decoded) as Box<dyn Source>);
    }
    super::compressed::Compressed::open(path, bytes)
        .map(|compressed| Box::new(compressed) as Box<dyn Source>)
}

/// Frames in memory, played from the start.
pub(crate) struct Frames {
    rate: u32,
    frames: Arc<[f32]>,
    at: usize,
}

impl Frames {
    pub(crate) fn new(rate: u32, frames: Arc<[f32]>) -> Frames {
        Frames {
            rate,
            frames,
            at: 0,
        }
    }
}

impl Source for Frames {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn fill(&mut self, out: &mut [f32]) -> Result<usize, String> {
        let samples = out.len().min(self.frames.len() - self.at) / 2 * 2;
        out[..samples].copy_from_slice(&self.frames[self.at..self.at + samples]);
        self.at += samples;
        Ok(samples / 2)
    }

    fn rewind(&mut self) -> Result<(), String> {
        self.at = 0;
        Ok(())
    }
}

/// A bundled sound's frames, made once.
fn made(bundled: Bundled) -> Arc<[f32]> {
    static MADE: [OnceLock<Arc<[f32]>>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let (at, make): (usize, fn() -> Vec<f32>) = match bundled {
        Bundled::Alarm => (0, alarm),
        Bundled::Bell => (1, bell),
        Bundled::Chime => (2, chime),
    };
    MADE[at]
        .get_or_init(|| {
            let mono = normalized(make());
            mono.iter().flat_map(|&sample| [sample, sample]).collect()
        })
        .clone()
}

/// `seconds` of silence at the bundled rate.
fn silence(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds * BUNDLED_RATE as f32) as usize]
}

/// The alarm: four beeps of 0.1 seconds at 2,000 Hz with a little of its
/// third harmonic, 0.06 seconds apart, then 0.4 seconds of silence before
/// the next round. Short fades keep each beep from clicking.
fn alarm() -> Vec<f32> {
    let mut sound = Vec::new();
    for _ in 0..4 {
        let beep = (0.1 * BUNDLED_RATE as f32) as usize;
        sound.extend((0..beep).map(|at| {
            let time = at as f32 / BUNDLED_RATE as f32;
            let tone = (TAU * 2000.0 * time).sin() + 0.25 * (TAU * 6000.0 * time).sin();
            tone * fade(at, beep, 0.005)
        }));
        sound.extend(silence(0.06));
    }
    sound.extend(silence(0.34));
    sound
}

/// The bell: a struck bell at 880 Hz, its partials at a bell's ratios,
/// each dying away, the higher ones sooner, over 2 seconds.
fn bell() -> Vec<f32> {
    const PARTIALS: [(f32, f32, f32); 5] = [
        // (ratio to the fundamental, loudness, seconds to fall to 1/e)
        (0.5, 0.3, 1.2),
        (1.0, 1.0, 0.9),
        (1.19, 0.4, 0.6),
        (2.0, 0.35, 0.45),
        (2.76, 0.2, 0.3),
    ];
    let length = (2.0 * BUNDLED_RATE as f32) as usize;
    (0..length)
        .map(|at| {
            let time = at as f32 / BUNDLED_RATE as f32;
            let tone: f32 = PARTIALS
                .iter()
                .map(|(ratio, loudness, decay)| {
                    loudness * (TAU * 880.0 * ratio * time).sin() * (-time / decay).exp()
                })
                .sum();
            tone * fade(at, length, 0.002)
        })
        .collect()
}

/// The chime: E6 then C6, each a soft sine with a faint octave, fading
/// over 0.8 seconds, the second starting 0.35 seconds after the first.
fn chime() -> Vec<f32> {
    let length = (1.6 * BUNDLED_RATE as f32) as usize;
    let note = |frequency: f32, time: f32| {
        if time < 0.0 {
            return 0.0;
        }
        let tone = (TAU * frequency * time).sin() + 0.15 * (TAU * 2.0 * frequency * time).sin();
        tone * (-time / 0.3).exp()
    };
    (0..length)
        .map(|at| {
            let time = at as f32 / BUNDLED_RATE as f32;
            (note(1318.5, time) + note(1046.5, time - 0.35)) * fade(at, length, 0.003)
        })
        .collect()
}

/// A gain that rises over the first `seconds` of `length` frames and falls
/// over its last, so a sound starts and ends without a click.
fn fade(at: usize, length: usize, seconds: f32) -> f32 {
    let edge = (seconds * BUNDLED_RATE as f32).max(1.0);
    let from_start = at as f32 / edge;
    let to_end = (length - at) as f32 / edge;
    from_start.min(to_end).min(1.0)
}

/// `sound` scaled so its loudest sample is [`BUNDLED_PEAK`].
fn normalized(mut sound: Vec<f32>) -> Vec<f32> {
    let peak = sound
        .iter()
        .fold(0f32, |peak, sample| peak.max(sample.abs()));
    if peak > 0.0 {
        let gain = BUNDLED_PEAK / peak;
        sound.iter_mut().for_each(|sample| *sample *= gain);
    }
    sound
}

/// A WAV file's samples, or an AIFF file's, read as they are played.
pub(crate) struct Wav {
    bytes: Vec<u8>,
    /// Where the samples start and end in `bytes`.
    data: std::ops::Range<usize>,
    encoding: Encoding,
    channels: usize,
    rate: u32,
    /// The next frame to play, from the start of the samples.
    at: usize,
}

/// How a WAV or AIFF file's samples are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    /// Unsigned 8-bit integers (WAV).
    U8,
    /// Signed 8-bit integers (AIFF).
    I8,
    /// Signed little-endian integers of this many bytes (2, 3 or 4).
    Int(usize),
    /// Little-endian floats of this many bytes (4 or 8).
    Float(usize),
    /// Signed big-endian integers of this many bytes (2, 3 or 4: AIFF).
    IntBig(usize),
    /// Big-endian floats of this many bytes (4 or 8: AIFF-C).
    FloatBig(usize),
}

impl Encoding {
    fn bytes(self) -> usize {
        match self {
            Encoding::U8 | Encoding::I8 => 1,
            Encoding::Int(bytes)
            | Encoding::Float(bytes)
            | Encoding::IntBig(bytes)
            | Encoding::FloatBig(bytes) => bytes,
        }
    }

    /// The sample at the start of `bytes`, from -1 to 1.
    fn sample(self, bytes: &[u8]) -> f32 {
        match self {
            // Big-endian: read as the little-endian sample of the same
            // width, its bytes turned round.
            Encoding::IntBig(width) | Encoding::FloatBig(width) => {
                let mut little = [0; 8];
                for (at, byte) in bytes[..width].iter().rev().enumerate() {
                    little[at] = *byte;
                }
                match self {
                    Encoding::IntBig(_) => Encoding::Int(width).sample(&little),
                    _ => Encoding::Float(width).sample(&little),
                }
            }
            Encoding::I8 => f32::from(bytes[0] as i8) / 128.0,
            Encoding::U8 => (f32::from(bytes[0]) - 128.0) / 128.0,
            Encoding::Int(2) => f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0,
            Encoding::Int(3) => {
                (i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) >> 8) as f32 / 8_388_608.0
            }
            Encoding::Int(_) => {
                i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32
                    / 2_147_483_648.0
            }
            Encoding::Float(4) => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            Encoding::Float(_) => f64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]) as f32,
        }
    }
}

impl Wav {
    /// Reads `bytes` as a WAV file: `None` when it is no RIFF WAVE file,
    /// or one whose encoding Pane does not read itself (the system's
    /// decoder may); why, when it is one that is broken.
    pub(crate) fn read(bytes: Vec<u8>) -> Result<Option<Wav>, String> {
        if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Ok(None);
        }
        let mut format = None;
        let mut data = None;
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let id = &bytes[at..at + 4];
            let size =
                u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
                    as usize;
            let body = at + 8;
            // A data chunk may say it runs past the file's end when the
            // writer never went back to fill its size in: it is cut there.
            let end = body.saturating_add(size).min(bytes.len());
            match id {
                b"fmt " => format = Some(&bytes[body..end]),
                b"data" => data = Some(body..end),
                _ => {}
            }
            // Chunks are padded to an even size.
            at = body.saturating_add(size).saturating_add(size % 2);
        }
        let format = format.ok_or("it has no format chunk")?;
        let data = data.ok_or("it has no sound data")?;
        if format.len() < 16 {
            return Err("its format chunk is too short".into());
        }
        let word = |at: usize| u16::from_le_bytes([format[at], format[at + 1]]);
        let mut tag = word(0);
        let channels = usize::from(word(2));
        let rate = u32::from_le_bytes([format[4], format[5], format[6], format[7]]);
        let bits = word(14);
        // WAVE_FORMAT_EXTENSIBLE names its encoding in its subformat GUID,
        // whose first two bytes are the plain format tag.
        if tag == 0xFFFE {
            if format.len() < 26 {
                return Err("its extensible format chunk is too short".into());
            }
            tag = word(24);
        }
        let encoding = match (tag, bits) {
            (1, 8) => Encoding::U8,
            (1, 16 | 24 | 32) => Encoding::Int(usize::from(bits / 8)),
            (3, 32 | 64) => Encoding::Float(usize::from(bits / 8)),
            // ADPCM, A-law and the rest: for the system's decoder.
            _ => return Ok(None),
        };
        if channels == 0 {
            return Err("it has no channels".into());
        }
        if !(1_000..=384_000).contains(&rate) {
            return Err(format!(
                "its rate of {rate} samples a second is not a sound's"
            ));
        }
        let frame = encoding.bytes() * channels;
        if (data.end - data.start) < frame {
            return Err("it holds no sound".into());
        }
        Ok(Some(Wav {
            bytes,
            data,
            encoding,
            channels,
            rate,
            at: 0,
        }))
    }

    /// Reads `bytes` as an AIFF or AIFF-C file of uncompressed samples, as
    /// macOS's sounds are: `None` when it is none; why, when it is one Pane
    /// cannot play.
    pub(crate) fn read_aiff(bytes: &[u8]) -> Result<Option<Wav>, String> {
        if bytes.len() < 12 || &bytes[..4] != b"FORM" {
            return Ok(None);
        }
        let compressed = match &bytes[8..12] {
            b"AIFF" => false,
            b"AIFC" => true,
            _ => return Ok(None),
        };
        let mut common = None;
        let mut data = None;
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let id = &bytes[at..at + 4];
            let size =
                u32::from_be_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
                    as usize;
            let body = at + 8;
            let end = body.saturating_add(size).min(bytes.len());
            match id {
                b"COMM" => common = Some(&bytes[body..end]),
                // The sound data chunk starts with the offset of its first
                // sample past its eight bytes of header, and a block size.
                b"SSND" if end - body >= 8 => {
                    let offset = u32::from_be_bytes([
                        bytes[body],
                        bytes[body + 1],
                        bytes[body + 2],
                        bytes[body + 3],
                    ]) as usize;
                    data = Some((body + 8).saturating_add(offset).min(end)..end);
                }
                _ => {}
            }
            // Chunks are padded to an even size.
            at = body.saturating_add(size).saturating_add(size % 2);
        }
        let common = common.ok_or("it has no common chunk")?;
        let data = data.ok_or("it has no sound data")?;
        if common.len() < 18 || (compressed && common.len() < 22) {
            return Err("its common chunk is too short".into());
        }
        let channels = usize::from(u16::from_be_bytes([common[0], common[1]]));
        let bits = u16::from_be_bytes([common[6], common[7]]);
        let rate = extended(&common[8..18]);
        let kind: &[u8] = if compressed { &common[18..22] } else { b"NONE" };
        let width = usize::from(bits.div_ceil(8));
        let encoding = match (kind, width) {
            (b"NONE", 1) => Encoding::I8,
            (b"NONE", 2..=4) => Encoding::IntBig(width),
            (b"sowt", 2..=4) => Encoding::Int(width),
            (b"fl32" | b"FL32", _) => Encoding::FloatBig(4),
            (b"fl64" | b"FL64", _) => Encoding::FloatBig(8),
            _ => {
                return Err(format!(
                    "its samples are compressed ({}), which Pane does not decode",
                    String::from_utf8_lossy(kind)
                ));
            }
        };
        if channels == 0 {
            return Err("it has no channels".into());
        }
        if !(1_000.0..=384_000.0).contains(&rate) {
            return Err(format!(
                "its rate of {rate} samples a second is not a sound's"
            ));
        }
        if (data.end - data.start) < encoding.bytes() * channels {
            return Err("it holds no sound".into());
        }
        Ok(Some(Wav {
            bytes: bytes.to_vec(),
            data,
            encoding,
            channels,
            rate: rate.round() as u32,
            at: 0,
        }))
    }

    fn frames(&self) -> usize {
        (self.data.end - self.data.start) / (self.encoding.bytes() * self.channels)
    }
}

/// An 80-bit IEEE 754 extended float, big-endian, as AIFF writes its rate.
fn extended(bytes: &[u8]) -> f64 {
    let exponent = i32::from(u16::from_be_bytes([bytes[0] & 0x7F, bytes[1]]));
    let mantissa = u64::from_be_bytes([
        bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9],
    ]);
    if exponent == 0 && mantissa == 0 {
        return 0.0;
    }
    let value = mantissa as f64 * 2f64.powi(exponent - 16_383 - 63);
    if bytes[0] & 0x80 != 0 { -value } else { value }
}

impl Source for Wav {
    fn rate(&self) -> u32 {
        self.rate
    }

    /// A mono file plays on both sides; one of more channels plays its
    /// first two, front left and right.
    fn fill(&mut self, out: &mut [f32]) -> Result<usize, String> {
        let width = self.encoding.bytes();
        let frame = width * self.channels;
        let frames = (out.len() / 2).min(self.frames() - self.at);
        for (index, pair) in out
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .take(frames)
            .enumerate()
        {
            let start = self.data.start + (self.at + index) * frame;
            let left = self.encoding.sample(&self.bytes[start..]);
            let right = if self.channels > 1 {
                self.encoding.sample(&self.bytes[start + width..])
            } else {
                left
            };
            *pair = [left, right];
        }
        self.at += frames;
        Ok(frames)
    }

    fn rewind(&mut self) -> Result<(), String> {
        self.at = 0;
        Ok(())
    }
}

/// An AIFF file of `frames` (interleaved stereo 16-bit, big-endian) at
/// `rate`, as macOS's sounds are.
#[cfg(test)]
pub(crate) fn aiff_file(rate: u32, frames: &[[i16; 2]]) -> Vec<u8> {
    let data = (frames.len() * 4) as u32;
    let mut file = Vec::with_capacity(54 + data as usize);
    file.extend_from_slice(b"FORM");
    file.extend_from_slice(&(46 + data).to_be_bytes());
    file.extend_from_slice(b"AIFFCOMM");
    file.extend_from_slice(&18u32.to_be_bytes());
    file.extend_from_slice(&2u16.to_be_bytes());
    file.extend_from_slice(&(frames.len() as u32).to_be_bytes());
    file.extend_from_slice(&16u16.to_be_bytes());
    // The rate as an 80-bit extended float: its mantissa's highest bit set,
    // the exponent saying where the point is.
    let shift = 31 - rate.leading_zeros();
    file.extend_from_slice(&(16_383 + shift as u16).to_be_bytes());
    file.extend_from_slice(&(u64::from(rate) << (63 - shift)).to_be_bytes());
    file.extend_from_slice(b"SSND");
    file.extend_from_slice(&(8 + data).to_be_bytes());
    file.extend_from_slice(&[0; 8]);
    for [left, right] in frames {
        file.extend_from_slice(&left.to_be_bytes());
        file.extend_from_slice(&right.to_be_bytes());
    }
    file
}

/// A WAV file of `frames` (interleaved stereo 16-bit) at `rate`: what the
/// tests write as a package's own sound.
#[cfg(test)]
pub(crate) fn wav_file(rate: u32, frames: &[[i16; 2]]) -> Vec<u8> {
    let data = (frames.len() * 4) as u32;
    let mut file = Vec::with_capacity(44 + data as usize);
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + data).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&2u16.to_le_bytes());
    file.extend_from_slice(&rate.to_le_bytes());
    file.extend_from_slice(&(rate * 4).to_le_bytes());
    file.extend_from_slice(&4u16.to_le_bytes());
    file.extend_from_slice(&16u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&data.to_le_bytes());
    for [left, right] in frames {
        file.extend_from_slice(&left.to_le_bytes());
        file.extend_from_slice(&right.to_le_bytes());
    }
    file
}
