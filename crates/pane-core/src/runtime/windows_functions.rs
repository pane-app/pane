//! The guest's side of the `windows` host functions (`wit/windows.wit`):
//! listing the open windows Alt+Tab would show, and bringing one of them
//! to the front. Each takes the launcher's [`SwitchWindows`] (through its
//! `HostFunctions`) and has the work done on a thread of its own: the
//! runtime thread awaits it, serving other packages' calls meanwhile, and
//! the wait is Pane's time, never the guest's computing (#18, #136),
//! since enumerating the windows asks their processes (a process's
//! program path, its package identity, its elevation) and bringing one
//! to the front waits for it to really be there.
//!
//! Stopped code does nothing more: each function answers that the code
//! was stopped. A runtime no launcher drives (tests of the runtime alone)
//! answers as a launcher given no windows adapter does.

use std::sync::Arc;

use super::{GuestState, lock, stopped_code, windows_host};
use crate::switch_windows::{self as host_windows, SwitchWindows, Window, WindowsError};

impl GuestState {
    /// The launcher's windows adapter, unless the instance's code is
    /// stopped (then why).
    fn windows_adapter(&self) -> Result<Arc<dyn SwitchWindows>, String> {
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        // Taken out first: the launcher is locked to read its adapter,
        // and never while the runtime's handle on it is.
        let host = lock(&self.host_functions).clone();
        Ok(host.map_or_else(host_windows::none, |host| host.switch_windows()))
    }
}

/// `window` as the WIT carries it.
fn wire(window: Window) -> windows_host::Window {
    windows_host::Window {
        id: window.id,
        title: window.title,
        application_name: window.application,
        icon: window.icon,
        minimized: window.minimized,
        maximized: window.maximized,
        elsewhere: window.elsewhere,
        elevated: window.elevated,
    }
}

/// `error` as the WIT carries it.
fn wire_error(error: WindowsError) -> windows_host::WindowsError {
    match error {
        WindowsError::NotAvailable(why) => windows_host::WindowsError::NotAvailable(why),
        WindowsError::Failed(why) => windows_host::WindowsError::Failed(why),
    }
}

/// What a command is told when the adapter's thread could not start or
/// failed.
fn failed() -> WindowsError {
    WindowsError::Failed("Pane could not reach the open windows (its thread failed)".into())
}

/// Runs `work` on a thread of its own, since the system may block (the
/// windows' processes, the foreground coming back), and answers what it
/// answered; `gone` answers when the thread could not start or failed.
async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    gone: impl FnOnce() -> T,
) -> T {
    let (reply, response) = tokio::sync::oneshot::channel();
    let started = std::thread::Builder::new()
        .name("pane-windows".into())
        .spawn(move || {
            let _ = reply.send(work());
        });
    if started.is_err() {
        return gone();
    }
    response.await.unwrap_or_else(|_| gone())
}

impl windows_host::Host for GuestState {
    async fn list_windows(
        &mut self,
    ) -> Result<Vec<windows_host::Window>, windows_host::WindowsError> {
        let windows = self
            .windows_adapter()
            .map_err(windows_host::WindowsError::Failed)?;
        let answer = self
            .hosted(off_thread(move || windows.list(), || Err(failed())))
            .await;
        answer
            .map_err(wire_error)
            .map(|listed| listed.into_iter().map(wire).collect())
    }

    async fn activate(&mut self, id: String) -> Result<(), windows_host::WindowsError> {
        let windows = self
            .windows_adapter()
            .map_err(windows_host::WindowsError::Failed)?;
        let answer = self
            .hosted(off_thread(move || windows.activate(&id), || Err(failed())))
            .await;
        answer.map_err(wire_error)
    }
}
