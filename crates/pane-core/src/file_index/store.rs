//! The name index itself, in the shape of `minidex` (#126, "The engine"):
//! a memory table of recent changes backed by a write-ahead log, immutable
//! segments on disk each with its own term dictionary, tombstones that hide
//! a deleted folder's entries in older segments, and compaction that merges
//! the segments into one. Its files are Pane's own, versioned by
//! [`FORMAT_VERSION`]: an index of another version is rebuilt, never read.
//!
//! One writer at a time changes the index (its writer lock); queries read
//! it at the same time, never waiting for a segment to be written or
//! merged, only for the moment its result replaces what was there.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::{self, File};
use std::io;
use std::ops::{Bound, ControlFlow};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::Entry;
use super::format::{EntryKind, FORMAT_VERSION, Meta, SEPARATOR, key_path, path_key};
use super::journal::JournalCursor;
use super::segment::{self, Segment, Stored, Tombstone};
use super::terms::{
    FOLDER_TAG, NAME_TAG, Prepared, Roots, hint_day, hint_depth, hint_kind, terms_and_hint,
};
use super::text;
use super::wal::{self, Op, Wal};
use crate::atomic::{Readers, write_atomically};
use crate::util::lock;

/// Entries the memory table holds before they are written as a segment.
const MEMORY_TABLE_ENTRIES: usize = 1 << 16;
/// Entries a first index writes per segment, before merging them.
const BULK_SEGMENT_ENTRIES: usize = 1 << 17;
/// Segments beyond which they are merged into one.
const MAX_SEGMENTS: usize = 8;
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
/// newest version of each key, and each term's entries.
#[derive(Default)]
struct MemTable {
    entries: Vec<MemEntry>,
    by_key: BTreeMap<Vec<u8>, u32>,
    terms: BTreeMap<Vec<u8>, Vec<u32>>,
}

