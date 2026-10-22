//! The completions for the text typed in Run's field (#264, ADR 0040):
//! what the Run dialog's own autocompletion offers, gathered from the
//! sources the spec names — the Run dialog's history first, then programs
//! from App Paths and the registry search path, Control Panel applets,
//! management consoles, registered URL schemes and environment variables.
//! Plain text work, compiled and tested on every system: the adapter
//! gathers the candidates ([`Candidates`]) and this module matches them
//! against the typed text, so the matching is the same everywhere and the
//! Windows half is only the gathering.
//!
//! A completion is a command line the Run dialog runs as it is: a history
//! entry runs again, a program's name resolves on the search path and in
//! App Paths, an applet's and a console's file names keep their extension
//! (`desk.cpl`, `devmgmt.msc`), a scheme's address ends in its `:`, an
//! environment variable's line is `%NAME%`. Each matches the typed text
//! when the line starts with it, ignoring case, as the Run dialog's own
//! autocompletion matches; a line two sources offer is offered once, from
//! the source that came first.

use std::collections::HashSet;

/// The most completions one call answers, so a broad text (one letter)
/// does not list a whole system folder: typing more of the text narrows
/// the same sources. Provisional, as the list limits are.
pub const MAX_COMPLETIONS: usize = 100;

/// Which source a completion came from, as its subtitle says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The Run dialog's history, newest first.
    History,
    /// A program App Paths registered for its name.
    AppPath,
    /// A program on the registry search path.
    SearchPath,
    /// A Control Panel applet.
    Applet,
    /// A management console.
    Console,
    /// A registered scheme.
    Scheme,
    /// An environment variable.
    Variable,
}

/// One completion for the text typed so far: the command line the text
/// completes to, and which source it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// The command line as it would be typed next: what runs.
    pub line: String,
    /// Which source offered it.
    pub source: Source,
}

/// The candidates the sources offer, before matching, as the adapter
/// gathered them at the time of the call: the history newest first, and
/// each other group's lines as its source lists them (the Windows
/// adapter sorts its own groups, so its answers are the same every time).
/// Each entry is already a line the Run dialog runs as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Candidates {
    /// The Run dialog's history, newest first, as the user typed it.
    pub history: Vec<String>,
    /// Programs App Paths registered, by their names.
    pub app_paths: Vec<String>,
    /// Programs on the registry search path, by their names.
    pub programs: Vec<String>,
    /// Control Panel applets, by their file names.
    pub applets: Vec<String>,
    /// Management consoles, by their file names.
    pub consoles: Vec<String>,
    /// Registered schemes, by their addresses (`ms-settings:`, …).
    pub schemes: Vec<String>,
    /// Environment variables, by their `%NAME%` lines.
    pub variables: Vec<String>,
}

