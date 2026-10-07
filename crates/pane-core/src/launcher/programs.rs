//! Which installed packages run system programs (ADR 0033, #147), on the
//! extension list: a package whose component imports
//! `pane:extension/programs` says "Runs system programs", and a row per such
//! package shows the programs it ran this session. Its install, update and
//! reload preview says so too. Pane does not gate the programs a command
//! runs (extensions are trusted code, ADR 0002): beyond the system's own
//! elevation prompt, this is what the user sees of them, as network use is.

use super::{Entry, Launcher, LauncherView, Row, Screen, State};
use crate::packages::PackageIdentity;

/// What the extension list says of a package that runs system programs,
/// after its state.
pub(super) const RUNS_SYSTEM_PROGRAMS: &str = "Runs system programs";

/// What a package preview says of a package whose code runs system
/// programs.
pub(super) const PREVIEW_NOTE: &str = "Runs system programs: its code imports \
     pane:extension/programs, so it can run any program on this computer";

impl Launcher {
    /// A row per installed package that runs system programs, showing which
    /// it ran.
    pub(super) fn program_rows(&self, state: &State) -> Vec<(Row, Entry)> {
        state
            .packages
            .iter()
            .filter(|package| package.uses_programs)
            .map(|package| {
                let row = Row {
                    id: format!("programs:{}", package.identity.key()),
                    title: details_title(&package.title()),
                    subtitle: Some("The programs it ran this session".into()),
                    unavailable: None,
                };
                (row, Entry::ProgramDetails(package.identity.clone()))
            })
            .collect()
    }

    /// Shows the programs the installed package with `identity` ran this
    /// session; the extension list if it is gone or runs none.
    pub(super) fn show_program_details(&self, state: &mut State, identity: &PackageIdentity) {
        let Some(package) = state
            .package(identity)
            .filter(|package| package.uses_programs)
        else {
            self.show_extensions(state);
            return;
        };
        let title = package.title();
        let ran = self
            .runtime()
            .map(|runtime| runtime.programs_run(&identity.key()))
            .unwrap_or_default();
        let mut details = vec![
            format!(
                "{title} can run programs installed on this computer: its code imports \
                 pane:extension/programs. Pane runs any program it names, and ends each with \
                 everything it started once the command is done with it."
            ),
            format!("From {identity}"),
        ];
        if ran.is_empty() {
            details.push("It has run no program this session.".into());
        } else {
            details.push("Programs it ran this session:".into());
            details.extend(ran);
        }
        state.next_screen();
        state.entries = Vec::new();
        let screen = Screen::ProgramDetails {
            identity: identity.clone(),
            details,
        };
        state.view = LauncherView::new(screen, details_title(&title));
    }
}

/// The title of the program details of the package titled `title`.
fn details_title(title: &str) -> String {
    format!("Programs run by {title}")
}
