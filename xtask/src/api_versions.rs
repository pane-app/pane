//! The one extension API number (#317, ADR 0046): the version of the
//! `pane:extension` WIT package is the extension API version, and
//! everything else that names the API agrees with it — pane-core's
//! `EXTENSION_API` and `EXTENSION_API_PREVIOUS`, the versioned interface
//! names the runtime looks a component's exports up by, the JS guest
//! world's package version and references, the interface names Pane's
//! JavaScript build writes into a command's world, the SDKs' versions,
//! the templates' `apiVersion` and SDK pins, and every in-repo manifest
//! built from the current WIT. The fixtures that model builds of another
//! shape carry their own WIT copies and no manifest of this repository,
//! so nothing checks them; the previous world's positive fixtures name
//! the version they were built for on purpose, and are exempt by name.
//! Every disagreeing place is named at once, so a version bump is one
//! change, not a hunt.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

/// The previous world's positive fixtures under `guests/packages`, by
/// folder name: components built from `wit/previous` (ADR 0046), whose
/// manifests name the version they were built for, which this Pane runs
/// through the host's adapter. A fixture is added here as deliberately as
/// the negative controls' own WIT copies are kept.
const PREVIOUS_PACKAGES: [&str; 1] = ["previous-api"];

/// Checks that the `pane:extension` WIT package's version is the one
/// number the extension API has: every file and constant that names the
/// API says the version `wit/` declares. `ci-lints` runs this beside the
/// schema and the prebuilt-samples checks, in their shape: a failing
/// message names what to fix, and passing prints one line.
pub(crate) fn check() -> Result<(), String> {
    let root = crate::root();
    let mut problems = Vec::new();
    // The one number: the version wit/'s own files declare, all of them
    // the same, the patch 0 (the extension API version is MAJOR.MINOR,
    // ADR 0046). Nothing else can be compared while it cannot be read.
    let Some(version) = wit_versions(&root.join("wit"), "wit", &mut problems) else {
        return Err(failure(&problems));
    };
    let api = Api::new(version);

    // Pane's own number: the code carries what the WIT declares, never a
    // number of its own.
    if pane_core::EXTENSION_API != api.numbers() {
        problems.push(format!(
            "crates/pane-core/src/packages.rs: EXTENSION_API is {}.{}, while wit/ declares {}",
            pane_core::EXTENSION_API.0,
            pane_core::EXTENSION_API.1,
            api.wit
        ));
    }

    // The previous version's world, while its window lasts (ADR 0046):
    // `wit/previous/` holds it, and pane-core's constant names the
    // version it declares — None while the folder does not exist, as
    // today, 0.1 being the first version.
    let previous = root.join("wit/previous");
    let exists = previous.is_dir();
    let declared = if exists {
        wit_versions(&previous, "wit/previous", &mut problems).map(|version| (version.0, version.1))
    } else {
        None
    };
    if let Some(problem) =
        previous_constant_problem(pane_core::EXTENSION_API_PREVIOUS, exists, declared)
    {
        problems.push(problem);
    }

    // Every versioned `pane:` name in the three sources that name the
    // contract: the runtime's interface-name constants, which a
    // component's exports are looked up by; the JS guest world, which
    // JS/TS commands are componentized against; and the world Pane's
    // JavaScript build writes for each command.
    for path in [
        "crates/pane-core/src/runtime.rs",
        "guests/js/wit/world.wit",
        "crates/pane-build/src/js.rs",
    ] {
        api.check_names(&root, path, &mut problems);
    }

    // The SDKs are versioned with the API they target (ADR 0046): the
    // Rust crate's and the npm package's MAJOR.MINOR is the API version,
    // the patch their own.
    let sdk = "guests/pane-extension/Cargo.toml";
    api.check_cargo_version(&root, sdk, &mut problems);
    let package = "guests/js/package.json";
    api.check_npm_version(&root, package, &mut problems);

    // The authoring templates write the API an author targets: pane.json's
    // `apiVersion`, and the SDK pin the template's package installs. The
    // `@pane-app/cli` pins follow the CLI's own version, not the API.
    let templates = ["list", "detail", "form", "no-view"];
    let mut manifests = vec!["packages/extension-template/pane.json".to_owned()];
    let mut cargo_pins = Vec::new();
    let mut npm_pins = vec!["packages/extension-template/package.json".to_owned()];
    for template in templates {
        // The Rust templates `pane-ext new` writes.
        let folder = format!("crates/pane-core/templates/rust/{template}");
        manifests.push(format!("{folder}/pane.json"));
        cargo_pins.push(format!("{folder}/Cargo.toml"));
    }
    for template in templates {
        // The TypeScript templates `pane-ext new` writes.
        let folder = format!("crates/pane-core/templates/typescript/{template}");
        manifests.push(format!("{folder}/pane.json"));
        npm_pins.push(format!("{folder}/package.json"));
    }
    for path in &manifests {
        api.check_manifest(&root, path, &mut problems);
    }
    for path in &cargo_pins {
        api.check_cargo_pin(&root, path, &mut problems);
    }
    for path in &npm_pins {
        api.check_npm_pin(&root, path, &mut problems);
    }

    // The in-repo manifests: every package built from the current WIT.
    // The previous world's positive fixtures are exempt by name: they are
    // built from `wit/previous` on purpose and name the version they were
    // built for, exactly as the negative controls under `guests/fixtures/`
    // carry their own WIT and no manifest of this repository, so nothing
    // checks them.
    for folder in package_folders(&root, &mut problems) {
        let path = format!("guests/packages/{folder}/pane.json");
        // A folder without a manifest is no package of this walk's to
        // read; one with it is checked like every other manifest.
        if root.join(&path).is_file() && !PREVIOUS_PACKAGES.contains(&folder.as_str()) {
            api.check_manifest(&root, &path, &mut problems);
        }
    }

    if problems.is_empty() {
        println!(
            "xtask: every place that names the extension API says {}",
            api.manifest
        );
        return Ok(());
    }
    Err(failure(&problems))
}

