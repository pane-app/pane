//! Packing Pane's Windows package as a zip (#51).
//!
//! A Windows user unzips a package with whatever is at hand — Explorer,
//! `Expand-Archive`, any unzip — and Windows has no tar a user can rely on,
//! so the Windows package is a zip rather than the Linux package's
//! tarball. It is written here rather than through a zip library so that
//! its bytes are fixed, as the tarballs' are: every entry in name order,
//! one fixed time (npm's, the same the tarballs carry), no extra fields,
//! no comments, and the same `flate2` deflate the tarballs' gzip uses. The
//! tests below pin the bytes of a known input.
//!
//! What is written is the plain zip every reader expects: for each file a
//! local header and its deflated bytes, then one central-directory entry
//! per file, then the end record. Sizes are known before anything is
//! written (everything is packed in memory), so no data descriptor and no
//! zip64 record is needed: a file, name or archive beyond zip's 16- or
//! 32-bit bounds is refused rather than silently mispacked.

/// A local file header: the beginning of one file's entry.
const LOCAL_HEADER: u32 = 0x0403_4b50;
/// A central-directory entry, one per file, after every file's bytes.
const DIRECTORY_ENTRY: u32 = 0x0201_4b50;
/// The end of the central directory: the last thing in the archive.
const END_RECORD: u32 = 0x0605_4b50;
/// The version that made this archive, and the one needed to read it:
/// 2.0, the one deflate requires, written as MS-DOS so no reader looks
/// for Unix permissions a zip of Pane's package has no use for.
const VERSION: u16 = 20;
/// The deflate compression method.
const DEFLATE: u16 = 8;

/// One file's directory entry: what the central directory says about it.
struct Entry {
    name: String,
    crc: u32,
    size: u64,
    compressed: u64,
    /// Where the file's local header is, from the archive's beginning.
    at: u64,
}

/// Packs `files` (path in the archive, contents) as a zip holding them
/// under `prefix/`, in name order. The result is the same bytes on every
/// system, so the package's sha256 is too.
pub(crate) fn pack(files: &[(String, Vec<u8>)], prefix: &str) -> Result<Vec<u8>, String> {
    let mut files: Vec<&(String, Vec<u8>)> = files.iter().collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let (date, time) = dos_time(crate::PACKED_MTIME);
    let mut zip: Vec<u8> = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();
    for (path, contents) in files {
        let name = format!("{prefix}/{path}");
        if name.len() > u16::MAX as usize {
            return Err(format!("{name}: a zip names no path this long"));
        }
        let mut deflate =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        std::io::Write::write_all(&mut deflate, contents)
            .map_err(|error| format!("deflating {name} failed: {error}"))?;
        let compressed = deflate
            .finish()
            .map_err(|error| format!("deflating {name} failed: {error}"))?;
        let size = contents.len() as u64;
        let compressed_size = compressed.len() as u64;
        let crc = crc32fast::hash(contents);
        let at = zip.len() as u64;
        for (bound, what) in [(size, "the file"), (compressed_size, "the packed file")] {
            if bound >= u32::MAX as u64 {
                return Err(format!(
                    "{name}: {what} is larger than a plain zip holds; zip64 is not packed"
                ));
            }
        }
        if at >= u32::MAX as u64 {
            return Err(format!(
                "{name}: the archive is larger than a plain zip holds; zip64 is not packed"
            ));
        }
        push_u32(&mut zip, LOCAL_HEADER);
        push_u16(&mut zip, VERSION);
        push_u16(&mut zip, 0); // no flags: the sizes and digest are here, not after the bytes
        push_u16(&mut zip, DEFLATE);
        push_u16(&mut zip, time);
        push_u16(&mut zip, date);
        push_u32(&mut zip, crc);
        push_u32(&mut zip, compressed_size as u32);
        push_u32(&mut zip, size as u32);
        push_u16(&mut zip, name.len() as u16);
        push_u16(&mut zip, 0); // no extra field
        zip.extend_from_slice(name.as_bytes());
        zip.extend_from_slice(&compressed);
        entries.push(Entry {
            name,
            crc,
            size,
            compressed: compressed_size,
            at,
        });
    }
    let directory_at = zip.len() as u64;
    for entry in &entries {
        push_u32(&mut zip, DIRECTORY_ENTRY);
        push_u16(&mut zip, VERSION);
        push_u16(&mut zip, VERSION);
        push_u16(&mut zip, 0); // flags
        push_u16(&mut zip, DEFLATE);
        push_u16(&mut zip, time);
        push_u16(&mut zip, date);
        push_u32(&mut zip, entry.crc);
        push_u32(&mut zip, entry.compressed as u32);
        push_u32(&mut zip, entry.size as u32);
        push_u16(&mut zip, entry.name.len() as u16);
        push_u16(&mut zip, 0); // extra
        push_u16(&mut zip, 0); // comment
        push_u16(&mut zip, 0); // the first disk
        push_u16(&mut zip, 0); // internal attributes
        push_u32(&mut zip, 0); // external attributes: none
        push_u32(&mut zip, entry.at as u32);
        zip.extend_from_slice(entry.name.as_bytes());
    }
    let directory_size = (zip.len() as u64 - directory_at) as u32;
    if entries.len() >= u16::MAX as usize {
        return Err("the archive holds more files than a plain zip lists".to_owned());
    }
    push_u32(&mut zip, END_RECORD);
    push_u16(&mut zip, 0); // this disk
    push_u16(&mut zip, 0); // the disk holding the directory
    push_u16(&mut zip, entries.len() as u16);
    push_u16(&mut zip, entries.len() as u16);
    push_u32(&mut zip, directory_size);
    push_u32(&mut zip, directory_at as u32);
    push_u16(&mut zip, 0); // no comment
    Ok(zip)
}

