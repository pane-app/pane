//! Checking downloads against the sha512 integrity their source gives
//! (`sha512-<base64>`, as npm writes it): an npm package's tarball.

use sha2::{Digest, Sha512};

/// The base64 values of the sha512 hashes in the integrity string
/// `integrity` (space-separated `<algorithm>-<base64>[?options]`).
pub(crate) fn sha512_values(integrity: &str) -> impl Iterator<Item = &str> {
    integrity.split_whitespace().filter_map(|hash| {
        let value = hash.strip_prefix("sha512-")?;
        Some(value.split('?').next().unwrap_or(value))
    })
}

/// Checks `bytes` against the sha512 hashes of `integrity`: they match when
/// one of them does.
pub(crate) fn check_integrity(bytes: &[u8], integrity: &str) -> Result<(), String> {
    let actual = base64(&Sha512::digest(bytes));
    let mut values = sha512_values(integrity).peekable();
    if values.peek().is_none() {
        return Err("has no sha512 integrity to be checked against".into());
    }
    if values.any(|expected| expected == actual) {
        Ok(())
    } else {
        Err(format!(
            "does not match the sha512 integrity the registry gives (it is sha512-{actual})"
        ))
    }
}

/// Standard base64 with padding.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                text.push(ALPHABET[(n >> shift) as usize & 63] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// `bytes` as lowercase hexadecimal digits, two to a byte: the sha256 a
/// release's `.sha256` file gives, and the tests' object ids.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrity_is_checked_against_sha512() {
        let bytes = b"the tarball";
        let good = format!("sha512-{}", base64(&Sha512::digest(bytes)));
        assert_eq!(check_integrity(bytes, &good), Ok(()));
        // Several hashes: any sha512 one matching is enough.
        assert_eq!(check_integrity(bytes, &format!("sha1-abc {good}")), Ok(()));
        let error = check_integrity(b"another tarball", &good).unwrap_err();
        assert!(
            error.starts_with("does not match the sha512 integrity"),
            "{error}"
        );
        let error = check_integrity(bytes, "sha1-abc").unwrap_err();
        assert!(error.contains("no sha512"), "{error}");
    }

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }
}
