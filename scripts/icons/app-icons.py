"""Draws Pane's application icons (#454) from Pane's mark: the two squares
of crates/pane/assets/icons/mark-back.svg (stroked) and mark-front.svg
(filled), coloured as the launcher's footer draws them in the dark theme
(the back square in the muted text colour #8E8F94, the front one in
#EDEDEF at .92), on a rounded tile of the dark panel's colour #16171A —
the mark as the launcher itself shows it, on the launcher's own surface.

- crates/pane/assets/app/pane.ico: the icon of pane.exe, which the pane
  crate's build script embeds as the program's first icon resource (the
  one Explorer, the Start menu's shortcut and the taskbar show, and the one
  GPUI gives Pane's windows). It holds 16 to 64 pixels as 32-bit images
  with an alpha channel and 256 pixels as a PNG, as Windows reads them.
- crates/pane/assets/app/pane.icns: the icon of the Pane.app bundle the
  macOS install script builds, named by its Info.plist (CFBundleIconFile).
  It holds every size Finder and the Dock ask for, 16 to 1024 pixels, as
  PNGs. The tile follows the macOS icon grid: 824 of 1024 pixels, centred,
  so the system's own shadow and spacing fit around it.
- crates/pane/assets/app/hicolor/<size>x<size>/apps/pane.png: the icon of
  Pane's Linux desktop entry (`Icon=pane`), laid out as the hicolor icon
  theme is, where a desktop looks the name up; the Linux package's install
  script and the AppImage install them (#464). It holds the sizes desktops ask
  for, 16 to 512 pixels, with the macOS tile's margin, as GNOME's and KDE's
  own application icons leave one.

The images are drawn here, with only the standard library, from the mark's
SVGs (read as the tray's script reads them): each pixel's coverage of each
shape comes from its signed distance to the shape, a pixel wide ramp at the
edge, and the files are written with struct and zlib. The output depends
only on the SVGs and the constants below, so running this again writes the
same bytes.

Usage: app-icons.py
Re-run after changing the mark's SVGs or the constants below, and commit
the files it writes.
"""
import importlib.util
import os
import struct
import sys
import zlib

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT = os.path.join(ROOT, "crates", "pane", "assets", "app")

# The mark's geometry, read from its SVGs by the tray's script, so the two
# cannot read it differently (and no bytecode cache is left beside it).
sys.dont_write_bytecode = True
_spec = importlib.util.spec_from_file_location(
    "tray_icons", os.path.join(os.path.dirname(__file__), "tray-icons.py")
)
tray = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(tray)
BACK, STROKE, FRONT = tray.BACK, tray.STROKE, tray.FRONT

# The launcher's dark palette (crates/pane/src/ui/theme.rs): the panel, the
# muted text the footer draws the back square in, and the footer mark's
# colour, with its alpha.
TILE = (0x16, 0x17, 0x1A)
BACK_COLOUR = (0x8E, 0x8F, 0x94)
FRONT_COLOUR = (0xED, 0xED, 0xEF)
FRONT_ALPHA = 0xEB / 255

# Where the tile sits, as fractions of the image: its inset from each edge
# and its corner radius, as a fraction of its side. Windows draws an
# application's icon as it is, so the tile fills the image but for a sliver;
# macOS's grid puts an 824-pixel tile in a 1024-pixel image.
WINDOWS_TILE = (1 / 32, 0.22)
MACOS_TILE = (100 / 1024, 0.225)
LINUX_TILE = MACOS_TILE
# The mark's width, as a fraction of the tile's side.
MARK = 0.62

WINDOWS_SIZES = [16, 20, 24, 32, 40, 48, 64]
LINUX_SIZES = [16, 24, 32, 48, 64, 128, 256, 512]
WINDOWS_PNG = 256
# Each ICNS entry: its type and the pixels it holds (the @2x types hold
# twice the points they are named for).
ICNS = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic13", 256),
    (b"ic14", 512),
]


