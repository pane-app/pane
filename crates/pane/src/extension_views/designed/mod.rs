//! A designed view's tree, drawn with GPUI's flex layout: the layout
//! primitives, the shared UI components, and the tokens and raw values
//! the tree styles with, resolved onto Pane's theme in light and dark
//! (`docs/designed-tree.md`, ADR 0036, #237). The tree the launcher holds
//! is typed ([`pane_core::DesignedTree`]); the window never meets the
//! JSON the extension answered with. Layout is computed here, whole: a
//! `column` and a `row` are flex containers with the space tokens
//! resolved onto Pane's spacing rhythm, a `stack` places its children
//! over each other, a `scroll` scrolls, and the shared components draw
//! from Pane's own control families (`ui::controls`, `ui::keycap`,
//! `ui::extension_icon`), so an extension's screen is Pane's design, not
//! a copy of it. Every node may carry a [`Style`] — sizing and a surface
//! with `hover` and `pressed` variants Pane applies itself, never asking
//! the extension.
//!
//! What the window keeps for the tree is kept by key (#238,
//! [`reconcile`]): the focus of each focusable control, the editing state
//! of each text field — the text, the caret, the selection, the input
//! method's composition — the select's own open state, and each scroll
//! region's position, so a re-render that still draws a node keeps what
//! the user was doing with it. The fields are partially controlled: they
//! edit at once, their input events are coalesced to the latest while
//! one is in flight (and throttled, when a field asks), their commits go
//! out on Enter and blur, and a value the extension sets — one that
//! differs from the node's value in the extension's previous render —
//! replaces the text.
//!
//! The interactive components are focusable, in tree order: Tab and
//! Shift+Tab move through them, and Enter and Space press the focused
//! one (bound in [`bind_keys`], under each control's key context, which
//! sits below the launcher's window context). Escape stays with Pane, as
//! it does for a custom view: it pops the navigation stack (#239),
//! leaving the screen when only the root view is on it. Events are
//! raised on the tree the user saw — each control carries the render it
//! was drawn from, and a node's handler still named by the view's tree
//! receives the event even after a later render, while one that went is
//! dropped (the launcher's stale-event rule).
//!
//! Pressing a control hands its callback id to the launcher
//! ([`Launcher::send_designed_seen`]), which sends the event to the view
//! and shows the tree it answers with; the extension's state lives in
//! its view, and the whole screen is redrawn from the answer.

mod canvas;
mod components;
mod fields;
mod list;
mod markdown;
mod reconcile;
mod tree;

use std::time::{Duration, Instant};

use gpui::AnimationExt as _;
use gpui::Focusable as _;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, KeyBinding, PathPromptOptions, Window, actions, div, px};

use gpui_elements::editable_text::EditableTextState;
use pane_core::{DesignedHandler, DesignedViewSnapshot};

use crate::app::LauncherWindow;

use pane_core::Node;
use reconcile::{FieldEvents, Held};
use tree::Draw;

pub(crate) use canvas::measures_of;
pub(crate) use reconcile::DesignedControls;

/// The key context of a designed view's button and link: Enter and Space
/// press them there, below the launcher's window context (whose Escape
/// and Tab bubble).
const BUTTON_CONTEXT: &str = "DesignedButton";
/// The key context of a designed view's toggle and checkbox: Enter and
/// Space change them.
const TOGGLE_CONTEXT: &str = "DesignedToggle";
/// The key context of a designed view's checkbox.
const CHECKBOX_CONTEXT: &str = "DesignedCheckbox";
/// The key context of a designed view's segmented control: its arrows
/// move the choice.
const SEGMENTED_CONTEXT: &str = "DesignedSegmented";
/// The key context of a designed view's slider: its arrows adjust it.
const SLIDER_CONTEXT: &str = "DesignedSlider";
/// The key context of a designed view's text fields: Enter commits.
const INPUT_CONTEXT: &str = "DesignedInput";
/// The key context of a designed view's text areas: Enter inserts a
/// newline, as a text area's does (the commit is a blur's; #241's forms
/// bind Ctrl+Enter to submit).
const AREA_CONTEXT: &str = "DesignedTextArea";
/// The key context of a designed view's select: the searchable select's
/// trigger takes its keys from `ui::select`'s bindings.
const SELECT_CONTEXT: &str = "DesignedSelect";
/// The key context of a designed view's canvas (#242): it binds nothing of
/// its own — its keys are the node's `onKey` events — but its semantic
/// handlers' bindings sit under [`CANVAS_SEMANTICS_CONTEXT`].
const CANVAS_CONTEXT: &str = "DesignedCanvas";
/// The key context of a canvas that names the semantic handlers
/// (`onIncrement`, `onDecrement`, `onActivate`): the up and down arrows
/// and Space run them, taking those keys from the canvas's own key
/// events.
const CANVAS_SEMANTICS_CONTEXT: &str = "DesignedCanvasSemantics";
/// The key context of a designed form's date and date-time fields: their
/// arrows step the date.
const DATE_CONTEXT: &str = "DesignedDate";
/// The key context of a designed form's tag picker: Enter commits the
/// highlighted option.
const TAGS_CONTEXT: &str = "DesignedTags";
/// The key context of a designed form's picker of many paths: Enter opens
/// the system's dialog.
const PATHS_CONTEXT: &str = "DesignedPaths";

actions!(
    designed,
    [
        Press,
        Toggle,
        Move,
        Adjust,
        Commit,
        SubmitForm,
        StepDateUp,
        StepDateDown,
        CommitTag,
        ChoosePath,
        Increment,
        Decrement,
        Activate,
        CanvasKeyUp,
        CanvasKeyDown,
        CanvasKeyLeft,
        CanvasKeyRight,
        CanvasKeyHome,
        CanvasKeyEnd
    ]
);

