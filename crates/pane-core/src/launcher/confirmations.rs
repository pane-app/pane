//! Confirmations a command asks for before it does something it cannot
//! undo (`feedback.confirm`, #146), and the answers Pane remembers for
//! "Don't ask again".
//!
//! **Asking.** A confirmation is shown over the launcher's current screen,
//! one at a time ([`Launcher::confirmation`]); while the launcher is hidden
//! (a no-view command's hotkey, a command that closed it first), the window
//! shows itself first ([`crate::WindowControl::confirmation`]). The window
//! answers it ([`Launcher::answer_confirmation`]): the primary button
//! (Enter) confirms; the dismiss button (Escape), a click outside it, the
//! window losing the focus or hiding answer that the user did not. A call
//! no window was shown for (a background launch, a schedule, a service, an
//! operation) is refused at once with [`NOT_AVAILABLE`], as is a
//! confirmation asked while another is shown: a refusal is an answer, never
//! a reason to pause. The command's call only awaits the answer, so other
//! calls are served meanwhile; if the call is dropped first (its generation
//! ended, it was cancelled), the confirmation leaves the screen.
//!
//! **Remembering.** A confirmation with a `remember` key, of an installed
//! package, offers "Don't ask again". Answered with a button while it is
//! ticked, the answer (confirmed or not) is Pane's own record per package
//! identity and key (`confirmations.json` beside `installed.json`, see
//! `choices`), and the next confirmation with that key is answered with it
//! at once, showing nothing. The record survives restarts, disabling and
//! updates (the identity stays the same); the package's card in Settings ›
//! Extensions offers "Reset confirmations" (an extension-list row,
//! [`RESET_ROW`]), which forgets them all, and uninstalling forgets them.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use tokio::sync::oneshot;

use super::choices::{Choices, Record, split};
use super::{Entry, Launcher, Row, Screen, State, Status, WeakLauncher, owner};
use crate::feedback::{
    Asking, Caller, ConfirmAnswer, Confirmation, DISMISS, GivenConfirmation, WindowControl,
    WindowPresence,
};
use crate::packages::{InstalledPackage, PackageIdentity};

/// What a confirmation asked in a call no window was shown for answers.
pub(super) const NOT_AVAILABLE: &str = "A confirmation is not available here: no window is shown \
     for this call (a background launch, a schedule, a service or an operation), so Pane asks the \
     user nothing";

/// What a confirmation asked while another one is shown answers.
pub(super) const ANOTHER_SHOWN: &str = "Pane is already asking the user to confirm something \
     else; ask again once that is answered";

/// The kind of the extension-list row that resets a package's remembered
/// answers: its id is `reset-confirmations:<identity key>`.
pub(super) const RESET_ROW: &str = "reset-confirmations";

/// The longest key Pane remembers an answer by, and the longest title,
/// message or label it shows, in characters; longer ones are cut.
const MAX_KEY_CHARS: usize = 200;
const MAX_TEXT_CHARS: usize = 2000;

/// The answers the user told Pane to remember, recorded in
/// `confirmations.json` as `{ "version": 1, "confirmations": { "<identity
/// key>": { "<key>": true } } }`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Confirmations {
    by_package: BTreeMap<String, BTreeMap<String, bool>>,
}

impl Confirmations {
    /// The answer remembered for the package with identity key `package`
    /// under `key`, if one is.
    fn answer(&self, package: &str, key: &str) -> Option<bool> {
        self.by_package.get(package)?.get(key).copied()
    }

    /// Remembers `answer` for `package` under `key`.
    fn remember(&mut self, package: &str, key: &str, answer: bool) {
        self.by_package
            .entry(package.to_owned())
            .or_default()
            .insert(key.to_owned(), answer);
    }

