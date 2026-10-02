//! The launcher window: orchestration of the launcher's screens, navigation
//! and action dispatch.
//!
//! [`LauncherWindow`] is a thin renderer over [`pane_core::Launcher`]: key
//! and mouse input call launcher actions, and each frame draws the
//! launcher's snapshot. The query field, forms and custom views bind their
//! data in their own modules — [`crate::features`] and
//! [`crate::extension_views`] — whose `impl LauncherWindow` blocks supply
//! the per-screen sync and render methods this orchestration calls.

use std::future::Future;
use std::mem::{Discriminant, discriminant};
use std::path::Path;

use gpui::{
    ClipboardItem, Context, Div, FocusHandle, Hsla, KeyDownEvent, PathPromptOptions, Pixels, Role,
    ScrollHandle, SharedString, Size, Stateful, Window, WindowControlArea, div, prelude::*, px,
    relative,
};
use pane_core::changes::Changes;
use pane_core::hotkeys::Shortcut;
use pane_core::{Launcher, LauncherView, Row, Screen, Status};

use crate::extension_views::{custom_view, form};
use crate::features::root_search;
use crate::ui::icon::{Glyph, IconTone};
use crate::ui::result_row::{RowContent, result_row};
use crate::ui::{self, material::Material};
use crate::{Back, Confirm, FocusNext, FocusPrevious, SelectNext, SelectPrevious};

pub(crate) const KEY_CONTEXT: &str = "Launcher";

/// The launcher window's root view.
pub struct LauncherWindow {
    pub(crate) launcher: Launcher,
    /// The list's focus, on screens other than root search.
    pub(crate) focus_handle: FocusHandle,
    /// Root search's query field, which has focus on root search.
    pub(crate) query: root_search::QueryField,
    /// The open form's controls; `Some` exactly on the form screen.
    pub(crate) form: Option<form::FormControls>,
    /// The open custom view's focus and layout; `Some` exactly on the
    /// custom view screen.
    pub(crate) custom_view: Option<custom_view::CustomViewControls>,
    /// The list's scroll position.
    scroll: ScrollHandle,
    /// What the list was last scrolled for.
    scrolled_for: Option<ScrolledFor>,
    /// Whether the next frame scrolls to the selected row again, once the
    /// list changed in this one has been laid out.
    scroll_again: bool,
    /// The launcher's view as the last frame drew it, for tests (see
    /// [`LauncherWindow::drawn_view`]). Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    drawn: Option<LauncherView>,
}

/// What the list was last scrolled for. When any of it changes, the list
/// scrolls the least it can to keep the selected row visible: the screen,
/// title or selection; the rows, as reloaded after an install; or the size
/// of the window or of the list. The mouse wheel changes none of it, so the
/// list never scrolls back while the user scrolls it.
#[derive(PartialEq)]
struct ScrolledFor {
    /// Which screen, not its contents (a form's values, a view's drawing).
    screen: Discriminant<Screen>,
    title: String,
    selected: Option<usize>,
    rows: Vec<Row>,
    window: Size<Pixels>,
    list: Size<Pixels>,
}

impl LauncherWindow {
    pub fn new(launcher: Launcher, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let query = root_search::QueryField::new(cx);
        // The launcher starts at root search.
        query.focus(window, cx);
        // Quitting ends development: its watchers go and a running build
        // is stopped with the processes it started.
        cx.on_app_quit(|this: &mut Self, _| {
            this.launcher.stop_all_development();
            async {}
        })
        .detach();
        LauncherWindow {
            launcher,
            focus_handle,
            query,
            form: None,
            scroll: ScrollHandle::new(),
            scrolled_for: None,
            scroll_again: false,
            custom_view: None,
            #[cfg(any(test, debug_assertions))]
            drawn: None,
        }
    }

    pub fn launcher(&self) -> &Launcher {
        &self.launcher
    }

