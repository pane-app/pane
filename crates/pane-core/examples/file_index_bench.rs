//! The file index's benchmark (#174, #183): first index time (first run
//! and warm runs), index size on disk, query latency percentiles per kind
//! of query on two shapes of the index (one segment, as the first index
//! leaves it; several segments and changes in memory, as a stream of
//! changes leaves it) and while changes arrive and segments are merged in
//! the background (#187), a changed file re-indexed near the root and in a
//! deep folder under ignore files (the folders above kept between changes,
//! #186, and with nothing kept), the catch-up after 10,000 changes
//! (Windows), and memory, against the targets of #126 (Raycast: 450,097
//! entries in 12.9 s, 61.8 MB on disk).
//!
//! Run through `cargo xtask file-index-bench [options]`, which builds this
//! in release. CI runs it over a reduced tree as a regression guard
//! (`cargo xtask file-index-guard`, with `--guard`). Options:
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
//! - `--queries <n>` (default 1000): queries in the set, about as many of
//!   each kind (a typed-out name counts each keystroke); the set is timed
//!   twice on each shape of the index.
//! - `--threads <n>`: walker threads (default: as Pane uses).
//! - `--foreground`: walk at normal priority instead of background.
//! - `--drop-caches`: before run 1, drop the system's file cache where that
//!   is allowed (Linux as root, macOS with `sudo purge`), so that run 1 is
//!   cold. Windows offers no way without administrator rights: there run 1
//!   is cold only when it is the first after a restart.
//! - `--guard`: check the measures against the regression guard's ceilings
//!   (`GUARD_*` below, set for CI's reduced run) and exit with 1 when one
//!   is over its ceiling.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pane_core::file_index::{
    self, Admitted, Category, Change, Entry, EntryKind, FileIndex, JournalRead, Meta, Opened,
    Query, Scope, ScopeRules, WalkOptions,
};

/// The regression guard's ceilings (#183, `--guard`), for the run CI makes
/// (`cargo xtask file-index-guard`: a generated tree of about 20,000
/// entries, one run, built in the development profile, unoptimized, on a
/// shared runner). They are set far above what that run should take, so
/// that only a large regression (a query or a change gone quadratic, an
/// index several times larger) fails it, never a slow runner.
const GUARD_FIRST_INDEX: Duration = Duration::from_secs(180);
const GUARD_BYTES_PER_ENTRY: f64 = 400.0;
/// For the 95th percentile of every kind of query and of all of them, on
/// each shape of the index (warm).
const GUARD_QUERY_P95: Duration = Duration::from_millis(2_500);
/// For the 95th percentile of each re-index row.
const GUARD_REINDEX_P95: Duration = Duration::from_millis(2_500);

/// The batches of the stream of changes timed queries see on the second
/// shape of the index: all but the last are written as a segment of their
/// own, the last stays in the memory table (at most 10,000 changes, below
/// the 65,536 entries past which it is written). The index's background
/// merges (#187) are held off while that shape is timed, so it holds 5
/// segments (the one the first index left, and the stream's 4) and the
/// changes in memory; the row while changes arrive lets them run.
const STREAM_BATCHES: usize = 5;

/// The words and extensions of the generated tree's names, and of the
/// stream's.
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

struct Options {
    mode: Mode,
    tree: Option<PathBuf>,
    index: Option<PathBuf>,
    runs: usize,
    queries: usize,
    threads: Option<usize>,
    foreground: bool,
    drop_caches: bool,
    guard: bool,
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
         [--foreground] [--drop-caches] [--guard]"
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
        guard: false,
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
            "--guard" => options.guard = true,
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

    // The query set, the same for the same tree (see `query_set`): built
    // from the names the walk found, sorted, not in the order the walk's
    // threads found them. Its misses are checked on an index opened for
    // that and closed again, so that the index timed below is opened
    // afresh, as Pane opens it.
    sample.sort();
    let queries = {
        let (index, _) =
            FileIndex::open(&index_dir, std::slice::from_ref(&root)).expect("the index opens");
        query_set(&index, &sample, options.queries)
    };

