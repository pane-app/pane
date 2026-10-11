//! Root search through the launcher's public interface: a query narrows root
//! search to the matching commands, best match first, from command metadata
//! alone; only the command the user invokes starts a guest. Installed
//! packages are built in temporary folders around the real guest components
//! from `cargo xtask guests`.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{
    CallError, CommandRegistration, Launcher, PackageIdentity, Runtime, Screen, SearchSensitivity,
    Status, Unavailable,
};
use tempfile::TempDir;

#[path = "support/platforms.rs"]
mod platforms;

#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use guests::guest;
use rows::titles;

const CREATE_ROW: &str = "Create Extension…";
const IMPORT_ROW: &str = "Import Extension…";
const INSTALL_ROW: &str = "Install extension from folder…";
const NPM_ROW: &str = "Install extension from npm…";
const GIT_ROW: &str = "Install extension from Git…";
const MANAGE_ROW: &str = "Manage Extensions";

/// A command built into the launcher, backed by the Rust sample.
fn command(title: &str, subtitle: Option<&str>) -> CommandRegistration {
    command_titled(&title.to_lowercase().replace(' ', "-"), title, subtitle)
}

/// A command built into the launcher with an id of its own, for rows that
/// would otherwise share one.
fn command_titled(id: &str, title: &str, subtitle: Option<&str>) -> CommandRegistration {
    CommandRegistration {
        id: id.into(),
        title: title.into(),
        subtitle: subtitle.map(Into::into),
        component: guest("sample_rust"),
        takes_query: false,
        search: false,
        keywords: Vec::new(),
        when: pane_core::CommandWhen::Always,
        matches: pane_core::CommandMatches::Title,
    }
}

/// A launcher holding `commands`, on a data folder of its own so the
/// records the tests give them stick.
fn with_commands(dirs: &Dirs, commands: Vec<CommandRegistration>) -> Launcher {
    Launcher::with_packages(Ok(dirs.runtime()), commands, dirs.packages_dir())
}

/// A launcher whose runtime never started: searching runs no guest.
fn without_runtime(commands: Vec<CommandRegistration>) -> Launcher {
    let unavailable = Err(CallError::RuntimeUnavailable("no engine".into()));
    Launcher::new(unavailable, commands)
}

fn selected_title(launcher: &Launcher) -> Option<String> {
    let view = launcher.view();
    view.selected.map(|index| view.rows[index].title.clone())
}

fn downloads() -> Launcher {
    without_runtime(vec![
        command("Clear cache", Some("Delete downloaded files")),
        command("Undownloadable files", None),
        command("Recent downloads", None),
        command("Downloader", None),
        command("Download", None),
        command("Settings", None),
    ])
}

#[test]
fn root_search_opens_with_an_empty_query_listing_every_result_in_the_no_query_order() {
    let launcher = downloads();
    let view = launcher.view();
    assert_eq!(view.query(), Some(""));
    // Nothing is learned yet, so the blank query's order is the no-query
    // order without its frecency: having an alias, then kind, provider,
    // then the title (#199). Pane's own rows rank with the commands.
    assert_eq!(
        titles(&launcher),
        [
            "Clear cache",
            "Download",
            "Downloader",
            "Recent downloads",
            "Settings",
            "Settings…",
            "Undownloadable files"
        ]
    );
    assert_eq!(view.selected, Some(0));
}

#[test]
fn a_query_keeps_the_matching_commands_best_match_first() {
    let launcher = downloads();
    // The ladder holds the whole order at Low; High (the default) drops
    // the mid-word containment (see
    // `search_sensitivity_decides_how_good_a_score_must_be`).
    launcher.set_search_sensitivity(SearchSensitivity::Low);
    block_on(launcher.set_query("download"));

    assert_eq!(launcher.view().query(), Some("download"));
    // The query is the whole title (step 2), then the better scores (step
    // 8): a title that starts with the query, a word of it, a subtitle
    // that starts a word, and last a mid-word containment.
    assert_eq!(
        titles(&launcher),
        [
            "Download",
            "Downloader",
            "Recent downloads",
            "Clear cache",
            "Undownloadable files"
        ]
    );
    assert_eq!(selected_title(&launcher).as_deref(), Some("Download"));
}

