//! Search Files like Raycast's (#177, spec #161): Pane's registered Files
//! command, open on its own search, lists the file index (#126, #175)
//! itself, with no folder to choose and nothing explained first.
//!
//! - **Before typing** the list is "Recently Used": the most recently
//!   modified entries the index holds. Typing ranks by the index's own
//!   matching. Opening from root search's "Search Files for “q”" row keeps
//!   the query (`Opening::initial_search`).
//! - **A type** ([`FileType`]) filters the list as Raycast's dropdown does:
//!   All Types, Folder, Document, Image, Video, Audio, Archive, Text,
//!   Application, Other, by the index's table of extensions
//!   ([`crate::file_index::Category`]).
//! - **Pages**: the first [`PAGE`] entries are listed; more load as the
//!   list scrolls ([`Launcher::load_more_files`]), until the index has no
//!   more.
//! - **Rows** are the launcher's own file rows (`files::FileRow`): their
//!   actions are Pane's (`own_actions`: Open, Show in Explorer, Open With…,
//!   Copy Path, Copy Name, Copy File, Move to Recycle Bin; Enter on a
//!   program shows it, and only Run runs it), each checking the entry
//!   again first. The window draws them in the split view with the
//!   system's icon of each ([`Launcher::search_files_icon`]) and the
//!   selected entry's detail ([`Launcher::search_files_details`]): an
//!   image's preview and the Metadata (Name, Where, Type, Size, Created,
//!   Modified), read from the file system when it is shown.
//! - **The index's state** ([`SearchFilesView::note`]): "Indexing… (N
//!   found so far)" while it is built, and why it stopped or is off.
//!
//! Only Pane's registered Files default extension is browsed this way,
//! told by its verified package and command identity (the default
//! extension `files` and its command of the same id), never by a title:
//! a copy of Files installed from a folder, and every other command that
//! searches, keep their own search (`command_search`). The index is asked
//! on the package's behalf, under its identity, as its own search would.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::UNIX_EPOCH;

use super::files::FileRow;
use super::{Entry, Launcher, Row, Screen, State, Status, off_thread, owner};
use crate::file_index::{Category, EntryKind, Found, IndexState, IndexStatus, SearchOptions, Sort};
use crate::icons::{DRAWN_IMAGE_EXTENSIONS, Icon};
use crate::packages::PackageIdentity;

/// The id of Pane's Files default extension, and of its Search Files
/// command in its manifest.
pub const FILES: &str = "files";

/// How many entries one page lists: the first page, and each the list
/// loads as it scrolls (the index answers 200 at most per call).
pub const PAGE: usize = 50;

/// The section over a blank query's list, as Raycast titles it.
pub const RECENTLY_USED: &str = "Recently Used";

/// The largest image the detail previews, in bytes: a bigger one shows its
/// Metadata alone.
pub const PREVIEW_LIMIT: u64 = 32 * 1024 * 1024;

/// What the type dropdown keeps (#177): Raycast's types, in its order.
/// Each but All Types and Folder is one of the index's categories
/// ([`Category`]), whose table and names it reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileType {
    #[default]
    All,
    Folder,
    Of(Category),
}

impl FileType {
    /// The types, in the dropdown's order: All Types, Folder, then the
    /// index's categories in [`Category::ALL`]'s order.
    pub const ALL: [FileType; 10] = {
        let mut all = [FileType::All; 10];
        all[1] = FileType::Folder;
        let mut at = 0;
        while at < Category::ALL.len() {
            all[at + 2] = FileType::Of(Category::ALL[at]);
            at += 1;
        }
        all
    };

