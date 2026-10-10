//! The rule that says whether a window is one Alt+Tab would show (see
//! the parent module): a pure function over the facts the Windows
//! adapter reads of a window, compiled and tested on every system. It
//! keeps an ordinary window — visible, unowned, titled, of an
//! application's own — and drops the helpers (tool windows, "no
//! activate", owned dialogs, cloaked windows), the shell's own surfaces,
//! and a Store app's inner core window, keeping its frame; a window the
//! shell cloaked because it is on another virtual desktop counts only
//! where the user's Alt+Tab shows all desktops, and is marked as
//! elsewhere.

use crate::system::front::{WindowFacts, is_shell_surface};

/// What the adapter reads of one window: everything the rule decides
/// with, and nothing of what it shows.
pub struct Attributes {
    /// Whether the window is visible (`IsWindowVisible`).
    pub visible: bool,
    /// Whether another top-level window owns this one, as a dialog is
    /// owned by the window that opened it (`GetWindow(GW_OWNER)`).
    pub owned: bool,
    /// Whether the window has the "no activate" style
    /// (`WS_EX_NOACTIVATE`), as a window that must never take the focus
    /// does.
    pub no_activate: bool,
    /// Whether the window has the tool-window style
    /// (`WS_EX_TOOLWINDOW`), as the shell's helper windows do.
    pub tool: bool,
    /// Whether the application forced the window onto the taskbar
    /// (`WS_EX_APPWINDOW`): a tool window that is forced there is shown,
    /// as Alt+Tab shows it.
    pub forced: bool,
    /// Whether the application removed the window from the taskbar
    /// (`ITaskbarList::DeleteTab`, whose mark the shell leaves on the
    /// window as a property): not one to switch to.
    pub removed: bool,
    /// Whether the application cloaked the window (`DWMWA_CLOAKED`'s
    /// application flag): hidden on purpose, as a suspended Store app's
    /// is.
    pub cloaked: bool,
    /// Whether the shell cloaked the window (`DWMWA_CLOAKED`'s shell
    /// flag), as it does one on another virtual desktop.
    pub shell_cloaked: bool,
    /// Whether the documented virtual desktop manager says the window is
    /// on another virtual desktop than the one shown
    /// (`IVirtualDesktopManager::IsWindowOnCurrentDesktop`).
    pub elsewhere: bool,
    /// The window's class name, as `GetClassNameW` reads it.
    pub class: String,
    /// The full path of the program of the window's process, if it could
    /// be read.
    pub program: Option<String>,
    /// Whether the window's process is Pane's own: Pane's windows (the
    /// launcher's) are never listed.
    pub own: bool,
    /// The window's title, as the system shows it ("notes.txt -
    /// Notepad").
    pub title: String,
}

/// What the user's Alt+Tab says, as the adapter read it.
pub struct Scope {
    /// Whether the user's Alt+Tab shows the windows of every virtual
    /// desktop, not only the one shown.
    pub all_desktops: bool,
}

/// The program that shows a hosted Store app's frame, whose window is
/// kept while the app's inner core window is not.
const FRAME_HOST: &str = "applicationframehost.exe";

/// Whether `window` counts as one Alt+Tab would show, and whether it is
/// on another virtual desktop (the record's `elsewhere`): `None` when it
/// does not count. See the module doc for the rule; the Windows adapter
/// reads the facts.
pub fn counts(window: &Attributes, scope: &Scope) -> Option<bool> {
    // An invisible window is not shown, and one another window owns is
    // its dialog or helper, listed with its owner's application if at
    // all.
    if !window.visible || window.owned {
        return None;
    }
    // A window that cannot be activated is not switched to; a tool
    // window is a helper the application did not force onto the taskbar;
    // and one the application removed from the taskbar is not one to
    // switch to either.
    if window.no_activate || (window.tool && !window.forced) || window.removed {
        return None;
    }
    // An empty title shows nothing to find the window by, and Pane's own
    // windows are never listed.
    if window.own || window.title.trim().is_empty() {
        return None;
    }
    // The shell's own surfaces (the desktop, the taskbars, Alt+Tab and
    // task view, Start, Search) are not applications, and a Store app's
    // inner core window is not either: the frame the frame host shows —
    // which carries the app's AppUserModelID — counts in its place.
    if is_shell_surface(&facts_of(window)) || inner_core_window(window) {
        return None;
    }
    // A window the application cloaked is hidden on purpose. One the
    // shell cloaked because it is on another virtual desktop counts only
    // where the user's Alt+Tab shows all desktops, and the desktop
    // manager says it is elsewhere: marked so, for the record.
    if window.cloaked {
        return None;
    }
    if window.shell_cloaked {
        return (scope.all_desktops && window.elsewhere).then_some(true);
    }
    Some(window.elsewhere)
}

