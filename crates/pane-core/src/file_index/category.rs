//! The kinds of file (#126 "The Search Files command", #177): one table of
//! extensions, the same on every system, telling what an entry is from its
//! name. Search Files' type dropdown, its Metadata's Type, the index's
//! category filter and `pane:extension/file-index` all read it.

use std::path::Path;

use super::format::EntryKind;
use crate::files::program_named;

/// A kind of file, told from its name's extension, the same table on
/// every system. Search Files' type dropdown offers each (#177), as
/// Raycast's File Search does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// Documents to read or edit in an application: PDF, office and
    /// e-book files.
    Documents,
    Images,
    Audio,
    Video,
    Archives,
    /// Programs, scripts, shortcuts, installers and application bundles.
    Applications,
    /// Plain text: notes, data and configuration files, source code.
    Text,
    /// A file of none of the other categories (a folder is in none).
    Other,
}

const DOCUMENTS: &[&str] = &[
    "pdf", "doc", "docx", "odt", "rtf", "pages", "xls", "xlsx", "ods", "numbers", "ppt", "pptx",
    "odp", "key", "epub", "html", "htm",
];
const TEXT: &[&str] = &[
    "txt", "text", "md", "markdown", "rst", "adoc", "org", "tex", "csv", "tsv", "json", "jsonc",
    "xml", "yaml", "yml", "toml", "ini", "cfg", "conf", "env", "log", "srt", "vtt", "css", "scss",
    "less", "js", "mjs", "cjs", "jsx", "ts", "tsx", "rs", "py", "rb", "go", "java", "kt", "swift",
    "c", "h", "cc", "cpp", "hpp", "cs", "php", "lua", "sql", "r", "dart", "vue", "svelte", "zig",
];
/// Every extension people call an image, whether or not Pane's window can
/// draw it (a camera's raw file, a Photoshop document): what the Images
/// category holds. What the window draws, and so what Search Files'
/// detail previews, is the narrower [`crate::icons::DRAWN_IMAGE_EXTENSIONS`].
const IMAGES: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif", "svg", "ico", "raw",
    "cr2", "nef", "arw", "dng", "psd", "ai", "avif",
];
const AUDIO: &[&str] = &[
    "mp3", "wav", "flac", "aac", "m4a", "ogg", "oga", "opus", "wma", "aiff", "aif", "mid", "midi",
];
const VIDEO: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "wmv", "webm", "m4v", "mpg", "mpeg", "flv", "3gp", "ogv",
];
const ARCHIVES: &[&str] = &[
    "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "iso", "dmg", "cab", "lz", "lzma",
];

impl Category {
    /// Every category, in the order Search Files' type dropdown lists them
    /// (Raycast's order).
    pub const ALL: [Category; 8] = [
        Category::Documents,
        Category::Images,
        Category::Video,
        Category::Audio,
        Category::Archives,
        Category::Text,
        Category::Applications,
        Category::Other,
    ];

    /// The categories but [`Category::Other`], in the order an entry is
    /// told by: the first that holds it names it ([`Category::of`]).
    const NAMED: [Category; 7] = [
        Category::Applications,
        Category::Images,
        Category::Video,
        Category::Audio,
        Category::Archives,
        Category::Documents,
        Category::Text,
    ];

    /// What one entry of it is called, as the type dropdown and the
    /// Metadata's Type say it: "Document", "Image", "Video", "Audio",
    /// "Archive", "Text", "Application", "Other".
    pub fn noun(self) -> &'static str {
        match self {
            Category::Documents => "Document",
            Category::Images => "Image",
            Category::Video => "Video",
            Category::Audio => "Audio",
            Category::Archives => "Archive",
            Category::Text => "Text",
            Category::Applications => "Application",
            Category::Other => "Other",
        }
    }

    /// Its stable id, as the type dropdown names its choice: "document",
    /// "image", … (the noun, lowercased).
    pub fn id(self) -> &'static str {
        match self {
            Category::Documents => "document",
            Category::Images => "image",
            Category::Video => "video",
            Category::Audio => "audio",
            Category::Archives => "archive",
            Category::Text => "text",
            Category::Applications => "application",
            Category::Other => "other",
        }
    }

    /// Whether the entry at `path`, of `kind`, is of this category. A
    /// script is both an application and text; a folder is of none but an
    /// application bundle's.
    pub fn holds(self, path: &Path, kind: EntryKind) -> bool {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let table = match self {
            Category::Documents => DOCUMENTS,
            Category::Images => IMAGES,
            Category::Audio => AUDIO,
            Category::Video => VIDEO,
            Category::Archives => ARCHIVES,
            Category::Text => TEXT,
            Category::Applications => {
                return match kind {
                    EntryKind::Folder => extension == "app",
                    EntryKind::File | EntryKind::Link => program_named(path),
                };
            }
            Category::Other => {
                return kind != EntryKind::Folder
                    && !Category::NAMED
                        .iter()
                        .any(|category| category.holds(path, kind));
            }
        };
        kind != EntryKind::Folder && table.contains(&extension.as_str())
    }

    /// The one category that names the entry at `path`, of `kind`, for
    /// people (an application before text, an image before a document);
    /// `None` for a folder that is no application bundle.
    pub fn of(path: &Path, kind: EntryKind) -> Option<Category> {
        Category::NAMED
            .into_iter()
            .chain([Category::Other])
            .find(|category| category.holds(path, kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_is_listed_once_with_its_own_id() {
        for category in Category::ALL {
            assert_eq!(Category::ALL.iter().filter(|c| **c == category).count(), 1);
            assert_eq!(category.id(), category.noun().to_lowercase());
        }
    }

    #[test]
    fn every_image_the_window_draws_is_an_image() {
        for extension in crate::icons::DRAWN_IMAGE_EXTENSIONS {
            let path = format!("/x/picture.{extension}");
            assert_eq!(
                Category::of(Path::new(&path), EntryKind::File),
                Some(Category::Images),
                "{extension}"
            );
        }
    }
}
