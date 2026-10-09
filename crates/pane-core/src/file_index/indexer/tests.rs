//! The coordinator driven through its change source's seam: a fake source
//! the test scripts (what a catch-up finds) and drives (the live changes it
//! reports), over a fixture folder standing for the home folder, with the
//! real index and walker.

use std::collections::BTreeSet;
use std::fs;
use std::sync::atomic::{AtomicU64, AtomicUsize};

use super::*;
use crate::file_index::Entry;
use crate::file_index::volume::tests::fake_volumes;

/// A change source the test scripts and drives.
#[derive(Default)]
struct Fake {
    /// What the next catch-up answers; by default, nothing changed.
    caught: Mutex<Option<Caught>>,
    /// The latest watch's sink.
    sink: Mutex<Option<Sink>>,
    /// Watches running now.
    watching: Arc<AtomicUsize>,
    /// The roots the latest watch was asked to watch.
    watched: Mutex<Vec<PathBuf>>,
    /// Catch-ups made.
    catch_ups: AtomicUsize,
    /// Its catch-up reads the folder-id table, as the NTFS journal's does.
    reads_folder_ids: AtomicBool,
    /// It watches each indexed folder, as Linux's inotify does.
    watches_each_folder: AtomicBool,
    /// The folders the latest watch was given.
    folders: Mutex<Vec<PathBuf>>,
}

struct FakeWatch(Arc<AtomicUsize>);

impl Watching for FakeWatch {}

impl Drop for FakeWatch {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn cursor() -> JournalCursor {
    JournalCursor {
        volume: "fake".into(),
        journal_id: 1,
        next_usn: 7,
    }
}

impl ChangeSource for Fake {
    fn cursors(&self, _scope: &Scope) -> Vec<JournalCursor> {
        vec![cursor()]
    }

    fn catch_up(
        &self,
        _index: &FileIndex,
        _scope: &Scope,
        cursors: &[JournalCursor],
        folders: &mut FolderIds<'_>,
        _cancel: &AtomicBool,
    ) -> Caught {
        assert_eq!(cursors, [cursor()], "the cursors saved with the index");
        self.catch_ups.fetch_add(1, Ordering::SeqCst);
        if self.reads_folder_ids.load(Ordering::SeqCst) {
            assert!(!folders.ids().is_empty(), "the folders indexed");
        }
        lock(&self.caught).take().unwrap_or(Caught::Changes {
            changes: Vec::new(),
            walk: Vec::new(),
            reconcile: Vec::new(),
            recheck: Vec::new(),
            cursors: vec![cursor()],
            how: CaughtUpBy::Journal,
            note: None,
        })
    }

    fn watches_folders(&self) -> bool {
        self.watches_each_folder.load(Ordering::SeqCst)
    }

    fn watch(
        &self,
        scope: &Scope,
        _cursors: &[JournalCursor],
        folders: Vec<PathBuf>,
        sink: Sink,
    ) -> Result<Box<dyn Watching>, String> {
        *lock(&self.watched) = scope.watched_roots();
        *lock(&self.folders) = folders;
        *lock(&self.sink) = Some(sink);
        self.watching.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeWatch(self.watching.clone())))
    }
}

impl Fake {
    fn report(&self, changed: Changed) {
        lock(&self.sink)
            .as_ref()
            .expect("a watch is running")
            .send(changed)
            .expect("the coordinator listens");
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    index_dir: PathBuf,
    fake: Arc<Fake>,
    indexer: Indexer,
}

const OWNER: &str = "owner";
const LIMIT: Duration = Duration::from_secs(20);

fn users(owners: &[&str]) -> BTreeSet<String> {
    owners.iter().map(|owner| (*owner).to_owned()).collect()
}

fn fixture() -> Fixture {
    fixture_with(|_| {})
}

/// The fixture, its indexer configured as `edit` says.
fn fixture_with(edit: impl FnOnce(&mut IndexerConfig)) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    for (file, text) in [
        ("Documents/plan.txt", "plan"),
        ("Documents/Invoices 2026/march.pdf", "pdf"),
        ("Music/song.mp3", "mp3"),
        (".hidden notes.txt", "hidden"),
        ("node_modules/left out.js", "js"),
    ] {
        let path = home.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let index_dir = dir.path().join("cache").join(INDEX_DIR);
    let fake = Arc::new(Fake::default());
    let indexer = Indexer::default();
    let mut config = config(&index_dir, &home, fake.clone());
    edit(&mut config);
    indexer.configure(config, UserRules::default());
    Fixture {
        _dir: dir,
        home,
        index_dir,
        fake,
        indexer,
    }
}

fn config(index_dir: &Path, home: &Path, fake: Arc<Fake>) -> IndexerConfig {
    IndexerConfig {
        dir: index_dir.to_path_buf(),
        rules: ScopeRules::for_home(home.to_path_buf()),
        source: fake,
        // The launcher's showing starts the walk here, never the clock.
        first_walk_delay: Duration::from_secs(3600),
        settle: Duration::from_millis(10),
        walk: WalkOptions {
            background: false,
            ..WalkOptions::default()
        },
        reconcile_unwatched: Duration::from_secs(3600),
        valves: Valves::default(),
        volumes: Arc::new(volume_kind),
    }
}

impl Fixture {
    /// The fixture indexed: used by a package, shown, settled.
    fn indexed() -> Fixture {
        Fixture::indexed_with(|_| {})
    }

    /// The fixture, configured as `edit` says, indexed.
    fn indexed_with(edit: impl FnOnce(&mut IndexerConfig)) -> Fixture {
        let fixture = fixture_with(edit);
        fixture.indexer.launcher_shown();
        fixture.indexer.set_users(users(&[OWNER]));
        assert!(fixture.indexer.wait_until_settled(LIMIT));
        fixture
    }

    fn names(&self, query: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .indexer
            .search(OWNER, query, SearchOptions::default())
            .unwrap()
            .into_iter()
            .map(|found| found.name)
            .collect();
        names.sort();
        names
    }

