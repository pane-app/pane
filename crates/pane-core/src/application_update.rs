//! Pane's own updates: finding out a newer version of Pane exists, and
//! installing it by the user's choice (#54 wired the Windows half, #56
//! the Linux one; the check, the download, the verification and the swap
//! are the same on every system, and another OS's slice wires the same
//! machinery to its own program).
//!
//! Pane never updates itself on its own: it lists the releases of its own
//! repository on GitHub ([`crate::releases`], ADR 0048) when it starts,
//! takes the newest one that carries this system's package, and tells the
//! user what it found — a row in root search and a word on the status
//! line. Only the user's choice downloads the package (with progress and
//! retries, checked against the sha256 the release's `.sha256` file
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
//! The package installed is the release's package for this Pane's
//! system, named as `cargo xtask package-*` names it
//! (`pane-<version>-<target>.zip` or `.tar.gz`): a zip holding `pane.exe`
//! under `pane/` on Windows, a gzipped tarball holding `pane` under
//! `pane/` on Linux — each read as strictly as an npm package's tarball
//! is ([`crate::zip`], [`crate::archive::unpack_within`]). A Pane that
//! runs from a Linux AppImage installs the release's AppImage instead
//! (`Pane-<version>-<arch>.AppImage`), which replaces the AppImage file
//! itself (ADR 0049, #464); a macOS `Pane.app`, wherever it was dragged,
//! keeps taking its program from the zip.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::Digest;

use crate::defaults::{Failure, answer, failed, with_retries};
use crate::releases::{Asset, Release, ReleaseSource};

/// The largest application package Pane downloads, packed: the package
/// holds the whole program, so it is far larger than any other package
/// Pane downloads for itself (and it unpacks to [`crate::zip`]'s own,
/// larger, bound).
pub(crate) const MAX_PACKAGE: u64 = 512 << 20;

/// The largest `.sha256` file Pane reads: one digest and one file name.
const MAX_CHECKSUM: u64 = 4 << 10;

/// The archives a package is packed as, as `cargo xtask package-*`
/// names them: the zip of Windows and macOS, the gzipped tarball of
/// Linux.
const ARCHIVES: [&str; 2] = ["zip", "tar.gz"];

/// The name of the folder a package's files are staged in, inside the
/// install folder until the swap ends, and of the file the running program
/// is renamed to.
const STAGING: &str = "update";

/// How the program an update replaces was installed, which decides the
/// release asset that replaces it and how the swap stages it (ADR 0049).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Install {
    /// A program unpacked from one of the release's packages (the zip or
    /// the tarball), as the install scripts put it and as a macOS
    /// `Pane.app` holds it: an update unpacks the package for this system
    /// and takes its program.
    Unpacked,
    /// A Linux AppImage, one file holding the program and its desktop
    /// entry and icons: an update downloads the release's AppImage and puts
    /// it in that file's place, as it is.
    AppImage,
}

impl Install {
    /// The names a release of `version` gives this install's asset for
    /// `target` (such as `linux-x86_64`), as `cargo xtask package-*` names
    /// them: the zip or the tarball an unpacked program comes from, or the
    /// AppImage (`Pane-<version>-<arch>.AppImage`, which only Linux has).
    fn assets(self, version: &str, target: &str) -> Vec<String> {
        match self {
            Install::Unpacked => ARCHIVES
                .iter()
                .map(|archive| format!("pane-{version}-{target}.{archive}"))
                .collect(),
            Install::AppImage => target
                .strip_prefix("linux-")
                .map(|arch| vec![format!("Pane-{version}-{arch}.AppImage")])
                .unwrap_or_default(),
        }
    }
}

/// The program Pane runs from, and where an update replaces it: the folder
/// it is installed in and its file name (`%LOCALAPPDATA%\Pane` holding
/// `pane.exe` on Windows, `~/.local/bin` holding `pane` on Linux, or the
/// folder an AppImage was saved in holding it).
#[derive(Clone, Debug)]
pub(crate) struct Program {
    folder: PathBuf,
    name: String,
    install: Install,
}

