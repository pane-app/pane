//! Reaching the running Pane, and starting one where none answers.
//!
//! The Pane started is the program `PANE_APP` names if it is set; else the
//! `pane` program beside `pane-ext` (a build folder, or an installation
//! holding both); else where Pane's packages install it for the user
//! (`%LOCALAPPDATA%\Pane\pane.exe` on Windows, `Pane.app` in
//! `/Applications` or `~/Applications` on macOS, `~/.local/bin/pane` on
//! Linux); else `pane` on the search path. It runs on its own: stopping
//! `pane-ext` does not stop it.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use pane_core::local_channel::{self, Endpoint, Event, Sender};

/// The environment variable naming the Pane program to start, instead of
/// looking for one.
const PANE_APP: &str = "PANE_APP";

/// How long a Pane that was started has to listen.
const START_LIMIT: Duration = Duration::from_secs(60);

/// How often `pane-ext` tries the endpoint while it waits.
const POLL: Duration = Duration::from_millis(200);

/// A connection to the Pane listening on `endpoint`, starting one if none
/// does and waiting for it to listen; or why there is none.
pub(crate) fn reach(endpoint: &Endpoint) -> Result<(Sender, Receiver<Event>), String> {
    match local_channel::connect(endpoint) {
        Ok(connected) => return Ok(connected),
        Err(error) if nobody_listens(&error) => {}
        Err(error) => return Err(format!("Pane could not be reached on {endpoint}: {error}")),
    }
    let (found, looked) = find_pane();
    let Some(pane) = found else {
        return Err(format!(
            "no Pane answered on {endpoint}, and pane-ext found no Pane to start; it looked for {}",
            looked.join(", ")
        ));
    };
    println!(
        "pane-ext: no Pane answered on {endpoint}, so pane-ext starts {}",
        pane.display()
    );
    start(&pane).map_err(|error| format!("{} could not be started: {error}", pane.display()))?;
    wait_for_pane(endpoint, START_LIMIT)
}

/// A connection to the Pane listening on `endpoint`, once one does, within
/// `limit`.
fn wait_for_pane(
    endpoint: &Endpoint,
    limit: Duration,
) -> Result<(Sender, Receiver<Event>), String> {
    let deadline = Instant::now() + limit;
    loop {
        match local_channel::connect(endpoint) {
            Ok(connected) => return Ok(connected),
            Err(error) if !nobody_listens(&error) => {
                return Err(format!("Pane could not be reached on {endpoint}: {error}"));
            }
            Err(_) if Instant::now() >= deadline => {
                return Err(format!(
                    "the Pane started did not listen on {endpoint} within {} seconds",
                    limit.as_secs()
                ));
            }
            Err(_) => {}
        }
        std::thread::sleep(POLL);
    }
}

/// Whether `error`, connecting, says that nothing listens there.
fn nobody_listens(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

/// The Pane program to start, if one is found, and where `pane-ext`
/// looked, in order.
fn find_pane() -> (Option<PathBuf>, Vec<String>) {
    if let Some(named) = std::env::var_os(PANE_APP).filter(|value| !value.is_empty()) {
        let named = PathBuf::from(named);
        let looked = vec![format!("{PANE_APP} ({})", named.display())];
        return (named.is_file().then_some(named), looked);
    }
    let program = if cfg!(windows) { "pane.exe" } else { "pane" };
    let mut candidates = Vec::new();
    if let Some(folder) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        candidates.push(folder.join(program));
    }
    candidates.extend(installed());
    let mut looked: Vec<String> = candidates
        .iter()
        .map(|candidate| candidate.display().to_string())
        .collect();
    if let Some(found) = candidates.into_iter().find(|path| path.is_file()) {
        return (Some(found), looked);
    }
    looked.push(format!("{program} on the search path"));
    let on_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|folder| folder.join(program))
            .find(|candidate| candidate.is_file())
    });
    (on_path, looked)
}

/// Where Pane's packages install its program for this user
/// (docs/installer.md).
fn installed() -> Vec<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|home| !home.is_empty())
        .map(PathBuf::from);
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .filter(|folder| !folder.is_empty())
            .map(|folder| PathBuf::from(folder).join("Pane").join("pane.exe"))
            .into_iter()
            .collect()
    } else if cfg!(target_os = "macos") {
        let bundle = "Pane.app/Contents/MacOS/pane";
        let mut found = vec![Path::new("/Applications").join(bundle)];
        found.extend(home.map(|home| home.join("Applications").join(bundle)));
        found
    } else {
        let local = home.map(|home| home.join(".local/bin/pane"));
        local.into_iter().collect()
    }
}

/// Starts `pane`, on its own: Ctrl+C in `pane-ext`'s terminal does not
/// reach it, and it outlives `pane-ext`.
fn start(pane: &Path) -> io::Result<()> {
    let mut command = Command::new(pane);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    let mut child = command.spawn()?;
    // Waited for, should it quit while pane-ext runs.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(any(windows, unix)))]
fn detach(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use pane_core::local_channel::Request;
    use pane_core::{Launcher, Runtime};

    use super::*;

    /// An endpoint of this test's own. On Unix its folder is left for Pane
    /// to make, as only this user's: a temporary folder others can enter is
    /// refused.
    fn endpoint(folder: &Path) -> Endpoint {
        if cfg!(windows) {
            let name = folder.file_name().unwrap().to_string_lossy();
            Endpoint::at(format!(r"\\.\pipe\pane-ext-test-{name}"))
        } else {
            Endpoint::at(folder.join("pane").join("channel"))
        }
    }

    #[test]
    fn a_pane_that_starts_listening_is_waited_for() {
        let data = tempfile::tempdir().unwrap();
        let endpoint = endpoint(data.path());
        let extensions = data.path().join("extensions");
        let launcher = Launcher::with_packages(Runtime::start(), Vec::new(), extensions);
        let listening = {
            let endpoint = endpoint.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(500));
                local_channel::serve(launcher, &endpoint).unwrap()
            })
        };
        let (sender, _events) = wait_for_pane(&endpoint, Duration::from_secs(60)).unwrap();
        assert!(sender.send(&Request::Stop));
        drop(listening.join().unwrap());
    }

    #[test]
    fn a_pane_that_does_not_listen_is_given_up_on() {
        let data = tempfile::tempdir().unwrap();
        let error = wait_for_pane(&endpoint(data.path()), Duration::from_millis(300))
            .err()
            .unwrap();
        assert!(error.contains("did not listen"), "{error}");
    }
}
