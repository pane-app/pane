//! Search Files in the split view (#177, spec #161): the launcher window's
//! adapter between the core's Search Files ([`pane_core::Launcher::search_files_view`],
//! over the launcher's own file rows) and the split view's components
//! ([`crate::ui::split_view`]), as Raycast's File Search is drawn.
//!
//! The view shows only while Pane's registered Files command is open on
//! its own search; a copy of Files installed from a folder, and every other
//! command, keep the launcher's list. The rows, the selection, Enter and the
//! Actions panel are the launcher's own, as on any command's search: the
//! search field is root search's field (the same control, the same keys),
//! Enter runs the selected file's primary action (Open; Show in Explorer
//! for a program, which never runs it), Ctrl+Enter its second (Show in
//! Explorer; Open With… for a program), and Ctrl+K the Actions panel with
//! Open With…, Copy Path, Copy Name, Copy File and Move to Recycle Bin.
//!
//! - The header: back, the search field ("Search files…") and the type
//!   dropdown at its right (All Types, Folder, Document, Image, Video,
//!   Audio, Archive, Text, Application, Other).
//! - The list: "Recently Used" before anything is typed, then what the
//!   index finds, each row with the system's icon of the file and its
//!   folder below the home folder; more rows load as the list scrolls
//!   toward its end. A line over the list says when the index is being
//!   built ("Indexing… (N found so far)"), or why it stopped, with a way to
//!   the File Search page in Settings.
//! - The detail: an image's preview (another file's large icon) over its
//!   Metadata: Name, Where, Type, Size, Created and Modified.
//! - The footer: the command's icon and title (or the launcher's status),
//!   the primary action and Actions.
//! - Escape clears the query first (Recently Used comes back), then leaves
//!   for root search; the back button leaves at once.
//!
//! The launcher window takes the view's 940×600 while it shows, and its
//! own size again once it leaves.

use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, Entity, EntityInputHandler, Focusable, Role,
    SharedString, Task, Window, div, prelude::*, px,
};
use pane_core::file_index::{IndexState, size_words};
use pane_core::search_files::{FileDetails, FileType, SearchFilesView};
use pane_core::{LauncherView, Screen, Status};

use crate::app::{KEY_CONTEXT, LauncherWindow, Spot};
use crate::features::announcer::{self, Listing, Noun, Opening, Selected, Target};
use crate::features::root_search;
use crate::ui::extension_icon::{self, IconSize};
use crate::ui::footer;
use crate::ui::icon::TileSize;
use crate::ui::select::{Choice, Model, Select};
use crate::ui::shell::{self, SectionLabel};
use crate::ui::split_view::{self, FoundFile, InfoRow};
use crate::ui::virtual_list::{self, ListChild, VirtualList};
use crate::{Back, ReturnToRoot};

/// The search field's placeholder, Raycast's.
pub(crate) const PLACEHOLDER: &str = "Search files…";

/// The type dropdown's debug selector: its trigger is this, a choice's row
/// `files-type-<id>` (`all`, `folder`, `document`, …).
pub(crate) const TYPE_SELECT: &str = "files-type";

/// The title of the File Search page in Settings (#176), where the index's
/// roots, exclusions and state are: that page's own title, so the link
/// always opens it.
pub(crate) const FILE_SEARCH_PAGE: &str = crate::features::settings::file_search::TITLE;

/// How many rows from the end of the list a drawn row starts the next
/// page loading.
const LOAD_AHEAD: usize = 10;

/// How often the open view looks at the index for what changed behind it:
/// while it is built, what it found grows, and the list is asked again.
const REFRESH: Duration = Duration::from_secs(1);

/// The type dropdown's choices: the types, in order.
fn type_choices() -> Vec<Choice> {
    FileType::ALL
        .into_iter()
        .map(|kind| Choice {
            id: kind.id().into(),
            label: kind.label().into(),
            subtitle: None,
            keywords: Vec::new(),
            unavailable_reason: None,
        })
        .collect()
}

/// When something happened, as the Metadata says it: "Today at 14:02",
/// "Sep 28 at 16:12".
fn when(at: u64, now: u64) -> String {
    pane_core::clipboard_view::copied_at_label(
        at,
        now,
        pane_core::clipboard_view::local_offset_ms(now),
    )
}

