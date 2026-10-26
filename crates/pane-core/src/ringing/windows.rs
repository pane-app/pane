//! The ringing sound on Windows: WASAPI in shared mode for the output, on
//! the ringing's own thread, so Windows mixes it with every other
//! application's sound at the device's own volume (Pane never changes it);
//! Media Foundation for the files Pane does not read itself (an MP3); and
//! the sound scheme's notification sound.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Media::Audio::{
    AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator,
    MMDeviceEnumerator, WAVEFORMATEX, eConsole, eRender,
};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFSourceReader, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION,
    MFAudioFormat_Float, MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Audio,
    MFSTARTUP_LITE, MFShutdown, MFStartup,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows::core::{HSTRING, PCWSTR};

use super::sounds::Source;
use super::{OpenError, Output, Sink};
use crate::windows_shell::Com;

/// `WAVE_FORMAT_IEEE_FLOAT`: 32-bit float samples.
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

/// How much sound WASAPI holds ahead of the device: enough that a busy
/// moment never starves it, short enough that a stop is heard at once
/// (the stream is stopped, not drained).
const BUFFER_100NS: i64 = 2_000_000;

/// The system's audio output through WASAPI.
pub(super) struct Wasapi;

impl Output for Wasapi {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn open(&self, rate: u32) -> Result<Box<dyn Sink>, OpenError> {
        let com = Com::new().map_err(OpenError::Failed)?;
        // SAFETY: plain COM creation of the device enumerator, with COM
        // held on this thread for as long as the stream lives.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(
                |error| {
                    OpenError::Failed(format!(
                        "Windows did not reach the audio devices: {}",
                        error.message()
                    ))
                },
            )?;
        // SAFETY: the default output endpoint, of the role Windows plays
        // ordinary sounds through. Windows answers an error when there is
        // none: no device is plugged in or enabled.
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
            .map_err(|_| OpenError::NoDevice("there is no sound output device".into()))?;
        let failed = |error: windows::core::Error| {
            OpenError::Failed(format!(
                "the sound output device refused the sound: {}",
                error.message()
            ))
        };
        // SAFETY: plain COM activation of the device's audio client.
        let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(failed)?;
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
            nChannels: 2,
            nSamplesPerSec: rate,
            nAvgBytesPerSec: rate * 8,
            nBlockAlign: 8,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        // SAFETY: `format` outlives the call. Shared mode, with Windows
        // converting the sound's rate to the device's mix format.
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                BUFFER_100NS,
                0,
                &format,
                None,
            )
        }
        .map_err(failed)?;
        // SAFETY: the client was initialized above.
        let capacity = unsafe { client.GetBufferSize() }.map_err(failed)?;
        // SAFETY: as above.
        let render: IAudioRenderClient = unsafe { client.GetService() }.map_err(failed)?;
        Ok(Box::new(Stream {
            render,
            client,
            capacity,
            started: false,
            _com: com,
        }))
    }
}

/// An open WASAPI stream. Its fields drop in order: the interfaces before
/// COM.
struct Stream {
    render: IAudioRenderClient,
    client: IAudioClient,
    capacity: u32,
    started: bool,
    _com: Com,
}

/// Why a stream stopped working: the device was removed, or it failed.
fn lost(error: windows::core::Error) -> String {
    if error.code() == AUDCLNT_E_DEVICE_INVALIDATED {
        "the sound output device was removed".into()
    } else {
        format!("the sound output device failed: {}", error.message())
    }
}

impl Sink for Stream {
    fn room(&mut self) -> Result<usize, String> {
        // SAFETY: the client is initialized.
        let queued = unsafe { self.client.GetCurrentPadding() }.map_err(lost)?;
        Ok(self.capacity.saturating_sub(queued) as usize)
    }

    fn write(&mut self, frames: &[f32]) -> Result<(), String> {
        let count = (frames.len() / 2) as u32;
        // SAFETY: at most the room the client said it has, so the buffer
        // it answers holds `count` frames of 8 bytes, which are written
        // whole before it is released.
        unsafe {
            let buffer = self.render.GetBuffer(count).map_err(lost)?;
            std::ptr::copy_nonoverlapping(frames.as_ptr().cast::<u8>(), buffer, count as usize * 8);
            self.render.ReleaseBuffer(count, 0).map_err(lost)?;
        }
        if !self.started {
            // SAFETY: the client is initialized, and holds the first sound.
            unsafe { self.client.Start() }.map_err(lost)?;
            self.started = true;
        }
        Ok(())
    }

    fn idle(&mut self) -> Result<bool, String> {
        // SAFETY: the client is initialized.
        Ok(unsafe { self.client.GetCurrentPadding() }.map_err(lost)? == 0)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the client is initialized; stopping one never started
        // does nothing.
        let _ = unsafe { self.client.Stop() };
    }
}

/// A file Media Foundation decodes, read as it plays.
pub(super) struct Decoded {
    path: PathBuf,
    rate: u32,
    channels: usize,
    reader: Option<IMFSourceReader>,
    /// What the last sample decoded holds that was not played yet.
    pending: Vec<f32>,
    /// Where the next frame of `pending` is.
    at: usize,
    _media: Media,
    _com: Com,
}

/// Media Foundation started on this thread, until dropped.
struct Media;

impl Media {
    fn start() -> Result<Media, String> {
        // SAFETY: paired with MFShutdown in `drop`.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) }
            .map_err(|error| format!("Media Foundation did not start: {}", error.message()))?;
        Ok(Media)
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        // SAFETY: paired with the successful MFStartup in `start`.
        let _ = unsafe { MFShutdown() };
    }
}

