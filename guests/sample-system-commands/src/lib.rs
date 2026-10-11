//! Pane's system commands sample: a command that calls the session and
//! power commands ([`pane_extension::system_commands`], #255) — Lock
//! Screen, Log Out, Restart, Shut Down, Sleep, Hibernate, Turn Off
//! Displays and Start Screen Saver — and the audio commands (#265) —
//! Volume Up, Volume Down, Toggle Mute, Set Volume and Toggle Microphone
//! Mute — and the bin, appearance and device commands (#266) — Open
//! Recycle Bin, Empty Recycle Bin, Toggle System Appearance, Toggle HDR,
//! Show Desktop, Toggle Hidden Files, Eject Removable Drives and Toggle
//! Bluetooth — one item for each, which runs it and says what it answered
//! in a toast: the state the system ended in ("Sleep: Sleeping", "Volume
//! Up: Volume 52%"), or why nothing changed ("Hibernate: Hibernation is
//! not available on this computer: there is no hibernation file").
//!
//! The sample calls each host function as it is: no confirmation, no HUD
//! — the System Commands default extension (pane-app/system-commands) is the
//! one that composes them with ADR 0037's confirm and HUD host functions,
//! and a test drives this sample with a fake system, so nothing it runs
//! ever reaches the real one. Set Volume takes no argument of the
//! sample's own: it sets the level the sample chose, standing in for the
//! argument the System Commands extension declares. The JavaScript and
//! TypeScript samples answer the same.

#![no_std]

use pane_extension::alloc::{format, string::String};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::system_commands::{self, Outcome};
use pane_extension::{Action, Command, Item, List, NoCustomView};

struct SystemCommandsSample;
pane_extension::export!(SystemCommandsSample);

/// One item of the sample: the host function it calls, by what it is
/// called.
struct Step {
    /// The item's id.
    id: &'static str,
    /// The item's title.
    title: &'static str,
    /// Its subtitle.
    subtitle: &'static str,
    /// What it asks the system for.
    ask: fn() -> Outcome,
}

const STEPS: [Step; 21] = [
    Step {
        id: "lock-screen",
        title: "Lock Screen",
        subtitle: "Locks the screen; your session stays on",
        ask: system_commands::lock_screen,
    },
    Step {
        id: "log-out",
        title: "Log Out",
        subtitle: "Ends your session; applications are not forced closed",
        ask: system_commands::log_out,
    },
    Step {
        id: "restart",
        title: "Restart",
        subtitle: "Restarts the computer; applications are forced closed",
        ask: system_commands::restart,
    },
    Step {
        id: "shut-down",
        title: "Shut Down",
        subtitle: "Powers the computer off; applications are forced closed",
        ask: system_commands::shut_down,
    },
    Step {
        id: "sleep",
        title: "Sleep",
        subtitle: "Sleeps the computer, as its Modern Standby signal decides",
        ask: system_commands::sleep,
    },
    Step {
        id: "hibernate",
        title: "Hibernate",
        subtitle: "Hibernates the computer, or says that it cannot",
        ask: system_commands::hibernate,
    },
    Step {
        id: "turn-off-displays",
        title: "Turn Off Displays",
        subtitle: "Turns the displays off",
        ask: system_commands::turn_off_displays,
    },
    Step {
        id: "start-screen-saver",
        title: "Start Screen Saver",
        subtitle: "Starts the screen saver, or says that none is set",
        ask: system_commands::start_screen_saver,
    },
    Step {
        id: "volume-up",
        title: "Volume Up",
        subtitle: "Raises the volume by the step Windows' volume keys take",
        ask: system_commands::volume_up,
    },
    Step {
        id: "volume-down",
        title: "Volume Down",
        subtitle: "Lowers the volume by the same step",
        ask: system_commands::volume_down,
    },
    Step {
        id: "toggle-mute",
        title: "Toggle Mute",
        subtitle: "Mutes the sound, or unmutes it, saying the volume it ended at",
        ask: system_commands::toggle_mute,
    },
    Step {
        id: "set-volume",
        title: "Set Volume",
        subtitle: "Sets the volume to 40, the level this sample chose",
        ask: set_volume_to_40,
    },
    Step {
        id: "toggle-microphone-mute",
        title: "Toggle Microphone Mute",
        subtitle: "Mutes every microphone when any is on, unmutes them all otherwise",
        ask: system_commands::toggle_microphone_mute,
    },
    Step {
        id: "open-recycle-bin",
        title: "Open Recycle Bin",
        subtitle: "Opens the Recycle Bin in File Explorer",
        ask: system_commands::open_recycle_bin,
    },
    Step {
        id: "empty-recycle-bin",
        title: "Empty Recycle Bin",
        subtitle: "Empties the Recycle Bin; an already empty one is a success",
        ask: system_commands::empty_recycle_bin,
    },
    Step {
        id: "toggle-appearance",
        title: "Toggle System Appearance",
        subtitle: "Switches Windows between light and dark",
        ask: system_commands::toggle_appearance,
    },
    Step {
        id: "toggle-hdr",
        title: "Toggle HDR",
        subtitle: "Turns HDR on when any capable display is off, otherwise off",
        ask: system_commands::toggle_hdr,
    },
    Step {
        id: "show-desktop",
        title: "Show Desktop",
        subtitle: "Hides the open windows, or brings them back",
        ask: system_commands::show_desktop,
    },
    Step {
        id: "toggle-hidden-files",
        title: "Toggle Hidden Files",
        subtitle: "Shows hidden files in File Explorer, or hides them again",
        ask: system_commands::toggle_hidden_files,
    },
    Step {
        id: "eject-removable-drives",
        title: "Eject Removable Drives",
        subtitle: "Ejects every removable drive, reporting each drive that refused",
        ask: system_commands::eject_removable_drives,
    },
    Step {
        id: "toggle-bluetooth",
        title: "Toggle Bluetooth",
        subtitle: "Turns Bluetooth on when any radio is off, otherwise off",
        ask: system_commands::toggle_bluetooth,
    },
];

/// What the sample's Set Volume item asks for: a level of the sample's
/// own, since the sample declares no argument.
fn set_volume_to_40() -> Outcome {
    system_commands::set_volume(40)
}

/// `step` as an item of the list: running it calls its host function and
/// says what it answered in a toast.
fn item(step: &Step) -> Item {
    let title = step.title;
    let ask = step.ask;
    Item::new(step.id, step.title)
        .subtitle(step.subtitle)
        .action(Action::new(title, move || async move {
            answer(title, ask());
            Ok::<(), String>(())
        }))
}

/// Says what `ask` answered in a toast: a success when it happened, a
/// failure when nothing changed.
fn answer(title: &str, outcome: Outcome) {
    let toast = if outcome.done() {
        Toast::success(format!("{title}: {}", outcome.text()))
    } else {
        Toast::failure(format!("{title}: {}", outcome.text()))
    };
    show_toast(toast);
}

impl Command for SystemCommandsSample {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        Ok(List::new("System commands").items(STEPS.iter().map(item)))
    }
}
