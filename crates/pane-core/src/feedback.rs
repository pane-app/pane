//! What a command does after it acts (ADR 0037): the window and feedback
//! host functions every command has, whatever its mode (`wit/feedback.wit`),
//! and the seam through which the launcher drives the window for them.
//!
//! A command closes the launcher, pops back to root search or clears the
//! search field, and tells the user what happened with a **toast** in the
//! launcher's footer or a **HUD** in a small window of its own over other
//! applications. The launcher keeps the toast (one at a time: a new one
//! replaces the old one) and decides what the window does; the window draws
//! the toast where the status line was, times it, and opens the HUD's
//! window, through [`WindowControl`].
//!
//! A command also asks the user to **confirm** before it does something it
//! cannot undo: the launcher keeps the [`Confirmation`] (one at a time) and
//! its remembered answers, and the window draws it over the current screen,
//! showing itself first if it is hidden, and answers it
//! ([`ConfirmAnswer`]).
//!
//! The host functions reach the launcher through the runtime (see
//! [`HostFunctions`]): each knows its [`Caller`], the guest's component
//! and what Pane knows of the call it is made in (the command it is for,
//! and whether a window was shown for it).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::keyboard::Binding;

/// How long a success or failure toast stays before it hides by itself
/// (decision 2, ADR 0035), not counting the time the pointer is over it or
/// it has the focus.
pub const TOAST_DURATION: Duration = Duration::from_secs(3);

/// How long a HUD stays: 1.2 seconds (ADR 0035).
pub const HUD_DURATION: Duration = Duration::from_millis(1200);

/// How long a HUD in the failure style stays: 3 seconds (ADR 0035).
pub const FAILURE_HUD_DURATION: Duration = Duration::from_secs(3);

/// How a toast or a HUD is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastStyle {
    /// Work in progress, with a spinner: such a toast stays until it is
    /// updated or hidden, or the window deactivates.
    Animated,
    /// The work succeeded.
    Success,
    /// The work failed.
    Failure,
}

impl ToastStyle {
    /// Whether a toast in this style hides by itself after
    /// [`TOAST_DURATION`]: a success or a failure does, work in progress
    /// does not.
    pub fn hides_by_itself(self) -> bool {
        self != ToastStyle::Animated
    }
}

/// A toast as the footer shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub style: ToastStyle,
    pub title: String,
    /// More text under the title.
    pub message: Option<String>,
    /// Its primary action: the first the keyboard reaches.
    pub primary: Option<ToastAction>,
    pub secondary: Option<ToastAction>,
}

impl Toast {
    /// A toast in `style` titled `title`, with nothing more.
    pub fn new(style: ToastStyle, title: impl Into<String>) -> Toast {
        Toast {
            style,
            title: title.into(),
            message: None,
            primary: None,
            secondary: None,
        }
    }

    /// The toast's action in `slot`, if it has one there.
    pub fn action(&self, slot: ToastSlot) -> Option<&ToastAction> {
        match slot {
            ToastSlot::Primary => self.primary.as_ref(),
            ToastSlot::Secondary => self.secondary.as_ref(),
        }
    }

    /// The slot of the action `binding` runs, when one of its actions has
    /// it bound.
    pub fn bound_to(&self, binding: &Binding) -> Option<ToastSlot> {
        [ToastSlot::Primary, ToastSlot::Secondary]
            .into_iter()
            .find(|slot| {
                self.action(*slot)
                    .is_some_and(|action| action.shortcut.as_ref() == Some(binding))
            })
    }

    /// The title and message as one line, "Title: message", as a HUD or a
    /// copied error says it.
    pub fn text(&self) -> String {
        match &self.message {
            Some(message) if !message.trim().is_empty() => format!("{}: {message}", self.title),
            _ => self.title.clone(),
        }
    }
}

/// Which of a toast's two actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToastSlot {
    Primary,
    Secondary,
}

