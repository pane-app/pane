//! Pane's searchable select: the choice control for a setting whose
//! options a user may want to search rather than scan — a trigger row
//! showing the committed choice, opening an L2 popover that holds a
//! local search field and the choices as rows. The search is local and
//! optional to *use*: a blank query lists every choice in the
//! consumer's order, and typing narrows it — no list is too short for
//! the control, and no search is forced on one.
//!
//! Interaction reference: the Raycast dropdown and the shadcn combobox
//! as *behavior* references only (`docs/research/searchable-settings-
//! selects.md`) — the visual language is entirely Pane's, the reference's
//! families (#99, `ui::controls`): the trigger an inline well at a settings
//! row's end (the row names the setting and says what it does), the L2
//! popover material with its own shadows at the trigger's width, the well
//! for the popup's search field, and the Actions panel's entries for the
//! choices. Nothing fades:
//! the Settings board's controls change at once. Nothing here imports a
//! React toolkit, re-creates the theme, or knows what a setting is.
//!
//! ## State: the control's own, and only the control's
//!
//! The control is one [`Select`] entity, embedded by its consumer as a
//! child. It owns everything transient — open or closed, the query, the
//! highlighted (active) choice — and owns its whole interaction: the
//! popup's opening and dismissal, the query's local filtering, the
//! arrows' movement of the highlight, Enter's commit, Escape's cancel,
//! Tab's leave. The committed choice is *not* transient state: the
//! consumer supplies it live through the model callback every render,
//! and commits travel out through the `on_commit` callback into the
//! consumer's own persistence path. Opening, filtering and highlighting
//! never save anything; a failed save is the consumer's to report, and
//! the trigger simply shows whatever the model holds next — the honest
//! choice, rolled back or not.
//!
//! ## The draft/commit boundary
//!
//! While open, the query filters the choices and the arrows move the
//! highlight; neither touches the committed choice. Enter, or a click on
//! an enabled row, commits that choice through `on_commit` and closes.
//! Escape cancels the whole draft — query and highlight are discarded,
//! nothing is committed — and returns focus to the trigger. Tab and
//! Shift-Tab close without committing and continue focus traversal from
//! the trigger. A mouse-down outside the popup cancels the draft and
//! lets the click land: whatever was clicked takes focus as it would
//! have without the popup. A mouse-down on the trigger itself toggles
//! the popup closed, consuming the click so it cannot reopen.
//!
//! This deliberately differs from the extension form's choice control,
//! where the arrows change the choice and Enter submits the form: this
//! is a settings control with its own draft/commit boundary, and the
//! page around it commits nothing on the form's behalf.
//!
//! ## Placement and constraint
//!
//! The popup is a [`deferred`] + [`anchored`] overlay: deferred so it
//! paints above the whole window (and is not clipped by the page's
//! scroll viewport), anchored below the trigger — it is a child of a
//! zero-height row the control renders after the trigger, so the anchor
//! point is the trigger's bottom-left however tall the trigger grew —
//! and snapped to the window, so a popup that would overflow the
//! window's edge is constrained inside it. The option list scrolls
//! within that bound and keeps the highlighted choice visible. The
//! popup follows the trigger as the page scrolls, being deferred from
//! the trigger's own place in the scrolled content.
//!
//! The popup's own motion is the shared policy's popup family (see
//! [`crate::ui::motion`]): it enters from the trigger over a tiny shift
//! and fade, and exits back toward it, faster, on the same tween
//! machinery every Pane transition runs. The shift and the fade sit on
//! a wrapper *inside* the popup, so the anchored element still measures
//! the popup at its resting size and the window-edge snapping never
//! moves mid-flight. While the exit runs the popup is inert — no row
//! can commit, the whole subtree is hidden from accessibility, and the
//! overlay keeps occluding so a click on it cannot reach the page
//! underneath — and the frame that completes the exit unmounts it, so
//! nothing of a closed popup intercepts a click. Typing, filtering,
//! the highlight's movement and the focus contract are exactly what
//! they were: only the popup's paint moves.
//!
//! ## Accessibility
//!
//! The trigger is a combo box: named, carrying the committed choice as
//! its value and its expanded state. The popup's content is the field's
//! node, as root search's wrapper is — an editable combo box whose list
//! is the choices — the committed one selected, the highlighted one the
//! focused field's active descendant, a choice the system cannot answer
//! disabled with its reason as the description. Keyboard focus sits in
//! the field while the popup is open, so the arrows move the highlight
//! without stealing text focus from the query.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, Focusable, KeyBinding,
    MouseDownEvent, Pixels, Role, ScrollHandle, SharedString, Subscription, Window, actions,
    anchored, deferred, div, prelude::*, px,
};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};

use super::controls;
use super::icon::Glyph;
use super::material::{Material, popover_shadows};
use super::theme::Theme;

/// The trigger's key context: its activation keys (Enter, Space, Down)
/// are bound here, so they open the select rather than acting as the
/// window's keys while the trigger holds focus.
const TRIGGER: &str = "PaneSelect";

