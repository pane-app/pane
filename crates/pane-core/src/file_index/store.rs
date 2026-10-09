//! The name index itself, in the shape of `minidex` (#126, "The engine"):
//! a memory table of recent changes backed by a write-ahead log, immutable
//! segments on disk each with its own term dictionary (and, like the memory
//! table, the terms having each fragment of their words, for the pass
//! inside words) and key filter, tombstones that hide a deleted folder's
//! entries in older segments, and merges of a few segments of similar size
//! at a time, in the background (#187). Its files are Pane's own, versioned
//! by [`FORMAT_VERSION`]: an index of another version is rebuilt, never
//! read.
//!
//! One writer at a time changes the index (its writer lock); queries read
//! it at the same time, never waiting for a segment to be written or
//! merged, only for the moment its result replaces what was there. Merges
//! run on a thread of the index's own ([`Merger`]), at background priority,
//! so that applying changes never waits for one either.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::{self, File};
use std::io;
use std::ops::{Bound, ControlFlow, Range};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, RwLock, RwLockReadGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::Entry;
use super::format::{EntryKind, FORMAT_VERSION, Meta, SEPARATOR, key_path, path_key};
use super::journal::JournalCursor;
use super::segment::{self, Segment, Stored, Tombstone};
use super::terms::{
    FOLDER_TAG, FRAGMENT, NAME_TAG, Prepared, Roots, fragments, hint_day, hint_depth, hint_kind,
    terms_and_hint,
};
use super::text;
use super::wal::{self, Op, Wal};
use crate::atomic::{Readers, write_atomically};
use crate::util::lock;

/// Entries the memory table holds before they are written as a segment.
const MEMORY_TABLE_ENTRIES: usize = 1 << 16;
/// Entries a first index writes per segment, before merging them.
const BULK_SEGMENT_ENTRIES: usize = 1 << 17;
/// Neighbouring segments a merge takes at a time (#187): a few, so that no
/// change waits for the whole index to be written again.
const MERGE_FACTOR: usize = 4;
/// A segment smaller than this counts as this many entries when sizes are
/// compared: the small segments that flushes and starts leave are alike.
const MERGE_FLOOR: u64 = 1 << 12;
/// Segments beyond which the smallest run of [`MERGE_FACTOR`] neighbours is
/// merged even if their sizes differ, so that a query reads at most about
/// this many.
const MAX_SEGMENTS: usize = 8;
/// Tombstones the segments above the oldest may carry, at least, before
/// every segment is merged into one ([`oldest_run_due`]): only a merge from
/// the oldest segment drops them, and the tiers alone seldom reach a first
/// index's large segment.
const TOMBSTONES_DUE: u64 = 4_096;
/// Candidates a query reads and scores per segment, at least.
const CANDIDATES: usize = 1_000;
/// For a word of one or two characters, entries gathered before the words
/// it starts stop being expanded.
const SHORT_WORD_ENTRIES: usize = 50_000;

const MANIFEST: &str = "index.json";
const LOCK: &str = "lock";

/// Why the index could not be opened or changed.
#[derive(Debug)]
pub enum IndexError {
    /// Another Pane holds the index's folder.
    InUse,
    Io(io::Error),
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IndexError::InUse => write!(f, "another Pane is using file search"),
            IndexError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<io::Error> for IndexError {
    fn from(error: io::Error) -> IndexError {
        IndexError::Io(error)
    }
}

/// What opening found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opened {
    /// No index was there: it is empty.
    Fresh,
    /// The index of an earlier run, with its record.
    Existing,
    /// An index that could not be read (another format version, a damaged
    /// file): it was deleted and the index is empty. Says why.
    Rebuilt(String),
}

/// What Pane records with the index, beside its entries: whether a first
/// walk finished, and where each volume's change records were read up to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRecord {
    pub built: bool,
    #[serde(default)]
    pub cursors: Vec<JournalCursor>,
    /// The roots and rules the entries were indexed under (#175): when
    /// Pane next opens the index under other rules (the user changed them,
    /// even while file search was off), it brings the entries to the new
    /// ones first.
    #[serde(default)]
    pub rules: Option<super::scope::ScopeRules>,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: u32,
    segments: Vec<u64>,
    next_file: u64,
    record: IndexRecord,
}

/// A change to apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// The entry at a path, added or changed.
    Put(Entry),
    /// The entry at a path is gone.
    Remove(PathBuf),
    /// The folder at a path is gone with everything under it (deleted, or
    /// renamed: its new name is walked again).
    RemoveUnder(PathBuf),
}

/// Entries prepared for the index on the walker's threads, so that the
/// writer only stores them.
pub struct PreparedBatch(Vec<Prepared>);

impl PreparedBatch {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A query.
#[derive(Clone, Debug)]
pub struct Query<'a> {
    pub text: &'a str,
    pub limit: usize,
    pub offset: usize,
    /// Only entries of this kind.
    pub kind: Option<EntryKind>,
}

/// An entry a query found.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub path: PathBuf,
    pub meta: Meta,
    /// Higher is better; 0 for a list of recent entries.
    pub score: f64,
}

/// Counts for the benchmark and the File search page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexStats {
    pub segments: usize,
    /// Versions stored: entries, superseded versions and deletions not yet
    /// merged away. After [`FileIndex::compact`] it is the entries.
    pub stored: u64,
    pub bytes_on_disk: u64,
}

/// An entry of the memory table.
struct MemEntry {
    key: Vec<u8>,
    seq: u64,
    meta: Option<Meta>,
    hint: u32,
    /// False once a newer version of its key is in the table.
    newest: bool,
}

/// The recent changes, in memory: entries in the order they came, the
/// newest version of each key, each term's entries, and, as a segment has,
/// the terms having each fragment of their words.
#[derive(Default)]
struct MemTable {
    entries: Vec<MemEntry>,
    by_key: BTreeMap<Vec<u8>, u32>,
    terms: BTreeMap<Vec<u8>, Vec<u32>>,
    /// Each term, by the number it was given when it first came.
    term_list: Vec<Vec<u8>>,
    /// Each fragment (`terms::fragments`) and the numbers of the terms
    /// having it, ascending.
    fragments: HashMap<u32, Vec<u32>>,
}

impl MemTable {
    fn insert(&mut self, seq: u64, prepared: Prepared) {
        let id = self.entries.len() as u32;
        if let Some(older) = self.by_key.insert(prepared.key.clone(), id) {
            self.entries[older as usize].newest = false;
        }
        for term in prepared.terms {
            if let Some(ids) = self.terms.get_mut(&term) {
                ids.push(id);
                continue;
            }
            let number = self.term_list.len() as u32;
            for fragment in fragments(&term) {
                self.fragments.entry(fragment).or_default().push(number);
            }
            self.term_list.push(term.clone());
            self.terms.insert(term, vec![id]);
        }
        self.entries.push(MemEntry {
            key: prepared.key,
            seq,
            meta: prepared.meta,
            hint: prepared.hint,
            newest: true,
        });
    }

    fn get(&self, key: &[u8]) -> Option<&MemEntry> {
        self.by_key.get(key).map(|&id| &self.entries[id as usize])
    }

