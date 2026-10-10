//! Root providers (#164): commands whose `pane.json` entry says `"mode":
//! "provider"`, such as the calculator and Applications. A provider only
//! answers root search, through its `rootResults` or `indexedResults`; it
//! is never launched, so it has no row in root search, no quick slot, no
//! alias, fallback or hotkey, and neither the Actions panel nor the
//! Shortcuts page offers it. Its extension's Settings card lists it with the
//! extension's switch, which turns its results off and on.
//!
//! A command can become a provider after the user recorded such choices
//! for it: the calculator and Applications were view commands with rows
//! before #164, and a package update can change a command's mode. What was
//! recorded for one is forgotten at start, once — the records are written
//! without it — and a toast names what went: aliases, fallbacks and hotkeys
//! when the launcher is created ([`Launcher::forget_provider_choices`]),
//! pins when its quick slots are read (`quick_slots`). What root search
//! learned (#199) is kept: a provider's results are still root results a
//! use is recorded for, while an entry of the command itself, from before
//! it became a provider, ranks nothing (a provider has no row) and decays
//! away as any unused entry does.

use super::aliases::AliasChoices;
use super::choices::Choices;
use super::hotkeys::HotkeyChoices;
use super::{Launcher, State};
use crate::feedback::{Toast, ToastStyle};

/// What a start forgot because its command is a root provider: each
/// command's title, in the order first met, with the kinds of choice
/// forgotten ("pin", "alias", "fallback", "hotkey").
#[derive(Default)]
pub(super) struct Forgotten {
    commands: Vec<(String, Vec<&'static str>)>,
}

impl Forgotten {
    /// Notes that the `what` of the command titled `title` was forgotten.
    fn note(&mut self, title: &str, what: &'static str) {
        match self.commands.iter_mut().find(|(seen, _)| seen == title) {
            Some((_, kinds)) if kinds.contains(&what) => {}
            Some((_, kinds)) => kinds.push(what),
            None => self.commands.push((title.to_owned(), vec![what])),
        }
    }

    /// The toast naming everything forgotten, or `None` if nothing was.
    fn toast(&self) -> Option<Toast> {
        if self.commands.is_empty() {
            return None;
        }
        let titles: Vec<String> = self
            .commands
            .iter()
            .map(|(title, _)| title.clone())
            .collect();
        let (verb, have, own) = if titles.len() == 1 {
            ("answers", "It has", "its")
        } else {
            ("answer", "They have", "their")
        };
        let parts: Vec<String> = self
            .commands
            .iter()
            .map(|(title, kinds)| {
                let mut kinds = kinds.clone();
                kinds.sort_by_key(|kind| ORDER.iter().position(|known| known == kind));
                format!("the {} of {title}", listed(&kinds))
            })
            .collect();
        let mut toast = Toast::new(
            ToastStyle::Success,
            format!("{} now only {verb} root search", listed(&titles)),
        );
        toast.message = Some(format!(
            "{have} no row of {own} own now, so Pane removed {}.",
            listed(&parts)
        ));
        Some(toast)
    }
}

/// The kinds of choice, in the order a toast names them.
const ORDER: [&str; 4] = ["pin", "alias", "fallback", "hotkey"];

/// `items` as a sentence lists them: "a", "a and b", "a, b and c".
fn listed(items: &[impl AsRef<str>]) -> String {
    match items {
        [] => String::new(),
        [only] => only.as_ref().to_owned(),
        [rest @ .., last] => {
            let rest: Vec<&str> = rest.iter().map(AsRef::as_ref).collect();
            format!("{} and {}", rest.join(", "), last.as_ref())
        }
    }
}

/// Every installed root provider's command id with its title, whatever its
/// package's state, in the order of the installed packages.
pub(super) fn providers(state: &State) -> Vec<(String, String)> {
    state
        .packages
        .iter()
        .flat_map(|package| package.providers())
        .map(|command| (command.id, command.title))
        .collect()
}

/// The title of the root provider with command id `id` among `providers`.
pub(super) fn title_of<'a>(providers: &'a [(String, String)], id: &str) -> Option<&'a str> {
    providers
        .iter()
        .find(|(provider, _)| provider == id)
        .map(|(_, title)| title.as_str())
}

