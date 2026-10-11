//! The `system-commands` host functions (#255, the Windows power
//! features' System Commands, ADR 0040): locking the screen, logging out,
//! restarting, shutting down, sleeping, hibernating, turning the displays
//! off and starting the screen saver, the audio commands (#265): the
//! volume of the default output device (Volume Up, Volume Down, Toggle
//! Mute, Set Volume) and Toggle Microphone Mute, and the bin, appearance
//! and device commands (#266): the Recycle Bin (Open, Empty), the system's
//! appearance, HDR, the desktop, the hidden files, the removable drives
//! and Bluetooth. The decisions are pure functions, tested here on every
//! system; the commands themselves are exercised through the launcher's
//! public interface with a recording fake of the system
//! (`support/system_commands.rs`), so no test ever locks, logs out,
//! restarts, shuts down, sleeps, hibernates, turns off the displays of a
//! real session, changes the volume or mutes a microphone, empties the
//! Recycle Bin, changes the appearance or the hidden files, ejects a
//! drive or toggles Bluetooth: the samples in Rust, JavaScript and
//! TypeScript call each host function and say what it answered, and the
//! System Commands default extension is acquired as Windows does — at
//! first setup, from the repository its pin names: the sample's package
//! in a repository of the test's own, served on 127.0.0.1
//! (`support/repo_server.rs`, `support/defaults.rs`), standing in for the
//! default's own repository, which lives outside this one (#301). The
//! sample calls each host function as it is: no confirmation, no HUD —
//! the composition the real extension adds is the extension's own, its
//! repository's to test, as the calculator's arithmetic was (#285) — so
//! those tests carry the default-extension mechanics: acquired with the
//! default's identity, enabled and disableable on its own, its command
//! listed and driven through the launcher's public interface, and which
//! host function ran per item. The Windows defaults are Windows-only, so
//! those tests run on Windows only.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::executor::block_on;
use pane_core::system_commands::{
    self, Appearance, BluetoothRadio, Capabilities, Command, Drive, HdrDisplay, HibernatePlan,
    Microphone, MicrophonePlan, NO_BLUETOOTH, NO_HDR, NO_MICROPHONE, NO_REMOVABLE_DRIVE, Outcome,
    PowerRequest, RecycleBin, SET_VOLUME_RANGE, SleepPlan, TogglePlan, Volume, bluetooth_plan,
    hdr_plan, hibernate_plan, microphone_plan, run, sleep_plan, toggled_appearance, toggled_mute,
    volume_down, volume_up,
};
use pane_core::{Launcher, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/defaults.rs"]
mod defaults;
#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/system_commands.rs"]
mod recording;
#[path = "support/repo_server.rs"]
mod repo_server;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guests;
use recording::{Done, RecordingSystemCommands};
use rows::select_title;

/// Every command, as the extension's `pane.json` lists them.
const COMMANDS: [Command; 21] = [
    Command::LockScreen,
    Command::LogOut,
    Command::Restart,
    Command::ShutDown,
    Command::Sleep,
    Command::Hibernate,
    Command::TurnOffDisplays,
    Command::StartScreenSaver,
    Command::VolumeUp,
    Command::VolumeDown,
    Command::ToggleMute,
    Command::SetVolume(40),
    Command::ToggleMicrophoneMute,
    Command::OpenRecycleBin,
    Command::EmptyRecycleBin,
    Command::ToggleAppearance,
    Command::ToggleHdr,
    Command::ShowDesktop,
    Command::ToggleHiddenFiles,
    Command::EjectRemovableDrives,
    Command::ToggleBluetooth,
];

/// `capabilities` as a computer with `modern_standby` and
/// `hibernation_file` set as asked.
fn capabilities(modern_standby: bool, hibernation_file: bool) -> Capabilities {
    Capabilities {
        modern_standby,
        hibernation_file,
    }
}

/// The volume of the fake at `level`, unmuted (or muted as `muted` says).
fn volume(level: u8, muted: bool) -> Volume {
    Volume { level, muted }
}

/// A microphone called `id`, muted as `muted` says.
fn microphone(id: &str, muted: bool) -> Microphone {
    Microphone {
        id: id.into(),
        muted,
    }
}

/// A display that can show HDR, called `id`, its advanced colour on as
/// `hdr` says.
fn display(id: &str, hdr: bool) -> HdrDisplay {
    HdrDisplay { id: id.into(), hdr }
}

/// A Bluetooth radio, called `id`, on as `on` says.
fn radio(id: &str, on: bool) -> BluetoothRadio {
    BluetoothRadio { id: id.into(), on }
}

/// A removable drive, called `id` (its letter and colon).
fn drive(id: &str) -> Drive {
    Drive { id: id.into() }
}

#[test]
fn the_destructive_set_is_logging_out_restarting_shutting_down_and_emptying_the_bin() {
    for command in COMMANDS {
        assert_eq!(
            command.destructive(),
            matches!(
                command,
                Command::LogOut | Command::Restart | Command::ShutDown | Command::EmptyRecycleBin
            ),
            "{command:?}"
        );
    }
}

#[test]
fn restart_and_shut_down_force_applications_closed_and_log_out_does_not() {
    assert!(Command::Restart.forces_applications_closed());
    assert!(Command::ShutDown.forces_applications_closed());
    assert!(!Command::LogOut.forces_applications_closed());
    for command in [
        Command::LockScreen,
        Command::Sleep,
        Command::Hibernate,
        Command::TurnOffDisplays,
        Command::StartScreenSaver,
        Command::VolumeUp,
        Command::VolumeDown,
        Command::ToggleMute,
        Command::SetVolume(40),
        Command::ToggleMicrophoneMute,
        Command::OpenRecycleBin,
        Command::EmptyRecycleBin,
        Command::ToggleAppearance,
        Command::ToggleHdr,
        Command::ShowDesktop,
        Command::ToggleHiddenFiles,
        Command::EjectRemovableDrives,
        Command::ToggleBluetooth,
    ] {
        assert!(!command.forces_applications_closed(), "{command:?}");
    }
}

#[test]
fn sleep_turns_the_displays_off_only_on_a_modern_standby_computer() {
    assert_eq!(
        sleep_plan(&capabilities(true, true)),
        SleepPlan::DisplaysOff
    );
    assert_eq!(sleep_plan(&capabilities(false, true)), SleepPlan::Suspend);
}

#[test]
fn hibernate_needs_a_hibernation_file() {
    assert_eq!(
        hibernate_plan(&capabilities(false, true)),
        HibernatePlan::Hibernate
    );
    assert_eq!(
        hibernate_plan(&capabilities(false, false)),
        HibernatePlan::Explain
    );
}

#[test]
fn volume_steps_follow_windows_own_volume_keys_and_neither_pass_the_ends() {
    assert_eq!(volume_up(volume(50, false)), volume(52, false));
    assert_eq!(volume_up(volume(99, true)), volume(100, true));
    assert_eq!(volume_up(volume(100, false)), volume(100, false));
    assert_eq!(volume_down(volume(50, false)), volume(48, false));
    assert_eq!(volume_down(volume(1, true)), volume(0, true));
    assert_eq!(volume_down(volume(0, false)), volume(0, false));
    assert_eq!(toggled_mute(volume(50, false)), volume(50, true));
    assert_eq!(toggled_mute(volume(50, true)), volume(50, false));
}

#[test]
fn the_microphone_toggle_mutes_all_when_any_is_on_and_explains_none() {
    assert_eq!(
        microphone_plan(&[microphone("a", false), microphone("b", true)]),
        MicrophonePlan::Mute
    );
    assert_eq!(
        microphone_plan(&[microphone("a", true), microphone("b", true)]),
        MicrophonePlan::Unmute
    );
    assert_eq!(microphone_plan(&[]), MicrophonePlan::Explain);
}

#[test]
fn the_appearance_toggle_flips_light_and_dark() {
    assert_eq!(toggled_appearance(Appearance::Light), Appearance::Dark);
    assert_eq!(toggled_appearance(Appearance::Dark), Appearance::Light);
}

#[test]
fn hdr_turns_on_when_any_capable_display_is_off_and_off_when_all_are_on() {
    assert_eq!(hdr_plan(&[display("a", false)]), TogglePlan::On);
    assert_eq!(
        hdr_plan(&[display("a", true), display("b", false)]),
        TogglePlan::On
    );
    assert_eq!(hdr_plan(&[display("a", true)]), TogglePlan::Off);
    assert_eq!(hdr_plan(&[]), TogglePlan::Explain);
}

#[test]
fn bluetooth_turns_on_when_any_radio_is_off_and_off_when_all_are_on() {
    assert_eq!(bluetooth_plan(&[radio("a", false)]), TogglePlan::On);
    assert_eq!(
        bluetooth_plan(&[radio("a", true), radio("b", false)]),
        TogglePlan::On
    );
    assert_eq!(bluetooth_plan(&[radio("a", true)]), TogglePlan::Off);
    assert_eq!(bluetooth_plan(&[]), TogglePlan::Explain);
}

#[test]
fn each_command_says_what_it_ended_in_and_the_adapter_what_it_was_asked() {
    let commands = RecordingSystemCommands::default();
    let cases: [(Command, &str, Done); 11] = [
        (Command::LockScreen, "Locking the screen", Done::Locked),
        (
            Command::LogOut,
            "Logging out",
            Done::Power(PowerRequest::LogOut, false),
        ),
        (
            Command::Restart,
            "Restarting",
            Done::Power(PowerRequest::Restart, true),
        ),
        (
            Command::ShutDown,
            "Shutting down",
            Done::Power(PowerRequest::ShutDown, true),
        ),
        (Command::Sleep, "Sleeping", Done::Suspended(false)),
        (Command::Hibernate, "Hibernating", Done::Suspended(true)),
        (
            Command::TurnOffDisplays,
            "Turning off the displays",
            Done::DisplaysOff,
        ),
        (
            Command::StartScreenSaver,
            "Starting the screen saver",
            Done::ScreenSaver,
        ),
        (
            Command::OpenRecycleBin,
            "Opening the Recycle Bin",
            Done::OpenedBin,
        ),
        (
            Command::EmptyRecycleBin,
            "Emptied the Recycle Bin",
            Done::EmptiedBin,
        ),
        (
            Command::ShowDesktop,
            "Showing the desktop",
            Done::ShowedDesktop,
        ),
    ];
    for (command, text, done) in cases {
        assert_eq!(
            run(command, &commands),
            Outcome::Done(text.into()),
            "{command:?}"
        );
        assert_eq!(commands.take(), [done], "{command:?}");
    }
}

#[test]
fn each_volume_command_answers_the_volume_it_ended_at() {
    let commands = RecordingSystemCommands::default();
    // The fake starts at volume 50, unmuted.
    assert_eq!(
        run(Command::VolumeUp, &commands),
        Outcome::Done("Volume 52%".into())
    );
    assert_eq!(commands.take(), [Done::SetVolume(volume(52, false))]);
    // Volume Down steps back down from where Volume Up left the volume.
    assert_eq!(
        run(Command::VolumeDown, &commands),
        Outcome::Done("Volume 50%".into())
    );
    assert_eq!(commands.take(), [Done::SetVolume(volume(50, false))]);
    assert_eq!(
        run(Command::SetVolume(40), &commands),
        Outcome::Done("Volume 40%".into())
    );
    assert_eq!(commands.take(), [Done::SetVolume(volume(40, false))]);
    // Toggle Mute says which it did, with the volume when it unmuted.
    assert_eq!(
        run(Command::ToggleMute, &commands),
        Outcome::Done("Muted".into())
    );
    assert_eq!(commands.take(), [Done::SetVolume(volume(40, true))]);
    assert_eq!(
        run(Command::ToggleMute, &commands),
        Outcome::Done("Unmuted, Volume 40%".into())
    );
    assert_eq!(commands.take(), [Done::SetVolume(volume(40, false))]);
}

#[test]
fn a_level_not_from_0_to_100_changes_nothing_and_is_explained() {
    let commands = RecordingSystemCommands::default();
    assert_eq!(
        run(Command::SetVolume(101), &commands),
        Outcome::Explained(SET_VOLUME_RANGE.into())
    );
    assert!(commands.take().is_empty(), "nothing was asked");
}

#[test]
fn the_microphone_toggle_mutes_or_unmutes_every_microphone_and_says_which() {
    let commands = RecordingSystemCommands::default();
    commands.set_microphones(vec![microphone("a", false), microphone("b", true)]);
    // One microphone unmuted: every microphone is muted.
    assert_eq!(
        run(Command::ToggleMicrophoneMute, &commands),
        Outcome::Done("Microphones muted".into())
    );
    assert_eq!(
        commands.take(),
        [
            Done::SetMicrophoneMute("a".into(), true),
            Done::SetMicrophoneMute("b".into(), true)
        ]
    );
    // None unmuted any more: every microphone is unmuted.
    assert_eq!(
        run(Command::ToggleMicrophoneMute, &commands),
        Outcome::Done("Microphones unmuted".into())
    );
    assert_eq!(
        commands.take(),
        [
            Done::SetMicrophoneMute("a".into(), false),
            Done::SetMicrophoneMute("b".into(), false)
        ]
    );
}

#[test]
fn a_microphone_that_vanishes_mid_toggle_is_skipped() {
    let commands = RecordingSystemCommands::default();
    commands.set_microphones(vec![microphone("a", false), microphone("b", false)]);
    commands.vanish("b");
    // The vanished microphone is skipped: the rest are muted.
    assert_eq!(
        run(Command::ToggleMicrophoneMute, &commands),
        Outcome::Done("Microphones muted".into())
    );
    assert_eq!(commands.take(), [Done::SetMicrophoneMute("a".into(), true)]);
    // Every microphone vanishing: nothing changed, and the answer says why.
    commands.vanish("a");
    assert_eq!(
        run(Command::ToggleMicrophoneMute, &commands),
        Outcome::Explained("The microphone is gone".into())
    );
}

#[test]
fn no_microphone_at_all_is_explained() {
    let commands = RecordingSystemCommands::default();
    commands.set_microphones(vec![]);
    assert_eq!(
        run(Command::ToggleMicrophoneMute, &commands),
        Outcome::Explained(NO_MICROPHONE.into())
    );
    assert!(commands.take().is_empty(), "nothing was asked");
}

#[test]
fn an_already_empty_bin_is_a_success_nothing_is_asked_for() {
    let commands = RecordingSystemCommands::default();
    commands.set_bin(RecycleBin { items: 0, size: 0 });
    assert_eq!(
        run(Command::EmptyRecycleBin, &commands),
        Outcome::Done("The Recycle Bin is already empty".into())
    );
    assert!(commands.take().is_empty(), "nothing was asked");
}

#[test]
fn the_appearance_toggle_says_which_mode_it_ended_in() {
    let commands = RecordingSystemCommands::default();
    // The fake starts in the light mode, so the first toggle ends in the
    // dark one, and the next one back.
    assert_eq!(
        run(Command::ToggleAppearance, &commands),
        Outcome::Done("Dark mode".into())
    );
    assert_eq!(commands.take(), [Done::SetAppearance(Appearance::Dark)]);
    assert_eq!(
        run(Command::ToggleAppearance, &commands),
        Outcome::Done("Light mode".into())
    );
    assert_eq!(commands.take(), [Done::SetAppearance(Appearance::Light)]);
}

#[test]
fn hdr_turns_every_capable_display_on_then_all_off_and_explains_none() {
    let commands = RecordingSystemCommands::default();
    // The fake starts with one capable display with HDR off, so the first
    // toggle turns it on, and the next one off.
    assert_eq!(
        run(Command::ToggleHdr, &commands),
        Outcome::Done("HDR on".into())
    );
    assert_eq!(commands.take(), [Done::SetHdr("display".into(), true)]);
    assert_eq!(
        run(Command::ToggleHdr, &commands),
        Outcome::Done("HDR off".into())
    );
    assert_eq!(commands.take(), [Done::SetHdr("display".into(), false)]);
    // No capable display at all: nothing happens and the answer says so.
    commands.set_hdr_displays(vec![]);
    assert_eq!(
        run(Command::ToggleHdr, &commands),
        Outcome::Explained(NO_HDR.into())
    );
    assert!(commands.take().is_empty(), "nothing was asked");
}

#[test]
fn the_hidden_files_toggle_says_which_it_chose() {
    let commands = RecordingSystemCommands::default();
    // The fake starts with hidden files hidden, so the first toggle shows
    // them, and the next one hides them again.
    assert_eq!(
        run(Command::ToggleHiddenFiles, &commands),
        Outcome::Done("Hidden files shown".into())
    );
    assert_eq!(commands.take(), [Done::SetHiddenFiles(true)]);
    assert_eq!(
        run(Command::ToggleHiddenFiles, &commands),
        Outcome::Done("Hidden files hidden".into())
    );
    assert_eq!(commands.take(), [Done::SetHiddenFiles(false)]);
}

#[test]
fn ejecting_reports_the_drives_that_were_ejected_and_the_ones_that_refused() {
    let commands = RecordingSystemCommands::default();
    // The fake starts with one removable drive, which is ejected.
    assert_eq!(
        run(Command::EjectRemovableDrives, &commands),
        Outcome::Done("Ejected E:".into())
    );
    assert_eq!(commands.take(), [Done::Ejected("E:".into())]);
    // One of two refuses: the other still goes, and the report names the
    // refusal and why.
    commands.set_drives(vec![drive("E:"), drive("F:")]);
    commands.busy("F:");
    assert_eq!(
        run(Command::EjectRemovableDrives, &commands),
        Outcome::Done("Ejected E:; F: was not ejected: The drive is in use".into())
    );
    assert_eq!(commands.take(), [Done::Ejected("E:".into())]);
    // Every drive refusing: nothing was ejected, and the answer says why.
    commands.set_drives(vec![drive("E:")]);
    commands.busy("E:");
    assert_eq!(
        run(Command::EjectRemovableDrives, &commands),
        Outcome::Explained("E: was not ejected: The drive is in use".into())
    );
    assert!(commands.take().is_empty(), "nothing was asked");
    // No removable drive at all: nothing happens and the answer says so.
    commands.set_drives(vec![]);
    assert_eq!(
        run(Command::EjectRemovableDrives, &commands),
        Outcome::Explained(NO_REMOVABLE_DRIVE.into())
    );
    assert!(commands.take().is_empty(), "nothing was asked");
}

#[test]
fn bluetooth_turns_the_radios_off_then_on_and_explains_none() {
    let commands = RecordingSystemCommands::default();
    // The fake starts with one radio on, so the first toggle turns it
    // off, and the next one on.
    assert_eq!(
        run(Command::ToggleBluetooth, &commands),
        Outcome::Done("Bluetooth off".into())
    );
    assert_eq!(commands.take(), [Done::SetBluetooth("radio".into(), false)]);
    assert_eq!(
        run(Command::ToggleBluetooth, &commands),
        Outcome::Done("Bluetooth on".into())
    );
    assert_eq!(commands.take(), [Done::SetBluetooth("radio".into(), true)]);
    // A radio that vanishes mid-toggle is skipped: it answers why nothing
    // changed.
    commands.set_bluetooth_radios(vec![radio("a", false)]);
    commands.vanish_radio("a");
    assert_eq!(
        run(Command::ToggleBluetooth, &commands),
        Outcome::Explained("The Bluetooth radio is gone".into())
    );
    // No radio at all: nothing happens and the answer says so.
    commands.set_bluetooth_radios(vec![]);
    assert_eq!(
        run(Command::ToggleBluetooth, &commands),
        Outcome::Explained(NO_BLUETOOTH.into())
    );
    assert!(commands.take().is_empty(), "nothing was asked");
}

#[test]
fn sleep_follows_the_computers_modern_standby_signal_and_hibernate_its_file() {
    let commands = RecordingSystemCommands::default();
    commands.set_capabilities(capabilities(true, false));
    assert_eq!(
        run(Command::Sleep, &commands),
        Outcome::Done("Sleeping".into())
    );
    assert_eq!(commands.take(), [Done::DisplaysOff]);
    // Without a hibernation file, hibernate changes nothing.
    assert_eq!(
        run(Command::Hibernate, &commands),
        Outcome::Explained(system_commands::NO_HIBERNATION_FILE.into())
    );
    assert!(commands.take().is_empty(), "nothing was asked");
}

#[test]
fn a_failing_system_is_explained_and_nothing_happens() {
    let commands = RecordingSystemCommands::default();
    commands.fail("Windows did not lock the screen");
    assert_eq!(
        run(Command::LockScreen, &commands),
        Outcome::Explained("Windows did not lock the screen".into())
    );
    // Sleep and hibernate follow the capabilities' own failure, and the
    // volume and microphone commands the volume's and the list's.
    assert_eq!(
        run(Command::Sleep, &commands),
        Outcome::Explained("Windows did not lock the screen".into())
    );
    assert_eq!(
        run(Command::Hibernate, &commands),
        Outcome::Explained("Windows did not lock the screen".into())
    );
    assert_eq!(
        run(Command::VolumeUp, &commands),
        Outcome::Explained("Windows did not lock the screen".into())
    );
    assert_eq!(
        run(Command::ToggleMicrophoneMute, &commands),
        Outcome::Explained("Windows did not lock the screen".into())
    );
    assert!(commands.take().is_empty(), "nothing was asked");
}

#[test]
fn a_launcher_given_no_commands_explains_every_one() {
    for command in COMMANDS {
        assert_eq!(
            run(command, system_commands::none().as_ref()),
            Outcome::Explained(
                "Not available: this Pane reaches no session and power commands".into()
            ),
            "{command:?}"
        );
    }
}

#[cfg(not(target_os = "windows"))]
#[test]
fn other_systems_explain_that_the_commands_are_windows_only() {
    for command in COMMANDS {
        match run(command, system_commands::native().as_ref()) {
            Outcome::Explained(why) => {
                assert!(why.starts_with("Not available on "), "{why}");
                assert!(why.contains("Windows-only for now"), "{why}");
            }
            other => panic!("expected explained, got {other:?}"),
        }
    }
}

/// The Windows adapter's one reversible read: what the computer can do.
/// Nothing here locks, ends a session, sleeps or touches the displays.
#[cfg(target_os = "windows")]
#[test]
fn windows_says_what_this_computer_can_do() {
    assert!(
        system_commands::native().capabilities().is_ok(),
        "GetPwrCapabilities answers"
    );
}

/// The Windows adapter's reversible audio reads: the volume of the
/// default output device and the microphones there are. Nothing here
/// changes the volume or mutes anything; a machine with no audio device
/// is answered, never an error thrown.
#[cfg(target_os = "windows")]
#[test]
fn windows_says_the_volume_and_the_microphones() {
    if std::env::var("PANE_TEST_REAL_INPUT").as_deref() != Ok("1") {
        eprintln!("skipped: set PANE_TEST_REAL_INPUT=1 to let it read the audio devices");
        return;
    }
    let native = system_commands::native();
    match native.volume() {
        Ok(volume) => assert!(volume.level <= 100, "{:?}", volume),
        Err(why) => assert!(!why.is_empty()),
    }
    assert!(native.microphones().is_ok(), "EnumAudioEndpoints answers");
}

/// The Windows adapter's one reversible read of the Recycle Bin: what it
/// holds. Nothing here opens or empties it.
#[cfg(target_os = "windows")]
#[test]
fn windows_says_what_the_recycle_bin_holds() {
    if std::env::var("PANE_TEST_REAL_INPUT").as_deref() != Ok("1") {
        eprintln!("skipped: set PANE_TEST_REAL_INPUT=1 to let it read the Recycle Bin");
        return;
    }
    let native = system_commands::native();
    match native.recycle_bin() {
        Ok(bin) => {
            assert!(bin.items < 1_000_000, "{:?}", bin);
            assert!(bin.size < 10_000_000_000_000, "{:?}", bin);
        }
        Err(why) => assert!(!why.is_empty()),
    }
}

/// One language's system commands sample package.
struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its command's title in root search.
    title: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-system-commands",
    title: "System commands sample",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-system-commands-js",
    title: "JavaScript system commands sample",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-system-commands-ts",
    title: "TypeScript system commands sample",
};