    fn settle(&self) {
        assert!(
            self.indexer.wait_until_settled(LIMIT),
            "{:?}",
            self.indexer.status()
        );
    }
}

/// Waits until `done`, at most [`LIMIT`].
fn until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + LIMIT;
    while !done() {
        assert!(Instant::now() < deadline, "it never happened");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn changing_rules_does_not_wait_for_a_stopped_watchers_inflight_report() {
    // A native callback may already be sending when dropping its watch
    // asks the source thread to stop. Hold that report across the restart.
    struct HeldCounter {
        delegate: Arc<dyn Counter>,
        entered: Sender<()>,
        release: Mutex<Receiver<()>>,
    }
    impl Counter for HeldCounter {
        fn sent(&self) {
            self.delegate.sent();
            self.entered.send(()).unwrap();
            lock(&self.release).recv().unwrap();
        }
        fn unsent(&self) {
            self.delegate.unsent();
        }
    }

    let fixture = Fixture::indexed();
    let mut old_sink = lock(&fixture.fake.sink).as_ref().unwrap().clone();
    let (entered, ready) = channel();
    let (release, resume) = channel();
    let held: Arc<dyn Counter> = Arc::new(HeldCounter {
        delegate: old_sink.counter.as_ref().unwrap().upgrade().unwrap(),
        entered,
        release: Mutex::new(resume),
    });
    old_sink.counter = Some(Arc::downgrade(&held));
    let report = std::thread::spawn(move || old_sink.send(Changed::Paths(Vec::new())));
    ready.recv_timeout(LIMIT).unwrap();

    let mut rules = fixture.indexer.user_rules();
    rules.include_hidden = true;
    fixture.indexer.set_user_rules(rules);
    let settled = fixture.indexer.wait_until_settled(Duration::from_secs(2));

    // Always release and join the callback before asserting, including
    // against a broken implementation, so the test leaves no stuck thread.
    release.send(()).unwrap();
    assert!(
        report.join().unwrap().is_err(),
        "the old receiver is closed"
    );
    assert!(settled, "the replacement index must settle independently");
    assert_eq!(fixture.names("hidden notes"), [".hidden notes.txt"]);
}

#[test]
fn the_first_walk_waits_for_the_launcher_to_be_shown() {
    let fixture = fixture();
    fixture.indexer.set_users(users(&[OWNER]));
    until(|| fixture.indexer.status().waiting);
    assert_eq!(fixture.indexer.status().state, IndexState::Building);
    assert!(fixture.names("plan").is_empty(), "nothing is walked yet");
    assert!(
        !fixture
            .indexer
            .wait_until_settled(Duration::from_millis(50))
    );

    fixture.indexer.launcher_shown();
    fixture.settle();
    let status = fixture.indexer.status();
    assert_eq!(status.state, IndexState::Current);
    assert_eq!(
        status.caught_up.map(|(by, _)| by),
        Some(CaughtUpBy::FullWalk)
    );
    assert!(status.entries > 0);
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
    assert_eq!(fixture.names("invoices march"), ["march.pdf"]);
    // Hidden entries and node_modules are left out by default.
    assert!(fixture.names("hidden").is_empty());
    assert!(fixture.names("left").is_empty());
}

#[test]
fn only_a_package_that_uses_the_index_may_search_it_and_ids_are_its_own() {
    let fixture = Fixture::indexed();
    assert!(
        fixture
            .indexer
            .search("other", "plan", SearchOptions::default())
            .is_err()
    );
    let found = fixture
        .indexer
        .search(OWNER, "plan", SearchOptions::default())
        .unwrap();
    let plan = &found[0];
    assert_eq!(plan.name, "plan.txt");
    assert_eq!(plan.folder, "~/Documents");
    assert_eq!(plan.kind, EntryKind::File);
    assert_eq!(plan.size, 4);
    assert!(!plan.program);
    let known = fixture.indexer.known(OWNER, &plan.id).unwrap();
    assert_eq!(known.name, "plan.txt");
    assert_eq!(fixture.indexer.known("other", &plan.id), None);
    assert_eq!(fixture.indexer.known(OWNER, "i99-1"), None);
    let checked = fixture.indexer.checked(OWNER, &plan.id).unwrap();
    assert_eq!(
        checked.path,
        canonical(&fixture.home.join("Documents/plan.txt")).unwrap()
    );
    assert!(fixture.indexer.checked("other", &plan.id).is_err());

    // Changed since it was found: explained, never acted on.
    fs::remove_file(fixture.home.join("Documents/plan.txt")).unwrap();
    assert_eq!(
        fixture.indexer.checked(OWNER, &plan.id),
        Err("it no longer exists".into())
    );
    fs::create_dir(fixture.home.join("Documents/plan.txt")).unwrap();
    assert_eq!(
        fixture.indexer.checked(OWNER, &plan.id),
        Err("it is now a folder".into())
    );
}

#[test]
fn folders_kinds_categories_sorting_and_pages() {
    let fixture = Fixture::indexed();
    let search = |query: &str, options: SearchOptions| -> Vec<String> {
        fixture
            .indexer
            .search(OWNER, query, options)
            .unwrap()
            .into_iter()
            .map(|found| found.name)
            .collect()
    };
    let folders = SearchOptions {
        kind: Some(EntryKind::Folder),
        ..SearchOptions::default()
    };
    assert_eq!(search("invoices", folders), ["Invoices 2026"]);
    let audio = SearchOptions {
        category: Some(Category::Audio),
        ..SearchOptions::default()
    };
    assert_eq!(search("", audio), ["song.mp3"]);
    // A blank query lists the newest first. Its time is a minute later, so
    // that it is the newest whatever the clock's resolution (times are kept
    // in whole seconds, and the fixture was made within the same one).
    let newest = fixture.home.join("Music/newest.txt");
    fs::write(&newest, "new").unwrap();
    touch(&newest);
    fixture
        .fake
        .report(Changed::Paths(vec![fixture.home.join("Music/newest.txt")]));
    fixture.settle();
    let recent = search("", SearchOptions::default());
    assert_eq!(recent.first().map(String::as_str), Some("newest.txt"));
    // Pages follow each other.
    let first = search(
        "",
        SearchOptions {
            limit: 2,
            ..SearchOptions::default()
        },
    );
    let second = search(
        "",
        SearchOptions {
            limit: 2,
            offset: 2,
            ..SearchOptions::default()
        },
    );
    assert_eq!(first.len(), 2);
    assert!(second.iter().all(|name| !first.contains(name)));
}

#[test]
fn live_changes_are_found_once_applied() {
    let fixture = Fixture::indexed();
    let home = &fixture.home;
    // Created.
    fs::write(home.join("Documents/Résumé.txt"), "cv").unwrap();
    fixture
        .fake
        .report(Changed::Paths(vec![home.join("Documents/Résumé.txt")]));
    fixture.settle();
    assert_eq!(fixture.names("resume"), ["Résumé.txt"]);
    // A folder moved in, with files in it, is walked.
    let outside = fixture._dir.path().join("outside");
    fs::create_dir_all(outside.join("deep")).unwrap();
    fs::write(outside.join("deep/brought.txt"), "x").unwrap();
    fs::rename(&outside, home.join("Projects")).unwrap();
    fixture
        .fake
        .report(Changed::Paths(vec![home.join("Projects")]));
    fixture.settle();
    assert_eq!(fixture.names("brought"), ["brought.txt"]);
    // Renamed: the old name gone with everything under it.
    fs::rename(home.join("Projects"), home.join("Work")).unwrap();
    fixture.fake.report(Changed::Paths(vec![
        home.join("Projects"),
        home.join("Work"),
    ]));
    fixture.settle();
    let found = fixture
        .indexer
        .search(OWNER, "brought", SearchOptions::default())
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].folder, "~/Work/deep");
    // Deleted.
    fs::remove_file(home.join("Documents/Résumé.txt")).unwrap();
    fixture
        .fake
        .report(Changed::Paths(vec![home.join("Documents/Résumé.txt")]));
    fixture.settle();
    assert!(fixture.names("resume").is_empty());
    // A hidden file is reported but not indexed.
    fs::write(home.join(".secret plan"), "x").unwrap();
    fixture
        .fake
        .report(Changed::Paths(vec![home.join(".secret plan")]));
    fixture.settle();
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
}