/// The Metadata of `details`, now being `now` (milliseconds since the
/// Unix epoch): Name, Where, Type, then Size, Created and Modified where
/// they are known.
pub(crate) fn metadata(details: &FileDetails, now: u64) -> Vec<(&'static str, String)> {
    let mut rows = vec![
        ("Name", details.name.clone()),
        ("Where", details.place.clone()),
        ("Type", details.kind.clone()),
    ];
    if let Some(size) = details.size {
        rows.push(("Size", size_words(size)));
    }
    if let Some(created) = details.created {
        rows.push(("Created", when(created, now)));
    }
    if let Some(modified) = details.modified {
        rows.push(("Modified", when(modified, now)));
    }
    rows
}

/// Search Files' state, owned by the launcher window while Pane's Files
/// command is open on its own search.
pub(crate) struct SearchFiles {
    /// The type chosen, shared with the dropdown's live model.
    chosen: Rc<Cell<FileType>>,
    /// The type dropdown at the search field's right.
    types: Entity<Select>,
    /// The list, drawn virtually (#165): only the rows in view are laid
    /// out and painted. Page Down and Up move by its page.
    pub(crate) list: VirtualList,
    /// What the list's children are drawn from, as the last frame read it.
    frame: Option<Rc<FilesFrame>>,
    /// The row the list last kept in view.
    revealed: Option<usize>,
    /// The selected file's detail, read when it was selected.
    details: Option<FileDetails>,
    /// How many rows were listed when the next page was last asked for:
    /// it is asked once per page, not on every frame that draws a row
    /// near the end.
    asked_more: usize,
    /// Looks at the index for what changed behind the view (see
    /// [`REFRESH`]); dropped, and so stopped, with the view.
    _watching: Task<()>,
}

/// What a frame draws in the list: its children are drawn from it as the
/// list lays them out.
struct FilesFrame {
    /// The rows listed: each one's title and folder (not its id, which a
    /// search gives anew each time: the same files listed again are the
    /// same rows, and the list stays where it is).
    rows: Vec<(String, String)>,
    sections: Vec<SectionLabel>,
    children: Vec<ListChild>,
    selected: Option<usize>,
    /// Whether more rows may load as the list scrolls.
    more: bool,
}

/// What of the index the view shows that changes behind it: its state and
/// how much it holds.
type Fingerprint = (IndexState, u64, u64, Option<String>);

fn fingerprint(view: &SearchFilesView) -> Fingerprint {
    (
        view.status.state,
        view.status.entries,
        view.status.found,
        view.status.reason.clone(),
    )
}

impl SearchFiles {
    fn new(window: &mut Window, cx: &mut Context<LauncherWindow>) -> SearchFiles {
        let chosen = Rc::new(Cell::new(FileType::All));
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
                    let Some(kind) = FileType::from_id(id) else {
                        return;
                    };
                    commit_to.set(kind);
                    this.update(cx, |this, cx| this.choose_file_type(kind, cx))
                        .ok();
                }),
                window,
                cx,
            )
        });
        let watching = cx.spawn(async move |this, cx| {
            let mut seen: Option<Fingerprint> = None;
            loop {
                cx.background_executor().timer(REFRESH).await;
                let open = this.update(cx, |this, cx| {
                    let now = this.launcher.search_files_view();
                    let print = now.as_ref().map(fingerprint);
                    if print == seen {
                        return;
                    }
                    // The index grew, or its state changed: the list is
                    // asked again while it may have grown with it.
                    let grew = seen.is_some()
                        && now.as_ref().is_some_and(|view| {
                            view.status.state == IndexState::Building
                                || seen.as_ref().map(|seen| seen.0) == Some(IndexState::Building)
                        });
                    seen = print;
                    if grew {
                        this.refresh_file_list(cx);
                    }
                    cx.notify();
                });
                if open.is_err() {
                    break;
                }
            }
        });
        SearchFiles {
            chosen,
            types,
            list: {
                let geometry = &crate::settings::launcher_visuals(cx).theme.geometry;
                VirtualList::new(geometry.row_min_height + geometry.row_list_gap)
            },
            frame: None,
            revealed: None,
            details: None,
            asked_more: 0,
            _watching: watching,
        }
    }
}

impl LauncherWindow {
    /// Test support: whether Search Files' split view shows (rather than
    /// another screen).
    #[doc(hidden)]
    pub fn search_files_shown(&self) -> bool {
        self.files.is_some()
    }

    /// Test support: the type Search Files' dropdown keeps.
    #[doc(hidden)]
    pub fn search_files_type(&self) -> Option<FileType> {
        self.launcher.search_files_view().map(|view| view.filter)
    }