#[test]
fn matching_ignores_letter_case_and_surrounding_spaces() {
    let launcher = downloads();
    block_on(launcher.set_query("  DOWNLOADER "));
    assert_eq!(titles(&launcher), ["Downloader"]);
    block_on(launcher.set_query("   "));
    assert_eq!(titles(&launcher).len(), 7, "a blank query lists everything");
}

#[test]
fn spaces_inside_and_around_a_title_do_not_lower_its_rank() {
    let launcher = without_runtime(vec![
        command("Clear cache and history", None),
        command("Settings for clear cache", None),
        command("Clear  cache ", None),
        command("Clear  cache  files", None),
    ]);
    block_on(launcher.set_query("clear cache"));
    // The query is the whole title (step 2), then titles that start with
    // it (steps 8 and 10), then one whose words start the query's.
    assert_eq!(
        titles(&launcher),
        [
            "Clear  cache ",
            "Clear cache and history",
            "Clear  cache  files",
            "Settings for clear cache"
        ]
    );
}

#[test]
fn composed_and_decomposed_accents_match_each_other() {
    // "é" as one character (NFC) and as "e" plus a combining accent (NFD).
    let launcher = without_runtime(vec![
        command("Cafe\u{301} menu", None),
        command("Résumé", None),
        command("Directions", Some("To the café")),
    ]);
    block_on(launcher.set_query("café"));
    assert_eq!(titles(&launcher), ["Cafe\u{301} menu", "Directions"]);
    block_on(launcher.set_query("RE\u{301}SUME\u{301}"));
    assert_eq!(titles(&launcher), ["Résumé"]);
}

/// Fuzzy matching without accents (#193): the query's letters place in a
/// result's title, subtitle or package title as a subsequence, and
/// transliteration folds away the accents and diacritics in between.
#[test]
fn an_abbreviation_finds_the_command_by_its_word_starts() {
    let launcher = without_runtime(vec![
        command("Visual Studio Code", None),
        command("Clear History", None),
        command("Clipboard History", None),
    ]);
    block_on(launcher.set_query("vsc"));
    assert_eq!(titles(&launcher), ["Visual Studio Code"]);
    // A match the scorer found alone: the tighter placements (steps 8
    // and 10) first, and titles collate what they cannot tell apart.
    block_on(launcher.set_query("clhis"));
    assert_eq!(
        titles(&launcher),
        ["Clear History", "Clipboard History"],
        "Clear History's letters sit closer than Clipboard's"
    );
}

#[test]
#[allow(clippy::single_range_in_vec_init)]
fn accents_and_diacritics_never_stand_between_the_query_and_the_result() {
    let launcher = without_runtime(vec![
        command("Café", None),
        command("Tiếng Việt", None),
        command("Đường", Some("A street in Hà Nội")),
        command("Straße", None),
    ]);
    // Both ways: without the accents, and with them on the other side.
    block_on(launcher.set_query("cafe"));
    assert_eq!(titles(&launcher), ["Café"]);
    block_on(launcher.set_query("tieng viet"));
    assert_eq!(titles(&launcher), ["Tiếng Việt"]);
    // The highlight maps back to the title as written: "duong" is all of
    // "Đường".
    block_on(launcher.set_query("duong"));
    assert_eq!(titles(&launcher), ["Đường"]);
    let ranges = launcher.presentation().rows[0].matched.clone();
    assert_eq!(ranges, [0.."Đường".len()]);
    assert_eq!(&"Đường"[ranges[0].clone()], "Đường");
    block_on(launcher.set_query("strasse"));
    assert_eq!(titles(&launcher), ["Straße"]);
}

#[test]
fn search_sensitivity_decides_how_good_a_score_must_be() {
    let launcher = without_runtime(vec![command("Undownloadable files", None)]);
    // "download" sits mid-word: exactly 2n, which High — the default —
    // rejects, and Medium and Low hold.
    block_on(launcher.set_query("download"));
    assert!(titles(&launcher).is_empty(), "High, the default");
    launcher.set_search_sensitivity(SearchSensitivity::Medium);
    // The same query re-searched changes nothing, so the retry carries a
    // space the matcher folds away.
    block_on(launcher.set_query("download "));
    assert_eq!(titles(&launcher), ["Undownloadable files"]);
    // The choice applies on the next keystroke: the list the current
    // query has already made stays as it is.
    launcher.set_search_sensitivity(SearchSensitivity::High);
    assert_eq!(
        titles(&launcher),
        ["Undownloadable files"],
        "the list stays until the query changes"
    );
    block_on(launcher.set_query("download"));
    assert!(
        titles(&launcher).is_empty(),
        "the next keystroke applies it"
    );
}

