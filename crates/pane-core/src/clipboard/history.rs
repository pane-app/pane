//! The clipboard history file, `clipboard-history.json`: every package's
//! history under its package identity's key, typed and versioned.
//!
//! ```json
//! { "version": 1, "packages": { "local:/…": {
//!     "capture": "on",
//!     "excluded": ["keepass.exe"],
//!     "retentionSeconds": 86400,
//!     "items": [{ "id": 7, "text": "hello", "copiedAt": 1790000000000, "source": "notepad.exe" }],
//!     "nextId": 8 } } }
//! ```
//!
//! Changes are made in memory under the store's lock and written after it
//! is released, so a copy never waits for another's write while holding it,
//! and a write never holds up a capture. Writes are made one at a time, and
//! a write older than the last one written is skipped, so the file always
//! ends with the latest state. A change is on disk once the call that made
//! it returns; a crash before that loses it (the file is replaced
//! atomically, so it holds the state before or after, never a torn one).
//!
//! Items expire: each is kept for its package's retention after it was
//! copied ([`PackageHistory::retention`]), by the store's [`Clock`]. Every
//! read and change first removes the expired items of every package (and
//! writes the file without them), so nothing expired is ever shown, counted
//! or kept again, whether the package ran meanwhile or not; and while Pane
//! runs, a thread of its own removes them when they expire
//! ([`HistoryStore::keep_expiring`]).
//!
//! Pane's own Clipboard History also keeps copied images and files (#167).
//! An image item names its PNG by the PNG's SHA-256; the PNG is kept in
//! `clipboard-images/<owner>/<digest>.png` beside the file (the owner's
//! folder named as its web images are), readable by the user only, as the
//! file is. It is written before the item is, and every write of the file
//! then deletes the PNGs no item names any more
//! ([`HistoryStore::prune_images`]): those of items that expired, were
//! deleted or cleared, or dropped past [`MAX_ITEMS`], and an owner's whole
//! folder once its history is removed. A files item keeps only the paths.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{
    CaptureState, Clock, CopiedImage, DEFAULT_RETENTION_SECONDS, MAX_EXCLUDED, MAX_ITEMS,
    MAX_RETENTION_SECONDS, MIN_RETENTION_SECONDS, ProgramName, SystemClock,
};
use crate::atomic::{Readers, write_atomically};
use crate::extension_data::Removal;

/// The history file's name, beside `installed.json`.
pub(crate) const FILE: &str = "clipboard-history.json";

/// The folder beside the file holding the PNGs of kept images, one folder
/// per owner.
pub(crate) const IMAGES_DIR: &str = "clipboard-images";

/// The version of the history file's format.
const VERSION: u64 = 1;

/// Whether the package whose identity key is `owner` records what is
/// copied before anyone turned its history on: only Pane's own Clipboard
/// History default extension does (ADR 0042, amending ADR 0020); every
/// other package's history is off until the package turns it on. Its
/// history, while the file holds none for it, is on (and nothing more);
/// one the user turned off, or paused, stays so, since the file then
/// holds it (see [`HistoryStore`]).
pub(crate) fn records_by_default(owner: &str) -> bool {
    owner == default_owner()
}

/// The identity key of Pane's own Clipboard History default extension.
pub(crate) fn default_owner() -> String {
    // Its history records from the first start.
    crate::packages::PackageIdentity::default_extension(
        crate::launcher::clipboard_view::CLIPBOARD_HISTORY,
    )
    .key()
}

/// Whether the package whose identity key is `owner` keeps copied images
/// and files beside text: only Pane's own Clipboard History (#167). Every
/// other package keeps plain text only, as `wit/clipboard.wit` says.
pub(crate) fn keeps_images_and_files(owner: &str) -> bool {
    owner == default_owner()
}

/// The title of a copied image `width` × `height`: "Image (1920×1080)".
pub fn image_title(width: u32, height: u32) -> String {
    format!("Image ({width}×{height})")
}

/// The history of `owner` while the file holds none for it: recording,
/// for Pane's own Clipboard History; otherwise off and empty.
pub(crate) fn fresh(owner: &str) -> PackageHistory {
    PackageHistory {
        capture: if records_by_default(owner) {
            CaptureState::On
        } else {
            CaptureState::Off
        },
        ..PackageHistory::default()
    }
}

/// Whether the file need not keep `history` of `owner` at all: it is what
/// the owner's history is while the file holds none ([`fresh`]) and no
/// item was ever kept, so no id could be given twice once it is gone. A
/// Clipboard History turned off is kept (as an entry with no capture), so
/// it does not record again at the next start.
fn forgettable(owner: &str, history: &PackageHistory) -> bool {
    history.items.is_empty()
        && history.next_id == 0
        && history.excluded.is_empty()
        && history.retention_seconds.is_none()
        && history.capture == fresh(owner).capture
}

/// One kept copy: a text, an image or a list of files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// Identifies it among the package's items; later items have greater
    /// ids.
    pub id: u64,
    /// The text copied; for an image its title ([`image_title`]), for
    /// files their paths, one per line: what a command lists of it
    /// through `entries` (`wit/clipboard.wit`).
    pub text: String,
    /// The image copied, if it is one (#167).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<StoredImage>,
    /// The files copied, by their paths, if it is a list of files (#167).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<PathBuf>,
    /// When it was copied, in milliseconds since the Unix epoch.
    pub copied_at: u64,
    /// The program it was copied from, if the system said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl Item {
    /// Whether it holds what `other` holds: the same text, image or files.
    fn same_copy(&self, other: &Item) -> bool {
        self.text == other.text
            && self.files == other.files
            && self.image.as_ref().map(|image| &image.digest)
                == other.image.as_ref().map(|image| &image.digest)
    }
}

/// A kept image: its size, and its PNG's size and SHA-256, which names the
/// PNG's file ([`HistoryStore::image_path`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredImage {
    pub width: u32,
    pub height: u32,
    /// How many bytes its PNG has.
    pub bytes: u64,
    /// Its PNG's SHA-256, in lowercase hex.
    pub digest: String,
}

