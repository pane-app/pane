//! The ringing sound (`wit/alerts.wit`'s `ring`, ADR 0053): a sound an
//! extension starts and owns as an owned registration (ADR 0041), played
//! on a thread of Pane's own so that neither the window nor the runtime
//! ever waits for audio.
//!
//! A ringing lasts until the first of: its sound having played as often as
//! asked; the guest dropping its handle; the instance holding it going (a
//! crash, a stopped call, an unresponsive stop — the instance's
//! `GuestState` drop ends it); its generation ending (a disable, a pause, a
//! reload, an update, an uninstall: its entry on the generation's undo list
//! stops it); Pane quitting ([`Ringer::stop_all`]); the output device going
//! away mid-ring, which is written to the package's extension log; and
//! [`RINGING_LIMIT`], however it was asked. Whichever comes first takes the
//! entry off the registry and the undo list, and stopping it is setting a
//! flag the thread looks at every few milliseconds and waking it: nothing
//! waits for the thread to end.
//!
//! The sounds are read and decoded on the ringing's thread too, before
//! `ring` answers, so a file that cannot be read answers `failed` and a
//! missing output device `not-available`, while the guest's call awaits
//! the answer without holding the runtime thread. The audio itself goes
//! through an [`Output`]: Windows' WASAPI in shared mode (`windows`),
//! ALSA's `default` device on Linux, which PipeWire and PulseAudio serve
//! as well as a bare sound card (`alsa`, #559), an AudioToolbox audio
//! queue on macOS (`macos`, #559), none on any other system (every `ring`
//! answers `not-available`), and a stand-in sink for tests, which plays
//! nothing and records what it was given.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::Thread;
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use crate::extension_log::{ExtensionLogs, LogLevel};
use crate::generation::{Generation, Registration};

#[cfg(target_os = "linux")]
mod alsa;
#[cfg(target_os = "macos")]
mod macos;
mod sounds;
#[cfg(any(test, debug_assertions))]
mod stand_in;
#[cfg(windows)]
mod windows;

pub(crate) use sounds::Bundled;
#[cfg(any(test, debug_assertions))]
pub use stand_in::StandInSound;

/// How many sounds one package may ring at once.
pub(crate) const MAX_PER_PACKAGE: usize = 2;

/// How many sounds all packages together may ring at once.
pub(crate) const MAX_IN_ALL: usize = 8;

/// The most times `times` may ask a sound to play.
pub(crate) const MAX_TIMES: u32 = 100;

/// The largest sound file `ring` reads: 10 MiB.
pub(crate) const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// How long a ringing may last, however it was asked: an alarm nobody is
/// there to hear does not ring all day.
pub(crate) const RINGING_LIMIT: Duration = Duration::from_secs(10 * 60);

/// How often a ringing's thread tops up its output and looks whether it
/// was stopped. A stop wakes it at once.
const LOOK_EVERY: Duration = Duration::from_millis(10);

/// The most frames written to the output at one look.
const CHUNK: usize = 4800;

/// The handle a guest's `ringing` resource holds: the ringing's id. Public
/// because the runtime's bindings re-export it; the module stays private,
/// so it reaches no public API.
#[derive(Debug)]
pub struct RingingHandle(pub(crate) u64);

/// What `ring` plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Sound {
    /// One of the sounds Pane ships.
    Bundled(Bundled),
    /// The sound the system's sound scheme plays for notifications.
    System,
    /// A WAV or MP3 file, by its absolute path (a package-relative one was
    /// resolved already).
    File(PathBuf),
}

/// How often `ring` plays its sound. `Times` was checked to be within
/// 1..=[`MAX_TIMES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Repeat {
    Once,
    Times(u32),
    UntilStopped,
}

