//! Reading the zip of Pane's Windows package, which an application update
//! downloads ([#54](https://github.com/hoangvu12/pane/issues/54)).
//!
//! A Windows user unzips Pane's package with whatever is at hand —
//! Explorer, `Expand-Archive`, any unzip — and Windows has no tar a user
//! can rely on, so the Windows package is a zip ([`xtask`'s zip
//! writer](../../../xtask/src/zip.rs) documents why it is written by hand
//! rather than through a library). An application update installs that
//! same package, so Pane reads it here, as strictly as it reads an npm
//! package's tarball ([`crate::npm::unpack`]): only files and folders,
//! each path inside the package and one plain name on every system, and
//! the whole archive within the same bounds. What each entry says about
//! itself is checked twice — once in the central directory, once in the
//! file's own local header — so no two readers can see different files in
//! one zip, and every file's bytes are checked against the CRC32 its
//! entry names, as every unzip does.
//!
//! Only what Pane's own packaging writes is accepted: entries stored or
//! deflated (method 0 or 8), no encryption, no zip64, no data
//! descriptors, no extra fields. A zip that says anything else is
//! explained and refused rather than guessed at, because a wrong guess
//! here installs a wrong program.

use std::fs;
use std::io::Read;
use std::path::Path;

use crate::npm::inside;

/// The largest an application package unpacks to, however it is packed
/// (a zip, or the gzipped tarball the Linux package is): the package
/// holds the whole program, which a development build of is larger than
/// a release one by an order of magnitude.
pub(crate) const MAX_UNPACKED: u64 = 2 << 30;

/// The largest number of entries an application package holds.
const MAX_ENTRIES: usize = 10_000;

/// A local file header: the beginning of one file's entry.
const LOCAL_HEADER: u32 = 0x0403_4b50;
/// A central-directory entry, one per file, after every file's bytes.
const DIRECTORY_ENTRY: u32 = 0x0201_4b50;
/// The end of the central directory: the last thing in the archive.
const END_RECORD: u32 = 0x0605_4b50;
/// An entry stored uncompressed.
const STORED: u16 = 0;
/// An entry deflated, the method Pane's packaging uses.
const DEFLATE: u16 = 8;
/// The flag bit that marks an entry encrypted.
const ENCRYPTED: u16 = 1;
/// How far from the end of an archive the end record can sit: the largest
/// comment an end record names.
const MOST_COMMENT: usize = 64 * 1024;
/// The fixed length of a central-directory entry, before its name, extra
/// field and comment.
const DIRECTORY_FIXED: usize = 46;

/// Unpacks the zip `zip` into `dest`, which must not exist, without its
/// top folder (`pane/` in Pane's packages): only regular files and
/// folders, each inside `dest`, within [`MAX_UNPACKED`] bytes and
/// [`MAX_ENTRIES`] entries, every entry stored or deflated. Files are
/// written without execute permission (the program an update installs
/// gets it separately); the time, owner and mode the archive records are
/// ignored. Refuses the whole archive, explaining why, at the first entry
/// it cannot take.
pub(crate) fn unpack(zip: &[u8], dest: &Path) -> Result<(), String> {
    let entries = directory(zip)?;
    fs::create_dir(dest).map_err(|error| error.to_string())?;
    let mut total: u64 = 0;
    for entry in entries {
        let read = read_entry(zip, &entry)?;
        // The top folder of the package is not unpacked, as an npm
        // package's tarball's is not.
        let relative = inside(entry.name.as_bytes(), entry.name.ends_with('/')).map_err(|why| {
            format!(
                "its zip contains `{}`, {}; Pane unpacks only paths inside the package",
                entry.name, why
            )
        })?;
        let Some(relative) = relative else {
            continue;
        };
        let path = dest.join(&relative);
        if entry.name.ends_with('/') {
            fs::create_dir_all(&path).map_err(|error| format!("`{}`: {error}", entry.name))?;
            continue;
        }
        total = total.saturating_add(read.len() as u64);
        if total > MAX_UNPACKED {
            return Err(format!(
                "it unpacks to more than the {} MiB Pane allows",
                MAX_UNPACKED >> 20
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("`{}`: {error}", entry.name))?;
        }
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(format!("its zip contains `{}` twice", entry.name));
            }
            Err(error) => return Err(format!("`{}`: {error}", entry.name)),
        };
        std::io::Write::write_all(&mut file, &read)
            .map_err(|error| format!("`{}`: {error}", entry.name))?;
    }
    Ok(())
}

