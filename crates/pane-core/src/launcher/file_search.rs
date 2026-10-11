//! File search's place in the launcher (#126, #175): the file index runs
//! while an enabled, unpaused package declares `"fileIndex": true`, its
//! first walk waits for the launcher to be shown, and uninstalling the last
//! package that uses it deletes it. In root search a package's file rows
//! (named by the ids the index gave them, titled with the entry's own name
//! and its folder) come after what is found by title, at most
//! [`ROOT_FILE_ROWS`] of them under "Files", followed by a row opening the
//! command that searches them all with the query typed. Each row shows the
//! system's icon for its path (#142) and reads File, or Folder.
//!
//! The rows' actions are Pane's own (see `own_actions`): Enter on a program
//! the index found shows it in the file manager, never runs it.
//!
//! The File search page in Settings (#176) reads and changes the index
//! here: its status and problems ([`Launcher::file_index_status`],
//! [`Launcher::file_search_problems`]), its rules
//! ([`Launcher::set_file_search_rules`]), a folder taken out for churn put
//! back ([`Launcher::include_in_file_search`]), and Rebuild index. A folder
//! granted to a package that now uses the index (Files under #29) is moved
//! into the roots where the index would not cover it otherwise, and the
//! grant forgotten (#126 story 60).

use std::collections::BTreeSet;
use std::future::Future;
use std::path::{Path, PathBuf};

use super::{CommandRegistration, Entry, Launcher, Opening, Row, State, off_thread};
use crate::file_index::{
    Admitted, IndexStatus, Indexer, IndexerConfig, Problem, Scope, ScopeRules, UserRules,
};
use crate::files::{FileAccess, canonical};
use crate::icons::{Icon, IconSource};
use crate::launch::LaunchSource;
use crate::packages::InstalledPackage;

/// The most file rows of the file index one command lists in root search
/// (#126's proposed default).
pub(super) const ROOT_FILE_ROWS: usize = 5;

/// Whether `package` uses the file index (`"fileIndex": true`).
pub(super) fn uses_file_index(package: &InstalledPackage) -> bool {
    package
        .manifest
        .as_ref()
        .is_ok_and(|manifest| manifest.file_index)
}

/// Whether the user left `package` a command on (#168): a package whose
/// every command is turned off in Settings uses nothing, the index
/// included. A package without commands is judged by its own switch alone.
fn has_a_command_on(package: &InstalledPackage) -> bool {
    let commands = package.listed_commands();
    commands.is_empty() || commands.iter().any(|command| command.enabled)
}

/// Whether the index under `rules` leaves out `folder` (canonical, as a
/// grant is recorded): it is under no root, or the rules exclude it or a
/// folder above it.
fn index_leaves_out(rules: &ScopeRules, folder: &Path) -> bool {
    let scope = Scope::new(rules.clone());
    for root in &rules.roots {
        let Ok(resolved) = canonical(root) else {
            continue;
        };
        if let Ok(below) = folder.strip_prefix(&resolved) {
            // Named as the root names it, as the index does.
            let path = root.join(below);
            return !scope.admits(&path, true, &mut Admitted::default());
        }
    }
    true
}

/// The folders granted to `owners` (packages that use the index) under #29
/// that the index under `base` with `rules` over it would not cover, added
/// to `rules`' roots; and the owners whose grant is to be forgotten once
/// the rules are recorded. `true` if the rules changed.
fn move_grants(
    files: &FileAccess,
    owners: &[String],
    base: &ScopeRules,
    rules: &mut UserRules,
) -> (bool, Vec<String>) {
    let mut changed = false;
    let mut forget = Vec::new();
    for owner in owners {
        let Some(folder) = files.granted(owner) else {
            continue;
        };
        if index_leaves_out(&rules.applied_to(base), &folder)
            && !rules.added_roots.contains(&folder)
        {
            rules.added_roots.push(folder);
            changed = true;
        }
        forget.push(owner.clone());
    }
    (changed, forget)
}

/// The row after `command`'s file rows that opens it with `query` typed
/// into its own search field ("Search Files for “plan”"), if it searches.
pub(super) fn search_all_row(command: &CommandRegistration, query: &str) -> Option<(Row, Entry)> {
    let text = query.trim();
    if !command.search || text.is_empty() {
        return None;
    }
    let mut opening = Opening::of(command, false, LaunchSource::RootSearch);
    opening.initial_search = Some(text.to_owned());
    let row = Row {
        id: format!("{}:search-all-files", command.id),
        title: format!("{} for “{text}”", command.title),
        subtitle: Some("Searches every file Pane indexed".into()),
        unavailable: None,
    };
    Some((row, Entry::Open(opening)))
}

