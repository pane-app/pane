//! `pane-ext dev [folder]`: developing a package in the running Pane from
//! the terminal (ADR 0047, #217).
//!
//! It runs development mode's session, the `pane-build` crate's as Pane's
//! own development mode does, so the builds run here and print here, the
//! compiler's errors included. Meanwhile it reaches the running Pane over
//! the local channel (`pane_core::local_channel`), starting Pane if none
//! answers (see `start`), and stops if there is none to start. The first
//! build that succeeds is handed to Pane; if Pane has not installed the
//! folder, it shows its install preview first, which the author confirms in
//! Pane. After that each save builds the package here again, and Pane
//! reloads each build that succeeds, while a build that fails leaves it
//! running the working code.
//!
//! Pane's messages about the package (its builds, reloads, crashes) and
//! what the package prints, its extension log, are printed as they come.
//! Ctrl+C, or the connection closing, ends the development in Pane as Stop
//! developing does; ending it in Pane ends `pane-ext dev`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard};

use pane_build::{BuildFailure, Claim, Host, MAX_OBSOLETE, Prepared, Session};
use pane_core::develop::{PaneManifest, Toolchains};
use pane_core::local_channel::{Endpoint, Event, Request, Sender};

/// How a development ended: as asked, or because something failed.
enum Ending {
    Stopped(String),
    Failed(String),
}

