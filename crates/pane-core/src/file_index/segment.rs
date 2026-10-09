//! A segment: an immutable, sorted run of the index's entries in one file,
//! read through a memory map.
//!
//! Layout, after the 16-byte header:
//!
//! - the entries, sorted by key, in blocks of [`BLOCK`]: each key shares a
//!   prefix with the one before it in its block (front coding), followed by
//!   its sequence number and metadata (or a deletion);
//! - each block's offset (`u32`), then each entry's hint (`u32`);
//! - the terms, in their order: each term itself, then its postings (how
//!   many entries have it, then their ids as gaps); a term's place in this
//!   order is its ordinal;
//! - the term dictionary, an `fst` map from term to its postings' offset;
//! - the prefix tombstones the segment carries for older segments;
//! - each term's offset (`u32`), by ordinal;
//! - the fragment lists: for each fragment of the terms' words
//!   (`terms::fragments`), how many terms have it, then their ordinals as
//!   gaps;
//! - the fragment table: each fragment (`u32`) and its list's offset
//!   (`u32`), sorted by fragment;
//! - the key filter: a Bloom filter of every key, which `find` asks first;
//! - a footer of fixed size giving where each part is, and the magic again.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fst::{IntoStreamer, Streamer};
use memmap2::Mmap;

use super::format::{
    self, FileKind, MAGIC, Meta, get_bytes, get_meta, get_varint, put_bytes, put_meta, put_varint,
    u32_at, u64_at,
};
use super::terms::{Prepared, fragments};

/// Entries per block: a lookup by key or id decodes at most this many.
pub(crate) const BLOCK: u32 = 16;

/// Footer: 21 numbers and the magic.
const FOOTER: usize = 21 * 8 + 8;

/// The key filter's bits per key, and the bits each key sets: about one
/// key in a hundred that a segment does not hold passes its filter.
const FILTER_BITS_PER_KEY: usize = 10;
const FILTER_HASHES: u64 = 7;

/// Everything under `prefix` written before `seq` is gone: a folder that
/// was deleted or renamed, in segments older than the one carrying it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tombstone {
    pub(crate) prefix: Vec<u8>,
    pub(crate) seq: u64,
}

impl Tombstone {
    pub(crate) fn covers(&self, key: &[u8], seq: u64) -> bool {
        seq < self.seq && key.starts_with(&self.prefix)
    }
}

/// One entry as a segment stores it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stored {
    pub(crate) key: Vec<u8>,
    pub(crate) seq: u64,
    /// `None` for a deletion, which hides older versions of its key.
    pub(crate) meta: Option<Meta>,
}

/// Writes the entries `docs` (sorted by key, each key once) and
/// `tombstones` to a new segment file at `path`, through a temporary file
/// renamed into place once it is on disk.
pub(crate) fn write(
    path: &Path,
    docs: impl Iterator<Item = (u64, Prepared)>,
    tombstones: &[Tombstone],
) -> io::Result<()> {
    write_unless(path, docs, tombstones, &AtomicBool::new(false))
}

/// [`write`], given up once `cancel` is set when `docs` ends (a merge cut
/// short ends its entries then): what was read is neither sorted nor put on
/// the disk, the temporary file is deleted, and the answer is an
/// [`io::ErrorKind::Interrupted`] error.
pub(crate) fn write_unless(
    path: &Path,
    docs: impl Iterator<Item = (u64, Prepared)>,
    tombstones: &[Tombstone],
    cancel: &AtomicBool,
) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    let written =
        write_to(&temporary, docs, tombstones, cancel).and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

