//! Linux: the applications the XDG desktop entry specification lists, one
//! `.desktop` file per application in the `applications` folder of each XDG
//! data folder, opened by running the program its `Exec` key names. An
//! application is identified by its desktop file id, as the Desktop Entry
//! specification defines it.
//!
//! An entry is titled with its `Name` for the user's messages locale, and
//! found by its `Keywords` for that locale, both chosen by the Desktop
//! Entry specification's matching rules ([`super::names::locale_keys`]);
//! its plain `Name` still finds it, and the program its `Exec` runs is the
//! program whose name may be an alternate title ([`super::names`]).

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::names::{arguments_name_something, locale_keys, localized, messages_locale};
use super::{
    Changes, Discovery, Key, Source, Watch, env_dir, has_extension, id_path, sorted_entries,
    watching,
};

/// How deep Pane looks into subfolders of an `applications` folder.
const MAX_DEPTH: usize = 8;

/// The desktop entries in `applications` folders.
#[derive(Clone, Debug)]
pub struct DesktopEntries {
    /// The `applications` folders, the one whose entries win first: an entry
    /// with the same desktop file id in a later folder is not listed.
    folders: Vec<PathBuf>,
    /// The desktops the session runs, for `OnlyShowIn` and `NotShowIn`.
    desktops: Vec<String>,
    /// The keys localized values are chosen by, best first, for the user's
    /// messages locale ([`locale_keys`]); none for untranslated values.
    locale: Vec<String>,
}

impl DesktopEntries {
    /// The entries in `folders`, the first winning, for a session that runs
    /// no particular desktop, with untranslated names.
    pub fn new(folders: Vec<PathBuf>) -> DesktopEntries {
        DesktopEntries {
            folders,
            desktops: Vec::new(),
            locale: Vec::new(),
        }
    }

    /// These entries, named and found by their values localized for
    /// `locale`, a POSIX locale such as `vi_VN.UTF-8`.
    pub fn with_locale(mut self, locale: &str) -> DesktopEntries {
        self.locale = locale_keys(locale);
        self
    }

    /// The entries in the folders the XDG base directory specification
    /// names: `$XDG_DATA_HOME/applications` (default
    /// `~/.local/share/applications`), then `applications` in each of
    /// `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`), for the
    /// desktops in `$XDG_CURRENT_DESKTOP`, localized for the user's
    /// messages locale (`$LC_ALL`, `$LC_MESSAGES`, `$LANG`).
    pub fn from_env() -> DesktopEntries {
        let home = env_dir("XDG_DATA_HOME")
            .or_else(|| env_dir("HOME").map(|home| home.join(".local/share")));
        let dirs = std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|dirs| !dirs.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        let folders = home
            .into_iter()
            .chain(
                dirs.split(':')
                    .filter(|dir| !dir.is_empty())
                    .map(PathBuf::from),
            )
            .map(|dir| dir.join("applications"))
            .collect();
        let desktops = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter(|desktop| !desktop.is_empty())
            .map(str::to_owned)
            .collect();
        DesktopEntries {
            folders,
            desktops,
            locale: locale_keys(&messages_locale()),
        }
    }

    /// Adds each `.desktop` file under `dir` whose desktop file id (its path
    /// below `root`, `/` becoming `-`) is not `seen` yet.
    fn collect(
        &self,
        (root, place): (&Path, usize),
        dir: &Path,
        depth: usize,
        seen: &mut HashSet<String>,
        found: &mut Vec<Source>,
    ) {
        for entry in sorted_entries(dir) {
            let path = entry.path();
            if path.is_dir() {
                if depth < MAX_DEPTH {
                    self.collect((root, place), &path, depth + 1, seen, found);
                }
                continue;
            }
            if !has_extension(&path, "desktop") {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let id = relative.to_string_lossy().replace(['/', '\\'], "-");
            // The first entry with an id hides the others, even when it is
            // hidden itself: that is how a user removes a system entry.
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(desktop) = Entry::read(&path, &self.locale) else {
                continue;
            };
            if !desktop.listed(&self.desktops) {
                continue;
            }
            // An entry Pane could not run correctly is not offered.
            let exec = desktop.exec.as_deref().unwrap_or_default();
            let arguments = match exec_arguments(exec, &desktop, &path) {
                Ok(arguments) => arguments,
                Err(problem) => {
                    eprintln!("pane: skipped desktop entry {}: {problem}", path.display());
                    continue;
                }
            };
            let untranslated = desktop
                .untranslated
                .clone()
                .filter(|plain| !plain.trim().is_empty() && *plain != desktop.name);
            let (program, rest) = match arguments.split_first() {
                Some((program, rest)) => (Some(program.clone()), rest),
                None => (None, &[][..]),
            };
            found.push(Source {
                untranslated,
                keywords: desktop.keywords.clone(),
                arguments: arguments_name_something(rest),
                program,
                ..Source::new(
                    Key::DesktopFile(id),
                    path.to_string_lossy(),
                    desktop.name,
                    dir.display().to_string(),
                    place,
                )
            });
        }
    }
}

impl Discovery for DesktopEntries {
    fn sources(&self) -> Result<Vec<Source>, String> {
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        for (place, folder) in self.folders.iter().enumerate() {
            self.collect((folder, place), folder, 0, &mut seen, &mut found);
        }
        Ok(found)
    }

