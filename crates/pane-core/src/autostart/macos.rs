//! Launch at login on macOS: the login item ServiceManagement's
//! `SMAppService` registers for the application bundle Pane runs from —
//! the API macOS 13 owns for exactly this, which registers the bundle
//! itself (no path of Pane's to keep in step with an update) and keeps
//! the registration to one item. Registering an application whose
//! registration still awaits the user's own approval reports that state
//! (`SMAppServiceStatusRequiresApproval`); Pane cannot click the
//! approval for the user, so it says what is left to do.
//!
//! The API is reached through the Objective-C runtime by name: the class
//! is looked up with `objc_getClass`, so a macOS older than 13 — the
//! deployment target Pane builds for is older — answers "no such class"
//! instead of failing to load, and the adapter reports the limit. The
//! ServiceManagement framework is loaded by path before the lookup, the
//! one thing the runtime does not do by itself.
//!
//! A program that does not run from an application bundle — a
//! development build, a bare binary — has no bundle for the API to
//! register, so the adapter says so: the honest development-install
//! limitation, not an error from deep in the system.

use std::ffi::CStr;
use std::path::Path;

use objc2::msg_send;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, NSObject};
use objc2_foundation::NSError;

use super::{Autostart, Registration};

/// The path of the ServiceManagement framework, whose `SMAppService`
/// class the runtime lookup below needs loaded.
const FRAMEWORK: &CStr =
    c"/System/Library/Frameworks/ServiceManagement.framework/ServiceManagement";

/// `dlopen`'s lazy binding, as the macOS headers define it (the `libc`
/// crate names the other `RTLD` constants but not this one).
const RTLD_LAZY: libc::c_int = 0x1;

/// The statuses `SMAppService` reports, as its header numbers them.
const NOT_REGISTERED: isize = 0;
const ENABLED: isize = 1;
const REQUIRES_APPROVAL: isize = 2;
const NOT_FOUND: isize = 3;

/// The adapter: `SMAppService`'s main-app service, for the bundle this
/// program runs from.
pub struct MacLogin {
    /// The `SMAppService` class, found and kept for the calls below.
    class: &'static AnyClass,
}

impl MacLogin {
    /// The adapter over this program's own bundle. `Err`, phrased for the
    /// Settings page, when the program does not run from an application
    /// bundle, or when the macOS running it has no `SMAppService` to
    /// reach.
    pub fn new() -> Result<MacLogin, String> {
        let exe = std::env::current_exe()
            .map_err(|why| format!("Pane's own program could not be found: {why}"))?;
        if !runs_from_a_bundle(&exe) {
            return Err(
                "Pane runs here as a program on its own, not from the installed application \
                 bundle macOS registers login items for"
                    .into(),
            );
        }
        // Load ServiceManagement so its class is there to find: the
        // runtime looks classes up only among loaded images. The handle
        // is deliberately kept — the framework stays loaded for the
        // process's life, as a linked framework would.
        // SAFETY: `FRAMEWORK` is a plain C string path of a system
        // framework, and the load has no effect beyond making its
        // classes available.
        let _ = unsafe { libc::dlopen(FRAMEWORK.as_ptr(), RTLD_LAZY) };
        let class = AnyClass::get(c"SMAppService")
            .ok_or("this macOS predates the login-items API Pane uses (macOS 13)".to_owned())?;
        Ok(MacLogin { class })
    }
}

impl Autostart for MacLogin {
    fn unavailable(&self) -> Option<String> {
        // The bundle and the class were both checked when the adapter was
        // made; nothing further is refused here.
        None
    }

    fn registered(&self) -> Result<Registration, String> {
        autoreleasepool(|_| {
            let status = self.status()?;
            match status {
                ENABLED => Ok(Registration::Enabled),
                REQUIRES_APPROVAL => Ok(Registration::NeedsApproval),
                NOT_REGISTERED => Ok(Registration::Disabled),
                NOT_FOUND => Err(
                    "macOS reports no login item for Pane to ask about: the application it \
                     registered for is gone"
                        .into(),
                ),
                other => Err(format!(
                    "macOS answered a login-item status Pane does not know: {other}"
                )),
            }
        })
    }

