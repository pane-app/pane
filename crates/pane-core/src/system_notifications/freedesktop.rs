//! System notifications on Linux (see the parent module): the freedesktop
//! notification service (`org.freedesktop.Notifications`) on the D-Bus
//! session bus, which GNOME, KDE Plasma, Xfce, Cinnamon, MATE and the
//! standalone daemons (dunst, mako, notification-daemon) all serve, on X11
//! and Wayland alike, through zbus's blocking API.
//!
//! The notification is Pane's (`app_name` [`APP_NAME`], the desktop entry
//! `pane.desktop` the AppImage install writes, so the desktop shows Pane's
//! icon and its notification settings apply), silent (the
//! `suppress-sound` hint: sound is the ringing sound's job), its summary
//! the title, and its body the message with the extension's title on its
//! last line, as Windows' attribution line shows it. A service that reads
//! markup in the body (the `body-markup` capability) gets the body escaped,
//! so it stays plain text.
//!
//! The connection is made at the first post and kept; a post that fails
//! drops it, so the next one connects again (a desktop whose notification
//! service or session bus restarted). No session bus, and no service
//! answering on it, answer `not-available` saying so, never a silent
//! nothing. The service's own Do Not Disturb keeps the notification quiet
//! and the post still succeeds, as on Windows.
//!
//! The click is the service's `default` action (offered only when the
//! service has the `actions` capability): its `ActionInvoked` signal comes
//! to a thread of the connection's, which calls what the click does. The
//! last 32 notifications' clicks are kept, as on Windows; a click on an
//! older one does nothing.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use zbus::blocking::{Connection, Proxy, connection};
use zbus::zvariant::Value;

use super::{APP_NAME, Clicked, Notice, Notifier, NotifyError};

/// The service's well-known name, which is also its interface's.
const SERVICE: &str = "org.freedesktop.Notifications";

/// The service's object.
const PATH: &str = "/org/freedesktop/Notifications";

/// The desktop entry the AppImage install writes (`pane.desktop`), whose
/// name and icon the desktop shows the notification under.
const DESKTOP_ENTRY: &str = "pane";

/// How many posted notifications' clicks are kept.
const KEPT: usize = 32;

/// How long a call to the service may take before Pane gives up on it:
/// the post is a guest's call, which must not hang on a stuck desktop.
const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// The freedesktop notification service, on the session bus or a bus a
/// test names.
pub(crate) struct Freedesktop {
    /// The bus's address; `None` for the user's session bus.
    address: Option<String>,
    /// The service, once a post reached it, until a post fails.
    service: Mutex<Option<Service>>,
    /// The ids of the notifications posted last, with what their click
    /// does, newest at the back.
    clicks: Arc<Mutex<VecDeque<(u32, Clicked)>>>,
}

/// A connection to the service, and what it said it can do.
struct Service {
    connection: Connection,
    proxy: Proxy<'static>,
    /// It offers actions, so a click reaches Pane.
    actions: bool,
    /// It reads markup in a notification's body.
    markup: bool,
}

impl Freedesktop {
    /// The service on the user's session bus.
    pub(crate) fn session() -> Freedesktop {
        Freedesktop::on(None)
    }

    fn on(address: Option<String>) -> Freedesktop {
        Freedesktop {
            address,
            service: Mutex::new(None),
            clicks: Arc::default(),
        }
    }

    /// Connects to the bus, asks the service what it can do, and listens
    /// for its clicks when it offers actions.
    fn connect(&self) -> Result<Service, NotifyError> {
        let builder = match &self.address {
            None => connection::Builder::session(),
            Some(address) => connection::Builder::address(address.as_str()),
        };
        let connection = builder
            .and_then(|builder| builder.method_timeout(ANSWER_WITHIN).build())
            .map_err(|error| {
                NotifyError::NotAvailable(format!(
                    "Pane could not reach the desktop's D-Bus session bus, which notifications \
                     go through: {error}"
                ))
            })?;
        let proxy = Proxy::new(&connection, SERVICE, PATH, SERVICE).map_err(explained)?;
        let capabilities: Vec<String> = proxy.call("GetCapabilities", &()).map_err(explained)?;
        let has = |capability: &str| capabilities.iter().any(|offered| offered == capability);
        let service = Service {
            actions: has("actions"),
            markup: has("body-markup"),
            connection,
            proxy,
        };
        if service.actions {
            let invoked = service
                .proxy
                .receive_signal("ActionInvoked")
                .map_err(explained)?;
            let clicks = self.clicks.clone();
            std::thread::Builder::new()
                .name("pane-notification-clicks".into())
                .spawn(move || {
                    // Ends when the connection closes.
                    for message in invoked {
                        let Ok((id, action)) = message.body().deserialize::<(u32, String)>() else {
                            continue;
                        };
                        if action != "default" {
                            continue;
                        }
                        let clicked = lock(&clicks)
                            .iter()
                            .find(|(posted, _)| *posted == id)
                            .map(|(_, clicked)| clicked.clone());
                        if let Some(clicked) = clicked {
                            clicked();
                        }
                    }
                })
                .map_err(|error| {
                    NotifyError::Failed(format!(
                        "Pane could not start the thread that hears notifications' clicks: \
                         {error}"
                    ))
                })?;
        }
        Ok(service)
    }
}

