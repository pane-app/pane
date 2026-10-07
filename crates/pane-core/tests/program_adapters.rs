//! The containment of a system program's processes against the real
//! system (#147): a program Pane starts runs in a tree of its own (a Job
//! Object on Windows, a process group on macOS and Linux), and ending the
//! tree ends the program and the program it started in turn, which Pane
//! never saw start.
//!
//! The program is `pane-echo --parent`, built for this system by `cargo
//! xtask guests`: it starts a descendant, says so once the descendant
//! beats, and holds; each beats in a file of its own beside the program
//! every 20 ms. A check that a process ended does not trust a process id:
//! its file must stop growing.
//!
//! It starts and ends real processes, so it runs only where
//! `PANE_TEST_PROCESS_TREES=1` is set, as CI's check legs do.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use pane_core::Target;
use pane_core::process_tree::testing;

/// Whether this run opted in.
fn opted_in() -> bool {
    let opted = std::env::var_os("PANE_TEST_PROCESS_TREES").is_some_and(|value| value == "1");
    if !opted {
        eprintln!("skipped: set PANE_TEST_PROCESS_TREES=1 to check program containment");
    }
    opted
}

/// `pane-echo`, as `cargo xtask guests` built it for this system.
fn built_echo() -> PathBuf {
    let this = Target::current().expect("Pane names this system's target");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages/sample-helper/helpers")
        .join(this.id())
        .join(format!("pane-echo{}", this.exe_suffix()));
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// The length of the heartbeat file at `path`, if it exists.
fn beats(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

/// Waits until the file at `path` has grown past `before`.
fn until_it_beats(path: &Path, before: Option<u64>) {
    let started = Instant::now();
    while beats(path) <= before {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{} does not beat",
            path.display()
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn ending_a_program_s_tree_ends_the_program_it_started() {
    if !opted_in() {
        return;
    }
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join(built_echo().file_name().unwrap());
    fs::copy(built_echo(), &program).unwrap();
    let (parent, descendant) = (
        folder.path().join("pane-echo.parent.alive"),
        folder.path().join("pane-echo.descendant.alive"),
    );
    let mut command = Command::new(&program);
    command
        .args(["--parent", "30"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut contained = testing::start(command).unwrap();

    assert!(
        contained.contained(),
        "the system put the program in no Job Object"
    );
    let output = contained.child.stdout.take().unwrap();
    let mut said = String::new();
    BufReader::new(output).read_line(&mut said).unwrap();
    assert_eq!(said.trim(), "started a descendant");
    until_it_beats(&descendant, None);
    until_it_beats(&parent, None);

    contained.end().unwrap();

    // Both ended: neither beats any more, however long one looks.
    thread::sleep(Duration::from_millis(100));
    let last = (beats(&parent), beats(&descendant));
    thread::sleep(Duration::from_millis(500));
    assert_eq!(
        (beats(&parent), beats(&descendant)),
        last,
        "a process of the tree still runs"
    );
}

/// The program alone, without its tree, would leave its descendant
/// running: what the check above sees ending is the tree's doing. The
/// descendant is released through its file before the test ends.
#[test]
fn ending_only_the_program_leaves_what_it_started() {
    if !opted_in() {
        return;
    }
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join(built_echo().file_name().unwrap());
    fs::copy(built_echo(), &program).unwrap();
    let descendant = folder.path().join("pane-echo.descendant.alive");
    let mut child = Command::new(&program)
        .args(["--parent", "30"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut said = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut said)
        .unwrap();
    until_it_beats(&descendant, None);

    child.kill().unwrap();
    child.wait().unwrap();

    let before = beats(&descendant);
    until_it_beats(&descendant, before);
    // Released: it stops by itself.
    fs::write(folder.path().join("pane-echo.release"), "").unwrap();
    let started = Instant::now();
    while beats(&descendant).is_some() {
        assert!(started.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(5));
    }
}