/// Copies the assembled package `name` under `target/guests/packages` to
/// `folder`.
fn copy(name: &str, folder: &Path) -> PathBuf {
    let assembled = guests().join("packages").join(name);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
    }
    folder.to_path_buf()
}

/// One test's Pane: its folders, the fake of the system its commands
/// reach, and the launcher.
struct Pane {
    _sources: TempDir,
    _data: TempDir,
    commands: Arc<RecordingSystemCommands>,
    launcher: Launcher,
}

impl Pane {
    fn with(fixture: &Fixture) -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let commands = Arc::new(RecordingSystemCommands::default());
        let launcher =
            Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
                .with_system_commands(commands.clone());
        let folder = copy(fixture.package, &sources.path().join(fixture.package));
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        Pane {
            _sources: sources,
            _data: data,
            commands,
            launcher,
        }
    }

    /// Root search, with `query` typed.
    fn search(&self, query: &str) {
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
        block_on(self.launcher.set_query(query));
    }

    /// Opens the sample's command from root search.
    fn open(&self, fixture: &Fixture) {
        self.search(fixture.title);
        select_title(&self.launcher, fixture.title);
        block_on(self.launcher.activate_selected());
        assert_eq!(self.launcher.view().screen, Screen::Command);
    }

    /// Runs the item titled `item` of the open command and says what it
    /// showed: its toast, or the status line.
    fn run(&self, item: &str) -> Status {
        select_title(&self.launcher, item);
        block_on(self.launcher.activate_selected());
        shown(&self.launcher)
    }
}

