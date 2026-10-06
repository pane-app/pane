//! Telling the user what happened (`pane:extension/feedback`): a toast in
//! the launcher's footer, which the command can update and give actions, or
//! a HUD, a short message over other applications once the launcher has
//! closed.
//!
//! ```ignore
//! use pane_guest::feedback::{Toast, ToastAction, show_toast};
//!
//! let shown = show_toast(Toast::animated("Uploading…"));
//! // ... the work ...
//! shown.update(
//!     Toast::success("Uploaded")
//!         .primary(ToastAction::new("Open", || async { Ok(()) })),
//! );
//! ```
//!
//! Pane shows one toast at a time: a new one replaces the one shown, whose
//! [`ShownToast`] then does nothing. While the launcher is hidden or
//! collapsed to its search field, a toast is shown as a HUD instead. A
//! toast's action runs its closure when the user chooses it (with the
//! pointer, from the keyboard, or with its shortcut while the toast shows),
//! as an item's action does; it can be chosen again while the toast shows.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use core::cell::{Cell, RefCell};
use core::future::Future;

use crate::list::{Answer, Shortcut, write_shortcut};
use crate::pane::extension::feedback as wit;

pub use wit::ToastStyle;

/// What a toast's action runs: each time the user chooses it.
type Run = Rc<dyn Fn() -> Answer>;

/// One of a toast's actions: what it is called, its shortcut, and the
/// closure it runs.
pub struct ToastAction {
    title: String,
    shortcut: Option<Shortcut>,
    run: Run,
}

impl ToastAction {
    /// An action titled `title` that runs `run` each time the user chooses
    /// it while the toast shows. An error it answers is shown as a failure
    /// toast.
    pub fn new<F, A>(title: impl Into<String>, run: F) -> ToastAction
    where
        F: Fn() -> A + 'static,
        A: Future<Output = Result<(), String>> + 'static,
    {
        ToastAction {
            title: title.into(),
            shortcut: None,
            run: Rc::new(move || Box::pin(run()) as Answer),
        }
    }

    /// This action run by `shortcut` while the toast shows. Pane never
    /// binds one of its own keys, or the primary action's shortcut again.
    pub fn shortcut(mut self, shortcut: Shortcut) -> ToastAction {
        self.shortcut = Some(shortcut);
        self
    }
}

/// A toast: its style, title, an optional message, and up to two actions.
pub struct Toast {
    style: ToastStyle,
    title: String,
    message: Option<String>,
    primary: Option<ToastAction>,
    secondary: Option<ToastAction>,
}

impl Toast {
    /// A toast in `style` titled `title`.
    pub fn new(style: ToastStyle, title: impl Into<String>) -> Toast {
        Toast {
            style,
            title: title.into(),
            message: None,
            primary: None,
            secondary: None,
        }
    }

    /// Work in progress, with a spinner: it stays until it is updated or
    /// hidden, or the window deactivates (or the no-view run that showed it
    /// ends).
    pub fn animated(title: impl Into<String>) -> Toast {
        Toast::new(ToastStyle::Animated, title)
    }

    /// The work succeeded: it hides after 3 seconds.
    pub fn success(title: impl Into<String>) -> Toast {
        Toast::new(ToastStyle::Success, title)
    }

    /// The work failed: it hides after 3 seconds.
    pub fn failure(title: impl Into<String>) -> Toast {
        Toast::new(ToastStyle::Failure, title)
    }

    /// This toast with `message` under its title.
    pub fn message(mut self, message: impl Into<String>) -> Toast {
        self.message = Some(message.into());
        self
    }

    /// This toast with `action` as its primary action: the first the
    /// keyboard reaches.
    pub fn primary(mut self, action: ToastAction) -> Toast {
        self.primary = Some(action);
        self
    }

    /// This toast with `action` as its secondary action.
    pub fn secondary(mut self, action: ToastAction) -> Toast {
        self.secondary = Some(action);
        self
    }
}

/// A toast the command showed, to update or hide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShownToast {
    id: u64,
    /// The SDK's own number for it, which names its actions' callbacks.
    number: u64,
}

impl ShownToast {
    /// Changes the toast to `toast`: its style, title, message and actions.
    /// It is shown again if it had left. Does nothing once another toast
    /// replaced it or it was hidden.
    pub fn update(&self, toast: Toast) {
        let wire = remember(self.number, toast);
        wit::update_toast(self.id, &wire);
    }

    /// Hides the toast. Does nothing once another toast replaced it.
    pub fn hide(&self) {
        wit::hide_toast(self.id);
        forget(self.number);
    }
}

/// Shows `toast` in the launcher's footer, replacing the one shown: as a
/// HUD while the launcher is hidden or collapsed to its search field.
pub fn show_toast(toast: Toast) -> ShownToast {
    let number = NEXT.0.get() + 1;
    NEXT.0.set(number);
    let wire = remember(number, toast);
    let id = wit::show_toast(&wire);
    ShownToast { id, number }
}

/// Closes the launcher, then shows `title` in a small window of its own
/// over other applications: for 1.2 seconds, or 3 for the failure style.
pub fn show_hud(title: &str, style: ToastStyle) {
    wit::show_hud(title, style);
}

/// The actions of the toasts the instance showed, by callback id; only the
/// newest toast's are kept, since Pane shows one at a time.
struct Actions(RefCell<BTreeMap<String, Run>>);

/// The number the last toast shown got.
struct Next(Cell<u64>);

// SAFETY: a component's code runs on one thread, and no borrow of the map
// is held across an `await`.
unsafe impl Sync for Actions {}
// SAFETY: as above.
unsafe impl Sync for Next {}

static ACTIONS: Actions = Actions(RefCell::new(BTreeMap::new()));
static NEXT: Next = Next(Cell::new(0));

/// The prefix of the callback ids of toast number `number`'s actions.
fn prefix(number: u64) -> String {
    format!("toast:{number}:")
}

/// `toast` as the WIT carries it; its actions are kept by their callback
/// ids, in place of those of the toast shown before.
fn remember(number: u64, toast: Toast) -> wit::Toast {
    let mut actions = ACTIONS.0.borrow_mut();
    actions.clear();
    let mut action = |slot: &str, action: Option<ToastAction>| {
        let action = action?;
        let callback = format!("{}{slot}", prefix(number));
        let shortcut = action.shortcut.as_ref().map(|shortcut| {
            let mut json = String::new();
            write_shortcut(&mut json, shortcut);
            json
        });
        actions.insert(callback.clone(), action.run);
        Some(wit::ToastAction {
            title: action.title,
            callback,
            shortcut,
        })
    };
    let primary = action("primary", toast.primary);
    let secondary = action("secondary", toast.secondary);
    wit::Toast {
        style: toast.style,
        title: toast.title,
        message: toast.message,
        primary,
        secondary,
    }
}

/// Forgets toast number `number`'s actions, once it was hidden.
fn forget(number: u64) {
    let prefix = prefix(number);
    ACTIONS
        .0
        .borrow_mut()
        .retain(|callback, _| !callback.starts_with(&prefix));
}

/// The action of a toast the instance showed named `callback`, if it is
/// one: [`crate::Command`]'s `handle-event` runs it.
pub(crate) fn toast_action(callback: &str) -> Option<Run> {
    ACTIONS.0.borrow().get(callback).cloned()
}