impl Decoded {
    /// Opens `path` and decodes its first sound, so a file Windows cannot
    /// decode is refused at once.
    pub(super) fn open(path: &Path) -> Result<Decoded, String> {
        let shown = path.display();
        let com = Com::new().map_err(|why| format!("{shown} cannot be decoded: {why}"))?;
        let media = Media::start().map_err(|why| format!("{shown} cannot be decoded: {why}"))?;
        let reader = reader(path)?;
        // SAFETY: the reader's first audio stream was set to float PCM.
        let format =
            unsafe { reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32) }
                .map_err(|error| format!("{shown} cannot be decoded: {}", error.message()))?;
        // SAFETY: plain attribute reads of a media type Windows answered.
        let (rate, channels) = unsafe {
            (
                format.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND),
                format.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS),
            )
        };
        let (rate, channels) = match (rate, channels) {
            (Ok(rate), Ok(channels)) if rate > 0 && channels > 0 => (rate, channels as usize),
            _ => return Err(format!("{shown} has no sound Windows can decode")),
        };
        let mut decoded = Decoded {
            path: path.to_path_buf(),
            rate,
            channels,
            reader: Some(reader),
            pending: Vec::new(),
            at: 0,
            _media: media,
            _com: com,
        };
        if !decoded.next()? {
            return Err(format!("{shown} holds no sound"));
        }
        Ok(decoded)
    }

    /// Decodes the next sample into `pending`: `false` at the end.
    fn next(&mut self) -> Result<bool, String> {
        let Some(reader) = &self.reader else {
            return Ok(false);
        };
        let shown = self.path.display();
        loop {
            let mut flags = 0u32;
            let mut sample = None;
            // SAFETY: a synchronous read of the first audio stream; the
            // out-pointers outlive the call.
            unsafe {
                reader.ReadSample(
                    MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )
            }
            .map_err(|error| format!("{shown} cannot be decoded: {}", error.message()))?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(false);
            }
            // A read may answer no sample (a gap, a format change): read on.
            let Some(sample) = sample else { continue };
            // SAFETY: the sample's buffers made one, locked only while its
            // bytes are copied; float PCM, as the reader was set to.
            let floats = unsafe {
                let buffer = sample
                    .ConvertToContiguousBuffer()
                    .map_err(|error| format!("{shown} cannot be decoded: {}", error.message()))?;
                let mut data = std::ptr::null_mut();
                let mut length = 0u32;
                buffer
                    .Lock(&mut data, None, Some(&mut length))
                    .map_err(|error| format!("{shown} cannot be decoded: {}", error.message()))?;
                let floats =
                    std::slice::from_raw_parts(data.cast::<f32>(), length as usize / 4).to_vec();
                let _ = buffer.Unlock();
                floats
            };
            self.pending = floats
                .chunks_exact(self.channels)
                .flat_map(|frame| [frame[0], *frame.get(1).unwrap_or(&frame[0])])
                .collect();
            self.at = 0;
            if !self.pending.is_empty() {
                return Ok(true);
            }
        }
    }
}

/// A source reader of `path`'s first audio stream, decoding to float PCM.
fn reader(path: &Path) -> Result<IMFSourceReader, String> {
    let shown = path.display();
    let url = HSTRING::from(path.as_os_str());
    // SAFETY: the path outlives the call; no attributes.
    let reader =
        unsafe { MFCreateSourceReaderFromURL(&url, None::<&IMFAttributes>) }.map_err(|error| {
            format!(
                "{shown} is not a sound file Windows can decode: {}",
                error.message()
            )
        })?;
    let failed =
        |error: windows::core::Error| format!("{shown} cannot be decoded: {}", error.message());
    // SAFETY: plain configuration of the reader: only its first audio
    // stream, as float PCM at the file's own rate and channels.
    unsafe {
        reader
            .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
            .map_err(failed)?;
        reader
            .SetStreamSelection(MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32, true)
            .map_err(|_| format!("{shown} holds no sound"))?;
        let wanted = MFCreateMediaType().map_err(failed)?;
        wanted
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(failed)?;
        wanted
            .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)
            .map_err(failed)?;
        reader
            .SetCurrentMediaType(MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32, None, &wanted)
            .map_err(failed)?;
    }
    Ok(reader)
}

impl Source for Decoded {
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

    /// Opens the file again: simpler than seeking, and done once per play.
    fn rewind(&mut self) -> Result<(), String> {
        self.reader = None;
        self.reader = Some(reader(&self.path)?);
        self.pending.clear();
        self.at = 0;
        Ok(())
    }
}

/// The file the user's sound scheme plays for notifications (Sound
/// settings' "Notification"), its environment variables expanded.
pub(super) fn notification_sound() -> Result<PathBuf, String> {
    let key = HSTRING::from(r"AppEvents\Schemes\Apps\.Default\Notification.Default\.Current");
    let mut size = 0u32;
    // SAFETY: with no buffer the call only reports the default value's
    // size in bytes.
    let answer = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if answer != ERROR_SUCCESS {
        return Err("the sound scheme names no notification sound".into());
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: `buffer` holds `size` bytes, which the call writes at most.
    let answer = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if answer != ERROR_SUCCESS {
        return Err("the sound scheme names no notification sound".into());
    }
    // The size counts the terminator.
    buffer.truncate((size as usize / 2).saturating_sub(1));
    let path = String::from_utf16_lossy(&buffer);
    if path.trim().is_empty() {
        return Err("the sound scheme plays no sound for notifications".into());
    }
    Ok(PathBuf::from(path))
}