impl MemTable {
    fn insert(&mut self, seq: u64, prepared: Prepared) {
        let id = self.entries.len() as u32;
        if let Some(older) = self.by_key.insert(prepared.key.clone(), id) {
            self.entries[older as usize].newest = false;
        }
        for term in prepared.terms {
            self.terms.entry(term).or_default().push(id);
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
    /// anywhere (at its start too) to `all`: every term of the tag is
    /// looked at, so this is asked only when the words' starts find too
    /// few entries ([`FileIndex::search`]). Stops past
    /// [`SHORT_WORD_ENTRIES`] entries.
    fn word_inside(&self, tag: u8, word: &str, all: &mut Vec<u32>) {
        let needle = word.as_bytes();
        let holds = |term: &[u8]| {
            term.get(1..)
                .is_some_and(|rest| rest.windows(needle.len()).any(|part| part == needle))
        };
        match self {
            Source::Memory(table) => {
                let range = table.terms.range::<[u8], _>((
                    Bound::Included(&[tag][..]),
                    Bound::Excluded(&[tag + 1][..]),
                ));
                for (found, ids) in range {
                    if holds(found) {
                        all.extend_from_slice(ids);
                        if all.len() >= SHORT_WORD_ENTRIES {
                            break;
                        }
                    }
                }
            }
            Source::Segment(segment) => {
                segment.terms_with_prefix(&[tag], |found, offset| {
                    if holds(found) {
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

struct State {
    memory: MemTable,
    /// The memory table being written as a segment, still searched.
    frozen: Option<Arc<MemTable>>,
    /// Oldest first; each newer segment's entries are newer than all of an
    /// older one's.
    segments: Vec<Arc<Segment>>,
    /// Every tombstone not yet merged away.
    tombstones: Vec<Tombstone>,
    /// The tombstones the next segment written carries.
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
    /// version.
    fn is_current(&self, sources: &[Source<'_>], at: usize, key: &[u8], seq: u64) -> bool {
        if self
            .tombstones
            .iter()
            .any(|tombstone| tombstone.covers(key, seq))
        {
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
    state: RwLock<State>,
    writer: Mutex<Writer>,
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
        let mut tombstones: Vec<Tombstone> = loaded
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
        let index = FileIndex {
            dir: dir.to_path_buf(),
            roots,
            _lock: lock_file,
            state: RwLock::new(State {
                memory,
                frozen: None,
                segments: loaded.segments,
                tombstones,
                unflushed_tombstones: unflushed,
            }),
            writer: Mutex::new(Writer {
                wal,
                next_seq: max_seq + 1,
                next_file: next_file + 1,
                record: loaded.record,
                garbage: Vec::new(),
            }),
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
    /// segment is written. Other changes wait until it is finished.
    pub fn bulk(&self) -> Result<Bulk<'_>, IndexError> {
        let mut writer = lock(&self.writer);
        // The memory table's entries are older than the segments to come.
        self.flush_locked(&mut writer)?;
        Ok(Bulk {
            index: self,
            writer,
            buffer: Vec::new(),
        })
    }

    /// Writes the memory table as a segment, and merges the segments if
    /// there are too many.
    pub fn flush(&self) -> Result<(), IndexError> {
        let mut writer = lock(&self.writer);
        self.flush_locked(&mut writer)
    }

    /// Merges every segment and the memory table into one segment, dropping
    /// superseded versions, deletions and what tombstones hide.
    pub fn compact(&self) -> Result<(), IndexError> {
        let mut writer = lock(&self.writer);
        self.compact_locked(&mut writer)
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
        let prepared = text::Prepared::new(query.text);
        if prepared.words.is_empty() || query.limit == 0 {
            return Vec::new();
        }
        let wanted = query.offset.saturating_add(query.limit);
        let cap = CANDIDATES.max(wanted.saturating_mul(4));
        let now = now_seconds();
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
    pub fn folder_ids(&self) -> HashMap<u64, PathBuf> {
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
        self.dir.join(format!("{id}.seg"))
    }

    fn save_manifest(&self, writer: &Writer) -> Result<(), IndexError> {
        let manifest = Manifest {
            format: FORMAT_VERSION,
            segments: read(&self.state).segments.iter().map(|s| s.id).collect(),
            next_file: writer.next_file,
            record: writer.record.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?;
        write_atomically(&self.dir.join(MANIFEST), &bytes, Readers::OwnerOnly)?;
        Ok(())
    }

    fn maintain_locked(&self, writer: &mut Writer) -> Result<(), IndexError> {
        let full = read(&self.state).memory.entries.len() >= MEMORY_TABLE_ENTRIES;
        if full {
            self.flush_locked(writer)?;
        }
        if read(&self.state).segments.len() > MAX_SEGMENTS {
            self.compact_locked(writer)?;
        }
        writer
            .garbage
            .retain(|path| fs::remove_file(path).is_err() && path.exists());
        Ok(())
    }

    /// Writes the memory table as a new segment and starts a new log.
    fn flush_locked(&self, writer: &mut Writer) -> Result<(), IndexError> {
        let (frozen, carried, tombstones) = {
            let mut state = write(&self.state);
            if state.memory.is_empty() && state.unflushed_tombstones.is_empty() {
                return Ok(());
            }
            let frozen = Arc::new(std::mem::take(&mut state.memory));
            state.frozen = Some(frozen.clone());
            let carried = std::mem::take(&mut state.unflushed_tombstones);
            (frozen, carried, state.tombstones.clone())
        };
        let wal_id = writer.next_file;
        let segment_id = writer.next_file + 1;
        writer.next_file += 2;
        let new_wal = Wal::create(self.dir.join(format!("{wal_id}.wal")))?;
        let old_wal = std::mem::replace(&mut writer.wal, new_wal);
        let docs = frozen.by_key.values().filter_map(|&id| {
            let entry = &frozen.entries[id as usize];
            if tombstones
                .iter()
                .any(|tombstone| tombstone.covers(&entry.key, entry.seq))
            {
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
        }
        self.save_manifest(writer)?;
        let old_path = old_wal.path.clone();
        drop(old_wal);
        let _ = fs::remove_file(old_path);
        Ok(())
    }

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
        let path = self.segment_path(id);
        let merged = Merge::new(&segments).filter_map(|stored| {
            let meta = stored.meta?;
            if tombstones
                .iter()
                .any(|tombstone| tombstone.covers(&stored.key, stored.seq))
            {
                return None;
            }
            let (terms, hint) = terms_and_hint(&self.roots, &stored.key, Some(&meta));
            Some((
                stored.seq,
                Prepared {
                    key: stored.key,
                    meta: Some(meta),
                    terms,
                    hint,
                },
            ))
        });
        segment::write(&path, merged, &[])?;
        let merged = Arc::new(Segment::open(id, path)?);
        {
            let mut state = write(&self.state);
            state.segments = vec![merged];
            state.tombstones.clear();
        }
        self.save_manifest(writer)?;
        // Each old segment's map is released with its last reference;
        // Windows refuses to delete a mapped file, so one a query still
        // holds is deleted later.
        let old: Vec<PathBuf> = segments.iter().map(|s| s.path.clone()).collect();
        drop(segments);
        for path in old {
            if fs::remove_file(&path).is_err() && path.exists() {
                writer.garbage.push(path);
            }
        }
        Ok(())
    }
}

/// Entries going straight into segments; see [`FileIndex::bulk`].
pub struct Bulk<'a> {
    index: &'a FileIndex,
    writer: MutexGuard<'a, Writer>,
    buffer: Vec<(u64, Prepared)>,
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
/// it has `text::INSIDE_FROM` letters or more.
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
        let manifest = fixture.index_dir.join(MANIFEST);
        let text = fs::read_to_string(&manifest).unwrap();
        let other = text.replace(
            &format!("\"format\": {FORMAT_VERSION}"),
            &format!("\"format\": {}", FORMAT_VERSION + 1),
        );
        assert_ne!(text, other);
        fs::write(&manifest, other).unwrap();
        let (index, opened) = fixture.open();
        assert!(matches!(opened, Opened::Rebuilt(_)), "{opened:?}");
        assert!(index.search(&query("march")).is_empty());
        assert_eq!(index.record(), IndexRecord::default());
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

    #[test]
    fn many_segments_are_merged_into_one() {
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
        assert!(index.stats().segments <= MAX_SEGMENTS);
        assert_eq!(index.search(&query("file")).len(), MAX_SEGMENTS + 2);
    }
}
