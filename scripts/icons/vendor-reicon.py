"""Vendors the reicon icon library (https://reicon.dev, MIT): Pane's own
glyphs as standalone SVG files in crates/pane/assets/icons/reicon/, one per
Pane glyph, named as `crates/pane/src/ui/icon.rs` asks for them, and the
whole set, every icon in both weights, in crates/pane-core/assets/reicon/,
which extensions draw from by name (#139; `crates/pane-core/src/icons.rs`).

reicon ships each icon as a JS module, `icons/<Name>.js`, whose
`createIcon('<Name>', { O: `...`, F: `...` })` call holds the inner SVG
markup of its Outline (O) and Filled (F) weights on a 24x24 viewBox. This
script fetches the pinned tarball from the npm registry (or reads one given
with --tarball), checks its integrity, and wraps the chosen weight of each
mapped icon in the root element reicon's own `toSvg` writes (less its size
and class). The folder's other SVG files are removed, so it holds exactly the
mapping; its README.md and LICENSE are written alongside.

The whole set is two files: `icons.bin`, each icon's inner markup in each
weight compressed on its own with zlib, one after another, and `index.tsv`,
a line per icon in name order: its name, then the offset and length in
`icons.bin` of its Outline markup and of its Filled markup. An icon's name
is its reicon name in kebab case (`ArrowUpRight` is `arrow-up-right`,
`Setting4` is `setting4`). README.md and LICENSE are written beside them.

Usage: vendor-reicon.py [--tarball PATH]
Re-run after changing VERSION/INTEGRITY or the mapping below.
"""
import argparse
import base64
import hashlib
import io
import os
import re
import sys
import tarfile
import urllib.request
import zlib

VERSION = "1.2.5"
URL = f"https://registry.npmjs.org/reicon/-/reicon-{VERSION}.tgz"
# The registry's `dist.integrity` for VERSION.
INTEGRITY = "sha512-2I7zqhdNftyIO4FFv8Txy168gFIvy5qOO56zHlLNl1cyBmt6fy41LM8k+566epgcJN2pJd1Wk9TA+CXUZszUNA=="

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "crates", "pane", "assets", "icons", "reicon")
FULL = os.path.join(ROOT, "crates", "pane-core", "assets", "reicon")

# Pane glyph file -> (reicon icon, weight: "O" Outline or "F" Filled).
MAPPING = {
    "search": ("Search", "O"),
    "terminal": ("BrowserTerminal", "O"),
    "prompt": ("TerminalSquare", "O"),
    "code": ("Code", "O"),
    "folder": ("Folder", "O"),
    "blocks": ("ElementPlus", "O"),
    "clipboard": ("Clipboard", "O"),
    "search-none": ("SearchMinus", "O"),
    "arrow-right": ("ArrowRight", "O"),
    "clock": ("Clock", "O"),
    "gear": ("Gear", "O"),
    "keyboard": ("Keyboard", "O"),
    "chevron-right": ("AngleRight", "O"),
    "monitor": ("Monitor", "O"),
    "sliders": ("Setting4", "O"),
    "action-open": ("ArrowUpRight", "O"),
    "action-run": ("Play", "O"),
    "action-hotkey": ("Keyboard", "O"),
    "action-alias": ("Tag", "O"),
    "arrow-left": ("ArrowLeft", "O"),
    "pause": ("Pause", "O"),
    "shield": ("ShieldCheck", "O"),
    "lines": ("TextalignLeft", "O"),
    "action-pin": ("PinTack", "O"),
    "plus": ("Plus", "O"),
    "reset": ("RotateLeft", "O"),
    "record": ("Keyboard", "O"),
}

LICENSE = """MIT License

Copyright (c) 2025 REICON
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"""

WEIGHTS = {"O": "Outline", "F": "Filled"}
CREATE_ICON = re.compile(r"createIcon\('([^']+)',\s*\{(.*)\}\);", re.S)
WEIGHT = re.compile(r"(\w):\s*`([^`]*)`")
PAGE = re.compile(r"https://reicon\.dev/icons/[\w-]+")


def tarball(path):
    if path:
        with open(path, "rb") as file:
            data = file.read()
    else:
        with urllib.request.urlopen(URL) as response:
            data = response.read()
    digest = "sha512-" + base64.b64encode(hashlib.sha512(data).digest()).decode()
    if digest != INTEGRITY:
        sys.exit(f"reicon {VERSION}: integrity {digest} is not the pinned {INTEGRITY}")
    return tarfile.open(fileobj=io.BytesIO(data), mode="r:gz")


def icon(archive, name):
    """The icon's markup by weight key, and its reicon.dev page, from its
    module."""
    try:
        source = archive.extractfile(f"package/icons/{name}.js").read().decode("utf-8")
    except KeyError:
        sys.exit(f"reicon {VERSION} has no icon {name}")
    found = CREATE_ICON.search(source)
    if not found or found.group(1) != name:
        sys.exit(f"reicon {VERSION}: {name}.js holds no createIcon('{name}', ...) call")
    page = PAGE.search(source)
    return dict(WEIGHT.findall(found.group(2))), page.group(0) if page else "https://reicon.dev/icons"


