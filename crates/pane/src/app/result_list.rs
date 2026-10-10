//! The launcher's result list, drawn virtually (#165): root search's
//! results, a command's list and every other screen's rows lay out and
//! paint only the children in view, plus a few rows' overscan (see
//! [`crate::ui::virtual_list`]), so a list of ten thousand rows costs a
//! frame what a list of ten does.
//!
//! Each frame's render reads the launcher's view and what it draws of the
//! whole list — the section labels, the notice, the empty line — once,
//! cheaply, into a [`ListFrame`]; the list then draws each child it lays
//! out from that frame ([`LauncherWindow::render_list_child`]), and only
//! then reads the row's own presentation from the launcher
//! ([`pane_core::Launcher::present_row`]), which is also when the row's
//! icons start loading: a row out of view requests none.
//!
//! The list's children are, in order: the head — the pinned home over a
//! blank query, root search's notice that nothing matched, or a screen's
//! line when none of its rows is selected — when it shows; then the rows,
//! each section's label ahead of its first row. A row is still the
//! accessible list's option it always was, now also saying where it is in
//! the whole list and how long that is, since the options out of view are
//! not drawn.

use std::mem::{Discriminant, discriminant};
use std::rc::Rc;

use gpui::{AnyElement, Context, Div, IntoElement, Window, div, prelude::*};
use pane_core::{LauncherView, Row, Screen};

use super::{LauncherWindow, ScrollAfter};
use crate::features::root_search;
use crate::ui::result_layouts::NoticeCopy;
use crate::ui::shell::{self, SectionLabel};
use crate::ui::theme::Theme;
use crate::ui::virtual_list::{self, ListChild, VirtualList};

/// The line a screen shows above its rows while none is selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum EmptyLine {
    /// A command's search that failed lists nothing; its error says why,
    /// in the footer, not "No results".
    Failed,
    /// A command's search found nothing for this query.
    NoResults(String),
    /// What the screen says when it lists nothing.
    Note(&'static str),
}

/// What a frame draws in the result list, read once by the frame's
/// render: the list's children are drawn from it as the list lays them
/// out.
pub(super) struct ListFrame {
    /// The view the frame draws, without its rows (they are `rows`).
    pub(super) view: LauncherView,
    /// The view's rows.
    pub(super) rows: Vec<Row>,
    /// The list's children, in order.
    pub(super) children: Vec<ListChild>,
    /// The section labels over the rows.
    pub(super) sections: Vec<SectionLabel>,
    /// Whether the list is root search's.
    pub(super) root: bool,
    /// The number hints' look: 0 hidden, 1 shown.
    pub(super) numbers: f32,
    /// The number each of the first rows is picked with, if it has one.
    pub(super) row_numbers: Vec<Option<usize>>,
    /// Whether the head holds the pinned home.
    pub(super) home: bool,
    /// Root search's notice that nothing matched, in the head.
    pub(super) notice: Option<NoticeCopy>,
    /// The screen's line while none of its rows is selected, in the head.
    pub(super) empty: Option<EmptyLine>,
}

/// The result list's state between frames: the list's own, and the frame
/// it lays out. Debug builds also keep which rows the last frame drew, for
/// the tests.
pub(super) struct ResultList {
    pub(super) list: VirtualList,
    frame: Option<Rc<ListFrame>>,
    #[cfg(any(test, debug_assertions))]
    pub(super) drawn_rows: std::collections::BTreeSet<usize>,
}

impl ResultList {
    pub(super) fn new(theme: &Theme) -> ResultList {
        let geometry = &theme.geometry;
        ResultList {
            list: VirtualList::new(geometry.row_min_height + geometry.row_list_gap),
            frame: None,
            #[cfg(any(test, debug_assertions))]
            drawn_rows: Default::default(),
        }
    }

    /// Lays the list out for `frame` from now on. When its rows, its
    /// children or its screen changed since the last frame, every child is
    /// measured again as it is drawn and the list starts from its top (its
    /// owner then reveals the selected row); otherwise nothing moves —
    /// not for an icon that arrived, a toast or the number hints. Whether
    /// they changed.
    pub(super) fn show(&mut self, frame: ListFrame) -> bool {
        let screen: Discriminant<Screen> = discriminant(&frame.view.screen);
        let changed = self.frame.as_ref().is_none_or(|last| {
            discriminant(&last.view.screen) != screen
                || last.children != frame.children
                || last.rows != frame.rows
        });
        if changed || self.list.count() != frame.children.len() {
            self.list.reset(frame.children.len());
        }
        self.frame = Some(Rc::new(frame));
        changed
    }

    /// Scrolls the least that shows row `row` of the frame laid out. The
    /// first row is shown with what is above it — the head (the pinned
    /// home) and its section's label — as far as there is room.
    pub(super) fn reveal_row(&self, row: usize) {
        if let Some(child) = self.child_of_row(row) {
            if row == 0 {
                self.list.reveal_from_top(child);
            } else {
                self.list.reveal(child);
            }
        }
    }