/// A ringing a guest asks for, with whose it is.
pub(crate) struct Ring {
    /// The package's identity key: whose limit it counts towards and
    /// whose extension log hears of it.
    pub(crate) owner: String,
    /// The component and the instance holding it: the instance going ends
    /// it.
    pub(crate) component: PathBuf,
    pub(crate) instance: (u64, u64),
    /// The generation it belongs to, whose end stops it; `None` for a
    /// command built into Pane, which runs as long as Pane.
    pub(crate) generation: Option<Generation>,
    pub(crate) sound: Sound,
    pub(crate) repeat: Repeat,
    /// 0 to 100, a percentage of the sound's full level.
    pub(crate) volume: u8,
}

/// Why `ring` did not start a sound: the WIT's `alerts-error` cases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RingError {
    NotAvailable(String),
    Limit(String),
    Failed(String),
}

/// Where a ringing's sound goes: the system's audio output, or a stand-in.
/// Opened on the ringing's own thread, which holds what it answers until
/// the ringing ends.
pub(crate) trait Output: Send + Sync {
    /// Why no sound can be played on this system at all, if none can: no
    /// adapter here yet. Checked before anything is read.
    fn unavailable(&self) -> Option<String>;

    /// An output stream for stereo 32-bit float frames at `rate` frames a
    /// second, or why there is none.
    fn open(&self, rate: u32) -> Result<Box<dyn Sink>, OpenError>;
}

/// Why an output could not be opened.
#[derive(Debug)]
pub(crate) enum OpenError {
    /// There is no output device: `not-available`.
    NoDevice(String),
    /// The device refused, or is busy with another program: `failed`.
    #[cfg_attr(
        not(any(windows, target_os = "linux", target_os = "macos")),
        allow(dead_code)
    )]
    Failed(String),
}

/// An open output stream, written by one ringing's thread. Dropping it
/// stops the sound at once.
pub(crate) trait Sink {
    /// How many frames it can take now, or why the device is gone.
    fn room(&mut self) -> Result<usize, String>;

    /// Queues `frames` (interleaved stereo, at most [`Sink::room`] of
    /// them), starting the stream with the first.
    fn write(&mut self, frames: &[f32]) -> Result<(), String>;

    /// Whether everything written has been played.
    fn idle(&mut self) -> Result<bool, String>;
}

/// No output on this system: every `ring` answers `not-available`.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
struct NoOutput;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
impl Output for NoOutput {
    fn unavailable(&self) -> Option<String> {
        Some("a ringing sound is not available on this system".into())
    }

    fn open(&self, _rate: u32) -> Result<Box<dyn Sink>, OpenError> {
        Err(OpenError::NoDevice(
            "a ringing sound is not available on this system".into(),
        ))
    }
}

/// The system's own output.
fn system_output() -> Arc<dyn Output> {
    #[cfg(windows)]
    {
        Arc::new(windows::Wasapi)
    }
    #[cfg(target_os = "linux")]
    {
        Arc::new(alsa::AlsaOutput::default_device())
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::AudioQueue)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Arc::new(NoOutput)
    }
}

/// Every ringing sound of the runtime's guests, shared by every runtime
/// thread: the registry of who rings what, and the output they play
/// through. Its lock is held only for short work, never while a thread is
/// waited for.
pub(crate) struct Ringer {
    state: Mutex<State>,
    output: Mutex<Arc<dyn Output>>,
    /// [`RINGING_LIMIT`], or a test's shorter one.
    limit: Mutex<Duration>,
    /// Where a ringing that ended badly says so: its package's extension
    /// log.
    logs: ExtensionLogs,
}

#[derive(Default)]
struct State {
    /// The next ringing's id, never reused.
    next: u64,
    /// Every ringing still playing, or starting.
    entries: Vec<Entry>,
    /// Set once Pane quits: nothing rings afterwards.
    quitting: bool,
}

