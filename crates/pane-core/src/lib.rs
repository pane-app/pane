//! Pane's core: the launcher model, extension packages and the extension
//! runtime the launcher drives.

pub mod appimage;
mod application_update;
pub mod applications;
mod archive;
mod arguments;
mod atomic;
pub mod autostart;
pub mod changes;
pub mod check;
pub mod clipboard;
mod collections;
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
pub mod game_mode;
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
pub mod markdown;
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
mod registrations;
pub mod releases;
pub mod restart;
pub mod run;
mod runtime;
pub mod schema;
mod search;
mod source_map;
pub mod switch_windows;
pub mod system;
pub mod system_commands;
pub mod system_icons;
pub mod templates;
mod threads;
pub mod tokens;
pub mod tray;
mod typed_folder;
mod util;
mod waiting;
pub mod whats_new;
#[cfg(windows)]
mod windows_shell;
mod zip;

pub use arguments::{ArgumentKind, MAX_ARGUMENTS, ManifestArgument};
pub use defaults::DefaultExtension;
pub use dropdown::DropdownOption;
pub use feedback::{
    ConfirmAnswer, Confirmation, Hud, NextShowing, PopToRoot, ShownToast, Toast, ToastAction,
    ToastSlot, ToastStyle, WindowControl, WindowPresence,
};
pub use game_mode::GameMode;
pub use helpers::runner::{MAX_HELPER_INPUT, MAX_HELPER_OUTPUT};
pub use host_settings::{
    BackgroundEffect, EscapeBehavior, HostSettings, MaterialPreference, NavigationBindings,
    OpeningMonitor, PinnedLayout, Reopening, SearchSensitivity, ThemePreference, WindowMode,
};
#[doc(hidden)]
pub use http::HttpLimits;
pub use keyboard::{Binding, Keyboard, KeyboardAction, PaneKeys};
pub use launch::{LaunchRecord, LaunchSource, LaunchType};
pub use launcher::clipboard_view;
pub use launcher::search_files;
pub use launcher::{
    AliasFlow, AliasOutcome, ApplicationUpdate, ArgumentFields, BuildFailure, Capability,
    CapabilityProvider, Choice, ChoiceExtension, ChoiceOutcome, ChoiceView, CommandPreferences,
    CommandRegistration, ComputedAnswer, DISMISS_NOTICE, DISMISS_WHATS_NEW, DesignedDropdown,
    DesignedList, DesignedRow, DesignedViewSnapshot, Development, ExtensionDetails, ExtensionMark,
    ExtensionOperation, ExtensionWait, FieldKind, FixAction, FolderAsk, FormField, GridShape,
    HotkeyOutcome, ItemAction, ItemActions, LOADING_MS, Launcher, LauncherView, ListPresentation,
    LogNotice, MANAGE_EXTENSIONS, OfficialExtension, OpenSubmenu, OperationKind,
    PackagePreferences, PaneForm, PaneFormField, PathPick, PinTarget, PreferenceField,
    PreferencesTarget, Presentation, ProvidedCapability, QUIT_PANE, Question, QuickSlot,
    RESTART_ROW, RequirementCycle, RequirementFix, RestartHold, ResultAction, ResultActionItem,
    ResultActions, Row, RowKind, RowPresentation, Screen, Section, SelectedAction, SettingsTarget,
    ShortcutCatalog, ShortcutCommand, ShortcutGroup, SlotChange, Status, SubmenuState,
    UNEXPECTED_QUIT, Unavailable, UnboundShortcut, UnmetRequirement, UpdateChecks, UpdateHold,
    UpdateResult, UpdateResults, UpdateResultsAction, WHATS_NEW_ROW, answer_sections,
    root_sections,
};
pub use links::LinkOpener;
pub use operations::{MAX_CALL_DEPTH, MAX_OPERATION_JSON};
pub use packages::{
    CommandMatches, CommandMode, CommandWhen, EXTENSION_API, EXTENSION_API_PREVIOUS,
    InstalledPackage, ListedCommand, MANIFEST_FILE, MANIFEST_VERSION, MAX_KEYWORDS,
    MAX_SCHEDULE_SECONDS, MIN_SCHEDULE_SECONDS, Manifest, ManifestCommand, ManifestHelper,
    ManifestOperation, ManifestSchedule, PackageError, PackageIdentity, RetainedData, SavedData,
    UnsupportedApi,
};
#[doc(hidden)]
pub use pane_build::process_tree;
pub use pane_target::{Arch, Target};
pub use platform::Platform;
pub use preferences::{HELP_FILE, Preference, PreferenceKind};
pub use programs::runner::{MAX_PROGRAM_OUTPUT, SearchPath};
pub use releases::ReleaseSource;
#[cfg(debug_assertions)]
#[doc(hidden)]
pub use runtime::Fault;
#[doc(hidden)]
pub use runtime::Limits;
pub use runtime::TextMeasures;
#[cfg(debug_assertions)]
#[doc(hidden)]
pub use runtime::Timers;
#[cfg(debug_assertions)]
#[doc(hidden)]
pub use runtime::memory_peak;
pub use runtime::{
    Action, ActionKind, ActionStyle, ActionSubmenu, Align, Answer, Badge, Border, Button,
    ButtonTone, COMPONENT_SET, COMPUTE_LIMIT, CallError, Canvas, CanvasA11y, CanvasHandlers,
    CanvasOp, CanvasRole, CanvasStroke, CanvasText, Checkbox, DateField, DesignedEvent,
    DesignedHandler, DesignedNext, DesignedRendered, DesignedTree, DropdownItem, EmptyState,
    FieldProps, FilePicker, Finite, Fit, FormNode, FormValue, GRID_COLUMNS, GUEST_MEMORY, GridItem,
    IconExtent, IconNode, Image, Item, Justify, KeySequence, Keycap, Layout, Length, Link,
    ListAction, ListDropdown, ListItem, ListNode, ListSection, Loading, MAX_CANVAS_OPS,
    MAX_CANVAS_TEXT_CHARS, MAX_DEPTH, MAX_GRID_COLUMNS, MAX_INLINE_IMAGE, MAX_MARKDOWN_CHARS,
    MAX_NAVIGATION_DEPTH, MAX_NODES, MAX_PAGE_SIZE, MAX_PX, MAX_TREE_BYTES, Markdown, MetadataItem,
    MetadataList, Node, NodeKind, Offset, Orientation, Padding, Paint, Place, Progress,
    RadiusLength, RichRow, RowAccessory, Runtime, RuntimeFailure, RuntimeStatus, SectionHeader,
    Segment, Segmented, Select, Sizing, Slider, Space, Span, StrokeCap, StrokeJoin, Style,
    SubmenuEntries, Surface, TREE_VERSION, Tag, TagPicker, Text, TextContent, TextInput, TextLevel,
    TextStyle, Toggle, UNRESPONSIVE_LIMIT, View, ViewId, WARN_AFTER, key_problems,
};
pub use search::{SettingsEntry, settings_matches, title_matches};
pub use tokens::{IconSize, Radius};
// Icons, accessories and tooltips (#139).
pub use icons::{Color, Icon, IconSource, Mask, Tint, Tone};

pub use launcher::{AccessoryKind, ShownAccessory, absolute_date, relative_date};
pub use runtime::{Accessory, AccessoryContent, ItemLook, MAX_ACCESSORIES};
