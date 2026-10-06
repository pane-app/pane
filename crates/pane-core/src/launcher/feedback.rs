//! The window and feedback host functions on the launcher's side (#141,
//! ADR 0037): what Pane does when a command closes the window, pops back to
//! root search, clears the search field, or shows a toast or a HUD.
//!
//! A host function reaches the launcher from the runtime's thread through
//! [`Hosted`], the [`HostFunctions`] the launcher gives the runtime, with
//! its [`Caller`]. The launcher changes its own state there and then (the
//! screen, the toast) and has the window do what only it can (hide, show a
//! HUD's window) through the [`WindowControl`] attached to it
//! ([`Launcher::attach_window`]); the window then redraws from the launcher,
//! as it does for any change made in the background.
//!
//! **Window.** `close` hides the launcher; `pop` decides what its next
//! showing shows ([`Launcher::take_next_showing`]), and `clear-root-search`
//! empties root search's query. `pop-to-root` returns to root search with
//! the window open, and `clear-search` empties the search field on screen.
//! In a call no window was shown for (a background launch, a schedule, a
//! service, an operation), they do nothing and answer so.
//!
//! **Toast.** One at a time: a new toast replaces the current one, whose
//! id then does nothing. The footer shows it where the status line is,
//! while the launcher is shown expanded; while it is hidden or collapsed to
//! its search field ([`WindowPresence`]), a toast shown or updated is shown
//! as a HUD instead. The window hides a success or failure toast after
//! [`crate::feedback::TOAST_DURATION`] ([`Launcher::toast_left`]), and any
//! toast leaves the footer when the window deactivates
//! ([`Launcher::window_deactivated`]); updating it shows it again. Choosing
//! one of its actions calls its command's `handle-event` with the action's
//! callback id, as an item's action does ([`Launcher::run_toast_action`]).
//!
//! **HUD.** Closes the window first, then the window shows it in a window
//! of its own for 1.2 seconds, or 3 for a failure.
//!
//! **Run-ending feedback.** An action's or a no-view run's answer shows
//! nothing (the status line no longer shows answered text); an error a
//! command answers with is a failure toast with a "Copy Error" action, and
//! a toast a no-view run left in the animated style is hidden once the run
//! ends.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Launcher, Screen, State, Status, WeakLauncher, item_actions, stopped};
use crate::extension_data::PackageData;
use crate::feedback::{
    Caller, GivenAction, GivenToast, HostFunctions, Hud, NextShowing, NoWindow, PopToRoot,
    ShownToast, Toast, ToastAction, ToastDoes, ToastSlot, ToastStyle, WindowControl,
    WindowPresence,
};
use crate::keyboard::{Binding, PaneKeys};
use crate::runtime::{CallError, read_shortcut};

/// What a failure toast for an error a command answered with is titled:
/// with its message, it reads as [`CallError::Guest`] does.
pub(super) const FAILURE_TITLE: &str = "The extension reported an error";

/// The title of the action a failure toast offers to copy its error.
pub(super) const COPY_ERROR: &str = "Copy Error";

/// What the launcher keeps for the window and feedback host functions.
pub(super) struct Feedback {
    /// Has the window hide and show HUDs.
    pub(super) window: Arc<dyn WindowControl>,
    /// Whether the window is shown, as it last said.
    pub(super) presence: WindowPresence,
    /// What the window's next showing shows, as the last `close` asked.
    pub(super) next_showing: NextShowing,
    /// The toast, while there is one: shown in the footer, or left (its
    /// time ran out, the window deactivated, it was shown as a HUD) but
    /// still updatable.
    pub(super) toast: Option<CurrentToast>,
    /// The last toast id given.
    pub(super) last_toast: u64,
}

impl Default for Feedback {
    fn default() -> Feedback {
        Feedback {
            window: Arc::new(NoWindow),
            presence: WindowPresence::Shown,
            next_showing: NextShowing::BySetting,
            toast: None,
            last_toast: 0,
        }
    }
}

/// The toast the launcher has, and whose it is.
pub(super) struct CurrentToast {
    id: u64,
    /// Counts its updates.
    revision: u64,
    /// The component of the command that showed it: its actions call this
    /// component's `handle-event`. Pane's own failure toast is the failed
    /// command's.
    owner: PathBuf,
    /// The manifest id of that command, when Pane knows it.
    command: Option<String>,
    toast: Toast,
    /// Whether the footer shows it now.
    in_footer: bool,
}

