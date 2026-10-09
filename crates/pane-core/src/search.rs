//! Matching and ranking for root search.
//!
//! A query is matched against each result's texts — its title, each
//! alternate title, its subtitle (with its keywords), the composites
//! "title subtitle" and "subtitle title", and, for an installed
//! command, its package's title — as a subsequence with a score: a
//! letter matched at the text's first position scores 4, at a word
//! start (after a separator) 3, elsewhere 2; a separator matched to a
//! separator scores 1. Each gap between two consecutive matched
//! positions costs 1; adjacency is free. A query separator that cannot
//! be placed is skipped, counted as skipped and never a failure, while
//! a letter that cannot be placed means no match. The best placement's
//! score counts, and an exact equality of the folded query and a folded
//! text is its own outcome, the best one. A query starting with "/"
//! treats the first "/" in a text as a space.
//!
//! How good a score must be to count as a match is the user's Search
//! sensitivity ([`SearchSensitivity`]): Low accepts any placement,
//! Medium asks for more, and High (the default) asks for a match that
//! starts the text or a word of it.
//!
//! Text is compared after transliteration: Unicode NFC normalization,
//! so an accent typed as one character matches the same accent typed as
//! a letter plus a combining mark, then a mapping to ASCII through the
//! `any_ascii` table (é to e, đ and Đ to d, ß to ss, ligatures split,
//! every script the table covers romanised), then lowercase and
//! collapsing runs of whitespace. So "cafe" finds "Café" and "tieng
//! viet" finds "Tiếng Việt". Lowercasing is not locale-aware case
//! folding: language-specific rules such as Turkish dotted and dotless
//! I are out of scope.
//!
//! An indexed result may also have alternate titles (an application's
//! untranslated or program name), each matched as the title is, the
//! best of them counting, and keywords, matched as the subtitle is; the
//! row still shows its title.
//!
//! Matches are ranked by how well the title matches, best first:
//!
//! 1. the title is the query;
//! 2. the title starts with the query;
//! 3. every word of the query starts a word of the title;
//! 4. every word of the query appears in the title;
//! 5. every word appears in the title or subtitle;
//! 6. otherwise (some word appears only in the package title);
//! 7. a match only the scorer found — the query's letters scattered
//!    through a title, or words the texts merely hold — ranks after the
//!    six, best score first.
//!
//! Results that rank the same keep their order in root search, and an
//! empty query lists every result in that order. This ranking is the
//! deliberately simple one from before fuzzy matching, kept until the
//! ranking ticket that follows (#197); no typo tolerance, frequency or
//! recency.
//!
//! A query that is the alias the user gave a result ([`Query::is_alias_of`])
//! is compared caselessly instead ([`same_text`]), never transliterated,
//! so a recorded alias means exactly what it meant; the launcher lists such
//! a result before every other.

use unicase::UniCase;
use unicode_normalization::UnicodeNormalization;

use crate::host_settings::SearchSensitivity;

/// Whether `a` and `b` are the same text caselessly: after NFC and
/// collapsing whitespace, with full Unicode case folding (so "STRASSE" is
/// "straße" and a final sigma is a sigma). Folding is not locale-aware:
/// Turkish dotted and dotless I are not folded together.
///
/// This is the aliases' comparison, and only theirs: the user's own words
/// are kept as they were recorded, never transliterated (see [`fold`]),
/// so an alias means exactly what it meant.
pub(crate) fn same_text(a: &str, b: &str) -> bool {
    UniCase::unicode(normalize(a)) == UniCase::unicode(normalize(b))
}

/// How well a result matches a query; lower is better. The first six
/// steps are the order root search has kept since its first matching;
/// the seventh holds a match only the scorer found, ranked by its score.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Exact,
    Prefix,
    WordPrefixes,
    InTitle,
    InSubtitle,
    InPackage,
    /// A match only the scorer found, ranked after the six by its score.
    Fuzzy,
}

/// Whether `c` separates words for the scorer: whitespace, or one of
/// `- . / ( ) [ ]`. Capital letters inside a word are not word starts in
/// root search, so a change of case separates nothing.
fn is_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '-' | '.' | '/' | '(' | ')' | '[' | ']')
}

