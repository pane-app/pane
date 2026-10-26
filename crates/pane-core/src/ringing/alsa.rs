//! The ringing sound on Linux: ALSA's PCM interface, on the ringing's own
//! thread. ALSA's `default` device is what every Linux sound setup serves:
//! PipeWire and PulseAudio route it to themselves (their ALSA plugins), and
//! a system with neither plays it on the sound card through ALSA's own
//! mixing (`dmix`). So Pane assumes no sound server, and the sound mixes
//! with every other application's at the output's own volume (Pane never
//! changes it).
//!
//! `libasound.so.2` is loaded when the first sound rings, not linked: a
//! system without it answers `not-available` saying so, Pane itself starts
//! without it, and building Pane needs no ALSA headers. A `default` device
//! that cannot be opened (no sound card, no server) is no output device:
//! `not-available` too. The device is opened without blocking, so one
//! another program holds answers at once rather than waiting for it.

use std::ffi::{CStr, CString, c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::sync::OnceLock;

use libloading::Library;

use super::{OpenError, Output, Sink};

/// The ALSA library's file, as every distribution installs it.
const LIBRARY: &str = "libasound.so.2";

/// `SND_PCM_STREAM_PLAYBACK`.
const PLAYBACK: c_int = 0;
/// `SND_PCM_NONBLOCK`.
const NONBLOCK: c_int = 1;
/// `SND_PCM_FORMAT_FLOAT_LE`: 32-bit float samples.
const FLOAT_LE: c_int = 14;
/// `SND_PCM_ACCESS_RW_INTERLEAVED`.
const RW_INTERLEAVED: c_int = 3;
/// `SND_PCM_STATE_PREPARED`: set up, not started.
const PREPARED: c_int = 2;

/// How much sound ALSA holds ahead of the device, in microseconds: enough
/// that a busy moment never starves it, short enough that a stop is heard
/// at once (the stream is dropped, not drained).
const LATENCY_US: c_uint = 200_000;

/// The errors a stream answers that mean its device went away.
const GONE: [c_int; 2] = [libc::ENODEV, libc::EBADFD];

/// An open PCM, as ALSA hands it out.
type Pcm = *mut c_void;

/// The functions of ALSA's library Pane calls, looked up once.
struct Alsa {
    open: unsafe extern "C" fn(*mut Pcm, *const c_char, c_int, c_int) -> c_int,
    set_params: unsafe extern "C" fn(Pcm, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int,
    get_params: unsafe extern "C" fn(Pcm, *mut c_ulong, *mut c_ulong) -> c_int,
    avail_update: unsafe extern "C" fn(Pcm) -> c_long,
    writei: unsafe extern "C" fn(Pcm, *const c_void, c_ulong) -> c_long,
    recover: unsafe extern "C" fn(Pcm, c_int, c_int) -> c_int,
    state: unsafe extern "C" fn(Pcm) -> c_int,
    start: unsafe extern "C" fn(Pcm) -> c_int,
    drop: unsafe extern "C" fn(Pcm) -> c_int,
    close: unsafe extern "C" fn(Pcm) -> c_int,
    strerror: unsafe extern "C" fn(c_int) -> *const c_char,
    /// Kept loaded for as long as Pane runs: the functions point into it.
    _library: Library,
}

impl Alsa {
    /// ALSA's library, loaded at the first call; why not, if it cannot be.
    fn get() -> Result<&'static Alsa, String> {
        static ALSA: OnceLock<Result<Alsa, String>> = OnceLock::new();
        ALSA.get_or_init(Alsa::load).as_ref().map_err(Clone::clone)
    }

    fn load() -> Result<Alsa, String> {
        let unavailable = |error: libloading::Error| {
            format!(
                "a ringing sound plays through ALSA, and this system has no ALSA library \
                 ({LIBRARY}): {error}"
            )
        };
        // SAFETY: loading the system's ALSA library runs its initializers,
        // which have no requirements of the loading program.
        let library = unsafe { Library::new(LIBRARY) }.map_err(unavailable)?;
        // SAFETY: each symbol is looked up by its name in ALSA's library and
        // given the signature `asoundlib.h` declares for it (`snd_pcm_t *`
        // as an opaque pointer, `snd_pcm_uframes_t` and
        // `snd_pcm_sframes_t` as the C longs they are). The functions are
        // used only while `library` stays loaded: it is kept with them.
        unsafe {
            Ok(Alsa {
                open: *library.get(b"snd_pcm_open\0").map_err(unavailable)?,
                set_params: *library.get(b"snd_pcm_set_params\0").map_err(unavailable)?,
                get_params: *library.get(b"snd_pcm_get_params\0").map_err(unavailable)?,
                avail_update: *library
                    .get(b"snd_pcm_avail_update\0")
                    .map_err(unavailable)?,
                writei: *library.get(b"snd_pcm_writei\0").map_err(unavailable)?,
                recover: *library.get(b"snd_pcm_recover\0").map_err(unavailable)?,
                state: *library.get(b"snd_pcm_state\0").map_err(unavailable)?,
                start: *library.get(b"snd_pcm_start\0").map_err(unavailable)?,
                drop: *library.get(b"snd_pcm_drop\0").map_err(unavailable)?,
                close: *library.get(b"snd_pcm_close\0").map_err(unavailable)?,
                strerror: *library.get(b"snd_strerror\0").map_err(unavailable)?,
                _library: library,
            })
        }
    }

    /// What ALSA says error `code` (negative) is.
    fn says(&self, code: c_int) -> String {
        // SAFETY: snd_strerror answers a static string for any code.
        let text = unsafe { (self.strerror)(code) };
        if text.is_null() {
            return format!("error {code}");
        }
        // SAFETY: a non-null answer is a NUL-terminated static string.
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    }

    /// Why a stream stopped working, from the error `code` it answered.
    fn lost(&self, code: c_int) -> String {
        if GONE.contains(&-code) {
            "the sound output device was removed".into()
        } else {
            format!("the sound output device failed: {}", self.says(code))
        }
    }
}

/// The system's audio output through ALSA, on the PCM device `device`:
/// `default` for Pane, another name for a test.
pub(super) struct AlsaOutput {
    device: CString,
}

impl AlsaOutput {
    /// ALSA's `default` device: the sound server's, or the sound card's.
    pub(super) fn default_device() -> AlsaOutput {
        AlsaOutput::named("default")
    }

    pub(super) fn named(device: &str) -> AlsaOutput {
        AlsaOutput {
            device: CString::new(device).expect("an ALSA device name has no NUL"),
        }
    }
}

impl Output for AlsaOutput {
    fn unavailable(&self) -> Option<String> {
        Alsa::get().err()
    }

    fn open(&self, rate: u32) -> Result<Box<dyn Sink>, OpenError> {
        let alsa = Alsa::get().map_err(OpenError::NoDevice)?;
        let mut pcm: Pcm = std::ptr::null_mut();
        // SAFETY: `pcm` is written by the call when it succeeds; the name
        // is a NUL-terminated string that outlives it.
        let opened = unsafe { (alsa.open)(&mut pcm, self.device.as_ptr(), PLAYBACK, NONBLOCK) };
        if opened < 0 {
            return Err(if -opened == libc::EBUSY {
                OpenError::Failed("the sound output device is busy with another program".into())
            } else {
                OpenError::NoDevice(format!(
                    "there is no sound output device (ALSA's `{}` device: {})",
                    self.device.to_string_lossy(),
                    alsa.says(opened)
                ))
            });
        }
        // From here the stream closes itself if anything below fails.
        let mut stream = Stream {
            alsa,
            pcm,
            buffer: 0,
        };
        // SAFETY: an open PCM; stereo float frames at the sound's rate,
        // which ALSA's plug layer converts to the device's when it differs.
        let set =
            unsafe { (alsa.set_params)(pcm, FLOAT_LE, RW_INTERLEAVED, 2, rate, 1, LATENCY_US) };
        if set < 0 {
            return Err(OpenError::Failed(format!(
                "the sound output device refused the sound: {}",
                alsa.says(set)
            )));
        }
        let (mut buffer, mut period) = (0, 0);
        // SAFETY: an open PCM whose parameters were set; both are written.
        let got = unsafe { (alsa.get_params)(pcm, &mut buffer, &mut period) };
        if got < 0 {
            return Err(OpenError::Failed(format!(
                "the sound output device refused the sound: {}",
                alsa.says(got)
            )));
        }
        stream.buffer = buffer as usize;
        Ok(Box::new(stream))
    }
}

/// An open ALSA stream, used by one ringing's thread.
struct Stream {
    alsa: &'static Alsa,
    pcm: Pcm,
    /// The frames the device's buffer holds.
    buffer: usize,
}

impl Stream {
    /// The frames the stream can take now, recovering it once from an
    /// underrun (the sound ran dry, as at its end).
    fn available(&mut self) -> Result<usize, c_int> {
        // SAFETY: an open PCM.
        let available = unsafe { (self.alsa.avail_update)(self.pcm) };
        if available >= 0 {
            return Ok(available as usize);
        }
        let error = available as c_int;
        // SAFETY: an open PCM; a silent recovery of an underrun or a
        // suspend, which answers the error back when it cannot.
        let recovered = unsafe { (self.alsa.recover)(self.pcm, error, 1) };
        if recovered < 0 {
            return Err(recovered);
        }
        // SAFETY: as above.
        let available = unsafe { (self.alsa.avail_update)(self.pcm) };
        if available < 0 {
            return Err(available as c_int);
        }
        Ok(available as usize)
    }
}

impl Sink for Stream {
    fn room(&mut self) -> Result<usize, String> {
        self.available().map_err(|code| self.alsa.lost(code))
    }

    fn write(&mut self, frames: &[f32]) -> Result<(), String> {
        let mut written = 0;
        let count = frames.len() / 2;
        while written < count {
            // SAFETY: an open PCM; the frames are interleaved stereo floats,
            // `count - written` of them from `written` on, which ALSA
            // copies before it returns.
            let wrote = unsafe {
                (self.alsa.writei)(
                    self.pcm,
                    frames[written * 2..].as_ptr().cast(),
                    (count - written) as c_ulong,
                )
            };
            if wrote >= 0 {
                written += wrote as usize;
                continue;
            }
            let error = wrote as c_int;
            if -error == libc::EAGAIN {
                // Full after all: the rest is dropped, as more than the
                // room was never asked for.
                break;
            }
            // SAFETY: as in `available`.
            if unsafe { (self.alsa.recover)(self.pcm, error, 1) } < 0 {
                return Err(self.alsa.lost(error));
            }
        }
        Ok(())
    }

    fn idle(&mut self) -> Result<bool, String> {
        // SAFETY: an open PCM.
        if unsafe { (self.alsa.state)(self.pcm) } == PREPARED {
            // A sound shorter than the device's buffer never reached the
            // level that starts the stream by itself: it starts now.
            // SAFETY: an open, prepared PCM holding the sound.
            let started = unsafe { (self.alsa.start)(self.pcm) };
            if started < 0 {
                return Err(self.alsa.lost(started));
            }
        }
        // SAFETY: an open PCM.
        let available = unsafe { (self.alsa.avail_update)(self.pcm) };
        if available < 0 {
            // It ran dry: everything written was played.
            return if -(available as c_int) == libc::EPIPE {
                Ok(true)
            } else {
                Err(self.alsa.lost(available as c_int))
            };
        }
        Ok(available as usize >= self.buffer)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the open PCM, stopped at once (what it holds is
        // dropped, not played) and closed once.
        unsafe {
            (self.alsa.drop)(self.pcm);
            (self.alsa.close)(self.pcm);
        }
    }
}

#[cfg(test)]
mod tests {
    //! The ALSA output against ALSA's `null` device, which plays nothing
    //! and takes whatever it is given, and against `default` (on a CI
    //! runner: no sound card, so no output device): opted in with
    //! `PANE_TEST_REAL_ALERTS`, which CI's Linux test jobs set
    //! (`.github/actions/setup`), so a developer's own `default` device
    //! never sounds.

    use super::*;

    use crate::real_alerts::{opted_in, record};

    #[test]
    fn the_null_device_takes_the_sound_and_dropping_the_stream_closes_it() {
        if !opted_in() {
            return;
        }
        let output = AlsaOutput::named("null");
        assert_eq!(output.unavailable(), None, "ALSA's library loads");
        let mut sink = output
            .open(48_000)
            .unwrap_or_else(|error| panic!("ALSA's null device did not open: {error:?}"));
        let room = sink.room().unwrap();
        assert!(room > 0, "the null device has no room");
        let frames = vec![0.25f32; room.min(4800) * 2];
        sink.write(&frames).unwrap();
        // The null device plays at once.
        let began = std::time::Instant::now();
        while !sink.idle().unwrap() {
            assert!(began.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        drop(sink);
    }

    /// What `default` answers on this machine, recorded in the test's
    /// output: a CI runner has no sound card, so no output device; a
    /// desktop plays through its sound server (it is not written to here).
    #[test]
    fn the_default_device_opens_or_says_there_is_no_output_device() {
        if !opted_in() {
            return;
        }
        match AlsaOutput::default_device().open(48_000) {
            Ok(_) => record("ALSA's default device opened"),
            Err(OpenError::NoDevice(why)) => record(&format!("ALSA's default device: {why}")),
            Err(OpenError::Failed(why)) => panic!("ALSA's default device failed: {why}"),
        }
    }
}
