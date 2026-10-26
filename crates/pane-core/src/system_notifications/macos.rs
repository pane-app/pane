//! System notifications on macOS (see the parent module): UserNotifications'
//! notification center (`UNUserNotificationCenter`), posted as Pane's,
//! with the extension's title as the notification's subtitle and no
//! sound (the ringing sound's job).
//!
//! macOS gives a notification center only to an app with a bundle
//! identifier: a program run outside `Pane.app` (a development build, the
//! bare `pane` program) has none, and asking for its center would end the
//! process with an Objective-C exception. Pane checks the bundle first and
//! answers `not-available` saying so. In a bundle, the user is asked once
//! whether Pane may post notifications, at the first post; while they have
//! not answered, and after they refused, the post answers `not-available`
//! saying where to turn them on. macOS may also refuse an app whose
//! signature it does not accept (Pane is not signed, and not notarized):
//! its error is passed on as `not-available` too, with what macOS said.
//! What an unsigned Pane gets is recorded in `docs/platforms/macos.md`.
//!
//! The click comes to the center's delegate, Pane's own class
//! ([`Delegate`]), which looks the notification's identifier up among the
//! last 32 posted, as on Windows. The delegate also asks macOS to show a
//! notification while Pane is the frontmost app, which macOS otherwise
//! keeps to the notification center.

use std::collections::VecDeque;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSBundle, NSError, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSettings, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use super::{Clicked, Notice, Notifier, NotifyError};

// Links UserNotifications, whose classes the adapter uses: the bindings
// crate links nothing of its own.
#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}

/// How many posted notifications' clicks are kept.
const KEPT: usize = 32;

/// How long the center may take to answer a question or take a post: the
/// post is a guest's call, which must not hang.
const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// How long the first post waits for the user to answer macOS's question
/// whether Pane may post notifications, before it answers that macOS is
/// still asking.
const ASKED_WITHIN: Duration = Duration::from_secs(3);

/// The notifications posted last, by their request identifier, with what
/// their click does, newest at the back.
#[derive(Default)]
struct Clicks(Mutex<VecDeque<(String, Clicked)>>);

impl Clicks {
    fn keep(&self, identifier: String, clicked: Clicked) {
        let mut kept = self.lock();
        kept.push_back((identifier, clicked));
        while kept.len() > KEPT {
            kept.pop_front();
        }
    }

    fn clicked(&self, identifier: &str) {
        let clicked = self
            .lock()
            .iter()
            .find(|(posted, _)| posted == identifier)
            .map(|(_, clicked)| clicked.clone());
        if let Some(clicked) = clicked {
            clicked();
        }
    }

    fn lock(&self) -> MutexGuard<'_, VecDeque<(String, Clicked)>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements, and `Delegate`
    // does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "PaneNotificationCenterDelegate"]
    #[ivars = Arc<Clicks>]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        // The user clicked a notification: its click runs.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion_handler: &DynBlock<dyn Fn()>,
        ) {
            let identifier = response.notification().request().identifier().to_string();
            self.ivars().clicked(&identifier);
            completion_handler.call(());
        }

        // A notification while Pane is frontmost is shown as a banner and
        // kept in the list, as when another app is.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion_handler.call((
                UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,
            ));
        }
    }
);

impl Delegate {
    fn new(clicks: Arc<Clicks>) -> Retained<Delegate> {
        let this = Self::alloc().set_ivars(clicks);
        // SAFETY: `Delegate` is a class of Pane's own whose ivars are set
        // before the superclass initializes; `NSObject`'s `init` returns a
        // fully initialized object of this class.
        unsafe { msg_send![super(this), init] }
    }
}

/// The clicks of every notification this process posted: the center's
/// delegate, made and set once, keeps them for as long as Pane runs (the
/// center holds its delegate weakly, so the delegate is never released).
fn clicks(center: &UNUserNotificationCenter) -> Arc<Clicks> {
    static CLICKS: OnceLock<Arc<Clicks>> = OnceLock::new();
    CLICKS
        .get_or_init(|| {
            let clicks = Arc::new(Clicks::default());
            let delegate = Delegate::new(clicks.clone());
            center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            std::mem::forget(delegate);
            clicks
        })
        .clone()
}