fn each_command_calls_the_capability_and_answers_its_state(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    let cases: [(&str, Status, Vec<Done>); 11] = [
        (
            "Lock Screen",
            Status::Result("Lock Screen: Locking the screen".into()),
            vec![Done::Locked],
        ),
        (
            "Log Out",
            Status::Result("Log Out: Logging out".into()),
            vec![Done::Power(PowerRequest::LogOut, false)],
        ),
        (
            "Restart",
            Status::Result("Restart: Restarting".into()),
            vec![Done::Power(PowerRequest::Restart, true)],
        ),
        (
            "Shut Down",
            Status::Result("Shut Down: Shutting down".into()),
            vec![Done::Power(PowerRequest::ShutDown, true)],
        ),
        (
            "Sleep",
            Status::Result("Sleep: Sleeping".into()),
            vec![Done::Suspended(false)],
        ),
        (
            "Hibernate",
            Status::Result("Hibernate: Hibernating".into()),
            vec![Done::Suspended(true)],
        ),
        (
            "Turn Off Displays",
            Status::Result("Turn Off Displays: Turning off the displays".into()),
            vec![Done::DisplaysOff],
        ),
        (
            "Start Screen Saver",
            Status::Result("Start Screen Saver: Starting the screen saver".into()),
            vec![Done::ScreenSaver],
        ),
        (
            "Open Recycle Bin",
            Status::Result("Open Recycle Bin: Opening the Recycle Bin".into()),
            vec![Done::OpenedBin],
        ),
        (
            "Empty Recycle Bin",
            Status::Result("Empty Recycle Bin: Emptied the Recycle Bin".into()),
            vec![Done::EmptiedBin],
        ),
        (
            "Show Desktop",
            Status::Result("Show Desktop: Showing the desktop".into()),
            vec![Done::ShowedDesktop],
        ),
    ];
    for (item, expected, done) in cases {
        assert_eq!(pane.run(item), expected, "{item}: {}", fixture.title);
        assert_eq!(pane.commands.take(), done, "{item}: {}", fixture.title);
    }
}

