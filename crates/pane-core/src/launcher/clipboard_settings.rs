//! Pane's own Clipboard History on its extension's page in Settings (#166):
//! recording, retention, the applications whose copies are not recorded,
//! and clearing the history, beside the Actions panel's entries for the
//! same (see `clipboard_view`).
//!
//! The package declares three preferences in its `pane.json` —
//! [`KEEP_HISTORY_FOR`] (a dropdown of retentions), [`PAUSE_RECORDING`] (a
//! checkbox) and [`DISABLED_APPLICATIONS`] (a list of applications, picked
//! with the system's application picker) — so its Settings page draws them
//! with the controls every preference has. Their values are not stored as
//! settings: they are the clipboard history's own state, which Pane keeps
//! and honours before recording (ADR 0020, ADR 0042). Reading them reads
//! the history ([`Launcher::preferences_of`] overlays them), and setting
//! one changes the history through its existing operations
//! ([`Launcher::set_preference`] routes them here), so the Settings page,
//! the Actions panel and the history never disagree. Only Pane's
//! registered Clipboard History default extension is so: another
//! package declaring preferences of the same names keeps them as settings.
//!
//! Clear History is an operation of the extension's card (an
//! extension-list row, [`CLEAR_ROW`]), asking first, as clearing a cache
//! does.

use std::collections::BTreeMap;

use super::clipboard_view::{CLIPBOARD_HISTORY, kept_items};
use super::{Entry, Launcher, LauncherView, Question, Row, Screen, State, Status};
use crate::clipboard::{CaptureState, Commands, DEFAULT_RETENTION_SECONDS, program_file_name};
use crate::packages::PackageIdentity;

/// The preference choosing how long each item is kept, in seconds: a
/// dropdown of the retentions the Actions panel offers too.
pub const KEEP_HISTORY_FOR: &str = "keepHistoryFor";

/// The preference pausing recording: a checkbox, on while recording is
/// paused (or off).
pub const PAUSE_RECORDING: &str = "pauseRecording";

/// The preference naming the applications whose copies are not recorded:
/// their file names, separated by commas (`KeePass.exe, 1Password.exe`).
pub const DISABLED_APPLICATIONS: &str = "disabledApplications";

/// The kind of the extension-list row that clears the history: its id is
/// `clear-clipboard-history:<identity key>`.
pub const CLEAR_ROW: &str = "clear-clipboard-history";

/// Whether `identity` is Pane's own Clipboard History default extension.
fn is_own(identity: &PackageIdentity) -> bool {
    *identity == PackageIdentity::default_extension(CLIPBOARD_HISTORY)
}

/// The applications `value` names, as file names: separated by commas,
/// semicolons or new lines, each trimmed, a path read as its file name.
fn applications(value: &str) -> Vec<String> {
    value
        .split([',', ';', '\n'])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| program_file_name(name).trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect()
}

impl Launcher {
    /// The values of Pane's own Clipboard History's preferences, read from
    /// its history now, when `identity` is it; none for every other
    /// package. A history that cannot be read gives none, so its page
    /// shows the defaults.
    pub(super) fn clipboard_preference_values(
        &self,
        identity: &PackageIdentity,
    ) -> BTreeMap<String, String> {
        let mut values = BTreeMap::new();
        if !is_own(identity) {
            return values;
        }
        let Some(installation) = &self.installation else {
            return values;
        };
        let Ok(history) = installation.data.clipboard_history().get(&identity.key()) else {
            return values;
        };
        values.insert(KEEP_HISTORY_FOR.into(), history.retention().to_string());
        values.insert(
            PAUSE_RECORDING.into(),
            (history.capture != CaptureState::On).to_string(),
        );
        if !history.excluded.is_empty() {
            let names: Vec<&str> = history.excluded.iter().map(|name| name.as_str()).collect();
            values.insert(DISABLED_APPLICATIONS.into(), names.join(", "));
        }
        values
    }

