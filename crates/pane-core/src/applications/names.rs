//! What an application is called and what tells it apart: the pure rules
//! behind its alternate titles, its keywords and the subtitle that tells
//! applications of one name apart (#124, "Names and alternate titles" and
//! "Same-name subtitles"), compiled and tested on every system.
//!
//! - An application's **title** is its primary source's name, as the
//!   system shows it (localized). Every other name it was found by (the
//!   untranslated name, a second source's name) is an **alternate title**,
//!   which root search matches as it matches a title.
//! - The **program's name** (`code` for `Code.exe`, `gnome-terminal` for a
//!   desktop entry running it) is an alternate title too, unless the source
//!   passes the program arguments that name something (a browser's web app
//!   runs the browser), the name is a generic one ([`is_generic`]), or
//!   another application has the same program name: then it would pick one
//!   of them arbitrarily.
//! - Applications sharing a title each get a **distinction**
//!   ([`distinctions`]): the first of their program's name, their parent
//!   folder's name and their full path that is unique among them.
//! - On Linux, `Name`, `Keywords` and the like are chosen for the user's
//!   messages locale by the Desktop Entry specification's matching rules
//!   ([`locale_keys`], [`localized`]).

use std::collections::HashMap;

use super::identity::{Identified, Source};

/// Program names that say what role a program plays rather than which
/// program it is: typing one would list every application whose file is
/// called that, so none is an alternate title. Compared in lowercase, after
/// an architecture suffix is dropped ([`is_generic`]).
pub const GENERIC_PROGRAM_NAMES: &[&str] = &[
    "app",
    "application",
    "bootstrap",
    "bootstrapper",
    "client",
    "config",
    "console",
    "env",
    "game",
    "helper",
    "host",
    "install",
    "installer",
    "launch",
    "launcher",
    "loader",
    "main",
    "program",
    "run",
    "server",
    "service",
    "settings",
    "setup",
    "shell",
    "start",
    "stub",
    "tool",
    "uninstall",
    "uninstaller",
    "update",
    "updater",
    "wrapper",
];

/// The architecture suffixes a program's name may end with
/// (`launcher64`, `setup_x64`), dropped before comparing it with
/// [`GENERIC_PROGRAM_NAMES`].
const ARCHITECTURE_SUFFIXES: &[&str] = &["x86_64", "amd64", "arm64", "x64", "x86", "64", "32"];

/// Whether `program`, a program's name without its extension, is a
/// generic one ([`GENERIC_PROGRAM_NAMES`]), ignoring case and an
/// architecture suffix (`Launcher64`, `setup-x64`).
pub fn is_generic(program: &str) -> bool {
    let lowered = program.trim().to_lowercase();
    let mut name = lowered.as_str();
    for suffix in ARCHITECTURE_SUFFIXES {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.trim_end_matches(['-', '_', '.', ' ']);
            break;
        }
    }
    GENERIC_PROGRAM_NAMES.contains(&name)
}

/// The last segment of `path`, split at `\` or `/`.
fn file_name(path: &str) -> &str {
    path.trim()
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
}

/// The extensions that make a file a program, dropped from its name
/// ([`program_name`]).
const PROGRAM_EXTENSIONS: &[&str] = &["exe", "bat", "cmd", "com", "appimage"];

/// The name of the program at `program`, a path or a command: its file
/// name, without its extension when that is a program's ([`PROGRAM_EXTENSIONS`],
/// so Windows' `Code.exe` is `Code`, while a Linux program such as
/// `python3.12` keeps its dots). `None` when nothing is left.
pub fn program_name(program: &str) -> Option<String> {
    let name = file_name(program);
    let name = match name.rsplit_once('.') {
        Some((stem, extension))
            if !stem.is_empty()
                && PROGRAM_EXTENSIONS
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(extension)) =>
        {
            stem
        }
        _ => name,
    };
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Whether `arguments`, those a desktop entry's `Exec` passes its program
/// (field codes already dropped), name something, such as a web app
/// (`--app-id=…`), a profile or a file, so that the program's name says
/// nothing about this entry. Options that only switch a mode
/// (`--new-window`, `-n`) do not: an argument names something when it does
/// not start with `-` or carries a value (`=`).
pub fn arguments_name_something(arguments: &[String]) -> bool {
    arguments
        .iter()
        .any(|argument| !argument.starts_with('-') || argument.contains('='))
}

