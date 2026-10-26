//! The guest's side of the `alerts` host functions (`wit/alerts.wit`, ADR
//! 0053): a system notification and a ringing sound. No system has a
//! notification adapter yet, so `notify` answers `not-available`, which is
//! never a reason to pause the extension; Windows' notification (#555)
//! replaces that answer. The ringing sound is `ringing_functions`'s (#556).
//! The interface is linked so the world Pane hosts imports it, and a
//! component built against it loads.
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

    async fn ring(
        &mut self,
        sound: alerts_host::Sound,
        repeat: alerts_host::Repeat,
        volume: u8,
    ) -> Result<Resource<alerts_host::Ringing>, alerts_host::AlertsError> {
        if let Some(stopped) = self.alerts_stopped() {
            return Err(stopped);
        }
        self.ring_sound(sound, repeat, volume).await
    }
}

impl alerts_host::HostRinging for GuestState {
    /// The handle is gone: so is the sound.
    fn drop(&mut self, ringing: Resource<alerts_host::Ringing>) -> wasmtime::Result<()> {
        self.drop_ringing(ringing);
        Ok(())
    }
}