/// Registers the designed view's key bindings: Enter and Space press a
/// button or a link, change a toggle or a checkbox; a segmented
/// control's and a slider's arrows move them; Enter commits a text
/// field; Enter in a text area inserts a newline. A form's keys submit:
/// Ctrl+Enter in any field (#241), and a date field's arrows step the
/// date, a tag picker's Enter commits its highlighted option, a picker
/// of many paths' Enter opens the system's dialog.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Press, Some(BUTTON_CONTEXT)),
        KeyBinding::new("space", Press, Some(BUTTON_CONTEXT)),
        KeyBinding::new("enter", Toggle, Some(TOGGLE_CONTEXT)),
        KeyBinding::new("space", Toggle, Some(TOGGLE_CONTEXT)),
        KeyBinding::new("enter", Toggle, Some(CHECKBOX_CONTEXT)),
        KeyBinding::new("space", Toggle, Some(CHECKBOX_CONTEXT)),
        KeyBinding::new("left", Move, Some(SEGMENTED_CONTEXT)),
        KeyBinding::new("right", Move, Some(SEGMENTED_CONTEXT)),
        KeyBinding::new("left", Adjust, Some(SLIDER_CONTEXT)),
        KeyBinding::new("right", Adjust, Some(SLIDER_CONTEXT)),
        KeyBinding::new("enter", Commit, Some(INPUT_CONTEXT)),
        KeyBinding::new("enter", Commit, Some(SELECT_CONTEXT)),
        KeyBinding::new("space", Commit, Some(SELECT_CONTEXT)),
        KeyBinding::new(
            "enter",
            gpui_elements::editable_text::actions::Enter,
            Some(AREA_CONTEXT),
        ),
        // A canvas that names the semantic handlers takes the up and down
        // arrows and Space itself (#242): a slider-like canvas needs no key
        // parsing. Other keys are its own `onKey` events, which the
        // window's keystroke observer raises.
        KeyBinding::new("up", Increment, Some(CANVAS_SEMANTICS_CONTEXT)),
        KeyBinding::new("down", Decrement, Some(CANVAS_SEMANTICS_CONTEXT)),
        KeyBinding::new("space", Activate, Some(CANVAS_SEMANTICS_CONTEXT)),
        // A canvas whose tree asks for keys (its `onKey`) takes the
        // navigation keys as its own while it holds the focus: the binding
        // is the deepest context's, so the focused canvas's own keys win
        // over the launcher's root-list navigation — the keystroke
        // observer never sees a key that acted, and without a binding of
        // its own the arrows moved the root's selection instead.
        KeyBinding::new("up", CanvasKeyUp, Some(CANVAS_CONTEXT)),
        KeyBinding::new("down", CanvasKeyDown, Some(CANVAS_CONTEXT)),
        KeyBinding::new("left", CanvasKeyLeft, Some(CANVAS_CONTEXT)),
        KeyBinding::new("right", CanvasKeyRight, Some(CANVAS_CONTEXT)),
        KeyBinding::new("home", CanvasKeyHome, Some(CANVAS_CONTEXT)),
        KeyBinding::new("end", CanvasKeyEnd, Some(CANVAS_CONTEXT)),
        KeyBinding::new("ctrl-enter", SubmitForm, Some(INPUT_CONTEXT)),
        KeyBinding::new("ctrl-enter", SubmitForm, Some(AREA_CONTEXT)),
        KeyBinding::new("up", StepDateUp, Some(DATE_CONTEXT)),
        KeyBinding::new("down", StepDateDown, Some(DATE_CONTEXT)),
        KeyBinding::new("enter", CommitTag, Some(TAGS_CONTEXT)),
        KeyBinding::new("enter", ChoosePath, Some(PATHS_CONTEXT)),
    ]);
}

impl LauncherWindow {
    /// Opens `url` with the system's handler, as a Markdown link of the
    /// open designed view is (#240): the status line says what it did.
    fn designed_open_link(&mut self, url: &str, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let pending = self.launcher.open_link(url.to_owned());
        self.show_until_done(pending, window, cx);
    }