#[cfg(unix)]
#[test]
fn an_entry_replaced_by_a_link_since_it_was_found_is_refused() {
    let fixture = Fixture::indexed();
    let found = fixture
        .indexer
        .search(OWNER, "plan", SearchOptions::default())
        .unwrap();
    let plan = fixture.home.join("Documents/plan.txt");
    let elsewhere = fixture._dir.path().join("elsewhere.txt");
    fs::write(&elsewhere, "x").unwrap();
    fs::remove_file(&plan).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &plan).unwrap();
    assert_eq!(
        fixture.indexer.checked(OWNER, &found[0].id),
        Err("it is now a link".into())
    );
}

#[test]
fn folders_the_source_cannot_watch_are_counted_and_reconciled_every_few_minutes() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fs::create_dir_all(home.join("Deep/Deeper")).unwrap();
    let fake = Arc::new(Fake::default());
    let indexer = Indexer::default();
    indexer.configure(
        IndexerConfig {
            // Reconciled at once, as the few minutes are up.
            reconcile_unwatched: Duration::from_millis(50),
            ..config(
                &dir.path().join("cache").join(INDEX_DIR),
                &home,
                fake.clone(),
            )
        },
        UserRules::default(),
    );
    indexer.launcher_shown();
    indexer.set_users(users(&[OWNER]));
    assert!(indexer.wait_until_settled(LIMIT));
    // Linux's watch limit reached: the source says which folders it could
    // not watch.
    let deeper = home.join("Deep/Deeper");
    fake.report(Changed::Unwatched(vec![deeper.clone()]));
    until(|| indexer.status().unwatched == 1);
    // Listed on the File search page, with what raises the limit.
    let problems = indexer.problems();
    let unwatched = problems
        .iter()
        .find(|problem| problem.kind == ProblemKind::Unwatched)
        .expect("listed");
    assert!(unwatched.reason.starts_with("1 folders are not watched"));
    assert!(unwatched.remedy.contains("fs.inotify.max_user_watches"));
    // A change there is reported by nothing, and found all the same.
    fs::write(deeper.join("unreported plan.txt"), "x").unwrap();
    touch(&deeper);
    until(|| {
        indexer
            .search(OWNER, "unreported", SearchOptions::default())
            .is_ok_and(|found| found.len() == 1)
    });
}

#[test]
fn an_overflow_reconciles_the_folder() {
    let fixture = Fixture::indexed();
    let documents = fixture.home.join("Documents");
    fs::write(documents.join("unreported.txt"), "x").unwrap();
    fs::remove_file(documents.join("plan.txt")).unwrap();
    touch(&documents);
    fixture.fake.report(Changed::Rescan(documents));
    fixture.settle();
    assert_eq!(fixture.names("unreported"), ["unreported.txt"]);
    assert!(fixture.names("plan").is_empty());
}

#[test]
fn disabling_stops_watching_and_enabling_again_catches_up_without_walking() {
    let fixture = Fixture::indexed();
    assert_eq!(fixture.fake.watching.load(Ordering::SeqCst), 1);
    fixture.indexer.set_users(BTreeSet::new());
    assert_eq!(fixture.indexer.status().state, IndexState::Off);
    until(|| fixture.fake.watching.load(Ordering::SeqCst) == 0);
    assert!(fixture.index_dir.exists(), "the index stays on disk");

    // Changed while off: not found until the index is caught up.
    let new = fixture.home.join("Documents/while off.txt");
    fs::write(&new, "x").unwrap();
    *lock(&fixture.fake.caught) = Some(Caught::Changes {
        changes: vec![Change::Put(Entry::read(&new).unwrap())],
        walk: Vec::new(),
        reconcile: Vec::new(),
        recheck: Vec::new(),
        cursors: vec![cursor()],
        how: CaughtUpBy::Journal,
        note: None,
    });
    fixture.indexer.set_users(users(&[OWNER]));
    fixture.settle();
    assert_eq!(fixture.names("while off"), ["while off.txt"]);
    assert_eq!(
        fixture.indexer.status().caught_up.map(|(by, _)| by),
        Some(CaughtUpBy::Journal)
    );
    assert_eq!(fixture.fake.catch_ups.load(Ordering::SeqCst), 1);
}

#[test]
fn records_that_are_gone_lead_to_a_reconciling_walk_with_the_same_result() {
    let fixture = Fixture::indexed();
    fixture.indexer.set_users(BTreeSet::new());
    until(|| fixture.fake.watching.load(Ordering::SeqCst) == 0);
    fs::write(fixture.home.join("Music/missed.mp3"), "x").unwrap();
    touch(&fixture.home.join("Music"));
    *lock(&fixture.fake.caught) = Some(Caught::Reconcile("the journal was recreated".into()));
    fixture.indexer.set_users(users(&[OWNER]));
    fixture.settle();
    assert_eq!(fixture.names("missed"), ["missed.mp3"]);
    let status = fixture.indexer.status();
    assert_eq!(
        status.caught_up.map(|(by, _)| by),
        Some(CaughtUpBy::ReconcilingWalk)
    );
}

#[test]
fn a_restart_over_the_same_cache_folder_catches_up() {
    let fixture = Fixture::indexed();
    let (index_dir, home) = (fixture.index_dir.clone(), fixture.home.clone());
    fixture.indexer.set_users(BTreeSet::new());
    drop(fixture.indexer);
    // Another start of Pane, as after a restart.
    let fake = Arc::new(Fake::default());
    let indexer = Indexer::default();
    // Its index may be let go by the old coordinator a moment later.
    indexer.configure(
        config(&index_dir, &home, fake.clone()),
        UserRules::default(),
    );
    indexer.set_users(users(&[OWNER]));
    assert!(indexer.wait_until_settled(LIMIT), "{:?}", indexer.status());
    assert_eq!(
        fake.catch_ups.load(Ordering::SeqCst),
        1,
        "caught up, not walked"
    );
    let found = indexer
        .search(OWNER, "plan", SearchOptions::default())
        .unwrap();
    assert_eq!(found.len(), 1);
}

#[test]
fn a_second_pane_on_the_same_cache_folder_does_not_index() {
    let fixture = Fixture::indexed();
    let second = Indexer::default();
    second.configure(
        config(&fixture.index_dir, &fixture.home, Arc::new(Fake::default())),
        UserRules::default(),
    );
    second.set_users(users(&[OWNER]));
    until(|| second.status().state == IndexState::Stopped);
    assert_eq!(
        second.status().reason.as_deref(),
        Some("Another Pane is using file search on this computer")
    );
}