impl Program {
    /// The program `path`, in the folder it is installed in.
    pub(crate) fn at(path: PathBuf) -> Result<Program, String> {
        Program::installed(path, Install::Unpacked)
    }

    /// The AppImage at `path`, which an update replaces as a whole.
    pub(crate) fn appimage(path: PathBuf) -> Result<Program, String> {
        Program::installed(path, Install::AppImage)
    }

    fn installed(path: PathBuf, install: Install) -> Result<Program, String> {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} names no program file", path.display()))?
            .to_owned();
        let folder = path
            .parent()
            .ok_or_else(|| format!("{} names no folder", path.display()))?
            .to_path_buf();
        Ok(Program {
            folder,
            name,
            install,
        })
    }

    /// How this program was installed.
    pub(crate) fn install(&self) -> Install {
        self.install
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

    /// Where a chosen package's files are staged until the swap ends. An
    /// AppImage is usually kept in a folder of the user's own (Downloads,
    /// `~/Applications`), where a folder named `update` may well be the
    /// user's, which a start of Pane would remove: its staging folder is
    /// named for the AppImage, and hidden.
    fn staging(&self) -> PathBuf {
        match self.install {
            Install::Unpacked => self.folder.join(STAGING),
            Install::AppImage => self.folder.join(format!(".{}.{STAGING}", self.name)),
        }
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

/// An offered Pane version: the newer release's package for this
/// system, already checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    pub(crate) version: String,
    /// The package's file name, as the release names it.
    file: String,
    /// Where the package is downloaded from.
    url: String,
    /// Where the `.sha256` file beside it is downloaded from.
    checksum: String,
    /// The package's size in bytes, as the release gives it, for
    /// progress.
    pub(crate) size: u64,
}

impl Offer {
    /// The offer `releases` make this Pane: the newest release that
    /// carries the package for `target` (`pane-<version>-<target>.zip` or
    /// `.tar.gz`, or for an AppImage `Pane-<version>-<arch>.AppImage`, as
    /// `install` says), if its version is newer than `current`. `None` when no
    /// release carries this system's package or none is newer: neither is
    /// this Pane's business, so each stays silent (a release that carries
    /// only other systems' packages is passed over). A newer release whose
    /// package cannot be checked — no `.sha256` beside it — or that is not
    /// downloaded from Pane's releases is an error, so the check explains
    /// it.
    pub(crate) fn of(
        releases: &[Release],
        source: &ReleaseSource,
        current: &str,
        target: &str,
        install: Install,
    ) -> Result<Option<Offer>, String> {
        let mut newest: Option<(&Release, &Asset)> = None;
        for release in releases {
            let Some(package) = package_of(release, target, install) else {
                continue;
            };
            // Both versions are dotted numbers (the listing keeps only
            // release tags), so the comparison cannot fail.
            if newest.is_none_or(|(other, _)| newer(&release.version, &other.version) == Ok(true)) {
                newest = Some((release, package));
            }
        }
        let Some((release, package)) = newest else {
            return Ok(None);
        };
        if !newer(&release.version, current)? {
            return Ok(None);
        }
        let checksum_name = format!("{}.sha256", package.name);
        let checksum = release
            .assets
            .iter()
            .find(|asset| asset.name == checksum_name)
            .ok_or_else(|| {
                format!(
                    "the release of Pane {} has the package `{}` but no `{checksum_name}` to \
                     check it against, which Pane needs to offer the update",
                    release.version, package.name
                )
            })?;
        for asset in [package, checksum] {
            if !source.is_download(&asset.url) {
                return Err(format!(
                    "the release of Pane {} gives `{}` from `{}`, which is not where Pane's \
                     releases are downloaded from",
                    release.version, asset.name, asset.url
                ));
            }
        }
        Ok(Some(Offer {
            version: release.version.clone(),
            file: package.name.clone(),
            url: package.url.clone(),
            checksum: checksum.url.clone(),
            size: package.size,
        }))
    }
}

/// The package `release` carries for `target` and `install`, named as
/// `cargo xtask package-*` names it.
fn package_of<'a>(release: &'a Release, target: &str, install: Install) -> Option<&'a Asset> {
    install
        .assets(&release.version, target)
        .into_iter()
        .find_map(|name| release.assets.iter().find(|asset| asset.name == name))
}

