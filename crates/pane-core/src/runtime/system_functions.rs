//! The guest's side of the `system` host functions (`wit/system.wit`): the
//! clipboard, opening things, revealing them and moving them to the
//! Recycle Bin, pasting into the application that was in front before
//! Pane, that application and the text selected in it. Each takes the
//! launcher's [`System`] (through its
//! `HostFunctions`), checks what the command gave (a path must be
//! absolute), and has the system do the work on a thread of its own: the
//! runtime thread awaits it, serving other packages' calls meanwhile, and
//! the wait is Pane's time, never the guest's computing (#18, #136). None
//! closes the window or tells the user anything but `paste`, which closes
//! it (as `window.close` does) once the system says it can paste, and none
//! is a reason to pause the extension: a refusal is an answer, and "not
//! available on this system yet" is one too, distinct from a failure.
//!
//! Stopped code does nothing more: each function answers that the code
//! was stopped. A runtime no launcher drives (tests of the runtime alone)
//! answers as a launcher given no system does.

use std::sync::Arc;

use super::{GuestState, lock, stopped_code, system_host};
use crate::feedback::PopToRoot;
use crate::platform::Platform;
use crate::system::{
    self as host_system, Clip, MAX_CLIPBOARD_TEXT, NotTrashed, System, SystemError,
};

impl GuestState {
    /// The launcher's system, unless the instance's code is stopped (then
    /// why).
    fn system(&self) -> Result<Arc<dyn System>, String> {
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        // Taken out first: the launcher is locked to read its system, and
        // never while the runtime's handle on it is.
        let host = lock(&self.host_functions).clone();
        Ok(host.map_or_else(host_system::none, |host| host.system()))
    }

    /// Closes the window for a paste, as `window.close` does: in a call a
    /// window was shown for, with the next showing as the user's Launcher
    /// setting says. Why not, if the code was stopped meanwhile.
    fn close_for_paste(&self) -> Result<(), SystemError> {
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(SystemError::Failed(stopped_code(end)));
        }
        let host = lock(&self.host_functions).clone();
        if let Some(host) = host {
            host.close(&self.caller(), false, PopToRoot::Default);
        }
        Ok(())
    }
}

/// `error` as the WIT carries it.
fn wire_error(error: SystemError) -> system_host::SystemError {
    match error {
        SystemError::NotAvailable(why) => system_host::SystemError::NotAvailable(why),
        SystemError::Failed(why) => system_host::SystemError::Failed(why),
    }
}

/// A command's clip as Pane's: a file's path must be absolute.
fn clip_from(content: system_host::Clip) -> Result<Clip, String> {
    Ok(match content {
        system_host::Clip::Text(text) => Clip::Text(text),
        system_host::Clip::File(path) => Clip::File(host_system::absolute(&path)?),
    })
}

/// What the system's thread failing is, for the functions answering a
/// [`SystemError`].
fn failed_error() -> SystemError {
    SystemError::Failed(failed())
}

/// Runs `work` on a thread of its own, since the system may block (a
/// handler starting, the clipboard held open), and answers what it
/// answered.
async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    gone: impl FnOnce() -> T,
) -> T {
    let (reply, response) = tokio::sync::oneshot::channel();
    let started = std::thread::Builder::new()
        .name("pane-system".into())
        .spawn(move || {
            let _ = reply.send(work());
        });
    if started.is_err() {
        return gone();
    }
    response.await.unwrap_or_else(|_| gone())
}

/// What a command is told when the system's thread could not start or
/// failed.
fn failed() -> String {
    "Pane could not reach the system for this (its thread failed)".into()
}

impl system_host::Host for GuestState {
    async fn running_on(&mut self) -> system_host::HostSystem {
        match Platform::current() {
            Some(Platform::Windows) => system_host::HostSystem::Windows,
            Some(Platform::Macos) => system_host::HostSystem::Macos,
            Some(Platform::Linux) => system_host::HostSystem::Linux,
            None => system_host::HostSystem::Other,
        }
    }

    async fn copy(&mut self, content: system_host::Clip, concealed: bool) -> Result<(), String> {
        let system = self.system()?;
        let clip = clip_from(content)?;
        self.hosted(off_thread(
            move || system.copy(&clip, concealed),
            || Err(failed()),
        ))
        .await
    }