/// The launcher's host functions, which the runtime holds: weakly, so
/// they keep neither the launcher nor the runtime running. Once the
/// launcher has gone, a window function answers that no window was shown
/// and a toast or HUD is shown nowhere.
pub(super) struct Hosted(pub(super) WeakLauncher);

impl HostFunctions for Hosted {
    fn close(&self, caller: &Caller, clear_root_search: bool, pop: PopToRoot) -> bool {
        self.0
            .upgrade()
            .is_some_and(|launcher| launcher.close_window(caller, clear_root_search, pop))
    }

    fn pop_to_root(&self, caller: &Caller, clear_search: bool) -> bool {
        self.0
            .upgrade()
            .is_some_and(|launcher| launcher.pop_to_root_search(caller, clear_search))
    }

    fn clear_search(&self, caller: &Caller) -> bool {
        self.0
            .upgrade()
            .is_some_and(|launcher| launcher.clear_search_field(caller))
    }

    fn show_toast(&self, caller: &Caller, toast: GivenToast) -> u64 {
        self.0
            .upgrade()
            .map_or(0, |launcher| launcher.show_given_toast(caller, toast))
    }

    fn update_toast(&self, caller: &Caller, id: u64, toast: GivenToast) {
        if let Some(launcher) = self.0.upgrade() {
            launcher.update_given_toast(caller, id, toast);
        }
    }

    fn hide_toast(&self, caller: &Caller, id: u64) {
        if let Some(launcher) = self.0.upgrade() {
            launcher.hide_given_toast(caller, id);
        }
    }

    fn show_hud(&self, _caller: &Caller, hud: Hud) {
        if let Some(launcher) = self.0.upgrade() {
            launcher.show_hud(hud);
        }
    }

    fn set_subtitle(&self, caller: &Caller, subtitle: Option<String>) -> Result<(), String> {
        match self.0.upgrade() {
            Some(launcher) => launcher.set_subtitle(caller, subtitle),
            None => Err("Pane is stopping".into()),
        }
    }

    fn system(&self) -> Arc<dyn crate::system::System> {
        self.0
            .upgrade()
            .map_or_else(crate::system::none, |launcher| launcher.system())
    }
}

impl Launcher {
    /// Has `window` hide the launcher and show HUDs for the host functions
    /// commands call. Without one (a launcher no window shows), nothing is
    /// hidden or shown, though the launcher still keeps its toast and
    /// changes its screen.
    pub fn attach_window(&self, window: Arc<dyn WindowControl>) {
        self.lock().feedback.window = window;
    }

    /// Tells the launcher whether its window is shown, collapsed to its
    /// search field, or hidden. The window says so whenever it changes; a
    /// toast shown while it is not shown expanded is shown as a HUD, and
    /// the toast in the footer leaves when it stops being shown.
    pub fn set_window_presence(&self, presence: WindowPresence) {
        let mut state = self.lock();
        if state.feedback.presence == presence {
            return;
        }
        state.feedback.presence = presence;
        if presence != WindowPresence::Shown
            && let Some(current) = state.feedback.toast.as_mut()
        {
            current.in_footer = false;
        }
    }

    /// Whether the launcher's window is shown, as it last said (or as a
    /// command's `close` or HUD left it).
    pub fn window_presence(&self) -> WindowPresence {
        self.lock().feedback.presence
    }

    /// What the window's next showing shows, as the last command that
    /// closed it asked: by the Launcher page's reopening choice, or the
    /// screen left on display. Asked once per showing: it goes back to the
    /// reopening choice.
    pub fn take_next_showing(&self) -> NextShowing {
        std::mem::take(&mut self.lock().feedback.next_showing)
    }

    /// The toast the footer shows now, if any.
    pub fn toast(&self) -> Option<ShownToast> {
        let state = self.lock();
        let current = state
            .feedback
            .toast
            .as_ref()
            .filter(|current| current.in_footer)?;
        Some(ShownToast {
            id: current.id,
            revision: current.revision,
            toast: current.toast.clone(),
        })
    }

    /// The toast `id` in its `revision` has been shown for its time (the
    /// window counts it, pausing while the pointer is over it or it has the
    /// focus): it leaves the footer. Nothing happens once it was updated or
    /// replaced. Its command may still update it, which shows it again.
    pub fn toast_left(&self, id: u64, revision: u64) {
        let mut state = self.lock();
        if let Some(current) = state.feedback.toast.as_mut()
            && current.id == id
            && current.revision == revision
        {
            current.in_footer = false;
        }
    }

