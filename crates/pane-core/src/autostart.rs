//! Launch at login: the registration that starts Pane when the user logs
//! in, which Pane offers to manage from Settings' General page.
//!
//! The user's own choice is a host setting, kept in the settings record
//! beside the appearance preferences (see `host_settings` and the window
//! layer's settings entity); this module is the system's half — one small
//! trait, [`Autostart`], with one adapter per system, chosen by
//! [`native`]:
//!
//! - Windows: the `Run` key under `HKEY_CURRENT_USER`, the per-user
//!   startup list the shell starts at login (`windows`);
//! - macOS: the login item ServiceManagement's `SMAppService` registers
//!   for the application bundle Pane runs from (`macos`);
//! - Linux: the XDG autostart convention, a desktop entry in the
//!   configuration folder's `autostart` directory (`linux`).
//!
//! None of these needs administrator rights: each is the user's own
//! startup list, changed for this user only, and Pane registers no other
//! kind of system change.
//!
//! The choice, the registration and the platform's ability are kept
//! apart on purpose. A preference that is on is not proof the platform
//! will start Pane: the adapter reports what the system actually holds
//! ([`Autostart::registered`], including macOS's registration that still
//! needs the user's approval), and an adapter that cannot manage the
//! registration here — an unsupported platform, a development build whose
//! program is not the installed application, a macOS older than the API —
//! says so through [`Autostart::unavailable`] instead of failing every
//! call. The window layer reconciles the choice with the registration at
//! start (see the settings entity): a missing or stale registration is
//! repaired to what the user chose, and a registration the user did not
//! choose is removed — a disabled choice is never silently enabled.
//!
//! The adapters are also kept honest about the *limits* of the
//! convention they use: registering is idempotent (one value, one file,
//! one login item — repeated changes replace, never accumulate), and the
//! Linux entry follows the freedesktop autostart convention, which not
//! every desktop environment honors; the page says so rather than
//! promising all desktops behave alike.

use std::sync::Arc;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::XdgAutostart;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::MacLogin;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsLogin;

/// The registration the platform reports for Pane's login integration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Registration {
    /// The platform starts Pane at login.
    Enabled,
    /// Pane is registered, but the platform still needs the user's own
    /// approval before it starts Pane — macOS's login items, which the
    /// user must allow in System Settings. Pane cannot give that approval
    /// for the user.
    NeedsApproval,
    /// The platform does not start Pane at login.
    #[default]
    Disabled,
}

impl Registration {
    /// Whether this registration will start Pane at login, approval
    /// notwithstanding: the *effective* registration, as distinct from
    /// the user's preference.
    pub fn registered(&self) -> bool {
        !matches!(self, Registration::Disabled)
    }
}

/// The system's login integration: the registration that starts Pane when
/// the user logs in, read and changed for this user only.
///
/// Implementations must be idempotent: Pane holds one registration, so
/// [`Autostart::enable`] replaces whatever registration of Pane's is
/// already there rather than adding another, and [`Autostart::disable`]
/// on a system without Pane's registration succeeds as the no-op it is.
pub trait Autostart: Send + Sync + 'static {
    /// Why the registration cannot be managed here at all — an
    /// unsupported platform, a development build, an operating system
    /// without the API — phrased for the Settings page, which explains it
    /// instead of offering a toggle that cannot work. `None` when the
    /// registration can be managed.
    fn unavailable(&self) -> Option<String>;

    /// The registration the system reports for Pane now: the state the
    /// platform actually holds, as distinct from the user's preference.
    /// `Err` with the problem when the system cannot be asked.
    fn registered(&self) -> Result<Registration, String>;

    /// Registers Pane to start at login, replacing Pane's earlier
    /// registration rather than adding another. The registration the
    /// change left — an approval may still be pending — is returned;
    /// `Err` with the problem when the system refused.
    fn enable(&self) -> Result<Registration, String>;

    /// Removes Pane's registration, so Pane no longer starts at login;
    /// a system without Pane's registration is a success. `Err` with the
    /// problem when the system refused.
    fn disable(&self) -> Result<Registration, String>;
}

/// This system's adapter. Where the running program is not an application
/// the user could meaningfully start at login — a development build, whose
/// program is build output rather than an installed application — the
/// adapter explains that instead of registering build output as the
/// startup program; the Settings page shows the limitation.
pub fn native() -> Arc<dyn Autostart> {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        if cfg!(debug_assertions) {
            return Arc::new(Unavailable(
                "Not available in this development build: Pane registers itself at login only as \
                 the installed application, and this program is a development build's output"
                    .into(),
            ));
        }
    }
    #[cfg(target_os = "linux")]
    {
        match XdgAutostart::new() {
            Ok(adapter) => Arc::new(adapter),
            Err(reason) => Arc::new(Unavailable(format!("Not available: {reason}"))),
        }
    }
    #[cfg(target_os = "macos")]
    {
        match MacLogin::new() {
            Ok(adapter) => Arc::new(adapter),
            Err(reason) => Arc::new(Unavailable(format!("Not available: {reason}"))),
        }
    }
    #[cfg(target_os = "windows")]
    {
        match WindowsLogin::new() {
            Ok(adapter) => Arc::new(adapter),
            Err(reason) => Arc::new(Unavailable(format!("Not available: {reason}"))),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Arc::new(Unavailable(format!(
            "Not available on this system ({}): Pane starts at login on Windows, macOS and Linux",
            std::env::consts::OS
        )))
    }
}

/// An adapter for where the registration cannot be managed, saying why.
/// The public shape of the adapters' own construction failures, so the
/// reason is reported the same way wherever it comes from.
pub struct Unavailable(pub String);

impl Autostart for Unavailable {
    fn unavailable(&self) -> Option<String> {
        Some(self.0.clone())
    }

    fn registered(&self) -> Result<Registration, String> {
        Err(self.0.clone())
    }

    fn enable(&self) -> Result<Registration, String> {
        Err(self.0.clone())
    }

    fn disable(&self) -> Result<Registration, String> {
        Err(self.0.clone())
    }
}

/// The adapter of a settings entity that was given none: the window
/// layer's fallback for entities built without a startup step, as the
/// tests' are. It manages nothing, and says so.
pub fn none() -> Arc<dyn Autostart> {
    Arc::new(Unavailable(
        "Not available: this Pane has no login integration".into(),
    ))
}