/// macOS's notification center, for an app with a bundle identifier.
pub(crate) struct UserNotifications {
    /// The next request's number, for its identifier.
    next: AtomicU64,
}

impl UserNotifications {
    pub(crate) fn new() -> UserNotifications {
        UserNotifications {
            next: AtomicU64::new(1),
        }
    }
}

impl Notifier for UserNotifications {
    fn notify(&self, notice: Notice) -> Result<(), NotifyError> {
        if bundle_identifier().is_none() {
            return Err(NotifyError::NotAvailable(
                "macOS posts notifications only for an app, and this Pane runs outside \
                 Pane.app (a development build, or the program on its own)"
                    .into(),
            ));
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let clicks = clicks(&center);
        authorized(&center)?;
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&notice.title));
        if let Some(message) = &notice.message {
            content.setBody(&NSString::from_str(message));
        }
        content.setSubtitle(&NSString::from_str(&notice.attribution));
        // No sound is set: the notification is silent.
        let identifier = format!(
            "pane-{}-{}",
            std::process::id(),
            self.next.fetch_add(1, Ordering::Relaxed)
        );
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&identifier),
            &content,
            None,
        );
        let (answer, answered) = mpsc::channel();
        let handler = RcBlock::new(move |error: *mut NSError| {
            let _ = answer.send(described(error));
        });
        // Kept before the post: a click may come back at once.
        clicks.keep(identifier, notice.clicked);
        center.addNotificationRequest_withCompletionHandler(&request, Some(&handler));
        match answered.recv_timeout(ANSWER_WITHIN) {
            Ok(None) => Ok(()),
            Ok(Some(why)) => Err(NotifyError::Failed(format!(
                "macOS would not post the notification: {why}"
            ))),
            Err(_) => Err(NotifyError::Failed(format!(
                "macOS did not take the notification within {} seconds",
                ANSWER_WITHIN.as_secs()
            ))),
        }
    }
}

/// The bundle identifier of the app this process runs as, if it runs as
/// one: `Pane.app`'s `dev.pane.launcher`.
pub(crate) fn bundle_identifier() -> Option<String> {
    NSBundle::mainBundle()
        .bundleIdentifier()
        .map(|identifier| identifier.to_string())
}

/// Whether the user lets Pane post notifications: asked once, at the first
/// post. `not-available` says what stands in the way.
fn authorized(center: &UNUserNotificationCenter) -> Result<(), NotifyError> {
    let (answer, answered) = mpsc::channel();
    let handler = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        // SAFETY: the center answers a valid settings object, alive for
        // the call.
        let _ = answer.send(unsafe { settings.as_ref() }.authorizationStatus());
    });
    center.getNotificationSettingsWithCompletionHandler(&handler);
    let status = answered.recv_timeout(ANSWER_WITHIN).map_err(|_| {
        NotifyError::NotAvailable(format!(
            "macOS did not say within {} seconds whether Pane may post notifications",
            ANSWER_WITHIN.as_secs()
        ))
    })?;
    match status {
        UNAuthorizationStatus::Authorized
        | UNAuthorizationStatus::Provisional
        | UNAuthorizationStatus::Ephemeral => Ok(()),
        UNAuthorizationStatus::Denied => Err(turned_off()),
        _ => ask(center),
    }
}

/// Asks the user whether Pane may post notifications, waiting a little for
/// their answer.
fn ask(center: &UNUserNotificationCenter) -> Result<(), NotifyError> {
    let (answer, answered) = mpsc::channel();
    let handler = RcBlock::new(move |granted: Bool, error: *mut NSError| {
        let _ = answer.send((granted.as_bool(), described(error)));
    });
    center
        .requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert, &handler);
    match answered.recv_timeout(ASKED_WITHIN) {
        Ok((true, _)) => Ok(()),
        Ok((false, None)) => Err(turned_off()),
        Ok((false, Some(why))) => Err(NotifyError::NotAvailable(format!(
            "macOS does not let Pane post notifications: {why}"
        ))),
        Err(_) => Err(NotifyError::NotAvailable(
            "macOS is asking whether Pane may post notifications; once that is allowed, \
             Pane's notifications show"
                .into(),
        )),
    }
}