    fn enable(&self) -> Result<Registration, String> {
        autoreleasepool(|_| {
            let service = self.service()?;
            // SAFETY: `service` is the main-app service object, and
            // `registerAndReturnError:` is its registering method; the
            // `_` argument has the macro answer its error parameter, so
            // the call reports `Err` only when the system refused.
            let outcome: Result<(), Retained<NSError>> =
                unsafe { msg_send![&*service, registerAndReturnError: _] };
            outcome.map_err(|error| {
                format!(
                    "macOS would not register Pane: {}",
                    error.localizedDescription()
                )
            })?;
            // Registering may leave the registration awaiting the user's
            // approval: a state the caller must see, not assume away.
            self.registered()
        })
    }

    fn disable(&self) -> Result<Registration, String> {
        autoreleasepool(|_| {
            let service = self.service()?;
            // SAFETY: as `enable`'s call, for the releasing method.
            let outcome: Result<(), Retained<NSError>> =
                unsafe { msg_send![&*service, unregisterAndReturnError: _] };
            outcome.map_err(|error| {
                format!(
                    "macOS would not remove Pane's login item: {}",
                    error.localizedDescription()
                )
            })?;
            self.registered()
        })
    }
}

impl MacLogin {
    /// The main-app service object, whose methods the adapter calls.
    fn service(&self) -> Result<Retained<NSObject>, String> {
        // SAFETY: `self.class` is the `SMAppService` class, and
        // `mainAppService` is its class method answering the service for
        // the application this program runs from — the bundle check at
        // construction said there is one. A `nil` answer (there is no
        // such application) is the `Option` the call is declared to
        // return.
        let service: Option<Retained<NSObject>> = unsafe { msg_send![self.class, mainAppService] };
        service.ok_or_else(|| "macOS would not hand out the login-items service".to_owned())
    }

    /// The status the service reports, as its header's numbers.
    fn status(&self) -> Result<isize, String> {
        let service = self.service()?;
        // SAFETY: `service` is the main-app service object, and `status`
        // is the status-reporting method it answers.
        let status: isize = unsafe { msg_send![&*service, status] };
        Ok(status)
    }
}

/// Whether `exe` runs from an application bundle: the
/// `<bundle>.app/Contents/MacOS/<program>` layout macOS installs, with
/// the `Info.plist` a bundle carries.
fn runs_from_a_bundle(exe: &Path) -> bool {
    let Some(programs) = exe.parent() else {
        return false;
    };
    if !programs.ends_with("MacOS") {
        return false;
    }
    let Some(contents) = programs.parent() else {
        return false;
    };
    if !contents.ends_with("Contents") || !contents.join("Info.plist").is_file() {
        return false;
    }
    contents.parent().is_some_and(|bundle| {
        bundle
            .file_name()
            .is_some_and(|name| name.to_str().is_some_and(|name| name.ends_with(".app")))
    })
}

#[cfg(test)]
mod tests {
    use super::runs_from_a_bundle;

    /// A program in a bundle is one; a program on its own, whatever sits
    /// beside it, is not.
    #[test]
    fn a_program_in_a_bundle_is_one() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Pane.app");
        let programs = bundle.join("Contents/MacOS");
        std::fs::create_dir_all(&programs).unwrap();
        let exe = programs.join("pane");
        assert!(!runs_from_a_bundle(&exe), "no Info.plist yet");
        std::fs::write(bundle.join("Contents/Info.plist"), "{}").unwrap();
        assert!(runs_from_a_bundle(&exe));
        assert!(!runs_from_a_bundle(&programs.join("other/pane")));
    }

    #[test]
    fn a_program_of_its_own_is_not() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!runs_from_a_bundle(&dir.path().join("pane")));
        // Even an app-named folder is not a bundle without the layout.
        std::fs::create_dir_all(dir.path().join("Pane.app/Contents/MacOS")).unwrap();
        std::fs::write(dir.path().join("Pane.app/Contents/Info.plist"), "{}").unwrap();
        assert!(!runs_from_a_bundle(&dir.path().join("pane")));
    }
}
