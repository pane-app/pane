//! How the window presents root search's rows: a narrow, read-only
//! projection of what the launcher already knows about each row beyond
//! its title and subtitle — what kind of thing it is, the alias and the
//! global hotkey the user gave its command, where the query matched its
//! title, the answer a command computed from the query, its icon — and
//! how the rows group under section labels.
//!
//! Nothing here changes what root search lists, in which order, or what a
//! row does: the projection is computed from the same state the rows and
//! their entries come from, row for row. A row's kind comes from what
//! activating it does (its entry), never from its title; a part of the
//! projection the launcher has no data for is absent, not guessed. Root
//! search is projected, and an opened command's own list for how its items
//! look (#139: their icons, tooltips and accessories, see `looks`): the
//! rows of a command's search results, Manage extensions and the other
//! screens present as they always did, with no kind, alias, hotkey, match,
//! icon or section.

use std::ops::Range;

use super::aliases::{Sending, Via};
use super::looks::{self, ShownAccessory};
use super::{Entry, Row, Screen, State};
use crate::hotkeys::Shortcut;
use crate::icons::Icon;
use crate::search::title_matches;

/// What kind of thing a root row is, from what activating it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// A command — an extension's, or one of Pane's own rows.
    Command,
    /// An installed application, found ahead of the query.
    Application,
    /// A file a command found for the query.
    File,
    /// A web address a command answered with.
    Link,
    /// A fallback: a command the user chose to offer any text to.
    Fallback,
}

impl RowKind {
    /// The kind as the row's trailing label names it.
    pub fn label(self) -> &'static str {
        match self {
            RowKind::Command => "Command",
            RowKind::Application => "Application",
            RowKind::File => "File",
            RowKind::Link => "Link",
            RowKind::Fallback => "Fallback",
        }
    }
}

/// One row's presentation, beside its [`super::Row`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowPresentation {
    /// What kind of thing the row is; `None` where activating it says
    /// nothing a label could (a computed answer, an explanation).
    pub kind: Option<RowKind>,
    /// The alias the user gave the row's command, while it is active.
    pub alias: Option<String>,
    /// The global hotkey the user gave the row's command, while it is
    /// registered with the system — the one that actually opens it.
    pub hotkey: Option<Shortcut>,
    /// Where the query matched the row's title: byte ranges into the
    /// title, in order and not overlapping. Empty for a blank query, or a
    /// row found by its subtitle, package or alias alone.
    pub matched: Vec<Range<usize>>,
    /// The answer the row is, when a command computed it from the query
    /// and activating it copies it (see [`ComputedAnswer`]).
    pub answer: Option<ComputedAnswer>,
    /// Whether the row's command needs setup: a required preference of
    /// its package's or its own is unset, so only a launch by the user,
    /// through the Setup screen, runs it (see the launcher's `setup`). The
    /// row says "Needs setup".
    pub needs_setup: bool,
    /// The row's icon, drawn bare (#139): an installed command's in root
    /// search (its own, its package's, or its package's first-letter
    /// tile), or an item's in an opened command's list. `None` for Pane's
    /// own rows, which keep their tiles, and an item without one.
    pub icon: Option<Icon>,
    /// Shown when the pointer rests on the row's title (an item's).
    pub title_tooltip: Option<String>,
    /// Shown when the pointer rests on the row's subtitle (an item's).
    pub subtitle_tooltip: Option<String>,
    /// What the row shows on its right (an item's, at most
    /// [`crate::runtime::MAX_ACCESSORIES`]), dates relative to now.
    pub accessories: Vec<ShownAccessory>,
}

/// A computed answer: a root result a command computed from the query
/// whose action copies its text, such as the calculator's answer to
/// "6*7". Only what the launcher holds: the query it answers, the text
/// activating it copies and the command that computed it — no units,
/// conversions or history, which no command supplies. The row keeps its
/// id and its copy action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputedAnswer {
    /// The query it answers, without its surrounding spaces.
    pub query: String,
    /// The text activating it copies.
    pub answer: String,
    /// The title of the command that computed it ("Calculator").
    pub command: String,
}

