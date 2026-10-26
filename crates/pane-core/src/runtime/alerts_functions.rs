//! The guest's side of the `alerts` host functions (`wit/alerts.wit`, ADR
//! 0053): a system notification and a ringing sound. The notification is
//! `notification_functions`'s (#555), the ringing sound
//! `ringing_functions`'s (#556); on a system with no adapter for one, it
//! answers `not-available`, which is never a reason to pause the
//! extension.
//!
//! Stopped code does nothing more: each function answers that the code was
//! stopped, as the other host functions do.

use wasmtime::component::Resource;

use super::{GuestState, alerts_host, stopped_code};

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
        notification: alerts_host::Notification,
    ) -> Result<(), alerts_host::AlertsError> {
        if let Some(stopped) = self.alerts_stopped() {
            return Err(stopped);
        }
        self.post_notification(notification)
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
