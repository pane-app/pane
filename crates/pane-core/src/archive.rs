//! Unpacking an archive into a folder of its own with one discipline,
//! whoever made it: only regular files and folders, each inside the
//! folder, within a bound on what it unpacks to. An npm package's
//! tarball, a default extension's payload and Pane's own application
//! package all unpack through here (a zip through [`inside`]).

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::downloads::check_part;

/// The most entries a tarball may hold: files, folders and the extension
/// headers (long names, PAX) describing them.
pub const MAX_ENTRIES: usize = 10_000;
/// The largest extension header (a GNU long name, a PAX header) Pane reads.
pub const MAX_EXTENSION: u64 = 64 << 10;

/// As [`crate::npm::unpack`], within `max_unpacked` bytes instead of npm's own bound: a
/// package larger than an npm package's tarball — Pane's own application
/// package, a whole program — unpacks with the bound its reader gives.
pub(crate) fn unpack_within(tgz: &[u8], dest: &Path, max_unpacked: u64) -> Result<(), String> {
    fs::create_dir(dest).map_err(|error| error.to_string())?;
    // The decompressed stream is limited too, so a small tarball cannot
    // expand without bound (headers and padding take some of it).
    let stream = flate2::read::GzDecoder::new(tgz).take(max_unpacked + (max_unpacked >> 2));
    let mut archive = tar::Archive::new(stream);
    let mut total: u64 = 0;
    let mut count = 0;
    // What the extension headers read so far say of the next entry.
    let mut next = Described::default();
    let unreadable = |error: io::Error| format!("its tarball cannot be read: {error}");
    let entries = archive.entries().map_err(unreadable)?.raw(true);
    for entry in entries {
        let mut entry = entry.map_err(unreadable)?;
        count += 1;
        if count > MAX_ENTRIES {
            return Err(format!("its tarball holds more than {MAX_ENTRIES} entries"));
        }
        let size = entry.size();
        let add = |total: &mut u64, size: u64| {
            *total = total.saturating_add(size);
            if *total > max_unpacked {
                return Err(format!(
                    "it unpacks to more than the {} MiB Pane allows",
                    max_unpacked >> 20
                ));
            }
            Ok(())
        };
        let kind = entry.header().entry_type();
        if matches!(
            kind,
            tar::EntryType::GNULongName
                | tar::EntryType::GNULongLink
                | tar::EntryType::XHeader
                | tar::EntryType::XGlobalHeader
        ) {
            if size > MAX_EXTENSION {
                return Err(format!(
                    "its tarball has an extension header of {size} bytes; Pane reads at most {} \
                     KiB of one",
                    MAX_EXTENSION >> 10
                ));
            }
            add(&mut total, size)?;
            let mut data = Vec::new();
            (&mut entry)
                .take(size)
                .read_to_end(&mut data)
                .map_err(unreadable)?;
            if data.len() as u64 != size {
                return Err("its tarball ends inside an extension header".into());
            }
            next.read(kind, &data)?;
            continue;
        }
        let described = std::mem::take(&mut next);
        let raw = match described.path {
            Some(path) => path,
            None => entry.path_bytes().into_owned(),
        };
        let shown = String::from_utf8_lossy(&raw).into_owned();
        let is_dir = match kind {
            tar::EntryType::Regular | tar::EntryType::Continuous => false,
            tar::EntryType::Directory => true,
            other => {
                let what = match other {
                    tar::EntryType::Symlink => "a symbolic link".to_owned(),
                    tar::EntryType::Link => "a hard link".to_owned(),
                    tar::EntryType::Char | tar::EntryType::Block => "a device".to_owned(),
                    tar::EntryType::Fifo => "a named pipe".to_owned(),
                    other => format!("an entry of type {:?}", other.as_byte() as char),
                };
                return Err(format!(
                    "its tarball contains {what}, `{shown}`; Pane unpacks only files and folders"
                ));
            }
        };
        if let Some(declared) = described.size
            && declared != size
        {
            return Err(format!(
                "its tarball gives `{shown}` two sizes, {size} and {declared} bytes; Pane \
                 unpacks only entries whose size is unambiguous"
            ));
        }
        let Some(relative) = inside(&raw, is_dir).map_err(|why| {
            format!(
                "its tarball contains `{shown}`, {why}; Pane unpacks only paths inside the package"
            )
        })?
        else {
            // The top folder itself.
            continue;
        };
        let path = dest.join(&relative);
        if is_dir {
            fs::create_dir_all(&path).map_err(|error| format!("`{shown}`: {error}"))?;
            continue;
        }
        add(&mut total, size)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("`{shown}`: {error}"))?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| match error.kind() {
                io::ErrorKind::AlreadyExists => {
                    format!("its tarball contains `{shown}` twice")
                }
                _ => format!("`{shown}`: {error}"),
            })?;
        let copied = io::copy(&mut (&mut entry).take(size), &mut file).map_err(unreadable)?;
        if copied != size {
            return Err(format!("its tarball ends inside `{shown}`"));
        }
    }
    if next != Described::default() {
        return Err("its tarball ends with an extension header that describes no entry".into());
    }
    Ok(())
}

