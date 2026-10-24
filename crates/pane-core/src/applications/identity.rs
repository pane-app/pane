//! What makes an application the same application: the pure rules behind
//! the stable id the host gives each one (ADR 0038, "a live list, by
//! identity"), compiled and tested on every system.
//!
//! Each system's adapter reports the [`Source`]s it finds (a shortcut, a
//! packaged app, a bundle, a desktop entry), each with the typed [`Key`] of
//! what it opens. Sources with one key are one application, whose id is a
//! digest of that key ([`Key::id`]): the same on every start and on every
//! machine with the same installation, and opaque to extensions. The
//! [`Catalog`] groups the sources, merges the shortcuts that run one program
//! under one name ([`owners`]), elects each application's primary source
//! ([`primary`]), whose name is its title and which Pane opens, and keeps
//! the map from every id (every earlier id of its sources, and every
//! source's path, which was the application's id before identities) to
//! what opens it.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use super::{Application, names};

/// What identifies an application, whichever source it was found by.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// A Windows desktop program whose shortcut carries no AppUserModelID:
    /// its shortcut's resolved target, in lowercase with version folders as
    /// a wildcard ([`program_target`]), and the arguments the shortcut
    /// passes, so that two browser profiles or two progressive web apps
    /// stay apart. Build it with [`Key::program`].
    Program { target: String, arguments: String },
    /// A Windows desktop program's shortcut that carries its own
    /// AppUserModelID (not a packaged app's), in lowercase: the id Windows
    /// groups a program's shortcuts and windows by, so Discord's Desktop and
    /// Start menu shortcuts, which pass it different arguments, are one
    /// application, while browser profiles and web apps, which carry their
    /// own, stay apart.
    App(String),
    /// A Windows packaged (AppX/MSIX) app: its package family name, in
    /// lowercase, and its AppUserModelID when it is not the package's
    /// first app ([`packaged_keys`]).
    Package { family: String, app: Option<String> },
    /// A macOS bundle identifier, in lowercase (Launch Services compares
    /// them without case).
    Bundle(String),
    /// A Linux desktop file id (`org.gnome.Terminal.desktop`).
    DesktopFile(String),
    /// A Windows shortcut that opens no program file but a link the system
    /// hands to its handler, in lowercase: an internet shortcut's (`.url`)
    /// URL, such as `steam://rungameid/570`, or a ClickOnce application's
    /// (`.appref-ms`) deployment, so that one link on the Desktop and in the
    /// Start menu is one application.
    Link(String),
    /// A source with nothing better to identify it by, by its own path: a
    /// bundle without an identifier, a shortcut whose target Pane could not
    /// read.
    Path(String),
}

impl Key {
    /// The key of a Windows shortcut to `target` with `arguments`.
    pub fn program(target: &str, arguments: &str) -> Key {
        Key::Program {
            target: program_target(target),
            arguments: arguments.trim().to_owned(),
        }
    }