fn write_to(
    path: &Path,
    docs: impl Iterator<Item = (u64, Prepared)>,
    tombstones: &[Tombstone],
    cancel: &AtomicBool,
) -> io::Result<()> {
    let given_up = || {
        if cancel.load(Ordering::Relaxed) {
            Err(io::Error::from(io::ErrorKind::Interrupted))
        } else {
            Ok(())
        }
    };
    let file = create_private(path)?;
    let mut out = Counting {
        inner: BufWriter::with_capacity(1 << 20, file),
        written: 0,
    };
    out.write_all(&format::header(FileKind::Segment))?;

    let docs_off = out.written;
    let mut block_offsets: Vec<u32> = Vec::new();
    let mut hints: Vec<u32> = Vec::new();
    let mut term_ids: HashMap<Vec<u8>, u32> = HashMap::new();
    let mut postings: Vec<Vec<u32>> = Vec::new();
    let mut previous: Vec<u8> = Vec::new();
    let mut key_hashes: Vec<u64> = Vec::new();
    let mut max_seq = 0u64;
    let mut record = Vec::new();
    for (id, (seq, doc)) in docs.enumerate() {
        let id = u32::try_from(id).map_err(|_| io::Error::other("too many entries"))?;
        if id % BLOCK == 0 {
            let offset = u32::try_from(out.written - docs_off)
                .map_err(|_| io::Error::other("a segment over 4 GB"))?;
            block_offsets.push(offset);
            previous.clear();
        }
        let shared = previous
            .iter()
            .zip(&doc.key)
            .take_while(|(a, b)| a == b)
            .count();
        record.clear();
        put_varint(&mut record, shared as u64);
        put_bytes(&mut record, &doc.key[shared..]);
        put_varint(&mut record, seq);
        put_meta(&mut record, doc.meta.as_ref());
        out.write_all(&record)?;
        hints.push(doc.hint);
        for term in doc.terms {
            let next = postings.len() as u32;
            let term_id = *term_ids.entry(term).or_insert(next);
            if term_id == next {
                postings.push(Vec::new());
            }
            let list = &mut postings[term_id as usize];
            if list.last() != Some(&id) {
                list.push(id);
            }
        }
        key_hashes.push(key_hash(&doc.key));
        previous = doc.key;
        max_seq = max_seq.max(seq);
    }
    given_up()?;
    let docs_len = out.written - docs_off;

    let blocks_off = out.written;
    for offset in &block_offsets {
        out.write_all(&offset.to_le_bytes())?;
    }
    let hints_off = out.written;
    for hint in &hints {
        out.write_all(&hint.to_le_bytes())?;
    }
    let count = hints.len() as u64;
    drop(hints);

    let mut terms: Vec<(Vec<u8>, u32)> = term_ids.into_iter().collect();
    terms.sort_unstable();
    let postings_off = out.written;
    let mut dictionary = fst::MapBuilder::memory();
    let mut list_bytes = Vec::new();
    let mut term_offsets: Vec<u32> = Vec::with_capacity(terms.len());
    // Each fragment with the ordinal of a term having it, in the terms'
    // order.
    let mut fragment_terms: Vec<(u32, u32)> = Vec::new();
    for (ordinal, (term, term_id)) in terms.iter().enumerate() {
        let list = std::mem::take(&mut postings[*term_id as usize]);
        let start = out.written - postings_off;
        let offset = u32::try_from(start).map_err(|_| io::Error::other("a segment over 4 GB"))?;
        term_offsets.push(offset);
        list_bytes.clear();
        put_bytes(&mut list_bytes, term);
        dictionary
            .insert(term, start + list_bytes.len() as u64)
            .map_err(io::Error::other)?;
        put_ids(&mut list_bytes, &list);
        out.write_all(&list_bytes)?;
        let ordinal = u32::try_from(ordinal).map_err(|_| io::Error::other("too many terms"))?;
        for fragment in fragments(term) {
            fragment_terms.push((fragment, ordinal));
        }
    }
    drop(terms);
    let postings_len = out.written - postings_off;
    let dictionary = dictionary.into_inner().map_err(io::Error::other)?;
    let fst_off = out.written;
    out.write_all(&dictionary)?;
    let fst_len = dictionary.len() as u64;

    let tomb_off = out.written;
    let mut tomb_bytes = Vec::new();
    put_varint(&mut tomb_bytes, tombstones.len() as u64);
    for tombstone in tombstones {
        put_bytes(&mut tomb_bytes, &tombstone.prefix);
        put_varint(&mut tomb_bytes, tombstone.seq);
        max_seq = max_seq.max(tombstone.seq);
    }
    out.write_all(&tomb_bytes)?;
    let tomb_len = tomb_bytes.len() as u64;

    let ordinals_off = out.written;
    for offset in &term_offsets {
        out.write_all(&offset.to_le_bytes())?;
    }
    let term_count = term_offsets.len() as u64;
    drop(term_offsets);

    // Sorted by fragment, each fragment's terms in their order.
    fragment_terms.sort_unstable();
    let lists_off = out.written;
    let mut fragment_table: Vec<(u32, u32)> = Vec::new();
    let mut ordinals: Vec<u32> = Vec::new();
    for group in fragment_terms.chunk_by(|a, b| a.0 == b.0) {
        let offset = u32::try_from(out.written - lists_off)
            .map_err(|_| io::Error::other("a segment over 4 GB"))?;
        fragment_table.push((group[0].0, offset));
        ordinals.clear();
        ordinals.extend(group.iter().map(|&(_, ordinal)| ordinal));
        list_bytes.clear();
        put_ids(&mut list_bytes, &ordinals);
        out.write_all(&list_bytes)?;
    }
    let lists_len = out.written - lists_off;
    drop(fragment_terms);
    let fragments_off = out.written;
    for (fragment, offset) in &fragment_table {
        out.write_all(&fragment.to_le_bytes())?;
        out.write_all(&offset.to_le_bytes())?;
    }
    let fragment_count = fragment_table.len() as u64;

    let filter = key_filter(&key_hashes);
    let filter_off = out.written;
    out.write_all(&filter)?;
    let filter_len = filter.len() as u64;

    let footer = [
        docs_off,
        docs_len,
        blocks_off,
        block_offsets.len() as u64,
        hints_off,
        count,
        postings_off,
        postings_len,
        fst_off,
        fst_len,
        tomb_off,
        tomb_len,
        max_seq,
        ordinals_off,
        term_count,
        lists_off,
        lists_len,
        fragments_off,
        fragment_count,
        filter_off,
        filter_len,
    ];
    for value in footer {
        out.write_all(&value.to_le_bytes())?;
    }
    out.write_all(&MAGIC)?;
    let file = out.inner.into_inner().map_err(|error| error.into_error())?;
    given_up()?;
    file.sync_all()
}

