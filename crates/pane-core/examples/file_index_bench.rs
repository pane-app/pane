//! The file index's benchmark (#174): first index time (first run and
//! warm runs), index size on disk, query latency percentiles, a changed
//! file re-indexed, the catch-up after 10,000 changes (Windows), and
//! memory, against the targets of #126 (Raycast: 450,097 entries in 12.9 s,
//! 61.8 MB on disk).
//!
//! Run through `cargo xtask file-index-bench [options]`, which builds this
//! in release. Options:
//!
//! - `--home`: index the real home folder with the default rules (read
//!   only: nothing is written there).
//! - `--root <folder>`: index that folder with the default rules.
//! - `--generate <entries>` (the default, 450000): index a generated
//!   home-shaped tree of about that many indexable entries (9 files per
//!   folder, deep paths, accents, plus hidden folders, `node_modules`,
//!   Git repositories with ignored folders), kept in `--tree` and reused by
//!   later runs.
//! - `--tree <folder>`: where the generated tree lives (default: the
//!   system's temporary folder, `pane-file-index-bench/tree-<entries>`).
//! - `--index <folder>`: where the index is written (default: the system's
//!   temporary folder); deleted before each run and at the end.
//! - `--runs <n>` (default 3): first index runs; run 1 is the first, the
//!   others are warm.
//! - `--queries <n>` (default 1000): queries timed, twice each.
//! - `--threads <n>`: walker threads (default: as Pane uses).
//! - `--foreground`: walk at normal priority instead of background.
//! - `--drop-caches`: before run 1, drop the system's file cache where that
//!   is allowed (Linux as root, macOS with `sudo purge`), so that run 1 is
//!   cold. Windows offers no way without administrator rights: there run 1
//!   is cold only when it is the first after a restart.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use pane_core::file_index::{
    self, Change, Entry, EntryKind, FileIndex, JournalRead, Opened, Query, Scope, ScopeRules,
    WalkOptions,
};

struct Options {
    mode: Mode,
    tree: Option<PathBuf>,
    index: Option<PathBuf>,
    runs: usize,
    queries: usize,
    threads: Option<usize>,
    foreground: bool,
    drop_caches: bool,
}

enum Mode {
    Home,
    Root(PathBuf),
    Generate(usize),
}

fn usage() -> ! {
    eprintln!(
        "usage: file_index_bench [--home | --root <folder> | --generate <entries>] \
         [--tree <folder>] [--index <folder>] [--runs <n>] [--queries <n>] [--threads <n>] \
         [--foreground] [--drop-caches]"
    );
    std::process::exit(2)
}

fn options() -> Options {
    let mut options = Options {
        mode: Mode::Generate(450_000),
        tree: None,
        index: None,
        runs: 3,
        queries: 1_000,
        threads: None,
        foreground: false,
        drop_caches: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || match args.next() {
            Some(value) => value,
            None => usage(),
        };
        match arg.as_str() {
            "--home" => options.mode = Mode::Home,
            "--root" => options.mode = Mode::Root(value().into()),
            "--generate" => {
                options.mode = Mode::Generate(value().parse().unwrap_or_else(|_| usage()))
            }
            "--tree" => options.tree = Some(value().into()),
            "--index" => options.index = Some(value().into()),
            "--runs" => options.runs = value().parse().unwrap_or_else(|_| usage()),
            "--queries" => options.queries = value().parse().unwrap_or_else(|_| usage()),
            "--threads" => options.threads = Some(value().parse().unwrap_or_else(|_| usage())),
            "--foreground" => options.foreground = true,
            "--drop-caches" => options.drop_caches = true,
            _ => usage(),
        }
    }
    options.runs = options.runs.max(1);
    options
}

fn home_dir() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .expect("the home folder's variable is set")
}

