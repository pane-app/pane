//! The host's registry of what extensions registered at run time (ADR
//! 0041, #158): dynamic root items, timers, folder watchers and run-time
//! provisions, each an owned registration the guest holds as a WIT
//! resource.
//!
//! A registration lasts until the first of three: the guest dropping its
//! handle (the host side of the resource runs its `drop` here), the
//! instance holding it going away (a crash, a stopped call, an
//! unresponsive stop, a search cancellation — every instance drop ends
//! its registrations, through [`crate::runtime::GuestState`]'s own
//! `Drop`), or the generation ending (the package is disabled, paused,
//! reloaded, updated or uninstalled). Each entry records how it is undone
//! on its generation's undo list ([`crate::generation`], #136); ending it
//! another way takes the entry off the list, and the undo list's test
//! hook finds it empty after every generation ends.
//!
//! The registry is shared: the runtime's guests reach it from their host
//! functions, the launcher reads it (root search's dynamic rows, the
//! timers and watchers threads' work, the provisions the operation router
//! and the waiting model see), and whichever thread ends a generation or
//! drops an instance runs its teardown. Its lock is held only for short,
//! self-contained work — never while calling a hook, never while taking
//! another lock — so no caller can deadlock on it.
//!
//! What changes tells the hooks ([`Registrations::set_changed`]), with
//! the lock let go: the launcher's refreshes root search, recomputes who
//! waits (a run-time provision makes the package a provider) and sees to
//! the activation entry point; the timers and watchers threads are woken
//! to take their new work. The activation entry point's own tracking
//! lives here too: which instance ran it for which generation, so its
//! instance going while the generation continues starts it again.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use crate::generation::{End, Generation, Registration};
use crate::runtime::Item;

/// How many dynamic root items one package may hold.
pub(crate) const MAX_ROOT_ITEMS: usize = 1000;

/// How many timers one package may hold.
pub(crate) const MAX_TIMERS: usize = 64;

/// How many folder watchers one package may hold.
pub(crate) const MAX_WATCHERS: usize = 16;

/// How many run-time provisions one package may hold.
pub(crate) const MAX_PROVISIONS: usize = 16;

/// The registrations of every package the runtime serves, and the
/// activation entry points it ran. Kept by the runtime's shared state;
/// cloned by every guest instance and read by the launcher.
#[derive(Debug, Default)]
pub(crate) struct Registrations {
    state: Mutex<RegistrationsState>,
    /// Told, with the state unlocked, after every change: a registration
    /// made or undone, an activation begun or its instance gone.
    hooks: Mutex<Vec<Weak<dyn Fn() + Send + Sync>>>,
}

/// What the registry keeps.
#[derive(Debug, Default)]
struct RegistrationsState {
    /// The next registration's id, never reused.
    next: u64,
    /// Every registration still held, in the order they were made.
    entries: Vec<Entry>,
    /// The activation entry points that ran: one per package and
    /// generation, held by the instance that ran it.
    activations: Vec<Activation>,
}

/// One owned registration, whatever its kind.
#[derive(Debug)]
struct Entry {
    id: u64,
    /// The package's identity key, as the launcher and the operation
    /// router name it.
    owner: String,
    /// The component whose instance made it: where events are delivered
    /// and which command a dynamic root item is under.
    component: PathBuf,
    /// Which instance holds it: the runtime thread's number and the
    /// instance's serial, never reused together. The instance going ends
    /// the registration.
    instance: (u64, u64),
    /// The generation it belongs to; its end undoes the registration.
    generation: Generation,
    /// The entry's place on that generation's undo list: dropped with
    /// the entry when the registration ends another way, so the list
    /// holds only what is still held.
    _undo: Registration,
    kind: Kind,
}

/// What a registration registers. One root item's actions make the
/// largest variant, and a package's registrations are bounded by its
/// limits.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A dynamic root item under the command with this manifest id.
    RootItem { command: String, item: DynamicItem },
    /// A timer: one firing (`after`) or one every interval (`every`), of
    /// `seconds`, firing with `tag`.
    Timer {
        after: bool,
        seconds: u64,
        tag: String,
    },
    /// A watcher of `path`, recursive or not, reporting with `tag`.
    Watcher {
        path: PathBuf,
        recursive: bool,
        tag: String,
    },
    /// A provision of the capability with this name.
    Provision { capability: String },
}

/// A dynamic root item, as the guest's JSON described it (the item shape
/// of docs/list-tree.md plus its mode).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DynamicItem {
    /// The item itself: id, title, subtitle, actions, look.
    pub(crate) item: Item,
    /// The mode a dynamic command declares; `None` for a plain item,
    /// whose invoking runs its first action's callback.
    pub(crate) mode: Option<DynamicMode>,
}

