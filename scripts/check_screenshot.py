"""Asserts that a smoke screenshot shows Pane text drawn in a given color.

The behavior smokes explicitly select PANE_THEME=dark and PANE_MATERIAL=opaque.
The panel is #16171a with a sheen and footer wash; hints are #8e8f94,
results #9fd8a8, errors #ff9a92 and warnings #d6a36a. Within the window's
bounds (the largest connected region of the panel's neutral surfaces, so
a live desktop's own near-background pixels beside the
window stay out), pixels near the text color prove text actually rendered:
a window without a text system shows only its backgrounds. Antialiasing
blends glyph edges, so a pixel counts when it is near the target and closer
to it than to any other color Pane draws. Requires Pillow.

With --distinct, asserts instead that the Pane window looks different in every
given screenshot, so steps that should show different content (each guest's
answer) cannot silently show the same view. With --same, asserts that two
screenshots show the same Pane window, allowing only a few pixels — each
within one channel level of rounding noise: a screen that should
list the same rows as an earlier one (after a restart, a disabled package's
command is gone again) cannot silently list another. With --absent, asserts
that the given color itself is not drawn in the Pane window (at most a few
stray near pixels, antialiasing excluded): a confirmation just opened whose
idle hint is not the result color cannot silently show a result.

With --locate, prints the center of the largest connected region of pixels drawn
exactly in the given color inside the Pane window
(such as one swatch of a custom view), as "x y" screenshot pixels, so a
smoke can click there.

Usage: python3 scripts/check_screenshot.py <png> <role or hex color> [min pixels]
       python3 scripts/check_screenshot.py --distinct <png> <png>...
       python3 scripts/check_screenshot.py --same <png> <png>
       python3 scripts/check_screenshot.py --absent <png> <role or hex color> [max pixels]
       python3 scripts/check_screenshot.py --locate <png> <hex color>
       python3 scripts/check_screenshot.py <png> selected [min pixels]
       python3 scripts/check_screenshot.py <png> answer
       python3 scripts/check_screenshot.py <png> progress|subtitle [min pixels]
       python3 scripts/check_screenshot.py --preview <png>

Selected rows must contain a broad connected wash, not just similarly colored
text or icons. A computed answer is drawn as a card instead: selected, it
takes the same selection wash on the card's own bounds — no ring, no edge
(ADR 0035) — so what tells the card from a row is the wash region's height,
at least a card tall. Host color roles are hint, details, success, error and warning;
progress and subtitle additionally restrict the region being checked.
Literal hex colors remain available for extension-authored drawings.
--preview waits for visible metadata below the package heading
at the default startup size, where root search's header is empty. These are
behavior checks, not dark/light or native material acceptance evidence.
"""
import sys

from PIL import Image

# Opaque dark semantic colors, including composited neutral surfaces. Guest
# drawing colors are intentionally not remapped: hex checks/locate still test
# exactly the authored colors (blue, purple and green in the sample picker).
HOST_COLORS = {
    "hint": "8e8f94",
    "details": "a3a4a9",
    "success": "9fd8a8",
    "error": "ff9a92",
    "warning": "d6a36a",
}
# The accent the caret and the pin hint draw (the answer card's ring is gone).
ACCENT = "c9ee6a"
PALETTE = ["16171a", "131416", "222326", "2a2b2e", "353639", "ededef",
           HOST_COLORS["details"], HOST_COLORS["hint"], "f3f3f5", "86878c", "e9e9ec",
           ACCENT, HOST_COLORS["success"], HOST_COLORS["error"], HOST_COLORS["warning"]]


def panel_surface(pixel) -> bool:
    """Solid panel, sheen, footer, dividers and selected wash (source-over).

    Include the connecting inset edge: cropping to only the flat center would
    lose the search header and footer, and could make a missing result pass.
    The hue constraint excludes colored desktop pixels and guest swatches.
    """
    r, g, b = pixel
    return 18 <= r <= 59 and 0 <= g - r <= 2 and 1 <= b - r <= 5


def rgb(color: str) -> tuple[int, int, int]:
    color = color.lstrip("#")
    return tuple(int(color[i:i + 2], 16) for i in (0, 2, 4))


def near(a, b, tolerance: float) -> bool:
    return sum((x - y) ** 2 for x, y in zip(a, b)) <= tolerance ** 2


def pixels_of(image: Image.Image) -> list:
    return list(getattr(image, "get_flattened_data", image.getdata)())  # Pillow 12 renamed it