/// A section label over a run of rows: the rows from `first` up to the
/// next section's `first` (or the end).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// The label's title ("Commands", "Results").
    pub label: String,
    /// The label's note on its right ("3 matches"), if any.
    pub note: Option<String>,
    /// The index of the section's first row.
    pub first: usize,
}

/// The rows' presentation and their sections, as the launcher's view
/// lists them now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Presentation {
    /// One entry per row of [`super::LauncherView::rows`], in order.
    pub rows: Vec<RowPresentation>,
    /// The section labels, in order; empty off root search.
    pub sections: Vec<Section>,
}

/// The presentation of `state`'s rows.
pub(super) fn presentation(state: &State) -> Presentation {
    let listed = match &state.view.screen {
        Screen::Command => true,
        Screen::CommandSearch { query } => query.trim().is_empty(),
        _ => false,
    };
    if listed {
        // An opened command's own list: how its items look.
        let now = state.clock.now();
        let rows = state
            .view
            .rows
            .iter()
            .map(|row| match state.looks.of(&row.id) {
                Some(look) => RowPresentation {
                    // Web images and system icons as they are now (#142).
                    icon: look
                        .icon
                        .as_ref()
                        .map(|icon| looks::shown_icon(state, icon)),
                    title_tooltip: look.title_tooltip.clone(),
                    subtitle_tooltip: look.subtitle_tooltip.clone(),
                    accessories: looks::shown_accessories(look, now)
                        .into_iter()
                        .map(|accessory| ShownAccessory {
                            icon: accessory
                                .icon
                                .as_ref()
                                .map(|icon| looks::shown_icon(state, icon)),
                            ..accessory
                        })
                        .collect(),
                    ..RowPresentation::default()
                },
                None => RowPresentation::default(),
            })
            .collect();
        return Presentation {
            rows,
            sections: Vec::new(),
        };
    }
    let Screen::Root { query } = &state.view.screen else {
        return Presentation {
            rows: vec![RowPresentation::default(); state.view.rows.len()],
            sections: Vec::new(),
        };
    };
    let rows = state
        .view
        .rows
        .iter()
        .zip(&state.entries)
        .map(|(row, entry)| {
            let command = matches!(entry, Entry::Open(_) | Entry::Unavailable(_));
            RowPresentation {
                kind: kind(entry),
                alias: command
                    .then(|| state.aliases.chosen.active_alias(&row.id))
                    .flatten()
                    .map(str::to_owned),
                hotkey: command
                    .then(|| state.bindings.registered_of(&row.id))
                    .flatten(),
                matched: title_matches(&row.title, query),
                answer: answer(state, row, entry, query),
                needs_setup: matches!(entry, Entry::Open(_))
                    && state.setup_needed.contains(&row.id),
                icon: icon(state, row, entry),
                ..RowPresentation::default()
            }
        })
        .collect::<Vec<_>>();
    let shown = state.view.rows.len();
    let first_fallback = state
        .entries
        .iter()
        .position(|entry| {
            matches!(
                entry,
                Entry::Send(Sending {
                    via: Via::Fallback,
                    ..
                })
            )
        })
        .unwrap_or(shown);
    let answers: Vec<Option<&str>> = rows
        .iter()
        .map(|row| row.answer.as_ref().map(|answer| answer.command.as_str()))
        .collect();
    let sections = answer_sections(query, &answers, first_fallback);
    Presentation { rows, sections }
}

/// The computed answer `row` is, when `entry` copies text a command
/// computed from `query`.
fn answer(state: &State, row: &Row, entry: &Entry, query: &str) -> Option<ComputedAnswer> {
    let Entry::Copy(text) = entry else {
        return None;
    };
    let computed = state
        .computed
        .iter()
        .find(|computed| computed.row.id == row.id)?;
    Some(ComputedAnswer {
        query: query.trim().to_owned(),
        answer: text.clone(),
        command: computed.command_title.clone(),
    })
}

