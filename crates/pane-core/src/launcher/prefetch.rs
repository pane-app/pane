//! The official extensions' revisions, fetched as first setup's list of
//! them opens (#466), so the list shows each extension's own icon — read
//! from its own manifest, as any extension's preview reads one — and Set
//! Up installs what was fetched rather than fetching it again.
//!
//! When the list opens, each pin's repository is fetched at its pinned
//! commit in the background, [`AT_ONCE`] at a time, with the fetch Set Up
//! makes (`defaults::fetch`, into the same downloads folder). As each one
//! lands, its manifest's icon replaces the row's first-letter tile, in
//! place: the rows keep their order, their ticks and the selection. One
//! that fails, or a system with no connection, leaves the letter tile and
//! says nothing; Set Up then fetches it as it always did, and explains a
//! failure as it always did.
//!
//! The list stays usable meanwhile. Set Up takes, for each ticked
//! extension, the revision fetched already; waits for the fetch in flight
//! and takes its answer, revision or failure; and fetches one still
//! waiting for its turn itself, so no repository is fetched twice. A
//! revision fetched for an extension the user unticked is dropped once
//! Set Up declines it, with its download.

use std::path::Path;

use tokio::sync::oneshot;

use super::{DOWNLOADS_DIR, Launcher, State, WeakLauncher};
use crate::defaults::{DefaultExtension, Failed, Fetched};
use crate::icons::Icon;

/// How many of the official extensions' repositories are fetched at once
/// while first setup's list is open: enough that the icons arrive
/// together, few enough not to crowd a slow connection.
const AT_ONCE: usize = 3;

/// The official extensions' revisions first setup's list asked for, by
/// default extension id, in the pins' order.
#[derive(Default)]
pub(in crate::launcher) struct Prefetches {
    slots: Vec<(String, Slot)>,
    /// How many threads are fetching them now.
    workers: usize,
}

/// Where one extension's revision stands.
enum Slot {
    /// Waiting for a thread to fetch it.
    Queued(DefaultExtension),
    /// Being fetched; the acquisition waiting for it, once Set Up reaches
    /// it first.
    Fetching(Option<oneshot::Sender<Result<Fetched, Failed>>>),
    /// Fetched, and held until Set Up installs it: the revision, and the
    /// icon its manifest names, which the list draws.
    Fetched {
        fetched: Fetched,
        icon: Option<Icon>,
    },
}

/// What acquiring a default extension finds of its fetch here.
pub(in crate::launcher) enum Taken {
    /// Its revision, fetched already.
    Fetched(Fetched),
    /// Its fetch is in flight: the answer to wait for.
    Landing(oneshot::Receiver<Result<Fetched, Failed>>),
    /// Nothing: the acquisition fetches it itself.
    Nothing,
}

impl Prefetches {
    fn at(&self, id: &str) -> Option<usize> {
        self.slots.iter().position(|(slot, _)| slot == id)
    }

    /// The icon the fetched revision of the default `id` names, once it
    /// is fetched: what its row on the list draws.
    pub(in crate::launcher) fn icon(&self, id: &str) -> Option<Icon> {
        match self.at(id).map(|at| &self.slots[at].1) {
            Some(Slot::Fetched { icon, .. }) => icon.clone(),
            _ => None,
        }
    }

    /// What acquiring the default `id` finds: its revision, taken; its
    /// fetch in flight, whose answer is now sent to the acquisition; or
    /// nothing — never asked for, failed, or still waiting for its turn,
    /// which is then left to the acquisition.
    pub(in crate::launcher) fn take(&mut self, id: &str) -> Taken {
        let Some(at) = self.at(id) else {
            return Taken::Nothing;
        };
        match &mut self.slots[at].1 {
            Slot::Fetching(waiting @ None) => {
                let (answer, landing) = oneshot::channel();
                *waiting = Some(answer);
                Taken::Landing(landing)
            }
            // One acquisition flow runs at a time (`Acquisitions`), so a
            // second one never waits here; were it to, it fetches itself.
            Slot::Fetching(Some(_)) => Taken::Nothing,
            Slot::Queued(_) | Slot::Fetched { .. } => match self.slots.remove(at).1 {
                Slot::Fetched { fetched, .. } => Taken::Fetched(fetched),
                _ => Taken::Nothing,
            },
        }
    }

    /// Drops what was asked for of the defaults `declined`, which Set Up
    /// declined: a revision fetched (its download removed with it) and one
    /// still waiting for its turn. One in flight is dropped as it lands.
    pub(in crate::launcher) fn decline(&mut self, declined: &[String]) {
        self.slots.retain(|(id, slot)| {
            matches!(slot, Slot::Fetching(_)) || !declined.iter().any(|declined| declined == id)
        });
    }