    /// The numbers of the terms having every fragment of `wanted`,
    /// ascending.
    fn terms_with_fragments(&self, wanted: &[u32]) -> Vec<u32> {
        let mut lists: Vec<&[u32]> = Vec::with_capacity(wanted.len());
        for fragment in wanted {
            match self.fragments.get(fragment) {
                Some(list) => lists.push(list),
                None => return Vec::new(),
            }
        }
        lists.sort_unstable_by_key(|list| list.len());
        let Some((first, rest)) = lists.split_first() else {
            return Vec::new();
        };
        let mut found = first.to_vec();
        for list in rest {
            if found.is_empty() {
                break;
            }
            found = intersect(&found, list);
        }
        found
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Where a query looks: the memory tables and segments, newest first.
enum Source<'a> {
    Memory(&'a MemTable),
    Segment(&'a Segment),
}

impl Source<'_> {
    /// Appends the ids of entries with a term starting with `tag` and
    /// `word` to `all`, and of those whose term is exactly that to `exact`.
    fn word(&self, tag: u8, word: &str, all: &mut Vec<u32>, exact: &mut Vec<u32>) {
        let mut term = Vec::with_capacity(word.len() + 1);
        term.push(tag);
        term.extend_from_slice(word.as_bytes());
        let short = word.chars().count() < 3;
        match self {
            Source::Memory(table) => {
                let mut upper = term.clone();
                upper.push(0xFF);
                let range = table.terms.range::<[u8], _>((
                    Bound::Included(term.as_slice()),
                    Bound::Excluded(upper.as_slice()),
                ));
                for (found, ids) in range {
                    all.extend_from_slice(ids);
                    if *found == term {
                        exact.extend_from_slice(ids);
                    }
                    if short && all.len() >= SHORT_WORD_ENTRIES {
                        break;
                    }
                }
            }
            Source::Segment(segment) => {
                segment.terms_with_prefix(&term, |found, offset| {
                    let start = all.len();
                    segment.postings_into(offset, all);
                    if found == term.as_slice() {
                        exact.extend_from_slice(&all[start..]);
                    }
                    if short && all.len() >= SHORT_WORD_ENTRIES {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                });
            }
        }
    }

    /// Appends the ids of entries with a term of `tag` holding `word`
    /// anywhere (at its start too) to `all`, term by term in the terms'
    /// order: this is asked only when the words' starts find too few
    /// entries ([`FileIndex::search`]). Stops past [`SHORT_WORD_ENTRIES`]
    /// entries. Only the terms having every fragment of the word are looked
    /// at, through the fragments' lists; what it appends to an empty `all`
    /// is exactly what [`Source::word_inside_scan`] does, reading every
    /// term.
    fn word_inside(&self, tag: u8, word: &str, all: &mut Vec<u32>) {
        let needle = word.as_bytes();
        if needle.len() < FRAGMENT {
            self.word_inside_scan(tag, word, all);
            return;
        }
        let mut term = Vec::with_capacity(needle.len() + 1);
        term.push(tag);
        term.extend_from_slice(needle);
        let wanted = fragments(&term);
        match self {
            Source::Memory(table) => {
                let mut found: Vec<&[u8]> = table
                    .terms_with_fragments(&wanted)
                    .into_iter()
                    .map(|number| table.term_list[number as usize].as_slice())
                    .filter(|found| holds(found, needle))
                    .collect();
                // In the terms' order, as the scan reads them.
                found.sort_unstable();
                for found in found {
                    if let Some(ids) = table.terms.get(found) {
                        all.extend_from_slice(ids);
                        if all.len() >= SHORT_WORD_ENTRIES {
                            break;
                        }
                    }
                }
            }
            Source::Segment(segment) => {
                // Ordinals ascend in the dictionary's order.
                for ordinal in segment.terms_with_fragments(&wanted) {
                    let Some((found, offset)) = segment.term(ordinal) else {
                        continue;
                    };
                    if holds(found, needle) {
                        segment.postings_into(offset, all);
                        if all.len() >= SHORT_WORD_ENTRIES {
                            break;
                        }
                    }
                }
            }
        }
    }

    /// What [`Source::word_inside`] finds, by reading every term of `tag`:
    /// for a word shorter than a fragment, and for the tests to compare.
    fn word_inside_scan(&self, tag: u8, word: &str, all: &mut Vec<u32>) {
        let needle = word.as_bytes();
        match self {
            Source::Memory(table) => {
                let range = table.terms.range::<[u8], _>((
                    Bound::Included(&[tag][..]),
                    Bound::Excluded(&[tag + 1][..]),
                ));
                for (found, ids) in range {
                    if holds(found, needle) {
                        all.extend_from_slice(ids);
                        if all.len() >= SHORT_WORD_ENTRIES {
                            break;
                        }
                    }
                }
            }
            Source::Segment(segment) => {
                segment.terms_with_prefix(&[tag], |found, offset| {
                    if holds(found, needle) {
                        segment.postings_into(offset, all);
                    }
                    if all.len() >= SHORT_WORD_ENTRIES {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                });
            }
        }
    }

    /// The hint of entry `id`, unless it is a superseded version in a
    /// memory table.
    fn hint(&self, id: u32) -> Option<u32> {
        match self {
            Source::Memory(table) => {
                let entry = table.entries.get(id as usize)?;
                (entry.newest && entry.meta.is_some()).then_some(entry.hint)
            }
            Source::Segment(segment) => Some(segment.hint(id)),
        }
    }

    fn stored(&self, id: u32) -> Option<Stored> {
        match self {
            Source::Memory(table) => {
                let entry = table.entries.get(id as usize)?;
                Some(Stored {
                    key: entry.key.clone(),
                    seq: entry.seq,
                    meta: entry.meta,
                })
            }
            Source::Segment(segment) => segment.doc(id),
        }
    }

    fn len(&self) -> u32 {
        match self {
            Source::Memory(table) => table.entries.len() as u32,
            Source::Segment(segment) => segment.len(),
        }
    }

    /// The version of `key` this source holds.
    fn find(&self, key: &[u8]) -> Option<u64> {
        match self {
            Source::Memory(table) => table.get(key).map(|entry| entry.seq),
            Source::Segment(segment) => segment.find(key).map(|stored| stored.seq),
        }
    }
}

/// Whether the tagged term `term`'s word holds `needle` anywhere.
fn holds(term: &[u8], needle: &[u8]) -> bool {
    term.get(1..)
        .is_some_and(|rest| rest.windows(needle.len()).any(|part| part == needle))
}

/// Every tombstone not yet merged away, by prefix, so that checking a key
/// looks up the folders above it instead of reading every tombstone.
#[derive(Clone, Default)]
struct Tombstones {
    /// The newest tombstone's sequence number for each prefix ending with a
    /// separator, as every tombstone [`FileIndex::apply`] makes does.
    by_prefix: BTreeMap<Vec<u8>, u64>,
    /// The shortest and longest of those prefixes.
    shortest: usize,
    longest: usize,
    /// Tombstones of any other prefix, read one by one.
    others: Vec<Tombstone>,
    /// The newest tombstone's sequence number: no tombstone hides a later
    /// version.
    newest: u64,
}

impl Tombstones {
    fn push(&mut self, tombstone: Tombstone) {
        self.newest = self.newest.max(tombstone.seq);
        if tombstone.prefix.last() != Some(&SEPARATOR) {
            self.others.push(tombstone);
            return;
        }
        let len = tombstone.prefix.len();
        if self.by_prefix.is_empty() {
            (self.shortest, self.longest) = (len, len);
        } else {
            self.shortest = self.shortest.min(len);
            self.longest = self.longest.max(len);
        }
        let seq = self.by_prefix.entry(tombstone.prefix).or_insert(0);
        *seq = (*seq).max(tombstone.seq);
    }

    /// Whether a tombstone hides the version `seq` of `key`
    /// ([`Tombstone::covers`]).
    fn covers(&self, key: &[u8], seq: u64) -> bool {
        if seq >= self.newest {
            return false;
        }
        if !self.by_prefix.is_empty() {
            // Each folder above `key`: its prefix ends at a separator.
            let folders = key
                .iter()
                .enumerate()
                .take(self.longest)
                .skip(self.shortest - 1);
            for (at, &byte) in folders {
                if byte == SEPARATOR
                    && let Some(&newest) = self.by_prefix.get(&key[..=at])
                    && seq < newest
                {
                    return true;
                }
            }
        }
        self.others
            .iter()
            .any(|tombstone| tombstone.covers(key, seq))
    }

    fn is_empty(&self) -> bool {
        self.by_prefix.is_empty() && self.others.is_empty()
    }
}

impl FromIterator<Tombstone> for Tombstones {
    fn from_iter<I: IntoIterator<Item = Tombstone>>(tombstones: I) -> Tombstones {
        let mut all = Tombstones::default();
        for tombstone in tombstones {
            all.push(tombstone);
        }
        all
    }
}

struct State {
    memory: MemTable,
    /// The memory table being written as a segment, still searched.
    frozen: Option<Arc<MemTable>>,
    /// Oldest first; each newer segment's entries are newer than all of an
    /// older one's.
    segments: Vec<Arc<Segment>>,
    /// Every tombstone not yet merged away: those the segments carry and
    /// the unflushed ones.
    tombstones: Tombstones,
    /// The tombstones the next segment written carries, kept here until
    /// that segment is among `segments`.
    unflushed_tombstones: Vec<Tombstone>,
}

impl State {
    fn sources(&self) -> Vec<Source<'_>> {
        let mut sources = vec![Source::Memory(&self.memory)];
        if let Some(frozen) = &self.frozen {
            sources.push(Source::Memory(frozen));
        }
        sources.extend(
            self.segments
                .iter()
                .rev()
                .map(|segment| Source::Segment(segment)),
        );
        sources
    }

    /// Whether the version `seq` of `key`, found in `sources[at]`, is the
    /// current one: no tombstone covers it and no newer source holds a newer
    /// version (a segment whose key filter rules `key` out is not read).
    fn is_current(&self, sources: &[Source<'_>], at: usize, key: &[u8], seq: u64) -> bool {
        if self.tombstones.covers(key, seq) {
            return false;
        }
        sources[..at]
            .iter()
            .all(|newer| newer.find(key).is_none_or(|newer_seq| newer_seq <= seq))
    }
}

struct Writer {
    wal: Wal,
    next_seq: u64,
    next_file: u64,
    record: IndexRecord,
    /// Segment files replaced by a merge that could not be deleted yet.
    garbage: Vec<PathBuf>,
}

/// The file index: open on a folder of Pane's cache, which it holds locked.
pub struct FileIndex {
    dir: PathBuf,
    roots: Roots,
    _lock: File,
    /// Shared with the merger's thread, as are the writer and `merges`.
    state: Arc<RwLock<State>>,
    writer: Arc<Mutex<Writer>>,
    merges: Arc<Merges>,
    /// The merger's thread, stopped and waited for when the index closes,
    /// before its folder is unlocked.
    merger: Option<std::thread::JoinHandle<()>>,
    /// How often [`FileIndex::folder_ids`] read the table since the index
    /// opened, for the tests (#187).
    folder_id_reads: AtomicUsize,
}

impl Drop for FileIndex {
    /// Stops the merger, letting go of a merge under way, before the folder
    /// is unlocked: nothing writes there once the index is closed.
    fn drop(&mut self) {
        self.merges.close();
        if let Some(merger) = self.merger.take() {
            let _ = merger.join();
        }
    }
}

/// The merges in the background (#187): whenever segments are added, the
/// merger's thread looks for a run to merge ([`tier_to_merge`]) and merges
/// it, while changes go on being applied and queries answered.
#[derive(Default)]
struct Merges {
    /// Held while segments are merged (by the merger, or by a full
    /// compaction) and while a first walk writes its segments: one at a
    /// time. Always taken before the writer, never while holding it.
    merging: Mutex<()>,
    turns: Mutex<Turns>,
    changed: Condvar,
    /// The index is closing: a merge under way is let go.
    closing: AtomicBool,
}

/// Whether the merger has something to do.
#[derive(Default)]
struct Turns {
    /// Segments were added since the merger last looked.
    wanted: bool,
    /// The merger is looking for runs to merge, or merging one.
    running: bool,
    /// No merge starts while held ([`FileIndex::hold_merges`]).
    held: bool,
    /// Tests: a merge waits before its segment takes the run's place while
    /// this is set; `paused` says it waits.
    pause_before_swap: bool,
    #[cfg_attr(not(test), allow(dead_code))]
    paused: bool,
    /// Tests: the next merge panics.
    #[cfg(test)]
    panic_next: bool,
}

impl Merges {
    fn turns(&self) -> MutexGuard<'_, Turns> {
        lock(&self.turns)
    }

    fn wait<'a>(&self, turns: MutexGuard<'a, Turns>) -> MutexGuard<'a, Turns> {
        self.changed
            .wait(turns)
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    /// Asks the merger to look for segments to merge.
    fn want(&self) {
        self.turns().wanted = true;
        self.changed.notify_all();
    }

    /// Waits for the merger's next turn: `false` once the index closes.
    fn next_turn(&self) -> bool {
        let mut turns = self.turns();
        loop {
            if self.closing() {
                return false;
            }
            if turns.wanted && !turns.held {
                turns.wanted = false;
                turns.running = true;
                return true;
            }
            turns = self.wait(turns);
        }
    }

    /// Whether the merger's turn goes on to another merge.
    fn going_on(&self) -> bool {
        !self.closing() && !self.turns().held
    }

    fn turn_done(&self) {
        self.turns().running = false;
        self.changed.notify_all();
    }

    /// The index closes: the merger stops.
    fn close(&self) {
        let _turns = self.turns();
        self.closing.store(true, Ordering::SeqCst);
        self.changed.notify_all();
    }

    /// Waits until no merge is due or under way.
    fn wait_until_done(&self) {
        let mut turns = self.turns();
        while !self.closing() && ((turns.wanted && !turns.held) || turns.running) {
            turns = self.wait(turns);
        }
    }

    /// Waits while a test pauses merges before their segment takes the
    /// run's place; at once otherwise.
    fn before_swap(&self) {
        let mut turns = self.turns();
        if !turns.pause_before_swap {
            return;
        }
        turns.paused = true;
        self.changed.notify_all();
        while turns.pause_before_swap && !self.closing() {
            turns = self.wait(turns);
        }
        turns.paused = false;
        self.changed.notify_all();
    }
}

/// Ends the merger's turn, even if a merge panics, so that nothing waits
/// for it forever.
struct Turn<'a>(&'a Merges);

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.0.turn_done();
    }
}

/// What the merger's thread holds of the index.
struct Merger {
    dir: PathBuf,
    roots: Roots,
    state: Arc<RwLock<State>>,
    writer: Arc<Mutex<Writer>>,
    merges: Arc<Merges>,
}

impl Merger {
    /// The merger's thread, at background priority, as the walker's and the
    /// coordinator's: each time segments were added, merges what is due,
    /// until the index closes.
    fn run(self) {
        super::lower_current_thread();
        while self.merges.next_turn() {
            let _turn = Turn(&self.merges);
            while self.merges.going_on() {
                // A merge that panics is given up as one that fails is,
                // and said so: the merger goes on with its next turn, and
                // nothing waits for this one.
                let merged =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.merge_due()));
                match merged {
                    Ok(Ok(true)) => {}
                    // Nothing due, or a merge failed (it is tried again when
                    // segments are next added).
                    Ok(Ok(false) | Err(_)) => break,
                    Err(_) => {
                        crate::diagnostic!("pane: a merge of the file index's segments panicked");
                        break;
                    }
                }
            }
        }
    }

    /// Merges the next run of segments due, if one is: `true` when it did.
    /// The writer is held only to name the new segment and to put it in
    /// place, never while it is written, so that changes go on being
    /// applied meanwhile.
    fn merge_due(&self) -> Result<bool, IndexError> {
        #[cfg(test)]
        if std::mem::take(&mut self.merges.turns().panic_next) {
            panic!("a merge panics, as the test asks");
        }
        let _merging = lock(&self.merges.merging);
        let segments = read(&self.state).segments.clone();
        let sizes: Vec<u64> = segments
            .iter()
            .map(|segment| u64::from(segment.len()))
            .collect();
        // The tombstones only a merge from the oldest segment drops.
        let carried: u64 = segments
            .iter()
            .skip(1)
            .map(|segment| segment.tombstones.len() as u64)
            .sum();
        let Some(run) = tier_to_merge(&sizes).or_else(|| oldest_run_due(&sizes, carried)) else {
            return Ok(false);
        };
        let id = {
            let mut writer = lock(&self.writer);
            let id = writer.next_file;
            writer.next_file += 1;
            id
        };
        // The tombstones as of now: one applied later hides what it hides
        // in the merged segment as it did in the run.
        let tombstones = read(&self.state).tombstones.clone();
        let written = write_merged(
            &self.dir,
            &self.roots,
            &segments[run.clone()],
            &tombstones,
            run.start == 0,
            id,
            &self.merges.closing,
        )?;
        let Some(merged) = written else {
            return Ok(false);
        };
        self.merges.before_swap();
        let mut writer = lock(&self.writer);
        if !replace_run(&self.state, &segments[run.clone()], merged) {
            drop(writer);
            let _ = fs::remove_file(segment_path(&self.dir, id));
            return Ok(false);
        }
        write_manifest(&self.dir, &self.state, &writer)?;
        let old: Vec<PathBuf> = segments[run]
            .iter()
            .map(|segment| segment.path.clone())
            .collect();
        drop(segments);
        delete_replaced(old, &mut writer.garbage);
        Ok(true)
    }
}

/// The run of neighbouring segments to merge next, given each segment's
/// entries, oldest first: [`MERGE_FACTOR`] neighbours of similar size (the
/// largest at most [`MERGE_FACTOR`] times the smallest, each counted as at
/// least [`MERGE_FLOOR`]), the smallest such run; and with more than
/// [`MAX_SEGMENTS`] segments, the smallest run of neighbours whatever their
/// sizes. `None` when nothing is due. Neighbours only, so that the merged
/// segment takes their place and each segment's entries stay newer than
/// every older one's.
fn tier_to_merge(sizes: &[u64]) -> Option<Range<usize>> {
    if sizes.len() < MERGE_FACTOR {
        return None;
    }
    let mut similar: Option<(u64, usize)> = None;
    let mut smallest: Option<(u64, usize)> = None;
    for start in 0..=sizes.len() - MERGE_FACTOR {
        let run = &sizes[start..start + MERGE_FACTOR];
        let total: u64 = run.iter().sum();
        let counted = run.iter().map(|&size| size.max(MERGE_FLOOR));
        let least = counted.clone().min().unwrap_or(MERGE_FLOOR);
        let most = counted.max().unwrap_or(MERGE_FLOOR);
        if most <= least.saturating_mul(MERGE_FACTOR as u64)
            && similar.is_none_or(|(best, _)| total < best)
        {
            similar = Some((total, start));
        }
        if smallest.is_none_or(|(best, _)| total < best) {
            smallest = Some((total, start));
        }
    }
    if similar.is_none() && sizes.len() > MAX_SEGMENTS {
        // Too many segments: the smallest run, whatever the sizes.
        similar = smallest;
    }
    let (_, start) = similar?;
    Some(start..start + MERGE_FACTOR)
}

