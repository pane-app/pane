"""Asserts that a smoke screenshot shows Pane text drawn in a given color.

Pane's window background is #20252d; its hint line is #8a96a3 and action
results are #9fd8a8. Within the window's bounds (found by the background
color), pixels near the text color prove text actually rendered: a window
without a text system shows only its backgrounds. Antialiasing blends glyph
edges, so a pixel counts when it is near the target and closer to it than to
any other color Pane draws. Requires Pillow.

With --distinct, asserts instead that the Pane window looks different in every
given screenshot, so steps that should show different content (each guest's
answer) cannot silently show the same view.

Usage: python3 scripts/check_screenshot.py <png> <hex color> [min pixels]
       python3 scripts/check_screenshot.py --distinct <png> <png>...
"""
import sys

from PIL import Image

BACKGROUND = (0x20, 0x25, 0x2D)
# Every color Pane draws, so a pixel counts only if the target is the closest.
PALETTE = ["20252d", "364355", "2e3a48", "f1f3f5", "aab4c0", "8a96a3", "d6c27a",
           "9fd8a8", "f08c8c"]


def rgb(color: str) -> tuple[int, int, int]:
    color = color.lstrip("#")
    return tuple(int(color[i:i + 2], 16) for i in (0, 2, 4))


def near(a, b, tolerance: float) -> bool:
    return sum((x - y) ** 2 for x, y in zip(a, b)) <= tolerance ** 2


def pane_window(path: str) -> Image.Image:
    """The screenshot cropped to the Pane window, found by its background color."""
    image = Image.open(path).convert("RGB")
    width = image.width
    flattened = getattr(image, "get_flattened_data", image.getdata)  # Pillow 12 renamed it
    pixels = list(flattened())
    background = [i for i, pixel in enumerate(pixels) if near(pixel, BACKGROUND, 4)]
    if not background:
        raise SystemExit(f"{path}: the Pane window is not visible")
    rows = [i // width for i in background]
    columns = [i % width for i in background]
    return image.crop((min(columns), min(rows), max(columns) + 1, max(rows) + 1))


def distinct(paths: list[str]) -> None:
    windows = [(path, pane_window(path).tobytes()) for path in paths]
    for i, (first, pixels) in enumerate(windows):
        for second, other in windows[i + 1:]:
            if pixels == other:
                raise SystemExit(f"{first} and {second} show the same Pane window")
    print(f"{len(paths)} screenshots show different Pane windows")


def main(path: str, color: str, minimum: int = 20) -> None:
    window = pane_window(path)
    target = rgb(color)
    palette = [rgb(c) for c in PALETTE] + [target]

    def closest(pixel):
        return min(palette, key=lambda c: sum((x - y) ** 2 for x, y in zip(pixel, c)))

    window_pixels = getattr(window, "get_flattened_data", window.getdata)()
    count = sum(1 for pixel in window_pixels
                if near(pixel, target, 40) and closest(pixel) == target)
    if count < minimum:
        raise SystemExit(f"{path}: {count} pixels near #{color.lstrip('#')} in the Pane window, "
                         f"expected at least {minimum}")
    print(f"{path}: {count} pixels near #{color.lstrip('#')} in the Pane window")


if __name__ == "__main__":
    if sys.argv[1] == "--distinct":
        distinct(sys.argv[2:])
    else:
        main(sys.argv[1], sys.argv[2], *(int(n) for n in sys.argv[3:4]))
