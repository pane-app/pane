//! Attention launches through the launcher's public interface (ADR 0053,
//! #557): a user-initiated `launch` of one of the calling package's own
//! view commands, asked from a call no window was shown for (a timer's
//! firing, a schedule, a service). The launcher queues it and shows it on
//! the command, with its launch record and context, once the window asks
//! ([`Launcher::attend`]) and nothing holds it back: the user typing, an
//! Alert pending, Game mode holding the hotkeys, or another attention
//! launch's screen still shown. How long the keyboard and the pointer
//! have been still is what each test tells `attend`, so none needs a real
//! desktop; Game mode's foreground source is a fake, as in `game_mode.rs`.
//! The calls stand in for the guest's (`Launcher::launch_as`); the command
//! is the no-view sample's view command "Show launch", which lists its
//! launch record.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::clipboard::{Clock, ManualClock, SystemClock};
use pane_core::feedback::WindowPresence;
use pane_core::game_mode::{Foreground, ForegroundSource, ForegroundTold, GameMode};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{LaunchType, Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::{manage, titles};

/// How long a launch may take: compiling the guest once is included.
const PROMPTLY: Duration = Duration::from_secs(30);

/// The keyboard and the pointer still for long enough.
const STILL: Option<Duration> = Some(Duration::from_secs(5));

/// The user typing: input a moment ago.
const TYPING: Option<Duration> = Some(Duration::from_millis(200));

/// A system whose global hotkeys always register.
#[derive(Default)]
struct FakeHotkeys;

impl Hotkeys for FakeHotkeys {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, _shortcut: &Shortcut) -> Result<(), HotkeyError> {
        Ok(())
    }

    fn unregister(&self, _shortcut: &Shortcut) {}
}

/// The foreground source as the tests fake it, as `game_mode.rs` does.
#[derive(Default)]
struct FakeForeground {
    told: Mutex<Option<ForegroundTold>>,
}

impl FakeForeground {
    /// `program`'s window came to the front, full screen or not.
    fn front(&self, program: &str, full_screen: bool) {
        let told = self.told.lock().unwrap().clone();
        if let Some(told) = told {
            told.front(&Foreground {
                program: Some(program.to_owned()),
                full_screen,
            });
        }
    }
}

impl ForegroundSource for FakeForeground {
    fn watch(&self, told: ForegroundTold) {
        *self.told.lock().unwrap() = Some(told);
    }
}

/// One test's Pane: its folders, its clock, its foreground and the
/// launcher, its window hidden.
struct Pane {
    sources: TempDir,
    _data: TempDir,
    clock: Arc<ManualClock>,
    foreground: Arc<FakeForeground>,
    launcher: Launcher,
}

impl Pane {
    fn new() -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let clock = ManualClock::at(SystemClock.now());
        let foreground = Arc::new(FakeForeground::default());
        let launcher =
            Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
                .with_clock(clock.clone())
                .with_hotkeys(Arc::new(FakeHotkeys))
                .with_foreground(foreground.clone());
        launcher.set_window_presence(WindowPresence::Hidden);
        Pane {
            sources,
            _data: data,
            clock,
            foreground,
            launcher,
        }
    }

    /// Installs the no-view sample from the source folder `folder`; its
    /// identity.
    fn install(&self, folder: &str) -> PackageIdentity {
        let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages/sample-no-view");
        assert!(
            assembled.exists(),
            "{} is missing; run `cargo xtask guests`",
            assembled.display()
        );
        let folder = self.sources.path().join(folder);
        copy(&assembled, &folder);
        block_on(self.launcher.install_package(&folder));
        assert_eq!(
            self.launcher.view().status,
            Status::Result("Installed No-view sample".into())
        );
        PackageIdentity::local(&folder).unwrap()
    }

    /// `caller` asks for a user-initiated launch of its own "Show launch"
    /// with `context`, from a call no window was shown for.
    fn ask(&self, caller: &PackageIdentity, context: &str) -> Result<(), String> {
        self.launcher.launch_as(
            caller,
            None,
            "show",
            LaunchType::UserInitiated,
            Some(context),
            false,
        )
    }

    /// The window asks whether to show a waiting launch, the keyboard and
    /// the pointer still for `idle`; whether one was shown, once it opened.
    fn attend(&self, idle: Option<Duration>) -> bool {
        let shown = self.launcher.attend(idle);
        assert!(self.launcher.wait_for_launches(PROMPTLY));
        shown
    }

    /// The window shows itself, as it does when the launcher asks it to.
    fn show_window(&self) {
        self.launcher.set_window_presence(WindowPresence::Shown);
    }

    /// The context the shown "Show launch" received, once it is on screen
    /// and the launcher asked for the window with the focus.
    fn opened(&self) -> String {
        let launcher = &self.launcher;
        assert!(launcher.take_window_request(), "the window is asked for");
        assert!(launcher.take_focus_request(), "with the keyboard focus");
        self.show_window();
        let view = launcher.view();
        assert_eq!(view.screen, Screen::Command, "{view:?}");
        let rows = titles(launcher);
        assert!(
            rows.iter().any(|row| row == "Launch type: user-initiated"),
            "{rows:?}"
        );
        assert!(rows.iter().any(|row| row == "Source: command"), "{rows:?}");
        rows.iter()
            .find_map(|row| row.strip_prefix("Context: "))
            .expect("its context is shown")
            .to_owned()
    }

    /// The user leaves the screen shown, back to root search.
    fn leave(&self) {
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
    }
}