/// Text compared without case: two names are the same name when they
/// match in lowercase.
fn folded(text: &str) -> String {
    text.trim().to_lowercase()
}

/// The program name `source` claims as an alternate title: its program's
/// name, when it passes it no arguments and the name is not generic.
fn claimed_program(source: &Source) -> Option<String> {
    if source.arguments {
        return None;
    }
    let name = program_name(source.program.as_deref()?)?;
    (!is_generic(&name)).then_some(name)
}

/// The alternate titles of each of `applications`, in the same order:
/// every name one of its sources has, untranslated or not, that differs
/// from its title, then its program's name when only it claims that name
/// ([`claimed_program`]). Each once, compared without case, in the order
/// its sources were found.
pub fn alternate_titles(applications: &[Identified]) -> Vec<Vec<String>> {
    let claims: Vec<Vec<String>> = applications
        .iter()
        .map(|application| {
            let mut names: Vec<String> = Vec::new();
            for name in application.sources.iter().filter_map(claimed_program) {
                if !names.iter().any(|kept| folded(kept) == folded(&name)) {
                    names.push(name);
                }
            }
            names
        })
        .collect();
    let mut claimants: HashMap<String, usize> = HashMap::new();
    for names in &claims {
        for name in names {
            *claimants.entry(folded(name)).or_default() += 1;
        }
    }
    applications
        .iter()
        .zip(claims)
        .map(|(application, programs)| {
            let title = folded(&application.primary().name);
            let mut alternates: Vec<String> = Vec::new();
            let names = application
                .sources
                .iter()
                .flat_map(|source| std::iter::once(&source.name).chain(&source.untranslated))
                .cloned()
                .chain(
                    programs
                        .into_iter()
                        .filter(|name| claimants.get(&folded(name)) == Some(&1)),
                );
            for name in names {
                let key = folded(&name);
                if !key.is_empty()
                    && key != title
                    && !alternates.iter().any(|kept| folded(kept) == key)
                {
                    alternates.push(name.trim().to_owned());
                }
            }
            alternates
        })
        .collect()
}

/// The keywords of `application`: those of all its sources, each once
/// (compared without case), in the order they were found.
pub fn keywords(application: &Identified) -> Vec<String> {
    let mut keywords: Vec<String> = Vec::new();
    for keyword in application
        .sources
        .iter()
        .flat_map(|source| &source.keywords)
    {
        let keyword = keyword.trim();
        if !keyword.is_empty() && !keywords.iter().any(|kept| folded(kept) == folded(keyword)) {
            keywords.push(keyword.to_owned());
        }
    }
    keywords
}

/// The parent folder of `path` (split at `\` or `/`), by name; `None` at a
/// root or for a bare name.
fn parent_folder(path: &str) -> Option<String> {
    let path = path.trim().trim_end_matches(['\\', '/']);
    let (parent, _) = path.rsplit_once(['\\', '/'])?;
    let name = file_name(parent);
    (!name.is_empty() && !name.ends_with(':')).then(|| name.to_owned())
}

/// What could tell `application` apart from another of its title, most
/// readable first: its program's name, its parent folder's name, its full
/// path, then its primary source's own path (two shortcuts to one program
/// with different arguments differ only there).
fn candidates(application: &Identified) -> [Option<String>; 4] {
    let primary = application.primary();
    let program = primary
        .program
        .as_deref()
        .filter(|program| !program.trim().is_empty());
    // A program given as a bare command (`gnome-terminal`) has no folder:
    // the source's own path places it then.
    let placed = program
        .filter(|program| program.contains(['\\', '/']))
        .unwrap_or(&primary.path);
    [
        program.and_then(program_name),
        parent_folder(placed),
        Some(placed.trim().to_owned()),
        Some(primary.path.clone()),
    ]
}