/// The extension API the check holds everything against: the version the
/// `pane:extension` WIT package declares, in the shapes the places that
/// name it write.
struct Api {
    /// The version wit/ declares, `X.Y.Z` with the patch 0: the one
    /// number.
    version: (u64, u64, u64),
    /// `X.Y`, as a manifest's `apiVersion` names the API.
    manifest: String,
    /// `pane:extension@X.Y.Z`, as the WIT package and the interface names
    /// declare it.
    wit: String,
}

impl Api {
    /// The API the WIT declares, in the shapes its names take.
    fn new(version: (u64, u64, u64)) -> Api {
        Api {
            version,
            manifest: format!("{}.{}", version.0, version.1),
            wit: format!("pane:extension@{}.{}.{}", version.0, version.1, version.2),
        }
    }

    /// (MAJOR, MINOR), as the SDKs' versions and the templates' pins
    /// target the API: the patch is their own.
    fn numbers(&self) -> (u64, u64) {
        (self.version.0, self.version.1)
    }

    /// Checks every versioned `pane:` name in the file at `path`
    /// (relative to the repository root): a name that carries a version
    /// must carry the one wit/ declares. Names without a version (the
    /// programs prefix a bundle's imports are found by, an interface a
    /// comment mentions) carry no number and are skipped; a file with no
    /// versioned name at all leaves the check blind, and is a problem of
    /// its own.
    fn check_names(&self, root: &Path, path: &str, problems: &mut Vec<String>) {
        let Some(text) = read(root, path, problems) else {
            return;
        };
        let mut names = versioned_pane_names(&text);
        names.sort();
        names.dedup();
        if names.is_empty() {
            problems.push(format!(
                "{path} names no `pane:` interface with a version, so this check cannot see \
                 the interface names"
            ));
        }
        for (name, version) in names {
            if version != self.version {
                problems.push(format!("{path}: {name}, while wit/ declares {}", self.wit));
            }
        }
    }

