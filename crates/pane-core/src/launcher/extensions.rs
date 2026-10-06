//! The extension list ("Manage extensions…"): its rows, from each
//! installed package's state, reload, update, cache and uninstall rows to
//! the hotkeys, choices, development and retained data beneath them, and
//! the lines of information above them.

use super::pausing::{self, Pauses};
use super::{
    Entry, Launcher, LauncherView, Row, Screen, State, first_index, network, programs, retained,
    updates,
};
use crate::packages::{InstalledPackage, PackageIdentity};

impl Launcher {
    /// The extension list, as "Manage extensions…" shows it: its rows and
    /// its lines of information, read without leaving the screen the
    /// launcher is on. The rows are the ones [`Launcher::manage_extensions`]
    /// shows once the list is entered; a second window over the launcher
    /// (Pane's Settings) lists the installed extensions through this before
    /// the user opens the flow itself, so it stays a reading of the
    /// launcher's own records rather than a copy of them.
    pub fn extension_list(&self) -> LauncherView {
        let state = self.lock();
        let (rows, _) = self.extension_rows(&state);
        let details = extension_details(&state);
        LauncherView::new(Screen::Extensions { details }, "Extensions").with_rows(rows)
    }

    /// Shows the installed packages, each enabled or disabled.
    pub(in crate::launcher) fn show_extensions(&self, state: &mut State) {
        let (rows, entries) = self.extension_rows(state);
        self.leave_command(state);
        state.entries = entries;
        let details = extension_details(state);
        state.view =
            LauncherView::new(Screen::Extensions { details }, "Extensions").with_rows(rows);
        self.show_kept_development_status(state);
    }

    /// The extension list's rows: each package's state, reload and cache
    /// rows, then the hotkey of each command of the enabled packages, then
    /// one row per identity with retained data, then the global
    /// automatic-update choice, last of all.
    fn extension_rows(&self, state: &State) -> (Vec<Row>, Vec<Entry>) {
        let developed = |identity: &PackageIdentity| self.is_developed(identity);
        let (mut rows, mut entries): (Vec<Row>, Vec<Entry>) =
            self.runtime_rows().into_iter().unzip();
        let (package_rows, package_entries) = extension_rows(
            &state.packages,
            &state.paused,
            developed,
            &state.update_controls.off,
        );
        rows.extend(package_rows);
        entries.extend(package_entries);
        for (row, entry) in self
            .network_rows(state)
            .into_iter()
            .chain(self.program_rows(state))
        {
            rows.push(row);
            entries.push(entry);
        }
        let development = self.development_rows(&state.packages);
        for (row, entry) in self
            .hotkey_rows(state)
            .into_iter()
            .chain(self.choice_rows(state))
            .chain(development)
        {
            rows.push(row);
            entries.push(entry);
        }
        if let Some(installation) = &self.installation {
            let (retained_rows, retained_entries) =
                retained::rows(&state.retained, &installation.data);
            rows.extend(retained_rows);
            entries.extend(retained_entries);
            // The global automatic-update choice comes last, after every
            // package's rows: the packages are the list, and what governs
            // them all is found beneath them.
            let (row, entry) = updates::global_row(state.update_controls.automatic);
            rows.push(row);
            entries.push(entry);
        }
        (rows, entries)
    }

    /// Shows the extension list with the first row whose entry is `wanted`
    /// selected, or the first row if there is none.
    pub(in crate::launcher) fn show_extensions_at(
        &self,
        state: &mut State,
        wanted: impl Fn(&Entry) -> bool,
    ) {
        self.show_extensions(state);
        let row = state.entries.iter().position(wanted);
        if row.is_some() {
            state.view.selected = row;
        }
    }

    /// Updates the installed packages on screen after one changed, keeping
    /// the selection on the same row.
    pub(in crate::launcher) fn refresh_extensions(&self, state: &mut State) {
        let (rows, entries) = self.extension_rows(state);
        // The same row stays selected; if it is gone (a Retry row once the
        // package started), the row before it.
        let selected = state.view.selected.and_then(|index| {
            let id = &state.view.rows.get(index)?.id;
            rows.iter()
                .position(|row| row.id == *id)
                .or_else(|| Some(index.saturating_sub(1).min(rows.len().checked_sub(1)?)))
        });
        state.entries = entries;
        state.view.selected = selected.or_else(|| first_index(&rows));
        state.view.rows = rows;
    }
}

