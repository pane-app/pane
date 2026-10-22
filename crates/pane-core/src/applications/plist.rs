//! Just enough of Apple's property lists to read a string from a bundle's
//! `Info.plist`, such as its `CFBundleIdentifier`: the XML form and the
//! binary form (`bplist00`, which most of the system's own bundles use).
//! Plain byte work, so it is compiled and tested on every system.

/// The string value of `key` in the property list `data`'s top dictionary
/// (in the XML form, its first `<key>` element with that name); `None` if
/// it has none, the value is not a string, or `data` is not a property list
/// this reads.
pub(super) fn string(data: &[u8], key: &str) -> Option<String> {
    if data.starts_with(b"bplist00") {
        binary_string(data, key)
    } else {
        xml_string(std::str::from_utf8(data).ok()?, key)
    }
}

/// The XML form: the `<string>` element after `<key>key</key>`, with
/// nothing but white space and comments between them.
fn xml_string(text: &str, key: &str) -> Option<String> {
    let wanted = format!("<key>{}</key>", escape(key));
    let after = &text[text.find(&wanted)? + wanted.len()..];
    let mut rest = after.trim_start();
    while let Some(comment) = rest.strip_prefix("<!--") {
        rest = comment[comment.find("-->")? + 3..].trim_start();
    }
    let value = rest.strip_prefix("<string>")?;
    Some(unescape(&value[..value.find("</string>")?]))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `text` with XML's predefined entities and character references
/// replaced.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';') else {
            break;
        };
        let entity = &rest[1..end];
        let character = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .map(|hex| u32::from_str_radix(hex, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse::<u32>))
                .and_then(Result::ok)
                .and_then(char::from_u32),
        };
        match character {
            Some(character) => {
                out.push(character);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A big-endian unsigned number of 1 to 8 bytes at `at`.
fn number(data: &[u8], at: usize, size: usize) -> Option<u64> {
    if size == 0 || size > 8 {
        return None;
    }
    let bytes = data.get(at..at.checked_add(size)?)?;
    Some(
        bytes
            .iter()
            .fold(0u64, |value, byte| (value << 8) | u64::from(*byte)),
    )
}

/// The binary form: its trailer gives the offset table, which locates
/// each object; the top object must be a dictionary.
fn binary_string(data: &[u8], key: &str) -> Option<String> {
    let trailer = data.get(data.len().checked_sub(32)?..)?;
    let offset_size = usize::from(trailer[6]);
    let reference_size = usize::from(trailer[7]);
    let count = number(trailer, 8, 8)?;
    let top = number(trailer, 16, 8)?;
    let table = usize::try_from(number(trailer, 24, 8)?).ok()?;
    let object = |index: u64| -> Option<usize> {
        if index >= count {
            return None;
        }
        let at = table.checked_add(usize::try_from(index).ok()?.checked_mul(offset_size)?)?;
        usize::try_from(number(data, at, offset_size)?).ok()
    };
    let reference = |at: usize, index: usize| -> Option<u64> {
        number(
            data,
            at.checked_add(index.checked_mul(reference_size)?)?,
            reference_size,
        )
    };
    let dictionary = object(top)?;
    if data.get(dictionary)? >> 4 != 0xD {
        return None;
    }
    let (entries, keys) = length(data, dictionary)?;
    for index in 0..entries {
        if binary_text(data, object(reference(keys, index)?)?).as_deref() == Some(key) {
            let value = reference(keys, entries.checked_add(index)?)?;
            return binary_text(data, object(value)?);
        }
    }
    None
}

/// The length of the object at `at` and where its contents start: the low
/// four bits of its marker, or the integer object after it when those are
/// all set.
fn length(data: &[u8], at: usize) -> Option<(usize, usize)> {
    let low = data.get(at)? & 0x0F;
    if low != 0x0F {
        return Some((usize::from(low), at + 1));
    }
    let marker = *data.get(at + 1)?;
    if marker >> 4 != 0x1 {
        return None;
    }
    let size = 1usize << (marker & 0x0F);
    let length = usize::try_from(number(data, at + 2, size)?).ok()?;
    Some((length, at + 2 + size))
}

/// The string object at `at`: ASCII, or UTF-16 big-endian.
fn binary_text(data: &[u8], at: usize) -> Option<String> {
    let kind = data.get(at)? >> 4;
    let (length, start) = length(data, at)?;
    match kind {
        0x5 => {
            let bytes = data.get(start..start.checked_add(length)?)?;
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
        0x6 => {
            let bytes = data.get(start..start.checked_add(length.checked_mul(2)?)?)?;
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_be_bytes(*pair))
                .collect();
            String::from_utf16(&units).ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
        <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
        \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
        <plist version=\"1.0\">\n<dict>\n\
        \t<key>CFBundleExecutable</key>\n\t<string>marker</string>\n\
        \t<key>CFBundleIdentifier</key>\n\t<!-- the identity -->\n\
        \t<string>dev.pane.Marker&amp;Co</string>\n\
        \t<key>LSUIElement</key>\n\t<true/>\n\
        </dict>\n</plist>\n";

    #[test]
    fn a_string_is_read_from_the_xml_form() {
        assert_eq!(
            string(XML.as_bytes(), "CFBundleIdentifier").as_deref(),
            Some("dev.pane.Marker&Co")
        );
        assert_eq!(
            string(XML.as_bytes(), "CFBundleExecutable").as_deref(),
            Some("marker")
        );
        // Not a string, or not there.
        assert_eq!(string(XML.as_bytes(), "LSUIElement"), None);
        assert_eq!(string(XML.as_bytes(), "CFBundleName"), None);
        assert_eq!(string(b"not a plist", "CFBundleIdentifier"), None);
    }

    #[test]
    fn character_references_are_replaced() {
        assert_eq!(unescape("a&#x41;&#66;&lt;&unknown;b"), "aAB<&unknown;b");
    }

    /// A binary property list holding one dictionary of `entries`, each
    /// value a string (ASCII, or UTF-16 when it is not ASCII), as
    /// `plutil -convert binary1` writes one.
    fn binary(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut data = b"bplist00".to_vec();
        let mut offsets = Vec::new();
        let count = 1 + 2 * entries.len();
        // The dictionary, object 0: its key references, then its values'.
        offsets.push(data.len());
        data.push(0xD0 | u8::try_from(entries.len()).unwrap());
        for index in 0..entries.len() {
            data.push(u8::try_from(1 + index).unwrap());
        }
        for index in 0..entries.len() {
            data.push(u8::try_from(1 + entries.len() + index).unwrap());
        }
        let text = |data: &mut Vec<u8>, offsets: &mut Vec<usize>, text: &str| {
            offsets.push(data.len());
            if text.is_ascii() {
                if text.len() < 15 {
                    data.push(0x50 | u8::try_from(text.len()).unwrap());
                } else {
                    data.extend([0x5F, 0x10, u8::try_from(text.len()).unwrap()]);
                }
                data.extend(text.as_bytes());
            } else {
                let units: Vec<u16> = text.encode_utf16().collect();
                data.push(0x60 | u8::try_from(units.len()).unwrap());
                for unit in units {
                    data.extend(unit.to_be_bytes());
                }
            }
        };
        for (key, _) in entries {
            text(&mut data, &mut offsets, key);
        }
        for (_, value) in entries {
            text(&mut data, &mut offsets, value);
        }
        let table = data.len();
        for offset in &offsets {
            data.push(u8::try_from(*offset).unwrap());
        }
        data.extend([0; 6]);
        data.extend([1, 1]);
        data.extend(u64::try_from(count).unwrap().to_be_bytes());
        data.extend(0u64.to_be_bytes());
        data.extend(u64::try_from(table).unwrap().to_be_bytes());
        data
    }

    #[test]
    fn a_string_is_read_from_the_binary_form() {
        let data = binary(&[
            ("CFBundleName", "Café"),
            ("CFBundleIdentifier", "com.apple.calculator"),
        ]);
        assert_eq!(
            string(&data, "CFBundleIdentifier").as_deref(),
            Some("com.apple.calculator")
        );
        assert_eq!(string(&data, "CFBundleName").as_deref(), Some("Café"));
        assert_eq!(string(&data, "CFBundleExecutable"), None);
    }

    #[test]
    fn a_damaged_binary_form_reads_nothing_and_does_not_panic() {
        let data = binary(&[("CFBundleIdentifier", "com.example.tool")]);
        for end in 0..data.len() {
            let _ = string(&data[..end], "CFBundleIdentifier");
        }
        let mut corrupt = data.clone();
        let table = corrupt.len() - 32 - 3;
        corrupt[table] = 0xFF;
        assert_eq!(string(&corrupt, "CFBundleIdentifier"), None);
    }
}
