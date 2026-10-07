//! Clipboard History in the split view (#102): the launcher window's
//! adapter between the core's read-only projection
//! ([`pane_core::Launcher::clipboard_history`]) and the split view's
//! components ([`crate::ui::split_view`]).
//!
//! The view shows only while Pane's registered Clipboard History command
//! is open on its own list; every other command, a similarly titled one
//! included, keeps the generic list. The adapter owns what the user does
//! in the view — the query, the tab and the selected record
//! ([`ClipboardBrowse`]) — and reads the records anew each frame, so a
//! record deleted or expired is gone from the list at once and a stale
//! selection falls back to the first record listed.
//!
//! - Typing searches the records' text and source; the tabs are All and
//!   Text, the kinds Pane keeps.
//! - Up and Down move the selection and keep it in view; a click selects
//!   (it never copies).
//! - Enter, or the footer's Paste, pastes the selected record into the
//!   application that was in front, closing the window (#150); where Pane
//!   cannot paste yet, it copies the record instead and a HUD says so.
//!   Ctrl+Enter, or the footer's Copy, copies it again, closing the window
//!   with a "Copied to Clipboard" HUD as every Copy action does; Ctrl+D (as
//!   Explorer deletes), or the footer's Delete, deletes it — never Delete
//!   alone, which edits the search. Copy and Delete are the history's
//!   existing operations; the core revalidates all three first.
//! - The header's button turns capture on, pauses or resumes it, and says
//!   which is in force; the caption under it says what is kept, as it is.
//! - Ctrl+K (the Open actions binding), or the footer's Manage, routes to
//!   the command's own list: retention, exclusions, clearing, turning off
//!   and deleting, as they always were. Escape comes back.
//! - Escape clears the query, then leaves for root search; the back
//!   button leaves at once.
//!
//! The launcher window takes the view's 940×600 while it shows, and its
//! own size again once it leaves. Where the window is smaller the view
//! adapts (see [`crate::ui::split_view`]).

use std::time::Duration;

use gpui::{
    App, ClickEvent, Context, Div, Entity, EntityInputHandler, Focusable, KeyBinding, Role,
    ScrollHandle, SharedString, Subscription, Task, Toggled, Window, actions, div, prelude::*, px,
};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged};
use pane_core::clipboard::CaptureState;
use pane_core::clipboard_view::{
    ClipboardBrowse, ClipboardFilter, ClipboardHistoryView, copied_line, local_offset_ms,
    time_label,
};
use pane_core::{Binding, Keyboard, KeyboardAction, LauncherView, Screen, Status};

use crate::app::{KEY_CONTEXT, LauncherWindow};
use crate::ui::footer::{self, ButtonWash};
use crate::ui::icon::Glyph;
use crate::ui::input::TextEditingKeys;
use crate::ui::keycap::{CapStyle, KeySequence};
use crate::ui::shell::{self, SectionLabel};
use crate::ui::split_view::{self, ClipRow};
use crate::{Back, Confirm, OpenActions, SelectNext, SelectPrevious};

actions!(clipboard_history, [DeleteRecord, CopyRecord]);

/// The split view's key context, under the launcher's.
pub(crate) const CONTEXT: &str = "ClipboardHistory";

/// The keys that delete the selected record: explicit, and not a text
/// edit in the search field (Delete and Shift+Delete edit text there).
pub(crate) const DELETE_BINDING: &str = "ctrl-d";

/// Registers the view's keys: the previous and next result bindings in
/// its search field, above the field's own caret keys (as root search's
/// field has them), and Ctrl+D and the secondary action's chord (Copy,
/// Ctrl+Enter) in the view.
pub(crate) fn bind_keys(cx: &mut App, _: &TextEditingKeys, keyboard: &Keyboard) {
    let field = format!("{CONTEXT} > {DEFAULT_INPUT_CONTEXT}");
    cx.bind_keys([
        KeyBinding::new(
            &keyboard.binding(KeyboardAction::NextResult).id(),
            SelectNext,
            Some(&field),
        ),
        KeyBinding::new(
            &keyboard.binding(KeyboardAction::PreviousResult).id(),
            SelectPrevious,
            Some(&field),
        ),
        KeyBinding::new(DELETE_BINDING, DeleteRecord, Some(CONTEXT)),
        KeyBinding::new(&copy_binding().id(), CopyRecord, Some(CONTEXT)),
    ]);
}

