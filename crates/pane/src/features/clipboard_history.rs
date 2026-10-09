//! Clipboard History in the split view (#102, #166): the launcher window's
//! adapter between the core's read-only projection
//! ([`pane_core::Launcher::clipboard_history`]) and the split view's
//! components ([`crate::ui::split_view`]).
//!
//! The view shows only while Pane's registered Clipboard History command
//! is open on its own list; every other command, a similarly titled one
//! included, keeps the generic list. The adapter owns what the user does
//! in the view — the query, the type chosen and the selected record
//! ([`ClipboardBrowse`]) — and reads the records each frame, so a record
//! deleted or expired is gone from the list at once and a stale selection
//! falls back to the first record listed. The records are shared, made
//! again only when the history changed (#192), and the list's frame is
//! made again only when they, the view's own state or the minute changed:
//! a frame drawn with nothing changed copies nothing.
//!
//! - Typing filters the records by their text and source ("Type to filter
//!   entries…"); the type dropdown at the search field's right keeps All
//!   Types, Text, Images, Files, Links or Colors. The search field carries
//!   no badge: the footer names the command by its icon and title (#162).
//! - The records are grouped by local day: Today, Yesterday, then dates.
//!   A copied image's row shows its thumbnail, titled "Image (W×H)"; a
//!   files row the first file's system icon, titled by its name ("+N" for
//!   more) (#167).
//! - The detail pane previews the selected record — its text, its image,
//!   or its files with their icons — over its Information: Source (the
//!   application's name and, where Pane knows its path, its icon), Type,
//!   Characters (text) or Dimensions (an image), and Copied.
//! - Up and Down move the selection and keep it in view; a click selects
//!   (it never copies).
//! - Enter, or the footer's Paste, pastes the selected record into the
//!   application that was in front, closing the window (#150); where Pane
//!   cannot paste yet, it copies the record instead and a HUD says so.
//!   Ctrl+Enter copies it again, closing the window with a "Copied to
//!   Clipboard" HUD as every Copy action does; Ctrl+D (as Explorer
//!   deletes) deletes it — never Delete alone, which edits the search.
//! - Ctrl+K (the Open actions binding), or the footer's Actions, opens the
//!   Actions panel over the view: the record's Paste, Copy and Delete, then
//!   Pause or Resume Recording, Clear History… (which asks first), Keep
//!   History For, and Disabled Applications…, which opens the extension's
//!   page in Settings ([`pane_core::clipboard_view::ClipboardHistoryView::actions`]).
//!   The core revalidates every one of them.
//! - Escape clears the query, then leaves for root search; the back
//!   button leaves at once.
//!
//! The launcher window takes the view's 940×600 while it shows, and its
//! own size again once it leaves. Where the window is smaller the view
//! adapts (see [`crate::ui::split_view`]).

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, Entity, EntityInputHandler, Focusable,
    KeyBinding, Role, SharedString, Subscription, Task, Window, actions, div, prelude::*, px,
};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged};
use pane_core::clipboard::CaptureState;
use pane_core::clipboard_view::{
    ClipboardAction, ClipboardBrowse, ClipboardFilter, ClipboardHistoryView, ClipboardKind,
    ClipboardRecord, file_name, information, local_offset_ms, time_label,
};
use pane_core::{Binding, Keyboard, KeyboardAction, LauncherView, Screen, Status};

use crate::app::{KEY_CONTEXT, LauncherWindow, Spot};
use crate::features::announcer::{self, Listing, Noun, Opening, Selected, Target};
use crate::ui::extension_icon::{self, IconSize};
use crate::ui::footer::{self, ButtonWash};
use crate::ui::icon::{Glyph, IconTone, TileSize};
use crate::ui::input::TextEditingKeys;
use crate::ui::keycap::{CapStyle, KeySequence};
use crate::ui::select::{Choice, Model, Select};
use crate::ui::shell::{self, SectionLabel};
use crate::ui::split_view::{self, ClipRow, InfoRow};
use crate::ui::theme::Theme;
use crate::ui::virtual_list::{self, ListChild, VirtualList};
use crate::{
    Back, Confirm, OpenActions, SelectNext, SelectNextPage, SelectPrevious, SelectPreviousPage,
};

actions!(clipboard_history, [DeleteRecord, CopyRecord]);

/// The split view's key context, under the launcher's.
pub(crate) const CONTEXT: &str = "ClipboardHistory";