/// The icon of the file index's entry at `path`: the system's icon for it
/// (#142), a document's or a folder's outline until it is loaded.
pub(super) fn entry_icon(path: &std::path::Path, folder: bool) -> Icon {
    let stand_in = if folder { "folder" } else { "document" };
    Icon {
        fallback: Some(Box::new(Icon::new(IconSource::Builtin {
            name: stand_in.into(),
            filled: false,
        }))),
        ..Icon::new(IconSource::File(path.to_path_buf()))
    }
}

/// Starts loading the system icons of the file rows among `entries`.
pub(super) fn want_icons<'a>(state: &State, entries: impl IntoIterator<Item = &'a Entry>) {
    for entry in entries {
        if let Entry::File(file) = entry
            && let Some(path) = &file.path
        {
            state.icon_loads.want(None, &entry_icon(path, file.folder));
        }
    }
}

impl Launcher {
    /// Tells the file index which packages use it and may run now: it is
    /// kept current while there is one, and stops watching at once when
    /// there is none.
    ///
    /// A package uses it while it is enabled, not paused, and has a command
    /// the user left on: turning off Files' Search Files command in Settings
    /// stops the index as disabling Files does.
    pub(super) fn sync_file_index(&self, state: &State) {
        let Some(files) = &state.files else {
            return;
        };
        let users: BTreeSet<String> = state
            .packages
            .iter()
            .filter(|package| {
                state.runs(package) && uses_file_index(package) && has_a_command_on(package)
            })
            .map(|package| package.identity.key())
            .collect();
        files.indexer().set_users(users);
        self.move_grants_later(state, files);
    }

    /// Moves the folder granted to a package that has come to use the index
    /// (an update of Files while Pane runs) into the roots, off the calling
    /// thread, as [`Launcher::with_file_index`] does at start.
    fn move_grants_later(&self, state: &State, files: &FileAccess) {
        let indexer = files.indexer();
        if !indexer.configured() {
            return;
        }
        let owners: Vec<String> = state
            .packages
            .iter()
            .filter(|package| uses_file_index(package))
            .map(|package| package.identity.key())
            .filter(|owner| files.granted(owner).is_some())
            .collect();
        if owners.is_empty() || !indexer.start_moving_grants() {
            return;
        }
        let files = files.clone();
        let moving = indexer.clone();
        let spawned = std::thread::Builder::new()
            .name("pane-file-search-grants".into())
            .spawn(move || {
                if let Some(base) = moving.base_rules() {
                    let mut rules = moving.user_rules();
                    let (changed, forget) = move_grants(&files, &owners, &base, &mut rules);
                    let recorded = !changed || moving.change_rules(rules).is_ok();
                    if recorded {
                        for owner in forget {
                            let _ = files.revoke(&owner);
                        }
                    }
                }
                moving.done_moving_grants();
            });
        if spawned.is_err() {
            indexer.done_moving_grants();
        }
    }

    /// This launcher keeping a file index as `config` says (in Pane's cache
    /// folder, of the home folder), with the user's rules recorded beside
    /// the installed packages. Without it, file search finds nothing.
    ///
    /// A folder granted under #29 to a package that uses the index (Files)
    /// is added to the roots here if the index would not cover it otherwise
    /// (it is outside the home folder, or excluded), and the grant is
    /// forgotten: nothing the user could find before is lost.
    pub fn with_file_index(self, config: IndexerConfig) -> Self {
        let files = self.lock().files.clone();
        let home = config.rules.home.clone();
        if let Some(files) = files {
            let dir = self
                .installation
                .as_ref()
                .map(|installation| installation.dir.clone());
            let mut rules = dir.as_deref().map(UserRules::read).unwrap_or_default();
            let owners: Vec<String> = self
                .lock()
                .packages
                .iter()
                .filter(|package| uses_file_index(package))
                .map(|package| package.identity.key())
                .collect();
            let (changed, forget) = move_grants(&files, &owners, &config.rules, &mut rules);
            let recorded = match (&dir, changed) {
                (Some(dir), true) => rules.write(dir).is_ok(),
                _ => true,
            };
            if recorded {
                for owner in forget {
                    let _ = files.revoke(&owner);
                }
            }
            if let Some(dir) = dir {
                files.indexer().keep_rules_in(dir);
            }
            files.indexer().configure(config, rules);
        }
        // The index's home folder is the one `~` in a typed path resolves
        // to (#195): the same folder the index covers.
        self.lock().home = home;
        self.sync_file_index(&self.lock());
        self
    }