/// Every segment, oldest first, to merge into one when the tombstones the
/// segments above the oldest carry (`carried`) are more than
/// [`TOMBSTONES_DUE`] and more than a 32nd of the oldest segment's entries
/// (`sizes`, oldest first): a merge from the oldest segment is the only one
/// that drops them, and while they pile up each query checks against them
/// and they take room on the disk. `None` when they are fewer.
fn oldest_run_due(sizes: &[u64], carried: u64) -> Option<Range<usize>> {
    let oldest = *sizes.first()?;
    if sizes.len() >= 2 && carried > TOMBSTONES_DUE.max(oldest / 32) {
        Some(0..sizes.len())
    } else {
        None
    }
}

/// Writes `run`, neighbouring segments oldest first, merged into a new
/// segment `id` in `dir`: the newest version of each key, without what
/// `tombstones` hide. When `oldest` (the run starts at the oldest segment,
/// so that nothing older is left for them to hide) its deletions and the
/// tombstones it carries are dropped; otherwise the merged segment keeps
/// them for the segments older than it. A tombstone is so dropped only by
/// a merge covering every segment it could hide. `None` when the index
/// closed meanwhile (`closing`): the merge is let go at once, before what it
/// read is sorted and written to the disk, and its file deleted, so that
/// closing never waits for it.
fn write_merged(
    dir: &Path,
    roots: &Roots,
    run: &[Arc<Segment>],
    tombstones: &Tombstones,
    oldest: bool,
    id: u64,
    closing: &AtomicBool,
) -> Result<Option<Segment>, IndexError> {
    let carried: Vec<Tombstone> = if oldest {
        Vec::new()
    } else {
        run.iter()
            .flat_map(|segment| segment.tombstones.iter().cloned())
            .collect()
    };
    let path = segment_path(dir, id);
    let merged = Merge::new(run)
        .take_while(|_| !closing.load(Ordering::Relaxed))
        .filter(|stored| {
            !tombstones.covers(&stored.key, stored.seq) && !(oldest && stored.meta.is_none())
        })
        .map(|stored| {
            let (terms, hint) = terms_and_hint(roots, &stored.key, stored.meta.as_ref());
            (
                stored.seq,
                Prepared {
                    key: stored.key,
                    meta: stored.meta,
                    terms,
                    hint,
                },
            )
        });
    let written = segment::write_unless(&path, merged, &carried, closing);
    if closing.load(Ordering::Relaxed) {
        // Cut short: never put in place.
        let _ = fs::remove_file(&path);
        return Ok(None);
    }
    written?;
    Ok(Some(Segment::open(id, path)?))
}

/// Puts `merged` in the place of `run` among the segments and rebuilds the
/// tombstones from what the segments and the unflushed changes carry now;
/// `false`, changing nothing, if the run is no longer there as it was.
fn replace_run(state: &RwLock<State>, run: &[Arc<Segment>], merged: Segment) -> bool {
    let mut guard = write(state);
    let state = &mut *guard;
    let ids: Vec<u64> = run.iter().map(|segment| segment.id).collect();
    let Some(at) = state
        .segments
        .iter()
        .position(|segment| ids.first() == Some(&segment.id))
    else {
        return false;
    };
    let now: Vec<u64> = state
        .segments
        .iter()
        .skip(at)
        .take(ids.len())
        .map(|segment| segment.id)
        .collect();
    if now != ids {
        return false;
    }
    let newer = state.segments.split_off(at + ids.len());
    state.segments.truncate(at);
    state.segments.push(Arc::new(merged));
    state.segments.extend(newer);
    let tombstones: Tombstones = state
        .segments
        .iter()
        .flat_map(|segment| segment.tombstones.iter().cloned())
        .chain(state.unflushed_tombstones.iter().cloned())
        .collect();
    state.tombstones = tombstones;
    true
}

/// Deletes the files of segments a merge replaced, once nothing maps them;
/// Windows refuses to delete a mapped file, so one a query still holds goes
/// to `garbage`, deleted later.
fn delete_replaced(old: Vec<PathBuf>, garbage: &mut Vec<PathBuf>) {
    for path in old {
        if fs::remove_file(&path).is_err() && path.exists() {
            garbage.push(path);
        }
    }
}

fn segment_path(dir: &Path, id: u64) -> PathBuf {
    dir.join(format!("{id}.seg"))
}

