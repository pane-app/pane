//! Matching and ranking for root search.
//!
//! A query is matched against each result's title, subtitle and, for an
//! installed command, its package's title. Text is compared after Unicode
//! NFC normalization (so an accent typed as one character matches the same
//! accent typed as a letter plus a combining mark), full Unicode lowercasing
//! and collapsing runs of whitespace to single spaces. Lowercasing is not
//! locale-aware case folding: language-specific rules such as Turkish dotted
//! and dotless I are out of scope.
//!
//! Every word of the query must appear in the title, subtitle or package
//! title. Matches are ranked by how well the title matches, best first:
//!
//! 1. the title is the query;
//! 2. the title starts with the query;
//! 3. every word of the query starts a word of the title;
//! 4. every word of the query appears in the title;
//! 5. every word appears in the title or subtitle;
//! 6. otherwise (some word appears only in the package title).
//!
//! Results that rank the same keep their order in root search, and an empty
//! query lists every result in that order. This is a deliberately simple
//! first ranking, not tuned relevance: no typo tolerance, abbreviations,
//! frequency or recency.
//!
//! A query that is the alias the user gave a result ([`Query::is_alias_of`])
//! is compared caselessly instead ([`same_text`]); the launcher lists such a
//! result before every other.

use unicase::UniCase;
use unicode_normalization::UnicodeNormalization;

/// Whether `a` and `b` are the same text caselessly: after NFC and collapsing
/// whitespace, with full Unicode case folding (so "STRASSE" is "straße" and a
/// final sigma is a sigma). Folding is not locale-aware: Turkish dotted and
/// dotless I are not folded together.
pub(crate) fn same_text(a: &str, b: &str) -> bool {
    UniCase::unicode(normalize(a)) == UniCase::unicode(normalize(b))
}

/// How well a result matches a query; lower is better.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Exact,
    Prefix,
    WordPrefixes,
    InTitle,
    InSubtitle,
    InPackage,
}

/// `text` as it is compared: NFC, lowercase, its words separated by single
/// spaces.
fn normalize(text: &str) -> String {
    let text: String = text.to_lowercase().nfc().collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A result's text as it is matched, normalized once when the result is
/// listed rather than on every keystroke.
#[derive(Clone, Debug)]
pub(crate) struct Keys {
    title: String,
    /// The title's words: runs of letters and digits.
    title_words: Vec<String>,
    subtitle: String,
    package: String,
    /// The alias the user gave the result, if any.
    alias: Option<String>,
}

impl Keys {
    /// The keys of a result titled `title`, with `subtitle`, offered by the
    /// package titled `package`, if any.
    pub(crate) fn new(title: &str, subtitle: Option<&str>, package: Option<&str>) -> Keys {
        let title = normalize(title);
        let title_words = title
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect();
        Keys {
            title,
            title_words,
            subtitle: subtitle.map(normalize).unwrap_or_default(),
            package: package.map(normalize).unwrap_or_default(),
            alias: None,
        }
    }

    /// These keys, also matched by `alias`, which the user gave the result.
    pub(crate) fn with_alias(self, alias: Option<&str>) -> Keys {
        Keys {
            alias: alias.map(normalize).filter(|alias| !alias.is_empty()),
            ..self
        }
    }
}

/// A query as it is matched (see [`normalize`]).
pub(crate) struct Query {
    text: String,
    words: Vec<String>,
}

impl Query {
    pub(crate) fn new(query: &str) -> Query {
        let text = normalize(query);
        let words = text.split(' ').filter(|w| !w.is_empty()).map(str::to_owned);
        Query {
            words: words.collect(),
            text,
        }
    }

    /// Whether this query is the alias the user gave the result with `keys`,
    /// compared caselessly ([`same_text`]).
    pub(crate) fn is_alias_of(&self, keys: &Keys) -> bool {
        !self.text.is_empty()
            && keys
                .alias
                .as_deref()
                .is_some_and(|alias| UniCase::unicode(alias) == UniCase::unicode(&self.text))
    }

    /// How well a result with `keys` matches this non-empty query; `None`
    /// if it does not.
    fn rank(&self, keys: &Keys) -> Option<Rank> {
        let in_title = |word: &String| keys.title.contains(word.as_str());
        let in_subtitle = |word: &String| in_title(word) || keys.subtitle.contains(word.as_str());
        let rank = if keys.title == self.text {
            Rank::Exact
        } else if keys.title.starts_with(&self.text) {
            Rank::Prefix
        } else if self.words.iter().all(|word| {
            keys.title_words
                .iter()
                .any(|title_word| title_word.starts_with(word.as_str()))
        }) {
            Rank::WordPrefixes
        } else if self.words.iter().all(in_title) {
            Rank::InTitle
        } else if self.words.iter().all(in_subtitle) {
            Rank::InSubtitle
        } else if self
            .words
            .iter()
            .all(|word| in_subtitle(word) || keys.package.contains(word.as_str()))
        {
            Rank::InPackage
        } else {
            return None;
        };
        Some(rank)
    }
}

/// The indices of the results with `keys` that match `query`, best match
/// first; equally good matches keep their order. An empty query matches
/// every result, in order.
pub(crate) fn ranked_matches<'a>(
    query: &Query,
    keys: impl ExactSizeIterator<Item = &'a Keys>,
) -> Vec<usize> {
    if query.words.is_empty() {
        return (0..keys.len()).collect();
    }
    let mut matches: Vec<(Rank, usize)> = keys
        .enumerate()
        .filter_map(|(index, keys)| query.rank(keys).map(|rank| (rank, index)))
        .collect();
    // Stable, so equal ranks keep root search order.
    matches.sort_by_key(|&(rank, _)| rank);
    matches.into_iter().map(|(_, index)| index).collect()
}