/// One file's central-directory entry: what the directory says about it.
struct Entry {
    name: String,
    /// Method 0 (stored) or 8 (deflated).
    method: u16,
    crc: u32,
    /// The file's own size, once decompressed.
    size: u64,
    /// The deflated bytes' size.
    compressed: u64,
    /// Where the file's local header is, from the archive's beginning.
    at: u64,
}

/// Reads every entry of the archive's central directory, checking each
/// against what the archive's own shape says.
fn directory(zip: &[u8]) -> Result<Vec<Entry>, String> {
    let end = find_end(zip)?;
    let record = zip
        .get(end..)
        .filter(|record| record.len() >= 22)
        .ok_or("its zip ends inside its end record")?;
    let mut read = &record[4..];
    let disk = u16(&mut read);
    let holding = u16(&mut read);
    let here = u16(&mut read);
    let all = u16(&mut read);
    let size = u32(&mut read);
    let at = u32(&mut read);
    // The bounds of a plain zip: what the end record says must fit, and
    // zip64 markers and other disks are refused rather than read past.
    if disk != 0 || holding != 0 {
        return Err("its zip is one of several disks, which Pane does not read".into());
    }
    if here != all {
        return Err("its zip's directory holds fewer entries than the archive says".into());
    }
    if here == u16::MAX || at == u32::MAX {
        return Err("its zip is a zip64 archive, which Pane does not read".into());
    }
    let directory_at = at as usize;
    if directory_at + size as usize > end {
        return Err("its zip's directory reaches past the archive's end".into());
    }
    let mut entries = Vec::new();
    let mut tail = &zip[directory_at..directory_at + size as usize];
    while !tail.is_empty() {
        if entries.len() >= MAX_ENTRIES {
            return Err(format!("its zip holds more than {MAX_ENTRIES} entries"));
        }
        let mut read = tail;
        if tail.len() < DIRECTORY_FIXED {
            return Err("its zip ends inside an entry of its directory".into());
        }
        if u32(&mut read) != DIRECTORY_ENTRY {
            return Err("its zip ends inside an entry of its directory".into());
        }
        // The versions that made this archive and that it needs: read for
        // the shape of the entry rather than any meaning.
        let _ = u16(&mut read);
        let _ = u16(&mut read);
        let flags = u16(&mut read);
        if flags & ENCRYPTED != 0 {
            return Err("its zip holds an encrypted entry, which Pane does not read".into());
        }
        let method = u16(&mut read);
        let _ = u16(&mut read); // the time
        let _ = u16(&mut read); // the date
        let crc = u32(&mut read);
        let compressed = u32(&mut read) as u64;
        let size = u32(&mut read) as u64;
        let name_len = u16(&mut read) as usize;
        let extra = u16(&mut read) as usize;
        let comment = u16(&mut read) as usize;
        let _ = u16(&mut read); // the disk the entry's file starts on
        let _ = u16(&mut read); // internal attributes
        let _ = u32(&mut read); // external attributes
        let at = u32(&mut read) as u64;
        let held = name_len + extra + comment;
        if read.len() < held {
            return Err("its zip ends inside an entry of its directory".into());
        }
        let name = String::from_utf8(read[..name_len].to_vec())
            .map_err(|_| "its zip holds a file name that is not valid UTF-8".to_owned())?;
        tail = &read[held..];
        if !matches!(method, STORED | DEFLATE) {
            return Err(format!(
                "its zip holds `{name}` with a compression method Pane does not read"
            ));
        }
        entries.push(Entry {
            name,
            method,
            crc,
            size,
            compressed,
            at,
        });
    }
    if entries.len() != here as usize {
        return Err("its zip's directory holds fewer entries than the archive says".into());
    }
    Ok(entries)
}

