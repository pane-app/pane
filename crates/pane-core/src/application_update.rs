//! Pane's own updates: finding out a newer version of Pane exists, and
//! installing it by the user's choice (#54 wired the Windows half, #56
//! the Linux one; the check, the download, the verification and the swap
//! are the same on every system, and another OS's slice wires the same
//! machinery to its own program).
//!
//! Pane never updates itself on its own: it checks the artifact source
//! ([`crate::defaults`], which now serves Pane's own application updates
//! alone) when it starts, reading the index's `application` entry, and
//! tells the user what it found — a row in root search and a word on the
//! status line. Only the user's choice downloads the package (with
//! progress and retries, checked against the sha512 integrity the index
//! gives) and installs it; nothing is ever downloaded, installed or
//! restarted automatically
//! ([decision](https://github.com/pane-app/pane/issues/1): US76).
//!
//! Installing works around the program running from the very file it
//! would replace: the new program is staged in the install folder and the
//! running one renamed out of its way, which every system allows of a
//! running program, so the new file takes the old one's name and place
//! while Pane keeps running — **the new version is used the next time
//! Pane starts**, which the user does when they choose. Pane itself never
//! restarts. The renamed old program is removed on a later start, and
//! Pane's data (its installed extensions, their settings and everything
//! else it keeps) lives beside the program, untouched by the swap.
//!
//! The package installed is the one the artifact source serves: the
//! package built for this Pane's system — a zip holding `pane.exe` under
//! `pane/` on Windows, a gzipped tarball holding `pane` under `pane/` on
//! Linux — each read as strictly as an npm package's tarball is
//! ([`crate::zip`], [`crate::archive::unpack_within`]).

use std::fs;
use std::path::{Path, PathBuf};

use crate::defaults::{ArtifactSource, Failure, answer, failed, interrupted, with_retries};

/// The largest application package Pane downloads, packed: the package
/// holds the whole program, so it is far larger than any other package
/// Pane downloads for itself (and it unpacks to [`crate::zip`]'s own,
/// larger, bound).
pub(crate) const MAX_PACKAGE: u64 = 512 << 20;

/// The name of the folder a package's files are staged in, inside the
/// install folder until the swap ends, and of the file the running program
/// is renamed to.
const STAGING: &str = "update";

/// The program Pane runs from, and where an update replaces it: the folder
/// it is installed in and its file name (`%LOCALAPPDATA%\Pane` holding
/// `pane.exe` on Windows, `~/.local/bin` holding `pane` on Linux).
#[derive(Clone, Debug)]
pub(crate) struct Program {
    folder: PathBuf,
    name: String,
}

impl Program {
    /// The program `path`, in the folder it is installed in.
    pub(crate) fn at(path: PathBuf) -> Result<Program, String> {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} names no program file", path.display()))?
            .to_owned();
        let folder = path
            .parent()
            .ok_or_else(|| format!("{} names no folder", path.display()))?
            .to_path_buf();
        Ok(Program { folder, name })
    }

    /// The program file, in the install folder.
    fn path(&self) -> PathBuf {
        self.folder.join(&self.name)
    }

    /// The name of the file the running program is renamed to when an
    /// update replaces it: the old program, removed on a later start.
    fn old(&self) -> PathBuf {
        let mut name = self.name.clone();
        name.push_str(".old");
        self.folder.join(name)
    }

    /// Where a chosen package's files are staged until the swap ends.
    fn staging(&self) -> PathBuf {
        self.folder.join(STAGING)
    }

    /// The `Info.plist` of the macOS application bundle the program runs
    /// from, when it runs from one: the install script puts the program at
    /// `Pane.app/Contents/MacOS/pane`, beside `Pane.app/Contents/Info.plist`.
    /// A program anywhere else (Windows', Linux's) is in no bundle.
    fn bundle_info(&self) -> Option<PathBuf> {
        let contents = self.folder.parent()?;
        let in_bundle = self.folder.file_name()? == "MacOS"
            && contents.file_name()? == "Contents"
            && contents.parent()?.extension()? == "app";
        in_bundle.then(|| contents.join("Info.plist"))
    }

    /// Removes what earlier updates left in the install folder, as Pane
    /// starts: the program an update renamed out of its way, and a staging
    /// folder a Pane stopped mid-install left. The old program may still
    /// be running (another Pane that has not exited yet), and the staging
    /// folder may belong to another Pane installing now, so both are
    /// removed best effort: what cannot go now goes on a later start.
    pub(crate) fn clean_install_folder(&self) {
        if self.old().is_file() {
            let _ = fs::remove_file(self.old());
        }
        let _ = fs::remove_dir_all(self.staging());
    }
}