/// Writes `ids` (sorted): how many, then the first and the gaps after it.
fn put_ids(out: &mut Vec<u8>, ids: &[u32]) {
    put_varint(out, ids.len() as u64);
    let mut last = 0u32;
    for (at, &id) in ids.iter().enumerate() {
        put_varint(out, u64::from(if at == 0 { id } else { id - last }));
        last = id;
    }
}

/// Appends the ids [`put_ids`] wrote at `at` in `bytes` to `out`, in
/// order, stopping where they cannot be read.
fn read_ids(bytes: &[u8], mut at: usize, out: &mut Vec<u32>) {
    let Some(count) = get_varint(bytes, &mut at) else {
        return;
    };
    read_gaps(bytes, at, count, |id| {
        out.push(id);
        true
    });
}

/// A key's hash for the key filter. It is Pane's own, so that a filter
/// written by one Pane is read alike by the next (`std`'s hasher may change
/// between Rust releases, the format version would not).
fn key_hash(key: &[u8]) -> u64 {
    const MULTIPLY: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut hash = (key.len() as u64).wrapping_mul(MULTIPLY);
    let (chunks, rest) = key.as_chunks::<8>();
    for chunk in chunks {
        hash ^= u64::from_le_bytes(*chunk);
        hash = hash.wrapping_mul(MULTIPLY).rotate_left(29);
    }
    let mut word = [0u8; 8];
    word[..rest.len()].copy_from_slice(rest);
    hash ^= u64::from_le_bytes(word);
    hash = hash.wrapping_mul(MULTIPLY);
    // SplitMix64's finish, so that every bit depends on every byte.
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94D0_49BB_1331_11EB);
    hash ^ (hash >> 31)
}

/// The bits of a key filter of `bits` bits that a key of hash `hash` sets.
fn filter_bits(hash: u64, bits: u64) -> impl Iterator<Item = u64> {
    let (first, step) = (hash & 0xFFFF_FFFF, (hash >> 32) | 1);
    (0..FILTER_HASHES).map(move |n| first.wrapping_add(n.wrapping_mul(step)) % bits)
}

/// A key filter of the keys whose hashes are `hashes`: at least 8 bytes.
fn key_filter(hashes: &[u64]) -> Vec<u8> {
    let bytes = (hashes.len() * FILTER_BITS_PER_KEY).div_ceil(8).max(8);
    let mut filter = vec![0u8; bytes];
    let bits = bytes as u64 * 8;
    for &hash in hashes {
        for bit in filter_bits(hash, bits) {
            filter[(bit / 8) as usize] |= 1u8 << (bit % 8);
        }
    }
    filter
}