    fn open(&self, id: &str) -> Result<(), String> {
        let path = id_path(id, "desktop", "a desktop entry")?;
        let entry = Entry::read(&path, &self.locale)
            .ok_or_else(|| format!("{id} is not an application"))?;
        let exec = entry
            .exec
            .as_deref()
            .ok_or_else(|| format!("{id} names no program to run (no Exec key)"))?;
        let mut arguments = exec_arguments(exec, &entry, &path)?;
        if entry.terminal {
            let terminal = std::env::var("TERMINAL").ok();
            arguments = terminal_command(&arguments, terminal.as_deref(), &program_exists)
                .ok_or_else(|| {
                    format!(
                        "{} runs in a terminal, and Pane found no terminal to open it in \
                         (set $TERMINAL, or install x-terminal-emulator or xterm)",
                        entry.name
                    )
                })?;
        }
        let (program, rest) = arguments
            .split_first()
            .ok_or_else(|| format!("the Exec key of {id} is empty"))?;
        let mut command = Command::new(program);
        command
            .args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(dir) = entry.path.as_deref().filter(|dir| Path::new(dir).is_dir()) {
            command.current_dir(dir);
        }
        #[cfg(unix)]
        {
            // Its own process group, so signals meant for Pane do not reach it.
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => format!("cannot find the program {program}"),
            io::ErrorKind::PermissionDenied => format!("not allowed to run {program}"),
            _ => format!("cannot run {program}: {error}"),
        })?;
        // Collected when it exits, so it does not linger as a zombie.
        std::thread::spawn(move || child.wait());
        Ok(())
    }

    /// Watches the `applications` folders and their subfolders (inotify on
    /// Linux); one that does not exist yet is watched for from the nearest
    /// folder above it that does.
    fn watch(&self, changes: Changes) -> Result<Watch, String> {
        watching::watch(
            self.folders
                .iter()
                .map(|folder| watching::Folder::walked(folder.clone()))
                .collect(),
            changes,
        )
    }
}

/// The keys of a desktop entry's `[Desktop Entry]` group that Pane uses.
#[derive(Default)]
struct Entry {
    kind: Option<String>,
    /// `Name` for the locale it was read for.
    name: String,
    /// The plain `Name`, untranslated.
    untranslated: Option<String>,
    /// `Keywords` for the locale it was read for.
    keywords: Vec<String>,
    exec: Option<String>,
    try_exec: Option<String>,
    path: Option<String>,
    icon: Option<String>,
    hidden: bool,
    no_display: bool,
    terminal: bool,
    only_show_in: Option<Vec<String>>,
    not_show_in: Vec<String>,
}

impl Entry {
    /// The entry in `path`, its localized values chosen for the locale
    /// whose [`locale_keys`] are `locale`; `None` if it cannot be read.
    fn read(path: &Path, locale: &[String]) -> Option<Entry> {
        let text = std::fs::read_to_string(path).ok()?;
        Some(Entry::parse(&text, locale))
    }

