//! The host side of `pane:extension/file-index` (wit/file-index.wit): a
//! package that sets `"fileIndex": true` searches Pane's file index and
//! reads its status. Answers at once from what is indexed; the search never
//! waits for a walk, and the guest never reads the file system for it.

use super::category::Category;
use super::format::EntryKind;
use super::indexer::{IndexState, SearchOptions, Sort};
use crate::runtime::{GuestState, bindings};

use bindings::pane::extension::file_index as wit;

fn kind_of(kind: wit::EntryKind) -> EntryKind {
    match kind {
        wit::EntryKind::File => EntryKind::File,
        wit::EntryKind::Folder => EntryKind::Folder,
        wit::EntryKind::Link => EntryKind::Link,
    }
}

fn wit_kind(kind: EntryKind) -> wit::EntryKind {
    match kind {
        EntryKind::File => wit::EntryKind::File,
        EntryKind::Folder => wit::EntryKind::Folder,
        EntryKind::Link => wit::EntryKind::Link,
    }
}

fn category_of(category: wit::Category) -> Category {
    match category {
        wit::Category::Documents => Category::Documents,
        wit::Category::Images => Category::Images,
        wit::Category::Audio => Category::Audio,
        wit::Category::Video => Category::Video,
        wit::Category::Archives => Category::Archives,
        wit::Category::Applications => Category::Applications,
        wit::Category::Text => Category::Text,
        wit::Category::Other => Category::Other,
    }
}

impl wit::Host for GuestState {
    fn search(
        &mut self,
        query: String,
        options: wit::SearchOptions,
    ) -> Result<Vec<wit::FileEntry>, String> {
        // Answers from what the index holds; never waits for a walk.
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(crate::runtime::stopped_code(end));
        }
        let Some(owner) = self.owner() else {
            return Err("Pane's file index is for installed extensions".into());
        };
        let found = self.file_access().indexer().search(
            &owner,
            &query,
            SearchOptions {
                kind: options.kind.map(kind_of),
                category: options.category.map(category_of),
                sort: match options.sort {
                    wit::Sort::Relevance => Sort::Relevance,
                    wit::Sort::Modified => Sort::Modified,
                },
                limit: options.limit as usize,
                offset: options.offset as usize,
            },
        )?;
        Ok(found
            .into_iter()
            .map(|found| wit::FileEntry {
                id: found.id,
                path: found.path.to_string_lossy().into_owned(),
                name: found.name,
                folder: found.folder,
                kind: wit_kind(found.kind),
                program: found.program,
                size: found.size,
                modified: found.modified,
                volume: found.volume,
            })
            .collect())
    }

    fn status(&mut self) -> wit::IndexStatus {
        let status = self.file_access().indexer().status();
        wit::IndexStatus {
            state: match status.state {
                IndexState::Off => wit::IndexState::Off,
                IndexState::Building => wit::IndexState::Building,
                IndexState::Current => wit::IndexState::Current,
                IndexState::Stopped => wit::IndexState::Stopped,
            },
            entries: status.entries,
            found: status.found,
            reason: status.reason,
        }
    }
}
