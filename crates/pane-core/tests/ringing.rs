//! The ringing sound through the launcher's public interface (ADR 0053,
//! #556), with the alerts fixture (`guests/fixtures/alerts`), a real
//! guest that rings through `pane:extension/alerts` and keeps the handles,
//! and the runtime's stand-in sound output, so the tests make no noise and
//! see what played. Each way the extension's code can end stops the sound
//! at once — the handle dropped; the instance gone by a crash, a stopped
//! call or an unresponsive stop; a disable, a pause, a reload, an update or
//! an uninstall ending the generation; Pane quitting — and leaves no
//! ringing on the generation's undo list. A sound stops by itself too: as
//! often as asked, or at the runtime's limit (here a test's short one). The
//! limits and the refusals answer the extension, which is never paused for
//! them.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::{Launcher, Limits, PackageIdentity, Runtime, SavedData, StandInSound, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{select_title, to_root};

/// How long a guest call, a sound starting or stopping may take: compiling
/// the guest once is included; a slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(20);

/// The fixture's package: its command, and the settings sample's Greeting,
/// whose "Crash" item crashes another component of the same package.
const MANIFEST: &str = r#"{
  "manifestVersion": 1,
  "title": "Alerts fixture",
  "version": "0.1.0",
  "apiVersion": "0.2",
  "commands": [
    { "id": "fixture", "title": "Alerts fixture", "component": "alerts_fixture.wasm",
      "rootResults": true },
    { "id": "greeting", "title": "Greeting", "component": "sample_settings.wasm" }
  ]
}"#;

/// A built guest under `target/guests`.
fn built(file: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(file);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// A Pane with the alerts fixture installed, playing through a stand-in.
struct Installed {
    _sources: TempDir,
    _cache: TempDir,
    data: TempDir,
    runtime: Runtime,
    launcher: Launcher,
    folder: PathBuf,
    identity: PackageIdentity,
    sound: Arc<StandInSound>,
}

impl Installed {
    /// Installs the fixture with the settings `settings` saved for it.
    fn new(settings: &[(&str, &str)]) -> Installed {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let runtime = Runtime::start_with_cache(cache.path().to_path_buf()).unwrap();
        let sound = StandInSound::new();
        runtime.use_stand_in_sound(sound.clone());
        let folder = sources.path().join("alerts");
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            built("alerts_fixture.wasm"),
            folder.join("alerts_fixture.wasm"),
        )
        .unwrap();
        fs::copy(
            built("sample_settings.wasm"),
            folder.join("sample_settings.wasm"),
        )
        .unwrap();
        fs::write(folder.join("pane.json"), MANIFEST).unwrap();
        let identity = PackageIdentity::local(&folder).unwrap();
        if !settings.is_empty() {
            let mut saved = serde_json::json!({ "version": 1, "packages": {} });
            for (key, value) in settings {
                saved["packages"][identity.key()][key] = (*value).into();
            }
            let file = data.path().join("extensions/settings.json");
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, serde_json::to_string(&saved).unwrap()).unwrap();
        }
        // Started once the settings are saved: Pane reads them as it starts.
        let launcher =
            Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result("Installed Alerts fixture".into())
        );
        Installed {
            _sources: sources,
            _cache: cache,
            data,
            runtime,
            launcher,
            folder,
            identity,
            sound,
        }
    }

    /// Opens the fixture's command and runs its item titled `item`,
    /// answering what it showed.
    fn run(&self, item: &str) -> Status {
        to_root(&self.launcher);
        block_on(self.launcher.set_query(""));
        select_title(&self.launcher, "Alerts fixture");
        block_on(self.launcher.activate_selected());
        select_title(&self.launcher, item);
        block_on(self.launcher.activate_selected());
        shown(&self.launcher)
    }

    /// Rings the alarm until stopped, and checks it plays.
    fn ring_the_alarm(&self) {
        assert_eq!(
            self.run("Ring the alarm"),
            Status::Result("Ringing 1 sounds".into())
        );
        self.assert_playing(1);
    }

    /// `sounds` sounds play: the stand-in has as many streams open, taking
    /// frames, and the undo list holds a ringing for each.
    fn assert_playing(&self, sounds: usize) {
        until("the sound to play", || {
            self.sound.playing() == sounds && self.sound.frames().iter().all(|frames| *frames > 0)
        });
        let ringing = self
            .launcher
            .undo_list(&self.identity)
            .into_iter()
            .filter(|what| *what == "ringing sound")
            .count();
        assert_eq!(ringing, sounds);
    }

    /// The sound stopped: no stream is open, nothing more is written, the
    /// runtime holds no ringing and the undo list none.
    fn assert_stopped(&self) {
        until("the sound to stop", || self.sound.playing() == 0);
        let written = self.sound.frames();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(self.sound.frames(), written, "the sound went on");
        until("the ringing to be let go", || {
            self.runtime.ringing_sounds() == 0
        });
        assert!(
            !self
                .launcher
                .undo_list(&self.identity)
                .contains(&"ringing sound"),
            "{:?}",
            self.launcher.undo_list(&self.identity)
        );
    }

    /// Whether the package is enabled and not paused: none of the
    /// ringing's refusals and ends is a reason to pause it.
    fn assert_not_paused(&self) {
        let package = self
            .launcher
            .packages()
            .into_iter()
            .find(|package| package.identity == self.identity)
            .unwrap();
        assert!(package.enabled);
        assert_eq!(
            self.run("Drop the sounds"),
            Status::Result("Dropped the sounds".into())
        );
    }

    /// The installed package's folder, where its own sounds are.
    fn location(&self) -> PathBuf {
        self.launcher
            .packages()
            .into_iter()
            .find(|package| package.identity == self.identity)
            .unwrap()
            .location
    }

    /// What Pane wrote to the package's extension log.
    fn log(&self) -> Vec<String> {
        self.launcher
            .extension_log(&self.identity)
            .into_iter()
            .map(|line| line.text)
            .collect()
    }
}

