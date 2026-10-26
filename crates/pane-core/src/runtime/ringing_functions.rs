//! The guest's side of the ringing sound (`wit/alerts.wit`'s `ring` and
//! its `ringing` resource, ADR 0053), over the runtime's shared
//! [`crate::ringing::Ringer`]: what the guest asked is checked here, the
//! ringing's own thread reads the sound and opens the output, and the
//! guest's call awaits its answer without holding the runtime thread. The
//! handle the guest holds is the ringing's id; dropping it, the instance
//! going ([`GuestState`]'s drop) and the generation ending each stop the
//! sound, as for every owned registration (ADR 0041).

use std::path::PathBuf;

use wasmtime::component::Resource;

use super::{GuestState, alerts_host};
use crate::ringing::{Bundled, MAX_TIMES, Repeat, Ring, RingError, RingingHandle, Sound};

/// The WIT's answer for a ringing that did not start.
fn answer(error: RingError) -> alerts_host::AlertsError {
    match error {
        RingError::NotAvailable(why) => alerts_host::AlertsError::NotAvailable(why),
        RingError::Limit(why) => alerts_host::AlertsError::Limit(why),
        RingError::Failed(why) => alerts_host::AlertsError::Failed(why),
    }
}

impl GuestState {
    /// Starts `sound` as `repeat` says at `volume`, answering the guest's
    /// handle once it plays. The caller checked the code is not stopped.
    pub(super) async fn ring_sound(
        &mut self,
        sound: alerts_host::Sound,
        repeat: alerts_host::Repeat,
        volume: u8,
    ) -> Result<Resource<alerts_host::Ringing>, alerts_host::AlertsError> {
        let starting = {
            let _host = self.host();
            let ring = self.ring_asked(sound, repeat, volume)?;
            self.ringing.ring(ring).map_err(answer)?
        };
        // Reading the sound and opening the output happen on the ringing's
        // own thread; this awaits its answer as host work.
        let id = self.hosted(starting.started()).await.map_err(answer)?;
        self.table.push(RingingHandle(id)).map_err(|error| {
            self.ringing.release(id);
            alerts_host::AlertsError::Failed(error.to_string())
        })
    }

    /// The ringing the guest asked for, checked: an installed package's,
    /// `times` within its bounds, a file's path resolved against the
    /// package's folder.
    fn ring_asked(
        &self,
        sound: alerts_host::Sound,
        repeat: alerts_host::Repeat,
        volume: u8,
    ) -> Result<Ring, alerts_host::AlertsError> {
        let owner = self.owner().ok_or_else(|| {
            alerts_host::AlertsError::Failed(
                "only installed extensions ring sounds; this command is built into Pane".into(),
            )
        })?;
        let repeat = match repeat {
            alerts_host::Repeat::Once => Repeat::Once,
            alerts_host::Repeat::Times(times) if (1..=MAX_TIMES).contains(&times) => {
                Repeat::Times(times)
            }
            alerts_host::Repeat::Times(times) => {
                return Err(alerts_host::AlertsError::Failed(format!(
                    "`times` asks for the sound {times} times; Pane plays a sound 1 to \
                     {MAX_TIMES} times"
                )));
            }
            alerts_host::Repeat::UntilStopped => Repeat::UntilStopped,
        };
        let sound = match sound {
            alerts_host::Sound::Bundled(bundled) => Sound::Bundled(match bundled {
                alerts_host::BundledSound::Alarm => Bundled::Alarm,
                alerts_host::BundledSound::Bell => Bundled::Bell,
                alerts_host::BundledSound::Chime => Bundled::Chime,
            }),
            alerts_host::Sound::System => Sound::System,
            alerts_host::Sound::File(path) => {
                let path = PathBuf::from(path);
                Sound::File(if path.is_absolute() {
                    path
                } else {
                    // Relative to the package's folder, so a package can
                    // ship its own.
                    self.location_of(&self.installed()).join(path)
                })
            }
        };
        Ok(Ring {
            owner,
            component: self.component.clone(),
            instance: self.identity,
            generation: self.generation().cloned(),
            sound,
            repeat,
            volume,
        })
    }

    /// The guest dropped its handle: the sound stops, and leaves its
    /// generation's undo list. One that already ended changes nothing.
    pub(super) fn drop_ringing(&mut self, own: Resource<alerts_host::Ringing>) {
        if let Ok(handle) = self.table.delete(own) {
            self.ringing.release(handle.0);
        }
    }
}
