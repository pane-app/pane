//! Names as file search matches them: folded (letter case and accents
//! ignored, "Résumé" finds "resume") and split into words, and the score
//! of a name against a query: by the start of its words, by path segments
//! for a query with `/` or `\`, and, when those find too few, inside its
//! words ("port" finds "report", ranked below).

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// `text` lowercased and without accents: its compatibility decomposition
/// without combining marks, then lowercased.
pub(crate) fn fold(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    let mut folded = String::with_capacity(text.len());
    for c in text.nfkd() {
        if !is_combining_mark(c) {
            folded.extend(c.to_lowercase());
        }
    }
    folded
}

/// The words of a name, folded: each run of letters and digits, and, where
/// a run changes from lower to upper case or between letters and digits
/// ("invoiceMay2026"), each part of it as well, without repeats.
pub(crate) fn words(name: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut push = |word: &str| {
        let word = fold(word);
        if !word.is_empty() && !words.contains(&word) {
            words.push(word);
        }
    };
    let mut run_start: Option<usize> = None;
    let mut parts: Vec<(usize, usize)> = Vec::new();
    let mut part_start = 0;
    let mut previous: Option<char> = None;
    let chars: Vec<(usize, char)> = name.char_indices().collect();
    for (at, &(index, c)) in chars.iter().enumerate() {
        if c.is_alphanumeric() {
            match (run_start, previous) {
                (None, _) => {
                    run_start = Some(index);
                    part_start = index;
                }
                (Some(_), Some(before)) if splits(before, c) => {
                    parts.push((part_start, index));
                    part_start = index;
                }
                _ => {}
            }
            previous = Some(c);
        } else if let Some(start) = run_start.take() {
            end_run(name, start, index, part_start, &mut parts, &mut push);
            previous = None;
        }
        if at + 1 == chars.len()
            && let Some(start) = run_start.take()
        {
            end_run(name, start, name.len(), part_start, &mut parts, &mut push);
        }
    }
    words
}

/// Ends a run of letters and digits: the run as one word, then its parts if
/// it has more than one.
fn end_run(
    name: &str,
    start: usize,
    end: usize,
    part_start: usize,
    parts: &mut Vec<(usize, usize)>,
    push: &mut impl FnMut(&str),
) {
    push(&name[start..end]);
    if !parts.is_empty() {
        parts.push((part_start, end));
        for &(from, to) in parts.iter() {
            push(&name[from..to]);
        }
    }
    parts.clear();
}

/// Whether a word ends between `before` and `c`: lower to upper case, or
/// between a letter and a digit.
fn splits(before: char, c: char) -> bool {
    (before.is_lowercase() && c.is_uppercase())
        || (before.is_alphabetic() && c.is_numeric())
        || (before.is_numeric() && c.is_alphabetic())
}

/// The words of a query, folded, split only where letters and digits stop
/// (a query is not split by case), without repeats.
pub(crate) fn query_words(query: &str) -> Vec<String> {
    let folded = fold(query);
    let mut words: Vec<String> = Vec::new();
    for word in folded.split(|c: char| !c.is_alphanumeric()) {
        if !word.is_empty() && !words.iter().any(|known| known == word) {
            words.push(word.to_owned());
        }
    }
    words
}

/// The fewest letters a query word needs to be found inside a word ("port"
/// in "report"); a shorter one must start a word.
pub(crate) const INSIDE_FROM: usize = 3;

/// A query ready to score names against.
pub(crate) struct Prepared {
    /// The whole query, folded and trimmed.
    pub(crate) text: String,
    pub(crate) words: Vec<String>,
    /// A query with `/` or `\` matches path segments in order: the words of
    /// each part between them, outermost first, the last the entry's own
    /// name (none after a trailing separator: anything inside).
    pub(crate) segments: Option<Vec<Vec<String>>>,
}

impl Prepared {
    pub(crate) fn new(query: &str) -> Prepared {
        let text = fold(query.trim());
        let segments = text.contains(['/', '\\']).then(|| {
            let mut parts: Vec<&str> = text.split(['/', '\\']).collect();
            // A leading separator says nothing: the path below a root
            // starts anywhere.
            while parts.first().is_some_and(|part| part.trim().is_empty()) && parts.len() > 1 {
                parts.remove(0);
            }
            parts.into_iter().map(query_words).collect()
        });
        Prepared {
            text,
            words: query_words(query),
            segments,
        }
    }

