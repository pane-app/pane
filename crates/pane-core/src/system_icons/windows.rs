//! System icons on Windows: the shell draws the item at a path (a
//! document, a folder, a program, a shortcut, or a packaged application by
//! its `shell:AppsFolder\<id>`) as Explorer shows it, icon only, through
//! `IShellItemImageFactory` (`windows_shell::shell_image`, which
//! application icons use too); Pane keeps its pixels as a PNG.

use std::path::Path;

use super::SystemIcon;
use crate::windows_shell::{Com, shell_image};

pub(super) fn icon(path: &Path) -> Result<SystemIcon, String> {
    let _com = Com::new()?;
    let image = shell_image(path)?;
    crate::icons::encode_png(image.width, image.height, &image.rgba)
        .map(SystemIcon::Png)
        .ok_or_else(|| format!("no icon for {}: its pixels make no image", path.display()))
}
