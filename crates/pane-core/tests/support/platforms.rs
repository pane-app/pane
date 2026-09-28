//! The system the tests run on, the two others, and the explanations Pane
//! gives here for something only other systems support. Shared by the test
//! binaries of `pane-core` and `pane`, so every expected explanation comes
//! from one place and from the test binary's own target system.
#![allow(dead_code)]

use pane_core::Platform;

/// The system the test binary was built for.
pub fn this_system() -> Platform {
    Platform::current().expect("the tests run on Windows, macOS or Linux")
}

/// The two other systems, in Pane's order.
pub fn other_systems() -> [Platform; 2] {
    let mut others = Platform::ALL
        .into_iter()
        .filter(|&platform| platform != this_system());
    [others.next().unwrap(), others.next().unwrap()]
}

/// The name people know `platform` by.
pub fn name(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "Windows",
        Platform::Macos => "macOS",
        Platform::Linux => "Linux",
    }
}

/// The other systems as Pane lists them: "macOS and Linux" on Windows.
pub fn other_names() -> String {
    let [first, second] = other_systems();
    format!("{} and {}", name(first), name(second))
}

/// The explanation for `subject` ("this action") supporting only
/// `supported` ("Windows"), on this system.
pub fn only(subject: &str, supported: &str) -> String {
    format!(
        "Not available on {}: {subject} supports only {supported}",
        name(this_system())
    )
}

/// The explanation for `subject` declaring an empty list of systems.
pub fn nowhere(subject: &str) -> String {
    format!(
        "Not available on {}: {subject} supports no operating system",
        name(this_system())
    )
}

/// (id, title) of an item of the samples.
pub type SampleItem = (&'static str, &'static str);

const WINDOWS_ONLY: SampleItem = ("windows-only", "Windows-only action");
const NOT_WINDOWS: SampleItem = ("not-windows", "macOS and Linux action");

/// The samples' two platform-limited items on this system: the one that
/// runs, the one that is unavailable, and the reason given for it.
pub fn sample_items() -> (SampleItem, SampleItem, String) {
    if this_system() == Platform::Windows {
        (
            WINDOWS_ONLY,
            NOT_WINDOWS,
            only("this action", "macOS and Linux"),
        )
    } else {
        (NOT_WINDOWS, WINDOWS_ONLY, only("this action", "Windows"))
    }
}