    /// Checks the manifest at `path` (relative to the repository root):
    /// its `apiVersion` names the API the package was built for, and
    /// every package of this repository is built from the current WIT.
    fn check_manifest(&self, root: &Path, path: &str, problems: &mut Vec<String>) {
        let Some(text) = read(root, path, problems) else {
            return;
        };
        let Some(manifest) = json(path, &text, problems) else {
            return;
        };
        let found = manifest["apiVersion"].as_str();
        if found != Some(self.manifest.as_str()) {
            problems.push(format!(
                "{path}: apiVersion {}, while wit/ declares {}",
                found.unwrap_or("none"),
                self.wit
            ));
        }
    }

    /// Checks the `[package]` `version` of the Cargo manifest at `path`:
    /// the Rust SDK's version targets the API, its patch its own.
    fn check_cargo_version(&self, root: &Path, path: &str, problems: &mut Vec<String>) {
        let Some(text) = read(root, path, problems) else {
            return;
        };
        let version = cargo_string_value(&text, "package", "version");
        if version.as_deref().and_then(parse_major_minor) != Some(self.numbers()) {
            problems.push(format!(
                "{path}: version {}, while wit/ declares {}",
                version.as_deref().unwrap_or("none"),
                self.wit
            ));
        }
    }

    /// Checks the `version` of the package.json at `path`: the npm
    /// SDK's version targets the API, its patch its own.
    fn check_npm_version(&self, root: &Path, path: &str, problems: &mut Vec<String>) {
        let Some(text) = read(root, path, problems) else {
            return;
        };
        let Some(package) = json(path, &text, problems) else {
            return;
        };
        let version = package["version"].as_str();
        if version.and_then(parse_major_minor) != Some(self.numbers()) {
            problems.push(format!(
                "{path}: version {}, while wit/ declares {}",
                version.unwrap_or("none"),
                self.wit
            ));
        }
    }

    /// Checks the `pane-extension` dependency of the Cargo manifest at
    /// `path`: a Rust template's package installs the SDK of the API it
    /// targets, so the pin's MAJOR.MINOR is the API version, the patch
    /// the SDK's own.
    fn check_cargo_pin(&self, root: &Path, path: &str, problems: &mut Vec<String>) {
        let Some(text) = read(root, path, problems) else {
            return;
        };
        let pin = cargo_string_value(&text, "dependencies", "pane-extension");
        if pin.as_deref().and_then(parse_major_minor) != Some(self.numbers()) {
            problems.push(format!(
                "{path}: pane-extension {}, while wit/ declares {}",
                pin.as_deref().unwrap_or("none"),
                self.wit
            ));
        }
    }

    /// Checks the `@pane-app/extension` dependency of the package.json at
    /// `path`: a TypeScript template's package installs the SDK of the
    /// API it targets, so the pin's MAJOR.MINOR is the API version, the
    /// patch the SDK's own. The `@pane-app/cli` pins follow the CLI's own
    /// version and are not checked.
    fn check_npm_pin(&self, root: &Path, path: &str, problems: &mut Vec<String>) {
        let Some(text) = read(root, path, problems) else {
            return;
        };
        let Some(package) = json(path, &text, problems) else {
            return;
        };
        let pin = npm_pin(&package, "@pane-app/extension");
        if pin.and_then(parse_major_minor) != Some(self.numbers()) {
            problems.push(format!(
                "{path}: @pane-app/extension {}, while wit/ declares {}",
                pin.unwrap_or("none"),
                self.wit
            ));
        }
    }
}

