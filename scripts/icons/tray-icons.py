"""Draws the tray's icons from Pane's mark (#131): the two squares of
crates/pane/assets/icons/mark-back.svg (stroked) and mark-front.svg
(filled), the mark the launcher's footer shows, in one colour.

- crates/pane-core/assets/tray/mark-dark.ico: the mark in black, for a light
  taskbar, and
- crates/pane-core/assets/tray/mark-light.ico: the mark in white, for a dark
  taskbar. Each holds the mark at every small-icon size the notification
  area asks for between 100% and 400% scaling (16 to 64 pixels), as 32-bit
  images with an alpha channel; the Windows tray adapter
  (crates/pane-core/src/tray/windows.rs) embeds both and loads the one size
  the display's DPI asks for.
- crates/pane-core/assets/tray/mark-template.png: the mark in black on
  transparency, 36 pixels square, for macOS's menu bar at 18 points on a
  Retina display. The adapter marks it as a template image, which the
  system tints for light and dark menu bars and the selected state.

One colour cannot tell the two squares apart where they overlap, so the
back square's stroke stops short of the front square by a gap of
GAP units (a pixel at 16 pixels). The images are drawn here, with only the
standard library: each pixel's coverage is sampled on a SAMPLES x SAMPLES
grid, and the files are written with struct and zlib.

Usage: tray-icons.py
Re-run after changing the mark's SVGs or the constants below, and commit
the files it writes.
"""
import os
import re
import struct
import zlib

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
MARK = os.path.join(ROOT, "crates", "pane", "assets", "icons")
OUT = os.path.join(ROOT, "crates", "pane-core", "assets", "tray")

# The small-icon sizes of 100%, 125%, ... 400% scaling: 16 * dpi / 96.
SIZES = [16, 20, 24, 28, 32, 36, 40, 48, 56, 64]
# The menu bar image: 18 points at 2x.
TEMPLATE_SIZE = 36
# The square of the mark's 24-unit viewBox the images show: the two
# squares, their stroke, and a little room around them, centred.
VIEW = 20.0
# The gap left between the back square's stroke and the front square.
GAP = 1.25
SAMPLES = 8

BLACK = (0, 0, 0)
WHITE = (255, 255, 255)


def rect(svg):
    """The first <rect> of `svg` as (x, y, width, height, radius), and its
    stroke width (0 for a filled one)."""
    match = re.search(r"<rect\b([^>]*)/?>", svg)
    attributes = dict(re.findall(r'([\w-]+)="([^"]*)"', match.group(1)))
    shape = tuple(float(attributes[name]) for name in ("x", "y", "width", "height", "rx"))
    stroke = re.search(r'stroke-width="([^"]*)"', svg)
    return shape, (float(stroke.group(1)) if stroke else 0.0)


def read(name):
    with open(os.path.join(MARK, name), encoding="utf-8") as file:
        return file.read()


BACK, STROKE = rect(read("mark-back.svg"))
FRONT, _ = rect(read("mark-front.svg"))


def distance(shape, px, py):
    """The signed distance from (px, py) to the rounded rectangle `shape`:
    negative inside."""
    x, y, width, height, radius = shape
    cx, cy = x + width / 2, y + height / 2
    qx = abs(px - cx) - (width / 2 - radius)
    qy = abs(py - cy) - (height / 2 - radius)
    outside = (max(qx, 0.0) ** 2 + max(qy, 0.0) ** 2) ** 0.5
    return outside + min(max(qx, qy), 0.0) - radius


def covered(px, py):
    """Whether the mark covers the point (px, py) of its viewBox."""
    front = distance(FRONT, px, py)
    if front <= 0:
        return True
    back = distance(BACK, px, py)
    return abs(back) <= STROKE / 2 and front > GAP


def bounds():
    """The viewBox square the images show, centred on the mark."""
    left = min(BACK[0] - STROKE / 2, FRONT[0])
    top = min(BACK[1] - STROKE / 2, FRONT[1])
    right = max(BACK[0] + BACK[2] + STROKE / 2, FRONT[0] + FRONT[2])
    bottom = max(BACK[1] + BACK[3] + STROKE / 2, FRONT[1] + FRONT[3])
    return (left + right) / 2 - VIEW / 2, (top + bottom) / 2 - VIEW / 2


def coverage(size):
    """Each pixel's coverage, 0 to 255, row by row from the top."""
    left, top = bounds()
    scale = VIEW / size
    rows = []
    for row in range(size):
        line = []
        for column in range(size):
            hits = 0
            for sy in range(SAMPLES):
                for sx in range(SAMPLES):
                    px = left + (column + (sx + 0.5) / SAMPLES) * scale
                    py = top + (row + (sy + 0.5) / SAMPLES) * scale
                    hits += covered(px, py)
            line.append(round(255 * hits / (SAMPLES * SAMPLES)))
        rows.append(line)
    return rows


def dib(size, alpha, colour):
    """One icon image as an .ico file holds it: a BITMAPINFOHEADER of twice
    the height, the 32-bit BGRA pixels bottom-up, then the 1-bit AND mask
    (set where the image is fully transparent), each row padded to 32 bits."""
    red, green, blue = colour
    pixels = bytearray()
    for line in reversed(alpha):
        for value in line:
            pixels += bytes((blue, green, red, value))
    stride = ((size + 31) // 32) * 4
    mask = bytearray()
    for line in reversed(alpha):
        row = bytearray(stride)
        for column, value in enumerate(line):
            if value == 0:
                row[column // 8] |= 0x80 >> (column % 8)
        mask += row
    header = struct.pack(
        "<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, len(pixels) + len(mask), 0, 0, 0, 0
    )
    return header + bytes(pixels) + bytes(mask)


def ico(images):
    """An .ico file of `images`, a list of (size, image bytes)."""
    directory = struct.pack("<HHH", 0, 1, len(images))
    offset = len(directory) + 16 * len(images)
    entries = b""
    data = b""
    for size, image in images:
        entries += struct.pack(
            "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(image), offset + len(data)
        )
        data += image
    return directory + entries + data


def png(size, alpha, colour):
    """A PNG of the mark in `colour` on transparency (8-bit RGBA)."""
    red, green, blue = colour
    raw = bytearray()
    for line in alpha:
        raw.append(0)
        for value in line:
            raw += bytes((red, green, blue, value))

    def chunk(kind, body):
        crc = zlib.crc32(kind + body) & 0xFFFFFFFF
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", crc)

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def write(name, data):
    with open(os.path.join(OUT, name), "wb") as file:
        file.write(data)
    print(f"{name}: {len(data)} bytes")


def main():
    os.makedirs(OUT, exist_ok=True)
    masks = {size: coverage(size) for size in SIZES}
    write("mark-dark.ico", ico([(size, dib(size, masks[size], BLACK)) for size in SIZES]))
    write("mark-light.ico", ico([(size, dib(size, masks[size], WHITE)) for size in SIZES]))
    write("mark-template.png", png(TEMPLATE_SIZE, coverage(TEMPLATE_SIZE), BLACK))


if __name__ == "__main__":
    main()