/// Whether the user's Alt+Tab shows the windows of every desktop:
/// `value` is the setting's (`VirtualDesktopAltTabFilter`), `None` when
/// it is not set. 0 shows every desktop's windows; any other value, and
/// the setting's absence, filters to the desktop shown.
pub fn shows_all_desktops(value: Option<u32>) -> bool {
    value == Some(0)
}

/// `window` as the front application's rules read it (the shell's own
/// surfaces, by class, host process and style).
fn facts_of(window: &Attributes) -> WindowFacts {
    WindowFacts {
        class: window.class.clone(),
        program: window.program.clone(),
        own: window.own,
        owned: window.owned,
        tool: window.tool,
    }
}

/// Whether `window` is a Store app's inner core window: a window of the
/// CoreWindow class (`Windows.UI.Core.CoreWindow`) of a process that is
/// not the frame host that shows the app — the shell's Xaml surfaces and
/// a packaged app's own window alike. The frame the frame host shows is
/// kept.
fn inner_core_window(window: &Attributes) -> bool {
    core_class(&window.class) && file_name(window.program.as_deref()).as_deref() != Some(FRAME_HOST)
}

/// Whether `class` is the CoreWindow class, compared without case.
fn core_class(class: &str) -> bool {
    class.trim().to_lowercase().starts_with("windows.ui.core.")
}

