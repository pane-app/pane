//! The guest's side of the window and feedback host functions
//! (`wit/feedback.wit`) and of `commands.set-subtitle`: each is a short
//! host call on the runtime's thread that hands the request, with its
//! [`Caller`], to the launcher's [`HostFunctions`]
//! ([`super::Runtime::set_host_functions`]), and answers what the launcher
//! answers. None waits for the user, so none is ever a reason to pause the
//! extension, and its time is Pane's, not the guest's ([`GuestState::host`]).
//!
//! Stopped code does nothing more: a window function then answers that no
//! window was shown, a toast or HUD is shown nowhere, and a subtitle is
//! refused. So does a runtime no launcher drives (tests of the runtime
//! alone).
//!
//! `feedback.confirm` waits for the user (#146): a WIT `async` function
//! with the store's [`Accessor`], as `helpers::Runs::run` is ([`Confirms`]).
//! It hands the confirmation to the launcher in a short synchronous part,
//! then awaits the answer without the store: other calls are served
//! meanwhile, and the wait is the host's time, never the guest's. A call
//! dropped meanwhile (its generation ended, the call was cancelled, Pane
//! quit) drops the wait, which takes the confirmation off the screen.

use std::sync::Arc;

use wasmtime::component::{Accessor, HasData};

use super::{GuestState, feedback_host, lock, stopped_code, window_host};
use crate::feedback::{
    Asking, Caller, GivenAction, GivenConfirmation, GivenToast, HostFunctions, Hud, PopToRoot,
    ToastStyle,
};

/// The host side of `feedback.confirm`, the one feedback function that
/// waits for the user; the others are [`feedback_host::Host`]'s.
pub(crate) struct Confirms;

impl HasData for Confirms {
    type Data<'a> = &'a mut GuestState;
}

impl<T> feedback_host::HostWithStore<T> for Confirms {
    async fn confirm(
        accessor: &Accessor<T, Self>,
        confirmation: feedback_host::Confirmation,
    ) -> Result<bool, String> {
        let (asking, watch) = accessor.with(|mut view| {
            let state = view.get();
            (state.ask_to_confirm(confirmation), state.watch())
        });
        // Waiting for the user is not the guest's computing, nor a hang:
        // the runtime thread only awaits it.
        super::deadlines::hosted(watch, asking).await
    }
}

impl GuestState {
    /// Hands `confirmation` to the launcher, which shows it, answers it from
    /// a remembered answer, or refuses it; what to await for the answer.
    fn ask_to_confirm(&self, confirmation: feedback_host::Confirmation) -> Asking {
        let refused = |why: String| -> Asking { Box::pin(std::future::ready(Err(why))) };
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return refused(stopped_code(end));
        }
        let Some(host) = lock(&self.host_functions).clone() else {
            return refused("this Pane asks the user nothing for extensions".into());
        };
        host.confirm(
            &self.caller(),
            GivenConfirmation {
                title: confirmation.title,
                message: confirmation.message,
                primary: confirmation.primary,
                destructive: confirmation.destructive,
                dismiss: confirmation.dismiss,
                remember: confirmation.remember,
            },
        )
    }
}

impl GuestState {
    /// Who makes a host call: this instance's component, and what the call
    /// it runs is for.
    fn caller(&self) -> Caller {
        Caller {
            component: self.component.clone(),
            command: self.call.command.clone(),
            windowed: self.call.windowed,
        }
    }

    /// The launcher's host functions, unless the instance's code is
    /// stopped or no launcher drives this runtime.
    fn host_functions(&self) -> Option<Arc<dyn HostFunctions>> {
        if self.stopped().is_some() {
            return None;
        }
        lock(&self.host_functions).clone()
    }

    /// `commands.set-subtitle`, which the `commands` interface's host
    /// implementation hands here.
    pub(super) fn set_command_subtitle(&mut self, subtitle: Option<String>) -> Result<(), String> {
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        let host = lock(&self.host_functions)
            .clone()
            .ok_or("this Pane keeps no subtitles for extensions")?;
        host.set_subtitle(&self.caller(), subtitle)
    }
}

impl window_host::Host for GuestState {
    fn close(&mut self, clear_root_search: bool, pop: window_host::PopToRootType) -> bool {
        let _host = self.host();
        let Some(host) = self.host_functions() else {
            return false;
        };
        let pop = match pop {
            window_host::PopToRootType::Default => PopToRoot::Default,
            window_host::PopToRootType::Immediate => PopToRoot::Immediate,
            window_host::PopToRootType::Suspended => PopToRoot::Suspended,
        };
        host.close(&self.caller(), clear_root_search, pop)
    }

    fn pop_to_root(&mut self, clear_search: bool) -> bool {
        let _host = self.host();
        let Some(host) = self.host_functions() else {
            return false;
        };
        host.pop_to_root(&self.caller(), clear_search)
    }

    fn clear_search(&mut self) -> bool {
        let _host = self.host();
        let Some(host) = self.host_functions() else {
            return false;
        };
        host.clear_search(&self.caller())
    }
}

impl feedback_host::Host for GuestState {
    fn show_toast(&mut self, toast: feedback_host::Toast) -> feedback_host::ToastId {
        let _host = self.host();
        let Some(host) = self.host_functions() else {
            return 0;
        };
        host.show_toast(&self.caller(), given(toast))
    }

    fn update_toast(&mut self, id: feedback_host::ToastId, toast: feedback_host::Toast) {
        let _host = self.host();
        if let Some(host) = self.host_functions() {
            host.update_toast(&self.caller(), id, given(toast));
        }
    }

    fn hide_toast(&mut self, id: feedback_host::ToastId) {
        let _host = self.host();
        if let Some(host) = self.host_functions() {
            host.hide_toast(&self.caller(), id);
        }
    }

    fn show_hud(&mut self, title: String, style: feedback_host::ToastStyle) {
        let _host = self.host();
        if let Some(host) = self.host_functions() {
            host.show_hud(
                &self.caller(),
                Hud {
                    title,
                    style: style_of(style),
                },
            );
        }
    }
}

/// `toast` as the guest's bindings carry it, for the launcher.
fn given(toast: feedback_host::Toast) -> GivenToast {
    let action = |action: feedback_host::ToastAction| GivenAction {
        title: action.title,
        callback: action.callback,
        shortcut: action.shortcut,
    };
    GivenToast {
        style: style_of(toast.style),
        title: toast.title,
        message: toast.message,
        primary: toast.primary.map(action),
        secondary: toast.secondary.map(action),
    }
}

fn style_of(style: feedback_host::ToastStyle) -> ToastStyle {
    match style {
        feedback_host::ToastStyle::Animated => ToastStyle::Animated,
        feedback_host::ToastStyle::Success => ToastStyle::Success,
        feedback_host::ToastStyle::Failure => ToastStyle::Failure,
    }
}
