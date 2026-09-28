//! The cache in front of a system adapter, so that a guest asking for the
//! installed applications gets them at once after the first scan.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::{Application, Applications};

/// The installed applications as `inner` last listed them. The first call
/// scans, on the calling thread (there is nothing to return before); later
/// calls return the kept list at once and, when it is older than `max_age`,
/// rescan on a thread of their own, so the next call gets the new list. A
/// failed rescan keeps the old list. Nothing is scanned until the first
/// call, so a disabled Applications extension causes no scanning.
pub struct Cached {
    inner: Arc<dyn Applications>,
    max_age: Duration,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    /// The last list and when its scan started.
    list: Option<(Vec<Application>, Instant)>,
    /// Whether a rescan is running.
    rescanning: bool,
}

impl Cached {
    pub fn new(inner: Arc<dyn Applications>, max_age: Duration) -> Cached {
        Cached {
            inner,
            max_age,
            state: Arc::default(),
        }
    }

    fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
        state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Applications for Cached {
    fn installed(&self) -> Result<Vec<Application>, String> {
        let mut state = Cached::lock(&self.state);
        let Some((list, scanned)) = &state.list else {
            drop(state);
            let started = Instant::now();
            let list = self.inner.installed()?;
            Cached::lock(&self.state).list = Some((list.clone(), started));
            return Ok(list);
        };
        let list = list.clone();
        if scanned.elapsed() >= self.max_age && !state.rescanning {
            state.rescanning = true;
            let (inner, shared) = (self.inner.clone(), self.state.clone());
            std::thread::spawn(move || {
                let started = Instant::now();
                let rescanned = inner.installed();
                let mut state = Cached::lock(&shared);
                state.rescanning = false;
                if let Ok(list) = rescanned {
                    state.list = Some((list, started));
                }
            });
        }
        Ok(list)
    }

    fn open(&self, id: &str) -> Result<(), String> {
        self.inner.open(id)
    }
}