/// The open popup's key context: the navigation, commit and dismissal
/// keys are bound here over the search field's own editing context, so
/// they drive the popup only while its field is focused.
const POPUP: &str = "PaneSelectPopup";

/// The popup's search field's placeholder, and its accessible name.
const PLACEHOLDER: &str = "Search choices";

/// The popup's option list's height bound: the list scrolls past this,
/// keeping the highlighted choice visible, so the popup stays a
/// reasonable overlay even over a long choice list.
const LIST_MAX_HEIGHT: Pixels = px(300.);

/// The space between the trigger and the popup below it, and the least
/// the popup keeps from the window's edges.
const POPUP_GAP: Pixels = px(4.);

actions!(
    pane_select,
    [
        OpenSelect,
        NextChoice,
        PreviousChoice,
        FirstChoice,
        LastChoice,
        CommitChoice,
        CancelSelect,
        LeaveForward,
        LeaveBackward
    ]
);

/// Registers the select's key bindings: the trigger's activation keys in
/// the trigger's context, and the popup's navigation, commit and
/// dismissal keys in the popup's context above the editable text
/// element's own — the same nesting the Settings search's field uses,
/// so the arrows take the keystrokes from the caret only while the
/// field is focused, and Tab, Enter and Escape (left unbound for every
/// editable element) never reach the window's traversal or the page.
pub(crate) fn bind_keys(cx: &mut App) {
    let field = format!("{POPUP} > {DEFAULT_INPUT_CONTEXT}");
    cx.bind_keys([
        KeyBinding::new("enter", OpenSelect, Some(TRIGGER)),
        KeyBinding::new("space", OpenSelect, Some(TRIGGER)),
        KeyBinding::new("down", OpenSelect, Some(TRIGGER)),
        KeyBinding::new("down", NextChoice, Some(&field)),
        KeyBinding::new("up", PreviousChoice, Some(&field)),
        KeyBinding::new("home", FirstChoice, Some(&field)),
        KeyBinding::new("end", LastChoice, Some(&field)),
        KeyBinding::new("enter", CommitChoice, Some(&field)),
        KeyBinding::new("escape", CancelSelect, Some(&field)),
        KeyBinding::new("tab", LeaveForward, Some(&field)),
        KeyBinding::new("shift-tab", LeaveBackward, Some(&field)),
    ]);
}

/// One choice the select offers: its stable identity, what the row
/// shows, and the words the local search matches beside the label. A
/// choice the system cannot answer carries its reason, is listed with
/// it, and cannot be committed.
#[derive(Clone, Debug)]
pub(crate) struct Choice {
    /// The choice's stable id — the identity commits travel out as, and
    /// the one the consumer's saved choice comes back as.
    pub(crate) id: SharedString,
    /// The row's label: what the trigger shows when this choice is
    /// committed, and what the search matches first.
    pub(crate) label: SharedString,
    /// The row's subtitle, if it has one.
    pub(crate) subtitle: Option<SharedString>,
    /// The words the local search matches beside the label — the
    /// consumer's own declared vocabulary for the choice, not words
    /// invented here.
    pub(crate) keywords: Vec<SharedString>,
    /// Why this choice cannot be used here, if it cannot: the row lists
    /// it, cannot be committed, and reads it as its description.
    pub(crate) unavailable_reason: Option<SharedString>,
}

/// What the consumer supplies the control live, every render: the
/// visuals to draw with, the choices as they stand, and which of them
/// is committed. Nothing here is cached — the control re-reads it each
/// frame, so a choice that appears, goes or changes underneath it is
/// what the next frame shows, exactly as the Settings search re-reads
/// its catalog.
pub(crate) struct Model {
    /// The theme the control renders with.
    pub(crate) theme: Theme,
    /// The material the popup's surface renders with.
    pub(crate) material: Material,
    /// The choices, in the order the consumer lists them.
    pub(crate) choices: Vec<Choice>,
    /// The id of the committed choice, if one is: what the trigger
    /// shows and the list marks as selected.
    pub(crate) committed: Option<SharedString>,
}