#[test]
fn deleting_the_index_removes_it_and_forgets_its_ids() {
    let fixture = Fixture::indexed();
    let found = fixture
        .indexer
        .search(OWNER, "plan", SearchOptions::default())
        .unwrap();
    fixture.indexer.set_users(BTreeSet::new());
    fixture.indexer.delete().unwrap();
    assert!(!fixture.index_dir.exists());
    assert_eq!(fixture.indexer.known(OWNER, &found[0].id), None);
    assert!(fixture.indexer.checked(OWNER, &found[0].id).is_err());
}

#[test]
fn the_users_rules_change_what_is_indexed() {
    let fixture = Fixture::indexed();
    assert!(fixture.names("hidden").is_empty());
    let extra = fixture._dir.path().join("Second drive");
    fs::create_dir_all(&extra).unwrap();
    fs::write(extra.join("far plan.txt"), "x").unwrap();
    fixture.indexer.set_user_rules(UserRules {
        include_hidden: true,
        added_roots: vec![extra.clone()],
        excluded_folders: vec![fixture.home.join("Music")],
        ..UserRules::default()
    });
    fixture.settle();
    assert_eq!(fixture.names("hidden"), [".hidden notes.txt"]);
    assert_eq!(fixture.names("far"), ["far plan.txt"]);
    assert!(fixture.names("song").is_empty());

    // A root removed takes its entries with it.
    fixture.indexer.set_user_rules(UserRules {
        include_hidden: true,
        ..UserRules::default()
    });
    fixture.settle();
    assert!(fixture.names("far").is_empty());
    assert_eq!(fixture.names("song"), ["song.mp3"]);
}

#[test]
fn an_added_root_away_is_hidden_and_back_when_it_returns() {
    let fixture = Fixture::indexed();
    let drive = fixture._dir.path().join("USB stick");
    fs::create_dir_all(&drive).unwrap();
    fs::write(drive.join("holiday.jpg"), "x").unwrap();
    fixture.indexer.set_user_rules(UserRules {
        added_roots: vec![drive.clone()],
        ..UserRules::default()
    });
    fixture.settle();
    assert_eq!(fixture.names("holiday"), ["holiday.jpg"]);
    let unplugged = fixture._dir.path().join("unplugged");
    fs::rename(&drive, &unplugged).unwrap();
    std::thread::sleep(Duration::from_millis(2100));
    assert!(fixture.names("holiday").is_empty());
    fs::rename(&unplugged, &drive).unwrap();
    std::thread::sleep(Duration::from_millis(2100));
    assert_eq!(fixture.names("holiday"), ["holiday.jpg"]);
}

#[test]
fn categories_and_folders_for_people() {
    let path = Path::new("/home/me/Pictures/cat.JPG");
    assert!(Category::Images.holds(path, EntryKind::File));
    assert!(!Category::Images.holds(path, EntryKind::Folder));
    assert!(Category::Applications.holds(Path::new("/x/setup.exe"), EntryKind::File));
    assert!(Category::Applications.holds(Path::new("/x/Pane.app"), EntryKind::Folder));
    assert!(Category::Archives.holds(Path::new("/x/a.tar.gz"), EntryKind::File));
    assert!(!Category::Documents.holds(Path::new("/x/song.mp3"), EntryKind::File));
    // Text and Other (#177): plain text is not a document, and a file of no
    // category is Other; a folder is never Other.
    assert!(Category::Text.holds(Path::new("/x/notes.md"), EntryKind::File));
    assert!(Category::Text.holds(Path::new("/x/main.rs"), EntryKind::File));
    assert!(!Category::Documents.holds(Path::new("/x/notes.txt"), EntryKind::File));
    assert!(Category::Documents.holds(Path::new("/x/report.PDF"), EntryKind::File));
    assert!(Category::Other.holds(Path::new("/x/data.bin"), EntryKind::File));
    assert!(Category::Other.holds(Path::new("/x/README"), EntryKind::File));
    assert!(!Category::Other.holds(Path::new("/x/cat.png"), EntryKind::File));
    assert!(!Category::Other.holds(Path::new("/x/Projects"), EntryKind::Folder));
    assert_eq!(
        Category::of(Path::new("/x/setup.exe"), EntryKind::File),
        Some(Category::Applications)
    );
    assert_eq!(
        Category::of(Path::new("/x/notes.txt"), EntryKind::File),
        Some(Category::Text)
    );
    assert_eq!(
        Category::of(Path::new("/x/data.bin"), EntryKind::File),
        Some(Category::Other)
    );
    assert_eq!(
        Category::of(Path::new("/x/Projects"), EntryKind::Folder),
        None
    );

    let home = Path::new("/home/me");
    assert_eq!(
        describe(Path::new("/home/me/notes.txt"), Some(home)),
        ("notes.txt".to_owned(), "~".to_owned())
    );
    assert_eq!(
        describe(
            &home.join("Documents").join("Invoices").join("a.pdf"),
            Some(home)
        ),
        ("a.pdf".to_owned(), "~/Documents/Invoices".to_owned())
    );
}

/// Sets `folder`'s (or a file's) modified time a minute later, so that a
/// reconciling walk sees it changed whatever the clock's resolution.
fn touch(folder: &Path) {
    let time = fs::metadata(folder).unwrap().modified().unwrap() + Duration::from_secs(60);
    let mut options = fs::OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.access_mode(0x100).custom_flags(0x0200_0000);
    }
    #[cfg(not(windows))]
    options.read(true);
    options
        .open(folder)
        .and_then(|file| file.set_modified(time))
        .expect("the folder's time is set");
}

