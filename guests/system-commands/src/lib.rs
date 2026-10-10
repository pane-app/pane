//! Pane's System Commands, a default extension of no-view commands (ADR
//! 0037, ADR 0040): one for each session and power command the
//! `system-commands` host functions offer — Lock Screen, Log Out,
//! Restart, Shut Down, Sleep, Hibernate, Turn Off Displays and Start
//! Screen Saver — one for each audio command — Volume Up, Volume Down,
//! Toggle Mute, Set Volume and Toggle Microphone Mute — and one for each
//! bin, appearance and device command — Open Recycle Bin, Empty Recycle
//! Bin, Toggle System Appearance, Toggle HDR, Show Desktop, Toggle
//! Hidden Files, Eject Removable Drives and Toggle Bluetooth — all
//! twenty-one sharing this component. A command's hotkey runs it without
//! showing Pane's window, and it answers with a HUD of the state the
//! system ended in, or why nothing changed (1.2 seconds, or 3 for the
//! failure style, ADR 0035): "Restarting", "Sleeping", "Volume 52%",
//! "Muted", "Microphones muted", "Hibernation is not available on this
//! computer: there is no hibernation file".
//!
//! Log Out, Restart, Shut Down and Empty Recycle Bin ask first, with a
//! destructive confirmation offering "Don't ask again" (ADR 0040): a
//! mistyped query never ends a session or empties the bin. Restart and
//! Shut Down then force applications closed, so an application with
//! unsaved work cannot hold one up, while Log Out leaves applications the
//! chance to save — the host's decision, not this extension's; each
//! command only calls the host function named for it. Set Volume takes
//! its level through its `level` argument (0 to 100), which Pane's
//! argument form asks for; anything else is explained in the HUD, as the
//! audio, appearance and device commands are not destructive and ask
//! nothing first. A background launch (a schedule) is answered with an
//! error, since no confirmation is available there: nothing happens.
//!
//! The extension declares `windows` alone in its pane.json, and only the
//! Windows default set lists it (ADR 0040).

#![no_std]

use pane_extension::alloc::{format, string::String};
use pane_extension::feedback::{self, Confirmation, ToastStyle};
use pane_extension::system_commands::{self, Outcome};
use pane_extension::{Command, LaunchRecord, NoCustomView};

struct SystemCommands;
pane_extension::export!(SystemCommands);

/// The commands' ids in `pane.json`, which `run` dispatches by.
const LOCK_SCREEN: &str = "lock-screen";
const LOG_OUT: &str = "log-out";
const RESTART: &str = "restart";
const SHUT_DOWN: &str = "shut-down";
const SLEEP: &str = "sleep";
const HIBERNATE: &str = "hibernate";
const DISPLAYS: &str = "turn-off-displays";
const SCREEN_SAVER: &str = "start-screen-saver";
const VOLUME_UP: &str = "volume-up";
const VOLUME_DOWN: &str = "volume-down";
const TOGGLE_MUTE: &str = "toggle-mute";
const SET_VOLUME: &str = "set-volume";
const TOGGLE_MICROPHONES: &str = "toggle-microphone-mute";
const OPEN_BIN: &str = "open-recycle-bin";
const EMPTY_BIN: &str = "empty-recycle-bin";
const APPEARANCE: &str = "toggle-appearance";
const HDR: &str = "toggle-hdr";
const DESKTOP: &str = "show-desktop";
const HIDDEN_FILES: &str = "toggle-hidden-files";
const DRIVES: &str = "eject-removable-drives";
const BLUETOOTH: &str = "toggle-bluetooth";

/// The name of Set Volume's `level` argument, as `pane.json` declares it
/// and the launch record carries it.
const LEVEL: &str = "level";

/// What Set Volume answers when its argument names no level from 0 to 100,
/// as the host answers a level beyond it: the same sentence either way.
const SET_VOLUME_RANGE: &str = "The volume can be set only from 0 to 100";