def window_box(path: str) -> tuple[Image.Image, tuple[int, int, int, int]]:
    """The screenshot and the Pane window's box in it, found by its background color.

    The window is the largest connected region of the background color, not
    the bounding box of every pixel near it: a live desktop holds pixels of
    its own near Pane's background, and those stretch the box to include
    whatever changes beside the window, so a --same comparison would flake
    on the desktop's own changes (the local smoke's 57/58 at 41c4096: the
    windows were identical, a desktop strip beside the window was not) and
    a color check could count the desktop's pixels as Pane's.
    """
    image = Image.open(path).convert("RGB")
    width = image.width
    matching = {i for i, pixel in enumerate(pixels_of(image)) if panel_surface(pixel)}
    if not matching:
        raise SystemExit(f"{path}: the Pane window is not visible")
    window = largest_region(matching, width)
    rows = [i // width for i in window]
    columns = [i % width for i in window]
    box = (min(columns), min(rows), max(columns) + 1, max(rows) + 1)
    if box[2] - box[0] < 200 or box[3] - box[1] < 150:
        raise SystemExit(f"{path}: no complete dark opaque Pane panel found")
    return image, box


def pane_window(path: str) -> Image.Image:
    """The screenshot cropped to the Pane window."""
    image, box = window_box(path)
    return image.crop(box)


def distinct(paths: list[str]) -> None:
    windows = []
    for path in paths:
        window = pane_window(path)
        windows.append((path, (window.size, window.tobytes())))
    for i, (first, pixels) in enumerate(windows):
        for second, other in windows[i + 1:]:
            if pixels == other:
                raise SystemExit(f"{first} and {second} show the same Pane window")
    print(f"{len(paths)} screenshots show different Pane windows")


# The outermost pixels of the window: rounded corners (macOS) antialias against
# whatever is behind the window, so they differ between otherwise equal frames.
EDGE = 12


def inner(window: Image.Image) -> Image.Image:
    return window.crop((EDGE, EDGE, window.width - EDGE, window.height - EDGE))


def largest_region(matching: set, width: int) -> list[int]:
    """The largest 4-connected region of `matching`, holding image indices.

    A window or a swatch rather than a stray antialiased pixel or the
    desktop around it: what is looked for is one connected place.
    """
    largest: list[int] = []
    while matching:
        start = matching.pop()
        region, frontier = [start], [start]
        while frontier:
            i = frontier.pop()
            x = i % width
            for j in (i - width, i + width, i - 1 if x > 0 else -1, i + 1 if x + 1 < width else -1):
                if j in matching:
                    matching.remove(j)
                    region.append(j)
                    frontier.append(j)
        if len(region) > len(largest):
            largest = region
    return largest


def same(first: str, second: str) -> None:
    a, b = inner(pane_window(first)), inner(pane_window(second))
    if a.size != b.size:
        raise SystemExit(f"{first} and {second} show different Pane windows")
    # CI 36951745142: the same TypeScript result on Windows and macOS
    # differed at exactly one interior pixel by one channel level (on
    # macOS, two channels at that one pixel). CI 37054210836: the same
    # family had grown past that one-pixel budget — the same view drawn
    # by two window instances differed along one glyph's antialiased
    # edge (11 one-level pixels on Windows, 2 on macOS), where the run
    # before #71 and #72 compared equal. The one-level bound is what
    # guards the comparison — a different command title, answer or
    # selection moves pixels by far more than a rounding step — so a
    # handful of such pixels is allowed rather than exactly one: still
    # no percentage budget, text mask or broad tolerance.
    changed = 0
    for left, right in zip(pixels_of(a), pixels_of(b)):
        if left != right:
            changed += 1
            if changed > 16 or any(abs(x - y) > 1 for x, y in zip(left, right)):
                raise SystemExit(f"{first} and {second} show different Pane windows")
    print(f"{first} and {second} show the same Pane window")


def locate(path: str, color: str) -> None:
    image, (left, top, right, bottom) = window_box(path)
    window = image.crop((left, top, right, bottom))
    target = rgb(color)
    width = window.width
    matching = {i for i, pixel in enumerate(pixels_of(window)) if near(pixel, target, 4)}
    if not matching:
        raise SystemExit(f"{path}: no pixels of #{color.lstrip('#')} in the Pane window")
    # The largest 4-connected region of the color: a swatch rather than a
    # stray antialiased pixel, and one place rather than the middle of two.
    largest = largest_region(matching, width)
    xs = [i % width for i in largest]
    ys = [i // width for i in largest]
    print(left + (min(xs) + max(xs)) // 2, top + (min(ys) + max(ys)) // 2)


def drawn_in(window: Image.Image, target: tuple[int, int, int]) -> list[int]:
    """The indices of the pixels of `window` near `target` and closer to it
    than to any other color Pane draws (antialiasing blends glyph edges)."""
    palette = [rgb(c) for c in PALETTE] + [target]

    def closest(pixel):
        return min(palette, key=lambda c: sum((x - y) ** 2 for x, y in zip(pixel, c)))

    return [i for i, pixel in enumerate(pixels_of(window))
            if near(pixel, target, 40) and closest(pixel) == target]


def count_near(window: Image.Image, target: tuple[int, int, int]) -> int:
    """How many pixels of `window` are drawn in `target` ([`drawn_in`])."""
    return len(drawn_in(window, target))


def main(path: str, color: str, minimum: int = 20) -> None:
    if color == "selected":
        selected(path, minimum)
        return
    if color == "answer":
        answer(path)
        return
    window = pane_window(path)
    if color in ("progress", "subtitle"):
        # Progress now shares the unavailable-reason color; subtitles share
        # the idle hint color. Require the intended region, not any matching
        # text elsewhere. Find the darker footer wash rather than assuming
        # its height (long statuses can grow it).
        region = largest_region({i for i, pixel in enumerate(pixels_of(window))
                                 if panel_surface(pixel) and pixel[0] <= 20}, window.width)
        if not region:
            raise SystemExit(f"{path}: footer wash not visible")
        footer_top = min(i // window.width for i in region)
        scale = window.width / 760
        if color == "progress":
            window = window.crop((0, footer_top, window.width, window.height))
            color = "warning"
        else:
            window = window.crop((0, round(44 * scale), window.width, footer_top))
            color = "hint"
    color = HOST_COLORS.get(color, color)
    count = count_near(window, rgb(color))
    if count < minimum:
        raise SystemExit(f"{path}: {count} pixels near #{color.lstrip('#')} in the Pane window, "
                         f"expected at least {minimum}")
    print(f"{path}: {count} pixels near #{color.lstrip('#')} in the Pane window")


def selected(path: str, minimum: int = 3000) -> None:
    """A selection wash spans the row; a hover, sheen or tile cannot pass."""
    window = pane_window(path)
    matching = {i for i, pixel in enumerate(pixels_of(window))
                if panel_surface(pixel) and 41 <= pixel[0] <= 54}
    region = largest_region(matching, window.width)
    xs = [i % window.width for i in region]
    ys = [i // window.width for i in region]
    if (len(region) < minimum or max(xs, default=0) - min(xs, default=0) < window.width / 2
            or max(ys, default=0) - min(ys, default=0) < 12):
        raise SystemExit(f"{path}: no broad selected row wash (at least {minimum} pixels)")
    print(f"{path}: selected row wash contains {len(region)} pixels")


def answer(path: str) -> None:
    """The selected computed answer's card (#96): the selection wash like a
    row's — the accent ring it used to draw is gone (ADR 0035) — on the
    card's own bounds, which no row matches: the wash region must be broad
    and at least 60 pixels tall (a card is padding, its values and their
    captions, 79 and up; a row is 36; CI 37430002287 measured the ring's
    edges 79 pixels apart, the card's height). The caret and the pin hint
    are accent, but never that broad."""
    window = pane_window(path)
    width = window.width
    matching = {i for i, pixel in enumerate(pixels_of(window))
                if panel_surface(pixel) and 41 <= pixel[0] <= 54}
    region = largest_region(matching, width)
    xs = [i % width for i in region]
    ys = [i // width for i in region]
    if (len(region) < 3000
            or max(xs, default=0) - min(xs, default=0) < width / 2
            or max(ys, default=0) - min(ys, default=0) < 60):
        raise SystemExit(f"{path}: no selected answer card (a tall broad wash)")
    print(f"{path}: selected answer card wash contains {len(region)} pixels")


def preview(path: str) -> None:
    """Positive metadata evidence before Enter, replacing the removed blue border.

    Only used immediately after --install at the 760-logical-pixel startup
    width. Scale from the screenshot for Retina/Windows DPI. The band below
    the package heading contains its first metadata line; on root search it
    is blank, below the placeholder and above the 64px header divider.
    """
    window = pane_window(path)
    scale = window.width / 760
    band = window.crop((round(20 * scale), round(48 * scale),
                        window.width - round(20 * scale), round(61 * scale)))
    # Metadata is text, not a solid swatch: Linux glyph rasterization can
    # leave few pixels within the exact-color tolerance. Use the same
    # antialias-aware, nearest-palette test as the other text assertions.
    count = count_near(band, rgb(HOST_COLORS["details"]))
    if count < 20:
        raise SystemExit(f"{path}: package preview metadata not yet visible ({count} pixels)")
    print(f"{path}: package preview metadata visible ({count} pixels)")


def absent(path: str, color: str, allowance: int = 5) -> int:
    """Asserts that the color itself is not drawn: at most `allowance` pixels
    are within a tight tolerance of it, so antialiased blends of other colors
    (a preview's details pass near the query field's border color) do not
    count, while the solid pixels of what is looked for always do (a query
    field's border, a result status's glyphs)."""
    color = HOST_COLORS.get(color, color)
    window = pane_window(path)
    target = rgb(color)
    count = sum(1 for pixel in pixels_of(window) if near(pixel, target, 4))
    if count > allowance:
        raise SystemExit(f"{path}: {count} pixels near #{color.lstrip('#')} in the Pane window, "
                         f"expected at most {allowance}")
    print(f"{path}: {count} pixels near #{color.lstrip('#')} in the Pane window, at most "
          f"{allowance} expected")
    return count


if __name__ == "__main__":
    if sys.argv[1] == "--distinct":
        distinct(sys.argv[2:])
    elif sys.argv[1] == "--same":
        same(sys.argv[2], sys.argv[3])
    elif sys.argv[1] == "--absent":
        absent(sys.argv[2], sys.argv[3], *(int(n) for n in sys.argv[4:5]))
    elif sys.argv[1] == "--locate":
        locate(sys.argv[2], sys.argv[3])
    elif sys.argv[1] == "--preview":
        preview(sys.argv[2])
    else:
        main(sys.argv[1], sys.argv[2], *(int(n) for n in sys.argv[3:4]))
