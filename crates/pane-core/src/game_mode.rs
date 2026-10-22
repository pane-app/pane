//! Game mode (#125, ADR 0039): an optional setting, Windows only and off
//! by default, that pauses Pane's hotkeys while a game is in front, so
//! the game gets every key. While it is on, Pane decides on each
//! foreground change — a system event, never a timer — whether the
//! window in front is a game: Windows reports a full-screen Direct3D
//! application in front through its documented notification state
//! (`SHQueryUserNotificationState`, `QUNS_RUNNING_D3D_FULL_SCREEN`), or
//! the program of the window is one the user listed in game mode's
//! settings, so windowed games are covered too. While a game is in
//! front, every Pane hotkey — the Open Pane hotkey included — is
//! released, and the keyboard hook is left with no binding, which
//! uninstalls it (it is kept only while a binding needs it, #252): so
//! everything passes through to the game. When the game leaves the
//! front, the hotkeys come back by themselves, through the registration
//! path every other change takes (see `launcher::hotkeys`).
//!
//! The decision itself is a pure function of the settings and the facts
//! of the window in front ([`is_game`]), compiled and tested on every
//! system. The settings — the on/off choice and the programs to treat
//! as games — are Pane's own record, `game-mode.json` beside
//! `installed.json`, kept as the hotkeys' and the aliases' are kept:
//! versioned, validated and written atomically ([`GameMode::open`],
//! [`GameMode::save`]).
//!
//! The foreground source is the seam the tests fake
//! ([`ForegroundSource`]): a source of foreground changes that the
//! launcher subscribes to ([`crate::Launcher::with_foreground`]). On
//! Windows, [`native`] is the system's own foreground event hook — a
//! `SetWinEventHook` consumer mirroring the front application watcher's
//! (#253) rather than sharing it, so that watcher's behavior stands
//! untouched — reporting each window that comes to the front from its
//! own thread. The pause happens there, where the events arrive: game
//! mode's source exists only on Windows, whose hotkey adapter takes
//! register and release requests from any thread (it queues them to the
//! thread that owns the registrations), so no adapter is ever asked off
//! the thread it needs; the tests' fakes report on the tests' own
//! thread. The window learns of a pause or a resume through the
//! launcher's change notification, on its own thread, where the tray
//! icon's tooltip follows (`crate::tray`).

use std::path::Path;
use std::sync::Arc;

use crate::atomic::{Readers, write_atomically};

#[cfg(target_os = "windows")]
mod windows;

/// The file game mode's settings are recorded in, beside
/// `installed.json` and the hotkeys' and aliases' records.
const FILE: &str = "game-mode.json";

/// The record's version as this Pane writes it.
const VERSION: u64 = 1;

/// Game mode's settings: whether it is on, and the programs to treat as
/// games. Off by default, with no programs — Pane takes no keys from any
/// game unless the user asks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameMode {
    /// Whether game mode is on.
    pub on: bool,
    /// The programs to treat as games, by the file name of their
    /// program, such as `game.exe`: a windowed game the system does not
    /// report as full-screen, named so its windows pause Pane's hotkeys
    /// too. Matched against the window in front's program by file name,
    /// whatever its case and wherever it is installed.
    pub programs: Vec<String>,
}

impl GameMode {
    /// Reads the settings recorded in `dir`. No record at all means the
    /// defaults, as a record with missing fields does; a record this
    /// Pane cannot read — another version, a program that is not a
    /// plain name, one that is not a record at all — is never replaced
    /// by reading: the defaults are in force and the next change the
    /// user makes writes the record anew, as `updates.json`'s controls
    /// are.
    pub(crate) fn open(dir: &Path) -> GameMode {
        let text = match std::fs::read_to_string(dir.join(FILE)) {
            Ok(text) => text,
            Err(_) => return GameMode::default(),
        };
        let Ok(read) = serde_json::from_str::<Recorded>(&text) else {
            return GameMode::default();
        };
        if read.version == VERSION && read.programs.iter().all(|program| is_program_name(program)) {
            GameMode {
                on: read.on,
                programs: read.programs,
            }
        } else {
            GameMode::default()
        }
    }

    /// Writes these settings to the record in `dir`, atomically: the
    /// record is replaced whole or not at all. `Err` with the problem,
    /// phrased with the file's path, when they cannot be recorded — a
    /// program that is not a plain name — or the record cannot be
    /// written.
    pub(crate) fn save(&self, dir: &Path) -> Result<(), String> {
        if let Some(reason) = self.refusal() {
            return Err(reason);
        }
        let recorded = Recorded {
            version: VERSION,
            on: self.on,
            programs: self.programs.clone(),
        };
        let text = serde_json::to_string_pretty(&recorded).map_err(|error| error.to_string())?;
        write_atomically(&dir.join(FILE), text.as_bytes(), Readers::Default)
            .map_err(|error| format!("{} cannot be written: {error}", dir.join(FILE).display()))
    }