/// A writer that counts what it wrote, for the offsets.
struct Counting<W> {
    inner: W,
    written: u64,
}

impl<W: Write> Counting<W> {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.inner.write_all(bytes)?;
        self.written += bytes.len() as u64;
        Ok(())
    }
}

/// A new file only the user can read (mode 0600 on Unix; on Windows the
/// index's folder, inside the user's own cache folder, decides).
pub(crate) fn create_private(path: &Path) -> io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Part of a segment's memory map, for the term dictionary.
#[derive(Clone)]
struct Slice {
    map: Arc<Mmap>,
    start: usize,
    end: usize,
}

impl AsRef<[u8]> for Slice {
    fn as_ref(&self) -> &[u8] {
        &self.map[self.start..self.end]
    }
}

/// A segment file, open for reading.
pub(crate) struct Segment {
    pub(crate) id: u64,
    pub(crate) path: PathBuf,
    map: Arc<Mmap>,
    dictionary: fst::Map<Slice>,
    docs: usize,
    docs_end: usize,
    blocks: usize,
    block_count: usize,
    hints: usize,
    count: u32,
    postings: usize,
    postings_end: usize,
    /// Each term's offset in the postings, by ordinal.
    ordinals: usize,
    term_count: usize,
    fragment_lists: usize,
    fragment_lists_end: usize,
    fragments: usize,
    fragment_count: usize,
    filter: usize,
    filter_len: usize,
    pub(crate) tombstones: Vec<Tombstone>,
    pub(crate) max_seq: u64,
}

fn corrupt(path: &Path, what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{}: {what}", path.display()),
    )
}

