//! The cap on each guest's memory, 128 MiB (#120), through the launcher's
//! public interface. A command that grows past it crashes, told as running
//! out of memory with the limit named, and the crash counts towards pausing
//! its package like any other; one that grows to just under it completes.
//! The faulty fixture grows its memory to either side of the cap, so these
//! pin the constant, not "something small". Also how far under the cap
//! every sample and default extension starts, printed for a verify run's
//! log (`.config/nextest.toml` shows it on success).

use std::fs;
use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::{CommandRegistration, GUEST_MEMORY, Launcher, Runtime, Screen, Status};

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest;
use rows::{manage, select_title, to_root};

/// What the user is told when a command runs out of memory.
const OUT_OF_MEMORY: &str =
    "The extension crashed: it ran out of memory: an extension may use at most 128 MiB";

/// Where the fixture's "grow-near-cap" item ends: two pages under 128 MiB.
const JUST_UNDER: usize = 128 * 1024 * 1024 - 2 * 64 * 1024;

/// Opens the command titled "Faulty" from root search and runs its item
/// `item`, returning the outcome: its toast, or the status line.
fn run(launcher: &Launcher, item: &str) -> Status {
    to_root(launcher);
    select_title(launcher, "Faulty");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "{:?}",
        launcher.view().status
    );
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

fn error(status: Status) -> String {
    match status {
        Status::Error(message) => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn a_command_that_grows_to_just_under_the_cap_completes() {
    let launcher = Launcher::new(
        Runtime::start(),
        vec![CommandRegistration {
            id: "faulty".into(),
            title: "Faulty".into(),
            subtitle: None,
            component: guest("faulty"),
            takes_query: false,
            search: false,
            when: pane_core::CommandWhen::Always,
            matches: pane_core::CommandMatches::Title,
        }],
    );

    assert_eq!(
        run(&launcher, "grow-near-cap"),
        Status::Result(format!("grew to {JUST_UNDER} bytes"))
    );
    // The instance that holds it keeps answering.
    assert_eq!(run(&launcher, "ok"), Status::Result("fine".into()));
    let peak = pane_core::memory_peak("faulty.wasm").expect("the fixture ran");
    assert!((JUST_UNDER..=GUEST_MEMORY).contains(&peak), "{peak} bytes");
}

#[test]
fn a_command_that_grows_past_the_cap_runs_out_of_memory_and_is_paused_like_any_crash() {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let folder = sources.path().join("faulty");
    fs::create_dir_all(&folder).unwrap();
    fs::copy(guest("faulty"), folder.join("faulty.wasm")).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{
  "manifestVersion": 1,
  "title": "Faulty",
  "apiVersion": "0.1",
  "commands": [{ "id": "faulty", "title": "Faulty", "component": "faulty.wasm" }]
}"#,
    )
    .unwrap();
    let launcher = Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    );
    block_on(launcher.install_package(&folder));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Faulty".into())
    );

    for _ in 0..2 {
        assert_eq!(
            run(&launcher, "grow-past-cap"),
            Status::Error(OUT_OF_MEMORY.into())
        );
        // A fresh instance answers the next call.
        assert_eq!(run(&launcher, "ok"), Status::Result("fine".into()));
    }
    // The third crash within five minutes pauses the package, as any
    // crash's would.
    let toast = error(run(&launcher, "grow-past-cap"));
    assert!(
        toast.starts_with("Faulty crashed 3 times within 5 minutes and is paused"),
        "{toast}"
    );

    // Why it is paused names the memory.
    manage(&launcher);
    select_title(&launcher, "Why Faulty is paused");
    block_on(launcher.activate_selected());
    let details = launcher.view().details().to_vec();
    let last = format!("Crashed 3 times within 5 minutes; the last time: {OUT_OF_MEMORY}");
    assert!(
        details.iter().any(|line| line.starts_with(&last)),
        "{details:?}"
    );
}

/// The language each component is written in, for the report.
fn language(name: &str) -> &'static str {
    if name.ends_with("_ts") {
        "TypeScript"
    } else if name.ends_with("_js") {
        "JavaScript"
    } else {
        "Rust"
    }
}

/// Every default extension and sample, started and asked for its list,
/// stays under the cap; the log shows each one's peak. Their own suites
/// run under the cap too, which is what shows that they fit; this tells by
/// how much.
#[test]
fn memory_peaks_of_the_samples_and_default_extensions() {
    let mut names: Vec<String> = [
        "calculator",
        "applications",
        "quicklinks",
        "files",
        "clipboard_history",
        "sample_rust",
        "sample_settings",
        "sample_operations",
        "sample_dependencies",
        "sample_query",
        "sample_no_view",
        "sample_search",
        "sample_schedule",
        "sample_service",
        "sample_helper",
        "sample_actions",
        "sample_preferences",
        "sample_arguments",
        "sample_icons",
        "sample_programs",
    ]
    .map(String::from)
    .into();
    // The JavaScript and TypeScript samples, whose engine is the largest
    // part of what they hold.
    let prebuilt = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../guests/prebuilt");
    for entry in fs::read_dir(prebuilt).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "wasm")
        {
            names.push(path.file_stem().unwrap().to_str().unwrap().to_owned());
        }
    }
    names.sort();

    // A few at a time: each compiles its component.
    let chunks: Vec<Vec<String>> = names
        .chunks(names.len().div_ceil(4))
        .map(<[String]>::to_vec)
        .collect();
    std::thread::scope(|scope| {
        for chunk in chunks {
            scope.spawn(move || {
                for name in chunk {
                    let runtime = Runtime::start().unwrap();
                    // Its answer does not matter (a command may need data it
                    // has none of here): only that it started and ran.
                    let _ = block_on(runtime.render(&guest(&name)));
                }
            });
        }
    });

    let mut largest: Vec<(&str, usize, &str)> = Vec::new();
    for name in names.iter().map(String::as_str) {
        let peak = pane_core::memory_peak(&format!("{name}.wasm"))
            .unwrap_or_else(|| panic!("{name} never ran"));
        println!(
            "memory peak: {name} ({}): {:.1} MiB",
            language(name),
            peak as f64 / (1024.0 * 1024.0)
        );
        assert!(peak <= GUEST_MEMORY, "{name}: {peak} bytes");
        match largest
            .iter_mut()
            .find(|(lang, _, _)| *lang == language(name))
        {
            Some(entry) if entry.1 < peak => *entry = (language(name), peak, name),
            Some(_) => {}
            None => largest.push((language(name), peak, name)),
        }
    }
    for (lang, peak, name) in largest {
        println!(
            "largest memory peak in {lang}: {name}, {:.1} MiB of {} MiB",
            peak as f64 / (1024.0 * 1024.0),
            GUEST_MEMORY / (1024 * 1024)
        );
    }
}