/// The extension list's lines of information: what each kind of action
/// there does to an extension's data, and what a pause or retained data
/// is. Read for the list itself and for
/// [`Launcher::extension_list`], which shows the same lines without
/// entering the list.
fn extension_details(state: &State) -> Vec<String> {
    let mut details = vec![
        "A disabled extension adds no commands and runs nothing; it keeps its settings.".into(),
        "Reloading replaces an extension's code with its source folder's current build; it \
         keeps its settings."
            .into(),
        "Clearing an extension's cache keeps its settings, content and credentials.".into(),
        "Uninstalling an extension asks whether to keep its settings and content.".into(),
        "An automatic update replaces an extension's copy with a compatible newer version \
         of it, from npm, once no command of it is running; a pinned version never moves."
            .into(),
        format!(
            "An extension that cannot start, or crashes or stops responding {}, is paused \
             until you retry it; it keeps its settings.",
            pausing::within()
        ),
    ];
    if !state.retained.is_empty() {
        details.push(
            "Data kept for an uninstalled extension is listed until you delete it or install \
             it again from the same source."
                .into(),
        );
    }
    details
}

/// One row per installed package, saying whether it is enabled or paused
/// and which source it is, so copies with the same title can be told apart;
/// then the rows that reload each enabled package, each followed, if Pane
/// paused it, by a row that retries it and one that shows why it is paused;
/// then one row per package to clear its cache, and one to uninstall it, in
/// the same order.
fn extension_rows(
    packages: &[InstalledPackage],
    paused: &Pauses,
    developed: impl Fn(&PackageIdentity) -> bool,
    off: &std::collections::HashSet<String>,
) -> (Vec<Row>, Vec<Entry>) {
    let failure = |package: &InstalledPackage| paused.of(&package.identity).cloned();
    let toggles = packages.iter().map(|package| {
        let state = match (package.enabled, failure(package).map(|pause| pause.after)) {
            (false, _) => "Disabled",
            (true, None) => "Enabled",
            (true, Some(cause)) => cause.state(),
        };
        let developing = if developed(&package.identity) {
            " · Developing"
        } else {
            ""
        };
        let row = Row {
            id: package.identity.key(),
            title: package.title(),
            subtitle: Some(format!(
                "{state}{developing}{network}{programs} · {}",
                package.identity,
                network = if package.uses_network {
                    format!(" · {}", network::USES_THE_NETWORK)
                } else {
                    String::new()
                },
                programs = if package.uses_programs {
                    format!(" · {}", programs::RUNS_SYSTEM_PROGRAMS)
                } else {
                    String::new()
                }
            )),
            unavailable: None,
        };
        (row, Entry::Toggle(package.identity.clone()))
    });
    let reloads = packages
        .iter()
        .filter(|package| package.enabled)
        .flat_map(|package| {
            let title = package.title();
            let source = match package.identity.local_folder() {
                Some(folder) => folder.display().to_string(),
                None => package.identity.to_string(),
            };
            let reload = Row {
                id: format!("reload:{}", package.identity.key()),
                title: format!("Reload {title}"),
                subtitle: Some(format!(
                    "Replace its code with the current build in {source}"
                )),
                unavailable: None,
            };
            let paused = failure(package).into_iter().flat_map(move |pause| {
                let retry = Row {
                    id: format!("retry:{}", package.identity.key()),
                    title: pause.after.retry_title(&title),
                    subtitle: Some(format!(
                        "Paused: {}; start it again",
                        pause.after.failure("it")
                    )),
                    unavailable: None,
                };
                let details = Row {
                    id: format!("paused:{}", package.identity.key()),
                    title: pausing::details_title(&title),
                    subtitle: Some("The error and its diagnostics".into()),
                    unavailable: None,
                };
                [
                    (retry, Entry::Retry(package.identity.clone())),
                    (details, Entry::PauseDetails(package.identity.clone())),
                ]
            });
            // A package from npm or Git has no source folder to reload from;
            // to replace its code, install it again (Update).
            let local = package.identity.local_folder().is_some();
            std::iter::once((reload, Entry::Reload(package.identity.clone())))
                .filter(move |_| local)
                .chain(paused)
        });
    // Which packages the user turned updates off for, for their rows.
    let automatic = updates::package_rows(packages, off);
    let clear_cache = packages.iter().map(|package| {
        let row = Row {
            id: format!("clear-cache:{}", package.identity.key()),
            title: format!("Clear cache of {}", package.title()),
            subtitle: Some(format!(
                "Keeps its settings, content and credentials · {}",
                package.identity
            )),
            unavailable: None,
        };
        (row, Entry::AskClearCache(package.identity.clone()))
    });
    let uninstall = packages.iter().map(|package| {
        let row = Row {
            id: format!("uninstall:{}", package.identity.key()),
            title: format!("Uninstall {}", package.title()),
            subtitle: Some(format!(
                "Remove it and choose whether to keep its saved data · {}",
                package.identity
            )),
            unavailable: None,
        };
        (row, Entry::AskUninstall(package.identity.clone()))
    });
    toggles
        .chain(reloads)
        .chain(automatic)
        .chain(clear_cache)
        .chain(uninstall)
        .unzip()
}
