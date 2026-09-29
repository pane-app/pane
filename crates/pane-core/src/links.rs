//! Opening web links and files: the `open-url` and `open-file` actions of a
//! computed root result, such as a quicklink or a file the Files extension
//! found. Pane hands the address or path to a [`LinkOpener`], normally the
//! system's handlers (the window supplies it). It accepts only `http://` and
//! `https://` addresses as links, so an extension cannot have the system run
//! another URL scheme's handler that way; a file reaches the opener only
//! once the host has checked it inside the package's granted folder
//! (`crate::files`).

use std::path::Path;

/// Opens web links and files; the launcher calls it off the window's
/// thread, since a system handler may take a moment to start.
pub trait LinkOpener: Send + Sync + 'static {
    /// Opens `url`, an `http://` or `https://` address, with the system's
    /// handler for web links. An error explains to the user why it could
    /// not, such as that no handler is installed or that the system refused.
    fn open(&self, url: &str) -> Result<(), String>;

    /// Opens `path`, the absolute path of a regular file Pane has checked,
    /// with the system's handler for its type, as the system's file manager
    /// would. An error explains why it could not. An opener that opens no
    /// files keeps this default, which says so.
    fn open_file(&self, path: &Path) -> Result<(), String> {
        let _ = path;
        Err("this Pane has no handler for files".into())
    }
}

/// The opener of a launcher that was given none.
pub(crate) struct NoOpener;

impl LinkOpener for NoOpener {
    fn open(&self, _url: &str) -> Result<(), String> {
        Err("this Pane has no link handler".into())
    }
}

/// Why Pane does not open `url` at all, if it does not: it is not a web
/// address.
pub(crate) fn refusal(url: &str) -> Option<String> {
    let scheme = url.split_once("://").map(|(scheme, _)| scheme);
    let web = scheme.is_some_and(|scheme| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    });
    (!web).then(|| "Pane opens only http:// and https:// links".into())
}

/// The last name of `path`, for the user: the file's own name.
pub(crate) fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}