fn each_volume_item_answers_the_volume_it_ended_at(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    // The fake starts at volume 50, unmuted, which each case resets to.
    let cases: [(&str, Status, Done); 4] = [
        (
            "Volume Up",
            Status::Result("Volume Up: Volume 52%".into()),
            Done::SetVolume(volume(52, false)),
        ),
        (
            "Volume Down",
            Status::Result("Volume Down: Volume 48%".into()),
            Done::SetVolume(volume(48, false)),
        ),
        (
            "Toggle Mute",
            Status::Result("Toggle Mute: Muted".into()),
            Done::SetVolume(volume(50, true)),
        ),
        (
            "Set Volume",
            Status::Result("Set Volume: Volume 40%".into()),
            Done::SetVolume(volume(40, false)),
        ),
    ];
    for (item, expected, done) in cases {
        pane.commands.set_volume_state(volume(50, false));
        assert_eq!(pane.run(item), expected, "{item}: {}", fixture.title);
        assert_eq!(pane.commands.take(), [done], "{item}: {}", fixture.title);
    }
    // Muting once, the next unmute says the volume it ended at.
    assert_eq!(
        pane.run("Toggle Mute"),
        Status::Result("Toggle Mute: Muted".into()),
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.run("Toggle Mute"),
        Status::Result("Toggle Mute: Unmuted, Volume 40%".into()),
        "{}",
        fixture.title
    );
}

