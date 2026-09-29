//! Generations: who owns a guest call, and so when it stops.
//!
//! A [`Generation`] is one run of an installed package's code, from when the
//! package is enabled (or installed, or Pane starts) until it is disabled,
//! reloaded, updated or paused after it failed. Every call into the package, and every guest
//! instance serving one, belongs to the generation that was current when the
//! user or another extension asked for it; so does an operation call it
//! serves for another package's call, which also belongs to its caller's
//! generation through the call chain.
//!
//! When a generation ends, the runtime stops its work at once: a call not
//! started yet is not started; a call waiting inside the guest (on a timer,
//! an operation, any async import) is abandoned and its instance, with
//! everything the store holds (views, streams, futures, host tasks), is
//! dropped; a result that completes anyway is discarded; and host imports
//! refuse the stopped code (saving data, calling operations). Since #18 a
//! guest computing without awaiting yields at each epoch tick, so it is
//! stopped there too.
//!
//! Code of a runtime thread Pane gave up on (a runtime hang, #18) is stopped
//! the same way, through that thread's [`Fence`]: its generation has not
//! ended, but nothing it still runs may change anything.

use std::sync::{Arc, RwLock, RwLockReadGuard};

use tokio::sync::watch;

/// Why a generation ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// The user disabled the package.
    Disabled,
    /// The package's code was replaced by a reload or an update; the new
    /// code runs in a new generation.
    Replaced,
    /// The user uninstalled the package.
    Uninstalled,
    /// Pane paused the package after it failed: it could not start, or it
    /// crashed too often. Retry, a reload or an update runs
    /// it in a new generation.
    Paused,
    /// Not an end of the generation: the runtime thread running this code
    /// stopped responding and Pane gave up on it (a runtime hang). A fresh
    /// thread runs the package's next calls; this code changes nothing more.
    Abandoned,
}

/// Closed once Pane gives up on a runtime thread: code that thread runs is
/// then stopped, as if its generation had ended ([`End::Abandoned`]).
/// Cloning shares it.
///
/// A host call that changes something checks it and makes its change while
/// holding it ([`Fence::hold`]); closing waits for such a change to finish,
/// so none lands after the thread was given up on. What is held is never
/// blocking work, so closing never waits long.
#[derive(Clone, Debug, Default)]
pub(crate) struct Fence(Arc<RwLock<bool>>);

impl Fence {
    /// Whether the fence was closed.
    pub fn closed(&self) -> bool {
        *self.hold()
    }

    /// Holds the fence open, if it is, until the guard is dropped: a change
    /// made meanwhile lands before any close. The guard says whether it was
    /// closed already.
    pub fn hold(&self) -> RwLockReadGuard<'_, bool> {
        self.0
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Closes the fence, once no change held it open.
    pub fn close(&self) {
        *self
            .0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
    }
}

/// One run of an installed package's code. Cloning shares it.
#[derive(Clone, Debug)]
pub(crate) struct Generation(Arc<watch::Sender<Option<End>>>);

impl Generation {
    /// A generation that has not ended.
    pub fn new() -> Generation {
        Generation(Arc::new(watch::channel(None).0))
    }

    /// Ends the generation for `why`, waking the work waiting on it. A
    /// generation ends once; ending it again keeps the first reason.
    pub fn end(&self, why: End) {
        self.0.send_if_modified(|end| {
            let first = end.is_none();
            if first {
                *end = Some(why);
            }
            first
        });
    }

    /// Why it ended, if it has.
    pub fn ended(&self) -> Option<End> {
        *self.0.borrow()
    }

    /// Resolves when the generation ends, with why.
    pub fn wait_end(&self) -> impl Future<Output = End> + Send + 'static {
        let mut ended = self.0.subscribe();
        async move {
            let end = ended.wait_for(Option::is_some).await.map(|end| *end);
            match end {
                Ok(end) => end.expect("waited for an end"),
                // The sender lives as long as this generation's clones, one
                // of which made this future, so it cannot close first.
                Err(_) => std::future::pending().await,
            }
        }
    }
}