    /// The entry `text` holds, its localized values chosen for the locale
    /// whose [`locale_keys`] are `locale`.
    fn parse(text: &str, locale: &[String]) -> Entry {
        let mut entry = Entry::default();
        let mut names: Vec<(Option<String>, String)> = Vec::new();
        let mut keywords: Vec<(Option<String>, String)> = Vec::new();
        let mut in_group = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_group = line == "[Desktop Entry]";
                continue;
            }
            if !in_group || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = unescape(value.trim());
            let list = |value: &str| -> Vec<String> {
                value
                    .split(';')
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned)
                    .collect()
            };
            // A localized key, `Name[vi]`: its locale apart.
            let (key, key_locale) = match key.trim().split_once('[') {
                Some((key, rest)) => match rest.strip_suffix(']') {
                    Some(key_locale) => (key.trim(), Some(key_locale.to_owned())),
                    None => continue,
                },
                None => (key.trim(), None),
            };
            match key {
                "Name" => {
                    names.push((key_locale, value));
                    continue;
                }
                "Keywords" => {
                    keywords.push((key_locale, value));
                    continue;
                }
                // The other keys Pane uses are not localized.
                _ if key_locale.is_some() => continue,
                _ => {}
            }
            match key {
                "Type" => entry.kind = Some(value),
                "Exec" => entry.exec = Some(value),
                "TryExec" => entry.try_exec = Some(value),
                "Path" => entry.path = Some(value),
                "Icon" => entry.icon = Some(value),
                "Hidden" => entry.hidden = value == "true",
                "NoDisplay" => entry.no_display = value == "true",
                "Terminal" => entry.terminal = value == "true",
                "OnlyShowIn" => entry.only_show_in = Some(list(&value)),
                "NotShowIn" => entry.not_show_in = list(&value),
                _ => {}
            }
        }
        entry.name = localized(&names, locale).unwrap_or_default().to_owned();
        entry.untranslated = localized(&names, &[]).map(str::to_owned);
        entry.keywords = localized(&keywords, locale)
            .map(|value| {
                value
                    .split(';')
                    .map(str::trim)
                    .filter(|keyword| !keyword.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        entry
    }

    /// Whether a launcher lists it, in a session running `desktops`.
    fn listed(&self, desktops: &[String]) -> bool {
        let shown_here = match &self.only_show_in {
            Some(only) => only.iter().any(|desktop| desktops.contains(desktop)),
            None => true,
        } && !self
            .not_show_in
            .iter()
            .any(|desktop| desktops.contains(desktop));
        self.kind.as_deref() == Some("Application")
            && !self.name.trim().is_empty()
            && self.exec.is_some()
            && !self.hidden
            && !self.no_display
            && shown_here
            && self.try_exec.as_deref().is_none_or(program_exists)
    }
}

/// Whether `program`, a path or a name looked up in `PATH`, is a file.
fn program_exists(program: &str) -> bool {
    let program = Path::new(program);
    if program.is_absolute() {
        return program.is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// A string value with the escapes every desktop entry value may use:
/// `\s`, `\n`, `\t`, `\r` and `\\`. Other backslashes are kept, for the
/// `Exec` key's own quoting.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The program and arguments an `Exec` value runs, with no files or URLs:
/// arguments split at spaces except inside double quotes (where `\"`,
/// `` \` ``, `\$` and `\\` stand for the character), and the field codes
/// expanded (`%c` the name, `%k` the entry's path, `%i` its icon, `%%` a
/// percent sign) or, for files and URLs, dropped.
fn exec_arguments(exec: &str, entry: &Entry, path: &Path) -> Result<Vec<String>, String> {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(escaped @ ('"' | '`' | '$' | '\\')) => word.push(escaped),
                            Some(other) => {
                                word.push('\\');
                                word.push(other);
                            }
                            None => return Err("its Exec key ends inside quotes".into()),
                        },
                        Some(other) => word.push(other),
                        None => return Err("its Exec key has an unclosed quote".into()),
                    }
                }
            }
            ' ' | '\t' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            other => {
                in_word = true;
                word.push(other);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    check_field_codes(&words)?;
    let mut arguments = Vec::new();
    for word in words {
        match word.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            "%i" => {
                if let Some(icon) = &entry.icon {
                    arguments.push("--icon".into());
                    arguments.push(icon.clone());
                }
            }
            _ => arguments.push(expand(&word, entry, path)),
        }
    }
    Ok(arguments)
}

