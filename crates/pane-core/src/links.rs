//! Opening web links: the `open-url` action of a computed root result, such
//! as a quicklink. Pane hands the address to a [`LinkOpener`], normally the
//! system's handler for web links (the window supplies it), and accepts only
//! `http://` and `https://` addresses, so an extension cannot have the system
//! open a local file or run another URL scheme's handler this way.

/// Opens web links; the launcher calls it off the window's thread, since a
/// system handler may take a moment to start.
pub trait LinkOpener: Send + Sync + 'static {
    /// Opens `url`, an `http://` or `https://` address, with the system's
    /// handler for web links. An error explains to the user why it could
    /// not, such as that no handler is installed or that the system refused.
    fn open(&self, url: &str) -> Result<(), String>;
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
