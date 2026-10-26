//! The guest's side of the `alerts` host functions (`wit/alerts.wit`, ADR
//! 0053): a system notification and a ringing sound. No system has an
//! adapter yet, so every call answers `not-available`, which is never a
//! reason to pause the extension; Windows' notification (#555) and ringing
//! sound (#556) replace these answers. The interface is linked now so the
//! world Pane hosts imports it, and a component built against it loads.
//!
//! Stopped code does nothing more: each function answers that the code was
//! stopped, as the other host functions do.

use wasmtime::component::Resource;

use super::{GuestState, alerts_host, stopped_code};

/// What every call answers until this system has an adapter.
fn not_available(what: &str) -> alerts_host::AlertsError {
    alerts_host::AlertsError::NotAvailable(format!(
        "{what} is not available in this version of Pane yet"
    ))
}

impl GuestState {
    /// Why the call is refused before anything is asked of the system: the
    /// instance's code was stopped.
    fn alerts_stopped(&self) -> Option<alerts_host::AlertsError> {
        let _host = self.host();
        self.stopped()
            .map(|end| alerts_host::AlertsError::Failed(stopped_code(end)))
    }
}

impl alerts_host::Host for GuestState {
    fn notify(
        &mut self,
        _notification: alerts_host::Notification,
    ) -> Result<(), alerts_host::AlertsError> {
        if let Some(stopped) = self.alerts_stopped() {
            return Err(stopped);
        }
        Err(not_available("A system notification"))
    }

    fn ring(
        &mut self,
        _sound: alerts_host::Sound,
        _repeat: alerts_host::Repeat,
        _volume: u8,
    ) -> Result<Resource<alerts_host::Ringing>, alerts_host::AlertsError> {
        if let Some(stopped) = self.alerts_stopped() {
            return Err(stopped);
        }
        Err(not_available("A ringing sound"))
    }
}

impl alerts_host::HostRinging for GuestState {
    /// No ringing is ever made yet, so there is nothing to stop: a handle a
    /// guest drops can only be one Pane never gave.
    fn drop(&mut self, _ringing: Resource<alerts_host::Ringing>) -> wasmtime::Result<()> {
        Ok(())
    }
}
