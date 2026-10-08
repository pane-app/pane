//! Where ClickOnce installs a deployment's program (#124 "Icons"): a
//! ClickOnce reference (`.appref-ms`) names its deployment
//! (`https://host/Orders.application#Orders.application, Culture=neutral,
//! PublicKeyToken=0123456789abcdef, ...`), and ClickOnce keeps the
//! deployed files in the user's application store,
//! `%LOCALAPPDATA%\Apps\2.0\<random>\<random>\<name>_<token>_<version>_<hash>\`,
//! the name lowercased and, when long, shortened to its first and last
//! four characters (`orde..tion` for `orders.application`). The program
//! there is the reference's application, whose own icon Pane draws before
//! it falls back to the shell's image of the reference.
//!
//! Plain file system reading, so the tests build a store of their own on
//! any system.

use std::path::{Path, PathBuf};

/// The deployment's name, lowercased, and its publisher's key token, as
/// a ClickOnce reference's `deployment` names them in its identity after
/// `#` (the name, else the URL's last segment); `None` when it names no
/// `.application` with a token.
fn identity(deployment: &str) -> Option<(String, String)> {
    let (url, identity) = deployment.split_once('#')?;
    let is_deployment = |name: &&str| name.to_lowercase().ends_with(".application");
    let name = identity
        .split(',')
        .map(str::trim)
        .next()
        .filter(is_deployment)
        .or_else(|| url.rsplit(['/', '\\']).next().filter(is_deployment))?;
    let token = identity.split(',').map(str::trim).find_map(|field| {
        let (key, value) = field.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("PublicKeyToken")
            .then(|| value.trim().to_lowercase())
    })?;
    (!token.is_empty() && token.chars().all(|c| c.is_ascii_hexdigit()))
        .then_some((name.to_lowercase(), token))
}

/// `name` as ClickOnce names a folder of its store after it: lowercased,
/// and, past ten characters, its first four and last four characters
/// around `..`.
fn store_name(name: &str) -> String {
    let name = name.to_lowercase();
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= 10 {
        return name;
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}..{tail}")
}

/// The program of the deployment `deployment` names (a ClickOnce
/// reference's first line) in the ClickOnce store at `store`
/// (`%LOCALAPPDATA%\Apps\2.0`): in the folders named after the deployment
/// and its publisher's token, two levels down, the newest version's, the
/// program named after the deployment (`Orders.exe` for
/// `Orders.application`), else its first program by name. `None` when
/// none is installed there.
pub(super) fn deployed_program(deployment: &str, store: &Path) -> Option<PathBuf> {
    let (name, token) = identity(deployment)?;
    let prefixes = [
        format!("{}_{token}_", store_name(&name)),
        format!("{name}_{token}_"),
    ];
    let stem = name
        .strip_suffix(".application")
        .unwrap_or(&name)
        .to_owned();
    // (version, program): the newest version wins.
    let mut best: Option<(String, PathBuf)> = None;
    for first in folders(store) {
        for second in folders(&first) {
            for deployed in folders(&second) {
                let Some(folder) = deployed
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_lowercase)
                else {
                    continue;
                };
                let Some(rest) = prefixes
                    .iter()
                    .find_map(|prefix| folder.strip_prefix(prefix.as_str()))
                else {
                    continue;
                };
                // `<version>_<hash>`: the version is fixed width
                // (`0001.0002`), so it orders as text.
                let version = rest.split('_').next().unwrap_or_default().to_owned();
                let Some(program) = program_in(&deployed, &stem) else {
                    continue;
                };
                if best.as_ref().is_none_or(|(newest, _)| version > *newest) {
                    best = Some((version, program));
                }
            }
        }
    }
    best.map(|(_, program)| program)
}

/// The folders in `dir`, by name; none when it cannot be read.
fn folders(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
}