    // Pane starting again: the index opened, as it is while idle.
    let before_open = memory().private;
    let opened_at = Instant::now();
    let (index, opened) =
        FileIndex::open(&index_dir, std::slice::from_ref(&root)).expect("the index opens");
    let open_time = opened_at.elapsed();
    assert_eq!(opened, Opened::Existing);
    let after_open = memory().private;

    // Queries on the first index: one segment, nothing in memory.
    let one_first = time_queries(&index, &queries);
    let one_warm = time_queries(&index, &queries);
    let idle = memory().private;

    // A changed file re-indexed, near the root and in a deep folder under
    // ignore files.
    let changes = if generated {
        root.join("pane-bench-changes")
    } else {
        base.join(format!("changes-{}", std::process::id()))
    };
    let _ = std::fs::remove_dir_all(&changes);
    std::fs::create_dir_all(&changes).expect("a folder for the changed files");
    // Outside the root (the home folder is never written to), the file is
    // indexed as if it were in the root, as the engine does not mind.
    let near_root = reindex(
        &index,
        &scope,
        &changes,
        "report",
        (!generated).then_some(root.as_path()),
        false,
    );
    let deep = deep_folder(&root, &scope, &sample);
    let deep_times = deep.as_ref().map(|deep| {
        let folder = Some(deep.path.as_path());
        reindex(&index, &scope, &changes, "deep report", folder, false)
    });
    // The same with nothing kept, every folder above read again each time:
    // the first change after a start, or after what the scope kept of the
    // folders above was dropped (#186), as #183 measured every change.
    let deep_cold = deep.as_ref().map(|deep| {
        let folder = Some(deep.path.as_path());
        reindex(&index, &scope, &changes, "deep cold report", folder, true)
    });

    // The catch-up after 10,000 changes, from the change journal.
    let catch_up = if generated {
        catch_up(&index, &scope, &root, &changes)
    } else {
        None
    };
    let _ = std::fs::remove_dir_all(&changes);

