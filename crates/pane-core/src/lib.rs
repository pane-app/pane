//! Pane's core: the launcher model, extension packages and the extension
//! runtime the launcher drives.

pub mod applications;
mod atomic;
mod extension_data;
mod generation;
pub mod hotkeys;
mod launcher;
mod links;
mod operations;
mod packages;
mod platform;
mod runtime;
mod search;

pub use launcher::{
    CommandRegistration, CustomViewSnapshot, FormField, FormView, Launcher, LauncherView, Question,
    Row, Screen, Status,
};
pub use links::LinkOpener;
pub use operations::{MAX_CALL_DEPTH, MAX_OPERATION_JSON};
pub use packages::{
    EXTENSION_API, InstalledPackage, MANIFEST_FILE, MANIFEST_VERSION, Manifest, ManifestCommand,
    ManifestOperation, PackageError, PackageIdentity, RetainedData, SavedData,
};
pub use platform::Platform;
pub use runtime::{
    CallError, Choice, CustomViewInfo, CustomViewRole, Field, FieldKind, FieldValue, Form,
    FormError, Frame, Item, Key, MAX_FRAME_SHAPES, MAX_FRAME_SIZE, MAX_TEXT_CHARS, Point, Rgb,
    Runtime, Shape, View, ViewEvent, ViewId,
};