/// One ringing, held until it ends. Dropping it stops the sound and takes
/// it off its generation's undo list, however it ended.
struct Entry {
    id: u64,
    owner: String,
    component: PathBuf,
    instance: (u64, u64),
    stop: Arc<Stop>,
    _undo: Option<Registration>,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

/// Tells a ringing's thread to stop, waking it.
#[derive(Default)]
struct Stop {
    stopped: AtomicBool,
    thread: Mutex<Option<Thread>>,
}

impl Stop {
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(thread) = lock(&self.thread).as_ref() {
            thread.unpark();
        }
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Waits up to `limit`, or until stopped.
    fn wait(&self, limit: Duration) {
        if !self.stopped() {
            std::thread::park_timeout(limit);
        }
    }
}

impl Ringer {
    pub(crate) fn new(logs: ExtensionLogs) -> Ringer {
        Ringer {
            state: Mutex::default(),
            output: Mutex::new(system_output()),
            limit: Mutex::new(RINGING_LIMIT),
            logs,
        }
    }

    /// Plays every later ringing through `output`.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn set_output(&self, output: Arc<dyn Output>) {
        *lock(&self.output) = output;
    }

    /// Stops every later ringing after `limit` instead of
    /// [`RINGING_LIMIT`].
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn set_limit(&self, limit: Duration) {
        *lock(&self.limit) = limit;
    }

    /// Starts `ring` on a thread of its own, once the limits allow it: the
    /// answer is [`Starting::started`]'s, once the sound was read and the
    /// output opened. The ringing is held (and counts towards the limits)
    /// from now on, so dropping what this answers before it started stops
    /// it.
    pub(crate) fn ring(self: &Arc<Self>, ring: Ring) -> Result<Starting, RingError> {
        let output = lock(&self.output).clone();
        if let Some(why) = output.unavailable() {
            return Err(RingError::NotAvailable(why));
        }
        let id = {
            let mut state = lock(&self.state);
            if state.quitting {
                return Err(RingError::NotAvailable(
                    "Pane is quitting; no sound starts".into(),
                ));
            }
            let package = state
                .entries
                .iter()
                .filter(|entry| entry.owner == ring.owner)
                .count();
            if package >= MAX_PER_PACKAGE {
                return Err(RingError::Limit(format!(
                    "this extension already rings {package} sounds, Pane's limit for one \
                     extension; drop one to ring another"
                )));
            }
            if state.entries.len() >= MAX_IN_ALL {
                return Err(RingError::Limit(format!(
                    "extensions already ring {MAX_IN_ALL} sounds, Pane's limit; one must stop \
                     before another rings"
                )));
            }
            let id = state.next;
            state.next += 1;
            id
        };
        let stop = Arc::new(Stop::default());
        // Recorded outside the lock: on a generation that ended already the
        // undo runs at once, and takes the lock.
        let undo = ring.generation.as_ref().map(|generation| {
            let ringer = Arc::downgrade(self);
            generation.on_end("ringing sound", move || {
                if let Some(ringer) = ringer.upgrade() {
                    ringer.release(id);
                }
                Ok(())
            })
        });
        lock(&self.state).entries.push(Entry {
            id,
            owner: ring.owner.clone(),
            component: ring.component.clone(),
            instance: ring.instance,
            stop: stop.clone(),
            _undo: undo,
        });
        // A generation that ended meanwhile undid nothing (the entry was not
        // pushed yet): it is undone now.
        if let Some(end) = ring.generation.as_ref().and_then(Generation::ended) {
            self.release(id);
            return Err(RingError::Failed(crate::registrations::replaced(end)));
        }
        let (answer, answered) = oneshot::channel();
        let playing = Playing {
            ringer: Arc::downgrade(self),
            id,
            stop,
            output,
            limit: *lock(&self.limit),
            log: Log {
                logs: self.logs.clone(),
                owner: ring.owner,
                generation: ring.generation.as_ref().map_or(0, Generation::number),
            },
        };
        let (sound, repeat, volume) = (ring.sound, ring.repeat, ring.volume.min(100));
        let spawned = std::thread::Builder::new()
            .name("pane-ringing".into())
            .spawn(move || playing.run(sound, repeat, volume, answer));
        if let Err(error) = spawned {
            self.release(id);
            return Err(RingError::Failed(format!(
                "Pane could not start the sound's thread: {error}"
            )));
        }
        Ok(Starting {
            ringer: Arc::downgrade(self),
            id,
            answered,
            kept: false,
        })
    }

