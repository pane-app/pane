//! The Logs screen (#213): a developed package's extension log in the
//! launcher window, titled "Logs for <title>", reached from its
//! development row among its operations (the Actions menu of its page in
//! Settings, and the extension list the tests show) and from its build's
//! details ("Why <title> did not build").
//!
//! The launcher holds the screen ([`pane_core::Screen::ExtensionLog`]); the
//! window draws the log's lines ([`pane_core::Launcher::extension_log`]),
//! read again whenever the launcher says it changed, which it does as a
//! developed package's log grows while its Logs screen shows. The window
//! owns what the user does with them:
//!
//! - Each line shows its local time, its level and who wrote it, the
//!   extension or Pane, then its text. Errors and warnings are in their
//!   tones and debug lines muted; Pane's own lines carry the accent's mark
//!   on their left edge and say "Pane" in it.
//! - The list follows new lines while it is at its end, the last line
//!   selected. Scrolling up, or moving the selection up, stops following;
//!   coming back to the end (scrolling down to it, End, or the last line
//!   selected) follows again.
//! - Enter or Ctrl+C copies the selected line and Ctrl+Shift+C every line,
//!   as the log file writes them; Ctrl+L clears the lines Pane keeps (the
//!   log file keeps them); Ctrl+O opens the log file. On macOS, Command
//!   takes Ctrl's place. The buttons above the list do the same.
//!
//! Opened from Settings, the screen brings the launcher window forward.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Div, FollowMode, KeyBinding, Role,
    SharedString, Stateful, Window, actions, canvas, div, prelude::*, px,
};
use pane_core::clipboard_view::local_offset_ms;
use pane_core::extension_log::{LogLevel, LogLine, LogSource};
use pane_core::{Binding, Launcher, PackageIdentity, Screen, Status};

use crate::app::{LauncherWindow, Spot};
use crate::ui::footer::{self, ButtonWash};
use crate::ui::keycap::{CapStyle, KeySequence};
use crate::ui::theme::{Theme, faded};
use crate::ui::virtual_list::{self, VirtualList};
use crate::{Confirm, SelectNext, SelectNextPage, SelectPrevious, SelectPreviousPage};

actions!(
    extension_log,
    [
        CopyLine,
        CopyAllLines,
        ClearLog,
        OpenLogFile,
        FirstLine,
        LastLine
    ]
);

/// The Logs screen's key context, under the launcher's.
pub(crate) const CONTEXT: &str = "ExtensionLog";

/// The height of a line of the log: one line of text, so the list knows
/// every line's height before it draws it.
const LINE_HEIGHT: f32 = 24.;

/// `key` with the modifier Pane's own commands take on this system:
/// Command on macOS, Ctrl elsewhere (`ctrl-c`).
fn chord(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}

/// The keys of [`chord`]`(key)`, as the screen's buttons show them.
fn chord_keys(key: &str) -> KeySequence {
    crate::keyboard::binding_keys(&Binding::parse(&chord(key)).expect("a chord binds"))
}

/// Registers the screen's keys: copying the selected line and every line,
/// clearing the log, opening its file, and the first and last lines.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(&chord("c"), CopyLine, Some(CONTEXT)),
        KeyBinding::new(&chord("shift-c"), CopyAllLines, Some(CONTEXT)),
        KeyBinding::new(&chord("l"), ClearLog, Some(CONTEXT)),
        KeyBinding::new(&chord("o"), OpenLogFile, Some(CONTEXT)),
        KeyBinding::new("home", FirstLine, Some(CONTEXT)),
        KeyBinding::new("end", LastLine, Some(CONTEXT)),
    ]);
}

/// The Logs screen's state, owned by the launcher window while the
/// launcher shows the screen.
pub(crate) struct ExtensionLogView {
    identity: PackageIdentity,
    /// The lines, as the window last read them, oldest first.
    lines: Vec<LogLine>,
    selected: Option<usize>,
    /// Whether the list follows new lines, the last selected.
    following: bool,
    /// The list, drawn virtually (#165): only the lines in view are laid
    /// out and painted.
    list: VirtualList,
    /// Whether the next frame scrolls the list to the selected line.
    reveal: bool,
    /// The local time's offset from UTC, as the screen opened.
    offset_ms: i64,
    /// Whether the status line says what one of the screen's actions came
    /// to, which goes once the user moves on.
    outcome: bool,
}

impl ExtensionLogView {
    fn new(identity: PackageIdentity) -> ExtensionLogView {
        let list = VirtualList::new(px(LINE_HEIGHT));
        list.state().set_follow_mode(FollowMode::Tail);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        ExtensionLogView {
            identity,
            lines: Vec::new(),
            selected: None,
            following: true,
            list,
            reveal: false,
            offset_ms: local_offset_ms(now),
            outcome: false,
        }
    }

