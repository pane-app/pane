//! The subtitles commands give their root search rows (`commands.set-subtitle`,
//! #141), such as "3 unread": Pane's own record by command id
//! (`subtitles.json` beside `installed.json`, see `choices`), so a subtitle
//! survives restarts and updates and is forgotten when its package is
//! uninstalled. A command's row shows its subtitle instead of the one its
//! `pane.json` entry declares, and root search matches it.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::choices::{Choices, Record};
use super::{Launcher, State, owner};
use crate::feedback::Caller;
use crate::packages::PackageIdentity;

/// The longest subtitle Pane keeps, in characters; a longer one is cut
/// with an ellipsis.
const MAX_SUBTITLE_CHARS: usize = 200;

/// The subtitles commands set, recorded in `subtitles.json` as
/// `{ "version": 1, "subtitles": { "<command id>": "3 unread" } }`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Subtitles {
    by_command: BTreeMap<String, String>,
}

impl Subtitles {
    /// The subtitle the command `command` set, if it set one.
    pub(super) fn subtitle_of(&self, command: &str) -> Option<&str> {
        self.by_command.get(command).map(String::as_str)
    }
}

impl Choices for Subtitles {
    const FILE: &'static str = "subtitles.json";
    const VERSION: u64 = 1;
    const WHAT: &'static str = "subtitles";

    fn read(fields: &Map<String, Value>) -> Result<Self, String> {
        let mut subtitles = Subtitles::default();
        if let Some(by_command) = fields.get("subtitles") {
            let by_command = by_command
                .as_object()
                .ok_or("`subtitles` is not an object")?;
            for (command, subtitle) in by_command {
                if let Some(subtitle) = subtitle.as_str() {
                    subtitles
                        .by_command
                        .insert(command.clone(), subtitle.to_owned());
                }
            }
        }
        Ok(subtitles)
    }

    fn write(&self) -> Map<String, Value> {
        let by_command = self
            .by_command
            .iter()
            .map(|(command, subtitle)| (command.clone(), Value::String(subtitle.clone())))
            .collect();
        Map::from_iter([("subtitles".to_string(), Value::Object(by_command))])
    }

    fn restore(&mut self, command: &str, other: &Self) {
        match other.by_command.get(command) {
            Some(subtitle) => self.by_command.insert(command.to_owned(), subtitle.clone()),
            None => self.by_command.remove(command),
        };
    }

    fn retain(&mut self, keep: &dyn Fn(&str) -> bool) -> bool {
        let before = self.by_command.len();
        self.by_command.retain(|command, _| keep(command));
        self.by_command.len() != before
    }

    fn of(state: &mut State) -> &mut Record<Self> {
        &mut state.subtitles
    }
}

impl Launcher {
    /// Sets the subtitle of the command `caller` runs for (none gives back
    /// its manifest's), shows it at once where root search is on screen,
    /// and records it off the calling thread (the runtime's).
    pub(super) fn set_subtitle(
        &self,
        caller: &Caller,
        subtitle: Option<String>,
    ) -> Result<(), String> {
        let command = caller.command.as_deref().ok_or(
            "Pane does not know which command this call is for; set the subtitle from the \
             command's own screen, run or actions",
        )?;
        let subtitle = subtitle
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
            .map(|text| cut(&text));
        let mut state = self.lock();
        let state = &mut *state;
        let id = match owner(&state.packages, &caller.component) {
            Some(package) => package.identity.command_id(command),
            None => self
                .commands
                .iter()
                .find(|registered| registered.component == caller.component)
                .map(|registered| registered.id.clone())
                .ok_or("only a command Pane lists in root search has a subtitle")?,
        };
        let chosen = &mut state.subtitles.chosen.by_command;
        let changed = match subtitle {
            Some(subtitle) => chosen.insert(id.clone(), subtitle.clone()) != Some(subtitle),
            None => chosen.remove(&id).is_some(),
        };
        if !changed {
            return Ok(());
        }
        if matches!(state.view.screen, super::Screen::Root { .. }) {
            self.refresh_root(state);
        } else {
            // Shown when root search is next listed.
            state.root = self.root_results(state);
        }
        let launcher = self.clone();
        let saves = state.subtitle_saves.clone();
        let ended = saves.clone();
        saves.begin();
        let saving = std::thread::Builder::new()
            .name("pane-subtitle".into())
            .spawn(move || {
                if let Err(error) = launcher.save::<Subtitles>(None) {
                    eprintln!("Pane could not record a command's subtitle: {error}");
                }
                ended.end();
            });
        if let Err(error) = saving {
            saves.end();
            eprintln!("Pane could not record a command's subtitle: {error}");
        }
        self.changed();
        Ok(())
    }

    /// Waits until every subtitle set so far has been recorded; `false` if
    /// one has not within `limit`. For tests and development builds, which
    /// so wait for the record without timing it.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_subtitles_recorded(&self, limit: std::time::Duration) -> bool {
        let saves = self.lock().subtitle_saves.clone();
        saves.settled(limit)
    }

    /// The subtitle the installed or registered command `id` set for its
    /// root search row, if it set one.
    pub fn command_subtitle(&self, id: &str) -> Option<String> {
        self.lock()
            .subtitles
            .chosen
            .subtitle_of(id)
            .map(str::to_owned)
    }

    /// Forgets the subtitles of the uninstalled package with `identity`.
    /// Returns what writes the record without them, to run off the
    /// window's thread; nothing to write if it had none.
    pub(super) fn forget_subtitles_of(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) -> Option<impl FnOnce() -> Result<(), String> + Send + 'static> {
        if !state.subtitles.forget(identity) {
            return None;
        }
        state.root = self.root_results(state);
        let launcher = self.clone();
        Some(move || launcher.save::<Subtitles>(None))
    }
}

/// `text` within [`MAX_SUBTITLE_CHARS`].
fn cut(text: &str) -> String {
    if text.chars().count() <= MAX_SUBTITLE_CHARS {
        return text.to_owned();
    }
    let mut kept: String = text.chars().take(MAX_SUBTITLE_CHARS - 1).collect();
    kept.push('…');
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitles_round_trip_through_their_record() {
        let mut subtitles = Subtitles::default();
        subtitles
            .by_command
            .insert("local:abc#inbox".into(), "3 unread".into());
        let read = Subtitles::read(&subtitles.write()).unwrap();
        assert_eq!(read, subtitles);
        assert_eq!(read.subtitle_of("local:abc#inbox"), Some("3 unread"));
        assert_eq!(read.subtitle_of("local:abc#other"), None);
    }

    #[test]
    fn a_long_subtitle_is_cut() {
        let long = "x".repeat(MAX_SUBTITLE_CHARS + 10);
        let kept = cut(&long);
        assert_eq!(kept.chars().count(), MAX_SUBTITLE_CHARS);
        assert!(kept.ends_with('…'));
        assert_eq!(cut("short"), "short");
    }
}