    /// Stops the ringing `id` and takes it off its generation's undo list:
    /// the guest dropped its handle, or its start was given up. Nothing
    /// happens when it already ended.
    pub(crate) fn release(&self, id: u64) {
        let removed = {
            let mut state = lock(&self.state);
            state
                .entries
                .iter()
                .position(|entry| entry.id == id)
                .map(|at| state.entries.remove(at))
        };
        // Dropped with the lock let go: dropping it takes the generation's
        // undo list's lock.
        drop(removed);
    }

    /// Stops every ringing the instance `instance` of `component` holds:
    /// its store went, so nothing of its code survives.
    pub(crate) fn instance_gone(&self, component: &std::path::Path, instance: (u64, u64)) {
        let removed: Vec<Entry> = {
            let mut state = lock(&self.state);
            let (gone, kept) = std::mem::take(&mut state.entries)
                .into_iter()
                .partition(|entry| entry.instance == instance && entry.component == component);
            state.entries = kept;
            gone
        };
        drop(removed);
    }

    /// Pane is quitting: every ringing stops, and none starts afterwards.
    pub(crate) fn stop_all(&self) {
        let removed = {
            let mut state = lock(&self.state);
            state.quitting = true;
            std::mem::take(&mut state.entries)
        };
        drop(removed);
    }

    /// How many ringings are held now: playing, or starting. A diagnostic
    /// for tests.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn count(&self) -> usize {
        lock(&self.state).entries.len()
    }
}

/// A ringing whose thread is reading its sound and opening the output:
/// awaiting [`Starting::started`] answers whether it plays. Dropped before
/// that, it stops the ringing (the guest's call was stopped meanwhile).
pub(crate) struct Starting {
    ringer: Weak<Ringer>,
    id: u64,
    answered: oneshot::Receiver<Result<(), RingError>>,
    kept: bool,
}

impl Starting {
    /// The ringing's id once it plays, or why it does not (it was then
    /// released).
    pub(crate) async fn started(mut self) -> Result<u64, RingError> {
        let answer = (&mut self.answered).await.unwrap_or_else(|_| {
            Err(RingError::Failed(
                "the sound stopped before it started".into(),
            ))
        });
        if answer.is_ok() {
            self.kept = true;
        }
        answer.map(|()| self.id)
    }
}

impl Drop for Starting {
    fn drop(&mut self) {
        if !self.kept
            && let Some(ringer) = self.ringer.upgrade()
        {
            ringer.release(self.id);
        }
    }
}

/// Where a ringing that ended badly says so.
struct Log {
    logs: ExtensionLogs,
    owner: String,
    generation: u64,
}

impl Log {
    fn say(&self, level: LogLevel, text: &str) {
        self.logs.pane(&self.owner, self.generation, level, text);
    }
}

/// One ringing's thread: what it plays through, how long it may, and how
/// it is told to stop.
struct Playing {
    ringer: Weak<Ringer>,
    id: u64,
    stop: Arc<Stop>,
    output: Arc<dyn Output>,
    limit: Duration,
    log: Log,
}

/// How a ringing ended, for its extension log.
enum Ended {
    /// It played as often as asked, or was stopped: nothing to say.
    Quietly,
    /// It reached [`Playing::limit`].
    Limit,
    /// The device went away, or failed.
    Device(String),
    /// The sound could not be read any more.
    Sound(String),
}