    /// Scrolls the least that shows the label of the section whose first
    /// row is `row`, and that row: the jump that lands on the row keeps
    /// the section's head in view with it (#258).
    pub(super) fn reveal_section(&self, row: usize) {
        if let Some(child) = self.child_of_row(row) {
            self.list.reveal_section(child, row == 0);
        }
    }

    /// The element, placed after the list, that scrolls to what `after`
    /// names again once this frame laid the list out (see
    /// [`VirtualList::reveal_after_layout`]): the row a selection moved
    /// to, or the label of the section a jump crossed (#258).
    pub(super) fn reveal_after(&self, after: ScrollAfter) -> Option<AnyElement> {
        match after {
            ScrollAfter::Row(row) => self.child_of_row(row).map(|child| {
                self.list
                    .reveal_after_layout(child, row == 0)
                    .into_any_element()
            }),
            ScrollAfter::Section(row) => self.child_of_row(row).map(|child| {
                self.list
                    .reveal_section_after_layout(child, row == 0)
                    .into_any_element()
            }),
        }
    }

    /// The list's child that shows row `row` of the frame laid out.
    fn child_of_row(&self, row: usize) -> Option<usize> {
        let frame = self.frame.as_ref()?;
        let head = frame.children.first() == Some(&ListChild::Head);
        Some(virtual_list::child_of_row(head, &frame.sections, row))
    }
}

impl LauncherWindow {
    /// The result list's child at `index` of the frame laid out, as the
    /// list draws it: the head, a section label or a row, with the gap
    /// after it and the list's side padding.
    pub(super) fn render_list_child(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(frame) = self.results.frame.clone() else {
            return div().into_any_element();
        };
        let theme = crate::settings::launcher_visuals(cx).theme;
        let child = match frame.children.get(index) {
            Some(ListChild::Head) => self.render_list_head(&frame, &theme, cx),
            Some(&ListChild::Label(at)) => {
                let section = &frame.sections[at];
                let debug = format!("section-{}", section.label);
                shell::section_label(section.label.clone(), section.note.clone(), &theme)
                    .debug_selector(move || debug)
                    .into_any_element()
            }
            Some(&ListChild::Row(row)) => self.render_list_row(&frame, row, cx),
            None => div().into_any_element(),
        };
        let last = index + 1 >= frame.children.len();
        virtual_list::item(
            child,
            last,
            theme.geometry.row_list_gap,
            theme.geometry.list_padding_x,
        )
        .into_any_element()
    }

    /// The head: the pinned home, then the screen's empty line or root
    /// search's notice.
    fn render_list_head(
        &mut self,
        frame: &ListFrame,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let home = if frame.home {
            self.render_home(&frame.view, frame.numbers, theme, cx)
        } else {
            None
        };
        div()
            .flex()
            .flex_col()
            .gap(theme.geometry.row_list_gap)
            .children(home.into_iter().flatten())
            .children(frame.empty.as_ref().map(|line| empty_line(line, theme)))
            .children(
                frame
                    .notice
                    .as_ref()
                    .map(|copy| root_search::layouts::notice(copy, theme)),
            )
            .into_any_element()
    }

    /// Row `index` of the frame: a computed answer as its card, any other
    /// as a result row, its presentation read now — which starts loading
    /// its icons — and its place in the whole list said to assistive
    /// technology.
    fn render_list_row(
        &mut self,
        frame: &ListFrame,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = frame.rows.get(index).cloned() else {
            return div().into_any_element();
        };
        #[cfg(any(test, debug_assertions))]
        self.results.drawn_rows.insert(index);
        let selected = frame.view.selected == Some(index);
        let shown = self.launcher.present_row(index);
        let number = frame
            .row_numbers
            .get(index)
            .copied()
            .flatten()
            .filter(|_| frame.numbers > 0.)
            .map(|number| (number, frame.numbers));
        let element = match shown.answer.clone() {
            Some(answer) => self.render_answer(index, row, &answer, selected, number, cx),
            None => self.render_row(index, row, selected, shown, frame.root, number, cx),
        };
        element
            .aria_position_in_set(index + 1)
            .aria_size_of_set(frame.rows.len())
            .into_any_element()
    }
}

/// The line `line` says, muted.
fn empty_line(line: &EmptyLine, theme: &Theme) -> gpui::Stateful<Div> {
    match line {
        EmptyLine::Failed => div().id("empty"),
        EmptyLine::NoResults(query) => div()
            .id("no-results")
            .debug_selector(|| "no-results".into())
            .child(format!("No results for “{query}”")),
        EmptyLine::Note(note) => div().id("empty").child(*note),
    }
    .text_color(theme.text_muted)
}