/// The title characters of the best placement, as the presentation hands
/// them to the window: contiguous letters one run, scattered ones one run
/// each, and nothing for a title the query did not match.
#[test]
fn root_search_highlights_the_title_characters_of_the_best_placement() {
    let launcher = without_runtime(vec![
        command("Clipboard History", None),
        command("Clear cache", Some("Delete downloaded files")),
    ]);
    let matched = |launcher: &Launcher, title: &str| {
        launcher
            .presentation()
            .rows
            .iter()
            .zip(launcher.view().rows)
            .find(|(_, row)| row.title == title)
            .map(|(shown, _)| shown.matched.clone())
            .unwrap_or_default()
    };
    block_on(launcher.set_query("clhis"));
    assert_eq!(titles(&launcher), ["Clipboard History"]);
    assert_eq!(matched(&launcher, "Clipboard History"), [0..2, 10..13]);
    // A row found by its subtitle alone highlights nothing in its title.
    block_on(launcher.set_query("del files"));
    assert_eq!(titles(&launcher), ["Clear cache"]);
    assert!(matched(&launcher, "Clear cache").is_empty());
}

#[test]
fn searching_the_same_query_again_keeps_the_selection() {
    let launcher = downloads();
    block_on(launcher.set_query("download"));
    launcher.move_selection(1);
    block_on(launcher.set_query("download"));
    assert_eq!(selected_title(&launcher).as_deref(), Some("Downloader"));
}

#[test]
fn every_letter_of_the_query_must_place_in_order() {
    let launcher = downloads();
    block_on(launcher.set_query("rec down"));
    assert_eq!(titles(&launcher), ["Recent downloads"]);
    block_on(launcher.set_query("dnolw"));
    // "downloads" holds every letter but not in this order.
    assert!(titles(&launcher).is_empty(), "{:?}", titles(&launcher));
    // A word of the query may sit in the title and the rest in the
    // subtitle: "files" is only in Clear cache's, "delete" only in it.
    block_on(launcher.set_query("delete files"));
    assert_eq!(titles(&launcher), ["Clear cache"]);
    // The query's own separator helps: placed on a separator, it lifts the
    // score, so "down files" holds the mid-word match too at High — below
    // the subtitle that starts a word with it.
    block_on(launcher.set_query("down files"));
    assert_eq!(titles(&launcher), ["Clear cache", "Undownloadable files"]);
}

#[test]
fn matches_the_comparator_cannot_tell_apart_keep_root_search_order() {
    let launcher = without_runtime(vec![
        command_titled("first", "Sample", Some("One row of a title two rows share")),
        command_titled(
            "second",
            "Sample",
            Some("One row of a title two rows share"),
        ),
        command_titled("third", "Sample", Some("One row of a title two rows share")),
    ]);
    // Three rows no step can tell apart: the exact title ties (step 2),
    // the scores tie, the kind and the provider tie, and the titles
    // collate equal — so root search's own order stands.
    block_on(launcher.set_query("sample"));
    assert_eq!(
        rows(&launcher),
        [
            "Sample · One row of a title two rows share",
            "Sample · One row of a title two rows share",
            "Sample · One row of a title two rows share"
        ]
    );
}

/// The rows on screen as "title · subtitle", to read ties by.
fn rows(launcher: &Launcher) -> Vec<String> {
    launcher
        .view()
        .rows
        .into_iter()
        .map(|row| match row.subtitle {
            Some(subtitle) => format!("{} · {}", row.title, subtitle),
            None => row.title,
        })
        .collect()
}

/// A query that is the result's alias ranks it above a result whose title
/// it exactly is, whatever their scores (step 1 before step 2).
#[test]
fn the_query_being_the_alias_ranks_above_an_exact_title() {
    let dirs = Dirs::new();
    let launcher = with_commands(&dirs, vec![command("Alpha", None), command("Beta", None)]);
    block_on(
        launcher
            .set_alias("beta", "alpha")
            .expect("the alias is taken"),
    );

    block_on(launcher.set_query("alpha"));
    assert_eq!(titles(&launcher), ["Beta", "Alpha"]);
}