/// The completions for `text`, in the order they are offered (see the
/// module's): each candidate whose line starts with the typed text,
/// ignoring case, from the history first, then App Paths' programs, the
/// search path's programs, applets, consoles, schemes and environment
/// variables; a line two sources offer is offered once, from the source
/// that came first; at most [`MAX_COMPLETIONS`] are answered. Blank text
/// is answered with nothing: a blank field shows the command's own list.
pub fn complete(candidates: &Candidates, text: &str) -> Vec<Completion> {
    let text = text.trim().to_lowercase();
    if text.is_empty() {
        return Vec::new();
    }
    let groups = [
        (Source::History, &candidates.history),
        (Source::AppPath, &candidates.app_paths),
        (Source::SearchPath, &candidates.programs),
        (Source::Applet, &candidates.applets),
        (Source::Console, &candidates.consoles),
        (Source::Scheme, &candidates.schemes),
        (Source::Variable, &candidates.variables),
    ];
    let mut seen: HashSet<String> = HashSet::new();
    let mut completions: Vec<Completion> = Vec::new();
    for (source, lines) in groups {
        for offered in lines {
            let line = offered.trim();
            if line.is_empty() || !line.to_lowercase().starts_with(&text) {
                continue;
            }
            // A line two sources offers is offered once, matched ignoring
            // case, from the source that came first.
            if !seen.insert(line.to_lowercase()) {
                continue;
            }
            completions.push(Completion {
                line: line.to_owned(),
                source,
            });
            if completions.len() == MAX_COMPLETIONS {
                return completions;
            }
        }
    }
    completions
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Candidates holding one line of every source, in the order the
    /// sources are offered.
    fn every_source() -> Candidates {
        Candidates {
            history: vec!["notepad -a".into()],
            app_paths: vec!["winget".into()],
            programs: vec!["mspaint".into()],
            applets: vec!["desk.cpl".into()],
            consoles: vec!["devmgmt.msc".into()],
            schemes: vec!["ms-settings:".into()],
            variables: vec!["%TEMP%".into()],
        }
    }

    #[test]
    fn each_source_completes_the_text_it_matches() {
        let candidates = every_source();
        let source = |text: &str| {
            complete(&candidates, text)
                .into_iter()
                .map(|completion| (completion.line, completion.source))
                .collect::<Vec<_>>()
        };
        // The history's entry matches its head, as typed with arguments.
        assert_eq!(source("not"), [("notepad -a".into(), Source::History)]);
        // The typed text matches ignoring case.
        assert_eq!(source("NOTEPAD"), [("notepad -a".into(), Source::History)]);
        assert_eq!(source("Win"), [("winget".into(), Source::AppPath)]);
        assert_eq!(source("mspaint"), [("mspaint".into(), Source::SearchPath)]);
        assert_eq!(source("des"), [("desk.cpl".into(), Source::Applet)]);
        assert_eq!(source("dev"), [("devmgmt.msc".into(), Source::Console)]);
        assert_eq!(source("ms-s"), [("ms-settings:".into(), Source::Scheme)]);
        assert_eq!(source("%te"), [("%TEMP%".into(), Source::Variable)]);
        // A text no line starts with is answered with nothing.
        assert!(source("nothing by this").is_empty());
    }

    #[test]
    fn the_sources_are_offered_in_their_order() {
        let candidates = Candidates {
            history: vec!["tool".into()],
            app_paths: vec!["tool-app".into()],
            programs: vec!["tool-path".into()],
            applets: vec!["tool.cpl".into()],
            consoles: vec!["tool.msc".into()],
            schemes: vec!["tool:".into()],
            variables: vec!["%TOOL%".into()],
        };
        let offered = complete(&candidates, "tool");
        assert_eq!(
            offered
                .iter()
                .map(|completion| completion.source)
                .collect::<Vec<_>>(),
            [
                Source::History,
                Source::AppPath,
                Source::SearchPath,
                Source::Applet,
                Source::Console,
                Source::Scheme,
            ]
        );
        // A variable's line is `%NAME%`, so it completes the text once
        // the `%` is typed, never the plain name — and a history entry
        // the user ran can start with one too, as a line the Run dialog
        // expands, so the two keep their order.
        let percent = Candidates {
            history: vec!["%tool -a".into()],
            variables: vec!["%TOOL%".into()],
            ..Candidates::default()
        };
        assert_eq!(
            complete(&percent, "%to")
                .iter()
                .map(|completion| completion.source)
                .collect::<Vec<_>>(),
            [Source::History, Source::Variable]
        );
        // The history's own order, newest first, is kept within it.
        let history = Candidates {
            history: vec!["fresh".into(), "older".into()],
            ..Candidates::default()
        };
        assert_eq!(
            complete(&history, "o")
                .iter()
                .map(|c| c.line.as_str())
                .collect::<Vec<_>>(),
            ["older"]
        );
        assert_eq!(
            complete(&history, "f")
                .iter()
                .map(|c| c.line.as_str())
                .collect::<Vec<_>>(),
            ["fresh"]
        );
    }

    #[test]
    fn a_line_two_sources_offer_is_offered_once() {
        let candidates = Candidates {
            history: vec!["notepad".into()],
            programs: vec!["Notepad".into(), "notepad++".into()],
            ..Candidates::default()
        };
        // The history's entry wins, matched ignoring case, and the search
        // path's duplicate is left out while its other offer stays.
        assert_eq!(
            complete(&candidates, "notepad")
                .into_iter()
                .map(|completion| (completion.line, completion.source))
                .collect::<Vec<_>>(),
            [
                ("notepad".into(), Source::History),
                ("notepad++".into(), Source::SearchPath),
            ]
        );
    }

    #[test]
    fn blank_text_is_answered_with_nothing() {
        assert!(complete(&every_source(), "").is_empty());
        assert!(complete(&every_source(), "   ").is_empty());
    }

    #[test]
    fn at_most_max_completions_are_answered() {
        let candidates = Candidates {
            programs: (0..MAX_COMPLETIONS + 10)
                .map(|number| format!("tool{number}"))
                .collect(),
            ..Candidates::default()
        };
        let offered = complete(&candidates, "tool");
        assert_eq!(offered.len(), MAX_COMPLETIONS);
        assert_eq!(offered.first().map(|c| c.line.as_str()), Some("tool0"));
        let cut = format!("tool{}", MAX_COMPLETIONS - 1);
        assert_eq!(offered.last().map(|c| c.line.as_str()), Some(cut.as_str()));
    }

    #[test]
    fn a_line_is_completed_as_it_is_offered_trimmed() {
        // A candidate with space around it is offered trimmed, and an
        // empty one is not offered at all.
        let candidates = Candidates {
            history: vec!["  spaced  ".into(), "  ".into()],
            ..Candidates::default()
        };
        assert_eq!(
            complete(&candidates, "sp")
                .into_iter()
                .map(|completion| completion.line)
                .collect::<Vec<_>>(),
            ["spaced"]
        );
    }
}