/// The distinction of each of `applications`, in the same order: `None`
/// for an application alone with its title (compared without case), and
/// for each of two or more sharing a title the first of its
/// [`candidates`] that no other of them has (compared without case), so
/// it is the shortest thing that tells it apart.
pub fn distinctions(applications: &[Identified]) -> Vec<Option<String>> {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, application) in applications.iter().enumerate() {
        groups
            .entry(folded(&application.primary().name))
            .or_default()
            .push(index);
    }
    let candidates: Vec<[Option<String>; 4]> = applications.iter().map(candidates).collect();
    let mut distinctions = vec![None; applications.len()];
    for members in groups.values().filter(|members| members.len() > 1) {
        for &index in members {
            let unique = |rank: usize, value: &String| {
                members
                    .iter()
                    .filter(|&&other| other != index)
                    .all(|&other| {
                        candidates[other][rank]
                            .as_ref()
                            .is_none_or(|theirs| folded(theirs) != folded(value))
                    })
            };
            distinctions[index] = candidates[index]
                .iter()
                .enumerate()
                .find_map(|(rank, value)| value.as_ref().filter(|value| unique(rank, value)))
                .or(candidates[index][3].as_ref())
                .cloned();
        }
    }
    distinctions
}

/// The keys a localized value is looked up by for `locale`, a POSIX
/// locale (`lang_COUNTRY.ENCODING@MODIFIER`), best first, as the Desktop
/// Entry specification orders them: `lang_COUNTRY@MODIFIER`,
/// `lang_COUNTRY`, `lang@MODIFIER`, `lang`, each only when the locale has
/// its parts. The encoding is ignored. None for the `C` and `POSIX`
/// locales or an empty one, which use the unlocalized value.
pub fn locale_keys(locale: &str) -> Vec<String> {
    let locale = locale.trim();
    let (locale, modifier) = match locale.split_once('@') {
        Some((locale, modifier)) => (locale, Some(modifier).filter(|m| !m.is_empty())),
        None => (locale, None),
    };
    let locale = locale.split_once('.').map_or(locale, |(locale, _)| locale);
    let (lang, country) = match locale.split_once('_') {
        Some((lang, country)) => (lang, Some(country).filter(|c| !c.is_empty())),
        None => (locale, None),
    };
    if lang.is_empty() || lang == "C" || lang == "POSIX" {
        return Vec::new();
    }
    let mut keys = Vec::new();
    if let (Some(country), Some(modifier)) = (country, modifier) {
        keys.push(format!("{lang}_{country}@{modifier}"));
    }
    if let Some(country) = country {
        keys.push(format!("{lang}_{country}"));
    }
    if let Some(modifier) = modifier {
        keys.push(format!("{lang}@{modifier}"));
    }
    keys.push(lang.to_owned());
    keys
}

/// The value of a localized key for the locale whose [`locale_keys`] are
/// `keys`: among `values`, each `(locale, value)` as a desktop entry gives
/// them (`None` for the plain key), the one for the best key that has one,
/// else the plain one.
pub fn localized<'a>(values: &'a [(Option<String>, String)], keys: &[String]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| {
            values
                .iter()
                .find(|(locale, _)| locale.as_deref() == Some(key.as_str()))
        })
        .or_else(|| values.iter().find(|(locale, _)| locale.is_none()))
        .map(|(_, value)| value.as_str())
}