/// A query longer than three characters that is exactly a title ranks it
/// above a result the alias of which starts with the query (step 2 before
/// step 5), both scoring alike.
#[test]
fn an_exact_title_ranks_above_a_result_the_alias_of_which_starts_with_the_query() {
    let dirs = Dirs::new();
    let launcher = with_commands(
        &dirs,
        vec![
            command("Alpha", None),
            command_titled("best", "Alpha Best", None),
        ],
    );
    block_on(
        launcher
            .set_alias("best", "alphabet")
            .expect("the alias is taken"),
    );

    block_on(launcher.set_query("alpha"));
    assert_eq!(titles(&launcher), ["Alpha", "Alpha Best"]);
}

/// A query that is exactly the subtitle ranks a result above one whose
/// title scores as well without being it (step 4 before step 10).
#[test]
fn the_query_being_the_subtitle_ranks_above_a_title_score() {
    let launcher = without_runtime(vec![
        command_titled("first", "First Note", Some("alpha")),
        command("Alphabet", None),
    ]);
    block_on(launcher.set_query("alpha"));
    assert_eq!(titles(&launcher), ["First Note", "Alphabet"]);
}

/// A query the alias of a result starts with matches it even when no text
/// of it holds the query's letters, and ranks it above a title match
/// (step 5 before step 8).
#[test]
fn the_alias_starting_with_the_query_matches_and_ranks_above_the_scores() {
    let dirs = Dirs::new();
    let launcher = with_commands(
        &dirs,
        vec![command("Zed Row", None), command("Quiet Owl", None)],
    );
    block_on(
        launcher
            .set_alias("quiet-owl", "zedx")
            .expect("the alias is taken"),
    );

    block_on(launcher.set_query("zed"));
    assert_eq!(
        titles(&launcher),
        ["Quiet Owl", "Zed Row"],
        "Quiet Owl matches through its alias alone"
    );
}

/// The best of the title, alternate-title and subtitle scores ranks a
/// result above one whose texts hold the query less well (step 8).
#[test]
fn the_best_score_ranks_the_better_placement_first() {
    let launcher = without_runtime(vec![
        command_titled("first", "First", Some("Notebook")),
        command_titled("second", "Second", Some("Another note")),
    ]);
    block_on(launcher.set_query("note"));
    // Both match through their subtitles alone; the tighter placement
    // ("Notebook", one word from the start) ranks above the scattered one.
    assert_eq!(titles(&launcher), ["First", "Second"]);
}

/// The title's own score breaks a tie the best score left (step 10).
#[test]
fn the_title_score_breaks_a_tie_of_the_best_scores() {
    let launcher = without_runtime(vec![
        command_titled("first", "Notebook", Some("Zz")),
        command_titled("second", "Second", Some("Notebook")),
    ]);
    block_on(launcher.set_query("note"));
    // Both hold the query equally well once title and subtitle count
    // (step 8); only the first holds it in its title.
    assert_eq!(titles(&launcher), ["Notebook", "Second"]);
}

/// Titles compare with their digit runs as numbers, so "Tool 2" comes
/// before "Tool 10" however they were registered (step 13).
#[test]
fn titles_collate_their_digit_runs_by_value() {
    let launcher = without_runtime(vec![
        command("Tool 10", None),
        command("Tool 2", None),
        command("Tool 1", None),
    ]);
    block_on(launcher.set_query("tool"));
    assert_eq!(titles(&launcher), ["Tool 1", "Tool 2", "Tool 10"]);
}

#[test]
fn the_selection_moves_among_the_matches_and_enter_opens_the_selected_one() {
    let launcher = Launcher::new(
        Runtime::start(),
        vec![
            command("Rust sample", None),
            command("Other sample", None),
            command("Unrelated", None),
        ],
    );
    block_on(launcher.set_query("sample"));
    // The two matches rank by title collation (#197): "Other sample"
    // before "Rust sample".
    launcher.move_selection(1);
    assert_eq!(selected_title(&launcher).as_deref(), Some("Rust sample"));
    launcher.move_selection(5);
    assert_eq!(
        selected_title(&launcher).as_deref(),
        Some("Rust sample"),
        "the selection stays among the matches"
    );

    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(view.title, "Rust sample", "the guest's view title");
}

#[test]
fn a_query_that_matches_nothing_shows_no_rows_and_enter_does_nothing() {
    let launcher = downloads();
    block_on(launcher.set_query("zzz"));
    let view = launcher.view();
    assert!(view.rows.is_empty());
    assert_eq!((view.selected, &view.status), (None, &Status::Idle));

    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert_eq!(
        (view.query(), &view.status),
        (Some("zzz"), &Status::Idle),
        "a missing result is not a failed action"
    );
}

