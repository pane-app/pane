//! The Run dialog's work the `run` host functions act on
//! (`wit/run.wit`, `crate::run`): running what Windows' Run dialog (Win+R)
//! runs, elevated through Windows' own prompt, and the history the two
//! share. The launcher keeps it, as it keeps its system, and hands it to
//! the runtime's host calls through `feedback::Hosted`; the calls
//! themselves run off the runtime's thread (`crate::runtime`'s
//! `run_functions`), so the launcher is never locked while the shell
//! starts a program or the elevation prompt waits for the user.

use std::sync::Arc;

use super::Launcher;
use crate::run::Run;

impl Launcher {
    /// This launcher's commands running what the Run dialog runs and
    /// sharing its history through `run`, normally Windows'
    /// ([`crate::run::native`]). Without one, each of those host
    /// functions answers that this Pane runs nothing and keeps no Run
    /// history.
    pub fn with_run(self, run: Arc<dyn Run>) -> Self {
        self.lock().run = run;
        self
    }

    /// The Run dialog's work the `run` host functions act on.
    pub(super) fn run(&self) -> Arc<dyn Run> {
        self.lock().run.clone()
    }
}
