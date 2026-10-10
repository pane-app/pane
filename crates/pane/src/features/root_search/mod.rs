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
//! query as its value. It stays the focused node while the selection moves,
//! so a screen reader echoes what is typed: no result reports itself as
//! focused, and the window's announcer says the selected one instead
//! ([`crate::features::announcer`], #132).

use gpui::{
    AnyElement, App, Context, Div, Entity, Focusable, KeyBinding, Role, Subscription, Window,
    WindowControlArea, div, prelude::*, px,
};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};
use pane_core::{Keyboard, KeyboardAction};

use crate::app::LauncherWindow;
use crate::ui::icon::{Glyph, glyph};
use crate::ui::input::TextEditingKeys;
use crate::ui::theme::Theme;
use crate::{SelectNext, SelectPrevious};

pub(crate) mod layouts;

pub(crate) const CONTEXT: &str = "RootSearch";
/// The query field's placeholder on root search: the reference's "Search
/// apps, commands, plugins…" in Pane's own terms — root search finds
/// installed applications and commands, and Pane has extensions, not
/// plugins, whose results arrive as commands — so the adaptation drops the
/// third noun rather than rename it to a search Pane does not offer.
pub(crate) const ROOT_PLACEHOLDER: &str = "Search apps and commands…";
/// The query field's placeholder in an opened command that searches.
pub(crate) const COMMAND_PLACEHOLDER: &str = "Search";

/// The context of the query field with focus, as a binding's context is
/// written: the search field inside the window.
pub(crate) fn field_context() -> String {
    format!("{CONTEXT} > {DEFAULT_INPUT_CONTEXT}")
}

/// Registers the selection keys in the query field, under the bindings
/// in force for previous and next result. They are registered after, and
/// so take precedence over, the text element's own Up and Down, which in
/// a single-line field move the caret to its start or end — the
/// arrangement the fixed Up and Down had, kept for whatever keys the
/// Keyboard page puts in their place. The field's other editing keys, and
/// Enter and Escape bubbling to the launcher, come from the shared text
/// editing keys.
pub(crate) fn bind_keys(cx: &mut App, _: &TextEditingKeys, keyboard: &Keyboard) {
    let context = field_context();
    cx.bind_keys([
        KeyBinding::new(
            &keyboard.binding(KeyboardAction::NextResult).id(),
            SelectNext,
            Some(&context),
        ),
        KeyBinding::new(
            &keyboard.binding(KeyboardAction::PreviousResult).id(),
            SelectPrevious,
            Some(&context),
        ),
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
    pub(crate) fn new(window: &mut Window, cx: &mut Context<LauncherWindow>) -> QueryField {
        let input = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        input.focus_handle(cx).tab_stop(true);
        let changes = cx.subscribe_in(&input, window, |this, input, _: &TextChanged, window, cx| {
            // Results computed from the query (the calculator's answer)
            // arrive later, without holding up typing. The announcer waits
            // for them before it says the selected row (#132).
            //
            // The sensitivity is pushed as the query changes, so the
            // keystroke that changed it matches by the choice the
            // Launcher page holds now.
            this.launcher
                .set_search_sensitivity(crate::settings::search_sensitivity_of(cx));
            let computed = this.launcher.set_query(input.read(cx).as_str());
            this.announcer.search_started();
            cx.notify();
            cx.spawn_in(window, async move |this, cx| {
                computed.await;
                this.update_in(cx, |this, window, cx| {
                    this.announcer.search_ended();
                    // The query's list is published once its search has
                    // answered: the keys the window held for it are
                    // applied (#203).
                    this.replay_held_keys_if_published(window, cx);
                    cx.notify();
                })
                .ok();
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

    /// Replaces the query with `query`, as Tab's completion of a typed
    /// folder and Shift+Tab's removal of a path component do (#204): the
    /// field searches as if `query` were typed, and the announcer waits
    /// for the results as it does for typing.
    pub(crate) fn replace(&self, query: &str, cx: &mut App) {
        self.input.update(cx, |input, cx| input.emplace(query, cx));
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
        let query = self.launcher.screen().search_field().map(str::to_owned);
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
        let visuals = crate::settings::launcher_visuals(cx);
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
            .child(search_header(input, placeholder, &visuals.theme))
            .child(list)
            .into_any_element()
    }
}

/// The search header's chrome: the reference's 64px row — the magnifier,
/// the 20px horizontal padding, the 14px gap and the hairline below —
/// around the editable text element `input`, which is the caller's own
/// (the launcher's query field, or another search field built the same
/// way).
///
/// The magnifier's wrapper is the search header's drag region: with the
/// native title bar hidden, the window can be moved by grabbing the icon —
/// the editable field itself never drags. The hit target is a 44×44
/// square, while −12px horizontal margins keep its layout box at the
/// glyph's 20px: the header's alignment is unchanged, and the input keeps
/// its 14px gap minus the 12px bleed — 2px of clear space before the
/// editable field begins.
///
/// The query is Geist 19/400, as the reference's `.q`. Its `letter-spacing:
/// -.005em` (−0.095px a character) is not applied: GPUI's editable text
/// element shapes its text with no letter spacing whatever its style says,
/// so a typed query runs about 0.1px a character wider than the
/// reference's — under a pixel for the authored queries (#93).
///
/// Over a background image (ADR 0028, `Theme::frost`) the same field is a
/// frosted pill inside the 64px row, with no hairline below it.
///
/// The launcher's search screens ([`LauncherWindow::render_search`])
/// compose this header.
pub(crate) fn search_header(
    input: &Entity<EditableTextState>,
    placeholder: &'static str,
    theme: &Theme,
) -> Div {
    let geometry = &theme.geometry;
    let typography = &theme.typography;
    let field = div()
        .flex()
        .items_center()
        .gap(geometry.search_gap)
        .child(
            div()
                .window_control_area(WindowControlArea::Drag)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .size(px(44.))
                .mx(px(-12.))
                .child(glyph(
                    Glyph::Search,
                    geometry.search_glyph_size,
                    theme.text_muted,
                )),
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
                .font_features(typography.features.clone())
                .pl(geometry.search_text_inset)
                .w_full()
                .min_w(px(0.))
                .whitespace_nowrap()
                .overflow_x_scroll(),
        );
    match theme.frost {
        None => field
            .flex_none()
            .h(geometry.search_height)
            .px(geometry.search_padding_x)
            .border_b_1()
            .border_color(theme.hairline_soft),
        // Over a background image (ADR 0028), the field is a frosted pill
        // inside the header's row, with no hairline under it.
        Some(frost) => {
            let (top, bottom, side) = frost.pill_margin;
            div()
                .flex_none()
                .flex()
                .h(geometry.search_height)
                .pt(top)
                .pb(bottom)
                .px(side)
                .child(
                    field
                        .flex_1()
                        .px(frost.pill_padding_x)
                        .rounded(frost.pill_radius)
                        .backdrop_blur(frost.blur)
                        .bg(frost.tint)
                        .shadow(frost.edges()),
                )
        }
    }
}