fn the_microphone_toggle_mutes_all_unmutes_all_and_explains_none(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    // Two microphones, one muted: every microphone is muted.
    pane.commands
        .set_microphones(vec![microphone("a", false), microphone("b", true)]);
    assert_eq!(
        pane.run("Toggle Microphone Mute"),
        Status::Result("Toggle Microphone Mute: Microphones muted".into()),
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.commands.take(),
        [
            Done::SetMicrophoneMute("a".into(), true),
            Done::SetMicrophoneMute("b".into(), true)
        ],
        "{}",
        fixture.title
    );
    // None unmuted any more: every microphone is unmuted.
    assert_eq!(
        pane.run("Toggle Microphone Mute"),
        Status::Result("Toggle Microphone Mute: Microphones unmuted".into()),
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.commands.take(),
        [
            Done::SetMicrophoneMute("a".into(), false),
            Done::SetMicrophoneMute("b".into(), false)
        ],
        "{}",
        fixture.title
    );
    // No microphone at all: nothing happens and the answer says so.
    pane.commands.set_microphones(vec![]);
    assert_eq!(
        pane.run("Toggle Microphone Mute"),
        Status::Error(format!("Toggle Microphone Mute: {}", NO_MICROPHONE)),
        "{}",
        fixture.title
    );
    assert!(pane.commands.take().is_empty(), "nothing was asked");
}

