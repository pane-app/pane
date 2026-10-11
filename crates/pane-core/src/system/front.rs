//! The application that was in front before Pane (#125, ADR 0039): the
//! pure rules the Windows watcher decides with, compiled and tested on
//! every system. They say whether a window that came to the front is
//! the target paste reaches — not one of Pane's own windows, and not
//! one of the shell's surfaces (the taskbars and their overflow, the
//! desktop, Alt+Tab and task view, Start, Search, the lock screen, the
//! input and notification hosts, Widgets) — from the window's class
//! name, its process, its ownership and its style; and they resolve a
//! window Pane did record to the application it belongs to,
//! best-effort, through its AppUserModelID, its program's path or its
//! process's package identity matched against the installed
//! applications (ADR 0038), which the "Applications done properly"
//! specification (#124) refines later. The Switch Windows host functions
//! (#263) reuse the same rules and the same resolution: which windows
//! Alt+Tab would show is decided with `is_shell_surface`, and each
//! window's application is `front_application`. The Windows half — the
//! system foreground hook and the window facts it reads — is in `windows`
//! beside this.

// The rules are tested on every system, and the Switch Windows host
// functions reach them too; only the watcher is Windows'.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

#[cfg(target_os = "windows")]
pub(crate) mod windows;

use super::FrontApplication;
use crate::applications::identity::{Catalog, Key};

/// What the watcher reads of a window that came to the front: enough to
/// say whether it is a paste target, and nothing of what it shows.
pub(crate) struct WindowFacts {
    /// The window's class name, as `GetClassNameW` reads it.
    pub class: String,
    /// The full path of the program of the window's process, if it could
    /// be read; its file name says whose window it is.
    pub program: Option<String>,
    /// Whether the window's process is Pane's own: Pane's windows (the
    /// launcher's) are never the target.
    pub own: bool,
    /// Whether another top-level window owns this one, as a dialog is
    /// owned by the window that opened it.
    pub owned: bool,
    /// Whether the window has the tool-window style
    /// (`WS_EX_TOOLWINDOW`), as the shell's helper windows do.
    pub tool: bool,
}

/// The window classes of the shell's own surfaces, in lowercase — the
/// class name is compared in lowercase: a window of one of them in front
/// is never the application the user was in.
const SHELL_CLASSES: &[&str] = &[
    // The taskbars, and their notification areas' overflow.
    "shell_traywnd",
    "shell_secondarytraywnd",
    "notifyiconoverflowwindow",
    "toplevelwindowforoverflowxamlisland",
    // The desktop: the window behind its icons, and the icons' view.
    "progman",
    "workerw",
    "shelldll_defview",
    // Alt+Tab's switcher and task view.
    "taskswitcherwnd",
    "tasklistthumbnailwnd",
    "xamlexplorerhostislandwindow",
    "multitaskingviewframe",
    // The lock screen.
    "lockscreenbackstopframe",
];

/// The programs that host the shell's Xaml surfaces: a window of the
/// `Windows.UI.Core.CoreWindow` class of one of them in front is one of
/// the shell's own (Start, Search, the lock screen, the input and
/// notification hosts, Widgets), while the same class of another
/// process — a packaged application, or the frame host that shows one —
/// is the application itself.
const SHELL_HOSTS: &[&str] = &[
    "explorer.exe",
    "searchhost.exe",
    "searchapp.exe",
    "searchui.exe",
    "lockapp.exe",
    "lockapphost.exe",
    "textinputhost.exe",
    "ctfmon.exe",
    "shellexperiencehost.exe",
    "widgets.exe",
    "widgetservice.exe",
];