def bounds():
    """The mark's extent in its viewBox: left, top, right, bottom."""
    left = min(BACK[0] - STROKE / 2, FRONT[0])
    top = min(BACK[1] - STROKE / 2, FRONT[1])
    right = max(BACK[0] + BACK[2] + STROKE / 2, FRONT[0] + FRONT[2])
    bottom = max(BACK[1] + BACK[3] + STROKE / 2, FRONT[1] + FRONT[3])
    return left, top, right, bottom


def ramp(distance, pixel):
    """A shape's coverage of a pixel whose centre is `distance` from its
    edge (negative inside), the pixel being `pixel` units wide."""
    return min(1.0, max(0.0, 0.5 - distance / pixel))


def draw(size, tile):
    """The icon at `size` pixels with the tile placed by `tile`, as rows of
    (red, green, blue, alpha), from the top."""
    inset, radius = tile
    side = size * (1 - 2 * inset)
    tile_shape = (size * inset, size * inset, side, side, side * radius)
    left, top, right, bottom = bounds()
    # The mark's viewBox units per pixel, and where its centre lands: the
    # tile's centre.
    scale = (right - left) / (side * MARK)
    origin_x = (left + right) / 2 - size / 2 * scale
    origin_y = (top + bottom) / 2 - size / 2 * scale
    rows = []
    for row in range(size):
        line = []
        for column in range(size):
            x, y = column + 0.5, row + 0.5
            alpha = ramp(tray.distance(tile_shape, x, y), 1.0)
            if alpha == 0:
                line.append((0, 0, 0, 0))
                continue
            mx, my = origin_x + x * scale, origin_y + y * scale
            back = ramp(abs(tray.distance(BACK, mx, my)) - STROKE / 2, scale)
            front = ramp(tray.distance(FRONT, mx, my), scale) * FRONT_ALPHA
            colour = TILE
            colour = mix(colour, BACK_COLOUR, back)
            colour = mix(colour, FRONT_COLOUR, front)
            line.append(tuple(round(c) for c in colour) + (round(255 * alpha),))
        rows.append(line)
    return rows


def mix(under, over, amount):
    return tuple(u + (o - u) * amount for u, o in zip(under, over))


def png(rows):
    """A PNG of `rows` (8-bit RGBA)."""
    size = len(rows)
    raw = bytearray()
    for line in rows:
        raw.append(0)
        for pixel in line:
            raw += bytes(pixel)

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


def dib(rows):
    """One icon image as an .ico file holds it: a BITMAPINFOHEADER of twice
    the height, the 32-bit BGRA pixels bottom-up, then the 1-bit AND mask
    (set where the image is fully transparent), each row padded to 32 bits."""
    size = len(rows)
    pixels = bytearray()
    for line in reversed(rows):
        for red, green, blue, alpha in line:
            pixels += bytes((blue, green, red, alpha))
    stride = ((size + 31) // 32) * 4
    mask = bytearray()
    for line in reversed(rows):
        row = bytearray(stride)
        for column, pixel in enumerate(line):
            if pixel[3] == 0:
                row[column // 8] |= 0x80 >> (column % 8)
        mask += row
    header = struct.pack(
        "<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, len(pixels) + len(mask), 0, 0, 0, 0
    )
    return header + bytes(pixels) + bytes(mask)


def icns(entries):
    """An .icns file of `entries`, a list of (type, PNG bytes)."""
    body = b"".join(kind + struct.pack(">I", 8 + len(data)) + data for kind, data in entries)
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


def write(name, data):
    path = os.path.join(OUT, name)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as file:
        file.write(data)
    print(f"{name}: {len(data)} bytes")


def main():
    os.makedirs(OUT, exist_ok=True)
    images = [(size, dib(draw(size, WINDOWS_TILE))) for size in WINDOWS_SIZES]
    images.append((WINDOWS_PNG, png(draw(WINDOWS_PNG, WINDOWS_TILE))))
    write("pane.ico", tray.ico(images))
    pngs = {}
    for _, size in ICNS:
        if size not in pngs:
            pngs[size] = png(draw(size, MACOS_TILE))
    write("pane.icns", icns([(kind, pngs[size]) for kind, size in ICNS]))
    for size in LINUX_SIZES:
        write(f"hicolor/{size}x{size}/apps/pane.png", png(draw(size, LINUX_TILE)))


if __name__ == "__main__":
    main()
