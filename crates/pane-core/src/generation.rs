//! Generations: who owns a guest call, and so when it stops.
//!
//! A [`Generation`] is one run of an installed package's code, from when the
//! package is enabled (or installed, or Pane starts) until it is disabled,
//! reloaded or updated. Every call into the package, and every guest
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
//! refuse the stopped code (saving data, calling operations). What it cannot
//! stop is a guest computing without yielding: the runtime thread is inside
//! that guest until it yields or returns (hang recovery is #18).

use std::sync::Arc;

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
