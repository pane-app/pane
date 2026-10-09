//! The protector between extension data in memory and its files (#130):
//! local credentials and clipboard history are written protected where the
//! system offers a protector, and read back plain. Values stay plain in
//! memory while Pane runs; only the files change.
//!
//! - Windows: DPAPI (`CryptProtectData`) in the current user's scope, with
//!   Pane's own entropy ([`ENTROPY`]) and the flag that forbids any prompt
//!   (`CRYPTPROTECT_UI_FORBIDDEN`). Only the same Windows user can decrypt
//!   it, on the same computer (or wherever a domain account's keys roam).
//!   A copy of Pane's data folder (a backup, a synced profile, a disk read
//!   outside Windows) or another user of the computer gets no value.
//! - macOS and Linux: none in this slice. The files stay plain, readable by
//!   the user only (mode 0600). The macOS Keychain and the Secret Service
//!   are later slices of #129.
//!
//! A file records how each value is protected ([`Stored`]), so a value
//! protected by another system, or one Windows can no longer decrypt (an
//! administrator reset the user's password, the folder came from another
//! user or computer, the bytes were damaged), is explained, never dropped
//! or replaced on Pane's own.
//!
//! What it does not protect against: a program running as the same user
//! can call DPAPI too, and Pane's entropy is in its program. Extension data
//! stays "lifecycle behavior, not secret isolation from trusted code".

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Whether this system protects values before writing them: Windows only,
/// in this slice.
pub(crate) const PROTECTS: bool = cfg!(windows);

/// The entropy Pane adds to every value it protects with DPAPI, so that
/// another program calling DPAPI for the same user must know it too. Fixed
/// for good: a value protected with other entropy cannot be decrypted. The
/// smokes' `scripts/clipboard_history.py` uses the same bytes.
#[cfg_attr(not(windows), allow(dead_code))]
const ENTROPY: &[u8] = b"Pane extension data, protected for this user (#130)";

/// A value as a file holds it, recording how it is protected:
/// `{"plain": "…"}` or `{"dpapi": "<base64>"}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Stored {
    /// As it is, written where the system has no protector.
    Plain(String),
    /// Encrypted with DPAPI for the Windows user who wrote it, in base64.
    Dpapi(String),
}

impl Stored {
    /// `plain` protected with this system's protector; as it is where the
    /// system has none. Fails only when the protector does, saying why.
    pub fn protect(plain: &str) -> Result<Stored, String> {
        #[cfg(windows)]
        {
            dpapi::protect(plain.as_bytes())
                .map(|bytes| Stored::Dpapi(crate::integrity::base64(&bytes)))
                .map_err(|reason| format!("Windows could not encrypt it ({reason})"))
        }
        #[cfg(not(windows))]
        {
            Ok(Stored::Plain(plain.to_owned()))
        }
    }

    /// Whether it is written as it is.
    pub fn is_plain(&self) -> bool {
        matches!(self, Stored::Plain(_))
    }

    /// The text it holds, or why it cannot be read on this computer, such
    /// as "Windows could not decrypt it (Key not valid for use in specified
    /// state)".
    pub fn open(&self) -> Result<String, String> {
        match self {
            Stored::Plain(text) => Ok(text.clone()),
            Stored::Dpapi(encoded) => open_dpapi(encoded),
        }
    }
}

#[cfg(windows)]
fn open_dpapi(encoded: &str) -> Result<String, String> {
    let failed = |reason: &str| format!("Windows could not decrypt it ({reason})");
    let bytes =
        crate::icons::decode_base64(encoded).ok_or_else(|| failed("its bytes are not base64"))?;
    let plain = dpapi::unprotect(&bytes).map_err(|reason| failed(&reason))?;
    String::from_utf8(plain).map_err(|_| failed("what it holds is not text"))
}

#[cfg(not(windows))]
fn open_dpapi(_encoded: &str) -> Result<String, String> {
    Err("Windows encrypted it, and only that Windows user can decrypt it".into())
}

/// What Pane says of a value of what is called `what` ("credential",
/// "copy") that cannot be read on this computer, given why: "Pane cannot
/// read this credential on this computer: Windows could not decrypt it
/// (…)".
pub(crate) fn cannot_read(what: &str, why: &str) -> String {
    format!("Pane cannot read this {what} on this computer: {why}")
}

/// One value of extension data in memory: as its file holds it, and what it
/// reads as. A protected value is decrypted the first time it is read, and
/// never to count, move or remove it.
#[derive(Clone, Debug)]
pub(crate) struct Value {
    stored: Stored,
    /// What a protected value reads as, once read: its text, or why it
    /// cannot be read on this computer.
    read: OnceLock<Result<String, String>>,
}

impl Value {
    /// `text`, written as it is.
    pub fn plain(text: String) -> Value {
        Value {
            stored: Stored::Plain(text),
            read: OnceLock::new(),
        }
    }

    /// A value as a file held it.
    pub fn from_stored(stored: Stored) -> Value {
        Value {
            stored,
            read: OnceLock::new(),
        }
    }