/// Why the field codes in the `Exec` arguments `words` break the Desktop
/// Entry specification, if they do: `%i`, `%F` and `%U` (which expand to
/// several arguments or none) only as a whole argument, at most one of `%f`,
/// `%u`, `%F` and `%U`, only the codes it defines, and `%` always followed by
/// a code. Such an entry is not run: guessing what it meant could pass the
/// wrong arguments.
fn check_field_codes(words: &[String]) -> Result<(), String> {
    let mut file_codes = 0;
    for word in words {
        let mut chars = word.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                continue;
            }
            match chars.next() {
                None => return Err("its Exec key ends with a lone %".into()),
                Some(code @ ('i' | 'F' | 'U')) if word.chars().count() != 2 => {
                    return Err(format!(
                        "its Exec key uses %{code} inside an argument ({word})"
                    ));
                }
                Some('f' | 'u' | 'F' | 'U') => file_codes += 1,
                Some('%' | 'i' | 'c' | 'k' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm') => {}
                Some(other) => {
                    return Err(format!("its Exec key has an unknown field code %{other}"));
                }
            }
        }
    }
    if file_codes > 1 {
        return Err("its Exec key has more than one of %f, %u, %F and %U".into());
    }
    Ok(())
}

/// The terminal emulators Pane tries for an entry with `Terminal=true`, in
/// order after `$TERMINAL`, each with the option before the command it runs.
const TERMINALS: [(&str, Option<&str>); 8] = [
    ("x-terminal-emulator", Some("-e")),
    ("gnome-terminal", Some("--")),
    ("konsole", Some("-e")),
    ("xfce4-terminal", Some("-x")),
    ("alacritty", Some("-e")),
    ("kitty", None),
    ("foot", None),
    ("xterm", Some("-e")),
];

/// `argv` run in a terminal emulator: `$TERMINAL -e` (`terminal_env`) when
/// it is installed, else the first of [`TERMINALS`] installed; `None` when
/// there is none. `installed` says whether a program can be run.
fn terminal_command(
    argv: &[String],
    terminal_env: Option<&str>,
    installed: &dyn Fn(&str) -> bool,
) -> Option<Vec<String>> {
    let chosen = terminal_env
        .filter(|terminal| !terminal.is_empty() && installed(terminal))
        .map(|terminal| (terminal, Some("-e")))
        .or_else(|| {
            TERMINALS
                .into_iter()
                .find(|(terminal, _)| installed(terminal))
        })?;
    let (terminal, option) = chosen;
    Some(
        std::iter::once(terminal.to_owned())
            .chain(option.map(str::to_owned))
            .chain(argv.iter().cloned())
            .collect(),
    )
}

