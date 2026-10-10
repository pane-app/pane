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
//! real System Commands default extension — the package `cargo xtask
//! guests` assembles in `target/guests/packages/system-commands`, acquired
//! as a Windows default extension from an artifact source on 127.0.0.1
//! (`support/artifacts.rs`) — answers with a HUD of the state it ended
//! in, confirms the destructive ones first (with "Don't ask again"
//! remembered), takes Set Volume's level through its argument form, runs
//! from a hotkey without showing the window, and is acquired enabled and
//! disableable on its own. The extension's package declares `windows`
//! alone, so those tests run on Windows only; on other systems acquiring
//! it is refused with the platform's explanation.

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

/// The real System Commands default extension, acquired as Windows does:
/// from an artifact source on 127.0.0.1, with the default extension's
/// identity. The package declares `windows` alone, so these tests run on
/// Windows only.
#[cfg(target_os = "windows")]
mod extension {
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use futures::executor::block_on;
    use pane_core::feedback::WindowRequest;
    use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
    use pane_core::system_commands::{
        NO_BLUETOOTH, NO_HDR, NO_HIBERNATION_FILE, NO_MICROPHONE, NO_REMOVABLE_DRIVE, PowerRequest,
        RecycleBin, SET_VOLUME_RANGE,
    };
    use pane_core::{
        ConfirmAnswer, Confirmation, Hud, Launcher, PackageIdentity, Runtime, Screen, Status,
        ToastStyle,
    };

    use super::defaults;
    use super::feedback::RecordingWindow;
    use super::recording::{Done, RecordingSystemCommands};
    use super::repo_server;
    use super::rows::{manage, select_title, titles};
    use super::{Appearance, capabilities, guests, microphone, volume};

    /// How long a launch or a guest call may take: compiling the guest
    /// once is included; a slow, busy machine is not.
    const PROMPTLY: Duration = Duration::from_secs(60);

    /// A system whose global hotkeys always register.
    #[derive(Default)]
    struct FakeHotkeys {
        registered: std::sync::Mutex<Vec<Shortcut>>,
    }

    impl Hotkeys for FakeHotkeys {
        fn unavailable(&self) -> Option<String> {
            None
        }

        fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
            self.registered.lock().unwrap().push(shortcut.clone());
            Ok(())
        }

