//! Installed applications' own icons in root search and quick slots (#172,
//! ADR 0038 and ADR 0035's bare application icons).
//!
//! Every root row whose action opens an installed application, whichever
//! extension supplied it, shows the application's own icon, drawn bare
//! (without the tile Pane's own rows keep), from the host's cache of the
//! applications' icons ([`IconCache`]); so does a quick slot pinning one.
//! While the icon is not kept yet, or could not be extracted, the row shows
//! a neutral placeholder of the same size in its place ([`placeholder`]),
//! so nothing moves when the icon arrives. Asking what a row shows never
//! waits: it only looks at what the cache keeps, and wants the icon of a
//! row on screen ahead of the background refresh.
//!
//! The applications listed by the commands' kept results are the ones the
//! cache refreshes after each start (extracting only an icon missing,
//! changed or old); a command disabled or replaced takes its applications
//! out of that refresh with its results.

use std::path::PathBuf;
use std::sync::Arc;

use super::{Entry, Launcher, State};
use crate::applications::icons::{
    Changed, FOLDER, IconCache, IconExtractor, NativeExtractor, SourceOf,
};
use crate::icons::{Color, Icon, IconSource, Tint, Tone};
use crate::runtime::Runtime;

/// The launcher's application icons: the cache, when it keeps one, and
/// what makes another one (a test's, [`Launcher::with_application_icons`]).
pub(super) struct ApplicationIcons {
    pub(super) cache: Option<IconCache>,
    source_of: SourceOf,
    changed: Changed,
}

impl ApplicationIcons {
    /// The icons of the applications `runtime` finds, kept in its cache
    /// folder's [`FOLDER`] and extracted by this system ([`NativeExtractor`]),
    /// telling `changed` when they changed. Without a runtime, or one
    /// keeping no cache, rows show their placeholders.
    pub(super) fn new(runtime: Option<&Runtime>, changed: Changed) -> ApplicationIcons {
        let source_of: SourceOf = match runtime {
            Some(runtime) => {
                // The applications as they are replaced, without keeping the
                // runtime itself.
                let applications = runtime.applications_handle();
                Arc::new(move |id: &str| applications.current().icon_source(id))
            }
            None => Arc::new(|_: &str| None),
        };
        let cache = runtime.and_then(Runtime::cache_folder).map(|folder| {
            IconCache::new(
                folder.join(FOLDER),
                Arc::new(NativeExtractor),
                source_of.clone(),
                changed.clone(),
            )
        });
        ApplicationIcons {
            cache,
            source_of,
            changed,
        }
    }
}

/// What a row shows while an application's icon is not there: the generic
/// application glyph in the secondary tone, without a tile, at the row's
/// icon size.
pub(super) fn placeholder() -> Icon {
    Icon {
        tint: Some(Tint::Same(Color::Tone(Tone::Secondary))),
        ..Icon::new(IconSource::Builtin {
            name: "category".into(),
            filled: false,
        })
    }
}

/// The icon of the installed application `id` as a row shows it now: its
/// own, light and dark, once the cache keeps it, else the placeholder.
/// Decorative: it has no tooltip, so assistive technology reads the row by
/// its title and subtitle only.
pub(super) fn shown(state: &State, id: &str) -> Icon {
    let kept = state
        .application_icons
        .cache
        .as_ref()
        .and_then(|cache| cache.shown(id));
    match kept {
        Some(shown) => Icon {
            // Drawn in its place should its file be gone.
            fallback: Some(Box::new(placeholder())),
            ..Icon::new(IconSource::Image {
                light: shown.light,
                dark: shown.dark,
            })
        },
        None => placeholder(),
    }
}

/// The application icon of the root row with id `row` (`<command
/// id>:<result id>`, as a quick slot names an indexed result), when the
/// kept result with that id opens an installed application: its kept
/// image, or — not there yet or failed — the reference itself with the
/// placeholder standing in for it, the shape the window draws an icon
/// that has not loaded in (its fallback), so the placeholder shows in the
/// icon's place rather than as an icon of its own.
pub(super) fn of_row(state: &State, row: &str) -> Option<Icon> {
    state
        .indexes
        .results()
        .find(|result| result.row.id == row)
        .and_then(|result| match &result.entry {
            Entry::OpenApplication { id, .. } => {
                let shown = shown(state, id);
                Some(if matches!(&shown.source, IconSource::Image { .. }) {
                    shown
                } else {
                    Icon {
                        fallback: Some(Box::new(shown)),
                        ..Icon::new(IconSource::Application(id.clone()))
                    }
                })
            }
            _ => None,
        })
}

/// Tells the cache which applications the commands' kept results list
/// now: their icons are refreshed in the background when missing, changed
/// or old, the others' no more.
pub(super) fn listed(state: &State) {
    let Some(cache) = &state.application_icons.cache else {
        return;
    };
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<String> = state
        .indexes
        .results()
        .filter_map(|result| match &result.entry {
            Entry::OpenApplication { id, .. } => Some(id.clone()),
            _ => None,
        })
        .filter(|id| seen.insert(id.clone()))
        .collect();
    cache.listed(ids);
}

impl Launcher {
    /// This launcher keeping the installed applications' icons in `folder`
    /// and extracting them with `extractor` instead of the system's
    /// ([`NativeExtractor`]): a test's stand-in, or a cache folder of its
    /// own.
    pub fn with_application_icons(
        self,
        folder: PathBuf,
        extractor: Arc<dyn IconExtractor>,
    ) -> Self {
        {
            let mut state = self.lock();
            let icons = &mut state.application_icons;
            let cache = IconCache::new(
                folder,
                extractor,
                icons.source_of.clone(),
                icons.changed.clone(),
            );
            icons.cache = Some(cache.clone());
            state.icon_loads.set_application_icons(Some(cache));
            listed(&state);
        }
        self
    }

    /// The cache of the installed applications' icons this launcher keeps,
    /// if it keeps one: for tests, which wait on it and bound it.
    #[doc(hidden)]
    pub fn application_icons(&self) -> Option<IconCache> {
        self.lock().application_icons.cache.clone()
    }

    /// Waits up to `limit` until no application icon is being extracted;
    /// whether none is. For tests.
    #[doc(hidden)]
    pub fn wait_for_application_icons(&self, limit: std::time::Duration) -> bool {
        match self.application_icons() {
            Some(cache) => cache.wait_idle(limit),
            None => true,
        }
    }
}