/// The keys that delete the selected record: explicit, and not a text
/// edit in the search field (Delete and Shift+Delete edit text there).
pub(crate) const DELETE_BINDING: &str = "ctrl-d";

/// The search field's placeholder, Raycast's.
pub(crate) const PLACEHOLDER: &str = "Type to filter entries…";

/// The search field's accessible name.
pub(crate) const SEARCH_LABEL: &str = "Search clipboard history";

/// The type dropdown's debug selector: its trigger is this, a choice's row
/// `clipboard-type-<id>` (`all`, `text`, `images`, `files`, `links`,
/// `colors`).
pub(crate) const TYPE_SELECT: &str = "clipboard-type";

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
/// or type keeps none of the records there are (`kept`), or nothing is
/// kept, for the reason `capture` gives.
pub(crate) fn empty_note(unreadable: Option<&str>, kept: bool, capture: CaptureState) -> String {
    if let Some(reason) = unreadable {
        return reason.to_owned();
    }
    if kept {
        return "No entries match. Try another search or type.".into();
    }
    match capture {
        CaptureState::Off | CaptureState::Paused => {
            "Nothing is kept. Recording is paused until you resume it (Ctrl+K).".into()
        }
        CaptureState::On => "Nothing copied yet. What you copy from now on is listed here.".into(),
    }
}

/// The glyph of a record of `kind` on its row's tile. An image's row shows
/// its thumbnail and a files row the first file's icon instead; their
/// glyphs stand in only where those are not known.
fn kind_glyph(kind: ClipboardKind) -> Glyph {
    match kind {
        ClipboardKind::Text => Glyph::Lines,
        ClipboardKind::Link => Glyph::ArrowRight,
        ClipboardKind::Color => Glyph::Sliders,
        ClipboardKind::Image => Glyph::Monitor,
        ClipboardKind::Files => Glyph::Folder,
    }
}

/// What a listed record's row shows before its title (#167).
#[derive(Clone, Debug, PartialEq, Eq)]
enum ClipMark {
    /// Its kind's glyph on the neutral tile.
    Kind(ClipboardKind),
    /// An image's thumbnail, from its PNG.
    Thumbnail(PathBuf),
    /// The system's icon of the first of its files.
    File(PathBuf),
}

impl ClipMark {
    fn of(record: &ClipboardRecord) -> ClipMark {
        if let Some(image) = &record.image {
            ClipMark::Thumbnail(image.path.clone())
        } else if let Some(file) = record.files.first() {
            ClipMark::File(file.clone())
        } else {
            ClipMark::Kind(record.kind)
        }
    }
}

/// The type dropdown's choices: the filters, in order.
fn type_choices() -> Vec<Choice> {
    ClipboardFilter::ALL
        .into_iter()
        .map(|filter| Choice {
            id: filter.id().into(),
            label: filter.label().into(),
            subtitle: None,
            keywords: Vec::new(),
            unavailable_reason: None,
        })
        .collect()
}

/// The split view's state, owned by the launcher window while Pane's
/// Clipboard History command is open.
pub(crate) struct ClipboardHistory {
    browse: ClipboardBrowse,
    query: Entity<EditableTextState>,
    /// The type chosen, shared with the dropdown's live model.
    chosen: Rc<Cell<ClipboardFilter>>,
    /// The type dropdown at the search field's right.
    types: Entity<Select>,
    /// The list, drawn virtually (#165): only the records in view are laid
    /// out and painted.
    list: VirtualList,
    /// What the list's children are drawn from, as the last frame read it.
    frame: Option<Rc<ClipFrame>>,
    /// Whether the next frame scrolls the list to the selected record.
    reveal: bool,
    /// Whether the footer shows the outcome of the last operation this
    /// view ran (the launcher's status), rather than the command's icon
    /// and title: until the user moves on — selects, types or changes the type.
    /// (Copying a record again while recording keeps the copy as the
    /// newest record, so the outcome is not tied to an id.)
    outcome: bool,
    _typing: Subscription,
    /// Redraws the view when the history changes behind it (see
    /// [`REFRESH`]); dropped, and so stopped, with the view.
    _watching: Task<()>,
}

/// What a frame draws in the split view's list: its children are drawn
/// from it as the list lays them out.
struct ClipFrame {
    /// What it was made from: drawn again over the same, it is kept.
    made_from: FrameKey,
    /// The records listed, in order.
    rows: Vec<ClipFrameRow>,
    /// Their days' labels.
    sections: Vec<SectionLabel>,
    /// The list's children: the day labels and the records.
    children: Vec<ListChild>,
    /// The selected record's index in `rows`.
    selected: Option<usize>,
}