/// The file name of the program at `program`, a path, in lowercase:
/// Windows paths are compared without case.
fn file_name(program: &str) -> String {
    program
        .trim()
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

/// Whether `program`, the file name of a window's process's program, is
/// one of the shell hosts': the programs the shell's own surfaces run
/// in.
fn is_host(program: String) -> bool {
    SHELL_HOSTS.contains(&program.as_str())
}

/// Whether a window with `facts` is one of the shell's surfaces, which
/// paste must skip: by its class, by the class and host process of a
/// shell Xaml surface, or by being an owned tool window — a helper, as
/// the taskbar's flyouts are. An application's own dialogs, which are
/// owned but not tool windows, stay targets.
pub(crate) fn is_shell_surface(facts: &WindowFacts) -> bool {
    let class = facts.class.trim().to_lowercase();
    if SHELL_CLASSES.contains(&class.as_str()) {
        return true;
    }
    // A shell host's Xaml surface: the `CoreWindow` class of one of the
    // programs the shell's own surfaces run in.
    let host = facts.program.as_deref().map(file_name).is_some_and(is_host);
    if class == "windows.ui.core.corewindow" && host {
        return true;
    }
    facts.owned && facts.tool
}

/// Whether a window with `facts` is the target paste reaches: not one of
/// Pane's own windows, and not one of the shell's surfaces.
fn is_target(facts: &WindowFacts) -> bool {
    !facts.own && !is_shell_surface(facts)
}

/// A window the watcher recorded, as the front application and the paste
/// read it: its title and its own AppUserModelID read when asked, its
/// program's path and its package family as the watcher read them.
pub(crate) struct Target {
    /// The window's title, as the system shows it ("notes.txt -
    /// Notepad").
    pub title: String,
    /// The window's own AppUserModelID, if it has one: what the taskbar
    /// groups and launches it by, which a packaged application or a web
    /// app has and a plain desktop program usually does not.
    pub aumid: Option<String>,
    /// The full path of the program of the window's process, if it could
    /// be read.
    pub program: Option<String>,
    /// The package family name of the window's process, if it has one.
    pub family: Option<String>,
}

/// The application `target`'s window belongs to — its name and its icon —
/// best-effort, resolved against the `installed` applications
/// ([`Catalog`]) in the order the specification names: the window's own
/// AppUserModelID first, then its program's path, then its process's
/// package identity. A match names the application as the system does;
/// without one, the window's title stands in, and the icon is what the
/// system's icon of it is: the `shell:AppsFolder` name of its
/// AppUserModelID, or its program's path.
pub(crate) fn front_application(target: &Target, installed: &Catalog) -> FrontApplication {
    if let Some(aumid) = trimmed(target.aumid.as_deref()) {
        let reference = format!("shell:AppsFolder\\{aumid}");
        let name = installed
            .find(&reference)
            .map(|found| found.primary().name.clone())
            .unwrap_or_else(|| named(target));
        return FrontApplication {
            name,
            icon: Some(reference),
        };
    }
    if let Some(program) = trimmed(target.program.as_deref()) {
        let id = Key::program(program, "").id();
        if let Some(name) = installed
            .find(&id)
            .map(|found| found.primary().name.clone())
        {
            return FrontApplication {
                name,
                icon: Some(program.to_owned()),
            };
        }
    }
    if let Some(family) = trimmed(target.family.as_deref()) {
        let key = Key::Package {
            family: family.to_lowercase(),
            app: None,
        };
        if let Some(found) = installed.find(&key.id()) {
            // The application's own source is an Apps Folder name, which
            // the system's icon of it is; else its program stands in.
            let primary = found.primary();
            let icon = if apps_folder(&primary.path) {
                Some(primary.path.clone())
            } else {
                target.program.clone()
            };
            return FrontApplication {
                name: primary.name.clone(),
                icon,
            };
        }
    }
    FrontApplication {
        name: named(target),
        icon: target.program.clone(),
    }
}

/// `text` trimmed, and `None` when nothing is left.
fn trimmed(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}

/// Whether `path` is the Apps Folder name of a packaged application,
/// which the system's icon of it is.
fn apps_folder(path: &str) -> bool {
    path.to_lowercase().starts_with("shell:appsfolder\\")
}

/// The name of the application `target`'s window stands for when no
/// installed application matches: its title, else its program's name.
fn named(target: &Target) -> String {
    let title = target.title.trim();
    if !title.is_empty() {
        return title.to_owned();
    }
    target
        .program
        .as_deref()
        .and_then(crate::applications::names::program_name)
        .unwrap_or_else(|| "the application in front".into())
}

/// What [`super::System::can_paste`] answers when no window has been
/// tracked as the target: nothing to paste into.
pub(super) const NO_TARGET: &str = "There is no application in front to paste into";

/// Why a paste into the tracked target did not happen (#125): each
/// answers the command with a failure saying so. What Pane put on the
/// clipboard stays there, still tagged, so the user can paste it by
/// hand; what it held before is not put back.
pub(crate) enum PasteRefusal {
    /// The window is gone: its application closed.
    Gone,
    /// The window is not responding, and would not take the keys.
    NotResponding,
    /// The window's process is running as administrator, which would
    /// drop the keys Pane sends.
    Elevated,
    /// The window did not come back to the front.
    NotFront,
    /// A newer paste request superseded this one.
    Superseded,
    /// The paste worker took longer than Pane waits for it.
    TookTooLong,
}

impl PasteRefusal {
    /// What the command is told, for the user.
    pub(super) fn message(&self) -> String {
        match self {
            PasteRefusal::Gone => "The application that was in front is gone; the text is \
                 still on the clipboard"
                .into(),
            PasteRefusal::NotResponding => "The application that was in front is not \
                 responding; the text is still on the clipboard"
                .into(),
            PasteRefusal::Elevated => "The application that was in front is running as \
                 administrator, which Pane cannot paste into; the text is still on the \
                 clipboard"
                .into(),
            PasteRefusal::NotFront => "The application that was in front did not come back \
                 to the front; the text is still on the clipboard"
                .into(),
            // The newer paste owns the clipboard now.
            PasteRefusal::Superseded => "A newer paste replaced this one".into(),
            PasteRefusal::TookTooLong => "Pane waited too long for the application in \
                 front; the text is still on the clipboard"
                .into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::applications::identity::Source;

    /// The facts of a window: its class, its process's program, and
    /// whether it is Pane's own, owned, and a tool window.
    fn facts(class: &str, program: Option<&str>) -> WindowFacts {
        WindowFacts {
            class: class.into(),
            program: program.map(str::to_owned),
            own: false,
            owned: false,
            tool: false,
        }
    }

    /// The facts of a window of Pane's own process.
    fn own(facts: WindowFacts) -> WindowFacts {
        WindowFacts { own: true, ..facts }
    }

    /// The facts of an owned tool window.
    fn helper(facts: WindowFacts) -> WindowFacts {
        WindowFacts {
            owned: true,
            tool: true,
            ..facts
        }
    }

    #[test]
    fn a_plain_application_window_is_the_target() {
        let notepad = facts("Notepad", Some(r"C:\Windows\System32\notepad.exe"));
        assert!(is_target(&notepad));
        // File Explorer runs in the shell's process, but its windows are
        // the application, not the shell's surfaces.
        let explorer = facts("CabinetWClass", Some(r"C:\Windows\explorer.exe"));
        assert!(is_target(&explorer));
        // A packaged application's own CoreWindow, in its process or in
        // the frame host that shows one, is the application itself.
        let packaged = facts(
            "Windows.UI.Core.CoreWindow",
            Some(r"C:\Windows\System32\ApplicationFrameHost.exe"),
        );
        assert!(is_target(&packaged));
        // An application's dialog: owned, but not a tool window.
        let dialog = facts("#32770", Some(r"C:\Apps\app.exe"));
        assert!(is_target(&dialog));
    }

    #[test]
    fn the_shell_s_surfaces_are_never_the_target() {
        for class in [
            "Progman",
            "WorkerW",
            "SHELLDLL_DefView",
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
            "NotifyIconOverflowWindow",
            "TopLevelWindowForOverflowXamlIsland",
            "TaskSwitcherWnd",
            "TaskListThumbnailWnd",
            "XamlExplorerHostIslandWindow",
            "MultitaskingViewFrame",
            "LockScreenBackstopFrame",
        ] {
            // Whatever process the shell's window runs in, and whatever
            // its case.
            let surface = facts(class, Some(r"C:\Windows\explorer.exe"));
            assert!(!is_target(&surface), "{class}");
            let lowered = facts(&class.to_lowercase(), None);
            assert!(!is_target(&lowered), "{}", class.to_lowercase());
        }
    }

    #[test]
    fn a_shell_host_s_core_window_is_not_the_application_in_front() {
        for program in [
            r"C:\Windows\explorer.exe",
            r"C:\Windows\SystemApps\Search\SearchHost.exe",
            r"C:\Windows\SystemApps\Search\SearchApp.exe",
            r"C:\Windows\SystemApps\Search\SearchUI.exe",
            r"C:\Windows\SystemApps\LockApp\LockApp.exe",
            r"C:\Windows\SystemApps\LockApp\LockAppHost.exe",
            r"C:\Windows\SystemApps\Input\TextInputHost.exe",
            r"C:\Windows\System32\ctfmon.exe",
            r"C:\Windows\SystemApps\ShellExperienceHost\ShellExperienceHost.exe",
            r"C:\Windows\SystemApps\Widget\Widgets.exe",
            r"C:\Windows\SystemApps\Widget\WidgetService.exe",
        ] {
            let surface = facts("Windows.UI.Core.CoreWindow", Some(program));
            assert!(!is_target(&surface), "{program}");
        }
    }

    #[test]
    fn pane_s_own_windows_are_never_the_target_whatever_they_are() {
        for class in [
            "PaneLauncher",
            "Windows.UI.Core.CoreWindow",
            "Shell_TrayWnd",
        ] {
            let window = own(facts(class, Some(r"C:\Apps\Pane\pane.exe")));
            assert!(!is_target(&window), "{class}");
        }
    }

    #[test]
    fn an_owned_tool_window_is_not_the_target() {
        // The taskbar's overflow flyout, or an application's floating
        // palette: the window before it stays the target.
        let palette = helper(facts("WinFormsPane", Some(r"C:\Apps\app.exe")));
        assert!(!is_target(&palette));
    }

    /// A shortcut named `name` to `program`, as the Start menu's
    /// discovery reports it.
    fn shortcut(name: &str, program: &str) -> Source {
        let key = Key::program(program, "");
        let path = format!(r"C:\Menu\{name}.lnk");
        Source {
            program: Some(program.into()),
            ..Source::new(key, path, name, "the Start menu", 2)
        }
    }

    #[test]
    fn the_app_user_model_id_names_a_packaged_application_first() {
        let aumid = "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App";
        let family = "microsoft.windowscalculator_8wekyb3d8bbwe";
        let key = Key::Package {
            family: family.into(),
            app: None,
        };
        let source = Source::new(
            key,
            format!("shell:AppsFolder\\{aumid}"),
            "Calculator",
            "the Apps folder",
            5,
        );
        let catalog = Catalog::new(vec![source]);
        let target = Target {
            title: "Standard Calculator".into(),
            aumid: Some(aumid.into()),
            program: None,
            family: Some(family.into()),
        };
        assert_eq!(
            front_application(&target, &catalog),
            FrontApplication {
                name: "Calculator".into(),
                icon: Some(format!("shell:AppsFolder\\{aumid}")),
            }
        );
        // Without a match, the title stands in, and the Apps Folder name
        // still names the icon.
        let unknown = Target {
            title: "Contoso Suite".into(),
            aumid: Some("Contoso.Suite_abc!Writer".into()),
            program: None,
            family: None,
        };
        assert_eq!(
            front_application(&unknown, &catalog),
            FrontApplication {
                name: "Contoso Suite".into(),
                icon: Some("shell:AppsFolder\\Contoso.Suite_abc!Writer".into()),
            }
        );
    }

    #[test]
    fn a_program_s_path_is_matched_against_the_installed_applications() {
        let code = shortcut("Visual Studio Code", r"C:\VS Code\Code.exe");
        let catalog = Catalog::new(vec![code]);
        let target = Target {
            title: "notes.rs - Visual Studio Code".into(),
            aumid: None,
            program: Some(r"C:\VS Code\Code.exe".into()),
            family: None,
        };
        assert_eq!(
            front_application(&target, &catalog),
            FrontApplication {
                name: "Visual Studio Code".into(),
                icon: Some(r"C:\VS Code\Code.exe".into()),
            }
        );
        // A program nothing installed names: the title, and the
        // program's own icon.
        let plain = Target {
            title: "Untitled - Paint".into(),
            aumid: None,
            program: Some(r"C:\Windows\System32\mspaint.exe".into()),
            family: None,
        };
        assert_eq!(
            front_application(&plain, &catalog),
            FrontApplication {
                name: "Untitled - Paint".into(),
                icon: Some(r"C:\Windows\System32\mspaint.exe".into()),
            }
        );
    }

    #[test]
    fn a_process_s_package_family_names_its_application() {
        let key = Key::Package {
            family: "contoso.suite_abc".into(),
            app: None,
        };
        let source = Source::new(
            key,
            r"shell:AppsFolder\Contoso.Suite_abc!Writer",
            "Contoso Suite",
            "the Apps folder",
            5,
        );
        let catalog = Catalog::new(vec![source]);
        let target = Target {
            title: "Writer".into(),
            aumid: None,
            program: None,
            family: Some("Contoso.Suite_abc".into()),
        };
        assert_eq!(
            front_application(&target, &catalog),
            FrontApplication {
                name: "Contoso Suite".into(),
                icon: Some(r"shell:AppsFolder\Contoso.Suite_abc!Writer".into()),
            }
        );
    }

    #[test]
    fn a_window_with_nothing_known_of_it_is_named_by_its_title() {
        let nothing = Target {
            title: String::new(),
            aumid: None,
            program: None,
            family: None,
        };
        assert_eq!(
            front_application(&nothing, &Catalog::default()),
            FrontApplication {
                name: "the application in front".into(),
                icon: None,
            }
        );
        // An empty title falls back to the program's name.
        let untitled = Target {
            title: " ".into(),
            aumid: None,
            program: Some(r"C:\Tools\wt.exe".into()),
            family: None,
        };
        assert_eq!(
            front_application(&untitled, &Catalog::default()),
            FrontApplication {
                name: "wt".into(),
                icon: Some(r"C:\Tools\wt.exe".into()),
            }
        );
    }

    #[test]
    fn a_refusal_says_why_and_that_the_text_stays() {
        for (refusal, says) in [
            (PasteRefusal::Gone, "is gone"),
            (PasteRefusal::NotResponding, "not responding"),
            (PasteRefusal::Elevated, "administrator"),
            (PasteRefusal::NotFront, "did not come back to the front"),
            (PasteRefusal::TookTooLong, "waited too long"),
        ] {
            let message = refusal.message();
            assert!(message.contains(says), "{message}");
            assert!(message.contains("clipboard"), "{message}");
        }
        // The newer paste owns the clipboard, so this one says nothing
        // of it.
        assert_eq!(
            PasteRefusal::Superseded.message(),
            "A newer paste replaced this one"
        );
    }
}