/// `word` with the field codes inside it expanded or dropped.
fn expand(word: &str, entry: &Entry, path: &Path) -> String {
    let mut out = String::new();
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('c') => out.push_str(&entry.name),
            Some('k') => out.push_str(&path.to_string_lossy()),
            // Files, URLs and deprecated codes: there are none to pass.
            Some(_) | None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(exec: &str) -> Vec<String> {
        let entry = Entry {
            name: "Viewer".into(),
            icon: Some("viewer".into()),
            ..Entry::default()
        };
        exec_arguments(&unescape(exec), &entry, Path::new("/apps/viewer.desktop")).unwrap()
    }

    #[test]
    fn exec_splits_at_spaces_outside_quotes() {
        assert_eq!(arguments("viewer  --new"), ["viewer", "--new"]);
        assert_eq!(
            arguments(r#""/opt/My Viewer/viewer" "" --title "a \"b\"""#),
            ["/opt/My Viewer/viewer", "", "--title", r#"a "b""#]
        );
    }

    #[test]
    fn exec_keeps_quoted_backslashes_and_dollars_after_the_general_escapes() {
        // `\\$` in the file is `\$` after the general escapes, then `$`.
        assert_eq!(
            arguments(r#"sh -c "echo \\$HOME \\\\ done""#),
            ["sh", "-c", r"echo $HOME \ done"]
        );
        // The general escapes apply first: `\s` is a space, which separates.
        assert_eq!(arguments(r"viewer a\sb"), ["viewer", "a", "b"]);
    }

    #[test]
    fn exec_field_codes_are_expanded_or_dropped() {
        assert_eq!(
            arguments("viewer %U --name=%c %i --entry %k 100%%"),
            [
                "viewer",
                "--name=Viewer",
                "--icon",
                "viewer",
                "--entry",
                "/apps/viewer.desktop",
                "100%"
            ]
        );
    }

    #[test]
    fn an_unclosed_quote_is_an_error() {
        let entry = Entry::default();
        assert!(exec_arguments(r#"viewer "a"#, &entry, Path::new("/a.desktop")).is_err());
    }

    fn invalid(exec: &str) -> String {
        let entry = Entry::default();
        exec_arguments(exec, &entry, Path::new("/a.desktop")).unwrap_err()
    }

    #[test]
    fn field_codes_used_against_the_specification_make_the_exec_line_invalid() {
        // %i, %F and %U expand to several arguments or none: only whole.
        assert_eq!(
            invalid("viewer --icon=%i"),
            "its Exec key uses %i inside an argument (--icon=%i)"
        );
        assert_eq!(
            invalid("viewer --files=%F"),
            "its Exec key uses %F inside an argument (--files=%F)"
        );
        // At most one of %f, %u, %F and %U.
        assert_eq!(
            invalid("viewer %f%u"),
            "its Exec key has more than one of %f, %u, %F and %U"
        );
        assert_eq!(
            invalid("viewer %f %U"),
            "its Exec key has more than one of %f, %u, %F and %U"
        );
        assert_eq!(
            invalid("viewer %z"),
            "its Exec key has an unknown field code %z"
        );
        assert_eq!(invalid("viewer 100%"), "its Exec key ends with a lone %");
    }

    const FILES: &str = "[Desktop Entry]\n\
        Type=Application\n\
        Name=Files\n\
        Name[vi]=Tệp\n\
        Name[pt_BR]=Arquivos\n\
        Keywords=folder;manager; explore ;\n\
        Keywords[vi]=thư mục;quản lý;\n\
        Exec[vi]=wrong\n\
        Exec=nautilus --new-window %U\n\
        [Desktop Action new]\n\
        Name[vi]=Cửa sổ mới\n";

    #[test]
    fn name_and_keywords_are_chosen_for_the_locale_and_the_plain_name_kept() {
        let vietnamese = Entry::parse(FILES, &locale_keys("vi_VN.UTF-8"));
        assert_eq!(vietnamese.name, "Tệp");
        assert_eq!(vietnamese.untranslated.as_deref(), Some("Files"));
        assert_eq!(vietnamese.keywords, ["thư mục", "quản lý"]);
        // Only the plain key of what is not localized.
        assert_eq!(vietnamese.exec.as_deref(), Some("nautilus --new-window %U"));

        // No localized keywords: the plain ones.
        let brazilian = Entry::parse(FILES, &locale_keys("pt_BR"));
        assert_eq!(brazilian.name, "Arquivos");
        assert_eq!(brazilian.keywords, ["folder", "manager", "explore"]);

        let untranslated = Entry::parse(FILES, &[]);
        assert_eq!(untranslated.name, "Files");
        assert!(untranslated.listed(&[]));
    }

    fn terminal(
        terminal_env: Option<&str>,
        installed: &[&str],
        argv: &[&str],
    ) -> Option<Vec<String>> {
        let argv: Vec<String> = argv.iter().map(|word| (*word).to_owned()).collect();
        terminal_command(&argv, terminal_env, &|program| installed.contains(&program))
    }

    #[test]
    fn a_terminal_application_runs_in_the_first_terminal_found() {
        let top = ["top", "-d", "1"];
        assert_eq!(
            terminal(Some("foot"), &["foot", "xterm"], &top).unwrap(),
            ["foot", "-e", "top", "-d", "1"]
        );
        // $TERMINAL names a missing program: the usual ones are tried.
        assert_eq!(
            terminal(Some("missing"), &["x-terminal-emulator", "xterm"], &top).unwrap(),
            ["x-terminal-emulator", "-e", "top", "-d", "1"]
        );
        assert_eq!(
            terminal(None, &["gnome-terminal", "xterm"], &top).unwrap(),
            ["gnome-terminal", "--", "top", "-d", "1"]
        );
        assert_eq!(
            terminal(None, &["konsole"], &top).unwrap(),
            ["konsole", "-e", "top", "-d", "1"]
        );
        assert_eq!(
            terminal(Some(""), &["xterm"], &top).unwrap(),
            ["xterm", "-e", "top", "-d", "1"]
        );
        assert_eq!(terminal(None, &[], &top), None);
    }
}