/// What a list's frame was made from (#192): it is made again only once
/// one of these changed.
struct FrameKey {
    /// The records, shared by the launcher until the history changes.
    records: Arc<[ClipboardRecord]>,
    /// The query, the type and the record chosen.
    browse: ClipboardBrowse,
    /// The minute it was made in, since the Unix epoch: the rows tell the
    /// time and the day to the minute.
    minute: u64,
    /// The local time's offset from UTC, in milliseconds.
    offset: i64,
}

impl FrameKey {
    /// Whether a frame made from this is the frame of `records` under
    /// `browse` in `minute`, at `offset` from UTC.
    fn holds(
        &self,
        records: &Arc<[ClipboardRecord]>,
        browse: &ClipboardBrowse,
        minute: u64,
        offset: i64,
    ) -> bool {
        Arc::ptr_eq(&self.records, records)
            && self.browse == *browse
            && self.minute == minute
            && self.offset == offset
    }
}

/// A listed record as its row shows it.
struct ClipFrameRow {
    id: SharedString,
    title: SharedString,
    time: SharedString,
    mark: ClipMark,
}

/// How often the open view looks at the history for what changed behind
/// it: a copy kept, a record expired, recording changed from elsewhere
/// (Settings). The history tells the window nothing itself, and a stale
/// list or preview must not stay on screen (a stale selection never acts:
/// the core revalidates every operation). The launcher shares the same
/// records until the history changes (#192), so other records are another
/// history.
const REFRESH: Duration = Duration::from_secs(1);

