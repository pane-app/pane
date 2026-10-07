//! The system functions on macOS (see the parent module): the general
//! pasteboard through the clipboard adapter, `/usr/bin/open` for opening
//! (`-a` names the application) and revealing (`-R`), and `NSFileManager`'s
//! `trashItemAtURL` for the trash, from where Finder's "Put Back" returns a
//! file.

use std::path::{Path, PathBuf};

use objc2::rc::autoreleasepool;
use objc2_foundation::{NSFileManager, NSString, NSURL};

use super::{Clip, MAX_CLIPBOARD_TEXT, NotTrashed, System, programs};
use crate::clipboard::macos::{put_clip, read_clip};

/// The program that opens and reveals things as Finder does.
const OPEN: &str = "/usr/bin/open";

/// The system functions on macOS.
pub(super) struct MacosSystem;

impl System for MacosSystem {
    fn copy(&self, clip: &Clip, concealed: bool) -> Result<(), String> {
        put_clip(clip, concealed)
    }

    fn read_clipboard(&self) -> Result<Option<Clip>, String> {
        read_clip(MAX_CLIPBOARD_TEXT)
    }

    fn open(&self, target: &str, application: Option<&str>) -> Result<(), String> {
        match application {
            None => programs::run(OPEN, &[target]),
            // `-a` takes an application's name or the path of its bundle,
            // which is the id Pane gives installed applications here.
            Some(application) => programs::run(OPEN, &["-a", application, target]),
        }
    }

    fn reveal(&self, path: &Path) -> Result<(), String> {
        programs::run(OPEN, &["-R", &path.to_string_lossy()])
    }

    fn trash(&self, paths: &[PathBuf]) -> Vec<NotTrashed> {
        paths
            .iter()
            .filter_map(|path| {
                trash_one(path).err().map(|reason| NotTrashed {
                    path: path.clone(),
                    reason,
                })
            })
            .collect()
    }
}

/// Moves `path` to the trash, or says why not.
fn trash_one(path: &Path) -> Result<(), String> {
    autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|error| error.localizedDescription().to_string())
    })
}
