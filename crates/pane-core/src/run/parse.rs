//! How a command line the Run dialog takes splits into what runs and what
//! it is given, and how environment variables expand in both. Plain text
//! work, compiled and tested on every system; the adapter gives the file
//! system and the environment through [`Sources`], so the longest prefix
//! that exists on disk is found as Windows finds it.
//!
//! The rules, as the Run dialog's own are:
//!
//! - a quoted head is taken as written: `"C:\Program Files\T\tool.exe" -a`
//!   runs `C:\Program Files\T\tool.exe` with `-a`, quotes and all;
//! - otherwise, a head ending in a program, applet or console extension
//!   (`.exe`, `.com`, `.bat`, `.cmd`, `.cpl`, `.msc`) splits there, so
//!   `notepad.exe -a` runs `notepad.exe` with `-a` and
//!   `C:\Windows\System32\devmgmt.msc -a` the console with `-a`;
//! - otherwise, for a rooted path with spaces (a drive, a network path,
//!   or an environment variable that names one), the longest
//!   space-separated prefix that exists on disk is the program, so
//!   `C:\Program Files\Tool\tool.exe -a` runs without quotes;
//! - otherwise the first space splits, so `mmc devmgmt.msc` runs `mmc`
//!   with `devmgmt.msc`.
//!
//! Environment variables (`%NAME%`) are expanded in the program and in
//! the arguments alike, before anything runs.

use super::RunError;

/// How the parser reaches the file system and the environment: the
/// Windows adapter gives the real ones, the tests fakes of their own.
pub trait Sources {
    /// Whether `path` exists on disk.
    fn exists(&self, path: &str) -> bool;

    /// `text` with each `%NAME%` replaced by the value the environment
    /// gives, as Windows expands it; a name no variable has stays as it
    /// is.
    fn expand(&self, text: &str) -> String;
}

/// A command line split into what runs and what it is given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Split {
    /// The program, path or address: what runs.
    pub head: String,
    /// The arguments that follow it, as typed with its environment
    /// variables expanded; empty when there are none.
    pub arguments: String,
}

/// The extensions whose name ends a head: a program, a Control Panel
/// applet or a management console.
const PROGRAM_EXTENSIONS: [&str; 6] = ["exe", "com", "bat", "cmd", "cpl", "msc"];

/// Splits `line` as the Run dialog reads it, expanding environment
/// variables in both halves (see the module's rules). A blank line is
/// refused.
pub fn split(line: &str, sources: &dyn Sources) -> Result<Split, RunError> {
    let line = line.trim();
    if line.is_empty() {
        return Err(RunError::Failed(
            "no command was given: type a program, path or address".into(),
        ));
    }
    let (head, arguments) = match line.split_once(' ') {
        Some((head, arguments)) => (head, arguments.trim()),
        // With no space, the whole line is the head.
        None => (line, ""),
    };

    // A quoted head is taken as written, with the rest as the arguments.
    if let Some(after_quote) = line.strip_prefix('"') {
        return Ok(match after_quote.split_once('"') {
            Some(head) => Split {
                head: sources.expand(head.0),
                arguments: sources.expand(head.1.trim_start()),
            },
            // No closing quote: the rest of the line is the head.
            None => Split {
                head: sources.expand(after_quote),
                arguments: String::new(),
            },
        });
    }

    // A head ending in a program, applet or console extension splits
    // there, so an unquoted program with arguments is found before
    // anything touches the disk.
    if ends_program(head) {
        return Ok(Split {
            head: sources.expand(head),
            arguments: sources.expand(arguments),
        });
    }

    // A rooted path with spaces: the longest prefix that exists on disk
    // is the program, with the rest as its arguments.
    if rooted(head) {
        let spaces: Vec<usize> = line.match_indices(' ').map(|(at, _)| at).collect();
        for at in spaces.iter().rev() {
            let candidate = sources.expand(&line[..*at]);
            if sources.exists(&candidate) {
                return Ok(Split {
                    head: candidate,
                    arguments: sources.expand(line[at + 1..].trim()),
                });
            }
        }
    }

    // Otherwise the first space splits.
    Ok(Split {
        head: sources.expand(head),
        arguments: sources.expand(arguments),
    })
}

/// Whether `head` ends in a program, applet or console extension.
fn ends_program(head: &str) -> bool {
    let Some((_, extension)) = head.rsplit_once('.') else {
        return false;
    };
    PROGRAM_EXTENSIONS
        .iter()
        .any(|known| extension.eq_ignore_ascii_case(known))
}

