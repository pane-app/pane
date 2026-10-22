//! Which installed packages use the network, on the extension list: a
//! package whose component imports `wasi:http` says "Uses the network",
//! and a row per such package shows the addresses it tried to reach this
//! session. Pane does not gate web requests (extensions are trusted code,
//! ADR 0018); this is what the user can see of them.

use super::{Entry, Launcher, LauncherView, Row, Screen, State};
use crate::packages::PackageIdentity;

/// What the extension list says of a package that uses the network, after
/// its state.
pub(super) const USES_THE_NETWORK: &str = "Uses the network";

impl Launcher {
    /// A row per installed package that uses the network, showing what it
    /// reached.
    pub(super) fn network_rows(&self, state: &State) -> Vec<(Row, Entry)> {
        state
            .packages
            .iter()
            .filter(|package| package.uses_network)
            .map(|package| {
                let row = Row {
                    id: format!("network:{}", package.identity.key()),
                    title: details_title(&package.title()),
                    subtitle: Some("The addresses it tried to reach this session".into()),
                    unavailable: None,
                };
                (row, Entry::NetworkDetails(package.identity.clone()))
            })
            .collect()
    }

    /// Shows what the installed package with `identity` did on the network
    /// this session; the extension list if it is gone or does not use it.
    pub(super) fn show_network_details(&self, state: &mut State, identity: &PackageIdentity) {
        let Some(package) = state
            .package(identity)
            .filter(|package| package.uses_network)
        else {
            self.show_extensions(state);
            return;
        };
        let title = package.title();
        let contacted = self
            .runtime()
            .map(|runtime| runtime.contacted(&identity.key()))
            .unwrap_or_default();
        let mut details = vec![
            format!(
                "{title} can make web requests: its code imports wasi:http. Pane sends them \
                 to any address it asks for, this computer's own services and the local \
                 network included."
            ),
            format!("From {identity}"),
        ];
        if contacted.is_empty() {
            details.push("It has tried to reach no address this session.".into());
        } else {
            details.push("Addresses it tried to reach this session:".into());
            details.extend(contacted);
        }
        state.next_screen();
        state.entries = Vec::new();
        let screen = Screen::NetworkDetails {
            identity: identity.clone(),
            details,
        };
        state.view = LauncherView::new(screen, details_title(&title));
    }
}

/// The title of the network details of the package titled `title`.
fn details_title(title: &str) -> String {
    format!("Network use of {title}")
}
