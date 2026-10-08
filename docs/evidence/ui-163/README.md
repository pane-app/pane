# #163 evidence: the default extensions' tiles on Windows

Captured on 2026-10-08 from a release build of `main` at `ff9ad18d`, built with debug assertions so that it takes its default extensions from a local artifact source (`cargo xtask package-windows`, served by `scripts/artifact_server.py`), plus this branch's fix to the Actions panel's header. Windows 11 build 26200, 1920×1080 displays, opaque material, a scratch data and cache folder. Each image is Pane's own window only (`PrintWindow`). The Actions panel images are cropped to the panel, leaving out the file rows behind it.

**Scales:** 1x is 96 DPI. 150 % (144 DPI) and 175 % (168 DPI) were set on the display Pane opened on, then put back. Windows offers no 200 % on a 1080p display. The tiles are SVGs drawn at the row tile's size, and `default_icons.rs` checks their size and placement.

**Files** (`<scale>-<theme>-<surface>.png`):

- `root`: root search with a blank query, the default extensions' commands.
- `pins`: the pinned home, which holds four default commands and three applications.
- `actions`: the Actions panel on a default command. Before this branch, its header drew Pane's generic command glyph instead of the command's tile.
- `settings`: Settings, with the Extensions group's sidebar entries.
- `shortcuts`: Settings › Shortcuts.

At 175 % the dark Settings capture opened the launcher instead of Settings, so it is not included. The 175 % light and 150 % dark Settings captures cover that surface.