    // The same queries on the index a stream of changes left: several
    // segments and changes in memory, below the merge threshold. It starts
    // from one segment again, without what the changes above put in the
    // generated tree's index. The background merges (#187) are held off
    // meanwhile, so that the queries are timed on the segments the stream
    // wrote, as they are before merges catch up, whatever the runner's speed.
    if generated {
        index
            .apply(&[Change::RemoveUnder(changes.clone())])
            .expect("the changed files removed again");
    }
    index.compact().expect("the index merged into one segment");
    let per_batch = (runs[0].entries as usize / 50).clamp(500, 10_000);
    index.hold_merges(true);
    let streamed = stream_changes(&index, &root, &sample, per_batch);
    let several_first = time_queries(&index, &queries);
    let several_warm = time_queries(&index, &queries);
    // The same queries while changes go on arriving and segments are merged
    // in the background (#187), the merges let go.
    index.hold_merges(false);
    let arriving = while_changes_arrive(&index, &root, &sample, per_batch, &queries);
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
        "| Query, 95th percentile, one segment (first pass after opening) | {} | under 10 ms | not measured |",
        millis(percentile(&one_first, 95.0))
    );
    println!(
        "| Query, 95th percentile, one segment (warm) | {} | under 10 ms | not measured |",
        spread(&one_warm)
    );
    println!(
        "| Query, 95th percentile, several segments and changes in memory (first pass after the stream) | {} | under 10 ms | not measured |",
        millis(percentile(&several_first, 95.0))
    );
    println!(
        "| Query, 95th percentile, several segments and changes in memory (warm) | {} | under 10 ms | not measured |",
        spread(&several_warm)
    );
    println!(
        "| Query, 95th percentile, while changes arrive and segments merge | {} | under 10 ms | not measured |",
        spread(&arriving.times)
    );
    println!(
        "| A changed file re-indexed near the root, 95th percentile | {} (p50 {}) | under 10 ms | 4–10 ms |",
        millis(percentile(&near_root, 95.0)),
        millis(percentile(&near_root, 50.0)),
    );
    match (&deep, &deep_times, &deep_cold) {
        (Some(deep), Some(times), Some(cold)) => {
            println!(
                "| A changed file re-indexed in a deep folder under ignore files, 95th percentile | {} (p50 {}; {} folders down, {} of them or the root with ignore files) | under 10 ms | 4–10 ms |",
                millis(percentile(times, 95.0)),
                millis(percentile(times, 50.0)),
                deep.depth,
                deep.with_ignore_files,
            );
            println!(
                "| A changed file re-indexed in a deep folder under ignore files, nothing kept (every folder above read again), 95th percentile | {} (p50 {}) | under 10 ms | 4–10 ms |",
                millis(percentile(cold, 95.0)),
                millis(percentile(cold, 50.0)),
            );
        }
        _ => println!(
            "| A changed file re-indexed in a deep folder under ignore files, 95th percentile | not measured: the walk found no folder | under 10 ms | |"
        ),
    }
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
        "Queries: {} of {} kinds (a typed-out name counts each keystroke), limit 20, the same set on both shapes of the index; every query above counts in the 95th percentile. A miss finds nothing; a kind filter runs as Search Files runs it (Folder in the index's own query, Document by asking for more and keeping documents).",
        queries.len(),
        QueryKind::ALL.len()
    );
    println!(
        "Several segments and changes in memory: {} segments and {} changes in memory, left by a stream of {} changes in {STREAM_BATCHES} batches of about {per_batch}, the background merges held off while they were timed.",
        streamed.segments, streamed.in_memory, streamed.changes,
    );
    println!(
        "Re-indexed: each row's 100 changes are applied as a batch is, the file's own folder read again and the folders above it kept from the change before (#186); the row with nothing kept reads every folder above again each time, as the first change after a start does."
    );
    println!(
        "While changes arrive: {} changes applied while the queries ran, in batches of about {} each written as a segment, a pause of 100 ms after every {STREAM_BATCHES}; {} segments at the end.",
        arriving.changes, arriving.per_batch, arriving.segments,
    );
    println!();
    println!(
        "| Query kind (warm) | Queries | One segment: p50 | p95 | p99 | Several segments and changes in memory: p50 | p95 | p99 |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- |");
    let row = |label: &str, one: &[Duration], several: &[Duration]| {
        println!(
            "| {label} | {} | {} | {} | {} | {} | {} | {} |",
            one.len(),
            millis(percentile(one, 50.0)),
            millis(percentile(one, 95.0)),
            millis(percentile(one, 99.0)),
            millis(percentile(several, 50.0)),
            millis(percentile(several, 95.0)),
            millis(percentile(several, 99.0)),
        );
    };
    for kind in QueryKind::ALL {
        let one = of_kind(&queries, &one_warm, kind);
        if !one.is_empty() {
            let several = of_kind(&queries, &several_warm, kind);
            row(kind.label(), one.as_slice(), several.as_slice());
        }
    }
    row("all kinds", one_warm.as_slice(), several_warm.as_slice());

    if options.guard {
        let mut over: Vec<String> = Vec::new();
        let mut checked = 0;
        let mut check = |what: String, value: Duration, ceiling: Duration| {
            checked += 1;
            if value > ceiling {
                over.push(format!(
                    "{what}: {}, over its ceiling of {}",
                    millis(value),
                    millis(ceiling)
                ));
            }
        };
        check("first index".into(), first.total, GUARD_FIRST_INDEX);
        for (shape, times) in [
            ("one segment", &one_warm),
            ("several segments and changes in memory", &several_warm),
        ] {
            let p95 = percentile(times, 95.0);
            check(
                format!("query p95, {shape}, all kinds"),
                p95,
                GUARD_QUERY_P95,
            );
            for kind in QueryKind::ALL {
                let of = of_kind(&queries, times, kind);
                if !of.is_empty() {
                    let what = format!("query p95, {shape}, {}", kind.label());
                    check(what, percentile(&of, 95.0), GUARD_QUERY_P95);
                }
            }
        }
        let p95 = percentile(&near_root, 95.0);
        check("re-index p95, near the root".into(), p95, GUARD_REINDEX_P95);
        if let Some(times) = &deep_times {
            let p95 = percentile(times, 95.0);
            check("re-index p95, deep".into(), p95, GUARD_REINDEX_P95);
        }
        if let Some(times) = &deep_cold {
            let p95 = percentile(times, 95.0);
            check(
                "re-index p95, deep, nothing kept".into(),
                p95,
                GUARD_REINDEX_P95,
            );
        }
        let per_entry = first.bytes as f64 / entries as f64;
        checked += 1;
        if per_entry > GUARD_BYTES_PER_ENTRY {
            over.push(format!(
                "index on disk: {per_entry:.0} B per entry, over its ceiling of \
                 {GUARD_BYTES_PER_ENTRY:.0} B"
            ));
        }
        println!();
        if over.is_empty() {
            println!("Regression guard (#183): {checked} measures, each within its ceiling.");
        } else {
            println!(
                "Regression guard (#183): {} of {checked} measures over their ceilings:",
                over.len()
            );
            for line in &over {
                println!("- {line}");
            }
            std::process::exit(1);
        }
    }
}

