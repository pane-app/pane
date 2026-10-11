//! Explorer's RunMRU format: the history Windows' Run dialog keeps in the
//! registry under `HKEY_CURRENT_USER`, which Pane's Run shares with it in
//! both directions. The codec is plain text work, compiled and tested on
//! every system; only the registry half is Windows' (the adapter's).
//!
//! The key holds one value per entry, named by a letter `a` to `z`, its
//! data the command line as the user typed it followed by a marker byte
//! (U+0001); the `MRUList` value names the letters in the order the
//! dialog lists them, the newest first. At most 26 entries fit. When the
//! history is written, the letters name the entries in order — the
//! newest takes `a` — and any letter the key holds that no entry takes
//! is deleted, so the key never says more than the list does.

/// The marker Explorer writes after each entry's command line.
const MARKER: char = '\u{1}';

/// The letters the entries are named by, in the order the newest entry
/// takes first.
pub(crate) const LETTERS: [char; 26] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];

/// The entries the key's `values` (each named by one letter) and `list`
/// (its `MRUList`) hold, in the order `list` names them, the newest
/// first: Explorer's marker stripped from each, a letter no value holds
/// skipped, and duplicates removed ignoring case (the newest of them
/// kept).
pub fn decode(values: &[(String, String)], list: &str) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    for letter in list.chars() {
        let Some(data) = values
            .iter()
            .find(|(name, _)| name.len() == 1 && name.starts_with(letter))
            .map(|(_, data)| data)
        else {
            continue;
        };
        let entry = data.strip_suffix(MARKER).unwrap_or(data);
        if entry.is_empty() || entries.iter().any(|kept| kept.eq_ignore_ascii_case(entry)) {
            continue;
        }
        entries.push(entry.to_owned());
    }
    entries
}

/// `entries`, at most 26 and the newest first, as the key's lettered
/// values and its `MRUList`: the first entry takes `a`, the next `b`, and
/// so on. The caller deletes any letter the key holds that no entry
/// takes.
pub fn encode(entries: &[String]) -> (Vec<(String, String)>, String) {
    let mut values = Vec::new();
    let mut list = String::new();
    for (entry, letter) in entries.iter().zip(LETTERS) {
        values.push((letter.to_string(), format!("{entry}{MARKER}")));
        list.push(letter);
    }
    (values, list)
}

/// `entries` with `line` recorded: first, its case-insensitive duplicates
/// removed, at most 26 kept (the oldest beyond them dropped), as the Run
/// dialog records a command that ran.
pub fn record(entries: Vec<String>, line: &str) -> Vec<String> {
    let mut recorded = Vec::with_capacity(entries.len() + 1);
    recorded.push(line.to_owned());
    for entry in entries {
        if !entry.eq_ignore_ascii_case(line) {
            recorded.push(entry);
        }
    }
    recorded.truncate(LETTERS.len());
    recorded
}

/// `entries` without their case-insensitive matches of `line`, if the
/// history held one; `None` when it held none.
pub fn remove(entries: &[String], line: &str) -> Option<Vec<String>> {
    let removed: Vec<String> = entries
        .iter()
        .filter(|entry| !entry.eq_ignore_ascii_case(line))
        .cloned()
        .collect();
    (removed.len() != entries.len()).then_some(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values a key holds, from (letter, command line) pairs.
    fn values(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(letter, line)| ((*letter).to_owned(), (*line).to_owned()))
            .collect()
    }

    #[test]
    fn the_list_names_the_order_and_the_marker_is_stripped() {
        let key = values(&[
            ("a", "notepad\x01"),
            ("b", "cmd.exe\x01"),
            ("c", "calc\x01"),
        ]);
        // MRUList names the letters newest first, whatever letters they
        // are.
        assert_eq!(decode(&key, "cab"), ["calc", "notepad", "cmd.exe"]);
        // The marker is optional: a value without it is an entry too.
        let key = values(&[("a", "notepad"), ("b", "cmd.exe\x01")]);
        assert_eq!(decode(&key, "ba"), ["cmd.exe", "notepad"]);
    }

    #[test]
    fn a_letter_no_value_holds_or_an_empty_entry_is_skipped() {
        let key = values(&[("a", "notepad\x01"), ("b", "cmd.exe\x01")]);
        // `c` names no value; `d`'s is the marker alone, so no entry.
        let key = [key, values(&[("d", "\x01")])].concat();
        assert_eq!(decode(&key, "abcd"), ["notepad", "cmd.exe"]);
        // An empty list answers none.
        assert_eq!(decode(&key, ""), Vec::<String>::new());
    }

    #[test]
    fn duplicates_are_removed_ignoring_case() {
        let key = values(&[
            ("a", "Notepad\x01"),
            ("b", "cmd.exe\x01"),
            ("c", "NOTEPAD\x01"),
        ]);
        assert_eq!(decode(&key, "abc"), ["Notepad", "cmd.exe"]);
    }

    #[test]
    fn the_entries_take_the_letters_in_order() {
        let (values, list) = encode(&["notepad".into(), "cmd.exe".into()]);
        assert_eq!(list, "ab");
        assert_eq!(
            values,
            [
                ("a".to_owned(), "notepad\u{1}".to_owned()),
                ("b".to_owned(), "cmd.exe\u{1}".to_owned()),
            ]
        );
        // At most 26 entries fit; a round trip keeps them all.
        let many: Vec<String> = (0..30).map(|n| format!("run {n}")).collect();
        let (values, list) = encode(&many);
        assert_eq!(list.len(), 26);
        assert_eq!(decode(&values, &list), many[..26]);
    }

    #[test]
    fn recording_puts_the_line_first_without_its_duplicates() {
        let history = vec![
            "notepad".to_owned(),
            "cmd.exe".to_owned(),
            "calc".to_owned(),
        ];
        assert_eq!(record(history, "cmd.exe"), ["cmd.exe", "notepad", "calc"]);
        // A duplicate ignores case, and a new entry goes first.
        let history = vec!["notepad".to_owned(), "calc".to_owned()];
        assert_eq!(record(history, "NOTEPAD"), ["NOTEPAD", "calc"]);
        // At most 26 are kept; the oldest beyond them are dropped.
        let history: Vec<String> = (0..26).map(|n| format!("run {n}")).collect();
        let recorded = record(history, "new");
        assert_eq!(recorded.len(), 26);
        assert_eq!(recorded[0], "new");
        assert_eq!(recorded[1], "run 0");
        assert_eq!(recorded[25], "run 24", "run 25 is dropped");
    }

    #[test]
    fn removing_takes_its_case_insensitive_matches_or_nothing() {
        let history = vec!["notepad".to_owned(), "cmd.exe".to_owned()];
        assert_eq!(
            remove(&history, "CMD.EXE"),
            Some(vec!["notepad".to_owned()])
        );
        // An entry that is not in the history answers none.
        assert_eq!(remove(&history, "calc"), None);
        assert_eq!(remove(&[], "calc"), None);
    }
}