    /// Whether a word of the query may be looked for inside words, when
    /// too few entries have words starting with them: one of
    /// [`INSIDE_FROM`] letters or more, in a query that is no path.
    pub(crate) fn finds_inside(&self) -> bool {
        self.segments.is_none()
            && self
                .words
                .iter()
                .any(|word| word.chars().count() >= INSIDE_FROM)
    }
}

/// What a candidate is scored on.
pub(crate) struct Candidate<'a> {
    /// Its own name.
    pub(crate) name: &'a str,
    /// The folders it is in, below its root, outermost first.
    pub(crate) folders: &'a [&'a str],
    pub(crate) is_folder: bool,
    /// Last modified, in seconds since 1970.
    pub(crate) modified: u64,
}

/// Ranks of a match, best first; the score's integer part.
const EXACT_NAME: f64 = 100.0;
const EXACT_STEM: f64 = 90.0;
const NAME_PREFIX: f64 = 80.0;
const PATH_SEGMENTS: f64 = 70.0;
const WORDS_IN_NAME: f64 = 60.0;
const WORDS_IN_FOLDERS: f64 = 40.0;
const INSIDE_NAME: f64 = 30.0;
const INSIDE_FOLDERS: f64 = 20.0;
/// How far below every match by the start of words a match inside a word
/// is put ([`score_inside`]): the bonuses never lift it over one.
const INSIDE_BAND: f64 = 100.0;

/// The score of `candidate` for `query`, higher is better, or `None` when
/// a word of the query starts no word of its name or folders. The shape is
/// Raycast's (#126, "Matching and ranking"): an exact name above an exact
/// stem, above a name prefix, above every word starting a word of the name,
/// above words found only in the folders; a match only in the extension
/// scores lower; then a little for recent changes, decaying with age, a
/// little for a folder, words in order, and a little less for depth. A
/// query with `/` or `\` matches path segments in order
/// ([`Prepared::segments`]): each part's words start words of one folder,
/// the folders in the parts' order, and the last part's start words of
/// the name.
pub(crate) fn score(query: &Prepared, candidate: &Candidate<'_>, now: u64) -> Option<f64> {
    if query.words.is_empty() {
        return None;
    }
    let name_words = words(candidate.name);
    if let Some(segments) = &query.segments {
        let folders: Vec<Vec<String>> = candidate
            .folders
            .iter()
            .map(|folder| words(folder))
            .collect();
        if !segments_match(segments, &folders, &name_words) {
            return None;
        }
        return Some(PATH_SEGMENTS + bonuses(candidate, now));
    }
    let name = fold(candidate.name);
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], Some(&name[dot + 1..])),
        _ => (name.as_str(), None),
    };
    let folder_words = folder_words(candidate);
    let score = if name == query.text {
        EXACT_NAME
    } else if stem == query.text {
        EXACT_STEM
    } else if name.starts_with(&query.text) {
        NAME_PREFIX
    } else {
        let mut only_in_folders = 0usize;
        let mut only_extension = 0usize;
        let mut positions = Vec::with_capacity(query.words.len());
        for word in &query.words {
            let in_name = name_words.iter().position(|known| known.starts_with(word));
            match in_name {
                Some(at) => {
                    positions.push(at);
                    let matches_stem = name_words
                        .iter()
                        .any(|known| known.starts_with(word) && Some(known.as_str()) != extension);
                    if !matches_stem {
                        only_extension += 1;
                    }
                }
                None if folder_words.iter().any(|known| known.starts_with(word)) => {
                    only_in_folders += 1;
                }
                None => return None,
            }
        }
        let in_order = positions.windows(2).all(|pair| pair[0] <= pair[1]);
        let base = if only_in_folders == 0 {
            WORDS_IN_NAME
        } else {
            WORDS_IN_FOLDERS - 4.0 * (only_in_folders - 1) as f64
        };
        base + if in_order { 3.0 } else { 0.0 } - 8.0 * only_extension as f64
    };
    Some(score + bonuses(candidate, now))
}

