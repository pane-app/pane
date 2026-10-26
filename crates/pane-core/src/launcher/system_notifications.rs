//! The system notifications commands post (`alerts.notify`, ADR 0053,
//! #555, `crate::system_notifications`). The launcher keeps the notifier
//! and each package's posts of the last minute; the runtime's host call
//! hands the notification here through `feedback::Hosted`. The launcher
//! names the package (its title is the notification's attribution),
//! checks the click and the limit, cuts the texts, and builds what a
//! click does: a user-initiated launch of the package's command, through
//! the path `launch` of `wit/commands.wit` takes, as the user's own action
//! (not an attention launch: the user just clicked, so nothing waits for
//! them to stop typing), or, without a click or when the launch is
//! refused by then, the launcher shown as a second start shows it.

use std::sync::Arc;

use super::{Launcher, launching::check_context, owner};
use crate::feedback::Caller;
use crate::launch::{LaunchRequest, LaunchType};
use crate::system_notifications::{
    Clicked, GivenNotification, MAX_MESSAGE, MAX_TITLE, Notice, Notifier, NotifyError, PER_MINUTE,
    Posted, cut,
};

/// The launcher's notifier, and what each package posted in the last
/// minute.
pub(super) struct Notifications {
    notifier: Arc<dyn Notifier>,
    posted: Posted,
}

impl Default for Notifications {
    fn default() -> Notifications {
        Notifications {
            notifier: crate::system_notifications::none(),
            posted: Posted::default(),
        }
    }
}

impl Launcher {
    /// This launcher's commands posting system notifications through
    /// `notifier`, normally the system's own
    /// ([`crate::system_notifications::native`]). Without one, `notify`
    /// answers that this Pane posts no system notifications.
    pub fn with_notifier(self, notifier: Arc<dyn Notifier>) -> Self {
        self.lock().notifications.notifier = notifier;
        self
    }

    /// Posts `given` for the package of `caller`, or says why not: the
    /// caller is no installed package, the click names no command of its
    /// package or passes a context that is not JSON within the limit, the
    /// package posted [`PER_MINUTE`] in the last minute, or the notifier
    /// refused it.
    pub(super) fn notify(
        &self,
        caller: &Caller,
        given: GivenNotification,
    ) -> Result<(), NotifyError> {
        let mut guard = self.lock();
        let state = &mut *guard;
        let package = owner(&state.packages, &caller.component).ok_or_else(|| {
            NotifyError::Failed(
                "only an installed extension's command posts system notifications".into(),
            )
        })?;
        let title = package.title();
        if let Some(click) = &given.click {
            let declared = package
                .available_commands()
                .into_iter()
                .any(|(registration, _)| registration.manifest_id() == click.command);
            if !declared {
                return Err(NotifyError::Failed(format!(
                    "a notification's click opens a command of {title}'s own, and it has no \
                     command `{}`",
                    click.command
                )));
            }
            if let Some(context) = &click.context {
                check_context(context).map_err(NotifyError::Failed)?;
            }
        }
        let now = state.clock.now();
        if !state
            .notifications
            .posted
            .admit(&package.identity.key(), now)
        {
            return Err(NotifyError::Limit(format!(
                "{title} posted {PER_MINUTE} notifications in the last minute, the most Pane \
                 posts for an extension"
            )));
        }
        let notifier = state.notifications.notifier.clone();
        let request = given.click.map(|click| LaunchRequest {
            caller: caller.component.clone(),
            source: None,
            command: click.command,
            launch_type: LaunchType::UserInitiated,
            arguments: Vec::new(),
            context: click.context,
            // The click is the user's own action, as a hotkey is: the
            // launcher is shown on the command at once, not held as an
            // attention launch waiting for the user to stop typing.
            windowed: true,
        });
        drop(guard);
        notifier.notify(Notice {
            title: cut(&given.title, MAX_TITLE),
            message: given.message.map(|message| cut(&message, MAX_MESSAGE)),
            attribution: title,
            clicked: self.clicked(request),
        })
    }

    /// What clicking a notification does: launches `request` as the user
    /// launching it, or shows the launcher without one, or when the launch
    /// is refused by then (the package was disabled meanwhile). It holds
    /// the launcher weakly, so a notification left in the notification
    /// center keeps nothing running.
    fn clicked(&self, request: Option<LaunchRequest>) -> Clicked {
        let launcher = self.downgrade();
        Arc::new(move || {
            let Some(launcher) = launcher.upgrade() else {
                return;
            };
            if let Some(request) = &request {
                match launcher.launch_from_guest(request.clone()) {
                    Ok(()) => return,
                    Err(why) => crate::diagnostic!(
                        "pane: a notification's click could not open {}: {why}",
                        request.command
                    ),
                }
            }
            launcher.lock().window_wanted = true;
            launcher.changed();
        })
    }
}