    /// Sets the preference kept as `key` of Pane's own Clipboard History to
    /// `value` by changing its history, when `identity` is it and `key` is
    /// one of its controls: `Some` with whether it changed (an empty value
    /// is the default: 7 days, recording, no application disabled); `None`
    /// for every other preference, which is stored as a setting.
    pub(super) fn set_clipboard_preference(
        &self,
        identity: &PackageIdentity,
        key: &str,
        value: &str,
    ) -> Option<Result<(), String>> {
        if !is_own(identity)
            || ![KEEP_HISTORY_FOR, PAUSE_RECORDING, DISABLED_APPLICATIONS].contains(&key)
        {
            return None;
        }
        let Some(installation) = &self.installation else {
            return Some(Err("this launcher does not install packages".into()));
        };
        let data = installation.data.owned_by(identity);
        let commands = Commands {
            data: &data,
            capture: self.clipboard.clone(),
        };
        let value = value.trim();
        Some(match key {
            KEEP_HISTORY_FOR if value.is_empty() => {
                commands.set_retention(DEFAULT_RETENTION_SECONDS)
            }
            KEEP_HISTORY_FOR => value
                .parse::<u64>()
                .map_err(|_| format!("“{value}” is not a number of seconds"))
                .and_then(|seconds| commands.set_retention(seconds)),
            PAUSE_RECORDING if value == "true" => commands.status().and_then(|status| {
                if status.capture == CaptureState::On {
                    commands.set_capture(CaptureState::Paused)
                } else {
                    Ok(())
                }
            }),
            PAUSE_RECORDING => commands.status().and_then(|status| {
                if status.capture == CaptureState::On {
                    Ok(())
                } else {
                    commands.set_capture(CaptureState::On)
                }
            }),
            _ => commands.set_excluded(&applications(value)),
        })
    }

    /// The extension list's Clear History row of Pane's own Clipboard
    /// History, while it is installed and runs: its card's "Clear history"
    /// button in Settings.
    pub(super) fn clipboard_rows(&self, state: &State) -> Vec<(Row, Entry)> {
        state
            .packages
            .iter()
            .filter(|package| is_own(&package.identity) && state.runs(package))
            .map(|package| {
                let row = Row {
                    id: format!("{CLEAR_ROW}:{}", package.identity.key()),
                    title: format!("Clear history of {}", package.title()),
                    subtitle: Some("Deletes every item it kept; recording does not change".into()),
                    unavailable: None,
                };
                (
                    row,
                    Entry::AskClearClipboardHistory(package.identity.clone()),
                )
            })
            .collect()
    }

    /// Asks whether to clear the history of Pane's own Clipboard History
    /// (`identity`), saying how many items it deletes and that recording
    /// goes on.
    pub(super) fn show_clear_clipboard_history(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) {
        let title = state.title_of(identity);
        let kept = self
            .installation
            .as_ref()
            .and_then(|installation| {
                installation
                    .data
                    .clipboard_history()
                    .get(&identity.key())
                    .ok()
            })
            .map_or(0, |history| history.items.len());
        let choice = |title: &str, subtitle: &str| Row {
            id: title.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            unavailable: None,
        };
        state.next_screen();
        state.entries = vec![
            Entry::ClearClipboardHistory(identity.clone()),
            Entry::Cancel,
        ];
        let details = vec![
            format!("From {identity}"),
            format!(
                "Pane deletes the {} this extension kept. Recording goes on as it is, and it \
                 cannot be undone.",
                kept_items(kept)
            ),
        ];
        let screen = Screen::Confirm {
            question: Question::ClearClipboardHistory(identity.clone()),
            details,
        };
        state.view =
            LauncherView::new(screen, format!("Clear the history of {title}?")).with_rows(vec![
                choice("Clear history", "Delete every kept item now"),
                choice("Cancel", "Keep the history"),
            ]);
    }

    /// Clears the history of Pane's own Clipboard History (`identity`)
    /// through the history's existing clear, as its confirmation asked,
    /// then shows the extension list at its row with the outcome.
    pub(super) fn clear_clipboard_history_of(&self, state: &mut State, identity: &PackageIdentity) {
        let title = state.title_of(identity);
        let cleared = match &self.installation {
            Some(installation) => Commands {
                data: &installation.data.owned_by(identity),
                capture: self.clipboard.clone(),
            }
            .clear(),
            None => Err("this launcher does not install packages".into()),
        };
        self.show_extensions_at_clear_clipboard_history(state, identity);
        state.view.status = match cleared {
            Ok(deleted) => Status::Result(format!("Deleted {} of {title}", kept_items(deleted))),
            Err(why) => Status::Error(format!("Could not clear the history of {title}: {why}")),
        };
    }

    /// Shows the extension list with the Clear History row of `identity`
    /// selected, where the user asked.
    pub(super) fn show_extensions_at_clear_clipboard_history(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) {
        self.show_extensions_at(
            state,
            |entry| matches!(entry, Entry::AskClearClipboardHistory(asked) if asked == identity),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applications_are_named_by_their_file_names() {
        assert_eq!(
            applications(" KeePass.exe, ,1Password.exe;\nC:\\Tools\\Bitwarden.exe "),
            ["KeePass.exe", "1Password.exe", "Bitwarden.exe"]
        );
        assert!(applications(" , ").is_empty());
    }
}
