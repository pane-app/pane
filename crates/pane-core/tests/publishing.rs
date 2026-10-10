//! Root search's publishing of a query's list (#201), through the
//! launcher's public interface, with the `faulty` fixture's slow provider
//! — the query "0 + 0", answered after about a second of busy work — and
//! the real calculator. A query's list is published once every provider
//! asked has answered, or 200 ms after the query changed, whichever comes
//! first: until then the rows shown stay the previous query's while the
//! field shows what was typed. An answer arriving after the list was
//! published is merged into it, coalesced within 16 ms, moving the
//! selection to the new first row when the first row was selected — a
//! preselected fallback (ADR 0031) gives way to a late result — and never
//! taking it from a row the user moved to.
//!
//! The clock is a manual one (`Launcher::with_clock`): the budget and the
//! coalescing are timed by it, so advancing the clock is what publishes
//! what is due, exactly when the tests say so.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use futures::executor::block_on;
use pane_core::clipboard::ManualClock;
use pane_core::{Launcher, Limits, Runtime, Status};

#[path = "support/rows.rs"]
mod rows;

use rows::titles;

fn built(path: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(path);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// The assembled calculator package's folder.
fn calculator() -> PathBuf {
    built("packages/calculator")
}

/// The assembled sample package holding Echo, which takes a query.
fn echo() -> PathBuf {
    built("packages/sample-query")
}

/// A manifest for a package titled `title` with one command of the same
/// title.
fn manifest(title: &str) -> String {
    format!(
        r#"{{ "manifestVersion": 1, "title": "{title}", "apiVersion": "0.1",
  "commands": [{{ "id": "command", "title": "{title}", "component": "command.wasm" }}] }}"#
    )
}

/// A manifest for a package titled `title` whose one command computes root
/// results from the query.
fn manifest_computing(title: &str) -> String {
    format!(
        r#"{{ "manifestVersion": 1, "title": "{title}", "apiVersion": "0.1",
  "commands": [{{ "id": "command", "title": "{title}", "component": "command.wasm",
    "rootResults": true }}] }}"#
    )
}

/// The slow fixture's package folder: a root provider that answers
/// "0 + 0" after about a second of busy work — well past the budget — and
/// any other query at once, with nothing.
fn slow(dirs: &Dirs) -> PathBuf {
    dirs.package("slow", &manifest_computing("Slow"), &built("faulty.wasm"))
}

/// A plain command titled "Sum 0 + 0", which the slow query matches by
/// title, so the list the budget publishes has something in it.
fn sums(dirs: &Dirs) -> PathBuf {
    dirs.package("sums", &manifest("Sum 0 + 0"), &built("sample_rust.wasm"))
}

/// A plain command titled "Drill 0 + 0", which the slow query matches by
/// title too.
fn drills(dirs: &Dirs) -> PathBuf {
    dirs.package(
        "drills",
        &manifest("Drill 0 + 0"),
        &built("sample_rust.wasm"),
    )
}

/// Pane's data location and compiled code cache for one test, with its
/// sources.
struct Dirs {
    sources: tempfile::TempDir,
    data: tempfile::TempDir,
    cache: tempfile::TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
        }
    }

    fn runtime(&self) -> Runtime {
        Runtime::start_with_cache(self.cache.path().to_path_buf()).unwrap()
    }

    fn launcher(&self, runtime: Runtime) -> Launcher {
        Launcher::with_packages(Ok(runtime), vec![], self.data.path().join("extensions"))
    }

    /// A package folder named `name` holding `manifest` and `component` as
    /// `command.wasm`.
    fn package(&self, name: &str, manifest: &str, component: &Path) -> PathBuf {
        let folder = self.sources.path().join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("pane.json"), manifest).unwrap();
        fs::copy(component, folder.join("command.wasm")).unwrap();
        folder
    }
}

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
}

fn selected_title(launcher: &Launcher) -> Option<String> {
    let view = launcher.view();
    view.selected.map(|index| view.rows[index].title.clone())
}

/// Searches for `query` and waits for its list to be published.
fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

/// Runs `launcher.set_query(query)` on a thread of its own, returning what
/// hears once it has finished: a provider that misses the budget keeps
/// answering there, whatever the clock says.
fn search_in_background(launcher: &Launcher, query: &str) -> std::sync::mpsc::Receiver<()> {
    let searching = launcher.set_query(query);
    let (done, finished) = std::sync::mpsc::channel();
    thread::spawn(move || {
        block_on(searching);
        let _ = done.send(());
    });
    finished
}

/// A launcher with the slow fixture and `packages` installed, in order, by
/// the manual clock at 1_000.
fn slow_launcher(dirs: &Dirs, packages: &[PathBuf]) -> (Launcher, Arc<ManualClock>) {
    let runtime = dirs.runtime();
    // The slow fixture computes for about a second, which a loaded machine
    // can stretch past the default 5-second computing limit; waiting
    // longer is each test's own business.
    runtime.set_limits(Limits {
        compute: Duration::from_secs(180),
        ..Limits::default()
    });
    let launcher = dirs.launcher(runtime);
    install(&launcher, &slow(dirs));
    for folder in packages {
        install(&launcher, folder);
    }
    let clock = ManualClock::at(1_000);
    (launcher.with_clock(clock.clone()), clock)
}