fn turned_off() -> NotifyError {
    NotifyError::NotAvailable(
        "Notifications are turned off for Pane in macOS's System Settings (Notifications)".into(),
    )
}

/// What `error` says, when the center answered one.
fn described(error: *mut NSError) -> Option<String> {
    // SAFETY: the center answers null or a valid error, alive for the
    // call.
    let error = unsafe { error.as_ref() }?;
    Some(format!(
        "{} ({} {})",
        error.localizedDescription(),
        error.domain(),
        error.code()
    ))
}

#[cfg(test)]
mod tests {
    //! What the real notification center answers this build, recorded in
    //! the test's output: opted in with `PANE_TEST_REAL_ALERTS`, which CI's
    //! macOS test jobs set (`.github/actions/setup`). Nothing is shown on a
    //! developer's own Mac unless they opt in.

    use std::process::Command;
    use std::time::Instant;

    use super::*;

    fn opted_in() -> bool {
        std::env::var_os("PANE_TEST_REAL_ALERTS").is_some()
    }

    /// Set in the copy of the test program run inside a bundle.
    const IN_BUNDLE: &str = "PANE_TEST_NOTIFICATION_BUNDLE";

    /// The line the copy prints its answer on.
    const ANSWERED: &str = "pane-notification-answer: ";

    fn notice() -> Notice {
        Notice {
            title: "Pane's notification test".into(),
            message: Some("Posted by pane-core's tests on a CI runner".into()),
            attribution: "Tests".into(),
            clicked: Arc::new(|| {}),
        }
    }

    /// The test program on its own has no bundle: the center is never
    /// asked (which would end the process), and the answer says why.
    #[test]
    fn outside_a_bundle_notifications_are_not_available() {
        if !opted_in() || std::env::var_os(IN_BUNDLE).is_some() {
            return;
        }
        assert_eq!(bundle_identifier(), None);
        let answer = UserNotifications::new().notify(notice());
        eprintln!("outside a bundle the notifier answered {answer:?}");
        match answer {
            Err(NotifyError::NotAvailable(why)) => assert!(why.contains("Pane.app"), "{why}"),
            other => panic!("outside a bundle the notifier answered {other:?}"),
        }
    }

    /// Inside a bundle that is not signed for it, as an installed Pane is:
    /// a copy of this test program in an app bundle of its own, with a
    /// bundle identifier, posts, and prints what the real center answered.
    /// Any answer but a crash passes: what it is, is the record.
    #[test]
    fn inside_an_unsigned_bundle_the_center_answers() {
        if !opted_in() {
            return;
        }
        if std::env::var_os(IN_BUNDLE).is_some() {
            let answer = UserNotifications::new().notify(notice());
            println!("{ANSWERED}bundle {:?}: {answer:?}", bundle_identifier());
            return;
        }
        let folder = tempfile::tempdir().unwrap();
        let contents = folder.path().join("PaneNotificationTest.app/Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\n\
             <key>CFBundleExecutable</key><string>tests</string>\n\
             <key>CFBundleIdentifier</key><string>dev.pane.notification-test</string>\n\
             <key>CFBundleName</key><string>Pane notification test</string>\n\
             <key>CFBundlePackageType</key><string>APPL</string>\n\
             <key>LSUIElement</key><true/>\n\
             </dict></plist>\n",
        )
        .unwrap();
        let program = contents.join("MacOS/tests");
        std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
        let began = Instant::now();
        let output = Command::new(&program)
            .args([
                "system_notifications::macos::tests::inside_an_unsigned_bundle_the_center_answers",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(IN_BUNDLE, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let answered = stdout
            .lines()
            .find_map(|line| line.strip_prefix(ANSWERED))
            .unwrap_or_else(|| {
                panic!(
                    "the bundled copy answered nothing ({}): {stdout}\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                )
            });
        eprintln!(
            "inside an unsigned bundle, after {:?}, the notifier answered {answered}",
            began.elapsed()
        );
        assert!(output.status.success(), "{}", output.status);
        assert!(
            answered.contains("dev.pane.notification-test"),
            "{answered}"
        );
    }
}