/// The user's messages locale, as POSIX defines it: `LC_ALL`, else
/// `LC_MESSAGES`, else `LANG`, the first set and not empty.
pub fn messages_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::identity::{Catalog, Key};
    use super::*;

    /// A shortcut named `name` at `path` to `program` (`None`: none),
    /// passing arguments or not.
    fn source(name: &str, path: &str, program: Option<&str>, arguments: bool) -> Source {
        let key = match program {
            Some(program) => Key::program(program, if arguments { "--app" } else { "" }),
            None => Key::Path(path.to_lowercase()),
        };
        Source {
            program: program.map(str::to_owned),
            arguments,
            ..Source::new(key, path, name, "here", 2)
        }
    }

    fn alternates_of(sources: Vec<Source>) -> Vec<Vec<String>> {
        let catalog = Catalog::new(sources);
        alternate_titles(catalog.identified())
    }

    #[test]
    fn role_names_are_generic_whatever_their_case_or_architecture() {
        for name in [
            "setup",
            "Setup",
            "launcher",
            "Launcher64",
            "update",
            "Updater",
            "setup_x64",
            "installer-x86",
            "client",
            "app",
            " tool ",
        ] {
            assert!(is_generic(name), "{name}");
        }
        for name in [
            "code",
            "wt",
            "devenv",
            "firefox",
            "steam",
            "launchpad",
            "x64",
        ] {
            assert!(!is_generic(name), "{name}");
        }
    }

    #[test]
    fn a_program_s_name_is_its_file_name_without_a_program_s_extension() {
        assert_eq!(
            program_name(r"C:\Program Files\Microsoft VS Code\Code.exe").as_deref(),
            Some("Code")
        );
        assert_eq!(
            program_name(r"C:\Tools\Code - Insiders.EXE").as_deref(),
            Some("Code - Insiders")
        );
        assert_eq!(program_name("wt.exe").as_deref(), Some("wt"));
        assert_eq!(
            program_name(r"C:\Tools\build.cmd").as_deref(),
            Some("build")
        );
        // Other dots are part of the name, as in Linux programs.
        assert_eq!(
            program_name("/usr/bin/python3.12").as_deref(),
            Some("python3.12")
        );
        assert_eq!(
            program_name("gnome-terminal").as_deref(),
            Some("gnome-terminal")
        );
        assert_eq!(program_name(""), None);
        assert_eq!(program_name(".exe").as_deref(), Some(".exe"));
        assert_eq!(program_name(r"C:\Folder\").as_deref(), Some("Folder"));
    }

    #[test]
    fn only_arguments_that_name_something_count() {
        let arguments = |words: &[&str]| -> Vec<String> {
            words.iter().map(|word| (*word).to_owned()).collect()
        };
        assert!(!arguments_name_something(&arguments(&[])));
        assert!(!arguments_name_something(&arguments(&["--window", "-n"])));
        assert!(arguments_name_something(&arguments(&[
            "--profile-directory=Default",
            "--app-id=abc"
        ])));
        assert!(arguments_name_something(&arguments(&[
            "run",
            "org.gnome.Foo"
        ])));
    }

    #[test]
    fn a_program_s_name_is_an_alternate_title_unless_generic_shared_or_given_arguments() {
        let alternates = alternates_of(vec![
            source(
                "Visual Studio Code",
                r"C:\Menu\Visual Studio Code.lnk",
                Some(r"C:\VS Code\Code.exe"),
                false,
            ),
            source(
                "Windows Terminal",
                r"C:\Menu\Windows Terminal.lnk",
                Some(r"C:\Terminal\wt.exe"),
                false,
            ),
            // Generic.
            source(
                "Game One",
                r"C:\Menu\Game One.lnk",
                Some(r"C:\Games\One\launcher.exe"),
                false,
            ),
            // Shared by two applications: neither has it.
            source(
                "Editor Stable",
                r"C:\Menu\Editor Stable.lnk",
                Some(r"C:\Editor\editor.exe"),
                false,
            ),
            source(
                "Editor Beta",
                r"C:\Menu\Editor Beta.lnk",
                Some(r"C:\Editor Beta\editor.exe"),
                false,
            ),
            // A browser and its web app: only the browser has its name.
            source(
                "Browser",
                r"C:\Menu\Browser.lnk",
                Some(r"C:\Browser\chrome.exe"),
                false,
            ),
            source(
                "Mail",
                r"C:\Menu\Mail.lnk",
                Some(r"C:\Browser\chrome.exe"),
                true,
            ),
            // The program's name is the title: nothing to add.
            source(
                "firefox",
                r"C:\Menu\firefox.lnk",
                Some(r"C:\Firefox\Firefox.exe"),
                false,
            ),
        ]);

        let expected: [&[&str]; 8] = [&["Code"], &["wt"], &[], &[], &[], &["chrome"], &[], &[]];
        assert_eq!(alternates, expected);
    }

    #[test]
    fn every_other_name_an_application_was_found_by_is_an_alternate_title() {
        let program = r"C:\Paint\mspaint.exe";
        let mut localized = source("Ứng dụng Vẽ", r"C:\Menu\Paint.lnk", Some(program), false);
        localized.untranslated = Some("Paint".into());
        let mut desktop = source("My Paint", r"C:\Desktop\My Paint.lnk", Some(program), false);
        desktop.place = 3;
        // The same name again, in another case: once.
        let mut again = source("PAINT", r"C:\Other\PAINT.lnk", Some(program), false);
        again.place = 4;

        let alternates = alternates_of(vec![localized, desktop, again]);

        assert_eq!(alternates, [["Paint", "My Paint", "mspaint"]]);
    }

    #[test]
    fn keywords_are_every_source_s_once() {
        let mut first = source("Web", "/a/web.desktop", None, false);
        first.keywords = vec!["browser".into(), "internet".into()];
        let mut second = first.clone();
        second.path = "/b/web.desktop".into();
        second.keywords = vec!["Browser".into(), "www".into(), " ".into()];
        let catalog = Catalog::new(vec![first, second]);

        assert_eq!(
            keywords(&catalog.identified()[0]),
            ["browser", "internet", "www"]
        );
    }

    fn distinctions_of(sources: Vec<Source>) -> Vec<Option<String>> {
        let catalog = Catalog::new(sources);
        distinctions(catalog.identified())
    }

    #[test]
    fn an_application_alone_with_its_title_has_no_distinction() {
        assert_eq!(
            distinctions_of(vec![
                source(
                    "Python",
                    r"C:\Menu\Python.lnk",
                    Some(r"C:\Py\python.exe"),
                    false
                ),
                source(
                    "Code",
                    r"C:\Menu\Code.lnk",
                    Some(r"C:\Code\code.exe"),
                    false
                ),
            ]),
            [None, None]
        );
    }

    #[test]
    fn same_name_applications_are_told_apart_by_the_first_unique_of_program_folder_and_path() {
        let distinctions = distinctions_of(vec![
            // Program names differ.
            source(
                "Editor",
                r"C:\Menu\Editor.lnk",
                Some(r"C:\Editor\Editor.exe"),
                false,
            ),
            source(
                "editor",
                r"C:\Menu\Preview\Editor.lnk",
                Some(r"C:\Editor Preview\Editor-Preview.exe"),
                false,
            ),
            // One program name, two folders.
            source(
                "Python",
                r"C:\Menu\3.11\Python.lnk",
                Some(r"C:\Python311\python.exe"),
                false,
            ),
            source(
                "Python",
                r"C:\Menu\3.12\Python.lnk",
                Some(r"C:\Python312\python.exe"),
                false,
            ),
            // One program name and one folder name: the full path.
            source(
                "Tool",
                r"C:\Menu\A\Tool.lnk",
                Some(r"C:\A\bin\tool.exe"),
                false,
            ),
            source(
                "Tool",
                r"C:\Menu\B\Tool.lnk",
                Some(r"D:\B\bin\tool.exe"),
                false,
            ),
        ]);

        assert_eq!(
            distinctions,
            [
                Some("Editor".into()),
                Some("Editor-Preview".into()),
                Some("Python311".into()),
                Some("Python312".into()),
                Some(r"C:\A\bin\tool.exe".into()),
                Some(r"D:\B\bin\tool.exe".into()),
            ]
        );
    }

    #[test]
    fn each_takes_the_first_thing_unique_to_it_within_its_group() {
        // Three Pythons: one has a program name of its own; the other two
        // share theirs and differ by folder.
        let distinctions = distinctions_of(vec![
            source(
                "Python",
                r"C:\Menu\1\Python.lnk",
                Some(r"C:\Python311\python.exe"),
                false,
            ),
            source(
                "Python",
                r"C:\Menu\2\Python.lnk",
                Some(r"C:\Python312\python.exe"),
                false,
            ),
            source(
                "Python",
                r"C:\Menu\3\Python.lnk",
                Some(r"C:\Python312\pythonw.exe"),
                false,
            ),
        ]);
        assert_eq!(
            distinctions,
            [
                Some("Python311".into()),
                Some(r"C:\Python312\python.exe".into()),
                Some("pythonw".into()),
            ]
        );
    }

    #[test]
    fn without_a_program_s_folder_the_source_s_own_folder_and_path_tell_them_apart() {
        // A command without a folder places the application by its
        // desktop entry.
        let entry = |id: &str, path: &str| Source {
            program: Some("gnome-terminal".into()),
            ..Source::new(Key::DesktopFile(id.into()), path, "Terminal", "here", 0)
        };
        let distinctions = distinctions_of(vec![
            source("Notes", "/Applications/Notes.app", None, false),
            source("Notes", "/Applications/Old/Notes.app", None, false),
            entry("a.desktop", "/usr/share/applications/a.desktop"),
            entry("b.desktop", "/opt/apps/b.desktop"),
            entry("c.desktop", "/usr/local/share/applications/c.desktop"),
        ]);
        assert_eq!(
            distinctions,
            [
                Some("Applications".into()),
                Some("Old".into()),
                Some("/usr/share/applications/a.desktop".into()),
                Some("apps".into()),
                Some("/usr/local/share/applications/c.desktop".into()),
            ]
        );
    }

    #[test]
    fn locale_keys_follow_the_desktop_entry_matching_order() {
        assert_eq!(
            locale_keys("sr_YU.UTF-8@Latn"),
            ["sr_YU@Latn", "sr_YU", "sr@Latn", "sr"]
        );
        assert_eq!(locale_keys("vi_VN.UTF-8"), ["vi_VN", "vi"]);
        assert_eq!(locale_keys("de@euro"), ["de@euro", "de"]);
        assert_eq!(locale_keys("fr"), ["fr"]);
        assert!(locale_keys("C").is_empty());
        assert!(locale_keys("C.UTF-8").is_empty());
        assert!(locale_keys("POSIX").is_empty());
        assert!(locale_keys("").is_empty());
    }

    #[test]
    fn the_best_localized_value_wins_else_the_plain_one() {
        let values: Vec<(Option<String>, String)> = vec![
            (None, "Files".into()),
            (Some("vi".into()), "Tệp".into()),
            (Some("sr@Latn".into()), "Datoteke".into()),
            (Some("pt_BR".into()), "Arquivos".into()),
        ];
        let name = |locale: &str| localized(&values, &locale_keys(locale));
        assert_eq!(name("vi_VN.UTF-8"), Some("Tệp"));
        assert_eq!(name("sr_RS@Latn"), Some("Datoteke"));
        assert_eq!(name("pt_BR.UTF-8"), Some("Arquivos"));
        // `pt` alone has no value: the plain one.
        assert_eq!(name("pt_PT"), Some("Files"));
        assert_eq!(name("C"), Some("Files"));
        assert_eq!(localized(&[], &locale_keys("vi")), None);
    }
}
