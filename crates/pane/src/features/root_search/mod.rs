//! Root search's query field and its results, also used as the search field
//! of an opened command that searches as the user types.
//!
//! The query field is GPUI CE's single-line editable text element (typing,
//! editing keys, clipboard, undo and input-method composition). It has
//! keyboard focus whenever root search, or a command's search, is on screen;
//! every change searches at once (the launcher decides what: root search's
//! providers, or only the opened command). Up and Down move the selection
//! through the results instead of the caret, Enter opens the selected result
//! and Escape clears the query.
//!
//! The field's editing keys are the ones the form's text fields use, bound
//! once by [`crate::ui::input::bind_text_editing`] without Tab, Enter and
//! Escape: those bubble from the field to the launcher's Confirm and Back
//! actions. [`bind_keys`] takes that binding's result, so root search cannot
//! be registered without it.
//!
//! For assistive technology the field and the result list form one
//! `EditableComboBox` node, which tracks the field's focus and carries the
//! query as its value; the selected result is its active descendant, so it
//! is reported as focused, and with no result the combo box itself is.

use gpui::{
    AnyElement, App, Context, Entity, Focusable, KeyBinding, Role, Subscription, Window,
    WindowControlArea, div, prelude::*, px,
};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};

use crate::app::LauncherWindow;
use crate::ui::icon::{Glyph, glyph};
use crate::ui::input::TextEditingKeys;
use crate::{SelectNext, SelectPrevious};

const CONTEXT: &str = "RootSearch";
/// The query field's placeholder on root search.
pub(crate) const ROOT_PLACEHOLDER: &str = "Search commands";
/// The query field's placeholder in an opened command that searches.
pub(crate) const COMMAND_PLACEHOLDER: &str = "Search";

/// Registers Up and Down in the query field to move the selection. They are
/// registered after, and so take precedence over, the text element's own
/// Up and Down, which in a single-line field move the caret to its start or
/// end. The field's other editing keys, and Enter and Escape bubbling to
/// the launcher, come from the shared text editing keys.
pub(crate) fn bind_keys(cx: &mut App, _: &TextEditingKeys) {
    let context = format!("{CONTEXT} > {DEFAULT_INPUT_CONTEXT}");
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(&context)),
        KeyBinding::new("up", SelectPrevious, Some(&context)),
    ]);
}

/// Root search's query field, kept for the window's lifetime.
pub(crate) struct QueryField {
    input: Entity<EditableTextState>,
    /// Whether root search was on screen when focus last followed the
    /// launcher's screen.
    shown: bool,
    _changes: Subscription,
}

impl QueryField {
    /// A query field whose every change searches root, or the opened
    /// command that searches.
    pub(crate) fn new(cx: &mut Context<LauncherWindow>) -> QueryField {
        let input = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        input.focus_handle(cx).tab_stop(true);
        let changes = cx.subscribe(&input, |this, input, _: &TextChanged, cx| {
            // Results computed from the query (the calculator's answer)
            // arrive later, without holding up typing.
            let computed = this.launcher.set_query(input.read(cx).as_str());
            cx.notify();
            cx.spawn(async move |this, cx| {
                computed.await;
                this.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
        });
        QueryField {
            input,
            shown: true,
            _changes: changes,
        }
    }

    pub(crate) fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.input.focus_handle(cx), cx);
    }
}

impl LauncherWindow {
    /// Test support: the query field's editing state, which a platform
    /// input method talks to while composing text (see
    /// [`LauncherWindow::text_field`]).
    #[doc(hidden)]
    pub fn query_field(&self) -> Entity<EditableTextState> {
        self.query.input.clone()
    }

    /// Makes the query field follow the launcher: it shows the launcher's
    /// query (empty again after navigating back to root search, opening a
    /// command that searches, or Escape) and takes focus when a screen with
    /// a search field comes on screen; focus moves to the list when the
    /// field leaves the screen.
    pub(crate) fn sync_root_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.launcher.view().search_field().map(str::to_owned);
        let was_shown = self.query.shown;
        self.query.shown = query.is_some();
        match query {
            Some(query) => {
                let input = self.query.input.clone();
                if input.read(cx).as_str() != query {
                    input.update(cx, |input, cx| input.emplace(&query, cx));
                }
                if !was_shown {
                    self.query.focus(window, cx);
                }
            }
            // A form of Pane's own (the npm or Git package form), or a form or
            // custom view opened from a command's search, has taken focus
            // already.
            None if was_shown && self.form.is_none() && self.custom_view.is_none() => {
                window.focus(&self.focus_handle, cx)
            }
            None => {}
        }
    }

    /// Root search, or an opened command's search: the query field, showing
    /// `placeholder` while empty, above `list`, the results — the content
    /// that arrives with a view transition, wrapped by the caller (see
    /// [`crate::app::LauncherWindow::render`]); the field above it is the
    /// shell's search header and never moves.
    ///
    /// The field's chrome is the reference's search header: a 64px row with
    /// the magnifier, 20px padding, a 14px gap and a hairline below — no
    /// boxed input. The editable text element, its IME plumbing, focus
    /// tracking and accessibility node are exactly the ones the launcher
    /// always used; only the paint around them is new.
    pub(crate) fn render_search(
        &self,
        query: String,
        placeholder: &'static str,
        list: impl gpui::IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input = &self.query.input;
        let visuals = crate::settings::visuals(cx);
        let theme = &visuals.theme;
        let geometry = &theme.geometry;
        let typography = &theme.typography;
        div()
            .id("search")
            .debug_selector(|| "search".into())
            .key_context(CONTEXT)
            // The editable text element has no accessibility node of its
            // own; this node is the field's and tracks its focus handle.
            .track_focus(&input.focus_handle(cx))
            .role(Role::EditableComboBox)
            .aria_label("Search")
            .aria_value(query)
            .aria_placeholder(placeholder)
            .flex_1()
            // Lets the results shrink below their content and scroll.
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(geometry.search_gap)
                    .h(geometry.search_height)
                    .px(geometry.search_padding_x)
                    .border_b_1()
                    .border_color(theme.hairline_soft)
                    // The magnifier's wrapper is the search header's drag
                    // region: with the native title bar hidden, the window
                    // can be moved by grabbing the icon — the editable
                    // field itself never drags. The hit target is a
                    // 44×44 square, while −12px horizontal margins keep
                    // its layout box at the glyph's 20px: the header's
                    // alignment is unchanged, and the input keeps its
                    // 14px gap minus the 12px bleed — 2px of clear space
                    // before the editable field begins.
                    .child(
                        div()
                            .window_control_area(WindowControlArea::Drag)
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(44.))
                            .mx(px(-12.))
                            .child(glyph(Glyph::Search, px(20.), theme.text_muted)),
                    )
                    .child(
                        text_input("query")
                            .state(input.downgrade())
                            .placeholder(placeholder)
                            .placeholder_color(theme.text_placeholder)
                            .caret_color(theme.accent_text)
                            .selection_color(theme.row_selected)
                            .marked_color(theme.accent_text)
                            .text_size(typography.search_size)
                            .text_color(theme.text_query)
                            .font_family(typography.family.clone())
                            .w_full()
                            .min_w(px(0.))
                            .whitespace_nowrap()
                            .overflow_x_scroll(),
                    ),
            )
            .child(list)
            .into_any_element()
    }
}
