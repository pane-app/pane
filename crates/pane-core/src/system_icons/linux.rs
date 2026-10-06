//! System icons on Linux: the icon theme's file. A desktop entry names its
//! application's icon (`Icon=`: a file, or a name looked up in the
//! themes); a folder is `folder`; another file is its kind's generic icon
//! (`text-x-generic`, `image-x-generic`, …). Names are looked up in the
//! `hicolor`, Adwaita and Breeze themes of the data folders
//! (`$XDG_DATA_HOME`, `$XDG_DATA_DIRS`, `~/.icons`), largest first, and in
//! `pixmaps`.

use std::path::{Path, PathBuf};

use super::SystemIcon;

/// The themes looked in, in order.
const THEMES: [&str; 3] = ["hicolor", "Adwaita", "breeze"];

/// The sizes looked for, best first.
const SIZES: [&str; 9] = [
    "scalable", "256x256", "512x512", "128x128", "96x96", "64x64", "48x48", "32x32", "symbolic",
];

/// The contexts of a theme an icon may be in.
const CONTEXTS: [&str; 5] = ["apps", "places", "mimetypes", "devices", "mimes"];

pub(super) fn icon(path: &Path) -> Result<SystemIcon, String> {
    let none = || format!("the icon theme has no icon for {}", path.display());
    let names = if path.is_dir() {
        vec!["folder".to_owned(), "inode-directory".to_owned()]
    } else if path
        .extension()
        .is_some_and(|extension| extension == "desktop")
    {
        let named = desktop_icon(path).ok_or_else(none)?;
        let file = Path::new(&named);
        if file.is_absolute() {
            return file
                .is_file()
                .then(|| SystemIcon::File(file.to_path_buf()))
                .ok_or_else(none);
        }
        vec![named]
    } else {
        kind_names(path)
    };
    names
        .iter()
        .find_map(|name| themed(name))
        .map(SystemIcon::File)
        .ok_or_else(none)
}

/// The `Icon` a desktop entry at `path` names, in its `[Desktop Entry]`
/// group.
fn desktop_icon(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry
            && let Some(value) = line.strip_prefix("Icon")
            && let Some(value) = value.trim_start().strip_prefix('=')
        {
            let value = value.trim();
            return (!value.is_empty()).then(|| value.to_owned());
        }
    }
    None
}

/// The generic icon names of the file at `path`, by its extension, most
/// specific first.
fn kind_names(path: &Path) -> Vec<String> {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let kind = match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "tif" | "tiff" => {
            "image-x-generic"
        }
        "mp3" | "ogg" | "flac" | "wav" | "m4a" | "opus" => "audio-x-generic",
        "mp4" | "mkv" | "webm" | "mov" | "avi" => "video-x-generic",
        "pdf" => "application-pdf",
        "zip" | "tar" | "gz" | "xz" | "bz2" | "zst" | "7z" => "package-x-generic",
        "html" | "htm" => "text-html",
        _ if is_executable(path) => "application-x-executable",
        _ => "text-x-generic",
    };
    vec![kind.to_owned(), "text-x-generic".to_owned()]
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

/// The folders icon themes and pixmaps are in.
fn data_folders() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from);
    let mut folders = Vec::new();
    match std::env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => folders.push(PathBuf::from(dir)),
        None => folders.extend(home.as_ref().map(|home| home.join(".local/share"))),
    }
    let shared = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    folders.extend(
        shared
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from),
    );
    folders
}

/// The file of the icon `name` in the themes or pixmaps, if one has it.
fn themed(name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from);
    let mut bases: Vec<PathBuf> = home.iter().map(|home| home.join(".icons")).collect();
    let folders = data_folders();
    bases.extend(folders.iter().map(|folder| folder.join("icons")));
    for theme in THEMES {
        for base in &bases {
            let theme = base.join(theme);
            if !theme.is_dir() {
                continue;
            }
            for size in SIZES {
                for context in CONTEXTS {
                    for extension in ["svg", "png"] {
                        let file = theme
                            .join(size)
                            .join(context)
                            .join(format!("{name}.{extension}"));
                        if file.is_file() {
                            return Some(file);
                        }
                    }
                }
            }
        }
    }
    folders
        .iter()
        .map(|folder| folder.join("pixmaps"))
        .chain(bases.iter().cloned())
        .flat_map(|folder| {
            ["png", "svg"].map(|extension| folder.join(format!("{name}.{extension}")))
        })
        .find(|file| file.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_entry_names_its_icon() {
        let folder = tempfile::tempdir().unwrap();
        let entry = folder.path().join("editor.desktop");
        std::fs::write(
            &entry,
            "[Desktop Action New]\nIcon=wrong\n[Desktop Entry]\nName=Editor\nIcon = editor\n",
        )
        .unwrap();
        assert_eq!(desktop_icon(&entry).as_deref(), Some("editor"));
        // An icon by file is that file.
        let image = folder.path().join("editor.png");
        std::fs::write(&image, b"x").unwrap();
        std::fs::write(
            &entry,
            format!("[Desktop Entry]\nIcon={}\n", image.display()),
        )
        .unwrap();
        assert_eq!(icon(&entry), Ok(SystemIcon::File(image)));
    }

    #[test]
    fn a_file_is_named_by_its_kind() {
        assert_eq!(kind_names(Path::new("/a/b.PNG"))[0], "image-x-generic");
        assert_eq!(kind_names(Path::new("/a/b.txt"))[0], "text-x-generic");
    }
}