/// One of a toast's actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToastAction {
    pub title: String,
    /// Its shortcut, when Pane binds it: it runs the action while the
    /// toast shows.
    pub shortcut: Option<Binding>,
    /// Why its shortcut is not bound, when it has one Pane does not bind.
    pub unbound: Option<String>,
    /// What choosing it does.
    pub(crate) does: ToastDoes,
}

impl ToastAction {
    /// The text choosing this action copies to the clipboard, when it is
    /// one of Pane's own actions that copies (a failure's "Copy Error"):
    /// the window copies it, as it copies a root result's answer.
    pub fn copies(&self) -> Option<&str> {
        match &self.does {
            ToastDoes::Copy(text) => Some(text),
            ToastDoes::Callback(_) => None,
        }
    }
}

/// What choosing a toast's action does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ToastDoes {
    /// Calls the toast's command's `handle-event` with this callback id.
    Callback(String),
    /// Copies this text: Pane's own "Copy Error".
    Copy(String),
}

/// The toast the launcher shows now, as the window reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownToast {
    /// Names this toast: a new toast has a new id, so the window's timer
    /// and the actions it offers are for this one only.
    pub id: u64,
    /// Changes each time the toast is updated, which starts its time again.
    pub revision: u64,
    pub toast: Toast,
}

/// A HUD: a short message in a small window of its own, near the bottom
/// of the screen the launcher was on, over other applications, which never
/// takes the focus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hud {
    pub title: String,
    pub style: ToastStyle,
}

impl Hud {
    /// How long it stays: [`FAILURE_HUD_DURATION`] for a failure,
    /// [`HUD_DURATION`] otherwise.
    pub fn duration(&self) -> Duration {
        match self.style {
            ToastStyle::Failure => FAILURE_HUD_DURATION,
            ToastStyle::Animated | ToastStyle::Success => HUD_DURATION,
        }
    }
}

/// What the launcher shows the next time it is shown, once a command
/// closed it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PopToRoot {
    /// What the user's Launcher setting says (restore the view, or root
    /// search).
    #[default]
    Default,
    /// Root search, which the launcher returns to at once.
    Immediate,
    /// The screen left on display, whatever the setting says.
    Suspended,
}

/// What the window does when it is next summoned, by what the last command
/// that closed it asked ([`crate::Launcher::take_next_showing`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NextShowing {
    /// What the Launcher page's reopening choice says.
    #[default]
    BySetting,
    /// The screen left on display, whatever the choice says.
    Restore,
}

/// Whether the launcher's window shows, and how, as the window tells the
/// launcher ([`crate::Launcher::set_window_presence`]): a toast is shown as
/// a HUD unless the launcher is shown expanded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowPresence {
    /// Shown, with its footer.
    #[default]
    Shown,
    /// Shown collapsed to its search field (the compact window mode): no
    /// footer to show a toast in.
    Compact,
    /// Hidden.
    Hidden,
}

/// The dismiss button's label when the command names none.
pub const DISMISS: &str = "Cancel";

/// What the box that remembers a confirmation's answer says.
pub const DONT_ASK_AGAIN: &str = "Don't ask again";

/// A confirmation a command asks for (`feedback.confirm`), as the window
/// draws it over the launcher's current screen
/// ([`crate::Launcher::confirmation`]): one at a time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirmation {
    /// Names this confirmation: the window answers it by its id
    /// ([`crate::Launcher::answer_confirmation`]).
    pub id: u64,
    pub title: String,
    /// More text under the title.
    pub message: Option<String>,
    /// The primary button's label: Enter chooses it.
    pub primary: String,
    /// Whether the primary button is drawn in the destructive style.
    pub destructive: bool,
    /// The dismiss button's label ([`DISMISS`] unless the command named
    /// another): Escape chooses it.
    pub dismiss: String,
    /// Whether it offers [`DONT_ASK_AGAIN`]: the command gave a key to
    /// remember the answer by, and its package is installed.
    pub rememberable: bool,
}