#[test]
fn a_folder_that_changes_constantly_is_taken_out_until_it_is_included_again() {
    let fixture = Fixture::indexed_with(|config| {
        config.valves.churn_changes = 20;
        // Long enough that the coordinator, applying each batch on a busy
        // runner, never lets a whole window pass unseen, which would start
        // the count again.
        config.valves.churn_window = Duration::from_secs(1);
        config.valves.churn_windows = 3;
    });
    let record = fixture._dir.path().join("data");
    fs::create_dir_all(&record).unwrap();
    fixture.indexer.keep_rules_in(record.clone());
    let busy = fixture.home.join("Busy");
    fs::create_dir_all(&busy).unwrap();
    let log = busy.join("busy log.txt");
    fs::write(&log, "x").unwrap();
    fixture.fake.report(Changed::Paths(vec![busy.clone()]));
    fixture.settle();
    assert_eq!(fixture.names("busy log"), ["busy log.txt"]);
    assert!(fixture.indexer.problems().is_empty());

    // A build writing its log over and over, far more often than the
    // valve allows, for longer than its windows in a row.
    until(|| {
        fixture.fake.report(Changed::Paths(vec![log.clone(); 10]));
        std::thread::sleep(Duration::from_millis(10));
        fixture.indexer.user_rules().quarantined == [busy.clone()]
    });
    fixture.settle();
    assert!(
        fixture.names("busy log").is_empty(),
        "taken out of the index"
    );
    assert_eq!(fixture.names("plan"), ["plan.txt"], "nothing else is");
    let problems = fixture.indexer.problems();
    let churned = problems
        .iter()
        .find(|problem| problem.kind == ProblemKind::Churned)
        .expect("listed on the File search page");
    assert_eq!(churned.folder.as_deref(), Some(busy.as_path()));
    assert_eq!(
        churned.reason,
        "It changed more than 20 times in 1 second, 3 times in a row, so Pane took it out of \
         the index"
    );
    // Recorded in Pane's own record, so it stays out after a restart.
    assert_eq!(
        UserRules::read(&record).quarantined,
        std::slice::from_ref(&busy)
    );

    // A change there is no longer looked at.
    fixture.fake.report(Changed::Paths(vec![log.clone()]));
    fixture.settle();
    assert!(fixture.names("busy log").is_empty());
    // A change of the user's rules keeps it out.
    fixture
        .indexer
        .change_rules(UserRules {
            include_hidden: true,
            ..UserRules::default()
        })
        .unwrap();
    fixture.settle();
    assert_eq!(
        fixture.indexer.user_rules().quarantined,
        std::slice::from_ref(&busy)
    );
    assert!(fixture.names("busy log").is_empty());

    // Included again: walked again, found, and no longer listed.
    fixture.indexer.include_again(&busy).unwrap();
    fixture.settle();
    assert_eq!(fixture.names("busy log"), ["busy log.txt"]);
    assert!(
        fixture
            .indexer
            .problems()
            .iter()
            .all(|problem| problem.kind != ProblemKind::Churned)
    );
    assert!(UserRules::read(&record).quarantined.is_empty());
    assert!(UserRules::read(&record).include_hidden);
}

#[test]
fn a_folder_changing_now_and_then_is_never_taken_out() {
    let fixture = Fixture::indexed_with(|config| {
        config.valves.churn_changes = 20;
        config.valves.churn_window = Duration::from_millis(100);
        config.valves.churn_windows = 3;
    });
    let plan = fixture.home.join("Documents/plan.txt");
    // Busy in one window, then quiet: never three busy windows in a row.
    for _ in 0..3 {
        fixture.fake.report(Changed::Paths(vec![plan.clone(); 30]));
        fixture.settle();
        std::thread::sleep(Duration::from_millis(250));
    }
    fixture.fake.report(Changed::Paths(vec![plan.clone()]));
    fixture.settle();
    assert!(fixture.indexer.user_rules().quarantined.is_empty());
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
}

#[test]
fn a_walk_stops_at_the_ceiling_and_says_so() {
    let fixture = Fixture::indexed_with(|config| config.walk.max_entries = 3);
    let status = fixture.indexer.status();
    assert!(status.ceiling_reached);
    assert_eq!(status.state, IndexState::Current);
    assert_eq!(
        status.reason.as_deref(),
        Some("Pane stopped indexing at 3 entries; exclude folders to index the rest")
    );
    let problems = fixture.indexer.problems();
    let ceiling = problems
        .iter()
        .find(|problem| problem.kind == ProblemKind::Ceiling)
        .expect("listed on the File search page");
    assert_eq!(ceiling.reason, "Indexing stopped at 3 entries");
    assert!(ceiling.remedy.contains("Exclude folders"));
    // The default is 5 million, as #126 proposes.
    assert_eq!(WalkOptions::default().max_entries, 5_000_000);
    assert_eq!(count_words(5_000_000), "5 million");
}

#[test]
fn indexing_stops_while_the_disk_is_short_of_space_and_starts_again_by_itself() {
    let free = Arc::new(AtomicU64::new(0));
    let seen = free.clone();
    let fixture = fixture_with(|config| {
        config.valves.free_space_floor = 1_000;
        config.valves.space_retry = Duration::from_millis(20);
        config.valves.free_space = Arc::new(move |_| Some(seen.load(Ordering::SeqCst)));
    });
    fixture.indexer.launcher_shown();
    fixture.indexer.set_users(users(&[OWNER]));
    fixture.settle();
    let status = fixture.indexer.status();
    assert_eq!(status.state, IndexState::Stopped);
    assert!(status.low_space);
    assert!(fixture.names("plan").is_empty(), "nothing was written");
    let problems = fixture.indexer.problems();
    let low = problems
        .iter()
        .find(|problem| problem.kind == ProblemKind::LowSpace)
        .expect("listed on the File search page");
    assert!(low.reason.starts_with("Less than 1 KB"), "{}", low.reason);
    // The default floor is #126's proposed gigabyte, in the decimal units
    // Pane shows sizes in.
    assert_eq!(Valves::default().free_space_floor, 1_000_000_000);
    assert_eq!(size_words(Valves::default().free_space_floor), "1 GB");

    // Room again: the first walk runs, by itself.
    free.store(1 << 40, Ordering::SeqCst);
    until(|| fixture.indexer.status().state == IndexState::Current);
    fixture.settle();
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
    assert!(!fixture.indexer.status().low_space);
    assert!(fixture.indexer.problems().is_empty());

    // Short again while Pane runs: a change is let go of, and caught up
    // once there is room.
    free.store(0, Ordering::SeqCst);
    let documents = fixture.home.join("Documents");
    let written = documents.join("while short.txt");
    fs::write(&written, "x").unwrap();
    touch(&documents);
    fixture.fake.report(Changed::Paths(vec![written.clone()]));
    until(|| fixture.indexer.status().low_space);
    fixture.settle();
    assert!(fixture.names("while short").is_empty());
    free.store(1 << 40, Ordering::SeqCst);
    until(|| {
        let status = fixture.indexer.status();
        status.state == IndexState::Current && !status.low_space
    });
    fixture.settle();
    assert_eq!(fixture.names("while short"), ["while short.txt"]);
}

#[test]
fn a_folder_that_does_not_answer_is_skipped_for_the_walk_and_listed() {
    let fixture = fixture_with(|config| {
        config.walk.hung_after = Some(Duration::from_millis(200));
    });
    let music = fixture.home.join("Music");
    crate::file_index::walker::STALLED
        .lock()
        .unwrap()
        .push((music.clone(), Duration::from_secs(3)));
    let started = Instant::now();
    fixture.indexer.launcher_shown();
    fixture.indexer.set_users(users(&[OWNER]));
    fixture.settle();
    crate::file_index::walker::STALLED
        .lock()
        .unwrap()
        .retain(|(path, _)| *path != music);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the walk did not wait for it"
    );
    let status = fixture.indexer.status();
    assert_eq!(status.state, IndexState::Current);
    assert_eq!(status.hung, 1);
    assert_eq!(status.hung_folders, std::slice::from_ref(&music));
    assert!(fixture.names("song").is_empty(), "skipped for this walk");
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
    let problems = fixture.indexer.problems();
    let hung = problems
        .iter()
        .find(|problem| problem.kind == ProblemKind::Hung)
        .expect("listed on the File search page");
    assert_eq!(hung.folder.as_deref(), Some(music.as_path()));
    assert_eq!(
        hung.reason,
        "It did not answer within 200 ms, so Pane skipped it for this walk"
    );
}