    /// The window lost the focus: the toast leaves the footer, an animated
    /// one too. Its command may still update it, which shows it again.
    pub fn window_deactivated(&self) {
        if let Some(current) = self.lock().feedback.toast.as_mut() {
            current.in_footer = false;
        }
    }

    /// The text choosing the action in `slot` of the toast `id` copies, if
    /// it is one of Pane's own actions that copies (a failure's "Copy
    /// Error"), for the window to put on the clipboard before it calls
    /// [`Launcher::run_toast_action`], as it does for a root result's
    /// answer.
    pub fn toast_action_copy(&self, id: u64, slot: ToastSlot) -> Option<String> {
        let state = self.lock();
        let current = state
            .feedback
            .toast
            .as_ref()
            .filter(|current| current.id == id && current.in_footer)?;
        current.toast.action(slot)?.copies().map(str::to_owned)
    }

    /// Chooses the action in `slot` of the toast `id`, while the footer
    /// shows it: Pane's own copy says it copied; a command's calls its
    /// `handle-event` with the action's callback id, as an item's action
    /// does, and lists the command's screen again when it is on display.
    /// Await the returned future to apply the answer. Nothing happens for a
    /// toast replaced or left meanwhile, or a slot without an action.
    pub fn run_toast_action(
        &self,
        id: u64,
        slot: ToastSlot,
    ) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let mut run = None;
        let chosen = state
            .feedback
            .toast
            .as_ref()
            .filter(|current| current.id == id && current.in_footer)
            .and_then(|current| {
                let action = current.toast.action(slot)?;
                Some((
                    current.owner.clone(),
                    current.command.clone(),
                    action.does.clone(),
                ))
            });
        match chosen {
            None => {}
            Some((owner, command, ToastDoes::Copy(_))) => {
                let copied = Toast::new(ToastStyle::Success, "Copied the error to the clipboard");
                self.put_toast(&mut state, owner, command, copied);
            }
            Some((owner, command, ToastDoes::Callback(callback))) => {
                // The status line is about this action from now on.
                state.sent_from = None;
                let data = self.data_in(&state, &owner);
                run = Some((state.screen_epoch, owner, command, callback, data));
            }
        }
        drop(state);
        self.changed();
        let launcher = self.clone();
        async move {
            if let Some((epoch, owner, command, callback, data)) = run {
                launcher
                    .run_toast_callback(epoch, owner, command, callback, data)
                    .await;
            }
        }
    }

    /// Has the command in `component` handle `callback`, its toast's
    /// action's: an error it answers with is a failure toast, and its
    /// screen, when on display, is listed again.
    async fn run_toast_callback(
        &self,
        epoch: u64,
        component: PathBuf,
        command: Option<String>,
        callback: String,
        data: Option<PackageData>,
    ) {
        let result = match self.runtime() {
            Ok(runtime) => {
                runtime
                    .handle_event_with(
                        &component,
                        command.as_deref(),
                        &callback,
                        "{}",
                        data.clone(),
                    )
                    .await
            }
            Err(error) => Err(error),
        };
        let handled = matches!(
            result,
            Ok(_) | Err(CallError::Guest(_) | CallError::Unreadable(_))
        );
        let list_again = {
            let mut state = self.lock();
            let state = &mut *state;
            let ended = stopped(state, &component, &data);
            match (&ended, result) {
                (Some(_), _) | (None, Ok(_)) => {}
                (None, Err(CallError::Guest(message))) => {
                    self.show_failure(state, &component, command.as_deref(), message);
                }
                (None, Err(error)) => {
                    if state.screen_epoch == epoch {
                        state.view.status = Status::Error(error.to_string());
                    }
                }
            }
            let open_here = state.open.as_ref() == Some(&component)
                && matches!(
                    state.view.screen,
                    Screen::Command | Screen::CommandSearch { .. }
                );
            handled && ended.is_none() && open_here && state.screen_epoch == epoch
        };
        if list_again {
            self.list_again(epoch, component, data).await;
        }
        self.changed();
    }

    /// `window.close` for `caller`: hides the window, with what its next
    /// showing shows; whether a window was shown for the call.
    fn close_window(&self, caller: &Caller, clear_root_search: bool, pop: PopToRoot) -> bool {
        if !caller.windowed {
            return false;
        }
        {
            let mut state = self.lock();
            let state = &mut *state;
            state.feedback.next_showing = match pop {
                PopToRoot::Suspended => NextShowing::Restore,
                PopToRoot::Default | PopToRoot::Immediate => NextShowing::BySetting,
            };
            if pop == PopToRoot::Immediate {
                self.show_root(state, None);
            } else if clear_root_search && root_query_typed(state) {
                let _ = self.search(state, "");
            }
            hide_window(state);
        }
        self.changed();
        true
    }

    /// `window.pop-to-root` for `caller`: root search, with the window
    /// open; whether a window was shown for the call.
    fn pop_to_root_search(&self, caller: &Caller, clear_search: bool) -> bool {
        if !caller.windowed {
            return false;
        }
        {
            let mut state = self.lock();
            let state = &mut *state;
            if !matches!(state.view.screen, Screen::Root { .. }) {
                // Root search is listed afresh, its query empty.
                self.show_root(state, None);
            } else if clear_search && root_query_typed(state) {
                let _ = self.search(state, "");
            }
        }
        self.changed();
        true
    }

    /// `window.clear-search` for `caller`: empties the search field on
    /// screen; whether a window was shown for the call.
    fn clear_search_field(&self, caller: &Caller) -> bool {
        if !caller.windowed {
            return false;
        }
        {
            let mut state = self.lock();
            let state = &mut *state;
            let command_search = matches!(
                &state.view.screen,
                Screen::CommandSearch { query } if !query.is_empty()
            );
            if root_query_typed(state) {
                let _ = self.search(state, "");
            } else if command_search {
                self.clear_search_in_command(state);
            }
        }
        self.changed();
        true
    }

    /// `feedback.show-toast` for `caller`: the new toast's id.
    fn show_given_toast(&self, caller: &Caller, toast: GivenToast) -> u64 {
        let id = {
            let mut state = self.lock();
            let toast = bind(toast, &state.pane_keys);
            self.put_toast(
                &mut state,
                caller.component.clone(),
                caller.command.clone(),
                toast,
            )
        };
        self.changed();
        id
    }

    /// `feedback.update-toast` for `caller`: the toast `id` is updated, and
    /// shown again, while it is still the launcher's and `caller`'s.
    fn update_given_toast(&self, caller: &Caller, id: u64, toast: GivenToast) {
        {
            let mut state = self.lock();
            let state = &mut *state;
            let toast = bind(toast, &state.pane_keys);
            let Some(current) = state
                .feedback
                .toast
                .as_mut()
                .filter(|current| current.id == id && current.owner == caller.component)
            else {
                return;
            };
            current.toast = toast;
            current.revision += 1;
            present(&mut state.feedback);
        }
        self.changed();
    }

    /// `feedback.hide-toast` for `caller`: the toast `id` goes, while it is
    /// still the launcher's and `caller`'s.
    fn hide_given_toast(&self, caller: &Caller, id: u64) {
        {
            let mut state = self.lock();
            let current = state.feedback.toast.as_ref();
            if !current.is_some_and(|current| current.id == id && current.owner == caller.component)
            {
                return;
            }
            state.feedback.toast = None;
        }
        self.changed();
    }

    /// `feedback.show-hud`: closes the window, if it is shown, and shows
    /// `hud` in its own.
    fn show_hud(&self, hud: Hud) {
        {
            let mut state = self.lock();
            let state = &mut *state;
            if state.feedback.presence != WindowPresence::Hidden {
                state.feedback.next_showing = NextShowing::BySetting;
                hide_window(state);
            }
            state.feedback.window.show_hud(&hud);
        }
        self.changed();
    }

    /// Makes `toast`, of the command `command` in `owner`, the launcher's
    /// toast, replacing the one it had, and shows it; its id.
    fn put_toast(
        &self,
        state: &mut State,
        owner: PathBuf,
        command: Option<String>,
        toast: Toast,
    ) -> u64 {
        state.feedback.last_toast += 1;
        let id = state.feedback.last_toast;
        state.feedback.toast = Some(CurrentToast {
            id,
            revision: 0,
            owner,
            command,
            toast,
            in_footer: false,
        });
        present(&mut state.feedback);
        id
    }

    /// Shows the error `message` the command `command` in `component`
    /// answered with as a failure toast, with a "Copy Error" action.
    pub(super) fn show_failure(
        &self,
        state: &mut State,
        component: &Path,
        command: Option<&str>,
        message: String,
    ) {
        let toast = Toast {
            style: ToastStyle::Failure,
            title: FAILURE_TITLE.into(),
            message: Some(message.clone()),
            primary: Some(ToastAction {
                title: COPY_ERROR.into(),
                shortcut: None,
                unbound: None,
                does: ToastDoes::Copy(message),
            }),
            secondary: None,
        };
        self.put_toast(
            state,
            component.to_path_buf(),
            command.map(str::to_owned),
            toast,
        );
    }

    /// Hides the toast of the command in `component` if it is still in the
    /// animated style: its no-view run has ended, and nothing will finish
    /// it.
    pub(super) fn clear_animated_toast(&self, state: &mut State, component: &Path) {
        let left = state.feedback.toast.as_ref().is_some_and(|current| {
            current.owner == component && current.toast.style == ToastStyle::Animated
        });
        if left {
            state.feedback.toast = None;
        }
    }
}

