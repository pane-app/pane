//! The guest's side of the `system-commands` host functions
//! (`wit/system-commands.wit`): locking the screen, logging out,
//! restarting, shutting down, sleeping, hibernating, turning the displays
//! off and starting the screen saver, the audio commands — raising,
//! lowering, setting or muting the volume of the default output device,
//! and muting or unmuting every microphone — and the bin, appearance and
//! device commands: opening and emptying the Recycle Bin, the system's
//! light and dark appearance, HDR, the desktop, the file manager's
//! hidden files, the removable drives' ejection and Bluetooth. Each takes
//! the launcher's [`SystemCommands`] (through its `HostFunctions`),
//! decides what the command does — which commands force applications
//! closed, how `sleep` sleeps, whether `hibernate` can happen, how far a
//! volume step moves the level, whether the microphone toggle mutes or
//! unmutes — and has the system do it on a thread of its own: the
//! runtime thread awaits it, serving other packages' calls meanwhile,
//! and the wait is Pane's time, never the guest's computing (#18, #136).
//!
//! Each command answers what it ended in — the state the system is in
//! now, or why nothing changed — never an error and never a reason to
//! pause the extension. Stopped code does nothing more: each command
//! answers that the code was stopped. A runtime no launcher drives (tests
//! of the runtime alone) answers as a launcher given no system commands
//! does.

use std::sync::Arc;

use super::{GuestState, lock, stopped_code, system_commands_host};
use crate::system_commands::{self as host_commands, Command, Outcome, SystemCommands};

impl GuestState {
    /// The launcher's system commands, unless the instance's code is
    /// stopped (then why).
    fn system_commands(&self) -> Result<Arc<dyn SystemCommands>, String> {
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        // Taken out first: the launcher is locked to read its commands,
        // and never while the runtime's handle on them is.
        let host = lock(&self.host_functions).clone();
        Ok(host.map_or_else(host_commands::none, |host| host.system_commands()))
    }

    /// What `command` does through the launcher's system commands, off
    /// the runtime's thread, as the answer the command shows.
    async fn system_command(&mut self, command: Command) -> system_commands_host::Outcome {
        let commands = match self.system_commands() {
            Ok(commands) => commands,
            Err(why) => return wire(Outcome::Explained(why)),
        };
        let answer = self
            .hosted(off_thread(
                move || host_commands::run(command, commands.as_ref()),
                || Outcome::Explained(failed()),
            ))
            .await;
        wire(answer)
    }
}

/// `outcome` as the WIT carries it.
fn wire(outcome: Outcome) -> system_commands_host::Outcome {
    match outcome {
        Outcome::Done(text) => system_commands_host::Outcome::Done(text),
        Outcome::Explained(text) => system_commands_host::Outcome::Explained(text),
    }
}

/// Runs `work` on a thread of its own, since the system may block (the
/// session ending, the displays' power message), and answers what it
/// answered; `gone` answers when the thread could not start or failed.
async fn off_thread(
    work: impl FnOnce() -> Outcome + Send + 'static,
    gone: impl FnOnce() -> Outcome,
) -> Outcome {
    let (reply, response) = tokio::sync::oneshot::channel();
    let started = std::thread::Builder::new()
        .name("pane-system".into())
        .spawn(move || {
            let _ = reply.send(work());
        });
    if started.is_err() {
        return gone();
    }
    response.await.unwrap_or_else(|_| gone())
}

/// What a command is answered when the system's thread could not start or
/// failed.
fn failed() -> String {
    "Pane could not reach the system for this (its thread failed)".into()
}

impl system_commands_host::Host for GuestState {
    async fn lock_screen(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::LockScreen).await
    }

    async fn log_out(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::LogOut).await
    }

    async fn restart(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::Restart).await
    }

    async fn shut_down(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::ShutDown).await
    }

    async fn sleep(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::Sleep).await
    }

    async fn hibernate(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::Hibernate).await
    }

    async fn turn_off_displays(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::TurnOffDisplays).await
    }

    async fn start_screen_saver(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::StartScreenSaver).await
    }

    async fn volume_up(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::VolumeUp).await
    }

    async fn volume_down(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::VolumeDown).await
    }

    async fn toggle_mute(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::ToggleMute).await
    }

    async fn set_volume(&mut self, level: u8) -> system_commands_host::Outcome {
        self.system_command(Command::SetVolume(level)).await
    }

    async fn toggle_microphone_mute(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::ToggleMicrophoneMute).await
    }

    async fn open_recycle_bin(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::OpenRecycleBin).await
    }

    async fn empty_recycle_bin(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::EmptyRecycleBin).await
    }

    async fn toggle_appearance(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::ToggleAppearance).await
    }

    async fn toggle_hdr(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::ToggleHdr).await
    }

    async fn show_desktop(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::ShowDesktop).await
    }

    async fn toggle_hidden_files(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::ToggleHiddenFiles).await
    }

    async fn eject_removable_drives(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::EjectRemovableDrives).await
    }

    async fn toggle_bluetooth(&mut self) -> system_commands_host::Outcome {
        self.system_command(Command::ToggleBluetooth).await
    }
}