/// The destructive set, which asks first (ADR 0040): logging out,
/// restarting, shutting down and emptying the Recycle Bin. The host's
/// `Command` holds the same decision; this list follows it.
const DESTRUCTIVE: [&str; 4] = [LOG_OUT, RESTART, SHUT_DOWN, EMPTY_BIN];

/// One command's answer: the HUD of the state it ended in, or why nothing
/// changed (3 seconds for the failure style).
fn answer(outcome: Outcome) {
    match outcome {
        Outcome::Done(text) => feedback::show_hud(&text, ToastStyle::Success),
        Outcome::Explained(text) => feedback::show_hud(&text, ToastStyle::Failure),
    }
}

impl Command for SystemCommands {
    type CustomView = NoCustomView;

    async fn run(command: String, launch: LaunchRecord) -> Result<(), String> {
        // The destructive ones ask first, offering "Don't ask again"; a
        // background launch is answered that no confirmation is
        // available there, so nothing happens.
        if DESTRUCTIVE.contains(&command.as_str()) && !confirm(&command).await? {
            return Ok(());
        }
        let outcome = match command.as_str() {
            LOCK_SCREEN => system_commands::lock_screen(),
            LOG_OUT => system_commands::log_out(),
            RESTART => system_commands::restart(),
            SHUT_DOWN => system_commands::shut_down(),
            SLEEP => system_commands::sleep(),
            HIBERNATE => system_commands::hibernate(),
            DISPLAYS => system_commands::turn_off_displays(),
            SCREEN_SAVER => system_commands::start_screen_saver(),
            VOLUME_UP => system_commands::volume_up(),
            VOLUME_DOWN => system_commands::volume_down(),
            TOGGLE_MUTE => system_commands::toggle_mute(),
            SET_VOLUME => set_volume(&launch),
            TOGGLE_MICROPHONES => system_commands::toggle_microphone_mute(),
            OPEN_BIN => system_commands::open_recycle_bin(),
            EMPTY_BIN => system_commands::empty_recycle_bin(),
            APPEARANCE => system_commands::toggle_appearance(),
            HDR => system_commands::toggle_hdr(),
            DESKTOP => system_commands::show_desktop(),
            HIDDEN_FILES => system_commands::toggle_hidden_files(),
            DRIVES => system_commands::eject_removable_drives(),
            BLUETOOTH => system_commands::toggle_bluetooth(),
            other => return Err(format!("`{other}` is no system command")),
        };
        answer(outcome);
        Ok(())
    }
}

/// What Set Volume asks the host for: the level its `level` argument
/// names, from 0 to 100, with the mute left as it is. Anything else — no
/// number, or a number beyond 100, which the host explains the same way
/// — changes nothing and is explained.
fn set_volume(launch: &LaunchRecord) -> Outcome {
    let level = launch.argument(LEVEL).unwrap_or_default().trim();
    match level.parse::<u8>() {
        Ok(level) => system_commands::set_volume(level),
        Err(_) => Outcome::Explained(SET_VOLUME_RANGE.into()),
    }
}

/// Asks the user to confirm the destructive command `id`, offering "Don't
/// ask again" remembered under the command's id: once the user ticks it
/// and confirms, Pane answers `true` at once from then on.
async fn confirm(id: &str) -> Result<bool, String> {
    let (title, message, primary) = match id {
        LOG_OUT => (
            "Log out?",
            "Your session ends; applications that need saving are asked to close.",
            "Log Out",
        ),
        RESTART => (
            "Restart?",
            "Windows restarts, closing every application whether saved or not.",
            "Restart",
        ),
        EMPTY_BIN => (
            "Empty the Recycle Bin?",
            "Windows empties the Recycle Bin of every drive; what it holds is gone.",
            "Empty Recycle Bin",
        ),
        _ => (
            "Shut down?",
            "Windows powers off, closing every application whether saved or not.",
            "Shut Down",
        ),
    };
    let asked = Confirmation::new(title)
        .message(message)
        .primary(primary)
        .destructive()
        .remember(id);
    feedback::confirm(asked).await
}