    /// The designed view's search header (#240): the navigation title
    /// above the search field (which the List's field owns — the footer
    /// title was the interim, #239), the list's search-bar dropdown
    /// beside the field, and the loading bar under it once the list's
    /// loading has run past its threshold (300 ms; the bar's look is the
    /// launcher polish's, #248 — this is the flag it rides), above
    /// `body` (the rows list, the Grid's cells, the empty view).
    pub(crate) fn render_designed_search(
        &mut self,
        view: &DesignedViewSnapshot,
        list: &pane_core::DesignedList,
        title: String,
        body: impl gpui::IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input = self.query.entity().clone();
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = visuals.theme;
        let placeholder: gpui::SharedString = list
            .placeholder
            .clone()
            .unwrap_or_else(|| "Search".into())
            .into();
        let geometry = theme.geometry.clone();
        let dropdown = self.render_designed_dropdown(view, cx);
        // The navigation title, above the field: a screen's title row.
        let heading = (!title.is_empty()).then(|| {
            div()
                .id("designed-title")
                .debug_selector(|| "designed-title".into())
                .flex_none()
                .h(px(28.))
                .flex()
                .items_end()
                .px(geometry.search_padding_x)
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(theme.typography.section_size)
                        .font_weight(theme.typography.medium)
                        .text_color(theme.text_muted)
                        .child(title.clone()),
                )
        });
        let field = div()
            .flex()
            .flex_1()
            .min_w(px(0.))
            .items_center()
            .gap(geometry.search_gap)
            .child(
                div()
                    .window_control_area(gpui::WindowControlArea::Drag)
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(44.))
                    .mx(px(-12.))
                    .child(crate::ui::icon::glyph(
                        crate::ui::icon::Glyph::Search,
                        geometry.search_glyph_size,
                        theme.text_muted,
                    )),
            )
            .child(
                gpui_elements::editable_text::text_input("query")
                    .state(input.downgrade())
                    .placeholder(placeholder.clone())
                    .placeholder_color(theme.text_placeholder)
                    .caret_color(theme.accent_text)
                    .selection_color(theme.selection_wash)
                    .marked_color(theme.accent_text)
                    .text_size(theme.typography.search_size)
                    .text_color(theme.text_query)
                    .font_family(theme.typography.family.clone())
                    .font_features(theme.typography.features.clone())
                    .w_full()
                    .min_w(px(0.))
                    .whitespace_nowrap()
                    .overflow_x_scroll(),
            )
            .when_some(dropdown, |field, dropdown| field.child(dropdown));
        let header = div()
            .flex_none()
            .flex()
            .h(geometry.search_height)
            .px(geometry.search_padding_x)
            .border_b_1()
            .border_color(theme.hairline_soft)
            .child(field);
        // The loading bar: drawn once the list's loading has run past its
        // threshold (300 ms), so a list that answers quickly shows none.
        let loading = list
            .loading
            .filter(|since| self.launcher.now_ms().saturating_sub(*since) >= pane_core::LOADING_MS);
        let bar = loading.map(|_| {
            let width = geometry.search_text_inset * 40.;
            div()
                .id("designed-loading")
                .flex_none()
                .overflow_hidden()
                .h(px(2.))
                .w_full()
                .bg(theme.hairline_soft)
                .child(
                    div()
                        .id("designed-loading-highlight")
                        .h(px(2.))
                        .w(width)
                        .bg(theme.accent)
                        .with_animation(
                            String::from("designed-loading-sweep"),
                            gpui::Animation::new(std::time::Duration::from_millis(1200))
                                .repeat()
                                .with_max_fps(60.),
                            move |highlight, phase| {
                                let span =
                                    geometry.search_text_inset.as_f32() * 40. + width.as_f32();
                                let at = phase * span - width.as_f32();
                                highlight.ml(px(at))
                            },
                        ),
                )
        });
        div()
            .id("designed-search")
            .debug_selector(|| "search".into())
            .key_context(crate::features::root_search::CONTEXT)
            .track_focus(&self.query.entity().focus_handle(cx))
            .role(gpui::Role::EditableComboBox)
            .aria_label("Search")
            .aria_value(list.search.clone())
            .aria_placeholder(placeholder)
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .when_some(heading, |search, heading| search.child(heading))
            .child(header)
            .when_some(bar, |search, bar| search.child(bar))
            .child(body)
            .into_any_element()
    }

    /// Creates or drops the designed view's controls to match the
    /// launcher's screen, and reconciles the keyed state with the tree it
    /// now shows: a newly opened view takes the keyboard on its first
    /// focusable control (or the one that asks for it), and focus returns
    /// to the list when the view closes. The tree's loaded icons are
    /// drawn first, as a list's rows are (#142).
    pub(crate) fn sync_designed_view(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.launcher.refresh_designed_view();
        // The screen alone: the tree changes with each answer, and the
        // keys of two trees of one view decide which state is kept.
        let shown = match self.launcher.screen() {
            pane_core::Screen::DesignedView(view) => Some((view.id, view.render, view.tree)),
            // A form Pane itself asks (#241) is a designed tree too: its
            // fields' controls are kept for it, keyed by its own number
            // (a form's id, so one form replacing another builds afresh
            // and a form's own re-render reconciles), and the same draw
            // covers both.
            pane_core::Screen::PaneForm(form) => {
                Some((pane_core::ViewId::of_number(form.id), form.id, form.tree))
            }
            _ => None,
        };
        let open = shown
            .as_ref()
            .is_some_and(|(id, _, _)| Some(*id) == self.designed.as_ref().map(|c| c.view));
        if open {
            // The same view: the keyed state is reconciled with the tree
            // it now shows, and a field's value placed under the
            // partially controlled rule.
            if let Some((_, render, tree)) = shown {
                let controls = self.designed.as_mut().expect("a view is open");
                controls.reconcile(&tree, render, window, cx);
            }
            return;
        }
        // A view whose controls are gone closed: the keyboard goes with
        // them, and the list takes it back on the screens whose list
        // holds it, as `sync_root_search` then gives the field its own
        // on the search screens. Only the closing itself: the sync runs
        // over every change the launcher reports, and a screen that
        // holds no view of its own leaves the keyboard where the user
        // put it — the search field, a toast's open details — while the
        // root handle resolves to no node on the search screens, so a
        // sync-time focus there would leave every key falling through
        // to nothing.
        let closed = self.designed.is_some() && shown.is_none();
        // A view whose List searches as the user types opens on its
        // search field: the field the header shows is the window's own,
        // and typing into it is the view's first act — a tree that asks
        // for the keyboard keeps it, and one with no search of its own
        // falls to its first focusable control as before.
        let searches = matches!(
            &self.launcher.screen(),
            pane_core::Screen::DesignedView(view)
                if view.list.as_ref().is_some_and(|list| list.searching)
        );
        let opened = shown
            .map(|(view, render, tree)| DesignedControls::new(view, &tree, render, window, cx));
        let asked = opened.as_ref().is_some_and(|(_, asked)| *asked);
        self.designed = opened.map(|(controls, _)| controls);
        if searches && !asked {
            self.query.focus(window, cx);
        }
        if closed {
            // The view's focused control is gone: the list's handle is
            // the keyboard's back on a screen whose list tracks it, and
            // a search screen's field re-focuses over this in
            // `sync_root_search`.
            window.focus(&self.focus_handle, cx);
        }
    }

    /// Test support: how many focusable controls the open designed view's
    /// keyed state holds.
    #[doc(hidden)]
    pub fn designed_button_count(&self) -> usize {
        self.designed
            .as_ref()
            .map(|controls| controls.state.len())
            .unwrap_or(0)
    }

    /// Test support: the editing state of the open designed view's field
    /// keyed `key`, which a platform input method talks to while
    /// composing text. GPUI CE's test platform cannot reach the window's
    /// input handler, so the window tests compose through this instead.
    #[doc(hidden)]
    pub fn designed_field(&self, key: &str) -> Option<gpui::Entity<EditableTextState>> {
        self.designed_field_at(key).map(|(entity, _, _)| entity)
    }

    /// Test support: the value the open designed view's field keyed `key`
    /// holds — its live text, what the user typed.
    #[doc(hidden)]
    pub fn designed_field_text(&self, key: &str, cx: &gpui::App) -> Option<String> {
        self.designed_field_at(key)
            .map(|(entity, _, _)| entity.read(cx).as_str().to_owned())
    }

    /// Test support: the value the open designed view's select keyed
    /// `key` committed — the value a form's submission collects for it,
    /// as a field's live text is what it collects for a text field.
    #[doc(hidden)]
    pub fn designed_select_text(&self, key: &str) -> Option<String> {
        let controls = self.designed.as_ref()?;
        let (_, entry) = controls
            .state
            .iter()
            .find(|(path, _)| path.ends_with(&format!("/{key}")))?;
        let Held::Select { .. } = &entry.held else {
            return None;
        };
        // What the control last reported, else the value its tree names.
        controls
            .sent
            .get(key)
            .cloned()
            .or_else(|| select_value_of(&self.launcher, key))
    }

    /// The editing state, the focus and the bookkeeping of the open
    /// designed view's field keyed `key`.
    fn designed_field_at(
        &self,
        key: &str,
    ) -> Option<(
        gpui::Entity<EditableTextState>,
        gpui::FocusHandle,
        &FieldEvents,
    )> {
        let controls = self.designed.as_ref()?;
        let (_, entry) = controls
            .state
            .iter()
            .find(|(path, _)| path.ends_with(&format!("/{key}")))?;
        match &entry.held {
            Held::Field {
                editing,
                focus,
                events,
            } => Some((editing.clone(), focus.clone(), events)),
            _ => None,
        }
    }

    /// Test support: focuses the open designed view's field keyed `key`,
    /// as the keyboard would.
    #[doc(hidden)]
    pub fn focus_designed_field(
        &mut self,
        key: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self.designed_field_at(key).map(|(_, focus, _)| focus);
        if let Some(focus) = focus {
            window.focus(&focus, cx);
            cx.notify();
        }
    }

    /// Test support: how far the open designed view's scroll region keyed
    /// `key` is scrolled, from its top-left.
    #[doc(hidden)]
    pub fn designed_scroll(&self, key: &str) -> Option<gpui::Point<gpui::Pixels>> {
        let controls = self.designed.as_ref()?;
        let (_, entry) = controls
            .state
            .iter()
            .find(|(path, _)| path.ends_with(&format!("/{key}")))?;
        match &entry.held {
            Held::Scroll(handle) => Some(handle.offset()),
            _ => None,
        }
    }

    /// The designed view's fields' editing states, which a platform input
    /// method talks to while composing text — for the launcher's back
    /// key, which cancels an active composition before it acts.
    pub(crate) fn designed_text_fields(&self) -> Vec<gpui::Entity<EditableTextState>> {
        let Some(controls) = &self.designed else {
            return Vec::new();
        };
        controls
            .state
            .values()
            .filter_map(|entry| match &entry.held {
                Held::Field { editing, .. } => Some(editing.clone()),
                _ => None,
            })
            .collect()
    }

    /// Whether the open designed view holds the keyboard in a control
    /// that takes Backspace for itself — a field or a select, whose
    /// editing keys never reach the window's key handling, and whose
    /// other states (a select's trigger, its popup closed) are guarded
    /// here: Backspace pops the view only from what does not edit with
    /// it.
    pub(crate) fn designed_takes_backspace(&self, window: &gpui::Window) -> bool {
        let Some(controls) = &self.designed else {
            return false;
        };
        controls.state.values().any(|entry| match &entry.held {
            Held::Field { focus, .. } | Held::Select { focus, .. } => focus.is_focused(window),
            _ => false,
        })
    }

    /// The designed view screen for the launcher's snapshot of the view.
    pub(crate) fn render_designed_view(
        &self,
        view: DesignedViewSnapshot,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.render_designed_tree(&view.tree, view.render, cx)
    }

    /// A form Pane itself asks, as its tree says (#241): the same drawing
    /// as an extension's designed view, its submission routed to the
    /// launcher rather than the view.
    pub(crate) fn render_pane_form(
        &self,
        form: pane_core::PaneForm,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        // The form's own number is its render, as its keyed state is
        // keyed by it: the tree the form re-renders (a field's error set)
        // is the one its fields' values were collected from.
        let render = form.id;
        self.render_designed_tree(&form.tree, render, cx)
    }

    /// One designed tree, drawn with the keyed state the window keeps for
    /// it.
    fn render_designed_tree(
        &self,
        tree: &pane_core::DesignedTree,
        render: u64,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(controls) = &self.designed else {
            return div().into_any_element();
        };
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = visuals.theme;
        let draw = Draw {
            theme: &theme,
            state: &controls.state,
            render,
            surface: theme.panel_solid,
        };
        let drawn = {
            let mut path = String::new();
            tree::push(&mut path, tree.root.key.as_deref(), 0);
            tree::draw_node(&tree.root, &mut path, draw, cx)
        };
        // Fills the body as the list and the form do, so the status line
        // stays at the bottom; the tree's own `scroll` regions scroll
        // inside it, the screen scrolling what grows past it otherwise.
        let geometry = &theme.geometry;
        div()
            .id("designed-view")
            .key_context("DesignedView")
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .items_start()
            .overflow_y_scroll()
            .px(geometry.search_padding_x)
            .pt(crate::ui::tokens::space(pane_core::Space::S))
            .pb(geometry.list_padding_bottom)
            .child(drawn)
            .into_any_element()
    }

    /// Submits the form the screen holds (#241): the values of its
    /// fields, collected from the state the window keeps for them, run
    /// the form's action — an extension form's `onSubmit`, with the
    /// values as its event's payload, or a form Pane itself asks, which
    /// the launcher answers. Enter in a single-line field of the form
    /// runs this, as Ctrl+Enter does in a text area and the form's submit
    /// button and the footer's primary action do.
    pub(crate) fn submit_designed_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((form, values)) = self.collect_form(cx) else {
            return;
        };
        if matches!(self.launcher.screen(), pane_core::Screen::PaneForm(_)) {
            let values: Vec<(String, String)> = values
                .into_iter()
                .map(|(key, value)| (key, value.as_text()))
                .collect();
            let pending = self.launcher.submit_pane_form(values);
            self.show_until_done(pending, window, cx);
        } else {
            let pending = self.launcher.submit_designed_form(Some(&form), values);
            self.show_until_done(pending, window, cx);
        }
    }

    /// The form the screen holds, and the values its fields hold as a
    /// submission collects them: keyed by the fields' keys — the text a
    /// field edits, the state a checkbox or toggle last told the
    /// extension, the tags or paths a picker chose. `None` when no form
    /// is on screen.
    fn collect_form(
        &self,
        cx: &gpui::App,
    ) -> Option<(String, Vec<(String, pane_core::FormValue)>)> {
        let tree = match self.launcher.screen() {
            pane_core::Screen::DesignedView(view) => Some(view.tree),
            pane_core::Screen::PaneForm(form) => Some(form.tree),
            _ => None,
        }?;
        let controls = self.designed.as_ref()?;
        let mut collected = Collected {
            form: None,
            values: Vec::new(),
        };
        let mut path = String::new();
        tree::push(&mut path, tree.root.key.as_deref(), 0);
        collect_node(
            &tree.root,
            None,
            &mut path,
            &controls.state,
            &controls.sent,
            &mut collected,
            cx,
        );
        let form = collected.form?;
        Some((form, collected.values))
    }

    /// Whether the field at `path` sits inside a form on this screen
    /// (#241): Enter in it submits the form.
    fn field_in_form(&self, path: &str) -> bool {
        let tree = match self.launcher.screen() {
            pane_core::Screen::DesignedView(view) => Some(view.tree),
            pane_core::Screen::PaneForm(form) => Some(form.tree),
            _ => None,
        };
        tree.is_some_and(|tree| tree::inside_form(&tree, path))
    }

    /// The date field at `path`, stepped by `days` days (a date and time
    /// by as many minutes): its text rewritten, its caret at the end.
    /// Nothing steps a text that does not parse as the field's format.
    fn designed_date_stepped(
        &mut self,
        path: &str,
        minutes: i64,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let date_time = self.designed.as_ref().and_then(|controls| {
            controls
                .state
                .get(path)
                .and_then(|entry| match &entry.held {
                    Held::Field { editing, .. } => Some(editing.clone()),
                    _ => None,
                })
        });
        let Some(editing) = date_time else {
            return;
        };
        let text = editing.read(cx).as_str().to_owned();
        let Some(stepped) = fields::step_date(&text, minutes) else {
            return;
        };
        let len = stepped.len();
        editing.update(cx, |editing, cx| {
            editing.emplace(&stepped, cx);
            editing.move_to(len, cx);
        });
        let _ = window;
        cx.notify();
    }

    /// Adds the tag `value` to the tag picker at `path`, telling the
    /// extension what it now holds.
    fn designed_tag_added(
        &mut self,
        path: &str,
        value: &str,
        callback: Option<u32>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let tags = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(path)?;
            let Held::Tags { chosen, .. } = &mut entry.held else {
                return None;
            };
            if chosen.iter().any(|chosen| chosen == value) {
                return None;
            }
            chosen.push(value.to_owned());
            Some(chosen.clone())
        });
        let Some(tags) = tags else {
            return;
        };
        self.designed_tags_changed(path, tags, callback, window, cx);
    }

    /// Removes the tag `value` from the tag picker at `path`, telling the
    /// extension what it now holds.
    fn designed_tag_removed(
        &mut self,
        path: &str,
        value: &str,
        callback: Option<u32>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let tags = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(path)?;
            let Held::Tags { chosen, .. } = &mut entry.held else {
                return None;
            };
            chosen.retain(|chosen| chosen != value);
            Some(chosen.clone())
        });
        let Some(tags) = tags else {
            return;
        };
        self.designed_tags_changed(path, tags, callback, window, cx);
    }

    /// The tag picker at `path`'s query's Enter: the highlighted option is
    /// added, the query cleared.
    fn designed_tag_committed(
        &mut self,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some((value, callback)) = self.designed.as_ref().and_then(|controls| {
            let entry = controls.state.get(path)?;
            let Held::Tags {
                query, highlighted, ..
            } = &entry.held
            else {
                return None;
            };
            let text = query.read(cx).as_str().to_owned();
            let tree = match self.launcher.screen() {
                pane_core::Screen::DesignedView(view) => Some(view.tree),
                pane_core::Screen::PaneForm(form) => Some(form.tree),
                _ => None,
            }?;
            let picker = tree::node_at(&tree, path)?;
            let pane_core::NodeKind::TagPicker(picker) = &picker.kind else {
                return None;
            };
            let options = fields::matching(&picker.options, &text);
            let at = highlighted.filter(|at| *at < options.len()).unwrap_or(0);
            let chosen = options.get(at).map(|option| option.value.clone())?;
            Some((chosen, picker.on_change))
        }) else {
            return;
        };
        let query = self.designed.as_ref().and_then(|controls| {
            controls
                .state
                .get(path)
                .and_then(|entry| match &entry.held {
                    Held::Tags { query, .. } => Some(query.clone()),
                    _ => None,
                })
        });
        if let Some(query) = query {
            query.update(cx, |query, cx| query.emplace("", cx));
        }
        self.designed_tag_added(path, &value, callback, window, cx);
    }

    /// Tells the extension the tag picker at `path` now holds `tags`.
    fn designed_tags_changed(
        &mut self,
        path: &str,
        tags: Vec<String>,
        callback: Option<u32>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
        let Some(callback) = callback else {
            return;
        };
        // The event's key and render: what the entry the picker's state
        // lives in holds, as the change is raised from its own row.
        let Some((key, seen)) = self
            .designed
            .as_ref()
            .and_then(|controls| controls.state.get(path))
            .map(|entry| (entry.key.clone(), entry.render))
        else {
            return;
        };
        let payload = list_payload(&tags);
        self.designed_event(
            DesignedHandler::Change,
            callback,
            key,
            seen,
            payload,
            window,
            cx,
        );
    }

    /// Removes the last path of the picker of many at `path`.
    fn designed_path_removed(
        &mut self,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let _ = window;
        let removed = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(path)?;
            let Held::Paths { paths, .. } = &mut entry.held else {
                return None;
            };
            (!paths.is_empty()).then(|| paths.pop().expect("just checked"))
        });
        if removed.is_some() {
            cx.notify();
        }
    }

    /// The system's dialog for the picker at `path`, choosing what `pick`
    /// asks: the path (or paths, for one that allows many) fills the
    /// field.
    fn designed_paths_chosen(
        &mut self,
        path: &str,
        key: &str,
        pick: PathPick,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: pick.files,
            directories: pick.directories,
            multiple: pick.multiple,
            prompt: Some("Choose".into()),
        });
        let (path, key) = (path.to_owned(), key.to_owned());
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.designed_paths_arrived(&path, &key, paths, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// The paths the system's dialog chose, filling the picker at `path`:
    /// one path fills the field's text, as typing it is; several join the
    /// picker's chips, telling the extension what it now holds.
    fn designed_paths_arrived(
        &mut self,
        path: &str,
        _key: &str,
        paths: Vec<std::path::PathBuf>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let single = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(path)?;
            match &mut entry.held {
                Held::Field { editing, .. } => Some(editing.clone()),
                _ => None,
            }
        });
        if let (Some(editing), Some(picked)) = (single, paths.into_iter().next()) {
            let text = picked.to_string_lossy().into_owned();
            let len = text.len();
            editing.update(cx, |editing, cx| {
                editing.emplace(&text, cx);
                editing.move_to(len, cx);
            });
            cx.notify();
            return;
        }
        let _ = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(path)?;
            let Held::Paths { paths, .. } = &mut entry.held else {
                return None;
            };
            let before = paths.len();
            let arrived = paths.clone();
            for text in arrived {
                if !paths.contains(&text) {
                    paths.push(text);
                }
            }
            (paths.len() != before).then(|| paths.clone())
        });
        // The picker's chosen paths are its submission's value; the tree
        // the extension answers names what it draws of them.
        let _ = window;
        cx.notify();
    }

    /// Sends one event of the open designed view — the callback id its
    /// tree named, of `handler`'s kind, raised on the node with `key` (or
    /// none, when the tree gave it none), on the tree of the render
    /// `seen` (the tree the user saw the node in) — and redraws when its
    /// answer arrives.
    #[allow(clippy::too_many_arguments)]
    fn designed_event(
        &mut self,
        handler: DesignedHandler,
        callback: u32,
        key: String,
        seen: u64,
        payload: String,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        // A form Pane itself asks is answered by the launcher: its
        // controls' changes move into the tree the screen holds, as an
        // extension's change moves into the tree its answer holds
        // (#241).
        if handler == DesignedHandler::Change
            && matches!(self.launcher.screen(), pane_core::Screen::PaneForm(_))
        {
            if let Some(key) = (!key.is_empty()).then_some(key) {
                self.note_sent(&key, payload_value(&payload));
                self.launcher
                    .pane_form_changed(&key, &payload_value(&payload));
                cx.notify();
            }
            return;
        }
        let key = (!key.is_empty()).then_some(key);
        if handler == DesignedHandler::Change
            && let Some(named) = key.as_deref()
        {
            self.note_sent(named, payload_value(&payload));
        }
        let pending = self.launcher.send_designed_seen(
            handler,
            callback,
            key.as_deref(),
            Some(seen),
            payload,
        );
        self.show_until_done(pending, window, cx);
    }

    /// Notes the value a discrete control reported, so a submission reads
    /// it while the tree that carries it has not landed (#241).
    fn note_sent(&mut self, key: &str, value: String) {
        if let Some(controls) = self.designed.as_mut() {
            controls.sent.insert(key.to_owned(), value);
        }
    }

    /// The field at `path`'s text changed, as the user typed or composed
    /// it: the frame redraws its live value, and the input event its tree
    /// asked for is sent — coalesced to the latest while one is in
    /// flight, and held to the throttle's time when the field asks for
    /// one.
    fn designed_input_changed(
        &mut self,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
        let changed = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(path)?;
            let Held::Field {
                events, editing, ..
            } = &mut entry.held
            else {
                return None;
            };
            if events.placing {
                // The extension set the value; its own echo is not the
                // user's typing.
                events.placing = false;
                return None;
            }
            events.on_input?;
            events.pending = Some(editing.read(cx).as_str().to_owned());
            Some(())
        });
        if changed.is_some() {
            self.flush_designed_input(path, window, cx);
        }
    }

    /// Sends the field at `path`'s waiting input event, if its state
    /// allows one now: none while another is in flight (the waiting value
    /// replaces whatever waited before, and goes out when the in-flight
    /// event's answer lands), and none before the field's throttle time
    /// has passed, when it asks for one — the timer armed here fires then
    /// and sends it.
    fn flush_designed_input(
        &mut self,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let send = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(path)?;
            let (render, key) = (entry.render, entry.key.clone());
            let Held::Field { events, .. } = &mut entry.held else {
                return None;
            };
            if events.in_flight {
                return None;
            }
            let value = events.pending.clone()?;
            if let (Some(sent), Some(throttle)) = (events.sent, events.throttle) {
                let elapsed = cx.background_executor().now().duration_since(sent);
                if elapsed < throttle {
                    // Held to the throttle's time; one timer, armed once,
                    // sends whatever is waiting when it fires.
                    if !events.armed {
                        events.armed = true;
                        let waits = throttle - elapsed;
                        let path = path.to_owned();
                        cx.spawn_in(window, async move |this, cx| {
                            cx.background_executor().timer(waits).await;
                            let _ = this.update_in(cx, |this, window, cx| {
                                this.designed_input_timer(&path, window, cx);
                            });
                        })
                        .detach();
                    }
                    return None;
                }
            }
            events.pending = None;
            events.in_flight = true;
            events.sent = Some(cx.background_executor().now());
            events.reported(&value);
            Some((events.on_input?, render, key, value))
        });
        let Some((callback, render, key, value)) = send else {
            return;
        };
        let payload = payload(&value);
        let pending = self.launcher.send_designed_seen(
            DesignedHandler::Input,
            callback,
            (!key.is_empty()).then_some(key.as_str()),
            Some(render),
            payload,
        );
        // The answer's arrival clears the way for the waiting value, if
        // the user typed one while this event was in flight.
        let path = path.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            pending.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.designed_input_answered(&path, window, cx);
            });
        })
        .detach();
    }

    /// The throttle's time passed: whatever input value was held goes out
    /// now, if its state allows one.
    fn designed_input_timer(
        &mut self,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(controls) = self.designed.as_mut()
            && let Some(entry) = controls.state.get_mut(path)
            && let Held::Field { events, .. } = &mut entry.held
        {
            events.armed = false;
        }
        self.flush_designed_input(path, window, cx);
    }

    /// The in-flight input event of the field at `path` was answered: the
    /// tree its answer drew is reconciled (a value it named is placed),
    /// and the value waiting behind it goes out now.
    fn designed_input_answered(
        &mut self,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(controls) = self.designed.as_mut()
            && let Some(entry) = controls.state.get_mut(path)
            && let Held::Field { events, .. } = &mut entry.held
        {
            events.in_flight = false;
        }
        self.sync_screen(window, cx);
        cx.notify();
        self.flush_designed_input(path, window, cx);
    }

    /// Commits the field at `path`, as Enter and a blur do: its value, if
    /// it moved since the last commit, is told to the extension through
    /// the change handler its tree names, and any input waiting to be
    /// told goes with it — the commit carries the newest value.
    fn designed_field_committed(
        &mut self,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        // Enter in a single-line field of a form submits it (#241): the
        // form's primary action, not the field's own commit — the
        // submission carries the field's value. A blur does not: leaving
        // a field is not the form's action, or opening its select would
        // submit the half of it already typed.
        if self.field_in_form(path) {
            self.submit_designed_form(window, cx);
            return;
        }
        self.designed_field_value_committed(path, window, cx);
    }

    /// A field's commit as a blur makes it: the live value is the value
    /// the tree names, told to the field's own change handler — never
    /// the form's submission, which only Enter and the submit button
    /// run. A field in a form commits its value alone.
    fn designed_field_value_committed(
        &mut self,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let send = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(path)?;
            let (render, key) = (entry.render, entry.key.clone());
            let Held::Field {
                events, editing, ..
            } = &mut entry.held
            else {
                return None;
            };
            let live = editing.read(cx).as_str().to_owned();
            if live == events.committed {
                return None;
            }
            events.committed = live.clone();
            events.reported(&live);
            // The commit carries the newest value; the input waiting
            // behind it would tell an older one.
            events.pending = None;
            Some((events.on_change, render, key, live))
        });
        let Some((Some(callback), render, key, value)) = send else {
            return;
        };
        let payload = payload(&value);
        self.designed_event(
            DesignedHandler::Change,
            callback,
            key,
            render,
            payload,
            window,
            cx,
        );
    }

    /// A focus or blur of the node at `path`: a field's blur commits it
    /// (as Enter does), and the handler its tree names is raised.
    fn designed_focus_event(
        &mut self,
        path: &str,
        focused: bool,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if !focused {
            self.designed_field_value_committed(path, window, cx);
        }
        let send = self.designed.as_ref().and_then(|controls| {
            let entry = controls.state.get(path)?;
            let watching = entry.watching.as_ref()?;
            let callback = if focused {
                watching.on_focus
            } else {
                watching.on_blur
            }?;
            Some((callback, entry.render, entry.key.clone()))
        });
        let Some((callback, render, key)) = send else {
            return;
        };
        let handler = if focused {
            DesignedHandler::Focus
        } else {
            DesignedHandler::Blur
        };
        self.designed_event(handler, callback, key, render, "{}".into(), window, cx);
    }

    /// A keystroke over the designed view, for the focused node that asks
    /// for key events: the key, as a key sequence spells it, is told to
    /// its handler. Tab, Enter and Escape stay with Pane, and so does any
    /// key that acted (an action's binding matched it); a field's typing
    /// is its text, not its keys.
    fn designed_key_event(
        &mut self,
        event: &gpui::KeystrokeEvent,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if event.action.is_some() {
            return;
        }
        let keystroke = &event.keystroke;
        if matches!(keystroke.key.as_str(), "tab" | "enter" | "escape") {
            return;
        }
        let send = self.designed.as_ref().and_then(|controls| {
            controls
                .state
                .iter()
                .find(|(_, entry)| {
                    entry
                        .focus_handle()
                        .is_some_and(|handle| handle.is_focused(window))
                })
                .and_then(|(_, entry)| {
                    let callback = entry.on_key?;
                    // A field's typing is its text; its keys are the ones
                    // that do not type.
                    let field = matches!(entry.held, Held::Field { .. });
                    (!field || keystroke.key_char.is_none())
                        .then(|| (callback, entry.render, entry.key.clone()))
                })
        });
        let Some((callback, render, key)) = send else {
            return;
        };
        let payload = key_payload(keystroke);
        self.designed_event(
            DesignedHandler::Key,
            callback,
            key,
            render,
            payload,
            window,
            cx,
        );
    }

    /// The select at `path`'s choice was committed by its user: the
    /// change handler its tree names is told the value chosen.
    fn designed_select_committed(
        &mut self,
        path: &str,
        value: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let send = self.designed.as_ref().and_then(|controls| {
            let entry = controls.state.get(path)?;
            let Held::Select { on_change, .. } = &entry.held else {
                return None;
            };
            Some((*on_change, entry.render, entry.key.clone()))
        });
        let Some((callback, render, key)) = send else {
            return;
        };
        // The chosen value is what the trigger names and a submission of
        // the form reads, whether or not the tree named a change handler
        // for the select: noted as sent, the tree that carries it not
        // having landed (#241). A select of a form Pane itself asks —
        // which names no change handler of its own — still commits its
        // choice into the tree the screen holds, as `designed_event`
        // would take an extension's change into the tree its answer
        // holds (#241).
        if !key.is_empty() {
            self.note_sent(&key, value.to_owned());
            cx.notify();
        }
        if callback.is_none() {
            if matches!(self.launcher.screen(), pane_core::Screen::PaneForm(_)) && !key.is_empty() {
                self.launcher.pane_form_changed(&key, value);
            }
            return;
        }
        let payload = payload(value);
        self.designed_event(
            DesignedHandler::Change,
            callback.unwrap_or_default(),
            key,
            render,
            payload,
            window,
            cx,
        );
    }

    /// The select at `path`'s popup query, as the user typed it into a
    /// dropdown whose search the extension handles (#241): the query is
    /// told to the extension through the input handler its tree names,
    /// its answer replacing the choices the popup shows.
    fn designed_select_queried(
        &mut self,
        path: &str,
        text: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let send = self.designed.as_ref().and_then(|controls| {
            let entry = controls.state.get(path)?;
            let Held::Select { on_input, .. } = &entry.held else {
                return None;
            };
            let callback = *on_input;
            Some((callback?, entry.render, entry.key.clone()))
        });
        let Some((callback, render, key)) = send else {
            return;
        };
        let payload = payload(text);
        self.designed_event(
            DesignedHandler::Input,
            callback,
            key,
            render,
            payload,
            window,
            cx,
        );
    }

    /// The select at `path`'s model, read live each frame it draws: the
    /// choices and the choice the tree on screen names.
    fn designed_select_model(&self, path: &str, cx: &gpui::App) -> crate::ui::select::Model {
        let visuals = crate::settings::launcher_visuals(cx);
        let tree = match self.launcher.screen() {
            pane_core::Screen::DesignedView(view) => Some(view.tree),
            pane_core::Screen::PaneForm(form) => Some(form.tree),
            _ => None,
        };
        // The choices and the one chosen, read from the node at `path`: a
        // select's, or a List's search-bar dropdown's (#240), whose items
        // are the choices and whose value the one chosen.
        let read = tree
            .as_ref()
            .and_then(|tree| tree::node_at(tree, path))
            .and_then(|node| match &node.kind {
                pane_core::NodeKind::Select(select) => Some((
                    select
                        .options
                        .iter()
                        .map(|option| crate::ui::select::Choice {
                            id: option.value.clone().into(),
                            label: option
                                .label
                                .clone()
                                .unwrap_or_else(|| option.value.clone())
                                .into(),
                            subtitle: None,
                            keywords: Vec::new(),
                            unavailable_reason: None,
                            section: option.section.clone().map(gpui::SharedString::from),
                        })
                        .collect::<Vec<_>>(),
                    select.value.clone(),
                    node.key.clone(),
                )),
                pane_core::NodeKind::ListDropdown(dropdown) => Some((
                    dropdown
                        .items
                        .iter()
                        .map(|item| crate::ui::select::Choice {
                            id: item.value.clone().into(),
                            label: item
                                .label
                                .clone()
                                .unwrap_or_else(|| item.value.clone())
                                .into(),
                            subtitle: None,
                            keywords: Vec::new(),
                            unavailable_reason: None,
                            section: None,
                        })
                        .collect::<Vec<_>>(),
                    dropdown.value.clone(),
                    node.key.clone(),
                )),
                _ => None,
            });
        let (choices, committed, key) = read.unwrap_or_default();
        // A choice the window holds as sent, the tree that carries it not
        // having landed: the trigger names it, as a submission reads it
        // (#241).
        let committed = self
            .designed
            .as_ref()
            .and_then(|controls| {
                key.as_deref()
                    .and_then(|key| controls.sent.get(key).cloned())
            })
            .or(committed);
        crate::ui::select::Model {
            theme: visuals.theme,
            material: visuals.material,
            choices,
            committed: committed.map(gpui::SharedString::from),
        }
    }
}