        fn unregister(&self, shortcut: &Shortcut) {
            self.registered
                .lock()
                .unwrap()
                .retain(|kept| kept != shortcut);
        }
    }

    /// One test's Pane: its data folder, the served repository holding
    /// the default extension's package as a release revision (a
    /// stand-in for the one a release pins it to), the fake of the
    /// system its commands reach, and the launcher.
    struct Pane {
        _data: tempfile::TempDir,
        _repos: tempfile::TempDir,
        _server: repo_server::Server,
        commands: Arc<RecordingSystemCommands>,
        launcher: Launcher,
        window: Arc<RecordingWindow>,
        identity: PackageIdentity,
    }

    impl Pane {
        fn new() -> Pane {
            let data = tempfile::tempdir().unwrap();
            let server = repo_server::Server::start();
            let repos = tempfile::tempdir().unwrap();
            let pin = super::defaults::from_sample(
                &server,
                repos.path(),
                "system-commands",
                "System Commands",
                "system-commands",
            );
            let commands = Arc::new(RecordingSystemCommands::default());
            let launcher =
                Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
                    .with_defaults(vec![pin])
                    .with_system_commands(commands.clone())
                    .with_hotkeys(Arc::new(FakeHotkeys::default()));
            let window = RecordingWindow::attach(&launcher);
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
                window,
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

        /// The HUDs the window was asked to show so far, forgotten once
        /// read (the hides that closed the launcher for them are not).
        fn huds(&self) -> Vec<Hud> {
            self.window
                .take()
                .into_iter()
                .filter_map(|request| match request {
                    WindowRequest::Hud(hud) => Some(hud),
                    WindowRequest::Hide | WindowRequest::Confirmation => None,
                })
                .collect()
        }

        /// Types `title` in root search and runs the row titled `title`;
        /// what the window was asked to show.
        fn run(&self, title: &str) -> Vec<Hud> {
            self.search(title);
            select_title(&self.launcher, title);
            block_on(self.launcher.activate_selected());
            assert!(
                matches!(self.launcher.view().screen, Screen::Root { .. }),
                "{} opened no screen",
                title
            );
            self.huds()
        }

        /// Types `title` in root search, opens the command titled `title`'s
        /// argument form and submits it with `values`; what the window was
        /// asked to show.
        fn submit(&self, title: &str, values: &[(&str, &str)]) -> Vec<Hud> {
            self.search(title);
            select_title(&self.launcher, title);
            block_on(self.launcher.activate_selected());
            for (field, value) in values {
                self.launcher.set_field_value(field, value);
            }
            block_on(self.launcher.submit_form());
            self.huds()
        }

        /// Starts the command titled `title` from root search, on a thread
        /// of its own: the destructive ones wait on the user.
        fn start(&self, title: &str) -> Running {
            self.search(title);
            select_title(&self.launcher, title);
            let running = self.launcher.activate_selected();
            Running {
                thread: thread::spawn(move || {
                    block_on(running);
                }),
            }
        }

        /// The id of the command `command` of the extension.
        fn id(&self, command: &str) -> String {
            format!("{}#{command}", self.identity.key())
        }
    }

    /// A call that may wait on a confirmation, running on a thread of its
    /// own.
    struct Running {
        thread: thread::JoinHandle<()>,
    }

    impl Running {
        /// Waits for the call to end.
        fn ended(self) {
            let started = Instant::now();
            while !self.thread.is_finished() {
                assert!(started.elapsed() < PROMPTLY, "the call did not end");
                thread::sleep(std::time::Duration::from_millis(5));
            }
            self.thread.join().unwrap();
        }
    }

    /// The confirmation the launcher shows, once the command asked for it.
    fn asked(launcher: &Launcher) -> Confirmation {
        let started = Instant::now();
        loop {
            if let Some(confirmation) = launcher.confirmation() {
                return confirmation;
            }
            assert!(
                started.elapsed() < PROMPTLY,
                "no confirmation was asked: {:?} (toast {:?})",
                launcher.view(),
                launcher.toast()
            );
            thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// The HUD titled `title` in `style`.
    fn hud(title: &str, style: ToastStyle) -> Hud {
        Hud::new(style, title)
    }

    /// The command titled `title`'s manifest id, as the confirmation's
    /// remembered key names it.
    fn command_id(title: &str) -> &'static str {
        match title {
            "Log Out" => "log-out",
            "Restart" => "restart",
            "Empty Recycle Bin" => "empty-recycle-bin",
            _ => "shut-down",
        }
    }

    #[test]
    fn the_extension_is_acquired_enabled_and_disableable_on_its_own() {
        let pane = Pane::new();

        // Acquired with the default extension's identity, enabled, its
        // twenty-one commands root results the user can give an alias or
        // a hotkey.
        let packages = pane.launcher.packages();
        let package = packages
            .iter()
            .find(|package| package.identity == pane.identity)
            .expect("System Commands is installed");
        assert!(package.enabled, "enabled by default");
        for title in [
            "Lock Screen",
            "Log Out",
            "Restart",
            "Shut Down",
            "Sleep",
            "Hibernate",
            "Turn Off Displays",
            "Start Screen Saver",
            "Volume Up",
            "Volume Down",
            "Toggle Mute",
            "Set Volume",
            "Toggle Microphone Mute",
            "Open Recycle Bin",
            "Empty Recycle Bin",
            "Toggle System Appearance",
            "Toggle HDR",
            "Show Desktop",
            "Toggle Hidden Files",
            "Eject Removable Drives",
            "Toggle Bluetooth",
        ] {
            pane.search(title);
            assert!(
                titles(&pane.launcher).contains(&title.to_owned()),
                "{title} is no root result: {:?}",
                titles(&pane.launcher)
            );
        }

        // Its page in Settings lists its commands, and the extension's
        // switch is its own.
        manage(&pane.launcher);
        assert!(titles(&pane.launcher).contains(&"Clear cache of System Commands".to_owned()));
        assert!(titles(&pane.launcher).contains(&"Uninstall System Commands".to_owned()));
        for title in [
            "Hotkey for Lock Screen",
            "Hotkey for Sleep",
            "Hotkey for Set Volume",
        ] {
            assert!(
                titles(&pane.launcher).contains(&title.to_owned()),
                "{title} is not on the page: {:?}",
                titles(&pane.launcher)
            );
        }
        let subtitle = pane
            .launcher
            .view()
            .rows
            .first()
            .and_then(|row| row.subtitle.clone())
            .unwrap_or_default();
        assert!(subtitle.starts_with("Enabled"), "{subtitle}");

        // Disabled, its commands leave root search; enabled again, they
        // return.
        block_on(pane.launcher.set_enabled(&pane.identity, false));
        pane.search("Lock Screen");
        assert!(!titles(&pane.launcher).contains(&"Lock Screen".to_owned()));
        block_on(pane.launcher.set_enabled(&pane.identity, true));
        pane.search("Lock Screen");
        assert!(titles(&pane.launcher).contains(&"Lock Screen".to_owned()));
    }

    #[test]
    fn each_command_answers_the_hud_of_the_state_it_ended_in() {
        let pane = Pane::new();
        // The fake starts with the Recycle Bin holding three items, so
        // Empty Recycle Bin would empty it — its confirmation, and the
        // answer it gives, have their own test below.
        let cases: [(&str, &str, Done); 7] = [
            ("Lock Screen", "Locking the screen", Done::Locked),
            ("Sleep", "Sleeping", Done::Suspended(false)),
            ("Hibernate", "Hibernating", Done::Suspended(true)),
            (
                "Turn Off Displays",
                "Turning off the displays",
                Done::DisplaysOff,
            ),
            (
                "Start Screen Saver",
                "Starting the screen saver",
                Done::ScreenSaver,
            ),
            (
                "Open Recycle Bin",
                "Opening the Recycle Bin",
                Done::OpenedBin,
            ),
            ("Show Desktop", "Showing the desktop", Done::ShowedDesktop),
        ];
        for (command, text, done) in cases {
            assert_eq!(
                pane.run(command),
                [hud(text, ToastStyle::Success)],
                "{command}"
            );
            assert_eq!(pane.commands.take(), [done], "{command}");
        }
    }

    #[test]
    fn the_volume_commands_answer_the_hud_of_the_volume_they_ended_at() {
        let pane = Pane::new();
        // The fake starts at volume 50, unmuted; Volume Down steps back
        // down from where Volume Up left the volume.
        assert_eq!(
            pane.run("Volume Up"),
            [hud("Volume 52%", ToastStyle::Success)]
        );
        assert_eq!(pane.commands.take(), [Done::SetVolume(volume(52, false))]);
        assert_eq!(
            pane.run("Volume Down"),
            [hud("Volume 50%", ToastStyle::Success)]
        );
        assert_eq!(pane.commands.take(), [Done::SetVolume(volume(50, false))]);
        // Muting says which it did; unmuting says the volume it ended at.
        assert_eq!(pane.run("Toggle Mute"), [hud("Muted", ToastStyle::Success)]);
        assert_eq!(pane.commands.take(), [Done::SetVolume(volume(50, true))]);
        assert_eq!(
            pane.run("Toggle Mute"),
            [hud("Unmuted, Volume 50%", ToastStyle::Success)]
        );
        assert_eq!(pane.commands.take(), [Done::SetVolume(volume(50, false))]);
    }

    #[test]
    fn set_volume_takes_its_level_through_the_argument_form() {
        let pane = Pane::new();
        // Enter on Set Volume asks for its level first, as the command's
        // one argument, required.
        pane.search("Set Volume");
        select_title(&pane.launcher, "Set Volume");
        block_on(pane.launcher.activate_selected());
        let view = pane.launcher.view();
        assert_eq!(view.title, "Set Volume");
        let form = view.form().expect("the argument form is shown");
        assert_eq!(form.fields.len(), 1);
        assert_eq!(form.fields[0].id, "level");
        assert_eq!(form.fields[0].label, "Volume level (0 to 100)");
        assert!(form.fields[0].required);
        assert!(pane.launcher.back());

        // Anything but a number from 0 to 100 changes nothing and is
        // explained — "forty" by the command, 150 by the host, the same
        // sentence either way.
        for level in ["forty", "150"] {
            assert_eq!(
                pane.submit("Set Volume", &[("level", level)]),
                [hud(SET_VOLUME_RANGE, ToastStyle::Failure)],
                "{level}"
            );
            assert!(pane.commands.take().is_empty(), "{level} asked nothing");
        }

        // A level from 0 to 100 is the volume it ends at.
        assert_eq!(
            pane.submit("Set Volume", &[("level", "40")]),
            [hud("Volume 40%", ToastStyle::Success)]
        );
        assert_eq!(pane.commands.take(), [Done::SetVolume(volume(40, false))]);
    }

    #[test]
    fn the_microphone_toggle_mutes_all_unmutes_all_and_explains_none() {
        let pane = Pane::new();
        pane.commands
            .set_microphones(vec![microphone("a", false), microphone("b", true)]);
        // One microphone unmuted: every microphone is muted.
        assert_eq!(
            pane.run("Toggle Microphone Mute"),
            [hud("Microphones muted", ToastStyle::Success)]
        );
        assert_eq!(
            pane.commands.take(),
            [
                Done::SetMicrophoneMute("a".into(), true),
                Done::SetMicrophoneMute("b".into(), true)
            ]
        );
        // None unmuted any more: every microphone is unmuted.
        assert_eq!(
            pane.run("Toggle Microphone Mute"),
            [hud("Microphones unmuted", ToastStyle::Success)]
        );
        assert_eq!(
            pane.commands.take(),
            [
                Done::SetMicrophoneMute("a".into(), false),
                Done::SetMicrophoneMute("b".into(), false)
            ]
        );
        // No microphone at all: nothing happens and the answer says so.
        pane.commands.set_microphones(vec![]);
        assert_eq!(
            pane.run("Toggle Microphone Mute"),
            [hud(NO_MICROPHONE, ToastStyle::Failure)]
        );
        assert!(pane.commands.take().is_empty(), "nothing was asked");
    }

    #[test]
    fn sleep_on_modern_standby_turns_the_displays_off_and_a_missing_file_is_explained() {
        let pane = Pane::new();
        pane.commands.set_capabilities(capabilities(true, false));
        assert_eq!(pane.run("Sleep"), [hud("Sleeping", ToastStyle::Success)]);
        assert_eq!(pane.commands.take(), [Done::DisplaysOff]);
        assert_eq!(
            pane.run("Hibernate"),
            [hud(NO_HIBERNATION_FILE, ToastStyle::Failure)]
        );
        assert!(pane.commands.take().is_empty(), "nothing was asked");
    }

    #[test]
    fn the_bin_appearance_and_device_commands_answer_the_hud_of_the_state_they_ended_in() {
        let pane = Pane::new();
        // The fake starts in the light mode, with one capable display
        // with HDR off, hidden files hidden, one removable drive and one
        // radio on, each of which the toggle ends in the other state.
        assert_eq!(
            pane.run("Toggle System Appearance"),
            [hud("Dark mode", ToastStyle::Success)]
        );
        assert_eq!(
            pane.commands.take(),
            [Done::SetAppearance(Appearance::Dark)]
        );
        assert_eq!(pane.run("Toggle HDR"), [hud("HDR on", ToastStyle::Success)]);
        assert_eq!(pane.commands.take(), [Done::SetHdr("display".into(), true)]);
        assert_eq!(
            pane.run("Toggle Hidden Files"),
            [hud("Hidden files shown", ToastStyle::Success)]
        );
        assert_eq!(pane.commands.take(), [Done::SetHiddenFiles(true)]);
        assert_eq!(
            pane.run("Eject Removable Drives"),
            [hud("Ejected E:", ToastStyle::Success)]
        );
        assert_eq!(pane.commands.take(), [Done::Ejected("E:".into())]);
        assert_eq!(
            pane.run("Toggle Bluetooth"),
            [hud("Bluetooth off", ToastStyle::Success)]
        );
        assert_eq!(
            pane.commands.take(),
            [Done::SetBluetooth("radio".into(), false)]
        );
    }

    #[test]
    fn no_hdr_display_no_drive_and_no_radio_are_explained() {
        let pane = Pane::new();
        pane.commands.set_hdr_displays(vec![]);
        assert_eq!(pane.run("Toggle HDR"), [hud(NO_HDR, ToastStyle::Failure)]);
        pane.commands.set_drives(vec![]);
        assert_eq!(
            pane.run("Eject Removable Drives"),
            [hud(NO_REMOVABLE_DRIVE, ToastStyle::Failure)]
        );
        pane.commands.set_bluetooth_radios(vec![]);
        assert_eq!(
            pane.run("Toggle Bluetooth"),
            [hud(NO_BLUETOOTH, ToastStyle::Failure)]
        );
        assert!(pane.commands.take().is_empty(), "nothing was asked");
    }

    #[test]
    fn empty_recycle_bin_confirms_first_and_remembers_the_answer() {
        let pane = Pane::new();
        // Asked first, with the destructive style and "Don't ask again"
        // remembered under the command's id.
        pane.commands.set_bin(RecycleBin {
            items: 3,
            size: 1024,
        });
        let running = pane.start("Empty Recycle Bin");
        let confirmation = asked(&pane.launcher);
        assert_eq!(
            (
                confirmation.title.as_str(),
                confirmation.message.as_deref(),
                confirmation.primary.as_str(),
                confirmation.destructive,
                confirmation.rememberable
            ),
            (
                "Empty the Recycle Bin?",
                Some("Windows empties the Recycle Bin of every drive; what it holds is gone."),
                "Empty Recycle Bin",
                true,
                true
            )
        );
        // Confirmed without ticking it: the bin is emptied, and it asks
        // again next time.
        pane.launcher
            .answer_confirmation(confirmation.id, ConfirmAnswer::Confirmed, false);
        running.ended();
        assert_eq!(
            pane.huds(),
            [hud("Emptied the Recycle Bin", ToastStyle::Success)]
        );
        assert_eq!(pane.commands.take(), [Done::EmptiedBin]);
        assert!(
            pane.launcher
                .remembered_confirmations(&pane.identity)
                .is_empty()
        );

        // Ticked, the answer is remembered under the command's id and
        // given at once from then on; an already empty bin stays a
        // success.
        pane.commands.set_bin(RecycleBin {
            items: 2,
            size: 512,
        });
        let running = pane.start("Empty Recycle Bin");
        let confirmation = asked(&pane.launcher);
        pane.launcher
            .answer_confirmation(confirmation.id, ConfirmAnswer::Confirmed, true);
        running.ended();
        assert_eq!(
            pane.huds(),
            [hud("Emptied the Recycle Bin", ToastStyle::Success)]
        );
        assert_eq!(pane.commands.take(), [Done::EmptiedBin]);
        assert!(
            pane.launcher
                .remembered_confirmations(&pane.identity)
                .contains(&"empty-recycle-bin".to_owned())
        );
        let running = pane.start("Empty Recycle Bin");
        running.ended();
        assert_eq!(pane.launcher.confirmation(), None, "asked nothing");
        assert_eq!(
            pane.huds(),
            [hud("The Recycle Bin is already empty", ToastStyle::Success)]
        );
        assert!(pane.commands.take().is_empty(), "nothing was asked");

        // A dismissal changes nothing, ticked or not.
        pane.commands.set_bin(RecycleBin { items: 1, size: 8 });
        let running = pane.start("Empty Recycle Bin");
        let confirmation = asked(&pane.launcher);
        pane.launcher
            .answer_confirmation(confirmation.id, ConfirmAnswer::Dismissed, true);
        running.ended();
        assert!(pane.huds().is_empty(), "showed nothing");
        assert!(pane.commands.take().is_empty(), "did nothing");
    }

    #[test]
    fn the_destructive_ones_confirm_first_and_remember_the_answer() {
        let pane = Pane::new();
        let cases: [(&str, &str, &str, &str, Done); 3] = [
            (
                "Log Out",
                "Log out?",
                "Your session ends; applications that need saving are asked to close.",
                "Logging out",
                Done::Power(PowerRequest::LogOut, false),
            ),
            (
                "Restart",
                "Restart?",
                "Windows restarts, closing every application whether saved or not.",
                "Restarting",
                Done::Power(PowerRequest::Restart, true),
            ),
            (
                "Shut Down",
                "Shut down?",
                "Windows powers off, closing every application whether saved or not.",
                "Shutting down",
                Done::Power(PowerRequest::ShutDown, true),
            ),
        ];
        for (command, title, message, done_text, done) in cases {
            // Asked first, with the destructive style and "Don't ask
            // again" remembered under the command's id.
            let running = pane.start(command);
            let confirmation = asked(&pane.launcher);
            assert_eq!(
                (
                    confirmation.title.as_str(),
                    confirmation.message.as_deref(),
                    confirmation.primary.as_str(),
                    confirmation.destructive,
                    confirmation.rememberable
                ),
                (title, Some(message), command, true, true),
                "{command}"
            );
            // Confirmed without ticking it: it happens, and asks again
            // next time.
            pane.launcher
                .answer_confirmation(confirmation.id, ConfirmAnswer::Confirmed, false);
            running.ended();
            assert_eq!(
                pane.huds(),
                [hud(done_text, ToastStyle::Success)],
                "{command}"
            );
            assert_eq!(pane.commands.take(), vec![done.clone()], "{command}");
            assert!(
                pane.launcher
                    .remembered_confirmations(&pane.identity)
                    .is_empty()
            );

            // Ticked, the answer is remembered under the command's id and
            // given at once from then on.
            let running = pane.start(command);
            let confirmation = asked(&pane.launcher);
            pane.launcher
                .answer_confirmation(confirmation.id, ConfirmAnswer::Confirmed, true);
            running.ended();
            assert_eq!(
                pane.huds(),
                [hud(done_text, ToastStyle::Success)],
                "{command}"
            );
            assert_eq!(pane.commands.take(), vec![done.clone()], "{command}");
            assert!(
                pane.launcher
                    .remembered_confirmations(&pane.identity)
                    .contains(&command_id(command).to_owned()),
                "{command}: {:?}",
                pane.launcher.remembered_confirmations(&pane.identity)
            );
            let running = pane.start(command);
            running.ended();
            assert_eq!(
                pane.launcher.confirmation(),
                None,
                "{command} asked nothing"
            );
            assert_eq!(
                pane.huds(),
                [hud(done_text, ToastStyle::Success)],
                "{command}"
            );
            assert_eq!(pane.commands.take(), [done], "{command}");

            // A dismissal changes nothing, ticked or not.
            let running = pane.start(command);
            let confirmation = asked(&pane.launcher);
            pane.launcher
                .answer_confirmation(confirmation.id, ConfirmAnswer::Dismissed, true);
            running.ended();
            assert!(pane.huds().is_empty(), "{command} showed nothing");
            assert!(pane.commands.take().is_empty(), "{command} did nothing");
        }
        let remembered = pane.launcher.remembered_confirmations(&pane.identity);
        for id in ["log-out", "restart", "shut-down"] {
            assert!(remembered.contains(&id.to_owned()), "{id}: {remembered:?}");
        }
    }

    #[test]
    fn a_hotkey_runs_a_command_without_showing_the_window() {
        let pane = Pane::new();
        let shortcut = Shortcut::parse("ctrl+alt+l").unwrap();
        let set = pane
            .launcher
            .set_hotkey(&pane.id("lock-screen"), Some(shortcut.clone()));
        block_on(set.expect("the hotkey is accepted"));

        // Something else is typed, the window is where it is: the hotkey
        // runs the command without showing it.
        pane.search("abc");
        assert!(!pane.launcher.hotkey_shows_window(&shortcut));
        block_on(
            pane.launcher
                .press_hotkey(&shortcut)
                .expect("the hotkey is registered"),
        );
        assert_eq!(
            pane.huds(),
            [hud("Locking the screen", ToastStyle::Success)]
        );
        assert_eq!(pane.commands.take(), [Done::Locked]);
        assert_eq!(
            pane.launcher.view().screen,
            Screen::Root {
                query: "abc".into()
            }
        );
    }
}