def readme(pages):
    rows = "\n".join(
        f"| `{pane}.svg` | [{name}]({pages[name]}) | {WEIGHTS[weight]} |"
        for pane, (name, weight) in sorted(MAPPING.items())
    )
    return f"""# reicon

Pane's glyphs from [reicon](https://reicon.dev) {VERSION}
(npm `reicon`, <https://github.com/dqev/reicon>), under the MIT License in
`LICENSE`. Each file is the icon's markup wrapped in a standalone 24x24 SVG
root, unmodified otherwise.

Generated by `scripts/icons/vendor-reicon.py` — change the mapping there and
re-run it rather than editing these files.

| File | reicon icon | Weight |
| --- | --- | --- |
{rows}
"""


def kebab(name):
    """`name`, a reicon name such as `ArrowUpRight`, in kebab case
    (`arrow-up-right`): a dash before each capital that follows a small
    letter or a digit, and before a capital that starts a word after
    capitals (`ACUnit` is `ac-unit`). No two reicon names meet."""
    name = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "-", name)
    name = re.sub(r"(?<=[A-Z])(?=[A-Z][a-z])", "-", name)
    return name.lower()


def full_readme(count):
    return f"""# reicon, the whole set

Every icon of [reicon](https://reicon.dev) {VERSION} (npm `reicon`,
<https://github.com/dqev/reicon>), {count} icons in both their Outline and
Filled weights, under the MIT License in `LICENSE`. Extensions draw them by
name (#139, ADR 0036); `crates/pane-core/src/icons.rs` reads them.

- `icons.bin`: each icon's inner markup on its 24x24 viewBox, as reicon
  ships it, in each weight compressed on its own with zlib, one after
  another.
- `index.tsv`: a line per icon, in name order: its name, then the offset
  and length in `icons.bin` of its Outline markup and of its Filled
  markup, separated by tabs. An icon's name is its reicon name in kebab
  case (`ArrowUpRight` is `arrow-up-right`, `Setting4` is `setting4`).

Generated by `scripts/icons/vendor-reicon.py` — re-run it rather than
editing these files. Pane's own glyphs are the subset in
`crates/pane/assets/icons/reicon/`, written by the same script.
"""


def vendor_full_set(archive):
    """Writes every icon of `archive` in both weights to FULL (see the
    module docs)."""
    os.makedirs(FULL, exist_ok=True)
    icons = {}
    for member in archive.getmembers():
        found = re.fullmatch(r"package/icons/([A-Za-z0-9]+)\.js", member.name)
        if not found:
            continue
        name = found.group(1)
        markups, _ = icon(archive, name)
        if not markups.get("O") or not markups.get("F"):
            sys.exit(f"reicon {VERSION}: {name} lacks a weight")
        key = kebab(name)
        if key in icons:
            sys.exit(f"reicon {VERSION}: {name} and another icon are both named {key}")
        icons[key] = (markups["O"].strip(), markups["F"].strip())
    blob = bytearray()
    lines = []
    for key in sorted(icons):
        spans = []
        for markup in icons[key]:
            packed = zlib.compress(markup.encode("utf-8"), 9)
            spans.append(f"{len(blob)}\t{len(packed)}")
            blob.extend(packed)
        lines.append(f"{key}\t{spans[0]}\t{spans[1]}\n")
    with open(os.path.join(FULL, "icons.bin"), "wb") as file:
        file.write(bytes(blob))
    with open(os.path.join(FULL, "index.tsv"), "w", encoding="utf-8", newline="\n") as file:
        file.writelines(lines)
    with open(os.path.join(FULL, "README.md"), "w", encoding="utf-8", newline="\n") as file:
        file.write(full_readme(len(icons)))
    with open(os.path.join(FULL, "LICENSE"), "w", encoding="utf-8", newline="\n") as file:
        file.write(LICENSE)
    return len(icons)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--tarball", help=f"a local reicon-{VERSION}.tgz instead of the registry's")
    args = parser.parse_args()

    archive = tarball(args.tarball)
    os.makedirs(OUT, exist_ok=True)
    for stale in os.listdir(OUT):
        if stale.endswith(".svg") and stale[: -len(".svg")] not in MAPPING:
            os.remove(os.path.join(OUT, stale))
    pages = {}
    for pane, (name, weight) in sorted(MAPPING.items()):
        markups, pages[name] = icon(archive, name)
        markup = markups.get(weight)
        if not markup:
            sys.exit(f"reicon {VERSION}: {name} has no {WEIGHTS[weight]} weight")
        svg = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none">{markup.strip()}</svg>\n'
        with open(os.path.join(OUT, f"{pane}.svg"), "w", encoding="utf-8", newline="\n") as file:
            file.write(svg)
    with open(os.path.join(OUT, "README.md"), "w", encoding="utf-8", newline="\n") as file:
        file.write(readme(pages))
    with open(os.path.join(OUT, "LICENSE"), "w", encoding="utf-8", newline="\n") as file:
        file.write(LICENSE)
    print(f"reicon {VERSION}: wrote {len(MAPPING)} glyphs to {os.path.relpath(OUT, ROOT).replace(os.sep, '/')}")
    count = vendor_full_set(archive)
    print(f"reicon {VERSION}: wrote {count} icons to {os.path.relpath(FULL, ROOT).replace(os.sep, '/')}")


if __name__ == "__main__":
    main()
