//! Linux: the icon a desktop entry names, found by the freedesktop Icon
//! Theme specification. Plain file system work, compiled and tested on
//! every system with fixture themes.
//!
//! An entry's `Icon` is a file when it is an absolute path; otherwise it is
//! a name looked up in the user's current icon theme (GTK's
//! `gtk-icon-theme-name`, else KDE's `[Icons] Theme`), the themes it
//! inherits (`Inherits` in its `index.theme`), then `hicolor`, in each base
//! folder (`~/.icons`, then `icons` in each data folder), and finally in
//! the legacy `pixmaps` folders. Within a theme, an SVG of a scalable
//! folder, or the PNG whose size is closest to 256 pixels (the smallest at
//! least that large, else the largest), is chosen.

use std::path::{Path, PathBuf};

/// The size, in pixels, an icon is chosen closest to.
const WANTED: u32 = crate::system_icons::ICON_SIZE;

/// How many themes an inheritance chain follows at most.
const MAX_THEMES: usize = 12;

/// The `Icon` of a desktop entry's `[Desktop Entry]` group in `text`.
pub fn entry_icon(text: &str) -> Option<String> {
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

/// Where icon themes and pixmaps are, and the user's theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconThemes {
    /// The folders themes are in, the first preferred.
    bases: Vec<PathBuf>,
    /// The legacy folders of icons outside any theme.
    pixmaps: Vec<PathBuf>,
    /// The user's current theme, if one is set.
    theme: Option<String>,
}

impl IconThemes {
    /// The themes in `home`'s `.icons` and the `icons` of `data_folders`
    /// (the user's first), with the `pixmaps` of `data_folders`, the user's
    /// current theme being `theme`.
    pub fn new(data_folders: &[PathBuf], home: Option<&Path>, theme: Option<String>) -> IconThemes {
        let mut bases: Vec<PathBuf> = home.iter().map(|home| home.join(".icons")).collect();
        bases.extend(data_folders.iter().map(|folder| folder.join("icons")));
        IconThemes {
            bases,
            pixmaps: data_folders
                .iter()
                .map(|folder| folder.join("pixmaps"))
                .collect(),
            theme: theme.filter(|theme| !theme.trim().is_empty()),
        }
    }