/// The mode of a dynamic command, as a manifest command's mode is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DynamicMode {
    /// `"view"`: invoking the item opens its command's screen.
    View,
    /// `"no-view"`: invoking the item runs its command.
    NoView,
}

/// An activation entry point that ran: the package's, in the generation
/// it ran in, held by the instance that ran it.
#[derive(Debug)]
struct Activation {
    owner: String,
    /// The generation the activation ran in: another generation means it
    /// has not run yet.
    generation: Generation,
    /// The instance that ran it: its going starts the activation again
    /// while the generation continues.
    instance: (u64, u64),
}

/// The handle a guest's resource holds: the registration's id. Each kind
/// has its own type, as the runtime's bindings map each WIT resource to
/// one. Public because the runtime's bindings re-export them; the module
/// stays private, so they reach no public API.
#[derive(Debug)]
pub struct RootItemHandle(pub(crate) u64);

#[derive(Debug)]
pub struct TimerHandle(pub(crate) u64);

#[derive(Debug)]
pub struct WatcherHandle(pub(crate) u64);

#[derive(Debug)]
pub struct ProvisionHandle(pub(crate) u64);

/// One event of what a package registered, as the timers and watchers
/// threads hand it to the runtime, which calls the component's `events`
/// export with it (wit/registrations.wit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GuestEvent {
    /// A timer fired: its tag.
    Timer { tag: String },
    /// A watcher's coalesced changes: its tag and what changed.
    Watcher {
        tag: String,
        changes: WatcherChanges,
    },
}

/// What a folder watcher reports: the paths that changed, or an
/// overflow, which the extension should answer by rescanning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WatcherChanges {
    /// The paths that changed, relative to the watched folder.
    Paths(Vec<String>),
    /// Too many changes were lost.
    Rescan,
}

impl Registrations {
    /// Registers `kind` for `owner`, made by the instance numbered
    /// `instance` of `component` in the generation `generation`, named
    /// `what` on its undo list, answering its id. The caller has checked
    /// the limits, the manifest and the code's being current; this only
    /// records, and tells the hooks.
    pub(crate) fn add(
        self: &Arc<Self>,
        owner: &str,
        component: &Path,
        instance: (u64, u64),
        generation: &Generation,
        what: &'static str,
        kind: Kind,
    ) -> u64 {
        let id = {
            let mut state = self.lock();
            let id = state.next;
            state.next += 1;
            // The entry owns its undo registration: however the
            // registration ends — the handle dropped, the instance gone,
            // the generation ended — the entry goes with it and takes the
            // list entry off.
            let registrations = Arc::downgrade(self);
            let undone = id;
            let undo = generation.on_end(what, move || {
                // The entry is only removed here: the hooks are not told,
                // because this runs on whichever thread ends the
                // generation — often the launcher's own, holding the
                // state lock the hooks would take. Every path that ends a
                // generation re-reads the registry itself (it refreshes
                // root search and recomputes who waits) and wakes the
                // worker threads through the extension data's hooks.
                if let Some(registrations) = registrations.upgrade() {
                    registrations.remove(undone);
                }
                Ok(())
            });
            state.entries.push(Entry {
                id,
                owner: owner.to_owned(),
                component: component.to_path_buf(),
                instance,
                generation: generation.clone(),
                _undo: undo,
                kind,
            });
            id
        };
        self.changed();
        id
    }

    /// Undoes the registration `id` and takes its entry off its
    /// generation's undo list: the guest dropped its handle, or the
    /// instance holding it went. Nothing changes, and no hook is told,
    /// when it is already undone. (Not `drop`, which the destructor's
    /// name reserves.)
    pub(crate) fn release(&self, id: u64) {
        if self.remove(id).is_some() {
            self.changed();
        }
    }