/// Where the end record of `zip` is: the last place its signature sits,
/// counted from the end of the archive, since a comment may follow the
/// record but nothing else may.
fn find_end(zip: &[u8]) -> Result<usize, String> {
    let most = zip.len().min(MOST_COMMENT + 22);
    let tail = &zip[zip.len() - most..];
    let from_end = tail
        .windows(4)
        .rev()
        .position(|window| u32_of(window) == END_RECORD)
        .ok_or("its zip ends with no directory")?;
    Ok(zip.len() - 4 - from_end)
}

/// The file of `entry`, read from its local header, checked against what
/// the directory said and against its own CRC32.
fn read_entry(zip: &[u8], entry: &Entry) -> Result<Vec<u8>, String> {
    let mut read = zip
        .get(entry.at as usize..)
        .filter(|header| header.len() >= 30)
        .ok_or("its zip ends inside a file of the archive")?;
    if u32(&mut read) != LOCAL_HEADER {
        return Err("its zip's directory does not point at the files it names".into());
    }
    let _ = u16(&mut read); // the version this entry needs
    let flags = u16(&mut read);
    if flags & ENCRYPTED != 0 {
        return Err("its zip holds an encrypted entry, which Pane does not read".into());
    }
    let method = u16(&mut read);
    if method != entry.method {
        return Err(format!(
            "its zip gives `{}` two compression methods",
            entry.name
        ));
    }
    let _ = u16(&mut read); // the time
    let _ = u16(&mut read); // the date
    let crc = u32(&mut read);
    let compressed = u32(&mut read) as u64;
    let size = u32(&mut read) as u64;
    // The name of the local header, of the length it names itself.
    let name_len = u16(&mut read) as usize;
    let extra = u16(&mut read) as usize;
    if read.len() < name_len + extra {
        return Err("its zip ends inside a file of the archive".into());
    }
    let name = String::from_utf8(read[..name_len].to_vec())
        .map_err(|_| "its zip holds a file name that is not valid UTF-8".to_owned())?;
    read = &read[name_len + extra..];
    if crc != entry.crc || compressed != entry.compressed || size != entry.size {
        return Err(format!(
            "its zip gives `{}` two sizes or checks, which Pane unpacks only when they are \
             the same",
            entry.name
        ));
    }
    if name != entry.name {
        return Err(format!(
            "its zip's directory names `{}`, where the file itself says `{name}`",
            entry.name
        ));
    }
    let packed = read
        .get(..compressed as usize)
        .ok_or("its zip ends inside a file of the archive")?;
    let unpacked = match method {
        STORED => packed.to_vec(),
        DEFLATE => {
            let mut unpacked = Vec::new();
            flate2::read::DeflateDecoder::new(packed)
                .take(MAX_UNPACKED + (MAX_UNPACKED >> 2))
                .read_to_end(&mut unpacked)
                .map_err(|error| {
                    format!("its zip's `{}` cannot be inflated: {error}", entry.name)
                })?;
            unpacked
        }
        _ => unreachable!("the directory checked the method"),
    };
    if unpacked.len() as u64 != size {
        return Err(format!(
            "its zip's `{}` is not the size its entry says",
            entry.name
        ));
    }
    if crc32fast::hash(&unpacked) != crc {
        return Err(format!(
            "its zip's `{}` does not match the check its entry names",
            entry.name
        ));
    }
    Ok(unpacked)
}

/// A little-endian `u16` from the front of `read`, which it advances.
fn u16(read: &mut &[u8]) -> u16 {
    let value = u16::from_le_bytes([read[0], read[1]]);
    *read = &read[2..];
    value
}

/// A little-endian `u32` from the front of `read`, which it advances.
fn u32(read: &mut &[u8]) -> u32 {
    let value = u32_of(&read[..4]);
    *read = &read[4..];
    value
}