/// The icon of root search's `row`, when activating it (`entry`) reaches
/// an installed command: opening it, saying why it cannot, or sending it
/// text (whose row's id is the command's after `alias:` or `fallback:`).
fn icon(state: &State, row: &Row, entry: &Entry) -> Option<Icon> {
    let id = match entry {
        Entry::Open(_) | Entry::Unavailable(_) => row.id.as_str(),
        Entry::Send(_) => row
            .id
            .strip_prefix("alias:")
            .or_else(|| row.id.strip_prefix("fallback:"))?,
        _ => return None,
    };
    looks::icon_of(state, id)
}

/// What kind of thing activating `entry` from root search reaches.
pub(super) fn kind(entry: &Entry) -> Option<RowKind> {
    match entry {
        Entry::Open(_) | Entry::Unavailable(_) => Some(RowKind::Command),
        Entry::Send(Sending {
            via: Via::Fallback, ..
        }) => Some(RowKind::Fallback),
        Entry::Send(Sending {
            via: Via::Alias, ..
        }) => Some(RowKind::Command),
        Entry::OpenApplication { .. } => Some(RowKind::Application),
        Entry::File(_) => Some(RowKind::File),
        Entry::OpenUrl(_) => Some(RowKind::Link),
        // Pane's own rows are its commands.
        Entry::InstallFromFolder
        | Entry::AskNpm
        | Entry::AskGit
        | Entry::Acquire(_)
        | Entry::InstallUpdate
        | Entry::CheckUpdate
        | Entry::Manage
        | Entry::Settings => Some(RowKind::Command),
        _ => None,
    }
}

/// Root search's sections for `query`: every row under "Commands" for a
/// blank query — what root search lists then is its commands, in their
/// own order, not a suggestion of recent use — and for a query, the rows
/// it found under "Results" with their count, then the fallbacks the user
/// chose, under "Fallbacks" (after the window's own notice when nothing
/// else matched, as the reference's empty board composes them).
///
/// `rows` is how many rows are listed and `fallbacks` the index of the
/// first fallback (`rows` when none is listed).
pub fn root_sections(query: &str, rows: usize, fallbacks: usize) -> Vec<Section> {
    if rows == 0 {
        return Vec::new();
    }
    if query.trim().is_empty() {
        return vec![Section {
            label: "Commands".into(),
            note: None,
            first: 0,
        }];
    }
    let mut sections = Vec::new();
    if fallbacks > 0 {
        sections.push(Section {
            label: "Results".into(),
            note: Some(matches_note(fallbacks)),
            first: 0,
        });
    }
    if fallbacks < rows {
        sections.push(Section {
            label: "Fallbacks".into(),
            note: None,
            first: fallbacks,
        });
    }
    sections
}

/// Root search's sections for `query` (see [`root_sections`]), with each
/// run of computed answers among the results under a label of its own —
/// the title of the command that computed them, as the reference's
/// calculator board labels its card "Calculator" — and the results
/// before or after such a run under "Results" with their own count.
///
/// `answers` holds, for each listed row, the title of the command that
/// computed it when it is a computed answer; `fallbacks` is the index of
/// the first fallback (the number of rows when none is listed).
pub fn answer_sections(query: &str, answers: &[Option<&str>], fallbacks: usize) -> Vec<Section> {
    let rows = answers.len();
    let found = fallbacks.min(rows);
    if query.trim().is_empty() || answers[..found].iter().all(Option::is_none) {
        return root_sections(query, rows, fallbacks);
    }
    let mut sections = Vec::new();
    let mut first = 0;
    while first < found {
        let command = answers[first];
        let end = (first..found)
            .find(|&index| answers[index] != command)
            .unwrap_or(found);
        sections.push(match command {
            Some(command) => Section {
                label: command.to_owned(),
                note: None,
                first,
            },
            None => Section {
                label: "Results".into(),
                note: Some(matches_note(end - first)),
                first,
            },
        });
        first = end;
    }
    if found < rows {
        sections.push(Section {
            label: "Fallbacks".into(),
            note: None,
            first: found,
        });
    }
    sections
}

/// A "Results" label's note: how many rows it is over.
fn matches_note(found: usize) -> String {
    match found {
        1 => "1 match".into(),
        found => format!("{found} matches"),
    }
}