/// The 95th percentile of `times`, with the 50th, the 99th and the most.
fn spread(times: &[Duration]) -> String {
    format!(
        "{} (p50 {}, p99 {}, max {})",
        millis(percentile(times, 95.0)),
        millis(percentile(times, 50.0)),
        millis(percentile(times, 99.0)),
        millis(percentile(times, 100.0)),
    )
}

/// The times of the queries of `kind`.
fn of_kind(queries: &[TimedQuery], times: &[Duration], kind: QueryKind) -> Vec<Duration> {
    queries
        .iter()
        .zip(times)
        .filter(|(query, _)| query.kind == kind)
        .map(|(_, time)| *time)
        .collect()
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
                // Named for the queries: the first entry of each batch, and
                // every 400th after it. The walk hands over one folder's
                // entries at a time, its own entry first, so this is each
                // folder walked, and a file of a folder holding more than
                // 400 entries now and then.
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

/// The kinds of query timed, each reported on a row of its own. Their
/// labels name the rows, so that runs compare: a label is not changed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum QueryKind {
    WholeName,
    NamePrefix,
    Word,
    FolderAndName,
    OneLetter,
    /// A word no entry holds, of 3 to 8 letters: the pass inside words
    /// runs too (before #185 it read every term).
    Miss,
    /// A word of a name, then a word no entry holds.
    MissAfterWord,
    /// Each prefix of a name, from its first letter to the whole name, one
    /// after the other, as a user's keystrokes are.
    TypedOut,
    /// A word of a folder's name, only folders asked for.
    FolderFilter,
    /// A word of a name, only documents asked for (Search Files' type).
    DocumentFilter,
    /// Three or four letters from inside a word of a name, never its start.
    InsideWord,
}

impl QueryKind {
    const ALL: [QueryKind; 11] = [
        QueryKind::WholeName,
        QueryKind::NamePrefix,
        QueryKind::Word,
        QueryKind::FolderAndName,
        QueryKind::OneLetter,
        QueryKind::Miss,
        QueryKind::MissAfterWord,
        QueryKind::TypedOut,
        QueryKind::FolderFilter,
        QueryKind::DocumentFilter,
        QueryKind::InsideWord,
    ];

    fn label(self) -> &'static str {
        match self {
            QueryKind::WholeName => "whole name",
            QueryKind::NamePrefix => "name prefix (3 letters)",
            QueryKind::Word => "a word",
            QueryKind::FolderAndName => "folder and name words",
            QueryKind::OneLetter => "one letter",
            QueryKind::Miss => "miss: an absent word",
            QueryKind::MissAfterWord => "miss: a word, then an absent one",
            QueryKind::TypedOut => "typed out, each keystroke",
            QueryKind::FolderFilter => "kind filter: Folder",
            QueryKind::DocumentFilter => "kind filter: Document",
            QueryKind::InsideWord => "inside a word",
        }
    }
}

#[derive(Clone)]
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

/// A name the walk found, with its words and its folder's.
struct Named {
    name: String,
    words: Vec<String>,
    folder_words: Vec<String>,
}