/// One package's clipboard history.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PackageHistory {
    #[serde(default, skip_serializing_if = "CaptureState::is_off")]
    pub capture: CaptureState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<ProgramName>,
    /// How long each item is kept after it was copied, in seconds, if the
    /// user chose; otherwise [`DEFAULT_RETENTION_SECONDS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retention_seconds: Option<u64>,
    /// Newest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Item>,
    /// The id the next item gets.
    #[serde(default, skip_serializing_if = "is_zero")]
    next_id: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl PackageHistory {
    /// Whether nothing is kept: no items, and every choice as it starts.
    pub fn is_empty(&self) -> bool {
        !self.has_choices() && self.items.is_empty()
    }

    /// Whether the user chose anything for it: keeping history, excluded
    /// programs or how long items are kept.
    pub fn has_choices(&self) -> bool {
        !self.capture.is_off() || !self.excluded.is_empty() || self.retention_seconds.is_some()
    }

    /// How long each item is kept after it was copied, in seconds: within
    /// [`MIN_RETENTION_SECONDS`] and [`MAX_RETENTION_SECONDS`] however the
    /// file was written, so history is always finite.
    pub fn retention(&self) -> u64 {
        self.retention_seconds
            .unwrap_or(DEFAULT_RETENTION_SECONDS)
            .clamp(MIN_RETENTION_SECONDS, MAX_RETENTION_SECONDS)
    }

    /// Keeps each item `seconds` after it was copied from now on, or says
    /// why not: at least [`MIN_RETENTION_SECONDS`] and at most
    /// [`MAX_RETENTION_SECONDS`], so history is always finite. Items already
    /// older than that expire at once; none that expired comes back.
    pub fn set_retention(&mut self, seconds: u64) -> Result<(), String> {
        if !(MIN_RETENTION_SECONDS..=MAX_RETENTION_SECONDS).contains(&seconds) {
            return Err(format!(
                "Items are kept for at least {} minute and at most {} days",
                MIN_RETENTION_SECONDS / 60,
                MAX_RETENTION_SECONDS / 86_400
            ));
        }
        self.retention_seconds = Some(seconds);
        Ok(())
    }

    /// When `item` expires, in milliseconds since the Unix epoch.
    fn expires_at(&self, item: &Item) -> u64 {
        item.copied_at
            .saturating_add(self.retention().saturating_mul(1000))
    }

    /// Removes the items that expired by `now`; returns how many.
    pub fn expire(&mut self, now: u64) -> usize {
        self.keep_ids_used();
        let before = self.items.len();
        let items = std::mem::take(&mut self.items);
        self.items = items
            .into_iter()
            .filter(|item| self.expires_at(item) > now)
            .collect();
        before - self.items.len()
    }

    /// When the next item expires, if any is kept.
    fn next_expiry(&self) -> Option<u64> {
        self.items.iter().map(|item| self.expires_at(item)).min()
    }

    /// Removes the items whose ids are `ids`; returns how many there were.
    /// An id not kept (deleted, expired or never kept) is passed over.
    pub fn delete(&mut self, ids: &[u64]) -> usize {
        self.keep_ids_used();
        let before = self.items.len();
        self.items.retain(|item| !ids.contains(&item.id));
        before - self.items.len()
    }

    /// Keeps `text`, copied from `source` at `now`, as the newest item: an
    /// item with the same text moves to the front instead of being kept
    /// twice, and beyond [`MAX_ITEMS`] the oldest go.
    pub fn add(&mut self, text: &str, source: Option<&str>, now: u64) {
        self.keep(text.to_owned(), None, Vec::new(), source, now);
    }

    /// Keeps the image `image`, copied from `source` at `now`, as the
    /// newest item, as [`PackageHistory::add`] keeps text: the same image
    /// (by its PNG's digest) moves to the front.
    pub fn add_image(&mut self, image: StoredImage, source: Option<&str>, now: u64) {
        let title = image_title(image.width, image.height);
        self.keep(title, Some(image), Vec::new(), source, now);
    }

    /// Keeps the files `files`, copied from `source` at `now`, as the
    /// newest item, as [`PackageHistory::add`] keeps text: the same files,
    /// in the same order, move to the front.
    pub fn add_files(&mut self, files: &[PathBuf], source: Option<&str>, now: u64) {
        let text = files
            .iter()
            .map(|file| file.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        self.keep(text, None, files.to_vec(), source, now);
    }

    fn keep(
        &mut self,
        text: String,
        image: Option<StoredImage>,
        files: Vec<PathBuf>,
        source: Option<&str>,
        now: u64,
    ) {
        let mut item = Item {
            id: 0,
            text,
            image,
            files,
            copied_at: now,
            source: source.map(str::to_owned),
        };
        self.items.retain(|kept| !kept.same_copy(&item));
        let id = self
            .next_id
            .max(self.items.iter().map(|item| item.id + 1).max().unwrap_or(0));
        self.next_id = id + 1;
        item.id = id;
        self.items.insert(0, item);
        self.items.truncate(MAX_ITEMS);
    }

    /// Removes every item, keeping the capture state and the excluded
    /// programs; returns how many there were.
    pub fn clear(&mut self) -> usize {
        self.keep_ids_used();
        std::mem::take(&mut self.items).len()
    }

    /// Makes sure the next id is past every kept item's (a file written
    /// without `nextId` may hold items), before any of them goes, so that
    /// no id is ever given to a package twice.
    fn keep_ids_used(&mut self) {
        let past = self.items.iter().map(|item| item.id + 1).max().unwrap_or(0);
        self.next_id = self.next_id.max(past);
    }

    /// Replaces the excluded programs with `programs`, each once, in the
    /// order given; or why one is not a program's file name.
    pub fn set_excluded(&mut self, programs: &[String]) -> Result<(), String> {
        let mut kept: Vec<ProgramName> = Vec::new();
        for program in programs {
            let program = ProgramName::parse(program)?;
            if !kept.contains(&program) {
                kept.push(program);
            }
        }
        if kept.len() > MAX_EXCLUDED {
            return Err(format!("At most {MAX_EXCLUDED} programs can be excluded"));
        }
        self.excluded = kept;
        Ok(())
    }
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
struct HistoryJson {
    version: u64,
    packages: BTreeMap<String, PackageHistory>,
}

/// The longest the expiry thread waits before looking again, so that a
/// change of the system's time, or a computer waking from sleep, delays an
/// expiry on disk by at most this much (what is read is always expired).
const MAX_EXPIRY_WAIT: Duration = Duration::from_secs(3600);

/// The file as Pane last read or changed it.
struct State {
    file: Result<HistoryJson, String>,
    /// Counts changes, so that an older write is never made after a newer.
    changes: u64,
    /// Counts the times items were deleted (Clear, deleting items, turning
    /// history off and deleting it, uninstall): a capture that began
    /// reading before one keeps nothing (see [`Ticket`]). Expiry is not
    /// counted: what a capture keeps was copied after what expired.
    ///
    /// [`Ticket`]: super::Ticket
    deletions: u64,
}

/// Every package's clipboard history, kept in one file.
pub(crate) struct HistoryStore {
    path: PathBuf,
    /// The folder of the kept images' PNGs ([`IMAGES_DIR`]).
    images: PathBuf,
    /// The PNGs written for a capture still in progress, by their owner's
    /// folder and digest: not yet named by an item, and not to be pruned.
    pending_images: Mutex<HashSet<(String, String)>>,
    state: Mutex<State>,
    /// The number of the last change written, held while writing.
    written: Mutex<u64>,
    /// Tells when items expire.
    clock: Mutex<Arc<dyn Clock>>,
    /// Wakes the thread that removes expired items, if it runs.
    wake: Arc<Wake>,
}

/// A change made in memory, to be written once the store is unlocked.
pub(crate) struct Pending {
    change: u64,
    file: HistoryJson,
}

/// A package's change made in memory by [`HistoryStore::stage`], with its
/// answer, to be written by [`HistoryStore::write_staged`].
pub(crate) struct Staged<R> {
    answer: Result<R, String>,
    capture_changed: bool,
    /// Whether the change changed the package's history.
    changed: bool,
    /// The write of the change; or, when it failed or changed nothing, the
    /// write removing what expired meanwhile, if anything did.
    pending: Option<Pending>,
}

impl Drop for HistoryStore {
    fn drop(&mut self) {
        self.wake.stop();
    }
}

impl HistoryStore {
    /// Opens the history kept in `dir`, telling the time by the system's
    /// clock. Nothing is written until something is read or changed.
    pub fn open(dir: &Path) -> HistoryStore {
        let path = dir.join(FILE);
        let file = read(&path);
        HistoryStore {
            path,
            images: dir.join(IMAGES_DIR),
            pending_images: Mutex::new(HashSet::new()),
            state: Mutex::new(State {
                file,
                changes: 0,
                deletions: 0,
            }),
            written: Mutex::new(0),
            clock: Mutex::new(Arc::new(SystemClock)),
            wake: Arc::new(Wake::default()),
        }
    }

    /// Tells the time by `clock` from now on (tests and development builds
    /// replace the system's).
    #[cfg(any(test, debug_assertions))]
    pub fn set_clock(&self, clock: Arc<dyn Clock>) {
        let wake = Arc::downgrade(&self.wake);
        clock.on_change(Box::new(move || {
            if let Some(wake) = wake.upgrade() {
                wake.poke();
            }
        }));
        *self
            .clock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = clock;
        self.wake.poke();
    }

    /// Now, in milliseconds since the Unix epoch, by the store's clock.
    pub fn now(&self) -> u64 {
        let clock = self
            .clock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        clock.now()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn pending_images(&self) -> MutexGuard<'_, HashSet<(String, String)>> {
        self.pending_images
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The name of the folder of `owner`'s kept images, under
    /// [`IMAGES_DIR`]: named as its web images' folder is.
    fn image_folder(owner: &str) -> String {
        crate::icons::web_image_stem(owner)
    }

    /// Where the PNG of `owner`'s kept image with `digest` is.
    pub fn image_path(&self, owner: &str, digest: &str) -> PathBuf {
        self.images
            .join(HistoryStore::image_folder(owner))
            .join(format!("{digest}.png"))
    }

    /// Writes the PNG of `image`, copied for `owner`, where an item naming
    /// it finds it ([`HistoryStore::image_path`]), unless it is there
    /// already; until [`HistoryStore::release_image`], no write of the
    /// file deletes it. Called before the capture that keeps the item,
    /// with the store unlocked.
    pub fn keep_image(&self, owner: &str, image: &CopiedImage) -> io::Result<KeptImage> {
        use sha2::{Digest, Sha256};
        let digest: String = Sha256::digest(&image.png)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let folder = HistoryStore::image_folder(owner);
        self.pending_images()
            .insert((folder.clone(), digest.clone()));
        let path = self.image_path(owner, &digest);
        let mut created = false;
        if !path.is_file() {
            let written = fs::create_dir_all(self.images.join(&folder))
                .and_then(|()| write_atomically(&path, &image.png, Readers::OwnerOnly));
            if let Err(error) = written {
                self.pending_images().remove(&(folder, digest));
                return Err(error);
            }
            created = true;
        }
        Ok(KeptImage {
            owner: owner.to_owned(),
            folder,
            stored: StoredImage {
                width: image.width,
                height: image.height,
                bytes: image.png.len() as u64,
                digest,
            },
            created,
        })
    }

    /// Ends what [`HistoryStore::keep_image`] began: the PNG is pruned like
    /// any other from now on, and deleted at once if the capture did not
    /// keep it (`used` false) and no item named it before.
    pub fn release_image(&self, kept: KeptImage, used: bool) {
        self.pending_images()
            .remove(&(kept.folder.clone(), kept.stored.digest.clone()));
        if !used && kept.created {
            let _ = fs::remove_file(self.image_path(&kept.owner, &kept.stored.digest));
        }
    }

    /// Deletes the kept images' PNGs that no item of `file` names, except
    /// those of a capture in progress, and the folder of an owner left
    /// with none: run after each write of the file and each sweep, so an
    /// image goes with its item, however the item went (expired, deleted,
    /// cleared, dropped past [`MAX_ITEMS`], its history removed). A failure
    /// is reported on standard error; the next write tries again.
    fn prune_images(&self, file: &HistoryJson) {
        let Ok(folders) = fs::read_dir(&self.images) else {
            return;
        };
        let mut named: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
        for (owner, history) in &file.packages {
            let digests = named.entry(HistoryStore::image_folder(owner)).or_default();
            for item in &history.items {
                if let Some(image) = &item.image {
                    digests.insert(image.digest.as_str());
                }
            }
        }
        let pending = self.pending_images().clone();
        for folder in folders.flatten() {
            let name = folder.file_name().to_string_lossy().into_owned();
            let Ok(images) = fs::read_dir(folder.path()) else {
                continue;
            };
            // A folder a capture is writing into stays.
            let mut left = pending.iter().filter(|(of, _)| *of == name).count();
            for image in images.flatten() {
                let path = image.path();
                // A PNG being written (`write_atomically`'s hidden
                // temporary file) is not touched.
                if image.file_name().to_string_lossy().starts_with('.') {
                    left += 1;
                    continue;
                }
                let digest = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let kept = named
                    .get(&name)
                    .is_some_and(|digests| digests.contains(digest.as_str()))
                    || pending.contains(&(name.clone(), digest));
                if kept {
                    left += 1;
                } else if let Err(error) = fs::remove_file(&path) {
                    left += 1;
                    eprintln!(
                        "Pane could not delete a clipboard image no longer kept, {}: {error}",
                        path.display()
                    );
                }
            }
            if left == 0 {
                let _ = fs::remove_dir(folder.path());
            }
        }
    }

    /// The state with every expired item removed, and the write that
    /// removes them from the file too, if any expired.
    fn expired(&self) -> (MutexGuard<'_, State>, Option<Pending>) {
        let now = self.now();
        let mut state = self.lock();
        let pending = state.expire(now);
        (state, pending)
    }

    /// The history of `owner`, without the items that expired, or why it
    /// cannot be read.
    pub fn get(&self, owner: &str) -> Result<PackageHistory, String> {
        let (history, pending) = {
            let (state, pending) = self.expired();
            let file = state.file.as_ref().map_err(Clone::clone)?;
            (
                file.packages
                    .get(owner)
                    .cloned()
                    .unwrap_or_else(|| fresh(owner)),
                pending,
            )
        };
        self.write_logged(pending);
        Ok(history)
    }

    /// The owners whose capture is on: those the file says so of, and Pane's
    /// own Clipboard History while the file holds no history for it
    /// ([`records_by_default`]), whether or not it is installed (the watch
    /// also asks that the package runs).
    pub fn capturing_owners(&self) -> Vec<String> {
        let state = self.lock();
        let Ok(file) = &state.file else {
            return Vec::new();
        };
        let mut owners: Vec<String> = file
            .packages
            .iter()
            .filter(|(_, history)| history.capture == CaptureState::On)
            .map(|(owner, _)| owner.clone())
            .collect();
        let default = default_owner();
        if !file.packages.contains_key(&default) {
            owners.push(default);
        }
        owners
    }

    /// How many times items were deleted.
    pub fn deletions(&self) -> u64 {
        self.lock().deletions
    }

    /// Hands `change` the history of `owner`, without the items that
    /// expired, and makes what it changed (and what expired meanwhile, as
    /// under a retention it shortened) in memory, under the store's lock;
    /// [`HistoryStore::write_staged`] then writes it, before the change's
    /// call returns. Two steps, so that a guest's change is made while its
    /// runtime thread's fence is held and written once it is released (see
    /// `PackageData::update_clipboard_history`). Fails only when the file
    /// cannot be read.
    pub fn stage<R>(
        &self,
        owner: &str,
        change: impl FnOnce(&mut PackageHistory) -> Result<R, String>,
    ) -> Result<Staged<R>, String> {
        let now = self.now();
        let (mut state, expired) = self.expired();
        let file = state.file.as_ref().map_err(Clone::clone)?;
        let before = file
            .packages
            .get(owner)
            .cloned()
            .unwrap_or_else(|| fresh(owner));
        let mut history = before.clone();
        let answer = match change(&mut history) {
            Ok(answer) if history != before => answer,
            answer => {
                return Ok(Staged {
                    answer,
                    capture_changed: false,
                    changed: false,
                    pending: expired,
                });
            }
        };
        if history.items.len() < before.items.len() {
            state.deletions += 1;
        }
        history.expire(now);
        let capture_changed = history.capture != before.capture;
        let pending = state.change(|file| {
            if forgettable(owner, &history) {
                file.packages.remove(owner);
            } else {
                file.packages.insert(owner.to_owned(), history);
            }
        });
        Ok(Staged {
            answer: Ok(answer),
            capture_changed,
            changed: true,
            pending: Some(pending),
        })
    }

    /// Writes what [`HistoryStore::stage`] changed, if anything, before
    /// returning its answer and whether the capture state changed. What
    /// only expired is written as the expiry thread writes it: a failure
    /// is reported on standard error, not to the change.
    pub fn write_staged<R>(&self, staged: Staged<R>) -> Result<(R, bool), String> {
        if !staged.changed {
            self.write_logged(staged.pending);
            return staged.answer.map(|answer| (answer, false));
        }
        if let Some(pending) = staged.pending {
            self.write(pending)
                .map_err(|error| format!("Could not save the clipboard history: {error}"))?;
        }
        self.wake.poke();
        staged.answer.map(|answer| (answer, staged.capture_changed))
    }

    /// Changes the history of `owner` with `change` and writes it, as a
    /// command's change does in two steps ([`HistoryStore::stage`] and
    /// [`HistoryStore::write_staged`]).
    #[cfg(test)]
    pub fn update<R>(
        &self,
        owner: &str,
        change: impl FnOnce(&mut PackageHistory) -> Result<R, String>,
    ) -> Result<(R, bool), String> {
        self.write_staged(self.stage(owner, change)?)
    }

    /// Changes every history `change` asks to, given the time now, if items
    /// were not deleted since `deletions`, then writes them; a failed write
    /// is reported on standard error, never with what was copied.
    pub fn capture(
        &self,
        deletions: u64,
        change: impl FnOnce(&mut BTreeMap<String, PackageHistory>, u64) -> bool,
    ) {
        let now = self.now();
        let (pending, changed) = {
            let (mut state, expired) = self.expired();
            match &state.file {
                Ok(file) if state.deletions == deletions => {
                    let mut packages = file.packages.clone();
                    // Pane's own Clipboard History records while the file
                    // holds no history for it yet.
                    let default = default_owner();
                    if !packages.contains_key(&default) {
                        let history = fresh(&default);
                        packages.insert(default.clone(), history);
                    }
                    if change(&mut packages, now) {
                        // Kept only once it keeps something: a package that
                        // is not installed leaves no entry behind.
                        if packages
                            .get(&default)
                            .is_some_and(|history| forgettable(&default, history))
                        {
                            packages.remove(&default);
                        }
                        (Some(state.change(|file| file.packages = packages)), true)
                    } else {
                        (expired, false)
                    }
                }
                _ => (expired, false),
            }
        };
        self.write_logged(pending);
        if changed {
            self.wake.poke();
        }
    }

    /// Removes the history of `owner`, and nothing else; the file is read
    /// again first if it could not be read before, so one the user repaired
    /// is used without restarting Pane. On failure nothing is removed.
    pub fn remove(&self, owner: &str) -> Result<(), Removal> {
        let pending = {
            let mut state = self.lock();
            if state.file.is_err() {
                state.file = read(&self.path);
            }
            let file = state
                .file
                .as_ref()
                .map_err(|reason| Removal::Unreadable(reason.clone()))?;
            if !file.packages.contains_key(owner) {
                // No item names an image of its: none is kept either.
                let _ = fs::remove_dir_all(self.images.join(HistoryStore::image_folder(owner)));
                return Ok(());
            }
            state.deletions += 1;
            state.change(|file| {
                file.packages.remove(owner);
            })
        };
        self.write(pending)
            .map_err(|error| Removal::Unwritable(self.path.clone(), error))
    }

    /// How many unexpired items each owner keeps, read from the file now
    /// (an owner that keeps only its choices counts 0), or why it cannot be
    /// read.
    pub fn counts_now(&self) -> Result<BTreeMap<String, usize>, String> {
        let now = self.now();
        Ok(read(&self.path)?
            .packages
            .into_iter()
            .filter_map(|(owner, mut history)| {
                history.expire(now);
                (!history.is_empty()).then_some((owner, history.items.len()))
            })
            .collect())
    }

    /// Removes every expired item of every package, from the file too;
    /// returns when the next item expires, if any is kept.
    pub fn sweep(&self) -> Option<u64> {
        let (next, pending) = {
            let (state, pending) = self.expired();
            let next = state.file.as_ref().ok().and_then(|file| {
                file.packages
                    .values()
                    .filter_map(PackageHistory::next_expiry)
                    .min()
            });
            (next, pending)
        };
        self.write_logged(pending);
        // Images a capture left behind (Pane stopped between writing a PNG
        // and its item) go too.
        let file = self.lock().file.clone();
        if let Ok(file) = file {
            self.prune_images(&file);
        }
        next
    }

    /// Removes expired items on a thread of its own while this store is
    /// kept: whenever an item expires (or at least every
    /// [`MAX_EXPIRY_WAIT`]), whether its package runs or not. The thread
    /// ends once the store is dropped.
    pub fn keep_expiring(self: &Arc<Self>) {
        let store = Arc::downgrade(self);
        let wake = self.wake.clone();
        let started = std::thread::Builder::new()
            .name("pane-clipboard-expiry".into())
            .spawn(move || expire_until_dropped(&store, &wake));
        if let Err(error) = started {
            eprintln!("Pane cannot expire clipboard history in the background: {error}");
        }
    }

    /// Waits until the expiry thread swept after every change and clock
    /// change so far, and let go of the store; `false` if it did not within
    /// `limit` (it does not run, or never got to run). For tests, which so
    /// wait for expiry without timing it.
    #[cfg(any(test, debug_assertions))]
    pub fn wait_swept(&self, limit: Duration) -> bool {
        self.wake.wait_swept(limit)
    }

    /// Writes `pending`, if any, reporting a failure on standard error.
    fn write_logged(&self, pending: Option<Pending>) {
        if let Some(pending) = pending
            && let Err(error) = self.write(pending)
        {
            eprintln!(
                "Pane could not save the clipboard history in {}: {error}",
                self.path.display()
            );
        }
    }

    /// Writes `pending`, unless a newer change was written meanwhile.
    fn write(&self, pending: Pending) -> io::Result<()> {
        let mut written = self
            .written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.change <= *written {
            return Ok(());
        }
        let text = serde_json::to_string_pretty(&pending.file).map_err(io::Error::other)?;
        write_atomically(&self.path, text.as_bytes(), Readers::OwnerOnly)?;
        *written = pending.change;
        // The images no item written names any more go with it.
        self.prune_images(&pending.file);
        Ok(())
    }
}

/// A PNG [`HistoryStore::keep_image`] wrote (or found) for a capture in
/// progress, until [`HistoryStore::release_image`].
pub(crate) struct KeptImage {
    owner: String,
    folder: String,
    stored: StoredImage,
    /// Whether this capture wrote it, rather than finding it kept.
    created: bool,
}

impl KeptImage {
    /// The owner it was kept for.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// What an item keeping it says of it.
    pub fn stored(&self) -> &StoredImage {
        &self.stored
    }
}

/// The expiry thread: removes expired items, then waits until the next
/// expires, the history changes or the clock is changed, until the store
/// is dropped.
fn expire_until_dropped(store: &Weak<HistoryStore>, wake: &Wake) {
    loop {
        let Some(seen) = wake.pokes() else {
            return;
        };
        let Some(kept) = store.upgrade() else {
            return;
        };
        let next = kept.sweep();
        let now = kept.now();
        drop(kept);
        wake.swept(seen);
        let wait = next.map_or(MAX_EXPIRY_WAIT, |at| {
            Duration::from_millis(at.saturating_sub(now)).min(MAX_EXPIRY_WAIT)
        });
        if !wake.wait(seen, wait) {
            return;
        }
    }
}

/// Wakes the expiry thread when the time the next item expires may have
/// changed, and stops it.
#[derive(Default)]
struct Wake {
    state: Mutex<WakeState>,
    condvar: Condvar,
}

#[derive(Default)]
struct WakeState {
    stopped: bool,
    pokes: u64,
    /// The pokes seen before the thread's last sweep began, told once that
    /// sweep ended and the thread let go of the store (for tests).
    #[cfg_attr(not(any(test, debug_assertions)), allow(dead_code))]
    swept: u64,
}

impl Wake {
    fn lock(&self) -> MutexGuard<'_, WakeState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn poke(&self) {
        self.lock().pokes += 1;
        self.condvar.notify_all();
    }

    fn stop(&self) {
        self.lock().stopped = true;
        self.condvar.notify_all();
    }

    /// Notes that a sweep that began after `seen` pokes ended.
    fn swept(&self, seen: u64) {
        self.lock().swept = seen;
        self.condvar.notify_all();
    }

    /// Waits at most `limit` until a sweep begun after every poke so far
    /// ended; returns whether one did.
    #[cfg(any(test, debug_assertions))]
    fn wait_swept(&self, limit: Duration) -> bool {
        let state = self.lock();
        let target = state.pokes;
        let (state, _) = self
            .condvar
            .wait_timeout_while(state, limit, |state| !state.stopped && state.swept < target)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.swept >= target
    }

    /// How many times it was poked, or `None` once stopped.
    fn pokes(&self) -> Option<u64> {
        let state = self.lock();
        (!state.stopped).then_some(state.pokes)
    }

    /// Waits at most `limit` for a poke after the `seen`th; returns whether
    /// it is still running.
    fn wait(&self, seen: u64, limit: Duration) -> bool {
        let state = self.lock();
        let (state, _) = self
            .condvar
            .wait_timeout_while(state, limit, |state| !state.stopped && state.pokes == seen)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !state.stopped
    }
}

impl State {
    /// Applies `change` to the file in memory, returning it to be written.
    fn change(&mut self, change: impl FnOnce(&mut HistoryJson)) -> Pending {
        let file = self
            .file
            .as_mut()
            .expect("only a file that could be read is changed");
        change(file);
        self.changes += 1;
        Pending {
            change: self.changes,
            file: file.clone(),
        }
    }

    /// Removes the items that expired by `now` from every package (and a
    /// package left with nothing), returning the write that removes them
    /// from the file, if any expired.
    fn expire(&mut self, now: u64) -> Option<Pending> {
        let file = self.file.as_mut().ok()?;
        let mut expired = 0;
        file.packages.retain(|owner, history| {
            expired += history.expire(now);
            !forgettable(owner, history)
        });
        (expired > 0).then(|| self.change(|_| {}))
    }
}

/// Reads the history file; a missing file holds none.
fn read(path: &Path) -> Result<HistoryJson, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<HistoryJson>(&text)
            .map_err(|error| error.to_string())
            .and_then(|file| {
                if file.version == VERSION {
                    Ok(file)
                } else {
                    Err(format!(
                        "it has version {}, this Pane reads {VERSION}",
                        file.version
                    ))
                }
            }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(HistoryJson {
            version: VERSION,
            packages: BTreeMap::new(),
        }),
        Err(error) => Err(error.to_string()),
    }
    .map_err(|reason| format!("Cannot read {}: {reason}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::ManualClock;
    use serde_json::Value;

    const DAY: u64 = 86_400_000;

    /// The history file in `dir`, parsed.
    fn on_disk(dir: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(dir.join(FILE)).unwrap()).unwrap()
    }

    /// A store of `dir` whose clock shows `now` until advanced.
    fn store_at(dir: &Path, now: u64) -> (Arc<HistoryStore>, Arc<ManualClock>) {
        let store = Arc::new(HistoryStore::open(dir));
        let clock = ManualClock::at(now);
        store.set_clock(clock.clone());
        (store, clock)
    }

    fn texts(history: &PackageHistory) -> Vec<&str> {
        history
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect()
    }

    #[test]
    fn items_are_newest_first_once_per_text_and_bounded() {
        let mut history = PackageHistory::default();
        history.add("one", Some("notepad.exe"), 10);
        history.add("two", None, 20);
        assert_eq!(texts(&history), ["two", "one"]);
        assert_eq!(history.items[1].source.as_deref(), Some("notepad.exe"));
        assert_eq!(history.items[1].copied_at, 10);
        // Copying "one" again moves it to the front, with its new time.
        history.add("one", None, 30);
        assert_eq!(texts(&history), ["one", "two"]);
        assert_eq!(history.items[0].copied_at, 30);
        assert!(history.items[0].id > history.items[1].id);
        for number in 0..MAX_ITEMS {
            history.add(&format!("item {number}"), None, 40);
        }
        assert_eq!(history.items.len(), MAX_ITEMS);
        assert_eq!(history.items[0].text, format!("item {}", MAX_ITEMS - 1));
        assert!(!texts(&history).contains(&"two") && !texts(&history).contains(&"one"));
        // Ids are never reused, even after clearing.
        let last = history.items[0].id;
        history.clear();
        history.add("again", None, 50);
        assert!(history.items[0].id > last);
    }

    #[test]
    fn capture_state_and_exclusions_are_kept_apart_from_items() {
        let mut history = PackageHistory {
            capture: CaptureState::Paused,
            ..PackageHistory::default()
        };
        let programs = [
            "KeePass.exe".to_string(),
            "keepass.exe".into(),
            "Bitwarden.exe".into(),
        ];
        history.set_excluded(&programs).unwrap();
        let names: Vec<&str> = history.excluded.iter().map(ProgramName::as_str).collect();
        assert_eq!(names, ["keepass.exe", "bitwarden.exe"]);
        assert!(history.set_excluded(&["a/b".into()]).is_err());
        assert_eq!(history.excluded.len(), 2);
        history.add("kept", None, 1);
        assert_eq!(history.clear(), 1);
        assert_eq!(history.capture, CaptureState::Paused);
        assert_eq!(history.excluded.len(), 2);
        history.set_excluded(&[]).unwrap();
        history.capture = CaptureState::Off;
        assert!(history.is_empty());
    }

    #[test]
    fn the_file_is_typed_versioned_and_lowercases_excluded_programs() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(FILE),
            r#"{ "version": 1, "packages": { "local:/x": {
                "capture": "on", "excluded": ["KeePass.EXE", "1Password"],
                "items": [{ "id": 3, "text": "hi", "copiedAt": 5 }], "nextId": 4 } } }"#,
        )
        .unwrap();
        let (store, _) = store_at(dir.path(), 5);
        let history = store.get("local:/x").unwrap();
        assert_eq!(history.capture, CaptureState::On);
        let names: Vec<&str> = history.excluded.iter().map(ProgramName::as_str).collect();
        assert_eq!(names, ["keepass.exe", "1password"]);
        assert_eq!(history.items[0].text, "hi");

        fs::write(dir.path().join(FILE), r#"{ "version": 2, "packages": {} }"#).unwrap();
        let store = HistoryStore::open(dir.path());
        assert!(store.get("local:/x").unwrap_err().contains("version 2"));
        // Nothing overwrites a file of another version.
        assert!(store.update("local:/x", |_| Ok(())).is_err());
        assert!(
            fs::read_to_string(dir.path().join(FILE))
                .unwrap()
                .contains("\"version\": 2")
        );
    }

    /// ADR 0042: Pane's own Clipboard History records from the first start,
    /// with nothing in the file; turned off or paused, it stays so.
    #[test]
    fn panes_own_clipboard_history_records_until_it_is_turned_off() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = store_at(dir.path(), DAY);
        let own = default_owner();
        assert!(records_by_default(&own) && !records_by_default("a"));
        // Nothing written yet: it records, and the others do not.
        assert_eq!(store.get(&own).unwrap().capture, CaptureState::On);
        assert_eq!(store.get("a").unwrap().capture, CaptureState::Off);
        assert_eq!(store.capturing_owners(), std::slice::from_ref(&own));
        // A capture keeps the copy for it.
        store.capture(store.deletions(), |packages, now| {
            let history = packages.get_mut(&own).expect("its fresh history");
            assert_eq!(history.capture, CaptureState::On);
            history.add("first", Some("notepad.exe"), now);
            true
        });
        assert_eq!(texts(&store.get(&own).unwrap()), ["first"]);
        // A capture that keeps nothing for it leaves no entry for it.
        let other = tempfile::tempdir().unwrap();
        let (fresh_store, _) = store_at(other.path(), DAY);
        fresh_store.capture(fresh_store.deletions(), |packages, now| {
            packages
                .entry("a".into())
                .or_default()
                .add("a's", None, now);
            true
        });
        assert!(on_disk(other.path())["packages"].get(&own).is_none());

        // Turned off, it stays off: the file keeps saying so.
        store
            .update(&own, |history| {
                history.capture = CaptureState::Off;
                history.clear();
                Ok(())
            })
            .unwrap();
        let reopened = HistoryStore::open(dir.path());
        assert_eq!(reopened.get(&own).unwrap().capture, CaptureState::Off);
        assert!(!reopened.capturing_owners().contains(&own));
        // Paused, the same.
        store
            .update(&own, |history| {
                history.capture = CaptureState::Paused;
                Ok(())
            })
            .unwrap();
        let reopened = HistoryStore::open(dir.path());
        assert_eq!(reopened.get(&own).unwrap().capture, CaptureState::Paused);
        // Removed (uninstalled with its data), it records again.
        assert!(reopened.remove(&own).is_ok());
        assert_eq!(reopened.get(&own).unwrap().capture, CaptureState::On);
    }

    /// An image of `width` × `height` transparent pixels, as copied.
    fn image(width: u32, height: u32) -> CopiedImage {
        let pixels = vec![0u8; (width * height * 4) as usize];
        CopiedImage::from_png(crate::icons::encode_png(width, height, &pixels).unwrap()).unwrap()
    }

    /// Keeps `image` for Pane's own Clipboard History as a capture does;
    /// returns where its PNG is.
    fn capture_image(store: &HistoryStore, image: &CopiedImage) -> PathBuf {
        let own = default_owner();
        let kept = store.keep_image(&own, image).unwrap();
        let path = store.image_path(&own, &kept.stored().digest);
        let stored = kept.stored().clone();
        store.capture(store.deletions(), |packages, now| {
            packages
                .get_mut(&own)
                .expect("its fresh history")
                .add_image(stored, Some("mspaint.exe"), now);
            true
        });
        store.release_image(kept, true);
        path
    }

    /// #167: an image is kept as a PNG beside the file, named by its
    /// digest, and files by their paths; the PNG goes when its item
    /// expires, and one a capture did not keep goes at once.
    #[test]
    fn an_image_is_kept_beside_the_file_and_expires_with_its_item() {
        let dir = tempfile::tempdir().unwrap();
        let (store, clock) = store_at(dir.path(), DAY);
        let own = default_owner();
        let copied = image(2, 1);
        let path = capture_image(&store, &copied);
        assert!(path.starts_with(dir.path().join(IMAGES_DIR)));
        assert_eq!(fs::read(&path).unwrap(), copied.png);
        let history = store.get(&own).unwrap();
        assert_eq!(history.items[0].text, "Image (2×1)");
        let stored = history.items[0].image.clone().expect("an image item");
        assert_eq!((stored.width, stored.height), (2, 1));
        assert_eq!(stored.bytes, copied.png.len() as u64);
        // The file names the PNG; it does not hold it.
        let item = &on_disk(dir.path())["packages"][&own]["items"][0];
        assert_eq!(item["image"]["digest"], stored.digest.as_str());
        // The same image copied again moves to the front, kept once.
        capture_image(&store, &copied);
        assert_eq!(store.get(&own).unwrap().items.len(), 1);

        // Files, by their paths, in the order copied.
        let files = [PathBuf::from("/notes/a.txt"), PathBuf::from("/notes/b")];
        store
            .update(&own, |history| {
                history.add_files(&files, None, DAY);
                Ok(())
            })
            .unwrap();
        let items = store.get(&own).unwrap().items;
        assert_eq!(items[0].files, files);
        assert_eq!(items[0].text, "/notes/a.txt\n/notes/b");
        assert!(items[0].image.is_none() && items[1].files.is_empty());

        // Expired by the host's clock, the image's PNG goes with its item.
        clock.advance(Duration::from_secs(7 * 86_400));
        assert!(store.get(&own).unwrap().items.is_empty());
        assert!(!path.exists(), "the expired image's PNG is deleted");

        // A PNG a capture did not keep is deleted at once.
        let unused = store.keep_image(&own, &image(1, 1)).unwrap();
        let unused_path = store.image_path(&own, &unused.stored().digest);
        assert!(unused_path.is_file());
        store.release_image(unused, false);
        assert!(!unused_path.exists());
    }

    /// Deleting an image item, clearing the history or removing it (an
    /// uninstall that deletes its data) deletes the PNGs too; one left
    /// behind (Pane stopped between the PNG and its item) goes at the next
    /// sweep.
    #[test]
    fn an_image_goes_with_its_item_deleted_cleared_or_removed() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = store_at(dir.path(), DAY);
        let own = default_owner();
        let first = capture_image(&store, &image(1, 2));
        let second = capture_image(&store, &image(2, 2));
        let ids: Vec<u64> = store
            .get(&own)
            .unwrap()
            .items
            .iter()
            .map(|i| i.id)
            .collect();
        store
            .update(&own, |history| Ok(history.delete(&[ids[1]])))
            .unwrap();
        assert!(!first.exists() && second.is_file());
        store.update(&own, |history| Ok(history.clear())).unwrap();
        assert!(!second.exists());

        let third = capture_image(&store, &image(3, 2));
        assert!(third.is_file());
        assert!(store.remove(&own).is_ok());
        assert!(!third.parent().unwrap().exists(), "its folder is gone");

        // Left behind: no item names it, and the sweep deletes it.
        let orphan = store.keep_image(&own, &image(4, 4)).unwrap();
        let orphan_path = store.image_path(&own, &orphan.stored().digest);
        drop(orphan);
        assert!(orphan_path.is_file());
        // Still pending (its capture never ended): kept until released.
        store.sweep();
        assert!(orphan_path.is_file());
        store.pending_images().clear();
        store.sweep();
        assert!(!orphan_path.exists());
    }

    #[test]
    fn a_capture_begun_before_items_were_deleted_keeps_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = store_at(dir.path(), 1);
        store
            .update("a", |history| {
                history.add("old", None, 1);
                Ok(())
            })
            .unwrap();
        let before = store.deletions();
        store.update("a", |history| Ok(history.clear())).unwrap();
        let add = |packages: &mut BTreeMap<String, PackageHistory>, now: u64| {
            packages
                .entry("a".into())
                .or_default()
                .add("late", None, now);
            true
        };
        store.capture(before, add);
        assert!(store.get("a").unwrap().items.is_empty());
        store.capture(store.deletions(), add);
        assert_eq!(store.get("a").unwrap().items.len(), 1);
        let items = &on_disk(dir.path())["packages"]["a"]["items"];
        assert_eq!(items.as_array().unwrap().len(), 1);
        assert_eq!(items[0]["text"], "late");
    }

    #[test]
    fn items_are_kept_for_the_retention_after_they_were_copied() {
        let mut history = PackageHistory::default();
        assert_eq!(history.retention(), 7 * 86_400);
        assert!(!history.has_choices());
        history.add("old", None, 0);
        history.add("new", None, 3 * DAY);
        assert_eq!(history.next_expiry(), Some(7 * DAY));
        assert_eq!(history.expire(7 * DAY - 1), 0);
        assert_eq!(history.expire(7 * DAY), 1);
        assert_eq!(texts(&history), ["new"]);

        for refused in [0, 59, 365 * 86_400 + 1, u64::MAX] {
            assert!(history.set_retention(refused).is_err(), "{refused}");
        }
        assert_eq!(history.retention(), 7 * 86_400);
        history.set_retention(60).unwrap();
        history.set_retention(365 * 86_400).unwrap();
        history.set_retention(3600).unwrap();
        // A choice, kept like the others when the items go.
        assert!(history.has_choices());
        assert_eq!(history.clear(), 1);
        assert!(!history.is_empty());
        history.add("later", None, 10 * DAY);
        assert_eq!(history.next_expiry(), Some(10 * DAY + 3_600_000));
    }

    #[test]
    fn items_are_deleted_by_id_and_ids_no_longer_kept_are_passed_over() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = store_at(dir.path(), DAY);
        store
            .update("a", |history| {
                for text in ["one", "two", "three"] {
                    history.add(text, None, DAY);
                }
                Ok(())
            })
            .unwrap();
        let ids: Vec<u64> = store.get("a").unwrap().items.iter().map(|i| i.id).collect();
        let before = store.deletions();
        let (deleted, _) = store
            .update("a", |history| Ok(history.delete(&[ids[1], 999])))
            .unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(texts(&store.get("a").unwrap()), ["three", "one"]);
        // A deletion, so a capture begun before it keeps nothing.
        assert_eq!(store.deletions(), before + 1);
        let (deleted, _) = store
            .update("a", |history| Ok(history.delete(&[ids[1]])))
            .unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn expired_items_are_removed_before_anything_reads_them() {
        let dir = tempfile::tempdir().unwrap();
        // As Pane left it before it stopped: "a" keeps items by default for
        // 7 days, "b" for 1 hour, "c" only its choice and "d" only an item.
        fs::write(
            dir.path().join(FILE),
            format!(
                r#"{{ "version": 1, "packages": {{
                "a": {{ "capture": "on", "items": [
                    {{ "id": 2, "text": "a new", "copiedAt": {new} }},
                    {{ "id": 1, "text": "a old", "copiedAt": 0 }} ], "nextId": 3 }},
                "b": {{ "retentionSeconds": 3600, "items": [
                    {{ "id": 1, "text": "b gone", "copiedAt": {new} }} ], "nextId": 2 }},
                "c": {{ "capture": "paused" }},
                "d": {{ "items": [
                    {{ "id": 1, "text": "d gone", "copiedAt": 0 }} ], "nextId": 2 }},
                "e": {{ "items": [
                    {{ "id": 4, "text": "e gone", "copiedAt": 0 }} ] }},
                "f": {{ "retentionSeconds": 0, "items": [
                    {{ "id": 0, "text": "f kept", "copiedAt": {fresh} }} ], "nextId": 1 }} }} }}"#,
                new = 6 * DAY,
                fresh = 8 * DAY - 30_000
            ),
        )
        .unwrap();
        let (store, _) = store_at(dir.path(), 8 * DAY);
        // Counted from the file as it is, without what expired.
        let counts = store.counts_now().unwrap();
        assert_eq!(counts.get("a"), Some(&1));
        assert_eq!(counts.get("c"), Some(&0));
        assert_eq!(counts.get("b"), Some(&0));
        // "d" and "e" keep nothing the user sees any more.
        assert_eq!(counts.get("d"), None);
        assert_eq!(counts.get("e"), None);
        // A retention written out of bounds is taken as the nearest bound.
        assert_eq!(counts.get("f"), Some(&1));
        assert_eq!(store.get("f").unwrap().retention(), 60);
        assert_eq!(texts(&store.get("a").unwrap()), ["a new"]);
        // Reading removed them from the file too; "d" and "e" stay only
        // with the id their next item gets, so no id is given twice.
        let on_disk = on_disk(dir.path());
        let packages = on_disk["packages"].as_object().unwrap();
        assert_eq!(
            packages.keys().collect::<Vec<_>>(),
            ["a", "b", "c", "d", "e", "f"]
        );
        assert_eq!(packages["d"], serde_json::json!({ "nextId": 2 }));
        assert_eq!(packages["e"], serde_json::json!({ "nextId": 5 }));
        assert_eq!(packages["b"]["retentionSeconds"], 3600);
        assert_eq!(packages["a"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(packages["a"]["nextId"], 3);
        // A capture never brings back what expired: ids keep counting.
        store.capture(store.deletions(), |packages, now| {
            packages.get_mut("a").unwrap().add("a old", None, now);
            true
        });
        let history = store.get("a").unwrap();
        assert_eq!(texts(&history), ["a old", "a new"]);
        assert_eq!(history.items[0].copied_at, 8 * DAY);
        assert_eq!(history.items[0].id, 3);
    }

    #[test]
    fn shortening_the_retention_deletes_older_items_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let (store, clock) = store_at(dir.path(), DAY);
        store
            .update("a", |history| {
                history.add("older", None, DAY);
                Ok(())
            })
            .unwrap();
        clock.advance(std::time::Duration::from_secs(7200));
        store
            .update("a", |history| {
                history.add("newer", None, DAY + 7_200_000);
                Ok(())
            })
            .unwrap();
        store
            .update("a", |history| history.set_retention(3600))
            .unwrap();
        let items = &on_disk(dir.path())["packages"]["a"]["items"];
        let texts: Vec<&str> = items
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["text"].as_str().unwrap())
            .collect();
        assert_eq!(texts, ["newer"]);
        assert!(
            store
                .update("a", |history| history.set_retention(1))
                .is_err()
        );
    }

    #[test]
    fn the_expiry_thread_removes_items_when_they_expire() {
        let dir = tempfile::tempdir().unwrap();
        let (store, clock) = store_at(dir.path(), DAY);
        store
            .update("a", |history| {
                history.capture = CaptureState::On;
                history.add("soon gone", None, DAY);
                Ok(())
            })
            .unwrap();
        store.keep_expiring();
        clock.advance(std::time::Duration::from_secs(7 * 86_400));
        // Nothing reads the store: the thread does it once the clock moved.
        assert!(store.wait_swept(Duration::from_secs(300)));
        let package = &on_disk(dir.path())["packages"]["a"];
        assert_eq!(package["items"], Value::Null);
        // Its ids are not given again.
        assert_eq!(package["nextId"], 1);
        // Dropping the store stops the thread, which let go of it after its
        // sweep: it keeps no store alive.
        let weak = Arc::downgrade(&store);
        drop(store);
        assert!(weak.upgrade().is_none());
    }
}