/// The keys that copy the selected record: the secondary action's chord,
/// as an item's second action has it (Ctrl+Enter).
fn copy_binding() -> Binding {
    pane_core::keyboard::action_key(1).expect("the secondary action has a chord")
}

/// The keys [`copy_binding`] shows as.
pub(crate) fn copy_keys() -> KeySequence {
    crate::keyboard::binding_keys(&copy_binding())
}

/// The keys [`DELETE_BINDING`] shows as.
pub(crate) fn delete_keys() -> KeySequence {
    crate::keyboard::binding_keys(&Binding::parse(DELETE_BINDING).expect("ctrl-d binds"))
}

/// What the view lists in place of records when it lists none: why, as
/// it actually is — the history cannot be read (`unreadable`), the query
/// or tab keeps none of the records there are (`kept`), or nothing is
/// kept, for the reason `capture` gives.
pub(crate) fn empty_note(unreadable: Option<&str>, kept: bool, capture: CaptureState) -> String {
    if let Some(reason) = unreadable {
        return reason.to_owned();
    }
    if kept {
        return "No items match. Try another search or filter.".into();
    }
    match capture {
        CaptureState::Off => {
            "Clipboard history is off. Turn it on to keep the text you copy on this computer."
                .into()
        }
        CaptureState::Paused => "Nothing is kept. History is paused until you resume it.".into(),
        CaptureState::On => "Nothing kept yet. Text you copy from now on is listed here.".into(),
    }
}

/// The capture button for `capture`: what pressing it does, its glyph,
/// and whether it shows pressed (paused).
pub(crate) fn capture_control(capture: CaptureState) -> (&'static str, Glyph, bool) {
    match capture {
        CaptureState::On => ("Pause", Glyph::Pause, false),
        CaptureState::Paused => ("Resume", Glyph::ActionRun, true),
        CaptureState::Off => ("Turn on", Glyph::ActionRun, false),
    }
}

/// The search field's placeholder over `count` records.
pub(crate) fn placeholder(count: usize) -> String {
    match count {
        1 => "Search 1 item…".into(),
        count => format!("Search {count} items…"),
    }
}

/// The split view's state, owned by the launcher window while Pane's
/// Clipboard History command is open.
pub(crate) struct ClipboardHistory {
    browse: ClipboardBrowse,
    query: Entity<EditableTextState>,
    /// Whether the command's own list — its management controls — shows
    /// in place of the split view.
    managing: bool,
    list_scroll: ScrollHandle,
    /// Whether the next frame scrolls the list to the selected record.
    reveal: bool,
    /// Whether the footer shows the outcome of the last operation this
    /// view ran (the launcher's status), rather than when the selected
    /// record was copied: until the user moves on — selects, types or
    /// changes the tab. (Copying a record again while history is on keeps
    /// the copy as the newest record, so the outcome is not tied to an
    /// id.)
    outcome: bool,
    _typing: Subscription,
    /// Redraws the view when the history changes behind it (see
    /// [`REFRESH`]); dropped, and so stopped, with the view.
    _watching: Task<()>,
}

/// How often the open view looks at the history for what changed behind
/// it: a copy kept, a record expired, capture changed from elsewhere. The
/// history tells the window nothing itself, and a stale list or preview
/// must not stay on screen (a stale selection never acts: the core
/// revalidates every operation).
const REFRESH: Duration = Duration::from_secs(1);

/// What of the history the view shows that can change behind it: how many
/// records, the newest and the oldest, the capture and whether it reads.
type Fingerprint = (usize, Option<String>, Option<String>, CaptureState, bool);

fn fingerprint(view: &ClipboardHistoryView) -> Fingerprint {
    (
        view.records.len(),
        view.records.first().map(|record| record.id.clone()),
        view.records.last().map(|record| record.id.clone()),
        view.capture,
        view.unreadable.is_some(),
    )
}

impl ClipboardHistory {
    fn new(cx: &mut Context<LauncherWindow>) -> ClipboardHistory {
        let query = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        query.focus_handle(cx).tab_stop(true);
        let typing = cx.subscribe(&query, |this, input, _: &TextChanged, cx| {
            let text = input.read(cx).as_str().to_owned();
            if let Some(history) = this.clipboard.as_mut()
                && history.browse.query != text
            {
                history.browse.query = text;
                history.moved();
                cx.notify();
            }
        });
        let watching = cx.spawn(async move |this, cx| {
            let mut seen: Option<Fingerprint> = None;
            loop {
                cx.background_executor().timer(REFRESH).await;
                let open = this.update(cx, |this, cx| {
                    let now = this
                        .launcher
                        .clipboard_history()
                        .map(|view| fingerprint(&view));
                    if now != seen {
                        seen = now;
                        cx.notify();
                    }
                });
                if open.is_err() {
                    break;
                }
            }
        });
        ClipboardHistory {
            browse: ClipboardBrowse::default(),
            query,
            managing: false,
            list_scroll: ScrollHandle::new(),
            reveal: false,
            outcome: false,
            _typing: typing,
            _watching: watching,
        }
    }