    /// The next revision waiting for its turn, now in flight.
    fn next(&mut self) -> Option<DefaultExtension> {
        let slot = self
            .slots
            .iter_mut()
            .find(|(_, slot)| matches!(slot, Slot::Queued(_)))?;
        match std::mem::replace(&mut slot.1, Slot::Fetching(None)) {
            Slot::Queued(pin) => Some(pin),
            _ => None,
        }
    }

    fn queued(&self) -> bool {
        self.slots
            .iter()
            .any(|(_, slot)| matches!(slot, Slot::Queued(_)))
    }
}

impl Launcher {
    /// Fetches, in the background, the revision of each of `pins` not
    /// already fetched or asked for (#466): as first setup's list opens,
    /// and again if it opens again, for those that failed meanwhile.
    /// Without an installation, nothing: nothing is set up either.
    pub(in crate::launcher) fn prefetch(&self, state: &mut State, pins: &[DefaultExtension]) {
        let Some(installation) = self.installation.as_ref() else {
            return;
        };
        for pin in pins {
            if state.prefetches.at(&pin.id).is_none() {
                state
                    .prefetches
                    .slots
                    .push((pin.id.clone(), Slot::Queued(pin.clone())));
            }
        }
        let downloads = installation.dir.join(DOWNLOADS_DIR);
        while state.prefetches.workers < AT_ONCE && state.prefetches.queued() {
            let launcher = self.downgrade();
            let downloads = downloads.clone();
            let started = std::thread::Builder::new()
                .name("pane-first-setup-fetch".into())
                .spawn(move || fetch_queued(&launcher, &downloads));
            match started {
                Ok(_) => state.prefetches.workers += 1,
                // No thread: what is left waiting is fetched by Set Up, as
                // it was before the list fetched anything.
                Err(_) => break,
            }
        }
    }

    /// The fetch of `pin`'s revision has landed (`fetched`, its manifest
    /// naming `icon`): sent to the acquisition waiting for it, if one is;
    /// otherwise held for Set Up — unless it failed, which leaves the
    /// letter tile and says nothing, or Set Up declined it. Its row on the
    /// list then draws the icon.
    fn landed(&self, pin: &DefaultExtension, fetched: Result<Fetched, Failed>, icon: Option<Icon>) {
        let mut state = self.lock();
        let waiting = match state.prefetches.at(&pin.id) {
            Some(at) => match state.prefetches.slots.remove(at).1 {
                Slot::Fetching(waiting) => waiting,
                _ => None,
            },
            None => None,
        };
        // Declined by the choice made (Set Up): until it is made, what
        // the store declined before stays, since the list may tick it.
        let declined = state.first_setup.chosen() && state.first_setup.declines(&pin.id);
        match (waiting, fetched) {
            // Set Up reached it first: the acquisition takes the answer,
            // revision or failure, as its own fetch's.
            (Some(waiting), fetched) => {
                let _ = waiting.send(fetched);
            }
            (None, Ok(fetched)) if !declined => {
                let icon = icon.clone();
                state
                    .prefetches
                    .slots
                    .push((pin.id.clone(), Slot::Fetched { fetched, icon }));
            }
            // A failure is said only if Set Up meets it; a declined
            // revision goes, with its download.
            (None, _) => {}
        }
        if let Some(icon) = icon
            && self.show_first_setup_icon(&mut state, &pin.id, icon)
        {
            drop(state);
            self.changed();
        }
    }
}

/// A fetching thread: fetches the revisions waiting for their turn, one
/// after another, until none is left or the launcher is gone.
fn fetch_queued(launcher: &WeakLauncher, downloads: &Path) {
    loop {
        let pin = {
            let Some(launcher) = launcher.upgrade() else {
                return;
            };
            let mut state = launcher.lock();
            match state.prefetches.next() {
                Some(pin) => pin,
                None => {
                    state.prefetches.workers -= 1;
                    return;
                }
            }
        };
        let landed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let fetched = crate::defaults::fetch(&pin, downloads);
            let icon = fetched.as_ref().ok().and_then(|fetched| {
                crate::packages::ListedExtension::at(fetched.download.folder()).icon
            });
            (fetched, icon)
        }));
        let Some(launcher) = launcher.upgrade() else {
            return;
        };
        match landed {
            Ok((fetched, icon)) => launcher.landed(&pin, fetched, icon),
            // A fetch that panicked is forgotten: an acquisition waiting
            // for it hears no answer, and fetches it itself.
            Err(_) => {
                let mut state = launcher.lock();
                if let Some(at) = state.prefetches.at(&pin.id) {
                    state.prefetches.slots.remove(at);
                }
            }
        }
    }
}