    /// Reads the lines again from `launcher`, keeping the list where it
    /// is: lines added after the others are added to it, and while it
    /// follows, the last is selected; otherwise the same line stays
    /// selected, while Pane keeps it.
    fn read(&mut self, launcher: &Launcher) {
        self.sync_following();
        let lines = launcher.extension_log(&self.identity);
        if lines == self.lines {
            return;
        }
        let kept = self.lines.len();
        let added = kept > 0
            && lines.len() > kept
            && lines.first() == self.lines.first()
            && lines.get(kept - 1) == self.lines.last();
        let selected = self
            .selected
            .and_then(|index| self.lines.get(index))
            .cloned();
        self.lines = lines;
        if added {
            self.list.append(self.lines.len() - kept);
        } else {
            self.list.reset(self.lines.len());
            self.reveal = true;
        }
        let first = (!self.lines.is_empty()).then_some(0);
        self.selected = if self.following {
            self.lines.len().checked_sub(1)
        } else {
            selected
                .and_then(|line| self.lines.iter().rposition(|kept| *kept == line))
                .or(first)
        };
    }

    /// Takes the list's own following as the screen's: scrolling up stops
    /// it, and scrolling back to the end starts it again, with the last
    /// line selected.
    fn sync_following(&mut self) {
        let tail = self.list.state().is_following_tail();
        if self.following && !tail {
            self.following = false;
        } else if !self.following && tail {
            self.following = true;
            self.selected = self.lines.len().checked_sub(1);
        }
    }

    /// Selects line `index`, or the last if there are fewer: the last
    /// follows new lines as they come; any other stays where it is, in
    /// view.
    fn select(&mut self, index: usize) {
        let Some(last) = self.lines.len().checked_sub(1) else {
            return;
        };
        let index = index.min(last);
        self.selected = Some(index);
        if index == last {
            self.follow();
        } else {
            self.following = false;
            self.list.state().set_follow_mode(FollowMode::Normal);
            self.reveal = true;
        }
    }

    /// Follows new lines from now on.
    fn follow(&mut self) {
        self.following = true;
        self.list.state().set_follow_mode(FollowMode::Tail);
    }

    /// Moves the selection `delta` lines.
    fn step(&mut self, delta: isize) {
        let last = self.lines.len().saturating_sub(1);
        self.select(self.selected.unwrap_or(last).saturating_add_signed(delta));
    }

    /// The selected line, as the log file writes it.
    fn selected_text(&self) -> Option<String> {
        self.selected
            .and_then(|index| self.lines.get(index))
            .map(LogLine::file_line)
    }