    /// The user moved on: the list keeps the selection in view, and the
    /// footer says when the selected record was copied again.
    fn moved(&mut self) {
        self.reveal = true;
        self.outcome = false;
    }
}

impl LauncherWindow {
    /// Test support: whether the split view shows (rather than the
    /// command's own list, or another screen).
    #[doc(hidden)]
    pub fn clipboard_split_shown(&self) -> bool {
        self.clipboard
            .as_ref()
            .is_some_and(|history| !history.managing)
    }

    /// Test support: the split view's search field.
    #[doc(hidden)]
    pub fn clipboard_query(&self) -> Option<Entity<EditableTextState>> {
        self.clipboard.as_ref().map(|history| history.query.clone())
    }

    /// Makes the split view follow the launcher's screen: it opens, with
    /// its search focused and the window at the view's size, when the
    /// launcher shows Pane's Clipboard History command, and closes, giving
    /// the window back its own size, once the launcher leaves the command
    /// (a form or custom view opened from its list keeps it).
    pub(crate) fn sync_clipboard_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.launcher.clipboard_history().is_some() {
            if self.clipboard.is_none() {
                let history = ClipboardHistory::new(cx);
                window.focus(&history.query.focus_handle(cx), cx);
                self.clipboard = Some(history);
                self.fit_window(split_view::SPLIT_CLIENT, window, cx);
            }
            return;
        }
        let kept = matches!(
            self.launcher.view().screen,
            Screen::Form(_) | Screen::CustomView(_)
        );
        if !kept && self.clipboard.take().is_some() {
            self.fit_window(shell::LAUNCHER_CLIENT, window, cx);
        }
    }

    /// Resizes the window's client to `size` and places it as the
    /// launcher's placement does, unless it is that size already.
    fn fit_window(
        &mut self,
        (width, height): (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let size = gpui::size(px(width), px(height));
        if window.viewport_size() == size {
            return;
        }
        window.resize(size);
        self.place_sized(size, window, cx);
    }

    /// Leaves the command's own list for the split view, if it shows:
    /// whether it did. Escape takes this before leaving the command.
    pub(crate) fn leave_clipboard_controls(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(history) = self.clipboard.as_mut().filter(|history| history.managing) else {
            return false;
        };
        history.managing = false;
        let field = history.query.focus_handle(cx);
        window.focus(&field, cx);
        cx.notify();
        true
    }

    /// The command's own list in place of the split view: its management
    /// controls, as they always were.
    fn open_clipboard_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(history) = self.clipboard.as_mut() {
            history.managing = true;
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    fn clipboard_manage(&mut self, _: &OpenActions, window: &mut Window, cx: &mut Context<Self>) {
        self.open_clipboard_controls(window, cx);
    }

    fn clipboard_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.step_clipboard(1, cx);
    }

    fn clipboard_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.step_clipboard(-1, cx);
    }

    /// Moves the selection `delta` records, keeping it in view.
    fn step_clipboard(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(view) = self.launcher.clipboard_history() else {
            return;
        };
        if let Some(history) = self.clipboard.as_mut() {
            history.browse.step(&view.records, delta);
            history.moved();
            cx.notify();
        }
    }

    fn clipboard_confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        self.paste_selected_record(window, cx);
    }

    fn clipboard_copy(&mut self, _: &CopyRecord, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selected_record(cx);
    }

    fn clipboard_delete(&mut self, _: &DeleteRecord, _: &mut Window, cx: &mut Context<Self>) {
        self.delete_selected_record(cx);
    }

    /// Escape in the split view: an IME composition in the search field is
    /// cancelled first, then a query is cleared, and only then does the
    /// launcher's Back leave the command.
    fn clipboard_back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        let Some(query) = self.clipboard.as_ref().map(|history| history.query.clone()) else {
            self.back(&Back, window, cx);
            return;
        };
        let marked = query.update(cx, |input, cx| input.marked_text_range(window, cx));
        if let Some(marked) = marked {
            query.update(cx, |input, cx| {
                input.replace_text_in_range(Some(marked), "", window, cx)
            });
            return;
        }
        if !query.read(cx).as_str().is_empty() {
            query.update(cx, |input, cx| input.emplace("", cx));
            return;
        }
        self.back(&Back, window, cx);
    }

    /// The selected record and the reading it is listed in, if one is.
    fn selected_record(&self) -> Option<(ClipboardHistoryView, String)> {
        let view = self.launcher.clipboard_history()?;
        let history = self.clipboard.as_ref()?;
        // Which record is selected does not depend on the local day, so
        // the listing is made without the time zone's offset.
        let id = history
            .browse
            .listing(&view.records, view.now, 0)
            .selected_record()?
            .id
            .clone();
        Some((view, id))
    }

    /// Pastes the selected record into the application that was in front,
    /// through the core's revalidated operation, which closes the window
    /// (or copies it, saying so in a HUD, where Pane cannot paste yet);
    /// nothing with none selected.
    pub(crate) fn paste_selected_record(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((view, id)) = self.selected_record() {
            let pending = self.launcher.paste_clipboard_record(&view, &id);
            self.note_outcome(cx);
            self.show_until_done(pending, window, cx);
        }
    }

    /// Copies the selected record again, through the core's revalidated
    /// operation, which closes the window and says so in a HUD; nothing
    /// with none selected.
    pub(crate) fn copy_selected_record(&mut self, cx: &mut Context<Self>) {
        if let Some((view, id)) = self.selected_record() {
            self.launcher.copy_clipboard_record(&view, &id).ok();
            self.note_outcome(cx);
        }
    }

    /// Deletes the selected record, through the core's revalidated
    /// operation; nothing with none selected. The first record listed is
    /// selected next.
    pub(crate) fn delete_selected_record(&mut self, cx: &mut Context<Self>) {
        if let Some((view, id)) = self.selected_record() {
            self.launcher.delete_clipboard_record(&view, &id).ok();
            // The record selected next comes into view.
            if let Some(history) = self.clipboard.as_mut() {
                history.reveal = true;
            }
            self.note_outcome(cx);
        }
    }

    /// Turns capture on, pauses or resumes it, as the capture button says.
    fn toggle_clipboard_capture(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.launcher.clipboard_history() else {
            return;
        };
        let next = match view.capture {
            CaptureState::On => CaptureState::Paused,
            CaptureState::Paused | CaptureState::Off => CaptureState::On,
        };
        self.launcher.set_clipboard_capture(&view, next).ok();
        self.note_outcome(cx);
    }

    /// Shows the outcome of the operation just run in the footer, until
    /// the user moves on.
    fn note_outcome(&mut self, cx: &mut Context<Self>) {
        if let Some(history) = self.clipboard.as_mut() {
            history.outcome = true;
        }
        cx.notify();
    }

    /// The split view, when it shows (see [`LauncherWindow::clipboard_split_shown`]),
    /// for the launcher's frame: the launcher's keys bound as everywhere,
    /// the view's own on top. `view` is the launcher's view this frame
    /// draws.
    pub(crate) fn render_clipboard_history(
        &mut self,
        view: &LauncherView,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if !self.clipboard_split_shown() {
            return None;
        }
        let history = self.launcher.clipboard_history()?;
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = visuals.theme;
        let now = history.now;
        let offset = local_offset_ms(now);
        let state = self.clipboard.as_mut()?;
        let listing = state.browse.listing(&history.records, now, offset);
        let labels: Vec<SectionLabel> = listing
            .sections
            .iter()
            .map(|section| SectionLabel {
                first: section.first,
                label: section.day.label().into(),
                note: None,
            })
            .collect();
        if state.reveal {
            if let Some(selected) = listing.selected {
                state
                    .list_scroll
                    .scroll_to_item(shell::child_of_row(&labels, selected));
            }
            state.reveal = false;
        }
        let selected = listing.selected_record();
        let show_outcome = state.outcome;
        let filter = state.browse.filter;
        let query = state.query.clone();
        let list_scroll = state.list_scroll.clone();

        // The header: back, the command's chip, the search, the capture —
        // which is offered only while the history can be read, since its
        // label says the state in force.
        let (capture_label, capture_glyph, paused) = capture_control(history.capture);
        let capture = history.unreadable.is_none().then(|| {
            split_view::capture_button(capture_label, capture_glyph, paused, &theme)
                .role(Role::Button)
                .aria_label(capture_label)
                .aria_toggled(if paused {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.toggle_clipboard_capture(cx);
                }))
                .into_any_element()
        });
        let header = split_view::header(
            split_view::back_button(&theme)
                .id("clipboard-back")
                .debug_selector(|| "clipboard-back".into())
                .role(Role::Button)
                .aria_label("Back to all results")
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.back(&Back, window, cx);
                }))
                .into_any_element(),
            split_view::chip(history.title.clone(), &theme),
            split_view::search_field(&query, placeholder(history.records.len()), &theme)
                .into_any_element(),
            capture,
            &theme,
        );

        // The tabs, and what is kept, as it is.
        let tabs = ClipboardFilter::ALL
            .into_iter()
            .map(|choice| {
                let on = choice == filter;
                let tab_id = SharedString::from(format!("clipboard-tab-{}", choice.label()));
                split_view::tab(tab_id, choice.label(), on, &theme)
                    .debug_selector(move || format!("clipboard-tab-{}", choice.label()))
                    .role(Role::Tab)
                    .aria_selected(on)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if let Some(history) = this.clipboard.as_mut() {
                            history.browse.filter = choice;
                            history.moved();
                            cx.notify();
                        }
                    }))
                    .into_any_element()
            })
            .collect();
        // What is kept, as it is; or why it cannot be told.
        let (caption, caption_color) = match &history.unreadable {
            Some(reason) => (reason.clone(), theme.warning),
            None if history.problem.is_some() => (history.summary(), theme.warning),
            None => (history.summary(), theme.text_muted),
        };
        let tabs = split_view::tabs(tabs, Some((caption.into(), caption_color)), &theme);

        // The list: the day sections and their records, or why none.
        let rows = listing.records.iter().enumerate().map(|(index, record)| {
            let on = listing.selected == Some(index);
            let id = record.id.clone();
            let title = record.title().to_owned();
            split_view::clip_row(
                ("clip", index),
                ClipRow {
                    title: title.clone().into(),
                    time: time_label(record.copied_at, now, offset).into(),
                    selected: on,
                    glyph: Glyph::Lines,
                },
                &theme,
            )
            .debug_selector(move || format!("clip-{title}"))
            .role(Role::ListBoxOption)
            .aria_label(record.title().to_owned())
            .aria_selected(on)
            .when(on, |row| row.aria_active_descendant())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if let Some(history) = this.clipboard.as_mut() {
                    history.browse.select(id.clone());
                    history.outcome = false;
                    cx.notify();
                }
            }))
            .into_any_element()
        });
        let rows: Vec<gpui::AnyElement> = rows.collect();
        let list = split_view::list(&theme)
            .role(Role::ListBox)
            .aria_label("Clipboard history")
            .track_scroll(&list_scroll);
        let list = if listing.records.is_empty() {
            let note = empty_note(
                history.unreadable.as_deref(),
                !history.records.is_empty(),
                history.capture,
            );
            list.child(split_view::empty_note(note, &theme))
        } else {
            list.children(shell::with_section_labels(rows, &labels, &theme))
        };

        // The preview: the selected record's text, as it was copied.
        let preview = selected.map(|record| {
            split_view::preview_card(
                SharedString::from(format!("clipboard-preview-{}", record.id)).into(),
                split_view::text_preview(record.text.clone(), &theme).into_any_element(),
                &theme,
            )
            .into_any_element()
        });

        // The footer: when and where the selected record was copied (or
        // the outcome of what was just done to it), then Delete, Copy and
        // Manage.
        // A toast the command showed speaks where the outcome would (#141).
        let toast = self.footer_toast(&view.status).map(|shown| {
            let (selector, color) = super::toast::style_look(shown.toast.style, &theme);
            (selector, shown.toast.text(), color)
        });
        let status = match &view.status {
            _ if toast.is_some() => toast,
            Status::Idle => None,
            Status::Running => Some(("status-running", "Running…".to_owned(), theme.warning)),
            Status::Progress(work) => Some(("status-progress", work.clone(), theme.warning)),
            Status::Result(answer) => Some(("status-result", answer.clone(), theme.success)),
            Status::Error(message) => Some(("status-error", message.clone(), theme.danger)),
        }
        .filter(|_| show_outcome);
        let (selector, lead) = match &status {
            Some((selector, text, color)) => (
                *selector,
                split_view::footer_lead(text.clone(), *color, &theme),
            ),
            None => (
                "status-idle",
                split_view::footer_lead(
                    selected.map_or_else(
                        || "Nothing selected".to_owned(),
                        |record| copied_line(record, now, offset),
                    ),
                    theme.text_muted,
                    &theme,
                ),
            ),
        };
        let keyboard = crate::settings::keyboard_of(cx);
        let invoke =
            crate::keyboard::binding_keys(keyboard.binding(KeyboardAction::InvokeSelectedAction));
        let manage_keys =
            crate::keyboard::binding_keys(keyboard.binding(KeyboardAction::OpenActions));
        // Paste (Enter), Copy and Delete act on the selected record: with
        // none, there is no primary action at all.
        let copyable = history.copy_unavailable.is_none();
        let paste = selected.map(|_| {
            footer::footer_button(
                "clipboard-paste",
                "Paste",
                &invoke,
                CapStyle::Accent,
                ButtonWash::Hover,
                &theme,
            )
            .role(Role::Button)
            .aria_label("Paste")
            .aria_keyshortcuts(invoke.name())
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.paste_selected_record(window, cx);
            }))
            .into_any_element()
        });
        let copy_caps = copy_keys();
        let copy = selected.map(|_| {
            footer::footer_button(
                "clipboard-copy",
                "Copy",
                &copy_caps,
                CapStyle::Regular,
                ButtonWash::Hover,
                &theme,
            )
            .role(Role::Button)
            .aria_label("Copy")
            .aria_keyshortcuts(copy_caps.name())
            // Where this Pane cannot write the clipboard the button is
            // dimmed and inert, as the launcher's own unavailable primary
            // action is; Enter still asks, and the core says why not.
            .when(copyable, |button| {
                button
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.copy_selected_record(cx);
                    }))
            })
            .when(!copyable, |button| {
                button.opacity(0.5).cursor_default().aria_disabled(true)
            })
            .into_any_element()
        });
        let delete_caps = delete_keys();
        let delete = selected.map(|_| {
            footer::footer_button(
                "clipboard-delete",
                "Delete",
                &delete_caps,
                CapStyle::Regular,
                ButtonWash::Hover,
                &theme,
            )
            .role(Role::Button)
            .aria_label("Delete")
            .aria_keyshortcuts(delete_caps.name())
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.delete_selected_record(cx);
            }))
            .into_any_element()
        });
        let manage = footer::footer_button(
            "clipboard-manage",
            "Manage",
            &manage_keys,
            CapStyle::Regular,
            ButtonWash::Hover,
            &theme,
        )
        .role(Role::Button)
        .aria_label("Manage clipboard history")
        .aria_keyshortcuts(manage_keys.name())
        .cursor_pointer()
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            this.open_clipboard_controls(window, cx);
        }))
        .into_any_element();
        let buttons = split_view::footer_buttons(
            paste,
            copy.into_iter().chain(delete).collect(),
            manage,
            &theme,
        );
        let footer = split_view::footer(lead, buttons, &theme)
            .id("status")
            .role(Role::Status)
            .when_some(status.map(|(_, text, _)| text), |footer, text| {
                footer.aria_label(text)
            })
            .debug_selector(move || selector.into());

        let content = split_view::compose(
            header,
            tabs,
            list.into_any_element(),
            preview,
            footer.into_any_element(),
            &theme,
        )
        .key_context(CONTEXT)
        .on_action(cx.listener(Self::clipboard_delete))
        .on_action(cx.listener(Self::clipboard_copy));
        let root = div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::clipboard_next))
            .on_action(cx.listener(Self::clipboard_previous))
            .on_action(cx.listener(Self::clipboard_confirm))
            .on_action(cx.listener(Self::clipboard_back))
            .on_action(cx.listener(Self::clipboard_manage))
            .on_action(cx.listener(Self::return_to_root))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .size_full()
            .flex()
            .flex_col()
            .font_family(theme.typography.family.clone())
            .font_features(theme.typography.features.clone())
            .text_color(theme.text_title)
            .child(content);
        Some(visuals.material.panel(&theme, root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_controls_say_what_they_do() {
        assert_eq!(capture_control(CaptureState::On).0, "Pause");
        assert_eq!(
            capture_control(CaptureState::Paused),
            ("Resume", Glyph::ActionRun, true)
        );
        assert_eq!(capture_control(CaptureState::Off).0, "Turn on");
        assert_eq!(placeholder(1), "Search 1 item…");
        assert_eq!(placeholder(3), "Search 3 items…");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn delete_shows_its_windows_keys() {
        assert_eq!(delete_keys().name(), "Ctrl+D");
    }
}