/// The value the tree of the open screen names for the select keyed
/// `key`, when one does.
fn select_value_of(launcher: &pane_core::Launcher, key: &str) -> Option<String> {
    let tree = match launcher.screen() {
        pane_core::Screen::DesignedView(view) => Some(view.tree),
        pane_core::Screen::PaneForm(form) => Some(form.tree),
        _ => None,
    }?;
    fn of(node: &pane_core::Node, key: &str) -> Option<String> {
        if node.key.as_deref() == Some(key)
            && let pane_core::NodeKind::Select(select) = &node.kind
        {
            return select.value.clone();
        }
        node.children.iter().find_map(|child| of(child, key))
    }
    of(&tree.root, key)
}

/// The least time between the search events of a designed List whose view
/// handles the search itself, as the fields' input events are throttled:
/// Raycast's default.
const SEARCH_THROTTLE: Duration = Duration::from_millis(250);

/// The bookkeeping of the designed list's search field's changes: the
/// text waiting to be told to the view, one event in flight at a time,
/// and when the last was sent — the fields' input events' rules (#238)
/// for the header's field (#240).
#[derive(Default)]
pub(crate) struct SearchEvents {
    pending: Option<String>,
    in_flight: bool,
    sent: Option<Instant>,
    armed: bool,
}