/// Where `query` matched `title`, for the window to highlight: byte ranges
/// into `title`, in order and not overlapping. The whole query where the
/// title holds it, as one run; otherwise each word of the query where it
/// appears in the title — at the start of a word of the title if it does,
/// else its first appearance — so a word that matched the subtitle or the
/// package title instead highlights nothing. Compared lowercased, as
/// matching compares; a title whose text only matches once normalized
/// (a decomposed accent, collapsed spaces) highlights what still matches
/// as written. Empty for a blank query.
pub fn title_matches(title: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let query = normalize(query);
    if query.is_empty() {
        return Vec::new();
    }
    // The title lowercased, with each byte's origin: the title byte where
    // the character it came from starts, and where that character ends.
    let mut lowered = String::new();
    let mut origin: Vec<(usize, usize)> = Vec::new();
    for (start, character) in title.char_indices() {
        let end = start + character.len_utf8();
        for lower in character.to_lowercase() {
            lowered.push(lower);
            origin.extend(std::iter::repeat_n((start, end), lower.len_utf8()));
        }
    }
    let to_title = |found: usize, len: usize| origin[found].0..origin[found + len - 1].1;
    if let Some(found) = lowered.find(&query) {
        return vec![to_title(found, query.len())];
    }
    let starts_word = |at: usize| {
        lowered[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !before.is_alphanumeric())
    };
    let mut ranges: Vec<std::ops::Range<usize>> = query
        .split(' ')
        .filter(|word| !word.is_empty())
        .filter_map(|word| {
            let mut found = lowered.match_indices(word).map(|(at, _)| at);
            let first = found.clone().next()?;
            let at = found.find(|&at| starts_word(at)).unwrap_or(first);
            Some(to_title(at, word.len()))
        })
        .collect();
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// One entry of Pane's Settings search, as the Settings window registers
/// it: a setting or section's title, the group the control sits in (the
/// page's own words, such as "Theme"), and the title of the Settings page
/// it belongs to. Matching is the same code that matches root search's
/// results: every word of the query must appear in the title, the group
/// or the page's title, ranked by how well the title matches and then
/// how far out the words had to be found (see the module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsEntry {
    /// The setting or section's name, as the Settings page shows it.
    pub title: String,
    /// The group the control sits in on its page, if it names one.
    pub group: Option<String>,
    /// The title of the Settings page the control sits on.
    pub page: String,
}

/// The indices of the Settings entries that match `query`, best match
/// first; equally good matches keep their registration order. An empty
/// query matches every entry, in order — the Settings window decides
/// itself what an empty query shows (its sections list), and only asks
/// for matches to non-empty text. The window owns registration; this
/// only matches and ranks, so a page can register controls as they
/// appear and drop them as they go, without this code knowing pages.
pub fn settings_matches(query: &str, entries: &[SettingsEntry]) -> Vec<usize> {
    let keys = entries
        .iter()
        .map(|entry| {
            Keys::new(
                &entry.title,
                entry.group.as_deref(),
                Some(entry.page.as_str()),
            )
        })
        .collect::<Vec<_>>();
    let query = Query::new(query);
    ranked_matches(&query, keys.iter())
}

