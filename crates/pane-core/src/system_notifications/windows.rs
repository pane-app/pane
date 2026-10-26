//! System notifications on Windows (see the parent module): a toast
//! through WinRT's `ToastNotificationManager`, posted under Pane's
//! AppUserModelID ([`APP_ID`]) with the notifier made for it.
//!
//! A toast from a desktop program that is not packaged shows only when
//! Windows knows the program's AppUserModelID: Pane sets it on its own
//! process ([`adopt_app_id`]), the setup's Start menu shortcut carries it
//! (`scripts/setup-windows.iss`), and Pane registers it for the user
//! under `HKCU\Software\Classes\AppUserModelId` ([`register_app_id`]),
//! the documented way for a program with no such shortcut — a portable
//! Pane, one `install.ps1` installed, a development build. Before each
//! post the notifier checks that registration, then asks Windows' setting
//! for the id: an id not registered, and notifications turned off, answer
//! `not-available` saying so, never a silent nothing. Pane checks the
//! registration itself because Windows' setting cannot tell an unknown
//! id: with notifications turned off for the user it answers that for
//! every id, known or not.
//!
//! The click is the toast's `Activated` event, which Windows raises in
//! this process while Pane runs; the toasts posted last are kept, so
//! their handlers live as long as the user may still click them in the
//! notification center. A click Windows delivers to no running Pane
//! starts Pane's program, which shows the running one as a second start
//! does (ADR 0052).

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use ::windows::Data::Xml::Dom::XmlDocument;
use ::windows::Foundation::TypedEventHandler;
use ::windows::UI::Notifications::{
    NotificationSetting, ToastNotification, ToastNotificationManager, ToastNotifier,
};
use ::windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use ::windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegSetValueExW,
};
use ::windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use ::windows::core::HSTRING;

use super::{APP_ID, APP_NAME, Notice, Notifier, NotifyError, toast_xml};

/// Where a program with no packaged identity registers its
/// AppUserModelID for the user, under `HKEY_CURRENT_USER`.
const REGISTERED: &str = r"Software\Classes\AppUserModelId";

/// How many posted toasts are kept, with their click handlers.
const KEPT: usize = 32;

/// Windows' toasts, under one AppUserModelID.
pub(crate) struct Toasts {
    app_id: HSTRING,
    /// The key its registration is under, beneath `HKEY_CURRENT_USER`:
    /// [`REGISTERED`], or a test's scratch key.
    registered_under: String,
    /// The toasts posted last, newest at the back.
    kept: Mutex<VecDeque<ToastNotification>>,
}

impl Toasts {
    pub(crate) fn new(app_id: &str) -> Toasts {
        Toasts::under(REGISTERED, app_id)
    }

    /// Toasts under `app_id`, registered beneath `registered_under`.
    fn under(registered_under: &str, app_id: &str) -> Toasts {
        Toasts {
            app_id: HSTRING::from(app_id),
            registered_under: registered_under.to_owned(),
            kept: Mutex::new(VecDeque::new()),
        }
    }

    /// Why Windows shows nothing under this id: it does not know it, as
    /// `error` says, or since it is not registered.
    fn unknown(&self, error: Option<::windows::core::Error>) -> NotifyError {
        let why = error.map_or_else(
            || "it is not registered".to_owned(),
            |error| error.message(),
        );
        NotifyError::NotAvailable(format!(
            "Windows does not know Pane's AppUserModelID ({}), so it shows no notification \
             from Pane: {why}",
            self.app_id
        ))
    }

    /// Whether the id is registered for the user, as Pane registers it at
    /// each start ([`register_app_id`]) and the setup does.
    fn registered(&self) -> bool {
        let path = HSTRING::from(format!(r"{}\{}", self.registered_under, self.app_id));
        let mut key = HKEY::default();
        // SAFETY: `key` is filled in by the call when it succeeds, and
        // closed below.
        let answer =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &path, None, KEY_QUERY_VALUE, &mut key) };
        if answer != ERROR_SUCCESS {
            return false;
        }
        // SAFETY: the handle the open returned, closed once.
        let _ = unsafe { RegCloseKey(key) };
        true
    }

    /// The notifier of this id, once the id is registered and Windows
    /// says it shows its toasts: notifications turned off say where,
    /// before anything is posted.
    fn notifier(&self) -> Result<ToastNotifier, NotifyError> {
        if !self.registered() {
            return Err(self.unknown(None));
        }
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&self.app_id)
            .map_err(|error| self.unknown(Some(error)))?;
        let setting = notifier
            .Setting()
            .map_err(|error| self.unknown(Some(error)))?;
        match turned_off(setting) {
            Some(why) => Err(NotifyError::NotAvailable(why.into())),
            None => Ok(notifier),
        }
    }
}