/// The check's failure: every problem, and what they mean — the WIT
/// package's version is the one extension API number, so every place
/// that names the API moves with it, never apart.
fn failure(problems: &[String]) -> String {
    format!(
        "{}. The `pane:extension` WIT package's version is the one extension API number \
         (ADR 0046): every place that names the API moves with the WIT, never apart",
        problems.join("; ")
    )
}

/// A file's text, or the problem pushed for why it cannot be read: the
/// check reports the file and moves on, so one unreadable file cannot
/// hide the disagreements of the rest.
fn read(root: &Path, path: &str, problems: &mut Vec<String>) -> Option<String> {
    match std::fs::read_to_string(root.join(path)) {
        Ok(text) => Some(text),
        Err(error) => {
            problems.push(format!("read {path} failed: {error}"));
            None
        }
    }
}

/// The file's parsed JSON, or the problem pushed for why it is not: the
/// manifests and package files this reads are never written here.
fn json(path: &str, text: &str, problems: &mut Vec<String>) -> Option<Value> {
    match serde_json::from_str(text) {
        Ok(value) => Some(value),
        Err(error) => {
            problems.push(format!("{path} is not valid JSON: {error}"));
            None
        }
    }
}

/// The extension API version the `.wit` files directly under `dir`
/// declare in their `package pane:extension@…` lines, reported as the
/// files of `label`: every file must declare one version, its patch 0.
fn wit_versions(dir: &Path, label: &str, problems: &mut Vec<String>) -> Option<(u64, u64, u64)> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            problems.push(format!("read {label} failed: {error}"));
            return None;
        }
    };
    // Only the package's own files, in name order for a stable message:
    // `deps/` holds WASI's own WIT, and `previous/` inside `wit/` is the
    // previous world, which is checked on its own.
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_file() && name.ends_with(".wit") {
            names.push(name);
        }
    }
    names.sort();
    if names.is_empty() {
        problems.push(format!("{label} holds no .wit files"));
        return None;
    }
    let mut files = Vec::new();
    for name in names {
        // A file that cannot be read is reported and counts as declaring
        // nothing.
        let version = match std::fs::read_to_string(dir.join(&name)) {
            Ok(text) => wit_package_version(&text),
            Err(error) => {
                problems.push(format!("read {label}/{name} failed: {error}"));
                None
            }
        };
        files.push((name, version));
    }
    wit_agreement(files, label, problems)
}

/// The version the files' `package pane:extension@…` declarations agree
/// on, with every disagreement pushed onto `problems`: a file that
/// declares none, or a patch that is not 0, is a problem of its own, and
/// the version most files declare is the one the rest of the check
/// compares with, so a bump that misses files is reported against the
/// new version, not drowned in every file's disagreement.
fn wit_agreement(
    files: Vec<(String, Option<(u64, u64, u64)>)>,
    label: &str,
    problems: &mut Vec<String>,
) -> Option<(u64, u64, u64)> {
    let mut declared = Vec::new();
    for (name, version) in files {
        match version {
            None => {
                problems.push(format!("{label}/{name} declares no pane:extension package"));
            }
            Some((major, minor, patch)) => {
                if patch != 0 {
                    problems.push(format!(
                        "{label}/{name} declares pane:extension@{major}.{minor}.{patch}: the \
                         extension API version is MAJOR.MINOR, so the patch must be 0"
                    ));
                }
                declared.push((name, (major, minor, patch)));
            }
        }
    }
    // What most files declare is the number everything else is compared
    // with; a tie takes the largest, since the files that must move are
    // named either way.
    let mut counts: BTreeMap<(u64, u64), usize> = BTreeMap::new();
    for (_, (major, minor, _)) in &declared {
        *counts.entry((*major, *minor)).or_default() += 1;
    }
    let api = match counts.iter().max_by_key(|(_, count)| *count) {
        Some((version, _)) => *version,
        None => return None,
    };
    for (name, (major, minor, patch)) in declared {
        if (major, minor) != api {
            problems.push(format!(
                "{label}/{name} declares pane:extension@{major}.{minor}.{patch}, but the \
                 files under {label} must all declare pane:extension@{}.{}.0",
                api.0, api.1
            ));
        }
    }
    Some((api.0, api.1, 0))
}