impl Launcher {
    /// Forgets the aliases, fallbacks and hotkeys recorded for root
    /// providers and writes their records, saying what went in a toast (see
    /// the module docs). Called once, when the launcher is created, before
    /// any hotkey is registered. A record that cannot be read is left as it
    /// is, and one that cannot be written is tried again at the next start;
    /// meanwhile nothing it holds for a provider takes effect, since nothing
    /// offers a provider.
    pub(super) fn forget_provider_choices(&self) {
        let (aliases, hotkeys) = {
            let mut state = self.lock();
            let providers = providers(&state);
            if providers.is_empty() {
                return;
            }
            let mut forgotten = Vec::new();
            for (id, title) in &providers {
                let choices = &state.aliases.chosen;
                if choices.alias_of(id).is_some() {
                    forgotten.push((title.clone(), "alias"));
                }
                if choices.is_fallback(id) {
                    forgotten.push((title.clone(), "fallback"));
                }
                if state.bindings.hotkey_of(id).is_some() {
                    forgotten.push((title.clone(), "hotkey"));
                }
            }
            let keep = |command: &str| title_of(&providers, command).is_none();
            let aliases = Choices::retain(&mut AliasChoices::of(&mut state).chosen, &keep);
            let hotkeys = Choices::retain(&mut HotkeyChoices::of(&mut state).chosen, &keep);
            for (title, what) in forgotten {
                state.provider_forgotten.note(&title, what);
            }
            (aliases, hotkeys)
        };
        let saves = [
            aliases.then(|| self.save::<AliasChoices>(None)),
            hotkeys.then(|| self.save::<HotkeyChoices>(None)),
        ];
        for problem in saves.into_iter().flatten().filter_map(Result::err) {
            crate::diagnostic!("Pane could not forget what it kept for a root provider: {problem}");
        }
        self.show_provider_toast(&mut self.lock());
    }

    /// Notes that the pin of the root provider titled `title` was forgotten
    /// (see `quick_slots`).
    pub(super) fn note_provider_pin(state: &mut State, title: &str) {
        state.provider_forgotten.note(title, "pin");
    }

    /// Shows the toast naming everything forgotten for root providers so
    /// far, replacing an earlier one, if anything was.
    pub(super) fn show_provider_toast(&self, state: &mut State) {
        if let Some(toast) = state.provider_forgotten.toast() {
            self.show_own_toast(state, toast);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_names_each_command_and_what_it_lost_in_order() {
        let mut forgotten = Forgotten::default();
        assert!(forgotten.toast().is_none());
        forgotten.note("Calculator", "hotkey");
        forgotten.note("Calculator", "pin");
        forgotten.note("Applications", "alias");
        forgotten.note("Calculator", "pin");
        let toast = forgotten.toast().unwrap();
        assert_eq!(toast.style, ToastStyle::Success);
        assert_eq!(
            toast.title,
            "Calculator and Applications now only answer root search"
        );
        assert_eq!(
            toast.message.as_deref(),
            Some(
                "They have no row of their own now, so Pane removed the pin and hotkey of \
                 Calculator and the alias of Applications."
            )
        );
    }

    #[test]
    fn one_command_reads_in_the_singular() {
        let mut forgotten = Forgotten::default();
        forgotten.note("Calculator", "alias");
        forgotten.note("Calculator", "fallback");
        forgotten.note("Calculator", "pin");
        let toast = forgotten.toast().unwrap();
        assert_eq!(toast.title, "Calculator now only answers root search");
        assert_eq!(
            toast.message.as_deref(),
            Some(
                "It has no row of its own now, so Pane removed the pin, alias and fallback of \
                 Calculator."
            )
        );
    }
}