#[test]
fn a_matching_command_that_fails_explains_the_failure() {
    let launcher = without_runtime(vec![command("Broken", None)]);
    block_on(launcher.set_query("brok"));
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert!(
        matches!(&view.status, Status::Error(message) if message.contains("no engine")),
        "{:?}",
        view.status
    );
}

#[test]
fn back_clears_the_query_before_anything_else() {
    let launcher = downloads();
    block_on(launcher.set_query("settings"));
    launcher.back();
    let view = launcher.view();
    assert_eq!(view.query(), Some(""));
    assert_eq!(titles(&launcher).len(), 7);
}

#[test]
fn returning_to_root_search_starts_a_new_search() {
    let launcher = Launcher::new(
        Runtime::start(),
        vec![command("Rust sample", None), command("Other", None)],
    );
    block_on(launcher.set_query("rust"));
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command);
    block_on(launcher.set_query("ignored"));
    assert_eq!(
        launcher.view().query(),
        None,
        "only root search has a query"
    );

    launcher.back();
    let view = launcher.view();
    assert_eq!(view.query(), Some(""));
    // The blank query lists everything in the no-query order (#199):
    // title collation among the commands.
    assert_eq!(titles(&launcher), ["Other", "Rust sample", "Settings…"]);
}

/// Test-local directories: package sources and Pane's data location.
struct Dirs {
    sources: TempDir,
    data: TempDir,
    cache: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// A runtime that keeps compiled code, so installing many copies of the
    /// same component compiles it once.
    fn runtime(&self) -> Runtime {
        Runtime::start_with_cache(self.cache.path().to_path_buf()).unwrap()
    }

    /// Writes a package folder named `name` whose manifest is `manifest`
    /// and whose component `hello.wasm` is the Rust sample.
    fn package(&self, name: &str, manifest: &str) -> PathBuf {
        let folder = self.sources.path().join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("pane.json"), manifest).unwrap();
        fs::copy(guest("sample_rust"), folder.join("hello.wasm")).unwrap();
        folder
    }
}

/// A manifest for a package titled `title` with one command titled
/// `command`, on every system or only on `platforms` (a JSON list).
fn manifest(title: &str, command: &str, platforms: Option<&str>) -> String {
    let platforms = platforms
        .map(|list| format!(r#", "platforms": {list}"#))
        .unwrap_or_default();
    format!(
        r#"{{ "manifestVersion": 1, "title": "{title}", "apiVersion": "0.1",
  "commands": [{{ "id": "hello", "title": "{command}", "component": "hello.wasm"{platforms} }}] }}"#
    )
}

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
}

#[test]
fn many_installed_commands_are_searched_without_running_them_and_only_the_chosen_one_starts() {
    let dirs = Dirs::new();
    let runtime = dirs.runtime();
    let installer = Launcher::with_packages(Ok(runtime), vec![], dirs.packages_dir());
    for n in 1..=12 {
        let title = format!("Tool {n}");
        let folder = dirs.package(&format!("tool-{n}"), &manifest(&title, &title, None));
        install(&installer, &folder);
    }

    // A restart: listing and searching read only the managed manifests.
    // Pane's own rows number six once an extension is installed (#267).
    let runtime = dirs.runtime();
    let launcher = Launcher::with_packages(Ok(runtime.clone()), vec![], dirs.packages_dir());
    assert_eq!(launcher.view().rows.len(), 20);
    block_on(launcher.set_query("tool 1"));
    assert_eq!(
        titles(&launcher),
        ["Tool 1", "Tool 10", "Tool 11", "Tool 12"]
    );
    assert_eq!(block_on(runtime.running()), Vec::<PathBuf>::new());

    launcher.move_selection(2);
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command);
    let running = block_on(runtime.running());
    assert_eq!(running.len(), 1, "{running:?}");
    let chosen = launcher
        .packages()
        .into_iter()
        .find(|package| package.title() == "Tool 11")
        .unwrap();
    assert!(running[0].starts_with(&chosen.location), "{running:?}");
}