    /// Replaces the dynamic root item `id` owns with `item`, or answers
    /// why it cannot: the registration is gone, or its generation ended
    /// (this code of the extension was replaced).
    pub(crate) fn update_root_item(&self, id: u64, item: DynamicItem) -> Result<(), String> {
        {
            let mut state = self.lock();
            let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == id) else {
                return Err("its registration was undone".into());
            };
            if let Some(end) = entry.generation.ended() {
                return Err(replaced(end));
            }
            let Kind::RootItem { item: held, .. } = &mut entry.kind else {
                return Err("this handle is not a dynamic root item".into());
            };
            *held = item;
        }
        self.changed();
        Ok(())
    }

    /// Ends every registration made by the instance numbered `instance`
    /// of `component` — its store went, so nothing of its code survives —
    /// and forgets the activation entry point it ran, which while the
    /// generation continues runs again. Nothing is told unless something
    /// changed.
    pub(crate) fn instance_gone(&self, component: &Path, instance: (u64, u64)) {
        let changed = {
            let mut state = self.lock();
            let registrations = state.entries.len();
            state
                .entries
                .retain(|entry| !(entry.instance == instance && entry.component == component));
            let activations = state.activations.len();
            state
                .activations
                .retain(|activation| activation.instance != instance);
            (registrations != state.entries.len()) || (activations != state.activations.len())
        };
        if changed {
            self.changed();
        }
    }

    /// Every registration still held, whatever its package, in the order
    /// they were made: what the launcher lists and the worker threads
    /// take.
    pub(crate) fn all(&self) -> Vec<RegistrationOf> {
        self.lock()
            .entries
            .iter()
            .map(|entry| RegistrationOf {
                owner: entry.owner.clone(),
                component: entry.component.clone(),
                kind: entry.kind.clone(),
            })
            .collect()
    }

    /// The timers still held, in the order they were made.
    pub(crate) fn timers(&self) -> Vec<TimerOf> {
        self.lock()
            .entries
            .iter()
            .filter_map(|entry| match &entry.kind {
                Kind::Timer {
                    after,
                    seconds,
                    tag,
                } => Some(TimerOf {
                    id: entry.id,
                    owner: entry.owner.clone(),
                    component: entry.component.clone(),
                    after: *after,
                    seconds: *seconds,
                    tag: tag.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// The watchers still held, in the order they were made.
    pub(crate) fn watchers(&self) -> Vec<WatcherOf> {
        self.lock()
            .entries
            .iter()
            .filter_map(|entry| match &entry.kind {
                Kind::Watcher {
                    path,
                    recursive,
                    tag,
                } => Some(WatcherOf {
                    id: entry.id,
                    owner: entry.owner.clone(),
                    component: entry.component.clone(),
                    path: path.clone(),
                    recursive: *recursive,
                    tag: tag.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// Every package's run-time provisions, as the operation router and
    /// the waiting model see them: `(owner, capability)` pairs, only from
    /// unended generations.
    pub(crate) fn provisions(&self) -> Vec<(String, String)> {
        self.lock()
            .entries
            .iter()
            .filter(|entry| entry.generation.ended().is_none())
            .filter_map(|entry| match &entry.kind {
                Kind::Provision { capability } => Some((entry.owner.clone(), capability.clone())),
                _ => None,
            })
            .collect()
    }

    /// How many registrations of the kinds `of` names `owner` holds: the
    /// limit check's count.
    pub(crate) fn count(&self, owner: &str, of: impl Fn(&Kind) -> bool) -> usize {
        self.lock()
            .entries
            .iter()
            .filter(|entry| entry.owner == owner && of(&entry.kind))
            .count()
    }

    /// The refusal for a registration of `what` beyond `limit`, when the
    /// kinds `of` names already fill it: what the guest's call is
    /// answered with, the limit named.
    pub(crate) fn refuse_beyond(
        &self,
        owner: &str,
        what: &str,
        limit: usize,
        of: &impl Fn(&Kind) -> bool,
    ) -> Result<(), String> {
        let held = self.count(owner, of);
        if held >= limit {
            return Err(format!(
                "this package already holds {held} of Pane's limit of {limit} {what}s; drop one \
                 to register another"
            ));
        }
        Ok(())
    }

    /// Notes that the activation entry point of `owner` has begun, in the
    /// generation `generation`, held by the instance `instance`: it is
    /// not begun again while that instance lives.
    pub(crate) fn activated(&self, owner: &str, generation: &Generation, instance: (u64, u64)) {
        {
            let mut state = self.lock();
            let number = generation.number();
            state.activations.retain(|activation| {
                !(activation.owner == owner && activation.generation.number() == number)
            });
            state.activations.push(Activation {
                owner: owner.to_owned(),
                generation: generation.clone(),
                instance,
            });
        }
        self.changed();
    }

    /// Whether the activation entry point of `owner` has run in this
    /// `generation`, in an instance that still lives: it is not begun
    /// again while it has.
    pub(crate) fn is_activated(&self, owner: &str, generation: &Generation) -> bool {
        let number = generation.number();
        self.lock().activations.iter().any(|activation| {
            activation.owner == owner
                && activation.generation.number() == number
                && activation.generation.ended().is_none()
        })
    }

    /// Removes the entry `id` if it is still held, telling the caller so
    /// its hooks can be told too.
    fn remove(&self, id: u64) -> Option<()> {
        let mut state = self.lock();
        let at = state.entries.iter().position(|entry| entry.id == id)?;
        state.entries.remove(at);
        Some(())
    }

    /// Has `changed` called after each change of the registry, with its
    /// state unlocked. Held weakly, as the extension data's hooks are, so
    /// the registry never keeps the launcher or a worker thread alive.
    pub(crate) fn set_changed(&self, changed: Arc<dyn Fn() + Send + Sync>) {
        let weak = Arc::downgrade(&changed);
        self.hooks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(weak);
    }

    /// Calls each hook whose holder still lives.
    pub(crate) fn changed(&self) {
        let hooks: Vec<Weak<dyn Fn() + Send + Sync>> = self
            .hooks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        for hook in hooks {
            if let Some(hook) = hook.upgrade() {
                hook();
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, RegistrationsState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// One registration as the launcher and the worker threads read it: its
/// owner, its component and its kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegistrationOf {
    pub(crate) owner: String,
    pub(crate) component: PathBuf,
    pub(crate) kind: Kind,
}

/// What the timers thread fires: one timer registration, with its tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TimerOf {
    /// The registration's id, which the guest's handle names.
    pub(crate) id: u64,
    pub(crate) owner: String,
    pub(crate) component: PathBuf,
    /// Whether the timer fires once.
    pub(crate) after: bool,
    pub(crate) seconds: u64,
    pub(crate) tag: String,
}

/// What the watchers thread watches: one watcher registration, with its
/// tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WatcherOf {
    /// The registration's id, which the guest's handle names.
    pub(crate) id: u64,
    pub(crate) owner: String,
    pub(crate) component: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) recursive: bool,
    pub(crate) tag: String,
}

/// The refusal for a handle whose generation ended: the wording host
/// imports already use for stopped code.
pub(crate) fn replaced(end: End) -> String {
    match end {
        End::Disabled => "this code of the extension was disabled".to_owned(),
        End::Replaced => {
            "this code of the extension was replaced by a reload or an update".to_owned()
        }
        End::Uninstalled => "this code of the extension was uninstalled".to_owned(),
        End::Paused => "this code of the extension was paused after it failed".to_owned(),
        End::Abandoned => {
            "the runtime thread running this code of the extension was given up on".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::runtime::ItemLook;

    /// An item for the tests: id, title and nothing more.
    fn item(id: &str) -> DynamicItem {
        DynamicItem {
            item: crate::runtime::Item {
                id: id.into(),
                title: "the item".into(),
                subtitle: None,
                actions: Vec::new(),
                form: None,
                platforms: None,
                custom_view: None,
                look: ItemLook::default(),
            },
            mode: None,
        }
    }

    /// A handle used after its generation ended is refused with the
    /// message, and dropping it changes nothing: the code that made it
    /// was replaced, and the guest cannot act on state it no longer owns.
    /// (A guest's own code cannot reach this — its call is stopped first,
    /// which is the point — so the refusal is checked here, at the
    /// registry the host functions share.)
    #[test]
    fn a_handle_of_an_ended_generation_is_refused() {
        let registrations = Arc::new(Registrations::default());
        let generation = Generation::new();
        let id = registrations.add(
            "local:/somewhere",
            Path::new("/somewhere/fixture.wasm"),
            (1, 1),
            &generation,
            "dynamic root item",
            Kind::RootItem {
                command: "fixture".into(),
                item: item("held"),
            },
        );
        assert_eq!(
            registrations.update_root_item(id, item("replaced")),
            Ok(()),
            "a handle of a live generation updates its item"
        );
        drop(generation.end(End::Replaced));
        assert_eq!(
            registrations.update_root_item(id, item("again")),
            Err("its registration was undone".into()),
            "a handle of an ended generation is refused: the undo removed the entry, so it \
             says the registration is gone rather than why"
        );
        // The undo list is empty after the end, whatever the handle does.
        assert!(registrations.lock().entries.is_empty());
        registrations.release(id);
        assert!(registrations.lock().entries.is_empty());
    }

    /// The instance that holds a registration going ends it, and the
    /// activation entry point it ran starts again: that is what the
    /// launcher's hooks see.
    #[test]
    fn the_instance_going_ends_the_registration_and_the_activation() {
        let registrations = Arc::new(Registrations::default());
        let generation = Generation::new();
        let id = registrations.add(
            "local:/somewhere",
            Path::new("/somewhere/fixture.wasm"),
            (1, 7),
            &generation,
            "timer",
            Kind::Timer {
                after: true,
                seconds: 1,
                tag: "the timer".into(),
            },
        );
        registrations.activated("local:/somewhere", &generation, (1, 7));
        assert!(registrations.is_activated("local:/somewhere", &generation));
        registrations.instance_gone(Path::new("/somewhere/fixture.wasm"), (1, 7));
        assert!(
            registrations.lock().entries.is_empty(),
            "the registration ended"
        );
        assert!(
            !registrations.is_activated("local:/somewhere", &generation),
            "the activation starts again"
        );
        let _ = id;
    }
}