    /// Every line, as the log file writes them.
    fn all_text(&self) -> String {
        self.lines
            .iter()
            .map(LogLine::file_line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl LauncherWindow {
    /// Test support: how many lines the Logs screen shows, which is
    /// selected and whether it follows new lines; `None` while it does not
    /// show.
    #[doc(hidden)]
    pub fn extension_log_shown(&self) -> Option<(usize, Option<usize>, bool)> {
        self.log
            .as_ref()
            .map(|log| (log.lines.len(), log.selected, log.following))
    }

    /// Makes the Logs screen follow the launcher's: it opens, focused and
    /// with the launcher window brought forward (Settings opens it too),
    /// when the launcher shows a package's log, reads the log's lines again
    /// whenever the launcher changed, and closes once the launcher leaves
    /// the screen.
    pub(crate) fn sync_extension_log(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::ExtensionLog { identity } = self.launcher.screen() else {
            self.log = None;
            return;
        };
        let opened = self.log.as_ref().is_none_or(|log| log.identity != identity);
        if opened {
            self.log = Some(ExtensionLogView::new(identity));
        }
        if let Some(log) = self.log.as_mut() {
            log.read(&self.launcher);
        }
        if opened {
            self.unhide(window, cx);
            window.activate_window();
            cx.activate(true);
            window.focus(&self.focus_handle, cx);
        }
    }

    /// Whether the Logs screen shows.
    pub(crate) fn extension_log_open(&self) -> bool {
        self.log.is_some() && matches!(self.launcher.screen(), Screen::ExtensionLog { .. })
    }

    /// The user moved on: the selection moved by `delta` lines, and the
    /// status line is at rest again, its Copy line button back.
    fn step_log(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(log) = self.log.as_mut() {
            log.step(delta);
            self.moved_on_in_log(cx);
        }
    }

    /// Selects line `index` of the log (the last for `usize::MAX`).
    fn select_log_line(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(log) = self.log.as_mut() {
            log.select(index);
            self.moved_on_in_log(cx);
        }
    }

    /// Puts the status line at rest once the user moves on from what an
    /// action of the screen came to (and only that), and redraws.
    fn moved_on_in_log(&mut self, cx: &mut Context<Self>) {
        if let Some(log) = self.log.as_mut()
            && std::mem::take(&mut log.outcome)
        {
            self.launcher.show_status(Status::Idle);
        }
        cx.notify();
    }

    /// Says in the status line what an action of the screen came to, until
    /// the user moves on.
    fn show_log_outcome(&mut self, status: Status, cx: &mut Context<Self>) {
        self.launcher.show_status(status);
        if let Some(log) = self.log.as_mut() {
            log.outcome = true;
        }
        cx.notify();
    }

    fn log_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.step_log(1, cx);
    }

    fn log_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.step_log(-1, cx);
    }

    /// Page Down: the selection moves by the lines in view.
    fn log_next_page(&mut self, _: &SelectNextPage, _: &mut Window, cx: &mut Context<Self>) {
        let step = virtual_list::page_move(self.log.as_ref().map(|log| &log.list), true);
        self.step_log(step, cx);
    }

    /// Page Up: the selection moves back by the lines in view.
    fn log_previous_page(
        &mut self,
        _: &SelectPreviousPage,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let step = virtual_list::page_move(self.log.as_ref().map(|log| &log.list), false);
        self.step_log(step, cx);
    }

    fn log_first(&mut self, _: &FirstLine, _: &mut Window, cx: &mut Context<Self>) {
        self.select_log_line(0, cx);
    }

    fn log_last(&mut self, _: &LastLine, _: &mut Window, cx: &mut Context<Self>) {
        self.select_log_line(usize::MAX, cx);
    }

    fn log_confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_log_line(cx);
    }

    fn log_copy(&mut self, _: &CopyLine, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_log_line(cx);
    }

    fn log_copy_all(&mut self, _: &CopyAllLines, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_all_log_lines(cx);
    }

    fn log_clear(&mut self, _: &ClearLog, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_log(cx);
    }

    fn log_open_file(&mut self, _: &OpenLogFile, window: &mut Window, cx: &mut Context<Self>) {
        self.open_log_file(window, cx);
    }

    /// Copies the selected line, as the log file writes it: what Enter and
    /// the footer's Copy line do on the screen. Nothing with none.
    pub(crate) fn copy_log_line(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.log.as_ref().and_then(ExtensionLogView::selected_text) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.show_log_outcome(Status::Result("Copied the line".into()), cx);
    }

    /// Copies every line, as the log file writes them.
    fn copy_all_log_lines(&mut self, cx: &mut Context<Self>) {
        let Some(log) = self.log.as_ref() else {
            return;
        };
        let count = log.lines.len();
        let status = if count == 0 {
            Status::Error("The log has no lines to copy".into())
        } else {
            cx.write_to_clipboard(ClipboardItem::new_string(log.all_text()));
            let lines = if count == 1 { "line" } else { "lines" };
            Status::Result(format!("Copied {count} {lines}"))
        };
        self.show_log_outcome(status, cx);
    }

    /// Clears the lines Pane keeps of the log, which its log file keeps,
    /// and follows the lines written from now on.
    fn clear_log(&mut self, cx: &mut Context<Self>) {
        let Some(log) = self.log.as_mut() else {
            return;
        };
        self.launcher.clear_extension_log(&log.identity);
        log.follow();
        log.read(&self.launcher);
        let cleared = "Cleared the log; its log file keeps every line";
        self.show_log_outcome(Status::Result(cleared.into()), cx);
    }