impl ClipboardHistory {
    fn new(window: &mut Window, cx: &mut Context<LauncherWindow>) -> ClipboardHistory {
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
        let chosen = Rc::new(Cell::new(ClipboardFilter::All));
        let committed = chosen.clone();
        let commit_to = chosen.clone();
        let this = cx.entity().downgrade();
        let types = cx.new(|cx| {
            Select::new(
                "Type",
                "Filter by Type",
                TYPE_SELECT,
                Rc::new(move |cx: &App| {
                    let visuals = crate::settings::launcher_visuals(cx);
                    Model {
                        theme: visuals.theme,
                        material: visuals.material,
                        choices: type_choices(),
                        committed: Some(committed.get().id().into()),
                    }
                }),
                Rc::new(move |id: &str, _: &mut Window, cx: &mut App| {
                    let Some(filter) = ClipboardFilter::from_id(id) else {
                        return;
                    };
                    commit_to.set(filter);
                    this.update(cx, |this, cx| {
                        if let Some(history) = this.clipboard.as_mut() {
                            history.browse.filter = filter;
                            history.moved();
                        }
                        cx.notify();
                    })
                    .ok();
                }),
                window,
                cx,
            )
        });
        let watching = cx.spawn(async move |this, cx| {
            let mut seen: Option<Arc<[ClipboardRecord]>> = None;
            loop {
                cx.background_executor().timer(REFRESH).await;
                let open = this.update(cx, |this, cx| {
                    let now = this.launcher.clipboard_history().map(|view| view.records);
                    let same = match (&now, &seen) {
                        (Some(now), Some(seen)) => Arc::ptr_eq(now, seen),
                        (None, None) => true,
                        _ => false,
                    };
                    if !same {
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
            chosen,
            types,
            list: {
                let geometry = &crate::settings::launcher_visuals(cx).theme.geometry;
                VirtualList::new(geometry.row_min_height + geometry.row_list_gap)
            },
            frame: None,
            reveal: false,
            outcome: false,
            _typing: typing,
            _watching: watching,
        }
    }

    /// The user moved on: the list keeps the selection in view, and the
    /// footer shows the command again.
    fn moved(&mut self) {
        self.reveal = true;
        self.outcome = false;
    }
}

impl LauncherWindow {
    /// Test support: whether the split view shows (rather than another
    /// screen).
    #[doc(hidden)]
    pub fn clipboard_split_shown(&self) -> bool {
        self.clipboard.is_some()
    }

    /// Test support: the split view's search field.
    #[doc(hidden)]
    pub fn clipboard_query(&self) -> Option<Entity<EditableTextState>> {
        self.clipboard.as_ref().map(|history| history.query.clone())
    }

    /// Test support: the type the split view's dropdown keeps.
    #[doc(hidden)]
    pub fn clipboard_filter(&self) -> Option<ClipboardFilter> {
        self.clipboard.as_ref().map(|history| history.browse.filter)
    }

    /// Makes the split view follow the launcher's screen: it opens, with
    /// its search focused and the window at the view's size, when the
    /// launcher shows Pane's Clipboard History command, and closes, giving
    /// the window back its own size, once the launcher leaves the command
    /// (a form or custom view opened from it keeps it).
    pub(crate) fn sync_clipboard_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.launcher.clipboard_history().is_some() {
            if self.clipboard.is_none() {
                let history = ClipboardHistory::new(window, cx);
                window.focus(&history.query.focus_handle(cx), cx);
                self.clipboard = Some(history);
                self.fit_client(split_view::SPLIT_CLIENT, window, cx);
            }
            return;
        }
        let kept = matches!(
            self.launcher.screen(),
            Screen::Form(_) | Screen::CustomView(_)
        );
        if !kept && self.clipboard.take().is_some() {
            self.fit_client(shell::LAUNCHER_CLIENT, window, cx);
        }
    }

    /// The Open actions binding in the view: opens the Actions panel for
    /// the selected record and the history, or closes it.
    fn clipboard_actions(&mut self, _: &OpenActions, window: &mut Window, cx: &mut Context<Self>) {
        if self.actions_open() {
            self.close_actions(window, cx);
        } else {
            self.open_clipboard_panel(window, cx);
        }
    }

    /// Opens the Actions panel for the selected record (by id and title),
    /// or for the history with none selected.
    fn open_clipboard_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected_record().and_then(|(view, id)| {
            let title = view.record(&id)?.title().into_owned();
            Some((id, title))
        });
        self.open_clipboard_actions(selected, window, cx);
    }

    fn clipboard_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.step_clipboard(1, cx);
    }

    fn clipboard_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.step_clipboard(-1, cx);
    }

    /// Page Down: the selection moves by the records in view (#165).
    fn clipboard_next_page(&mut self, _: &SelectNextPage, _: &mut Window, cx: &mut Context<Self>) {
        let list = self.clipboard.as_ref().map(|history| &history.list);
        self.step_clipboard(virtual_list::page_move(list, true), cx);
    }

    /// Page Up: the selection moves back by the records in view.
    fn clipboard_previous_page(
        &mut self,
        _: &SelectPreviousPage,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let list = self.clipboard.as_ref().map(|history| &history.list);
        self.step_clipboard(virtual_list::page_move(list, false), cx);
    }

    /// Moves the selection `delta` records, keeping it in view.
    fn step_clipboard(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(view) = self.launcher.clipboard_history() else {
            return;
        };
        if let Some(history) = self.clipboard.as_mut() {
            history.browse.step(&view.records, delta);
            history.moved();
            self.announcer.user_moved();
            cx.notify();
        }
    }

    fn clipboard_confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        // As in the launcher's confirm handler, let the open panel take
        // Enter from the key press, once per press, instead of pasting.
        if self.actions_open() {
            cx.propagate();
            return;
        }
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
            self.paste_record(&view, &id, window, cx);
        }
    }

    fn paste_record(
        &mut self,
        view: &ClipboardHistoryView,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending = self.launcher.paste_clipboard_record(view, id);
        self.note_outcome(cx);
        self.show_until_done(pending, window, cx);
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
            self.delete_record(&view, &id, cx);
        }
    }

    fn delete_record(&mut self, view: &ClipboardHistoryView, id: &str, cx: &mut Context<Self>) {
        self.launcher.delete_clipboard_record(view, id).ok();
        // The record selected next comes into view.
        if let Some(history) = self.clipboard.as_mut() {
            history.reveal = true;
        }
        self.note_outcome(cx);
    }

    /// Runs `action`, chosen in the Actions panel, on the record `record`
    /// (by id) the panel opened for, and on the history: each through the
    /// core's revalidated operation.
    pub(crate) fn run_clipboard_action(
        &mut self,
        action: ClipboardAction,
        record: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.launcher.clipboard_history() else {
            return;
        };
        match (action, record) {
            (ClipboardAction::Paste, Some(id)) => self.paste_record(&view, &id, window, cx),
            (ClipboardAction::Copy, Some(id)) => {
                self.launcher.copy_clipboard_record(&view, &id).ok();
                self.note_outcome(cx);
            }
            (ClipboardAction::Delete, Some(id)) => self.delete_record(&view, &id, cx),
            (ClipboardAction::Paste | ClipboardAction::Copy | ClipboardAction::Delete, None) => {}
            (ClipboardAction::PauseRecording, _) => {
                self.launcher
                    .set_clipboard_capture(&view, CaptureState::Paused)
                    .ok();
                self.note_outcome(cx);
            }
            (ClipboardAction::ResumeRecording, _) => {
                self.launcher
                    .set_clipboard_capture(&view, CaptureState::On)
                    .ok();
                self.note_outcome(cx);
            }
            (ClipboardAction::KeepFor(seconds), _) => {
                self.launcher.set_clipboard_retention(&view, seconds).ok();
                self.note_outcome(cx);
            }
            // Asks first, over the view; the outcome shows once answered.
            (ClipboardAction::ClearHistory, _) => {
                let cleared = self.launcher.clear_clipboard_history(&view);
                self.note_outcome(cx);
                cx.spawn_in(window, async move |this, cx| {
                    cleared.await.ok();
                    this.update(cx, |this, cx| {
                        if let Some(history) = this.clipboard.as_mut() {
                            history.reveal = true;
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
            // The extension's page in Settings, where its preferences are.
            (ClipboardAction::DisabledApplications, _) => {
                crate::features::settings::open_at(
                    &self.launcher,
                    crate::features::settings::extensions::TITLE,
                    &view.owner.key(),
                    cx,
                );
            }
        }
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
        window: &mut Window,
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
        let minute = now / 60_000;
        let state = self.clipboard.as_mut()?;
        // The dropdown's choice, as its model reads it.
        state.chosen.set(state.browse.filter);
        // The list's frame: what its children are drawn from as it lays
        // them out (#165). It is kept while the records (shared until the
        // history changes, #192), the query, the type, the record chosen and
        // the minute stay the same, so a frame drawn again copies nothing.
        let kept = state.frame.clone().filter(|frame| {
            frame
                .made_from
                .holds(&history.records, &state.browse, minute, offset)
        });
        let (frame, changed) = match kept {
            Some(frame) => (frame, false),
            None => {
                let listing = state.browse.listing(&history.records, now, offset);
                let labels: Vec<SectionLabel> = listing
                    .sections
                    .iter()
                    .map(|section| SectionLabel {
                        first: section.first,
                        label: section.label.clone().into(),
                        note: None,
                    })
                    .collect();
                let frame = ClipFrame {
                    made_from: FrameKey {
                        records: history.records.clone(),
                        browse: state.browse.clone(),
                        minute,
                        offset,
                    },
                    rows: listing
                        .records
                        .iter()
                        .map(|record| ClipFrameRow {
                            id: record.id.as_str().into(),
                            title: record.title().into(),
                            time: time_label(record.copied_at, now, offset).into(),
                            mark: ClipMark::of(record),
                        })
                        .collect(),
                    children: virtual_list::children(false, listing.records.len(), &labels),
                    sections: labels,
                    selected: listing.selected,
                };
                // The list is measured again, from its top, only when the
                // records or their days changed, not as their times tick.
                let changed = state.frame.as_ref().is_none_or(|last| {
                    last.children != frame.children
                        || last.sections != frame.sections
                        || last.rows.len() != frame.rows.len()
                        || last.rows.iter().zip(&frame.rows).any(|(last, now)| {
                            last.id != now.id || last.title != now.title || last.mark != now.mark
                        })
                });
                (Rc::new(frame), changed)
            }
        };
        if changed || state.list.count() != frame.children.len() {
            state.list.reset(frame.children.len());
        }
        if state.reveal || changed {
            if let Some(selected) = frame.selected {
                state
                    .list
                    .reveal(virtual_list::child_of_row(false, &frame.sections, selected));
            }
            state.reveal = false;
        }
        // The list as the window's announcer follows it (#132): opening
        // with the command's title and count, its search the typing.
        let chosen = frame
            .selected
            .and_then(|index| Some((index, frame.rows.get(index)?)));
        let target = match chosen {
            Some((index, record)) => Target::Row(Selected {
                id: record.id.to_string(),
                title: record.title.to_string(),
                position: index + 1,
                unavailable: false,
                section: announcer::section_at(&frame.sections, index),
            }),
            None if frame.rows.is_empty() => Target::NoResults,
            None => Target::Nothing,
        };
        let followed = Listing {
            over: false,
            key: format!("clipboard {}", history.title),
            opening: Opening::Named(history.title.clone(), Noun::Results),
            count: frame.rows.len(),
            target,
            query: Some(state.browse.query.clone()),
            settled: true,
        };
        // The selected record, which the preview shows and Paste pastes.
        let selected = chosen.and_then(|(_, row)| history.record(&row.id));
        state.frame = Some(frame.clone());
        let show_outcome = state.outcome;
        let query = state.query.clone();
        let types = state.types.clone();
        let list_state = state.list.state().clone();

        // The header: back, the search — with no badge on it — and the type
        // dropdown at its right.
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
            // The field's accessibility node, as root search's: it tracks
            // the field's focus, which stays there while the selection
            // moves (#132).
            div()
                .id("clipboard-search")
                .flex_1()
                .min_w(px(0.))
                .track_focus(&query.focus_handle(cx))
                .role(Role::EditableComboBox)
                .aria_label(SEARCH_LABEL)
                .aria_value(query.read(cx).as_str().to_owned())
                .aria_placeholder(PLACEHOLDER)
                .child(split_view::search_field(&query, PLACEHOLDER, &theme))
                .into_any_element(),
            types.into_any_element(),
            &theme,
        );

        // The list: the day sections and their records, drawn virtually
        // (#165), or why none.
        let list = split_view::list(&theme)
            .role(Role::ListBox)
            .aria_label("Clipboard history");
        let split = &theme.split;
        let list = if frame.rows.is_empty() {
            let note = empty_note(
                history.unreadable.as_deref(),
                !history.records.is_empty(),
                history.capture,
            );
            list.child(
                div()
                    .pt(split.list_padding_top)
                    .px(split.list_padding_x)
                    .pb(split.list_padding_bottom)
                    .child(split_view::empty_note(note, &theme)),
            )
        } else {
            list.child(
                gpui::list(
                    list_state,
                    cx.processor(|this, index, _: &mut Window, cx| {
                        this.render_clip_child(index, cx)
                    }),
                )
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .pt(split.list_padding_top)
                .pb(split.list_padding_bottom),
            )
        };

        // The detail: the selected record as it was copied — its text, its
        // image or its files — over its Information.
        let detail = selected.map(|record| {
            let info = information(record, now, offset);
            let mut rows = Vec::new();
            if let Some(source) = info.source {
                let icon = info.source_path.as_deref().map(|path| {
                    let icon = self.launcher.clipboard_source_icon(path);
                    extension_icon::draw(
                        &crate::features::icons::drawn(&icon, &theme),
                        IconSize::small(theme.split.info_icon),
                        ("clipboard-source-icon", 0usize),
                        "clipboard-source-icon",
                        &theme,
                    )
                    .into_any_element()
                });
                rows.push(InfoRow {
                    label: "Source",
                    value: source.into(),
                    icon,
                });
            }
            rows.push(InfoRow {
                label: "Type",
                value: info.kind.into(),
                icon: None,
            });
            if let Some(characters) = info.characters {
                rows.push(InfoRow {
                    label: "Characters",
                    value: characters.to_string().into(),
                    icon: None,
                });
            }
            if let Some(dimensions) = info.dimensions {
                rows.push(InfoRow {
                    label: "Dimensions",
                    value: dimensions.into(),
                    icon: None,
                });
            }
            rows.push(InfoRow {
                label: "Copied",
                value: info.copied.into(),
                icon: None,
            });
            let preview = if let Some(image) = &record.image {
                split_view::image_preview(image.path.clone(), &theme)
            } else if !record.files.is_empty() {
                let files = record
                    .files
                    .iter()
                    .enumerate()
                    .map(|(index, file)| split_view::FileLine {
                        icon: self.file_icon(
                            file,
                            ("clipboard-preview-file-icon", index),
                            "clipboard-preview-file",
                            &theme,
                        ),
                        name: file_name(file).into(),
                        folder: file
                            .parent()
                            .map(|folder| folder.display().to_string())
                            .unwrap_or_default()
                            .into(),
                    })
                    .collect();
                split_view::files_preview(files, &theme)
            } else {
                split_view::text_preview(record.text.clone(), &theme)
            };
            split_view::detail(
                split_view::preview_card(
                    SharedString::from(format!("clipboard-preview-{}", record.id)).into(),
                    preview.into_any_element(),
                    &theme,
                )
                .into_any_element(),
                split_view::information(rows, &theme),
                &theme,
            )
            .into_any_element()
        });

        // The footer: the command's icon and title, as Raycast's footer
        // names the open command (#162), or the outcome of what was just
        // done, then Paste and Actions. A toast the command showed speaks
        // where the outcome would (#141).
        let toast = self.footer_toast(&view.status).map(|shown| {
            let (selector, color) = super::toast::style_look(shown.toast.style, &theme);
            (selector, shown.toast.text(), color)
        });
        let outcome = super::announcer::says_message(&view.status, toast.is_some());
        let status = match &view.status {
            _ if toast.is_some() => toast,
            Status::Idle => None,
            Status::Running => Some(("status-running", "Running…".to_owned(), theme.warning)),
            Status::Progress(work) => Some(("status-progress", work.clone(), theme.warning)),
            Status::Result(answer) => Some(("status-result", answer.clone(), theme.success)),
            Status::Error(message) => Some(("status-error", message.clone(), theme.danger)),
        }
        .filter(|_| show_outcome);
        // The window's announcer says it too (#132), when it is a toast or
        // an outcome.
        let said = status
            .as_ref()
            .filter(|_| outcome)
            .map(|(_, text, _)| text.clone());
        let (selector, lead) = match &status {
            Some((selector, text, color)) => (
                *selector,
                split_view::footer_status(text.clone(), *color, &theme),
            ),
            None => {
                let icon = match self.launcher.open_command_id() {
                    Some(id) => crate::features::icons::row_icon_of(&self.launcher, &id, &theme),
                    None => (IconTone::Command, Glyph::Clipboard).into(),
                };
                (
                    "status-idle",
                    footer::command_lead(&icon, history.title.clone(), &theme),
                )
            }
        };
        let keyboard = crate::settings::keyboard_of(cx);
        let invoke =
            crate::keyboard::binding_keys(keyboard.binding(KeyboardAction::InvokeSelectedAction));
        let actions_keys =
            crate::keyboard::binding_keys(keyboard.binding(KeyboardAction::OpenActions));
        // The footer buttons' hover washes, read as they are drawn and
        // reported by the buttons themselves (#245).
        let hover_now = cx.background_executor().now();
        let look = |spot: Spot| self.motion.hover.look(spot, hover_now);
        // Paste (Enter) acts on the selected record: with none, there is no
        // primary action at all.
        let paste = selected.map(|_| {
            footer::footer_button(
                "clipboard-paste",
                "Paste",
                &invoke,
                CapStyle::Accent,
                ButtonWash::Hover(look(Spot::Button("clipboard-paste"))),
                &theme,
            )
            .role(Role::Button)
            .aria_label("Paste")
            .aria_keyshortcuts(invoke.name())
            .cursor_pointer()
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                this.motion
                    .hover
                    .set(Spot::Button("clipboard-paste"), *over, cx);
            }))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.paste_selected_record(window, cx);
            }))
            .into_any_element()
        });
        let more = footer::footer_button(
            "clipboard-actions",
            "Actions",
            &actions_keys,
            CapStyle::Regular,
            ButtonWash::Hover(look(Spot::Button("clipboard-actions"))),
            &theme,
        )
        .role(Role::Button)
        .aria_label("Actions")
        .aria_keyshortcuts(actions_keys.name())
        .cursor_pointer()
        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
            this.motion
                .hover
                .set(Spot::Button("clipboard-actions"), *over, cx);
        }))
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            if this.actions_open() {
                this.close_actions(window, cx);
            } else {
                this.open_clipboard_panel(window, cx);
            }
        }))
        .into_any_element();
        let buttons = split_view::footer_buttons(paste, more, &theme);
        let footer = split_view::footer(lead, buttons, &theme)
            .id("status")
            .role(Role::Status)
            .when_some(status.map(|(_, text, _)| text), |footer, text| {
                footer.aria_label(text)
            })
            .debug_selector(move || selector.into())
            // The Actions panel, over the footer as the launcher's is.
            .when_some(self.render_actions_layer(window, cx), |footer, panel| {
                footer.child(panel)
            });

        let content = split_view::compose(
            header,
            list.into_any_element(),
            detail,
            footer.into_any_element(),
        )
        .key_context(CONTEXT)
        .on_action(cx.listener(Self::clipboard_delete))
        .on_action(cx.listener(Self::clipboard_copy));
        // The window's live region (#132): the open Actions panel's list,
        // else the history's.
        let followed = self.panel_listing(cx).or(Some(followed));
        let announcer = self.announce(followed, said.as_deref(), cx);
        let root = div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::clipboard_next))
            .on_action(cx.listener(Self::clipboard_previous))
            .on_action(cx.listener(Self::clipboard_next_page))
            .on_action(cx.listener(Self::clipboard_previous_page))
            .on_action(cx.listener(Self::clipboard_confirm))
            .on_action(cx.listener(Self::clipboard_back))
            .on_action(cx.listener(Self::clipboard_actions))
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
            .child(content)
            .child(announcer);
        Some(visuals.material.panel(&theme, root))
    }
}