    /// The file index, for the File search page (#176) and Search Files
    /// (#177); `None` without an extension runtime.
    pub fn file_indexer(&self) -> Option<Indexer> {
        self.lock().files.as_ref().map(|files| files.indexer())
    }

    /// Where the file index is now.
    pub fn file_index_status(&self) -> IndexStatus {
        self.file_indexer()
            .map(|indexer| indexer.status())
            .unwrap_or_default()
    }

    /// The roots and rules in force (the defaults with the user's over
    /// them), and the user's own rules; `None` when this Pane keeps no file
    /// index.
    pub fn file_search_rules(&self) -> Option<(ScopeRules, UserRules)> {
        let indexer = self.file_indexer()?;
        Some((indexer.rules()?, indexer.user_rules()))
    }

    /// Records the user's file search `rules` in Pane's own record and
    /// applies them without a restart, off the calling thread: only what
    /// they change is indexed again where it can be (see
    /// [`Indexer::set_user_rules`]). The folders taken out for churn stay
    /// out; [`Launcher::include_in_file_search`] puts one back.
    pub fn set_file_search_rules(
        &self,
        rules: UserRules,
    ) -> impl Future<Output = Result<(), String>> + Send + 'static {
        let indexer = self.file_indexer();
        async move {
            let indexer =
                indexer.ok_or_else(|| String::from("Pane's extension runtime is unavailable"))?;
            off_thread(move || indexer.change_rules(rules)).await
        }
    }

    /// Puts `folder`, which the index took out because it changed
    /// constantly, back in (the File search page's Include Again), off the
    /// calling thread.
    pub fn include_in_file_search(
        &self,
        folder: PathBuf,
    ) -> impl Future<Output = Result<(), String>> + Send + 'static {
        let indexer = self.file_indexer();
        async move {
            let indexer =
                indexer.ok_or_else(|| String::from("Pane's extension runtime is unavailable"))?;
            off_thread(move || indexer.include_again(&folder)).await
        }
    }

    /// What the File search page lists about the index: folders that could
    /// not be read, that macOS refused, that are not watched, that churned
    /// or did not answer, a walk stopped at the ceiling and a stop for
    /// space, each with why and what to do (see [`Indexer::problems`]).
    pub fn file_search_problems(&self) -> Vec<Problem> {
        self.file_indexer()
            .map(|indexer| indexer.problems())
            .unwrap_or_default()
    }

    /// The installed packages that declare `"fileIndex": true`, each by its
    /// title with why it does not use the index now, if it does not ("it is
    /// turned off", "it is paused", "its commands are turned off"): what
    /// the File search page says while file search is off.
    pub fn file_search_packages(&self) -> Vec<(String, Option<String>)> {
        let state = self.lock();
        state
            .packages
            .iter()
            .filter(|package| uses_file_index(package))
            .map(|package| {
                let off = if !package.enabled {
                    Some("it is turned off".to_owned())
                } else if state.paused.is_paused(&package.identity) {
                    Some("it is paused".to_owned())
                } else if !has_a_command_on(package) {
                    Some("its commands are turned off".to_owned())
                } else {
                    None
                };
                (package.title(), off)
            })
            .collect()
    }

    /// Deletes the file index and builds it again (the File search page's
    /// Rebuild index), off the calling thread.
    pub fn rebuild_file_index(&self) -> impl Future<Output = Result<(), String>> + Send + 'static {
        let indexer = self.file_indexer();
        async move {
            let indexer =
                indexer.ok_or_else(|| String::from("Pane's extension runtime is unavailable"))?;
            off_thread(move || indexer.rebuild()).await
        }
    }

    /// Waits until the file index has settled: caught up, walked and every
    /// change reported so far applied (or it is off or stopped); `false`
    /// if it did not within `limit`. For tests and development builds,
    /// which so wait for it without timing it.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_file_index(&self, limit: std::time::Duration) -> bool {
        self.file_indexer()
            .is_none_or(|indexer| indexer.wait_until_settled(limit))
    }

    /// Deletes the file index once no installed package uses it (the last
    /// one was uninstalled). Blocking work runs off the calling thread.
    pub(super) async fn forget_file_index_if_unused(&self) -> Result<(), String> {
        let indexer = {
            let state = self.lock();
            if state.packages.iter().any(uses_file_index) {
                return Ok(());
            }
            state.files.as_ref().map(|files| files.indexer())
        };
        match indexer {
            Some(indexer) => off_thread(move || indexer.delete()).await,
            None => Ok(()),
        }
    }
}