/// How the user answered a confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAnswer {
    /// The primary button, or Enter: the command is told the user
    /// confirmed.
    Confirmed,
    /// The dismiss button, or Escape.
    Dismissed,
    /// Neither button: a click outside the confirmation. It answers as
    /// [`ConfirmAnswer::Dismissed`] does, but is never remembered. (The
    /// window losing the focus or hiding answers so too, through
    /// [`crate::Launcher::window_deactivated`] and
    /// [`crate::Launcher::set_window_presence`].)
    Left,
}

/// What the launcher has the window do for the host functions commands
/// call. Called on the runtime's thread, with the launcher's state locked:
/// an implementation only hands the request on (the window's own thread
/// carries it out) and never blocks.
pub trait WindowControl: Send + Sync {
    /// Hides the launcher window.
    fn hide(&self);
    /// Shows `hud` in a window of its own, for [`Hud::duration`], replacing
    /// a HUD still shown.
    fn show_hud(&self, hud: &Hud);
    /// A confirmation was asked for, or went unanswered (its call was
    /// dropped): the window draws [`crate::Launcher::confirmation`] as it
    /// is now, showing itself first while one is asked and it is hidden.
    fn confirmation(&self) {}
}

/// A launcher with no window: nothing is hidden or shown.
pub struct NoWindow;

impl WindowControl for NoWindow {
    fn hide(&self) {}
    fn show_hud(&self, _hud: &Hud) {}
}

/// A request the launcher makes of the window ([`channel`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowRequest {
    Hide,
    Hud(Hud),
    /// See [`WindowControl::confirmation`].
    Confirmation,
}

/// A [`WindowControl`] that sends its requests to the window, and the
/// window's end, which receives them on its own thread.
pub fn channel() -> (Arc<dyn WindowControl>, WindowRequests) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    (Arc::new(Sender(sender)), WindowRequests(receiver))
}

struct Sender(tokio::sync::mpsc::UnboundedSender<WindowRequest>);

impl WindowControl for Sender {
    fn hide(&self) {
        let _ = self.0.send(WindowRequest::Hide);
    }

    fn show_hud(&self, hud: &Hud) {
        let _ = self.0.send(WindowRequest::Hud(hud.clone()));
    }

    fn confirmation(&self) {
        let _ = self.0.send(WindowRequest::Confirmation);
    }
}

/// The window's end of [`channel`].
#[derive(Debug)]
pub struct WindowRequests(tokio::sync::mpsc::UnboundedReceiver<WindowRequest>);

impl WindowRequests {
    /// The next request, in the order they were made; `None` once the
    /// launcher has gone.
    pub async fn next(&mut self) -> Option<WindowRequest> {
        self.0.recv().await
    }
}

/// Who calls a host function: the guest's component, and what Pane knows
/// of the call it is made in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Caller {
    pub component: PathBuf,
    /// The manifest id of the command the call is for; `None` where Pane
    /// does not know one (an operation, a root search's call, an indexed
    /// result's, a custom view's event).
    pub command: Option<String>,
    /// Whether a window was shown for the call: the user launched the
    /// command, or works in its screen. A background launch, a schedule,
    /// a service's cycle or a search of root search has none.
    pub windowed: bool,
}

/// A toast as a command gives it, before Pane binds its actions'
/// shortcuts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GivenToast {
    pub style: ToastStyle,
    pub title: String,
    pub message: Option<String>,
    pub primary: Option<GivenAction>,
    pub secondary: Option<GivenAction>,
}

/// A toast's action as a command gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GivenAction {
    pub title: String,
    pub callback: String,
    /// Its shortcut as the command wrote it (JSON, as in the tree).
    pub shortcut: Option<String>,
}