#[cfg(test)]
mod tests {
    use super::{SettingsEntry, settings_matches, title_matches};

    /// A small catalog, as the Settings window registers one.
    fn catalog() -> Vec<SettingsEntry> {
        [
            (
                "Appearance",
                Some("Theme and material choices"),
                "Appearance",
            ),
            ("System", Some("Theme"), "Appearance"),
            ("Dark", Some("Theme"), "Appearance"),
            ("Glass", Some("Material"), "Appearance"),
            ("Shortcuts", Some("Aliases and hotkeys"), "Shortcuts"),
        ]
        .into_iter()
        .map(|(title, group, page)| SettingsEntry {
            title: title.into(),
            group: group.map(str::to_owned),
            page: page.into(),
        })
        .collect()
    }

    /// The titles of the catalog's entries, in registration order, for
    /// reading the matches back.
    const TITLES: [&str; 5] = ["Appearance", "System", "Dark", "Glass", "Shortcuts"];

    /// The entries `query` matches, as their titles, in ranked order.
    fn titles(query: &str) -> Vec<&'static str> {
        settings_matches(query, &catalog())
            .into_iter()
            .map(|index| TITLES[index])
            .collect()
    }

    #[test]
    fn an_empty_query_matches_every_entry_in_order() {
        assert_eq!(
            settings_matches("", &catalog()),
            vec![0, 1, 2, 3, 4],
            "the window decides what an empty query shows; the core just ranks"
        );
        assert_eq!(
            settings_matches("   ", &catalog()),
            vec![0, 1, 2, 3, 4],
            "whitespace is no query"
        );
    }

    #[test]
    fn the_title_ranks_before_the_group_and_the_page() {
        // "dark" is in the Dark choice's title, so it ranks by title; the
        // page's own entry, whose description names no darkness, does not
        // match at all.
        assert_eq!(titles("dark"), vec!["Dark"]);
        // A word of the group matches beneath the title; of the page,
        // beneath that — the same order root search's results keep.
        assert_eq!(
            titles("material"),
            vec!["Appearance", "Glass"],
            "the description of the Appearance page names the material, and so does the group"
        );
        assert_eq!(titles("shortcuts"), vec!["Shortcuts"]);
        // Every word must appear somewhere: one that does not matches
        // nothing.
        assert!(titles("dark material").is_empty());
    }

    #[test]
    fn the_whole_query_highlights_as_one_run_where_the_title_holds_it() {
        assert_eq!(title_matches("Clipboard History", "clip"), [0..4]);
        assert_eq!(title_matches("Clipboard History", "CLIP"), [0..4]);
        assert_eq!(title_matches("Clipboard History", "board hi"), [4..12]);
        assert_eq!(title_matches("Clipboard History", "  hist "), [10..14]);
    }

    #[test]
    fn each_word_highlights_where_it_starts_a_word_of_the_title() {
        // "is" appears in "History" before it starts a word of the title.
        assert_eq!(
            title_matches("History is clipped", "clip is"),
            [8..10, 11..15]
        );
        // A word the title does not hold highlights nothing.
        assert_eq!(title_matches("Clipboard History", "clip pane"), [0..4]);
        assert!(title_matches("Clipboard History", "pane").is_empty());
        assert!(title_matches("Clipboard History", " ").is_empty());
    }

    #[test]
    fn ranges_are_bytes_of_the_title_as_written() {
        // Ä is two bytes, lowercased to ä of two bytes.
        assert_eq!(title_matches("Ärger übersetzen", "är"), [0..3]);
        assert_eq!(title_matches("Ärger übersetzen", "über"), [7..12]);
        let title = "Straße";
        let ranges = title_matches(title, "ße");
        assert_eq!(&title[ranges[0].clone()], "ße");
    }

    #[test]
    fn every_word_may_be_found_in_a_different_place() {
        // "dark" in the title, "theme" in the group: one result.
        assert_eq!(titles("dark theme"), vec!["Dark"]);
        // The page's title counts too, as a package's does in root search.
        assert_eq!(titles("glass appearance"), vec!["Glass"]);
    }
}