fn sleep_follows_the_computers_modern_standby_signal(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.commands.set_capabilities(capabilities(true, true));
    pane.open(fixture);
    assert_eq!(
        pane.run("Sleep"),
        Status::Result("Sleep: Sleeping".into()),
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.commands.take(),
        [Done::DisplaysOff],
        "{}",
        fixture.title
    );
}

fn hibernate_without_a_hibernation_file_is_explained(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.commands.set_capabilities(capabilities(false, false));
    pane.open(fixture);
    assert_eq!(
        pane.run("Hibernate"),
        Status::Error(format!(
            "Hibernate: {}",
            system_commands::NO_HIBERNATION_FILE
        )),
        "{}",
        fixture.title
    );
    assert!(pane.commands.take().is_empty(), "nothing was asked");
}

fn an_already_empty_bin_is_a_success(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.commands.set_bin(RecycleBin { items: 0, size: 0 });
    pane.open(fixture);
    assert_eq!(
        pane.run("Empty Recycle Bin"),
        Status::Result("Empty Recycle Bin: The Recycle Bin is already empty".into()),
        "{}",
        fixture.title
    );
    assert!(pane.commands.take().is_empty(), "nothing was asked");
}

fn the_appearance_toggle_says_which_mode_it_chose(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    // The fake starts in the light mode, so the toggle ends in the dark
    // one.
    assert_eq!(
        pane.run("Toggle System Appearance"),
        Status::Result("Toggle System Appearance: Dark mode".into()),
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.commands.take(),
        [Done::SetAppearance(Appearance::Dark)],
        "{}",
        fixture.title
    );
}

fn hdr_toggles_on_when_any_capable_display_is_off_and_explains_none(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    // The fake starts with one capable display with HDR off, so the
    // toggle turns it on.
    assert_eq!(
        pane.run("Toggle HDR"),
        Status::Result("Toggle HDR: HDR on".into()),
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.commands.take(),
        [Done::SetHdr("display".into(), true)],
        "{}",
        fixture.title
    );
    // No capable display at all: nothing happens and the answer says so.
    pane.commands.set_hdr_displays(vec![]);
    assert_eq!(
        pane.run("Toggle HDR"),
        Status::Error(format!("Toggle HDR: {}", NO_HDR)),
        "{}",
        fixture.title
    );
    assert!(pane.commands.take().is_empty(), "nothing was asked");
}

fn the_desktop_and_hidden_files_toggles_say_what_they_ended_in(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    assert_eq!(
        pane.run("Show Desktop"),
        Status::Result("Show Desktop: Showing the desktop".into()),
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.commands.take(),
        [Done::ShowedDesktop],
        "{}",
        fixture.title
    );
    // The fake starts with hidden files hidden, so the toggle shows them.
    assert_eq!(
        pane.run("Toggle Hidden Files"),
        Status::Result("Toggle Hidden Files: Hidden files shown".into()),
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.commands.take(),
        [Done::SetHiddenFiles(true)],
        "{}",
        fixture.title
    );
}

fn ejecting_reports_each_drive_and_its_failures(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    // The fake starts with one removable drive, which is ejected.
    assert_eq!(
        pane.run("Eject Removable Drives"),
        Status::Result("Eject Removable Drives: Ejected E:".into()),
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.commands.take(),
        [Done::Ejected("E:".into())],
        "{}",
        fixture.title
    );
    // One of two refuses: the other still goes, and the report names the
    // refusal and why.
    pane.commands.set_drives(vec![drive("E:"), drive("F:")]);
    pane.commands.busy("F:");
    assert_eq!(
        pane.run("Eject Removable Drives"),
        Status::Result(
            "Eject Removable Drives: Ejected E:; F: was not ejected: The drive is in use".into()
        ),
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.commands.take(),
        [Done::Ejected("E:".into())],
        "{}",
        fixture.title
    );
    // No removable drive at all: nothing happens and the answer says so.
    pane.commands.set_drives(vec![]);
    assert_eq!(
        pane.run("Eject Removable Drives"),
        Status::Error(format!("Eject Removable Drives: {}", NO_REMOVABLE_DRIVE)),
        "{}",
        fixture.title
    );
    assert!(pane.commands.take().is_empty(), "nothing was asked");
}