impl LauncherWindow {
    /// Whether the open designed view's List handles the search itself:
    /// its field's changes are throttled, not sent as they are typed.
    pub(crate) fn designed_list_searches(&self) -> bool {
        match self.launcher.screen() {
            pane_core::Screen::DesignedView(view) => {
                view.list.as_ref().is_some_and(|list| list.searching)
            }
            _ => false,
        }
    }

    /// The designed list's search field changed to `text`, as the user
    /// typed it: the text waits to be told to the view, one event in
    /// flight at a time and none before the throttle's time has passed.
    pub(crate) fn designed_search_typed(&mut self, text: &str, cx: &mut Context<Self>) {
        // [diag-ff23] Diagnostic scaffolding for the pane twin's typed
        // search that never starts: the typing's arrival, and whether the
        // flush's state allows a send. fix-forward-24 removes it with the
        // root fix.
        pane_core::diagnostic!(
            "[diag-ff23] typed: text={text:?} in_flight={}",
            self.designed_search.in_flight
        );
        self.designed_search.pending = Some(text.to_owned());
        self.designed_search_flush(cx);
    }

    /// Sends the search text waiting to be told to the view, if its state
    /// allows one now: none while another is in flight (the waiting text
    /// replaces whatever waited before, and goes out when the in-flight
    /// event's answer lands), and none before the throttle's time has
    /// passed since the last.
    fn designed_search_flush(&mut self, cx: &mut Context<Self>) {
        let send = {
            let events = &mut self.designed_search;
            if events.in_flight {
                None
            } else if let (Some(sent), true) = (events.sent, true) {
                let elapsed = cx.background_executor().now().duration_since(sent);
                if elapsed < SEARCH_THROTTLE {
                    if !events.armed {
                        events.armed = true;
                        let waits = SEARCH_THROTTLE - elapsed;
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(waits).await;
                            let _ = this.update(cx, |this, cx| {
                                this.designed_search_timer(cx);
                            });
                        })
                        .detach();
                    }
                    None
                } else {
                    events.pending.take()
                }
            } else {
                events.pending.take()
            }
        };
        let Some(text) = send else {
            // [diag-ff23] Diagnostic scaffolding; removed with the root
            // fix.
            pane_core::diagnostic!("[diag-ff23] typed held: nothing sent");
            return;
        };
        // [diag-ff23] Diagnostic scaffolding; removed with the root fix.
        pane_core::diagnostic!("[diag-ff23] typed sent: text={text:?}");
        self.designed_search.in_flight = true;
        self.designed_search.sent = Some(cx.background_executor().now());
        let pending = self.launcher.set_query(&text);
        cx.spawn(async move |this, cx| {
            pending.await;
            let _ = this.update(cx, |this, cx| {
                this.designed_search_answered(cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// The throttle's time passed: whatever text was held goes out now.
    fn designed_search_timer(&mut self, cx: &mut Context<Self>) {
        self.designed_search.armed = false;
        self.designed_search_flush(cx);
    }

    /// The in-flight search event was answered: the tree its answer drew
    /// is shown, and the text waiting behind it goes out now.
    fn designed_search_answered(&mut self, cx: &mut Context<Self>) {
        self.designed_search.in_flight = false;
        cx.notify();
        self.designed_search_flush(cx);
    }
}

/// What the system's dialog for a designed form's picker chooses: files
/// or folders (a macOS application is a folder, its bundle), one path or
/// several (#241).
#[derive(Clone, Copy)]
pub(super) struct PathPick {
    pub(super) files: bool,
    pub(super) directories: bool,
    pub(super) multiple: bool,
}

/// What a form's collection found: the form's key and each field's
/// value, keyed by its key.
struct Collected {
    form: Option<String>,
    values: Vec<(String, pane_core::FormValue)>,
}

/// Collects the values of the fields under `node` — the form `form`
/// holds, when one is given — into `collected`, with the keyed state the
/// window keeps for them. A field without a key is not collected: its
/// value has nowhere to be named by.
fn collect_node(
    node: &Node,
    form: Option<&str>,
    path: &mut String,
    state: &std::collections::HashMap<String, reconcile::KeyedState>,
    sent: &std::collections::HashMap<String, String>,
    collected: &mut Collected,
    cx: &gpui::App,
) {
    let form = match &node.kind {
        pane_core::NodeKind::Form(_) => node.key.as_deref().or(Some("form")),
        _ => form,
    };
    if form.is_some()
        && let Some(key) = node.key.clone()
        && let Some(value) = field_value(node, path, state, sent, cx)
    {
        collected.values.push((key, value));
    }
    if let pane_core::NodeKind::Form(_) = &node.kind
        && collected.form.is_none()
    {
        collected.form = Some(node.key.clone().unwrap_or_else(|| "form".into()));
    }
    let duplicates = tree::duplicate_keys(node);
    for (index, child) in node.children.iter().enumerate() {
        let start = path.len();
        tree::place_child(path, child, index, &duplicates);
        collect_node(child, form, path, state, sent, collected, cx);
        path.truncate(start);
    }
}

/// The value the field `node` holds, from the state the window keeps for
/// it: the text a field edits, the state a checkbox or toggle last told
/// the extension (until the tree carries it), the choice a dropdown
/// last committed, the tags or paths a picker chose.
fn field_value(
    node: &Node,
    path: &str,
    state: &std::collections::HashMap<String, reconcile::KeyedState>,
    sent: &std::collections::HashMap<String, String>,
    cx: &gpui::App,
) -> Option<pane_core::FormValue> {
    use pane_core::FormValue;
    let key = node.key.as_deref()?;
    let text = state.get(path).and_then(|entry| match &entry.held {
        Held::Field { editing, .. } => Some(editing.read(cx).as_str().to_owned()),
        _ => None,
    });
    Some(match &node.kind {
        pane_core::NodeKind::TextInput(_)
        | pane_core::NodeKind::PasswordInput(_)
        | pane_core::NodeKind::TextArea(_) => FormValue::Text(text?),
        pane_core::NodeKind::DatePicker(date) | pane_core::NodeKind::DateTimePicker(date) => {
            FormValue::Text(text.unwrap_or_else(|| date.value.clone()))
        }
        pane_core::NodeKind::Select(select) => FormValue::Text(
            sent.get(key)
                .cloned()
                .or_else(|| select.value.clone())
                .unwrap_or_default(),
        ),
        pane_core::NodeKind::Toggle(toggle) => FormValue::On(
            sent.get(key)
                .map(|value| value == "true")
                .unwrap_or(toggle.on),
        ),
        pane_core::NodeKind::Checkbox(checkbox) => FormValue::On(
            sent.get(key)
                .map(|value| value == "true")
                .unwrap_or(checkbox.checked),
        ),
        pane_core::NodeKind::TagPicker(picker) => {
            let tags = state.get(path).and_then(|entry| match &entry.held {
                Held::Tags { chosen, .. } => Some(chosen.clone()),
                _ => None,
            });
            FormValue::List(tags.unwrap_or_else(|| picker.tags.clone()))
        }
        pane_core::NodeKind::FilePicker(picker) | pane_core::NodeKind::FolderPicker(picker) => {
            if picker.multiple {
                let paths = state.get(path).and_then(|entry| match &entry.held {
                    Held::Paths { paths, .. } => Some(paths.clone()),
                    _ => None,
                });
                FormValue::List(paths.unwrap_or_else(|| picker.paths.clone()))
            } else {
                FormValue::Text(text?)
            }
        }
        _ => return None,
    })
}

/// A list as a change event's payload names it: `{"value": ["…", …]}`,
/// a picker's chosen tags or paths.
pub(super) fn list_payload(values: &[String]) -> String {
    let joined = values
        .iter()
        .map(|value| format!("\"{}\"", escape(value)))
        .collect::<Vec<String>>()
        .join(",");
    format!("{{\"value\":[{joined}]}}")
}

/// A string's JSON escapes, for a payload's list.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// A value as a change event's payload names it: `{"value": …}`, a string
/// escaped as JSON.
pub(super) fn payload(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                escaped.push_str(&format!("\\u{:04x}", other as u32));
            }
            other => escaped.push(other),
        }
    }
    format!("{{\"value\":\"{escaped}\"}}")
}

/// A value as a change event's payload names it: `{"value": …}`, a
/// boolean or a number.
pub(super) fn plain_payload(value: impl std::fmt::Display) -> String {
    format!("{{\"value\":{value}}}")
}

/// The value a change event's payload names, as a string: a string as it
/// is, a boolean as `true` or `false`. A submission's collection reads
/// the same payload a control's change carries.
pub(super) fn payload_value(payload: &str) -> String {
    let value = payload
        .split_once("\"value\"")
        .map(|(_, rest)| rest.trim_start().trim_start_matches(':').trim_start())
        .unwrap_or("");
    if let Some(rest) = value.strip_prefix('"') {
        let mut result = String::new();
        let mut characters = rest.chars();
        while let Some(character) = characters.next() {
            match character {
                '"' => break,
                '\\' => match characters.next() {
                    Some('n') => result.push('\n'),
                    Some('r') => result.push('\r'),
                    Some('t') => result.push('\t'),
                    Some(escaped) => result.push(escaped),
                    None => {}
                },
                other => result.push(other),
            }
        }
        return result;
    }
    value
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .collect()
}

/// A key as a key event's payload names it: `{"key": …}`, the keystroke
/// as a key sequence spells it (its modifiers, then its key).
pub(super) fn key_payload(keystroke: &gpui::Keystroke) -> String {
    let mut keys: Vec<&str> = Vec::new();
    if keystroke.modifiers.control {
        keys.push("ctrl");
    }
    if keystroke.modifiers.alt {
        keys.push("alt");
    }
    if keystroke.modifiers.function {
        keys.push("fn");
    }
    if keystroke.modifiers.shift {
        keys.push("shift");
    }
    keys.push(keystroke.key.as_str());
    let joined = keys.join("+");
    let mut escaped = String::with_capacity(joined.len() + 2);
    for character in joined.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            other => escaped.push(other),
        }
    }
    format!("{{\"key\":\"{escaped}\"}}")
}
