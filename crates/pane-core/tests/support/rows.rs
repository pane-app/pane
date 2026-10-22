//! Finding the launcher's rows by title and choosing one, as a user reads
//! and picks them, shared by the test binaries that drive the launcher.

#![allow(dead_code)]

use futures::executor::block_on;
use pane_core::{Launcher, Screen};

/// Root search's row that opens the extension manager.
pub const MANAGE_ROW: &str = "Manage Extensions";

/// The titles of the rows on screen, in order.
pub fn titles(launcher: &Launcher) -> Vec<String> {
    launcher
        .view()
        .rows
        .into_iter()
        .map(|row| row.title)
        .collect()
}

/// Selects the row titled `title`; panics, listing the rows, if there is
/// none.
pub fn select_title(launcher: &Launcher, title: &str) {
    let index = titles(launcher)
        .iter()
        .position(|row| row == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)));
    launcher.select(index);
}

/// Goes back from the extension manager's screens to root search.
pub fn to_root(launcher: &Launcher) {
    for _ in 0..3 {
        launcher.back();
    }
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
}

/// Opens the extension manager from root search.
pub fn manage(launcher: &Launcher) {
    to_root(launcher);
    select_title(launcher, MANAGE_ROW);
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
}
