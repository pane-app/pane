//! `pane-ext new [folder] [options]` (#221, ADR 0047): writing a new
//! extension package from the templates pane-core embeds, and adding a
//! command to one that already exists.
//!
//! Every choice has a flag, so a script (or `npm create @pane-app`, whose
//! `@pane-app/create` package runs this) can pass them all and never be
//! asked; in an interactive terminal the missing ones are prompted for,
//! each with a default, so Enter walks an author through. A folder that
//! exists is written into only while empty: an author's files are never
//! overwritten.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pane_core::templates::{self, Kind, Language, Name, NewCommand};

/// The arguments of `new`: an optional folder, then one flag per choice.
/// A first argument of `command` makes the folder the package a command is
/// added to instead.
struct Arguments {
    command: bool,
    folder: Option<PathBuf>,
    name: Option<String>,
    language: Option<Language>,
    kind: Option<Kind>,
    id: Option<String>,
    title: Option<String>,
}

/// Runs `pane-ext new` with `args`: a new package, or (when the first
/// argument is `command`) a command added to an existing package.
pub(crate) fn run(args: std::iter::Skip<std::env::ArgsOs>) -> ExitCode {
    let interactive = std::io::stdin().is_terminal();
    let arguments = match parse(strings(args)) {
        Ok(arguments) => arguments,
        Err(problem) => {
            eprintln!("pane-ext: {problem}");
            return crate::usage_error();
        }
    };
    let mut input = std::io::stdin().lock();
    let outcome = if arguments.command {
        command(&arguments, &mut input, interactive)
    } else {
        package(&arguments, &mut input, interactive)
    };
    match outcome {
        Ok(next) => {
            print!("{next}");
            ExitCode::SUCCESS
        }
        Err(problem) => {
            eprintln!("pane-ext: {problem}");
            ExitCode::FAILURE
        }
    }
}

/// The arguments after `new`, as strings: flags and package folders.
fn strings(args: std::iter::Skip<std::env::ArgsOs>) -> Vec<String> {
    args.map(|argument| argument.to_string_lossy().into_owned())
        .collect()
}

/// The arguments after `new`: an optional folder, then the flags, with
/// `command` first switching to adding a command.
fn parse(mut rest: Vec<String>) -> Result<Arguments, String> {
    let mut arguments = Arguments {
        command: false,
        folder: None,
        name: None,
        language: None,
        kind: None,
        id: None,
        title: None,
    };
    if rest.first().is_some_and(|first| first == "command") {
        arguments.command = true;
        rest.remove(0);
    }
    let mut taking: Option<String> = None;
    for argument in rest {
        if let Some(flag) = taking.take() {
            match flag.as_str() {
                "--name" => arguments.name = Some(argument),
                "--language" => {
                    let language = Language::parse(&argument)
                        .ok_or_else(|| not_a_choice("--language", &argument, LANGUAGES))?;
                    arguments.language = Some(language);
                }
                "--template" => {
                    let kind = Kind::parse(&argument)
                        .ok_or_else(|| not_a_choice("--template", &argument, TEMPLATES))?;
                    arguments.kind = Some(kind);
                }
                "--id" => arguments.id = Some(argument),
                "--title" => arguments.title = Some(argument),
                _ => unreachable!("the flags above are the ones taken"),
            }
            continue;
        }
        match argument.as_str() {
            "--name" | "--language" | "--template" | "--id" | "--title" => {
                taking = Some(argument);
            }
            _ if argument.starts_with('-') => {
                return Err(format!(
                    "`{argument}` is not an argument new takes: a package folder, then \
                     --name, --language and --template (--id and --title after \
                     `new command`)"
                ));
            }
            _ => {
                if arguments.folder.replace(PathBuf::from(&argument)).is_some() {
                    return Err(if arguments.command {
                        "new command takes one package folder, not several".into()
                    } else {
                        "new takes one package folder, not several".into()
                    });
                }
            }
        }
    }
    if arguments.command && arguments.folder.is_none() {
        return Err("new command needs the folder of the package it adds a command to".into());
    }
    Ok(arguments)
}

/// The language choices, as `--language`'s error lists them.
const LANGUAGES: &str = "rust, typescript";

/// The template choices, as `--template`'s error lists them.
const TEMPLATES: &str = "list, detail, form, no-view";

/// `value` is not one of the choices `flag` takes.
fn not_a_choice(flag: &str, value: &str, choices: &str) -> String {
    format!("`{value}` is not a {flag} choice: {choices}")
}

/// What `new` says when neither a folder nor a `--name` names the package.
const FOLDER_OR_NAME: &str = "new needs a package folder, or a --name to name the package by \
                              (it writes the folder from the name)";