    /// The application's id: the first 128 bits of a SHA-256 digest of the
    /// key's kind and fields, as 32 lowercase hexadecimal digits. Stable
    /// across starts and machines; extensions must not parse it.
    pub fn id(&self) -> String {
        let (kind, fields): (&str, Vec<&str>) = match self {
            Key::Program { target, arguments } => {
                ("program", vec![target.as_str(), arguments.as_str()])
            }
            Key::App(aumid) => ("app", vec![aumid.as_str()]),
            Key::Package { family, app: None } => ("package", vec![family.as_str()]),
            Key::Package {
                family,
                app: Some(app),
            } => ("package", vec![family.as_str(), app.as_str()]),
            Key::Bundle(identifier) => ("bundle", vec![identifier.as_str()]),
            Key::DesktopFile(id) => ("desktop-file", vec![id.as_str()]),
            Key::Link(link) => ("link", vec![link.as_str()]),
            Key::Path(path) => ("path", vec![path.as_str()]),
        };
        let mut digest = Sha256::new();
        digest.update(kind.as_bytes());
        for field in fields {
            // A separator no field holds, so ("ab", "c") and ("a", "bc")
            // never digest alike.
            digest.update([0]);
            digest.update(field.as_bytes());
        }
        digest.finalize()[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

/// Whether `id` has the form of an id [`Key::id`] gives, rather than a path
/// an application had as its id before identities.
pub fn is_identity(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// How long a version folder's prefix may be (`app-` in `app-1.2.3`).
const MAX_VERSION_PREFIX: usize = 8;

/// `segment`, one path segment in lowercase, with its version replaced by
/// `*` when it is a version folder: digits separated by one to three dots
/// (`1.2`, `1.2.3.4`), optionally after a short prefix of letters and one
/// separator (`app-1.2.3` becomes `app-*`, `v2.0` becomes `v*`); `None`
/// when it is not one. Squirrel-style updaters (Discord, Slack) move their
/// program into a new such folder on every update.
pub fn version_wildcard(segment: &str) -> Option<String> {
    let start = segment.find(|c: char| c.is_ascii_digit())?;
    let (prefix, version) = segment.split_at(start);
    let parts: Vec<&str> = version.split('.').collect();
    let is_version = (2..=4).contains(&parts.len())
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    let letters = prefix.strip_suffix(['-', '_', ' ']).unwrap_or(prefix);
    let short_prefix =
        letters.len() <= MAX_VERSION_PREFIX && letters.chars().all(|c| c.is_ascii_alphabetic());
    (is_version && short_prefix).then(|| format!("{prefix}*"))
}

/// A shortcut's resolved `target` as an identity key holds it: in
/// lowercase, with `\` separators, and every version folder
/// ([`version_wildcard`]) replaced by a wildcard, so that a program updated
/// into a new version folder keeps its identity.
pub fn program_target(target: &str) -> String {
    target
        .trim()
        .to_lowercase()
        .replace('/', "\\")
        .split('\\')
        .map(|segment| version_wildcard(segment).unwrap_or_else(|| segment.to_owned()))
        .collect::<Vec<_>>()
        .join("\\")
}

/// The keys of packaged apps, one per AppUserModelID in `aumids`
/// (`<package family>!<app>`), in the same order: the package family alone
/// for each package's first app (by app id, so the choice does not depend
/// on the order the shell lists them in), and the family with the
/// AppUserModelID for every other app of a package. A package keeps its
/// identity across updates, which keep its family.
pub fn packaged_keys(aumids: &[String]) -> Vec<Key> {
    let family = |aumid: &str| -> String {
        aumid
            .split_once('!')
            .map_or(aumid, |(family, _)| family)
            .to_lowercase()
    };
    let mut first: HashMap<String, String> = HashMap::new();
    for aumid in aumids {
        let lowered = aumid.to_lowercase();
        first
            .entry(family(aumid))
            .and_modify(|kept| {
                if lowered < *kept {
                    kept.clone_from(&lowered);
                }
            })
            .or_insert(lowered);
    }
    aumids
        .iter()
        .map(|aumid| {
            let family = family(aumid);
            let lowered = aumid.to_lowercase();
            let app = (first.get(&family) != Some(&lowered)).then_some(lowered);
            Key::Package { family, app }
        })
        .collect()
}

/// One place an application was found by: a shortcut, a packaged app, a
/// bundle or a desktop entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// What it opens, which says which application it is.
    pub key: Key,
    /// What the system opens: the path of the shortcut, bundle or desktop
    /// entry, or `shell:AppsFolder\<AppUserModelID>` for a packaged app.
    /// Before identities this was the application's id, so it still names
    /// the application ([`Catalog::find`]).
    pub path: String,
    /// Its name, as the system shows it: localized where the system
    /// localizes it.
    pub name: String,
    /// Where it was found, for people.
    pub location: String,
    /// How preferred the place it was found in is, lowest first: on Windows
    /// the user's Desktop, the all-users Desktop, the user's Start menu,
    /// the all-users Start menu, taskbar pins, then the Apps folder; on
    /// macOS `/Applications`, `/System/Applications`, `~/Applications`.
    pub place: usize,
    /// Its untranslated name, when the system shows it by another (a
    /// Windows shortcut's file name, a bundle's folder name, a desktop
    /// entry's plain `Name`): it still finds the application
    /// ([`super::names`]).
    pub untranslated: Option<String>,
    /// Words that find it besides its names: a desktop entry's `Keywords`
    /// for the user's locale.
    pub keywords: Vec<String>,
    /// The program it runs: a Windows shortcut's target path, or the
    /// program a desktop entry's `Exec` runs as written (a path or a
    /// command); `None` when Pane does not know one (a packaged app, a
    /// bundle). Its name may be an alternate title, and it tells
    /// same-name applications apart ([`super::names`]).
    pub program: Option<String>,
    /// Whether it passes [`Source::program`] arguments that say what it
    /// opens (a browser's web app), so that the program's name is not one
    /// of its alternate titles.
    pub arguments: bool,
    /// The key it had before shortcuts were keyed by their AppUserModelID
    /// (#508), when that is not [`Source::key`]: the id of that key still
    /// names its application ([`Catalog::find`]), so what a quick slot or
    /// root search's learning holds under it carries over.
    pub previous: Option<Key>,
}

impl Source {
    /// A source found by `name` alone, with no other names, no keywords
    /// and no program known.
    pub fn new(
        key: Key,
        path: impl Into<String>,
        name: impl Into<String>,
        location: impl Into<String>,
        place: usize,
    ) -> Source {
        Source {
            key,
            path: path.into(),
            name: name.into(),
            location: location.into(),
            place,
            untranslated: None,
            keywords: Vec::new(),
            program: None,
            arguments: false,
            previous: None,
        }
    }
}

/// Which of `sources`, all of one application, is its primary source, whose
/// name is its title and which Pane opens: the one found in the most
/// preferred place, then one passing its program no arguments, then the
/// one with the shorter path, then the first by path, so the choice never
/// depends on the order they were found in. `None` only when `sources` is
/// empty.
pub fn primary(sources: &[Source]) -> Option<usize> {
    (0..sources.len()).min_by(|&a, &b| {
        let (a, b) = (&sources[a], &sources[b]);
        (a.place, a.arguments, a.path.len(), &a.path).cmp(&(
            b.place,
            b.arguments,
            b.path.len(),
            &b.path,
        ))
    })
}

/// For each group of sources with one key (`keys`, the group of each of
/// `sources` in `group_of`), the group whose key names its application:
/// itself, or the group it merges into. Windows shortcuts that run one
/// program (their resolved [`Source::program`], as [`program_target`] keys
/// it) under one name (compared without case) are one application, whatever
/// arguments they pass: a Desktop shortcut passing `--processStart` and a
/// Start menu one passing nothing. Two different AppUserModelIDs are never
/// merged, nor is a plain shortcut merged into one of several that share
/// its program and name, so browser profiles and web apps stay apart.
///
/// A merged application is named by its AppUserModelID's group when it has
/// one, else by the group passing the fewest arguments (the shortest, then
/// the first in order), so its id does not depend on the order shortcuts
/// were found in.
pub fn owners(keys: &[Key], sources: &[Source], group_of: &[usize]) -> Vec<usize> {
    let is_app = |group: usize| matches!(keys[group], Key::App(_));
    // Each group's program and name pairs, and the groups sharing each.
    let mut shared: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (source, &group) in sources.iter().zip(group_of) {
        if !matches!(keys[group], Key::Program { .. } | Key::App(_)) {
            continue;
        }
        let (Some(program), name) = (source.program.as_deref(), names::folded(&source.name)) else {
            continue;
        };
        if name.is_empty() || program.trim().is_empty() {
            continue;
        }
        let groups = shared.entry((program_target(program), name)).or_default();
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    let mut shared: Vec<_> = shared.into_iter().collect();
    shared.sort();

    // Union-find over the groups, each set holding at most one
    // AppUserModelID.
    let mut parent: Vec<usize> = (0..keys.len()).collect();
    let mut holds_app: Vec<bool> = (0..keys.len()).map(is_app).collect();
    fn root(parent: &mut [usize], mut group: usize) -> usize {
        while parent[group] != group {
            parent[group] = parent[parent[group]];
            group = parent[group];
        }
        group
    }
    for (_, groups) in shared {
        // Several AppUserModelIDs under one program and name: which of
        // them a plain shortcut is cannot be told, so only plain ones merge.
        let apps = groups.iter().filter(|&&group| is_app(group)).count();
        let merging: Vec<usize> = groups
            .into_iter()
            .filter(|&group| apps <= 1 || !is_app(group))
            .collect();
        let Some((&first, rest)) = merging.split_first() else {
            continue;
        };
        for &other in rest {
            let (a, b) = (root(&mut parent, first), root(&mut parent, other));
            if a == b || (holds_app[a] && holds_app[b]) {
                continue;
            }
            let (kept, merged) = (a.min(b), a.max(b));
            parent[merged] = kept;
            holds_app[kept] |= holds_app[merged];
        }
    }

    // Each set's owner: its AppUserModelID, else the fewest arguments.
    let rank = |group: usize| match &keys[group] {
        Key::Program { arguments, .. } => (1, arguments.len(), group),
        _ => (0, 0, group),
    };
    let mut owner: HashMap<usize, usize> = HashMap::new();
    for group in 0..keys.len() {
        let set = root(&mut parent, group);
        let best = owner.entry(set).or_insert(group);
        if rank(group) < rank(*best) {
            *best = group;
        }
    }
    (0..keys.len())
        .map(|group| owner[&root(&mut parent, group)])
        .collect()
}

/// One application of a [`Catalog`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identified {
    /// Its id, [`Key::id`] of its sources' key.
    pub id: String,
    /// Every source it was found by, in the order they were found.
    pub sources: Vec<Source>,
    /// The index of its primary source in `sources` ([`primary`]).
    pub primary: usize,
    /// The other names that find it ([`super::names::alternate_titles`]).
    pub alternate_titles: Vec<String>,
    /// The words that find it besides its names
    /// ([`super::names::keywords`]).
    pub keywords: Vec<String>,
    /// What tells it apart from the other applications of its title, when
    /// there are any ([`super::names::distinctions`]).
    pub distinction: Option<String>,
}

impl Identified {
    /// Its primary source: the one Pane opens and names it by.
    pub fn primary(&self) -> &Source {
        &self.sources[self.primary]
    }

    /// The record an extension receives for it.
    pub fn application(&self) -> Application {
        let primary = self.primary();
        Application {
            id: self.id.clone(),
            name: primary.name.clone(),
            location: primary.location.clone(),
            alternate_titles: self.alternate_titles.clone(),
            keywords: self.keywords.clone(),
            distinction: self.distinction.clone(),
        }
    }
}

/// The installed applications by identity, from the sources one scan found:
/// sources with one key are one application, and every id, current or from
/// before identities, finds the application it names.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    /// The applications, in the order their first source was found.
    applications: Vec<Identified>,
    /// Each application's index by its id.
    by_id: HashMap<String, usize>,
    /// Each application's index by the ids it had before (#508): its
    /// sources' [`Source::previous`] keys' and those of the groups merged
    /// into it ([`owners`]).
    by_previous_id: HashMap<String, usize>,
    /// Each application's index by the path of each of its sources, which
    /// was an id before identities: as found, and in lowercase.
    by_path: HashMap<String, usize>,
    by_lowercase_path: HashMap<String, usize>,
}

impl Catalog {
    /// The applications `sources` are, grouped by key and merged where
    /// shortcuts run one program under one name ([`owners`]), each with its
    /// alternate titles, keywords and distinction ([`super::names`]).
    pub fn new(sources: Vec<Source>) -> Catalog {
        let mut keys: Vec<Key> = Vec::new();
        let mut by_key: HashMap<Key, usize> = HashMap::new();
        let group_of: Vec<usize> = sources
            .iter()
            .map(|source| {
                *by_key.entry(source.key.clone()).or_insert_with(|| {
                    keys.push(source.key.clone());
                    keys.len() - 1
                })
            })
            .collect();
        let owners = owners(&keys, &sources, &group_of);
        let ids: Vec<String> = keys.iter().map(Key::id).collect();

        let mut catalog = Catalog::default();
        let mut by_owner: HashMap<usize, usize> = HashMap::new();
        let mut previous: Vec<(String, usize)> = Vec::new();
        for (source, group) in sources.into_iter().zip(group_of) {
            let owner = owners[group];
            let index = *by_owner.entry(owner).or_insert_with(|| {
                catalog.applications.push(Identified {
                    id: ids[owner].clone(),
                    sources: Vec::new(),
                    primary: 0,
                    alternate_titles: Vec::new(),
                    keywords: Vec::new(),
                    distinction: None,
                });
                catalog.applications.len() - 1
            });
            if group != owner {
                previous.push((ids[group].clone(), index));
            }
            if let Some(key) = &source.previous {
                previous.push((key.id(), index));
            }
            catalog.by_path.entry(source.path.clone()).or_insert(index);
            catalog
                .by_lowercase_path
                .entry(source.path.to_lowercase())
                .or_insert(index);
            catalog.applications[index].sources.push(source);
        }
        for (index, application) in catalog.applications.iter_mut().enumerate() {
            application.primary = primary(&application.sources).unwrap_or(0);
            catalog.by_id.insert(application.id.clone(), index);
        }
        for (id, index) in previous {
            catalog.by_previous_id.entry(id).or_insert(index);
        }
        let alternate_titles = names::alternate_titles(&catalog.applications);
        let distinctions = names::distinctions(&catalog.applications);
        for ((application, alternates), distinction) in catalog
            .applications
            .iter_mut()
            .zip(alternate_titles)
            .zip(distinctions)
        {
            application.alternate_titles = alternates;
            application.keywords = names::keywords(application);
            application.distinction = distinction;
        }
        catalog
    }

    /// Every application, in the order their first source was found.
    pub fn identified(&self) -> &[Identified] {
        &self.applications
    }

    /// The records extensions receive, one per application.
    pub fn applications(&self) -> Vec<Application> {
        self.applications
            .iter()
            .map(Identified::application)
            .collect()
    }

    /// The application `id` names: by its id; by an id it had before
    /// shortcuts were keyed by their AppUserModelID and merged by program
    /// and name (#508), unless another application has that id now; or by
    /// the path of one of its sources, which was its id before identities
    /// (a Start menu shortcut's, a bundle's or a desktop entry's path, or
    /// `shell:AppsFolder\<AppUserModelID>`), exactly or else ignoring case.
    pub fn find(&self, id: &str) -> Option<&Identified> {
        let index = self
            .by_id
            .get(id)
            .or_else(|| self.by_previous_id.get(id))
            .or_else(|| self.by_path.get(id))
            .or_else(|| self.by_lowercase_path.get(&id.to_lowercase()))?;
        self.applications.get(*index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(key: Key, path: &str, place: usize) -> Source {
        let name = path.rsplit(['\\', '/']).next().unwrap();
        Source::new(key, path, name, "here", place)
    }

    #[test]
    fn version_folders_are_wildcarded_with_their_prefix() {
        assert_eq!(version_wildcard("app-1.0.9003").as_deref(), Some("app-*"));
        assert_eq!(version_wildcard("1.2.3.4").as_deref(), Some("*"));
        assert_eq!(version_wildcard("1.2").as_deref(), Some("*"));
        assert_eq!(version_wildcard("v2.0").as_deref(), Some("v*"));
        assert_eq!(version_wildcard("app_4.47.69").as_deref(), Some("app_*"));
        assert_eq!(
            version_wildcard("pycharm 2024.1").as_deref(),
            Some("pycharm *")
        );
    }

    #[test]
    fn what_is_not_a_version_folder_is_kept() {
        for segment in [
            "discord",
            "program files",
            "x64",
            "1",
            "1.2.3.4.5",
            "1..2",
            "1.2.",
            ".1.2",
            "1.2-beta",
            "a-very-long-prefix-1.2",
            "longprefix1.2",
            "app--1.2",
            "app-1.2.exe",
            "",
        ] {
            assert_eq!(version_wildcard(segment), None, "{segment:?}");
        }
    }

    #[test]
    fn a_target_is_keyed_in_lowercase_with_its_version_folders_wildcarded() {
        assert_eq!(
            program_target(r"C:\Users\Ann\AppData\Local\Discord\app-1.0.9003\Discord.exe"),
            r"c:\users\ann\appdata\local\discord\app-*\discord.exe"
        );
        assert_eq!(
            program_target("C:/Program Files/Tool/1.2.3.4/tool.exe"),
            r"c:\program files\tool\*\tool.exe"
        );
    }

    #[test]
    fn a_program_updated_into_a_new_version_folder_keeps_its_id() {
        let before = Key::program(r"C:\Discord\app-1.0.9003\Discord.exe", "");
        let after = Key::program(r"C:\DISCORD\app-1.0.9004\discord.exe", " ");
        assert_eq!(before, after);
        assert_eq!(before.id(), after.id());
    }

    #[test]
    fn arguments_keep_two_programs_apart() {
        let chrome = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
        let browser = Key::program(chrome, "");
        let mail = Key::program(chrome, "--profile-directory=Default --app-id=abc");
        let docs = Key::program(chrome, "--profile-directory=Default --app-id=def");
        assert_ne!(browser.id(), mail.id());
        assert_ne!(mail.id(), docs.id());
    }

    #[test]
    fn ids_are_opaque_digests_of_the_whole_typed_key() {
        let id = Key::DesktopFile("firefox.desktop".into()).id();
        assert!(is_identity(&id), "{id}");
        // The same text under another kind, or split differently, is
        // another application.
        assert_ne!(id, Key::Bundle("firefox.desktop".into()).id());
        assert_ne!(
            Key::Link("steam://rungameid/570".into()).id(),
            Key::Path("steam://rungameid/570".into()).id()
        );
        assert_ne!(Key::program("ab", "c").id(), Key::program("a", "bc").id());
        assert_ne!(
            Key::App("vendor.tool".into()).id(),
            Key::Link("vendor.tool".into()).id()
        );
        assert_ne!(
            Key::Package {
                family: "a".into(),
                app: None
            }
            .id(),
            Key::Package {
                family: "a".into(),
                app: Some(String::new())
            }
            .id()
        );
        // The same key always digests alike, on every start.
        assert_eq!(id, Key::DesktopFile("firefox.desktop".into()).id());
        assert!(!is_identity(r"C:\Menu\Firefox.lnk"));
        assert!(!is_identity("/usr/share/applications/firefox.desktop"));
    }

    #[test]
    fn a_packages_first_app_is_its_family_and_others_add_their_app_user_model_id() {
        let aumids: Vec<String> = [
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            "Contoso.Suite_abc!Writer",
            "Contoso.Suite_abc!Calc",
        ]
        .map(String::from)
        .to_vec();

        let keys = packaged_keys(&aumids);

        assert_eq!(
            keys,
            [
                Key::Package {
                    family: "microsoft.windowsterminal_8wekyb3d8bbwe".into(),
                    app: None
                },
                Key::Package {
                    family: "contoso.suite_abc".into(),
                    app: Some("contoso.suite_abc!writer".into())
                },
                // First by app id, whatever order the shell lists them in.
                Key::Package {
                    family: "contoso.suite_abc".into(),
                    app: None
                },
            ]
        );
    }

    #[test]
    fn the_primary_source_is_the_most_preferred_place_then_the_shorter_path() {
        let key = Key::program(r"C:\Tool\tool.exe", "");
        let sources = [
            source(
                key.clone(),
                r"C:\ProgramData\Start Menu\Programs\Tool.lnk",
                3,
            ),
            source(
                key.clone(),
                r"C:\Users\Ann\Start Menu\Programs\Tool\Tool.lnk",
                2,
            ),
            source(key.clone(), r"C:\Users\Ann\Start Menu\Programs\Tool.lnk", 2),
            source(key.clone(), r"C:\Users\Public\Desktop\Tool.lnk", 1),
        ];
        assert_eq!(primary(&sources), Some(3));
        assert_eq!(primary(&sources[..3]), Some(2));
        assert_eq!(primary(&sources[..2]), Some(1));
        assert_eq!(primary(&[]), None);
        // The order they were found in does not matter.
        let mut reversed = sources.to_vec();
        reversed.reverse();
        assert_eq!(reversed[primary(&reversed).unwrap()], sources[3]);
    }

    #[test]
    fn sources_with_one_key_are_one_application_named_and_opened_by_its_primary() {
        let tool = Key::program(r"C:\Tool\app-1.2\tool.exe", "");
        let updated = Key::program(r"C:\Tool\app-1.3\tool.exe", "");
        let other = Key::program(r"C:\Other\other.exe", "");
        let catalog = Catalog::new(vec![
            source(tool.clone(), r"C:\Start\Tool.lnk", 2),
            source(other.clone(), r"C:\Start\Other.lnk", 2),
            source(updated, r"C:\Desktop\My Tool.lnk", 0),
        ]);

        let applications = catalog.applications();
        let names: Vec<&str> = applications.iter().map(|app| app.name.as_str()).collect();
        assert_eq!(names, ["My Tool.lnk", "Other.lnk"]);
        assert_eq!(applications[0].id, tool.id());
        assert_eq!(applications[1].id, other.id());
        let found = catalog.find(&tool.id()).unwrap();
        assert_eq!(found.primary().path, r"C:\Desktop\My Tool.lnk");
        assert_eq!(found.sources.len(), 2);
    }

    #[test]
    fn an_id_from_before_identities_finds_the_application_its_source_belongs_to() {
        let tool = Key::program(r"C:\Tool\tool.exe", "");
        let catalog = Catalog::new(vec![
            source(tool.clone(), r"C:\Start\Tool.lnk", 2),
            source(tool.clone(), r"C:\Desktop\Tool.lnk", 0),
            source(
                Key::Package {
                    family: "calc_8wekyb3d8bbwe".into(),
                    app: None,
                },
                r"shell:AppsFolder\Calc_8wekyb3d8bbwe!App",
                5,
            ),
        ]);

        // A secondary source's path names the application, whose primary
        // source opens it.
        let found = catalog.find(r"C:\Start\Tool.lnk").unwrap();
        assert_eq!(found.id, tool.id());
        assert_eq!(found.primary().path, r"C:\Desktop\Tool.lnk");
        // Windows paths were never compared by case.
        assert_eq!(catalog.find(r"c:\start\tool.lnk").unwrap().id, tool.id());
        assert!(
            catalog
                .find(r"shell:AppsFolder\Calc_8wekyb3d8bbwe!App")
                .is_some()
        );
        assert!(catalog.find(r"C:\Start\Gone.lnk").is_none());
        assert!(catalog.find(&Key::Path("gone".into()).id()).is_none());
    }

    /// A shortcut named `name` at `path` running `program`, keyed `key`.
    fn shortcut(key: Key, path: &str, name: &str, program: &str, arguments: bool) -> Source {
        Source {
            program: Some(program.into()),
            arguments,
            ..Source::new(key, path, name, "here", 2)
        }
    }

    #[test]
    fn the_primary_source_passes_no_arguments_when_its_place_is_shared() {
        let key = Key::App("vendor.tool".into());
        let sources = [
            shortcut(key.clone(), r"C:\Start\Tool.lnk", "Tool", "tool.exe", true),
            shortcut(
                key.clone(),
                r"C:\Start\Vendor\Tool.lnk",
                "Tool",
                "tool.exe",
                false,
            ),
        ];
        assert_eq!(primary(&sources), Some(1));
    }

    #[test]
    fn one_program_under_one_name_is_one_application_whatever_its_arguments() {
        let tool = r"C:\Tool\app-1.2\tool.exe";
        let plain = Key::program(tool, "");
        let started = Key::program(tool, "--start");
        let catalog = Catalog::new(vec![
            shortcut(started.clone(), r"C:\Desktop\Tool.lnk", "Tool", tool, true),
            shortcut(plain.clone(), r"C:\Start\Tool.lnk", "tool", tool, false),
            // The same program under another name is another application:
            // a web app the shortcut's arguments name.
            shortcut(
                Key::program(tool, "--app=mail"),
                r"C:\Start\Mail.lnk",
                "Mail",
                tool,
                true,
            ),
        ]);

        let applications = catalog.applications();
        assert_eq!(applications.len(), 2);
        // Named by the shortcut passing the fewest arguments, wherever it
        // was found; the id it had alone still finds it.
        assert_eq!(applications[0].id, plain.id());
        assert_eq!(catalog.find(&started.id()).unwrap().id, plain.id());
        assert_eq!(catalog.identified()[0].sources.len(), 2);
        assert_eq!(applications[1].name, "Mail");
    }

    #[test]
    fn a_plain_shortcut_joins_the_one_app_user_model_id_running_its_program_under_its_name() {
        let tool = r"C:\Tool\tool.exe";
        let app = Key::App("vendor.tool".into());
        let plain = Key::program(tool, "");
        let catalog = Catalog::new(vec![
            shortcut(plain.clone(), r"C:\Desktop\Tool.lnk", "Tool", tool, false),
            shortcut(app.clone(), r"C:\Start\Tool.lnk", "Tool", tool, false),
        ]);
        assert_eq!(catalog.applications().len(), 1);
        assert_eq!(catalog.applications()[0].id, app.id());
        assert_eq!(catalog.find(&plain.id()).unwrap().id, app.id());
    }

    #[test]
    fn two_app_user_model_ids_are_never_merged() {
        let browser = r"C:\Browser\browser.exe";
        let first = Key::App("browser.profile1".into());
        let second = Key::App("browser.profile2".into());
        let plain = Key::program(browser, "");
        let catalog = Catalog::new(vec![
            shortcut(
                first.clone(),
                r"C:\Start\A\Browser.lnk",
                "Browser",
                browser,
                true,
            ),
            shortcut(
                second.clone(),
                r"C:\Start\B\Browser.lnk",
                "Browser",
                browser,
                true,
            ),
            // Which of the two a plain shortcut of their name is cannot be
            // told: it stays its own.
            shortcut(
                plain.clone(),
                r"C:\Desktop\Browser.lnk",
                "Browser",
                browser,
                false,
            ),
        ]);
        let ids: Vec<String> = catalog
            .applications()
            .into_iter()
            .map(|app| app.id)
            .collect();
        assert_eq!(ids, [first.id(), second.id(), plain.id()]);
    }

    #[test]
    fn a_previous_key_finds_its_application_unless_another_has_that_id_now() {
        let tool = r"C:\Tool\tool.exe";
        let old = Key::program(tool, "");
        let catalog = Catalog::new(vec![Source {
            previous: Some(old.clone()),
            ..shortcut(
                Key::App("vendor.tool".into()),
                r"C:\Start\Tool.lnk",
                "Tool",
                tool,
                false,
            )
        }]);
        assert_eq!(
            catalog.find(&old.id()).unwrap().id,
            Key::App("vendor.tool".into()).id()
        );

        // A shortcut keyed by that id now keeps it, under another name.
        let catalog = Catalog::new(vec![
            Source {
                previous: Some(old.clone()),
                ..shortcut(
                    Key::App("vendor.tool".into()),
                    r"C:\Start\Tool.lnk",
                    "Tool",
                    tool,
                    false,
                )
            },
            shortcut(old.clone(), r"C:\Start\Other.lnk", "Other", tool, false),
        ]);
        assert_eq!(catalog.find(&old.id()).unwrap().primary().name, "Other");
    }

    #[test]
    fn two_different_programs_never_share_an_identity() {
        let catalog = Catalog::new(vec![
            source(Key::program(r"C:\A\a.exe", ""), r"C:\Start\Same.lnk", 2),
            source(Key::program(r"C:\B\a.exe", ""), r"C:\Start\Sub\Same.lnk", 2),
        ]);
        let applications = catalog.applications();
        assert_eq!(applications.len(), 2);
        assert_ne!(applications[0].id, applications[1].id);
    }
}