    /// Test support: the launcher's view as the window last drew it; `None`
    /// before the first frame. The launcher changes on other threads (a
    /// crash pausing a package, a build) before the window is told to
    /// redraw, so a test compares this with [`Launcher::view`] to know that
    /// a frame shows what the launcher holds. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn drawn_view(&self) -> Option<&LauncherView> {
        self.drawn.as_ref()
    }

    /// Redraws whenever the launcher changes in the background, as
    /// `changes` (the other end of the launcher's
    /// [`with_development`](Launcher::with_development)) reports: a package
    /// being developed is building, failed to build or was reloaded.
    pub fn follow_changes(
        &mut self,
        mut changes: Changes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            while changes.next().await.is_some() {
                let shown = this.update_in(cx, |this, window, cx| {
                    this.sync_screen(window, cx);
                    cx.notify();
                });
                if shown.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.launcher.move_selection(1);
        cx.notify();
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.launcher.move_selection(-1);
        cx.notify();
    }

    fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.launcher.view().screen, Screen::Form(_)) {
            self.submit_form(window, cx);
        } else {
            self.activate_selected(window, cx);
        }
    }

    fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        self.launcher.back();
        self.sync_screen(window, cx);
        cx.notify();
    }

    /// Shows the package in `folder` with its identity and compatibility,
    /// redrawing when the check finishes.
    pub fn preview_package(&mut self, folder: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let pending = self.launcher.preview_package(folder);
        self.show_until_done(pending, window, cx);
    }

    /// Downloads and shows the npm package `spec` names, as
    /// [`LauncherWindow::preview_package`] shows a folder.
    pub fn preview_npm(&mut self, spec: &str, window: &mut Window, cx: &mut Context<Self>) {
        let pending = self.launcher.preview_npm(spec);
        self.show_until_done(pending, window, cx);
    }

    /// Fetches and shows the revision of the Git repository `spec` names,
    /// as [`LauncherWindow::preview_package`] shows a folder.
    pub fn preview_git(&mut self, spec: &str, window: &mut Window, cx: &mut Context<Self>) {
        let pending = self.launcher.preview_git(spec);
        self.show_until_done(pending, window, cx);
    }

    /// Asks for the folder to grant the package with `identity` with the
    /// platform's folder picker, then has Pane check and record it.
    /// Cancelling changes nothing. A debug build run by the native smokes
    /// takes the folder `PANE_TEST_CHOOSE_FOLDER` names instead of showing
    /// the picker (nothing else sets it; a release build has no such hook).
    fn choose_granted_folder(
        &mut self,
        identity: pane_core::PackageIdentity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(debug_assertions)]
        if let Some(folder) = std::env::var_os("PANE_TEST_CHOOSE_FOLDER") {
            let pending = self.launcher.grant_folder(&identity, Path::new(&folder));
            self.show_until_done(pending, window, cx);
            return;
        }
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let folder = match chosen.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.launcher
                            .show_error(format!("Could not open a folder picker: {error:#}"));
                        cx.notify();
                    })
                    .ok();
                    None
                }
            };
            if let Some(folder) = folder {
                this.update_in(cx, |this, window, cx| {
                    let pending = this.launcher.grant_folder(&identity, &folder);
                    this.show_until_done(pending, window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// Asks for a package folder with the platform's folder picker, then
    /// previews it. Cancelling leaves root search as it was.
    fn choose_package_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Install".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let folder = match chosen.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.launcher
                            .show_error(format!("Could not open a folder picker: {error:#}"));
                        cx.notify();
                    })
                    .ok();
                    None
                }
            };
            if let Some(folder) = folder {
                this.update_in(cx, |this, window, cx| {
                    this.preview_package(&folder, window, cx)
                })
                .ok();
            }
        })
        .detach();
    }

    /// Opens the command whose global hotkey `shortcut` is, as the system
    /// reported it pressed while any application had focus: the window
    /// comes to the front and shows the command. A press that opens nothing
    /// (a hotkey released meanwhile) leaves the window where it is.
    pub fn hotkey_pressed(
        &mut self,
        shortcut: &Shortcut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.launcher.press_hotkey(shortcut) else {
            return;
        };
        window.activate_window();
        cx.activate(true);
        self.show_until_done(pending, window, cx);
    }

    /// On the hotkey screen, a key pressed with its modifiers is the new
    /// hotkey. Keys the launcher binds (Enter, Escape, arrows, Tab) do not
    /// reach here; pressing a modifier alone is not a key press.
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.launcher.view().screen, Screen::Hotkey { .. }) {
            return;
        }
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let shortcut = Shortcut::new(
            modifiers.control,
            modifiers.alt,
            modifiers.shift,
            modifiers.platform,
            &keystroke.key,
        );
        cx.stop_propagation();
        match shortcut {
            Ok(shortcut) => {
                let pending = self.launcher.record_hotkey(shortcut);
                self.show_until_done(pending, window, cx);
            }
            Err(problem) => {
                self.launcher.show_error(problem);
                cx.notify();
            }
        }
    }

    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
    }

    fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
    }

    /// Starts the selected row's action and redraws when the guest answers,
    /// without blocking the window meanwhile.
    fn activate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.launcher.selected_asks_for_folder() {
            self.choose_package_folder(window, cx);
            return;
        }
        if let Some(identity) = self.launcher.folder_to_choose() {
            self.choose_granted_folder(identity, window, cx);
            return;
        }
        if let Some(text) = self.launcher.selected_copy() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        let pending = self.launcher.activate_selected();
        self.show_until_done(pending, window, cx);
    }

    /// Shows the launcher's state now and again when `pending`, a launcher
    /// action's reply, has been applied, without blocking the window
    /// meanwhile. Each time the form's and custom view's controls follow the
    /// launcher's screen (opening a form needs no guest call, so its controls
    /// appear at once).
    pub(crate) fn show_until_done(
        &mut self,
        pending: impl Future<Output = ()> + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_screen(window, cx);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            pending.await;
            this.update_in(cx, |this, window, cx| {
                this.sync_screen(window, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Scrolls the list to the selected row when what it shows or its size
    /// changed since it was last scrolled for (see [`ScrolledFor`]).
    fn keep_selected_visible(&mut self, view: &LauncherView, window: &mut Window) {
        let shown = ScrolledFor {
            screen: discriminant(&view.screen),
            title: view.title.clone(),
            selected: view.selected,
            rows: view.rows.clone(),
            window: window.viewport_size(),
            // As laid out in the last frame.
            list: self.scroll.bounds().size,
        };
        let last = self.scrolled_for.as_ref();
        if last == Some(&shown) && !self.scroll_again {
            return;
        }
        // Scrolling uses the list's size and rows as last laid out. When the
        // window, the screen or the rows changed, those are known only once
        // this frame is laid out, so the next frame scrolls again with them:
        // otherwise a short screen after a long list, scrolled far down,
        // would keep an offset that hides its selected row.
        let relaid = last.is_some_and(|last| {
            last.window != shown.window
                || last.screen != shown.screen
                || last.title != shown.title
                || last.rows != shown.rows
        });
        self.scroll_again = relaid && !self.scroll_again;
        if self.scroll_again {
            window.request_animation_frame();
        }
        if let Some(selected) = view.selected {
            self.scroll.scroll_to_item(selected);
        }
        self.scrolled_for = Some(shown);
    }

    /// Makes the form's and custom view's controls, root search's query
    /// field, and focus, follow the launcher's screen.
    fn sync_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_form(window, cx);
        self.sync_custom_view(window, cx);
        // Last: coming back to root search, even as a view closes, focuses
        // the query rather than the list.
        self.sync_root_search(window, cx);
    }

    fn render_row(
        &self,
        index: usize,
        row: Row,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &ui::visuals().theme;
        let reason = row.unavailable.as_ref().map(|u| u.reason().to_owned());
        // The row's accessible description: its subtitle and, when it
        // cannot run, the reason, together.
        let description = match (&row.subtitle, &reason) {
            (Some(subtitle), Some(reason)) => Some(format!("{subtitle}. {reason}")),
            (subtitle, reason) => subtitle.clone().or(reason.clone()),
        };
        // Presentation only: the shared row paints the chrome, and the
        // identity, accessibility and click behavior are attached here.
        result_row(
            RowContent {
                title: row.title.clone().into(),
                subtitle: row.subtitle.clone().map(SharedString::from),
                unavailable_reason: reason.map(SharedString::from),
                selected,
                unavailable_id: ("unavailable", index).into(),
                icon: row_icon(&row.id),
            },
            theme,
        )
        .id(("row", index))
        .debug_selector(|| format!("row-{}", row.title))
        .role(Role::ListBoxOption)
        .aria_label(row.title.clone())
        .aria_selected(selected)
        .when(selected, |row| row.aria_active_descendant())
        // An unavailable row stays listed and selectable; it says why it
        // cannot run here, on screen and to assistive technology.
        .when(row.unavailable.is_some(), |row| row.aria_disabled(true))
        .when_some(description, |row, description| {
            row.aria_description(description)
        })
        .on_click(cx.listener(move |this, _, window, cx| {
            this.launcher.select(index);
            this.activate_selected(window, cx);
        }))
    }
}

impl Render for LauncherWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.launcher.view();
        #[cfg(any(test, debug_assertions))]
        {
            self.drawn = Some(view.clone());
        }
        self.keep_selected_visible(&view, window);
        let theme = ui::visuals().theme.clone();
        let material = ui::visuals().material;
        let (empty, hint) = match &view.screen {
            Screen::Root { .. } => (
                "No commands are installed.",
                "Type to search · ↑↓ select · Enter open · Esc clear",
            ),
            Screen::Command => (
                "This command has no items.",
                "↑↓ select · Enter run · Esc back",
            ),
            Screen::CommandSearch { .. } => (
                "This command has no items.",
                "Type to search · ↑↓ select · Enter run · Esc clear, then back",
            ),
            Screen::Package { .. } => ("Nothing to install.", "Enter confirm · Esc back"),
            Screen::Form(_) => ("", "Tab next field · Enter submit · Esc back"),
            Screen::Extensions { .. } => (
                "No extensions are installed.",
                "↑↓ select · Enter choose · Esc back",
            ),
            Screen::CustomView(_) => ("", "Keys and pointer go to the view · Esc back"),
            Screen::Confirm { .. } => ("", "↑↓ select · Enter choose · Esc cancel"),
            Screen::Hotkey { .. } => ("", "Press the new hotkey · Enter choose · Esc back"),
            Screen::PauseDetails { .. } => ("", "Enter retry · Esc back"),
            Screen::NetworkDetails { .. } => ("", "Esc back"),
            Screen::RuntimeDetails { .. } => ("", "Enter restart · Esc back"),
            Screen::BuildDetails { .. } => ("", "Enter build again · Esc back"),
        };
        // A confirmation, and a package preview offering Install or Update
        // (an npm or Git package's has several more lines), keep their choices in
        // view.
        let preview = matches!(view.screen, Screen::Package { .. }) && !view.rows.is_empty();
        let confirm = matches!(view.screen, Screen::Confirm { .. }) || preview;
        // A preview has one or two rows (Install or Update) and more lines
        // to read, which may take more of the window than a confirmation's.
        let details_share = if preview { 0.62 } else { 0.4 };
        let details: Vec<_> = view
            .details()
            .iter()
            .enumerate()
            .map(|(index, line)| {
                div()
                    .id(("detail", index))
                    .debug_selector(|| format!("detail-{line}"))
                    .text_size(theme.typography.row_subtitle_size)
                    .text_color(theme.text_body)
                    .child(line.clone())
            })
            .collect();
        // A command's search that failed lists nothing; its error says why,
        // not "No results".
        let search_failed = matches!(
            (&view.screen, &view.status),
            (Screen::CommandSearch { .. }, Status::Error(_))
        );
        let (status_selector, status_text, status_color): (&str, SharedString, Hsla) =
            match view.status {
                Status::Idle => ("status-idle", hint.into(), theme.text_muted),
                Status::Running => ("status-running", "Running…".into(), theme.warning),
                Status::Progress(work) => ("status-progress", work.into(), theme.warning),
                Status::Result(answer) => ("status-result", answer.into(), theme.success),
                Status::Error(message) => ("status-error", message.into(), theme.danger),
            };
        let rows: Vec<_> = view
            .rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let selected = view.selected == Some(index);
                self.render_row(index, row, selected, cx)
            })
            .collect();
        let empty = match &view.screen {
            Screen::CommandSearch { .. } if search_failed => div().id("empty"),
            Screen::Root { query } | Screen::CommandSearch { query }
                if !query.trim().is_empty() =>
            {
                div()
                    .id("no-results")
                    .debug_selector(|| "no-results".into())
                    .child(format!("No results for “{}”", query.trim()))
            }
            _ => div().id("empty").child(empty),
        };
        let list = div()
            .id("rows")
            .debug_selector(|| "rows".into())
            .role(Role::ListBox)
            .aria_label(match view.screen {
                Screen::Root { .. } => "Results".into(),
                Screen::CommandSearch { .. } => format!("{} results", view.title),
                _ => view.title.clone(),
            })
            .flex_1()
            .flex()
            .flex_col()
            .gap(theme.geometry.row_list_gap)
            .px(theme.geometry.row_padding_x)
            .pt(px(4.))
            .pb(px(10.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            // Above the rows: with none selected, the only rows are root
            // search's fallbacks, listed below "No results".
            .when(view.selected.is_none(), |rows| {
                rows.child(empty.text_color(theme.text_muted))
            })
            .children(rows);
        // The launcher decides what an item opens; its screen says which.
        // The search screens carry their own header (the query field);
        // every other screen keeps its heading. Root search has no extra
        // title — the reference's launcher has none. The heading is
        // computed before the body dispatch, which moves the screen.
        // It is also the non-search screens' drag region: with the native
        // title bar hidden, the heading is the one place outside the
        // editable field to grab the window by, and a long heading
        // truncates instead of eating the list.
        let heading = match &view.screen {
            Screen::Root { .. } => None,
            _ => Some(
                div()
                    .flex_none()
                    .px(theme.geometry.search_padding_x)
                    .py(px(12.))
                    .truncate()
                    .text_size(theme.typography.row_title_size)
                    .font_weight(theme.typography.medium)
                    .text_color(theme.text_title)
                    .window_control_area(WindowControlArea::Drag)
                    .child(view.title.clone()),
            ),
        };

        let body = match view.screen {
            Screen::Form(form) => self.render_form(view.title.clone(), form, cx),
            Screen::CustomView(custom_view) => self.render_custom_view(custom_view, cx),
            Screen::Root { query } => {
                self.render_search(query, root_search::ROOT_PLACEHOLDER, list, cx)
            }
            // The opened command's own search field, the same control.
            Screen::CommandSearch { query } => {
                self.render_search(query, root_search::COMMAND_PLACEHOLDER, list, cx)
            }
            // The list holds keyboard focus; the selected row is its active
            // descendant, and key actions bubble to the root.
            _ => list.track_focus(&self.focus_handle).into_any_element(),
        };

        // The launcher's content: the shared Geist family and base text
        // color on everything, the heading (or the search header, in
        // `body`), the details, the body and the status footer.
        let content = div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::back))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .on_key_down(cx.listener(Self::key_down))
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .font_family(theme.typography.family.clone())
            .text_color(theme.text_title)
            .when_some(heading, |content, heading| content.child(heading))
            // A confirmation's or preview's long details scroll within 40%
            // (a preview's 62%) of the window, leaving the rest to its
            // choices, which stay visible.
            .when(!details.is_empty(), |content| {
                content.child(
                    div()
                        .id("details")
                        .flex()
                        .flex_col()
                        .gap_2()
                        .px(theme.geometry.search_padding_x)
                        .when(confirm, |details| {
                            details
                                .flex_shrink(1.)
                                .max_h(relative(details_share))
                                .overflow_y_scroll()
                        })
                        .children(details),
                )
            })
            .child(body)
            .child(
                Material::footer(&theme)
                    .id("status")
                    // Scrolling needs a stateful element, so the strip
                    // becomes its own scroll viewport here, once it has its
                    // id: past the 35% cap the message scrolls inside the
                    // strip instead of being cut.
                    .overflow_y_scroll()
                    .role(Role::Status)
                    .aria_label(status_text.clone())
                    .debug_selector(|| status_selector.into())
                    .text_size(theme.typography.footer_size)
                    .text_color(status_color)
                    .child(
                        // The message fills the strip's width and wraps
                        // there — a long error is several readable lines,
                        // never one clipped at the window's right edge —
                        // and the strip grows with it (its own bounds carry
                        // the status-* debug selectors; this one, the
                        // message's, lets tests see wrapping and scroll).
                        div()
                            .w_full()
                            .min_w(px(0.))
                            .flex_none()
                            .py(px(12.))
                            .debug_selector(|| "status-message".into())
                            .child(status_text),
                    ),
            );
        // The panel surface: the frost material's L1 glass around the
        // content, with the sheen beneath it.
        material.panel(&theme, content)
    }
}

/// The icon presentation for a row, chosen by the row's stable id: the
/// built-in rows and this build's sample commands are known identities,
/// each with a reference tone and glyph; everything else is a plain
/// command. No presentation is inferred from a title's text.
fn row_icon(id: &str) -> Option<(IconTone, Glyph)> {
    match id {
        "rust-sample" => Some((IconTone::Term, Glyph::Prompt)),
        "javascript-sample" | "typescript-sample" => Some((IconTone::Code, Glyph::Code)),
        "pane.install-from-folder" => Some((IconTone::Folder, Glyph::Folder)),
        "pane.install-from-npm" => Some((IconTone::Web, Glyph::Blocks)),
        "pane.install-from-git" => Some((IconTone::Term, Glyph::Terminal)),
        "pane.manage-extensions" => Some((IconTone::Command, Glyph::Blocks)),
        _ => Some((IconTone::Command, Glyph::Prompt)),
    }
}
