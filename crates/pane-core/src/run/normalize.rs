//! How a rooted path is spelled as the system spells it. Plain text work
//! over a casing the caller asks for, compiled and tested on every
//! system; the Windows adapter asks the file system
//! ([`std::fs::canonicalize`], its verbatim prefix stripped).

use super::parse;

/// `path`, a rooted Windows path, as the system spells it: `/` separators
/// become `\`, the drive letter is uppercase, and each name's real casing
/// is `real_case`'s answer (which answers the path unchanged where the
/// file system has no such path, following links to where they point). A
/// network path (`\\server\share\…`) is kept one, its separators and
/// casing normalized the same way. A path that is not rooted is answered
/// as given.
pub fn normalize(path: &str, real_case: &dyn Fn(&str) -> String) -> String {
    if !parse::rooted(path) {
        return path.to_owned();
    }
    let backslashed = path.replace('/', "\\");
    let cased = real_case(&backslashed);
    // The drive letter is uppercase, however the file system spells it.
    let mut bytes = cased.into_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        bytes[0] = bytes[0].to_ascii_uppercase();
    }
    String::from_utf8(bytes).unwrap_or_else(|_| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A casing that spells the paths the file system is said to hold:
    /// the tests' fake of what the Windows adapter asks the disk.
    fn casing<'a>(files: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> String + 'a {
        move |path: &str| {
            files
                .iter()
                .find(|(given, _)| given.eq_ignore_ascii_case(path))
                .map(|(_, real)| (*real).to_owned())
                .unwrap_or_else(|| path.to_owned())
        }
    }

    #[test]
    fn separators_become_the_systems_own_and_the_drive_letter_uppercase() {
        let identity = casing(&[]);
        assert_eq!(
            normalize(r"c:/my folder/tool.exe", &identity),
            r"C:\my folder\tool.exe"
        );
        assert_eq!(normalize(r"\\server\share\", &identity), r"\\server\share\");
        assert_eq!(
            normalize(r"//server/share/file.txt", &identity),
            r"\\server\share\file.txt"
        );
        // A network path's server name is not a drive letter.
        assert_eq!(
            normalize(r"\\Server\Share\file.txt", &identity),
            r"\\Server\Share\file.txt"
        );
        // A drive on its own is a rooted path too.
        assert_eq!(normalize(r"c:\", &identity), r"C:\");
    }

    #[test]
    fn each_name_takes_its_real_casing() {
        let files = [
            (r"c:\my folder\tool.exe", r"C:\My Folder\Tool.exe"),
            (r"\\server\share\file.txt", r"\\Server\Share\file.txt"),
        ];
        let real = casing(&files);
        assert_eq!(
            normalize(r"c:\MY FOLDER\tool.exe", &real),
            r"C:\My Folder\Tool.exe"
        );
        // A path the file system does not hold keeps its casing.
        assert_eq!(
            normalize(r"c:\my folder\missing.txt", &real),
            r"C:\my folder\missing.txt"
        );
        // The real casing's drive letter is uppercased whatever it says.
        let files = [(r"c:\x", r"c:\X")];
        let real = casing(&files);
        assert_eq!(normalize(r"c:\x", &real), r"C:\X");
    }

    #[test]
    fn a_path_that_is_not_rooted_is_answered_as_given() {
        let identity = casing(&[]);
        assert_eq!(normalize("folder/tool.exe", &identity), "folder/tool.exe");
        assert_eq!(normalize("tool.exe", &identity), "tool.exe");
    }
}
