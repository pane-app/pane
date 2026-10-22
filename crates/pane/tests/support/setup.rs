//! Setting up the windows the way the binary does: the host settings
//! initialized from a record, and the keystroke that opens Settings.
//! Shared by the test binaries of `pane`.
#![allow(dead_code)]

use std::path::Path;

use gpui::TestAppContext;

/// The keystroke that opens Settings on this platform.
pub fn settings_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-,"
    } else {
        "ctrl-,"
    }
}

/// How Settings' keystroke ([`settings_shortcut`]) is named in hints and
/// messages on this platform.
pub fn settings_shortcut_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Command+,"
    } else {
        "Ctrl+,"
    }
}

/// The keystroke that opens the selected result's actions on this
/// platform (Ctrl+K deletes to the end of the line in a macOS field).
pub fn actions_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-k"
    } else {
        "ctrl-k"
    }
}

/// How the actions keystroke ([`actions_shortcut`]) is named in hints
/// and messages on this platform.
pub fn actions_shortcut_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Command+K"
    } else {
        "Ctrl+K"
    }
}

/// How the Ctrl modifier is named in hints and messages on this
/// platform: written out on macOS ("Control+K"), short elsewhere.
pub const CTRL: &str = if cfg!(target_os = "macos") {
    "Control"
} else {
    "Ctrl"
};

/// Initializes the settings record of `data` in `cx`, as the binary does
/// before its first window opens.
pub fn init_settings(data: Option<&Path>, cx: &mut TestAppContext) {
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            data.map(|data| data.to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
}