impl Notifier for Toasts {
    fn notify(&self, notice: Notice) -> Result<(), NotifyError> {
        let notifier = self.notifier()?;
        let toast = toast(&notice).map_err(|error| {
            NotifyError::Failed(format!(
                "Windows would not make the notification: {}",
                error.message()
            ))
        })?;
        notifier.Show(&toast).map_err(|error| {
            NotifyError::Failed(format!(
                "Windows would not show the notification: {}",
                error.message()
            ))
        })?;
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        kept.push_back(toast);
        while kept.len() > KEPT {
            kept.pop_front();
        }
        Ok(())
    }
}

/// Why notifications are turned off, as `setting` says; `None` when they
/// are on.
fn turned_off(setting: NotificationSetting) -> Option<&'static str> {
    match setting {
        NotificationSetting::Enabled => None,
        NotificationSetting::DisabledForApplication => Some(
            "Notifications are turned off for Pane in Windows' Settings (System > Notifications)",
        ),
        NotificationSetting::DisabledForUser => {
            Some("Notifications are turned off in Windows' Settings (System > Notifications)")
        }
        NotificationSetting::DisabledByGroupPolicy => {
            Some("Notifications are turned off by a policy of this computer")
        }
        _ => Some("Windows does not let Pane post notifications"),
    }
}

/// The toast for `notice`, raising its click on `Activated`.
fn toast(notice: &Notice) -> ::windows::core::Result<ToastNotification> {
    let content = XmlDocument::new()?;
    content.LoadXml(&HSTRING::from(toast_xml(notice)))?;
    let toast = ToastNotification::CreateToastNotification(&content)?;
    let clicked = notice.clicked.clone();
    toast.Activated(&TypedEventHandler::new(move |_, _| {
        clicked();
        Ok(())
    }))?;
    Ok(toast)
}

/// Sets Pane's AppUserModelID ([`APP_ID`]) on this process, before any
/// window is shown: the id its notifications are posted under, and the one
/// the setup's shortcut carries, so the taskbar counts Pane's windows as
/// the shortcut's.
pub fn adopt_app_id() -> Result<(), String> {
    // SAFETY: the id is a valid string for the call, which copies it.
    unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(APP_ID)) }
        .map_err(|error| format!("Pane's AppUserModelID could not be set: {error}"))
}

/// Registers Pane's AppUserModelID ([`APP_ID`]) for the user, with the
/// name ([`APP_NAME`]) and the icon (a PNG file, when given) Windows shows
/// its notifications under: what lets a Pane with no shortcut carrying the
/// id post them. Writing it again is harmless; nothing but the id's own
/// key is written.
pub fn register_app_id(icon: Option<&Path>) -> Result<(), String> {
    register_under(REGISTERED, APP_ID, APP_NAME, icon)
}

/// [`register_app_id`] under `parent` (beneath `HKEY_CURRENT_USER`), so
/// the tests register under a scratch key of their own.
fn register_under(parent: &str, id: &str, name: &str, icon: Option<&Path>) -> Result<(), String> {
    let mut key = HKEY::default();
    // SAFETY: `key` is a handle the call fills in when it succeeds, closed
    // below.
    let answer = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(format!(r"{parent}\{id}")),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    checked(answer, "Pane's AppUserModelID could not be registered")?;
    let mut result = set_string(key, "DisplayName", name);
    if let (Ok(()), Some(icon)) = (&result, icon) {
        result = set_string(key, "IconUri", &icon.to_string_lossy());
    }
    // SAFETY: the handle the create returned, closed once here. The
    // close's own answer is advisory: nothing can be done about it.
    let _ = unsafe { RegCloseKey(key) };
    result
}

/// Gives the open `key` the string value `name`.
fn set_string(key: HKEY, name: &str, text: &str) -> Result<(), String> {
    let mut data: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    data.extend_from_slice(&[0, 0]);
    // SAFETY: `key` is open for writing, and `data` is a plain byte buffer
    // (a null-terminated UTF-16 string) the call copies before returning.
    let answer = unsafe { RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(&data)) };
    checked(answer, "Pane's AppUserModelID could not be registered")
}