    /// `text` protected with this system's protector ([`Stored::protect`]);
    /// it reads as `text` without being decrypted again.
    pub fn protect(text: String) -> Result<Value, String> {
        let stored = Stored::protect(&text)?;
        let read = OnceLock::new();
        if !stored.is_plain() {
            let _ = read.set(Ok(text));
        }
        Ok(Value { stored, read })
    }

    /// Protects it with this system's protector if it is written as it is;
    /// whether it was.
    pub fn protect_in_place(&mut self) -> Result<bool, String> {
        let Stored::Plain(text) = &self.stored else {
            return Ok(false);
        };
        let protected = Value::protect(text.clone())?;
        if protected.is_plain() {
            return Ok(false);
        }
        *self = protected;
        Ok(true)
    }

    /// As its file holds it.
    pub fn stored(&self) -> &Stored {
        &self.stored
    }

    /// Whether it is written as it is.
    pub fn is_plain(&self) -> bool {
        self.stored.is_plain()
    }

    /// Its text if it is written as it is.
    pub fn plain_text(&self) -> Option<&str> {
        match &self.stored {
            Stored::Plain(text) => Some(text),
            Stored::Dpapi(_) => None,
        }
    }

    /// What it holds, or why it cannot be read on this computer; a
    /// protected value is decrypted the first time.
    pub fn text(&self) -> Result<&str, &str> {
        match &self.stored {
            Stored::Plain(text) => Ok(text),
            stored => self
                .read
                .get_or_init(|| stored.open())
                .as_deref()
                .map_err(String::as_str),
        }
    }
}

/// DPAPI in the current user's scope.
#[cfg(windows)]
mod dpapi {
    use std::ptr;

    use ::windows::Win32::Foundation::{HLOCAL, LocalFree};
    use ::windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    use ::windows::core::PCWSTR;

    use super::ENTROPY;

    /// A blob naming `bytes`, which the call only reads.
    fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, String> {
        Ok(CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(bytes.len()).map_err(|_| "it is too large".to_owned())?,
            pbData: bytes.as_ptr().cast_mut(),
        })
    }

    /// The bytes a call returned in `out`, which it allocated; freed.
    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        // SAFETY: the call succeeded and wrote `cbData` bytes at `pbData`.
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        // SAFETY: allocated by the call with LocalAlloc, and freed once.
        unsafe { LocalFree(Some(HLOCAL(out.pbData.cast()))) };
        bytes
    }

    /// Why a call failed, as Windows says it, without a final full stop.
    fn reason(error: &::windows::core::Error) -> String {
        error.message().trim().trim_end_matches('.').to_owned()
    }

    /// `plain` encrypted for the current user, with Pane's entropy, never
    /// prompting.
    pub(super) fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(plain)?;
        let entropy = blob(ENTROPY)?;
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` and `entropy` name bytes that outlive the call,
        // which only reads them; `out` is written by it and freed by `take`.
        unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(ptr::from_ref(&entropy)),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
        .map_err(|error| reason(&error))?;
        Ok(take(out))
    }

    /// `protected` decrypted for the current user, with Pane's entropy,
    /// never prompting; or why Windows could not.
    pub(super) fn unprotect(protected: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(protected)?;
        let entropy = blob(ENTROPY)?;
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as for `protect`.
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(ptr::from_ref(&entropy)),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
        .map_err(|error| reason(&error))?;
        Ok(take(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_reads_back_as_it_was_kept() {
        let value = Value::protect("sample-token".into()).unwrap();
        assert_eq!(value.text(), Ok("sample-token"));
        assert_eq!(value.is_plain(), !PROTECTS);
        // As a file holds it, then read again.
        let json = serde_json::to_string(value.stored()).unwrap();
        assert_eq!(json.contains("sample-token"), !PROTECTS, "{json}");
        let read = Value::from_stored(serde_json::from_str(&json).unwrap());
        assert_eq!(read.text(), Ok("sample-token"));
    }

    #[test]
    fn a_plain_value_is_protected_in_place_where_the_system_protects() {
        let mut value = Value::plain("sample-token".into());
        assert_eq!(value.protect_in_place(), Ok(PROTECTS));
        assert_eq!(value.text(), Ok("sample-token"));
        assert_eq!(value.protect_in_place(), Ok(false), "once");
    }

    /// A value Windows cannot decrypt (here, damaged bytes) is explained.
    #[test]
    fn damaged_bytes_are_explained() {
        let value = Value::from_stored(Stored::Dpapi("AAAA".into()));
        let why = value.text().unwrap_err();
        if PROTECTS {
            assert!(why.starts_with("Windows could not decrypt it ("), "{why}");
            assert!(why.ends_with(')'), "{why}");
        } else {
            assert_eq!(
                why,
                "Windows encrypted it, and only that Windows user can decrypt it"
            );
        }
        assert_eq!(
            cannot_read("credential", why),
            format!("Pane cannot read this credential on this computer: {why}")
        );
    }
}