/// Records the live segments and `writer`'s record in `dir`'s
/// [`MANIFEST`], at once.
fn write_manifest(dir: &Path, state: &RwLock<State>, writer: &Writer) -> Result<(), IndexError> {
    let manifest = Manifest {
        format: FORMAT_VERSION,
        segments: read(state).segments.iter().map(|s| s.id).collect(),
        next_file: writer.next_file,
        record: writer.record.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?;
    write_atomically(&dir.join(MANIFEST), &bytes, Readers::OwnerOnly)?;
    Ok(())
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Sorts `ids` and removes repeats.
fn sort_unique(ids: &mut Vec<u32>) {
    ids.sort_unstable();
    ids.dedup();
}

/// The ids in both sorted lists.
fn intersect(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (mut i, mut j) = (0, 0);
    let mut both = Vec::with_capacity(a.len().min(b.len()));
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                both.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    both
}

/// The ids in either sorted list, sorted.
fn union(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut either = Vec::with_capacity(a.len() + b.len());
    either.extend_from_slice(a);
    either.extend_from_slice(b);
    sort_unique(&mut either);
    either
}

impl FileIndex {
    /// Opens the index in `dir` (creating it), whose entries' words are
    /// counted from `roots`. An index of another format version, or one
    /// that cannot be read, is deleted and starts empty. Fails with
    /// [`IndexError::InUse`] while another Pane holds the folder.
    pub fn open(dir: &Path, roots: &[PathBuf]) -> Result<(FileIndex, Opened), IndexError> {
        create_private_dir(dir)?;
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LOCK))?;
        match lock_file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(IndexError::InUse),
            Err(fs::TryLockError::Error(error)) => return Err(error.into()),
        }
        let (loaded, opened) = match load(dir) {
            Ok(Some(loaded)) => (loaded, Opened::Existing),
            Ok(None) => {
                wipe(dir)?;
                (Loaded::default(), Opened::Fresh)
            }
            Err(why) => {
                wipe(dir)?;
                (Loaded::default(), Opened::Rebuilt(why))
            }
        };
        let roots = Roots::new(roots);
        let next_file = loaded.next_file;
        let wal = Wal::create(dir.join(format!("{next_file}.wal")))?;
        let mut memory = MemTable::default();
        let mut tombstones: Tombstones = loaded
            .segments
            .iter()
            .flat_map(|segment| segment.tombstones.iter().cloned())
            .collect();
        let mut unflushed = Vec::new();
        let mut max_seq = loaded.segments.iter().map(|s| s.max_seq).max().unwrap_or(0);
        for op in loaded.replayed {
            max_seq = max_seq.max(op.seq());
            match op {
                Op::Put { seq, key, meta } => {
                    let (terms, hint) = terms_and_hint(&roots, &key, Some(&meta));
                    memory.insert(
                        seq,
                        Prepared {
                            key,
                            meta: Some(meta),
                            terms,
                            hint,
                        },
                    );
                }
                Op::Delete { seq, key } => memory.insert(seq, Prepared::deletion(key)),
                Op::DeleteUnder { seq, prefix } => {
                    let tombstone = Tombstone { prefix, seq };
                    tombstones.push(tombstone.clone());
                    unflushed.push(tombstone);
                }
            }
        }
        let mut index = FileIndex {
            dir: dir.to_path_buf(),
            roots,
            _lock: lock_file,
            state: Arc::new(RwLock::new(State {
                memory,
                frozen: None,
                segments: loaded.segments,
                tombstones,
                unflushed_tombstones: unflushed,
            })),
            writer: Arc::new(Mutex::new(Writer {
                wal,
                next_seq: max_seq + 1,
                next_file: next_file + 1,
                record: loaded.record,
                garbage: Vec::new(),
            })),
            merges: Arc::new(Merges::default()),
            merger: None,
            folder_id_reads: AtomicUsize::new(0),
        };
        // What the logs held goes into a segment now, so that the logs can
        // be deleted.
        {
            let mut writer = lock(&index.writer);
            index.flush_locked(&mut writer)?;
            index.save_manifest(&writer)?;
        }
        for old in loaded.wals {
            let _ = fs::remove_file(old);
        }
        let merger = Merger {
            dir: index.dir.clone(),
            roots: index.roots.clone(),
            state: index.state.clone(),
            writer: index.writer.clone(),
            merges: index.merges.clone(),
        };
        let thread = std::thread::Builder::new()
            .name("pane-file-index-merge".into())
            .spawn(move || merger.run())?;
        index.merger = Some(thread);
        // An index left with segments to merge (a merge cut short when Pane
        // stopped) has them merged now.
        index.merges.want();
        Ok((index, opened))
    }

    /// Prepares entries for [`Bulk::add`] or [`FileIndex::apply`]'s like:
    /// their keys, terms and hints. Called on the walker's threads.
    pub fn prepare(&self, entries: Vec<Entry>) -> PreparedBatch {
        PreparedBatch(
            entries
                .into_iter()
                .map(|entry| Prepared::entry(&self.roots, &entry.path, entry.meta))
                .collect(),
        )
    }

    /// Applies `changes` at once: logged, then visible to queries.
    pub fn apply(&self, changes: &[Change]) -> Result<(), IndexError> {
        let mut prepared: Vec<Prepared> = Vec::new();
        let mut prefixes: Vec<Option<Vec<u8>>> = Vec::new();
        for change in changes {
            match change {
                Change::Put(entry) => {
                    prepared.push(Prepared::entry(&self.roots, &entry.path, entry.meta));
                    prefixes.push(None);
                }
                Change::Remove(path) => {
                    prepared.push(Prepared::deletion(path_key(path)));
                    prefixes.push(None);
                }
                Change::RemoveUnder(path) => {
                    let key = path_key(path);
                    let mut prefix = key.clone();
                    if prefix.last() != Some(&SEPARATOR) {
                        prefix.push(SEPARATOR);
                    }
                    prepared.push(Prepared::deletion(key));
                    prefixes.push(Some(prefix));
                }
            }
        }
        let mut writer = lock(&self.writer);
        let mut ops = Vec::with_capacity(prepared.len() + 1);
        let mut seqs = Vec::with_capacity(prepared.len());
        for (doc, prefix) in prepared.iter().zip(&prefixes) {
            if let Some(prefix) = prefix {
                let seq = writer.next_seq;
                writer.next_seq += 1;
                ops.push(Op::DeleteUnder {
                    seq,
                    prefix: prefix.clone(),
                });
            }
            let seq = writer.next_seq;
            writer.next_seq += 1;
            seqs.push(seq);
            ops.push(match doc.meta {
                Some(meta) => Op::Put {
                    seq,
                    key: doc.key.clone(),
                    meta,
                },
                None => Op::Delete {
                    seq,
                    key: doc.key.clone(),
                },
            });
        }
        writer.wal.append(&ops)?;
        {
            let mut state = write(&self.state);
            for op in &ops {
                if let Op::DeleteUnder { seq, prefix } = op {
                    let tombstone = Tombstone {
                        prefix: prefix.clone(),
                        seq: *seq,
                    };
                    state.tombstones.push(tombstone.clone());
                    state.unflushed_tombstones.push(tombstone);
                }
            }
            for (doc, seq) in prepared.into_iter().zip(seqs) {
                state.memory.insert(seq, doc);
            }
        }
        self.maintain_locked(&mut writer)
    }

    /// Starts writing many entries at once, as a first walk does: they go
    /// straight into segments, without the log, and become visible as each
    /// segment is written. Other changes wait until it is finished, and no
    /// merge runs meanwhile (one under way is finished first): it merges
    /// what it wrote at its end.
    pub fn bulk(&self) -> Result<Bulk<'_>, IndexError> {
        let merging = lock(&self.merges.merging);
        let mut writer = lock(&self.writer);
        // The memory table's entries are older than the segments to come.
        self.flush_locked(&mut writer)?;
        Ok(Bulk {
            index: self,
            writer,
            buffer: Vec::new(),
            _merging: merging,
        })
    }

    /// Writes the memory table as a segment; the merger then merges
    /// segments if some are due.
    pub fn flush(&self) -> Result<(), IndexError> {
        let mut writer = lock(&self.writer);
        self.flush_locked(&mut writer)
    }

    /// Merges every segment and the memory table into one segment, dropping
    /// superseded versions, deletions and what tombstones hide, here and
    /// now (after a merge under way).
    pub fn compact(&self) -> Result<(), IndexError> {
        let _merging = lock(&self.merges.merging);
        let mut writer = lock(&self.writer);
        self.compact_locked(&mut writer)
    }

    /// Waits until the merger has merged what is due: what the benchmark and
    /// the tests wait on, so that the segments they look at are as the
    /// merges leave them. Changes and queries never need to.
    pub fn wait_for_merges(&self) {
        self.merges.wait_until_done();
    }

    /// Holds merges off (none starts; one under way finishes first), or
    /// lets them go again: for the benchmark, which times queries on the
    /// segments a stream of changes leaves before they are merged, and the
    /// tests. Pane never holds them.
    pub fn hold_merges(&self, held: bool) {
        let mut turns = self.merges.turns();
        turns.held = held;
        self.merges.changed.notify_all();
        while held && turns.running {
            turns = self.merges.wait(turns);
        }
    }

    pub fn record(&self) -> IndexRecord {
        lock(&self.writer).record.clone()
    }

    /// Records `record` with the index, at once.
    pub fn set_record(&self, record: IndexRecord) -> Result<(), IndexError> {
        let mut writer = lock(&self.writer);
        writer.record = record;
        self.save_manifest(&writer)
    }

    /// The entry at `path`, if it is indexed.
    pub fn get(&self, path: &Path) -> Option<Meta> {
        let key = path_key(path);
        let state = read(&self.state);
        let sources = state.sources();
        for (at, source) in sources.iter().enumerate() {
            let stored = match source {
                Source::Memory(table) => table.get(&key).map(|entry| Stored {
                    key: entry.key.clone(),
                    seq: entry.seq,
                    meta: entry.meta,
                }),
                Source::Segment(segment) => segment.find(&key),
            };
            if let Some(stored) = stored {
                return if state.is_current(&sources, at, &key, stored.seq) {
                    stored.meta
                } else {
                    None
                };
            }
        }
        None
    }

    /// The entries indexed directly in `folder`, by path: what a
    /// reconciling walk compares with the folder's listing (#175).
    pub fn children(&self, folder: &Path) -> Vec<(PathBuf, Meta)> {
        let mut prefix = path_key(folder);
        if prefix.last() != Some(&SEPARATOR) {
            prefix.push(SEPARATOR);
        }
        let mut upper = prefix.clone();
        upper.push(0xFF);
        let direct = |key: &[u8]| {
            key.len() > prefix.len()
                && key.starts_with(&prefix)
                && !key[prefix.len()..].contains(&SEPARATOR)
        };
        let state = read(&self.state);
        let sources = state.sources();
        // The newest source holding a key decides it: listed, or hidden.
        let mut found: BTreeMap<Vec<u8>, Option<Meta>> = BTreeMap::new();
        for (at, source) in sources.iter().enumerate() {
            let mut decide = |key: &[u8], seq: u64, meta: Option<Meta>| {
                if !direct(key) || found.contains_key(key) {
                    return;
                }
                let current = state.is_current(&sources, at, key, seq);
                found.insert(key.to_vec(), meta.filter(|_| current));
            };
            match source {
                Source::Memory(table) => {
                    let range = table.by_key.range::<[u8], _>((
                        Bound::Included(prefix.as_slice()),
                        Bound::Excluded(upper.as_slice()),
                    ));
                    for (key, &id) in range {
                        let entry = &table.entries[id as usize];
                        decide(key, entry.seq, entry.meta);
                    }
                }
                Source::Segment(segment) => {
                    for stored in segment.iter_from(&prefix) {
                        if stored.key.as_slice() < prefix.as_slice() {
                            continue;
                        }
                        if !stored.key.starts_with(&prefix) {
                            break;
                        }
                        decide(&stored.key, stored.seq, stored.meta);
                    }
                }
            }
        }
        found
            .into_iter()
            .filter_map(|(key, meta)| Some((key_path(&key), meta?)))
            .collect()
    }

    /// The entries matching `query.text`, best first: every word of the
    /// query starts a word of the entry's name or of a folder it is in
    /// below its root (a query with `/` or `\`: its parts match the
    /// folders and the name in order, see `text::score`). When those are
    /// fewer than the page asks for, words of three letters or more are
    /// also looked for inside words ("port" finds "report"), listed after
    /// every match by the start of words. Never waits for a walk or a
    /// merge.
    pub fn search(&self, query: &Query<'_>) -> Vec<Hit> {
        self.search_at(query, now_seconds())
    }

    /// [`FileIndex::search`] as it would answer at `now`, in seconds since
    /// 1970 (what recent changes are scored against).
    fn search_at(&self, query: &Query<'_>, now: u64) -> Vec<Hit> {
        let prepared = text::Prepared::new(query.text);
        if prepared.words.is_empty() || query.limit == 0 {
            return Vec::new();
        }
        let wanted = query.offset.saturating_add(query.limit);
        let cap = CANDIDATES.max(wanted.saturating_mul(4));
        let state = read(&self.state);
        let mut hits = self.matching(&state, &prepared, query.kind, cap, false, now);
        if prepared.finds_inside() {
            let found: std::collections::HashSet<Vec<u8>> =
                hits.iter().map(|(key, _)| key.clone()).collect();
            if found.len() < wanted {
                let inside = self.matching(&state, &prepared, query.kind, cap, true, now);
                hits.extend(inside.into_iter().filter(|(key, _)| !found.contains(key)));
            }
        }
        drop(state);
        finish(hits, query.offset, query.limit, |a, b| {
            b.score
                .total_cmp(&a.score)
                .then(b.meta.modified.cmp(&a.meta.modified))
        })
    }

    /// The current entries of every source matching `prepared` by the
    /// start of words, or, `inside`, by words found inside words
    /// (`text::score_inside`), each scored, with its key.
    fn matching(
        &self,
        state: &State,
        prepared: &text::Prepared,
        kind: Option<EntryKind>,
        cap: usize,
        inside: bool,
        now: u64,
    ) -> Vec<(Vec<u8>, Hit)> {
        let sources = state.sources();
        let mut hits: Vec<(Vec<u8>, Hit)> = Vec::new();
        for (at, source) in sources.iter().enumerate() {
            for id in candidates(source, &prepared.words, kind, cap, inside) {
                let Some(stored) = source.stored(id) else {
                    continue;
                };
                let Some(meta) = stored.meta else {
                    continue;
                };
                if !state.is_current(&sources, at, &stored.key, stored.seq) {
                    continue;
                }
                let (folders, name) = self.roots.split(&stored.key);
                let name = String::from_utf8_lossy(name);
                let folders: Vec<std::borrow::Cow<'_, str>> = folders
                    .iter()
                    .map(|folder| String::from_utf8_lossy(folder))
                    .collect();
                let folders: Vec<&str> = folders.iter().map(|folder| folder.as_ref()).collect();
                let candidate = text::Candidate {
                    name: &name,
                    folders: &folders,
                    is_folder: meta.kind == EntryKind::Folder,
                    modified: meta.modified,
                };
                let score = if inside {
                    text::score_inside(prepared, &candidate, now)
                } else {
                    text::score(prepared, &candidate, now)
                };
                if let Some(score) = score {
                    hits.push((
                        stored.key.clone(),
                        Hit {
                            path: key_path(&stored.key),
                            meta,
                            score,
                        },
                    ));
                }
            }
        }
        hits
    }

    /// The most recently modified entries, newest first, as Search Files
    /// lists them before anything is typed.
    pub fn recent(&self, limit: usize, kind: Option<EntryKind>) -> Vec<Hit> {
        let state = read(&self.state);
        let sources = state.sources();
        let cap = limit.saturating_mul(4).max(64);
        let mut hits: Vec<(Vec<u8>, Hit)> = Vec::new();
        for (at, source) in sources.iter().enumerate() {
            let mut ranked: Vec<(u32, u32)> = (0..source.len())
                .filter_map(|id| {
                    let hint = source.hint(id)?;
                    kind.is_none_or(|kind| hint_kind(hint) == Some(kind))
                        .then_some((hint_day(hint), id))
                })
                .collect();
            if ranked.len() > cap {
                ranked.select_nth_unstable_by(cap, |a, b| b.0.cmp(&a.0));
                ranked.truncate(cap);
            }
            for (_, id) in ranked {
                let Some(stored) = source.stored(id) else {
                    continue;
                };
                let Some(meta) = stored.meta else {
                    continue;
                };
                if state.is_current(&sources, at, &stored.key, stored.seq) {
                    hits.push((
                        stored.key.clone(),
                        Hit {
                            path: key_path(&stored.key),
                            meta,
                            score: 0.0,
                        },
                    ));
                }
            }
        }
        drop(state);
        finish(hits, 0, limit, |a, b| b.meta.modified.cmp(&a.meta.modified))
    }

    /// Each indexed folder's file id, and its path: what the change journal
    /// names folders by. Folders whose file system gave no id are left out.
    /// It visits every entry of every segment: the coordinator reads it at
    /// most once per open (`changes::FolderIds`, #187).
    pub fn folder_ids(&self) -> HashMap<u64, PathBuf> {
        self.folder_id_reads.fetch_add(1, Ordering::Relaxed);
        let state = read(&self.state);
        let sources = state.sources();
        let mut ids = HashMap::new();
        // Oldest first, so that a newer folder of the same id wins.
        for (at, source) in sources.iter().enumerate().rev() {
            for id in 0..source.len() {
                if source.hint(id).and_then(hint_kind) != Some(EntryKind::Folder) {
                    continue;
                }
                let Some(stored) = source.stored(id) else {
                    continue;
                };
                if let Some(meta) = stored.meta
                    && meta.file_id != 0
                    && state.is_current(&sources, at, &stored.key, stored.seq)
                {
                    ids.insert(meta.file_id, key_path(&stored.key));
                }
            }
        }
        ids
    }

    pub fn stats(&self) -> IndexStats {
        let state = read(&self.state);
        let stored = state
            .segments
            .iter()
            .map(|s| u64::from(s.len()))
            .sum::<u64>()
            + state.memory.by_key.len() as u64;
        let segments = state.segments.len();
        drop(state);
        let bytes_on_disk = fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter_map(|entry| entry.metadata().ok())
                    .filter(|metadata| metadata.is_file())
                    .map(|metadata| metadata.len())
                    .sum()
            })
            .unwrap_or(0);
        IndexStats {
            segments,
            stored,
            bytes_on_disk,
        }
    }

    fn segment_path(&self, id: u64) -> PathBuf {
        segment_path(&self.dir, id)
    }

    fn save_manifest(&self, writer: &Writer) -> Result<(), IndexError> {
        write_manifest(&self.dir, &self.state, writer)
    }

    /// After changes are applied: the memory table written as a segment
    /// when it is full. Merges are the merger's, never done here (#187).
    fn maintain_locked(&self, writer: &mut Writer) -> Result<(), IndexError> {
        let full = read(&self.state).memory.entries.len() >= MEMORY_TABLE_ENTRIES;
        if full {
            self.flush_locked(writer)?;
        }
        writer
            .garbage
            .retain(|path| fs::remove_file(path).is_err() && path.exists());
        Ok(())
    }

    /// Writes the memory table as a new segment and starts a new log, then
    /// asks the merger to look at the segments.
    fn flush_locked(&self, writer: &mut Writer) -> Result<(), IndexError> {
        let (frozen, carried, tombstones) = {
            let mut state = write(&self.state);
            if state.memory.is_empty() && state.unflushed_tombstones.is_empty() {
                return Ok(());
            }
            let frozen = Arc::new(std::mem::take(&mut state.memory));
            state.frozen = Some(frozen.clone());
            // Still unflushed until the segment carrying them is in place,
            // so that a merge rebuilding the tombstones meanwhile keeps
            // them.
            let carried = state.unflushed_tombstones.clone();
            (frozen, carried, state.tombstones.clone())
        };
        let wal_id = writer.next_file;
        let segment_id = writer.next_file + 1;
        writer.next_file += 2;
        let new_wal = Wal::create(self.dir.join(format!("{wal_id}.wal")))?;
        let old_wal = std::mem::replace(&mut writer.wal, new_wal);
        let docs = frozen.by_key.values().filter_map(|&id| {
            let entry = &frozen.entries[id as usize];
            if tombstones.covers(&entry.key, entry.seq) {
                return None;
            }
            let (terms, hint) = terms_and_hint(&self.roots, &entry.key, entry.meta.as_ref());
            Some((
                entry.seq,
                Prepared {
                    key: entry.key.clone(),
                    meta: entry.meta,
                    terms,
                    hint,
                },
            ))
        });
        let path = self.segment_path(segment_id);
        segment::write(&path, docs, &carried)?;
        let segment = Arc::new(Segment::open(segment_id, path)?);
        {
            let mut state = write(&self.state);
            state.segments.push(segment);
            state.frozen = None;
            // No change came meanwhile (the writer is held): what was
            // unflushed is what the segment carries.
            let written = carried.len().min(state.unflushed_tombstones.len());
            state.unflushed_tombstones = state.unflushed_tombstones.split_off(written);
        }
        self.save_manifest(writer)?;
        let old_path = old_wal.path.clone();
        drop(old_wal);
        let _ = fs::remove_file(old_path);
        self.merges.want();
        Ok(())
    }

    /// Merges every segment into one, here and now, with the memory table
    /// written first. Called holding `merges.merging` (by
    /// [`FileIndex::compact`] and [`Bulk::finish`]) and the writer.
    fn compact_locked(&self, writer: &mut Writer) -> Result<(), IndexError> {
        self.flush_locked(writer)?;
        let (segments, tombstones) = {
            let state = read(&self.state);
            (state.segments.clone(), state.tombstones.clone())
        };
        if segments.len() <= 1 && tombstones.is_empty() {
            return Ok(());
        }
        let id = writer.next_file;
        writer.next_file += 1;
        let written = write_merged(
            &self.dir,
            &self.roots,
            &segments,
            &tombstones,
            true,
            id,
            &self.merges.closing,
        )?;
        let Some(merged) = written else {
            return Ok(());
        };
        if !replace_run(&self.state, &segments, merged) {
            let _ = fs::remove_file(self.segment_path(id));
            return Ok(());
        }
        self.save_manifest(writer)?;
        // Each old segment's map is released with its last reference.
        let old: Vec<PathBuf> = segments.iter().map(|s| s.path.clone()).collect();
        drop(segments);
        delete_replaced(old, &mut writer.garbage);
        Ok(())
    }
}