/// A fixed set of about `count` queries, as many of each kind (a typed-out
/// name counts each keystroke), from the names the walk found (`sample`,
/// sorted) and a seeded generator, so that runs over the same tree time
/// the same queries. A miss is kept only when `index` finds nothing for
/// it. The kinds take turns, a typed-out name's keystrokes together.
fn query_set(index: &FileIndex, sample: &[PathBuf], count: usize) -> Vec<TimedQuery> {
    let names: Vec<Named> = sample
        .iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            let folder_words = path
                .parent()
                .and_then(Path::file_name)
                .map(|n| words_of(&n.to_string_lossy()))
                .unwrap_or_default();
            Some(Named {
                words: words_of(&name),
                name,
                folder_words,
            })
        })
        .filter(|named| !named.name.trim().is_empty())
        .collect();
    if names.is_empty() {
        return Vec::new();
    }
    let per_kind = (count / QueryKind::ALL.len()).max(1);
    let mut seeded = Seeded(183);
    // Per kind, its queries in units: one query, or a typed-out name's
    // keystrokes.
    let mut units: Vec<Vec<Vec<TimedQuery>>> = Vec::new();
    for (salt, kind) in QueryKind::ALL.into_iter().enumerate() {
        let mut kind_units: Vec<Vec<TimedQuery>> = Vec::new();
        let mut made = 0;
        let mut at = 0usize;
        while made < per_kind && at < per_kind * 20 {
            let named = &names[(at * 7 + salt * 131) % names.len()];
            at += 1;
            let unit: Vec<TimedQuery> = texts(kind, named, at, &mut seeded)
                .into_iter()
                .map(|text| TimedQuery { kind, text })
                .collect();
            let Some(first) = unit.first() else {
                continue;
            };
            if matches!(kind, QueryKind::Miss | QueryKind::MissAfterWord)
                && run_query(index, first) > 0
            {
                continue;
            }
            made += unit.len();
            kind_units.push(unit);
        }
        units.push(kind_units);
    }
    let rounds = units.iter().map(Vec::len).max().unwrap_or(0);
    let mut queries = Vec::new();
    for round in 0..rounds {
        for kind_units in &units {
            if let Some(unit) = kind_units.get(round) {
                queries.extend(unit.iter().cloned());
            }
        }
    }
    queries
}

/// The texts of one unit of `kind` from `named` (none when the name has
/// nothing of that kind); `at` varies the word taken.
fn texts(kind: QueryKind, named: &Named, at: usize, seeded: &mut Seeded) -> Vec<String> {
    let first_word = named.words.first();
    let pick = |words: &[String]| words.get(at % words.len().max(1)).cloned();
    let text = match kind {
        QueryKind::WholeName => Some(named.name.clone()),
        QueryKind::NamePrefix => Some(named.name.chars().take(3).collect()),
        QueryKind::Word => pick(named.words.as_slice()),
        QueryKind::FolderAndName => match (named.folder_words.first(), first_word) {
            (Some(folder), Some(word)) => Some(format!("{folder} {word}")),
            _ => None,
        },
        QueryKind::OneLetter => Some(named.name.chars().take(1).collect()),
        QueryKind::Miss => Some(absent_word(seeded)),
        QueryKind::MissAfterWord => {
            first_word.map(|word| format!("{word} {}", absent_word(seeded)))
        }
        QueryKind::TypedOut => {
            let letters: Vec<char> = named.name.chars().collect();
            return (1..=letters.len())
                .map(|typed| letters[..typed].iter().collect::<String>())
                .filter(|text| !text.trim().is_empty())
                .collect();
        }
        QueryKind::FolderFilter => pick(named.folder_words.as_slice()),
        QueryKind::DocumentFilter => first_word.cloned(),
        QueryKind::InsideWord => inside_fragment(&named.words, at),
    };
    text.filter(|text| !text.trim().is_empty())
        .into_iter()
        .collect()
}

/// Three or four letters from inside the first word of five letters or
/// more of `words`, never its start, so that only the pass inside words
/// finds it there; `at` varies which.
fn inside_fragment(words: &[String], at: usize) -> Option<String> {
    let letters: Vec<char> = words
        .iter()
        .map(|word| word.chars().collect::<Vec<char>>())
        .find(|letters| letters.len() >= 5)?;
    let len = 3 + at % 2;
    let start = 1 + at % (letters.len() - len);
    Some(letters[start..start + len].iter().collect())
}