/// The name of the folder `folder`, when it has one that is not empty.
fn folder_name(folder: &Path) -> Option<&str> {
    let name = folder.file_name()?.to_str()?;
    (!name.is_empty()).then_some(name)
}

/// Writes the new package the arguments describe, asking `input` for the
/// choices the flags left out while the terminal is interactive. Answers
/// with what to print next.
fn package(
    arguments: &Arguments,
    input: &mut dyn BufRead,
    interactive: bool,
) -> Result<String, String> {
    let language = match arguments.language {
        Some(language) => language,
        None if interactive => ask_language(input),
        None => Language::TypeScript,
    };
    let kind = match arguments.kind {
        Some(kind) => kind,
        None if interactive => ask_kind(input),
        None => Kind::List,
    };
    // The name, when the flags leave none: from the folder when there is
    // one, else asked for (or refused without a terminal to ask on).
    let name = match &arguments.name {
        Some(name) => Name::parse(name)?,
        None => {
            let given = match arguments.folder.as_deref().and_then(folder_name) {
                Some(given) => given.to_owned(),
                None if interactive => ask_name(input),
                None => return Err(FOLDER_OR_NAME.into()),
            };
            Name::parse(&given)?
        }
    };
    let folder = match &arguments.folder {
        Some(folder) => folder.clone(),
        None => PathBuf::from(&name.package),
    };
    templates::scaffold(&folder, &name, language, kind)?;
    let mut next = format!(
        "pane-ext: wrote {} (the {} template, in {}) into {}\npane-ext: next:\n",
        name.title,
        kind.name(),
        language.name(),
        folder.display()
    );
    next.push_str(&format!("pane-ext:   cd {}\n", folder.display()));
    // `npm run dev` runs `pane-ext dev`; the dependencies come first.
    let runs = match language {
        Language::TypeScript => "pane-ext:   npm install\npane-ext:   npm run dev\n",
        Language::Rust => "pane-ext:   pane-ext dev\n",
    };
    next.push_str(runs);
    Ok(next)
}

/// Adds the command the arguments describe to the package in the folder,
/// asking `input` for the choices the flags left out while the terminal is
/// interactive. Answers with what to print next.
fn command(
    arguments: &Arguments,
    input: &mut dyn BufRead,
    interactive: bool,
) -> Result<String, String> {
    let folder = arguments.folder.clone().unwrap_or_default();
    let kind = match arguments.kind {
        Some(kind) => kind,
        None if interactive => ask_kind(input),
        None => Kind::List,
    };
    let title = match &arguments.title {
        Some(title) => Some(title.clone()),
        None if interactive => Some(ask("Command title", "Note", input)),
        None => None,
    };
    let new = NewCommand::parse(arguments.id.as_deref(), title.as_deref())?;
    templates::add_command(&folder, &new, kind)?;
    let mut next = format!(
        "pane-ext: added the command `{}` (the {} template) to {}\npane-ext: next:\n",
        new.id,
        kind.name(),
        folder.display()
    );
    next.push_str(&format!(
        "pane-ext:   cd {}\npane-ext:   pane-ext dev\n",
        folder.display()
    ));
    Ok(next)
}

/// Asks for the package's name, re-asking until it names a package, and
/// answers the name the author gave.
fn ask_name(input: &mut dyn BufRead) -> String {
    loop {
        let answer = ask("Extension title", "My Extension", input);
        match Name::parse(&answer) {
            Ok(_) => return answer,
            Err(problem) => println!("pane-ext: {problem}"),
        }
    }
}

/// Asks for the language, re-asking until it names one.
fn ask_language(input: &mut dyn BufRead) -> Language {
    loop {
        let answer = ask("Language (rust or typescript)", "typescript", input);
        if let Some(language) = Language::parse(&answer) {
            return language;
        }
        println!("pane-ext: \"{answer}\" is not a language pane-ext templates; choose again");
    }
}

/// Asks for the template, re-asking until it names one.
fn ask_kind(input: &mut dyn BufRead) -> Kind {
    loop {
        let answer = ask("Template (list, detail, form or no-view)", "list", input);
        if let Some(kind) = Kind::parse(&answer) {
            return kind;
        }
        println!("pane-ext: \"{answer}\" is not a template pane-ext writes; choose again");
    }
}