impl Segment {
    pub(crate) fn open(id: u64, path: PathBuf) -> io::Result<Segment> {
        let file = File::open(&path)?;
        // SAFETY: the index's own file, which nothing changes once it was
        // renamed into place (segments are immutable), in a folder the index
        // holds locked; a change from outside would only make reads fail
        // their bounds checks, never write memory.
        let map = Arc::new(unsafe { Mmap::map(&file)? });
        if !format::has_header(&map, FileKind::Segment) || map.len() < 16 + FOOTER {
            return Err(corrupt(&path, "not a segment of this format"));
        }
        let footer = map.len() - FOOTER;
        if map[map.len() - 8..] != MAGIC {
            return Err(corrupt(&path, "a torn segment"));
        }
        let field = |n: usize| -> io::Result<usize> {
            u64_at(&map, footer + n * 8)
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| corrupt(&path, "a bad footer"))
        };
        let (docs, docs_len, blocks, block_count, hints, count) = (
            field(0)?,
            field(1)?,
            field(2)?,
            field(3)?,
            field(4)?,
            field(5)?,
        );
        let (postings, postings_len, fst_off, fst_len, tomb_off, tomb_len) = (
            field(6)?,
            field(7)?,
            field(8)?,
            field(9)?,
            field(10)?,
            field(11)?,
        );
        let max_seq = u64_at(&map, footer + 12 * 8).unwrap_or(0);
        let (ordinals, term_count, fragment_lists, fragment_lists_len) =
            (field(13)?, field(14)?, field(15)?, field(16)?);
        let (fragments, fragment_count, filter, filter_len) =
            (field(17)?, field(18)?, field(19)?, field(20)?);
        let fits =
            |start: usize, len: usize| start.checked_add(len).is_some_and(|end| end <= footer);
        let fits_each = |start: usize, count: usize, size: usize| {
            count.checked_mul(size).is_some_and(|len| fits(start, len))
        };
        if !fits(docs, docs_len)
            || !fits(blocks, block_count * 4)
            || !fits(hints, count * 4)
            || !fits(postings, postings_len)
            || !fits(fst_off, fst_len)
            || !fits(tomb_off, tomb_len)
            || !fits_each(ordinals, term_count, 4)
            || !fits(fragment_lists, fragment_lists_len)
            || !fits_each(fragments, fragment_count, 8)
            || !fits(filter, filter_len)
            || filter_len == 0
            || block_count != count.div_ceil(BLOCK as usize)
        {
            return Err(corrupt(&path, "a footer pointing outside the file"));
        }
        let dictionary = fst::Map::new(Slice {
            map: map.clone(),
            start: fst_off,
            end: fst_off + fst_len,
        })
        .map_err(|error| corrupt(&path, &error.to_string()))?;
        if dictionary.len() != term_count {
            return Err(corrupt(&path, "a term table not matching its dictionary"));
        }
        let mut tombstones = Vec::new();
        let tomb_bytes = &map[tomb_off..tomb_off + tomb_len];
        let mut at = 0;
        let tomb_count =
            get_varint(tomb_bytes, &mut at).ok_or_else(|| corrupt(&path, "bad tombstones"))?;
        for _ in 0..tomb_count {
            let prefix =
                get_bytes(tomb_bytes, &mut at).ok_or_else(|| corrupt(&path, "bad tombstones"))?;
            let seq =
                get_varint(tomb_bytes, &mut at).ok_or_else(|| corrupt(&path, "bad tombstones"))?;
            tombstones.push(Tombstone {
                prefix: prefix.to_vec(),
                seq,
            });
        }
        Ok(Segment {
            id,
            path,
            dictionary,
            docs,
            docs_end: docs + docs_len,
            blocks,
            block_count,
            hints,
            count: u32::try_from(count).map_err(|_| io::Error::other("too many entries"))?,
            postings,
            postings_end: postings + postings_len,
            ordinals,
            term_count,
            fragment_lists,
            fragment_lists_end: fragment_lists + fragment_lists_len,
            fragments,
            fragment_count,
            filter,
            filter_len,
            tombstones,
            max_seq,
            map,
        })
    }

    pub(crate) fn len(&self) -> u32 {
        self.count
    }

    pub(crate) fn hint(&self, id: u32) -> u32 {
        u32_at(&self.map, self.hints + id as usize * 4).unwrap_or(0)
    }

    fn block_start(&self, block: usize) -> Option<usize> {
        let offset = u32_at(&self.map, self.blocks + block * 4)? as usize;
        Some(self.docs + offset)
    }

    /// Decodes the entry at `*at` (inside the docs), given the key before
    /// it in its block, moving past it.
    fn decode(&self, at: &mut usize, key: &mut Vec<u8>) -> Option<(u64, Option<Meta>)> {
        let docs = &self.map[..self.docs_end];
        let shared = usize::try_from(get_varint(docs, at)?).ok()?;
        let suffix = get_bytes(docs, at)?;
        if shared > key.len() {
            return None;
        }
        key.truncate(shared);
        key.extend_from_slice(suffix);
        let seq = get_varint(docs, at)?;
        let meta = get_meta(docs, at)?;
        Some((seq, meta))
    }

    /// The entry with id `id`.
    pub(crate) fn doc(&self, id: u32) -> Option<Stored> {
        if id >= self.count {
            return None;
        }
        let block = (id / BLOCK) as usize;
        let mut at = self.block_start(block)?;
        let mut key = Vec::new();
        for _ in block as u32 * BLOCK..id {
            self.decode(&mut at, &mut key)?;
        }
        let (seq, meta) = self.decode(&mut at, &mut key)?;
        Some(Stored { key, seq, meta })
    }

    /// Whether the segment may hold `key`, by its key filter: false only
    /// when it does not.
    fn may_hold(&self, key: &[u8]) -> bool {
        let filter = &self.map[self.filter..self.filter + self.filter_len];
        filter_bits(key_hash(key), filter.len() as u64 * 8)
            .all(|bit| filter[(bit / 8) as usize] & (1u8 << (bit % 8)) != 0)
    }

    /// The version of `key` this segment holds, if any. A key its filter
    /// rules out is not looked for.
    pub(crate) fn find(&self, key: &[u8]) -> Option<Stored> {
        if self.count == 0 || !self.may_hold(key) {
            return None;
        }
        // The last block whose first key is at most `key`.
        let first_key = |block: usize| -> Option<Vec<u8>> {
            let mut at = self.block_start(block)?;
            let mut first = Vec::new();
            self.decode(&mut at, &mut first)?;
            Some(first)
        };
        let (mut low, mut high) = (0usize, self.block_count);
        while low + 1 < high {
            let middle = (low + high) / 2;
            if first_key(middle)?.as_slice() <= key {
                low = middle;
            } else {
                high = middle;
            }
        }
        let mut at = self.block_start(low)?;
        let mut current = Vec::new();
        let first = low as u32 * BLOCK;
        let last = (first + BLOCK).min(self.count);
        for _ in first..last {
            let (seq, meta) = self.decode(&mut at, &mut current)?;
            match current.as_slice().cmp(key) {
                std::cmp::Ordering::Equal => {
                    return Some(Stored {
                        key: current,
                        seq,
                        meta,
                    });
                }
                std::cmp::Ordering::Greater => return None,
                std::cmp::Ordering::Less => {}
            }
        }
        None
    }

    /// Every entry, in key order.
    pub(crate) fn iter(&self) -> SegmentIter<'_> {
        SegmentIter {
            segment: self,
            next: 0,
            at: self.docs,
            key: Vec::new(),
        }
    }

    /// The entries in key order from the block holding `key` (or the first
    /// key after it) on: entries before `key` in that block come first, for
    /// the caller to skip.
    pub(crate) fn iter_from(&self, key: &[u8]) -> SegmentIter<'_> {
        let first_key = |block: usize| -> Option<Vec<u8>> {
            let mut at = self.block_start(block)?;
            let mut first = Vec::new();
            self.decode(&mut at, &mut first)?;
            Some(first)
        };
        let (mut low, mut high) = (0usize, self.block_count);
        while low + 1 < high {
            let middle = (low + high) / 2;
            match first_key(middle) {
                Some(first) if first.as_slice() <= key => low = middle,
                _ => high = middle,
            }
        }
        match self.block_start(low) {
            Some(at) if self.count > 0 => SegmentIter {
                segment: self,
                next: low as u32 * BLOCK,
                at,
                key: Vec::new(),
            },
            _ => SegmentIter {
                segment: self,
                next: self.count,
                at: self.docs,
                key: Vec::new(),
            },
        }
    }

    /// Calls `visit` with each term starting with `prefix` (the exact one
    /// first, if it is a term) and its postings' offset, until it breaks.
    pub(crate) fn terms_with_prefix(
        &self,
        prefix: &[u8],
        mut visit: impl FnMut(&[u8], u64) -> ControlFlow<()>,
    ) {
        if let Some(offset) = self.dictionary.get(prefix)
            && visit(prefix, offset).is_break()
        {
            return;
        }
        let mut upper = prefix.to_vec();
        upper.push(0xFF);
        let mut stream = self.dictionary.range().gt(prefix).lt(&upper).into_stream();
        while let Some((term, offset)) = stream.next() {
            if visit(term, offset).is_break() {
                return;
            }
        }
    }

    /// Appends the ids of the postings at `offset` to `out`, in order.
    pub(crate) fn postings_into(&self, offset: u64, out: &mut Vec<u32>) {
        let bytes = &self.map[..self.postings_end];
        let Some(at) = usize::try_from(offset)
            .ok()
            .and_then(|o| o.checked_add(self.postings))
        else {
            return;
        };
        read_ids(bytes, at, out);
    }

    /// The term of ordinal `ordinal` (its place in the dictionary's
    /// order) and its postings' offset.
    pub(crate) fn term(&self, ordinal: u32) -> Option<(&[u8], u64)> {
        if ordinal as usize >= self.term_count {
            return None;
        }
        let relative = u32_at(&self.map, self.ordinals + ordinal as usize * 4)? as usize;
        let bytes = &self.map[..self.postings_end];
        let mut at = self.postings.checked_add(relative)?;
        let term = get_bytes(bytes, &mut at)?;
        Some((term, (at - self.postings) as u64))
    }

    /// The ordinals of the terms having every fragment of `wanted` (as
    /// `terms::fragments` gives them), in the dictionary's order. Every
    /// term holding a word is among them, with any term having its
    /// fragments apart ("abcxbcd" for "abcd").
    pub(crate) fn terms_with_fragments(&self, wanted: &[u32]) -> Vec<u32> {
        let bytes = &self.map[..self.fragment_lists_end];
        let mut lists: Vec<(u64, usize)> = Vec::with_capacity(wanted.len());
        for &fragment in wanted {
            let Some(mut at) = self.fragment_list(fragment) else {
                return Vec::new();
            };
            let Some(count) = get_varint(bytes, &mut at) else {
                return Vec::new();
            };
            lists.push((count, at));
        }
        // The shortest list first; the others only keep what it found.
        lists.sort_unstable();
        let Some((&(count, at), rest)) = lists.split_first() else {
            return Vec::new();
        };
        let mut found = Vec::with_capacity(count.min(1 << 16) as usize);
        read_gaps(bytes, at, count, |ordinal| {
            found.push(ordinal);
            true
        });
        for &(count, at) in rest {
            if found.is_empty() {
                break;
            }
            let mut kept = Vec::with_capacity(found.len());
            let mut next = 0;
            read_gaps(bytes, at, count, |ordinal| {
                while next < found.len() && found[next] < ordinal {
                    next += 1;
                }
                if next < found.len() && found[next] == ordinal {
                    kept.push(ordinal);
                    next += 1;
                }
                next < found.len()
            });
            found = kept;
        }
        found
    }

    /// Where the list of terms having `fragment` starts, if any has it.
    fn fragment_list(&self, fragment: u32) -> Option<usize> {
        let (mut low, mut high) = (0usize, self.fragment_count);
        while low < high {
            let middle = low + (high - low) / 2;
            let record = self.fragments + middle * 8;
            match u32_at(&self.map, record)?.cmp(&fragment) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => {
                    let offset = u32_at(&self.map, record + 4)? as usize;
                    return self.fragment_lists.checked_add(offset);
                }
            }
        }
        None
    }
}