/// A seeded generator (SplitMix64), so that the misses are the same in
/// every run.
struct Seeded(u64);

impl Seeded {
    fn draw(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.draw() % bound as u64) as usize
    }
}

/// A word no name is likely to hold, of 3 to 8 letters: a `q`, a letter
/// that seldom follows one, then consonants.
fn absent_word(seeded: &mut Seeded) -> String {
    const AFTER_Q: &[u8] = b"xzjkvw";
    const CONSONANTS: &[u8] = b"bcdfghjklmnpqrstvwxz";
    let len = 3 + seeded.below(6);
    let mut word = String::with_capacity(len);
    word.push('q');
    word.push(char::from(AFTER_Q[seeded.below(AFTER_Q.len())]));
    while word.len() < len {
        word.push(char::from(CONSONANTS[seeded.below(CONSONANTS.len())]));
    }
    word
}

/// Runs `query` as Search Files does (`Indexer::search` in
/// `file_index/indexer.rs`, without the ids it gives), for a page of 20:
/// the Folder filter is the index's own; the Document filter asks the
/// index for at least 500 and keeps the documents, asking again for four
/// times as many while fewer than a page are kept, up to 20,000. Answers
/// how many entries it found.
fn run_query(index: &FileIndex, query: &TimedQuery) -> usize {
    const PAGE: usize = 20;
    if query.kind == QueryKind::DocumentFilter {
        // The indexer asks for the page or 500, whichever is more.
        let mut asked = 500;
        loop {
            let page = index.search(&Query {
                text: &query.text,
                limit: asked,
                offset: 0,
                kind: None,
            });
            let exhausted = page.len() < asked;
            let kept = page
                .iter()
                .filter(|hit| Category::Documents.holds(&hit.path, hit.meta.kind))
                .count();
            if kept >= PAGE || exhausted || asked >= 20_000 {
                return kept.min(PAGE);
            }
            asked *= 4;
        }
    }
    index
        .search(&Query {
            text: &query.text,
            limit: PAGE,
            offset: 0,
            kind: (query.kind == QueryKind::FolderFilter).then_some(EntryKind::Folder),
        })
        .len()
}

fn time_queries(index: &FileIndex, queries: &[TimedQuery]) -> Vec<Duration> {
    queries
        .iter()
        .map(|query| {
            let started = Instant::now();
            let found = run_query(index, query);
            std::hint::black_box(found);
            started.elapsed()
        })
        .collect()
}

/// A changed file re-indexed 100 times, as the indexer takes a change
/// (`Indexer::look_at` in `file_index/indexer.rs`): the file written, then,
/// timed, what the index holds at its path, the file read, the rules
/// applied to it as each batch of changes applies them (#186: the global
/// ignore file looked at, what the scope keeps of the file's folder
/// dropped, as when Windows reports the folder changed too, so that its
/// ignore files are read again, the folders above it kept from the run
/// before), the change applied and a query finding it. The first change
/// reads every folder above the file; with `nothing_kept`, every change
/// does, as the first after a start does. The file is written in
/// `changes`; with `indexed_in`, it is indexed as if it were in that
/// folder, which is not written to, and taken out again.
fn reindex(
    index: &FileIndex,
    scope: &Scope,
    changes: &Path,
    name: &str,
    indexed_in: Option<&Path>,
    nothing_kept: bool,
) -> Vec<Duration> {
    scope.forget_all_kept();
    let mut times = Vec::new();
    for n in 0..100 {
        let text = format!("changed {name} {n} zqx");
        let file = changes.join(format!("{text}.txt"));
        std::fs::write(&file, b"changed").expect("a changed file");
        if nothing_kept {
            scope.forget_all_kept();
        }
        let started = Instant::now();
        let path = match indexed_in {
            Some(folder) => folder.join(file.file_name().expect("the file's name")),
            None => file.clone(),
        };
        let indexed = index.get(&path);
        let mut entry = Entry::read(&file).expect("the changed file");
        entry.path = path.clone();
        let is_dir = entry.meta.kind == EntryKind::Folder;
        std::hint::black_box(scope.global_ignore_changed());
        scope.forget_kept(&path);
        if let Some(folder) = path.parent() {
            scope.forget_kept(folder);
        }
        let admitted = scope.admits_kept(&path, is_dir);
        if admitted {
            index.apply(&[Change::Put(entry)]).expect("applied");
        }
        let found = index
            .search(&Query {
                text: &text,
                limit: 5,
                offset: 0,
                kind: None,
            })
            .iter()
            .any(|hit| hit.path == path);
        times.push(started.elapsed());
        std::hint::black_box(indexed);
        assert!(admitted, "the rules admit {}", path.display());
        assert!(found, "the changed file is found");
        if indexed_in.is_some() {
            index.apply(&[Change::Remove(path)]).expect("removed again");
            let _ = std::fs::remove_file(&file);
        }
    }
    times
}