#[test]
fn excluding_a_folder_takes_only_it_out_and_including_it_walks_only_it() {
    let fixture = Fixture::indexed();
    let music = fixture.home.join("Music");
    fixture.indexer.set_user_rules(UserRules {
        excluded_folders: vec![music.clone()],
        ..UserRules::default()
    });
    fixture.settle();
    assert!(fixture.names("song").is_empty());
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
    let walked_again = |fixture: &Fixture| {
        fixture.indexer.status().caught_up.map(|(by, _)| by) == Some(CaughtUpBy::FullWalk)
    };
    assert!(!walked_again(&fixture), "not walked again in full");
    fixture.indexer.set_user_rules(UserRules::default());
    fixture.settle();
    assert_eq!(fixture.names("song"), ["song.mp3"]);
    assert!(!walked_again(&fixture), "only the folder was walked");
}

#[test]
fn problems_read_for_people() {
    assert_eq!(count_words(0), "0");
    assert_eq!(count_words(1_000), "1,000");
    assert_eq!(count_words(450_097), "450,097");
    assert_eq!(count_words(12_345_678), "12,345,678");
}

#[test]
fn a_folder_macos_refused_is_listed_apart_from_one_that_cannot_be_read() {
    // What a walk found: the refusals are told by the system's answer
    // (`privacy`), so any folder may be one, not only Desktop, Documents
    // and Downloads.
    let fixture = Fixture::indexed();
    let refused = fixture.home.join("Music");
    let unreadable = fixture.home.join("Documents");
    {
        let mut shared = fixture.indexer.shared();
        shared.status.refused = 3;
        shared.status.refused_folders = vec![refused.clone()];
        shared.status.unreadable = 1;
        shared.status.unreadable_folders = vec![unreadable.clone()];
    }
    let problems = fixture.indexer.problems();
    let of = |kind: ProblemKind| -> Vec<&Problem> {
        problems
            .iter()
            .filter(|problem| problem.kind == kind)
            .collect()
    };
    let refusals = of(ProblemKind::Refused);
    assert_eq!(refusals.len(), 2, "{problems:?}");
    assert_eq!(refusals[0].folder.as_deref(), Some(refused.as_path()));
    assert_eq!(refusals[0].reason, "macOS did not allow Pane to read it");
    assert!(refusals[0].remedy.contains("Privacy & Security"));
    assert_eq!(refusals[1].folder, None);
    assert_eq!(
        refusals[1].reason,
        "macOS did not allow Pane to read 2 more folders"
    );
    let unreadables = of(ProblemKind::Unreadable);
    assert_eq!(unreadables.len(), 1);
    assert_eq!(unreadables[0].folder.as_deref(), Some(unreadable.as_path()));
    assert!(!unreadables[0].remedy.contains("Privacy"));
}

#[test]
fn indexing_pauses_while_the_computer_sleeps_and_resumes_a_while_after_it_wakes() {
    let computer = Arc::new(crate::file_index::power::tests::FakeAwake::default());
    let resume_after = Duration::from_millis(600);
    let fixture = Fixture::indexed_with({
        let computer = computer.clone();
        move |config| {
            config.valves.awake = computer;
            config.valves.resume_after = resume_after;
        }
    });
    assert!(!fixture.indexer.status().resuming);
    // A change while awake is applied at once.
    let documents = fixture.home.join("Documents");
    let before = documents.join("before sleep.txt");
    fs::write(&before, "x").unwrap();
    fixture.fake.report(Changed::Paths(vec![before]));
    fixture.settle();
    assert_eq!(fixture.names("before sleep"), ["before sleep.txt"]);

    // The computer sleeps for an hour; a change reported as it wakes
    // waits for the pause after the wake, the status saying so.
    computer.sleep(Duration::from_secs(3600));
    let woke = Instant::now();
    let after = documents.join("after wake.txt");
    fs::write(&after, "x").unwrap();
    fixture.fake.report(Changed::Paths(vec![after]));
    until(|| fixture.indexer.status().resuming);
    assert!(
        fixture.names("after wake").is_empty(),
        "nothing is applied during the pause"
    );
    fixture.settle();
    assert!(
        woke.elapsed() >= resume_after,
        "resumed only once the pause was over ({:?})",
        woke.elapsed()
    );
    assert!(!fixture.indexer.status().resuming);
    assert_eq!(fixture.names("after wake"), ["after wake.txt"]);

    // Awake again: the next change is applied at once.
    let later = documents.join("later.txt");
    fs::write(&later, "x").unwrap();
    let reported = Instant::now();
    fixture.fake.report(Changed::Paths(vec![later]));
    fixture.settle();
    assert!(reported.elapsed() < resume_after);
    assert_eq!(fixture.names("later"), ["later.txt"]);
}

#[test]
fn a_walk_under_way_when_the_computer_sleeps_waits_out_the_pause_and_finishes() {
    let computer = Arc::new(crate::file_index::power::tests::FakeAwake::default());
    let resume_after = Duration::from_millis(400);
    let fixture = fixture_with({
        let computer = computer.clone();
        move |config| {
            config.valves.awake = computer;
            config.valves.resume_after = resume_after;
        }
    });
    // Music answers slowly, so the walk is under way when the computer
    // sleeps.
    let music = fixture.home.join("Music");
    crate::file_index::walker::STALLED
        .lock()
        .unwrap()
        .push((music.clone(), Duration::from_millis(800)));
    fixture.indexer.launcher_shown();
    fixture.indexer.set_users(users(&[OWNER]));
    until(|| fixture.indexer.status().found > 0);
    computer.sleep(Duration::from_secs(600));
    // The walk's next folder, after the wake, waits out the pause.
    until(|| fixture.indexer.status().resuming);
    fixture.settle();
    crate::file_index::walker::STALLED
        .lock()
        .unwrap()
        .retain(|(path, _)| *path != music);
    let status = fixture.indexer.status();
    assert_eq!(status.state, IndexState::Current);
    assert!(!status.resuming);
    assert_eq!(status.hung, 0, "the sleep is not counted as hanging");
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
    assert_eq!(fixture.names("song"), ["song.mp3"]);
}

/// The fixture's own temporary folder, from its configuration: where a test
/// puts the folders it says are on other volumes.
fn temporary_folder(config: &IndexerConfig) -> PathBuf {
    config
        .dir
        .parent()
        .and_then(Path::parent)
        .expect("the index is in the fixture's cache folder")
        .to_path_buf()
}