/// The score of `candidate` for `query` when a word of the query is found
/// only inside a word ("port" in "report"), never at its start: lower than
/// every match [`score`] gives (by [`INSIDE_BAND`]), so a name that starts
/// with the query, or whose words start with its words, always ranks
/// above. Every word must start a word of the name or folders or, when it
/// has [`INSIDE_FROM`] letters or more, be inside one; `None` when one is
/// neither, or when [`score`] matches it (it is listed there).
pub(crate) fn score_inside(query: &Prepared, candidate: &Candidate<'_>, now: u64) -> Option<f64> {
    if query.words.is_empty() || query.segments.is_some() {
        return None;
    }
    if score(query, candidate, now).is_some() {
        return None;
    }
    let name_words = words(candidate.name);
    let folder_words = folder_words(candidate);
    let mut only_in_folders = 0usize;
    for word in &query.words {
        let found = |known: &String| {
            known.starts_with(word.as_str())
                || (word.chars().count() >= INSIDE_FROM && known.contains(word.as_str()))
        };
        if name_words.iter().any(found) {
            continue;
        }
        if folder_words.iter().any(found) {
            only_in_folders += 1;
            continue;
        }
        return None;
    }
    let base = if only_in_folders == 0 {
        INSIDE_NAME
    } else {
        INSIDE_FOLDERS - 4.0 * (only_in_folders - 1) as f64
    };
    Some(base + bonuses(candidate, now) - INSIDE_BAND)
}

/// The words of the folders `candidate` is in, without repeats.
fn folder_words(candidate: &Candidate<'_>) -> Vec<String> {
    let mut folder_words: Vec<String> = Vec::new();
    for folder in candidate.folders {
        for word in words(folder) {
            if !folder_words.contains(&word) {
                folder_words.push(word);
            }
        }
    }
    folder_words
}

/// What every match adds: up to 6 for a recent change, halving every 30
/// days, 1.5 for a folder, and a quarter less for each folder it is in.
fn bonuses(candidate: &Candidate<'_>, now: u64) -> f64 {
    let mut bonus = 0.0;
    let age_days = now.saturating_sub(candidate.modified) as f64 / 86_400.0;
    if candidate.modified > 0 {
        bonus += 6.0 * 0.5f64.powf(age_days / 30.0);
    }
    if candidate.is_folder {
        bonus += 1.5;
    }
    bonus - 0.25 * candidate.folders.len().min(24) as f64
}

/// Whether every word of `part` starts a word of `component`.
fn part_matches(part: &[String], component: &[String]) -> bool {
    part.iter().all(|word| {
        component
            .iter()
            .any(|known| known.starts_with(word.as_str()))
    })
}