/// Develops the package in `folder`, or the current folder, until it ends.
pub(crate) fn run(folder: Option<PathBuf>) -> ExitCode {
    match develop(folder) {
        Ok(message) => {
            println!("pane-ext: {message}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("pane-ext: {message}");
            ExitCode::FAILURE
        }
    }
}

fn develop(folder: Option<PathBuf>) -> Result<String, String> {
    let folder = match folder {
        Some(folder) => folder,
        None => std::env::current_dir()
            .map_err(|error| format!("the current folder cannot be read: {error}"))?,
    };
    let folder = pane_build::canonical(&folder)
        .map_err(|error| format!("{} cannot be developed: {error}", folder.display()))?;
    if !folder.join(pane_build::MANIFEST_FILE).is_file() {
        return Err(format!(
            "{} has no {}, so it is not a package folder",
            folder.display(),
            pane_build::MANIFEST_FILE
        ));
    }
    let toolchains = Toolchains::from_env(None);
    let prepared = Prepared::new(
        &toolchains,
        Arc::new(PaneManifest),
        &folder,
        work_folder(&folder),
    )
    .map_err(|reason| format!("{} cannot be developed: {reason}", folder.display()))?
    .echo(Arc::new(|line: &str| println!("{line}")));
    let endpoint = Endpoint::from_env()
        .map_err(|error| format!("Pane's endpoint for this user is not known: {error}"))?;
    let command = prepared.command();
    println!(
        "pane-ext: developing {}: each save builds it here with `{command}`, and Pane reloads it",
        folder.display()
    );
    let (session, worker) = prepared.begin();
    let session = Arc::new(session);
    let (done, ended) = mpsc::channel();
    let shared = Arc::new(Shared {
        ended: AtomicBool::new(false),
        done: Mutex::new(Some(done)),
        installed: Mutex::new(None),
    });
    // Pane is reached, or started, while the first build runs.
    let (found, reached) = mpsc::channel();
    {
        let shared = shared.clone();
        let session = session.clone();
        std::thread::Builder::new()
            .name("pane-ext-events".into())
            .spawn(move || follow_pane(&endpoint, found, &shared, &session))
            .map_err(|error| format!("a thread could not start: {error}"))?;
    }
    worker.start(Handing {
        folder,
        command,
        pane: None,
        reached,
        developing: false,
        shared,
    });
    session.build_now();
    let ending = ended
        .recv()
        .unwrap_or_else(|_| Ending::Failed("the development stopped".into()));
    session.end();
    match ending {
        Ending::Stopped(message) => Ok(message),
        Ending::Failed(message) => Err(message),
    }
}

/// `pane-ext`'s own folder for the package's builds, their staging folders
/// and logs: in the user's cache folder, named by a hash of the package's
/// folder. `pack` builds there too, in its own subfolder.
pub(crate) fn work_folder(folder: &Path) -> PathBuf {
    let home = env_path(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    let cache = if cfg!(windows) {
        env_path("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        home.map(|home| home.join("Library/Caches"))
    } else {
        env_path("XDG_CACHE_HOME").or_else(|| home.map(|home| home.join(".cache")))
    };
    // FNV-1a, which is stable across Rust versions, unlike `DefaultHasher`.
    let hash = folder
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    cache
        .unwrap_or_else(std::env::temp_dir)
        .join("pane-ext")
        .join(format!("{hash:016x}"))
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// What the session's thread and the thread reading Pane's events share.
struct Shared {
    ended: AtomicBool,
    /// Tells `develop` how the development ended; the first ending is the
    /// one told.
    done: Mutex<Option<mpsc::Sender<Ending>>>,
    /// Where Pane's installed copy of the package is.
    installed: Mutex<Option<PathBuf>>,
}

impl Shared {
    fn end(&self, ending: Ending) {
        self.ended.store(true, Ordering::SeqCst);
        if let Some(done) = lock(&self.done).take() {
            let _ = done.send(ending);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

/// The running Pane, once reached.
struct Reached {
    sender: Sender,
    /// Pane's answers to `develop`; its other events are printed as they
    /// come.
    answers: Receiver<Event>,
}

/// The session's host: hands each build that succeeds to the running Pane.
struct Handing {
    folder: PathBuf,
    command: String,
    /// Pane, once reached; it comes on `reached`.
    pane: Option<Reached>,
    reached: Receiver<Reached>,
    /// Whether Pane develops the package yet: from then on, Pane tells of
    /// the builds in the package's log.
    developing: bool,
    shared: Arc<Shared>,
}

impl Host for Handing {
    type Claim = ();

    fn is_current(&self) -> bool {
        !self.shared.ended.load(Ordering::SeqCst)
    }

    fn building(&self, command: &str) {
        match &self.pane {
            Some(pane) if self.developing => {
                pane.sender.send(&Request::Building);
            }
            _ => println!("pane-ext: building with `{command}`"),
        }
    }

    fn failed(&self, failure: &BuildFailure, _command: &str) {
        if let Some(pane) = &self.pane
            && self.developing
        {
            // Pane keeps running the working code.
            pane.sender.send(&Request::Failed {
                summary: failure.summary.clone(),
                output: failure.output.clone(),
                earlier: failure.earlier,
                log: failure.log.clone(),
            });
            return;
        }
        println!("pane-ext: it did not build: {}", failure.summary);
        if let Some(log) = &failure.log {
            println!("pane-ext: the whole output is in {}", log.display());
        }
        println!("pane-ext: save a fix to build it again");
    }

    fn claim(&self) -> Claim<()> {
        if self.is_current() {
            Claim::Claimed(())
        } else {
            Claim::Ended
        }
    }

    fn deliver(&mut self, _claim: &mut (), staging: &Path) -> bool {
        if self.pane.is_none() {
            // Never sent when no Pane was found, which ends the development.
            let Ok(pane) = self.reached.recv() else {
                return false;
            };
            self.pane = Some(pane);
        }
        let Some(pane) = &self.pane else {
            return false;
        };
        let develop = Request::Develop {
            folder: self.folder.clone(),
            staging: staging.to_path_buf(),
            command: self.command.clone(),
        };
        if !pane.sender.send(&develop) {
            return false;
        }
        loop {
            match pane.answers.recv() {
                Ok(Event::Previewing { title }) => println!(
                    "pane-ext: Pane shows the install preview of {title}: choose Install there to \
                     develop it"
                ),
                Ok(Event::Developing {
                    title,
                    replaced,
                    installed,
                }) => {
                    *lock(&self.shared.installed) = installed;
                    if !self.developing {
                        self.developing = true;
                        println!(
                            "pane-ext: Pane develops {title}: each save builds it here and \
                             reloads it there; Ctrl+C stops"
                        );
                    }
                    return replaced;
                }
                Ok(Event::Refused { message }) => {
                    let message = format!("Pane did not take the build: {message}");
                    self.shared.end(Ending::Failed(message));
                    return false;
                }
                Ok(_) => {}
                // The connection closed, which the events' thread tells.
                Err(_) => return false,
            }
        }
    }

    fn delivered(&self, _claim: ()) {}

    fn installed(&self) -> Option<PathBuf> {
        lock(&self.shared.installed).clone()
    }

    fn gave_up(&self) {
        println!(
            "pane-ext: the sources kept changing during {MAX_OBSOLETE} builds in a row; save \
             again to build them"
        );
    }

    fn changed(&self) {}
}

/// Reaches the Pane listening on `endpoint`, starting one if none does, and
/// hands it to the session's thread through `found`; then reads its events
/// (see [`read_events`]). Ends the development if no Pane can be reached.
fn follow_pane(
    endpoint: &Endpoint,
    found: mpsc::Sender<Reached>,
    shared: &Shared,
    session: &Session,
) {
    let (sender, events) = match crate::start::reach(endpoint) {
        Ok(reached) => reached,
        Err(message) => return shared.end(Ending::Failed(message)),
    };
    // Asked before the first build is handed over, so that none of Pane's
    // messages about it are missed.
    sender.send(&Request::Subscribe);
    let (answers, answered) = mpsc::channel();
    let reached = Reached {
        sender,
        answers: answered,
    };
    if found.send(reached).is_ok() {
        read_events(events, answers, shared, session);
    }
}

/// Prints Pane's messages and the package's log as they come, has the
/// session build when Pane asks, and passes Pane's answers to `develop` to
/// the session's thread, until the development ends.
fn read_events(
    events: Receiver<Event>,
    answers: mpsc::Sender<Event>,
    shared: &Shared,
    session: &Session,
) {
    for event in events {
        match event {
            Event::Log {
                source,
                level,
                command,
                text,
                ..
            } => println!("{}", log_line(&source, &level, command.as_deref(), &text)),
            Event::Build => session.build_now(),
            Event::Ended { message } => {
                shared.end(Ending::Stopped(message));
                return;
            }
            answer => {
                let _ = answers.send(answer);
            }
        }
    }
    shared.end(Ending::Failed("Pane closed the connection".into()));
}

/// A line of the package's extension log as the terminal shows it: Pane's
/// own messages after "Pane:", the package's after their level and the
/// command it was running.
fn log_line(source: &str, level: &str, command: Option<&str>, text: &str) -> String {
    match (source, command) {
        ("pane", _) => format!("Pane: {text}"),
        (_, Some(command)) => format!("{level} [{command}] {text}"),
        (_, None) => format!("{level} {text}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_lines_say_who_wrote_them() {
        assert_eq!(
            log_line("pane", "info", None, "Reloaded Hello"),
            "Pane: Reloaded Hello"
        );
        assert_eq!(
            log_line("stdout", "info", Some("hello"), "saying hello"),
            "info [hello] saying hello"
        );
        assert_eq!(log_line("stderr", "error", None, "oops"), "error oops");
    }

    #[test]
    fn each_folder_has_a_work_folder_of_its_own() {
        let one = work_folder(Path::new("/src/one"));
        let other = work_folder(Path::new("/src/other"));
        assert_ne!(one, other);
        assert_eq!(one, work_folder(Path::new("/src/one")));
        assert!(
            one.parent().unwrap().ends_with("pane-ext"),
            "{}",
            one.display()
        );
    }
}
