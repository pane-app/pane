//! Virtualized lists (#165): a list that lays out and paints only the
//! children in view, plus a few rows' height past either edge (the
//! overscan), however long it is — GPUI's `list`, whose children may
//! differ in height (a section label, an answer card), measured as they
//! are drawn.
//!
//! A [`VirtualList`] is the list's state, kept by its owner between
//! frames: how many children it holds, how far it is scrolled and the
//! heights it measured. The owner says when what the children show
//! changed ([`VirtualList::reset`]), which measures them again as they are
//! drawn; nothing else moves the list — an icon that arrives for a row in
//! view redraws the row at its height, and the list stays where it is.
//! The keys' selection is kept in view with [`VirtualList::reveal`]; the
//! wheel scrolls the list itself.
//!
//! The gap between children and the list's side padding are each child's
//! own ([`item`]): the list places its children edge to edge, the full
//! width of its bounds.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Div, ListAlignment, ListState, Pixels, ScrollHandle, canvas, div, px,
};

use super::shell::{self, SectionLabel};

/// How many rows' height past either edge of the view a list lays out
/// ahead, so a row scrolled into view is measured already.
pub(crate) const OVERSCAN_ROWS: f32 = 3.;

/// A virtualized list's state (see the module docs). Cloning shares it.
#[derive(Clone)]
pub(crate) struct VirtualList {
    state: ListState,
    /// The height a child not yet drawn is taken to have.
    row_height: Pixels,
    /// The list's width when its children last had their heights hinted:
    /// GPUI's `list` forgets every hint when it is laid out at another
    /// width (its first layout included), so the hints are given again
    /// once the width they were given at is no longer the list's.
    hinted_width: Rc<Cell<Pixels>>,
}

impl VirtualList {
    /// An empty list whose children are about `row_height` high (the gap
    /// after one included), until each is drawn and measured.
    pub(crate) fn new(row_height: Pixels) -> VirtualList {
        VirtualList {
            state: ListState::new(0, ListAlignment::Top, row_height * OVERSCAN_ROWS),
            row_height,
            hinted_width: Rc::new(Cell::new(px(0.))),
        }
    }

    /// The state GPUI's `list` element draws from.
    pub(crate) fn state(&self) -> &ListState {
        &self.state
    }

    /// How many children the list holds.
    pub(crate) fn count(&self) -> usize {
        self.state.item_count()
    }

    /// Has the list hold `count` children whose content changed: each is
    /// measured again as it is drawn, taken to be a row high until then,
    /// and the list scrolls back to its top (the owner reveals what it
    /// keeps in view after this).
    pub(crate) fn reset(&self, count: usize) {
        self.state.reset_with_uniform_height(count, self.row_height);
        self.hinted_width.set(self.viewport().size.width);
    }

    /// Has the list hold `added` more children after the ones it holds,
    /// whose content did not change: the list stays where it is scrolled,
    /// and the new ones are taken to be a row high until each is drawn.
    pub(crate) fn append(&self, added: usize) {
        let count = self.count();
        self.state.splice(count..count, added);
        // The state is shared: hinting a clone hints the list.
        let _ = self.state.clone().with_uniform_item_height(self.row_height);
    }

    /// Scrolls the least that shows the child at `index` whole.
    ///
    /// A child not yet drawn is taken to be a row high here: were it taken
    /// to have no height — as GPUI's `list` takes it once a layout at a new
    /// width forgot the hints — revealing a row far past the ones drawn
    /// would stop short of it, there being nothing in between.
    pub(crate) fn reveal(&self, index: usize) {
        self.keep_hints();
        self.state.scroll_to_reveal_item(index);
    }

    /// Reveals the list's first child, then the child at `index`: what is
    /// above `index` (a head, a section's label) shows as far as the list
    /// has room for it with `index` whole. A list first laid out short and
    /// then grown keeps no offset that hides them.
    pub(crate) fn reveal_from_top(&self, index: usize) {
        self.reveal(0);
        self.reveal(index);
    }

    /// Scrolls the least that shows the child at `index` and the one
    /// above it — a section's label over the section's first row, which a
    /// jump landing on the row keeps in view with it (#258). `from_top`,
    /// as [`VirtualList::reveal_from_top`].
    pub(crate) fn reveal_section(&self, index: usize, from_top: bool) {
        if from_top {
            self.reveal_from_top(index);
            return;
        }
        self.reveal(index);
        if index > 0 {
            self.reveal(index - 1);
        }
    }

