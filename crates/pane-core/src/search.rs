//! Matching and ranking for root search.
//!
//! A query is matched against each result's texts — its title, each
//! alternate title, its subtitle, each keyword, the composites "title
//! subtitle" and "subtitle title", and, for an installed command, its
//! package's title — as a subsequence with a score: a
//! letter matched at the text's first position scores 4, at a word
//! start (after a separator) 3, elsewhere 2; a separator matched to a
//! separator scores 1. Each gap between two consecutive matched
//! positions costs 1; adjacency is free. A query separator that cannot
//! be placed is skipped, counted as skipped and never a failure, while
//! a letter that cannot be placed means no match. A query with no
//! letters at all matches nothing: there is nothing to place, and a row
//! of separators alone does not list every result that holds one. The
//! best placement's score counts, and an exact equality of the folded
//! query and a folded text is its own outcome, the best one. A query
//! starting with "/" treats the first "/" in a text as a space.
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
//! untranslated or program name), each matched as the title is, the best
//! of them counting, and keywords, each its own text matched as the
//! subtitle is; the row still shows its title. A command's keywords are
//! its manifest's — an author's search terms, distinct from the user's
//! aliases.
//!
//! A result matches when its alias matches — the query is it, or starts
//! it — or one of its texts passes the sensitivity's threshold. Matches
//! are then ordered by the comparator, the first difference winning:
//!
//! 1. the query is the result's alias;
//! 2. the query is longer than three characters and is exactly the title
//!    or an alternate title;
//! 3. the query is exactly one of the result's learned queries (#199);
//! 4. the query is exactly the subtitle (a keyword counts: keywords rank
//!    as subtitles);
//! 5. the alias starts with the query;
//! 6. a learned query starts with the query (#199);
//! 7. the query starts with a learned query of at least three characters,
//!    the longer learned query winning (#199);
//! 8. the higher of the title, alternate-title and subtitle scores (a
//!    keyword's counts where the subtitle's does);
//! 9. higher frecency (#199);
//! 10. higher title score;
//! 11. kind priority — commands above links, above applications, above
//!     files;
//! 12. the provider's own order;
//! 13. the title, with digits compared by their value ("Item 2" before
//!     "Item 10").
//!
//! What follows the last step is the **no-query order** — the blank
//! query's own order, and the last tiebreak of any query's: frecency,
//! then having an alias, then kind priority, then the provider's own
//! order, then the title. Results the comparator cannot tell apart keep
//! the order they were given — the provider's own, within one provider.
//! No typo tolerance; what is learned is the launcher's to give
//! ([`Learned`]).
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

/// What root search learned about a result the user has chosen from it
/// (ADR 0030, #199): its frecency and the queries it was last chosen
/// with, as the launcher holds them for the search this is. `None` for a
/// result never chosen, which scores 1 wherever frecency weighs and
/// holds no queries: it ranks as any other.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Learned {
    /// The frecency score of the result's uses, decayed to the moment of
    /// the search and floored at 1, an unused result's score.
    pub(crate) frecency: f64,
    /// The queries the result was last chosen with, newest first, each
    /// distinct and folded as the query is matched. The launcher leaves
    /// them empty while they stop counting — the frecency decayed to 1,
    /// or the result was last opened more than 17 days ago — so ranking
    /// never weighs them then; it knows the clock, this module does not.
    pub(crate) queries: Vec<String>,
}