#[test]
fn a_slow_provider_holds_the_list_up_to_the_budget_then_merges_late() {
    let dirs = Dirs::new();
    let (launcher, clock) = slow_launcher(&dirs, &[sums(&dirs)]);

    // The previous query's list, published with the slow provider's answer
    // of nothing for another query.
    search(&launcher, "sum");
    assert_eq!(titles(&launcher), ["Sum 0 + 0"]);

    // The slow query: its list is held while its provider answers — the
    // field shows the query typed at once, the rows stay the previous
    // query's — and it is held up to the budget.
    let answered = search_in_background(&launcher, "0 + 0");
    assert_eq!(launcher.view().query(), Some("0 + 0"));
    assert!(
        !launcher.list_published(),
        "the query's list is held while its providers answer"
    );
    assert_eq!(titles(&launcher), ["Sum 0 + 0"]);
    clock.advance(Duration::from_millis(199));
    assert!(!launcher.list_published(), "the budget has not ended");
    assert_eq!(titles(&launcher), ["Sum 0 + 0"]);

    // The budget ended: the list is published without the slow provider's
    // answer, which is still being computed.
    clock.advance(Duration::from_millis(1));
    assert!(launcher.list_published());
    assert_eq!(titles(&launcher), ["Sum 0 + 0"]);

    // The call goes on until it answers: the late answer merges into the
    // published list, coalesced within 16 ms.
    answered.recv_timeout(Duration::from_secs(240)).unwrap();
    assert_eq!(
        titles(&launcher),
        ["Sum 0 + 0"],
        "the merge waits out its coalescing"
    );
    clock.advance(Duration::from_millis(16));
    assert_eq!(titles(&launcher), ["Slow answer", "Sum 0 + 0"]);
    assert!(launcher.list_published());
}

#[test]
fn a_late_answer_moves_the_selection_from_the_first_row() {
    let dirs = Dirs::new();
    let (launcher, clock) = slow_launcher(&dirs, &[sums(&dirs), drills(&dirs)]);

    // The budget publishes the query's metadata list, its first row
    // selected.
    let answered = search_in_background(&launcher, "0 + 0");
    clock.advance(Duration::from_millis(200));
    assert!(launcher.list_published());
    assert_eq!(titles(&launcher), ["Sum 0 + 0", "Drill 0 + 0"]);
    assert_eq!(selected_title(&launcher).as_deref(), Some("Sum 0 + 0"));

    // The late answer's row goes first: with the first row selected, the
    // selection moves to the new first row.
    answered.recv_timeout(Duration::from_secs(240)).unwrap();
    clock.advance(Duration::from_millis(16));
    assert_eq!(
        titles(&launcher),
        ["Slow answer", "Sum 0 + 0", "Drill 0 + 0"]
    );
    assert_eq!(selected_title(&launcher).as_deref(), Some("Slow answer"));
}

#[test]
fn a_row_the_user_moved_to_stays_selected_when_a_late_answer_arrives() {
    let dirs = Dirs::new();
    let (launcher, clock) = slow_launcher(&dirs, &[sums(&dirs), drills(&dirs)]);

    let answered = search_in_background(&launcher, "0 + 0");
    clock.advance(Duration::from_millis(200));
    assert_eq!(titles(&launcher), ["Sum 0 + 0", "Drill 0 + 0"]);
    // The user moves the selection.
    launcher.move_selection(1);
    assert_eq!(selected_title(&launcher).as_deref(), Some("Drill 0 + 0"));

    // The late answer's section never takes the selection from a row the
    // user moved to.
    answered.recv_timeout(Duration::from_secs(240)).unwrap();
    clock.advance(Duration::from_millis(16));
    assert_eq!(
        titles(&launcher),
        ["Slow answer", "Sum 0 + 0", "Drill 0 + 0"]
    );
    assert_eq!(selected_title(&launcher).as_deref(), Some("Drill 0 + 0"));
}

#[test]
fn a_preselected_fallback_gives_way_to_a_late_answer() {
    let dirs = Dirs::new();
    // Echo, offered as a fallback for any text typed: with nothing else
    // matching the slow query, its row is the list the budget publishes,
    // preselected (ADR 0031).
    let (launcher, clock) = slow_launcher(&dirs, &[echo()]);
    rows::manage(&launcher);
    rows::select_title(&launcher, "Fallback: Echo");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Echo is now offered for any text typed in root search".into())
    );
    rows::to_root(&launcher);

    let answered = search_in_background(&launcher, "0 + 0");
    clock.advance(Duration::from_millis(200));
    assert!(launcher.list_published());
    assert_eq!(titles(&launcher), ["Echo"]);
    assert_eq!(
        launcher.view().selected,
        Some(0),
        "the first fallback is preselected"
    );

    // The late answer's row goes first: the preselected fallback gives
    // way, as the first row was selected.
    answered.recv_timeout(Duration::from_secs(240)).unwrap();
    clock.advance(Duration::from_millis(16));
    assert_eq!(titles(&launcher), ["Slow answer", "Echo"]);
    assert_eq!(selected_title(&launcher).as_deref(), Some("Slow answer"));
}