/// Whether the path segments `segments` match an entry in the folders
/// whose words are `folders` (outermost first), named by `name_words`: the
/// last segment its name (any name when it is empty), each one before it a
/// folder, in order, further folders allowed between them.
fn segments_match(
    segments: &[Vec<String>],
    folders: &[Vec<String>],
    name_words: &[String],
) -> bool {
    let Some((last, before)) = segments.split_last() else {
        return false;
    };
    if !last.is_empty() && !part_matches(last, name_words) {
        return false;
    }
    let mut next = 0;
    for part in before.iter().filter(|part| !part.is_empty()) {
        match folders[next.min(folders.len())..]
            .iter()
            .position(|folder| part_matches(part, folder))
        {
            Some(at) => next += at + 1,
            None => return false,
        }
    }
    // A trailing separator ("docs/"): something inside the folders named.
    !last.is_empty() || before.iter().any(|part| !part.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_ignores_case_and_accents() {
        assert_eq!(fold("Résumé PLAN ü"), "resume plan u");
        assert_eq!(fold("ÅNGSTRÖM"), "angstrom");
        assert_eq!(fold("plain"), "plain");
    }

    #[test]
    fn a_name_splits_into_words_and_their_parts() {
        assert_eq!(
            words("invoiceMay2026.pdf"),
            ["invoicemay2026", "invoice", "may", "2026", "pdf"]
        );
        assert_eq!(words("Résumé plan ü.txt"), ["resume", "plan", "u", "txt"]);
        assert_eq!(words("IMG_0042.JPG"), ["img", "0042", "jpg"]);
        assert_eq!(words("..."), Vec::<String>::new());
        assert_eq!(words("a-a"), ["a"]);
    }

    #[test]
    fn a_query_is_split_where_letters_and_digits_stop() {
        assert_eq!(
            query_words("Invoices 2026/may"),
            ["invoices", "2026", "may"]
        );
        assert_eq!(query_words("  "), Vec::<String>::new());
    }

    fn scored(query: &str, name: &str, folders: &[&str]) -> Option<f64> {
        let query = Prepared::new(query);
        let candidate = Candidate {
            name,
            folders,
            is_folder: false,
            modified: 0,
        };
        score(&query, &candidate, 0)
    }

    #[test]
    fn exact_names_rank_above_stems_prefixes_words_and_folders() {
        let exact = scored("report.pdf", "report.pdf", &[]).unwrap();
        let stem = scored("report", "report.pdf", &[]).unwrap();
        let prefix = scored("repo", "report.pdf", &[]).unwrap();
        let word = scored("final", "report final.pdf", &[]).unwrap();
        let folder = scored("invoices report", "report.pdf", &["Invoices"]).unwrap();
        assert!(exact > stem && stem > prefix && prefix > word && word > folder);
        assert_eq!(scored("missing", "report.pdf", &["Invoices"]), None);
    }

    #[test]
    fn accents_and_case_are_ignored_and_the_extension_alone_scores_lower() {
        assert!(scored("resume", "Résumé plan ü.txt", &[]).is_some());
        let in_stem = scored("pdf", "pdf notes.txt", &[]).unwrap();
        let in_extension = scored("pd", "notes.pdf", &[]).unwrap();
        assert!(in_stem > in_extension);
    }

    #[test]
    fn a_query_inside_a_word_is_found_below_every_match_by_word_starts() {
        let inside = |query: &str, name: &str, folders: &[&str]| {
            let query = Prepared::new(query);
            score_inside(
                &query,
                &Candidate {
                    name,
                    folders,
                    is_folder: false,
                    modified: 0,
                },
                0,
            )
        };
        // "port" is inside "report", not at the start of a word.
        assert_eq!(scored("port", "report.pdf", &[]), None);
        let mid = inside("port", "report.pdf", &[]).unwrap();
        let start = scored("port", "portfolio.txt", &["deep", "down", "in", "here"]).unwrap();
        let in_folders = scored("port", "a.txt", &["ports"]).unwrap();
        assert!(
            start > mid && in_folders > mid,
            "{start} {in_folders} {mid}"
        );
        // Inside a folder's word, lower again; with another word that
        // starts a word, still found.
        let folder_mid = inside("port", "a.txt", &["Reports"]).unwrap();
        assert!(mid > folder_mid);
        assert!(inside("port final", "report final.pdf", &[]).is_some());
        // A match by word starts is not listed again as one inside.
        assert_eq!(inside("port", "portfolio.txt", &[]), None);
        // Fewer than three letters must start a word.
        assert_eq!(inside("or", "report.pdf", &[]), None);
        assert!(!Prepared::new("or").finds_inside());
        assert!(Prepared::new("or port").finds_inside());
        assert_eq!(inside("missing", "report.pdf", &[]), None);
    }

    #[test]
    fn a_query_with_a_separator_matches_path_segments_in_order() {
        let docs = ["Documents", "Work 2026"];
        assert!(scored("documents/plan", "plan.txt", &docs).is_some());
        assert!(scored(r"doc\work\plan", "plan b.txt", &docs).is_some());
        // Folders between the parts are allowed, not the other order.
        assert!(scored("doc/pla", "plan.txt", &docs).is_some());
        assert_eq!(scored("work/documents/plan", "plan.txt", &docs), None);
        // The last part is the name.
        assert_eq!(scored("documents/work", "plan.txt", &docs), None);
        assert_eq!(scored("documents/plan", "plan.txt", &["Pictures"]), None);
        // A leading separator says nothing; a trailing one asks for what
        // is inside.
        assert!(scored("/documents/plan", "plan.txt", &docs).is_some());
        assert!(scored("work/", "plan.txt", &docs).is_some());
        assert_eq!(scored("plan/", "plan.txt", &docs), None);
        // A path match ranks below a name prefix and above words found
        // anywhere.
        let path = scored("documents/plan", "plan.txt", &docs).unwrap();
        let words = scored("documents plan", "plan.txt", &docs).unwrap();
        let prefix = scored("plan", "plan b.txt", &docs).unwrap();
        assert!(prefix > path && path > words, "{prefix} {path} {words}");
    }

    #[test]
    fn recent_and_shallow_entries_rank_a_little_higher() {
        let query = Prepared::new("plan");
        let now = 400 * 86_400;
        let at = |modified, folders: &'static [&'static str]| {
            score(
                &query,
                &Candidate {
                    name: "plan b.txt",
                    folders,
                    is_folder: false,
                    modified,
                },
                now,
            )
            .unwrap()
        };
        assert!(at(now, &[]) > at(now - 300 * 86_400, &[]));
        assert!(at(now, &[]) > at(now, &["a", "b", "c"]));
    }
}