    /// The dropdown's label for it: "All Types", "Folder", or the
    /// category's noun ("Document", "Image", …).
    pub fn label(self) -> &'static str {
        match self {
            FileType::All => "All Types",
            FileType::Folder => "Folder",
            FileType::Of(category) => category.noun(),
        }
    }

    /// Its stable id, as the dropdown names its choice: "all", "folder",
    /// or the category's id ("document", …).
    pub fn id(self) -> &'static str {
        match self {
            FileType::All => "all",
            FileType::Folder => "folder",
            FileType::Of(category) => category.id(),
        }
    }

    /// The type whose [`FileType::id`] is `id`.
    pub fn from_id(id: &str) -> Option<FileType> {
        FileType::ALL.into_iter().find(|kind| kind.id() == id)
    }

    /// The index's filter for it: an entry kind (folders) or a category.
    pub fn filter(self) -> (Option<EntryKind>, Option<Category>) {
        match self {
            FileType::All => (None, None),
            FileType::Folder => (Some(EntryKind::Folder), None),
            FileType::Of(category) => (None, Some(category)),
        }
    }

    /// The index's options for the page starting at `offset`.
    fn options(self, offset: usize) -> SearchOptions {
        let (kind, category) = self.filter();
        SearchOptions {
            kind,
            category,
            // Best match first; for a blank query, the most recently
            // modified first.
            sort: Sort::Relevance,
            limit: PAGE,
            offset,
        }
    }
}

/// What Search Files keeps while Pane's Files command is open on its own
/// search (in `command_search::Searching`).
#[derive(Clone, Debug)]
pub(super) struct Browsing {
    /// The package's identity key: the index is asked as it.
    owner: String,
    /// The command's component, which found the rows.
    component: PathBuf,
    /// The type the dropdown keeps.
    filter: FileType,
    /// How many entries of the current text and type are listed: the
    /// offset of the next page.
    loaded: usize,
    /// Whether the index has no more entries for them.
    exhausted: bool,
    /// The page asked and not yet listed, by its offset.
    asking: Option<usize>,
}

/// A search of the index started, to be awaited for its answer to be
/// listed.
type Asked = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Whether `state`'s open command is Search Files browsed by Pane.
pub(super) fn browsing(state: &State) -> bool {
    browsing_of(state).is_some()
}

fn browsing_of(state: &State) -> Option<&Browsing> {
    state.searching.as_ref()?.files.as_ref()
}

fn browsing_mut(state: &mut State) -> Option<&mut Browsing> {
    state.searching.as_mut()?.files.as_mut()
}

/// The row and entry listing `found`, found by the command in `component`
/// of the package with identity key `owner`.
fn listed(found: Found, owner: &str, component: &Path) -> (Row, Entry) {
    let row = Row {
        id: found.id.clone(),
        title: found.name.clone(),
        subtitle: Some(found.folder),
        unavailable: None,
    };
    let file = FileRow {
        owner: owner.to_owned(),
        id: found.id,
        name: found.name,
        program: found.program,
        component: component.to_path_buf(),
        indexed: true,
        typed: false,
        folder: found.kind == EntryKind::Folder,
        path: Some(found.path),
    };
    (row, Entry::File(file))
}

/// Search Files as the window draws it, read now (see
/// [`Launcher::search_files_view`]). The rows themselves are the
/// launcher's view's, each a file the index found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchFilesView {
    /// The command's own title.
    pub title: String,
    /// The text typed in its field.
    pub query: String,
    /// The type the dropdown keeps.
    pub filter: FileType,
    /// The label over the list: [`RECENTLY_USED`] for a blank query.
    pub section: Option<&'static str>,
    /// Where the index is.
    pub status: IndexStatus,
    /// Whether the first page of the text and type is still asked.
    pub loading: bool,
    /// Whether more entries may load as the list scrolls.
    pub more: bool,
}

