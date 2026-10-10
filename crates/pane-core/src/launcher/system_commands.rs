//! The session and power commands the `system-commands` host functions
//! act on (`wit/system-commands.wit`, `crate::system_commands`): locking
//! the screen, logging out, restarting, shutting down, sleeping,
//! hibernating, turning the displays off and starting the screen saver.
//! The launcher keeps them, as it keeps its system
//! (`crate::launcher::system`), and hands them to the runtime's host
//! calls through `feedback::Hosted`; the calls themselves run off the
//! runtime's thread (`crate::runtime`'s `system_command_functions`), so
//! the launcher is never locked while the system works.

use std::sync::Arc;

use super::Launcher;
use crate::system_commands::SystemCommands;

impl Launcher {
    /// This launcher's commands locking, logging out, restarting,
    /// shutting down, sleeping, hibernating, turning the displays off and
    /// starting the screen saver through `system-commands`, normally the
    /// system's own ([`crate::system_commands::native`]). Without one,
    /// each of those host functions answers that this Pane reaches no
    /// session and power commands.
    pub fn with_system_commands(self, commands: Arc<dyn SystemCommands>) -> Self {
        self.lock().system_commands = commands;
        self
    }

    /// The session and power commands the `system-commands` host
    /// functions act on.
    pub(super) fn system_commands(&self) -> Arc<dyn SystemCommands> {
        self.lock().system_commands.clone()
    }
}