/// What a check found.
#[derive(Debug)]
pub(crate) enum Found {
    /// A newer version, with what installing it needs.
    Offered(Offer),
    /// This Pane is new enough: nothing to tell the user.
    UpToDate,
}

/// An offered Pane version: the newer application package the artifact
/// source's index names, already checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    pub(crate) version: String,
    /// The package's file name, one plain name, as the index gives it.
    file: String,
    /// `sha512-<base64>`, which the package's bytes must match.
    integrity: String,
    /// The package's size in bytes, as the index gives it, for progress.
    pub(crate) size: u64,
}

impl Offer {
    /// The offer the index's `application` entry describes, if it is one
    /// for this Pane: a readable version newer than `current`, a sha512
    /// integrity, a plain file name, its size and this system's target.
    /// `None` when the entry is absent (a source that serves no
    /// application package), names another system's package, or names no
    /// newer version: none of those is this Pane's business, so each
    /// stays silent. An entry for this system that names a newer version
    /// but cannot be used is an error, so the check explains it.
    pub(crate) fn of(
        entry: &Option<serde_json::Value>,
        current: &str,
        target: &str,
    ) -> Result<Option<Offer>, String> {
        let Some(entry) = entry else {
            return Ok(None);
        };
        let field = |name: &str| entry.get(name).and_then(serde_json::Value::as_str);
        let missing = |what: &str| {
            format!(
                "the index's application entry names no {what}, which Pane needs to offer an \
                 update"
            )
        };
        let version = field("version").ok_or_else(|| missing("version"))?;
        let integrity = field("integrity").ok_or_else(|| missing("integrity"))?;
        let file = field("file").ok_or_else(|| missing("file"))?;
        let named = field("target").ok_or_else(|| missing("target"))?;
        let size = entry
            .get("size")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                "the index's application entry names no size, which Pane needs to show the \
                 download's progress"
                    .to_owned()
            })?;
        // A package for another system, or a version this one does not
        // need, is not this Pane's business: it stays silent (and a broken
        // entry for it is never read), exactly as an equal or older
        // version does.
        if named != target || !newer(version, current)? {
            return Ok(None);
        }
        let offer = Offer {
            version: version.to_owned(),
            file: file.to_owned(),
            integrity: integrity.to_owned(),
            size,
        };
        if !crate::integrity::has_sha512(&offer.integrity) {
            return Err(
                "the index's application entry gives no sha512 integrity, which Pane needs to \
                 check its download"
                    .to_owned(),
            );
        }
        crate::downloads::check_part(&offer.file).map_err(|why| {
            format!(
                "the index's application entry names the package file `{}`, {}",
                offer.file, why
            )
        })?;
        Ok(Some(offer))
    }
}

/// Reads the index of `source` and finds what it says of a newer Pane for
/// `target` than `current`. Blocks on the network, and tries an
/// unreachable source again up to [`ATTEMPTS`] times before explaining,
/// exactly as acquiring a default extension does.
pub(crate) fn check(source: &ArtifactSource, current: &str, target: &str) -> Result<Found, String> {
    with_retries(|| read_and_find(source, current, target))
}

/// One attempt: read the index, then take its application entry.
fn read_and_find(source: &ArtifactSource, current: &str, target: &str) -> Result<Found, Failure> {
    let index = crate::defaults::read_index(source)?;
    let offer = Offer::of(&index.application, current, target).map_err(failed)?;
    Ok(match offer {
        Some(offer) => Found::Offered(offer),
        None => Found::UpToDate,
    })
}

