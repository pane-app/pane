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

use unicode_normalization::UnicodeNormalization;

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