#[test]
fn an_installed_command_is_found_by_its_title_or_its_package_title() {
    let dirs = Dirs::new();
    let launcher = Launcher::with_packages(Ok(dirs.runtime()), vec![], dirs.packages_dir());
    install(
        &launcher,
        &dirs.package("weather", &manifest("Weather", "Forecast", None)),
    );

    block_on(launcher.set_query("forec"));
    assert_eq!(titles(&launcher), ["Forecast"]);
    // Without its own subtitle a command shows its package title, which
    // matches too.
    block_on(launcher.set_query("weather"));
    assert_eq!(titles(&launcher), ["Forecast"]);
    block_on(launcher.set_query("manage"));
    assert_eq!(titles(&launcher), [MANAGE_ROW]);
    // Pane's own rows are searched like commands: by their titles, and the
    // manager by its subtitle ("Configure, update and remove extensions in
    // Settings", #168).
    block_on(launcher.set_query("install"));
    assert_eq!(titles(&launcher), [INSTALL_ROW, GIT_ROW, NPM_ROW]);
    // The authoring rows are searched by their titles too (#222): each
    // matches its own verb, none of them the install rows'.
    block_on(launcher.set_query("create"));
    assert_eq!(titles(&launcher), [CREATE_ROW]);
    block_on(launcher.set_query("import"));
    assert_eq!(titles(&launcher), [IMPORT_ROW]);
    block_on(launcher.set_query("configure"));
    assert_eq!(titles(&launcher), [MANAGE_ROW]);
}

/// A manifest for a package titled `title` with one command titled
/// `command` whose own subtitle is `subtitle` and whose `keywords` are
/// given.
fn manifest_with_keywords(title: &str, command: &str, subtitle: &str, keywords: &[&str]) -> String {
    let keywords = keywords
        .iter()
        .map(|keyword| format!("\"{keyword}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"{{ "manifestVersion": 1, "title": "{title}", "apiVersion": "0.1",
  "commands": [{{ "id": "hello", "title": "{command}", "subtitle": "{subtitle}",
    "keywords": [{keywords}], "component": "hello.wasm" }}] }}"#
    )
}

/// A manifest for a package titled `title` with one command titled
/// `command` whose own subtitle is `subtitle`.
fn manifest_with_subtitle(title: &str, command: &str, subtitle: &str) -> String {
    format!(
        r#"{{ "manifestVersion": 1, "title": "{title}", "apiVersion": "0.1",
  "commands": [{{ "id": "hello", "title": "{command}", "subtitle": "{subtitle}", "component": "hello.wasm" }}] }}"#
    )
}

#[test]
fn a_command_with_its_own_subtitle_is_found_by_its_package_title_last() {
    let dirs = Dirs::new();
    let launcher = Launcher::with_packages(Ok(dirs.runtime()), vec![], dirs.packages_dir());
    let weather = manifest_with_subtitle("Weather", "Forecast", "Five days ahead");
    install(&launcher, &dirs.package("weather", &weather));
    let maps = manifest_with_subtitle("Maps", "Radar", "Weather radar");
    install(&launcher, &dirs.package("maps", &maps));

    block_on(launcher.set_query("weather"));
    // A subtitle match ranks above a package title match.
    assert_eq!(titles(&launcher), ["Radar", "Forecast"]);
    let view = launcher.view();
    assert_eq!(
        view.rows[1].subtitle.as_deref(),
        Some("Five days ahead"),
        "the command still shows its own subtitle"
    );
    // A query may span a result's title and subtitle, through the
    // composite of the two — but no longer across its package title:
    // "weather five" holds words of the package title and the subtitle,
    // and no single text holds them both.
    block_on(launcher.set_query("forecast five"));
    assert_eq!(titles(&launcher), ["Forecast"]);
    block_on(launcher.set_query("weather five"));
    assert!(titles(&launcher).is_empty(), "{:?}", titles(&launcher));
}

/// A manifest's `keywords` find a command as its subtitle does: below a
/// title match (step 10, a keyword holds nothing in the title), and the
/// query being one exactly ranks as the subtitle would (step 4).
#[test]
fn a_manifests_keywords_find_the_command_as_its_subtitle_does() {
    let dirs = Dirs::new();
    let launcher = Launcher::with_packages(Ok(dirs.runtime()), vec![], dirs.packages_dir());
    let tidy = manifest_with_keywords("Tidy", "Remove Notes", "Unused", &["trash"]);
    install(&launcher, &dirs.package("tidy", &tidy));
    let orderly = manifest("Orderly", "Trash Rules", None);
    install(&launcher, &dirs.package("orderly", &orderly));

    // "tras" holds in "Trash Rules"'s title and in Remove Notes' keyword
    // alike (step 8); only the former holds it in its title.
    block_on(launcher.set_query("tras"));
    assert_eq!(titles(&launcher), ["Trash Rules", "Remove Notes"]);
    // "trash" is Remove Notes' keyword exactly, as a subtitle would be
    // the query, ranking it above the title match.
    block_on(launcher.set_query("trash"));
    assert_eq!(titles(&launcher), ["Remove Notes", "Trash Rules"]);
}