    /// Why these settings cannot be recorded, if they cannot: a program
    /// that is not a plain program name. Game mode matches the file name
    /// of the program of the window in front, so a path would never
    /// match what it names.
    pub fn refusal(&self) -> Option<String> {
        self.programs
            .iter()
            .find(|program| !is_program_name(program))
            .map(|program| {
                format!(
                    "“{program}” is not a program name: name the program's file, such as \
                     game.exe"
                )
            })
    }
}

/// The settings as the record holds them. `on` is written only when on
/// and `programs` only when any is, so the file stays small and a record
/// from before a field existed still reads.
#[derive(serde::Serialize, serde::Deserialize)]
struct Recorded {
    version: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    on: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    programs: Vec<String>,
}

/// Whether `name` names a program as game mode's settings list one: a
/// plain file name, trimmed and not a path — matching is by file name,
/// so a path would never match what it names.
fn is_program_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name != "." && name != ".." && !name.contains(['\\', '/', ':'])
}

/// What a foreground source reports of the window in front, as game mode
/// decides on it: the program of the window's process, and whether the
/// system reports a full-screen Direct3D application in front.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Foreground {
    /// The full path of the program of the window in front's process, if
    /// it could be read: the identity game mode matches its listed
    /// programs against, by file name. A window whose program cannot be
    /// read is decided by the full-screen state alone.
    pub program: Option<String>,
    /// Whether the system reports a full-screen Direct3D application in
    /// front (Windows' notification state `QUNS_RUNNING_D3D_FULL_SCREEN`):
    /// a full-screen game, which needs no listing to be recognized.
    pub full_screen: bool,
}

/// Whether the window in front is a game, as game mode decides: the
/// system reports a full-screen Direct3D application in front, or the
/// program of the window is one the settings list — so windowed games
/// are covered too. Whether game mode is on is the launcher's to say; it
/// decides on this only while it is. A pure function, tested on every
/// system.
pub fn is_game(mode: &GameMode, front: &Foreground) -> bool {
    front.full_screen || listed(mode, front)
}

/// Whether `front`'s program is one game mode's settings list, matched
/// by file name — Windows paths are compared without case, and the same
/// program runs from wherever it is installed.
fn listed(mode: &GameMode, front: &Foreground) -> bool {
    front.program.as_deref().is_some_and(|program| {
        let name = file_name(program);
        mode.programs.iter().any(|listed| file_name(listed) == name)
    })
}