/// The zip's fixed time as a DOS date and time pair: npm's fixed time
/// (1985-10-26 08:15:00), the same the tarballs carry, so a zip entry
/// names the time the tar entries do.
fn dos_time(mtime: u64) -> (u16, u16) {
    let days = (mtime / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    let date = (((year - 1980) as u16) << 9) | ((month as u16) << 5) | day as u16;
    let seconds_of_day = mtime % 86_400;
    let hour = (seconds_of_day / 3_600) as u16;
    let minute = (seconds_of_day % 3_600 / 60) as u16;
    let second = (seconds_of_day % 60 / 2) as u16; // a DOS time counts two-second pairs
    (date, (hour << 11) | (minute << 5) | second)
}

/// The Gregorian date of `days` days after 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = (days - era * 146_097) as u64; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_of_year = (5 * day_of_year + 2) / 153; // [0, 11]
    let day = (day_of_year - (153 * month_of_year + 2) / 5 + 1) as u32; // [1, 31]
    let month = if month_of_year < 10 {
        month_of_year + 3
    } else {
        month_of_year - 9
    }; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month as u32, day)
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// The zip of a known input is the same bytes whatever order the files
    /// are given in, and those bytes are the recorded ones: the format was
    /// first checked by reading a zip this wrote with Python's `zipfile`
    /// module, and this pins the bytes so a change cannot pass unnoticed.
    /// They are the same on every system, so the digest is too.
    #[test]
    fn the_bytes_of_a_known_input_are_fixed() {
        let files = vec![
            ("pane.exe".to_owned(), b"the program".to_vec()),
            ("install.ps1".to_owned(), b"# the install script".to_vec()),
            ("README.txt".to_owned(), b"the readme".to_vec()),
        ];
        let packed = pack(&files, "pane").expect("packing into memory");
        let reversed: Vec<(String, Vec<u8>)> = files.iter().rev().cloned().collect();
        assert_eq!(
            packed,
            pack(&reversed, "pane").expect("packing into memory")
        );
        let digest: String = Sha256::digest(&packed)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            digest,
            "a0ab487a332d71c3888c9d8739cabaa36e3139c4e90b43f342c53f09f0ca4ce2"
        );
    }

    /// The fixed time a zip entry names is the one the tar entries name,
    /// read back as a date and time.
    #[test]
    fn the_time_is_npms() {
        let (date, time) = dos_time(crate::PACKED_MTIME);
        // 1985-10-26 08:15:00, as a DOS date and time.
        assert_eq!(date, (5 << 9) | (10 << 5) | 26);
        assert_eq!(time, (8 << 11) | (15 << 5)); // hour, minute; seconds 0
    }
}