/// Lists the releases of `source` and finds what they say of a newer Pane
/// for `target` and `install` than `current`. Blocks on the network, and
/// tries an unreachable source again up to [`crate::defaults::ATTEMPTS`]
/// times before explaining, exactly as acquiring a default extension does.
pub(crate) fn check(
    source: &ReleaseSource,
    current: &str,
    target: &str,
    install: Install,
) -> Result<Found, String> {
    with_retries(|| list_and_find(source, current, target, install))
}

/// One attempt: list the releases, then take the newest for this system.
fn list_and_find(
    source: &ReleaseSource,
    current: &str,
    target: &str,
    install: Install,
) -> Result<Found, Failure> {
    let releases = source.list()?;
    let offer = Offer::of(&releases, source, current, target, install).map_err(failed)?;
    Ok(match offer {
        Some(offer) => Found::Offered(offer),
        None => Found::UpToDate,
    })
}

/// Downloads the offered update's package, telling `progress` of its
/// bytes as they arrive (of the size the release gave), checked against
/// the sha256 its `.sha256` file gives. Blocks on the network, and tries
/// an interrupted download again up to [`crate::defaults::ATTEMPTS`]
/// times before explaining.
pub(crate) fn fetch(
    source: &ReleaseSource,
    offer: &Offer,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<Vec<u8>, String> {
    with_retries(|| download(source, offer, progress))
}

/// Downloads the `.sha256` file and the package `offer` names, and checks
/// the package against the digest before it is returned.
fn download(
    source: &ReleaseSource,
    offer: &Offer,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<Vec<u8>, Failure> {
    let file = offer.file.clone();
    let checksum_file = format!("{file}.sha256");
    let read = source.download(
        &offer.checksum,
        &format!("the checksum `{checksum_file}`"),
        MAX_CHECKSUM,
        &|_| {},
    )?;
    if read.status != 200 {
        return Err(not_there(read.status, &checksum_file));
    }
    let digest = sha256_of(&read.body)
        .ok_or_else(|| failed(format!("`{checksum_file}` gives no sha256 digest")))?;
    let size = offer.size;
    let told = move |bytes: u64| progress(bytes, size);
    let read = source.download(
        &offer.url,
        &format!("the package `{file}`"),
        MAX_PACKAGE,
        &told,
    )?;
    if read.status != 200 {
        return Err(not_there(read.status, &file));
    }
    let package = read.body;
    if crate::integrity::hex(&sha2::Sha256::digest(&package)) != digest {
        return Err(failed(format!(
            "the downloaded package `{file}` does not match the sha256 `{checksum_file}` gives"
        )));
    }
    Ok(package)
}

/// Why `file` of a release was not downloaded, when its download answered
/// `status`.
fn not_there(status: u16, file: &str) -> Failure {
    let why = match status {
        404 => format!("the release's `{file}` is not there"),
        status => format!("the release's `{file}` was answered with {status}"),
    };
    answer(status, why)
}

/// The digest a `.sha256` file gives, lowercase, as `sha256sum` writes
/// one (`<digest>  <file name>`); `None` when its first word is not a
/// sha256 digest.
fn sha256_of(text: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(text).ok()?;
    let digest = text.split_whitespace().next()?;
    (digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| digest.to_ascii_lowercase())
}

/// Unpacks the downloaded `package` into the staging folder (an AppImage
/// is staged as it came) and swaps the
/// running program for the new one: the old program is renamed out of its
/// way, the new one takes its name, and the staging folder goes. The new
/// version is used the next time Pane starts; nothing of Pane's data is
/// touched. For a program inside a macOS `Pane.app`, the bundle's
/// `Info.plist` is then given the new version, and for a Windows program
/// Pane's setup installed, its entry in Settings > Apps. On failure nothing is left
/// behind: the staging folder goes, the running program is the one it
/// was, and the user can try again.
pub(crate) fn swap(package: &[u8], offer: &Offer, program: &Program) -> Result<(), String> {
    let staging = program.staging();
    // A staging folder a stopped Pane left, or another Pane holds, goes:
    // what it holds is unpacked again below.
    let _ = fs::remove_dir_all(&staging);
    let staged = staging.join(&program.name);
    // The package comes in the format the system it is for packs: the zip
    // a Windows or macOS package is, or the gzipped tarball a Linux one
    // is. Each is read with the same strictness (only files and folders
    // inside the package, one plain name per part, within the
    // application package's bounds), and each unpacks without the
    // package's top folder (`pane/`), so the program the package holds
    // lands in the staging folder by its own name, which the swap below
    // takes. An AppImage is the program itself: it is staged as it came,
    // under the name of the AppImage it replaces, once it is checked to be
    // one.
    let unpacked = if program.install == Install::AppImage {
        stage_appimage(package, &staged)
    } else if offer.file.ends_with(".zip") {
        crate::zip::unpack(package, &staging)
    } else if offer.file.ends_with(".tar.gz") || offer.file.ends_with(".tgz") {
        crate::archive::unpack_within(package, &staging, crate::zip::MAX_UNPACKED)
    } else {
        Err(format!(
            "Pane unpacks a package named `.zip` or `.tar.gz`, not `{}`",
            offer.file
        ))
    };
    let unpacked = unpacked.map_err(|why| match program.install {
        Install::AppImage => format!("the AppImage `{}` cannot be installed: {why}", offer.file),
        Install::Unpacked => format!(
            "the package `{}` cannot be unpacked safely: {why}; Pane installs only the files \
             and folders inside the package",
            offer.file
        ),
    });
    if let Err(why) = unpacked {
        let _ = fs::remove_dir_all(&staging);
        return Err(why);
    }
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
    // On Windows, a Pane the setup installed has an entry in Settings >
    // Apps naming the version the setup installed (#463): it is given the
    // new version too, when the entry is this program's (a zip copy
    // elsewhere leaves the setup's entry alone), so Apps names the version
    // that runs. As with the plist, an entry that cannot be rewritten does
    // not undo the update.
    #[cfg(windows)]
    if swapped.is_ok() {
        let _ = apps_entry::set_version(apps_entry::KEY, &program.folder, &offer.version);
    }
    swapped
}

/// Whether an Apps entry whose `InstallLocation` is `location` is the one
/// of the program in `folder`: the same folder, compared as Windows compares
/// paths (letter case aside) and with or without the trailing separator
/// Inno Setup writes.
#[cfg(any(windows, test))]
fn installed_in(location: &str, folder: &Path) -> bool {
    let trimmed = |path: &str| path.trim_end_matches(['\\', '/']).to_lowercase();
    !location.trim().is_empty() && trimmed(location) == trimmed(&folder.to_string_lossy())
}

/// The major and minor numbers of `version`, as Inno Setup writes them in
/// an Apps entry (`MajorVersion`, `MinorVersion`); `None` when the version
/// does not begin with two numbers.
#[cfg(any(windows, test))]
fn major_minor(version: &str) -> Option<(u32, u32)> {
    let mut numbers = version.split('.').map(|part| part.parse::<u32>().ok());
    Some((numbers.next()??, numbers.next()??))
}

#[cfg(windows)]
mod apps_entry;

/// Writes the downloaded `appimage` to `staged`, once it is checked to be
/// an AppImage: an ELF program carrying the AppImage type 2 mark (`AI`
/// and the type, 2, at byte 8 of the ELF header, where the AppImage
/// specification puts it), which every AppImage `appimagetool` writes
/// carries. A release asset that is anything else would leave the user
/// with a file that does not start, so it is refused before anything is
/// replaced.
fn stage_appimage(appimage: &[u8], staged: &Path) -> Result<(), String> {
    if appimage.get(..4) != Some(b"\x7fELF") || appimage.get(8..11) != Some(b"AI\x02") {
        return Err("it is not an AppImage (no ELF header with the AppImage type 2 mark)".into());
    }
    let folder = staged.parent().expect("the staging folder");
    fs::create_dir_all(folder)
        .and_then(|()| fs::write(staged, appimage))
        .map_err(|error| format!("it could not be staged: {error}"))
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
                        "the version `{version}` is not dotted numbers, which Pane cannot \
                         compare with another version"
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

    /// A release of `version` carrying `assets` (name, with or without its
    /// `.sha256` beside it), downloaded from Pane's published releases.
    fn release(version: &str, packages: &[&str], checksums: bool) -> Release {
        let asset = |name: String| Asset {
            url: format!("https://github.com/pane-app/pane/releases/download/v{version}/{name}"),
            name,
            size: 12,
        };
        let mut assets = Vec::new();
        for package in packages {
            assets.push(asset((*package).to_owned()));
            if checksums {
                assets.push(asset(format!("{package}.sha256")));
            }
        }
        Release {
            version: version.to_owned(),
            assets,
        }
    }

    #[test]
    fn the_offer_is_the_newest_release_with_this_systems_package() {
        let source = ReleaseSource::published();
        let releases = [
            // The newest release carries another system's package only
            // (a fix for one system, say): passed over.
            release("0.4.0", &["pane-0.4.0-linux-x86_64.tar.gz"], true),
            release(
                "0.3.0",
                &[
                    "pane-0.3.0-windows-x86_64.zip",
                    "pane-0.3.0-linux-x86_64.tar.gz",
                    "pane-0.3.0-macos-aarch64.zip",
                ],
                true,
            ),
            // Listed later but numbered lower: the version decides, not
            // the order.
            release("0.2.0", &["pane-0.2.0-windows-x86_64.zip"], true),
        ];
        let offer = Offer::of(
            &releases,
            &source,
            "0.1.0",
            "windows-x86_64",
            Install::Unpacked,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            offer,
            Offer {
                version: "0.3.0".into(),
                file: "pane-0.3.0-windows-x86_64.zip".into(),
                url: "https://github.com/pane-app/pane/releases/download/v0.3.0/\
                      pane-0.3.0-windows-x86_64.zip"
                    .into(),
                checksum: "https://github.com/pane-app/pane/releases/download/v0.3.0/\
                           pane-0.3.0-windows-x86_64.zip.sha256"
                    .into(),
                size: 12,
            }
        );
        // Linux's package is the tarball, macOS's the zip.
        let linux = Offer::of(
            &releases,
            &source,
            "0.1.0",
            "linux-x86_64",
            Install::Unpacked,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            (linux.version.as_str(), linux.file.as_str()),
            ("0.4.0", "pane-0.4.0-linux-x86_64.tar.gz")
        );
        let macos = Offer::of(
            &releases,
            &source,
            "0.1.0",
            "macos-aarch64",
            Install::Unpacked,
        )
        .unwrap()
        .unwrap();
        assert_eq!(macos.file, "pane-0.3.0-macos-aarch64.zip");
        // No newer version, no package for this system, no release at all:
        // no offer, and no error.
        assert_eq!(
            Offer::of(
                &releases,
                &source,
                "0.3.0",
                "windows-x86_64",
                Install::Unpacked
            )
            .unwrap(),
            None
        );
        assert_eq!(
            Offer::of(
                &releases,
                &source,
                "0.1.0",
                "linux-aarch64",
                Install::Unpacked
            )
            .unwrap(),
            None
        );
        assert_eq!(
            Offer::of(&[], &source, "0.1.0", "windows-x86_64", Install::Unpacked).unwrap(),
            None
        );
        // A development package (`-dev`) is never a release's package.
        let dev = [release(
            "0.5.0",
            &["pane-0.5.0-windows-x86_64-dev.zip"],
            true,
        )];
        assert_eq!(
            Offer::of(&dev, &source, "0.1.0", "windows-x86_64", Install::Unpacked).unwrap(),
            None
        );
    }

    #[test]
    fn an_appimage_is_offered_the_newest_release_s_appimage() {
        let source = ReleaseSource::published();
        let releases = [
            // The newest release carries the tarball only: an AppImage
            // passes it over, as a release with another system's package.
            release("0.3.0", &["pane-0.3.0-linux-x86_64.tar.gz"], true),
            release(
                "0.2.0",
                &[
                    "pane-0.2.0-linux-x86_64.tar.gz",
                    "Pane-0.2.0-x86_64.AppImage",
                    "pane-0.2.0-macos-aarch64.zip",
                    "Pane-0.2.0.dmg",
                ],
                true,
            ),
        ];
        let appimage = Offer::of(
            &releases,
            &source,
            "0.1.0",
            "linux-x86_64",
            Install::AppImage,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            (appimage.version.as_str(), appimage.file.as_str()),
            ("0.2.0", "Pane-0.2.0-x86_64.AppImage")
        );
        assert!(
            appimage
                .checksum
                .ends_with("/Pane-0.2.0-x86_64.AppImage.sha256")
        );
        // A program the tarball installed keeps taking the tarball, and a
        // macOS Pane.app the zip, beside the release's dmg.
        let tarball = Offer::of(
            &releases,
            &source,
            "0.1.0",
            "linux-x86_64",
            Install::Unpacked,
        )
        .unwrap()
        .unwrap();
        assert_eq!(tarball.file, "pane-0.3.0-linux-x86_64.tar.gz");
        let macos = Offer::of(
            &releases,
            &source,
            "0.1.0",
            "macos-aarch64",
            Install::Unpacked,
        )
        .unwrap()
        .unwrap();
        assert_eq!(macos.file, "pane-0.2.0-macos-aarch64.zip");
        // No other system has an AppImage, nor another architecture's.
        for target in ["windows-x86_64", "macos-aarch64", "linux-aarch64"] {
            assert_eq!(
                Offer::of(&releases, &source, "0.1.0", target, Install::AppImage).unwrap(),
                None,
                "{target}"
            );
        }
    }

    /// An AppImage's first bytes, as `appimagetool` writes them: an ELF
    /// header carrying the AppImage type 2 mark at byte 8, then `body`.
    fn appimage(body: &[u8]) -> Vec<u8> {
        let mut bytes = b"\x7fELF\x02\x01\x01\x00AI\x02\x00\x00\x00\x00\x00".to_vec();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn an_update_puts_the_new_appimage_in_the_old_one_s_place() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("Pane-0.1.0-x86_64.AppImage");
        fs::write(&path, appimage(b"the 0.1.0 AppImage")).unwrap();
        // The user's own folder named `update`, beside the AppImage in
        // their Downloads: an AppImage's staging is not it.
        fs::create_dir_all(folder.path().join("update")).unwrap();
        fs::write(folder.path().join("update/notes.txt"), b"the user's").unwrap();
        let program = Program::appimage(path.clone()).unwrap();
        assert_eq!(
            program.staging(),
            folder.path().join(".Pane-0.1.0-x86_64.AppImage.update")
        );
        let offer = Offer {
            version: "0.2.0".to_owned(),
            file: "Pane-0.2.0-x86_64.AppImage".to_owned(),
            url: String::new(),
            checksum: String::new(),
            size: 0,
        };
        // A download that is not an AppImage replaces nothing.
        let wrong = swap(b"a tarball, say", &offer, &program).unwrap_err();
        assert!(wrong.contains("is not an AppImage"), "{wrong}");
        assert_eq!(fs::read(&path).unwrap(), appimage(b"the 0.1.0 AppImage"));
        assert!(!program.old().exists());
        assert!(!program.staging().exists());

        swap(&appimage(b"the 0.2.0 AppImage"), &offer, &program).unwrap();
        // The new AppImage took the old one's name and place (the path a
        // desktop entry or an autostart entry names), runnable; the old
        // one is renamed beside it, and the staging folder is gone.
        assert_eq!(fs::read(&path).unwrap(), appimage(b"the 0.2.0 AppImage"));
        assert_eq!(
            fs::read(program.old()).unwrap(),
            appimage(b"the 0.1.0 AppImage")
        );
        assert!(!program.staging().exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "{mode:o}");
        }
        // A later start removes the old AppImage, and never the user's
        // folder.
        program.clean_install_folder();
        assert!(!program.old().exists());
        assert_eq!(
            fs::read(folder.path().join("update/notes.txt")).unwrap(),
            b"the user's"
        );
    }

    #[test]
    fn an_offer_that_cannot_be_checked_is_explained() {
        let source = ReleaseSource::published();
        // A newer package with no `.sha256` beside it.
        let unchecked = [release("0.2.0", &["pane-0.2.0-windows-x86_64.zip"], false)];
        let wrong = Offer::of(
            &unchecked,
            &source,
            "0.1.0",
            "windows-x86_64",
            Install::Unpacked,
        )
        .unwrap_err();
        assert!(
            wrong.contains("no `pane-0.2.0-windows-x86_64.zip.sha256` to check it against"),
            "{wrong}"
        );
        // A package downloaded from somewhere other than Pane's releases.
        let mut elsewhere = release("0.2.0", &["pane-0.2.0-windows-x86_64.zip"], true);
        elsewhere.assets[0].url = "https://example.com/pane-0.2.0-windows-x86_64.zip".into();
        let wrong = Offer::of(
            &[elsewhere],
            &source,
            "0.1.0",
            "windows-x86_64",
            Install::Unpacked,
        )
        .unwrap_err();
        assert!(
            wrong.contains("not where Pane's releases are downloaded from"),
            "{wrong}"
        );
        // The version this Pane runs that is not dotted numbers.
        let fine = [release("0.2.0", &["pane-0.2.0-windows-x86_64.zip"], true)];
        let wrong = Offer::of(
            &fine,
            &source,
            "0.1.0-beta",
            "windows-x86_64",
            Install::Unpacked,
        )
        .unwrap_err();
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
            url: String::new(),
            checksum: String::new(),
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

    #[test]
    fn an_apps_entry_is_this_program_s_when_it_names_its_folder() {
        let folder = Path::new(r"C:\Users\u\AppData\Local\Pane");
        // As Inno Setup writes it, with its trailing separator, and in any
        // letter case.
        assert!(installed_in(r"C:\Users\u\AppData\Local\Pane\", folder));
        assert!(installed_in(r"c:\users\u\appdata\local\pane", folder));
        // Another folder, one inside it, or none at all.
        assert!(!installed_in(r"C:\Users\u\Downloads\pane\", folder));
        assert!(!installed_in(
            r"C:\Users\u\AppData\Local\Pane\data\",
            folder
        ));
        assert!(!installed_in("", folder));
        assert!(!installed_in("  ", Path::new("")));
    }

    #[test]
    fn an_apps_entry_s_major_and_minor_are_the_version_s_first_numbers() {
        assert_eq!(major_minor("0.2.0"), Some((0, 2)));
        assert_eq!(major_minor("12.3"), Some((12, 3)));
        assert_eq!(major_minor("99.0.0.1"), Some((99, 0)));
        assert_eq!(major_minor("1"), None);
        assert_eq!(major_minor("1.x"), None);
    }

    #[test]
    fn a_checksum_file_gives_its_digest() {
        let digest = "a".repeat(64);
        assert_eq!(
            sha256_of(format!("{digest}  pane-0.1.0-windows-x86_64.zip\n").as_bytes()),
            Some(digest.clone())
        );
        // Upper case, as some tools write it, and the digest alone.
        assert_eq!(sha256_of("A".repeat(64).as_bytes()), Some(digest));
        for text in ["", "abc  pane.zip", &"g".repeat(64), &"a".repeat(65)] {
            assert_eq!(sha256_of(text.as_bytes()), None, "{text}");
        }
    }
}
