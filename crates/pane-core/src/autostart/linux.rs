//! Launch at login on Linux: the XDG autostart convention — a desktop
//! entry named `pane.desktop` in the configuration folder's `autostart`
//! directory (`$XDG_CONFIG_HOME/autostart`, or `~/.config/autostart`),
//! which a desktop environment that follows the convention starts at
//! login. Writing the entry is one file, replaced whole, so repeated
//! changes keep exactly one registration; removing Pane's start is
//! deleting the file. No root is needed: the folder is the user's own.
//!
//! The convention is a convention: the major desktop environments honor
//! it, but not every desktop does — a plain window manager has nothing
//! that reads it — and nothing reports back whether the entry was
//! started. The entry says what Pane did; the Settings page says the
//! convention's limit rather than promising every desktop behaves alike.
//!
//! The entry's `Exec` line is the program's own path, quoted the way the
//! desktop-entry specification quotes an executable with spaces in its
//! path, so a path with spaces stays one command and not several.

use std::path::{Path, PathBuf};

use super::{Autostart, Registration};
use crate::atomic::{Readers, write_atomically};

/// The adapter: the autostart entry, in the configuration folder the
/// XDG base-directory specification names.
pub struct XdgAutostart {
    /// The autostart entry's own path, Pane's one registration.
    entry: PathBuf,
    /// The program this Pane runs from, the one the entry starts.
    exe: PathBuf,
}

impl XdgAutostart {
    /// The adapter over the running program's own path and the user's
    /// configuration folder. `Err` with the problem when either cannot
    /// be found — there is then no entry to write and no command for it.
    pub fn new() -> Result<XdgAutostart, String> {
        let exe = std::env::current_exe()
            .map_err(|why| format!("Pane's own program could not be found: {why}"))?;
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".config"))
            })
            .ok_or_else(|| {
                "Pane could not find the configuration folder (neither XDG_CONFIG_HOME nor \
                 HOME names one)"
                    .to_owned()
            })?;
        let entry = config.join("autostart").join("pane.desktop");
        Ok(XdgAutostart { entry, exe })
    }
}

impl Autostart for XdgAutostart {
    fn unavailable(&self) -> Option<String> {
        // The constructor found the folder and the program; writing the
        // entry needs no permission the user's own folder does not give.
        None
    }

    fn registered(&self) -> Result<Registration, String> {
        match self.entry.try_exists() {
            Ok(true) => Ok(Registration::Enabled),
            Ok(false) => Ok(Registration::Disabled),
            Err(why) => Err(format!(
                "{} could not be asked about: {why}",
                self.entry.display()
            )),
        }
    }

    fn enable(&self) -> Result<Registration, String> {
        let text = entry(&self.exe)?;
        write_atomically(&self.entry, text.as_bytes(), Readers::Default)
            .map_err(|why| format!("{} could not be written: {why}", self.entry.display()))?;
        Ok(Registration::Enabled)
    }

    fn disable(&self) -> Result<Registration, String> {
        match std::fs::remove_file(&self.entry) {
            Ok(()) => Ok(Registration::Disabled),
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(Registration::Disabled),
            Err(why) => Err(format!(
                "{} could not be removed: {why}",
                self.entry.display()
            )),
        }
    }
}

/// The autostart entry's whole text, starting the program at `exe`. The
/// entry is the same shape as the one Pane's Linux package installs for
/// its application menu (`Name=Pane`, no terminal), with the `Exec` line
/// the absolute path of the program actually running: an autostart entry
/// runs before the user's environment is assembled, so a bare program
/// name found through `PATH` is not something to rely on.
fn entry(exe: &Path) -> Result<String, String> {
    let path = exe
        .to_str()
        .ok_or_else(|| "Pane's own program path is not text a desktop entry can hold".to_owned())?;
    Ok(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Pane\n\
         Exec={}\n\
         Terminal=false\n",
        command(path)
    ))
}

/// The `Exec` line's command for the program at `path`: the path itself,
/// quoted as the desktop-entry specification quotes a command with a
/// reserved character in it — spaces among them — so a path with spaces
/// stays one command. Inside the quotes a backslash, a dollar or a
/// backquote is escaped, as the specification requires.
fn command(path: &str) -> String {
    let reserved = |character: char| {
        matches!(
            character,
            ' ' | '\t'
                | '\n'
                | '"'
                | '\''
                | '>'
                | '<'
                | '|'
                | '~'
                | '$'
                | '&'
                | ';'
                | '*'
                | '?'
                | '#'
                | '('
                | ')'
                | '`'
        )
    };
    if !path.chars().any(reserved) {
        return path.to_owned();
    }
    let mut quoted = String::from("\"");
    for character in path.chars() {
        if matches!(character, '\\' | '$' | '`') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{command, entry};

    /// A plain path is written bare.
    #[test]
    fn a_plain_path_needs_no_quotes() {
        assert_eq!(
            command("/home/vu/.local/bin/pane"),
            "/home/vu/.local/bin/pane"
        );
    }

    /// A path with spaces is one quoted command, not several.
    #[test]
    fn a_path_with_spaces_is_quoted() {
        assert_eq!(command("/opt/my apps/pane"), r#""/opt/my apps/pane""#);
    }

    /// Inside the quotes, the characters the specification escapes are
    /// escaped; the other reserved characters are safe there.
    #[test]
    fn a_quoted_path_escapes_what_the_specification_escapes() {
        assert_eq!(
            command("/home/vu/mo$ey`\\pane app"),
            r#""/home/vu/mo\$ey\`\\pane app""#
        );
    }

    /// The entry is the shape the package installs, with the absolute
    /// program path as its command.
    #[test]
    fn the_entry_is_a_desktop_entry_starting_the_program() {
        let text = entry(Path::new("/home/vu/.local/bin/pane")).unwrap();
        assert_eq!(
            text,
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Pane\n\
             Exec=/home/vu/.local/bin/pane\n\
             Terminal=false\n"
        );
    }
}