/// The folder the deep re-index row indexes its file in.
struct DeepFolder {
    path: PathBuf,
    /// Folders below the root, down to this one.
    depth: usize,
    /// Of those folders and the root, how many hold an ignore file
    /// (`.gitignore` or `.ignore`).
    with_ignore_files: usize,
}

/// The deepest folder of the sampled entries with ignore files on the way
/// down from the root, or the deepest when none has any, among the 1,000
/// deepest, in a fixed order; one whose rules admit the changed file.
fn deep_folder(root: &Path, scope: &Scope, sample: &[PathBuf]) -> Option<DeepFolder> {
    let mut folders: Vec<(usize, &Path)> = sample
        .iter()
        .filter_map(|path| path.parent())
        .filter_map(|folder| {
            let depth = folder.strip_prefix(root).ok()?.components().count();
            Some((depth, folder))
        })
        .collect();
    folders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    folders.dedup_by(|a, b| a.1 == b.1);
    let mut holds: HashMap<PathBuf, bool> = HashMap::new();
    let mut best: Option<DeepFolder> = None;
    for &(depth, folder) in folders.iter().take(1_000) {
        let mut with_ignore_files = 0;
        for above in folder.ancestors() {
            if !above.starts_with(root) {
                break;
            }
            let has = *holds.entry(above.to_path_buf()).or_insert_with(|| {
                [".gitignore", ".ignore"]
                    .iter()
                    .any(|file| above.join(file).symlink_metadata().is_ok())
            });
            if has {
                with_ignore_files += 1;
            }
        }
        let probe = folder.join("changed deep report 0 zqx.txt");
        if !scope.admits(&probe, false, &mut Admitted::default()) {
            continue;
        }
        let rank = (with_ignore_files > 0, depth, with_ignore_files);
        let better = best.as_ref().is_none_or(|best| {
            rank > (
                best.with_ignore_files > 0,
                best.depth,
                best.with_ignore_files,
            )
        });
        if better {
            best = Some(DeepFolder {
                path: folder.to_path_buf(),
                depth,
                with_ignore_files,
            });
        }
    }
    best
}

/// The queries timed while changes arrived, and what arrived.
struct Arriving {
    times: Vec<Duration>,
    /// Changes applied while the queries ran.
    changes: usize,
    per_batch: usize,
    /// Segments when the queries were done.
    segments: usize,
}

/// `queries` timed while another thread applies changes, as the indexer's
/// coordinator does (#187): streams of [`STREAM_BATCHES`] batches of about
/// a tenth of `per_batch` changes, each but the last written as a segment
/// of its own, 100 ms apart, so that the index merges segments in the
/// background while the queries run. A query never waits for a merge,
/// only for a batch being put in memory.
fn while_changes_arrive(
    index: &FileIndex,
    root: &Path,
    sample: &[PathBuf],
    per_batch: usize,
    queries: &[TimedQuery],
) -> Arriving {
    let per_batch = (per_batch / 10).max(50);
    let done = AtomicBool::new(false);
    let (times, changes) = std::thread::scope(|threads| {
        let stream = threads.spawn(|| {
            let mut changes = 0;
            while !done.load(Ordering::Relaxed) {
                changes += stream_changes(index, root, sample, per_batch).changes;
                std::thread::sleep(Duration::from_millis(100));
            }
            changes
        });
        let times = time_queries(index, queries);
        done.store(true, Ordering::Relaxed);
        (times, stream.join().unwrap_or(0))
    });
    Arriving {
        times,
        changes,
        per_batch,
        segments: index.stats().segments,
    }
}