/// The consumer's commit path, as the control holds it: the id of the
/// choice the user accepted, reported once per commit. Persistence,
/// validation and failure reporting are the consumer's; the control
/// only closes and hands the choice over.
pub(crate) type Commit = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// The searchable select control. One entity, embedded by its consumer
/// as a child (see the module docs); construct with [`Select::new`],
/// and let the consumer's render simply include it.
pub(crate) struct Select {
    /// The control's accessible name, and the trigger row's title.
    name: SharedString,
    /// What the trigger row says under its title: the consumer's own
    /// one-line description of the setting.
    description: SharedString,
    /// The prefix of the control's debug selectors, the tests' and
    /// smokes' convention: the trigger is `{debug}`, the popup
    /// `{debug}-popup`, a choice's row `{debug}-{id}`, the field
    /// `{debug}-query` and the no-results line `{debug}-empty`.
    debug: SharedString,
    /// The consumer's live model, read each render.
    model: Rc<dyn Fn(&App) -> Model>,
    /// The consumer's commit path (see [`Commit`]).
    on_commit: Commit,
    /// Whether the popup is open.
    open: bool,
    /// The popup's search field.
    query: Entity<EditableTextState>,
    /// The query's changes, moving the highlight to the first enabled
    /// match as the query narrows.
    _query_changes: Subscription,
    /// The popup's dismissal when the window loses activation, as
    /// native popovers close; ends with the control.
    _deactivation: Subscription,
    /// The highlighted (active) choice's id, if one is: navigation
    /// state only — never the committed choice, which the model holds.
    active: Option<SharedString>,
    /// The popup's entrance or exit in flight, if any: the popup's look
    /// (0 closed, 1 open) and its fade, presentation only — see
    /// [`crate::ui::motion`]. The draft below it outlives the close that
    /// started the exit (a fresh open resets it), so the exiting popup
    /// paints what the user saw; nothing interactive reads it while the
    /// exit runs.
    transition: crate::ui::motion::PopupMotion,
    /// Whether the last drawn frame drew the popup open — the one thing
    /// that starts or retargets the transition, so a reopen during the
    /// exit reverses from the presentation on screen.
    drawn_open: bool,
    /// Whether the next [`TextChanged`] the field reports is the popup's
    /// own draft reset (the clear a fresh open makes), not the user's
    /// typing: the reset is not a navigation, so it must not move the
    /// highlight off the open's start.
    opened: bool,
    /// The popup's presentation (offset from rest toward the trigger in
    /// px, opacity) as the last frame drew it; `None` when the last
    /// frame drew the popup settled — at rest while open, absent while
    /// closed. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    drawn_popup: Option<(f32, f32)>,
    /// The trigger's focus: a tab stop, where Escape and a commit
    /// return the keyboard.
    trigger: FocusHandle,
    /// The option list's scroll, keeping the highlighted choice
    /// visible.
    scroll: ScrollHandle,
}