impl SearchFilesView {
    /// What the view says of the index, if anything: "Indexing… (N found
    /// so far)" while it is built, why it stopped, or that file search is
    /// off; nothing while it is current.
    pub fn note(&self) -> Option<String> {
        let status = &self.status;
        let reason = status.reason.as_deref();
        match status.state {
            IndexState::Current => None,
            IndexState::Building => Some(format!(
                "Indexing… ({} found so far)",
                status.found.max(status.entries)
            )),
            IndexState::Stopped => Some(match reason {
                Some(reason) => format!("File search stopped: {reason}"),
                None => "File search stopped".into(),
            }),
            IndexState::Off => Some(match reason {
                Some(reason) => format!("File search is off: {reason}"),
                None => "File search is off".into(),
            }),
        }
    }

    /// Whether the note calls for the File search settings page: the index
    /// stopped or is off.
    pub fn needs_settings(&self) -> bool {
        matches!(self.status.state, IndexState::Stopped | IndexState::Off)
    }

    /// What the list says in place of rows when it lists none.
    pub fn empty_note(&self) -> String {
        if self.loading {
            return "Searching…".into();
        }
        if let Some(note) = self.note() {
            return note;
        }
        match (self.query.trim().is_empty(), self.filter) {
            (true, FileType::All) => "No files yet.".into(),
            (true, _) => format!("No {} files.", self.filter.label().to_lowercase()),
            (false, _) => "No files found. Try another search or type.".into(),
        }
    }
}

/// The selected entry's detail: what its Metadata says, read from the file
/// system now, and whether its image is previewed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDetails {
    /// The row it is listed by.
    pub id: String,
    /// Its absolute path.
    pub path: PathBuf,
    /// Name: its own name.
    pub name: String,
    /// Where: its folder below the home folder (`~/…`).
    pub place: String,
    /// Type: "Folder", "PNG Image", "PDF Document", "Application"…
    pub kind: String,
    /// Size, in bytes; `None` for a folder, or when it cannot be read.
    pub size: Option<u64>,
    /// When it was created, in milliseconds since the Unix epoch, where
    /// the system says.
    pub created: Option<u64>,
    /// When it was last modified, in milliseconds since the Unix epoch.
    pub modified: Option<u64>,
    /// Whether the detail previews it: an image the window can draw, of at
    /// most [`PREVIEW_LIMIT`] bytes, that is no link.
    pub preview: bool,
}

impl FileDetails {
    /// The detail of the entry at `path`, named `name`, in `place`, of
    /// `kind`, listed by the row `id`: its metadata is read now (a link's
    /// own, never its target's).
    pub fn read(id: &str, path: &Path, name: &str, place: &str, kind: EntryKind) -> FileDetails {
        let metadata = std::fs::symlink_metadata(path).ok();
        let millis = |time: std::io::Result<std::time::SystemTime>| {
            time.ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .and_then(|since| u64::try_from(since.as_millis()).ok())
        };
        let size = metadata
            .as_ref()
            .filter(|metadata| metadata.is_file())
            .map(std::fs::Metadata::len);
        let link = metadata
            .as_ref()
            .is_some_and(|metadata| metadata.file_type().is_symlink());
        FileDetails {
            id: id.to_owned(),
            path: path.to_path_buf(),
            name: name.to_owned(),
            place: place.to_owned(),
            kind: if link {
                "Link".into()
            } else {
                type_label(path, kind)
            },
            preview: kind == EntryKind::File
                && !link
                && previewed(path)
                && size.is_some_and(|size| size <= PREVIEW_LIMIT),
            size,
            created: metadata
                .as_ref()
                .and_then(|metadata| millis(metadata.created())),
            modified: metadata
                .as_ref()
                .and_then(|metadata| millis(metadata.modified())),
        }
    }
}

/// Whether the file at `path` is an image the detail can preview, by its
/// extension.
pub fn previewed(path: &Path) -> bool {
    extension(path).is_some_and(|extension| DRAWN_IMAGE_EXTENSIONS.contains(&extension.as_str()))
}

/// The extension of `path`, lowercased.
fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .map(str::to_ascii_lowercase)
}