/// Pane's own folders, which the index never holds.
fn panes_folders() -> Vec<PathBuf> {
    let home = home_dir();
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(|dir| vec![PathBuf::from(dir).join("Pane")])
            .unwrap_or_default()
    } else if cfg!(target_os = "macos") {
        vec![
            home.join("Library/Caches/Pane"),
            home.join("Library/Application Support/Pane"),
        ]
    } else {
        let xdg = |var: &str, default: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(default))
                .join("pane")
        };
        vec![
            xdg("XDG_CACHE_HOME", ".cache"),
            xdg("XDG_DATA_HOME", ".local/share"),
            xdg("XDG_STATE_HOME", ".local/state"),
        ]
    }
}

fn main() {
    let options = options();
    let base = std::env::temp_dir().join("pane-file-index-bench");
    let index_dir = options
        .index
        .clone()
        .unwrap_or_else(|| base.join(format!("index-{}", std::process::id())));
    let (root, described, generated) = match &options.mode {
        Mode::Home => (home_dir(), "the home folder".to_owned(), false),
        Mode::Root(root) => (root.clone(), root.display().to_string(), false),
        Mode::Generate(entries) => {
            let tree = options
                .tree
                .clone()
                .unwrap_or_else(|| base.join(format!("tree-{entries}")));
            let started = Instant::now();
            let made = generate(&tree, *entries);
            if made {
                println!(
                    "generated a tree of about {entries} indexable entries in {:.1} s at {}",
                    started.elapsed().as_secs_f64(),
                    tree.display()
                );
            }
            (
                tree,
                format!("a generated tree of about {entries} entries"),
                true,
            )
        }
    };
    let mut rules = ScopeRules::for_home(root.clone());
    rules.always_excluded = panes_folders();
    rules.always_excluded.push(index_dir.clone());
    let scope = Scope::new(rules);
    let mut walk_options = WalkOptions::default();
    if let Some(threads) = options.threads {
        walk_options.threads = threads.max(1);
    }
    walk_options.background = !options.foreground;

    println!(
        "File index benchmark: {described}, {} walker threads at {} priority, {} on {}",
        walk_options.threads,
        if walk_options.background {
            "background"
        } else {
            "normal"
        },
        std::env::consts::OS,
        std::env::consts::ARCH,
    );

    let mut dropped = false;
    if options.drop_caches {
        dropped = drop_caches();
        println!(
            "dropping the file cache before run 1: {}",
            if dropped {
                "done"
            } else {
                "not possible here (see --help)"
            }
        );
    }

    // First index, several runs.
    let mut runs: Vec<Run> = Vec::new();
    let mut sample: Vec<PathBuf> = Vec::new();
    for run in 1..=options.runs {
        let _ = std::fs::remove_dir_all(&index_dir);
        let measured = index_once(&index_dir, &root, &scope, &walk_options, &mut sample);
        println!(
            "run {run}: {} entries in {:.2} s (walk {:.2} s), {} on disk, {} folders unreadable",
            measured.entries,
            measured.total.as_secs_f64(),
            measured.walk.as_secs_f64(),
            megabytes(measured.bytes),
            measured.unreadable,
        );
        runs.push(measured);
    }
    let peak_after_index = memory().peak;

    // Pane starting again: the index opened, as it is while idle.
    let before_open = memory().private;
    let opened_at = Instant::now();
    let (index, opened) =
        FileIndex::open(&index_dir, std::slice::from_ref(&root)).expect("the index opens");
    let open_time = opened_at.elapsed();
    assert_eq!(opened, Opened::Existing);
    let after_open = memory().private;

    // Queries.
    let queries = query_set(&sample, options.queries);
    let first_pass = time_queries(&index, &queries);
    let second_pass = time_queries(&index, &queries);
    let idle = memory().private;

    // A changed file re-indexed: written, read, applied, found.
    let changes = if generated {
        root.join("pane-bench-changes")
    } else {
        base.join(format!("changes-{}", std::process::id()))
    };
    let _ = std::fs::remove_dir_all(&changes);
    std::fs::create_dir_all(&changes).expect("a folder for the changed files");
    let mut reindexed = Vec::new();
    for n in 0..100 {
        let file = changes.join(format!("changed report {n} zqx.txt"));
        std::fs::write(&file, b"changed").expect("a changed file");
        let started = Instant::now();
        let entry = Entry::read(&file).expect("the changed file");
        // Outside the root (the home folder is never written to), the
        // entry is indexed under the root, as the engine does not mind.
        let entry = if generated {
            entry
        } else {
            Entry {
                path: root.join(file.file_name().unwrap()),
                meta: entry.meta,
            }
        };
        let path = entry.path.clone();
        index.apply(&[Change::Put(entry)]).expect("applied");
        let text = format!("changed report {n} zqx");
        let found = index
            .search(&Query {
                text: &text,
                limit: 5,
                offset: 0,
                kind: None,
            })
            .iter()
            .any(|hit| hit.path == path);
        reindexed.push(started.elapsed());
        assert!(found, "the changed file is found");
        if !generated {
            index.apply(&[Change::Remove(path)]).expect("removed again");
        }
    }

    // The catch-up after 10,000 changes, from the change journal.
    let catch_up = if generated {
        catch_up(&index, &scope, &root, &changes)
    } else {
        None
    };
    let _ = std::fs::remove_dir_all(&changes);
    drop(index);
    let _ = std::fs::remove_dir_all(&index_dir);

    // The results.
    let first = &runs[0];
    let warm: Vec<Duration> = runs[1..].iter().map(|run| run.total).collect();
    let entries = first.entries.max(1);
    println!();
    println!("| Measure | Pane | Target (#126) | Raycast |");
    println!("| --- | --- | --- | --- |");
    println!("| Entries indexed | {} | | 450,097 |", first.entries);
    println!(
        "| First index, run 1 ({}) | {:.2} s | below 12.9 s, aim 6.45 s | 12.9 s |",
        if dropped {
            "cold: cache dropped"
        } else {
            "cold only if first since a restart"
        },
        first.total.as_secs_f64()
    );
    if !warm.is_empty() {
        println!(
            "| First index, warm (median of {} runs) | {:.2} s | below 12.9 s, aim 6.45 s | |",
            warm.len(),
            median(&warm).as_secs_f64()
        );
    }
    println!(
        "| Index on disk | {} ({:.0} B per entry) | below 61.8 MB | 61.8 MB (≈137 B per entry) |",
        megabytes(first.bytes),
        first.bytes as f64 / entries as f64
    );
    println!(
        "| Query, 95th percentile (first pass after opening) | {} | under 10 ms | not measured |",
        millis(percentile(&first_pass, 95.0))
    );
    println!(
        "| Query, 95th percentile (warm) | {} (p50 {}, p99 {}, max {}) | under 10 ms | not measured |",
        millis(percentile(&second_pass, 95.0)),
        millis(percentile(&second_pass, 50.0)),
        millis(percentile(&second_pass, 99.0)),
        millis(percentile(&second_pass, 100.0)),
    );
    println!(
        "| A changed file re-indexed, 95th percentile | {} (p50 {}) | under 10 ms | 4–10 ms |",
        millis(percentile(&reindexed, 95.0)),
        millis(percentile(&reindexed, 50.0)),
    );
    match &catch_up {
        Some(Ok((time, records))) => println!(
            "| Catch-up after 10,000 changes | {:.3} s ({records} records) | under 1 s | not measured |",
            time.as_secs_f64()
        ),
        Some(Err(why)) => {
            println!("| Catch-up after 10,000 changes | not measured: {why} | under 1 s | |")
        }
        None => println!(
            "| Catch-up after 10,000 changes | not measured (only on a generated tree, on Windows) | under 1 s | |"
        ),
    }
    println!("| Opening the index at start | {} | | |", millis(open_time));
    println!(
        "| Private memory added by the open index, idle | {} (after queries: {}) | after measuring (#4) | |",
        megabytes(after_open.saturating_sub(before_open)),
        megabytes(idle.saturating_sub(before_open)),
    );
    println!(
        "| Peak memory of the benchmark while indexing | {} | | |",
        megabytes(peak_after_index)
    );
    println!();
    println!(
        "Queries: {} of 5 kinds (whole name, name prefix, a word, folder and name words, one letter), limit 20.",
        queries.len()
    );
    for kind in QueryKind::ALL {
        let times: Vec<Duration> = queries
            .iter()
            .zip(&second_pass)
            .filter(|(query, _)| query.kind == kind)
            .map(|(_, time)| *time)
            .collect();
        if !times.is_empty() {
            println!(
                "  {:<22} p50 {}  p95 {}  max {}",
                kind.label(),
                millis(percentile(&times, 50.0)),
                millis(percentile(&times, 95.0)),
                millis(percentile(&times, 100.0)),
            );
        }
    }
}

