"""Asserts that a smoke screenshot shows Pane text drawn in a given color.

Pane's window background is #20252d; its hint line is #8a96a3, action
results are #9fd8a8, errors #f08c8c and an unavailable action's reason
#d6a36a. Within the window's bounds (the largest connected region of the
background color, so a live desktop's own near-background pixels beside the
window stay out), pixels near the text color prove text actually rendered:
a window without a text system shows only its backgrounds. Antialiasing
blends glyph edges, so a pixel counts when it is near the target and closer
to it than to any other color Pane draws. Requires Pillow.

With --distinct, asserts instead that the Pane window looks different in every
given screenshot, so steps that should show different content (each guest's
answer) cannot silently show the same view. With --same, asserts that two
screenshots show the same Pane window, pixel for pixel: a screen that should
list the same rows as an earlier one (after a restart, a disabled package's
command is gone again) cannot silently list another. With --absent, asserts
that the given color itself is not drawn in the Pane window (at most a few
stray near pixels, antialiasing excluded): a confirmation just opened whose
idle hint is not the result color cannot silently show a result, and a query
field whose solid border is the color cannot still be on screen.

With --locate, prints the center of the largest connected region of pixels drawn
exactly in the given color inside the Pane window
(such as one swatch of a custom view), as "x y" screenshot pixels, so a
smoke can click there.

Usage: python3 scripts/check_screenshot.py <png> <hex color> [min pixels]
       python3 scripts/check_screenshot.py --distinct <png> <png>...
       python3 scripts/check_screenshot.py --same <png> <png>
       python3 scripts/check_screenshot.py --absent <png> <hex color> [max pixels]
       python3 scripts/check_screenshot.py --locate <png> <hex color>
"""
import sys

from PIL import Image

BACKGROUND = (0x20, 0x25, 0x2D)
# Every color Pane draws, so a pixel counts only if the target is the closest.
PALETTE = ["20252d", "364355", "2e3a48", "f1f3f5", "aab4c0", "8a96a3", "d6c27a",
           "9fd8a8", "f08c8c", "1a1e24", "8ab4f8", "d6a36a"]


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
    matching = {i for i, pixel in enumerate(pixels_of(image)) if near(pixel, BACKGROUND, 4)}
    if not matching:
        raise SystemExit(f"{path}: the Pane window is not visible")
    window = largest_region(matching, width)
    rows = [i // width for i in window]
    columns = [i % width for i in window]
    return image, (min(columns), min(rows), max(columns) + 1, max(rows) + 1)


def pane_window(path: str) -> Image.Image:
    """The screenshot cropped to the Pane window."""
    image, box = window_box(path)
    return image.crop(box)


def distinct(paths: list[str]) -> None:
    windows = [(path, pane_window(path).tobytes()) for path in paths]
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
    if inner(pane_window(first)).tobytes() != inner(pane_window(second)).tobytes():
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


def count_near(window: Image.Image, target: tuple[int, int, int]) -> int:
    """How many pixels of `window` are near `target` and closer to it than to
    any other color Pane draws (antialiasing blends glyph edges)."""
    palette = [rgb(c) for c in PALETTE] + [target]

    def closest(pixel):
        return min(palette, key=lambda c: sum((x - y) ** 2 for x, y in zip(pixel, c)))

    window_pixels = pixels_of(window)
    return sum(1 for pixel in window_pixels
               if near(pixel, target, 40) and closest(pixel) == target)


def main(path: str, color: str, minimum: int = 20) -> None:
    count = count_near(pane_window(path), rgb(color))
    if count < minimum:
        raise SystemExit(f"{path}: {count} pixels near #{color.lstrip('#')} in the Pane window, "
                         f"expected at least {minimum}")
    print(f"{path}: {count} pixels near #{color.lstrip('#')} in the Pane window")


def absent(path: str, color: str, allowance: int = 5) -> int:
    """Asserts that the color itself is not drawn: at most `allowance` pixels
    are within a tight tolerance of it, so antialiased blends of other colors
    (a preview's details pass near the query field's border color) do not
    count, while the solid pixels of what is looked for always do (a query
    field's border, a result status's glyphs)."""
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
    else:
        main(sys.argv[1], sys.argv[2], *(int(n) for n in sys.argv[3:4]))