impl Select {
    /// The select control over `model`, committing through `on_commit`.
    ///
    /// `name` and `description` are the trigger row's title and
    /// subtitle; `debug` is the prefix of the control's debug
    /// selectors. `window` is the window the control lives in — the
    /// popup closes when it loses activation.
    pub(crate) fn new(
        name: impl Into<SharedString>,
        description: impl Into<SharedString>,
        debug: impl Into<SharedString>,
        model: Rc<dyn Fn(&App) -> Model>,
        on_commit: Commit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Select {
        let query = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        // Not a tab stop: the field is reached by opening the popup, not
        // by traversal — while the popup is open, Tab leaves it (and
        // continues traversal), and while it is closed the field is not
        // in the tree at all.
        query.focus_handle(cx).tab_stop(false);
        let _query_changes = cx.subscribe(&query, |this, _, _: &TextChanged, cx| {
            // The draft's reset when the popup opens is not a navigation:
            // the open's own start (the committed choice, else the first
            // that can be used) stands, and only the user's next keystroke
            // moves the highlight.
            if this.opened {
                this.opened = false;
                return;
            }
            // A new query is a new navigation, as every search of
            // Pane's treats it: the first enabled match is highlighted.
            let model = (this.model.clone())(cx);
            let query = this.query.read(cx).as_str().to_owned();
            this.active = first_enabled(&model, &query);
            cx.notify();
        });
        let _deactivation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() && this.open {
                this.close(window, cx);
            }
        });
        Select {
            name: name.into(),
            description: description.into(),
            debug: debug.into(),
            model,
            on_commit,
            open: false,
            query,
            _query_changes,
            _deactivation,
            active: None,
            transition: Default::default(),
            drawn_open: false,
            opened: false,
            #[cfg(any(test, debug_assertions))]
            drawn_popup: None,
            trigger: cx.focus_handle().tab_stop(true),
            scroll: ScrollHandle::new(),
        }
    }

    /// The popup's search field, for tests that drive composition the
    /// way a platform input method does.
    #[doc(hidden)]
    pub(crate) fn query(&self) -> &Entity<EditableTextState> {
        &self.query
    }

    /// The trigger's focus handle, for a consumer whose settings search
    /// jumps to the control and focuses it.
    pub(crate) fn trigger_focus(&self) -> FocusHandle {
        self.trigger.clone()
    }

    /// Test support: the popup's presentation as the last frame drew it
    /// — the offset from rest toward the trigger in px and the opacity;
    /// `None` when the last frame drew the popup settled (at rest while
    /// open, absent while closed), which is also what reduced motion
    /// ever reports. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub(crate) fn popup_presentation(&self) -> Option<(f32, f32)> {
        self.drawn_popup
    }

    /// Whether `choice` matches `query`: every whitespace-separated word
    /// of the query appears, caselessly, in the choice's label or one of
    /// its declared keywords. An empty query matches every choice. The
    /// matching is local and deterministic — no ranking, no reordering:
    /// the choices keep the consumer's order, so their identities and
    /// positions stay stable while filtering. (This is deliberately not
    /// the Settings search's matcher: that one ranks a whole catalog of
    /// settings; this one filters one control's rows.)
    fn matches(choice: &Choice, query: &str) -> bool {
        let lower = query.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();
        if words.is_empty() {
            return true;
        }
        let label = choice.label.to_lowercase();
        words.iter().all(|word| {
            label.contains(*word)
                || choice
                    .keywords
                    .iter()
                    .any(|keyword| keyword.to_lowercase().contains(*word))
        })
    }

    /// The choices matching the query as it stands, as indices into the
    /// model's list, in the model's order.
    fn filtered(&self, model: &Model, query: &str) -> Vec<usize> {
        model
            .choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| Self::matches(choice, query))
            .map(|(index, _)| index)
            .collect()
    }

    /// The indices of the filtered choices that can be committed.
    fn enabled(filtered: &[usize], model: &Model) -> Vec<usize> {
        filtered
            .iter()
            .copied()
            .filter(|&index| model.choices[index].unavailable_reason.is_none())
            .collect()
    }

    /// Opens the popup: the field takes focus (typing filters at once),
    /// the draft query starts empty, and the highlight starts on the
    /// committed choice — the one the user is most likely to keep — or
    /// the first choice that can be used. Nothing is saved by opening.
    /// The frame this draws starts the popup's entrance from the trigger
    /// (see [`crate::ui::motion`]).
    fn open_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            return;
        }
        self.open = true;
        // A fresh open starts a fresh draft: whatever a cancelled open
        // left in the field is gone — cleared first, so the start below
        // and the rows this frame draws are the fresh draft's, and the
        // field's own change report is marked as the reset it is (see
        // `opened`).
        let leftover = self.query.read(cx).as_str().to_owned();
        if !leftover.is_empty() {
            self.opened = true;
            self.query.update(cx, |query, cx| query.emplace("", cx));
        }
        let model = (self.model.clone())(cx);
        let query = self.query.read(cx).as_str().to_owned();
        let filtered = self.filtered(&model, &query);
        let committed = model
            .committed
            .as_ref()
            .and_then(|id| {
                filtered
                    .iter()
                    .copied()
                    .find(|&index| &model.choices[index].id == id)
            })
            .filter(|&index| model.choices[index].unavailable_reason.is_none());
        let start = committed
            .or_else(|| Self::enabled(&filtered, &model).first().copied())
            .map(|index| model.choices[index].id.clone());
        self.active = start;
        let field = self.query.focus_handle(cx);
        window.focus(&field, cx);
        cx.notify();
    }

    /// Closes the popup without committing anything: the keyboard
    /// returns to the trigger, and the popup's exit starts on the frame
    /// this draws — receding toward the trigger over the shorter span,
    /// inert (see the module docs). The draft query and the highlight
    /// stay exactly as the user left them for the exit to paint, and a
    /// fresh open resets both; nothing of them is saved, committed or
    /// offered while the exit runs. Whatever had focus before the popup
    /// opened is not tracked — the trigger is where the control's own
    /// contract returns it (Escape and commit); Tab continues traversal
    /// from here, and an outside click's target takes focus after the
    /// click lands on it.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        window.focus(&self.trigger, cx);
        cx.notify();
    }

    /// Commits `choice` — the user accepted it (Enter on the highlight,
    /// or a click on its row): the popup closes as a cancel does, and
    /// the choice travels out through the consumer's commit path. What
    /// the trigger shows next is whatever the consumer's model holds
    /// after that path runs — the choice, or the saved one rolled back
    /// with the failure reported beside it.
    fn commit(&mut self, id: &SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let on_commit = self.on_commit.clone();
        self.close(window, cx);
        on_commit(id, window, cx);
    }

    /// The choice the highlight is on, as the model holds it now: kept
    /// when it is still in the filtered list and can be used, else
    /// nothing. Called with the model the current frame just read, so
    /// the highlight never points at a choice that is no longer offered.
    fn active_choice<'a>(&self, model: &'a Model, filtered: &[usize]) -> Option<&'a Choice> {
        self.active.as_ref().and_then(|id| {
            filtered
                .iter()
                .copied()
                .map(|index| &model.choices[index])
                .find(|choice| &choice.id == id && choice.unavailable_reason.is_none())
        })
    }

    /// The trigger's activation — Enter, Space or Down on the trigger,
    /// or a click on it.
    fn open_action(&mut self, _: &OpenSelect, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_popup(window, cx);
    }

    /// Moves the highlight down, as far as the last choice that can be
    /// used — the codebase's every list clamps rather than wraps. From
    /// nothing, the first. Nothing is saved by moving.
    fn next_choice(&mut self, _: &NextChoice, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.move_highlight(1, cx);
    }

    /// Moves the highlight up, as far as the first.
    fn previous_choice(&mut self, _: &PreviousChoice, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.move_highlight(-1, cx);
    }

    /// The highlight to the first choice that can be used (Home).
    fn first_choice(&mut self, _: &FirstChoice, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.set_highlight_to_first(cx);
    }

    /// The highlight to the last choice that can be used (End).
    fn last_choice(&mut self, _: &LastChoice, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.set_highlight_to_last(cx);
    }

    /// Enter: commits the highlighted choice, if one is highlighted and
    /// can be used. On none — nothing matches, or every match is
    /// unavailable — Enter does nothing: no accidental value is saved.
    fn commit_action(&mut self, _: &CommitChoice, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let model = (self.model.clone())(cx);
        let query = self.query.read(cx).as_str().to_owned();
        let filtered = self.filtered(&model, &query);
        if let Some(choice) = self.active_choice(&model, &filtered) {
            let id = choice.id.clone();
            self.commit(&id, window, cx);
        }
    }

    /// Escape: cancels the draft and returns the keyboard to the
    /// trigger, the committed choice untouched.
    fn cancel_action(&mut self, _: &CancelSelect, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.close(window, cx);
    }

    /// Tab: closes the popup without committing and continues focus
    /// traversal forward from the trigger.
    fn leave_forward(&mut self, _: &LeaveForward, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.close(window, cx);
        window.focus_next(cx);
    }

    /// Shift-Tab: the same, backward.
    fn leave_backward(&mut self, _: &LeaveBackward, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.close(window, cx);
        window.focus_prev(cx);
    }

    /// A mouse-down outside the popup: the draft is cancelled and the
    /// click is left to land — whatever was clicked takes focus exactly
    /// as it would without the popup, so an outside click respects the
    /// clicked focus target. (The trigger's own press is handled before
    /// this, by the trigger: see [`Select::trigger_pressed`].)
    fn outside_down(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(window, cx);
        }
    }

    /// A mouse-down on the trigger while the popup is open: the click
    /// closes the popup, and is consumed so the click tracking never
    /// fires the trigger's own click (which would reopen it) — the
    /// trigger toggles. Runs in the capture phase (and only when the
    /// press is on the trigger), before the popup's outside dismissal:
    /// the main scene's listeners are registered before the deferred
    /// popup's, so the press on an open trigger is settled — closed and
    /// consumed — before anything else sees it.
    fn trigger_pressed(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(window, cx);
            cx.stop_propagation();
        }
    }

    /// Moves the highlight by `step` among the choices that can be
    /// used, clamped at the ends; from nothing, forward moves to the
    /// first and backward to the last. Keeps the new highlight visible
    /// in the list.
    fn move_highlight(&mut self, step: i32, cx: &mut Context<Self>) {
        let model = (self.model.clone())(cx);
        let query = self.query.read(cx).as_str().to_owned();
        let filtered = self.filtered(&model, &query);
        let enabled = Self::enabled(&filtered, &model);
        let position = enabled
            .iter()
            .position(|&index| Some(&model.choices[index].id) == self.active.as_ref());
        // Clamped at the ends, as every list of Pane's clamps; from
        // nothing, forward starts at the first and backward at the last.
        let next = match (position, step.is_positive()) {
            (None, true) => enabled.first().copied(),
            (None, false) => enabled.last().copied(),
            (Some(at), true) => enabled.get(at + 1).or_else(|| enabled.last()).copied(),
            (Some(at), false) => enabled
                .get(at.saturating_sub(1))
                .or_else(|| enabled.first())
                .copied(),
        };
        if let Some(index) = next {
            self.reveal_highlight(index, &filtered, &model, cx);
        }
    }

    /// The highlight to the first choice that can be used.
    fn set_highlight_to_first(&mut self, cx: &mut Context<Self>) {
        let model = (self.model.clone())(cx);
        let query = self.query.read(cx).as_str().to_owned();
        let filtered = self.filtered(&model, &query);
        if let Some(&index) = Self::enabled(&filtered, &model).first() {
            self.reveal_highlight(index, &filtered, &model, cx);
        }
    }

    /// The highlight to the last choice that can be used.
    fn set_highlight_to_last(&mut self, cx: &mut Context<Self>) {
        let model = (self.model.clone())(cx);
        let query = self.query.read(cx).as_str().to_owned();
        let filtered = self.filtered(&model, &query);
        if let Some(&index) = Self::enabled(&filtered, &model).last() {
            self.reveal_highlight(index, &filtered, &model, cx);
        }
    }

    /// Sets the highlight to the choice at `index` of the model (which
    /// must be filtered and usable) and scrolls the list to keep it
    /// visible — the list's own scroll, so the bounded popup never
    /// grows past its height for the highlight's sake.
    fn reveal_highlight(
        &mut self,
        index: usize,
        filtered: &[usize],
        model: &Model,
        cx: &mut Context<Self>,
    ) {
        self.active = Some(model.choices[index].id.clone());
        // The row's place among the list's children: the filtered
        // choices are the list's rows, in order.
        let row = filtered.iter().position(|&i| i == index).unwrap_or(0);
        self.scroll.scroll_to_item(row);
        cx.notify();
    }

    /// The trigger: a field's well (`ui::controls::select_trigger`, #99)
    /// carrying a combo box's semantics — the committed choice and the
    /// chevron that says a list opens; the setting's name and description
    /// are the field's label and description around it (see the render).
    /// The committed choice is what the consumer's model holds, read this
    /// frame: a save that failed has rolled it back by the time this
    /// draws, and the trigger shows the honest value. Like the Settings
    /// board's controls it answers the pointer with nothing that fades;
    /// the keyboard's focus rings the well.
    fn trigger_row(&self, model: &Model, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let theme = &model.theme;
        let committed = model
            .committed
            .as_ref()
            .and_then(|id| model.choices.iter().find(|choice| &choice.id == id))
            .map(|choice| choice.label.clone())
            .unwrap_or_else(|| "None".into());
        let ring = controls::well_shadows(true, theme);
        let row =
            controls::select_trigger(committed.clone(), theme).focus(move |row| row.shadow(ring));
        let name = self.name.clone();
        let debug = self.debug.clone();
        row.id("trigger")
            .debug_selector(move || debug.to_string())
            .key_context(TRIGGER)
            .track_focus(&self.trigger)
            .role(Role::ComboBox)
            .aria_label(name)
            .aria_value(committed)
            .aria_expanded(self.open)
            .aria_description(self.description.clone())
            .on_action(cx.listener(Self::open_action))
            .capture_any_mouse_down(cx.listener(Self::trigger_pressed))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.open_popup(window, cx);
            }))
    }

    /// The popup's search field: the shared editable text element in
    /// the boxed field the Settings search's field is. The box is
    /// presentation only — the accessibility and the focus live one
    /// level up, on the popup's content (see [`Select::popup`]), which
    /// is the field's node the way root search's wrapper is: an
    /// editable combo box whose list is the choices below it. The focus
    /// ring is drawn from the focus state read at render, since the box
    /// itself no longer tracks the handle.
    fn query_field(
        &self,
        model: &Model,
        field_focused: bool,
        draft: &str,
        inert: bool,
    ) -> gpui::Div {
        let theme = &model.theme;
        let input = &self.query;
        // A field's well (#99), the Settings search's own family, its ring
        // the focus color while the keyboard is in it.
        let field = controls::well(false, theme)
            .mb(theme.geometry.actions.list_padding)
            .shadow(controls::well_shadows(field_focused, theme))
            .child(controls::well_glyph(Glyph::Search, theme));
        // While the popup's exit paints, the draft shows as plain text:
        // an editable field takes focus on a click, and the exiting
        // visuals expose nothing interactive — not even focus, which the
        // closing frame has already returned to the trigger.
        if inert {
            let text = if draft.is_empty() {
                SharedString::from(PLACEHOLDER)
            } else {
                SharedString::from(draft)
            };
            field.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_color(if draft.is_empty() {
                        theme.text_placeholder
                    } else {
                        theme.text_title
                    })
                    .child(text),
            )
        } else {
            field.child(controls::well_input(
                text_input("query").state(input.downgrade()),
                PLACEHOLDER,
                theme,
            ))
        }
    }

    /// The popup: the L2 popover surface with the search field above
    /// the choice rows, on the anchored, deferred overlay that paints
    /// above the window and is constrained inside it. The popup is the
    /// trigger's width, 4 below it. See the module docs for the
    /// placement. `active` is the
    /// highlighted choice's id, if one is highlighted; `in_flight` is
    /// the popup's presentation while its entrance or exit runs — the
    /// offset from rest toward the trigger and the opacity — and it is
    /// `None` at rest. While the control is closed (`self.open` false)
    /// the popup is the exit's inert visuals: the rows cannot commit,
    /// and the whole subtree is hidden from accessibility, while the
    /// overlay still takes the clicks that land on it.
    fn popup(
        &self,
        model: &Model,
        filtered: &[usize],
        active: Option<&SharedString>,
        field_focused: bool,
        in_flight: Option<(f32, f32)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = &model.theme;
        let actions = &theme.geometry.actions;
        let query = self.query.read(cx).as_str().to_owned();
        let rows: Vec<AnyElement> = if filtered.is_empty() {
            let message = format!("No choices match “{}”", query.trim());
            vec![
                div()
                    .id("empty")
                    .debug_selector(move || format!("{}-empty", self.debug))
                    // A live status, so assistive technology announces that
                    // the query found nothing; not selectable. The Actions
                    // panel's own empty note ("No actions match").
                    .role(Role::Status)
                    .aria_label(message.clone())
                    .py(actions.empty_padding_y)
                    .px(actions.empty_padding_x)
                    .text_size(theme.typography.action_size)
                    .line_height(theme.typography.action_size * theme.typography.line_height)
                    .text_color(theme.text_muted)
                    .child(message)
                    .into_any_element(),
            ]
        } else {
            filtered
                .iter()
                .map(|&index| self.choice_row(model, index, active, cx))
                .collect()
        };
        let list = div()
            .id("list")
            .debug_selector(move || format!("{}-list", self.debug))
            .flex()
            .flex_col()
            .gap(actions.list_gap)
            .flex_1()
            .min_h(px(0.))
            .max_h(LIST_MAX_HEIGHT)
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .role(Role::ListBox)
            .aria_label("Choices")
            .children(rows);
        let input = &self.query;
        // The popup's content is the field's accessibility node, as root
        // search's wrapper is: it tracks the query's focus, and the list
        // is its list — so while the popup is open, the highlighted
        // choice reads as the focused field's active descendant, and
        // assistive technology follows the highlight as the arrows move
        // it. The editable text element itself has no node of its own.
        // While the exit paints, the content takes no focus either — the
        // keyboard has already returned to the trigger, and a click on
        // the fading overlay must not hand it back into what is closing
        // (the same inertness the footer menu's exit gives its list).
        let debug = format!("{}-query", self.debug);
        let content = div()
            .id("query")
            .debug_selector(move || debug.clone())
            .role(Role::EditableComboBox)
            .aria_label(PLACEHOLDER)
            .aria_value(query.clone())
            .aria_placeholder(PLACEHOLDER)
            .flex()
            .flex_col()
            .p(actions.list_padding)
            .when(self.open, |content| {
                content.track_focus(&input.focus_handle(cx))
            })
            .child(self.query_field(model, field_focused, &query, !self.open))
            .child(list);
        // The popup's motion wrapper: always in the tree while the popup
        // paints, with the entrance/exit's shift and fade as no-op styles
        // at rest, so the surface it wraps keeps its identity and state
        // across the transition. The shift is the same relative-inset
        // treatment the view transitions use — applied after layout, so
        // the anchored element still measures the popup at its resting
        // size, and the window-edge snapping never moves mid-flight —
        // and the elevation shadow sits on the wrapper, which GPUI
        // paints behind the surface's translucent fill, so the shadow
        // follows the surface it belongs to (the same treatment the
        // footer menu's popup gives its popover).
        let (offset, opacity) = in_flight.unwrap_or((0., 1.));
        let exiting = !self.open;
        let debug = format!("{}-popup", self.debug);
        let popup = div()
            .id("popup")
            // The trigger's own width, so the list reads as the trigger
            // opened.
            .w(theme.geometry.settings.choice_width)
            .flex()
            .flex_col()
            // The popup takes the clicks that land on it: a click on a
            // choice (or on a disabled one, or the empty state) is the
            // popup's own, never the page's rows underneath — the same
            // discipline the footer menu's overlay keeps, from the
            // other side (its outside dismissal). While the exit runs
            // this is what keeps the fading overlay inert: clicks on it
            // cannot invoke anything underneath, and the frame that
            // completes the exit unmounts it, so no invisible overlay
            // survives to intercept one.
            .occlude()
            // The exit's visuals are not in the accessibility tree: a
            // closed popup has nothing active to announce.
            .when(exiting, |popup| popup.aria_hidden());
        // The interactive popup: the open popup's key context over the
        // window's own keys, its handlers — the arrows, Enter, Escape
        // and Tab — and the outside dismissal that cancels the draft
        // while leaving the click to land. An exit carries none of
        // them: the fading visuals expose nothing active, exactly as
        // the footer menu's exit does for its list.
        let popup = if exiting {
            popup
        } else {
            popup
                .key_context(POPUP)
                .on_action(cx.listener(Self::next_choice))
                .on_action(cx.listener(Self::previous_choice))
                .on_action(cx.listener(Self::first_choice))
                .on_action(cx.listener(Self::last_choice))
                .on_action(cx.listener(Self::commit_action))
                .on_action(cx.listener(Self::cancel_action))
                .on_action(cx.listener(Self::leave_forward))
                .on_action(cx.listener(Self::leave_backward))
                // A mouse-down outside the popup cancels the draft and
                // is left to land, so the clicked target keeps its
                // focus (see [`Select::outside_down`]). The trigger's
                // own press was already settled in the capture phase
                // before this.
                .on_mouse_down_out(cx.listener(Self::outside_down))
        }
        .child(
            div()
                .relative()
                .top(px(offset))
                .when(opacity < 1., |wrapper| wrapper.opacity(opacity))
                // The popover's own outer shadows (`.pop`), as every
                // popover of Pane's carries them.
                .rounded(theme.geometry.popover_radius)
                .shadow(popover_shadows(theme))
                .debug_selector(move || debug.clone())
                .child(model.material.popover(theme, content)),
        );
        deferred(
            anchored()
                .anchor(gpui::Anchor::TopLeft)
                .offset(gpui::point(px(0.), POPUP_GAP))
                .snap_to_window_with_margin(POPUP_GAP)
                .child(popup),
        )
        .into_any_element()
    }

    /// One choice row: the Actions panel's entry family
    /// (`ui::controls::menu_row`, #99) carrying a list-box option's
    /// semantics. The committed choice is marked selected and carries the
    /// accent mark; the highlighted choice takes the entry's selected wash
    /// and is the focused field's active descendant; a choice the system
    /// cannot answer is listed with its reason and cannot be committed.
    /// Every wash changes at once, and every row carries its hover style
    /// whatever its state (see `ui::controls`).
    fn choice_row(
        &self,
        model: &Model,
        index: usize,
        active: Option<&SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = &model.theme;
        let choice = &model.choices[index];
        let committed = model.committed.as_ref() == Some(&choice.id);
        let highlighted = active.is_some_and(|id| id == &choice.id);
        let offered = choice.unavailable_reason.is_none();
        // What the row says under its label: the reason a choice cannot
        // be used here, where it cannot, else the choice's own subtitle.
        let description = choice
            .unavailable_reason
            .clone()
            .or_else(|| choice.subtitle.clone())
            .unwrap_or_default();
        let row_element = controls::menu_row(
            choice.id.clone(),
            choice.label.clone(),
            (!description.is_empty()).then(|| description.clone()),
            (highlighted, committed, offered),
            theme,
        );
        let id = choice.id.clone();
        let debug = format!("{}-{}", self.debug, choice.id);
        let label = choice.label.clone();
        let description_label = description.clone();
        let row_element = row_element
            .debug_selector(move || debug.clone())
            .role(Role::ListBoxOption)
            .aria_label(label)
            .aria_description(description_label)
            .aria_selected(committed)
            .when(highlighted, |row| row.aria_active_descendant())
            .when(!offered, |row| row.aria_disabled(true));
        let offered_row = if offered {
            let id = id.clone();
            row_element.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                // The exit's visuals are inert: a row of a closed popup
                // cannot commit, however visibly it is still fading.
                if this.open {
                    this.commit(&id, window, cx);
                }
            }))
        } else {
            row_element
        };
        offered_row.into_any_element()
    }
}