/// What kind of thing a result is, as the comparator's kind step ranks
/// it: commands — installed and Pane's own — above links (quicklinks),
/// above applications, above files. Files are never ranked by the
/// comparator (they keep their place below the results); the kind
/// completes the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Command,
    Link,
    Application,
    File,
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
/// Việt". The launcher also folds titles to tell rows of one title apart.
pub(crate) fn fold(text: &str) -> String {
    if text.is_ascii() {
        return text
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
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
    /// The subtitle, as it is matched.
    subtitle: String,
    /// The keywords, each its own folded text, matched as the subtitle
    /// is: an author's search terms, distinct from the user's aliases.
    keywords: Vec<String>,
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
}

impl Title {
    fn new(title: &str) -> Title {
        Title { text: fold(title) }
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
            keywords: Vec::new(),
            package: package.map(fold).unwrap_or_default(),
            alias: None,
        }
    }

    /// These keys, also matched by `alternate_titles` as the title is and
    /// by each of `keywords` as the subtitle is. Blank ones are ignored.
    pub(crate) fn with_alternates(self, alternate_titles: &[String], keywords: &[String]) -> Keys {
        Keys {
            alternates: alternate_titles
                .iter()
                .map(|title| Title::new(title))
                .filter(|title| !title.text.is_empty())
                .collect(),
            keywords: keywords
                .iter()
                .map(|keyword| fold(keyword))
                .filter(|keyword| !keyword.is_empty())
                .collect(),
            ..self
        }
    }

    /// These keys, also matched by each of `keywords` as the subtitle
    /// is: a command's manifest keywords. Blank ones are ignored.
    pub(crate) fn with_keywords(self, keywords: &[String]) -> Keys {
        Keys {
            keywords: keywords
                .iter()
                .map(|keyword| fold(keyword))
                .filter(|keyword| !keyword.is_empty())
                .collect(),
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
    /// `sensitivity`: the title, each alternate title, the subtitle, each
    /// keyword, the composites of the title and the subtitle, or the
    /// package title. `None` when no text holds the query's letters in
    /// order, or none of their best placements passes the sensitivity's
    /// threshold — which is the same thing, since every threshold is
    /// monotone in the score.
    fn scored(&self, query: &Query, sensitivity: SearchSensitivity) -> Option<i32> {
        let texts = self.texts();
        let best = texts.filter_map(|text| query.score(text)).max();
        best.filter(|&score| sensitivity.accepts(score, query.letters))
    }

    /// Every text the query is matched against, each a whole text of its
    /// own: the title, each alternate title, the subtitle, each keyword,
    /// the composites of the title and the subtitle, and the package
    /// title.
    fn texts(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.title.text.as_str())
            .chain(self.alternates.iter().map(|title| title.text.as_str()))
            .chain(std::iter::once(self.subtitle.as_str()))
            .chain(self.keywords.iter().map(String::as_str))
            .chain([self.composites[0].as_str(), self.composites[1].as_str()])
            .chain([self.package.as_str()])
    }

    /// The texts whose scores the comparator's eighth step weighs: the
    /// title, each alternate title, the subtitle and each keyword. A
    /// match the composites or the package title found holds none of
    /// them, and ranks by whatever else it has.
    fn ranked_texts(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.title.text.as_str())
            .chain(self.alternates.iter().map(|title| title.text.as_str()))
            .chain(std::iter::once(self.subtitle.as_str()))
            .chain(self.keywords.iter().map(String::as_str))
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
    /// Whether the query, as the user typed it, is longer than three
    /// characters: only then does an exact title match count as one.
    long: bool,
    /// How many of the query's characters are not separators: the n of
    /// the sensitivity thresholds. Separators are the only characters
    /// the scorer may skip, so they are not counted.
    letters: usize,
    /// The folded query's length in characters: the bound on how much
    /// longer than a learned query the query may be and still count as
    /// starting with it (step 7, #199).
    length: usize,
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
        let long = plain.chars().count() > 3;
        let letters = text.chars().filter(|c| !is_separator(*c)).count();
        let length = text.chars().count();
        Query {
            long,
            letters,
            length,
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

    /// Whether the alias the user gave the result with `keys` starts with
    /// this query (a prefix, not the alias itself: [`Query::is_alias_of`]),
    /// compared caselessly as aliases are. A result is matched by such a
    /// prefix even when no text of it holds the query's letters.
    pub(crate) fn is_prefix_of_alias_of(&self, keys: &Keys) -> bool {
        !self.plain.is_empty()
            && keys
                .alias
                .as_deref()
                .is_some_and(|alias| alias.starts_with(&self.plain))
    }

    /// Whether this query, longer than three characters, is exactly the
    /// title or an alternate title of the result with `keys`, folded.
    fn is_title_of(&self, keys: &Keys) -> bool {
        self.long
            && (keys.title.text == self.text
                || keys.alternates.iter().any(|title| title.text == self.text))
    }

    /// Whether this query is exactly the subtitle or a keyword of the
    /// result with `keys`, folded: keywords rank as subtitles.
    fn is_subtitle_of(&self, keys: &Keys) -> bool {
        keys.subtitle == self.text || keys.keywords.iter().any(|keyword| keyword == &self.text)
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
    /// A query with no letters at all matches nothing: there is nothing
    /// to place, and a row of separators alone should not list every
    /// result that holds one.
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
        if self.letters == 0 || chars.is_empty() {
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
    query.iter().filter(|c| !is_separator(**c)).all(|&c| {
        match text[at..].iter().position(|&t| t == c) {
            Some(found) => {
                at += found + 1;
                true
            }
            None => false,
        }
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
            if winner
                .as_ref()
                .is_none_or(|&(wi, wj)| here > dp[wi * m + wj])
            {
                winner = Some((i, j));
            }
        }
    }
    let (i, j) = winner?;
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

/// One result as the comparator ranks it, beside its matched texts:
/// what kind of thing it is, and the position of the provider that
/// supplied it among the others — Pane's root list first, then each
/// command that supplies results ahead of the query, in the order they
/// were first asked. A candidate's kind comes from what activating it
/// does, never from its title.
pub(crate) struct Candidate<'a> {
    pub(crate) keys: &'a Keys,
    pub(crate) kind: Kind,
    /// The provider's position: the comparator's twelfth step, ahead of
    /// the title comparison, so one provider's results stay together and
    /// rows of it the title cannot tell apart keep the order it gave.
    pub(crate) provider: usize,
    /// What root search learned about the result (#199, [`Learned`]);
    /// `None` for one never chosen, which ranks as any other at every
    /// step learning weighs.
    pub(crate) learned: Option<&'a Learned>,
}

/// The indices of the `candidates` that match `query` at `sensitivity`,
/// in the comparator's order, the first difference winning; candidates
/// the comparator cannot tell apart keep the order they were given. A
/// blank query matches every candidate, in the no-query order (see the
/// module docs).
pub(crate) fn ranked_matches<'a>(
    query: &Query,
    candidates: impl ExactSizeIterator<Item = Candidate<'a>>,
    sensitivity: SearchSensitivity,
) -> Vec<usize> {
    if query.text.is_empty() {
        // The blank query matches everything: the no-query order is its
        // whole order — frecency, then having an alias, then kind
        // priority, then the provider's own order, then the title.
        let mut ranked: Vec<(usize, Ranked)> = candidates
            .enumerate()
            .map(|(index, candidate)| (index, Ranked::of(query, &candidate)))
            .collect();
        ranked.sort_by(|(_, a), (_, b)| no_query(a, b));
        return ranked.into_iter().map(|(index, _)| index).collect();
    }
    // A result matches through its alias — the query is it, or starts it
    // — even when no text of it passes the threshold.
    let matches = |candidate: &Candidate| {
        query.is_alias_of(candidate.keys)
            || query.is_prefix_of_alias_of(candidate.keys)
            || candidate.keys.scored(query, sensitivity).is_some()
    };
    let mut ranked: Vec<(usize, Ranked)> = candidates
        .enumerate()
        .filter(|(_, candidate)| matches(candidate))
        .map(|(index, candidate)| (index, Ranked::of(query, &candidate)))
        .collect();
    ranked.sort_by(|(_, a), (_, b)| compare(a, b));
    ranked.into_iter().map(|(index, _)| index).collect()
}

/// One matching result, as the comparator ranks it: every step's key,
/// computed once per result for a query.
struct Ranked<'a> {
    /// Step 1: the query is the result's alias.
    alias: bool,
    /// Step 2: the query is longer than three characters and is exactly
    /// the title or an alternate title.
    exact_title: bool,
    /// Step 3: the query is exactly one of the result's learned queries.
    learned_query: bool,
    /// Step 4: the query is exactly the subtitle, or a keyword.
    exact_subtitle: bool,
    /// Step 5: the alias starts with the query.
    alias_prefix: bool,
    /// Step 6: one of the result's learned queries starts with the query.
    learned_prefix: bool,
    /// Step 7: the longest learned query of at least three characters the
    /// query starts with, while the query is at most three characters
    /// longer than it, by its length (an "overbounds" match); `None`
    /// when none qualifies.
    overbounds: Option<usize>,
    /// Step 8: the best score the query places in the title, an
    /// alternate title, the subtitle or a keyword; `None` when none of
    /// them holds the query's letters in order (a match the composites
    /// or the package title found).
    best: Option<i32>,
    /// Step 9, and the no-query order's first key: the frecency of the
    /// result's uses, decayed to now and floored at 1.
    frecency: f64,
    /// Step 10: the best score the query places in the title alone.
    title: Option<i32>,
    /// Step 11.
    kind: Kind,
    /// Step 12.
    provider: usize,
    /// The no-query order's second key: whether the user gave the result
    /// an alias.
    aliased: bool,
    /// The folded title, for the last step's collation.
    folded_title: &'a str,
}

impl<'a> Ranked<'a> {
    /// The comparator's keys for `candidate` under `query`.
    fn of(query: &Query, candidate: &Candidate<'a>) -> Ranked<'a> {
        let keys = candidate.keys;
        // What was learned about the result (#199): a result never chosen
        // scores 1 wherever frecency weighs, and holds no queries.
        let learned = candidate.learned;
        let queries = learned
            .map(|learned| learned.queries.as_slice())
            .unwrap_or_default();
        Ranked {
            alias: query.is_alias_of(keys),
            exact_title: query.is_title_of(keys),
            // A learned query is folded as the query is; an empty one
            // counts for nothing ("the last three distinct non-empty
            // queries").
            learned_query: !query.text.is_empty()
                && queries.iter().any(|learned| learned == &query.text),
            exact_subtitle: query.is_subtitle_of(keys),
            alias_prefix: query.is_prefix_of_alias_of(keys),
            learned_prefix: !query.text.is_empty()
                && queries
                    .iter()
                    .any(|learned| learned.starts_with(query.text.as_str())),
            overbounds: queries
                .iter()
                .map(|learned| learned.as_str())
                // At least three characters, and the query at most three
                // longer than it: ignored when it is more.
                .filter(|learned| {
                    query.text.starts_with(*learned)
                        && learned.chars().count() >= 3
                        && query.length <= learned.chars().count() + 3
                })
                .map(|learned| learned.chars().count())
                .max(),
            best: keys
                .ranked_texts()
                .filter_map(|text| query.score(text))
                .max(),
            title: query.score(&keys.title.text),
            frecency: learned.map_or(1.0, |learned| learned.frecency),
            kind: candidate.kind,
            provider: candidate.provider,
            aliased: keys.alias.is_some(),
            folded_title: &keys.title.text,
        }
    }
}

/// How two matching results stand against each other, the first
/// difference winning — the parent's comparator, with the steps it left
/// to learning (#199) filled in.
fn compare(a: &Ranked, b: &Ranked) -> std::cmp::Ordering {
    // 1. the query is the result's alias; 2. the query is longer than
    // three characters and is exactly the title or an alternate title;
    // 3. the query is exactly one of the result's learned queries; 4. the
    // query is exactly the subtitle; 5. the alias starts with the query;
    // 6. a learned query starts with the query; 7. the query starts with
    // a learned query of at least three characters, the longer learned
    // query winning; 8. the higher of the title, alternate-title and
    // subtitle scores; 9. higher frecency; 10. higher title score.
    let matched = b
        .alias
        .cmp(&a.alias)
        .then(b.exact_title.cmp(&a.exact_title))
        .then(b.learned_query.cmp(&a.learned_query))
        .then(b.exact_subtitle.cmp(&a.exact_subtitle))
        .then(b.alias_prefix.cmp(&a.alias_prefix))
        .then(b.learned_prefix.cmp(&a.learned_prefix))
        .then(b.overbounds.cmp(&a.overbounds))
        .then(b.best.cmp(&a.best))
        .then_with(|| b.frecency.total_cmp(&a.frecency))
        .then(b.title.cmp(&a.title));
    // 11. kind priority: commands above links, above applications, above
    // files; 12. the provider's own order; 13. the title, with digits
    // compared by their value; then the no-query order, the last
    // tiebreak.
    matched
        .then(a.kind.cmp(&b.kind))
        .then(a.provider.cmp(&b.provider))
        .then_with(|| collate(a.folded_title, b.folded_title))
        .then_with(|| no_query(a, b))
}

/// The no-query order — the blank query's whole order, and the
/// comparator's last tiebreak: frecency, then having an alias, then kind
/// priority, then the provider's own order, then the title. Wherever it
/// follows the comparator its frecency, kind, provider and title steps
/// have already compared equal; its alias step is the key that then
/// tells the results apart.
fn no_query(a: &Ranked, b: &Ranked) -> std::cmp::Ordering {
    b.frecency
        .total_cmp(&a.frecency)
        .then(b.aliased.cmp(&a.aliased))
        .then(a.kind.cmp(&b.kind))
        .then(a.provider.cmp(&b.provider))
        .then_with(|| collate(a.folded_title, b.folded_title))
}

/// How `a` and `b` stand as titles, compared with digits by their value
/// ("Item 2" before "Item 10"): a run of digits on both sides counts as
/// its number, everything else character by character.
fn collate(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a, b);
    loop {
        let (x, y) = (a.chars().next(), b.chars().next());
        let (Some(x), Some(y)) = (x, y) else {
            // The shorter title first, when one is a prefix of the other.
            return a.len().cmp(&b.len());
        };
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let a_run = a.chars().take_while(|c| c.is_ascii_digit()).count();
            let b_run = b.chars().take_while(|c| c.is_ascii_digit()).count();
            // Leading zeros aside, a longer run of digits is the bigger
            // number; equal lengths compare their digits, which are
            // their number's.
            let (a_digits, b_digits) = (
                a[..a_run].trim_start_matches('0'),
                b[..b_run].trim_start_matches('0'),
            );
            let by_number = a_digits
                .len()
                .cmp(&b_digits.len())
                .then_with(|| a_digits.cmp(b_digits));
            if by_number != std::cmp::Ordering::Equal {
                return by_number;
            }
            a = &a[a_run..];
            b = &b[b_run..];
            continue;
        }
        if x != y {
            return x.cmp(&y);
        }
        a = &a[x.len_utf8()..];
        b = &b[y.len_utf8()..];
    }
}

/// One text as it is highlighted: folded character by character, each
/// folded character carrying the byte range of the character of the text
/// as written that it came from, so the positions the scorer finds map
/// back to what the user sees. `None` marks a character that maps to
/// nothing highlightable: the space a collapsed run folded to (a
/// composite's, or one the placement never used) and the second half of
/// a composite. Whitespace collapses as [`fold`] collapses it.
struct Folded {
    chars: Vec<char>,
    origin: Vec<Option<(usize, usize)>>,
}

impl Folded {
    fn new(text: &str) -> Folded {
        let nfc: String = text.nfc().collect();
        let mut chars = Vec::new();
        let mut origin = Vec::new();
        // A space waits for the word it separates — carrying the range of
        // the first whitespace of the run it collapsed — so leading and
        // trailing whitespace and empty runs fold away.
        let mut space: Option<(usize, usize)> = None;
        for (start, character) in nfc.char_indices() {
            if character.is_whitespace() {
                let end = start + character.len_utf8();
                if !chars.is_empty() && space.is_none() {
                    space = Some((start, end));
                }
                continue;
            }
            if let Some(range) = space.take() {
                chars.push(' ');
                origin.push(Some(range));
            }
            let end = start + character.len_utf8();
            let transliterated = any_ascii::any_ascii_char(character);
            for folded in transliterated.chars().flat_map(char::to_lowercase) {
                chars.push(folded);
                origin.push(Some((start, end)));
            }
        }
        Folded { chars, origin }
    }
}

/// The composite of `title` and `subtitle`, `title` first: a text a query
/// can span, where only the title's characters keep their origins,
/// wherever they sit — the subtitle's half maps to nothing.
fn composite(title: &Folded, subtitle: &Folded, title_first: bool) -> Folded {
    let (first, second) = if title_first {
        (&title.chars, &subtitle.chars)
    } else {
        (&subtitle.chars, &title.chars)
    };
    let mut chars = first.clone();
    chars.push(' ');
    chars.extend(second.iter().copied());
    let mut origin: Vec<Option<(usize, usize)>> = if title_first {
        title.origin.clone()
    } else {
        std::iter::repeat_n(None, subtitle.chars.len()).collect()
    };
    origin.push(None);
    origin.extend(if title_first {
        std::iter::repeat_n(None, subtitle.chars.len()).collect::<Vec<_>>()
    } else {
        title.origin.clone()
    });
    Folded { chars, origin }
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
    let mut texts: Vec<Folded> = Vec::new();
    if let Some(subtitle) = subtitle.filter(|subtitle| !subtitle.chars.is_empty()) {
        texts.push(composite(&title, &subtitle, true));
        texts.push(composite(&title, &subtitle, false));
    }
    texts.push(title);
    // The best placement over the texts that pass the threshold.
    let mut best: Option<(i32, usize, Vec<Option<usize>>)> = None;
    for (index, text) in texts.iter().enumerate() {
        if let Some(placed) = query.placed_in(text.chars.clone(), true) {
            let better = sensitivity.accepts(placed.score, query.letters)
                && best
                    .as_ref()
                    .is_none_or(|(score, _, _)| placed.score > *score);
            if better {
                best = Some((placed.score, index, placed.at));
            }
        }
    }
    let Some((_, index, at)) = best else {
        return Vec::new();
    };
    let origin = &texts[index].origin;
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
///
/// Every entry is one kind of thing and one provider as far as the
/// comparator's kind and provider steps can see, so those steps never
/// distinguish entries; the steps that do — an exact title, an exact
/// group, the scores, the title's collation — order them as they order
/// root search's results.
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
    let candidates = keys
        .iter()
        .map(|keys| Candidate {
            keys,
            kind: Kind::Command,
            provider: 0,
            learned: None,
        })
        .collect::<Vec<_>>();
    let query = Query::new(query);
    ranked_matches(&query, candidates.into_iter(), SearchSensitivity::default())
}

#[cfg(test)]
mod alternate_tests {
    use super::{Candidate, Keys, Kind, Query, SearchSensitivity, ranked_matches};

    fn matches(query: &str, keys: &[Keys]) -> Vec<usize> {
        let candidates = keys
            .iter()
            .map(|keys| Candidate {
                keys,
                kind: Kind::Command,
                provider: 0,
                learned: None,
            })
            .collect::<Vec<_>>();
        ranked_matches(
            &Query::new(query),
            candidates.into_iter(),
            SearchSensitivity::default(),
        )
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
        // `wt` is Windows Terminal's alternate title: the title it
        // prefixes ("windows terminal") scores better than a title
        // merely starting with it.
        assert_eq!(matches("wt", &keys), [0, 1]);
        // `term` starts an alternate title.
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
        // A keyword is its own text: a query spanning a title's letters
        // and a keyword's holds in no single text, and matches nothing.
        assert!(matches("fire internet", &keys).is_empty());
    }

    #[test]
    fn a_keyword_found_exactly_ranks_as_the_subtitle_does() {
        let keys = [
            Keys::new("Notes", Some("Write things down"), None),
            Keys::new("Firefox", Some("Application"), None)
                .with_alternates(&[], &strings(&["browser"])),
        ];
        // "browser" is one of Firefox's keywords exactly: the
        // exact-subtitle step holds it, as it would a subtitle.
        assert_eq!(matches("browser", &keys), [1]);
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
    use super::{Candidate, Keys, Kind, Query, SearchSensitivity, ranked_matches};

    fn matched(query: &str, keys: &[Keys], sensitivity: SearchSensitivity) -> Vec<usize> {
        let candidates = keys
            .iter()
            .map(|keys| Candidate {
                keys,
                kind: Kind::Command,
                provider: 0,
                learned: None,
            })
            .collect::<Vec<_>>();
        ranked_matches(&Query::new(query), candidates.into_iter(), sensitivity)
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
        assert_eq!(
            matched("utub vid", &keys, SearchSensitivity::default()),
            [0]
        );
        assert_eq!(
            matched("vid utub", &keys, SearchSensitivity::default()),
            [0]
        );
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
            assert!(
                matched("dwl", &keys, sensitivity).is_empty(),
                "{sensitivity:?}"
            );
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
            vec![0, 2, 3, 4, 1],
            "the window decides what an empty query shows; the core just ranks, by the \
             no-query order (#199): title collation here, everything else tying"
        );
        assert_eq!(
            settings_matches("   ", &catalog()),
            vec![0, 2, 3, 4, 1],
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
        // beneath that — the same order root search's results keep. The
        // group that is the query exactly ranks above the better-scoring
        // description of it, as the subtitle does (step 4).
        assert_eq!(
            titles("material"),
            vec!["Glass", "Appearance"],
            "Glass's group is “Material”, and the Appearance page's description names it"
        );
        assert_eq!(titles("shortcuts"), vec!["Shortcuts"]);
        // A word that no text holds matches nothing.
        assert!(titles("dark material").is_empty());
    }

    #[test]
    fn every_word_may_be_found_in_a_different_place() {
        // "dark" in the title, "theme" in the group: one result.
        assert_eq!(titles("dark theme"), vec!["Dark"]);
        // "glass" in the title, "material" in the group: one result, the
        // composite of the two an exact match.
        assert_eq!(titles("glass material"), vec!["Glass"]);
    }

    // The arrays are the expected runs, one range each, not ranges to
    // collect.
    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn the_query_highlights_as_one_run_where_the_title_holds_it() {
        let high = SearchSensitivity::default();
        assert_eq!(
            title_matches("Clipboard History", None, "clip", high),
            [0..4]
        );
        assert_eq!(
            title_matches("Clipboard History", None, "CLIP", high),
            [0..4]
        );
        assert_eq!(
            title_matches("Clipboard History", None, "board hi", high),
            [4..12]
        );
        assert_eq!(
            title_matches("Clipboard History", None, "  hist ", high),
            [10..14]
        );
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
        assert_eq!(
            title_matches("Ärger übersetzen", None, "über", high),
            [7..12]
        );
        // "straße" is "Straße" folded: the whole title is the placement.
        assert_eq!(title_matches("Straße", None, "straße", high), [0..7]);
        // ß folds to two characters, so "ss" highlights it whole — a
        // placement only Medium holds, whose best run starts at the
        // title's first letter.
        let medium = SearchSensitivity::Medium;
        let title = "Straße";
        let ranges = title_matches(title, None, "ss", medium);
        assert_eq!(ranges, [0..1, 4..6]);
        assert_eq!(&title[ranges[1].clone()], "ß");
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
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
        assert_eq!(&"Search YouTube"[9..13], "uTub");
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn a_title_that_does_not_pass_highlights_nothing() {
        let high = SearchSensitivity::default();
        // "download" places in "Undownloadable files" mid-word: exactly
        // 2n, which High rejects, so nothing highlights — while Medium
        // holds it, as one run.
        assert!(title_matches("Undownloadable files", None, "download", high).is_empty());
        assert_eq!(
            title_matches(
                "Undownloadable files",
                None,
                "download",
                SearchSensitivity::Medium
            ),
            [2..10]
        );
        // A query found only in the subtitle highlights nothing in the
        // title: its placements in the composites land past the title.
        let matched = title_matches(
            "Clear cache",
            Some("Delete downloaded files"),
            "del files",
            high,
        );
        assert!(matched.is_empty());
    }
}