/// `text` as it is compared caselessly, for aliases: NFC, lowercase, its
/// words separated by single spaces. Accents survive.
fn normalize(text: &str) -> String {
    let text: String = text.to_lowercase().nfc().collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` as it is matched: NFC, then transliterated to ASCII through the
/// `any_ascii` table (é to e, đ and Đ to d, ß to ss, ligatures split,
/// every script the table covers romanised), then lowercase, its words
/// separated by single spaces. Unlike [`normalize`], accents never
/// survive it, so "cafe" finds "Café" and "tieng viet" finds "Tiếng
/// Việt".
fn fold(text: &str) -> String {
    if text.is_ascii() {
        return text.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    }
    let nfc: String = text.nfc().collect();
    any_ascii::any_ascii(&nfc)
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A result's text as it is matched, folded once when the result is
/// listed rather than on every keystroke.
#[derive(Clone, Debug)]
pub(crate) struct Keys {
    title: Title,
    /// Other titles that find the result as its title does (an indexed
    /// result's alternate titles); the row still shows the title.
    alternates: Vec<Title>,
    /// The subtitle and the keywords together, as the subtitle is
    /// matched: keywords find the result as the subtitle does today, and
    /// the composites below span them with the title (keywords join the
    /// matcher's own texts in #197).
    subtitle: String,
    /// The composites "title subtitle" and "subtitle title", folded, so
    /// a query can span both ("utub vid" finding a result titled "Search
    /// YouTube" with the subtitle "Videos"); both empty when there is no
    /// subtitle to span.
    composites: [String; 2],
    package: String,
    /// The alias the user gave the result, if any: normalized caselessly
    /// ([`normalize`]), never transliterated.
    alias: Option<String>,
}

/// A title as it is matched.
#[derive(Clone, Debug)]
struct Title {
    text: String,
    /// Its words: runs of letters and digits.
    words: Vec<String>,
}

impl Title {
    fn new(title: &str) -> Title {
        let text = fold(title);
        let words = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect();
        Title { text, words }
    }
}

/// The composites of a folded `title` with a folded `subtitle`, for a
/// query that spans both; both empty when there is no subtitle.
fn composites(title: &str, subtitle: &str) -> [String; 2] {
    if subtitle.is_empty() {
        [String::new(), String::new()]
    } else {
        [format!("{title} {subtitle}"), format!("{subtitle} {title}")]
    }
}

impl Keys {
    /// The keys of a result titled `title`, with `subtitle`, offered by the
    /// package titled `package`, if any.
    pub(crate) fn new(title: &str, subtitle: Option<&str>, package: Option<&str>) -> Keys {
        let title = Title::new(title);
        let subtitle = subtitle.map(fold).unwrap_or_default();
        Keys {
            composites: composites(&title.text, &subtitle),
            title,
            alternates: Vec::new(),
            subtitle,
            package: package.map(fold).unwrap_or_default(),
            alias: None,
        }
    }

    /// These keys, also matched by `alternate_titles` as the title is and
    /// by `keywords` as the subtitle is. Blank ones are ignored.
    pub(crate) fn with_alternates(self, alternate_titles: &[String], keywords: &[String]) -> Keys {
        let keywords = keywords
            .iter()
            .map(|keyword| fold(keyword))
            .filter(|keyword| !keyword.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let subtitle = [self.subtitle.clone(), keywords]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        Keys {
            alternates: alternate_titles
                .iter()
                .map(|title| Title::new(title))
                .filter(|title| !title.text.is_empty())
                .collect(),
            composites: composites(&self.title.text, &subtitle),
            subtitle,
            ..self
        }
    }

    /// These keys, also matched by `alias`, which the user gave the result.
    pub(crate) fn with_alias(self, alias: Option<&str>) -> Keys {
        Keys {
            alias: alias.map(normalize).filter(|alias| !alias.is_empty()),
            ..self
        }
    }

    /// The best score this query places in any of these keys' texts at
    /// `sensitivity`: the title, each alternate title, the subtitle with
    /// its keywords, the composites of the two, or the package title.
    /// `None` when no text holds the query's letters in order, or none
    /// of their best placements passes the sensitivity's threshold —
    /// which is the same thing, since every threshold is monotone in the
    /// score.
    fn scored(&self, query: &Query, sensitivity: SearchSensitivity) -> Option<i32> {
        let texts = std::iter::once(&self.title.text)
            .chain(self.alternates.iter().map(|title| &title.text))
            .chain([
                &self.subtitle,
                &self.composites[0],
                &self.composites[1],
                &self.package,
            ]);
        let best = texts.filter_map(|text| query.score(text)).max();
        best.filter(|&score| sensitivity.accepts(score, query.letters))
    }
}

/// One placement of a query in a text, as the scorer counts it: the
/// score, and — where the caller asked for it — which position each
/// query character landed at, `None` for a skipped separator.
struct Placed {
    score: i32,
    at: Vec<Option<usize>>,
}

/// A query as it is matched: folded for the scorer ([`fold`]), and kept
/// plain ([`normalize`]) for the aliases' caseless comparison.
pub(crate) struct Query {
    /// The query as the scorer sees it: folded.
    text: String,
    /// The query as aliases see it: normalized, not transliterated.
    plain: String,
    /// The folded text's words: runs of letters and digits.
    words: Vec<String>,
    /// How many of the query's characters are not separators: the n of
    /// the sensitivity thresholds. Separators are the only characters
    /// the scorer may skip, so they are not counted.
    letters: usize,
}

impl SearchSensitivity {
    /// Whether a query of `letters` characters that are not separators,
    /// whose best placement in a text scored `score`, counts as a match:
    /// Low accepts any placement, Medium at least 1.5·(n−2)+4, High more
    /// than 2n. (Medium's threshold is doubled into integers.)
    fn accepts(self, score: i32, letters: usize) -> bool {
        match self {
            SearchSensitivity::Low => true,
            SearchSensitivity::Medium => 2 * score >= 3 * letters as i32 + 2,
            SearchSensitivity::High => score > 2 * letters as i32,
        }
    }
}

impl Query {
    pub(crate) fn new(query: &str) -> Query {
        let text = fold(query);
        let plain = normalize(query);
        let words = text
            .split(' ')
            .filter(|word| !word.is_empty())
            .map(str::to_owned);
        let letters = text.chars().filter(|c| !is_separator(*c)).count();
        Query {
            words: words.collect(),
            letters,
            text,
            plain,
        }
    }

    /// Whether this query is the alias the user gave the result with `keys`,
    /// compared caselessly ([`same_text`]), never transliterated.
    pub(crate) fn is_alias_of(&self, keys: &Keys) -> bool {
        !self.plain.is_empty()
            && keys
                .alias
                .as_deref()
                .is_some_and(|alias| UniCase::unicode(alias) == UniCase::unicode(&self.plain))
    }

    /// How well a result with `keys` matches this non-empty query, as
    /// today's six steps count it; `None` when only the scorer found the
    /// match, which ranks after the six ([`Rank::Fuzzy`]).
    fn rank(&self, keys: &Keys) -> Option<Rank> {
        // The best a title or an alternate title matches.
        let titled = std::iter::once(&keys.title)
            .chain(&keys.alternates)
            .filter_map(|title| self.title_rank(title))
            .min();
        if titled.is_some() {
            return titled;
        }
        let in_titles = |word: &String| {
            std::iter::once(&keys.title)
                .chain(&keys.alternates)
                .any(|title| title.text.contains(word.as_str()))
        };
        let in_subtitle = |word: &String| in_titles(word) || keys.subtitle.contains(word.as_str());
        if self.words.iter().all(in_subtitle) {
            Some(Rank::InSubtitle)
        } else if self
            .words
            .iter()
            .all(|word| in_subtitle(word) || keys.package.contains(word.as_str()))
        {
            Some(Rank::InPackage)
        } else {
            None
        }
    }

    /// How well `title` alone matches this non-empty query, at best
    /// [`Rank::Exact`] and at worst [`Rank::InTitle`]; `None` if some word
    /// is not in it.
    fn title_rank(&self, title: &Title) -> Option<Rank> {
        if title.text == self.text {
            Some(Rank::Exact)
        } else if title.text.starts_with(&self.text) {
            Some(Rank::Prefix)
        } else if self.words.iter().all(|word| {
            title
                .words
                .iter()
                .any(|title_word| title_word.starts_with(word.as_str()))
        }) {
            Some(Rank::WordPrefixes)
        } else if self
            .words
            .iter()
            .all(|word| title.text.contains(word.as_str()))
        {
            Some(Rank::InTitle)
        } else {
            None
        }
    }

    /// The score of the best placement of this query in `text`, both
    /// folded; `None` when the text cannot hold the query's letters in
    /// order, or nothing at all can be placed.
    fn score(&self, text: &str) -> Option<i32> {
        self.placed_in(text.chars().collect(), false)
            .map(|placed| placed.score)
    }

    /// The best placement of this query in `chars`, already folded: the
    /// highest-scoring way the query's letters fall into the text in
    /// order, its separators placed on the text's or skipped. `trace`
    /// also recovers where each query character landed, for highlighting.
    fn placed_in(&self, chars: Vec<char>, trace: bool) -> Option<Placed> {
        let mut chars = chars;
        // A query starting with "/" treats the first "/" in the text as
        // a space.
        if self.text.starts_with('/')
            && let Some(at) = chars.iter().position(|&c| c == '/')
        {
            chars[at] = ' ';
        }
        let query: Vec<char> = self.text.chars().collect();
        if query.is_empty() || chars.is_empty() {
            return None;
        }
        // The quick in-order rejection: a text that cannot hold the
        // query's letters in order is never scored, and only a query
        // longer than two characters needs the check.
        if query.len() > 2 && !holds(&query, &chars) {
            return None;
        }
        place(&query, &chars, trace)
    }
}

/// Whether `text` can hold `query`'s letters in order: the cheap check
/// that rejects a text the scorer would only reject more expensively.
fn holds(query: &[char], text: &[char]) -> bool {
    let mut at = 0;
    query
        .iter()
        .filter(|c| !is_separator(**c))
        .all(|&c| match text[at..].iter().position(|&t| t == c) {
            Some(found) => {
                at += found + 1;
                true
            }
            None => false,
        })
}

/// What the query's `i`-th character scores placed at `j` in `text`, or
/// `None` if it cannot sit there: a separator only on a separator, a
/// letter only on the same letter, with the text's first position worth
/// 4, a word start 3 and anywhere else 2. A separator matched to a
/// separator is worth 1 wherever it lands.
fn base(query: &[char], text: &[char], i: usize, j: usize) -> Option<i32> {
    let placed = query[i];
    if is_separator(placed) {
        return is_separator(text[j]).then_some(1);
    }
    (placed == text[j]).then(|| {
        if j == 0 {
            4
        } else if is_separator(text[j - 1]) {
            3
        } else {
            2
        }
    })
}

/// The best placement of `query` in `text` (both folded), by the scorer's
/// count: every letter placed, each separator placed on a separator or
/// skipped, each gap between consecutive placements costing 1, adjacency
/// free. `trace` recovers where each query character landed. `None` when
/// nothing at all can be placed (a query of separators alone needs a
/// text that holds one).
fn place(query: &[char], text: &[char], trace: bool) -> Option<Placed> {
    let (n, m) = (query.len(), text.len());
    let impossible = i32::MIN;
    // dp[i][j]: the best score placing query[0..=i] with query[i] at j.
    // best[i][j] and best_at[i][j]: the best of those up to j, and where
    // it sits. from[i][j], while tracing: the placement the best score
    // came from — the previous placed character, or `None` for the first.
    let mut dp = vec![impossible; n * m];
    let mut best = vec![impossible; n * m];
    let mut best_at = vec![0; n * m];
    let mut from: Vec<Option<(usize, usize)>> = vec![None; n * m];
    // What placing query[0..=i] has before its last character, for each
    // end `j`: the best over the predecessors the query allows. The
    // previous placed character is the last letter before i, or any
    // character back to it, the separators between being skipped or
    // placed themselves; nothing at all, when every character before i
    // is a separator. Held as a running maximum, so a run of separators
    // costs the placement loop no more than one row.
    let mut run = vec![impossible; m];
    let mut run_from = vec![None; m];
    // Whether nothing is placed before row i, which is so while every
    // character before it is a separator; grown one character at a time.
    let mut nothing_before = true;
    for i in 0..n {
        if i > 0 {
            // A letter at i-1 resets the range to that row alone; a
            // separator extends it with the row just filled.
            let reset = !is_separator(query[i - 1]);
            for j in 0..m {
                let previous = i - 1;
                // What row `previous` offers a placement ending at j:
                // its own best ending just before j — adjacent, so free
                // — or anywhere before that, over one gap.
                let mut value = impossible;
                let mut came = None;
                if j >= 1 && dp[previous * m + j - 1] > impossible {
                    value = dp[previous * m + j - 1];
                    came = Some((previous, j - 1));
                }
                if j >= 2 && best[previous * m + j - 2] > impossible {
                    let gap = best[previous * m + j - 2] - 1;
                    if gap > value {
                        value = gap;
                        came = Some((previous, best_at[previous * m + j - 2]));
                    }
                }
                if reset || value > run[j] {
                    run[j] = value;
                    run_from[j] = came;
                }
            }
        }
        let none_before = nothing_before;
        for j in 0..m {
            let Some(base) = base(query, text, i, j) else {
                continue;
            };
            // Nothing placed before: every earlier character is a
            // separator, all skipped.
            let mut before = if none_before { Some(0) } else { None };
            let mut came = None;
            if run[j] > before.unwrap_or(impossible) {
                before = Some(run[j]);
                came = run_from[j];
            }
            let Some(before) = before else {
                continue;
            };
            dp[i * m + j] = base + before;
            if trace {
                from[i * m + j] = came;
            }
        }
        for j in 0..m {
            let here = dp[i * m + j];
            let up_to = if j == 0 {
                impossible
            } else {
                best[i * m + j - 1]
            };
            if here > up_to {
                best[i * m + j] = here;
                best_at[i * m + j] = j;
            } else {
                best[i * m + j] = up_to;
                best_at[i * m + j] = if j == 0 { 0 } else { best_at[i * m + j - 1] };
            }
        }
        nothing_before &= is_separator(query[i]);
    }
    // The placement ends at the query's last letter, or after it where
    // trailing separators are placed; the best of those, wherever it
    // ends in the text.
    let last = query.iter().rposition(|c| !is_separator(*c)).unwrap_or(0);
    let mut winner: Option<(usize, usize)> = None;
    for i in last..n {
        for j in 0..m {
            let here = dp[i * m + j];
            if here == impossible {
                continue;
            }
            if winner.as_ref().is_none_or(|&(wi, wj)| here > dp[wi * m + wj]) {
                winner = Some((i, j));
            }
        }
    }
    let Some((i, j)) = winner else {
        return None;
    };
    let score = dp[i * m + j];
    let mut at = Vec::new();
    if trace {
        at = vec![None; n];
        let mut walked = Some((i, j));
        while let Some((wi, wj)) = walked {
            at[wi] = Some(wj);
            walked = from[wi * m + wj];
        }
    }
    Some(Placed { score, at })
}

/// The indices of the results with `keys` that match `query` at
/// `sensitivity`, best match first; equally good matches keep their
/// order. An empty query matches every result, in order.
pub(crate) fn ranked_matches<'a>(
    query: &Query,
    keys: impl ExactSizeIterator<Item = &'a Keys>,
    sensitivity: SearchSensitivity,
) -> Vec<usize> {
    if query.text.is_empty() {
        return (0..keys.len()).collect();
    }
    let mut matches: Vec<(Option<Rank>, i32, usize)> = keys
        .enumerate()
        .filter_map(|(index, keys)| {
            let score = keys.scored(query, sensitivity)?;
            Some((query.rank(keys), score, index))
        })
        .collect();
    // Stable: the six ranked steps keep root search order among
    // themselves, and a fuzzy match orders by its score, best first.
    matches.sort_by(|a, b| match (a.0, b.0) {
        (None, None) => b.1.cmp(&a.1),
        _ => a.0.cmp(&b.0),
    });
    matches.into_iter().map(|(_, _, index)| index).collect()
}

/// One text as it is highlighted: folded character by character, each
/// folded character carrying the byte range of the character of the text
/// as written that it came from, so the positions the scorer finds map
/// back to what the user sees. Whitespace collapses as [`fold`]
/// collapses it.
struct Folded {
    chars: Vec<char>,
    origin: Vec<Option<(usize, usize)>>,
}

impl Folded {
    fn new(text: &str) -> Folded {
        let nfc: String = text.nfc().collect();
        let mut chars = Vec::new();
        let mut origin = Vec::new();
        // A space waits for the word it separates, so leading and
        // trailing whitespace and empty runs fold away.
        let mut space = false;
        for (start, character) in nfc.char_indices() {
            if character.is_whitespace() {
                if !chars.is_empty() {
                    space = true;
                }
                continue;
            }
            if space {
                chars.push(' ');
                origin.push(None);
                space = false;
            }
            let end = start + character.len_utf8();
            for folded in any_ascii::any_ascii_char(character).chars().flat_map(char::to_lowercase) {
                chars.push(folded);
                origin.push(Some((start, end)));
            }
        }
        Folded { chars, origin }
    }
}

/// Where `query` matched `title` (with `subtitle`) at `sensitivity`, for
/// the window to highlight: byte ranges into `title`, in order and not
/// overlapping — the title characters of the best placement, when the
/// title, or a query spanning it and the subtitle, matched. A title that
/// holds the query's letters but does not pass the threshold highlights
/// nothing, as a result found by an alternate title, its keywords, its
/// subtitle or its package title does; so does a blank query. Compared
/// transliterated and lowercased, as matching compares, so a title whose
/// text only matches once folded (an accent, a ligature, collapsed
/// spaces) highlights what still matches as written.
pub fn title_matches(
    title: &str,
    subtitle: Option<&str>,
    query: &str,
    sensitivity: SearchSensitivity,
) -> Vec<std::ops::Range<usize>> {
    let query = Query::new(query);
    if query.text.is_empty() {
        return Vec::new();
    }
    let title = Folded::new(title);
    let subtitle = subtitle.map(Folded::new);
    // The texts whose placements can highlight the title: the title, and
    // the composites of it with the subtitle, so a query spanning both
    // highlights its letters in the title. Only the title's characters
    // map back; wherever a placement lands in the subtitle, it
    // highlights nothing.
    let mut texts: Vec<(Vec<char>, Vec<Option<(usize, usize)>>)> = Vec::new();
    if let Some(subtitle) = subtitle.filter(|subtitle| !subtitle.chars.is_empty()) {
        let joined = |first: &Folded, second: &Folded| {
            let mut chars = first.chars.clone();
            chars.push(' ');
            chars.extend(second.chars.iter().copied());
            let mut origin = first.origin.clone();
            origin.push(None);
            origin.extend(second.origin.iter().map(|_| None));
            (chars, origin)
        };
        texts.push(joined(&title, &subtitle));
        texts.push(joined(&subtitle, &title));
    }
    texts.push((title.chars, title.origin));
    // The best placement over the texts that pass the threshold.
    let mut best: Option<(i32, usize, Vec<Option<usize>>)> = None;
    for (index, (chars, _)) in texts.iter().enumerate() {
        if let Some(placed) = query.placed_in(chars.clone(), true) {
            let better = sensitivity.accepts(placed.score, query.letters)
                && best.as_ref().is_none_or(|(score, _, _)| placed.score > *score);
            if better {
                best = Some((placed.score, index, placed.at));
            }
        }
    }
    let Some((_, index, at)) = best else {
        return Vec::new();
    };
    let origin = &texts[index].1;
    let mut ranges: Vec<std::ops::Range<usize>> = at
        .into_iter()
        .filter_map(|at| at.and_then(|at| origin.get(at).copied().flatten()))
        .map(|(start, end)| start..end)
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
/// results — folded, fuzzy and thresholded (see the module docs) — at the
/// default sensitivity: the Search sensitivity setting governs root
/// search, and Settings' own search keeps the matcher's default.
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
    ranked_matches(&query, keys.iter(), SearchSensitivity::default())
}

#[cfg(test)]
mod alternate_tests {
    use super::{Keys, Query, SearchSensitivity, ranked_matches};

    fn matches(query: &str, keys: &[Keys]) -> Vec<usize> {
        ranked_matches(&Query::new(query), keys.iter(), SearchSensitivity::default())
    }

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn an_alternate_title_matches_as_the_title_does_and_the_best_counts() {
        let keys = [
            Keys::new("Windows Terminal", Some("Application"), None)
                .with_alternates(&strings(&["wt", "Terminal"]), &[]),
            Keys::new("Wtf Notes", Some("Application"), None),
            Keys::new("Paint", Some("Application"), None),
        ];
        // `wt` is Windows Terminal's alternate title exactly: it ranks
        // before a title merely starting with it.
        assert_eq!(matches("wt", &keys), [0, 1]);
        // `term` starts an alternate title: as a prefix of a title.
        assert_eq!(matches("term", &keys), [0]);
        assert!(matches("paintbrush", &keys).is_empty());
    }

    #[test]
    fn keywords_match_as_the_subtitle_does() {
        let keys = [
            Keys::new("Browser Notes", None, None),
            Keys::new("Firefox", Some("Application"), None)
                .with_alternates(&[], &strings(&["web browser", "internet"])),
        ];
        // The title match first, then the keyword's.
        assert_eq!(matches("browser", &keys), [0, 1]);
        assert_eq!(matches("internet", &keys), [1]);
        // A query may span a title's letters and a keyword's, through the
        // composite of the two.
        assert_eq!(matches("fire internet", &keys), [1]);
    }

    #[test]
    fn blank_alternates_and_keywords_find_nothing() {
        let keys = [Keys::new("Firefox", None, None)
            .with_alternates(&strings(&["", "  "]), &strings(&[" "]))];
        // A letter the title lacks: the blanks match it no more than any.
        assert!(matches("q", &keys).is_empty());
        assert_eq!(matches("fire", &keys), [0]);
    }
}

#[cfg(test)]
mod matching_tests {
    use super::{Keys, Query, SearchSensitivity, ranked_matches};

    fn matched(query: &str, keys: &[Keys], sensitivity: SearchSensitivity) -> Vec<usize> {
        ranked_matches(&Query::new(query), keys.iter(), sensitivity)
    }

    fn texts(listed: &[(&str, Option<&str>)]) -> Vec<Keys> {
        listed
            .iter()
            .map(|&(title, subtitle)| Keys::new(title, subtitle, None))
            .collect()
    }

    #[test]
    fn an_abbreviation_matches_and_a_tighter_placement_ranks_first() {
        let keys = texts(&[
            ("Visual Studio Code", None),
            ("Clear History", None),
            ("Clipboard History", None),
        ]);
        // "vsc" places in Visual Studio Code's word starts.
        assert_eq!(matched("vsc", &keys, SearchSensitivity::default()), [0]);
        // Both "clhis" placements are fuzzy: the tighter one first.
        assert_eq!(
            matched("clhis", &keys, SearchSensitivity::default()),
            [1, 2],
            "Clear History's letters sit closer than Clipboard's"
        );
    }

    #[test]
    fn accents_and_scripts_never_stand_between_the_query_and_the_result() {
        let keys = texts(&[
            ("Café", None),
            // "é" as "e" plus a combining accent, decomposed.
            ("Cafe\u{301} menu", None),
            ("Tiếng Việt", None),
            ("Đường", None),
            ("Straße", None),
        ]);
        assert_eq!(
            matched("cafe", &keys, SearchSensitivity::default()),
            [0, 1],
            "the query without the accent, the titles with and without"
        );
        assert_eq!(matched("café", &keys, SearchSensitivity::default()), [0, 1]);
        assert_eq!(
            matched("tieng viet", &keys, SearchSensitivity::default()),
            [2],
            "Vietnamese without diacritics"
        );
        assert_eq!(matched("duong", &keys, SearchSensitivity::default()), [3]);
        assert_eq!(matched("strasse", &keys, SearchSensitivity::default()), [4]);
    }

    #[test]
    fn a_query_may_span_the_title_and_the_subtitle() {
        let keys = texts(&[("Search YouTube", Some("Videos"))]);
        // The composite "title subtitle" holds the whole query, and so
        // does the composite the other way round.
        assert_eq!(matched("utub vid", &keys, SearchSensitivity::default()), [0]);
        assert_eq!(matched("vid utub", &keys, SearchSensitivity::default()), [0]);
    }

    #[test]
    fn each_sensitivity_admits_and_rejects_its_documented_cases() {
        let keys = texts(&[("Undownloadable files", None)]);
        // "download" sits mid-word: exactly 2n, which High's "more than
        // 2n" rejects; Medium's 1.5·(n−2)+4 = 13 and Low admit it.
        assert!(matched("download", &keys, SearchSensitivity::High).is_empty());
        assert_eq!(matched("download", &keys, SearchSensitivity::Medium), [0]);
        assert_eq!(matched("download", &keys, SearchSensitivity::Low), [0]);
        // "dwl" is scattered through the one word: only Low holds it.
        for sensitivity in [SearchSensitivity::High, SearchSensitivity::Medium] {
            assert!(matched("dwl", &keys, sensitivity).is_empty(), "{sensitivity:?}");
        }
        assert_eq!(matched("dwl", &keys, SearchSensitivity::Low), [0]);
        // High still holds a match that starts a word.
        let titled = texts(&[("Clipboard History", None)]);
        assert_eq!(matched("clhis", &titled, SearchSensitivity::default()), [0]);
    }

    #[test]
    fn aliases_are_compared_caselessly_not_transliterated() {
        let keys = [Keys::new("Downloader", None, None).with_alias(Some("café"))];
        // The accented alias is the user's own word: folding it away would
        // make "cafe" it, which it is not.
        assert!(!Query::new("cafe").is_alias_of(&keys[0]));
        assert!(Query::new("café").is_alias_of(&keys[0]));
        assert!(Query::new("CAFÉ").is_alias_of(&keys[0]));
    }
}

#[cfg(test)]
mod tests {
    use super::{SearchSensitivity, SettingsEntry, settings_matches, title_matches};

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
        // A word that no text holds matches nothing.
        assert!(titles("dark material").is_empty());
    }

    #[test]
    fn every_word_may_be_found_in_a_different_place() {
        // "dark" in the title, "theme" in the group: one result.
        assert_eq!(titles("dark theme"), vec!["Dark"]);
        // The page's title counts too, as a package's does in root search.
        assert_eq!(titles("glass appearance"), vec!["Glass"]);
    }

    // The arrays are the expected runs, one range each, not ranges to
    // collect.
    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn the_query_highlights_as_one_run_where_the_title_holds_it() {
        let high = SearchSensitivity::default();
        assert_eq!(title_matches("Clipboard History", None, "clip", high), [0..4]);
        assert_eq!(title_matches("Clipboard History", None, "CLIP", high), [0..4]);
        assert_eq!(title_matches("Clipboard History", None, "board hi", high), [4..12]);
        assert_eq!(title_matches("Clipboard History", None, "  hist ", high), [10..14]);
    }

    #[test]
    fn a_scattered_placement_highlights_each_of_its_runs() {
        let high = SearchSensitivity::default();
        // "clhis" places c, l at the start and h, i, s in "History".
        assert_eq!(
            title_matches("Clipboard History", None, "clhis", high),
            [0..2, 10..13]
        );
        // A title the query's letters do not place in highlights nothing.
        assert!(title_matches("Clipboard History", None, "pane", high).is_empty());
        assert!(title_matches("Clipboard History", None, " ", high).is_empty());
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn ranges_are_bytes_of_the_title_as_written() {
        let high = SearchSensitivity::default();
        // Ä is two bytes, folded to a of one; the range covers both.
        assert_eq!(title_matches("Ärger übersetzen", None, "är", high), [0..3]);
        assert_eq!(title_matches("Ärger übersetzen", None, "über", high), [7..12]);
        // "straße" is "Straße" folded: the whole title is the placement.
        assert_eq!(title_matches("Straße", None, "straße", high), [0..7]);
        // ß folds to two characters, so "ss" highlights it whole — a
        // placement only Medium holds.
        let medium = SearchSensitivity::Medium;
        let title = "Straße";
        let ranges = title_matches(title, None, "ss", medium);
        assert_eq!(ranges, [4..6]);
        assert_eq!(&title[ranges[0].clone()], "ß");
    }

    #[test]
    fn transliterated_matches_highlight_what_was_written() {
        let high = SearchSensitivity::default();
        // "duong" is "Đường" folded: the whole title is the placement.
        let title = "Đường";
        let ranges = title_matches(title, None, "duong", high);
        assert_eq!(ranges, [0..title.len()]);
        assert_eq!(&title[ranges[0].clone()], "Đường");
        // A query spanning the title and the subtitle highlights the
        // title's letters of its placement.
        assert_eq!(
            title_matches("Search YouTube", Some("Videos"), "utub vid", high),
            [9..13]
        );
        assert_eq!(&"Search YouTube"[9..13], "utub");
    }

    #[test]
    fn a_title_that_does_not_pass_highlights_nothing() {
        let high = SearchSensitivity::default();
        // "download" places in "Undownloadable files" mid-word: exactly
        // 2n, which High rejects, so nothing highlights — while Medium
        // holds it, as one run.
        assert!(title_matches("Undownloadable files", None, "download", high).is_empty());
        assert_eq!(
            title_matches("Undownloadable files", None, "download", SearchSensitivity::Medium),
            [2..10]
        );
        // A query found only in the subtitle highlights nothing in the
        // title: its placements in the composites land past the title.
        let matched =
            title_matches("Clear cache", Some("Delete downloaded files"), "del files", high);
        assert!(matched.is_empty());
    }
}