/// The file name of the program at `program`, in lowercase: Windows
/// paths are compared without case.
fn file_name(program: Option<&str>) -> Option<String> {
    program.map(|program| {
        program
            .trim()
            .trim_end_matches(['\\', '/'])
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or_default()
            .to_lowercase()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The facts of an ordinary window: visible, unowned, titled, of an
    /// application's own.
    fn ordinary() -> Attributes {
        Attributes {
            visible: true,
            owned: false,
            no_activate: false,
            tool: false,
            forced: false,
            removed: false,
            cloaked: false,
            shell_cloaked: false,
            elsewhere: false,
            class: "Notepad".into(),
            program: Some(r"C:\Windows\System32\notepad.exe".into()),
            own: false,
            title: "notes.txt - Notepad".into(),
        }
    }

    /// Alt+Tab filtered to the desktop shown, the default.
    fn this_desktop() -> Scope {
        Scope {
            all_desktops: false,
        }
    }

    /// Alt+Tab showing every desktop's windows.
    fn every_desktop() -> Scope {
        Scope { all_desktops: true }
    }

    #[test]
    fn an_ordinary_window_counts() {
        assert_eq!(counts(&ordinary(), &this_desktop()), Some(false));
        // A window of File Explorer, in the shell's process but an
        // application of its own, and an application's dialog — not
        // owned here, so it counts as the application's own window.
        let mut explorer = ordinary();
        explorer.class = "CabinetWClass".into();
        explorer.program = Some(r"C:\Windows\explorer.exe".into());
        assert_eq!(counts(&explorer, &this_desktop()), Some(false));
    }

    #[test]
    fn an_invisible_window_does_not_count() {
        let mut hidden = ordinary();
        hidden.visible = false;
        assert_eq!(counts(&hidden, &this_desktop()), None);
    }

    #[test]
    fn an_owned_window_does_not_count() {
        let mut dialog = ordinary();
        dialog.owned = true;
        assert_eq!(counts(&dialog, &this_desktop()), None);
    }

    #[test]
    fn a_window_that_cannot_be_activated_does_not_count() {
        let mut passive = ordinary();
        passive.no_activate = true;
        assert_eq!(counts(&passive, &this_desktop()), None);
    }

    #[test]
    fn a_tool_window_counts_only_when_forced_onto_the_taskbar() {
        let mut helper = ordinary();
        helper.tool = true;
        assert_eq!(counts(&helper, &this_desktop()), None);
        // Forced onto the taskbar (WS_EX_APPWINDOW), Alt+Tab shows it.
        helper.forced = true;
        assert_eq!(counts(&helper, &this_desktop()), Some(false));
    }

    #[test]
    fn a_window_removed_from_the_taskbar_does_not_count() {
        let mut removed = ordinary();
        removed.removed = true;
        assert_eq!(counts(&removed, &this_desktop()), None);
    }

    #[test]
    fn a_window_without_a_title_does_not_count() {
        for title in ["", " "] {
            let mut untitled = ordinary();
            untitled.title = title.into();
            assert_eq!(counts(&untitled, &this_desktop()), None, "{title:?}");
        }
    }

    #[test]
    fn pane_s_own_windows_do_not_count() {
        let mut own = ordinary();
        own.own = true;
        own.class = "PaneLauncher".into();
        own.program = Some(r"C:\Apps\Pane\pane.exe".into());
        assert_eq!(counts(&own, &this_desktop()), None);
    }

    #[test]
    fn the_shell_s_own_windows_do_not_count() {
        for class in [
            "Progman",
            "WorkerW",
            "SHELLDLL_DefView",
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
            "NotifyIconOverflowWindow",
            "TaskSwitcherWnd",
            "TaskListThumbnailWnd",
            "XamlExplorerHostIslandWindow",
            "MultitaskingViewFrame",
            "LockScreenBackstopFrame",
        ] {
            // Whatever process the shell's window runs in, and whatever
            // its case.
            let mut surface = ordinary();
            surface.class = class.into();
            surface.program = Some(r"C:\Windows\explorer.exe".into());
            assert_eq!(counts(&surface, &this_desktop()), None, "{class}");
            let mut lowered = ordinary();
            lowered.class = class.to_lowercase();
            assert_eq!(counts(&lowered, &this_desktop()), None, "{class}");
        }
        // The shell's Xaml surfaces (Start, Search) are CoreWindows of
        // the shell's own hosts.
        let mut start = ordinary();
        start.class = "Windows.UI.Core.CoreWindow".into();
        start.program = Some(r"C:\Windows\SystemApps\Search\SearchHost.exe".into());
        assert_eq!(counts(&start, &this_desktop()), None);
    }

    #[test]
    fn a_store_app_s_inner_core_window_does_not_count_but_its_frame_does() {
        // The app's own window, in its own packaged process: the inner
        // core window.
        let mut inner = ordinary();
        inner.class = "Windows.UI.Core.CoreWindow".into();
        inner.program = Some(r"C:\Program Files\WindowsApps\Calculator\Calculator.exe".into());
        assert_eq!(counts(&inner, &this_desktop()), None);
        // The frame the frame host shows, which carries the app's
        // AppUserModelID, counts in its place.
        let mut frame = ordinary();
        frame.class = "Windows.UI.Core.CoreWindow".into();
        frame.program = Some(r"C:\Windows\System32\ApplicationFrameHost.exe".into());
        assert_eq!(counts(&frame, &this_desktop()), Some(false));
    }

    #[test]
    fn a_window_the_application_cloaked_does_not_count() {
        let mut suspended = ordinary();
        suspended.cloaked = true;
        assert_eq!(counts(&suspended, &this_desktop()), None);
        // Even where Alt+Tab shows all desktops: the application hid it.
        assert_eq!(counts(&suspended, &every_desktop()), None);
    }

    #[test]
    fn a_window_on_another_desktop_counts_only_where_alt_tab_shows_all() {
        let mut elsewhere = ordinary();
        elsewhere.shell_cloaked = true;
        elsewhere.elsewhere = true;
        // Filtered to the desktop shown: not listed.
        assert_eq!(counts(&elsewhere, &this_desktop()), None);
        // Showing every desktop: listed, marked as elsewhere.
        assert_eq!(counts(&elsewhere, &every_desktop()), Some(true));
        // Cloaked by the shell but on the desktop shown, or not said to
        // be elsewhere: an odd state, not one to list.
        let mut here = ordinary();
        here.shell_cloaked = true;
        here.elsewhere = false;
        assert_eq!(counts(&here, &every_desktop()), None);
    }

    #[test]
    fn a_window_the_desktop_manager_says_is_elsewhere_is_marked_so() {
        let mut elsewhere = ordinary();
        elsewhere.elsewhere = true;
        assert_eq!(counts(&elsewhere, &every_desktop()), Some(true));
    }

    #[test]
    fn the_alt_tab_setting_shows_all_desktops_only_when_it_says_so() {
        assert!(shows_all_desktops(Some(0)));
        assert!(!shows_all_desktops(Some(1)));
        // The setting's absence filters to the desktop shown.
        assert!(!shows_all_desktops(None));
    }
}