    /// The keys remembered for `package`, in order.
    fn keys_of(&self, package: &str) -> Vec<String> {
        self.by_package
            .get(package)
            .map(|answers| answers.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Forgets every answer of `package`; whether it had any.
    fn forget_package(&mut self, package: &str) -> bool {
        self.by_package
            .remove(package)
            .is_some_and(|answers| !answers.is_empty())
    }
}

impl Choices for Confirmations {
    const FILE: &'static str = "confirmations.json";
    const VERSION: u64 = 1;
    const WHAT: &'static str = "remembered confirmations";

    fn read(fields: &Map<String, Value>) -> Result<Self, String> {
        let mut confirmations = Confirmations::default();
        let Some(by_package) = fields.get("confirmations") else {
            return Ok(confirmations);
        };
        let by_package = by_package
            .as_object()
            .ok_or("`confirmations` is not an object")?;
        for (package, answers) in by_package {
            let Some(answers) = answers.as_object() else {
                continue;
            };
            for (key, answer) in answers {
                if let Some(answer) = answer.as_bool() {
                    confirmations.remember(package, key, answer);
                }
            }
        }
        Ok(confirmations)
    }

    fn write(&self) -> Map<String, Value> {
        let by_package = self
            .by_package
            .iter()
            .filter(|(_, answers)| !answers.is_empty())
            .map(|(package, answers)| {
                let answers = answers
                    .iter()
                    .map(|(key, answer)| (key.clone(), Value::Bool(*answer)))
                    .collect();
                (package.clone(), Value::Object(answers))
            })
            .collect();
        Map::from_iter([("confirmations".to_string(), Value::Object(by_package))])
    }

    /// `command` names a package here: its identity key and a `#`.
    fn restore(&mut self, command: &str, other: &Self) {
        let package = split(command).0;
        match other.by_package.get(package) {
            Some(answers) => self.by_package.insert(package.to_owned(), answers.clone()),
            None => self.by_package.remove(package),
        };
    }

    /// Asks `keep` about each package as `<identity key>#`, which
    /// [`split`] reads back as the key whatever it holds.
    fn retain(&mut self, keep: &dyn Fn(&str) -> bool) -> bool {
        let before = self.by_package.len();
        self.by_package
            .retain(|package, _| keep(&format!("{package}#")));
        self.by_package.len() != before
    }

    fn of(state: &mut State) -> &mut Record<Self> {
        &mut state.confirmations
    }
}

/// The confirmation a command waits on, and what answers it.
pub(super) struct Confirming {
    /// What the window draws.
    shown: Confirmation,
    /// The package identity key and the key the answer is remembered by,
    /// when it offers "Don't ask again".
    remember: Option<(String, String)>,
    /// Whether the window has shown it since it was asked: until it has,
    /// the window losing the focus or hiding (as it did before it showed
    /// itself for it) does not answer it.
    armed: bool,
    /// Tells the command's call the answer.
    answer: oneshot::Sender<bool>,
}

/// The confirmation waiting is shown: from now on the window losing the
/// focus or hiding answers it.
pub(super) fn arm_confirmation(state: &mut State) {
    if let Some(confirming) = state.feedback.confirming.as_mut() {
        confirming.armed = true;
    }
}

/// Answers the confirmation the window showed, if one waits, as not
/// confirmed and without remembering anything: the window lost the focus
/// or hid. Whether one was answered.
pub(super) fn leave_confirmation(state: &mut State) -> bool {
    if !state
        .feedback
        .confirming
        .as_ref()
        .is_some_and(|confirming| confirming.armed)
    {
        return false;
    }
    match state.feedback.confirming.take() {
        Some(confirming) => {
            let _ = confirming.answer.send(false);
            true
        }
        None => false,
    }
}

/// Has the window draw the confirmation as it is now (showing itself first
/// while one is asked and it is hidden).
fn redraw(state: &State) {
    let window: &dyn WindowControl = &*state.feedback.window;
    window.confirmation();
}

/// Takes the confirmation off the screen when the call waiting on it is
/// dropped before it is answered.
struct Waiting {
    launcher: WeakLauncher,
    id: u64,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if let Some(launcher) = self.launcher.upgrade() {
            launcher.withdraw_confirmation(self.id);
        }
    }
}

/// `text` within `limit` characters, cut with an ellipsis.
fn cut(text: String, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text;
    }
    let mut kept: String = text.chars().take(limit - 1).collect();
    kept.push('…');
    kept
}