    /// Opens the log file with the system's handler for it, saying what
    /// that came to while the screen still shows.
    fn open_log_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(identity) = self.log.as_ref().map(|log| log.identity.clone()) else {
            return;
        };
        let pending = self.launcher.open_extension_log_file(&identity);
        cx.spawn_in(window, async move |this, cx| {
            let status = match pending.await {
                Ok(done) => Status::Result(done),
                Err(why) => Status::Error(why),
            };
            this.update(cx, |this, cx| {
                let shown = this.log.as_ref().map(|log| &log.identity) == Some(&identity);
                if shown {
                    this.show_log_outcome(status, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The Logs screen's body, under its heading: the buttons, then the
    /// lines, drawn virtually, or what it says while there are none. It
    /// holds the keyboard's focus. `None` until the screen is read
    /// ([`LauncherWindow::sync_extension_log`]).
    pub(crate) fn render_extension_log(&mut self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        // The buttons' hover looks are read before the log is borrowed: a
        // button's wash fades behind the pointer, and the log's records
        // are read (and scrolled) while the buttons are drawn.
        let now = cx.background_executor().now();
        let look = |id: &'static str| self.motion.hover.look(Spot::Button(id), now);
        let (copy_all_look, clear_look, open_look) = (
            look("log-copy-all"),
            look("log-clear"),
            look("log-open-file"),
        );
        let theme = crate::settings::launcher_visuals(cx).theme;
        let log = self.log.as_mut()?;
        log.sync_following();
        if log.list.count() != log.lines.len() {
            log.list.reset(log.lines.len());
            log.reveal = true;
        }
        if std::mem::take(&mut log.reveal)
            && !log.following
            && let Some(selected) = log.selected
        {
            log.list.reveal(selected);
        }
        let empty = log.lines.is_empty();
        let list_state = log.list.state().clone();
        // The list starts following again once scrolled back to its end,
        // which it finds as it is laid out: the next frame takes that up.
        let laid_out = log.list.state().clone();
        let following = log.following;
        let follow_check = canvas(
            move |_, window, _| {
                if laid_out.is_following_tail() != following {
                    window.request_animation_frame();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0();
        let copy_all = cx.listener(|this, _: &ClickEvent, _, cx| this.copy_all_log_lines(cx));
        let clear = cx.listener(|this, _: &ClickEvent, _, cx| this.clear_log(cx));
        let open = cx.listener(|this, _: &ClickEvent, window, cx| this.open_log_file(window, cx));
        let buttons = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(theme.geometry.search_padding_x)
            .pb(px(6.))
            .child(log_button(
                "log-copy-all",
                "Copy All",
                "shift-c",
                copy_all_look,
                copy_all,
                cx,
                &theme,
            ))
            .child(log_button(
                "log-clear",
                "Clear",
                "l",
                clear_look,
                clear,
                cx,
                &theme,
            ))
            .child(log_button(
                "log-open-file",
                "Open Log File",
                "o",
                open_look,
                open,
                cx,
                &theme,
            ));
        let lines = if empty {
            div()
                .debug_selector(|| "log-empty".into())
                .px(theme.geometry.search_padding_x)
                .py(theme.geometry.screen_padding_y)
                .text_size(theme.typography.row_subtitle_size)
                .text_color(theme.text_muted)
                .child(
                    "Nothing is logged yet. What the extension writes, and Pane's messages \
                     about it, show here as they come.",
                )
                .into_any_element()
        } else {
            div()
                .id("log-lines")
                .debug_selector(|| "log-lines".into())
                .role(Role::ListBox)
                .aria_label("Log lines")
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                .child(
                    gpui::list(
                        list_state,
                        cx.processor(|this, index, _: &mut Window, cx| {
                            this.render_log_line(index, cx)
                        }),
                    )
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .pb(theme.geometry.list_padding_bottom),
                )
                .child(follow_check)
                .into_any_element()
        };
        Some(
            div()
                .id("extension-log")
                .debug_selector(|| "extension-log".into())
                .key_context(CONTEXT)
                .track_focus(&self.focus_handle)
                .on_action(cx.listener(Self::log_next))
                .on_action(cx.listener(Self::log_previous))
                .on_action(cx.listener(Self::log_next_page))
                .on_action(cx.listener(Self::log_previous_page))
                .on_action(cx.listener(Self::log_first))
                .on_action(cx.listener(Self::log_last))
                .on_action(cx.listener(Self::log_confirm))
                .on_action(cx.listener(Self::log_copy))
                .on_action(cx.listener(Self::log_copy_all))
                .on_action(cx.listener(Self::log_clear))
                .on_action(cx.listener(Self::log_open_file))
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                .child(buttons)
                .child(lines),
        )
    }

    /// The log's line `index` as the list draws it: its time, level and
    /// who wrote it, then its text, in its level's tone; Pane's own with
    /// the accent's mark. A click selects it.
    fn render_log_line(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(log) = self.log.as_ref() else {
            return div().into_any_element();
        };
        let Some(line) = log.lines.get(index) else {
            return div().into_any_element();
        };
        let theme = crate::settings::launcher_visuals(cx).theme;
        let selected = log.selected == Some(index);
        let hover = self
            .motion
            .hover
            .look(Spot::Log(index), cx.background_executor().now());
        let pane = line.source == LogSource::Pane;
        let tone = level_tone(line.level, &theme);
        let time = clock(line.time, log.offset_ms);
        let level = level_word(line.level);
        let who = if pane { "Pane" } else { "Extension" };
        // A message of Pane's may hold a backtrace: its first line shows,
        // and copying it copies all of it.
        let text: SharedString = match line.text.split_once('\n') {
            Some((first, _)) => format!("{first} …").into(),
            None => line.text.clone().into(),
        };
        let label = format!("{time} {level} {who}: {}", line.text);
        let source = if pane { "pane" } else { "extension" };
        div()
            .id(("log-line", index))
            .debug_selector(move || format!("log-line-{index}"))
            .h(px(LINE_HEIGHT))
            .mx(theme.geometry.list_padding_x)
            .px(theme.geometry.row_padding_x)
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(theme.geometry.row_radius)
            .border_l(px(2.))
            .border_color(if pane {
                theme.accent_text
            } else {
                gpui::transparent_black()
            })
            .whitespace_nowrap()
            .text_size(theme.typography.row_kind_size)
            .when(selected, |row| row.bg(theme.selection_wash))
            .when(!selected && hover > 0., |row| {
                row.bg(faded(theme.hover_wash, hover))
            })
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                this.motion.hover.set(Spot::Log(index), *over, cx);
            }))
            .child(
                div()
                    .debug_selector(move || format!("log-time-{index}"))
                    .flex_none()
                    .font_family(theme.typography.mono_family.clone())
                    .text_color(theme.text_muted)
                    .child(time),
            )
            .child(
                div()
                    .debug_selector(move || format!("log-{level}-{index}"))
                    .flex_none()
                    .w(px(44.))
                    .font_weight(theme.typography.medium)
                    .text_color(tone)
                    .child(level),
            )
            .child(
                div()
                    .debug_selector(move || format!("log-{source}-{index}"))
                    .flex_none()
                    .w(px(64.))
                    .font_weight(theme.typography.medium)
                    .text_color(if pane {
                        theme.accent_text
                    } else {
                        theme.text_muted
                    })
                    .child(who),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_color(tone)
                    .child(text),
            )
            .role(Role::ListBoxOption)
            .aria_label(label)
            .aria_selected(selected)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(log.lines.len())
            .when(selected, |row| row.aria_active_descendant())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.select_log_line(index, cx);
            }))
            .into_any_element()
    }
}

/// A button over the log's lines: `label` and the keys of its
/// [`chord`]`(key)`, in the footer buttons' chrome, doing `clicked`.
/// `look` is the button's hover wash strength (see `crate::app::hover_wash`),
/// read from the window's hover state; the button reports the pointer's
/// arrivals and departures itself, by its `id`.
fn log_button(
    id: &'static str,
    label: &'static str,
    key: &str,
    look: f32,
    clicked: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &Context<LauncherWindow>,
    theme: &Theme,
) -> Stateful<Div> {
    let keys = chord_keys(key);
    let spot = Spot::Button(id);
    footer::footer_button(
        id,
        label,
        &keys,
        CapStyle::Regular,
        ButtonWash::Hover(look),
        theme,
    )
    .role(Role::Button)
    .aria_label(label)
    .aria_keyshortcuts(keys.name())
    .cursor_pointer()
    .on_hover(cx.listener(move |this, over: &bool, _, cx| {
        this.motion.hover.set(spot, *over, cx);
    }))
    .on_click(clicked)
}

/// The tone a line of `level` is drawn in.
fn level_tone(level: LogLevel, theme: &Theme) -> gpui::Hsla {
    match level {
        LogLevel::Error => theme.danger,
        LogLevel::Warn => theme.warning,
        LogLevel::Info => theme.text_title,
        LogLevel::Debug => theme.text_muted,
    }
}

/// A level as its line names it.
fn level_word(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "Error",
        LogLevel::Warn => "Warn",
        LogLevel::Info => "Info",
        LogLevel::Debug => "Debug",
    }
}

/// `time` as a local clock with milliseconds, `offset_ms` from UTC:
/// "14:03:07.215".
fn clock(time: SystemTime, offset_ms: i64) -> String {
    let since = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64);
    let of_day = (since + offset_ms).rem_euclid(86_400_000);
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        of_day / 3_600_000,
        of_day / 60_000 % 60,
        of_day / 1_000 % 60,
        of_day % 1_000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_s_time_is_its_local_clock() {
        let time = UNIX_EPOCH + std::time::Duration::from_millis(1_760_000_000_123);
        assert_eq!(clock(time, 0), "08:53:20.123");
        assert_eq!(clock(time, 2 * 3_600_000), "10:53:20.123");
        assert_eq!(clock(time, -9 * 3_600_000), "23:53:20.123");
    }
}