/// Whether root search is on screen with a query typed.
fn root_query_typed(state: &State) -> bool {
    matches!(&state.view.screen, Screen::Root { query } if !query.is_empty())
}

/// Has the window hide, and notes it hidden: the toast leaves the footer.
fn hide_window(state: &mut State) {
    state.feedback.window.hide();
    state.feedback.presence = WindowPresence::Hidden;
    if let Some(current) = state.feedback.toast.as_mut() {
        current.in_footer = false;
    }
}

/// Shows the launcher's toast: in the footer while the window is shown
/// expanded, else as a HUD.
fn present(feedback: &mut Feedback) {
    let presence = feedback.presence;
    let window = feedback.window.clone();
    let Some(current) = feedback.toast.as_mut() else {
        return;
    };
    if presence == WindowPresence::Shown {
        current.in_footer = true;
    } else {
        current.in_footer = false;
        window.show_hud(&Hud {
            title: current.toast.text(),
            style: current.toast.style,
        });
    }
}

/// `toast` as a command gave it, its actions' shortcuts bound against
/// Pane's keys `keys`.
fn bind(toast: GivenToast, keys: &PaneKeys) -> Toast {
    let primary = toast.primary.map(|action| bind_action(action, keys, None));
    let taken = primary.as_ref().and_then(|action| action.shortcut.clone());
    let secondary = toast
        .secondary
        .map(|action| bind_action(action, keys, taken.as_ref()));
    Toast {
        style: toast.style,
        title: toast.title,
        message: toast.message,
        primary,
        secondary,
    }
}

