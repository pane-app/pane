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

use std::sync::Arc;

use super::{GuestState, feedback_host, lock, stopped_code, window_host};
use crate::feedback::{Caller, GivenAction, GivenToast, HostFunctions, Hud, PopToRoot, ToastStyle};

impl GuestState {
    /// Who makes a host call: this instance's component, and what the call
    /// it runs is for.
    pub(super) fn caller(&self) -> Caller {
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