#[test]
fn a_root_on_a_network_share_or_a_removable_drive_is_left_out_until_other_volumes_are_included() {
    // What Windows (GetDriveTypeW: a mapped drive, a USB stick) or macOS
    // (statfs: a mounted share, removable media) would say, through the
    // seam.
    let fixture = Fixture::indexed_with(|config| {
        let folder = temporary_folder(config);
        config.volumes = fake_volumes(&folder.join("share"), &folder.join("stick"));
        // The share is reconciled at once, as the few minutes are up.
        config.reconcile_unwatched = Duration::from_millis(50);
    });
    let share = fixture._dir.path().join("share");
    let stick = fixture._dir.path().join("stick");
    for (file, text) in [
        (share.join("Projects/far plan.txt"), "x"),
        (stick.join("Photos/holiday.jpg"), "x"),
    ] {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    let added = vec![share.clone(), stick.clone()];

    // Added as roots: left out by default, neither walked nor watched.
    fixture.indexer.set_user_rules(UserRules {
        added_roots: added.clone(),
        ..UserRules::default()
    });
    fixture.settle();
    assert!(fixture.names("far plan").is_empty());
    assert!(fixture.names("holiday").is_empty());
    assert_eq!(fixture.names("plan"), ["plan.txt"], "the home folder is");
    assert_eq!(
        *lock(&fixture.fake.watched),
        std::slice::from_ref(&fixture.home)
    );

    // Included: both are indexed; the removable drive is watched, the
    // network share never is.
    fixture.indexer.set_user_rules(UserRules {
        added_roots: added.clone(),
        include_other_volumes: true,
        ..UserRules::default()
    });
    fixture.settle();
    assert_eq!(fixture.names("far plan"), ["far plan.txt"]);
    assert_eq!(fixture.names("holiday"), ["holiday.jpg"]);
    assert_eq!(
        *lock(&fixture.fake.watched),
        [fixture.home.clone(), stick.clone()]
    );
    // A change on the share, which nothing reports, is found by the
    // reconciling walk made every few minutes.
    fs::write(share.join("Projects/later plan.txt"), "x").unwrap();
    touch(&share.join("Projects"));
    until(|| fixture.names("later plan") == ["later plan.txt"]);

    // Left out again: what was indexed of them goes.
    fixture.indexer.set_user_rules(UserRules {
        added_roots: added,
        ..UserRules::default()
    });
    fixture.settle();
    assert!(fixture.names("far plan").is_empty());
    assert!(fixture.names("holiday").is_empty());
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
}

#[test]
fn a_home_folder_on_a_network_share_is_left_out_until_other_volumes_are_included() {
    // A home folder redirected to a share, as the system would say.
    let fixture = Fixture::indexed_with(|config| {
        let folder = temporary_folder(config);
        config.volumes = fake_volumes(&folder.join("home"), &folder.join("stick"));
    });
    assert_eq!(fixture.indexer.status().state, IndexState::Current);
    assert!(fixture.names("plan").is_empty());
    assert!(lock(&fixture.fake.watched).is_empty());

    fixture.indexer.set_user_rules(UserRules {
        include_other_volumes: true,
        ..UserRules::default()
    });
    fixture.settle();
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
    assert!(
        lock(&fixture.fake.watched).is_empty(),
        "a network share is never watched"
    );
}

#[test]
fn churn_in_folders_the_index_leaves_out_never_takes_anything_out() {
    // The thresholds of the test above.
    let fixture = Fixture::indexed_with(|config| {
        config.valves.churn_changes = 20;
        config.valves.churn_window = Duration::from_secs(1);
        config.valves.churn_windows = 3;
    });
    let home = &fixture.home;
    for (file, text) in [
        ("repo/.git/HEAD", "ref: refs/heads/main\n"),
        ("repo/.git/index", "x"),
        ("repo/.gitignore", "build/\n"),
        ("repo/build/out.o", "x"),
        (".config/app/state.json", "x"),
        ("Busy/busy log.txt", "x"),
    ] {
        let path = home.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    // A busy `node_modules` (in the fixture), a Git repository's own
    // folder, an ignored build folder and a hidden folder, none of them
    // indexed; and a folder that is.
    let left_out = [
        home.join("node_modules/left out.js"),
        home.join("repo/.git/index"),
        home.join("repo/build/out.o"),
        home.join(".config/app/state.json"),
    ];
    let busy = home.join("Busy");
    let log = busy.join("busy log.txt");
    let burst = |paths: &[PathBuf]| {
        let reports: Vec<PathBuf> = paths
            .iter()
            .flat_map(|path| vec![path.clone(); 10])
            .collect();
        fixture.fake.report(Changed::Paths(reports));
    };

    // The same burst in each, far more often than the valve allows, until
    // the indexed folder is taken out: had the others been counted, they
    // would have been taken out with it.
    until(|| {
        burst(&left_out);
        burst(std::slice::from_ref(&log));
        std::thread::sleep(Duration::from_millis(10));
        fixture.indexer.user_rules().quarantined.contains(&busy)
    });
    // And on for more than a window: still nothing else.
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline {
        burst(&left_out);
        std::thread::sleep(Duration::from_millis(10));
    }
    fixture.settle();
    assert_eq!(
        fixture.indexer.user_rules().quarantined,
        std::slice::from_ref(&busy)
    );
    let churned: Vec<Problem> = fixture
        .indexer
        .problems()
        .into_iter()
        .filter(|problem| problem.kind == ProblemKind::Churned)
        .collect();
    assert_eq!(churned.len(), 1, "{churned:?}");
    assert_eq!(churned[0].folder.as_deref(), Some(busy.as_path()));
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
}

/// The folder-id table is read from the index once per open and shared by
/// the catch-up and the watch setup (#187): a source whose catch-up reads
/// it (as the NTFS journal's does) and that watches each folder (as
/// Linux's inotify does) has it read once, and the watch is given the
/// folders as the catch-up's changes left them.
#[test]
fn the_catch_up_and_the_watch_share_one_read_of_the_folder_ids() {
    let fixture = Fixture::indexed();
    fixture.indexer.set_users(BTreeSet::new());
    until(|| fixture.fake.watching.load(Ordering::SeqCst) == 0);

    // While off, a folder was made and another one deleted.
    let home = &fixture.home;
    let projects = home.join("Projects");
    fs::create_dir_all(&projects).unwrap();
    fs::remove_dir_all(home.join("Music")).unwrap();
    *lock(&fixture.fake.caught) = Some(Caught::Changes {
        changes: vec![
            Change::Put(Entry::read(&projects).unwrap()),
            Change::RemoveUnder(home.join("Music")),
        ],
        walk: Vec::new(),
        reconcile: Vec::new(),
        recheck: Vec::new(),
        cursors: vec![cursor()],
        how: CaughtUpBy::Journal,
        note: None,
    });
    fixture.fake.reads_folder_ids.store(true, Ordering::SeqCst);
    fixture
        .fake
        .watches_each_folder
        .store(true, Ordering::SeqCst);
    fixture.indexer.set_users(users(&[OWNER]));
    fixture.settle();

    let index = fixture.indexer.shared().index.clone().expect("open");
    assert_eq!(index.folder_id_reads(), 1, "read once, for both");
    let folders = lock(&fixture.fake.folders).clone();
    assert!(folders.contains(&projects), "{folders:?}");
    assert!(folders.contains(&home.join("Documents")), "{folders:?}");
    assert!(!folders.contains(&home.join("Music")), "{folders:?}");
    let depths: Vec<usize> = folders
        .iter()
        .map(|folder| folder.components().count())
        .collect();
    assert!(depths.is_sorted(), "shallowest first: {folders:?}");
}

/// On Windows a restart reads the folder-id table from the index once
/// (#187): the NTFS catch-up resolves the journal's records through it,
/// and watching each root whole needs none. Run on the runner's NTFS
/// volume; where the temporary folder's volume keeps no journal, the
/// catch-up reconciles and never reads it.
#[cfg(windows)]
#[test]
fn a_restart_on_windows_reads_the_folder_ids_once() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fs::create_dir_all(home.join("Documents/Drafts")).unwrap();
    fs::write(home.join("Documents/plan.txt"), "plan").unwrap();
    let index_dir = dir.path().join("cache").join(INDEX_DIR);
    let native = || IndexerConfig {
        source: crate::file_index::native_changes(),
        ..config(&index_dir, &home, Arc::new(Fake::default()))
    };
    let first = Indexer::default();
    first.configure(native(), UserRules::default());
    first.launcher_shown();
    first.set_users(users(&[OWNER]));
    assert!(first.wait_until_settled(LIMIT), "{:?}", first.status());
    first.set_users(BTreeSet::new());
    drop(first);

    // Changed while Pane is not running.
    fs::write(home.join("Documents/Drafts/letter.txt"), "x").unwrap();
    let indexer = Indexer::default();
    // Its index may be let go by the old coordinator a moment later.
    indexer.configure(native(), UserRules::default());
    indexer.launcher_shown();
    indexer.set_users(users(&[OWNER]));
    assert!(indexer.wait_until_settled(LIMIT), "{:?}", indexer.status());
    let index = indexer.shared().index.clone().expect("open");
    let reads = index.folder_id_reads();
    match indexer.status().caught_up.map(|(by, _)| by) {
        Some(CaughtUpBy::Journal) => assert_eq!(reads, 1, "read once, for both"),
        how => assert_eq!(reads, 0, "caught up {how:?}, never resolving records"),
    }
}

/// Makes a repository in the fixture's home folder whose `.gitignore`
/// ignores nothing yet, holding `src/trace.draft`, and has it indexed.
fn repository(fixture: &Fixture) -> PathBuf {
    let repo = fixture.home.join("repo");
    for (file, text) in [
        ("repo/.git/HEAD", "ref: refs/heads/main\n"),
        ("repo/.gitignore", ""),
        ("repo/src/trace.draft", "x"),
    ] {
        let path = fixture.home.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fixture.fake.report(Changed::Paths(vec![repo.clone()]));
    fixture.settle();
    assert_eq!(fixture.names("trace"), ["trace.draft"]);
    repo
}

/// A watcher's overflow (#186): an ignore file changed in place while the
/// system's buffer overflowed changes no folder's time, so the reconciling
/// walk does not see it; the folder is re-checked whole after it.
#[test]
fn an_overflow_rechecks_an_ignore_file_changed_in_place() {
    let fixture = Fixture::indexed();
    let repo = repository(&fixture);
    fs::write(repo.join(".gitignore"), "*.draft\n").unwrap();
    fixture.fake.report(Changed::Rescan(fixture.home.clone()));
    fixture.settle();
    assert!(fixture.names("trace").is_empty());
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
}

/// A catch-up whose records do not name what changed a folder's ignore
/// rules (a hidden `.gitignore` deleted, read without administrator
/// rights; a folder Linux's reconciling walk read again holding one) asks
/// for that folder to be re-checked, which runs with the walks (#186).
#[test]
fn a_folder_the_catch_up_asks_to_recheck_is_rechecked_with_the_walks() {
    let fixture = Fixture::indexed();
    let repo = repository(&fixture);
    fixture.indexer.set_users(BTreeSet::new());
    until(|| fixture.fake.watching.load(Ordering::SeqCst) == 0);

    fs::write(repo.join(".gitignore"), "*.draft\n").unwrap();
    *lock(&fixture.fake.caught) = Some(Caught::Changes {
        changes: Vec::new(),
        walk: Vec::new(),
        reconcile: Vec::new(),
        recheck: vec![repo],
        cursors: vec![cursor()],
        how: CaughtUpBy::Journal,
        note: None,
    });
    fixture.indexer.set_users(users(&[OWNER]));
    fixture.settle();
    assert!(fixture.names("trace").is_empty());
}

/// An ignore file in the home folder re-checks every folder, a few at a
/// time (#186): the result is the same as one walk, and a change reported
/// meanwhile is applied too.
#[test]
fn an_ignore_file_in_the_home_folder_rechecks_every_folder_a_few_at_a_time() {
    let fixture = Fixture::indexed();
    let home = &fixture.home;
    // More folders than a re-check reads before it lets changes through.
    let count = RECHECK_FOLDERS * 2;
    let mut made = Vec::new();
    for n in 0..count {
        let folder = home.join(format!("Folder {n}"));
        fs::create_dir_all(folder.join("deep")).unwrap();
        fs::write(folder.join(format!("deep/note {n}.draft")), "x").unwrap();
        made.push(folder);
    }
    fixture.fake.report(Changed::Paths(made));
    fixture.settle();
    let notes = || {
        fixture
            .indexer
            .search(
                OWNER,
                "note",
                SearchOptions {
                    limit: MAX_RESULTS,
                    ..SearchOptions::default()
                },
            )
            .unwrap()
            .len()
    };
    assert_eq!(notes(), count);

    fs::write(home.join(".ignore"), "*.draft\n").unwrap();
    fixture
        .fake
        .report(Changed::Paths(vec![home.join(".ignore")]));
    let meanwhile = home.join("Documents/meanwhile.txt");
    fs::write(&meanwhile, "x").unwrap();
    fixture.fake.report(Changed::Paths(vec![meanwhile]));
    fixture.settle();
    assert_eq!(notes(), 0);
    assert_eq!(fixture.names("meanwhile"), ["meanwhile.txt"]);
    assert_eq!(fixture.names("plan"), ["plan.txt"]);
}

/// Of a folder no change is reported from (here past Linux's watch limit),
/// the scope keeps nothing (#186): the check at Enter reads its ignore
/// files again, and sees one that changed since.
#[test]
fn the_check_at_enter_reads_again_the_rules_of_a_folder_not_watched() {
    let fixture = Fixture::indexed();
    let documents = fixture.home.join("Documents");
    let found = fixture
        .indexer
        .search(OWNER, "plan", SearchOptions::default())
        .unwrap();
    // Checked once: what the rules learned of Documents was kept.
    assert!(fixture.indexer.checked(OWNER, &found[0].id).is_ok());
    fixture
        .fake
        .report(Changed::Unwatched(vec![documents.clone()]));
    until(|| fixture.indexer.status().unwatched == 1);

    fs::write(documents.join(".ignore"), "plan.txt\n").unwrap();
    assert_eq!(
        fixture.indexer.checked(OWNER, &found[0].id),
        Err("it is no longer in the folders file search covers".into())
    );
}