impl Launcher {
    /// `feedback.confirm` for `caller`: answers at once from a remembered
    /// answer, or refuses (no window for the call, another confirmation
    /// shown); else shows `given`, having the window show itself first if
    /// it is hidden, and answers what the user chooses.
    pub(super) fn ask_to_confirm(&self, caller: &Caller, given: GivenConfirmation) -> Asking {
        let answered =
            |answer: Result<bool, String>| -> Asking { Box::pin(std::future::ready(answer)) };
        if !caller.windowed {
            return answered(Err(NOT_AVAILABLE.into()));
        }
        let (id, receiver) = {
            let mut state = self.lock();
            let state = &mut *state;
            let package =
                owner(&state.packages, &caller.component).map(|package| package.identity.key());
            let key = given
                .remember
                .map(|key| key.trim().to_owned())
                .filter(|key| !key.is_empty())
                .map(|key| cut(key, MAX_KEY_CHARS));
            let remember = package.zip(key);
            if let Some((package, key)) = &remember
                && let Some(answer) = state.confirmations.chosen.answer(package, key)
            {
                return answered(Ok(answer));
            }
            if state.feedback.confirming.is_some() {
                return answered(Err(ANOTHER_SHOWN.into()));
            }
            state.feedback.last_confirmation += 1;
            let id = state.feedback.last_confirmation;
            let dismiss = given
                .dismiss
                .map(|label| label.trim().to_owned())
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| DISMISS.to_owned());
            let (sender, receiver) = oneshot::channel();
            state.feedback.confirming = Some(Confirming {
                shown: Confirmation {
                    id,
                    title: cut(given.title, MAX_TEXT_CHARS),
                    message: given
                        .message
                        .filter(|message| !message.trim().is_empty())
                        .map(|message| cut(message, MAX_TEXT_CHARS)),
                    primary: cut(given.primary, MAX_TEXT_CHARS),
                    destructive: given.destructive,
                    dismiss: cut(dismiss, MAX_TEXT_CHARS),
                    rememberable: remember.is_some(),
                },
                remember,
                // Shown at once where the window is shown; else once it
                // says it is (see `Launcher::set_window_presence`).
                armed: state.feedback.presence != WindowPresence::Hidden,
                answer: sender,
            });
            // Drawn over the current screen, the window shown first if it
            // is hidden.
            redraw(state);
            (id, receiver)
        };
        self.changed();
        let waiting = Waiting {
            launcher: self.downgrade(),
            id,
        };
        Box::pin(async move {
            // A confirmation Pane let go of without an answer (Pane
            // stopping) is not confirmed.
            let confirmed = receiver.await.unwrap_or(false);
            drop(waiting);
            Ok(confirmed)
        })
    }

    /// The confirmation a command waits on, as the window draws it over the
    /// current screen; `None` while none does.
    pub fn confirmation(&self) -> Option<Confirmation> {
        self.lock()
            .feedback
            .confirming
            .as_ref()
            .map(|confirming| confirming.shown.clone())
    }

    /// Answers the confirmation `id` with `answer`, while it still waits.
    /// With `dont_ask_again` ticked (where it offers "Don't ask again") and
    /// a button's answer, the answer is remembered for its package and key,
    /// recorded off the calling thread. A click outside it
    /// ([`ConfirmAnswer::Left`]) is never remembered.
    pub fn answer_confirmation(&self, id: u64, answer: ConfirmAnswer, dont_ask_again: bool) {
        {
            let mut state = self.lock();
            let state = &mut *state;
            if !state
                .feedback
                .confirming
                .as_ref()
                .is_some_and(|confirming| confirming.shown.id == id)
            {
                return;
            }
            let Some(confirming) = state.feedback.confirming.take() else {
                return;
            };
            let confirmed = answer == ConfirmAnswer::Confirmed;
            let remembered = match (&confirming.remember, answer) {
                (Some((package, key)), ConfirmAnswer::Confirmed | ConfirmAnswer::Dismissed)
                    if dont_ask_again =>
                {
                    state.confirmations.chosen.remember(package, key, confirmed);
                    true
                }
                _ => false,
            };
            let _ = confirming.answer.send(confirmed);
            if remembered {
                if matches!(state.view.screen, Screen::Extensions { .. }) {
                    // Its "Reset confirmations" row appears.
                    self.refresh_extensions(state);
                }
                self.record_confirmations(state);
            }
        }
        self.changed();
    }

    /// Takes the confirmation `id` off the screen, if it still waits: the
    /// call that asked was dropped.
    fn withdraw_confirmation(&self, id: u64) {
        {
            let mut state = self.lock();
            let state = &mut *state;
            if !state
                .feedback
                .confirming
                .as_ref()
                .is_some_and(|confirming| confirming.shown.id == id)
            {
                return;
            }
            state.feedback.confirming = None;
            redraw(state);
        }
        self.changed();
    }

    /// The keys of the answers remembered for the installed package with
    /// `identity`, in order.
    pub fn remembered_confirmations(&self, identity: &PackageIdentity) -> Vec<String> {
        self.lock().confirmations.chosen.keys_of(&identity.key())
    }

    /// Forgets the answers remembered for the package with `identity`, so
    /// its commands ask again ("Reset confirmations" on its card); whether
    /// it had any. Recorded off the calling thread.
    pub fn reset_confirmations(&self, identity: &PackageIdentity) -> bool {
        let reset = {
            let mut state = self.lock();
            let state = &mut *state;
            let reset = self.reset_confirmations_in(state, identity);
            if reset && matches!(state.view.screen, Screen::Extensions { .. }) {
                self.refresh_extensions(state);
            }
            reset
        };
        if reset {
            self.changed();
        }
        reset
    }

    /// [`Launcher::reset_confirmations`] with the launcher locked.
    fn reset_confirmations_in(&self, state: &mut State, identity: &PackageIdentity) -> bool {
        if !state.confirmations.chosen.forget_package(&identity.key()) {
            return false;
        }
        self.record_confirmations(state);
        true
    }

    /// The extension list's "Reset confirmations" row activated: forgets
    /// the package's remembered answers and says so.
    pub(super) fn reset_confirmations_row(&self, state: &mut State, identity: &PackageIdentity) {
        let title = state.title_of(identity);
        self.reset_confirmations_in(state, identity);
        if matches!(state.view.screen, Screen::Extensions { .. }) {
            self.refresh_extensions(state);
        }
        state.view.status = Status::Result(format!(
            "{title} asks again before what you told it not to ask about"
        ));
    }

    /// The extension list's "Reset confirmations" rows: one per installed
    /// package with answers remembered.
    pub(super) fn reset_rows(&self, state: &State) -> Vec<(Row, Entry)> {
        state
            .packages
            .iter()
            .filter_map(|package: &InstalledPackage| {
                let key = package.identity.key();
                let remembered = state.confirmations.chosen.keys_of(&key).len();
                (remembered > 0).then(|| {
                    let answers = if remembered == 1 {
                        "1 remembered answer".to_owned()
                    } else {
                        format!("{remembered} remembered answers")
                    };
                    let row = Row {
                        id: format!("{RESET_ROW}:{key}"),
                        title: format!("Reset confirmations of {}", package.title()),
                        subtitle: Some(format!(
                            "Ask again where you chose \"Don't ask again\" ({answers}) · {}",
                            package.identity
                        )),
                        unavailable: None,
                    };
                    (row, Entry::ResetConfirmations(package.identity.clone()))
                })
            })
            .collect()
    }

    /// Writes the remembered answers as they are now, off the calling
    /// thread (the runtime's or the window's).
    fn record_confirmations(&self, state: &State) {
        let launcher = self.clone();
        let saves = state.confirmation_saves.clone();
        let ended = saves.clone();
        saves.begin();
        let saving = std::thread::Builder::new()
            .name("pane-confirmations".into())
            .spawn(move || {
                if let Err(error) = launcher.save::<Confirmations>(None) {
                    eprintln!("Pane could not record the remembered confirmations: {error}");
                }
                ended.end();
            });
        if let Err(error) = saving {
            saves.end();
            eprintln!("Pane could not record the remembered confirmations: {error}");
        }
    }

    /// Waits until every remembered answer so far has been recorded;
    /// `false` if one has not within `limit`. For tests and development
    /// builds, which so wait for the record without timing it.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_confirmations_recorded(&self, limit: std::time::Duration) -> bool {
        let saves = self.lock().confirmation_saves.clone();
        saves.settled(limit)
    }

    /// Forgets the answers remembered for the uninstalled package with
    /// `identity`. Returns what writes the record without them, to run off
    /// the window's thread; nothing to write if it had none.
    pub(super) fn forget_confirmations_of(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) -> Option<impl FnOnce() -> Result<(), String> + Send + 'static> {
        if !state.confirmations.forget(identity) {
            return None;
        }
        let launcher = self.clone();
        Some(move || launcher.save::<Confirmations>(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_answers_round_trip_through_their_record() {
        let mut confirmations = Confirmations::default();
        confirmations.remember("local:abc", "delete-note", true);
        confirmations.remember("local:abc", "empty#trash", false);
        confirmations.remember("npm:other", "delete-note", false);
        let read = Confirmations::read(&confirmations.write()).unwrap();
        assert_eq!(read, confirmations);
        assert_eq!(read.answer("local:abc", "delete-note"), Some(true));
        assert_eq!(read.answer("local:abc", "empty#trash"), Some(false));
        assert_eq!(read.answer("local:abc", "other"), None);
        assert_eq!(read.keys_of("local:abc"), ["delete-note", "empty#trash"]);
    }

    #[test]
    fn a_package_is_forgotten_whole_and_only_it() {
        let mut confirmations = Confirmations::default();
        confirmations.remember("local:a#b", "x", true);
        confirmations.remember("local:a", "y", true);
        // As `Record::forget` asks: by the package's own identity key, even
        // one holding a `#`.
        let gone = confirmations.retain(&|command| split(command).0 != "local:a#b");
        assert!(gone);
        assert_eq!(confirmations.answer("local:a#b", "x"), None);
        assert_eq!(confirmations.answer("local:a", "y"), Some(true));
        assert!(confirmations.forget_package("local:a"));
        assert!(!confirmations.forget_package("local:a"));
    }

    #[test]
    fn an_unusable_answer_is_left_out_when_read() {
        let fields: Map<String, Value> = serde_json::from_str(
            r#"{ "confirmations": { "local:a": { "x": true, "y": "yes" }, "local:b": 3 } }"#,
        )
        .unwrap();
        let read = Confirmations::read(&fields).unwrap();
        assert_eq!(read.keys_of("local:a"), ["x"]);
        assert!(read.keys_of("local:b").is_empty());
    }

    #[test]
    fn long_text_is_cut() {
        assert_eq!(cut("short".into(), 10), "short");
        let kept = cut("x".repeat(20), 10);
        assert_eq!(kept.chars().count(), 10);
        assert!(kept.ends_with('…'));
    }
}