struct Run {
    entries: u64,
    walk: Duration,
    total: Duration,
    bytes: u64,
    unreadable: u64,
}

/// One first index: the walk, the entries prepared on its threads and
/// written by this one, then merged into one segment.
fn index_once(
    index_dir: &Path,
    root: &Path,
    scope: &Scope,
    walk_options: &WalkOptions,
    sample: &mut Vec<PathBuf>,
) -> Run {
    let (index, _) = FileIndex::open(index_dir, std::slice::from_ref(&root.to_path_buf()))
        .expect("the index opens");
    let started = Instant::now();
    let (sender, receiver) = mpsc::sync_channel::<file_index::PreparedBatch>(64);
    let (sample_sender, sample_receiver) = mpsc::channel::<PathBuf>();
    let cancel = AtomicBool::new(false);
    let mut walk_time = Duration::ZERO;
    let report = std::thread::scope(|threads| {
        let index = &index;
        let walker = threads.spawn(move || {
            let report = file_index::walk(scope, walk_options, &cancel, &|batch: Vec<Entry>| {
                // About one entry in 400 named for the queries.
                for entry in batch.iter().step_by(400) {
                    let _ = sample_sender.send(entry.path.clone());
                }
                let _ = sender.send(index.prepare(batch));
            });
            (report, started.elapsed())
        });
        let mut bulk = index.bulk().expect("a bulk writer");
        for batch in receiver {
            bulk.add(batch).expect("entries written");
        }
        let (report, walked) = walker.join().expect("the walk ends");
        walk_time = walked;
        bulk.finish().expect("segments merged");
        report
    });
    let total = started.elapsed();
    let mut seen: HashSet<PathBuf> = sample.iter().cloned().collect();
    for path in sample_receiver.try_iter() {
        if seen.insert(path.clone()) {
            sample.push(path);
        }
    }
    let bytes = index.stats().bytes_on_disk;
    Run {
        entries: report.entries,
        walk: walk_time,
        total,
        bytes,
        unreadable: report.unreadable,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QueryKind {
    WholeName,
    NamePrefix,
    Word,
    FolderAndName,
    OneLetter,
}

impl QueryKind {
    const ALL: [QueryKind; 5] = [
        QueryKind::WholeName,
        QueryKind::NamePrefix,
        QueryKind::Word,
        QueryKind::FolderAndName,
        QueryKind::OneLetter,
    ];

    fn label(self) -> &'static str {
        match self {
            QueryKind::WholeName => "whole name",
            QueryKind::NamePrefix => "name prefix (3 letters)",
            QueryKind::Word => "a word",
            QueryKind::FolderAndName => "folder and name words",
            QueryKind::OneLetter => "one letter",
        }
    }
}

struct TimedQuery {
    kind: QueryKind,
    text: String,
}

fn words_of(name: &str) -> Vec<String> {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// A fixed set of queries from names the walk found: each kind in turn.
fn query_set(sample: &[PathBuf], count: usize) -> Vec<TimedQuery> {
    let mut queries = Vec::new();
    let mut at = 0usize;
    while queries.len() < count && !sample.is_empty() {
        let path = &sample[at % sample.len()];
        let kind = QueryKind::ALL[queries.len() % 5];
        at += 7;
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let words = words_of(&name);
        let text = match kind {
            QueryKind::WholeName => name.clone(),
            QueryKind::NamePrefix => name.chars().take(3).collect(),
            QueryKind::Word => match words.get(at % words.len().max(1)) {
                Some(word) => word.clone(),
                None => continue,
            },
            QueryKind::FolderAndName => {
                let folder = path
                    .parent()
                    .and_then(Path::file_name)
                    .map(|n| words_of(&n.to_string_lossy()))
                    .unwrap_or_default();
                match (folder.first(), words.first()) {
                    (Some(folder), Some(word)) => format!("{folder} {word}"),
                    _ => continue,
                }
            }
            QueryKind::OneLetter => name.chars().take(1).collect(),
        };
        if text.trim().is_empty() {
            continue;
        }
        queries.push(TimedQuery { kind, text });
    }
    queries
}

fn time_queries(index: &FileIndex, queries: &[TimedQuery]) -> Vec<Duration> {
    queries
        .iter()
        .map(|query| {
            let started = Instant::now();
            let hits = index.search(&Query {
                text: &query.text,
                limit: 20,
                offset: 0,
                kind: None,
            });
            std::hint::black_box(hits);
            started.elapsed()
        })
        .collect()
}

/// The catch-up after 10,000 files created while "Pane was not running":
/// read from the change journal, resolved, looked at and applied.
fn catch_up(
    index: &FileIndex,
    scope: &Scope,
    root: &Path,
    changes: &Path,
) -> Option<Result<(Duration, usize), String>> {
    if !cfg!(windows) {
        return None;
    }
    // The folder the changes go to is indexed first, with its file id.
    let folder = Entry::read(changes).ok()?;
    index.apply(&[Change::Put(folder)]).ok()?;
    let start = match file_index::read_journal(root, None, usize::MAX) {
        JournalRead::Records { cursor, .. } => cursor,
        other => return Some(Err(format!("{other:?}"))),
    };
    for n in 0..10_000 {
        if let Err(error) = std::fs::write(changes.join(format!("caught up {n}.txt")), b"x") {
            return Some(Err(error.to_string()));
        }
    }
    let started = Instant::now();
    let records = match file_index::read_journal(root, Some(&start), 1_000_000) {
        JournalRead::Records { records, .. } => records,
        other => return Some(Err(format!("{other:?}"))),
    };
    let mut folders = index.folder_ids();
    let mut names = file_index::Names::for_volume_of(root);
    let resolved = file_index::resolve(&records, &mut folders, &mut |id| names.name(id));
    let (found, _walk) = file_index::catch_up_changes(scope, &resolved);
    let mut changes_to_apply = file_index::missing_from(index, &resolved.listed);
    changes_to_apply.extend(found);
    if let Err(error) = index.apply(&changes_to_apply) {
        return Some(Err(error.to_string()));
    }
    let time = started.elapsed();
    let found = index.search(&Query {
        text: "caught up 9999",
        limit: 5,
        offset: 0,
        kind: Some(EntryKind::File),
    });
    if found.is_empty() {
        return Some(Err("the last file caught up was not found".into()));
    }
    Some(Ok((time, records.len())))
}

/// Writes the generated tree at `tree` unless it is already there: about
/// `entries` indexable entries, 9 files per folder, folders four to a
/// parent (so paths run about 8 folders deep), names with words, numbers
/// and accents; and, not indexed, a hidden folder in every 40th folder, a
/// `node_modules` in every 50th and a Git repository with an ignored
/// `build` folder and logs in every 100th. Returns whether it wrote it.
fn generate(tree: &Path, entries: usize) -> bool {
    let marker = tree.join(".pane-bench-complete");
    if marker.exists() {
        return false;
    }
    let _ = std::fs::remove_dir_all(tree);
    const WORDS: [&str; 24] = [
        "report",
        "invoice",
        "photo",
        "IMG",
        "notes",
        "draft",
        "Résumé",
        "budget",
        "plan",
        "meeting",
        "project",
        "src",
        "lib",
        "test",
        "data",
        "backup",
        "music",
        "video",
        "design",
        "final",
        "café",
        "Übersicht",
        "año",
        "summary",
    ];
    const EXTENSIONS: [&str; 10] = [
        ".txt", ".pdf", ".jpg", ".docx", ".rs", ".md", ".png", ".xlsx", ".mp3", ".zip",
    ];
    let folders = (entries / 10).max(1);
    let mut paths: Vec<PathBuf> = Vec::with_capacity(folders);
    paths.push(tree.to_path_buf());
    for n in 1..folders {
        let parent = paths[(n - 1) / 4].clone();
        paths.push(parent.join(format!("{} {n}", WORDS[n % WORDS.len()])));
    }
    for path in &paths {
        std::fs::create_dir_all(path).expect("a generated folder");
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for t in 0..threads {
            let paths = &paths;
            scope.spawn(move || {
                for (n, folder) in paths.iter().enumerate().skip(t).step_by(threads) {
                    for f in 0..9 {
                        let word = WORDS[(n * 9 + f) % WORDS.len()];
                        let other = WORDS[(n + f * 7) % WORDS.len()];
                        let extension = EXTENSIONS[(n + f) % EXTENSIONS.len()];
                        let name = format!("{word} {other}_{}{extension}", n * 9 + f);
                        std::fs::write(folder.join(name), b"").expect("a generated file");
                    }
                    let extra = |sub: &str, count: usize| {
                        let dir = folder.join(sub);
                        std::fs::create_dir_all(&dir).expect("a generated folder");
                        for i in 0..count {
                            std::fs::write(dir.join(format!("left out {i}.js")), b"")
                                .expect("a generated file");
                        }
                    };
                    if n % 40 == 39 {
                        extra(".cache", 5);
                    }
                    if n % 50 == 49 {
                        extra("node_modules", 20);
                    }
                    if n % 100 == 99 {
                        std::fs::create_dir_all(folder.join(".git")).expect("a repository");
                        std::fs::write(folder.join(".gitignore"), b"build/\n*.log\n")
                            .expect("an ignore file");
                        extra("build", 10);
                        for i in 0..2 {
                            std::fs::write(folder.join(format!("run {i}.log")), b"")
                                .expect("a log");
                        }
                    }
                }
            });
        }
    });
    std::fs::write(&marker, b"").expect("the tree's marker");
    true
}

/// Drops the system's file cache where that is allowed.
fn drop_caches() -> bool {
    if cfg!(target_os = "linux") {
        let _ = std::process::Command::new("sync").status();
        std::fs::write("/proc/sys/vm/drop_caches", b"3").is_ok()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("sudo")
            .args(["-n", "purge"])
            .status()
            .is_ok_and(|status| status.success())
    } else {
        false
    }
}

struct Memory {
    /// Private bytes (Windows), anonymous resident bytes (Linux), 0 where
    /// the system does not say (macOS).
    private: u64,
    /// The most resident memory the process has had.
    peak: u64,
}

#[cfg(windows)]
fn memory() -> Memory {
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: this process's pseudo handle and a structure of the size given.
    let read = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    };
    if read.is_err() {
        return Memory {
            private: 0,
            peak: 0,
        };
    }
    Memory {
        private: counters.PrivateUsage as u64,
        peak: counters.PeakWorkingSetSize as u64,
    }
}

#[cfg(target_os = "linux")]
fn memory() -> Memory {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find(|line| line.starts_with(name))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|kb| kb.parse::<u64>().ok())
            .map_or(0, |kb| kb * 1024)
    };
    Memory {
        private: field("RssAnon:"),
        peak: field("VmHWM:"),
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn memory() -> Memory {
    // SAFETY: a structure of the call's own type.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    Memory {
        private: 0,
        // Bytes on macOS.
        peak: usage.ru_maxrss as u64,
    }
}

fn median(times: &[Duration]) -> Duration {
    let mut sorted = times.to_vec();
    sorted.sort();
    sorted[sorted.len() / 2]
}

fn percentile(times: &[Duration], at: f64) -> Duration {
    if times.is_empty() {
        return Duration::ZERO;
    }
    let mut sorted = times.to_vec();
    sorted.sort();
    let rank = ((at / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn millis(time: Duration) -> String {
    format!("{:.2} ms", time.as_secs_f64() * 1000.0)
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}