#[test]
fn disabling_a_package_removes_its_matches_at_once_and_enabling_brings_them_back() {
    let dirs = Dirs::new();
    let launcher = Launcher::with_packages(Ok(dirs.runtime()), vec![], dirs.packages_dir());
    let first = dirs.package("first", &manifest("First", "Greet first", None));
    let second = dirs.package("second", &manifest("Second", "Greet second", None));
    install(&launcher, &first);
    install(&launcher, &second);
    block_on(launcher.set_query("greet"));
    launcher.move_selection(1);
    assert_eq!(selected_title(&launcher).as_deref(), Some("Greet second"));

    let disabling = launcher.set_enabled(&PackageIdentity::local(&first).unwrap(), false);
    assert_eq!(
        titles(&launcher),
        ["Greet second"],
        "the disabled package's command leaves the results before it is recorded"
    );
    assert_eq!(selected_title(&launcher).as_deref(), Some("Greet second"));
    assert_eq!(launcher.view().query(), Some("greet"));
    block_on(disabling);

    block_on(launcher.set_enabled(&PackageIdentity::local(&first).unwrap(), true));
    assert_eq!(titles(&launcher), ["Greet first", "Greet second"]);
    assert_eq!(
        selected_title(&launcher).as_deref(),
        Some("Greet second"),
        "the selection stays on its row"
    );
}

#[test]
fn an_update_finishing_while_the_user_searches_updates_the_results_and_keeps_the_query() {
    let dirs = Dirs::new();
    let launcher = Launcher::with_packages(Ok(dirs.runtime()), vec![], dirs.packages_dir());
    let folder = dirs.package("notes", &manifest("Notes", "Take a note", None));
    install(&launcher, &folder);

    fs::write(folder.join("pane.json"), manifest("Jots", "Jot down", None)).unwrap();
    block_on(launcher.preview_package(&folder));
    let update = launcher.activate_selected();
    // The user goes back to root search and types before it finishes.
    launcher.back();
    block_on(launcher.set_query("note"));
    assert_eq!(titles(&launcher), ["Take a note"]);
    block_on(update);

    let view = launcher.view();
    assert_eq!(view.query(), Some("note"));
    assert!(titles(&launcher).is_empty(), "{:?}", titles(&launcher));
    block_on(launcher.set_query("jot"));
    assert_eq!(titles(&launcher), ["Jot down"]);
}

#[test]
fn an_unavailable_command_matches_and_explains_why_it_does_not_run() {
    let dirs = Dirs::new();
    let runtime = dirs.runtime();
    let launcher = Launcher::with_packages(Ok(runtime.clone()), vec![], dirs.packages_dir());
    let [first, second] = platforms::other_systems();
    let elsewhere = format!(r#"["{}", "{}"]"#, first.id(), second.id());
    let folder = dirs.package(
        "elsewhere",
        &manifest("Elsewhere", "Native tool", Some(&elsewhere)),
    );
    install(&launcher, &folder);

    block_on(launcher.set_query("native"));
    let view = launcher.view();
    assert_eq!(titles(&launcher), ["Native tool"]);
    let reason = platforms::only("this command", &platforms::other_names());
    assert_eq!(
        view.rows[0].unavailable,
        Some(Unavailable::OnThisSystem(reason.clone()))
    );
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().status, Status::Error(reason));
    assert_eq!(block_on(runtime.running()), Vec::<PathBuf>::new());
}