/// What the stream of changes left.
struct Streamed {
    segments: usize,
    /// Changes applied in all.
    changes: usize,
    /// Changes in the memory table, not yet in a segment.
    in_memory: usize,
}

/// A stream of changes over the index, as days of use bring them:
/// [`STREAM_BATCHES`] batches of about `per_batch` changes, each
/// written as a segment of its own but the last, which stays in memory.
/// Each batch adds files beside entries the walk found (six tenths),
/// changes found entries, a newer version over the segment's (three
/// tenths), deletes files the batch before added (a tenth), and adds a
/// folder with files, deleting the folder the batch before added (a
/// tombstone). Only the index changes, never the disk.
fn stream_changes(
    index: &FileIndex,
    root: &Path,
    sample: &[PathBuf],
    per_batch: usize,
) -> Streamed {
    let folders: Vec<&Path> = sample
        .iter()
        .filter_map(|path| path.parent())
        .filter(|folder| folder.starts_with(root))
        .collect();
    let mut streamed = Streamed {
        segments: index.stats().segments,
        changes: 0,
        in_memory: 0,
    };
    if folders.is_empty() {
        return streamed;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let file = |size: usize| Meta {
        kind: EntryKind::File,
        size: size as u64,
        modified: now,
        file_id: 0,
        volume: 0,
    };
    let mut added: Vec<PathBuf> = Vec::new();
    let mut added_folder: Option<PathBuf> = None;
    for batch in 0..STREAM_BATCHES {
        let mut changes = Vec::with_capacity(per_batch + 16);
        if let Some(folder) = added_folder.take() {
            changes.push(Change::RemoveUnder(folder));
        }
        for path in added.drain(..).step_by(6).take(per_batch / 10) {
            changes.push(Change::Remove(path));
        }
        for k in 0..per_batch * 3 / 10 {
            let path = &sample[((batch * per_batch + k) * 13) % sample.len()];
            if let Some(meta) = index.get(path) {
                changes.push(Change::Put(Entry {
                    path: path.clone(),
                    meta: Meta {
                        size: meta.size + 1,
                        modified: now,
                        ..meta
                    },
                }));
            }
        }
        for k in 0..per_batch * 6 / 10 {
            let n = batch * per_batch + k;
            let name = format!(
                "{} {} stream_{n}{}",
                WORDS[n % WORDS.len()],
                WORDS[(n / 7) % WORDS.len()],
                EXTENSIONS[n % EXTENSIONS.len()]
            );
            let path = folders[(n * 7) % folders.len()].join(name);
            changes.push(Change::Put(Entry {
                path: path.clone(),
                meta: file(n),
            }));
            added.push(path);
        }
        let parent = folders[(batch * 7919) % folders.len()];
        let folder = parent.join(format!("stream folder {batch}"));
        changes.push(Change::Put(Entry {
            path: folder.clone(),
            meta: Meta {
                kind: EntryKind::Folder,
                size: 0,
                ..file(0)
            },
        }));
        for k in 0..10 {
            changes.push(Change::Put(Entry {
                path: folder.join(format!("stream notes {batch}-{k}.txt")),
                meta: file(k),
            }));
        }
        added_folder = Some(folder);
        index.apply(&changes).expect("the stream applied");
        streamed.changes += changes.len();
        streamed.in_memory = changes.len();
        if batch + 1 < STREAM_BATCHES {
            index.flush().expect("the batch written as a segment");
        }
    }
    streamed.segments = index.stats().segments;
    streamed
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
    // A catch-up runs at start, with a scope that keeps nothing yet (#186).
    scope.forget_all_kept();
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