impl LauncherWindow {
    /// The system's icon of the file at `path`, at a row tile's size, as a
    /// files record's row and preview show it (#167): requested only when
    /// drawn, so a row out of view asks for none. `scope` names it in the
    /// debug selectors.
    fn file_icon(
        &self,
        path: &Path,
        id: impl Into<ElementId>,
        scope: &str,
        theme: &Theme,
    ) -> AnyElement {
        let icon = self.launcher.clipboard_source_icon(path);
        extension_icon::draw(
            &crate::features::icons::drawn(&icon, theme),
            IconSize::of(TileSize::Row, theme),
            id,
            scope,
            theme,
        )
        .into_any_element()
    }

    /// The split view's list child at `index` of the frame laid out, as
    /// the list draws it: a day's label or a record's row, with the gap
    /// after it and the list's side padding.
    fn render_clip_child(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(frame) = self
            .clipboard
            .as_ref()
            .and_then(|history| history.frame.clone())
        else {
            return div().into_any_element();
        };
        let theme = crate::settings::launcher_visuals(cx).theme;
        let child = match frame.children.get(index) {
            Some(&ListChild::Label(at)) => {
                let section = &frame.sections[at];
                let debug = format!("section-{}", section.label);
                shell::section_label(section.label.clone(), section.note.clone(), &theme)
                    .debug_selector(move || debug)
                    .into_any_element()
            }
            Some(&ListChild::Row(row)) => match frame.rows.get(row) {
                Some(record) => {
                    let on = frame.selected == Some(row);
                    let id = record.id.clone();
                    let title = record.title.clone();
                    let mark = match &record.mark {
                        ClipMark::Kind(kind) => split_view::kind_mark(kind_glyph(*kind), &theme),
                        ClipMark::Thumbnail(path) => {
                            split_view::thumbnail_mark(path.clone(), &theme)
                        }
                        ClipMark::File(path) => {
                            self.file_icon(path, ("clip-file-icon", row), "clip-file", &theme)
                        }
                    };
                    split_view::clip_row(
                        ("clip", row),
                        ClipRow {
                            title: title.clone(),
                            time: record.time.clone(),
                            selected: on,
                            hover: self
                                .motion
                                .hover
                                .look(Spot::Clip(row), cx.background_executor().now()),
                        },
                        mark,
                        &theme,
                    )
                    .debug_selector(move || format!("clip-{title}"))
                    .role(Role::ListBoxOption)
                    .aria_label(record.title.clone())
                    .aria_selected(on)
                    .aria_position_in_set(row + 1)
                    .aria_size_of_set(frame.rows.len())
                    .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                        this.motion.hover.set(Spot::Clip(row), *over, cx);
                    }))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if let Some(history) = this.clipboard.as_mut() {
                            history.browse.select(id.clone());
                            history.outcome = false;
                            this.announcer.user_moved();
                            cx.notify();
                        }
                    }))
                    .into_any_element()
                }
                None => div().into_any_element(),
            },
            Some(ListChild::Head) | None => div().into_any_element(),
        };
        let last = index + 1 >= frame.children.len();
        virtual_list::item(
            child,
            last,
            theme.geometry.row_list_gap,
            theme.split.list_padding_x,
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_says_why_it_lists_nothing() {
        assert_eq!(
            empty_note(None, true, CaptureState::On),
            "No entries match. Try another search or type."
        );
        assert!(empty_note(None, false, CaptureState::Paused).contains("paused"));
        assert!(empty_note(None, false, CaptureState::On).starts_with("Nothing copied yet"));
        assert_eq!(
            empty_note(Some("Cannot read"), false, CaptureState::On),
            "Cannot read"
        );
        assert_eq!(PLACEHOLDER, "Type to filter entries…");
        let labels: Vec<String> = type_choices()
            .into_iter()
            .map(|c| c.label.to_string())
            .collect();
        assert_eq!(
            labels,
            ["All Types", "Text", "Images", "Files", "Links", "Colors"]
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn delete_shows_its_windows_keys() {
        assert_eq!(delete_keys().name(), "Ctrl+D");
    }
}