#[cfg(test)]
impl FileIndex {
    /// How often [`FileIndex::folder_ids`] read the table since the index
    /// opened.
    pub(crate) fn folder_id_reads(&self) -> usize {
        self.folder_id_reads.load(Ordering::Relaxed)
    }

    /// Makes merges wait before their segment takes the run's place, or
    /// lets them go on.
    pub(crate) fn pause_merges_before_swap(&self, pause: bool) {
        self.merges.turns().pause_before_swap = pause;
        self.merges.changed.notify_all();
    }

    /// Waits until a merge waits before its swap, at most `limit`: whether
    /// one does.
    pub(crate) fn wait_until_merge_paused(&self, limit: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + limit;
        let mut turns = self.merges.turns();
        while !turns.paused {
            let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) else {
                return false;
            };
            turns = self
                .merges
                .changed
                .wait_timeout(turns, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
        true
    }
}

/// Entries going straight into segments; see [`FileIndex::bulk`].
pub struct Bulk<'a> {
    index: &'a FileIndex,
    writer: MutexGuard<'a, Writer>,
    buffer: Vec<(u64, Prepared)>,
    /// No merge runs while a first walk writes.
    _merging: MutexGuard<'a, ()>,
}

impl Bulk<'_> {
    pub fn add(&mut self, batch: PreparedBatch) -> Result<(), IndexError> {
        for prepared in batch.0 {
            let seq = self.writer.next_seq;
            self.writer.next_seq += 1;
            self.buffer.push((seq, prepared));
        }
        if self.buffer.len() >= BULK_SEGMENT_ENTRIES {
            self.write_buffer()?;
        }
        Ok(())
    }

    fn write_buffer(&mut self) -> Result<(), IndexError> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let mut buffer = std::mem::take(&mut self.buffer);
        // Newest version of each key first, then the others dropped.
        buffer.sort_unstable_by(|a, b| a.1.key.cmp(&b.1.key).then(b.0.cmp(&a.0)));
        buffer.dedup_by(|later, kept| later.1.key == kept.1.key);
        let id = self.writer.next_file;
        self.writer.next_file += 1;
        let path = self.index.segment_path(id);
        segment::write(&path, buffer.into_iter(), &[])?;
        let segment = Arc::new(Segment::open(id, path)?);
        write(&self.index.state).segments.push(segment);
        self.index.save_manifest(&self.writer)
    }

    /// Writes what is left and merges the segments into one.
    pub fn finish(mut self) -> Result<(), IndexError> {
        self.write_buffer()?;
        self.index.compact_locked(&mut self.writer)
    }
}

/// The newest version of each key across segments, in key order.
struct Merge<'a> {
    iters: Vec<segment::SegmentIter<'a>>,
    heads: Vec<Option<Stored>>,
}

impl<'a> Merge<'a> {
    fn new(segments: &'a [Arc<Segment>]) -> Merge<'a> {
        let mut iters: Vec<segment::SegmentIter<'a>> =
            segments.iter().map(|segment| segment.iter()).collect();
        let heads = iters.iter_mut().map(Iterator::next).collect();
        Merge { iters, heads }
    }
}

impl Iterator for Merge<'_> {
    type Item = Stored;

    fn next(&mut self) -> Option<Stored> {
        let smallest = self
            .heads
            .iter()
            .flatten()
            .map(|head| head.key.as_slice())
            .min()?
            .to_vec();
        let mut newest: Option<Stored> = None;
        for (slot, iter) in self.heads.iter_mut().zip(self.iters.iter_mut()) {
            if slot.as_ref().is_some_and(|head| head.key == smallest) {
                let head = std::mem::replace(slot, iter.next()).expect("checked above");
                if newest.as_ref().is_none_or(|kept| head.seq > kept.seq) {
                    newest = Some(head);
                }
            }
        }
        newest
    }
}

/// The ids of the entries of `source` matching every word, at most `cap`:
/// those with every word in their own name first, then by an exact word,
/// recent change and shallowness, as their hints tell without reading them.
/// A word matches a term it starts or, `inside`, one it is anywhere in, when
/// it has `text::INSIDE_FROM` letters or more (found through the terms'
/// fragments, [`Source::word_inside`]).
fn candidates(
    source: &Source<'_>,
    words: &[String],
    kind: Option<EntryKind>,
    cap: usize,
    inside: bool,
) -> Vec<u32> {
    let mut matched: Option<Vec<u32>> = None;
    let mut in_names: Option<Vec<u32>> = None;
    let mut exact = Vec::new();
    let mut names = Vec::new();
    let mut folders = Vec::new();
    let mut ignored = Vec::new();
    for word in words {
        names.clear();
        folders.clear();
        if inside && word.chars().count() >= text::INSIDE_FROM {
            source.word_inside(NAME_TAG, word, &mut names);
            source.word_inside(FOLDER_TAG, word, &mut folders);
        } else {
            source.word(NAME_TAG, word, &mut names, &mut exact);
            source.word(FOLDER_TAG, word, &mut folders, &mut ignored);
        }
        ignored.clear();
        sort_unique(&mut names);
        sort_unique(&mut folders);
        let either = union(&names, &folders);
        let next = match matched {
            None => either,
            Some(before) => intersect(&before, &either),
        };
        if next.is_empty() {
            return Vec::new();
        }
        matched = Some(next);
        in_names = Some(match in_names {
            None => names.clone(),
            Some(before) => intersect(&before, &names),
        });
    }
    let (Some(matched), Some(in_names)) = (matched, in_names) else {
        return Vec::new();
    };
    sort_unique(&mut exact);
    let mut ranked: Vec<(u64, u32)> = matched
        .iter()
        .filter_map(|&id| {
            let hint = source.hint(id)?;
            if kind.is_some_and(|kind| hint_kind(hint) != Some(kind)) {
                return None;
            }
            let rank = (u64::from(in_names.binary_search(&id).is_ok()) << 63)
                | (u64::from(exact.binary_search(&id).is_ok()) << 62)
                | (u64::from(hint_day(hint)) << 16)
                | u64::from(255 - hint_depth(hint));
            Some((rank, id))
        })
        .collect();
    if ranked.len() > cap {
        ranked.select_nth_unstable_by(cap, |a, b| b.0.cmp(&a.0));
        ranked.truncate(cap);
    }
    ranked.into_iter().map(|(_, id)| id).collect()
}

/// Sorts `hits` by `order`, keeps the first of each key, then pages.
fn finish(
    mut hits: Vec<(Vec<u8>, Hit)>,
    offset: usize,
    limit: usize,
    order: impl Fn(&Hit, &Hit) -> std::cmp::Ordering,
) -> Vec<Hit> {
    hits.sort_by(|a, b| order(&a.1, &b.1).then_with(|| a.0.cmp(&b.0)));
    let mut seen = std::collections::HashSet::new();
    hits.into_iter()
        .filter(|(key, _)| seen.insert(key.clone()))
        .skip(offset)
        .take(limit)
        .map(|(_, hit)| hit)
        .collect()
}

/// What an index folder held.
#[derive(Default)]
struct Loaded {
    segments: Vec<Arc<Segment>>,
    replayed: Vec<Op>,
    wals: Vec<PathBuf>,
    next_file: u64,
    record: IndexRecord,
}

/// Reads the index in `dir`: `None` when there is none, an error saying
/// why when it cannot be read.
fn load(dir: &Path) -> Result<Option<Loaded>, String> {
    let manifest = match fs::read(dir.join(MANIFEST)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("its record could not be read: {error}")),
    };
    #[derive(Deserialize)]
    struct Version {
        format: u32,
    }
    let version: Version = serde_json::from_slice(&manifest)
        .map_err(|error| format!("its record is damaged: {error}"))?;
    if version.format != FORMAT_VERSION {
        return Err(format!(
            "it is of format {}, and this Pane reads format {FORMAT_VERSION}",
            version.format
        ));
    }
    let manifest: Manifest = serde_json::from_slice(&manifest)
        .map_err(|error| format!("its record is damaged: {error}"))?;
    let mut segments = Vec::new();
    for &id in &manifest.segments {
        let path = dir.join(format!("{id}.seg"));
        let segment = Segment::open(id, path).map_err(|error| format!("{error}"))?;
        segments.push(Arc::new(segment));
    }
    let mut wals: Vec<(u64, PathBuf)> = Vec::new();
    let mut next_file = manifest.next_file;
    let entries = fs::read_dir(dir).map_err(|error| error.to_string())?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let numbered = |suffix: &str| {
            name.strip_suffix(suffix)
                .and_then(|n| n.parse::<u64>().ok())
        };
        if let Some(id) = numbered(".wal") {
            next_file = next_file.max(id + 1);
            wals.push((id, path));
        } else if let Some(id) = numbered(".seg") {
            next_file = next_file.max(id + 1);
            if !manifest.segments.contains(&id) {
                // Written by a flush or merge that did not finish.
                let _ = fs::remove_file(&path);
            }
        } else if name.ends_with(".tmp") {
            let _ = fs::remove_file(&path);
        }
    }
    wals.sort();
    let mut replayed = Vec::new();
    for (_, path) in &wals {
        replayed.extend(wal::replay(path).map_err(|error| error.to_string())?);
    }
    Ok(Some(Loaded {
        segments,
        replayed,
        wals: wals.into_iter().map(|(_, path)| path).collect(),
        next_file,
        record: manifest.record,
    }))
}