/// What the Metadata's Type says of the entry at `path`, of `kind`: its
/// extension and its category ("PNG Image", "PDF Document", "MD Text"),
/// "Folder", "Application", "Link", or "File" for one of no extension.
pub fn type_label(path: &Path, kind: EntryKind) -> String {
    let category = Category::of(path, kind);
    match (kind, category) {
        (_, Some(Category::Applications)) => "Application".into(),
        (EntryKind::Folder, _) => "Folder".into(),
        (EntryKind::Link, _) => "Link".into(),
        (EntryKind::File, category) => {
            let noun = match category {
                None | Some(Category::Other) => "File",
                Some(category) => category.noun(),
            };
            match extension(path) {
                Some(extension) => format!("{} {noun}", extension.to_uppercase()),
                None => "File".into(),
            }
        }
    }
}

impl Launcher {
    /// Search Files as the window draws it, while Pane's registered Files
    /// command is open on its own search and its package runs; `None` on
    /// every other screen and for every other command (see the module
    /// documentation).
    pub fn search_files_view(&self) -> Option<SearchFilesView> {
        let state = self.lock();
        let Screen::CommandSearch { query } = &state.view.screen else {
            return None;
        };
        let browsing = browsing_of(&state)?;
        let package = owner(&state.packages, &browsing.component)?;
        if !state.runs(package) {
            return None;
        }
        let status = state
            .files
            .as_ref()
            .map(|files| files.indexer().status())
            .unwrap_or_default();
        Some(SearchFilesView {
            title: state.view.title.clone(),
            query: query.clone(),
            filter: browsing.filter,
            section: query.trim().is_empty().then_some(RECENTLY_USED),
            status,
            loading: browsing.asking == Some(0),
            more: !browsing.exhausted,
        })
    }

    /// The system's icon of Search Files' row at `index`, as it shows now
    /// (#142): loading starts if it has not; a document's or a folder's
    /// outline until it is loaded. `None` past the rows, or off Search
    /// Files.
    pub fn search_files_icon(&self, index: usize) -> Option<Icon> {
        let (icon, loads) = {
            let state = self.lock();
            browsing_of(&state)?;
            let Some(Entry::File(file)) = state.entries.get(index) else {
                return None;
            };
            let icon = super::file_search::entry_icon(file.path.as_deref()?, file.folder);
            (icon, state.icon_loads.clone())
        };
        loads.want(None, &icon);
        Some(loads.shown(None, &icon))
    }

    /// The detail of Search Files' row at `index` (see [`FileDetails`]),
    /// its metadata read now; `None` past the rows, or off Search Files.
    pub fn search_files_details(&self, index: usize) -> Option<FileDetails> {
        let (id, path, name, place, kind) = {
            let state = self.lock();
            browsing_of(&state)?;
            let row = state.view.rows.get(index)?;
            let Some(Entry::File(file)) = state.entries.get(index) else {
                return None;
            };
            let path = file.path.clone()?;
            let kind = if file.folder {
                EntryKind::Folder
            } else {
                EntryKind::File
            };
            (
                row.id.clone(),
                path,
                file.name.clone(),
                row.subtitle.clone().unwrap_or_default(),
                kind,
            )
        };
        // Off the launcher's lock: the file system may take a moment.
        Some(FileDetails::read(&id, &path, &name, &place, kind))
    }