/// Downloads the offered update's package, telling `progress` of its
/// bytes as they arrive (of the size the index gave), checked against the
/// integrity its index entry gives. Blocks on the network, and tries an
/// interrupted download again up to [`ATTEMPTS`] times before explaining.
pub(crate) fn fetch(
    source: &ArtifactSource,
    offer: &Offer,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<Vec<u8>, String> {
    with_retries(|| download(source, offer, progress))
}

/// Downloads the package `offer` names, checked against the offer's
/// integrity before it is returned.
fn download(
    source: &ArtifactSource,
    offer: &Offer,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<Vec<u8>, Failure> {
    let url = source.payload_url(&offer.file);
    let (file, size) = (offer.file.clone(), offer.size);
    let told = move |bytes: u64| progress(bytes, size);
    let read = source
        .get(&url, &[], MAX_PACKAGE, &told)
        .map_err(|error| match error {
            crate::http::GetError::TooLarge => failed(format!(
                "the package `{file}` is larger than the {} MiB Pane downloads",
                MAX_PACKAGE >> 20
            )),
            crate::http::GetError::Failed(why) => interrupted(format!(
                "Pane's downloads at {} could not be reached: {why}",
                source.url()
            )),
        })?;
    if read.status != 200 {
        let why = match read.status {
            404 => format!("the package `{file}` its index names is not there"),
            status => format!("it answered {status} for the package `{file}`"),
        };
        return Err(answer(read.status, why));
    }
    let package = read.body;
    crate::integrity::check_integrity(&package, &offer.integrity)
        .map_err(|why| failed(format!("the downloaded package `{file}` {why}")))?;
    Ok(package)
}

/// Unpacks the downloaded `package` into the staging folder and swaps the
/// running program for the new one: the old program is renamed out of its
/// way, the new one takes its name, and the staging folder goes. The new
/// version is used the next time Pane starts; nothing of Pane's data is
/// touched. For a program inside a macOS `Pane.app`, the bundle's
/// `Info.plist` is then given the new version. On failure nothing is left
/// behind: the staging folder goes, the running program is the one it
/// was, and the user can try again.
pub(crate) fn swap(package: &[u8], offer: &Offer, program: &Program) -> Result<(), String> {
    let staging = program.staging();
    // A staging folder a stopped Pane left, or another Pane holds, goes:
    // what it holds is unpacked again below.
    let _ = fs::remove_dir_all(&staging);
    // The package comes in the format the system it is for packs: the zip
    // a Windows or macOS package is, or the gzipped tarball a Linux one
    // is. Each is read with the same strictness (only files and folders
    // inside the package, one plain name per part, within the
    // application package's bounds), and each unpacks without the
    // package's top folder (`pane/`), so the program the package holds
    // lands in the staging folder by its own name, which the swap below
    // takes.
    let unpacked = if offer.file.ends_with(".zip") {
        crate::zip::unpack(package, &staging)
    } else if offer.file.ends_with(".tar.gz") || offer.file.ends_with(".tgz") {
        crate::archive::unpack_within(package, &staging, crate::zip::MAX_UNPACKED)
    } else {
        Err(format!(
            "Pane unpacks a package named `.zip` or `.tar.gz`, not `{}`",
            offer.file
        ))
    };
    let unpacked = unpacked.map_err(|why| {
        format!(
            "the package `{}` cannot be unpacked safely: {why}; Pane installs only the files \
             and folders inside the package",
            offer.file
        )
    });
    if let Err(why) = unpacked {
        let _ = fs::remove_dir_all(&staging);
        return Err(why);
    }
    let staged = staging.join(&program.name);
    let swap = || -> Result<(), String> {
        if !staged.is_file() {
            return Err(format!(
                "the package holds no {} for this program",
                program.name
            ));
        }
        // The new program is made runnable where the system has such a
        // bit, before anything is replaced, so a program that cannot be
        // replaced never was.
        crate::packages::make_executable(&staged)
            .map_err(|error| why_files("the new program", error))?;
        // The program an earlier update renamed away goes first; it may be
        // a program still running, which is explained rather than worked
        // around, and the user can try again once it has exited.
        if program.old().exists() {
            fs::remove_file(program.old())
                .map_err(|error| why_files("the previous version's program", error))?;
        }
        fs::rename(program.path(), program.old())
            .map_err(|error| why_files("the running program", error))?;
        match fs::rename(&staged, program.path()) {
            Ok(()) => Ok(()),
            // The new program could not take the old one's place: put the
            // old one back, exactly as it was. If even that fails, the old
            // program is left where the rename above put it and said so,
            // rather than silently leaving no program at all.
            Err(error) => {
                let why = why_files("the new program", error);
                match fs::rename(program.old(), program.path()) {
                    Ok(()) => Err(why),
                    Err(also) => Err(format!(
                        "{why}; the previous version's program could not be put back either \
                         ({}), so it is left as {}, which can be renamed to {} by hand",
                        why_files("the previous version's program", also),
                        program.old().display(),
                        program.path().display(),
                    )),
                }
            }
        }
    };
    let swapped = swap();
    let _ = fs::remove_dir_all(&staging);
    // On macOS the program is replaced inside its `Pane.app`, whose
    // `Info.plist` names the version Finder's "Get Info" shows: it is given
    // the new version too, so the bundle and its program agree. The new
    // program is already in place, so a plist that cannot be rewritten
    // does not undo the update; it keeps naming the old version until the
    // bundle is reinstalled from a package.
    if swapped.is_ok()
        && let Some(info) = program.bundle_info()
    {
        let _ = set_bundle_version(&info, &offer.version);
    }
    swapped
}

/// Why a file could not be changed, in the wording the failure explains
/// itself with.
fn why_files(what: &str, error: std::io::Error) -> String {
    format!("{what} could not be replaced: {error}")
}

/// The `Info.plist` keys that name a bundle's version: the version Finder
/// shows and the build's, both of which Pane's bundle sets to Pane's
/// version (`xtask`'s `app_plist`).
const BUNDLE_VERSION_KEYS: [&str; 2] = ["CFBundleShortVersionString", "CFBundleVersion"];

/// Writes `version` into the bundle's `Info.plist` at `info`. The new plist
/// is written beside it and renamed over it, so nothing ever reads half a
/// plist; a plist that names no version is left as it is.
fn set_bundle_version(info: &Path, version: &str) -> std::io::Result<()> {
    let plist = fs::read_to_string(info)?;
    let Some(rewritten) = with_bundle_version(&plist, version) else {
        return Ok(());
    };
    let written = info.with_file_name("Info.plist.update");
    let result = fs::write(&written, rewritten).and_then(|()| fs::rename(&written, info));
    if result.is_err() {
        let _ = fs::remove_file(&written);
    }
    result
}

/// `plist`, an XML property list, with the string each of
/// [`BUNDLE_VERSION_KEYS`] it gives replaced by `version`; `None` when that
/// changes nothing (a plist that gives neither key a string, or already
/// names `version`). Only the values change, so the rest of the plist, its
/// layout included, stays as the install script wrote it.
fn with_bundle_version(plist: &str, version: &str) -> Option<String> {
    let version = version
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let mut rewritten = plist.to_owned();
    for key in BUNDLE_VERSION_KEYS {
        let tag = format!("<key>{key}</key>");
        let Some(at) = rewritten.find(&tag) else {
            continue;
        };
        let Some(value) = rewritten[at + tag.len()..]
            .trim_start()
            .strip_prefix("<string>")
        else {
            continue;
        };
        let Some(length) = value.find("</string>") else {
            continue;
        };
        let start = rewritten.len() - value.len();
        rewritten.replace_range(start..start + length, &version);
    }
    (rewritten != plist).then_some(rewritten)
}

/// Whether `asked` is a newer version than `current`: versions are dotted
/// numbers (`0.1.0`), compared by number, a missing number counting as
/// zero. A version that is not dotted numbers is an error, explained
/// rather than guessed at.
pub(crate) fn newer(asked: &str, current: &str) -> Result<bool, String> {
    let numbers = |version: &str| -> Result<Vec<u64>, String> {
        version
            .split('.')
            .map(|part| {
                part.parse::<u64>().map_err(|_| {
                    format!(
                        "the version `{version}` the index gives is not dotted numbers, which \
                         Pane cannot compare with the version it runs"
                    )
                })
            })
            .collect()
    };
    let (asked, current) = (numbers(asked)?, numbers(current)?);
    let length = asked.len().max(current.len());
    let padding = |mut numbers: Vec<u64>| {
        numbers.resize(length, 0);
        numbers
    };
    Ok(padding(asked) > padding(current))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    #[test]
    fn a_newer_version_is_greater_and_an_older_one_is_not() {
        for (asked, current) in [
            ("0.2.0", "0.1.0"),
            ("1.0.0", "0.9.9"),
            ("0.1.1", "0.1"),
            ("10.0.0", "9.0.0"),
            ("0.1.0.1", "0.1.0"),
        ] {
            assert!(newer(asked, current).unwrap(), "{asked} > {current}");
        }
        for (asked, current) in [
            ("0.1.0", "0.1.0"),
            ("0.1", "0.1.0"),
            ("0.1.0", "0.2.0"),
            ("1.0.0", "1.0.1"),
        ] {
            assert!(!newer(asked, current).unwrap(), "{asked} <= {current}");
        }
    }

    #[test]
    fn a_version_that_is_not_dotted_numbers_is_explained() {
        for asked in ["0.1.0-beta", "", "1.0.0.0.0.0.0x", "v1"] {
            assert!(newer(asked, "0.1.0").is_err(), "{asked}");
        }
        // A single number is a version: 1 is 1.0.0, which is newer than
        // 0.1.0.
        assert!(newer("1", "0.1.0").unwrap());
    }

    #[test]
    fn a_program_names_its_folder_and_its_old_name() {
        let program = Program::at(PathBuf::from("/opt/pane/pane.exe")).unwrap();
        assert_eq!(program.path(), PathBuf::from("/opt/pane/pane.exe"));
        assert_eq!(program.old(), PathBuf::from("/opt/pane/pane.exe.old"));
        assert_eq!(program.staging(), PathBuf::from("/opt/pane/update"));
        // The Linux program, as the install script installs it: `pane` with
        // no suffix, in the user's own bin folder, whose name the swap
        // renames aside and whose folder the staging folder goes in.
        let program = Program::at(PathBuf::from("/home/u/.local/bin/pane")).unwrap();
        assert_eq!(program.path(), PathBuf::from("/home/u/.local/bin/pane"));
        assert_eq!(program.old(), PathBuf::from("/home/u/.local/bin/pane.old"));
        assert_eq!(
            program.staging(),
            PathBuf::from("/home/u/.local/bin/update")
        );
        assert!(Program::at(PathBuf::from("pane.exe")).is_ok());
        // The program file's name is where it is; a path with no file name
        // names no program.
        assert!(Program::at(PathBuf::from("/opt/pane/")).is_ok());
        assert!(Program::at(PathBuf::from("/")).is_err());
    }

    #[test]
    fn a_clean_removes_what_earlier_updates_left() {
        let folder = tempfile::tempdir().unwrap();
        let program = Program::at(folder.path().join("pane.exe")).unwrap();
        fs::write(program.old(), b"the old program").unwrap();
        fs::create_dir_all(program.staging()).unwrap();
        fs::write(program.staging().join("pane.exe"), b"the new program").unwrap();
        program.clean_install_folder();
        assert!(!program.old().exists());
        assert!(!program.staging().exists());
    }

    #[test]
    fn an_offer_needs_every_field_and_this_systems_target() {
        // The entry is read as it is written (a JSON object), so a broken
        // one is explained here rather than making the index unreadable.
        let entry = |version: serde_json::Value,
                     file: serde_json::Value,
                     integrity: serde_json::Value,
                     target: serde_json::Value| {
            serde_json::json!({
                "version": version,
                "file": file,
                "integrity": integrity,
                "target": target,
                "size": 12,
            })
        };
        let integrity = format!(
            "sha512-{}",
            crate::integrity::base64(&sha2::Sha512::digest(b"the package"))
        );
        let none = entry(
            serde_json::Value::Null,
            "pane.zip".into(),
            integrity.as_str().into(),
            "windows-x86_64".into(),
        );
        assert!(Offer::of(&Some(none), "0.1.0", "windows-x86_64").is_err());
        // Every field present and this system's target, with a version
        // this Pane reads: an offer.
        let whole = entry(
            "99.0.0".into(),
            "pane-99.0.0.zip".into(),
            integrity.as_str().into(),
            "windows-x86_64".into(),
        );
        assert_eq!(
            Offer::of(&Some(whole), "0.1.0", "windows-x86_64")
                .unwrap()
                .map(|offer| offer.version),
            Some("99.0.0".to_owned())
        );
        // Another system's package is not this Pane's business: like an
        // equal or older version, it offers nothing and says nothing (a
        // broken entry for THIS system is the one that explains itself).
        let elsewhere = entry(
            "99.0.0".into(),
            "pane-99.0.0.zip".into(),
            integrity.as_str().into(),
            "linux-aarch64".into(),
        );
        assert_eq!(
            Offer::of(&Some(elsewhere), "0.1.0", "windows-x86_64").unwrap(),
            None
        );
        // No newer version: no offer, and no error.
        let older = entry(
            "0.0.1".into(),
            "pane-0.0.1.zip".into(),
            integrity.as_str().into(),
            "windows-x86_64".into(),
        );
        assert_eq!(
            Offer::of(&Some(older), "0.1.0", "windows-x86_64").unwrap(),
            None
        );
        assert_eq!(Offer::of(&None, "0.1.0", "windows-x86_64").unwrap(), None);
        // A file name that is not one plain name, an integrity without a
        // sha512, a version that is not dotted numbers and a missing size
        // are explained.
        let escaping = entry(
            "99.0.0".into(),
            "../pane.zip".into(),
            integrity.as_str().into(),
            "windows-x86_64".into(),
        );
        let wrong = Offer::of(&Some(escaping), "0.1.0", "windows-x86_64").unwrap_err();
        assert!(wrong.contains("whose name holds `/`"), "{wrong}");
        let unmeasured = entry(
            "99.0.0".into(),
            "pane.zip".into(),
            integrity.as_str().into(),
            "windows-x86_64".into(),
        );
        let unmeasured: serde_json::Value = {
            let mut value = unmeasured;
            value.as_object_mut().unwrap().remove("size");
            value
        };
        let wrong = Offer::of(&Some(unmeasured), "0.1.0", "windows-x86_64").unwrap_err();
        assert!(wrong.contains("names no size"), "{wrong}");
        let unsigned = entry(
            "99.0.0".into(),
            "pane.zip".into(),
            "sha1-abc".into(),
            "windows-x86_64".into(),
        );
        let wrong = Offer::of(&Some(unsigned), "0.1.0", "windows-x86_64").unwrap_err();
        assert!(wrong.contains("no sha512 integrity"), "{wrong}");
        let strange = entry(
            "99x".into(),
            "pane.zip".into(),
            integrity.as_str().into(),
            "windows-x86_64".into(),
        );
        let wrong = Offer::of(&Some(strange), "0.1.0", "windows-x86_64").unwrap_err();
        assert!(wrong.contains("not dotted numbers"), "{wrong}");
    }

    /// The `Info.plist` the macOS package carries for `version`, as
    /// `xtask`'s `app_plist` writes it.
    fn app_plist(version: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
\t<key>CFBundleExecutable</key><string>pane</string>\n\
\t<key>CFBundleIdentifier</key><string>dev.pane.launcher</string>\n\
\t<key>CFBundleName</key><string>Pane</string>\n\
\t<key>CFBundlePackageType</key><string>APPL</string>\n\
\t<key>CFBundleShortVersionString</key><string>{version}</string>\n\
\t<key>CFBundleVersion</key><string>{version}</string>\n\
</dict>\n\
</plist>\n"
        )
    }

    #[test]
    fn a_program_in_an_app_bundle_names_the_bundle_s_info_plist() {
        let program = Program::at(PathBuf::from(
            "/Users/u/Applications/Pane.app/Contents/MacOS/pane",
        ))
        .unwrap();
        assert_eq!(
            program.bundle_info(),
            Some(PathBuf::from(
                "/Users/u/Applications/Pane.app/Contents/Info.plist"
            ))
        );
        // Windows' and Linux's programs, and a folder that only looks like
        // a bundle's, are in no bundle.
        for path in [
            "/opt/pane/pane.exe",
            "/home/u/.local/bin/pane",
            "/Users/u/Pane/Contents/MacOS/pane",
            "/Users/u/Pane.app/MacOS/pane",
            "pane",
        ] {
            let program = Program::at(PathBuf::from(path)).unwrap();
            assert_eq!(program.bundle_info(), None, "{path}");
        }
    }

    #[test]
    fn the_bundle_s_version_keys_are_given_the_new_version() {
        assert_eq!(
            with_bundle_version(&app_plist("0.1.0"), "0.2.0"),
            Some(app_plist("0.2.0"))
        );
        // A key's string may follow it on the next line, as a plist Xcode
        // writes has it.
        let spread = "<dict>\n\t<key>CFBundleVersion</key>\n\t<string>7</string>\n\
                      \t<key>CFBundleName</key>\n\t<string>Pane</string>\n</dict>\n";
        assert_eq!(
            with_bundle_version(spread, "0.2.0").as_deref(),
            Some(
                "<dict>\n\t<key>CFBundleVersion</key>\n\t<string>0.2.0</string>\n\
                 \t<key>CFBundleName</key>\n\t<string>Pane</string>\n</dict>\n"
            )
        );
        // A version is written as XML text.
        assert_eq!(
            with_bundle_version(&app_plist("0.1.0"), "1<&>"),
            Some(app_plist("1&lt;&amp;&gt;"))
        );
        // Nothing to change: the version already named, no version keys,
        // a key with no string after it, or a binary plist.
        assert_eq!(with_bundle_version(&app_plist("0.2.0"), "0.2.0"), None);
        assert_eq!(
            with_bundle_version(
                "<dict><key>CFBundleName</key><string>Pane</string></dict>",
                "1"
            ),
            None
        );
        assert_eq!(
            with_bundle_version("<key>CFBundleVersion</key><integer>1</integer>", "2"),
            None
        );
        assert_eq!(with_bundle_version("bplist00\u{1}", "2"), None);
    }

    #[test]
    fn an_update_inside_an_app_bundle_gives_its_info_plist_the_new_version() {
        let folder = tempfile::tempdir().unwrap();
        let contents = folder.path().join("Pane.app/Contents");
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        fs::write(contents.join("MacOS/pane"), b"the 0.1.0 program").unwrap();
        fs::write(contents.join("Info.plist"), app_plist("0.1.0")).unwrap();
        let program = Program::at(contents.join("MacOS/pane")).unwrap();
        // The macOS package is a zip, but the swap after unpacking is the
        // same for every package: a tarball, which the tar writer makes,
        // stands in for it.
        let mut package = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        let new_program = b"the 0.2.0 program";
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(new_program.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        package
            .append_data(&mut header, "pane/pane", &new_program[..])
            .unwrap();
        let package = package.into_inner().unwrap().finish().unwrap();
        let offer = Offer {
            version: "0.2.0".to_owned(),
            file: "pane-0.2.0.tar.gz".to_owned(),
            integrity: String::new(),
            size: package.len() as u64,
        };
        swap(&package, &offer, &program).unwrap();
        assert_eq!(fs::read(program.path()).unwrap(), new_program);
        assert_eq!(
            fs::read_to_string(contents.join("Info.plist")).unwrap(),
            app_plist("0.2.0")
        );
        // The plist was renamed into place: nothing else is left in the
        // bundle but the old program, which a later start removes.
        let mut left: Vec<_> = fs::read_dir(&contents)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["Info.plist", "MacOS"]);
        assert!(program.old().is_file());
        // A failed update leaves the plist naming the version installed.
        let broken = Offer {
            version: "0.3.0".to_owned(),
            file: "pane-0.3.0.tar.gz".to_owned(),
            ..offer
        };
        assert!(swap(b"not a tarball", &broken, &program).is_err());
        assert_eq!(
            fs::read_to_string(contents.join("Info.plist")).unwrap(),
            app_plist("0.2.0")
        );
    }
}
