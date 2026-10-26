//! The guest's side of `alerts.notify` (`wit/alerts.wit`, ADR 0053,
//! #555): a short host call that hands the notification, with its
//! [`Caller`](crate::feedback::Caller), to the launcher's host functions,
//! which name the package, check the click and the limit, and post it
//! through the launcher's notifier (`crate::system_notifications`). It
//! answers once the system took the notification, from any call, with or
//! without a window. A runtime no launcher drives (tests of the runtime
//! alone) answers that this Pane posts no system notifications.

use super::{GuestState, alerts_host};
use crate::system_notifications::{GivenClick, GivenNotification, NotifyError};

impl GuestState {
    /// Posts `notification` through the launcher, or says why not. The
    /// caller checked first that the instance's code is not stopped.
    pub(super) fn post_notification(
        &self,
        notification: alerts_host::Notification,
    ) -> Result<(), alerts_host::AlertsError> {
        let _host = self.host();
        let Some(host) = self.host_functions() else {
            return Err(alerts_host::AlertsError::NotAvailable(
                "This Pane posts no system notifications for extensions".into(),
            ));
        };
        let given = GivenNotification {
            title: notification.title,
            message: notification.message,
            click: notification.click.map(|click| GivenClick {
                command: click.command,
                context: click.context,
            }),
        };
        host.notify(&self.caller(), given).map_err(wire_error)
    }
}

/// `error` as the WIT carries it.
fn wire_error(error: NotifyError) -> alerts_host::AlertsError {
    match error {
        NotifyError::NotAvailable(why) => alerts_host::AlertsError::NotAvailable(why),
        NotifyError::Limit(why) => alerts_host::AlertsError::Limit(why),
        NotifyError::Failed(why) => alerts_host::AlertsError::Failed(why),
    }
}