    /// Keeps only entries of `filter` in Search Files (the type dropdown),
    /// listing the first page of the text typed again; await the returned
    /// future for it. Nothing changes off Search Files, or for the type
    /// already kept.
    pub fn set_file_type(&self, filter: FileType) -> impl Future<Output = ()> + Send + 'static {
        let asked = {
            let mut state = self.lock();
            let query = match &state.view.screen {
                Screen::CommandSearch { query } => Some(query.clone()),
                _ => None,
            };
            match (query, browsing_mut(&mut state)) {
                (Some(query), Some(browsing)) if browsing.filter != filter => {
                    browsing.filter = filter;
                    self.ask_files(&mut state, &query, 0)
                }
                _ => None,
            }
        };
        async move {
            if let Some(asked) = asked {
                asked.await;
            }
        }
    }

    /// Lists Search Files' next page as the list scrolls; await the
    /// returned future for it. Nothing is asked while a page is, once the
    /// index has no more, or off Search Files.
    pub fn load_more_files(&self) -> impl Future<Output = ()> + Send + 'static {
        let asked = {
            let mut state = self.lock();
            let query = match &state.view.screen {
                Screen::CommandSearch { query } => Some(query.clone()),
                _ => None,
            };
            let next = browsing_of(&state)
                .filter(|browsing| {
                    browsing.asking.is_none() && !browsing.exhausted && browsing.loaded > 0
                })
                .map(|browsing| browsing.loaded);
            match (query, next) {
                (Some(query), Some(next)) => self.ask_files(&mut state, &query, next),
                _ => None,
            }
        };
        async move {
            if let Some(asked) = asked {
                asked.await;
            }
        }
    }

    /// Lists Search Files' first page again, keeping the selected entry
    /// selected while it is listed: the window asks so as the index grows
    /// while it is built. Nothing is asked once more than a page is listed
    /// (the list the user scrolled stays), while a page is asked, or off
    /// Search Files.
    pub fn refresh_files(&self) -> impl Future<Output = ()> + Send + 'static {
        let asked = {
            let mut state = self.lock();
            let query = match &state.view.screen {
                Screen::CommandSearch { query } => Some(query.clone()),
                _ => None,
            };
            let again = browsing_of(&state)
                .is_some_and(|browsing| browsing.asking.is_none() && browsing.loaded <= PAGE);
            match query {
                Some(query) if again => self.ask_files(&mut state, &query, 0),
                _ => None,
            }
        };
        async move {
            if let Some(asked) = asked {
                asked.await;
            }
        }
    }

    /// Starts browsing Search Files as the command in `component`, whose
    /// manifest id is `command`, opens on its own search: when it is Pane's
    /// registered Files command, whose package runs and uses the file
    /// index, and this launcher keeps one. Whether it is.
    pub(super) fn begin_search_files(
        &self,
        state: &mut State,
        component: &Path,
        command: &str,
    ) -> bool {
        let Some(package) = owner(&state.packages, component) else {
            return false;
        };
        let registered = package.identity == PackageIdentity::default_extension(FILES)
            && command == FILES
            && state.runs(package)
            && super::file_search::uses_file_index(package)
            && state.files.is_some();
        if !registered {
            return false;
        }
        let owner = package.identity.key();
        let Some(searching) = state.searching.as_mut() else {
            return false;
        };
        searching.files = Some(Browsing {
            owner,
            component: component.to_path_buf(),
            filter: FileType::All,
            loaded: 0,
            exhausted: false,
            asking: None,
        });
        // Nothing listed until the index answers: the command's own list
        // is not Search Files' (a copy installed from a folder shows it).
        state.view.rows.clear();
        state.entries.clear();
        state.view.selected = None;
        true
    }

    /// Asks the index for the page of `query` starting at `offset`, as
    /// Search Files lists it, the type kept; the returned future lists it.
    /// The first page (`offset` 0) is a new search: the text becomes the
    /// field's, an older search's answer is not listed, and the rows listed
    /// meanwhile stay until it answers. `None` off Search Files.
    pub(super) fn ask_files(&self, state: &mut State, query: &str, offset: usize) -> Option<Asked> {
        let indexer = state.files.as_ref()?.indexer();
        browsing_of(state)?;
        if offset == 0 {
            state.search_epoch += 1;
            state.view.screen = Screen::CommandSearch {
                query: query.to_owned(),
            };
        }
        let browsing = browsing_mut(state)?;
        if offset == 0 {
            browsing.loaded = 0;
            browsing.exhausted = false;
        }
        browsing.asking = Some(offset);
        let owner = browsing.owner.clone();
        let options = browsing.filter.options(offset);
        let text = query.trim().to_owned();
        let epoch = state.screen_epoch;
        let search = state.search_epoch;
        let launcher = self.clone();
        Some(Box::pin(async move {
            // Off the calling thread: a blank query reads every entry's
            // modified time, and a type filters many.
            let found = off_thread(move || indexer.search(&owner, &text, options)).await;
            launcher.list_files(epoch, search, offset, found);
        }))
    }

    /// Clears Search Files' field (Escape, `window.clear-search`): the
    /// Recently Used list is asked for again at once, and listed in the
    /// background, the window told when it is.
    pub(super) fn clear_files_search(&self, state: &mut State) {
        let Some(asked) = self.ask_files(state, "", 0) else {
            return;
        };
        let launcher = self.clone();
        std::thread::spawn(move || {
            futures::executor::block_on(asked);
            launcher.changed();
        });
    }

    /// Lists `found`, the page starting at `offset` of the search `search`
    /// on the screen of `epoch`, unless a newer search, another page or
    /// another screen has come since: the first page replaces the rows
    /// (keeping the selected entry selected while it is listed again),
    /// a later one adds to them. A failure is said in the status.
    fn list_files(
        &self,
        epoch: u64,
        search: u64,
        offset: usize,
        found: Result<Vec<Found>, String>,
    ) {
        let Some(mut guard) = self.lock_if_current(epoch) else {
            return;
        };
        let state = &mut *guard;
        if state.search_epoch != search {
            return;
        }
        let Some(browsing) = browsing_mut(state) else {
            return;
        };
        if browsing.asking != Some(offset) || browsing.loaded != offset {
            return;
        }
        browsing.asking = None;
        let found = match found {
            Ok(found) => found,
            Err(why) => {
                browsing.exhausted = true;
                if offset == 0 {
                    state.view.rows.clear();
                    state.entries.clear();
                    state.view.selected = None;
                }
                state.view.status = Status::Error(format!("Cannot search your files: {why}"));
                return;
            }
        };
        browsing.exhausted = found.len() < PAGE;
        browsing.loaded = offset + found.len();
        let (owner, component) = (browsing.owner.clone(), browsing.component.clone());
        let (rows, entries): (Vec<Row>, Vec<Entry>) = found
            .into_iter()
            .map(|found| listed(found, &owner, &component))
            .unzip();
        if offset > 0 {
            state.view.rows.extend(rows);
            state.entries.extend(entries);
            return;
        }
        // The entry selected before, if it is listed again (the index grew
        // while it was built): the user's place is kept.
        let kept = state
            .view
            .selected
            .and_then(|index| match state.entries.get(index) {
                Some(Entry::File(file)) => file.path.clone(),
                _ => None,
            });
        let selected = kept
            .and_then(|kept| {
                entries.iter().position(
                    |entry| matches!(entry, Entry::File(file) if file.path.as_ref() == Some(&kept)),
                )
            })
            .or_else(|| (!rows.is_empty()).then_some(0));
        state.view.rows = rows;
        state.entries = entries;
        state.view.selected = selected;
        if matches!(state.view.status, Status::Error(_)) {
            state.view.status = Status::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_types_are_raycasts_in_its_order() {
        let labels: Vec<&str> = FileType::ALL.iter().map(|kind| kind.label()).collect();
        assert_eq!(
            labels,
            [
                "All Types",
                "Folder",
                "Document",
                "Image",
                "Video",
                "Audio",
                "Archive",
                "Text",
                "Application",
                "Other"
            ]
        );
        for kind in FileType::ALL {
            assert_eq!(FileType::from_id(kind.id()), Some(kind));
        }
        assert_eq!(FileType::from_id("pictures"), None);
        assert_eq!(FileType::Folder.filter(), (Some(EntryKind::Folder), None));
        assert_eq!(
            FileType::Of(Category::Text).filter(),
            (None, Some(Category::Text))
        );
        assert_eq!(
            FileType::Of(Category::Other).filter(),
            (None, Some(Category::Other))
        );
        assert_eq!(
            FileType::from_id("image"),
            Some(FileType::Of(Category::Images))
        );
        assert_eq!(FileType::All.options(100).offset, 100);
        assert_eq!(FileType::All.options(0).limit, PAGE);
    }

    #[test]
    fn a_type_names_the_extension_and_its_category() {
        let file = |name: &str| type_label(Path::new(name), EntryKind::File);
        assert_eq!(file("/x/cat.png"), "PNG Image");
        assert_eq!(file("/x/report.pdf"), "PDF Document");
        assert_eq!(file("/x/notes.md"), "MD Text");
        assert_eq!(file("/x/song.mp3"), "MP3 Audio");
        assert_eq!(file("/x/setup.exe"), "Application");
        assert_eq!(file("/x/data.bin"), "BIN File");
        assert_eq!(file("/x/README"), "File");
        assert_eq!(
            type_label(Path::new("/x/Projects"), EntryKind::Folder),
            "Folder"
        );
        assert_eq!(
            type_label(Path::new("/x/Pane.app"), EntryKind::Folder),
            "Application"
        );
        assert!(previewed(Path::new("/x/Shot.PNG")));
        assert!(!previewed(Path::new("/x/report.pdf")));
    }

    #[test]
    fn the_detail_reads_its_metadata_and_previews_a_small_image() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("shot.png");
        std::fs::write(&image, [0u8; 2048]).unwrap();
        let details = FileDetails::read("i1-1", &image, "shot.png", "~/Pictures", EntryKind::File);
        assert_eq!(details.name, "shot.png");
        assert_eq!(details.place, "~/Pictures");
        assert_eq!(details.kind, "PNG Image");
        assert_eq!(details.size, Some(2048));
        assert!(details.modified.is_some());
        assert!(details.preview);
        let folder = FileDetails::read("i1-2", dir.path(), "dir", "~", EntryKind::Folder);
        assert_eq!(folder.size, None);
        assert!(!folder.preview);
        assert_eq!(folder.kind, "Folder");
        let gone = FileDetails::read(
            "i1-3",
            &dir.path().join("gone.png"),
            "gone.png",
            "~",
            EntryKind::File,
        );
        assert_eq!(
            (gone.size, gone.modified, gone.preview),
            (None, None, false)
        );
    }

    #[test]
    fn the_note_says_where_the_index_is() {
        let view = |state: IndexState, found: u64, reason: Option<&str>| SearchFilesView {
            title: "Search Files".into(),
            query: String::new(),
            filter: FileType::All,
            section: Some(RECENTLY_USED),
            status: IndexStatus {
                state,
                found,
                reason: reason.map(str::to_owned),
                ..IndexStatus::default()
            },
            loading: false,
            more: true,
        };
        assert_eq!(view(IndexState::Current, 0, None).note(), None);
        assert_eq!(
            view(IndexState::Building, 1234, None).note().as_deref(),
            Some("Indexing… (1234 found so far)")
        );
        let stopped = view(IndexState::Stopped, 0, Some("the disk is full"));
        assert_eq!(
            stopped.note().as_deref(),
            Some("File search stopped: the disk is full")
        );
        assert!(stopped.needs_settings());
        assert!(!view(IndexState::Building, 1, None).needs_settings());
        assert_eq!(
            view(IndexState::Current, 0, None).empty_note(),
            "No files yet."
        );
        let typed = SearchFilesView {
            query: "plan".into(),
            ..view(IndexState::Current, 0, None)
        };
        assert_eq!(
            typed.empty_note(),
            "No files found. Try another search or type."
        );
        let loading = SearchFilesView {
            loading: true,
            ..view(IndexState::Current, 0, None)
        };
        assert_eq!(loading.empty_note(), "Searching…");
    }
}
