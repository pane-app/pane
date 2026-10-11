//! The open windows the `windows` host functions act on
//! (`wit/windows.wit`, `crate::switch_windows`): the windows Alt+Tab
//! would show, and bringing one of them to the front. The launcher keeps
//! them, as it keeps its system commands
//! (`crate::launcher::system_commands`), and hands them to the runtime's
//! host calls through `feedback::Hosted`; the calls themselves run off
//! the runtime's thread (`crate::runtime`'s `windows_functions`), so the
//! launcher is never locked while the windows are enumerated or one is
//! brought to the front.

use std::sync::Arc;

use super::Launcher;
use crate::switch_windows::SwitchWindows;

impl Launcher {
    /// This launcher's commands listing the open windows and bringing
    /// one of them to the front through `windows`, normally Windows'
    /// ([`crate::switch_windows::native`]). Without one, each of those
    /// host functions answers that this Pane lists no open windows.
    pub fn with_switch_windows(self, windows: Arc<dyn SwitchWindows>) -> Self {
        self.lock().switch_windows = windows;
        self
    }

    /// The open windows the `windows` host functions act on.
    pub(super) fn switch_windows(&self) -> Arc<dyn SwitchWindows> {
        self.lock().switch_windows.clone()
    }
}