/// Asks `question`, reading one line: the answer, or `default` for an
/// empty line (and at the end of the input, so a script's truncated input
/// still answers).
fn ask(question: &str, default: &str, input: &mut dyn BufRead) -> String {
    print!("{question} [{default}]: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    input.read_line(&mut line).unwrap_or(0);
    let line = line.trim();
    if line.is_empty() {
        default.to_owned()
    } else {
        line.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// The arguments of a new package, with only what `folder` gives.
    fn arguments(folder: Option<PathBuf>) -> Arguments {
        Arguments {
            command: false,
            folder,
            name: None,
            language: None,
            kind: None,
            id: None,
            title: None,
        }
    }

    #[test]
    fn the_arguments_parse() {
        let parsed = parse(vec![
            "notes".into(),
            "--name".into(),
            "Word Count".into(),
            "--language".into(),
            "rust".into(),
            "--template".into(),
            "form".into(),
        ])
        .unwrap();
        assert_eq!(parsed.folder, Some(PathBuf::from("notes")));
        assert_eq!(parsed.name.as_deref(), Some("Word Count"));
        assert_eq!(parsed.language, Some(Language::Rust));
        assert_eq!(parsed.kind, Some(Kind::Form));

        // `new command` switches to adding a command.
        let given = vec![
            "command".into(),
            "notes".into(),
            "--id".into(),
            "note".into(),
        ];
        let command = parse(given).unwrap();
        assert!(command.command);
        assert_eq!(command.folder, Some(PathBuf::from("notes")));
        assert_eq!(command.id.as_deref(), Some("note"));

        assert!(parse(vec!["command".into()]).is_err(), "needs a folder");
        let not_a_language = parse(vec!["--language".into(), "javascript".into()]);
        assert!(not_a_language.is_err());
        let not_a_template = parse(vec!["--template".into(), "table".into()]);
        assert!(not_a_template.is_err());
        assert!(parse(vec!["--wrong".into()]).is_err());
        assert!(parse(vec!["notes".into(), "other".into()]).is_err());
    }

    #[test]
    fn a_package_is_written_from_what_the_terminal_answers() {
        let folder = tempfile::tempdir().unwrap();
        let arguments = arguments(Some(folder.path().join("notes")));
        // The language, then the template, then the title (only when the
        // folder leaves none to name the package by).
        let mut input = Cursor::new("rust\nform\nWord Count\n");
        let next = package(&arguments, &mut input, true).unwrap();
        assert!(
            next.contains("wrote Word Count (the form template, in Rust) into"),
            "{next}"
        );
        assert!(folder.path().join("notes/Cargo.toml").is_file());
        assert!(folder.path().join("notes/src/lib.rs").is_file());
    }

    #[test]
    fn empty_answers_take_the_defaults_and_the_folders_name() {
        let folder = tempfile::tempdir().unwrap();
        let arguments = arguments(Some(folder.path().join("word-count")));
        let mut input = Cursor::new("\n\n".repeat(3));
        let next = package(&arguments, &mut input, true).unwrap();
        assert!(
            next.contains("wrote Word Count (the list template, in TypeScript) into"),
            "{next}"
        );
        assert!(folder.path().join("word-count/package.json").is_file());
    }

    #[test]
    fn without_a_terminal_the_flags_and_folder_name_the_package() {
        let folder = tempfile::tempdir().unwrap();
        let mut arguments = arguments(Some(folder.path().join("word-count")));
        arguments.language = Some(Language::Rust);
        let mut input = Cursor::new(Vec::new());
        let next = package(&arguments, &mut input, false).unwrap();
        assert!(next.contains("wrote Word Count"), "{next}");
        assert!(folder.path().join("word-count/Cargo.toml").is_file());
    }

    #[test]
    fn without_a_terminal_and_a_name_new_refuses_rather_than_guessing() {
        let mut input = Cursor::new(Vec::new());
        let error = package(&arguments(None), &mut input, false).unwrap_err();
        assert!(
            error.contains("new needs a package folder, or a --name"),
            "{error}"
        );
    }

    #[test]
    fn a_command_is_added_asking_for_its_title() {
        let folder = tempfile::tempdir().unwrap();
        let scaffolding = arguments(Some(folder.path().to_path_buf()));
        let mut input = Cursor::new(Vec::new());
        package(&scaffolding, &mut input, false).unwrap();
        let adding = Arguments {
            command: true,
            folder: Some(folder.path().to_path_buf()),
            name: None,
            language: None,
            kind: None,
            id: None,
            title: None,
        };
        let mut answers = Cursor::new("Note\n".repeat(2));
        let next = command(&adding, &mut answers, true).unwrap();
        assert!(
            next.contains("added the command `note` (the list template)"),
            "{next}"
        );
        assert!(folder.path().join("src/note.ts").is_file());
    }

    #[test]
    fn a_wrong_answer_is_asked_again() {
        let folder = tempfile::tempdir().unwrap();
        let arguments = arguments(Some(folder.path().join("notes")));
        // A wrong language, then a right one; a wrong template, then a
        // right one.
        let mut input = Cursor::new("kotlin\nrust\ntable\nno-view\n");
        let next = package(&arguments, &mut input, true).unwrap();
        assert!(next.contains("(the no-view template, in Rust)"), "{next}");
    }
}