    /// The themes of the environment: the data folders of
    /// `$XDG_DATA_HOME` (default `~/.local/share`) and `$XDG_DATA_DIRS`
    /// (default `/usr/local/share:/usr/share`), and the theme the user's
    /// configuration (`$XDG_CONFIG_HOME`, default `~/.config`) names.
    pub fn from_env() -> IconThemes {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let home = var("HOME").map(PathBuf::from);
        let mut folders = Vec::new();
        match var("XDG_DATA_HOME") {
            Some(dir) => folders.push(PathBuf::from(dir)),
            None => folders.extend(home.as_ref().map(|home| home.join(".local/share"))),
        }
        let shared = var("XDG_DATA_DIRS")
            .map(|dirs| dirs.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
        folders.extend(
            shared
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from),
        );
        let config = var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|home| home.join(".config")));
        let theme = config.as_deref().and_then(current_theme);
        IconThemes::new(&folders, home.as_deref(), theme)
    }

    /// The file of the icon `name`: an absolute path as it is (if it is a
    /// file), else the best file of the user's theme, its parents,
    /// `hicolor`, then `pixmaps`. A name given with an image extension
    /// (`firefox.png`, which the specification allows for old entries) is
    /// looked up without it.
    pub fn find(&self, name: &str) -> Option<PathBuf> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        let path = Path::new(name);
        if path.is_absolute() {
            return path.is_file().then(|| path.to_path_buf());
        }
        let name = ["png", "svg", "xpm"]
            .iter()
            .find_map(|extension| name.strip_suffix(&format!(".{extension}")))
            .unwrap_or(name);
        for theme in self.chain() {
            if let Some(found) = self.in_theme(&theme, name) {
                return Some(found);
            }
        }
        self.pixmaps
            .iter()
            .flat_map(|folder| {
                ["png", "svg"].map(|extension| folder.join(format!("{name}.{extension}")))
            })
            .find(|file| file.is_file())
    }

    /// The themes looked in, in order: the user's, those it inherits
    /// (depth first, each once), then `hicolor`.
    fn chain(&self) -> Vec<String> {
        let mut chain: Vec<String> = Vec::new();
        let mut pending: Vec<String> = self.theme.iter().cloned().collect();
        while let Some(theme) = pending.pop() {
            if chain.contains(&theme) || chain.len() >= MAX_THEMES {
                continue;
            }
            let parents = self.index(&theme).map_or_else(Vec::new, |index| {
                key(&index, "Icon Theme", "Inherits")
                    .map(|inherits| {
                        inherits
                            .split(',')
                            .map(str::trim)
                            .filter(|parent| !parent.is_empty())
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default()
            });
            chain.push(theme);
            // Depth first, in the order listed.
            pending.extend(parents.into_iter().rev());
        }
        if !chain.iter().any(|theme| theme == "hicolor") {
            chain.push("hicolor".into());
        }
        chain
    }

    /// The `index.theme` of `theme`, from the first base folder having one.
    fn index(&self, theme: &str) -> Option<String> {
        self.bases
            .iter()
            .find_map(|base| std::fs::read_to_string(base.join(theme).join("index.theme")).ok())
    }

    /// The best file of the icon `name` in `theme`.
    fn in_theme(&self, theme: &str, name: &str) -> Option<PathBuf> {
        let folders = self.folders(theme);
        let mut best: Option<((u8, u32), PathBuf)> = None;
        for base in &self.bases {
            let root = base.join(theme);
            if !root.is_dir() {
                continue;
            }
            for (folder, size, scalable) in &folders {
                for extension in ["svg", "png"] {
                    let file = root.join(folder).join(format!("{name}.{extension}"));
                    if !file.is_file() {
                        continue;
                    }
                    let rank = if extension == "svg" && *scalable {
                        (0, 0)
                    } else if *size >= WANTED {
                        (0, size - WANTED + 1)
                    } else {
                        (1, WANTED - size)
                    };
                    if best.as_ref().is_none_or(|(known, _)| rank < *known) {
                        best = Some((rank, file));
                    }
                }
            }
        }
        best.map(|(_, file)| file)
    }

    /// The folders of `theme` with the size of their icons and whether
    /// they are scalable: those its `index.theme` lists (`Directories`,
    /// with each one's `Size`, `Scale` and `Type`), or, without one, the
    /// folders two deep named by a size (`256x256/apps`, `scalable/apps`,
    /// `apps/48`).
    fn folders(&self, theme: &str) -> Vec<(String, u32, bool)> {
        if let Some(index) = self.index(theme) {
            let listed = key(&index, "Icon Theme", "Directories").unwrap_or_default();
            let scaled = key(&index, "Icon Theme", "ScaledDirectories").unwrap_or_default();
            return listed
                .split(',')
                .chain(scaled.split(','))
                .map(str::trim)
                .filter(|folder| !folder.is_empty())
                .map(|folder| {
                    let number = |name: &str| {
                        key(&index, folder, name).and_then(|value| value.trim().parse::<u32>().ok())
                    };
                    let size = number("Size").unwrap_or(0) * number("Scale").unwrap_or(1);
                    let scalable = key(&index, folder, "Type")
                        .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("scalable"));
                    (folder.to_owned(), size, scalable)
                })
                .collect();
        }
        let mut folders = Vec::new();
        for base in &self.bases {
            let Ok(entries) = std::fs::read_dir(base.join(theme)) else {
                continue;
            };
            for outer in entries.filter_map(Result::ok) {
                let outer_name = outer.file_name().to_string_lossy().into_owned();
                let Ok(inner) = std::fs::read_dir(outer.path()) else {
                    continue;
                };
                for inner in inner.filter_map(Result::ok) {
                    let inner_name = inner.file_name().to_string_lossy().into_owned();
                    let folder = format!("{outer_name}/{inner_name}");
                    let sized = folder_size(&outer_name).or_else(|| folder_size(&inner_name));
                    if let Some((size, scalable)) = sized
                        && !folders.iter().any(|(known, _, _)| *known == folder)
                    {
                        folders.push((folder, size, scalable));
                    }
                }
            }
        }
        folders
    }
}

/// The size a theme folder's name gives: `256x256` (or `256x256@2`),
/// `48`, or `scalable`.
fn folder_size(name: &str) -> Option<(u32, bool)> {
    if name == "scalable" {
        return Some((WANTED, true));
    }
    let (size, scale) = match name.split_once('@') {
        Some((size, scale)) => (size, scale.parse::<u32>().ok()?),
        None => (name, 1),
    };
    let side = size.split_once('x').map_or(size, |(side, _)| side);
    side.parse::<u32>().ok().map(|side| (side * scale, false))
}

/// The value of `name` in the group `[group]` of the key file `text`.
fn key(text: &str, group: &str, name: &str) -> Option<String> {
    let header = format!("[{group}]");
    let mut in_group = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == header;
            continue;
        }
        if in_group
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == name
        {
            return Some(value.trim().to_owned());
        }
    }
    None
}

