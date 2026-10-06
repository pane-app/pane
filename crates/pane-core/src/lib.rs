//! Pane's core: the launcher model, extension packages and the extension
//! runtime the launcher drives.

mod application_update;
pub mod applications;
mod archive;
mod atomic;
pub mod autostart;
pub mod changes;
pub mod clipboard;
pub mod defaults;
mod dependencies;
pub mod develop;
pub mod downloads;
mod extension_data;
pub mod files;
mod generation;
pub mod git;
mod helpers;
mod host_settings;
pub mod hotkeys;
mod http;
mod integrity;
mod keyboard;
mod launch;
mod launcher;
mod links;
pub mod npm;
mod operations;
mod packages;
#[cfg(test)]
mod peak_memory;
pub mod placement;
mod platform;
mod runtime;
mod search;
mod threads;
pub mod tray;
mod zip;

pub use defaults::{ArtifactSource, DefaultExtension};
pub use helpers::runner::{MAX_HELPER_INPUT, MAX_HELPER_OUTPUT};
pub use host_settings::{
    BackgroundEffect, EscapeBehavior, HostSettings, MaterialPreference, NavigationBindings,
    OpeningMonitor, PinnedLayout, Reopening, ThemePreference, WindowMode,
};
#[doc(hidden)]
pub use http::HttpLimits;
pub use keyboard::{Binding, Keyboard, KeyboardAction};
pub use launch::{LaunchRecord, LaunchSource, LaunchType};
pub use launcher::clipboard_view;
pub use launcher::{
    AliasOutcome, ApplicationUpdate, BuildFailure, CommandRegistration, ComputedAnswer,
    CustomViewSnapshot, Development, FormField, FormView, HotkeyOutcome, Launcher, LauncherView,
    PinTarget, Presentation, Question, QuickSlot, ResultAction, ResultActionItem, ResultActions,
    Row, RowKind, RowPresentation, Screen, Section, SelectedAction, ShortcutCatalog,
    ShortcutCommand, ShortcutGroup, SlotChange, Status, Unavailable, answer_sections,
    root_sections,
};
pub use links::LinkOpener;
pub use operations::{MAX_CALL_DEPTH, MAX_OPERATION_JSON};
pub use packages::{
    CommandMode, EXTENSION_API, InstalledPackage, MANIFEST_FILE, MANIFEST_VERSION,
    MAX_SCHEDULE_SECONDS, MIN_SCHEDULE_SECONDS, Manifest, ManifestCommand, ManifestHelper,
    ManifestOperation, ManifestSchedule, PackageError, PackageIdentity, RetainedData, SavedData,
};
pub use pane_target::{Arch, Target};
pub use platform::Platform;
#[cfg(debug_assertions)]
#[doc(hidden)]
pub use runtime::Fault;
#[doc(hidden)]
pub use runtime::Limits;
#[cfg(debug_assertions)]
#[doc(hidden)]
pub use runtime::memory_peak;
pub use runtime::{
    Action, Answer, COMPUTE_LIMIT, CallError, Choice, CustomViewInfo, CustomViewRole, Field,
    FieldKind, FieldValue, Form, FormError, Frame, GUEST_MEMORY, Item, Key, MAX_FRAME_SHAPES,
    MAX_FRAME_SIZE, MAX_TEXT_CHARS, Point, Rgb, Runtime, RuntimeFailure, RuntimeStatus, Shape,
    TREE_VERSION, UNRESPONSIVE_LIMIT, View, ViewEvent, ViewId, WARN_AFTER,
};
pub use search::{SettingsEntry, settings_matches, title_matches};