/// Calls `each` with the `count` ids written as gaps from `at` in `bytes`
/// (after their count), in order, while it answers true.
fn read_gaps(bytes: &[u8], mut at: usize, count: u64, mut each: impl FnMut(u32) -> bool) {
    let mut id = 0u32;
    for n in 0..count {
        let Some(gap) = get_varint(bytes, &mut at) else {
            return;
        };
        id = if n == 0 {
            gap as u32
        } else {
            id.wrapping_add(gap as u32)
        };
        if !each(id) {
            return;
        }
    }
}

/// The entries of a segment in key order.
pub(crate) struct SegmentIter<'a> {
    segment: &'a Segment,
    next: u32,
    at: usize,
    key: Vec<u8>,
}

impl Iterator for SegmentIter<'_> {
    type Item = Stored;

    fn next(&mut self) -> Option<Stored> {
        if self.next >= self.segment.count {
            return None;
        }
        if self.next.is_multiple_of(BLOCK) {
            self.key.clear();
        }
        let (seq, meta) = self.segment.decode(&mut self.at, &mut self.key)?;
        self.next += 1;
        Some(Stored {
            key: self.key.clone(),
            seq,
            meta,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_index::format::EntryKind;
    use crate::file_index::terms::{FOLDER_TAG, NAME_TAG, Roots};

    fn meta(n: u64) -> Meta {
        Meta {
            kind: EntryKind::File,
            size: n,
            modified: 1_700_000_000 + n,
            file_id: n,
            volume: 1,
        }
    }

    /// A write given up (a merge cut short as the index closes, #187)
    /// leaves no file: neither the segment nor its temporary file.
    #[test]
    fn a_write_given_up_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let roots = Roots::new(&[&home]);
        let entry = Prepared::entry(&roots, &home.join("plan.txt"), meta(1));
        let docs = std::iter::once((1, entry));
        let path = dir.path().join("7.seg");
        let cancel = AtomicBool::new(true);
        let written = write_unless(&path, docs, &[], &cancel);
        assert_eq!(written.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert!(!path.exists());
        assert!(!path.with_extension("tmp").exists());
    }

    fn written(count: u64) -> (tempfile::TempDir, Segment, Vec<PathBuf>) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let roots = Roots::new(&[&home]);
        let mut docs: Vec<(u64, Prepared)> = (0..count)
            .map(|n| {
                let path = home
                    .join(format!("folder {}", n % 7))
                    .join(format!("file {n}.txt"));
                (n + 1, Prepared::entry(&roots, &path, meta(n)))
            })
            .collect();
        docs.push((
            count + 1,
            Prepared::deletion(crate::file_index::format::path_key(&home.join("zz gone"))),
        ));
        docs.sort_by(|a, b| a.1.key.cmp(&b.1.key));
        let paths = docs
            .iter()
            .map(|(_, doc)| crate::file_index::format::key_path(&doc.key))
            .collect();
        let file = dir.path().join("1.seg");
        let tombstones = [Tombstone {
            prefix: b"old/".to_vec(),
            seq: 3,
        }];
        write(&file, docs.into_iter(), &tombstones).unwrap();
        (dir, Segment::open(1, file).unwrap(), paths)
    }

    #[test]
    fn entries_are_found_by_id_by_key_and_in_order() {
        let (_dir, segment, paths) = written(100);
        assert_eq!(segment.len(), 101);
        assert_eq!(segment.max_seq, 101);
        assert_eq!(segment.tombstones.len(), 1);
        let all: Vec<Stored> = segment.iter().collect();
        assert_eq!(all.len(), 101);
        for (id, (stored, path)) in all.iter().zip(&paths).enumerate() {
            assert_eq!(&crate::file_index::format::key_path(&stored.key), path);
            assert_eq!(segment.doc(id as u32).as_ref(), Some(stored));
            assert_eq!(segment.find(&stored.key).as_ref(), Some(stored));
        }
        assert_eq!(all.last().unwrap().meta, None);
        assert_eq!(segment.find(b"no such key"), None);
        assert_eq!(segment.doc(101), None);
    }

    #[test]
    fn terms_lead_to_their_entries() {
        let (_dir, segment, _) = written(40);
        let mut term = vec![NAME_TAG];
        term.extend_from_slice(b"1");
        let mut ids = Vec::new();
        let mut terms = Vec::new();
        segment.terms_with_prefix(&term, |found, offset| {
            terms.push(found.to_vec());
            segment.postings_into(offset, &mut ids);
            ControlFlow::Continue(())
        });
        // "1" exactly, then 10 to 19.
        assert_eq!(terms.len(), 11);
        assert_eq!(terms[0], term);
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 11);
        for id in ids {
            let name = crate::file_index::format::key_path(&segment.doc(id).unwrap().key);
            assert!(
                name.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("file 1")
            );
        }
    }

    #[test]
    fn each_terms_ordinal_is_its_place_in_the_dictionary() {
        let (_dir, segment, _) = written(60);
        let mut listed = Vec::new();
        segment.terms_with_prefix(b"", |term, offset| {
            listed.push((term.to_vec(), offset));
            ControlFlow::Continue(())
        });
        assert!(listed.len() > 60);
        for (ordinal, (term, offset)) in listed.iter().enumerate() {
            let found = segment.term(ordinal as u32);
            assert_eq!(found, Some((term.as_slice(), *offset)));
        }
        assert_eq!(segment.term(listed.len() as u32), None);
    }

    #[test]
    fn fragments_lead_to_the_terms_that_may_hold_a_word() {
        let (_dir, segment, _) = written(200);
        let having = |tag: u8, word: &str| -> Vec<Vec<u8>> {
            let mut term = vec![tag];
            term.extend_from_slice(word.as_bytes());
            segment
                .terms_with_fragments(&fragments(&term))
                .into_iter()
                .map(|ordinal| segment.term(ordinal).unwrap().0.to_vec())
                .collect()
        };
        assert_eq!(having(NAME_TAG, "ile"), [b"nfile".to_vec()]);
        assert_eq!(having(FOLDER_TAG, "olde"), [b"ffolder".to_vec()]);
        assert_eq!(having(NAME_TAG, "199"), [b"n199".to_vec()]);
        // Shorter than a fragment: nothing to look up.
        assert!(having(NAME_TAG, "19").is_empty());
        // Another tag's terms are not looked at.
        assert!(having(NAME_TAG, "olde").is_empty());
        assert!(having(NAME_TAG, "xyz").is_empty());
    }

    #[test]
    fn the_key_filter_passes_every_key_held_and_few_others() {
        let (_dir, segment, paths) = written(1000);
        for path in &paths {
            let key = crate::file_index::format::path_key(path);
            assert!(segment.may_hold(&key));
            assert!(segment.find(&key).is_some());
        }
        let passed = (0..1000)
            .filter(|n| segment.may_hold(format!("absent {n}").as_bytes()))
            .count();
        assert!(passed < 50, "{passed} of 1,000 absent keys passed");
    }

    #[test]
    fn a_torn_or_foreign_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("2.seg");
        fs::write(&file, b"PANEFIX\0not really").unwrap();
        assert!(Segment::open(2, file).is_err());
    }
}