/// `answer`, explained as `what` when it is not a success.
fn checked(answer: WIN32_ERROR, what: &str) -> Result<(), String> {
    if answer == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("{what}: error {}", answer.0))
    }
}

#[cfg(test)]
mod tests {
    use ::windows::Win32::System::Registry::{RRF_RT_REG_SZ, RegDeleteTreeW, RegGetValueW};

    use super::*;

    /// A scratch key of this test's own under `HKEY_CURRENT_USER`, standing
    /// in for `Software\Classes\AppUserModelId`, removed when dropped:
    /// Windows' own registrations are never touched.
    struct Scratch(String);

    impl Scratch {
        fn new() -> Scratch {
            Scratch(format!(
                r"Software\pane-test-app-id-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ))
        }

        /// The string value `name` of the scratch key's `id`, if any.
        fn value(&self, id: &str, name: &str) -> Option<String> {
            let key = HSTRING::from(format!(r"{}\{id}", self.0));
            let name = HSTRING::from(name);
            let mut buffer = vec![0u16; 1024];
            let mut size = (buffer.len() * 2) as u32;
            // SAFETY: `buffer` holds `size` bytes, which the call writes at
            // most.
            let answer = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    &key,
                    &name,
                    RRF_RT_REG_SZ,
                    None,
                    Some(buffer.as_mut_ptr().cast()),
                    Some(&mut size),
                )
            };
            if answer != ERROR_SUCCESS {
                return None;
            }
            buffer.truncate((size as usize / 2).saturating_sub(1));
            Some(String::from_utf16_lossy(&buffer))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // SAFETY: removes the scratch key this test made, and nothing
            // else.
            let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(&self.0)) };
        }
    }

    #[test]
    fn the_app_id_is_registered_with_its_name_and_icon() {
        let scratch = Scratch::new();
        let icon = Path::new(r"C:\Users\u\AppData\Local\Pane\cache\notification-icon.png");
        let id = "PaneApp.Pane.Test";
        assert_eq!(register_under(&scratch.0, id, "Pane", Some(icon)), Ok(()));
        assert_eq!(scratch.value(id, "DisplayName").as_deref(), Some("Pane"));
        assert_eq!(
            scratch.value(id, "IconUri").as_deref(),
            Some(icon.to_str().unwrap())
        );
        // Again, as each start does: the same values, nothing else.
        assert_eq!(register_under(&scratch.0, id, "Pane", None), Ok(()));
        assert_eq!(scratch.value(id, "DisplayName").as_deref(), Some("Pane"));
    }

    #[test]
    fn notifications_turned_off_say_where() {
        assert_eq!(turned_off(NotificationSetting::Enabled), None);
        assert!(
            turned_off(NotificationSetting::DisabledForApplication)
                .unwrap()
                .contains("turned off for Pane")
        );
        assert!(
            turned_off(NotificationSetting::DisabledByGroupPolicy)
                .unwrap()
                .contains("policy")
        );
    }

    /// An id not registered answers `not-available` saying so, whatever
    /// Windows' setting would say: what a Pane whose id is not registered
    /// is told. Registered, Windows' setting answers, and the id is no
    /// longer the reason. Only the setting is asked, of an id of this
    /// test's own, so nothing is ever posted.
    #[test]
    fn an_unregistered_app_id_is_not_available() {
        let scratch = Scratch::new();
        let id = "PaneApp.Pane.Test";
        let toasts = Toasts::under(&scratch.0, id);
        match toasts.notifier() {
            Err(NotifyError::NotAvailable(why)) => assert_eq!(
                why,
                "Windows does not know Pane's AppUserModelID (PaneApp.Pane.Test), so it shows \
                 no notification from Pane: it is not registered"
            ),
            Err(other) => panic!("an unregistered id answered {other:?}"),
            Ok(_) => panic!("an unregistered id would post"),
        }
        register_under(&scratch.0, id, "Pane", None).unwrap();
        assert!(toasts.registered());
        if let Err(NotifyError::NotAvailable(why)) = toasts.notifier() {
            assert!(!why.contains("it is not registered"), "{why}");
        }
    }
}