/// The file name of `program`, a path, in lowercase: the identity game
/// mode matches programs by.
fn file_name(program: &str) -> String {
    program
        .trim()
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

/// Where a foreground source reports each window that comes to the
/// front: the launcher's subscription, which
/// [`crate::Launcher::with_foreground`] makes. A source calls it from
/// its own thread (see the module docs).
#[derive(Clone)]
pub struct ForegroundTold(Arc<dyn Fn(&Foreground) + Send + Sync + 'static>);

impl ForegroundTold {
    /// The subscription that reports each window that comes to the front
    /// to `told`.
    pub(crate) fn of(told: Arc<dyn Fn(&Foreground) + Send + Sync + 'static>) -> ForegroundTold {
        ForegroundTold(told)
    }

    /// Reports that the window in front is `front`.
    pub fn front(&self, front: &Foreground) {
        (self.0)(front);
    }
}

/// A source of foreground changes for game mode (#125): each window
/// that comes to the front, reported to what [`watch`](Self::watch) is
/// given, for as long as the source lives. On Windows, [`native`] is
/// the system's own foreground event hook; the tests fake one.
pub trait ForegroundSource: Send + Sync + 'static {
    /// Reports each window that comes to the front to `told`, from the
    /// source's own thread, until the source is dropped. Watching
    /// begins when the source is made; subscribing more than once hands
    /// the reports to the last subscriber, which is the launcher's.
    fn watch(&self, told: ForegroundTold);
}

/// This system's foreground source for game mode: Windows' foreground
/// event hook, watching from the moment it is made (`windows`). `None`
/// everywhere else — game mode is Windows only, and the Keyboard page
/// says so — and on Windows when the hook was refused, where game mode
/// stays on but never pauses.
pub fn native() -> Option<Arc<dyn ForegroundSource>> {
    #[cfg(target_os = "windows")]
    {
        windows::Watcher::start()
            .ok()
            .map(|watcher| Arc::new(watcher) as Arc<dyn ForegroundSource>)
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{FILE, Foreground, GameMode, is_game};

    /// The settings of a game mode that is on and lists `programs`.
    fn mode(programs: &[&str]) -> GameMode {
        GameMode {
            on: true,
            programs: programs.iter().copied().map(str::to_owned).collect(),
        }
    }

    /// The facts of the window in front: its program, and whether the
    /// system reports a full-screen Direct3D application there.
    fn front(program: Option<&str>, full_screen: bool) -> Foreground {
        Foreground {
            program: program.map(str::to_owned),
            full_screen,
        }
    }

    /// Reads what `text` records in a fresh folder.
    fn reading(text: &str) -> GameMode {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), text).unwrap();
        GameMode::open(dir.path())
    }

    #[test]
    fn a_full_screen_application_in_front_is_a_game_without_being_listed() {
        // Windows reports a full-screen Direct3D application in front
        // through its notification state: no listing is needed, and
        // nothing else of the window matters.
        assert!(is_game(&mode(&[]), &front(None, true)));
        assert!(is_game(
            &mode(&["other.exe"]),
            &front(Some(r"C:\Windows\notepad.exe"), true)
        ));
        // A windowed application the settings do not list is not.
        assert!(!is_game(
            &mode(&["other.exe"]),
            &front(Some(r"C:\Games\game.exe"), false)
        ));
        // Whether game mode is on is the launcher's to say, not the
        // decision's: it decides on a window only while it is.
        assert!(is_game(&GameMode::default(), &front(None, true)));
    }

    #[test]
    fn a_listed_program_in_front_is_a_game_windowed_or_not() {
        // The program is matched by the file name of the window's
        // process's program, whatever its case and wherever it is
        // installed, so a windowed game pauses the hotkeys too.
        for (program, listed) in [
            (r"C:\Games\Helldivers.exe", "helldivers.exe"),
            (r"D:\Steam\steamapps\common\Game\Game.exe", "GAME.EXE"),
            (r"C:\Games\game.exe", r"C:\Games\game.exe"),
        ] {
            assert!(
                is_game(&mode(&[listed]), &front(Some(program), false)),
                "{program} is not a listed game"
            );
        }
        // Another program, or a window whose program could not be read,
        // is not.
        assert!(!is_game(
            &mode(&["game.exe"]),
            &front(Some(r"C:\Windows\notepad.exe"), false)
        ));
        assert!(!is_game(&mode(&["game.exe"]), &front(None, false)));
    }

    #[test]
    fn no_record_means_off_with_no_programs() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(GameMode::open(dir.path()), GameMode::default());
        assert!(!GameMode::default().on);
        assert!(GameMode::default().programs.is_empty());
    }

    #[test]
    fn a_record_round_trips_and_its_fields_default() {
        let dir = tempfile::tempdir().unwrap();
        let settings = mode(&["helldivers.exe", "game.exe"]);
        settings.save(dir.path()).unwrap();
        assert_eq!(GameMode::open(dir.path()), settings);
        let text = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        for field in ["\"version\": 1", "\"on\": true", "\"programs\""] {
            assert!(text.contains(field), "the record is {text}");
        }
        // Missing fields read as the defaults, as a record an older Pane
        // wrote does.
        assert_eq!(reading(r#"{ "version": 1 }"#), GameMode::default());
        // Off is written only while it is on, so an off record holds the
        // default.
        GameMode::default().save(dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(FILE)).unwrap(),
            "{\n  \"version\": 1\n}"
        );
        assert_eq!(GameMode::open(dir.path()), GameMode::default());
    }

    #[test]
    fn a_program_that_is_not_a_name_cannot_be_recorded_and_fails_the_record() {
        assert_eq!(mode(&["game.exe", "Game 2.EXE"]).refusal(), None);
        for bad in [
            "",
            "  ",
            "C:\\Games\\game.exe",
            "games/game.exe",
            "C:game.exe",
            ".",
            "..",
        ] {
            let why = mode(&[bad]).refusal();
            assert!(why.is_some(), "{bad:?} is recordable");
            assert!(why.unwrap().contains("is not a program name"), "{bad:?}");
            let dir = tempfile::tempdir().unwrap();
            assert!(mode(&[bad]).save(dir.path()).is_err(), "{bad:?} was saved");
            assert!(!dir.path().join(FILE).exists());
        }
        // A record holding one is not read, so it is never mistaken for
        // the defaults it half-names; it is never replaced by reading.
        assert_eq!(
            reading(r#"{ "version": 1, "on": true, "programs": ["C:\\Games\\game.exe"] }"#),
            GameMode::default()
        );
    }

    #[test]
    fn another_version_s_record_is_not_read() {
        for text in [
            r#"{ "version": 2, "on": true }"#,
            r#"{ "on": true }"#,
            "{ not a record",
        ] {
            assert_eq!(reading(text), GameMode::default(), "{text} half-loads");
        }
    }

    #[test]
    fn a_failed_write_leaves_the_previous_record_whole() {
        let dir = tempfile::tempdir().unwrap();
        mode(&["game.exe"]).save(dir.path()).unwrap();
        // A folder where the record belongs: the atomic replacement
        // fails, and the record it would have replaced is still the old
        // one.
        std::fs::remove_file(dir.path().join(FILE)).unwrap();
        std::fs::create_dir(dir.path().join(FILE)).unwrap();
        let failed = mode(&["other.exe"]).save(dir.path());
        assert!(failed.is_err(), "{failed:?}");
        assert!(dir.path().join(FILE).is_dir());
        // The temporary files of the failed write are cleaned up.
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [FILE]);
    }
}