    /// An empty element, placed after the list in the same frame, that
    /// reveals the child at `index` again once the list has been laid out
    /// — with the heights this frame measured rather than the row-high
    /// guesses a reveal before it used — and asks for another frame only
    /// when that moved the list. A list whose first reveal was right is
    /// left idle. With `from_top`, as [`VirtualList::reveal_from_top`].
    pub(crate) fn reveal_after_layout(&self, index: usize, from_top: bool) -> impl IntoElement {
        let list = self.clone();
        canvas(
            move |_, window, _| {
                let before = list.state.logical_scroll_top();
                if from_top {
                    list.reveal_from_top(index);
                } else {
                    list.reveal(index);
                }
                let after = list.state.logical_scroll_top();
                if (before.item_ix, before.offset_in_item) != (after.item_ix, after.offset_in_item)
                {
                    window.request_animation_frame();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }

    /// [`VirtualList::reveal_after_layout`], for a section jump: the
    /// label of the section the jump crossed scrolls into view again, as
    /// [`VirtualList::reveal_section`] scrolls (#258).
    pub(crate) fn reveal_section_after_layout(
        &self,
        index: usize,
        from_top: bool,
    ) -> impl IntoElement {
        let list = self.clone();
        canvas(
            move |_, window, _| {
                let before = list.state.logical_scroll_top();
                list.reveal_section(index, from_top);
                let after = list.state.logical_scroll_top();
                if (before.item_ix, before.offset_in_item) != (after.item_ix, after.offset_in_item)
                {
                    window.request_animation_frame();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }

    /// Gives the children not yet drawn their row's height again, when the
    /// list was laid out at a width other than the one they were hinted
    /// at; the heights it measured are kept.
    fn keep_hints(&self) {
        let width = self.viewport().size.width;
        if width != self.hinted_width.get() {
            // The state is shared: hinting a clone hints the list.
            let _ = self.state.clone().with_uniform_item_height(self.row_height);
            self.hinted_width.set(width);
        }
    }

    /// How far the list is scrolled from its top, in px.
    pub(crate) fn scrolled(&self) -> Pixels {
        (-self.state.scroll_px_offset_for_scrollbar().y).max(px(0.))
    }

    /// The list's bounds as the last frame laid it out.
    pub(crate) fn viewport(&self) -> Bounds<Pixels> {
        self.state.viewport_bounds()
    }

    /// How many rows a page is: the rows that fit in the list's view as
    /// last laid out, at least one.
    pub(crate) fn page(&self) -> usize {
        let height = f32::from(self.viewport().size.height);
        let row = f32::from(self.row_height).max(1.);
        ((height / row).floor() as usize).max(1)
    }
}

/// How far Page Down (`forward`) or Page Up moves a selection through
/// `list`: by the rows of its page, one row when no list is drawn.
pub(crate) fn page_move(list: Option<&VirtualList>, forward: bool) -> isize {
    let page = isize::try_from(list.map_or(1, VirtualList::page)).unwrap_or(isize::MAX);
    if forward { page } else { -page }
}

/// A child of a virtualized list as it is laid out: the list's side
/// padding (`padding_x`) on either side and, unless it is the `last`
/// child, the `gap` to the next one below it — the gap a flex column puts
/// between its children, here each child's own.
pub(crate) fn item(child: AnyElement, last: bool, gap: Pixels, padding_x: Pixels) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .px(padding_x)
        .when(!last, |item| item.pb(gap))
        .child(child)
}

/// How many rows a page's list holds before [`PageWindow`] draws only
/// those near the page's view: a short list is drawn whole, as it always
/// was.
pub(crate) const PAGE_WINDOW_ROWS: usize = 50;

/// The rows of a long list inside a page that scrolls as a whole — a
/// Settings page's (#165) — drawn only while near the page's view: within
/// a view's height above or below it. A row away from it is a stand-in as
/// high as the row last was, so the page keeps its length and the rows
/// around keep their places. Each row, and each stand-in, records where it
/// lays out in the page's content; a stand-in that turns out to be in
/// view (the rows above it changed height) asks for another frame, which
/// draws the row. Cloning shares it.
#[derive(Clone, Default)]
pub(crate) struct PageWindow {
    /// Where each row last laid out, by key: its top in the page's
    /// content (from the content's top, however scrolled) and its height.
    seen: Rc<RefCell<HashMap<String, (f32, f32)>>>,
}

impl PageWindow {
    /// Forgets where the rows were: the next frame draws them all, and
    /// records them again (the list changed as a whole, such as under a
    /// filter).
    pub(crate) fn forget(&self) {
        self.seen.borrow_mut().clear();
    }

    /// Whether the row `key` is near `page`'s view, as last laid out. A
    /// row never laid out is.
    pub(crate) fn near(&self, key: &str, page: &ScrollHandle) -> bool {
        match self.seen.borrow().get(key) {
            Some(&(top, height)) => near(top, height, page),
            None => true,
        }
    }

    /// One row of a page's list, keyed `key` within this window, built by
    /// `build` only when it is drawn: always while the list is short (fewer
    /// than [`PAGE_WINDOW_ROWS`] rows, `listed`) or the row is `kept` (it
    /// holds the focus, an edit or the selection); else only while near
    /// `page`'s view, a stand-in as high as it was otherwise.
    pub(crate) fn row(
        &self,
        listed: usize,
        key: String,
        kept: bool,
        page: &ScrollHandle,
        build: impl FnOnce() -> AnyElement,
    ) -> AnyElement {
        if listed < PAGE_WINDOW_ROWS {
            return build();
        }
        if !kept && !self.near(&key, page) {
            return self.stand_in(key, page).into_any_element();
        }
        self.track(key, page, build()).into_any_element()
    }

    /// `row`, keyed `key`, recording where it lays out in `page`'s content.
    pub(crate) fn track(&self, key: String, page: &ScrollHandle, row: AnyElement) -> Div {
        self.recorded(key, page, row, false)
    }

    /// The stand-in for the row `key` while it is away from `page`'s view:
    /// an empty block as high as the row last was.
    pub(crate) fn stand_in(&self, key: String, page: &ScrollHandle) -> Div {
        let height = self
            .seen
            .borrow()
            .get(&key)
            .map_or(0., |&(_, height)| height);
        self.recorded(key, page, div().h(px(height)).into_any_element(), true)
    }

    fn recorded(&self, key: String, page: &ScrollHandle, row: AnyElement, stand_in: bool) -> Div {
        let seen = self.seen.clone();
        let page = page.clone();
        div().relative().flex().flex_col().child(row).child(
            canvas(
                move |bounds, window, _| {
                    let view = page.bounds();
                    let top = f32::from(bounds.top() - view.top() - page.offset().y);
                    let height = f32::from(bounds.size.height);
                    seen.borrow_mut().insert(key, (top, height));
                    if stand_in && near(top, height, &page) {
                        window.request_animation_frame();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }
}

/// Whether a row `top` into `page`'s content and `height` high lies
/// within a view's height of the page's view, as last laid out (a page not
/// laid out yet has every row near).
fn near(top: f32, height: f32, page: &ScrollHandle) -> bool {
    let view = f32::from(page.bounds().size.height);
    if view <= 0. {
        return true;
    }
    let scrolled = -f32::from(page.offset().y);
    top + height >= scrolled - view && top <= scrolled + 2. * view
}

/// One child of a list of rows under section labels, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListChild {
    /// What shows above the rows, when anything does: the launcher's
    /// pinned home, a notice, the line a list shows when it lists nothing.
    Head,
    /// The label of the section at this index of the list's sections.
    Label(usize),
    /// The row at this index.
    Row(usize),
}

/// A list's children (see [`ListChild`]): the head when `head`, then
/// `rows` rows with each of `sections`' labels ahead of its first row, as
/// the sections are ordered. A label past the last row is not shown.
pub(crate) fn children(head: bool, rows: usize, sections: &[SectionLabel]) -> Vec<ListChild> {
    let mut children = Vec::with_capacity(usize::from(head) + rows + sections.len());
    if head {
        children.push(ListChild::Head);
    }
    let mut next = sections.iter().enumerate().peekable();
    for row in 0..rows {
        while let Some((index, section)) = next.next_if(|(_, section)| section.first <= row) {
            if section.first == row {
                children.push(ListChild::Label(index));
            }
        }
        children.push(ListChild::Row(row));
    }
    children
}

/// The child that shows row `row` of a list of [`children`]: after the
/// head, when it shows, and every label at or before the row.
pub(crate) fn child_of_row(head: bool, sections: &[SectionLabel], row: usize) -> usize {
    usize::from(head) + shell::child_of_row(sections, row)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(first: usize) -> SectionLabel {
        SectionLabel {
            first,
            label: format!("Section at {first}").into(),
            note: None,
        }
    }

    #[test]
    fn the_children_are_the_head_then_each_label_ahead_of_its_first_row() {
        let sections = [label(0), label(2), label(9)];
        let listed = children(true, 3, &sections);
        assert_eq!(
            listed,
            [
                ListChild::Head,
                ListChild::Label(0),
                ListChild::Row(0),
                ListChild::Row(1),
                ListChild::Label(1),
                ListChild::Row(2),
            ]
        );
        assert!(children(false, 0, &sections).is_empty());
        for row in 0..3 {
            assert_eq!(
                listed[child_of_row(true, &sections, row)],
                ListChild::Row(row)
            );
        }
    }

    #[test]
    fn ten_thousand_rows_are_ten_thousand_children_and_their_labels() {
        let sections = [label(0), label(1_000)];
        let listed = children(false, 10_000, &sections);
        assert_eq!(listed.len(), 10_002);
        assert_eq!(
            listed[child_of_row(false, &sections, 9_999)],
            ListChild::Row(9_999)
        );
    }
}
