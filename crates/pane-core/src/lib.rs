//! Pane's core: the launcher model, extension packages and the extension
//! runtime the launcher drives.

mod atomic;
mod launcher;
mod packages;
mod platform;
mod runtime;
mod settings;

pub use launcher::{
    CommandRegistration, CustomViewSnapshot, FormField, FormView, Launcher, LauncherView, Row,
    Screen, Status,
};
pub use packages::{
    EXTENSION_API, InstalledPackage, MANIFEST_FILE, MANIFEST_VERSION, Manifest, ManifestCommand,
    PackageError, PackageIdentity,
};
pub use platform::Platform;
pub use runtime::{
    CallError, Choice, CustomViewInfo, CustomViewRole, Field, FieldKind, FieldValue, Form,
    FormError, Frame, Item, Key, MAX_FRAME_SHAPES, MAX_FRAME_SIZE, MAX_TEXT_CHARS, Point, Rgb,
    Runtime, Shape, View, ViewEvent, ViewId,
};