/// The icon theme the user's configuration in `config` names: GTK 4's or
/// GTK 3's `gtk-icon-theme-name`, else KDE's `[Icons] Theme`.
pub fn current_theme(config: &Path) -> Option<String> {
    for settings in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
        if let Ok(text) = std::fs::read_to_string(config.join(settings))
            && let Some(theme) = key(&text, "Settings", "gtk-icon-theme-name")
        {
            let theme = theme.trim_matches('"').to_owned();
            if !theme.is_empty() {
                return Some(theme);
            }
        }
    }
    let kde = std::fs::read_to_string(config.join("kdeglobals")).ok()?;
    key(&kde, "Icons", "Theme").filter(|theme| !theme.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn a_desktop_entry_names_its_icon() {
        assert_eq!(
            entry_icon(
                "[Desktop Action New]\nIcon=wrong\n[Desktop Entry]\nName=Editor\nIcon = editor\n"
            )
            .as_deref(),
            Some("editor")
        );
        assert_eq!(entry_icon("[Desktop Entry]\nName=Editor\n"), None);
    }

    #[test]
    fn the_users_theme_then_its_parents_then_hicolor_then_pixmaps() {
        let folder = tempfile::tempdir().unwrap();
        let data = folder.path().join("share");
        let icons = data.join("icons");
        write(
            &icons.join("Mine/index.theme"),
            "[Icon Theme]\nName=Mine\nInherits=Parent\nDirectories=48x48/apps\n\n\
             [48x48/apps]\nSize=48\nType=Fixed\n",
        );
        write(
            &icons.join("Parent/index.theme"),
            "[Icon Theme]\nName=Parent\nDirectories=scalable/apps\n\n\
             [scalable/apps]\nSize=64\nType=Scalable\n",
        );
        write(
            &icons.join("hicolor/index.theme"),
            "[Icon Theme]\nName=Hicolor\nDirectories=32x32/apps,256x256/apps,512x512/apps\n\n\
             [32x32/apps]\nSize=32\n[256x256/apps]\nSize=256\n[512x512/apps]\nSize=512\n",
        );
        write(&icons.join("Mine/48x48/apps/editor.png"), "png");
        write(&icons.join("Parent/scalable/apps/editor.svg"), "svg");
        write(&icons.join("Parent/scalable/apps/terminal.svg"), "svg");
        for size in ["32x32", "256x256", "512x512"] {
            write(
                &icons.join(format!("hicolor/{size}/apps/viewer.png")),
                "png",
            );
        }
        write(&data.join("pixmaps/legacy.png"), "png");

        let themes = IconThemes::new(std::slice::from_ref(&data), None, Some("Mine".into()));
        // The user's theme has it.
        assert_eq!(
            themes.find("editor"),
            Some(icons.join("Mine/48x48/apps/editor.png"))
        );
        // Its parent does.
        assert_eq!(
            themes.find("terminal"),
            Some(icons.join("Parent/scalable/apps/terminal.svg"))
        );
        // hicolor, at the size closest to 256.
        assert_eq!(
            themes.find("viewer.png"),
            Some(icons.join("hicolor/256x256/apps/viewer.png"))
        );
        assert_eq!(themes.find("legacy"), Some(data.join("pixmaps/legacy.png")));
        assert_eq!(themes.find("missing"), None);
        // An absolute path is the file.
        let file = data.join("pixmaps/legacy.png");
        assert_eq!(themes.find(&file.to_string_lossy()), Some(file));
        // Without a theme of the user's, hicolor alone.
        let plain = IconThemes::new(std::slice::from_ref(&data), None, None);
        assert_eq!(plain.find("editor"), None);
        assert!(plain.find("viewer").is_some());
    }

    #[test]
    fn a_theme_without_an_index_is_read_by_its_folders_names() {
        let folder = tempfile::tempdir().unwrap();
        let data = folder.path().join("share");
        write(&data.join("icons/hicolor/48x48/apps/tool.png"), "png");
        write(&data.join("icons/hicolor/128x128/apps/tool.png"), "png");
        let themes = IconThemes::new(std::slice::from_ref(&data), None, None);
        assert_eq!(
            themes.find("tool"),
            Some(data.join("icons/hicolor/128x128/apps/tool.png"))
        );
        assert_eq!(folder_size("256x256@2"), Some((512, false)));
        assert_eq!(folder_size("scalable"), Some((256, true)));
        assert_eq!(folder_size("apps"), None);
    }

    #[test]
    fn the_users_theme_is_gtks_else_kdes() {
        let folder = tempfile::tempdir().unwrap();
        let config = folder.path();
        assert_eq!(current_theme(config), None);
        write(
            &config.join("kdeglobals"),
            "[General]\nName=x\n[Icons]\nTheme=breeze-dark\n",
        );
        assert_eq!(current_theme(config).as_deref(), Some("breeze-dark"));
        write(
            &config.join("gtk-3.0/settings.ini"),
            "[Settings]\ngtk-icon-theme-name=Papirus\n",
        );
        assert_eq!(current_theme(config).as_deref(), Some("Papirus"));
    }
}