    async fn read_clipboard(&mut self) -> Result<Option<system_host::Clip>, String> {
        let system = self.system()?;
        let read = self
            .hosted(off_thread(
                move || system.read_clipboard(),
                || Err(failed()),
            ))
            .await?;
        Ok(read.map(|clip| match clip {
            Clip::Text(text) => system_host::Clip::Text(text),
            Clip::File(path) => system_host::Clip::File(path.to_string_lossy().into_owned()),
        }))
    }

    async fn open(&mut self, target: String, application: Option<String>) -> Result<(), String> {
        let system = self.system()?;
        if target.trim().is_empty() {
            return Err("Nothing to open was named".into());
        }
        let application = application.filter(|application| !application.trim().is_empty());
        self.hosted(off_thread(
            move || system.open(&target, application.as_deref()),
            || Err(failed()),
        ))
        .await
    }

    async fn reveal(&mut self, path: String) -> Result<(), String> {
        let system = self.system()?;
        let path = host_system::absolute(&path)?;
        self.hosted(off_thread(move || system.reveal(&path), || Err(failed())))
            .await
    }

    async fn trash(&mut self, paths: Vec<String>) -> Result<(), Vec<system_host::NotTrashed>> {
        let wire = |not: NotTrashed| system_host::NotTrashed {
            path: not.path.to_string_lossy().into_owned(),
            reason: not.reason,
        };
        let system = match self.system() {
            Ok(system) => system,
            Err(why) => {
                return Err(paths
                    .into_iter()
                    .map(|path| system_host::NotTrashed {
                        path,
                        reason: why.clone(),
                    })
                    .collect());
            }
        };
        let (kept, mut refused) = host_system::trashable(&paths);
        if !kept.is_empty() {
            let failed_for = kept.clone();
            let not_moved = self
                .hosted(off_thread(
                    move || system.trash(&kept),
                    move || {
                        failed_for
                            .into_iter()
                            .map(|path| NotTrashed {
                                path,
                                reason: failed(),
                            })
                            .collect()
                    },
                ))
                .await;
            refused.extend(not_moved);
        }
        if refused.is_empty() {
            Ok(())
        } else {
            Err(refused.into_iter().map(wire).collect())
        }
    }

    async fn paste(&mut self, content: system_host::Clip) -> Result<(), system_host::SystemError> {
        let system = self.system().map_err(system_host::SystemError::Failed)?;
        let clip = clip_from(content).map_err(system_host::SystemError::Failed)?;
        // Asked first, so that a paste this system cannot make leaves the
        // window and the clipboard as they were.
        let asked = system.clone();
        self.hosted(off_thread(
            move || asked.can_paste(),
            || Err(failed_error()),
        ))
        .await
        .map_err(wire_error)?;
        // The application that was in front can only come back once
        // Pane's window has gone.
        self.close_for_paste().map_err(wire_error)?;
        self.hosted(off_thread(
            move || host_system::paste(system.as_ref(), &clip),
            || Err(failed_error()),
        ))
        .await
        .map_err(wire_error)
    }

    async fn front_application(
        &mut self,
    ) -> Result<Option<system_host::FrontApp>, system_host::SystemError> {
        let system = self.system().map_err(system_host::SystemError::Failed)?;
        let front = self
            .hosted(off_thread(
                move || system.front_application(),
                || Err(failed_error()),
            ))
            .await
            .map_err(wire_error)?;
        Ok(front.map(|front| system_host::FrontApp {
            name: front.name,
            icon: front.icon,
        }))
    }

    async fn selected_text(&mut self) -> Result<Option<String>, system_host::SystemError> {
        let system = self.system().map_err(system_host::SystemError::Failed)?;
        let selected = self
            .hosted(off_thread(
                move || system.selected_text(),
                || Err(failed_error()),
            ))
            .await
            .map_err(wire_error)?;
        match selected {
            // An empty selection is no selection.
            Some(text) if text.is_empty() => Ok(None),
            Some(text) if text.len() > MAX_CLIPBOARD_TEXT => {
                Err(system_host::SystemError::Failed(format!(
                    "The selected text is longer than the {} MiB Pane gives a command",
                    MAX_CLIPBOARD_TEXT / (1024 * 1024)
                )))
            }
            selected => Ok(selected),
        }
    }
}
