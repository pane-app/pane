//! Pane's native launcher window, rendered with GPUI CE.
//!
//! The window is a thin renderer over [`pane_core::Launcher`]: key and mouse
//! input call launcher actions, and each frame draws the launcher's snapshot.

use std::future::Future;
use std::mem::{Discriminant, discriminant};
use std::path::{Path, PathBuf};

use gpui::{
    App, ClipboardItem, Context, Div, FocusHandle, KeyBinding, KeyDownEvent, PathPromptOptions,
    Pixels, Role, ScrollHandle, SharedString, Size, Stateful, Window, actions, div, prelude::*,
    relative, rgb,
};
use pane_core::changes::Changes;
use pane_core::hotkeys::Shortcut;
use pane_core::{CommandRegistration, Launcher, LauncherView, Row, Screen, Status};

mod custom_view;
mod form;
mod links;
mod root_search;

pub use links::SystemLinks;

actions!(
    launcher,
    [
        SelectNext,
        SelectPrevious,
        Confirm,
        Back,
        FocusNext,
        FocusPrevious
    ]
);

const KEY_CONTEXT: &str = "Launcher";

/// Registers the launcher's key bindings.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(KEY_CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", Confirm, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", Back, Some(KEY_CONTEXT)),
        KeyBinding::new("tab", FocusNext, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(KEY_CONTEXT)),
    ]);
    let text_editing = form::bind_text_editing(cx);
    form::bind_keys(cx, &text_editing);
    root_search::bind_keys(cx, &text_editing);
    custom_view::bind_keys(cx);
}

/// The sample commands: (id, title, subtitle, component file name). Each
/// implements the same command in a different extension language.
const SAMPLES: [(&str, &str, &str, &str); 3] = [
    (
        "rust-sample",
        "Rust sample",
        "A sample command implemented by a Rust extension",
        "sample_rust.wasm",
    ),
    (
        "javascript-sample",
        "JavaScript sample",
        "The same command implemented by a JavaScript extension",
        "sample_js.wasm",
    ),
    (
        "typescript-sample",
        "TypeScript sample",
        "The same command implemented by a TypeScript extension",
        "sample_ts.wasm",
    ),
];

/// The commands this build offers: the Rust, JavaScript and TypeScript
/// sample commands.
///
/// Their components are read from `PANE_EXTENSIONS_DIR` when set, otherwise
/// from the development build output `target/guests`. A missing component
/// leaves its command listed; opening it explains what is missing.
pub fn sample_commands() -> Vec<CommandRegistration> {
    let dir = std::env::var_os("PANE_EXTENSIONS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests"));
    SAMPLES
        .iter()
        .map(|&(id, title, subtitle, file)| CommandRegistration {
            id: id.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            component: dir.join(file),
            takes_query: false,
            search: false,
        })
        .collect()
}

/// Where Pane keeps disposable cached data, such as compiled extension code:
/// `%LOCALAPPDATA%\Pane\cache` on Windows, `~/Library/Caches/Pane` on
/// macOS and `$XDG_CACHE_HOME/pane` (default `~/.cache/pane`) elsewhere.
pub fn cache_dir() -> Option<PathBuf> {
    platform_dir(
        r"Pane\cache",
        "Library/Caches/Pane",
        ("XDG_CACHE_HOME", ".cache"),
    )
}

/// Where Pane keeps installed extension packages: `PANE_DATA_DIR` when set,
/// otherwise `%LOCALAPPDATA%\Pane\data` on Windows,
/// `~/Library/Application Support/Pane` on macOS and `$XDG_DATA_HOME/pane`
/// (default `~/.local/share/pane`) elsewhere. Packages go in its
/// `extensions` folder.
pub fn data_dir() -> Option<PathBuf> {
    env_dir("PANE_DATA_DIR").or_else(|| {
        platform_dir(
            r"Pane\data",
            "Library/Application Support/Pane",
            ("XDG_DATA_HOME", ".local/share"),
        )
    })
}

/// A per-user folder: `windows` under `%LOCALAPPDATA%`, `macos` under
/// `$HOME`, and elsewhere `pane` under the XDG variable `xdg.0`, or under
/// `$HOME/xdg.1` when that is unset.
fn platform_dir(windows: &str, macos: &str, xdg: (&str, &str)) -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env_dir("LOCALAPPDATA").map(|dir| dir.join(windows))
    } else if cfg!(target_os = "macos") {
        env_dir("HOME").map(|home| home.join(macos))
    } else {
        let (variable, fallback) = xdg;
        env_dir(variable)
            .or_else(|| env_dir("HOME").map(|home| home.join(fallback)))
            .map(|dir| dir.join("pane"))
    }
}

