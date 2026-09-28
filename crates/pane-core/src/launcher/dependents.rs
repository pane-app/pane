//! Disabling a package that other installed packages require.
//!
//! When the user disables a package from the extension list and enabled
//! packages require it, directly or through each other (the required
//! dependent closure of `dependencies::required_dependents`, which leaves
//! out optional dependencies and those needed only on other systems),
//! nothing changes yet: a confirmation lists them, with Disable all and
//! Cancel. Cancel (or Back) returns to the list with everything as it was.
//! Disable all disables the package and exactly the dependents shown, in one
//! record, as an ordinary disable of each: their commands leave root search,
//! their instances stop, and their settings and data are kept. If an
//! enabled dependent that was not shown has appeared meanwhile, nothing is
//! disabled and the new set is shown instead.
//!
//! Enabling a package again enables it alone. Pane pausing a package after
//! it failed is not this user action and disables nothing else.

use super::{Change, Entry, Launcher, LauncherView, Question, Row, Screen, State, Status};
use crate::dependencies::{self, Dependent};
use crate::packages::{PackageError, PackageIdentity};
use crate::platform;

/// "Disabled A and B, which requires it", after disabling `title` and the
/// packages titled `dependents` that require it.
pub(super) fn disabled(title: &str, dependents: &[String]) -> String {
    match dependents {
        [dependent] => format!("Disabled {title} and {dependent}, which requires it"),
        _ => format!(
            "Disabled {title} and the {} extensions that require it: {}",
            dependents.len(),
            platform::join(dependents)
        ),
    }
}

impl Launcher {
    /// Asks whether to disable the installed package with `identity`
    /// together with the enabled packages of `closure`, its required
    /// dependents, listing them, and those of them disabled already.
    pub(super) fn show_disable_dependents(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
        closure: Vec<Dependent>,
    ) {
        let title = state.title_of(identity);
        let (to_disable, already): (Vec<Dependent>, Vec<Dependent>) =
            closure.into_iter().partition(|dependent| dependent.enabled);
        let shown: Vec<PackageIdentity> = to_disable
            .iter()
            .map(|dependent| dependent.package.identity.clone())
            .collect();
        let mut details = vec![
            format!("From {identity}"),
            format!(
                "These extensions require {title}, directly or through each other, and cannot \
                 work without it, so they are disabled with it:"
            ),
        ];
        details.extend(to_disable.iter().map(|dependent| {
            format!(
                "{}, which requires {} · {}",
                dependent.package.title, dependent.requires.title, dependent.package.identity
            )
        }));
        details.extend(already.iter().map(|dependent| {
            format!(
                "Already disabled: {}, which requires {}",
                dependent.package.title, dependent.requires.title
            )
        }));
        details.push(format!(
            "Each keeps its settings and saved data. Enabling {title} again does not enable \
             them: enable each in Manage extensions."
        ));
        let with = match to_disable.as_slice() {
            [dependent] => format!("{}, which requires it", dependent.package.title),
            _ => format!("the {} extensions that require it", to_disable.len()),
        };
        let choice = |id: &str, title: String, subtitle: String| Row {
            id: id.into(),
            title,
            subtitle: Some(subtitle),
            unavailable: None,
        };
        let rows = vec![
            choice(
                "disable-all",
                format!("Disable all {}", to_disable.len() + 1),
                format!("Disable {title} and {with}"),
            ),
            choice("cancel", "Cancel".into(), "Keep them all enabled".into()),
        ];
        state.screen_epoch += 1;
        state.entries = vec![Entry::DisableAll(identity.clone(), shown), Entry::Cancel];
        let screen = Screen::Confirm {
            question: Question::DisableDependents(identity.clone()),
            details,
        };
        state.view = LauncherView::new(
            screen,
            format!("Disable {title} and the extensions that require it?"),
        )
        .with_rows(rows);
    }

    /// Disables the package with `identity` and the enabled packages that
    /// require it, if they are among those the confirmation showed
    /// (`shown`), to be recorded by `finish_change`, and shows the
    /// extension list. If the package was disabled meanwhile, disables
    /// nothing; else if one that was not shown requires it now, disables
    /// nothing and asks again.
    pub(super) fn begin_disable_all(
        &self,
        state: &mut State,
        identity: PackageIdentity,
        shown: &[PackageIdentity],
    ) -> Option<Change> {
        let Some(package) = state.package(&identity) else {
            self.show_extensions(state);
            state.view.status = Status::Error(PackageError::NotInstalled(identity).to_string());
            return None;
        };
        let title = package.title();
        if !package.enabled {
            // Disabled meanwhile, which asks nothing more of its dependents,
            // even if a new one appeared.
            self.show_extensions_at(
                state,
                |entry| matches!(entry, Entry::Toggle(asked) if *asked == identity),
            );
            state.view.status = Status::Error(format!("{title} is disabled already"));
            return None;
        }
        let closure = dependencies::required_dependents(&state.packages, &identity);
        let now: Vec<PackageIdentity> = closure
            .iter()
            .filter(|dependent| dependent.enabled)
            .map(|dependent| dependent.package.identity.clone())
            .collect();
        if now.iter().any(|dependent| !shown.contains(dependent)) {
            self.show_disable_dependents(state, &identity, closure);
            state.view.status = Status::Error(format!(
                "What disabling {title} affects changed since it was shown; check it again and \
                 choose Disable all once more"
            ));
            return None;
        }
        self.show_extensions_at(
            state,
            |entry| matches!(entry, Entry::Toggle(asked) if *asked == identity),
        );
        let identities = std::iter::once(identity).chain(now).collect();
        self.begin_change(state, identities, false)
    }
}