/// The version a WIT file's `package pane:extension@X.Y.Z;` line
/// declares, or None when it declares none: the one number's source.
/// `release_notes` reads the current version with it, too.
pub(crate) fn wit_package_version(text: &str) -> Option<(u64, u64, u64)> {
    for line in text.lines() {
        // A package declaration is one line, its version ending at the
        // semicolon.
        let Some(rest) = line.trim_start().strip_prefix("package pane:extension@") else {
            continue;
        };
        let version = rest.split(';').next().unwrap_or_default().trim();
        if let Some(version) = parse_version(version) {
            return Some(version);
        }
    }
    None
}

/// The problem with pane-core's `EXTENSION_API_PREVIOUS` against the
/// world `wit/previous/` declares, if there is one: the constant is None
/// exactly while the folder holds no world, and Some naming the version
/// its files declare while it does. A folder whose files disagree has
/// already been reported, and there is no version to compare against.
fn previous_constant_problem(
    constant: Option<(u64, u64)>,
    folder_exists: bool,
    declared: Option<(u64, u64)>,
) -> Option<String> {
    match (constant, folder_exists, declared) {
        (_, true, None) => None,
        (None, false, _) => None,
        (constant, false, _) => Some(format!(
            "crates/pane-core/src/packages.rs: EXTENSION_API_PREVIOUS is {}, while \
             wit/previous/ holds no world",
            some_version(constant)
        )),
        (constant, true, Some((major, minor))) if constant != Some((major, minor)) => {
            Some(format!(
                "crates/pane-core/src/packages.rs: EXTENSION_API_PREVIOUS is {}, while \
                 wit/previous/ declares pane:extension@{major}.{minor}.0",
                some_version(constant)
            ))
        }
        _ => None,
    }
}

/// `Some(0.1)` or `None`, as a previous-version constant is named in a
/// problem.
fn some_version(version: Option<(u64, u64)>) -> String {
    match version {
        Some((major, minor)) => format!("Some({major}.{minor})"),
        None => "None".into(),
    }
}

/// Every `pane:` name in `text` that carries a version —
/// `pane:extension/…@X.Y.Z`, `pane:extension@X.Y.Z`,
/// `pane:js-guest@X.Y.Z` — as the name and the version it names. A name
/// without a version carries no number and is skipped: the programs
/// prefix a bundle's imports are found by, an interface a comment
/// mentions. A name runs on while its characters can be part of one, so
/// a quote, a semicolon or a space ends it — in a Rust source and a WIT
/// world alike, which is what picks the names out of either.
fn versioned_pane_names(text: &str) -> Vec<(String, (u64, u64, u64))> {
    let mut names = Vec::new();
    for (at, _) in text.match_indices("pane:") {
        let name: String = text[at..].chars().take_while(|c| name_char(*c)).collect();
        let Some((_, version)) = name.rsplit_once('@') else {
            continue;
        };
        if let Some(version) = parse_version(version) {
            names.push((name, version));
        }
    }
    names
}

/// Whether `c` can be part of a `pane:` name: a WIT identifier's letter,
/// digit or dash, a version's dot, or a separator that shapes one.
fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '/' | ':' | '@')
}

/// The `X.Y.Z` a version text names, if it is one. `wit_guard` reads
/// release tags with it, too.
pub(crate) fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

/// The `X.Y` a version or pin text names, with or without its patch: the
/// SDKs' versions and the templates' pins carry the API in their
/// MAJOR.MINOR, the patch their own (ADR 0046).
fn parse_major_minor(text: &str) -> Option<(u64, u64)> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    match parts.next() {
        None => Some((major, minor)),
        // A patch, when the text names one: not compared.
        Some(patch) => {
            (patch.parse::<u64>().is_ok() && parts.next().is_none()).then_some((major, minor))
        }
    }
}