impl Render for Select {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = (self.model.clone())(cx);
        let query = self.query.read(cx).as_str().to_owned();
        let filtered = self.filtered(&model, &query);
        // The highlight self-heals: a choice that went (the consumer's
        // model changed, or the query moved under it) hands the
        // highlight back to the committed choice, else the first that
        // can be used — never a stale or unusable one.
        if self.active_choice(&model, &filtered).is_none() {
            self.active = model
                .committed
                .as_ref()
                .and_then(|id| {
                    filtered
                        .iter()
                        .copied()
                        .find(|&index| &model.choices[index].id == id)
                })
                .filter(|&index| model.choices[index].unavailable_reason.is_none())
                .map(|index| model.choices[index].id.clone())
                .or_else(|| first_enabled(&model, &query));
        }
        let active = self
            .active_choice(&model, &filtered)
            .map(|choice| choice.id.clone());
        let trigger = self.trigger_row(&model, cx);
        // The popup's entrance or exit, on the shared tween machinery:
        // only the control's own open state flipping starts or retargets
        // it — filtering and the model's changes are content updates,
        // which never animate — and the popup the element below paints
        // is open (settled or arriving), exiting, or nothing at all. See
        // [`crate::ui::motion`] for the family's rules.
        let popup_in_flight = crate::ui::motion::advance_popup(
            &mut self.transition,
            self.open,
            -crate::ui::motion::VIEW_SHIFT,
            self.drawn_open != self.open,
            cx.reduce_motion(),
            cx.background_executor().now(),
        );
        self.drawn_open = self.open;
        #[cfg(any(test, debug_assertions))]
        {
            self.drawn_popup = popup_in_flight;
        }
        // The boxed field's focus ring, read from the focus state the
        // frame draws with: the box itself tracks nothing (the popup's
        // content is the field's accessibility node, as root search's
        // wrapper is — see [`Select::popup`]).
        let field_focused = self.query.focus_handle(cx).is_focused(window);
        let popup = (self.open || popup_in_flight.is_some()).then(|| {
            self.popup(
                &model,
                &filtered,
                active.as_ref(),
                field_focused,
                popup_in_flight,
                cx,
            )
        });
        // While the popup's entrance or exit is still in flight, keep
        // frames coming; the frame that completes it requests none, so a
        // control whose popup is settled (open at rest, or closed and
        // unmounted) schedules no cosmetic frame at all.
        if popup_in_flight.is_some() {
            window.request_animation_frame();
        }
        // The control's block: the trigger, then a zero-height row that
        // positions the popup — its content-box origin is the trigger's
        // bottom-left, so the anchored popup opens below the trigger
        // however tall the trigger grew, and the popup is deferred from
        // there (painting above the window, following the page's
        // scroll).
        //
        // The block sits at a settings row's end, as wide as a row's
        // choice; the row around it names the setting and says what it
        // does.
        let width = model.theme.geometry.settings.choice_width;
        div()
            .relative()
            .flex_none()
            .flex()
            .flex_col()
            .w(width)
            .child(trigger)
            .child(
                div()
                    .flex_none()
                    .h(px(0.))
                    .w_full()
                    .when_some(popup, |anchor, popup| anchor.child(popup)),
            )
    }
}

/// The first choice that matches `query` and can be used, as its id.
fn first_enabled(model: &Model, query: &str) -> Option<SharedString> {
    model
        .choices
        .iter()
        .find(|choice| choice.unavailable_reason.is_none() && Select::matches(choice, query))
        .map(|choice| choice.id.clone())
}