/// A confirmation as a command asks for it (`feedback.confirm`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GivenConfirmation {
    pub title: String,
    pub message: Option<String>,
    pub primary: String,
    pub destructive: bool,
    /// The dismiss button's label, if the command named one.
    pub dismiss: Option<String>,
    /// The key its answer is remembered by, if the command gave one.
    pub remember: Option<String>,
}

/// The answer to a confirmation, once the user gives it: whether they
/// confirmed, or why Pane asked nothing. The runtime's thread awaits it
/// without holding other calls; dropping it (the call was dropped) takes
/// the confirmation off the screen.
pub(crate) type Asking =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, String>> + Send>>;

/// What Pane does for the window and feedback host functions a command
/// calls: the launcher's own (`Launcher::host_functions`). Each is a short
/// host call on the runtime's thread; a refusal is an answer. The one that
/// waits for the user, `confirm`, answers at once with what it waits on.
/// Later host functions (copying, opening, pasting) are added here the same
/// way.
pub(crate) trait HostFunctions: Send + Sync {
    /// `window.close`: whether a window was shown for the call.
    fn close(&self, caller: &Caller, clear_root_search: bool, pop: PopToRoot) -> bool;
    /// `window.pop-to-root`: whether a window was shown for the call.
    fn pop_to_root(&self, caller: &Caller, clear_search: bool) -> bool;
    /// `window.clear-search`: whether a window was shown for the call.
    fn clear_search(&self, caller: &Caller) -> bool;
    /// `feedback.show-toast`: the new toast's id.
    fn show_toast(&self, caller: &Caller, toast: GivenToast) -> u64;
    /// `feedback.update-toast`.
    fn update_toast(&self, caller: &Caller, id: u64, toast: GivenToast);
    /// `feedback.hide-toast`.
    fn hide_toast(&self, caller: &Caller, id: u64);
    /// `feedback.show-hud`.
    fn show_hud(&self, caller: &Caller, hud: Hud);
    /// `commands.set-subtitle`.
    fn set_subtitle(&self, caller: &Caller, subtitle: Option<String>) -> Result<(), String>;
    /// `feedback.confirm`: shows `confirmation` (or answers from a
    /// remembered answer, or refuses) at once, and answers what to await.
    fn confirm(&self, caller: &Caller, confirmation: GivenConfirmation) -> Asking;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hud_stays_longer_for_a_failure() {
        let hud = |style| Hud {
            title: "Done".into(),
            style,
        };
        assert_eq!(
            hud(ToastStyle::Success).duration(),
            Duration::from_millis(1200)
        );
        assert_eq!(
            hud(ToastStyle::Animated).duration(),
            Duration::from_millis(1200)
        );
        assert_eq!(hud(ToastStyle::Failure).duration(), Duration::from_secs(3));
    }

    #[test]
    fn only_work_in_progress_stays() {
        assert!(!ToastStyle::Animated.hides_by_itself());
        assert!(ToastStyle::Success.hides_by_itself());
        assert!(ToastStyle::Failure.hides_by_itself());
    }

    #[test]
    fn a_toast_reads_as_its_title_and_message() {
        let mut toast = Toast::new(ToastStyle::Failure, "Upload failed");
        assert_eq!(toast.text(), "Upload failed");
        toast.message = Some("the server said no".into());
        assert_eq!(toast.text(), "Upload failed: the server said no");
    }

    #[test]
    fn requests_reach_the_window_in_order() {
        let (control, mut requests) = channel();
        control.hide();
        control.show_hud(&Hud {
            title: "Copied".into(),
            style: ToastStyle::Success,
        });
        control.confirmation();
        drop(control);
        let received = futures::executor::block_on(async {
            let mut all = Vec::new();
            while let Some(request) = requests.next().await {
                all.push(request);
            }
            all
        });
        assert_eq!(
            received,
            [
                WindowRequest::Hide,
                WindowRequest::Hud(Hud {
                    title: "Copied".into(),
                    style: ToastStyle::Success
                }),
                WindowRequest::Confirmation,
            ]
        );
    }
}