/// The quoted value of the `key = "…"` line in the `section` of a Cargo
/// manifest, if it has one: the plain lines the checked manifests hold
/// (the SDK's `version`, the Rust templates' `pane-extension` pin), with
/// comment lines and other sections skipped.
fn cargo_string_value(manifest: &str, section: &str, key: &str) -> Option<String> {
    let section = format!("[{section}]");
    let mut in_section = false;
    for line in manifest.lines() {
        // A table's header line starts a new section.
        if line.trim_start().starts_with('[') {
            in_section = line.trim() == section.as_str();
            continue;
        }
        if !in_section {
            continue;
        }
        let Some(rest) = line.trim_start().strip_prefix(key) else {
            continue;
        };
        // The key's line, `key = "value"`; a longer name the key starts
        // with is another key's line.
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim();
        return value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .map(String::from);
    }
    None
}

/// The version constraint the package.json `package` pins `dependency`
/// at, from its `dependencies` or `devDependencies`, or None when it
/// names none.
fn npm_pin<'a>(package: &'a Value, dependency: &str) -> Option<&'a str> {
    for section in ["dependencies", "devDependencies"] {
        let pin = package[section].get(dependency).and_then(Value::as_str);
        if pin.is_some() {
            return pin;
        }
    }
    None
}

/// The package folders under `guests/packages`, in name order: every one
/// of them is a package built from the current WIT. The fixtures that
/// model builds of another shape live under `guests/fixtures/`, not
/// here, and are never read.
fn package_folders(root: &Path, problems: &mut Vec<String>) -> Vec<String> {
    let entries = match std::fs::read_dir(root.join("guests/packages")) {
        Ok(entries) => entries,
        Err(error) => {
            problems.push(format!("read guests/packages failed: {error}"));
            return Vec::new();
        }
    };
    let mut folders = Vec::new();
    for entry in entries.flatten() {
        // A file beside the packages is none of them.
        if entry.path().is_dir() {
            let folder = entry.file_name().to_string_lossy().into_owned();
            folders.push(folder);
        }
    }
    folders.sort();
    folders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_three_dotted_numbers() {
        assert_eq!(parse_version("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.1"), None);
        assert_eq!(parse_version("0.1.0.1"), None);
        assert_eq!(parse_version("one"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn major_minor_versions_tolerate_their_own_patch() {
        assert_eq!(parse_major_minor("0.1"), Some((0, 1)));
        assert_eq!(parse_major_minor("0.1.5"), Some((0, 1)));
        assert_eq!(parse_major_minor("1.2.0"), Some((1, 2)));
        assert_eq!(parse_major_minor("^0.1"), None);
        assert_eq!(parse_major_minor("0"), None);
        assert_eq!(parse_major_minor("0.1.0.1"), None);
    }

    #[test]
    fn a_wit_file_s_package_declaration_is_read() {
        // After the header comment the files carry, with or without the
        // declaration's trailing semicolon.
        let wit = "// a header comment\n\npackage pane:extension@0.2.0;\n";
        assert_eq!(wit_package_version(wit), Some((0, 2, 0)));
        assert_eq!(
            wit_package_version("package pane:extension@0.1.0"),
            Some((0, 1, 0))
        );
        // Another package, or no declaration at all.
        assert_eq!(wit_package_version("package pane:js-guest@0.1.0;"), None);
        assert_eq!(wit_package_version("world w {}"), None);
        // A mention in a comment is not a declaration.
        assert_eq!(
            wit_package_version("// package pane:extension@9.9.9;"),
            None
        );
    }

    #[test]
    fn versioned_pane_names_skip_names_without_a_version() {
        // What the runtime's constants, the JS guest world and the
        // JavaScript build's world write: the names that carry a version
        // are compared, and the ones that carry none are not — the
        // programs prefix a bundle's imports are found by, an interface
        // a comment mentions, a resource's dotted name.
        let source = r#"
            /// It imports `pane:extension/programs`: its code can run
            const COMMAND_INTERFACE: &str = "pane:extension/command@0.1.0";
            const PROGRAMS_PREFIX: &str = "pane:extension/programs@";
            const REGISTRATIONS: &str = "pane:extension/registrations.timer";
            fn uses(bundle: &str) -> bool { bundle.contains("pane:extension/other") }
        "#;
        assert_eq!(
            versioned_pane_names(source),
            vec![("pane:extension/command@0.1.0".into(), (0, 1, 0))]
        );
        let world = "\
package pane:js-guest@0.2.0;\n\
world js-extension {\n  include pane:extension/extension-with-data@0.2.0;\n}\
";
        assert_eq!(
            versioned_pane_names(world),
            vec![
                ("pane:js-guest@0.2.0".into(), (0, 2, 0)),
                ("pane:extension/extension-with-data@0.2.0".into(), (0, 2, 0)),
            ]
        );
    }

    #[test]
    fn wit_files_must_all_declare_one_version() {
        // All agreeing: the version they declare. A nonzero patch is
        // reported on its own and left out of the comparison, since the
        // API version is MAJOR.MINOR.
        let mut problems = Vec::new();
        let files = vec![
            ("a.wit".to_owned(), Some((0, 2, 0))),
            ("b.wit".to_owned(), Some((0, 2, 5))),
        ];
        assert_eq!(wit_agreement(files, "wit", &mut problems), Some((0, 2, 0)));
        assert_eq!(
            problems,
            vec![
                "wit/b.wit declares pane:extension@0.2.5: the extension API version is \
                 MAJOR.MINOR, so the patch must be 0"
                    .to_owned()
            ]
        );
        // A bump that missed a file: the version most files declare is
        // the number, and the missed file is named against it.
        let mut problems = Vec::new();
        let files = vec![
            ("a.wit".to_owned(), Some((0, 1, 0))),
            ("b.wit".to_owned(), Some((0, 2, 0))),
            ("c.wit".to_owned(), Some((0, 2, 0))),
        ];
        assert_eq!(wit_agreement(files, "wit", &mut problems), Some((0, 2, 0)));
        assert_eq!(
            problems,
            vec![
                "wit/a.wit declares pane:extension@0.1.0, but the files under wit must all \
                 declare pane:extension@0.2.0"
                    .to_owned()
            ]
        );
        // A file that declares nothing: nothing to compare with.
        let mut problems = Vec::new();
        let files = vec![("a.wit".to_owned(), None)];
        assert_eq!(wit_agreement(files, "wit", &mut problems), None);
        assert_eq!(
            problems,
            vec!["wit/a.wit declares no pane:extension package".to_owned()]
        );
    }

    #[test]
    fn the_previous_constant_follows_the_previous_world() {
        // Today: no world, no previous version — and no problem.
        assert_eq!(previous_constant_problem(None, false, None), None);
        // The world without its constant, and the constant without its
        // world.
        assert_eq!(
            previous_constant_problem(None, true, Some((0, 1))),
            Some(
                "crates/pane-core/src/packages.rs: EXTENSION_API_PREVIOUS is None, while \
                 wit/previous/ declares pane:extension@0.1.0"
                    .to_owned()
            )
        );
        assert_eq!(
            previous_constant_problem(Some((0, 1)), false, None),
            Some(
                "crates/pane-core/src/packages.rs: EXTENSION_API_PREVIOUS is Some(0.1), \
                 while wit/previous/ holds no world"
                    .to_owned()
            )
        );
        // The constant naming the version the world declares.
        assert_eq!(
            previous_constant_problem(Some((0, 1)), true, Some((0, 1))),
            None
        );
        // A world whose files disagree: wit_agreement already reported.
        assert_eq!(previous_constant_problem(Some((0, 1)), true, None), None);
        // The constant naming another version.
        assert_eq!(
            previous_constant_problem(Some((0, 9)), true, Some((0, 1))),
            Some(
                "crates/pane-core/src/packages.rs: EXTENSION_API_PREVIOUS is Some(0.9), \
                 while wit/previous/ declares pane:extension@0.1.0"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_cargo_manifest_s_value_is_read_from_its_section() {
        // The SDK's [package] version and the Rust templates' pin, with
        // comment lines, other sections and inline tables skipped.
        let manifest = r#"
[package]
name = "pane-extension"
# The version follows the extension API it targets (ADR 0046).
version = "0.1.0"

[dependencies]
# Pane's Rust SDK, from crates.io.
pane-extension = "0.1"
wit-bindgen = { version = "=0.62.0", features = ["macros"] }
"#;
        assert_eq!(
            cargo_string_value(manifest, "package", "version"),
            Some("0.1.0".to_owned())
        );
        assert_eq!(
            cargo_string_value(manifest, "dependencies", "pane-extension"),
            Some("0.1".to_owned())
        );
        assert_eq!(
            cargo_string_value(manifest, "package", "name"),
            Some("pane-extension".to_owned())
        );
        // An inline table's version is no key of the section, and a key
        // another key's name starts with is not that key.
        assert_eq!(
            cargo_string_value(manifest, "dependencies", "version"),
            None
        );
        assert_eq!(cargo_string_value(manifest, "dependencies", "pane"), None);
    }

    #[test]
    fn an_npm_pin_is_read_from_either_dependency_table() {
        let package: Value = serde_json::from_str(
            r#"{
                "version": "0.1.0",
                "devDependencies": { "@pane-app/extension": "0.1.0" },
                "dependencies": { "other": "1.0.0" }
            }"#,
        )
        .unwrap();
        assert_eq!(npm_pin(&package, "@pane-app/extension"), Some("0.1.0"));
        assert_eq!(npm_pin(&package, "@pane-app/cli"), None);
        assert_eq!(npm_pin(&package, "other"), Some("1.0.0"));
    }

    #[test]
    fn a_file_that_is_not_json_is_a_problem() {
        let mut problems = Vec::new();
        assert!(json("x/pane.json", "{ not json", &mut problems).is_none());
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("x/pane.json is not valid JSON"));
    }

    #[test]
    fn a_file_that_cannot_be_read_is_a_problem() {
        // The paths this walks are relative to the repository root; a
        // path that names no file is reported, and nothing panics.
        let root = std::env::temp_dir();
        let mut problems = Vec::new();
        assert!(read(&root, "no/such/file", &mut problems).is_none());
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("read no/such/file failed:"));
    }

    #[test]
    fn a_manifest_names_the_api_or_is_a_problem() {
        // The check walks paths relative to a root folder, as it walks
        // the repository's own: a manifest that names the API passes,
        // one that names another version is a problem.
        let root = std::env::temp_dir().join("xtask-api-versions-test");
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::write(root.join("a/pane.json"), r#"{"apiVersion": "0.1"}"#).unwrap();
        std::fs::write(root.join("b/pane.json"), r#"{"apiVersion": "0.2"}"#).unwrap();
        let api = Api::new((0, 1, 0));
        let mut problems = Vec::new();
        api.check_manifest(&root, "a/pane.json", &mut problems);
        assert!(problems.is_empty());
        api.check_manifest(&root, "b/pane.json", &mut problems);
        assert_eq!(
            problems,
            vec![
                "b/pane.json: apiVersion 0.2, while wit/ declares pane:extension@0.1.0".to_owned()
            ]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_failure_names_every_problem_and_the_rule() {
        assert_eq!(
            failure(&["one".to_owned(), "two".to_owned()]),
            "one; two. The `pane:extension` WIT package's version is the one extension API \
             number (ADR 0046): every place that names the API moves with the WIT, never \
             apart"
        );
    }
}