impl Service {
    /// Posts `notice`, answering the id the service gave it.
    fn post(&self, notice: &Notice) -> zbus::Result<u32> {
        let actions: Vec<&str> = if self.actions {
            vec!["default", "Open"]
        } else {
            Vec::new()
        };
        let hints = HashMap::from([
            ("suppress-sound", Value::from(true)),
            ("desktop-entry", Value::from(DESKTOP_ENTRY)),
        ]);
        self.proxy.call(
            "Notify",
            &(
                APP_NAME,
                0u32,
                "",
                notice.title.as_str(),
                body(notice, self.markup),
                actions,
                hints,
                -1i32,
            ),
        )
    }

    /// Closes the connection, which ends the thread hearing its clicks.
    fn close(self) {
        let _ = self.connection.close();
    }
}

impl Notifier for Freedesktop {
    fn notify(&self, notice: Notice) -> Result<(), NotifyError> {
        let mut service = lock(&self.service);
        let connected = match service.take() {
            Some(connected) => connected,
            None => self.connect()?,
        };
        let id = match connected.post(&notice) {
            Ok(id) => id,
            Err(error) => {
                // The next post connects again: the bus or the service may
                // have restarted.
                connected.close();
                return Err(explained(error));
            }
        };
        *service = Some(connected);
        drop(service);
        let mut clicks = lock(&self.clicks);
        clicks.push_back((id, notice.clicked));
        while clicks.len() > KEPT {
            clicks.pop_front();
        }
        Ok(())
    }
}

/// The notification's body: the message, then the extension's title on a
/// line of its own; escaped for a service that reads markup.
fn body(notice: &Notice, markup: bool) -> String {
    let text = match &notice.message {
        Some(message) => format!("{message}\n{}", notice.attribution),
        None => notice.attribution.clone(),
    };
    if !markup {
        return text;
    }
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            character => escaped.push(character),
        }
    }
    escaped
}