/// The presentation root search hands the window: every row a command,
/// under one "Commands" label for a blank query — the no-query order
/// (#199), with no section of suggestions — and under "Results" with
/// their count for a query, each title's match where the query matched
/// it.
#[test]
fn root_search_presents_its_rows_with_kinds_sections_and_title_matches() {
    let launcher = downloads();
    let presentation = launcher.presentation();
    let rows = launcher.view().rows;
    assert_eq!(presentation.rows.len(), rows.len());
    assert_eq!(
        presentation.sections,
        [pane_core::Section {
            label: "Commands".into(),
            note: None,
            first: 0
        }]
    );
    assert!(
        presentation
            .rows
            .iter()
            .all(|row| row.kind == Some(pane_core::RowKind::Command)),
        "{presentation:?}"
    );
    assert!(presentation.rows.iter().all(|row| row.matched.is_empty()));

    block_on(launcher.set_query("down"));
    let view = launcher.view();
    let presentation = launcher.presentation();
    assert_eq!(
        presentation.sections,
        [pane_core::Section {
            label: "Results".into(),
            note: Some(format!("{} matches", view.rows.len())),
            first: 0
        }]
    );
    for (row, shown) in view.rows.iter().zip(&presentation.rows) {
        let matched: Vec<&str> = shown
            .matched
            .iter()
            .map(|range| &row.title[range.clone()])
            .collect();
        if row.title.to_lowercase().contains("down") {
            assert_eq!(matched.len(), 1, "{}", row.title);
            assert_eq!(matched[0].to_lowercase(), "down", "{}", row.title);
        }
    }

    let (view, presented) = launcher.presented_view();
    assert_eq!(presented, launcher.presentation());
    assert_eq!(view.selected, launcher.selected());

    block_on(launcher.set_query("download"));
    let presentation = launcher.presentation();
    assert_eq!(
        presentation.sections[0].note.as_deref(),
        Some(format!("{} matches", launcher.view().rows.len()).as_str())
    );
}

/// Off root search nothing is projected: Manage extensions' rows show as
/// they always did.
#[test]
fn rows_off_root_search_carry_no_presentation() {
    let dirs = Dirs::new();
    let launcher = Launcher::with_packages(Ok(dirs.runtime()), vec![], dirs.packages_dir());
    install(
        &launcher,
        &dirs.package("weather", &manifest("Weather", "Forecast", None)),
    );
    block_on(launcher.set_query("manage"));
    assert_eq!(selected_title(&launcher).as_deref(), Some(MANAGE_ROW));
    assert_eq!(
        launcher.presentation().rows[0].kind,
        Some(pane_core::RowKind::Command),
        "Pane's own rows are its commands"
    );
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    let presentation = launcher.presentation();
    assert!(presentation.sections.is_empty());
    assert_eq!(presentation.rows.len(), launcher.view().rows.len());
    assert!(
        presentation
            .rows
            .iter()
            .all(|row| *row == pane_core::RowPresentation::default())
    );
}

/// A computed answer's section (#96): a run of answers among a query's
/// results sits under the title of the command that computed it, the
/// results around it under "Results" with their own count, the
/// fallbacks under "Fallbacks"; with no answer listed, the sections are
/// root search's own, and a blank query's rows are its commands.
#[test]
fn computed_answers_sit_under_their_commands_title_and_results_keep_their_count() {
    use pane_core::{Section, answer_sections, root_sections};
    let section = |label: &str, note: Option<&str>, first: usize| Section {
        label: label.into(),
        note: note.map(Into::into),
        first,
    };

    // The calculator's answer, two title matches, then a fallback.
    let answers = [Some("Calculator"), None, None, None];
    assert_eq!(
        answer_sections("6*7", &answers, 3),
        [
            section("Calculator", None, 0),
            section("Results", Some("2 matches"), 1),
            section("Fallbacks", None, 3),
        ]
    );
    // The answer alone.
    assert_eq!(
        answer_sections("6*7", &[Some("Calculator")], 1),
        [section("Calculator", None, 0)]
    );
    // A row the alias names first, then two commands' answers.
    let sums = Some("Sums");
    let answers = [None, Some("Calculator"), sums, sums, None];
    assert_eq!(
        answer_sections("ec 1 + 1", &answers, 5),
        [
            section("Results", Some("1 match"), 0),
            section("Calculator", None, 1),
            section("Sums", None, 2),
            section("Results", Some("1 match"), 4),
        ]
    );
    // No answer: root search's own sections, whatever the query.
    for (query, rows, fallbacks) in [("down", 3, 2), ("", 4, 4), ("zqx", 1, 0), ("zqx", 0, 0)] {
        assert_eq!(
            answer_sections(query, &vec![None; rows], fallbacks),
            root_sections(query, rows, fallbacks),
            "{query:?}"
        );
    }
}
