//! Pane's core: the launcher model, extension packages and the extension
//! runtime the launcher drives.

mod application_update;
pub mod applications;
mod archive;
mod arguments;
mod atomic;
pub mod autostart;
pub mod changes;
pub mod check;
pub mod clipboard;
pub mod defaults;
mod dependencies;
pub mod develop;
pub mod diagnostics;
pub mod downloads;
mod dropdown;
mod extension_data;
pub mod extension_log;
pub mod feedback;
pub mod file_index;
pub mod files;
mod generation;
pub mod git;
mod helpers;
mod host_settings;
pub mod hotkeys;
mod http;
pub mod icons;
mod integrity;
pub mod keyboard;
mod launch;
mod launcher;
mod links;
pub mod local_channel;
pub mod npm;
mod operations;
pub mod pack;
mod packages;
#[cfg(test)]
mod peak_memory;
pub mod placement;
mod platform;
mod preferences;
mod programs;
mod protection;
mod runtime;
pub mod schema;
mod search;
mod source_map;
pub mod system;
pub mod system_icons;
pub mod templates;
mod threads;
pub mod tray;
mod util;
#[cfg(windows)]
mod windows_shell;
mod zip;

pub use arguments::{ArgumentKind, MAX_ARGUMENTS, ManifestArgument};
pub use defaults::{ArtifactSource, DefaultExtension};
pub use dropdown::DropdownOption;
pub use feedback::{
    ConfirmAnswer, Confirmation, Hud, NextShowing, PopToRoot, ShownToast, Toast, ToastAction,
    ToastSlot, ToastStyle, WindowControl, WindowPresence,
};
pub use helpers::runner::{MAX_HELPER_INPUT, MAX_HELPER_OUTPUT};
pub use host_settings::{
    BackgroundEffect, EscapeBehavior, HostSettings, MaterialPreference, NavigationBindings,
    OpeningMonitor, PinnedLayout, Reopening, ThemePreference, WindowMode,
};
#[doc(hidden)]
pub use http::HttpLimits;
pub use keyboard::{Binding, Keyboard, KeyboardAction, PaneKeys};
pub use launch::{LaunchRecord, LaunchSource, LaunchType};
pub use launcher::clipboard_view;
pub use launcher::search_files;
pub use launcher::{
    AliasOutcome, ApplicationUpdate, BuildFailure, CommandPreferences, CommandRegistration,
    ComputedAnswer, CustomViewSnapshot, DISMISS_NOTICE, Development, ExtensionMark,
    ExtensionOperation, FormField, FormView, HotkeyOutcome, ItemAction, ItemActions, Launcher,
    LauncherView, ListPresentation, LogNotice, MANAGE_EXTENSIONS, OpenSubmenu, OperationKind,
    PackagePreferences, PinTarget, PreferenceField, PreferencesTarget, Presentation, Question,
    QuickSlot, ResultAction, ResultActionItem, ResultActions, Row, RowKind, RowPresentation,
    Screen, Section, SelectedAction, SettingsTarget, SetupHeader, ShortcutCatalog, ShortcutCommand,
    ShortcutGroup, SlotChange, Status, SubmenuState, UNEXPECTED_QUIT, Unavailable, UnboundShortcut,
    UpdateHold, answer_sections, root_sections,
};
pub use links::LinkOpener;
pub use operations::{MAX_CALL_DEPTH, MAX_OPERATION_JSON};
pub use packages::{
    CommandMode, EXTENSION_API, InstalledPackage, ListedCommand, MANIFEST_FILE, MANIFEST_VERSION,
    MAX_KEYWORDS, MAX_SCHEDULE_SECONDS, MIN_SCHEDULE_SECONDS, Manifest, ManifestCommand,
    ManifestHelper, ManifestOperation, ManifestSchedule, PackageError, PackageIdentity,
    RetainedData, SavedData,
};
#[doc(hidden)]
pub use pane_build::process_tree;
pub use pane_target::{Arch, Target};
pub use platform::Platform;
pub use preferences::{HELP_FILE, Preference, PreferenceKind};
pub use programs::runner::{MAX_PROGRAM_OUTPUT, SearchPath};
#[cfg(debug_assertions)]
#[doc(hidden)]
pub use runtime::Fault;
#[doc(hidden)]
pub use runtime::Limits;
#[cfg(debug_assertions)]
#[doc(hidden)]
pub use runtime::Timers;
#[cfg(debug_assertions)]
#[doc(hidden)]
pub use runtime::memory_peak;
pub use runtime::{
    Action, ActionKind, ActionStyle, ActionSubmenu, Answer, COMPUTE_LIMIT, CallError, Choice,
    CustomViewInfo, CustomViewRole, Field, FieldKind, FieldValue, Form, FormError, Frame,
    GUEST_MEMORY, Item, Key, MAX_FRAME_SHAPES, MAX_FRAME_SIZE, MAX_TEXT_CHARS, PathKind, Point,
    Rgb, Runtime, RuntimeFailure, RuntimeStatus, Shape, SubmenuEntries, TREE_VERSION,
    UNRESPONSIVE_LIMIT, View, ViewEvent, ViewId, WARN_AFTER,
};
pub use search::{SettingsEntry, settings_matches, title_matches};
// Icons, accessories and tooltips (#139).
pub use icons::{Color, Icon, IconSource, Mask, Tint, Tone};
pub use launcher::{AccessoryKind, ShownAccessory, absolute_date, relative_date};
pub use runtime::{Accessory, AccessoryContent, ItemLook, MAX_ACCESSORIES};