/// Deletes everything in the index folder but its lock.
fn wipe(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
        if entry.file_name() != LOCK {
            let path = entry.path();
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}

/// Creates the index folder, readable by the user only: mode 0700 on Unix,
/// and on Windows a protected DACL for the user and SYSTEM, inherited by
/// its files, as `credentials.json` has (#175). A folder that exists keeps
/// its permissions on Windows.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        if !dir.exists() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
    }
    #[cfg(windows)]
    {
        if dir.is_dir() {
            return Ok(());
        }
        if let Some(parent) = dir.parent() {
            fs::create_dir_all(parent)?;
        }
        match crate::atomic::create_owner_only_dir(dir) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            done => done,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        fs::create_dir_all(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        home: PathBuf,
        index_dir: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let index_dir = dir.path().join("cache").join("file-index");
        Fixture {
            _dir: dir,
            home,
            index_dir,
        }
    }

    impl Fixture {
        fn open(&self) -> (FileIndex, Opened) {
            FileIndex::open(&self.index_dir, std::slice::from_ref(&self.home)).unwrap()
        }

        fn entry(&self, relative: &str, kind: EntryKind, modified: u64) -> Entry {
            let mut path = self.home.clone();
            for part in relative.split('/') {
                path.push(part);
            }
            Entry {
                path,
                meta: Meta {
                    kind,
                    size: 1,
                    modified,
                    file_id: relative.len() as u64 + 1,
                    volume: 1,
                },
            }
        }

        fn path(&self, relative: &str) -> PathBuf {
            let mut path = self.home.clone();
            for part in relative.split('/') {
                path.push(part);
            }
            path
        }
    }

    fn names(hits: &[Hit]) -> Vec<String> {
        hits.iter()
            .map(|hit| hit.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    fn query(text: &str) -> Query<'_> {
        Query {
            text,
            limit: 20,
            offset: 0,
            kind: None,
        }
    }

    fn sample(fixture: &Fixture) -> Vec<Entry> {
        vec![
            fixture.entry("Invoices 2026", EntryKind::Folder, 100),
            fixture.entry("Invoices 2026/march.pdf", EntryKind::File, 100),
            fixture.entry("Résumé plan ü.txt", EntryKind::File, 100),
            fixture.entry("plan.txt", EntryKind::File, 100),
            fixture.entry("notes/planning notes.md", EntryKind::File, 100),
            fixture.entry("notes", EntryKind::Folder, 100),
        ]
    }

    #[test]
    fn entries_are_found_by_name_prefix_words_and_folders_ignoring_case_and_accents() {
        let fixture = fixture();
        let (index, opened) = fixture.open();
        assert_eq!(opened, Opened::Fresh);
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(sample(&fixture))).unwrap();
        bulk.finish().unwrap();

        assert_eq!(
            names(&index.search(&query("plan"))),
            ["plan.txt", "planning notes.md", "Résumé plan ü.txt"]
        );
        assert_eq!(
            names(&index.search(&query("RESUME"))),
            ["Résumé plan ü.txt"]
        );
        assert_eq!(
            names(&index.search(&query("invoices march"))),
            ["march.pdf"]
        );
        assert_eq!(
            names(&index.search(&query("invoices"))),
            ["Invoices 2026", "march.pdf"]
        );
        assert!(index.search(&query("nothing")).is_empty());
        assert!(index.search(&query("  ")).is_empty());
        let folders = index.search(&Query {
            kind: Some(EntryKind::Folder),
            ..query("notes")
        });
        assert_eq!(names(&folders), ["notes"]);
    }

    #[test]
    fn a_query_inside_a_word_is_found_after_the_words_it_starts() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(vec![
            fixture.entry("report.pdf", EntryKind::File, 100),
            fixture.entry("portfolio.txt", EntryKind::File, 100),
            fixture.entry("Reports", EntryKind::Folder, 100),
            fixture.entry("Reports/q1.xlsx", EntryKind::File, 100),
            fixture.entry("sport/ball.txt", EntryKind::File, 100),
        ]))
        .unwrap();
        bulk.finish().unwrap();
        // The memory table too: a change since the last segment.
        index
            .apply(&[Change::Put(fixture.entry(
                "airport.md",
                EntryKind::File,
                100,
            ))])
            .unwrap();

        let found = names(&index.search(&query("port")));
        assert_eq!(found[0], "portfolio.txt", "{found:?}");
        for inside in ["report.pdf", "Reports", "airport.md", "q1.xlsx", "ball.txt"] {
            assert!(
                found.iter().any(|name| name == inside),
                "{inside}: {found:?}"
            );
        }
        // A page the words' starts fill lists nothing found inside.
        let first = index.search(&Query {
            limit: 1,
            ..query("port")
        });
        assert_eq!(names(&first), ["portfolio.txt"]);
        // Short words must start a word.
        assert!(
            names(&index.search(&query("po")))
                .iter()
                .all(|name| name == "portfolio.txt")
        );
    }

    #[test]
    fn a_query_with_a_separator_matches_path_segments_in_order() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(vec![
            fixture.entry("Documents/Work/plan.txt", EntryKind::File, 100),
            fixture.entry("Work/Documents/plan.txt", EntryKind::File, 100),
            fixture.entry("Documents/plan notes.md", EntryKind::File, 100),
        ]))
        .unwrap();
        bulk.finish().unwrap();
        let paths = |text: &str| -> Vec<PathBuf> {
            index
                .search(&query(text))
                .into_iter()
                .map(|hit| hit.path)
                .collect()
        };
        assert_eq!(
            paths("work/documents/plan"),
            [fixture.path("Work/Documents/plan.txt")]
        );
        assert_eq!(
            paths(r"documents\work\plan"),
            [fixture.path("Documents/Work/plan.txt")]
        );
        let both = paths("documents/plan");
        assert_eq!(both.len(), 3, "{both:?}");
        assert!(paths("plan/documents").is_empty());
    }

    #[test]
    fn changes_are_visible_at_once_and_survive_a_restart() {
        let fixture = fixture();
        {
            let (index, _) = fixture.open();
            index
                .apply(
                    &sample(&fixture)
                        .into_iter()
                        .map(Change::Put)
                        .collect::<Vec<_>>(),
                )
                .unwrap();
            assert_eq!(names(&index.search(&query("march"))), ["march.pdf"]);
            index
                .apply(&[Change::Remove(fixture.path("plan.txt"))])
                .unwrap();
            assert_eq!(index.get(&fixture.path("plan.txt")), None);
            assert!(index.get(&fixture.path("notes")).is_some());
            // Stopped without a flush: the log keeps the changes.
        }
        let (index, opened) = fixture.open();
        assert_eq!(opened, Opened::Existing);
        assert_eq!(names(&index.search(&query("march"))), ["march.pdf"]);
        assert!(
            names(&index.search(&query("plan")))
                .iter()
                .all(|name| name != "plan.txt")
        );
    }

    #[test]
    fn a_folders_children_are_its_current_direct_entries_across_segments_and_memory() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(sample(&fixture))).unwrap();
        bulk.finish().unwrap();
        index
            .apply(&[
                Change::Put(fixture.entry("notes/todo.txt", EntryKind::File, 300)),
                Change::Remove(fixture.path("notes/planning notes.md")),
            ])
            .unwrap();
        let children = |relative: &str| -> Vec<String> {
            index
                .children(&fixture.path(relative))
                .into_iter()
                .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(children("notes"), ["todo.txt"]);
        assert_eq!(children("Invoices 2026"), ["march.pdf"]);
        let mut top = index
            .children(&fixture.home)
            .into_iter()
            .map(|(path, meta)| (path, meta.kind))
            .collect::<Vec<_>>();
        top.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            top,
            [
                (fixture.path("Invoices 2026"), EntryKind::Folder),
                (fixture.path("Résumé plan ü.txt"), EntryKind::File),
                (fixture.path("notes"), EntryKind::Folder),
                (fixture.path("plan.txt"), EntryKind::File),
            ]
        );
        index
            .apply(&[Change::RemoveUnder(fixture.path("notes"))])
            .unwrap();
        assert!(children("notes").is_empty());
    }

    #[test]
    fn a_removed_folder_hides_everything_under_it_in_older_segments() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(sample(&fixture))).unwrap();
        bulk.finish().unwrap();
        index
            .apply(&[Change::RemoveUnder(fixture.path("Invoices 2026"))])
            .unwrap();
        assert!(index.search(&query("march")).is_empty());
        assert!(index.search(&query("invoices")).is_empty());
        // A new file of the same name is found again.
        index
            .apply(&[Change::Put(fixture.entry(
                "Invoices 2026/march.pdf",
                EntryKind::File,
                200,
            ))])
            .unwrap();
        assert_eq!(names(&index.search(&query("march"))), ["march.pdf"]);
        index.compact().unwrap();
        assert_eq!(names(&index.search(&query("march"))), ["march.pdf"]);
        assert_eq!(index.stats().segments, 1);
        // Merged away: the folder and the old file; the sample had 6.
        assert_eq!(index.stats().stored, 5);
    }

    #[test]
    fn a_changed_entry_replaces_its_older_version() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(sample(&fixture))).unwrap();
        bulk.finish().unwrap();
        let mut changed = fixture.entry("plan.txt", EntryKind::File, 999);
        changed.meta.size = 42;
        index.apply(&[Change::Put(changed)]).unwrap();
        let plan = fixture.path("plan.txt");
        for flushed in [false, true] {
            if flushed {
                index.flush().unwrap();
            }
            let hits = index.search(&query("plan.txt"));
            let found: Vec<&Hit> = hits.iter().filter(|hit| hit.path == plan).collect();
            assert_eq!(found.len(), 1, "flushed: {flushed}");
            assert_eq!(found[0].meta.size, 42);
            // The exact name ranks first.
            assert_eq!(hits[0].path, plan);
        }
    }

    #[test]
    fn recent_entries_come_newest_first() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        let entries = vec![
            fixture.entry("old.txt", EntryKind::File, 86_400),
            fixture.entry("new.txt", EntryKind::File, 86_400 * 30),
            fixture.entry("middle", EntryKind::Folder, 86_400 * 10),
        ];
        index
            .apply(&entries.into_iter().map(Change::Put).collect::<Vec<_>>())
            .unwrap();
        assert_eq!(
            names(&index.recent(10, None)),
            ["new.txt", "middle", "old.txt"]
        );
        assert_eq!(
            names(&index.recent(10, Some(EntryKind::Folder))),
            ["middle"]
        );
        assert_eq!(names(&index.recent(1, None)), ["new.txt"]);
    }

    #[test]
    fn pages_follow_each_other() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        let entries: Vec<Entry> = (0..30)
            .map(|n| fixture.entry(&format!("report {n:02}.txt"), EntryKind::File, 100 + n))
            .collect();
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(entries)).unwrap();
        bulk.finish().unwrap();
        let first = index.search(&Query {
            limit: 10,
            ..query("report")
        });
        let second = index.search(&Query {
            limit: 10,
            offset: 10,
            ..query("report")
        });
        assert_eq!(first.len(), 10);
        assert_eq!(second.len(), 10);
        assert!(first.iter().all(|hit| !second.contains(hit)));
    }

    #[test]
    fn another_format_version_is_rebuilt_never_read() {
        // The version before this one (an index written before #185's
        // fragments and key filters) and a later one.
        for version in [FORMAT_VERSION - 1, FORMAT_VERSION + 1] {
            let fixture = fixture();
            {
                let (index, _) = fixture.open();
                index
                    .apply(
                        &sample(&fixture)
                            .into_iter()
                            .map(Change::Put)
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                index.flush().unwrap();
                index
                    .set_record(IndexRecord {
                        built: true,
                        ..IndexRecord::default()
                    })
                    .unwrap();
            }
            let manifest = fixture.index_dir.join(MANIFEST);
            let text = fs::read_to_string(&manifest).unwrap();
            let other = text.replace(
                &format!("\"format\": {FORMAT_VERSION}"),
                &format!("\"format\": {version}"),
            );
            assert_ne!(text, other);
            fs::write(&manifest, other).unwrap();
            let (index, opened) = fixture.open();
            assert!(
                matches!(opened, Opened::Rebuilt(_)),
                "{version}: {opened:?}"
            );
            assert!(index.search(&query("march")).is_empty());
            assert_eq!(index.record(), IndexRecord::default());
            assert_eq!(index.stats().segments, 0);
        }
    }

    #[test]
    fn a_damaged_segment_is_rebuilt() {
        let fixture = fixture();
        {
            let (index, _) = fixture.open();
            index
                .apply(
                    &sample(&fixture)
                        .into_iter()
                        .map(Change::Put)
                        .collect::<Vec<_>>(),
                )
                .unwrap();
            index.flush().unwrap();
        }
        for entry in fs::read_dir(&fixture.index_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "seg") {
                fs::write(&path, b"damaged").unwrap();
            }
        }
        let (_, opened) = fixture.open();
        assert!(matches!(opened, Opened::Rebuilt(_)), "{opened:?}");
    }

    #[test]
    fn a_second_index_on_the_same_folder_is_refused() {
        let fixture = fixture();
        let (_index, _) = fixture.open();
        let second = FileIndex::open(&fixture.index_dir, std::slice::from_ref(&fixture.home));
        assert!(matches!(second, Err(IndexError::InUse)));
    }

    #[test]
    fn the_record_is_kept() {
        let fixture = fixture();
        let record = IndexRecord {
            built: true,
            cursors: vec![JournalCursor {
                volume: "C:\\".into(),
                journal_id: 7,
                next_usn: 1234,
            }],
            rules: Some(super::super::scope::ScopeRules::for_home(
                fixture.home.clone(),
            )),
        };
        {
            let (index, _) = fixture.open();
            index.set_record(record.clone()).unwrap();
        }
        let (index, _) = fixture.open();
        assert_eq!(index.record(), record);
    }

    #[test]
    fn folder_ids_name_the_current_folders() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(sample(&fixture))).unwrap();
        bulk.finish().unwrap();
        index
            .apply(&[Change::RemoveUnder(fixture.path("notes"))])
            .unwrap();
        let ids = index.folder_ids();
        let folder = fixture.entry("Invoices 2026", EntryKind::Folder, 0);
        assert_eq!(ids.get(&folder.meta.file_id), Some(&folder.path));
        assert_eq!(ids.len(), 1);
    }

    /// What the index should hold, kept without segments, memory tables or
    /// tombstones: each current entry by key, the changes applied in order.
    #[derive(Default)]
    struct Model(BTreeMap<Vec<u8>, Meta>);

    impl Model {
        fn apply(&mut self, changes: &[Change]) {
            for change in changes {
                match change {
                    Change::Put(entry) => {
                        self.0.insert(path_key(&entry.path), entry.meta);
                    }
                    Change::Remove(path) => {
                        self.0.remove(&path_key(path));
                    }
                    Change::RemoveUnder(path) => {
                        let key = path_key(path);
                        let mut prefix = key.clone();
                        if prefix.last() != Some(&SEPARATOR) {
                            prefix.push(SEPARATOR);
                        }
                        self.0
                            .retain(|known, _| *known != key && !known.starts_with(&prefix));
                    }
                }
            }
        }
    }

    /// `query` answered by brute force over `model`, as #126's "Matching
    /// and ranking" and [`FileIndex::search`] describe it: every entry of
    /// the kind asked for scored by the start of words; when fewer than the
    /// page asks for are found and a word has three letters or more, every
    /// other entry scored by words inside words; then best first, the more
    /// recently modified first, by path, and paged.
    fn reference(roots: &Roots, model: &Model, query: &Query<'_>, now: u64) -> Vec<Hit> {
        let prepared = text::Prepared::new(query.text);
        if prepared.words.is_empty() || query.limit == 0 {
            return Vec::new();
        }
        let scored = |inside: bool| -> Vec<(Vec<u8>, Hit)> {
            let mut hits = Vec::new();
            for (key, meta) in &model.0 {
                if query.kind.is_some_and(|kind| meta.kind != kind) {
                    continue;
                }
                let (folders, name) = roots.split(key);
                let name = String::from_utf8_lossy(name);
                let folders: Vec<String> = folders
                    .iter()
                    .map(|folder| String::from_utf8_lossy(folder).into_owned())
                    .collect();
                let folders: Vec<&str> = folders.iter().map(String::as_str).collect();
                let candidate = text::Candidate {
                    name: &name,
                    folders: &folders,
                    is_folder: meta.kind == EntryKind::Folder,
                    modified: meta.modified,
                };
                let score = if inside {
                    text::score_inside(&prepared, &candidate, now)
                } else {
                    text::score(&prepared, &candidate, now)
                };
                if let Some(score) = score {
                    let hit = Hit {
                        path: key_path(key),
                        meta: *meta,
                        score,
                    };
                    hits.push((key.clone(), hit));
                }
            }
            hits
        };
        let mut hits = scored(false);
        if prepared.finds_inside() && hits.len() < query.offset.saturating_add(query.limit) {
            hits.extend(scored(true));
        }
        finish(hits, query.offset, query.limit, |a, b| {
            b.score
                .total_cmp(&a.score)
                .then(b.meta.modified.cmp(&a.meta.modified))
        })
    }

    /// Prefixes, words found inside words, misses, accents and letter
    /// case, several words, whole names and paths.
    const REFERENCE_QUERIES: &[&str] = &[
        "r",
        "p",
        "do",
        "rep",
        "report",
        "Report",
        "REPORT",
        "pla",
        "inv",
        "port",
        "ort",
        "lann",
        "xplan",
        "oice",
        "may2026",
        "upport",
        "ssport",
        "ubers",
        "abcd",
        "qxzv",
        "zzzzz",
        "qx",
        "plan qxzv",
        "résumé",
        "RESUME",
        "Resume plan",
        "uber",
        "ÜBER",
        "übersicht",
        "invoices 2026",
        "sport passport",
        "documents plan",
        "port 2026",
        "ort club",
        "report.pdf",
        "Résumé plan ü.txt",
        "documents/plan",
        "sport/pass",
        "reports/",
    ];

    /// Pages: the limit, the offset and the kind asked for. A page of 1 or
    /// 3 is filled by the words' starts for some queries, so the pass
    /// inside words is left out; one of 20 rarely is.
    const REFERENCE_PAGES: &[(usize, usize, Option<EntryKind>)] = &[
        (20, 0, None),
        (3, 0, None),
        (5, 2, None),
        (1, 0, None),
        (20, 0, Some(EntryKind::Folder)),
    ];

    /// The fixture's first index: folders, and in each some of the names
    /// below, which start, hold and nearly hold one another ("port",
    /// "report", "portfolio", "sport"; "abcxbcd" has every fragment of
    /// "abcd" but not "abcd"), with accents, letter case, camel case and
    /// digits; modified on days apart, some on the same day.
    fn reference_entries(fixture: &Fixture, now: u64) -> Vec<Entry> {
        const FOLDERS: [&str; 9] = [
            "Documents",
            "Documents/Work 2026",
            "Invoices 2026",
            "Reports",
            "Sport Club",
            "Sport Club/Passports",
            "Projects",
            "Projects/airportRedesign",
            "Résumés",
        ];
        const NAMES: [&str; 16] = [
            "report.pdf",
            "Reports 2026.xlsx",
            "airport map.png",
            "Passport scan.jpg",
            "portfolio.txt",
            "Résumé plan ü.txt",
            "resume final.docx",
            "planning notes.md",
            "Explanation.md",
            "invoiceMay2026.pdf",
            "über uns.html",
            "ÜBERSICHT.txt",
            "sport club.txt",
            "porter stout.txt",
            "abcxbcd.txt",
            "plan.txt",
        ];
        let day = 86_400;
        let mut entries = Vec::new();
        for (at, folder) in FOLDERS.iter().enumerate() {
            let modified = now - (at as u64 % 4) * 9 * day;
            entries.push(fixture.entry(folder, EntryKind::Folder, modified));
        }
        let places = std::iter::once("").chain(FOLDERS);
        for (place_at, place) in places.enumerate() {
            for (name_at, name) in NAMES.iter().enumerate() {
                if (name_at + place_at) % 3 == 1 {
                    continue;
                }
                let relative = if place.is_empty() {
                    (*name).to_owned()
                } else {
                    format!("{place}/{name}")
                };
                let modified = now - ((place_at * 7 + name_at * 13) % 6) as u64 * 11 * day;
                entries.push(fixture.entry(&relative, EntryKind::File, modified));
            }
        }
        entries
    }

    /// Five batches of changes after the first index: new files, new
    /// versions, files deleted, folders deleted (tombstones) with new files
    /// put back under them, in older segments and in memory.
    fn reference_changes(fixture: &Fixture, now: u64) -> Vec<Vec<Change>> {
        let day = 86_400;
        let file = |relative: &str, days_ago: u64| {
            Change::Put(fixture.entry(relative, EntryKind::File, now - days_ago * day))
        };
        let folder = |relative: &str, days_ago: u64| {
            Change::Put(fixture.entry(relative, EntryKind::Folder, now - days_ago * day))
        };
        let remove = |relative: &str| Change::Remove(fixture.path(relative));
        let remove_under = |relative: &str| Change::RemoveUnder(fixture.path(relative));
        vec![
            vec![
                file("Documents/airport transfer.txt", 1),
                file("transport.md", 2),
                file("report.pdf", 0),
                remove("Passport scan.jpg"),
                // Never indexed: a deletion of nothing.
                remove("portfolio.txt"),
                remove("Documents/Work 2026/plan.txt"),
            ],
            vec![
                remove_under("Reports"),
                folder("Reports", 0),
                file("Reports/report draft.md", 3),
            ],
            vec![
                folder("Archive", 5),
                file("Archive/old report.pdf", 400),
                file("Archive/Résumé 2019.pdf", 2000),
                remove("Documents/Work 2026/planning notes.md"),
                file("Explanation.md", 1),
            ],
            vec![
                remove_under("Archive"),
                file("Sport Club/Passports/passport renewal.pdf", 4),
                remove_under("Sport Club/Passports"),
                file("Sport Club/Passports/support ticket.txt", 2),
            ],
            vec![
                file("Reports/report draft.md", 0),
                file("Documents/Report summary.txt", 6),
                remove("airport map.png"),
                remove_under("Projects/airportRedesign"),
                file("Projects/airportRedesign/support.txt", 1),
                file("Résumés/ÜBERSICHT 2026.txt", 3),
            ],
        ]
    }

    /// Asserts that `index` answers every query of the fixed set, on every
    /// page, as the brute-force reference does over `model`.
    fn assert_matches_reference(index: &FileIndex, model: &Model, now: u64, when: &str) {
        for &text in REFERENCE_QUERIES {
            for &(limit, offset, kind) in REFERENCE_PAGES {
                let query = Query {
                    text,
                    limit,
                    offset,
                    kind,
                };
                let found = index.search_at(&query, now);
                let expected = reference(&index.roots, model, &query, now);
                assert_eq!(
                    found, expected,
                    "{when}: {text:?}, limit {limit}, offset {offset}, {kind:?}"
                );
            }
        }
    }

    #[test]
    fn search_answers_as_a_brute_force_reference_across_segments_memory_and_tombstones() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        // The segments as each batch leaves them, unmerged.
        index.hold_merges(true);
        let now = 20_000 * 86_400;
        let mut model = Model::default();
        let first = reference_entries(&fixture, now);
        model.apply(&first.iter().cloned().map(Change::Put).collect::<Vec<_>>());
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(first)).unwrap();
        bulk.finish().unwrap();
        assert_matches_reference(&index, &model, now, "one segment");

        // Each batch but the last written as a segment of its own.
        let batches = reference_changes(&fixture, now);
        for (at, batch) in batches.iter().enumerate() {
            index.apply(batch).unwrap();
            model.apply(batch);
            if at + 1 < batches.len() {
                index.flush().unwrap();
            }
        }
        {
            let state = read(&index.state);
            assert_eq!(state.segments.len(), batches.len());
            assert!(!state.memory.is_empty());
            assert!(!state.tombstones.is_empty());
            assert!(!state.unflushed_tombstones.is_empty());
            // The fragments find what reading every term finds, in the
            // same order, in every segment and in memory.
            for source in state.sources() {
                for &text in REFERENCE_QUERIES {
                    for word in text::query_words(text) {
                        for tag in [NAME_TAG, FOLDER_TAG] {
                            let (mut scanned, mut looked_up) = (Vec::new(), Vec::new());
                            source.word_inside_scan(tag, &word, &mut scanned);
                            source.word_inside(tag, &word, &mut looked_up);
                            assert_eq!(looked_up, scanned, "{word:?}");
                        }
                    }
                }
            }
        }
        assert_matches_reference(&index, &model, now, "several segments and memory");

        // Opened again: the log's changes become a segment, and the merger
        // merges segments meanwhile.
        drop(index);
        let (index, opened) = fixture.open();
        assert_eq!(opened, Opened::Existing);
        assert_matches_reference(&index, &model, now, "opened again");
        index.wait_for_merges();
        assert!(index.stats().segments < batches.len() + 1);
        assert_matches_reference(&index, &model, now, "merged a few at a time");

        index.compact().unwrap();
        assert_eq!(index.stats().segments, 1);
        assert_matches_reference(&index, &model, now, "merged into one segment");
    }

    #[test]
    fn a_tombstone_hides_what_it_covers_as_reading_each_one_does() {
        let tombstones = [
            Tombstone {
                prefix: path_key(Path::new("home/Docs/")),
                seq: 10,
            },
            Tombstone {
                prefix: path_key(Path::new("home/Docs/")),
                seq: 4,
            },
            Tombstone {
                prefix: path_key(Path::new("home/Docs/Old/")),
                seq: 20,
            },
            // A prefix without a separator at its end.
            Tombstone {
                prefix: b"home/Pic".to_vec(),
                seq: 15,
            },
        ];
        let all: Tombstones = tombstones.iter().cloned().collect();
        let keys = [
            path_key(Path::new("home/Docs")),
            path_key(Path::new("home/Docs/a.txt")),
            path_key(Path::new("home/Docs/Old/b.txt")),
            path_key(Path::new("home/Docs/Older/c.txt")),
            path_key(Path::new("home/docs/a.txt")),
            path_key(Path::new("home/Pictures/d.png")),
            path_key(Path::new("home/Doc/e.txt")),
            path_key(Path::new("home/e.txt")),
            b"h".to_vec(),
        ];
        for key in &keys {
            for seq in [0, 3, 4, 9, 10, 14, 15, 19, 20, 21] {
                let each = tombstones
                    .iter()
                    .any(|tombstone| tombstone.covers(key, seq));
                assert_eq!(all.covers(key, seq), each, "{key:?} at {seq}");
            }
        }
        assert!(Tombstones::default().is_empty());
        assert!(!Tombstones::default().covers(&keys[1], 0));
    }

    #[test]
    fn many_segments_are_merged_a_few_at_a_time_in_the_background() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        for n in 0..(MAX_SEGMENTS + 2) {
            index
                .apply(&[Change::Put(fixture.entry(
                    &format!("file {n}.txt"),
                    EntryKind::File,
                    1,
                ))])
                .unwrap();
            index.flush().unwrap();
            index.apply(&[]).unwrap();
        }
        index.wait_for_merges();
        // Small segments are alike: any MERGE_FACTOR of them are merged.
        assert!(index.stats().segments < MERGE_FACTOR);
        assert_eq!(index.search(&query("file")).len(), MAX_SEGMENTS + 2);
    }

    #[test]
    fn a_few_neighbouring_segments_of_similar_size_are_merged_at_a_time() {
        let floor = MERGE_FLOOR;
        // Too few to merge.
        assert_eq!(tier_to_merge(&[]), None);
        assert_eq!(tier_to_merge(&[10, 10, 10]), None);
        // Small segments count as the floor: alike.
        assert_eq!(tier_to_merge(&[1, 10, 100, 1000]), Some(0..4));
        // A first index's large segment, then the changes' small ones: the
        // small ones.
        assert_eq!(
            tier_to_merge(&[400_000, floor, floor, floor, floor]),
            Some(1..5)
        );
        // Of two similar runs, the smaller; none mixing the two sizes.
        let two_tiers = [8, 8, 8, 8, 1, 1, 1, 1].map(|times| floor * times);
        assert_eq!(tier_to_merge(&two_tiers), Some(4..8));
        // Sizes too far apart: none, as long as there are few segments...
        let apart = [floor << 24, floor << 18, floor << 12, floor << 6, floor];
        assert_eq!(tier_to_merge(&apart), None);
        // ...and past MAX_SEGMENTS, the smallest run whatever the sizes.
        let many: Vec<u64> = (0..=MAX_SEGMENTS as u32)
            .rev()
            .map(|n| floor << (3 * n))
            .collect();
        let tail = many.len() - MERGE_FACTOR;
        assert_eq!(tier_to_merge(&many), Some(tail..many.len()));
    }

    /// Tombstones piling up above a first index's large segment, which the
    /// tiers seldom reach, merge every segment into one once they are many
    /// for its size (#187): only a merge from the oldest drops them.
    #[test]
    fn tombstones_piling_up_above_the_oldest_segment_merge_every_segment() {
        assert_eq!(oldest_run_due(&[], TOMBSTONES_DUE * 10), None);
        assert_eq!(oldest_run_due(&[400_000], TOMBSTONES_DUE * 10), None);
        // A 32nd of the oldest segment's entries, or TOMBSTONES_DUE.
        let sizes = [400_000, 4_000, 1_000];
        assert_eq!(oldest_run_due(&sizes, 400_000 / 32), None);
        assert_eq!(oldest_run_due(&sizes, 400_000 / 32 + 1), Some(0..3));
        assert_eq!(oldest_run_due(&[1_000, 10], TOMBSTONES_DUE), None);
        assert_eq!(oldest_run_due(&[1_000, 10], TOMBSTONES_DUE + 1), Some(0..2));
        // The tiers come first: here none is due, as after a first index.
        assert_eq!(tier_to_merge(&sizes), None);
    }

    /// A merge that panics is given up and said so (#187): waiting for the
    /// merges returns, and the next merge due runs.
    #[test]
    fn a_merge_that_panics_is_given_up_and_the_next_one_runs() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        index.hold_merges(true);
        let flush = |name: &str| {
            let entry = fixture.entry(name, EntryKind::File, 1);
            index.apply(&[Change::Put(entry)]).unwrap();
            index.flush().unwrap();
        };
        for n in 0..MERGE_FACTOR {
            flush(&format!("batch {n}.txt"));
        }
        index.merges.turns().panic_next = true;
        index.hold_merges(false);
        index.wait_for_merges();
        assert!(!index.merges.turns().panic_next, "the merge ran");
        assert_eq!(index.stats().segments, MERGE_FACTOR, "and merged nothing");

        flush("after.txt");
        index.wait_for_merges();
        assert!(
            index.stats().segments < MERGE_FACTOR,
            "merged once more due"
        );
        assert_eq!(index.search(&query("batch")).len(), MERGE_FACTOR);
        assert_eq!(names(&index.search(&query("after"))), ["after.txt"]);
    }

    /// A word of one or two letters stops gathering entries at
    /// [`SHORT_WORD_ENTRIES`] however many more of its terms there are, in
    /// memory and in a segment alike (#185).
    #[test]
    fn a_short_word_stops_gathering_entries_at_its_limit() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        index.hold_merges(true);
        // Distinct words of letters only, each starting with `a`.
        let word = |mut n: usize| {
            let mut word = String::from("a");
            for _ in 0..4 {
                word.push(char::from(b'a' + (n % 26) as u8));
                n /= 26;
            }
            word
        };
        let count = SHORT_WORD_ENTRIES + 10;
        let changes: Vec<Change> = (0..count)
            .map(|n| fixture.entry(&format!("{}.txt", word(n)), EntryKind::File, 1))
            .map(Change::Put)
            .collect();
        index.apply(&changes).unwrap();
        let gathered = |source: &Source<'_>, text: &str| {
            let (mut all, mut exact) = (Vec::new(), Vec::new());
            source.word(NAME_TAG, text, &mut all, &mut exact);
            all.len()
        };
        {
            let state = read(&index.state);
            assert!(state.segments.is_empty(), "all in memory");
            let memory = Source::Memory(&state.memory);
            assert_eq!(gathered(&memory, "a"), SHORT_WORD_ENTRIES);
            // A longer word is not stopped: every `aa…` word.
            assert_eq!(gathered(&memory, "aaa"), count.div_ceil(26 * 26));
        }
        index.flush().unwrap();
        let state = read(&index.state);
        let segment = Source::Segment(&state.segments[0]);
        assert_eq!(gathered(&segment, "a"), SHORT_WORD_ENTRIES);
        assert_eq!(gathered(&segment, "aaa"), count.div_ceil(26 * 26));
    }

    /// A query reads at most [`CANDIDATES`] entries of a segment (more when
    /// the page asks for more), the best its hints tell (#185): of more
    /// entries than that matching, the newest and shallowest is among those
    /// read, and found first.
    #[test]
    fn a_query_matching_more_entries_than_it_reads_still_finds_the_best_first() {
        let fixture = fixture();
        let (index, _) = fixture.open();
        let mut entries: Vec<Entry> = (0..CANDIDATES + 200)
            .map(|n| fixture.entry(&format!("old/drafts/report {n}.txt"), EntryKind::File, 1))
            .collect();
        let newest = fixture.entry("report latest.txt", EntryKind::File, now_seconds());
        entries.push(newest.clone());
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(entries)).unwrap();
        bulk.finish().unwrap();
        {
            let state = read(&index.state);
            assert_eq!(state.segments.len(), 1);
            let segment = Source::Segment(&state.segments[0]);
            let read_ids = candidates(&segment, &["rep".to_owned()], None, CANDIDATES, false);
            assert_eq!(read_ids.len(), CANDIDATES);
            let key = path_key(&newest.path);
            assert!(
                read_ids
                    .iter()
                    .any(|&id| segment.stored(id).is_some_and(|stored| stored.key == key))
            );
        }
        let hits = index.search(&Query {
            text: "rep",
            limit: 1,
            offset: 0,
            kind: None,
        });
        assert_eq!(names(&hits), ["report latest.txt"]);
    }

    /// The index with the reference fixture's first index as one segment
    /// and its first `flushed` batches of changes each written as a segment
    /// of its own, merges held off; and the model of what it holds.
    fn reference_index(fixture: &Fixture, now: u64, flushed: usize) -> (FileIndex, Model) {
        let (index, _) = fixture.open();
        index.hold_merges(true);
        let mut model = Model::default();
        let first = reference_entries(fixture, now);
        model.apply(&first.iter().cloned().map(Change::Put).collect::<Vec<_>>());
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(first)).unwrap();
        bulk.finish().unwrap();
        for batch in &reference_changes(fixture, now)[..flushed] {
            index.apply(batch).unwrap();
            model.apply(batch);
            index.flush().unwrap();
        }
        assert_eq!(index.stats().segments, flushed + 1);
        (index, model)
    }

    /// Lets the merger go and waits until a merge has written its segment
    /// but not yet put it in the place of the run it merged.
    fn start_a_merge(index: &FileIndex) {
        index.pause_merges_before_swap(true);
        index.hold_merges(false);
        assert!(
            index.wait_until_merge_paused(std::time::Duration::from_secs(20)),
            "no merge started"
        );
    }

    #[test]
    fn changes_arriving_during_a_merge_are_kept_and_queries_meanwhile_are_current() {
        let now = 20_000 * 86_400;
        // Three batches flushed: the merge takes the oldest segment and the
        // three after it, dropping their deletions and the tombstones they
        // carry. Four: it takes the four newest, above the oldest, and
        // carries their tombstones on.
        for flushed in [3, 4] {
            let fixture = fixture();
            let (index, mut model) = reference_index(&fixture, now, flushed);
            let before = index.stats().segments;
            start_a_merge(&index);
            assert_eq!(index.stats().segments, before, "not in place yet");
            assert_matches_reference(&index, &model, now, "as a merge starts");
            // Changes arrive while it runs, the first batch written as a
            // segment of its own, the next kept in memory: new versions of
            // entries the merge reads, files deleted, folders deleted.
            let batches = reference_changes(&fixture, now);
            for (at, batch) in batches[flushed..].iter().enumerate() {
                index.apply(batch).unwrap();
                model.apply(batch);
                if at == 0 {
                    index.flush().unwrap();
                }
                // A query during the merge answers from what is current.
                assert_matches_reference(&index, &model, now, "during a merge");
            }
            index.pause_merges_before_swap(false);
            index.wait_for_merges();
            // The run's MERGE_FACTOR segments are one, the one written
            // meanwhile is kept after it.
            assert_eq!(index.stats().segments, before - (MERGE_FACTOR - 1) + 1);
            {
                let state = read(&index.state);
                let merged = &state.segments[if flushed == 3 { 0 } else { 1 }];
                assert_eq!(
                    merged.tombstones.is_empty(),
                    flushed == 3,
                    "tombstones dropped only by a merge from the oldest segment"
                );
            }
            assert_matches_reference(&index, &model, now, "after the merge");
            // Nothing was lost on the way to the disk either.
            drop(index);
            let (index, opened) = fixture.open();
            assert_eq!(opened, Opened::Existing);
            assert_matches_reference(&index, &model, now, "opened again after the merge");
        }
    }

    /// The segments in `dir` its record does not list.
    fn unlisted_segments(dir: &Path) -> usize {
        let manifest: Manifest =
            serde_json::from_slice(&fs::read(dir.join(MANIFEST)).unwrap()).unwrap();
        fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                name.strip_suffix(".seg")?.parse::<u64>().ok()
            })
            .filter(|id| !manifest.segments.contains(id))
            .count()
    }

    #[test]
    fn a_restart_in_the_middle_of_a_merge_finds_every_change_through_the_log() {
        let now = 20_000 * 86_400;
        let fixture = fixture();
        let (index, mut model) = reference_index(&fixture, now, 3);
        start_a_merge(&index);
        // Changes arrive during the merge: one batch written as a segment,
        // the last only in the log.
        let batches = reference_changes(&fixture, now);
        index.apply(&batches[3]).unwrap();
        model.apply(&batches[3]);
        index.flush().unwrap();
        index.apply(&batches[4]).unwrap();
        model.apply(&batches[4]);
        // Pane stops here, the merged segment written but not yet in place:
        // what is on the disk now is what a restart finds.
        let stopped = fixture._dir.path().join("stopped mid-merge");
        fs::create_dir_all(&stopped).unwrap();
        for entry in fs::read_dir(&fixture.index_dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() != LOCK {
                fs::write(
                    stopped.join(entry.file_name()),
                    fs::read(entry.path()).unwrap(),
                )
                .unwrap();
            }
        }
        assert_eq!(unlisted_segments(&stopped), 1, "the merged segment");
        index.pause_merges_before_swap(false);
        drop(index);

        let (restarted, opened) =
            FileIndex::open(&stopped, std::slice::from_ref(&fixture.home)).unwrap();
        assert_eq!(opened, Opened::Existing);
        assert_matches_reference(&restarted, &model, now, "restarted mid-merge");
        restarted.hold_merges(true);
        assert_eq!(unlisted_segments(&stopped), 0, "the cut-short merge gone");
        restarted.hold_merges(false);
        restarted.wait_for_merges();
        assert_matches_reference(&restarted, &model, now, "merged after the restart");
    }
}
