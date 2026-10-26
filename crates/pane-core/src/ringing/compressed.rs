//! A ringing's MP3 or Ogg Vorbis file, decoded by Symphonia (pure Rust)
//! as it plays: every MP3 off Windows, and every Ogg Vorbis file, the
//! freedesktop sound theme's sounds among them (#559). Windows' Media
//! Foundation still decodes an MP3 there (`windows`).
//!
//! The file was read whole already (at most 10 MiB); the decoder reads it
//! from memory, and rewinding starts it again on the same bytes. Any
//! channels play as stereo: a mono file on both sides, of more channels
//! the first two.

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use super::sounds::Source;

/// An MP3 or Ogg Vorbis file being played.
pub(super) struct Compressed {
    bytes: Arc<[u8]>,
    /// The file's extension, a hint to which format it is.
    extension: Option<String>,
    /// The file's path, as the answers name it.
    shown: String,
    rate: u32,
    started: Started,
    /// The last packet's frames as stereo, not all played yet.
    pending: Vec<f32>,
    /// Where the next frame of `pending` is.
    at: usize,
    /// The last packet's samples as the file has them, interleaved.
    samples: Vec<f32>,
}

/// The file's reader and its audio track's decoder.
struct Started {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
    rate: u32,
}

impl Compressed {
    /// Opens `bytes`, read from `path`, and decodes its first sound, so a
    /// file that cannot be decoded is refused at once.
    pub(super) fn open(path: &Path, bytes: Vec<u8>) -> Result<Compressed, String> {
        let shown = path.display().to_string();
        let bytes: Arc<[u8]> = bytes.into();
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        let started = start(&bytes, extension.as_deref())
            .map_err(|error| format!("{shown} is not a sound file Pane can play: {error}"))?;
        let mut compressed = Compressed {
            bytes,
            extension,
            shown,
            rate: started.rate,
            started,
            pending: Vec::new(),
            at: 0,
            samples: Vec::new(),
        };
        if !compressed.next()? {
            return Err(format!("{} holds no sound", compressed.shown));
        }
        Ok(compressed)
    }

    /// Decodes the next packet into `pending`: `false` at the end.
    fn next(&mut self) -> Result<bool, String> {
        let started = &mut self.started;
        loop {
            let packet = match started.reader.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(false),
                Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(error) => return Err(format!("{} cannot be decoded: {error}", self.shown)),
            };
            if packet.track_id != started.track {
                continue;
            }
            let decoded = match started.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                // A damaged frame is skipped, as players do.
                Err(Error::DecodeError(_)) => continue,
                Err(error) => return Err(format!("{} cannot be decoded: {error}", self.shown)),
            };
            let channels = decoded.spec().channels().count();
            if channels == 0 || decoded.frames() == 0 {
                continue;
            }
            decoded.copy_to_vec_interleaved(&mut self.samples);
            self.pending = self
                .samples
                .chunks_exact(channels)
                .flat_map(|frame| [frame[0], *frame.get(1).unwrap_or(&frame[0])])
                .collect();
            self.at = 0;
            return Ok(true);
        }
    }
}

/// The reader of `bytes` and the decoder of its first audio track.
fn start(bytes: &Arc<[u8]>, extension: Option<&str>) -> Result<Started, Error> {
    let stream = MediaSourceStream::new(
        Box::new(Cursor::new(bytes.clone())),
        MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }
    let reader = symphonia::default::get_probe().probe(
        &hint,
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let track = reader
        .default_track(TrackType::Audio)
        .ok_or(Error::Unsupported("it has no audio track"))?;
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or(Error::Unsupported("it has no audio track"))?;
    let rate = parameters
        .sample_rate
        .filter(|rate| (1_000..=384_000).contains(rate))
        .ok_or(Error::Unsupported("it has no sample rate Pane can play"))?;
    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(parameters, &AudioDecoderOptions::default())?;
    let track = track.id;
    Ok(Started {
        reader,
        decoder,
        track,
        rate,
    })
}

impl Source for Compressed {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn fill(&mut self, out: &mut [f32]) -> Result<usize, String> {
        if self.at * 2 >= self.pending.len() && !self.next()? {
            return Ok(0);
        }
        let from = self.at * 2;
        let samples = out.len().min(self.pending.len() - from) / 2 * 2;
        out[..samples].copy_from_slice(&self.pending[from..from + samples]);
        self.at += samples / 2;
        Ok(samples / 2)
    }

    /// Starts the decoder again on the same bytes.
    fn rewind(&mut self) -> Result<(), String> {
        self.started = start(&self.bytes, self.extension.as_deref())
            .map_err(|error| format!("{} cannot be decoded: {error}", self.shown))?;
        self.pending.clear();
        self.at = 0;
        Ok(())
    }
}