/// Waits until `done` holds, saying `what` it waited for.
fn until(what: &str, done: impl Fn() -> bool) {
    let began = Instant::now();
    while !done() {
        assert!(began.elapsed() < PROMPTLY, "waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// A WAV file of `frames` stereo 16-bit frames at 8 kHz, every sample
/// `level`.
fn wav(path: &Path, frames: usize, level: i16) {
    let data = (frames * 4) as u32;
    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + data).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    // The format: 16 bytes of it, integer samples, two channels, 8,000
    // frames a second of 4 bytes each, 16 bits a sample.
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&2u16.to_le_bytes());
    file.extend_from_slice(&8_000u32.to_le_bytes());
    file.extend_from_slice(&32_000u32.to_le_bytes());
    file.extend_from_slice(&4u16.to_le_bytes());
    file.extend_from_slice(&16u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&data.to_le_bytes());
    for _ in 0..frames * 2 {
        file.extend_from_slice(&level.to_le_bytes());
    }
    fs::write(path, file).unwrap();
}

#[test]
fn dropping_the_handle_stops_the_sound() {
    let installed = Installed::new(&[]);
    installed.ring_the_alarm();

    assert_eq!(
        installed.run("Drop the sounds"),
        Status::Result("Dropped the sounds".into())
    );

    installed.assert_stopped();
    // The instance lives on: only its sound ended.
    assert_eq!(
        installed.launcher.undo_list(&installed.identity),
        ["extension instance"]
    );
}

#[test]
fn a_crash_stops_the_sound() {
    let installed = Installed::new(&[]);
    installed.ring_the_alarm();

    let crashed = installed.run("Ring and crash");

    assert!(
        matches!(&crashed, Status::Error(text) if text.starts_with("The extension crashed")),
        "{crashed:?}"
    );
    installed.assert_stopped();
    assert_eq!(
        installed.launcher.undo_list(&installed.identity),
        Vec::<&str>::new()
    );
}

#[test]
fn an_unresponsive_stop_stops_the_sound() {
    let installed = Installed::new(&[]);
    // The instance starts before the limits shrink: starting is not what is
    // checked.
    installed.ring_the_alarm();
    installed.runtime.set_limits(Limits {
        compute: Duration::from_millis(500),
        warn: Duration::from_millis(300),
        unresponsive: Duration::from_millis(5_000),
    });

    let stopped = installed.run("Ring and spin");

    assert!(
        matches!(&stopped, Status::Error(text) if text.starts_with("The extension stopped responding")),
        "{stopped:?}"
    );
    installed.assert_stopped();
    assert_eq!(
        installed.launcher.undo_list(&installed.identity),
        Vec::<&str>::new()
    );
}

/// Root search's query "ring and wait" has the fixture ring, then wait;
/// another query stops that call where it waits, and its instance with it.
#[test]
fn a_stopped_call_stops_the_sound() {
    let installed = Installed::new(&[]);
    to_root(&installed.launcher);

    // The search waits for the fixture's answer, so it runs beside the
    // test, which types on meanwhile.
    let waiting = {
        let asking = installed.launcher.set_query("ring and wait");
        thread::spawn(move || block_on(asking))
    };
    installed.assert_playing(1);
    block_on(installed.launcher.set_query("something else"));

    installed.assert_stopped();
    waiting.join().unwrap();
}

#[test]
fn disabling_stops_the_sound() {
    let installed = Installed::new(&[]);
    installed.ring_the_alarm();

    block_on(installed.launcher.set_enabled(&installed.identity, false));

    installed.assert_stopped();
    assert_eq!(
        installed.launcher.undo_list(&installed.identity),
        Vec::<&str>::new()
    );
}

/// The pause comes from another command of the same package, the settings
/// sample's Greeting, crashing three times: the fixture's instance is not
/// the one that crashes, so only the pause ends its sound.
#[test]
fn pausing_stops_the_sound() {
    let installed = Installed::new(&[]);
    installed.ring_the_alarm();

    for crash in 1..=3 {
        to_root(&installed.launcher);
        block_on(installed.launcher.set_query(""));
        select_title(&installed.launcher, "Greeting");
        block_on(installed.launcher.activate_selected());
        select_title(&installed.launcher, "Crash");
        block_on(installed.launcher.activate_selected());
        if crash < 3 {
            assert_eq!(installed.sound.playing(), 1, "crash {crash}");
        }
    }

    installed.assert_stopped();
    assert_eq!(
        installed.launcher.undo_list(&installed.identity),
        Vec::<&str>::new()
    );
}

#[test]
fn reloading_stops_the_sound() {
    let installed = Installed::new(&[]);
    installed.ring_the_alarm();

    block_on(installed.launcher.reload(&installed.identity));

    installed.assert_stopped();
    assert_eq!(
        installed.launcher.view().status,
        Status::Result("Reloaded Alerts fixture".into())
    );
    // The new code rings again.
    installed.ring_the_alarm();
}

#[test]
fn updating_stops_the_sound() {
    let installed = Installed::new(&[]);
    installed.ring_the_alarm();

    block_on(installed.launcher.preview_package(&installed.folder));
    select_title(&installed.launcher, "Update");
    block_on(installed.launcher.activate_selected());

    installed.assert_stopped();
    assert_eq!(
        installed.launcher.view().status,
        Status::Result("Updated Alerts fixture to 0.1.0".into())
    );
}

#[test]
fn uninstalling_stops_the_sound() {
    let installed = Installed::new(&[]);
    installed.ring_the_alarm();

    block_on(
        installed
            .launcher
            .uninstall(&installed.identity, SavedData::Keep),
    );

    installed.assert_stopped();
    assert_eq!(block_on(installed.runtime.running()), Vec::<PathBuf>::new());
}

/// Pane quitting (`main.rs` quits the runtime as the app quits) stops
/// every sound, and none starts afterwards.
#[test]
fn quitting_stops_the_sound() {
    let installed = Installed::new(&[]);
    installed.ring_the_alarm();

    installed.runtime.quit();

    until("the sound to stop", || installed.sound.playing() == 0);
    assert_eq!(installed.runtime.ringing_sounds(), 0);
}

#[test]
fn a_package_rings_at_most_two_sounds_at_once() {
    let installed = Installed::new(&[]);
    installed.ring_the_alarm();
    assert_eq!(
        installed.run("Ring the alarm"),
        Status::Result("Ringing 2 sounds".into())
    );
    installed.assert_playing(2);

    assert_eq!(
        installed.run("Ring the alarm"),
        Status::Result(
            "refused: limit: this extension already rings 2 sounds, Pane's limit for one \
             extension; drop one to ring another"
                .into()
        )
    );

    installed.assert_playing(2);
    installed.assert_not_paused();
    installed.assert_stopped();
    // Room again: it rings.
    installed.ring_the_alarm();
}

/// The package's own sound, by a path relative to its folder, plays as
/// often as asked at the volume asked, and ends by itself.
#[test]
fn a_package_s_own_sound_plays_as_often_as_asked_and_ends_by_itself() {
    let installed = Installed::new(&[
        ("file", "sounds/beep.wav"),
        ("repeat", "3"),
        ("volume", "50"),
    ]);
    fs::create_dir_all(installed.location().join("sounds")).unwrap();
    wav(&installed.location().join("sounds/beep.wav"), 1000, 16_384);

    assert_eq!(
        installed.run("Ring the file"),
        Status::Result("Ringing 1 sounds".into())
    );

    installed.assert_stopped();
    assert_eq!(installed.sound.frames(), [3000]);
    assert_eq!(installed.sound.rates(), [8_000]);
    assert_eq!(installed.sound.peaks(), [0.25]);
}

#[test]
fn times_outside_its_bounds_is_refused() {
    for times in ["0", "101"] {
        let installed = Installed::new(&[("file", "missing.wav"), ("repeat", times)]);

        assert_eq!(
            installed.run("Ring the file"),
            Status::Result(format!(
                "refused: failed: `times` asks for the sound {times} times; Pane plays a sound \
                 1 to 100 times"
            ))
        );

        assert_eq!(installed.sound.opened(), 0);
        installed.assert_not_paused();
    }
}

#[test]
fn a_file_that_cannot_be_read_fails_and_pauses_nothing() {
    let installed = Installed::new(&[("file", "missing.wav")]);

    let refused = installed.run("Ring the file");

    let Status::Result(text) = &refused else {
        panic!("{refused:?}");
    };
    assert!(
        text.starts_with("refused: failed: ") && text.contains("missing.wav cannot be read"),
        "{text}"
    );
    assert_eq!(installed.sound.opened(), 0);
    assert_eq!(installed.runtime.ringing_sounds(), 0);
    installed.assert_not_paused();
}

#[test]
fn no_output_device_is_not_available_and_pauses_nothing() {
    let installed = Installed::new(&[]);
    installed.sound.set_device(false);

    assert_eq!(
        installed.run("Ring the alarm"),
        Status::Result("refused: not-available: there is no sound output device".into())
    );

    assert_eq!(installed.runtime.ringing_sounds(), 0);
    installed.assert_not_paused();
    installed.sound.set_device(true);
    installed.ring_the_alarm();
}

#[test]
fn a_device_lost_mid_ring_ends_it_and_says_so_in_the_extension_log() {
    let installed = Installed::new(&[]);
    installed.ring_the_alarm();

    installed.sound.lose_device();

    installed.assert_stopped();
    let log = installed.log();
    assert!(
        log.contains(&"The ringing sound stopped: the sound output device was removed".into()),
        "{log:?}"
    );
    installed.assert_not_paused();
}

/// Every ringing stops by itself at the runtime's limit, 10 minutes, here
/// the test's 300 milliseconds, however it was asked.
#[test]
fn a_ringing_stops_by_itself_at_the_limit() {
    let installed = Installed::new(&[]);
    installed
        .runtime
        .set_ringing_limit(Duration::from_millis(300));
    assert_eq!(
        installed.run("Ring the alarm"),
        Status::Result("Ringing 1 sounds".into())
    );

    installed.assert_stopped();
    assert_eq!(installed.sound.opened(), 1);
    assert!(installed.sound.frames()[0] > 0);

    let log = installed.log();
    assert!(
        log.iter()
            .any(|line| line.contains("Pane's limit for one ringing")),
        "{log:?}"
    );
    installed.assert_not_paused();
}

/// Where Pane has no adapter for the system's notification sound yet, it
/// is not available: an answer, not a failure.
#[cfg(not(windows))]
#[test]
fn the_system_sound_is_not_available_here_yet() {
    let installed = Installed::new(&[]);

    assert_eq!(
        installed.run("Ring the system sound"),
        Status::Result(
            "refused: not-available: the system's notification sound is not available on this \
             system yet"
                .into()
        )
    );
    installed.assert_not_paused();
}

/// Saved data outlives the sound: what the package saved is kept.
#[test]
fn saved_settings_are_kept_after_the_sound_stops() {
    let installed = Installed::new(&[("file", "kept.wav")]);
    installed.ring_the_alarm();
    block_on(installed.launcher.set_enabled(&installed.identity, false));
    installed.assert_stopped();

    let saved = fs::read_to_string(installed.data.path().join("extensions/settings.json")).unwrap();
    assert!(saved.contains("kept.wav"), "{saved}");
}