fn u32_of(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The end record's length.
    const END_RECORD_LEN: usize = 22;
    /// The name of the one file these archives hold.
    const NAME: &str = "pane/pane.exe";

    /// A zip holding `files` (path in the archive, contents) under
    /// `pane/`, stored or deflated as `stored` says: the same shape the
    /// test support and `xtask`'s writer pack.
    fn zip(files: &[(&str, Vec<u8>)], stored: bool) -> Vec<u8> {
        let mut archive: Vec<u8> = Vec::new();
        let mut entries: Vec<(String, u32, u64, Vec<u8>, u64)> = Vec::new();
        for (path, contents) in files {
            let name = format!("pane/{path}");
            let packed = if stored {
                contents.clone()
            } else {
                let mut deflated =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
                std::io::Write::write_all(&mut deflated, contents).unwrap();
                deflated.finish().unwrap()
            };
            let crc = crc32fast::hash(contents);
            let at = archive.len() as u64;
            push_u32(&mut archive, LOCAL_HEADER);
            push_u16(&mut archive, 20);
            push_u16(&mut archive, 0);
            push_u16(&mut archive, if stored { STORED } else { DEFLATE });
            push_u16(&mut archive, 0);
            push_u16(&mut archive, 0);
            push_u32(&mut archive, crc);
            push_u32(&mut archive, packed.len() as u32);
            push_u32(&mut archive, contents.len() as u32);
            push_u16(&mut archive, name.len() as u16);
            push_u16(&mut archive, 0);
            archive.extend_from_slice(name.as_bytes());
            archive.extend_from_slice(&packed);
            entries.push((name, crc, contents.len() as u64, packed, at));
        }
        let directory_at = archive.len() as u64;
        for (name, crc, size, packed, at) in &entries {
            push_u32(&mut archive, DIRECTORY_ENTRY);
            push_u16(&mut archive, 20);
            push_u16(&mut archive, 20);
            push_u16(&mut archive, 0);
            push_u16(&mut archive, if stored { STORED } else { DEFLATE });
            push_u16(&mut archive, 0);
            push_u16(&mut archive, 0);
            push_u32(&mut archive, *crc);
            push_u32(&mut archive, packed.len() as u32);
            push_u32(&mut archive, *size as u32);
            push_u16(&mut archive, name.len() as u16);
            push_u16(&mut archive, 0);
            push_u16(&mut archive, 0);
            push_u16(&mut archive, 0);
            push_u16(&mut archive, 0);
            push_u32(&mut archive, 0);
            push_u32(&mut archive, *at as u32);
            archive.extend_from_slice(name.as_bytes());
        }
        let directory_size = (archive.len() as u64 - directory_at) as u32;
        push_u32(&mut archive, END_RECORD);
        push_u16(&mut archive, 0);
        push_u16(&mut archive, 0);
        push_u16(&mut archive, entries.len() as u16);
        push_u16(&mut archive, entries.len() as u16);
        push_u32(&mut archive, directory_size);
        push_u32(&mut archive, directory_at as u32);
        push_u16(&mut archive, 0);
        archive
    }

    fn files_of(dest: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn read(folder: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in fs::read_dir(folder).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    read(&path, out);
                } else {
                    out.push((path.clone(), fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        read(dest, &mut out);
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Reading the same files stored and deflated gives the same files,
    /// without the archive's top folder.
    #[test]
    fn a_zip_is_unpacked_without_its_top_folder() {
        for stored in [true, false] {
            let archive = zip(
                &[
                    ("pane.exe", b"the program".to_vec()),
                    ("README.txt", b"the readme".to_vec()),
                ],
                stored,
            );
            // The destination must not exist, as an npm tarball's does not.
            let dest = tempfile::tempdir().unwrap();
            let unpacked = dest.path().join("unpacked");
            unpack(&archive, &unpacked).unwrap();
            assert_eq!(
                files_of(&unpacked),
                vec![
                    (unpacked.join("README.txt"), b"the readme".to_vec()),
                    (unpacked.join("pane.exe"), b"the program".to_vec()),
                ]
            );
        }
    }

    /// Where the one file's central-directory entry starts, and its fields.
    fn directory_entry(archive: &[u8]) -> (usize, usize, usize, usize) {
        let start = archive.len() - END_RECORD_LEN - DIRECTORY_FIXED - NAME.len();
        // The compression method, the CRC and the sizes within the entry.
        (start, start + 10, start + 16, start + 24)
    }

    /// The first file the reader cannot take refuses the whole archive,
    /// and nothing is written.
    #[test]
    fn a_zip_that_cannot_be_taken_is_refused() {
        let files = [("pane.exe", b"the program".to_vec())];
        let dest = tempfile::tempdir().unwrap();
        // A name that climbs out of the package.
        let escaping = zip(&[("../pane.exe", b"x".to_vec())], true);
        let why = unpack(&escaping, &dest.path().join("a")).unwrap_err();
        assert!(why.contains("climbs out with `..`"), "{why}");

        // A local header the directory does not point at.
        let mut misplaced = zip(&files, true);
        misplaced[0..4].copy_from_slice(&0x1111_1111_u32.to_le_bytes());
        let why = unpack(&misplaced, &dest.path().join("b")).unwrap_err();
        assert!(
            why.contains("does not point at the files it names"),
            "{why}"
        );

        // A CRC32 that does not match the file's bytes: both the directory
        // and the local header say a wrong one, so the bytes are what fail.
        let mut damaged = zip(&files, true);
        let (_, _, central_crc, _) = directory_entry(&damaged);
        let local_crc = 14;
        for at in [central_crc, local_crc] {
            damaged[at..at + 4].copy_from_slice(&0x4141_4141_u32.to_le_bytes());
        }
        let why = unpack(&damaged, &dest.path().join("c")).unwrap_err();
        assert!(why.contains("does not match the check"), "{why}");

        // A local size that is not the directory's.
        let mut resized = zip(&files, true);
        resized[24..28].copy_from_slice(&9_u32.to_le_bytes());
        let why = unpack(&resized, &dest.path().join("d")).unwrap_err();
        assert!(why.contains("two sizes or checks"), "{why}");

        // A compression method Pane does not read, in the directory.
        let mut other = zip(&files, true);
        let (_, method, _, _) = directory_entry(&other);
        other[method..method + 2].copy_from_slice(&12_u16.to_le_bytes());
        let why = unpack(&other, &dest.path().join("e")).unwrap_err();
        assert!(
            why.contains("a compression method Pane does not read"),
            "{why}"
        );

        // Stored bytes said to be deflated, in both places: they are not.
        let mut mixed = zip(&files, true);
        let local_method = 8;
        let (_, method, _, _) = directory_entry(&mixed);
        for at in [method, local_method] {
            mixed[at..at + 2].copy_from_slice(&DEFLATE.to_le_bytes());
        }
        let why = unpack(&mixed, &dest.path().join("f")).unwrap_err();
        assert!(
            why.contains("cannot be inflated") || why.contains("the size"),
            "{why}"
        );

        // An archive that ends inside its end record.
        let mut short = zip(&files, true);
        short.truncate(short.len() - 10);
        let why = unpack(&short, &dest.path().join("g")).unwrap_err();
        assert!(why.contains("its end record"), "{why}");

        // A file the archive does not hold: its local header is somewhere
        // past the archive's end.
        let mut gone = zip(&files, true);
        let gone_at = gone.len() - END_RECORD_LEN - DIRECTORY_FIXED - NAME.len() + 42;
        gone[gone_at..gone_at + 4].copy_from_slice(&(1_u32 << 30).to_le_bytes());
        let why = unpack(&gone, &dest.path().join("h")).unwrap_err();
        assert!(why.contains("ends inside a file of the archive"), "{why}");

        // A zip64 marker in the end record.
        let mut sixty_four = zip(&files, true);
        let at = sixty_four.len() - END_RECORD_LEN + 16;
        sixty_four[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let why = unpack(&sixty_four, &dest.path().join("j")).unwrap_err();
        assert!(why.contains("zip64"), "{why}");

        // A directory that does not say where it is.
        let mut nowhere = zip(&files, true);
        let at = nowhere.len() - END_RECORD_LEN + 16;
        nowhere[at..at + 4].copy_from_slice(&(1_u32 << 20).to_le_bytes());
        let why = unpack(&nowhere, &dest.path().join("i")).unwrap_err();
        assert!(why.contains("reaches past the archive's end"), "{why}");

        // Nothing was written for any of them.
        assert_eq!(files_of(dest.path()), Vec::<(PathBuf, Vec<u8>)>::new());
    }

    fn push_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }
}