/// The folder in environment variable `name`, if it is set and not empty.
fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The launcher window's root view.
pub struct LauncherWindow {
    launcher: Launcher,
    /// The list's focus, on screens other than root search.
    focus_handle: FocusHandle,
    /// Root search's query field, which has focus on root search.
    query: root_search::QueryField,
    /// The open form's controls; `Some` exactly on the form screen.
    form: Option<form::FormControls>,
    /// The open custom view's focus and layout; `Some` exactly on the
    /// custom view screen.
    custom_view: Option<custom_view::CustomViewControls>,
    /// The list's scroll position.
    scroll: ScrollHandle,
    /// What the list was last scrolled for.
    scrolled_for: Option<ScrolledFor>,
    /// Whether the next frame scrolls to the selected row again, once the
    /// list changed in this one has been laid out.
    scroll_again: bool,
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
        }
    }

    pub fn launcher(&self) -> &Launcher {
        &self.launcher
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
    fn show_until_done(
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
        let reason_selector = format!("unavailable-reason-{}", row.title);
        div()
            .id(("row", index))
            .debug_selector(|| format!("row-{}", row.title))
            .flex()
            .flex_col()
            .px_3()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .role(Role::ListBoxOption)
            .aria_label(row.title.clone())
            .aria_selected(selected)
            .when(selected, |row| {
                row.aria_active_descendant().bg(rgb(0x364355))
            })
            // Hovering never hides which row is selected.
            .when(!selected, |row| row.hover(|row| row.bg(rgb(0x2e3a48))))
            .child(
                div()
                    .when(row.unavailable.is_some(), |title| {
                        title.text_color(rgb(0x8a96a3))
                    })
                    .child(row.title),
            )
            .when_some(row.subtitle.clone(), |element, subtitle| {
                element.child(div().text_sm().text_color(rgb(0xaab4c0)).child(subtitle))
            })
            // An unavailable row stays listed and selectable; it says why it
            // cannot run here, on screen and to assistive technology.
            .when_some(
                row.unavailable.as_ref().map(|u| u.reason().to_owned()),
                |element, reason| {
                    element.aria_disabled(true).child(
                        div()
                            .id(("unavailable", index))
                            .debug_selector(|| reason_selector)
                            .text_sm()
                            .text_color(rgb(0xd6a36a))
                            .child(reason),
                    )
                },
            )
            .when_some(
                match (row.subtitle, row.unavailable.map(|u| u.reason().to_owned())) {
                    (Some(subtitle), Some(reason)) => Some(format!("{subtitle}. {reason}")),
                    (subtitle, reason) => subtitle.or(reason),
                },
                |element, description| element.aria_description(description),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.launcher.select(index);
                this.activate_selected(window, cx);
            }))
    }
}

impl Render for LauncherWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.launcher.view();
        self.keep_selected_visible(&view, window);
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
        let confirm = matches!(view.screen, Screen::Confirm { .. });
        let details: Vec<_> = view
            .details()
            .iter()
            .enumerate()
            .map(|(index, line)| {
                div()
                    .id(("detail", index))
                    .debug_selector(|| format!("detail-{line}"))
                    .text_sm()
                    .text_color(rgb(0xaab4c0))
                    .child(line.clone())
            })
            .collect();
        // A command's search that failed lists nothing; its error says why,
        // not "No results".
        let search_failed = matches!(
            (&view.screen, &view.status),
            (Screen::CommandSearch { .. }, Status::Error(_))
        );
        let (status_selector, status_text, status_color): (&str, SharedString, u32) =
            match view.status {
                Status::Idle => ("status-idle", hint.into(), 0x8a96a3),
                Status::Running => ("status-running", "Running…".into(), 0xd6c27a),
                Status::Progress(work) => ("status-progress", work.into(), 0xd6c27a),
                Status::Result(answer) => ("status-result", answer.into(), 0x9fd8a8),
                Status::Error(message) => ("status-error", message.into(), 0xf08c8c),
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
            .gap_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            // Above the rows: with none selected, the only rows are root
            // search's fallbacks, listed below "No results".
            .when(view.selected.is_none(), |rows| {
                rows.child(empty.text_color(rgb(0x8a96a3)))
            })
            .children(rows);
        // The launcher decides what an item opens; its screen says which.
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

        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::back))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .bg(rgb(0x20252d))
            .text_color(rgb(0xf1f3f5))
            .child(div().text_xl().child(view.title.clone()))
            // A confirmation's long details scroll within 40% of the
            // window, leaving the rest to its choices, which stay visible.
            .when(!details.is_empty(), |root| {
                root.child(
                    div()
                        .id("details")
                        .flex()
                        .flex_col()
                        .gap_2()
                        .when(confirm, |details| {
                            details
                                .flex_shrink(1.)
                                .max_h(relative(0.4))
                                .overflow_y_scroll()
                        })
                        .children(details),
                )
            })
            .child(body)
            .child(
                div()
                    .id("status")
                    .role(Role::Status)
                    .aria_label(status_text.clone())
                    .debug_selector(|| status_selector.into())
                    .text_sm()
                    .text_color(rgb(status_color))
                    .child(status_text),
            )
    }
}