#[test]
fn late_answers_arriving_close_together_merge_into_one_update() {
    let dirs = Dirs::new();
    // The calculator, asked after the slow fixture, answers "0 + 0" as
    // soon as the runtime is free of the slow call — right behind its
    // answer, close enough to coalesce with it.
    let (launcher, clock) = slow_launcher(&dirs, &[calculator(), sums(&dirs)]);

    let answered = search_in_background(&launcher, "0 + 0");
    clock.advance(Duration::from_millis(200));
    assert_eq!(titles(&launcher), ["Sum 0 + 0"]);

    // Both answers arrive, close together, after the list was published.
    answered.recv_timeout(Duration::from_secs(240)).unwrap();
    // Neither is listed yet: the coalescing holds them back, so the two
    // become one update.
    assert_eq!(
        titles(&launcher),
        ["Sum 0 + 0"],
        "the late answers are coalescing"
    );
    clock.advance(Duration::from_millis(16));
    assert_eq!(titles(&launcher), ["Slow answer", "0", "Sum 0 + 0"]);
    assert_eq!(selected_title(&launcher).as_deref(), Some("Slow answer"));
    assert!(launcher.list_published());
}

#[test]
fn a_query_that_asks_no_provider_is_published_at_once() {
    let dirs = Dirs::new();
    let launcher = dirs
        .launcher(dirs.runtime())
        .with_clock(ManualClock::at(1_000));
    install(&launcher, &sums(&dirs));

    // No provider is asked: the query's list is published at once, the
    // metadata ranked as it always was.
    let pending = launcher.set_query("sum");
    assert!(launcher.list_published());
    assert_eq!(titles(&launcher), ["Sum 0 + 0"]);
    block_on(pending);
    assert!(launcher.list_published());

    // The blank query asks none either.
    search(&launcher, "");
    assert!(launcher.list_published());
    assert!(titles(&launcher).contains(&"Sum 0 + 0".to_owned()));
}

#[test]
fn a_late_answer_merges_without_ranking_the_static_rows_again() {
    let dirs = Dirs::new();
    let (merged, clock) = slow_launcher(&dirs, &[sums(&dirs), drills(&dirs)]);

    // The budget publishes the query's metadata list without the slow
    // provider's answer, ranking the static rows once for the query.
    let answered = search_in_background(&merged, "0 + 0");
    clock.advance(Duration::from_millis(200));
    assert_eq!(titles(&merged), ["Sum 0 + 0", "Drill 0 + 0"]);
    let ranked = merged.root_rankings();

    // The late answer merges into the published list without ranking the
    // rows that did not change (#202): the computed section is spliced
    // into the rows ranked for the query.
    answered.recv_timeout(Duration::from_secs(240)).unwrap();
    clock.advance(Duration::from_millis(16));
    assert_eq!(merged.root_rankings(), ranked, "the merge ranked nothing");
    assert_eq!(titles(&merged), ["Slow answer", "Sum 0 + 0", "Drill 0 + 0"]);
    assert_eq!(selected_title(&merged).as_deref(), Some("Slow answer"));

    // The list is what a full re-rank of the same rows gives: a second
    // launcher, asked the same way, has the slow provider answer while
    // its list is held, so its publication ranks everything together.
    let other = Dirs::new();
    let (full, _clock) = slow_launcher(&other, &[sums(&other), drills(&other)]);
    search(&full, "0 + 0");
    assert_eq!(titles(&full), ["Slow answer", "Sum 0 + 0", "Drill 0 + 0"]);
    assert_eq!(selected_title(&full).as_deref(), Some("Slow answer"));
}

#[test]
fn an_answer_for_an_older_search_is_discarded_as_before() {
    let dirs = Dirs::new();
    let (launcher, clock) = slow_launcher(&dirs, &[sums(&dirs)]);

    // The slow query is asked, and a newer search replaces it before its
    // call answered: the answer, arriving for the earlier search, never
    // shows.
    let earlier = launcher.set_query("0 + 0");
    search(&launcher, "sum");
    assert_eq!(titles(&launcher), ["Sum 0 + 0"]);
    assert!(launcher.list_published());
    clock.advance(Duration::from_millis(200));
    block_on(earlier);
    assert_eq!(titles(&launcher), ["Sum 0 + 0"]);
    assert!(launcher.list_published());
}