/// Why a call to the service failed, as `notify` answers it.
fn explained(error: zbus::Error) -> NotifyError {
    const NO_SERVICE: &str = "No notification service runs on this desktop: nothing answers \
                              org.freedesktop.Notifications on the D-Bus session bus";
    let unknown = |name: &str| {
        matches!(
            name,
            "org.freedesktop.DBus.Error.ServiceUnknown"
                | "org.freedesktop.DBus.Error.NameHasNoOwner"
        )
    };
    match &error {
        zbus::Error::MethodError(name, _, _) if unknown(name.as_str()) => {
            NotifyError::NotAvailable(NO_SERVICE.into())
        }
        zbus::Error::FDO(fdo)
            if matches!(
                **fdo,
                zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)
            ) =>
        {
            NotifyError::NotAvailable(NO_SERVICE.into())
        }
        zbus::Error::MethodError(name, detail, _) => NotifyError::Failed(format!(
            "The desktop's notification service refused the notification: {}",
            detail.as_deref().unwrap_or(name.as_str())
        )),
        zbus::Error::InputOutput(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            NotifyError::NotAvailable(format!(
                "The desktop's notification service did not answer within {} seconds",
                ANSWER_WITHIN.as_secs()
            ))
        }
        _ => NotifyError::Failed(format!(
            "Pane could not reach the desktop's notification service: {error}"
        )),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    //! The notifier against a stand-in notification service on a D-Bus bus
    //! of the test's own (`dbus-daemon`, from the `dbus-daemon` package),
    //! never the user's session: opted in with `PANE_TEST_REAL_ALERTS`,
    //! which CI's Linux test jobs set (`.github/actions/setup`).
    #![allow(clippy::too_many_arguments)]

    use std::io::{BufRead, BufReader};
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use zbus::zvariant::OwnedValue;

    use super::*;

    fn opted_in() -> bool {
        std::env::var_os("PANE_TEST_REAL_ALERTS").is_some()
    }

    /// A D-Bus bus of the test's own, with nothing on it to activate:
    /// `dbus-daemon` with a configuration of its own, its socket in a
    /// temporary folder. Dropping it ends the daemon.
    struct PrivateBus {
        daemon: Child,
        address: String,
        _folder: tempfile::TempDir,
    }

    impl PrivateBus {
        fn start() -> PrivateBus {
            let folder = tempfile::tempdir().unwrap();
            let socket: PathBuf = folder.path().join("bus");
            let config = folder.path().join("bus.conf");
            std::fs::write(
                &config,
                format!(
                    "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration \
                     1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                     <busconfig><type>session</type><listen>unix:path={}</listen>\
                     <auth>EXTERNAL</auth><policy context=\"default\">\
                     <allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/>\
                     <allow own=\"*\"/></policy></busconfig>\n",
                    socket.display()
                ),
            )
            .unwrap();
            let mut daemon = Command::new("dbus-daemon")
                .arg(format!("--config-file={}", config.display()))
                .args(["--nofork", "--print-address=1"])
                .stdout(Stdio::piped())
                .spawn()
                .expect("dbus-daemon starts a bus of the test's own (the dbus-daemon package)");
            let mut address = String::new();
            BufReader::new(daemon.stdout.take().unwrap())
                .read_line(&mut address)
                .unwrap();
            assert!(address.starts_with("unix:"), "dbus-daemon said {address:?}");
            PrivateBus {
                daemon,
                address: address.trim().to_owned(),
                _folder: folder,
            }
        }
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
        }
    }

    /// What the stand-in service was asked to show.
    #[derive(Debug)]
    struct Shown {
        app_name: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        suppress_sound: Option<bool>,
        desktop_entry: Option<String>,
    }

    /// A notification service that shows nothing and records each post.
    #[derive(Clone, Default)]
    struct StandIn {
        shown: Arc<Mutex<Vec<Shown>>>,
        capabilities: Vec<String>,
    }

    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl StandIn {
        fn notify(
            &self,
            app_name: String,
            _replaces_id: u32,
            _app_icon: String,
            summary: String,
            body: String,
            actions: Vec<String>,
            hints: HashMap<String, OwnedValue>,
            _expire_timeout: i32,
        ) -> u32 {
            let suppress_sound = match hints.get("suppress-sound").map(|value| &**value) {
                Some(Value::Bool(suppress)) => Some(*suppress),
                _ => None,
            };
            let desktop_entry = match hints.get("desktop-entry").map(|value| &**value) {
                Some(Value::Str(entry)) => Some(entry.to_string()),
                _ => None,
            };
            let mut shown = lock(&self.shown);
            shown.push(Shown {
                app_name,
                summary,
                body,
                actions,
                suppress_sound,
                desktop_entry,
            });
            shown.len() as u32
        }

        fn get_capabilities(&self) -> Vec<String> {
            self.capabilities.clone()
        }
    }

    /// The stand-in, offering `capabilities`, serving on `bus`.
    fn serve(bus: &PrivateBus, capabilities: &[&str]) -> (StandIn, Connection) {
        let stand_in = StandIn {
            capabilities: capabilities.iter().map(|&offered| offered.into()).collect(),
            ..StandIn::default()
        };
        let connection = connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at(PATH, stand_in.clone())
            .unwrap()
            .build()
            .unwrap();
        (stand_in, connection)
    }

    fn notice(message: Option<&str>, clicked: Clicked) -> Notice {
        Notice {
            title: "Tea <ready>".into(),
            message: message.map(str::to_owned),
            attribution: "Timers".into(),
            clicked,
        }
    }

    #[test]
    fn the_notification_reaches_the_service_and_its_click_comes_back() {
        if !opted_in() {
            return;
        }
        let bus = PrivateBus::start();
        let (stand_in, service) = serve(&bus, &["actions", "body"]);
        let notifier = Freedesktop::on(Some(bus.address.clone()));
        let clicks = Arc::new(AtomicUsize::new(0));
        let counted = clicks.clone();
        let clicked: Clicked = Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });

        notifier
            .notify(notice(Some("Steeped & done"), clicked))
            .unwrap();

        {
            let shown = lock(&stand_in.shown);
            let [shown] = &shown[..] else {
                panic!("the service was asked to show {shown:?}");
            };
            assert_eq!(shown.app_name, APP_NAME);
            assert_eq!(shown.summary, "Tea <ready>");
            assert_eq!(shown.body, "Steeped & done\nTimers");
            assert_eq!(shown.actions, ["default", "Open"]);
            assert_eq!(shown.suppress_sound, Some(true));
            assert_eq!(shown.desktop_entry.as_deref(), Some("pane"));
            eprintln!("the stand-in service was asked to show {shown:?}");
        }

        // Another action, and a click on a notification Pane did not
        // post, do nothing; the default action on Pane's is its click.
        service
            .emit_signal(
                None::<&str>,
                PATH,
                SERVICE,
                "ActionInvoked",
                &(1u32, "Snooze"),
            )
            .unwrap();
        service
            .emit_signal(
                None::<&str>,
                PATH,
                SERVICE,
                "ActionInvoked",
                &(7u32, "default"),
            )
            .unwrap();
        service
            .emit_signal(
                None::<&str>,
                PATH,
                SERVICE,
                "ActionInvoked",
                &(1u32, "default"),
            )
            .unwrap();
        let began = Instant::now();
        while clicks.load(Ordering::SeqCst) == 0 {
            assert!(
                began.elapsed() < Duration::from_secs(10),
                "the click did not come back"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(clicks.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_service_that_reads_markup_gets_plain_text_escaped() {
        if !opted_in() {
            return;
        }
        let bus = PrivateBus::start();
        let (stand_in, _service) = serve(&bus, &["body", "body-markup"]);
        let notifier = Freedesktop::on(Some(bus.address.clone()));

        notifier.notify(notice(None, Arc::new(|| {}))).unwrap();
        notifier
            .notify(notice(Some("<b>1 & 2</b>"), Arc::new(|| {})))
            .unwrap();

        let shown = lock(&stand_in.shown);
        assert_eq!(shown[0].body, "Timers");
        assert_eq!(shown[1].body, "&lt;b&gt;1 &amp; 2&lt;/b&gt;\nTimers");
        // No actions offered: none asked for.
        assert!(shown[1].actions.is_empty(), "{:?}", shown[1].actions);
    }

    #[test]
    fn no_service_on_the_bus_is_not_available_and_a_later_one_is_reached() {
        if !opted_in() {
            return;
        }
        let bus = PrivateBus::start();
        let notifier = Freedesktop::on(Some(bus.address.clone()));

        let answer = notifier.notify(notice(None, Arc::new(|| {})));
        eprintln!("with no notification service the notifier answered {answer:?}");
        match answer {
            Err(NotifyError::NotAvailable(why)) => {
                assert!(why.contains("No notification service"), "{why}")
            }
            other => panic!("with no service the notifier answered {other:?}"),
        }

        // A service that starts afterwards is reached by the next post.
        let (stand_in, _service) = serve(&bus, &["body"]);
        notifier.notify(notice(None, Arc::new(|| {}))).unwrap();
        assert_eq!(lock(&stand_in.shown).len(), 1);
    }

    #[test]
    fn no_bus_is_not_available() {
        let notifier = Freedesktop::on(Some("unix:path=/nonexistent/pane-test-bus".into()));
        match notifier.notify(notice(None, Arc::new(|| {}))) {
            Err(NotifyError::NotAvailable(why)) => {
                assert!(why.contains("D-Bus session bus"), "{why}")
            }
            other => panic!("with no bus the notifier answered {other:?}"),
        }
    }

    #[test]
    fn the_body_is_the_message_then_the_attribution() {
        let plain = notice(Some("a < b"), Arc::new(|| {}));
        assert_eq!(body(&plain, false), "a < b\nTimers");
        assert_eq!(body(&plain, true), "a &lt; b\nTimers");
        assert_eq!(body(&notice(None, Arc::new(|| {})), false), "Timers");
    }
}
