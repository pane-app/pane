//! The system's applications as the runtime's guests and the launcher see
//! them, with who asked for them: the host keeps its live list only while
//! a package that asked for it can run (ADR 0038), and the launcher asks
//! the commands that asked for results again when the list changes.
//!
//! A guest's `installed()` marks its component an asker, on its package's
//! generation ([`EndMark`]). Once every asker's generation ended (the
//! package was disabled, uninstalled, paused or its code replaced), the
//! applications are released ([`Applications::release`]): the list is
//! dropped and its watchers stop. A command built into Pane has no
//! generation, and its asking never ends.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use super::lock;
use crate::applications::Applications;
use crate::generation::{EndMark, Generation};

/// What the launcher is told when the installed applications changed: the
/// components that asked for them, whose code may still run.
pub(crate) type Changed = Arc<dyn Fn(Vec<PathBuf>) + Send + Sync>;

/// The system's applications, replaceable for tests, with their askers.
#[derive(Clone)]
pub(crate) struct SharedApplications(Arc<Inner>);

struct Inner {
    current: Mutex<Arc<dyn Applications>>,
    askers: Mutex<Vec<Asker>>,
    changed: Mutex<Option<Changed>>,
}

/// A component that asked for the installed applications.
struct Asker {
    component: PathBuf,
    /// Ended with the generation it asked in.
    mark: EndMark,
}

impl SharedApplications {
    pub fn new(applications: Arc<dyn Applications>) -> SharedApplications {
        let shared = SharedApplications(Arc::new(Inner {
            current: Mutex::new(applications.clone()),
            askers: Mutex::default(),
            changed: Mutex::default(),
        }));
        shared.follow(&applications);
        shared
    }

    /// The applications guests and the launcher use now.
    pub fn current(&self) -> Arc<dyn Applications> {
        lock(&self.0.current).clone()
    }

    /// Uses `applications` from now on.
    pub fn replace(&self, applications: Arc<dyn Applications>) {
        self.follow(&applications);
        *lock(&self.0.current) = applications;
    }

    /// Has `changed` told of each change of the installed applications.
    pub fn on_change(&self, changed: Changed) {
        *lock(&self.0.changed) = Some(changed);
    }

    /// The components that asked for the installed applications and may
    /// still run: what they supply ahead of the query may have changed
    /// when the applications did, so they are asked for again then (see
    /// the launcher's `application_changes`) — and no sooner, not for a
    /// show of root search that changed nothing.
    pub fn askers(&self) -> Vec<PathBuf> {
        lock(&self.0.askers)
            .iter()
            .filter(|asker| !asker.mark.ended())
            .map(|asker| asker.component.clone())
            .collect()
    }

    /// Notes that `component`, running in `generation` (none for a command
    /// built into Pane), asked for the installed applications.
    pub fn asked_by(&self, component: &Path, generation: Option<&Generation>) {
        let asking = |asker: &Asker| asker.component == component && !asker.mark.ended();
        if lock(&self.0.askers).iter().any(asking) {
            return;
        }
        // Marked with nothing locked: a generation that already ended runs
        // the mark's end at once.
        let weak = Arc::downgrade(&self.0);
        let mark = EndMark::on(
            generation,
            "asking for the installed applications",
            move || {
                release_if_unused(&weak);
            },
        );
        if mark.ended() {
            return;
        }
        lock(&self.0.askers).push(Asker {
            component: component.to_path_buf(),
            mark,
        });
    }

    /// Has `applications` tell this of its changes.
    fn follow(&self, applications: &Arc<dyn Applications>) {
        let weak = Arc::downgrade(&self.0);
        applications.on_change(Arc::new(move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let components: Vec<PathBuf> = lock(&inner.askers)
                .iter()
                .filter(|asker| !asker.mark.ended())
                .map(|asker| asker.component.clone())
                .collect();
            let changed = lock(&inner.changed).clone();
            if let Some(changed) = changed.filter(|_| !components.is_empty()) {
                changed(components);
            }
        }));
    }
}

/// Releases the applications once no asker's code can run any more. Runs
/// on whichever thread ended a generation, so it does not block.
fn release_if_unused(inner: &Weak<Inner>) {
    let Some(inner) = inner.upgrade() else {
        return;
    };
    let ended: Vec<Asker> = {
        let mut askers = lock(&inner.askers);
        if askers.iter().any(|asker| !asker.mark.ended()) {
            return;
        }
        std::mem::take(&mut *askers)
    };
    // Dropped with nothing locked: each takes its mark off its list.
    drop(ended);
    let applications = lock(&inner.current).clone();
    applications.release();
}