    /// Makes Search Files' split view follow the launcher's screen: it
    /// opens, at the view's size, when the launcher shows Pane's Files
    /// command on its own search (root search's field keeps the focus),
    /// and closes, giving the window back its own size, once the launcher
    /// leaves it (a form or custom view opened from it keeps it).
    pub(crate) fn sync_search_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.launcher.search_files_view().is_some() {
            if self.files.is_none() {
                self.files = Some(SearchFiles::new(window, cx));
                self.fit_client(split_view::SPLIT_CLIENT, window, cx);
            }
            return;
        }
        let kept = matches!(
            self.launcher.screen(),
            Screen::Form(_) | Screen::CustomView(_)
        );
        if !kept && self.files.take().is_some() && self.clipboard.is_none() {
            self.fit_client(shell::LAUNCHER_CLIENT, window, cx);
        }
    }

    /// The type dropdown chose `kind`: the list is asked again for it.
    fn choose_file_type(&mut self, kind: FileType, cx: &mut Context<Self>) {
        let pending = self.launcher.set_file_type(kind);
        if let Some(files) = self.files.as_mut() {
            files.revealed = None;
        }
        cx.notify();
        Self::redraw_after(pending, cx);
    }

    /// The index grew while it is built: the first page is listed again.
    fn refresh_file_list(&mut self, cx: &mut Context<Self>) {
        Self::redraw_after(self.launcher.refresh_files(), cx);
    }

    /// The list drew a row near its end: the next page loads, if there is
    /// one and none is loading.
    fn load_more_files(&mut self, cx: &mut Context<Self>) {
        Self::redraw_after(self.launcher.load_more_files(), cx);
    }

    /// Awaits `pending`, a page of the index being listed, in the
    /// background, and draws the window again once it is.
    fn redraw_after(pending: impl Future<Output = ()> + 'static, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            pending.await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    /// Escape in Search Files: what is open over the view, or an input
    /// method's composition, goes first (the launcher's Back); then a query
    /// is cleared, as typing does it, so Recently Used is listed again and
    /// drawn once it is; only then is the command left.
    fn files_back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query_field();
        let composing = query
            .update(cx, |input, cx| input.marked_text_range(window, cx))
            .is_some();
        let over = self.actions_open() || self.menu.is_some();
        if !composing && !over && !query.read(cx).as_str().is_empty() {
            query.update(cx, |input, cx| input.emplace("", cx));
            return;
        }
        self.back(&Back, window, cx);
    }

    /// The split view, when it shows (see [`LauncherWindow::search_files_shown`]),
    /// for the launcher's frame: the launcher's keys bound as on a
    /// command's search. `view` is the launcher's view this frame draws.
    pub(crate) fn render_search_files(
        &mut self,
        view: &LauncherView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if !self.search_files_shown() {
            return None;
        }
        let files = self.launcher.search_files_view()?;
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = visuals.theme;
        let selected = view.selected.filter(|index| *index < view.rows.len());
        // A section jump landed on a row: the section's label scrolls
        // into view with it (#258), as the launcher's own list reveals
        // the jump's landing.
        let jumped = self
            .take_jump_reveal()
            .filter(|row| Some(*row) == selected);
        // The selected file's detail, read again when another is selected.
        let details = selected.and_then(|index| {
            let id = &view.rows[index].id;
            let state = self.files.as_mut()?;
            if state
                .details
                .as_ref()
                .is_none_or(|details| details.id != *id)
            {
                state.details = self.launcher.search_files_details(index);
            }
            state.details.clone()
        });
        let state = self.files.as_mut()?;
        // The dropdown's choice, as its model reads it.
        state.chosen.set(files.filter);
        let sections: Vec<SectionLabel> = files
            .section
            .filter(|_| !view.rows.is_empty())
            .map(|label| SectionLabel {
                first: 0,
                label: label.into(),
                note: None,
            })
            .into_iter()
            .collect();
        let frame = FilesFrame {
            rows: view
                .rows
                .iter()
                .map(|row| (row.title.clone(), row.subtitle.clone().unwrap_or_default()))
                .collect(),
            children: virtual_list::children(false, view.rows.len(), &sections),
            sections,
            selected,
            more: files.more,
        };
        // Measured again, from its top, only when the rows changed; a page
        // added at the end (more rows as the list scrolls) is measured as
        // it comes into view, and the list stays where it is.
        let appended = state.frame.as_ref().and_then(|last| {
            (last.sections == frame.sections
                && last.rows.len() < frame.rows.len()
                && frame.rows.starts_with(&last.rows)
                && state.list.count() == last.children.len())
            .then_some(last.children.len())
        });
        let changed = appended.is_none()
            && state.frame.as_ref().is_none_or(|last| {
                last.children != frame.children
                    || last.sections != frame.sections
                    || last.rows != frame.rows
            });
        if let Some(from) = appended {
            state
                .list
                .state()
                .splice(from..from, frame.children.len() - from);
        } else if changed || state.list.count() != frame.children.len() {
            state.list.reset(frame.children.len());
            // Other rows: their next page is asked for anew.
            state.asked_more = 0;
        }
        if (changed || state.revealed != selected || jumped.is_some())
            && let Some(selected) = selected
        {
            let child = virtual_list::child_of_row(false, &frame.sections, selected);
            state.list.reveal(child);
            if jumped.is_some() && child > 0 {
                state.list.reveal(child - 1);
            }
        }
        state.revealed = selected;
        // The list as the window's announcer follows it (#132): opening
        // with the command's title and count, root search's field the
        // typing, settled once the field's search and the page it asked
        // for have arrived.
        let target = match selected {
            Some(index) => Target::Row(Selected {
                id: view.rows[index].id.clone(),
                title: view.rows[index].title.clone(),
                position: index + 1,
                unavailable: view.rows[index].unavailable.is_some(),
                section: announcer::section_at(&frame.sections, index),
            }),
            None if view.rows.is_empty() => Target::NoResults,
            None => Target::Nothing,
        };
        let followed = Listing {
            over: false,
            key: format!("files {}", files.title),
            opening: Opening::Named(files.title.clone(), Noun::Results),
            count: view.rows.len(),
            target,
            query: Some(files.query.clone()),
            settled: self.announcer.settled() && !files.loading,
        };
        state.frame = Some(Rc::new(frame));
        let types = state.types.clone();
        let list_state = state.list.state().clone();

        // The header: back, root search's own field — the same control
        // and keys — and the type dropdown at its right.
        let query = self.query_field();
        let header = split_view::header(
            split_view::back_button(&theme)
                .id("files-back")
                .debug_selector(|| "files-back".into())
                .role(Role::Button)
                .aria_label("Back to all results")
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.return_to_root(&ReturnToRoot, window, cx);
                }))
                .into_any_element(),
            div()
                .id("files-search")
                .flex_1()
                .min_w(px(0.))
                .key_context(root_search::CONTEXT)
                .track_focus(&query.focus_handle(cx))
                .role(Role::EditableComboBox)
                .aria_label("Search files")
                .aria_value(files.query.clone())
                .aria_placeholder(PLACEHOLDER)
                .child(split_view::search_field(&query, PLACEHOLDER, &theme))
                .into_any_element(),
            types.into_any_element(),
            &theme,
        );

        // The list: the index's state, when there is something to say,
        // then the rows drawn virtually (#165), or why there are none.
        let split = &theme.split;
        let note = files.note().map(|note| {
            let link = files.needs_settings().then(|| {
                div()
                    .id("files-settings-link")
                    .debug_selector(|| "files-settings-link".into())
                    .role(Role::Button)
                    .cursor_pointer()
                    .text_color(theme.accent_text)
                    .child("Open File Search Settings")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        crate::features::settings::open_at(
                            &this.launcher,
                            FILE_SEARCH_PAGE,
                            "",
                            cx,
                        );
                    }))
            });
            div()
                .debug_selector(|| "files-note".into())
                .flex_none()
                .flex()
                .flex_wrap()
                .gap(split.info_gap)
                .px(split.list_padding_x + theme.geometry.row_padding_x)
                .pt(split.list_padding_top + px(6.))
                .text_size(split.time_size)
                .text_color(theme.text_muted)
                .child(note)
                .children(link)
        });
        // A thin bar while a search or the index is in progress.
        let busy = files.loading || files.status.state == IndexState::Building;
        let mut list = split_view::list(&theme)
            .debug_selector(|| "files-list".into())
            .role(Role::ListBox)
            .aria_label("Files")
            .when(busy, |list| {
                list.child(
                    div()
                        .debug_selector(|| "files-loading".into())
                        .flex_none()
                        .h(px(2.))
                        .w_full()
                        .bg(theme.accent_text),
                )
            });
        if view.rows.is_empty() {
            // The note is the empty line itself, with its link.
            let empty = files.empty_note();
            let link = files.needs_settings().then(|| {
                div()
                    .id("files-settings-link")
                    .debug_selector(|| "files-settings-link".into())
                    .role(Role::Button)
                    .cursor_pointer()
                    .mt(px(8.))
                    .text_size(split.empty_size)
                    .text_color(theme.accent_text)
                    .child("Open File Search Settings")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        crate::features::settings::open_at(
                            &this.launcher,
                            FILE_SEARCH_PAGE,
                            "",
                            cx,
                        );
                    }))
            });
            list = list.child(
                div()
                    .pt(split.list_padding_top)
                    .px(split.list_padding_x)
                    .pb(split.list_padding_bottom)
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        split_view::empty_note(empty, &theme)
                            .debug_selector(|| "files-empty".into()),
                    )
                    .children(link),
            );
        } else {
            list = list.children(note).child(
                gpui::list(
                    list_state,
                    cx.processor(|this, index, _: &mut Window, cx| {
                        this.render_files_child(index, cx)
                    }),
                )
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .pt(split.list_padding_top)
                .pb(split.list_padding_bottom),
            );
        }

        // The detail: an image's preview, another file's large icon, over
        // its Metadata.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        let detail = selected.zip(details).map(|(index, details)| {
            let preview = if details.preview {
                split_view::file_image_preview(details.path.clone(), &theme).into_any_element()
            } else {
                let icon = self.launcher.search_files_icon(index).map(|icon| {
                    extension_icon::draw(
                        &crate::features::icons::drawn(&icon, &theme),
                        IconSize::small(px(96.)),
                        ("files-preview-icon", index),
                        "files-preview-icon",
                        &theme,
                    )
                    .into_any_element()
                });
                split_view::icon_preview(icon.unwrap_or_else(|| div().into_any_element()))
                    .into_any_element()
            };
            let rows = metadata(&details, now)
                .into_iter()
                .map(|(label, value)| InfoRow {
                    label,
                    value: value.into(),
                    icon: None,
                })
                .collect();
            split_view::detail(
                split_view::preview_card(
                    SharedString::from(format!("files-preview-{}", details.id)).into(),
                    preview,
                    &theme,
                )
                .debug_selector(|| "files-preview".into())
                .into_any_element(),
                split_view::info_section("Metadata", "files", rows, &theme),
                &theme,
            )
            .into_any_element()
        });

        // The footer: the command's icon and title, or the launcher's
        // status (a toast in its place), then the selected file's primary
        // action and Actions, as on any command's search.
        let toast = self.footer_toast(&view.status).map(|shown| {
            let (selector, color) = super::toast::style_look(shown.toast.style, &theme);
            (selector, shown.toast.text(), color)
        });
        let outcome = super::announcer::says_message(&view.status, toast.is_some(), false);
        let status = match &view.status {
            _ if toast.is_some() => toast,
            Status::Idle => None,
            Status::Running { .. } => {
                Some(("status-running", "Running…".to_owned(), theme.warning))
            }
            Status::Progress(work) => Some(("status-progress", work.clone(), theme.warning)),
            Status::Result(answer) => Some(("status-result", answer.clone(), theme.success)),
            Status::Error(message) => Some(("status-error", message.clone(), theme.danger)),
        };
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
            None => (
                "status-idle",
                self.footer_command(view, &theme).unwrap_or_else(|| {
                    footer::command_lead(
                        &crate::features::icons::row_icon_of(
                            &self.launcher,
                            &self.launcher.open_command_id().unwrap_or_default(),
                            &theme,
                        ),
                        files.title.clone(),
                        &theme,
                    )
                }),
            ),
        };
        let action = self.launcher.selected_action();
        let buttons = self.footer_buttons(&action, true, status.is_some(), &theme, cx);
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
            })
            // The open toast's details, above the footer as the
            // launcher's are (#249).
            .when_some(
                self.render_toast_details_layer(
                    &theme,
                    visuals.material,
                    window.viewport_size(),
                    cx,
                ),
                |footer, details| footer.child(details),
            );

        let content = split_view::compose(
            header,
            list.into_any_element(),
            detail,
            footer.into_any_element(),
        );
        // The window's live region (#132): the open Actions panel's list,
        // else the files'.
        let followed = self
            .panel_listing(cx)
            .or_else(|| self.toast_details_listing())
            .or(Some(followed));
        let announcer = self.announce(followed, said.as_deref(), cx);
        let root = div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            // Page Down and Up move by the rows of Search Files' list in
            // view (`LauncherWindow::paged_list`); Alt+Up and Alt+Down by
            // five rows, and Ctrl+Up and Ctrl+Down cross the view's
            // sections, as they do the launcher's own list (#258).
            .on_action(cx.listener(Self::select_next_page))
            .on_action(cx.listener(Self::select_previous_page))
            .on_action(cx.listener(Self::select_next_five))
            .on_action(cx.listener(Self::select_previous_five))
            .on_action(cx.listener(Self::select_next_section))
            .on_action(cx.listener(Self::select_previous_section))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::files_back))
            .on_action(cx.listener(Self::return_to_root))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::toggle_actions))
            .on_action(cx.listener(Self::open_toast_details))
            // A toast's actions' shortcuts first, then the selected
            // file's action chords (Ctrl+Enter), as on any command's
            // search.
            .capture_key_down(cx.listener(Self::toast_action_keys))
            .capture_key_down(cx.listener(Self::item_action_keys))
            // The Back-a-level key backs out of the view's empty search
            // (#258), beneath the field's own Backspace.
            .capture_key_down(cx.listener(Self::backspace_back_keys))
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

    /// The list's child at `index` of the frame laid out, as the list draws
    /// it: "Recently Used" or a file's row, with the gap after it and the
    /// list's side padding. A row near the end loads the next page.
    fn render_files_child(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(frame) = self.files.as_ref().and_then(|files| files.frame.clone()) else {
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
                Some((title, folder)) => {
                    let near_end = frame.more && row + LOAD_AHEAD >= frame.rows.len();
                    if let Some(files) = self.files.as_mut()
                        && near_end
                        && files.asked_more != frame.rows.len()
                    {
                        files.asked_more = frame.rows.len();
                        self.load_more_files(cx);
                    }
                    let on = frame.selected == Some(row);
                    let icon = self
                        .launcher
                        .search_files_icon(row)
                        .map(|icon| {
                            extension_icon::draw(
                                &crate::features::icons::drawn(&icon, &theme),
                                IconSize::of(TileSize::Row, &theme),
                                ("files-icon", row),
                                "files-icon",
                                &theme,
                            )
                            .into_any_element()
                        })
                        .unwrap_or_else(|| div().into_any_element());
                    let debug = format!("files-row-{title}");
                    split_view::file_row(
                        ("files-row", row),
                        FoundFile {
                            title: title.clone().into(),
                            subtitle: folder.clone().into(),
                            selected: on,
                            hover: self
                                .motion
                                .hover
                                .look(Spot::Clip(row), cx.background_executor().now()),
                            icon,
                        },
                        &theme,
                    )
                    .debug_selector(move || debug)
                    .role(Role::ListBoxOption)
                    .aria_label(title.clone())
                    .aria_description(folder.clone())
                    .aria_selected(on)
                    .aria_position_in_set(row + 1)
                    .aria_size_of_set(frame.rows.len())
                    .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                        this.motion.hover.set(Spot::Clip(row), *over, cx);
                    }))
                    // A click selects; a double click runs the primary
                    // action, as Enter does.
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.launcher.select(row);
                        this.announcer.user_moved();
                        if event.click_count() >= 2 {
                            this.activate_selected(window, cx);
                        }
                        cx.notify();
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
    use std::path::PathBuf;

    #[test]
    fn the_dropdown_offers_raycasts_types() {
        let labels: Vec<String> = type_choices()
            .into_iter()
            .map(|choice| choice.label.to_string())
            .collect();
        assert_eq!(
            labels,
            [
                "All Types",
                "Folder",
                "Document",
                "Image",
                "Video",
                "Audio",
                "Archive",
                "Text",
                "Application",
                "Other"
            ]
        );
        assert_eq!(PLACEHOLDER, "Search files…");
    }

    #[test]
    fn the_metadata_lists_what_is_known_in_order() {
        let details = FileDetails {
            id: "i1-1".into(),
            path: PathBuf::from("/home/me/Pictures/shot.png"),
            name: "shot.png".into(),
            place: "~/Pictures".into(),
            kind: "PNG Image".into(),
            size: Some(1_200_000),
            created: Some(1_790_000_000_000),
            modified: Some(1_791_000_000_000),
            preview: true,
        };
        let labels: Vec<&str> = metadata(&details, 1_791_100_000_000)
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert_eq!(
            labels,
            ["Name", "Where", "Type", "Size", "Created", "Modified"]
        );
        assert_eq!(metadata(&details, 0)[3].1, "1.2 MB");
        let folder = FileDetails {
            size: None,
            created: None,
            ..details
        };
        let labels: Vec<&str> = metadata(&folder, 0)
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert_eq!(labels, ["Name", "Where", "Type", "Modified"]);
    }
}