/// Copies the files of `from` into the new folder `to`.
fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
}

#[test]
fn a_hidden_launcher_shows_the_callers_command_with_its_context_and_the_focus() {
    let pane = Pane::new();
    let alarm = pane.install("alarm");
    let launcher = &pane.launcher;

    // Accepted means queued: nothing is shown until the window asks.
    assert_eq!(pane.ask(&alarm, r#"{"alarm":"07:00"}"#), Ok(()));
    assert!(launcher.attention_waiting());
    assert!(!launcher.take_window_request());

    assert!(pane.attend(STILL));
    assert_eq!(pane.opened(), r#"{"alarm":"07:00"}"#);
    assert!(!launcher.attention_waiting());
    // Asked once per showing.
    assert!(!launcher.take_focus_request());
    // Escape acts on the opened view: back to root search.
    assert!(launcher.back());
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
}

#[test]
fn the_user_typing_holds_it_for_a_second_of_stillness_at_most_ten_seconds() {
    let pane = Pane::new();
    let alarm = pane.install("alarm");

    pane.ask(&alarm, "1").unwrap();
    assert!(!pane.attend(TYPING));
    pane.clock.advance(Duration::from_secs(5));
    assert!(!pane.attend(TYPING), "still typing, 5 s on");
    assert!(
        !pane.attend(Some(Duration::from_millis(900))),
        "not yet still for a second"
    );
    assert!(pane.attend(Some(Duration::from_secs(1))));
    assert_eq!(pane.opened(), "1");
    pane.leave();

    // Typing on for 10 seconds: shown anyway.
    pane.ask(&alarm, "2").unwrap();
    assert!(!pane.attend(TYPING));
    pane.clock.advance(Duration::from_millis(9_900));
    assert!(!pane.attend(TYPING));
    pane.clock.advance(Duration::from_millis(100));
    assert!(pane.attend(TYPING), "at most 10 s");
    assert_eq!(pane.opened(), "2");

    // A system that cannot tell holds nothing back.
    pane.leave();
    pane.ask(&alarm, "3").unwrap();
    assert!(pane.attend(None));
    assert_eq!(pane.opened(), "3");
}

#[test]
fn the_launcher_open_on_another_screen_makes_way() {
    let pane = Pane::new();
    let alarm = pane.install("alarm");
    let launcher = &pane.launcher;
    pane.show_window();
    manage(launcher);
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));

    pane.ask(&alarm, r#""open""#).unwrap();
    assert!(pane.attend(STILL));
    assert_eq!(pane.opened(), r#""open""#);
}

#[test]
fn a_pending_alert_holds_it_until_it_is_answered() {
    let pane = Pane::new();
    let alarm = pane.install("alarm");
    let launcher = &pane.launcher;
    pane.show_window();
    manage(launcher);
    rows::select_title(launcher, "Clear cache of No-view sample");
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Confirm { .. }));

    pane.ask(&alarm, r#""after the alert""#).unwrap();
    assert!(!pane.attend(STILL), "the Alert is pending");
    pane.clock.advance(Duration::from_secs(60));
    assert!(!pane.attend(STILL), "however long it takes");
    assert!(matches!(launcher.view().screen, Screen::Confirm { .. }));

    // Cancel answers it.
    launcher.back();
    assert!(pane.attend(STILL));
    assert_eq!(pane.opened(), r#""after the alert""#);
}

#[test]
fn game_mode_holding_the_hotkeys_holds_it_until_the_game_leaves_the_front() {
    let pane = Pane::new();
    let alarm = pane.install("alarm");
    let launcher = &pane.launcher;
    block_on(launcher.set_game_mode(GameMode {
        on: true,
        programs: Vec::new(),
    }))
    .expect("the settings are recorded");
    pane.foreground.front(r"C:\Games\game.exe", true);
    assert!(launcher.hotkeys_paused());

    pane.ask(&alarm, r#""after the game""#).unwrap();
    assert!(!pane.attend(STILL), "not over the game");
    pane.clock.advance(Duration::from_secs(60));
    assert!(!pane.attend(STILL));

    pane.foreground.front(r"C:\Windows\explorer.exe", false);
    assert!(!launcher.hotkeys_paused());
    assert!(pane.attend(STILL));
    assert_eq!(pane.opened(), r#""after the game""#);
}

#[test]
fn two_extensions_queue_one_at_a_time_and_a_newer_launch_replaces_its_older_one() {
    let pane = Pane::new();
    let alarm = pane.install("alarm");
    let timer = pane.install("timer");

    pane.ask(&alarm, r#""alarm 1""#).unwrap();
    pane.ask(&timer, r#""timer""#).unwrap();
    // The alarm's newer launch takes its older one's place, first.
    pane.ask(&alarm, r#""alarm 2""#).unwrap();

    assert!(pane.attend(STILL));
    assert_eq!(pane.opened(), r#""alarm 2""#);
    // The timer's never takes the alarm's screen while it is shown.
    assert!(pane.launcher.attention_waiting());
    assert!(!pane.attend(STILL));
    assert_eq!(pane.launcher.view().screen, Screen::Command);

    // Back to root search: the next is shown.
    pane.leave();
    assert!(pane.attend(STILL));
    assert_eq!(pane.opened(), r#""timer""#);
    assert!(
        !pane.launcher.attention_waiting(),
        "the alarm's first is gone"
    );

    // The window hidden: the user left it too.
    pane.ask(&alarm, r#""alarm 3""#).unwrap();
    assert!(!pane.attend(STILL));
    pane.launcher.set_window_presence(WindowPresence::Hidden);
    assert!(pane.attend(STILL));
    assert_eq!(pane.opened(), r#""alarm 3""#);
}

#[test]
fn another_packages_command_is_refused_but_from_a_call_with_a_window_still_launches() {
    let pane = Pane::new();
    let alarm = pane.install("alarm");
    let other = pane.install("other");
    let launcher = &pane.launcher;
    let key = other.key();

    let refused = launcher
        .launch_as(
            &alarm,
            Some(&key),
            "show",
            LaunchType::UserInitiated,
            Some("1"),
            false,
        )
        .unwrap_err();
    assert!(
        refused.contains("is another extension") && refused.contains("only its own"),
        "{refused}"
    );
    assert!(!launcher.attention_waiting());

    // Its own package named by its identity is its own.
    let own = alarm.key();
    assert_eq!(
        launcher.launch_as(
            &alarm,
            Some(&own),
            "show",
            LaunchType::UserInitiated,
            Some("2"),
            false
        ),
        Ok(())
    );
    assert!(launcher.attention_waiting());
    assert!(pane.attend(STILL));
    assert_eq!(pane.opened(), "2");

    // From a call with a window, the user's own action: unchanged, shown
    // at once, without waiting its turn or taking the focus.
    pane.leave();
    assert_eq!(
        launcher.launch_as(
            &alarm,
            Some(&key),
            "show",
            LaunchType::UserInitiated,
            Some("3"),
            true
        ),
        Ok(())
    );
    assert!(launcher.wait_for_launches(PROMPTLY));
    assert!(!launcher.attention_waiting());
    assert!(launcher.take_window_request());
    assert!(!launcher.take_focus_request());
    assert!(titles(launcher).iter().any(|row| row == "Context: 3"));
}

#[test]
fn a_command_that_cannot_be_shown_is_refused() {
    let pane = Pane::new();
    let alarm = pane.install("alarm");
    let launcher = &pane.launcher;
    let ask = |command: &str| {
        launcher.launch_as(
            &alarm,
            None,
            command,
            LaunchType::UserInitiated,
            None,
            false,
        )
    };

    let no_view = ask("report").unwrap_err();
    assert!(no_view.contains("runs without a view"), "{no_view}");
    assert_eq!(
        ask("nothing").unwrap_err(),
        "No-view sample has no command `nothing`"
    );
    // A background launch from such a call is unchanged.
    assert_eq!(
        launcher.launch_as(&alarm, None, "report", LaunchType::Background, None, false),
        Ok(())
    );
    assert!(launcher.wait_for_launches(PROMPTLY));

    block_on(launcher.set_enabled(&alarm, false));
    let disabled = ask("show").unwrap_err();
    assert!(disabled.contains("is disabled"), "{disabled}");
    assert!(!launcher.attention_waiting());
    assert!(!launcher.take_window_request());
}

#[test]
fn a_waiting_launch_is_dropped_when_its_generation_ends() {
    let pane = Pane::new();
    let alarm = pane.install("alarm");
    let launcher = &pane.launcher;

    pane.ask(&alarm, r#""never""#).unwrap();
    assert!(!pane.attend(TYPING));
    assert!(launcher.attention_waiting());
    block_on(launcher.set_enabled(&alarm, false));
    assert!(!launcher.attention_waiting(), "dropped");
    block_on(launcher.set_enabled(&alarm, true));
    assert!(!pane.attend(STILL), "never shown");
    assert!(!launcher.take_window_request());
}