fn bluetooth_toggles_the_radios_and_explains_none(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    // The fake starts with one radio on, so the toggle turns it off.
    assert_eq!(
        pane.run("Toggle Bluetooth"),
        Status::Result("Toggle Bluetooth: Bluetooth off".into()),
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.commands.take(),
        [Done::SetBluetooth("radio".into(), false)],
        "{}",
        fixture.title
    );
    // No radio at all: nothing happens and the answer says so.
    pane.commands.set_bluetooth_radios(vec![]);
    assert_eq!(
        pane.run("Toggle Bluetooth"),
        Status::Error(format!("Toggle Bluetooth: {}", NO_BLUETOOTH)),
        "{}",
        fixture.title
    );
    assert!(pane.commands.take().is_empty(), "nothing was asked");
}

fn a_failing_system_answers_why_nothing_changed(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.commands.fail("Windows did not lock the screen");
    pane.open(fixture);
    assert_eq!(
        pane.run("Lock Screen"),
        Status::Error("Lock Screen: Windows did not lock the screen".into()),
        "{}",
        fixture.title
    );
    assert!(pane.commands.take().is_empty(), "nothing was asked");
}

macro_rules! contract {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(#[test] fn $check() { super::$check(&super::RUST) })*
        }
        mod javascript {
            $(#[test] fn $check() { super::$check(&super::JAVASCRIPT) })*
        }
        mod typescript {
            $(#[test] fn $check() { super::$check(&super::TYPESCRIPT) })*
        }
    };
}

contract!(
    each_command_calls_the_capability_and_answers_its_state,
    each_volume_item_answers_the_volume_it_ended_at,
    the_microphone_toggle_mutes_all_unmutes_all_and_explains_none,
    sleep_follows_the_computers_modern_standby_signal,
    hibernate_without_a_hibernation_file_is_explained,
    an_already_empty_bin_is_a_success,
    the_appearance_toggle_says_which_mode_it_chose,
    hdr_toggles_on_when_any_capable_display_is_off_and_explains_none,
    the_desktop_and_hidden_files_toggles_say_what_they_ended_in,
    ejecting_reports_each_drive_and_its_failures,
    bluetooth_toggles_the_radios_and_explains_none,
    a_failing_system_answers_why_nothing_changed,
);

/// The System Commands default extension, acquired as Windows does: at
/// first setup, from the repository its pin names — the sample's package
/// in a repository of this test's own, served on 127.0.0.1, a stand-in
/// for the default's own repository, which lives outside this one
/// (#301). The sample calls each host function as it is: no
/// confirmation, no HUD — the composition the real extension adds is the
/// extension's own, its repository's to test, as the calculator's
/// arithmetic was (#285) — so these tests carry the default-extension
/// mechanics and which host function ran per item. The Windows defaults
/// are Windows-only, so these tests run on Windows only.
#[cfg(target_os = "windows")]
mod extension {
    use std::sync::Arc;

    use futures::executor::block_on;
    use pane_core::system_commands::PowerRequest;
    use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};

    use super::defaults;
    use super::feedback::shown;
    use super::recording::{Done, RecordingSystemCommands};
    use super::repo_server;
    use super::rows::{manage, select_title, titles};
    use super::{Appearance, volume};

    /// One test's Pane: its data folder, the served repository holding
    /// the sample's package as the System Commands default's own (a
    /// stand-in for the one a release pins it to, which lives outside
    /// this one), the fake of the system its command reaches, and the
    /// launcher.
    struct Pane {
        _data: tempfile::TempDir,
        _repos: tempfile::TempDir,
        _server: repo_server::Server,
        commands: Arc<RecordingSystemCommands>,
        launcher: Launcher,
        identity: PackageIdentity,
    }

    impl Pane {
        fn new() -> Pane {
            let data = tempfile::tempdir().unwrap();
            let server = repo_server::Server::start();
            let repos = tempfile::tempdir().unwrap();
            // The sample's package, served as the default's own
            // repository's release, tagged as its manifest's version: a
            // stand-in for the one a release pins the default to.
            let pin = defaults::from_sample(
                &server,
                repos.path(),
                "system-commands",
                "System Commands",
                "sample-system-commands",
            );
            let commands = Arc::new(RecordingSystemCommands::default());
            let launcher =
                Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
                    .with_defaults(vec![pin])
                    .with_system_commands(commands.clone());
            block_on(launcher.acquire_defaults());
            assert!(
                matches!(launcher.view().status, Status::Result(_)),
                "{:?}",
                launcher.view().status
            );
            let identity = PackageIdentity::default_extension("system-commands");
            Pane {
                _data: data,
                _repos: repos,
                _server: server,
                commands,
                launcher,
                identity,
            }
        }

        /// Root search, with `query` typed.
        fn search(&self, query: &str) {
            while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
                self.launcher.back();
            }
            block_on(self.launcher.set_query(query));
        }

        /// Opens the sample's command from root search: one item per
        /// system command, titled as the real commands are.
        fn open(&self) {
            self.search("System commands sample");
            select_title(&self.launcher, "System commands sample");
            block_on(self.launcher.activate_selected());
            assert_eq!(self.launcher.view().screen, Screen::Command);
        }

        /// Runs the item titled `item` of the open command and says what
        /// it showed: its toast, or the status line.
        fn run(&self, item: &str) -> Status {
            select_title(&self.launcher, item);
            block_on(self.launcher.activate_selected());
            shown(&self.launcher)
        }
    }

    #[test]
    fn the_extension_is_acquired_enabled_and_disableable_on_its_own() {
        let pane = Pane::new();

        // Acquired with the default extension's identity, enabled, its
        // command a root result the user can give an alias or a hotkey.
        let packages = pane.launcher.packages();
        let package = packages
            .iter()
            .find(|package| package.identity == pane.identity)
            .expect("System Commands is installed");
        assert!(package.enabled, "enabled by default");
        pane.search("System commands sample");
        assert!(
            titles(&pane.launcher).contains(&"System commands sample".to_owned()),
            "the command is no root result: {:?}",
            titles(&pane.launcher)
        );

        // Its page in Settings lists its command, and the extension's
        // switch is its own.
        manage(&pane.launcher);
        assert!(
            titles(&pane.launcher).contains(&"Clear cache of System commands sample".to_owned())
        );
        assert!(titles(&pane.launcher).contains(&"Uninstall System commands sample".to_owned()));
        assert!(titles(&pane.launcher).contains(&"Hotkey for System commands sample".to_owned()));
        let subtitle = pane
            .launcher
            .view()
            .rows
            .first()
            .and_then(|row| row.subtitle.clone())
            .unwrap_or_default();
        assert!(subtitle.starts_with("Enabled"), "{subtitle}");

        // Disabled, its command leaves root search; enabled again, it
        // returns.
        block_on(pane.launcher.set_enabled(&pane.identity, false));
        pane.search("System commands sample");
        assert!(!titles(&pane.launcher).contains(&"System commands sample".to_owned()));
        block_on(pane.launcher.set_enabled(&pane.identity, true));
        pane.search("System commands sample");
        assert!(titles(&pane.launcher).contains(&"System commands sample".to_owned()));
    }

    #[test]
    fn each_item_calls_the_host_function_it_names_and_says_what_it_answered() {
        let pane = Pane::new();
        pane.open();
        // The fake starts as a computer that suspends with a hibernation
        // file, the volume at 50 and unmuted, one unmuted microphone, a
        // Recycle Bin holding three items, the light mode, one capable
        // display with HDR off, hidden files hidden, one removable drive
        // and one radio on — which each volume case resets to, as the
        // runs before it moved the volume.
        let cases: [(&str, Status, Done); 21] = [
            (
                "Lock Screen",
                Status::Result("Lock Screen: Locking the screen".into()),
                Done::Locked,
            ),
            (
                "Log Out",
                Status::Result("Log Out: Logging out".into()),
                Done::Power(PowerRequest::LogOut, false),
            ),
            (
                "Restart",
                Status::Result("Restart: Restarting".into()),
                Done::Power(PowerRequest::Restart, true),
            ),
            (
                "Shut Down",
                Status::Result("Shut Down: Shutting down".into()),
                Done::Power(PowerRequest::ShutDown, true),
            ),
            (
                "Sleep",
                Status::Result("Sleep: Sleeping".into()),
                Done::Suspended(false),
            ),
            (
                "Hibernate",
                Status::Result("Hibernate: Hibernating".into()),
                Done::Suspended(true),
            ),
            (
                "Turn Off Displays",
                Status::Result("Turn Off Displays: Turning off the displays".into()),
                Done::DisplaysOff,
            ),
            (
                "Start Screen Saver",
                Status::Result("Start Screen Saver: Starting the screen saver".into()),
                Done::ScreenSaver,
            ),
            (
                "Volume Up",
                Status::Result("Volume Up: Volume 52%".into()),
                Done::SetVolume(volume(52, false)),
            ),
            (
                "Volume Down",
                Status::Result("Volume Down: Volume 48%".into()),
                Done::SetVolume(volume(48, false)),
            ),
            (
                "Toggle Mute",
                Status::Result("Toggle Mute: Muted".into()),
                Done::SetVolume(volume(50, true)),
            ),
            (
                "Set Volume",
                Status::Result("Set Volume: Volume 40%".into()),
                Done::SetVolume(volume(40, false)),
            ),
            (
                "Toggle Microphone Mute",
                Status::Result("Toggle Microphone Mute: Microphones muted".into()),
                Done::SetMicrophoneMute("microphone".into(), true),
            ),
            (
                "Open Recycle Bin",
                Status::Result("Open Recycle Bin: Opening the Recycle Bin".into()),
                Done::OpenedBin,
            ),
            (
                "Empty Recycle Bin",
                Status::Result("Empty Recycle Bin: Emptied the Recycle Bin".into()),
                Done::EmptiedBin,
            ),
            (
                "Toggle System Appearance",
                Status::Result("Toggle System Appearance: Dark mode".into()),
                Done::SetAppearance(Appearance::Dark),
            ),
            (
                "Toggle HDR",
                Status::Result("Toggle HDR: HDR on".into()),
                Done::SetHdr("display".into(), true),
            ),
            (
                "Show Desktop",
                Status::Result("Show Desktop: Showing the desktop".into()),
                Done::ShowedDesktop,
            ),
            (
                "Toggle Hidden Files",
                Status::Result("Toggle Hidden Files: Hidden files shown".into()),
                Done::SetHiddenFiles(true),
            ),
            (
                "Eject Removable Drives",
                Status::Result("Eject Removable Drives: Ejected E:".into()),
                Done::Ejected("E:".into()),
            ),
            (
                "Toggle Bluetooth",
                Status::Result("Toggle Bluetooth: Bluetooth off".into()),
                Done::SetBluetooth("radio".into(), false),
            ),
        ];
        for (item, expected, done) in cases {
            pane.commands.set_volume_state(volume(50, false));
            assert_eq!(pane.run(item), expected, "{item}");
            assert_eq!(pane.commands.take(), [done], "{item}");
        }
    }
}