/// `action` with its shortcut bound, unless it is one of Pane's keys
/// `keys`, a key a search field types, one Pane cannot read, or `taken`
/// (the primary action's), as an item's action's is.
fn bind_action(action: GivenAction, keys: &PaneKeys, taken: Option<&Binding>) -> ToastAction {
    let (shortcut, unbound) = match action.shortcut.as_deref().and_then(read_shortcut) {
        None => (None, None),
        Some(Err(why)) => (None, Some(why)),
        Some(Ok(binding)) => {
            if let Some(does) = keys.taken(&binding) {
                (None, Some(format!("{binding} {does} in Pane")))
            } else if item_actions::types(&binding) {
                (
                    None,
                    Some(format!(
                        "{binding} has no Ctrl, Alt or Cmd, so it would take a key a search \
                         field types or moves with"
                    )),
                )
            } else if taken == Some(&binding) {
                (
                    None,
                    Some(format!("{binding} already runs the toast's primary action")),
                )
            } else {
                (Some(binding), None)
            }
        }
    };
    ToastAction {
        title: action.title,
        shortcut,
        unbound,
        does: ToastDoes::Callback(action.callback),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn given(shortcut: Option<&str>) -> GivenAction {
        GivenAction {
            title: "Retry".into(),
            callback: "retry".into(),
            shortcut: shortcut.map(str::to_owned),
        }
    }

    #[test]
    fn a_toast_actions_shortcut_is_bound_as_an_items_is() {
        let keys = PaneKeys::default();
        let bound = bind_action(
            given(Some(r#"{"modifiers": ["ctrl", "shift"], "key": "r"}"#)),
            &keys,
            None,
        );
        assert_eq!(
            bound.shortcut,
            Some(Binding::parse("ctrl-shift-r").unwrap())
        );
        let panes = bind_action(
            given(Some(r#"{"modifiers": ["ctrl"], "key": "k"}"#)),
            &keys,
            None,
        );
        assert_eq!(panes.shortcut, None);
        assert!(panes.unbound.unwrap().contains("in Pane"));
        let typed = bind_action(given(Some(r#"{"key": "r"}"#)), &keys, None);
        assert_eq!(typed.shortcut, None);
        let taken = Binding::parse("ctrl-shift-r").unwrap();
        let again = bind_action(
            given(Some(r#"{"modifiers": ["ctrl", "shift"], "key": "r"}"#)),
            &keys,
            Some(&taken),
        );
        assert!(again.unbound.unwrap().contains("primary action"));
        assert_eq!(bind_action(given(None), &keys, None).shortcut, None);
    }

    #[test]
    fn a_failure_toast_reads_as_the_error_did() {
        assert_eq!(
            Toast {
                message: Some("no network".into()),
                ..Toast::new(ToastStyle::Failure, FAILURE_TITLE)
            }
            .text(),
            CallError::Guest("no network".into()).to_string()
        );
    }

    use std::sync::Mutex;

    use futures::executor::block_on;

    use crate::feedback::WindowRequest;
    use crate::launcher::CommandRegistration;

    /// A window that records what the launcher has it do.
    #[derive(Default)]
    struct Recording(Mutex<Vec<WindowRequest>>);

    impl Recording {
        fn take(&self) -> Vec<WindowRequest> {
            std::mem::take(&mut *self.0.lock().unwrap())
        }
    }

    impl WindowControl for Recording {
        fn hide(&self) {
            self.0.lock().unwrap().push(WindowRequest::Hide);
        }

        fn show_hud(&self, hud: &Hud) {
            self.0.lock().unwrap().push(WindowRequest::Hud(hud.clone()));
        }
    }

    /// The component of the one command these tests register.
    const COMPONENT: &str = "notes.wasm";

    /// A launcher with no runtime (the host functions need none) and one
    /// registered command, with a recording window attached.
    fn launcher() -> (Launcher, Arc<Recording>) {
        let launcher = Launcher::new(
            Err(CallError::RuntimeUnavailable(
                "these tests run no guest".into(),
            )),
            vec![CommandRegistration {
                id: "notes".into(),
                title: "Notes".into(),
                subtitle: Some("Your notes".into()),
                component: PathBuf::from(COMPONENT),
                takes_query: false,
                search: false,
            }],
        );
        let window = Arc::new(Recording::default());
        launcher.attach_window(window.clone());
        (launcher, window)
    }

    /// A call of the command in `component`, with a window or not.
    fn call_of(component: &str, windowed: bool) -> Caller {
        Caller {
            component: PathBuf::from(component),
            command: Some("notes".into()),
            windowed,
        }
    }

    fn call(windowed: bool) -> Caller {
        call_of(COMPONENT, windowed)
    }

    fn toast(style: ToastStyle, title: &str) -> GivenToast {
        GivenToast {
            style,
            title: title.into(),
            message: None,
            primary: None,
            secondary: None,
        }
    }

    fn query(launcher: &Launcher) -> Option<String> {
        launcher.view().query().map(str::to_owned)
    }

    fn shown_title(launcher: &Launcher) -> Option<String> {
        launcher.toast().map(|shown| shown.toast.title)
    }

    #[test]
    fn with_no_window_the_window_functions_do_nothing_and_say_so() {
        let (launcher, window) = launcher();
        block_on(launcher.set_query("notes"));
        assert!(!launcher.close_window(&call(false), true, PopToRoot::Immediate));
        assert!(!launcher.pop_to_root_search(&call(false), true));
        assert!(!launcher.clear_search_field(&call(false)));
        assert_eq!(window.take(), []);
        assert_eq!(query(&launcher).as_deref(), Some("notes"));
        assert_eq!(launcher.window_presence(), WindowPresence::Shown);
    }

    #[test]
    fn close_hides_the_window_and_says_what_its_next_showing_shows() {
        let (launcher, window) = launcher();
        assert!(launcher.close_window(&call(true), false, PopToRoot::Default));
        assert_eq!(window.take(), [WindowRequest::Hide]);
        assert_eq!(launcher.window_presence(), WindowPresence::Hidden);
        assert_eq!(launcher.take_next_showing(), NextShowing::BySetting);

        launcher.set_window_presence(WindowPresence::Shown);
        assert!(launcher.close_window(&call(true), false, PopToRoot::Suspended));
        assert_eq!(launcher.take_next_showing(), NextShowing::Restore);
        // Asked once per showing.
        assert_eq!(launcher.take_next_showing(), NextShowing::BySetting);
    }

    #[test]
    fn close_empties_root_search_or_returns_to_it_as_asked() {
        let (launcher, _window) = launcher();
        block_on(launcher.set_query("notes"));
        assert!(launcher.close_window(&call(true), false, PopToRoot::Default));
        assert_eq!(query(&launcher).as_deref(), Some("notes"), "kept");
        assert!(launcher.close_window(&call(true), true, PopToRoot::Default));
        assert_eq!(query(&launcher).as_deref(), Some(""), "emptied");

        block_on(launcher.set_query("notes"));
        assert!(launcher.close_window(&call(true), false, PopToRoot::Immediate));
        assert_eq!(query(&launcher).as_deref(), Some(""), "root search afresh");
    }

    #[test]
    fn pop_to_root_and_clear_search_do_what_they_name_with_the_window_open() {
        let (launcher, window) = launcher();
        block_on(launcher.set_query("notes"));
        assert!(launcher.pop_to_root_search(&call(true), false));
        assert_eq!(query(&launcher).as_deref(), Some("notes"));
        assert!(launcher.pop_to_root_search(&call(true), true));
        assert_eq!(query(&launcher).as_deref(), Some(""));
        block_on(launcher.set_query("notes"));
        assert!(launcher.clear_search_field(&call(true)));
        assert_eq!(query(&launcher).as_deref(), Some(""));
        assert_eq!(window.take(), [], "the window stays open");
        assert_eq!(launcher.window_presence(), WindowPresence::Shown);
    }

    #[test]
    fn one_toast_at_a_time_and_a_replaced_ones_id_does_nothing() {
        let (launcher, _window) = launcher();
        let first = launcher.show_given_toast(&call(true), toast(ToastStyle::Animated, "First"));
        let second = launcher.show_given_toast(&call(true), toast(ToastStyle::Success, "Second"));
        assert_ne!(first, second);
        assert_eq!(launcher.toast().map(|shown| shown.id), Some(second));
        launcher.update_given_toast(&call(true), first, toast(ToastStyle::Failure, "Stale"));
        launcher.hide_given_toast(&call(true), first);
        assert_eq!(shown_title(&launcher).as_deref(), Some("Second"));

        launcher.update_given_toast(&call(true), second, toast(ToastStyle::Failure, "Updated"));
        let shown = launcher.toast().unwrap();
        assert_eq!(
            (shown.id, shown.revision, shown.toast.style),
            (second, 1, ToastStyle::Failure)
        );
        assert_eq!(shown.toast.title, "Updated");
        launcher.hide_given_toast(&call(true), second);
        assert_eq!(launcher.toast(), None);
    }

    #[test]
    fn only_the_command_that_showed_a_toast_changes_it() {
        let (launcher, _window) = launcher();
        let id = launcher.show_given_toast(&call(true), toast(ToastStyle::Success, "Mine"));
        let other = call_of("other.wasm", true);
        launcher.update_given_toast(&other, id, toast(ToastStyle::Success, "Theirs"));
        launcher.hide_given_toast(&other, id);
        assert_eq!(shown_title(&launcher).as_deref(), Some("Mine"));
    }

    #[test]
    fn while_the_launcher_is_hidden_or_compact_a_toast_is_a_hud() {
        let (launcher, window) = launcher();
        for presence in [WindowPresence::Hidden, WindowPresence::Compact] {
            launcher.set_window_presence(presence);
            let id = launcher.show_given_toast(&call(true), toast(ToastStyle::Animated, "Working"));
            assert_eq!(launcher.toast(), None, "{presence:?}");
            let mut done = toast(ToastStyle::Success, "Done");
            done.message = Some("3 files".into());
            launcher.update_given_toast(&call(true), id, done);
            assert_eq!(
                window.take(),
                [
                    WindowRequest::Hud(Hud {
                        title: "Working".into(),
                        style: ToastStyle::Animated
                    }),
                    WindowRequest::Hud(Hud {
                        title: "Done: 3 files".into(),
                        style: ToastStyle::Success
                    }),
                ],
                "{presence:?}"
            );
        }
        // Shown again, the next toast is in the footer.
        launcher.set_window_presence(WindowPresence::Shown);
        launcher.show_given_toast(&call(true), toast(ToastStyle::Success, "Back"));
        assert_eq!(shown_title(&launcher).as_deref(), Some("Back"));
        assert_eq!(window.take(), []);
    }

    #[test]
    fn a_toast_leaves_when_its_time_is_up_or_the_window_deactivates_and_an_update_brings_it_back() {
        let (launcher, _window) = launcher();
        let id = launcher.show_given_toast(&call(true), toast(ToastStyle::Success, "Saved"));
        launcher.toast_left(id, 0);
        assert_eq!(launcher.toast(), None);
        launcher.update_given_toast(&call(true), id, toast(ToastStyle::Success, "Saved again"));
        assert_eq!(launcher.toast().map(|shown| shown.revision), Some(1));
        // The time of an earlier revision is not this one's.
        launcher.toast_left(id, 0);
        assert_eq!(shown_title(&launcher).as_deref(), Some("Saved again"));

        let id = launcher.show_given_toast(&call(true), toast(ToastStyle::Animated, "Working"));
        launcher.window_deactivated();
        assert_eq!(launcher.toast(), None, "an animated toast leaves too");
        launcher.update_given_toast(&call(true), id, toast(ToastStyle::Success, "Done"));
        assert_eq!(shown_title(&launcher).as_deref(), Some("Done"));
    }

    #[test]
    fn a_hud_closes_the_window_first() {
        let (launcher, window) = launcher();
        let hud = Hud {
            title: "Copied to Clipboard".into(),
            style: ToastStyle::Success,
        };
        launcher.show_hud(hud.clone());
        assert_eq!(
            window.take(),
            [WindowRequest::Hide, WindowRequest::Hud(hud.clone())]
        );
        assert_eq!(launcher.window_presence(), WindowPresence::Hidden);
        // Already hidden: only the HUD.
        launcher.show_hud(hud.clone());
        assert_eq!(window.take(), [WindowRequest::Hud(hud)]);
    }

    #[test]
    fn a_failure_toast_offers_to_copy_the_error() {
        let (launcher, _window) = launcher();
        {
            let mut state = launcher.lock();
            launcher.show_failure(
                &mut state,
                Path::new(COMPONENT),
                Some("notes"),
                "the disk is full".into(),
            );
        }
        let shown = launcher.toast().unwrap();
        assert_eq!(shown.toast.style, ToastStyle::Failure);
        assert_eq!(
            shown.toast.text(),
            "The extension reported an error: the disk is full"
        );
        assert_eq!(
            shown
                .toast
                .primary
                .as_ref()
                .map(|action| action.title.as_str()),
            Some(COPY_ERROR)
        );
        assert_eq!(
            launcher
                .toast_action_copy(shown.id, ToastSlot::Primary)
                .as_deref(),
            Some("the disk is full")
        );
        block_on(launcher.run_toast_action(shown.id, ToastSlot::Primary));
        let copied = launcher.toast().unwrap();
        assert_eq!(
            (copied.toast.style, copied.toast.title.as_str()),
            (ToastStyle::Success, "Copied the error to the clipboard")
        );
    }

    #[test]
    fn an_animated_toast_a_run_left_is_hidden_and_nothing_else() {
        let (launcher, _window) = launcher();
        launcher.show_given_toast(&call(true), toast(ToastStyle::Animated, "Working"));
        launcher.clear_animated_toast(&mut launcher.lock(), Path::new("other.wasm"));
        assert_eq!(shown_title(&launcher).as_deref(), Some("Working"));
        launcher.clear_animated_toast(&mut launcher.lock(), Path::new(COMPONENT));
        assert_eq!(launcher.toast(), None);

        launcher.show_given_toast(&call(true), toast(ToastStyle::Success, "Done"));
        launcher.clear_animated_toast(&mut launcher.lock(), Path::new(COMPONENT));
        assert_eq!(shown_title(&launcher).as_deref(), Some("Done"));
    }

    #[test]
    fn a_subtitle_shows_on_the_commands_row_and_is_matched() {
        let (launcher, _window) = launcher();
        let subtitle = |launcher: &Launcher| {
            launcher
                .view()
                .rows
                .iter()
                .find(|row| row.title == "Notes")
                .and_then(|row| row.subtitle.clone())
        };
        assert_eq!(subtitle(&launcher).as_deref(), Some("Your notes"));
        launcher
            .set_subtitle(&call(true), Some("3 unread".into()))
            .unwrap();
        assert_eq!(subtitle(&launcher).as_deref(), Some("3 unread"));
        assert_eq!(
            launcher.command_subtitle("notes").as_deref(),
            Some("3 unread")
        );
        block_on(launcher.set_query("unread"));
        assert_eq!(subtitle(&launcher).as_deref(), Some("3 unread"), "matched");
        launcher.set_subtitle(&call(true), None).unwrap();
        block_on(launcher.set_query(""));
        assert_eq!(subtitle(&launcher).as_deref(), Some("Your notes"));

        let unknown = Caller {
            command: None,
            ..call(true)
        };
        assert!(launcher.set_subtitle(&unknown, Some("x".into())).is_err());
    }
}