/// Whether `text` names a rooted path: a drive (`C:\…`, `D:…`), a
/// network path (`\\server\…`, whose forward-slash spelling
/// `//server/…` is the same path on Windows) or an environment
/// variable (`%NAME%\…`), which expands to one. Run is Windows-only,
/// so these Windows shapes are decided on every system, not by the
/// platform the binary happens to run on.
pub fn rooted(text: &str) -> bool {
    let bytes = text.as_bytes();
    (bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic())
        || text.starts_with('\\')
        || text.starts_with('/')
        || text.starts_with('%')
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// A parser's fake ground: the files that exist (by their paths), and
    /// the environment's variables.
    struct Ground {
        files: Vec<String>,
        variables: HashMap<String, String>,
    }

    impl Ground {
        fn new() -> Ground {
            Ground {
                files: Vec::new(),
                variables: HashMap::new(),
            }
        }

        fn file(mut self, path: &str) -> Ground {
            self.files.push(path.to_owned());
            self
        }

        fn variable(mut self, name: &str, value: &str) -> Ground {
            // The Run dialog's variables match ignoring case, so the
            // ground keeps every name as it is looked up.
            self.variables
                .insert(name.to_ascii_lowercase(), value.into());
            self
        }
    }

    impl Sources for Ground {
        fn exists(&self, path: &str) -> bool {
            // Windows takes '/' as '\\': a path the parser spells with
            // forward separators is the file the ground holds, as the
            // real adapter's file system takes it.
            let spelled = |path: &str| path.replace('/', "\\");
            self.files.iter().any(|file| spelled(file) == spelled(path))
        }

        fn expand(&self, text: &str) -> String {
            let mut expanded = String::new();
            let mut rest = text;
            while let Some((before, after)) = rest.split_once('%') {
                match after.split_once('%') {
                    Some((name, tail)) => {
                        expanded.push_str(before);
                        // The Run dialog's variables match ignoring case.
                        if let Some(value) = self.variables.get(&name.to_ascii_lowercase()) {
                            expanded.push_str(value);
                        } else {
                            // A name no variable has stays as it is,
                            // spelled as typed, as Windows leaves it.
                            expanded.push('%');
                            expanded.push_str(name);
                            expanded.push('%');
                        }
                        rest = tail;
                    }
                    // A percent sign with no closing one is not a variable.
                    None => {
                        expanded.push_str(rest);
                        rest = "";
                    }
                }
            }
            expanded.push_str(rest);
            expanded
        }
    }

    fn split(line: &str, sources: &dyn Sources) -> Result<Split, RunError> {
        super::split(line, sources)
    }

    /// The ground that holds nothing: no file exists and no variable
    /// expands; the rules stand on it alone.
    fn nothing() -> Ground {
        Ground::new()
    }

    #[test]
    fn a_blank_line_is_refused() {
        let error = split("   ", &nothing()).unwrap_err();
        assert!(matches!(error, RunError::Failed(_)), "{error:?}");
        assert_eq!(
            error.message(),
            "no command was given: type a program, path or address"
        );
    }

    #[test]
    fn a_quoted_head_is_taken_as_written() {
        assert_eq!(
            split(r#""C:\Program Files\T\tool.exe" -a"#, &nothing()).unwrap(),
            Split {
                head: r"C:\Program Files\T\tool.exe".into(),
                arguments: "-a".into()
            }
        );
        // The head is quoted exactly; the arguments are the rest, and
        // both expand.
        let ground = Ground::new().variable("TEMP", r"C:\Users\v\AppData\Local\Temp");
        assert_eq!(
            split(r#""%TEMP%\T\tool.exe" %TEMP%\notes.txt"#, &ground).unwrap(),
            Split {
                head: r"C:\Users\v\AppData\Local\Temp\T\tool.exe".into(),
                arguments: r"C:\Users\v\AppData\Local\Temp\notes.txt".into()
            }
        );
        // One quote is a head to the end of the line.
        assert_eq!(
            split(r#""C:\My Tools\tool.exe"#, &nothing()).unwrap(),
            Split {
                head: r"C:\My Tools\tool.exe".into(),
                arguments: String::new()
            }
        );
    }

    #[test]
    fn a_head_ending_in_a_program_extension_splits_there() {
        // A bare name with a program extension.
        assert_eq!(
            split("notepad.exe -a", &nothing()).unwrap(),
            Split {
                head: "notepad.exe".into(),
                arguments: "-a".into()
            }
        );
        // A path without spaces, unquoted, is the head up to its
        // extension; an applet and a console are programs alike.
        assert_eq!(
            split(r"C:\Windows\System32\devmgmt.msc -a", &nothing()).unwrap(),
            Split {
                head: r"C:\Windows\System32\devmgmt.msc".into(),
                arguments: "-a".into()
            }
        );
        // Only the head's own ending counts, and the match ignores case.
        assert_eq!(
            split("TOOL.EXE -a", &nothing()).unwrap(),
            Split {
                head: "TOOL.EXE".into(),
                arguments: "-a".into()
            }
        );
        assert_eq!(
            split("tool.exe more.exe -a", &nothing()).unwrap(),
            Split {
                head: "tool.exe".into(),
                arguments: "more.exe -a".into()
            }
        );
    }

    #[test]
    fn a_rooted_path_with_spaces_takes_its_longest_existing_prefix() {
        let ground = Ground::new()
            .file(r"C:\My Documents\readme.txt")
            .variable("PROGRAMFILES", r"C:\Program Files");
        // The longest prefix that exists is the document; the rest is the
        // arguments.
        assert_eq!(
            split(r"C:\My Documents\readme.txt extra", &ground).unwrap(),
            Split {
                head: r"C:\My Documents\readme.txt".into(),
                arguments: "extra".into()
            }
        );
        // A program in a spaced path runs without quotes, extension or
        // not: the prefix that exists is the program.
        let ground = ground.file(r"C:\Program Files\Tool\tool.exe");
        assert_eq!(
            split(r"C:\Program Files\Tool\tool.exe -a", &ground).unwrap(),
            Split {
                head: r"C:\Program Files\Tool\tool.exe".into(),
                arguments: "-a".into()
            }
        );
        // Forward slashes are the same path on Windows — the file the
        // ground holds is found through them, its head kept as typed for
        // the run to normalize — and a variable that expands to a rooted
        // path is rooted too.
        let ground = ground.file(r"C:\Program Files\My Tool\tool.lnk");
        assert_eq!(
            split(r"%PROGRAMFILES%/My Tool/tool.lnk -a", &ground).unwrap(),
            Split {
                head: r"C:\Program Files/My Tool/tool.lnk".into(),
                arguments: "-a".into()
            }
        );
        // A network path is a rooted one.
        let network = Ground::new().file(r"\\server\my share\run.txt");
        assert_eq!(
            split(r"\\server\my share\run.txt -a", &network).unwrap(),
            Split {
                head: r"\\server\my share\run.txt".into(),
                arguments: "-a".into()
            }
        );
        // No prefix exists: the first space splits, as it would for a
        // word, and the run says the head does not exist.
        assert_eq!(
            split(r"C:\no such file.txt args", &ground).unwrap(),
            Split {
                head: r"C:\no".into(),
                arguments: r"such file.txt args".into()
            }
        );
    }

    #[test]
    fn otherwise_the_first_space_splits() {
        assert_eq!(
            split("mmc devmgmt.msc", &nothing()).unwrap(),
            Split {
                head: "mmc".into(),
                arguments: "devmgmt.msc".into()
            }
        );
        assert_eq!(
            split("shell:windows", &nothing()).unwrap(),
            Split {
                head: "shell:windows".into(),
                arguments: String::new()
            }
        );
        assert_eq!(
            split("https://example.com/a b", &nothing()).unwrap(),
            Split {
                head: "https://example.com/a".into(),
                arguments: "b".into()
            }
        );
        // Extra spaces are not arguments.
        assert_eq!(
            split("tool   -a", &nothing()).unwrap(),
            Split {
                head: "tool".into(),
                arguments: "-a".into()
            }
        );
    }

    #[test]
    fn environment_variables_expand_in_both_parts() {
        let ground = Ground::new().variable("TOOL", r"C:\Program Files\Tool");
        // A variable matches ignoring case.
        assert_eq!(
            split(r"%tool%\tool.exe -a", &ground).unwrap(),
            Split {
                head: r"C:\Program Files\Tool\tool.exe".into(),
                arguments: "-a".into()
            }
        );
        assert_eq!(
            split(r"tool.exe %TEMP%\notes.txt", &ground).unwrap(),
            Split {
                head: "tool.exe".into(),
                arguments: r"%TEMP%\notes.txt".into()
            },
            "a name no variable has stays as it is"
        );
        assert_eq!(
            split("tool.exe 100%", &ground).unwrap(),
            Split {
                head: "tool.exe".into(),
                arguments: "100%".into()
            },
            "a percent sign alone is not a variable"
        );
    }
}
