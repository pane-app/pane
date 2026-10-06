//! The system the `system` host functions act on (`wit/system.wit`,
//! `crate::system`): the clipboard, opening things, the file manager and
//! the Recycle Bin. The launcher keeps it, as it keeps its link opener, and
//! hands it to the runtime's host calls through `feedback::Hosted`; the
//! calls themselves run off the runtime's thread (`crate::runtime`'s
//! `system_functions`), so the launcher is never locked while the system
//! works.

use std::sync::Arc;

use super::Launcher;
use crate::system::System;

impl Launcher {
    /// This launcher's commands using the clipboard, opening things,
    /// revealing them in the file manager and moving them to the Recycle
    /// Bin through `system`, normally the system's own
    /// ([`crate::system::native`]). Without one, each of those host
    /// functions answers that this Pane does not reach the system.
    pub fn with_system(self, system: Arc<dyn System>) -> Self {
        self.lock().system = system;
        self
    }

    /// The system the `system` host functions act on.
    pub(super) fn system(&self) -> Arc<dyn System> {
        self.lock().system.clone()
    }
}