/// What the extension headers before an entry say of it.
#[derive(Default, PartialEq, Eq)]
struct Described {
    /// Its path, from a GNU long name or a PAX `path`.
    path: Option<Vec<u8>>,
    /// Its size, from a PAX `size`.
    size: Option<u64>,
    /// Whether a GNU long name or a PAX header was read for it.
    long_name: bool,
    pax: bool,
}

impl Described {
    /// Takes in the extension header of `kind` holding `data`.
    fn read(&mut self, kind: tar::EntryType, data: &[u8]) -> Result<(), String> {
        let twice = || Err("its tarball has two extension headers of one kind for an entry".into());
        match kind {
            tar::EntryType::GNULongName => {
                if self.long_name {
                    return twice();
                }
                self.long_name = true;
                let name = data.strip_suffix(&[0]).unwrap_or(data);
                if self.path.is_none() {
                    self.path = Some(name.to_vec());
                }
                Ok(())
            }
            tar::EntryType::GNULongLink => Err(
                "its tarball contains a long link name; Pane unpacks only files and folders".into(),
            ),
            tar::EntryType::XHeader => {
                if self.pax {
                    return twice();
                }
                self.pax = true;
                for record in tar::PaxExtensions::new(data) {
                    let record = record.map_err(|_| "its tarball has a damaged PAX header")?;
                    match record.key_bytes() {
                        // A PAX path wins over a GNU long name, as tar does.
                        b"path" => self.path = Some(record.value_bytes().to_vec()),
                        b"size" => {
                            let size = record
                                .value()
                                .ok()
                                .and_then(|value| value.parse().ok())
                                .ok_or("its tarball has a PAX header with a damaged size")?;
                            self.size = Some(size);
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            // Only one that changes no path or size (such as git's commit
            // id) is taken, and ignored.
            _ => {
                for record in tar::PaxExtensions::new(data) {
                    let record = record.map_err(|_| "its tarball has a damaged PAX header")?;
                    if matches!(record.key_bytes(), b"path" | b"size" | b"linkpath") {
                        return Err(
                            "its tarball has a global header that changes paths or sizes; Pane \
                             unpacks only entries described by their own headers"
                                .into(),
                        );
                    }
                }
                Ok(())
            }
        }
    }
}

/// The path of archive entry `raw` inside the package, without the
/// archive's top folder; `None` for the top folder itself. Refuses, with
/// why, a path that is absolute, climbs out (`..`), has an empty or `.`
/// part, or has a part that some system reads differently or cannot write
/// (see [`check_part`]). Read for npm's tarballs and Pane's application
/// package's zip alike, so both unpack with the same discipline.
pub(crate) fn inside(raw: &[u8], is_dir: bool) -> Result<Option<PathBuf>, &'static str> {
    let text = std::str::from_utf8(raw).map_err(|_| "whose name is not valid UTF-8")?;
    if text.starts_with('/') {
        return Err("an absolute path");
    }
    let text = match is_dir {
        true => text.strip_suffix('/').unwrap_or(text),
        false => text,
    };
    let mut parts = text.split('/');
    let top = parts.next().unwrap_or_default();
    check_part(top)?;
    let mut path = PathBuf::new();
    for part in parts {
        check_part(part)?;
        path.push(part);
    }
    Ok((!path.as_os_str().is_empty()).then_some(path))
}
