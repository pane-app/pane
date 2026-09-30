//! Pane's core: the launcher model, extension packages and the extension
//! runtime the launcher drives.

mod application_update;
pub mod applications;
mod atomic;
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
pub mod hotkeys;
mod http;
mod launcher;
mod links;
pub mod npm;
mod operations;
mod packages;
#[cfg(test)]
mod peak_memory;
mod platform;
mod runtime;
mod search;
mod threads;
mod zip;

pub use defaults::{ArtifactSource, DefaultExtension};
pub use helpers::runner::{HELPER_TIME_LIMIT, MAX_HELPER_INPUT, MAX_HELPER_OUTPUT};
#[doc(hidden)]
pub use http::HttpLimits;
pub use launcher::{
    BuildFailure, CommandRegistration, CustomViewSnapshot, Development, FormField, FormView,
    Launcher, LauncherView, Question, Row, Screen, Status, Unavailable,
};
pub use links::LinkOpener;
pub use operations::{MAX_CALL_DEPTH, MAX_OPERATION_JSON};
pub use packages::{
    EXTENSION_API, InstalledPackage, MANIFEST_FILE, MANIFEST_VERSION, MAX_SCHEDULE_SECONDS,
    MIN_SCHEDULE_SECONDS, Manifest, ManifestCommand, ManifestHelper, ManifestOperation,
    ManifestSchedule, PackageError, PackageIdentity, RetainedData, SavedData,
};
pub use pane_target::{Arch, Target};
pub use platform::Platform;
#[cfg(debug_assertions)]
#[doc(hidden)]
pub use runtime::Fault;
#[doc(hidden)]
pub use runtime::Limits;
pub use runtime::{
    COMPUTE_LIMIT, CallError, Choice, CustomViewInfo, CustomViewRole, Field, FieldKind, FieldValue,
    Form, FormError, Frame, Item, Key, MAX_FRAME_SHAPES, MAX_FRAME_SIZE, MAX_TEXT_CHARS, Point,
    Rgb, Runtime, RuntimeFailure, RuntimeStatus, Shape, UNRESPONSIVE_LIMIT, View, ViewEvent,
    ViewId, WARN_AFTER,
};