/// The program in the deployed folder `dir`: the one named `stem` (without
/// case), else the first by name.
fn program_in(dir: &Path, stem: &str) -> Option<PathBuf> {
    let mut programs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        })
        .collect();
    programs.sort();
    let named = programs.iter().position(|path| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|found| found.eq_ignore_ascii_case(stem))
    });
    match named {
        Some(at) => Some(programs.swap_remove(at)),
        None => programs.into_iter().next(),
    }
}

/// The ClickOnce store of this user: `%LOCALAPPDATA%\Apps\2.0`.
#[cfg(windows)]
pub(super) fn store() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local).join("Apps").join("2.0"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEPLOYMENT: &str = "http://apps.example.com/Orders/Orders.application#Orders.application, \
                              Culture=neutral, PublicKeyToken=0123456789abcdef, \
                              processorArchitecture=msil";

    fn program(store: &Path, folders: [&str; 3], name: &str) -> PathBuf {
        let dir = store.join(folders[0]).join(folders[1]).join(folders[2]);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, b"MZ").unwrap();
        path
    }

    #[test]
    fn a_reference_names_its_deployment_and_publisher() {
        assert_eq!(
            identity(DEPLOYMENT),
            Some(("orders.application".into(), "0123456789abcdef".into()))
        );
        assert_eq!(identity("http://x/Orders.application"), None, "no token");
        assert_eq!(identity("not a deployment"), None);
        assert_eq!(store_name("orders.application"), "orde..tion");
        assert_eq!(store_name("tool.application"), "tool..tion");
        assert_eq!(store_name("a.exe"), "a.exe");
    }

    #[test]
    fn the_deployed_program_is_found_where_click_once_installs_it() {
        let store = tempfile::tempdir().unwrap();
        // Another publisher's deployment of the same name, the manifests
        // folder and an older version are passed over.
        program(
            store.path(),
            [
                "XK2P1Q3R.ABC",
                "M4N5B6V7.DEF",
                "orde..tion_fedcba9876543210_0002.0000_aaaa",
            ],
            "Orders.exe",
        );
        program(
            store.path(),
            [
                "XK2P1Q3R.ABC",
                "M4N5B6V7.DEF",
                "orde..tion_0123456789abcdef_0001.0000_bbbb",
            ],
            "Orders.exe",
        );
        std::fs::create_dir_all(
            store
                .path()
                .join("XK2P1Q3R.ABC")
                .join("M4N5B6V7.DEF")
                .join("manifests"),
        )
        .unwrap();
        let newest = program(
            store.path(),
            [
                "XK2P1Q3R.ABC",
                "M4N5B6V7.DEF",
                "orde..tion_0123456789abcdef_0001.0002_cccc",
            ],
            "Orders.exe",
        );
        // A helper beside it is not the application.
        program(
            store.path(),
            [
                "XK2P1Q3R.ABC",
                "M4N5B6V7.DEF",
                "orde..tion_0123456789abcdef_0001.0002_cccc",
            ],
            "Arelper.exe",
        );
        assert_eq!(deployed_program(DEPLOYMENT, store.path()), Some(newest));
    }

    #[test]
    fn a_deployment_not_installed_has_no_program() {
        let store = tempfile::tempdir().unwrap();
        assert_eq!(deployed_program(DEPLOYMENT, store.path()), None);
        assert_eq!(
            deployed_program(DEPLOYMENT, &store.path().join("missing")),
            None
        );
        // A folder of the deployment without a program.
        std::fs::create_dir_all(
            store
                .path()
                .join("A")
                .join("B")
                .join("orde..tion_0123456789abcdef_0001.0000_dddd"),
        )
        .unwrap();
        assert_eq!(deployed_program(DEPLOYMENT, store.path()), None);
        // A program of another name is taken when none is named after
        // the deployment.
        let only = program(
            store.path(),
            ["A", "B", "orde..tion_0123456789abcdef_0001.0001_eeee"],
            "OrdersClient.exe",
        );
        assert_eq!(deployed_program(DEPLOYMENT, store.path()), Some(only));
    }
}
