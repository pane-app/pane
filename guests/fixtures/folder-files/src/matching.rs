//! Which of a folder's files a query finds, best first.

use pane_extension::alloc::{string::String, vec::Vec};
use pane_extension::files::FoundFile;

/// The most files one query lists.
pub const MAX_RESULTS: usize = 20;

/// The last name of `path`, written with `/` or `\` between names.
pub fn last_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
}

/// The files `query` finds: those whose name contains every word of the
/// query, then those where each word is in the name or the folders below
/// the chosen one, ignoring letter case, each group in listing order; at
/// most [`MAX_RESULTS`].
pub fn matching<'a>(files: &'a [FoundFile], query: &str) -> Vec<&'a FoundFile> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut by_name = Vec::new();
    let mut by_path = Vec::new();
    for file in files {
        let relative = file.relative.to_lowercase();
        let name = last_name(&relative);
        if words.iter().all(|word| name.contains(word.as_str())) {
            by_name.push(file);
        } else if words.iter().all(|word| relative.contains(word.as_str())) {
            by_path.push(file);
        }
        if by_name.len() == MAX_RESULTS {
            break;
        }
    }
    by_name.extend(by_path);
    by_name.truncate(MAX_RESULTS);
    by_name
}