impl Playing {
    /// Reads the sound, opens the output, answers whether it plays, and
    /// plays it until it ends however it does. The ringing is taken off
    /// the registry as the thread ends, even by a panic.
    fn run(
        self,
        sound: Sound,
        repeat: Repeat,
        volume: u8,
        answer: oneshot::Sender<Result<(), RingError>>,
    ) {
        let _finished = Finished(self.ringer.clone(), self.id);
        *lock(&self.stop.thread) = Some(std::thread::current());
        let started = sounds::open(&sound).and_then(|source| {
            let sink = self
                .output
                .open(source.rate())
                .map_err(|error| match error {
                    OpenError::NoDevice(why) => RingError::NotAvailable(why),
                    OpenError::Failed(why) => RingError::Failed(why),
                })?;
            Ok((source, sink))
        });
        let (mut source, mut sink) = match started {
            Ok(started) => started,
            Err(error) => {
                let _ = answer.send(Err(error));
                return;
            }
        };
        // A guest that went meanwhile gets no answer: the stop is set.
        let _ = answer.send(Ok(()));
        let ended = self.play(source.as_mut(), sink.as_mut(), repeat, volume);
        // The sound stops before anything is written.
        drop(sink);
        match ended {
            Ended::Quietly => {}
            Ended::Limit => self.log.say(
                LogLevel::Info,
                &format!(
                    "The ringing sound stopped after {} minutes, Pane's limit for one ringing",
                    self.limit.as_secs() / 60
                ),
            ),
            Ended::Device(why) => self
                .log
                .say(LogLevel::Warn, &format!("The ringing sound stopped: {why}")),
            Ended::Sound(why) => self
                .log
                .say(LogLevel::Warn, &format!("The ringing sound stopped: {why}")),
        }
    }

    /// Feeds `source` to `sink`, `repeat` times, at `volume`, until it
    /// ends.
    fn play(
        &self,
        source: &mut dyn sounds::Source,
        sink: &mut dyn Sink,
        repeat: Repeat,
        volume: u8,
    ) -> Ended {
        let times = match repeat {
            Repeat::Once => Some(1),
            Repeat::Times(times) => Some(times),
            Repeat::UntilStopped => None,
        };
        let gain = f32::from(volume) / 100.0;
        let began = Instant::now();
        let mut played = 0;
        let mut draining = false;
        let mut buffer = vec![0f32; CHUNK * 2];
        loop {
            if self.stop.stopped() {
                return Ended::Quietly;
            }
            if began.elapsed() >= self.limit {
                return Ended::Limit;
            }
            if draining {
                match sink.idle() {
                    Ok(true) => return Ended::Quietly,
                    Ok(false) => {}
                    Err(why) => return Ended::Device(why),
                }
            } else {
                let room = match sink.room() {
                    Ok(room) => room.min(CHUNK),
                    Err(why) => return Ended::Device(why),
                };
                let mut filled = 0;
                while filled < room {
                    let frames = match source.fill(&mut buffer[filled * 2..room * 2]) {
                        Ok(frames) => frames,
                        Err(why) => return Ended::Sound(why),
                    };
                    if frames > 0 {
                        filled += frames;
                        continue;
                    }
                    // The sound ended: again, or that was the last time.
                    played += 1;
                    if times.is_some_and(|times| played >= times) {
                        draining = true;
                        break;
                    }
                    if let Err(why) = source.rewind() {
                        return Ended::Sound(why);
                    }
                }
                if filled > 0 {
                    let frames = &mut buffer[..filled * 2];
                    if gain < 1.0 {
                        frames.iter_mut().for_each(|sample| *sample *= gain);
                    }
                    if let Err(why) = sink.write(frames) {
                        return Ended::Device(why);
                    }
                }
            }
            self.stop.wait(LOOK_EVERY);
        }
    }
}

/// Takes the ringing off the registry as its thread ends.
struct Finished(Weak<Ringer>, u64);

impl Drop for Finished {
    fn drop(&mut self) {
        if let Some(ringer) = self.0.upgrade() {
            ringer.release(self.1);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests;
