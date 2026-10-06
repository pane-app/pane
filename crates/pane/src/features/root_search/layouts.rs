//! Root search's no-results notice and computed answers (#96): what the
//! launcher's presentation of root search holds, translated into the
//! shared result layouts (`crate::ui::result_layouts`), for the
//! launcher.
//!
//! Only what the launcher holds is drawn. The notice names the real query
//! and is shown when nothing but fallbacks is listed for it; the fallbacks
//! under it stay unselected until the user selects one (#100). A computed
//! answer's card shows the query it answers and the text Enter copies —
//! no units, conversions or history, which no command supplies — and the
//! row behind it keeps its id, its selection and its copy action.

use gpui::prelude::*;
use gpui::{AnyElement, Div, Role, Stateful};
use pane_core::{ComputedAnswer, Presentation, RowKind, Screen};

use crate::ui::result_layouts::{self, AnswerCard, AnswerSide, NoticeCopy};
use crate::ui::shell::{self, SectionLabel};
use crate::ui::theme::Theme;

/// The notice's description while fallbacks are listed under it.
const WITH_FALLBACKS: &str = "Pick a fallback below, or install an extension that knows about it.";

/// The notice's description while the user has no fallback for the text:
/// Manage extensions offers a command that takes text as a fallback.
const WITHOUT_FALLBACKS: &str =
    "Install an extension that knows about it, or offer a fallback in Manage extensions.";

/// What the notice says for `query`: that nothing matches it — root
/// search looks at commands, applications and a granted folder's files,
/// not the whole computer, so it does not claim more — and what the user
/// can do, by whether `fallbacks` are listed under it. Pane installs
/// extensions; it has no store to suggest one from.
pub(crate) fn notice_copy(query: &str, fallbacks: bool) -> NoticeCopy {
    let description = if fallbacks {
        WITH_FALLBACKS
    } else {
        WITHOUT_FALLBACKS
    };
    NoticeCopy {
        title: format!("Nothing matches “{}”", query.trim()).into(),
        description: description.into(),
    }
}

/// The notice root search shows on `screen` with `presentation`'s rows, if
/// any: on root search, for a query that is not blank, while nothing but
/// fallbacks is listed — whichever of them is selected.
pub(crate) fn nothing_found(screen: &Screen, presentation: &Presentation) -> Option<NoticeCopy> {
    let Screen::Root { query } = screen else {
        return None;
    };
    let only_fallbacks = presentation
        .rows
        .iter()
        .all(|row| row.kind == Some(RowKind::Fallback));
    if query.trim().is_empty() || !only_fallbacks {
        return None;
    }
    Some(notice_copy(query, !presentation.rows.is_empty()))
}

/// The notice, as the result list's first child: its identity
/// (`no-results`) and its accessible note — the title as its name, the
/// description as its description.
pub(crate) fn notice(copy: &NoticeCopy, theme: &Theme) -> Stateful<Div> {
    result_layouts::no_results_notice(copy, theme)
        .id("no-results")
        .debug_selector(|| "no-results".into())
        .role(Role::Note)
        .aria_label(copy.title.clone())
        .aria_description(copy.description.clone())
}

/// The result list's children: the notice, when it shows, then the rows
/// with each section's label ahead of its first row (see
/// [`shell::with_section_labels`]).
pub(crate) fn list_children(
    notice: Option<AnyElement>,
    rows: impl IntoIterator<Item = AnyElement>,
    sections: &[SectionLabel],
    theme: &Theme,
) -> Vec<AnyElement> {
    notice
        .into_iter()
        .chain(shell::with_section_labels(rows, sections, theme))
        .collect()
}

/// The list child that shows row `row` (for scrolling the list to it):
/// after the notice, when it shows, and every label at or before it.
pub(crate) fn child_of_row(notice: bool, sections: &[SectionLabel], row: usize) -> usize {
    usize::from(notice) + shell::child_of_row(sections, row)
}

/// The card a computed answer is drawn as: the query it answers, its
/// answer, and whether it is the selected result. It carries no captions
/// and no chips: nothing the launcher holds fills them.
pub(crate) fn answer_card(answer: &ComputedAnswer, selected: bool, theme: &Theme) -> Div {
    let side = |value: &str| AnswerSide {
        value: value.to_owned().into(),
        caption: None,
    };
    result_layouts::answer_card(
        &AnswerCard {
            source: side(&answer.query),
            answer: side(&answer.answer),
            also: Vec::new(),
            selected,
        },
        theme,
    )
}

/// The card's accessible name: "6*7 = 42", what was typed and its answer.
pub(crate) fn answer_label(answer: &ComputedAnswer) -> String {
    format!("{} = {}", answer.query, answer.answer)
}
