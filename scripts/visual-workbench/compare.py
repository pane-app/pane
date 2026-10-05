"""The visual workbench's comparison (#91): native fixture vs authored reference.

Reads a native capture run (one directory per scenario: fixture-manifest.json,
native-run.json and the capture PNGs) and a reference capture run
(reference-manifest.json and its PNGs), measures both images, and writes:

- report.json: every check, in three sections -
  harness-native     the fixture's declared layout/colors vs what its
                     capture measures (validates the harness; should pass),
  harness-reference  the reference DOM's rects/classes vs what its capture
                     measures (validates the reference side; should pass),
  parity             native vs reference for the same component in the same
                     state (expected to fail broadly until #92-#99 land);
  a failed check that carries a recorded disposition (a measured platform
  limit a ticket explicitly accepted, with its reason) is counted as
  "accepted" rather than "failed" - it still fails, visibly, with its note;
  plus native-only captures (states the reference never authors) and, with
  --baseline, the sensitivity section: checks that passed in the baseline
  run and fail in this one, with both values.
- summary.md: the same, readable.
- per capture: side-by-side (native | reference), an amplified difference
  image and one crop pair per compared component - at 1:1, never rescaled.

Measurement rules (the ticket's proposed limits):
- edges and baselines: 1 logical px;
- deterministic flat fills: 2 channel levels. Translucent washes are
  compared as backdrop-relative overlay alpha - (fill - bg) / (255 - bg)
  for a white wash, (bg - fill) / bg for a black one - measured against
  the local background next to them, which survives the panels' different
  backdrops (opaque native panel, blurred wallpaper behind the glass);
- glyphs: only core pixels count (luma past 60% of the way from the local
  background to the text color), so antialiased edges never decide a
  check; glyph core colors use their own limit (4 levels) since glyph
  cores are not flat fills; the accent-colored caret is excluded from ink.
Images are compared at the same logical size or not at all: a size
mismatch is itself a failed check, never rescaled away.

Requires Pillow. Usage:
  python scripts/visual-workbench/compare.py --native <dir> --reference <dir> --out <dir>
      [--baseline <report.json>] [--label <name>]
"""
import argparse
import json
import math
import statistics
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFont

LIMITS = {"edge_px": 1.0, "flat_fill_levels": 2.0, "glyph_core_levels": 4.0}
# Pane's embedded faces, which both sides draw Geist from.
FONTS = Path(__file__).resolve().parents[2] / "crates" / "pane" / "assets" / "fonts"
# The Settings page heading's face and size (theme::SettingsType: 22/600).
HEADING_FACE, HEADING_SIZE = "Geist-SemiBold.ttf", 22.0
_BEARINGS = {}


def first_glyph_bearing(text, size=HEADING_SIZE, face=HEADING_FACE):
    """Where the first glyph of text begins to ink past its pen position,
    in logical px at size: its left side bearing, read from the font's own
    outline: the glyph drawn at 1000px from a known pen position, where its
    ink begins (Pillow's own text box starts at the pen, not the ink). A
    straight stem's bearing (K, L, E: 80 units, 1.76px at 22) is more than
    a round or diagonal one's (G 0.95, A 0.42). 0 for no text, or no face."""
    if not text:
        return 0.0
    key = (text[0], face)
    if key not in _BEARINGS:
        try:
            font = ImageFont.truetype(str(FONTS / face), 1000)
        except OSError:
            _BEARINGS[key] = 0.0
        else:
            pen = 200
            canvas = Image.new("L", (1600, 1600))
            ImageDraw.Draw(canvas).text((pen, 1200), text[0], font=font, fill=255, anchor="ls")
            ink = canvas.getbbox()
            _BEARINGS[key] = (ink[0] - pen) / 1000 if ink else 0.0
    return _BEARINGS[key] * size
# The muted subtitle text both sides author (#8E8F94).
MUTED = (142, 143, 148)
# What the measuring needs to know about a palette: whether its washes and
# rules are lighter than the panel (the dark palette's white overlays) or
# darker (the derived light palette's black ones), and the text colors ink
# is looked for in. The reference is always the dark one.
DARK = {"lighter": True, "muted": MUTED, "query": (243, 243, 245), "placeholder": (134, 135, 140),
        "alias": (185, 186, 190)}


def palette(manifest):
    """The native capture's palette, from its manifest's declared colors."""
    colors = manifest["declared"]["colors"]
    return {
        "lighter": manifest.get("appearance", "dark") == "dark",
        "muted": hex_rgba(colors["textMuted"])[:3],
        "query": hex_rgba(colors.get("textQuery", "#F3F3F5FF"))[:3],
        "placeholder": hex_rgba(colors["textPlaceholder"])[:3],
        "alias": hex_rgba(colors.get("aliasText", "#B9BABEFF"))[:3],
    }
CORE_FRACTION = 0.6
# Below this overlay alpha (levels) a row counts as having no wash: the
# hover wash is 3.5% (~9 levels), so half of it separates the two.
NO_WASH_LEVELS = 4.5

# ----------------------------------------------------------------- pixels


def load(path):
    return Image.open(path).convert("RGB")


def luma(color):
    r, g, b = color[:3]
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def is_accent(color):
    """The lime accent (the caret, the reference's Enter cap)."""
    r, g, b = color[:3]
    return g > 150 and g - b > 60 and g - r > 15


def pixels(image, box):
    """The RGB pixels inside box = (left, top, right, bottom), clamped."""
    left, top, right, bottom = clamp_box(image, box)
    data = image.load()
    return [data[x, y] for y in range(top, bottom) for x in range(left, right)]


def clamp_box(image, box):
    left, top, right, bottom = (int(round(v)) for v in box)
    return (max(0, left), max(0, top), min(image.width, right), min(image.height, bottom))


def box_visible(box):
    """Whether a clamped box still holds a pixel (a rect off the image clamps empty)."""
    left, top, right, bottom = box
    return right > left and bottom > top


def median_color(image, box):
    values = pixels(image, box)
    if not values:
        return None
    return tuple(statistics.median(channel) for channel in zip(*values))


def mean_color(image, box):
    """The mean color inside box: unlike the median it keeps a fraction of
    a level, which a faint overlay on a dark fill needs (see
    measure_settings)."""
    values = pixels(image, box)
    if not values:
        return None
    return tuple(statistics.mean(channel) for channel in zip(*values))


def white_alpha(fill, background):
    """A white overlay's alpha, in levels (0-255), from the composited fill
    and the background it sits on: (fill - bg) / (255 - bg) per channel."""
    estimates = [
        (f - b) / (255 - b) * 255 for f, b in zip(fill, background) if 255 - b > 8
    ]
    return statistics.mean(estimates) if estimates else None


def black_alpha(fill, background):
    """A black overlay's alpha, in levels: (bg - fill) / bg per channel."""
    estimates = [(b - f) / b * 255 for f, b in zip(fill, background) if b > 8]
    return statistics.mean(estimates) if estimates else None


def lit_run(lit, center, gap=1):
    """[start, end) of the run of lit samples holding center (or the lit
    sample nearest it), bridging single unlit samples - a thin dark glyph
    stroke inside a wash does not split it, while the list's 2px gap still
    separates one row's wash from its neighbor's."""
    indices = [i for i, value in enumerate(lit) if value]
    if not indices:
        return None
    start = end = min(indices, key=lambda i: abs(i - center))
    while True:
        ahead = [i for i in range(end + 1, min(len(lit), end + 2 + gap)) if lit[i]]
        if not ahead:
            break
        end = ahead[-1]
    while True:
        behind = [i for i in range(max(0, start - 1 - gap), start) if lit[i]]
        if not behind:
            break
        start = behind[0]
    return start, end + 1


def wash_edges(image, approx, background, min_delta, margin=8, lighter=True):
    """The rect of a lighter wash around approx = (x, y, w, h).

    Left and right: along rows through its middle, the outermost pixels
    brighter than the background by min_delta (darker, for a darkening
    wash: lighter=False) - nothing else sits in the
    list's side padding, while inside the wash a tile's drop shadow can be
    darker than the wash. Top and bottom: along columns through its
    interior, the run of such pixels that holds the approximate center - a
    neighbor's wash two pixels away, or a rule above, is a separate run.
    The medians over all the scan lines are the edges. Returns (x, y, w, h)
    or None."""
    x, y, w, h = approx
    bg = luma(background)
    sign = 1 if lighter else -1
    data = image.load()
    lefts, rights, tops, bottoms = [], [], [], []
    xs = range(max(0, int(x - margin)), min(image.width, int(x + w + margin)))
    ys = range(max(0, int(y - margin)), min(image.height, int(y + h + margin)))
    for row in range(int(y + h * 0.35), int(y + h * 0.65) + 1):
        if 0 <= row < image.height:
            lit = [sign * (luma(data[c, row]) - bg) >= min_delta for c in xs]
            if any(lit):
                lefts.append(xs.start + lit.index(True))
                rights.append(xs.start + len(lit) - lit[::-1].index(True))
    inset = min(14, w / 4)
    step = max(1, int((w - 2 * inset) / 24))
    for column in range(int(x + inset), int(x + w - inset), step):
        if 0 <= column < image.width:
            run = lit_run([sign * (luma(data[column, r]) - bg) >= min_delta for r in ys], (y + h / 2) - ys.start)
            if run:
                tops.append(ys.start + run[0])
                bottoms.append(ys.start + run[1])
    if not (lefts and rights and tops and bottoms):
        return None
    left, right = statistics.median(lefts), statistics.median(rights)
    top, bottom = statistics.median(tops), statistics.median(bottoms)
    return (left, top, right - left, bottom - top)


def accent_pixels(image, box):
    return sum(1 for color in pixels(image, box) if is_accent(color))


def ink(image, box, background, text_color, exclude_accent=True):
    """Core glyph pixels inside box: luma past CORE_FRACTION of the way
    from the background to text_color. Returns the core pixels' bounding
    box (x, y, w, h) and their median color, or None."""
    left, top, right, bottom = clamp_box(image, box)
    bg, fg = luma(background), luma(text_color)
    threshold = bg + CORE_FRACTION * (fg - bg)
    data = image.load()
    xs, ys, colors = [], [], []
    for row in range(top, bottom):
        for column in range(left, right):
            color = data[column, row]
            if exclude_accent and is_accent(color):
                continue
            if (luma(color) >= threshold) if fg >= bg else (luma(color) <= threshold):
                xs.append(column)
                ys.append(row)
                colors.append(color)
    if not xs:
        return None
    # The color of the fullest cores: the top 5% by contrast, where the
    # glyph covers its pixels completely if anywhere.
    colors.sort(key=lambda c: abs(luma(c) - bg), reverse=True)
    core = colors[: max(1, len(colors) // 20)]
    return {
        "box": (min(xs), min(ys), max(xs) + 1 - min(xs), max(ys) + 1 - min(ys)),
        "color": tuple(statistics.median(channel) for channel in zip(*core)),
        "count": len(xs),
    }


def ink_top(image, box, background, text_color):
    """The first row inside box that glyph ink covers: where two
    neighboring pixels' coverage (how far each is from the background
    toward text_color) adds up past CORE_FRACTION. A thin stem straddling
    two columns covers neither pixel fully, so ink() alone misses it, and
    where it falls depends on the renderer's subpixel phase, not on where
    the glyph sits. Returns the row in image pixels, or None."""
    left, top, right, bottom = clamp_box(image, box)
    bg, fg = luma(background), luma(text_color)
    if fg == bg:
        return None
    data = image.load()
    for row in range(top, bottom):
        cover = [min(1.0, max(0.0, (luma(data[column, row]) - bg) / (fg - bg)))
                 for column in range(left, right)]
        if any(a + b >= CORE_FRACTION for a, b in zip(cover, cover[1:])):
            return row
    return None


def hline(image, x_range, y_range, background=None, lighter=True):
    """The y of the most distinct horizontal line across x_range within
    y_range (a hairline rule): the row whose median luma stands out most
    from the rows two pixels above and below it - a line, not a gradient
    or a step. The caller keeps y_range to a few pixels around where the
    rule belongs, so a wash's edge nearby is never taken for it."""
    data = image.load()
    # A narrow client is narrower than the usual stretch.
    columns = range(max(0, x_range[0]), min(image.width, x_range[1]), 3)

    def level(row):
        return statistics.median(luma(data[c, row]) for c in columns)

    # A hairline rule is lighter than the panel on both of its sides (darker,
    # in the light palette); the edge of a wash below it is lighter on one
    # side only.
    best, best_y = 0.0, None
    for row in range(max(2, y_range[0]), min(image.height - 2, y_range[1])):
        above, here, below = level(row - 2), level(row), level(row + 2)
        contrast = min(here - above, here - below) if lighter else min(above - here, below - here)
        if contrast > best:
            best, best_y = contrast, row
    return best_y


# ----------------------------------------------------------------- checks


class Report:
    def __init__(self):
        self.checks = []

    def check(self, section, scenario, capture, subject, prop, measured, expected, limit, unit, note=None,
              accepted=None):
        delta = None
        passed = False
        if measured is None or expected is None:
            note = (note + "; " if note else "") + "not measurable"
        elif isinstance(measured, str) or isinstance(expected, str):
            passed = measured == expected
        else:
            delta = round(measured - expected, 2)
            passed = abs(delta) <= limit
        entry = {
            "id": f"{section}/{scenario}/{capture}/{subject}/{prop}",
            "section": section,
            "scenario": scenario,
            "capture": capture,
            "subject": subject,
            "property": prop,
            "measured" if section != "parity" else "native": _round(measured),
            "expected" if section != "parity" else "reference": _round(expected),
            "delta": delta,
            "limit": limit,
            "unit": unit,
            "passed": passed,
        }
        if note:
            entry["note"] = note
        if accepted and not passed:
            entry["accepted"] = accepted
        self.checks.append(entry)
        return entry


def _round(value):
    if isinstance(value, float):
        return round(value, 2)
    if isinstance(value, tuple):
        return [_round(v) for v in value]
    return value


def hex_rgba(text):
    text = text.lstrip("#")
    return tuple(int(text[i : i + 2], 16) for i in (0, 2, 4, 6))


def css_rgba(text):
    """'rgba(255, 255, 255, 0.086)' or 'rgb(...)' -> (r, g, b, a 0-255)."""
    inner = text[text.index("(") + 1 : text.rindex(")")]
    parts = [float(p) for p in inner.replace("/", ",").split(",")]
    alpha = parts[3] if len(parts) > 3 else 1.0
    return (*parts[:3], alpha * 255)


def scaled(rect, scale):
    return tuple(v * scale for v in rect)


def as_tuple(rect):
    if rect is None:
        return None
    if isinstance(rect, dict):
        return (rect["x"], rect["y"], rect["width"], rect["height"])
    return tuple(rect)


# --------------------------------------------------------------- measuring


def panel_background(image, y, scale=1.0):
    """The panel's local background beside the list at height y: the list's
    left padding, clear of rows (x 3-8 logical)."""
    return median_color(image, (3 * scale, y - 3 * scale, 8 * scale, y + 3 * scale))


def row_wash(image, rect, scale=1.0, floor=False, lighter=True, ceiling=False):
    """A row's wash, measured column by column: the strip just under its
    top edge (above the tile and the text) against the list gaps just above
    and below the row, interpolated to the strip's height. The panels'
    backdrops vary across and down the panel (the glass shows a blurred
    wallpaper under a fading sheen), so each column's wash is measured
    against the background directly beside it; the median over the columns
    is the row's. A row whose height is only a floor (it may grow) uses the
    gap above alone. A row flush with its list's top edge (ceiling) has no
    gap above, and a backdrop that shades down the panel makes the gap below
    a different background: its columns are read against the list's side
    padding beside its two ends, at the strip's own rows, interpolated
    across the row. Returns the median fill, background and overlay alpha
    (levels)."""
    x, y, w, h = scaled(rect, scale)
    data = image.load()
    top, bottom = int(round(y)), int(round(y + h))
    strip = top + int(round(4 * scale))
    if top - 1 < 0 or bottom >= image.height:
        return None
    weight = 0.0 if floor else (strip - (top - 1)) / (bottom - (top - 1))
    if ceiling:
        ends = (median_color(image, (x - 6 * scale, strip - 1, x - 2 * scale, strip + 2)),
                median_color(image, (x + w + 2 * scale, strip - 1, x + w + 6 * scale, strip + 2)))
        if None in ends:
            return None
    alphas, fills, backgrounds = [], [], []
    for column in range(int(x + 14 * scale), int(x + w - 14 * scale)):
        if not 0 <= column < image.width:
            continue
        if ceiling:
            across = (column - (x - 4 * scale)) / (w + 8 * scale)
            background = tuple(a + (b - a) * across for a, b in zip(*ends))
        else:
            above = data[column, top - 1]
            below = above if floor else data[column, bottom]
            background = tuple(a + (b - a) * weight for a, b in zip(above, below))
        fill = tuple(statistics.mean(data[column, r][i] for r in (strip - 1, strip, strip + 1)) for i in range(3))
        alpha = overlay_alpha(fill, background, lighter)
        if alpha is not None:
            alphas.append(alpha)
            fills.append(fill)
            backgrounds.append(background)
    if not alphas:
        return None
    return {
        "fill": tuple(statistics.median(c) for c in zip(*fills)),
        "background": tuple(statistics.median(c) for c in zip(*backgrounds)),
        "alpha": statistics.median(alphas),
    }


def measure_row(image, rect, washed, text_color, scale=1.0, floor=False, pal=DARK, ceiling=False):
    """What the image shows for a row at the given rect."""
    result = {}
    lighter = pal["lighter"]
    wash = row_wash(image, rect, scale, floor, lighter, ceiling)
    if wash is None:
        return result
    result["alpha"] = wash["alpha"]
    if washed:
        sign = 1 if lighter else -1
        delta = max(3.0, sign * (luma(wash["fill"]) - luma(wash["background"])) / 2)
        edges = wash_edges(image, scaled(rect, scale), wash["background"], delta, lighter=lighter)
        if edges:
            result["edges"] = tuple(v / scale for v in edges)
    x, y, w, h = scaled(rect, scale)
    title_box = (x + 44 * scale, y + 4 * scale, x + w * 0.55, y + h - 4 * scale)
    # Row titles keep accent pixels (the reference paints the matched part
    # of a title in the accent); the header's caret is excluded instead.
    found = ink(image, title_box, wash["fill"], text_color, exclude_accent=False)
    if found:
        bx, by, bw, bh = found["box"]
        # The type's body by coverage, over the title's own columns: the
        # rows holding a quarter of its fullest row's ink. A thin
        # diagonal's tips (a slash's) are single partial pixels that GPUI's
        # text contrast lifts past a core threshold and Chrome leaves under
        # it, which a core extent counts as a pixel at each end.
        lines = coverage_rows(image, (bx, y + 4 * scale, bx + bw, y + h - 4 * scale), wash["fill"], text_color,
                              fraction=0.25)
        result["title"] = {
            "left": (bx - x) / scale,
            "top": (by - y) / scale,
            "height": (lines[1] - lines[0]) / scale if lines else bh / scale,
            "color": found["color"],
        }
        # The subtitle: muted ink after the title's last core column (the
        # title's own pixels are brighter, so they are left out by position).
        sub = ink(image, (bx + bw + scale, y + 4 * scale, x + w * 0.7, y + h - 4 * scale), wash["fill"],
                  pal["muted"], exclude_accent=False)
        if sub:
            result["subtitleGap"] = (sub["box"][0] - (bx + bw)) / scale
    result["accent"] = accent_pixels(image, title_box)
    return result


def coverage_rows(image, box, background, color, fraction=0.0):
    """The first and one past the last image row inside box (left, top,
    right, bottom) holding a pixel's worth of ink by coverage (see
    coverage_edges), and at least fraction of the fullest row's, or
    None."""
    left, top, right, bottom = clamp_box(image, box)
    bg, fg = luma(background), luma(color)
    if fg == bg or right <= left:
        return None
    data = image.load()

    def coverage(value):
        amount = min(1.0, max(0.0, (luma(value) - bg) / (fg - bg)))
        return amount if amount >= 0.2 else 0.0

    totals = {y: sum(coverage(data[x, y]) for x in range(left, right)) for y in range(top, bottom)}
    if not totals:
        return None
    enough = max(1.0, fraction * max(totals.values()))
    rows = [y for y, total in totals.items() if total >= enough]
    return (rows[0], rows[-1] + 1) if rows else None


def measure_header(image, rule_y, scale=1.0, pal=DARK):
    """The search header: its rule (searched within 4 px of rule_y) and
    the query's or placeholder's ink (the caret excluded)."""
    background = median_color(image, (300 * scale, 4 * scale, 700 * scale, 8 * scale))
    rule = hline(image, (int(200 * scale), int(600 * scale)),
                 (int((rule_y - 4) * scale), int((rule_y + 5) * scale)), background, pal["lighter"])
    text = ink(image, (44 * scale, 8 * scale, 500 * scale, 56 * scale), background, pal["query"])
    if text is None:  # the placeholder's muted grey
        text = ink(image, (44 * scale, 8 * scale, 500 * scale, 56 * scale), background, pal["placeholder"])
    return {
        "rule": rule / scale if rule is not None else None,
        "text": {
            "left": text["box"][0] / scale,
            "top": text["box"][1] / scale,
            "height": text["box"][3] / scale,
        }
        if text
        else None,
    }


def measure_footer(image, rule_y, scale=1.0, lighter=True):
    """The footer: its top rule (searched within 4 px of rule_y) and its
    tint, as a black overlay over the panel just above the rule. Both are
    read in the list's left padding (x 3-8), which no row, wash or footer
    control reaches - a selected row scrolled flush with the list's bottom
    edge would otherwise be taken for the panel above the rule."""
    background = median_color(image, (3 * scale, (rule_y - 4) * scale, 8 * scale, (rule_y - 2) * scale))
    # The rule runs the panel's width; a row the list cuts off at its
    # bottom puts its title and keycaps right above the rule further in.
    rule = hline(image, (int(2 * scale), int(10 * scale)),
                 (int((rule_y - 4) * scale), int((rule_y + 5) * scale)), background, lighter)
    if rule is None:
        return {"rule": None, "alpha": None}
    fill = median_color(image, (3 * scale, rule + 4 * scale, 8 * scale, rule + 6 * scale))
    return {"rule": rule / scale, "alpha": black_alpha(fill, background), "fill": fill, "background": background}


def measure_cap(image, rect, scale=1.0, lighter=True):
    """A keycap's measured box and fill alpha near its declared rect."""
    x, y, w, h = scaled(rect, scale)
    background = median_color(image, (x - 8 * scale, y + h * 0.3, x - 3 * scale, y + h * 0.7))
    if background is None:
        return None
    fill = median_color(image, (x + 1.5 * scale, y + 2 * scale, x + 3 * scale, y + h - 2 * scale))
    if fill is None:
        return None
    sign = 1 if lighter else -1
    delta = max(3.0, sign * (luma(fill) - luma(background)) / 2)
    edges = wash_edges(image, (x, y, w, h), background, delta, margin=5, lighter=lighter)
    return {
        "edges": tuple(v / scale for v in edges) if edges else None,
        "alpha": overlay_alpha(fill, background, lighter) if fill else None,
    }


# ------------------------------------------------------------- comparing


def compare(native_dir, reference_dir, out_dir, label):
    report = Report()
    reference = json.loads(Path(reference_dir, "reference-manifest.json").read_text(encoding="utf-8-sig"))
    ref_scenarios = {s["name"]: s for s in reference.get("scenarios", [])}
    images = []
    native_only = []
    meta = {"label": label, "native": str(native_dir), "reference": str(reference_dir), "limits": LIMITS,
            "referenceSha256": reference.get("sha256"), "chrome": reference.get("chrome"), "scenarios": []}

    # The reference boards' own sizes: the three canonical clients.
    for board in reference.get("boards", []):
        if board.get("glass"):
            report.check("harness-reference", "boards", board["slug"], "glass", "size",
                         f"{board['image'][0]}x{board['image'][1]}", f"{board['glass'][2]}x{board['glass'][3]}", 0, "px")
            if not board.get("sourceOnly"):
                check_reference_fonts(report, "boards", board["slug"], board["slug"], board.get("fonts"))

    for scenario_dir in sorted(p for p in Path(native_dir).iterdir() if (p / "fixture-manifest.json").exists()):
        manifest = json.loads((scenario_dir / "fixture-manifest.json").read_text(encoding="utf-8-sig"))
        run = json.loads((scenario_dir / "native-run.json").read_text(encoding="utf-8-sig"))
        scenario = manifest["scenario"]
        name = scenario["name"]
        scale = (run.get("dpi") or 96) / 96
        colors = manifest["declared"]["colors"]
        text_title = hex_rgba(colors["textTitle"])[:3]
        selected_alpha = hex_rgba(colors["rowSelected"])[3]
        hover_alpha = hex_rgba(colors["rowHover"])[3]
        meta["scenarios"].append({"name": name, "dpi": run.get("dpi"), "client": run.get("client"),
                                  "material": manifest["material"], "perturbation": manifest["perturbation"]})
        ref = ref_scenarios.get(name)
        check_fonts(report, name, manifest)
        if not scenario["reference"]:
            native_only.append({"scenario": name, "captures": [c["file"] for c in run["captures"]],
                                "reason": "the reference authors no such state; captured as a native adaptation, not compared"})
        for declared in manifest["captures"]:
            capture = declared["name"]
            native_path = scenario_dir / f"{capture}.png"
            if not native_path.exists():
                report.check("harness-native", name, capture, "capture", "exists", "missing", "present", 0, "")
                continue
            native = load(native_path)
            client = scenario["client"]
            report.check("harness-native", name, capture, "client", "size",
                         f"{native.width / scale:g}x{native.height / scale:g}", f"{client[0]:g}x{client[1]:g}", 0, "logical px")
            ref_capture = None
            reference_image = None
            if ref:
                ref_capture = next((c for c in ref["captures"] if c["name"] == capture), None)
                if ref_capture:
                    reference_image = load(Path(reference_dir, ref_capture["file"]))
                    report.check("parity", name, capture, "client", "size",
                                 f"{native.width / scale:g}x{native.height / scale:g}",
                                 f"{reference_image.width}x{reference_image.height}", 0, "logical px",
                                 "compared at 1:1; never rescaled")
                    native_scale, ref_scale = scale_parity(run, reference)
                    report.check("parity", name, capture, "client", "device scale", native_scale, ref_scale, 0, "",
                                 "the native window's DPI / 96 against the reference's device scale factor")
                    check_reference_fonts(report, name, capture, ref.get("board", "root"), ref_capture.get("fonts"))
                    check_reference_pointer(report, name, capture, ref_capture.get("pointer"))
            crops = []
            if scenario["family"] == "root":
                compare_root(report, name, capture, declared, manifest, native, scale, ref_capture,
                             reference_image, text_title, selected_alpha, hover_alpha, crops)
            elif scenario["family"] == "tiles":
                compare_tiles(report, name, capture, manifest, native, scale, crops)
            elif scenario["family"] == "settings":
                compare_settings(report, name, capture, manifest, native, scale, ref_capture, reference_image, crops)
                if manifest.get("appearancePage"):
                    compare_appearance(report, name, capture, manifest, native, scale, ref_capture, reference_image,
                                       crops)
                if manifest.get("panePage"):
                    compare_pane_page(report, name, capture, manifest, native, scale, crops)
            elif scenario["family"] == "form":
                compare_pane_page(report, name, capture, manifest, native, scale, crops)
            elif scenario["family"] == "clipboard":
                compare_clipboard(report, name, capture, declared, manifest, native, scale, ref_capture,
                                  reference_image, crops)
            else:
                compare_keycaps(report, name, capture, manifest, native, scale, ref_capture, reference_image, crops)
            images.append(write_images(out_dir, name, capture, native, reference_image, crops, scale))

    sections = {}
    for check in report.checks:
        section = sections.setdefault(check["section"], {"passed": 0, "failed": 0, "accepted": 0})
        section["passed" if check["passed"] else "accepted" if check.get("accepted") else "failed"] += 1
    return {"meta": meta, "totals": sections, "checks": report.checks, "nativeOnly": native_only,
            "images": images, "pending": pending(native_dir)}


def pending(native_dir):
    summary = Path(native_dir, "registry.json")
    if summary.exists():
        return json.loads(summary.read_text(encoding="utf-8-sig")).get("pending", [])
    return []


def listed_rows(declared):
    """The declared rows the rows' own checks measure: an answer card in a
    row's place is measured as the card (#96)."""
    return [row for row in declared["rows"] if not row.get("answer")]


def compare_root(report, name, capture, declared, manifest, native, scale, ref_capture, reference_image,
                 text_title, selected_alpha, hover_alpha, crops):
    # ---- the native side against its declaration
    pal = palette(manifest)
    board = manifest["scenario"].get("board")
    # While the Actions panel is open, the results lie under its dimmer and
    # their right ends under the panel: the rows' own checks belong to the
    # captures with it closed.
    covered = bool(declared.get("actions"))
    native_rows = {}
    for row in listed_rows(declared):
        if covered:
            break
        if row["heightIsFloor"] and row["unavailable"] is None:
            continue
        if not row.get("visible", True):
            continue
        rect = as_tuple(row["rect"])
        washed = row["selected"] or row["hovered"]
        measured = measure_row(native, rect, washed, text_title, scale, floor=row["heightIsFloor"], pal=pal)
        native_rows[row["title"]] = (row, measured)
        subject = f"row:{row['title']}"
        if row["selected"]:
            expected_alpha, state = selected_alpha, "selected"
        elif row["hovered"]:
            expected_alpha, state = hover_alpha, "hovered"
        else:
            expected_alpha, state = 0, "rest"
        report.check("harness-native", name, capture, subject, f"wash alpha ({state})",
                     measured.get("alpha"), expected_alpha, LIMITS["flat_fill_levels"], "levels")
        if washed and not row["heightIsFloor"]:
            edges = measured.get("edges")
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                report.check("harness-native", name, capture, subject, f"wash {prop}",
                             edges[index] if edges else None, rect[index], LIMITS["edge_px"], "px")
        crops.append(("row-" + row["title"], rect, None))
    search = as_tuple(manifest["searchHeader"])
    header = measure_header(native, search[1] + search[3] - 1, scale, pal)
    report.check("harness-native", name, capture, "search-header", "rule y",
                 header["rule"], search[1] + search[3] - 1, LIMITS["edge_px"], "px")
    footer_rect = as_tuple(manifest["footer"])
    footer = measure_footer(native, footer_rect[1], scale, pal["lighter"])
    report.check("harness-native", name, capture, "footer", "rule y", footer["rule"], footer_rect[1], LIMITS["edge_px"], "px")
    # The tint is read against the list just above the rule, which the
    # open panel's dimmer darkens.
    if not covered:
        report.check("harness-native", name, capture, "footer", "tint alpha", footer["alpha"],
                     hex_rgba(manifest["declared"]["colors"]["footerTint"])[3], LIMITS["flat_fill_levels"], "levels")
    if declared.get("actionButton"):
        # The footer's buttons are transparent at rest (`.fbtn`), so they
        # have no edge to measure: their label starts at their padding, and
        # their keys are measured as key groups.
        button = as_tuple(declared["actionButton"])
        background = footer_strip_background(native, footer_rect, scale)
        label = ink_extent(native, (button[0], button[1], button[2] * 0.6, button[3]), background,
                           hex_rgba(manifest["declared"]["colors"]["footerButtonText"])[:3], scale)
        report.check("harness-native", name, capture, "footer-primary", "label ink left", label and label["left"],
                     button[0] + manifest["declared"]["geometry"].get("actionPaddingX", 8), LIMITS["edge_px"] * 1.5,
                     "px", "a glyph's side bearing allowed")
        measured = measure_cap(native, button, scale, pal["lighter"])
        report.check("harness-native", name, capture, "footer-primary", "wash alpha (rest)",
                     measured and measured["alpha"], 0, LIMITS["flat_fill_levels"], "levels")
        crops.append(("footer-primary", button, None))
    harness_footer(report, name, capture, declared, manifest, native, scale, pal, crops)
    harness_actions(report, name, capture, declared, manifest, native, scale, pal, crops)
    crops.append(("search-header", search, None))
    if manifest["scenario"].get("frame"):
        compare_frame(report, name, capture, declared, manifest, native, scale, ref_capture, reference_image, crops)
    for row in listed_rows(declared):
        if row.get("visible", True) and not row["heightIsFloor"] and not covered:
            harness_row_trailing(report, name, capture, row, manifest, native, scale, crops)
    harness_board(report, name, capture, declared, manifest, native, scale, pal, crops)
    if not covered:
        harness_pinned(report, name, capture, declared, manifest, native, scale, pal, crops)

    if ref_capture is None:
        return
    # ---- the reference side against its DOM
    state = ref_capture["state"]
    covered = covered or bool(state.get("panel"))
    ref_rows = {}
    for row in state["rows"]:
        if not row["visible"] or covered:
            continue
        rect = as_tuple(row["rect"])
        washed = row["selected"] or row["hovered"]
        measured = measure_row(reference_image, rect, washed, text_title)
        ref_rows[row["title"]] = (row, measured)
        subject = f"row:{row['title']}"
        dom_alpha = css_rgba(row["background"])[3]
        report.check("harness-reference", name, capture, subject, "wash alpha vs DOM background",
                     measured.get("alpha"), dom_alpha, LIMITS["flat_fill_levels"], "levels",
                     "the DOM serializes .085 as .086")
        if row["selected"]:
            edges = measured.get("edges")
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                report.check("harness-reference", name, capture, subject, f"wash {prop}",
                             edges[index] if edges else None, rect[index], LIMITS["edge_px"], "px")
    dom_header = as_tuple(state["searchHeader"])
    ref_header = measure_header(reference_image, dom_header[1] + dom_header[3] - 1)
    report.check("harness-reference", name, capture, "search-header", "rule y", ref_header["rule"],
                 dom_header[1] + dom_header[3] - 1, LIMITS["edge_px"], "px")
    dom_footer = as_tuple(state["footer"])
    ref_footer = measure_footer(reference_image, dom_footer[1])
    report.check("harness-reference", name, capture, "footer", "rule y", ref_footer["rule"], dom_footer[1],
                 LIMITS["edge_px"], "px")

    # ---- parity: the same component in the same state
    native_selected = sorted(r["title"] for r in listed_rows(declared) if r["selected"] and r.get("visible", True))
    ref_selected = sorted(r["title"] for r in state["rows"] if r["selected"])
    # The static empty board preselects a fallback; Pane's wait (#96, #100).
    unselected_fallbacks = board in STATIC_FALLBACK_BOARDS and not native_selected and bool(ref_selected)
    report.check("parity", name, capture, "selection", "selected rows", ", ".join(native_selected),
                 ", ".join(ref_selected), 0, "", "the pointer's movement selects the row it moves over, on both sides (#94)",
                 accepted=FALLBACK_SELECTION if unselected_fallbacks else None)
    native_hovered = sorted(r["title"] for r in listed_rows(declared)
                            if r["hovered"] and not r["selected"] and r.get("visible", True))
    ref_hover_only = sorted(r["title"] for r in state["rows"] if r["hovered"] and not r["selected"])
    report.check("parity", name, capture, "selection", "hover-only rows", ", ".join(native_hovered),
                 ", ".join(ref_hover_only), 0, "")
    for title, (row, measured) in native_rows.items():
        if title not in ref_rows:
            continue
        ref_row, ref_measured = ref_rows[title]
        subject = f"row:{title}"
        n_rect, r_rect = as_tuple(row["rect"]), as_tuple(ref_row["rect"])
        crops.append(("parity-row-" + title, n_rect, r_rect))
        report.check("parity", name, capture, subject, "wash alpha", measured.get("alpha"),
                     ref_measured.get("alpha"), LIMITS["flat_fill_levels"], "levels",
                     accepted=FALLBACK_SELECTION if unselected_fallbacks and ref_row["selected"] else None)
        n_edges, r_edges = measured.get("edges"), ref_measured.get("edges")
        # A hover-only wash is too faint to edge on the reference's glass,
        # whose blurred wallpaper shades across the row: its alpha is
        # compared, its edges only where the row is selected.
        if n_edges and r_edges and row["selected"]:
            for prop, index in (("left", 0), ("width", 2), ("height", 3)):
                report.check("parity", name, capture, subject, f"wash {prop}", n_edges[index], r_edges[index],
                             LIMITS["edge_px"], "px")
        extra = extra_labels_above(declared, state, n_rect[1], r_rect[1])
        report.check("parity", name, capture, subject, "top in client", n_rect[1], r_rect[1], LIMITS["edge_px"], "px",
                     "the pinned strip and the section labels lie above the rows on both sides (#101)",
                     accepted=EXTRA_LABEL_DISPOSITION if extra > 0 and abs(
                         r_rect[1] - n_rect[1] - extra * LABEL_STEP) <= LIMITS["edge_px"] else None)
        n_title, r_title = measured.get("title"), ref_measured.get("title")
        if n_title and r_title:
            report.check("parity", name, capture, subject, "title ink left in row", n_title["left"], r_title["left"],
                         LIMITS["edge_px"], "px")
            report.check("parity", name, capture, subject, "title ink top in row", n_title["top"], r_title["top"],
                         LIMITS["edge_px"], "px")
            report.check("parity", name, capture, subject, "title ink height", n_title["height"], r_title["height"],
                         LIMITS["edge_px"], "px")
            worst = max(abs(a - b) for a, b in zip(n_title["color"], r_title["color"]))
            report.check("parity", name, capture, subject, "title glyph core color (max channel)", worst, 0,
                         LIMITS["glyph_core_levels"], "levels",
                         f"native {fmt_color(n_title['color'])} vs reference {fmt_color(r_title['color'])}")
        if measured.get("subtitleGap") is not None and ref_measured.get("subtitleGap") is not None:
            report.check("parity", name, capture, subject, "title-to-subtitle ink gap", measured["subtitleGap"],
                         ref_measured["subtitleGap"], LIMITS["edge_px"], "px")
        compare_row_tile(report, name, capture, subject, row, ref_row, manifest, native, scale, reference_image,
                         measured, ref_measured, crops)
        compare_row_trailing(report, name, capture, subject, row, ref_row, manifest, native, scale,
                             reference_image, measured, ref_measured, crops)
        if state["query"]:
            report.check("parity", name, capture, subject, "title match highlighted",
                         "yes" if measured.get("accent") else "no", "yes" if ref_measured.get("accent") else "no", 0, "",
                         "the reference paints the matched part of a title in the accent",
                         accepted=FALLBACK_TITLES if board in STATIC_FALLBACK_BOARDS else None)
    report.check("parity", name, capture, "search-header", "rule y", header["rule"], ref_header["rule"], LIMITS["edge_px"], "px")
    if header["text"] and ref_header["text"]:
        for prop in ("left", "top", "height"):
            report.check("parity", name, capture, "search-header", f"query/placeholder ink {prop}",
                         header["text"][prop], ref_header["text"][prop], LIMITS["edge_px"], "px",
                         None if state["query"] else "placeholder copy is adapted: 'Search apps and commands…' vs 'Search apps, commands, plugins…' (#92)")
    report.check("parity", name, capture, "footer", "rule y", footer["rule"], ref_footer["rule"], LIMITS["edge_px"], "px")
    if not covered:
        report.check("parity", name, capture, "footer", "tint alpha", footer["alpha"], ref_footer["alpha"],
                     LIMITS["flat_fill_levels"], "levels")
    if declared.get("actionButton") and state.get("footerPrimary"):
        button = as_tuple(declared["actionButton"])
        ref_button = as_tuple(state["footerPrimary"]["rect"])
        native_label = declared.get("action") or ""
        report.check("parity", name, capture, "footer-primary", "label", native_label,
                     state["footerPrimary"]["label"].rstrip("↵").strip(), 0, "")
        report.check("parity", name, capture, "footer-primary", "button height", button[3], ref_button[3],
                     LIMITS["edge_px"], "px")
        ref_cap = measure_cap(reference_image, ref_button)
        native_cap = measure_cap(native, button, scale)
        report.check("parity", name, capture, "footer-primary", "button fill alpha",
                     native_cap and native_cap["alpha"], ref_cap and ref_cap["alpha"], LIMITS["flat_fill_levels"], "levels",
                     "the reference's footer buttons are transparent at rest")
        crops.append(("parity-footer-primary", button, ref_button))
    ref_groups = state.get("keycaps", {})
    for group in manifest.get("keycaps", []):
        footer_group = group.get("group") in ("footer-primary", "footer-actions")
        # The primary action's keys show only beside it: with nothing
        # selected (the empty board's unselected fallbacks, #96) the footer
        # shows no primary action, so its keys are not drawn.
        if group.get("group") == "footer-primary" and not declared.get("actionButton"):
            continue
        static_board = len((state.get("footerParts") or {}).get("hint") or []) == 1
        compare_key_group(report, name, capture, group, manifest, native, scale,
                          ref_groups.get(group.get("group")), reference_image, crops,
                          chained=group.get("group") == "footer-primary", footer=footer_group,
                          static=STATIC_ENTER if static_board and group.get("group") == "footer-primary" else None)
    if not covered:
        compare_sections(report, name, capture, declared, state, native, scale, reference_image, crops)
    parity_footer(report, name, capture, declared, manifest, native, scale, state, reference_image, pal, crops)
    parity_actions(report, name, capture, declared, manifest, native, scale, state, reference_image, pal, crops)
    parity_board(report, name, capture, declared, manifest, native, scale, state, reference_image, pal, crops)
    if not covered:
        parity_pinned(report, name, capture, declared, manifest, native, scale, state, reference_image, pal, crops)


# The launcher frame (#92). Windows rounds the launcher's window itself,
# through the Desktop Window Manager; a painted 18px radius would show the
# window's acrylic (which covers the whole window rectangle) as a plate
# behind the curve. PrintWindow reads the client before that clip, so the
# capture's corners are square.
CORNER_DISPOSITION = ("accepted (#92): the panel paints no radius on Windows - the acrylic backdrop covers the "
                      "whole window rectangle, so a painted 18px curve would show it as a plate - and the DWM's "
                      "corner preference (DWMWCP_ROUND, Microsoft's documented 8px) rounds the window instead. "
                      "This capture is PrintWindow's, taken before the DWM clip, so it shows the panel's square "
                      "corner, not the 8px the screen shows; the on-screen corner is documented, not measured here")


def overlay_alpha(fill, background, lighter):
    """A wash's alpha: a white overlay's when it lightens the panel, a black
    one's when it darkens it."""
    return white_alpha(fill, background) if lighter else black_alpha(fill, background)


def edge_alpha(image, edge, scale=1.0, lighter=True):
    """The panel's inset ring on one edge of the client, as an overlay alpha
    (levels): the outermost pixel line against the line two pixels in,
    median over a stretch of the edge clear of content - the list's side
    padding for the left and right edges, the header's top for the top,
    the footer's empty middle for the bottom."""
    width, height = image.width, image.height
    data = image.load()
    if edge in ("left", "right"):
        x_out = 0 if edge == "left" else width - 1
        x_in = 2 if edge == "left" else width - 3
        pairs = [(data[x_out, y], data[x_in, y]) for y in range(int(100 * scale), min(height, int(400 * scale)), 2)]
    else:
        y_out = 0 if edge == "top" else height - 1
        y_in = 2 if edge == "top" else height - 3
        lo, hi = ((100, 700) if edge == "top" else (320, 460))
        hi = min(hi, width / scale - 20)
        pairs = [(data[x, y_out], data[x, y_in]) for x in range(int(lo * scale), int(hi * scale), 3)]
    alphas = [a for a in (overlay_alpha(f, b, lighter) for f, b in pairs) if a is not None]
    return statistics.median(alphas) if alphas else None


def corner_inset(image, scale=1.0):
    """How far along the top-left corner's diagonal the panel begins, in
    logical px: 0 for a square corner; about r(1 - 1/sqrt 2) for a corner
    rounded at radius r (18px -> ~5.3). A diagonal pixel belongs to the
    panel once its luma is nearer the panel's (beside the left edge, below
    the curve) than the corner pixel's (outside any curve). A corner pixel
    as bright as the left edge's ring (or brighter: the ring under the top
    highlight) is the panel's own: a square corner."""
    data = image.load()
    inside = statistics.median(luma(data[x, y]) for x in range(int(3 * scale), int(8 * scale))
                               for y in range(int(30 * scale), int(34 * scale)))
    ring = statistics.median(luma(data[0, y]) for y in range(int(100 * scale), int(300 * scale)))
    outside = luma(data[0, 0])
    if abs(inside - outside) < 2 or outside >= ring - 3:
        return 0.0
    for t in range(0, int(20 * scale)):
        level = luma(data[t, t])
        if abs(level - inside) < abs(level - outside) or level > max(inside, outside):
            return t / scale
    return None


def compare_frame(report, name, capture, declared, manifest, native, scale, ref_capture, reference_image, crops):
    """The launcher frame: the panel's inset edges, its corner, and where
    the header, the list and the footer begin and end."""
    colors = manifest["declared"]["colors"]
    ring = hex_rgba(colors["hairline"])
    top = hex_rgba(colors.get("panelTopHighlight", "#00000000"))
    lighter = palette(manifest)["lighter"]
    for edge in ("left", "right"):
        report.check("harness-native", name, capture, "frame", f"{edge} edge ring alpha",
                     edge_alpha(native, edge, scale, lighter), ring[3], LIMITS["flat_fill_levels"], "levels",
                     "the panel's inset 1px ring")
    # The top edge is one overlay over another only where both lighten (the
    # light palette's ring darkens under a lightening highlight).
    if lighter and sum(top[:3]) > 384:
        combined = 255 * (1 - (1 - ring[3] / 255) * (1 - top[3] / 255))
        report.check("harness-native", name, capture, "frame", "top edge alpha", edge_alpha(native, "top", scale),
                     combined, LIMITS["flat_fill_levels"], "levels", "the ring under the top inset highlight")
    width, height = manifest["scenario"]["client"]
    search, list_rect, footer = (as_tuple(manifest[k]) for k in ("searchHeader", "list", "footer"))
    report.check("harness-native", name, capture, "frame", "header + list + footer height",
                 search[3] + list_rect[3] + footer[3], height, 0, "px",
                 f"{search[3]:g} + {list_rect[3]:g} + {footer[3]:g}: no border consumes the panel's layout")
    for label, rect in (("search header", search), ("list", list_rect), ("footer", footer)):
        report.check("harness-native", name, capture, "frame", f"{label} spans the width",
                     rect[2] if rect[0] == 0 else None, width, 0, "px")
    crops.append(("frame-corner", (0, 0, 40, 40), None))
    if ref_capture is None:
        return
    state = ref_capture["state"]
    for edge in ("left", "right", "top", "bottom"):
        report.check("parity", name, capture, "frame", f"{edge} edge alpha", edge_alpha(native, edge, scale),
                     edge_alpha(reference_image, edge), LIMITS["flat_fill_levels"], "levels",
                     "inset ring (and top highlight); the footer's wash covers the ring at the bottom")
    n_corner, r_corner = corner_inset(native, scale), corner_inset(reference_image)
    report.check("parity", name, capture, "frame", "top-left corner diagonal inset", n_corner, r_corner,
                 LIMITS["edge_px"], "px", "radius ~ inset x 3.41", accepted=CORNER_DISPOSITION)
    crops.append(("parity-frame-corner", (0, 0, 40, 40), (0, 0, 40, 40)))
    glass = as_tuple(state["glass"])
    report.check("parity", name, capture, "frame", "panel size", f"{width:g}x{height:g}", f"{glass[2]:g}x{glass[3]:g}", 0, "")
    for label, n_rect, key in (("search header", search, "searchHeader"), ("list", list_rect, "list"),
                               ("footer", footer, "footer")):
        r_rect = as_tuple(state[key])
        for prop, index in (("top", 1), ("height", 3), ("left", 0), ("width", 2)):
            report.check("parity", name, capture, "frame", f"{label} {prop}", n_rect[index], r_rect[index],
                         LIMITS["edge_px"], "px")
    # The footer's content ends with the Actions button's last key, on
    # both sides: its cap has a fill to edge (the `.fbtn` itself is
    # transparent at rest).
    native_keys = next((g for g in manifest.get("keycaps", []) if g.get("group") == "footer-actions"), None)
    ref_keys = (state.get("keycaps") or {}).get("footer-actions")
    if native_keys and ref_keys:
        mine = measure_key(native, as_tuple(native_keys["caps"][-1]["rect"]), False,
                           hex_rgba(manifest["declared"]["colors"]["keycapText"])[:3], scale, palette(manifest)["lighter"])
        theirs = measure_key(reference_image, as_tuple(ref_keys["caps"][-1]["rect"]), False,
                             hex_rgba(manifest["declared"]["colors"]["keycapText"])[:3])
        n_right = mine["edges"][0] + mine["edges"][2] if mine and mine["edges"] else None
        r_right = theirs["edges"][0] + theirs["edges"][2] if theirs and theirs["edges"] else None
        report.check("parity", name, capture, "frame", "footer content right edge", n_right, r_right,
                     LIMITS["edge_px"], "px", "the Actions button's last key: the footer's 8px right padding and the button's own")


def fmt_color(color):
    return "#" + "".join(f"{int(round(c)):02X}" for c in color[:3])


def check_fonts(report, name, manifest):
    """The embedded faces resolved as themselves: a face the text system
    could not find resolves to the same fallback face a family no system
    has resolves to at that weight (#93: a fallback cannot pass as Geist)."""
    faces = manifest.get("fontResolution") or []
    probes = {f["weight"]: f for f in faces if f["family"] == "Pane Missing Font Probe"}
    for face in faces:
        if face["family"] == "Pane Missing Font Probe":
            continue
        probe = probes.get(face["weight"])
        fallback = probe is not None and (face["fontId"] == probe["fontId"])
        report.check("harness-native", name, "manifest", "fonts", f"{face['family']} {face['weight']:g} resolved",
                     "the fallback face" if fallback or probe is None else "its own face", "its own face", 0, "",
                     f"probe widths: {face['probeWidth']:.2f} vs fallback {probe['probeWidth']:.2f}" if probe else
                     "no fallback probe at this weight")
    by_face = {(f["family"], f["weight"]): f["fontId"] for f in faces}
    for family in sorted({f["family"] for f in faces if f["family"] != "Pane Missing Font Probe"}):
        regular, medium = by_face.get((family, 400.0)), by_face.get((family, 500.0))
        if regular is not None and medium is not None:
            report.check("harness-native", name, "manifest", "fonts", f"{family} 500 is its own face",
                         "distinct" if regular != medium else "the 400 face", "distinct", 0, "",
                         "500 must resolve to the embedded Medium face, not a synthesized weight")


# The reference's faces each board draws with (#103). A face that failed or
# never loaded leaves Chrome to draw the board's text in a fallback, which
# the board's own DOM rects would follow, so the harness alone would not
# notice. Faces load only when a board draws with them, so these are the
# weights each board was seen to draw: Geist at 400 and 500 everywhere, 600
# for the Settings page's heading, Geist Mono for caps, values and times.
REFERENCE_FACES = {
    "root": [("Geist", 400), ("Geist", 500), ("Geist Mono", 400), ("Geist Mono", 500)],
    "actions": [("Geist", 400), ("Geist", 500), ("Geist Mono", 500)],
    "calculator": [("Geist", 400), ("Geist", 500), ("Geist Mono", 400), ("Geist Mono", 500)],
    "empty": [("Geist", 400), ("Geist", 500), ("Geist Mono", 500)],
    "clipboard": [("Geist", 400), ("Geist", 500), ("Geist Mono", 400), ("Geist Mono", 500)],
    "settings": [("Geist", 400), ("Geist", 500), ("Geist", 600), ("Geist Mono", 400), ("Geist Mono", 500)],
}
DEFAULT_REFERENCE_FACES = [("Geist", 400), ("Geist", 500)]


def _weight_covers(declared, weight):
    """Whether a FontFace's weight descriptor ("500", or a variable face's
    "100 900") covers `weight`."""
    try:
        parts = [float(p) for p in str(declared).split()]
    except ValueError:
        return False
    return bool(parts) and parts[0] <= weight <= parts[-1]


def face_status(faces, family, weight):
    """A family's state at one weight on a board: 'loaded' when one of its
    faces (the unicode-range subsets) loaded, 'error' when one failed and
    none loaded, 'absent' when the board declares none, else 'not loaded'."""
    statuses = [f.get("status") for f in faces if f.get("family") == family and _weight_covers(f.get("weight"), weight)]
    if not statuses:
        return "absent"
    if "loaded" in statuses:
        return "loaded"
    if "error" in statuses:
        return "error"
    return "not loaded"


def check_reference_fonts(report, name, capture, board, faces):
    """The reference board's own faces loaded before its capture (#103)."""
    if faces is None:
        report.check("harness-reference", name, capture, "fonts", "faces recorded", "not recorded", "recorded", 0, "",
                     "a capture without its board's font record cannot show it was drawn in Geist")
        return
    for family, weight in REFERENCE_FACES.get(board, DEFAULT_REFERENCE_FACES):
        report.check("harness-reference", name, capture, "fonts", f"{family} {weight:g} loaded",
                     face_status(faces, family, weight), "loaded", 0, "",
                     "the board's own FontFaceSet, read right after its capture")
    failed = sorted({f"{f.get('family')} {f.get('weight')}" for f in faces if f.get("status") == "error"})
    report.check("harness-reference", name, capture, "fonts", "faces failed", ", ".join(failed) or "none", "none", 0, "")


# The search header's height: the query field the reference's rest clicks
# to focus lies inside it, above every row.
HEADER_PX = 64


def pointer_rest(pointer):
    """Where a reference capture's pointer was, against where the capture
    put it (#103): (measured, expected). Before the scenario's own pointer
    steps it rests off the board (parked) or on the query field (the rest's
    click), never over a row a leftover position could hover; after them it
    is on the board."""
    if not pointer:
        return "not recorded", "recorded"
    on_board = pointer.get("board")
    if pointer.get("steps"):
        return ("on the board" if on_board else "off the board"), "on the board"
    expected = "off the board or on the query field"
    if on_board is None:
        return expected, expected
    if pointer.get("fieldClick") and on_board[1] < HEADER_PX:
        return expected, expected
    return f"over the board at {on_board[0]:.0f},{on_board[1]:.0f}", expected


def check_reference_pointer(report, name, capture, pointer):
    measured, expected = pointer_rest(pointer)
    report.check("harness-reference", name, capture, "pointer", "where the capture put it", measured, expected, 0, "",
                 "headless Chrome has no OS cursor; this script's moves are the only pointer")


def scale_parity(run, reference):
    """(native, reference) device scale: the native window's DPI over 96
    against the reference's device scale factor. Parity compares the two
    sides only at the same scale (#103)."""
    native = (run.get("dpi") or 0) / 96 if run.get("dpi") else None
    return native, reference.get("deviceScaleFactor")


def box_edges(image, rect, background, fill, scale=1.0, margin=2, avoid=(), chroma=False):
    """The edges of a filled box (a keycap, a tile) near its declared rect:
    left and right along its middle rows, top (and bottom) down columns
    just inside its left edge, where no label or glyph reaches. Pixels count
    as the box's when they differ from the background, in the fill's
    direction, by half the fill's contrast. margin stays inside the 3px gap
    between the caps of a sequence. Returns (x, y, w, h) in logical px; the
    bottom is reliable only for a box whose bottom row keeps the fill's
    direction (an accent cap, a tile) - a regular cap's bottom line darkens
    it. A label or glyph lighter than the fill is part of the box; one
    darker (an accent cap's ink) is inside its run and changes no edge."""
    x, y, w, h = scaled(rect, scale)
    data = image.load()
    bg = luma(background)
    sign = 1 if luma(fill) >= bg else -1
    delta = max(3.0, abs(luma(fill) - bg) / 2)

    # A saturated box (an app tile's gradient) keeps its chroma to its
    # edge, where a drop shadow only darkens the panel's own grey: in the
    # light palette that shadow is as far from the panel as the tile is.
    spread = max(fill) - min(fill)

    def lit(color):
        if chroma and spread >= 60 and max(color) - min(color) < 0.4 * spread:
            return False
        return sign * (luma(color) - bg) >= delta

    columns = range(max(0, int(x - margin * scale)), min(image.width, int(x + w + margin * scale)))

    def clear(row, near):
        """Whether nothing in avoid covers row near columns near (a range
        of columns): a slot's tile under its key hint leaves no panel
        there to find the edge against."""
        return not any(top <= row < bottom and left < near.stop and right > near.start
                       for left, top, right, bottom in avoid)

    def scan(scan_rows):
        lefts, rights = [], []
        for row in scan_rows:
            flags = [lit(data[c, row]) for c in columns]
            if not any(flags):
                continue
            first, last = columns.start + flags.index(True), columns.start + len(flags) - flags[::-1].index(True)
            # Each edge only from a row whose scan reaches it over the
            # panel: nothing avoided between the scan's end and the edge.
            if clear(row, range(columns.start, first + 1)):
                lefts.append(first)
            if clear(row, range(last - 1, columns.stop)):
                rights.append(last)
        return lefts, rights

    lefts, rights = scan(range(int(y + h * 0.35), int(y + h * 0.65) + 1))
    if avoid and not (lefts and rights):
        # The middle rows are covered at an edge: the rows above them,
        # under the rounded corner's last pixel of curve.
        more_lefts, more_rights = scan(range(int(y + 2 * scale), int(y + h * 0.35)))
        lefts, rights = lefts or more_lefts, rights or more_rights
    rows = range(max(0, int(y - margin * scale)), min(image.height, int(y + h + margin * scale)))
    tops, bottoms = [], []
    # Down the middle columns, clear of the rounded corners: a label or
    # glyph there is inside the run, which only its ends decide.
    for column in range(int(x + w / 2 - scale), int(x + w / 2 + scale) + 1):
        flags = [lit(data[column, r]) for r in rows]
        if any(flags):
            tops.append(rows.start + flags.index(True))
            bottoms.append(rows.start + len(flags) - flags[::-1].index(True))
    if not (lefts and rights and tops):
        return None
    left, right = statistics.median(lefts), statistics.median(rights)
    top, bottom = statistics.median(tops), statistics.median(bottoms)
    return tuple(v / scale for v in (left, top, right - left, bottom - top))


def bottom_line_alpha(image, rect, fill, scale=1.0):
    """The black line inset along a regular cap's bottom, as an alpha in
    levels, with the cap's 1px ring lying over it (the reference's order):
    the ring's alpha is read on the cap's top row, removed from the bottom
    row, and what is left is the line's darkening of the fill. Rows are
    read across the cap clear of its rounded corners (and of its label,
    which never reaches its top or bottom rows), each against the fill two
    rows inside it, since the panel's sheen shades the fill down the cap.
    Only a cap on whole pixels has a bottom row to read; None otherwise.
    (`fill` is unused: kept for the call's symmetry with the other
    measures.)"""
    x, y, w, h = scaled(rect, scale)
    if abs(y + h - round(y + h)) > 0.01 or abs(y - round(y)) > 0.01:
        return None
    data = image.load()
    across = range(int(x + 5 * scale), int(x + w - 5 * scale))

    def row_color(r):
        return tuple(statistics.median(data[c, r][i] for c in across) for i in range(3))

    top_row, bottom_row = int(round(y)), int(round(y + h)) - 1
    ring = (white_alpha(row_color(top_row), row_color(top_row + 2)) or 0) / 255
    under = tuple((b - 255 * ring) / (1 - ring) for b in row_color(bottom_row))
    return black_alpha(under, row_color(bottom_row - 2))


def key_alpha(image, rect, scale=1.0, lighter=True, avoid=()):
    """A regular cap's fill as an overlay's alpha (levels), measured column
    by column as a row's wash is: the cap's top interior (inside its ring,
    above its label) against the panel just above and just below the cap,
    interpolated to that height. The reference's glass shades across and
    down the panel, so each column is read against the panel beside it;
    the median over the columns is the cap's. None if unreadable."""
    x, y, w, h = scaled(rect, scale)
    data = image.load()
    top, bottom = int(round(y)), int(round(y + h))
    above_row, below_row = top - int(round(2 * scale)), bottom + int(round(1 * scale)) - 1
    rows = (top + int(round(2 * scale)), top + int(round(3 * scale)))
    if above_row < 0 or below_row >= image.height:
        return None
    weight = (statistics.mean(rows) - above_row) / (below_row - above_row)
    alphas = []

    def covered(column, row):
        return any(left <= column < right and top <= row < bottom for left, top, right, bottom in avoid)

    for column in range(int(x + 4 * scale), int(x + w - 4 * scale)):
        if not 0 <= column < image.width or any(covered(column, r) for r in rows):
            continue
        above, below = data[column, above_row], data[column, below_row]
        # Where something lies under the cap's bottom (a slot's tile), the
        # panel above the cap is its background alone.
        if covered(column, below_row):
            below = above
        background = tuple(a + (b - a) * weight for a, b in zip(above, below))
        fill = tuple(statistics.mean(data[column, r][i] for r in rows) for i in range(3))
        alpha = overlay_alpha(fill, background, lighter)
        if alpha is not None:
            alphas.append(alpha)
    return statistics.median(alphas) if alphas else None


def measure_key(image, rect, accent, text_color, scale=1.0, lighter=True, avoid=()):
    """One keycap: its edges, its fill (as a white overlay's alpha, or its
    color for an accent cap), the line along its bottom, and its label's
    ink box relative to the cap. avoid: logical rects of what lies under
    or beside the cap (a slot's tile under its key hint), each grown by
    its drop shadow, which the cap's measures read around."""
    x, y, w, h = scaled(rect, scale)
    avoid = [box_px(as_tuple(r), scale, pad=3) for r in avoid]
    fill_bottom = min([y + h - 3 * scale] + [top for left, top, right, bottom in avoid
                                             if left < x + 3 * scale and right > x + 1.5 * scale])
    above = median_color(image, (x + 2 * scale, y - 4 * scale, x + w - 2 * scale, y - 2 * scale))
    below = median_color(image, (x + 2 * scale, y + h + 2 * scale, x + w - 2 * scale, y + h + 4 * scale))
    fill = median_color(image, (x + 1.5 * scale, y + 2 * scale, x + 3 * scale, max(y + 3 * scale, fill_bottom)))
    if above is None or fill is None:
        return None
    if any(left < x + w and right > x and top < y + h + 4 * scale and bottom > y + h
           for left, top, right, bottom in avoid):
        below = None
    # The panel under a cap shades down the panel (the sheen); the cap's
    # fill is read against the panel beside it on both sides.
    background = tuple((a + b) / 2 for a, b in zip(above, below or above))
    edges = box_edges(image, rect, background, fill, scale, avoid=avoid)
    label_bottom = min([y + h - 2 * scale] + [top for left, top, right, bottom in avoid
                                              if left < x + w and right > x and top > y])
    label = ink(image, (x + 2 * scale, y + 2 * scale, x + w - 2 * scale, label_bottom), fill, text_color,
                exclude_accent=False)
    return {
        "edges": edges,
        "fill": fill,
        "alpha": None if accent else key_alpha(image, rect, scale, lighter, avoid),
        # The estimate assumes a lightening ring over the darkening line,
        # the dark palette's caps.
        "bottom": None if accent or not lighter else bottom_line_alpha(image, rect, fill, scale),
        # The label's place relative to the cap as drawn: its measured left
        # edge, where a cap's fractional declared left snapped.
        "label": None if label is None else {
            "left": label["box"][0] / scale - (edges[0] if edges else x / scale),
            "top": (label["box"][1] - y) / scale,
            "height": label["box"][3] / scale,
        },
    }


# Positions the fixture declares through a chain of shaped widths (the
# footer's hint, its rule, the primary action after Actions): layout rounds
# each part to whole pixels, so a declared position drifts by up to a pixel
# and a half; parity measures the real offset at 1px.
CHAIN_SLACK_PX = 1.5
CHAIN_NOTE = "declared through a chain of shaped widths, which layout rounds part by part"


def compare_key_group(report, name, capture, group, manifest, native, scale, ref_group, reference_image, crops,
                      harness=True, chained=False, footer=False, static=None, avoid=(), ref_avoid=()):
    """One key sequence: each cap against its declaration, then - when the
    reference shows the same effective binding - against the reference's
    caps: labels, sizes, fills, the bottom line and the label's place."""
    colors = manifest["declared"]["colors"]
    accent = group["style"] == "accent"
    text = hex_rgba(colors["accentInk"] if accent else colors["keycapText"])[:3]
    subject = f"keys:{group['binding']}"
    declared_fill = hex_rgba(colors["accent"] if accent else colors["keycapBackground"])
    lighter = palette(manifest)["lighter"]
    native_caps = []
    for index, cap in enumerate(group["caps"]):
        rect = as_tuple(cap["rect"])
        measured = measure_key(native, rect, accent, text, scale, lighter, avoid)
        native_caps.append(measured)
        if not harness:
            continue
        edges = measured and measured["edges"]
        props = (("left", 0), ("top", 1), ("width", 2)) + ((("height", 3),) if accent else ())
        for prop, i in props:
            slack = chained and prop == "left"
            report.check("harness-native", name, capture, subject, f"cap {index} {prop}",
                         edges[i] if edges else None, rect[i], CHAIN_SLACK_PX if slack else LIMITS["edge_px"], "px",
                         CHAIN_NOTE if slack else None)
        if accent:
            report.check("harness-native", name, capture, subject, f"cap {index} accent fill (max channel)",
                         max(abs(a - b) for a, b in zip(measured["fill"], declared_fill[:3])) if measured else None,
                         0, LIMITS["flat_fill_levels"], "levels")
        else:
            report.check("harness-native", name, capture, subject, f"cap {index} fill alpha",
                         measured and measured["alpha"], declared_fill[3], LIMITS["flat_fill_levels"], "levels")
            if lighter and not footer:
                report.check("harness-native", name, capture, subject, f"cap {index} bottom line alpha",
                             measured and measured["bottom"], hex_rgba(colors["keycapBottom"])[3],
                             BOTTOM_LINE_LIMIT, "levels", BOTTOM_LINE_NOTE)
    crops.append((subject, as_tuple(group["rect"]), None))
    if not ref_group or reference_image is None:
        return
    ref_rect = as_tuple(ref_group["rect"])
    crops.append(("parity-" + subject, as_tuple(group["rect"]), ref_rect))
    ref_labels = [c["label"] for c in ref_group["caps"]]
    report.check("parity", name, capture, subject, "labels", " | ".join(c["label"] for c in group["caps"]),
                 " | ".join(ref_labels), 0, "", "one cap per key, the same effective binding on both sides")
    family = ref_group["font"]["family"].split(",")[0].strip().strip('"')
    report.check("parity", name, capture, subject, "label font", group["font"],
                 f"{ref_group['font']['size']} {family} {ref_group['font']['weight']}", 0, "")
    n_first, n_last = native_caps[0], native_caps[-1]
    if n_first and n_last and n_first["edges"] and n_last["edges"]:
        width = n_last["edges"][0] + n_last["edges"][2] - n_first["edges"][0]
        report.check("parity", name, capture, subject, "group width", width, ref_rect[2], LIMITS["edge_px"], "px")
    for index, (cap, ref_cap) in enumerate(zip(group["caps"], ref_group["caps"])):
        mine = native_caps[index]
        theirs = measure_key(reference_image, as_tuple(ref_cap["rect"]), accent, text, avoid=ref_avoid)
        if not mine or not theirs:
            continue
        if mine["edges"] and theirs["edges"]:
            report.check("parity", name, capture, subject, f"cap {index} width", mine["edges"][2], theirs["edges"][2],
                         LIMITS["edge_px"], "px")
            if accent:
                report.check("parity", name, capture, subject, f"cap {index} height", mine["edges"][3],
                             theirs["edges"][3], LIMITS["edge_px"], "px")
        if accent:
            report.check("parity", name, capture, subject, f"cap {index} accent fill (max channel)",
                         max(abs(a - b) for a, b in zip(mine["fill"], theirs["fill"])), 0,
                         LIMITS["flat_fill_levels"], "levels",
                         f"native {fmt_color(mine['fill'])} vs reference {fmt_color(theirs['fill'])}",
                         accepted=static)
        else:
            report.check("parity", name, capture, subject, f"cap {index} fill alpha", mine["alpha"], theirs["alpha"],
                         LIMITS["flat_fill_levels"], "levels")
            if theirs["bottom"] is not None:
                report.check("parity", name, capture, subject, f"cap {index} bottom line alpha", mine["bottom"],
                             theirs["bottom"], BOTTOM_LINE_LIMIT, "levels", BOTTOM_LINE_NOTE)
        if mine["label"] and theirs["label"]:
            subset = SUBSET_DISPOSITION if not cap["label"].isascii() else None
            for prop in ("left", "top", "height"):
                report.check("parity", name, capture, subject, f"cap {index} label ink {prop}", mine["label"][prop],
                             theirs["label"][prop], LIMITS["edge_px"], "px", accepted=subset)


# The bottom-line estimate removes the ring and divides by the fill, so each
# level of the bottom row moves it by about 255 / fill (about 5 levels on
# the dark panel): an estimate's limit, not a flat fill's.
BOTTOM_LINE_LIMIT = 12
BOTTOM_LINE_NOTE = "the black 35% inset along the cap's bottom, under its ring; a two-overlay estimate"
# The reference embeds Geist and Geist Mono as 225-glyph subsets with no
# arrows and no return symbol, so Chrome draws those labels in a system
# fallback face; Pane draws Geist Mono's own glyphs.
SUBSET_DISPOSITION = ("accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without "
                      "this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own")


def compare_keycaps(report, name, capture, manifest, native, scale, ref_capture, reference_image, crops):
    groups = (ref_capture or {}).get("state", {}).get("keycaps", {}) if ref_capture else {}
    for group in manifest["keycaps"]:
        compare_key_group(report, name, capture, group, manifest, native, scale,
                          groups.get(group["group"]) if group["group"] else None, reference_image, crops)


def coverage_box(image, box, color):
    """The (x, y, w, h) image-px box of the columns and rows inside box
    (left, top, right, bottom) holding a pixel's worth of ink by coverage
    (see coverage_edges), or None. Each row's background is read at that
    row's ends, the box's two outermost columns on either side: an app
    tile's gradient changes down the tile, never across it, and the box
    lies clear of the glyph there."""
    left, top, right, bottom = clamp_box(image, box)
    if right - left < 6 or bottom <= top:
        return None
    data = image.load()
    fg = luma(color)

    def row_cover(y):
        values = [luma(data[x, y]) for x in range(left, right)]
        bg = statistics.median(values[:2] + values[-2:])
        if fg == bg:
            return [0.0] * len(values)
        amounts = (min(1.0, max(0.0, (value - bg) / (fg - bg))) for value in values)
        return [amount if amount >= 0.2 else 0.0 for amount in amounts]

    cover = [row_cover(y) for y in range(top, bottom)]
    columns = [i for i in range(right - left) if sum(row[i] for row in cover) >= 1.0]
    rows = [i for i, row in enumerate(cover) if sum(row) >= 1.0]
    if not columns or not rows:
        return None
    return (left + columns[0], top + rows[0], columns[-1] - columns[0] + 1, rows[-1] - rows[0] + 1)


def measure_tile(image, rect, app, glyph_color, background, scale=1.0):
    """An icon tile: its edges, its gradient's two ends (an app tile) or its
    fill's overlay alpha (a command tile), and its glyph's core ink - box
    relative to the tile and core pixel count (the stroke's weight)."""
    x, y, w, h = scaled(rect, scale)
    # The gradient's ends a quarter of the way across, where the tile's top
    # and bottom edges are straight (its corner is a quarter of its side,
    # 11 of 42), and clear of the glyph in its middle.
    across = x + w / 4
    top = median_color(image, (across, y + 3 * scale, across + 2 * scale, y + 5 * scale))
    bottom = median_color(image, (across, y + h - 6 * scale, across + 2 * scale, y + h - 4 * scale))
    middle = median_color(image, (x + 3 * scale, y + h / 2 - 2 * scale, x + 5 * scale, y + h / 2 + 2 * scale))
    if top is None or middle is None:
        return None
    inner = (x + 2 * scale, y + 2 * scale, x + w - 2 * scale, y + h - 2 * scale)
    glyph = ink(image, inner, middle, glyph_color, exclude_accent=False)
    # The glyph's box by coverage (a pixel's worth of ink per column or
    # row), not by core pixels: GPUI lifts an icon's anti-aliased edges
    # (see SPRITE_CONTRAST), which moves a core extent by an edge pixel on
    # each side, and a coverage extent by a fraction.
    # Clear of the rounded corners' ring and of a slot's key hint over the
    # tile's corner: every glyph sits well inside its tile (22 in 42, 16 in
    # 28, 11 in 18).
    clear = 0.15 * w
    covered = coverage_box(image, (x + clear, y + clear, x + w - clear, y + h - clear), glyph_color)
    if glyph is not None and covered is not None:
        glyph = dict(glyph, box=covered)
    return {
        "edges": box_edges(image, rect, background, middle, scale, chroma=app),
        "top": top,
        "bottom": bottom,
        "alpha": None if app else white_alpha(middle, background),
        "glyph": None if glyph is None else {
            "left": (glyph["box"][0] - x) / scale,
            "top": (glyph["box"][1] - y) / scale,
            "width": glyph["box"][2] / scale,
            "height": glyph["box"][3] / scale,
            "count": glyph["count"] / (scale * scale),
            "color": glyph["color"],
        },
    }


# The reference's app tile glyphs are white on their gradients; a command
# tile's are #E9E9EC.
APP_GLYPH, COMMAND_GLYPH = (255, 255, 255), (233, 233, 236)
# How far a glyph's core pixel count may differ between the two
# rasterizers (resvg's in GPUI, Skia's in Chrome) for the same path and
# stroke, in percent: an engineering limit, not a reference value. The
# 2px application stroke carries about 25% more core than the 1.6px one,
# so a wrong stroke width still fails it.
STROKE_LIMIT = 25


def compare_row_tile(report, name, capture, subject, row, ref_row, manifest, native, scale, reference_image,
                     measured, ref_measured, crops):
    """A row's icon tile, native against reference: where it lies, its
    fills and its glyph."""
    geometry = manifest["declared"]["geometry"]
    x, y, w, h = as_tuple(row["rect"])
    side = geometry["tileSize"]
    n_rect = (x + geometry["rowPaddingX"], y + (h - side) / 2, side, side)
    r_rect = as_tuple(ref_row.get("tile")) if ref_row.get("tile") else None
    if r_rect is None or not measured or not ref_measured:
        return
    app = ref_row.get("tileApp", None)
    if app is None:
        app = row.get("tone", "command") != "command"
    color = APP_GLYPH if app else COMMAND_GLYPH
    n_bg = native_wash_fill(native, row, scale)
    r_bg = native_wash_fill(reference_image, ref_row, 1.0)
    mine = measure_tile(native, n_rect, app, color, n_bg, scale)
    theirs = measure_tile(reference_image, r_rect, app, color, r_bg)
    if not mine or not theirs:
        return
    sub = subject + "/tile"
    crops.append(("parity-tile-" + row["title"], n_rect, r_rect))
    if mine["edges"] and theirs["edges"]:
        for prop, i in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
            n_value = mine["edges"][i] - (x if i == 0 else y if i == 1 else 0)
            r_value = theirs["edges"][i] - (as_tuple(ref_row["rect"])[0] if i == 0 else
                                            as_tuple(ref_row["rect"])[1] if i == 1 else 0)
            report.check("parity", name, capture, sub, f"{prop}{' in row' if i < 2 else ''}", n_value, r_value,
                         LIMITS["edge_px"], "px")
    if app:
        # A gradient is not a flat fill: across a 28px tile it changes about
        # 1.5 levels a row, so half a pixel of placement moves a sample by
        # about that much. The glyph-core limit applies.
        for end in ("top", "bottom"):
            report.check("parity", name, capture, sub, f"gradient {end} (max channel)",
                         max(abs(a - b) for a, b in zip(mine[end], theirs[end])), 0, LIMITS["glyph_core_levels"],
                         "levels", f"native {fmt_color(mine[end])} vs reference {fmt_color(theirs[end])}")
    else:
        report.check("parity", name, capture, sub, "fill alpha", mine["alpha"], theirs["alpha"],
                     LIMITS["flat_fill_levels"], "levels")
    if mine["glyph"] and theirs["glyph"]:
        for prop in ("left", "top", "width", "height"):
            report.check("parity", name, capture, sub, f"glyph ink {prop}", mine["glyph"][prop],
                         theirs["glyph"][prop], LIMITS["edge_px"], "px")
        weight = 100 * (mine["glyph"]["count"] / max(1, theirs["glyph"]["count"]) - 1)
        report.check("parity", name, capture, sub, "glyph core pixels (stroke weight)", weight, 0, STROKE_LIMIT, "%",
                     f"{mine['glyph']['count']:.0f} vs {theirs['glyph']['count']:.0f} core pixels",
                     accepted=SPRITE_CONTRAST if weight > 0 else None)


def kind_ink(image, rect, fill, scale=1.0, pal=DARK):
    """A row's kind text: its core ink box, read in the row's right part."""
    x, y, w, h = scaled(rect, scale)
    found = ink(image, (x, y + 4 * scale, x + w + 2 * scale, y + h - 4 * scale), fill, pal["muted"],
                exclude_accent=False)
    if not found:
        return None
    bx, by, bw, bh = found["box"]
    return {"right": (bx + bw) / scale, "top": by / scale, "height": bh / scale}


def measure_alias(image, rect, fill, scale=1.0, pal=DARK):
    """An alias chip: its ring's box (the chip's own edges) and its
    label's ink, near its declared place in the row."""
    x, y, w, h = scaled(rect, scale)
    middle = y + h / 2
    chip_rect = (x / scale, (middle - 9 * scale) / scale, w / scale, 18)
    background = median_color(image, (x - 6 * scale, middle - 2 * scale, x - 3 * scale, middle + 2 * scale)) or fill
    # The chip's ring is its only fill: the column near its declared left
    # that stands out most from the row is the ring's.
    columns = [median_color(image, (column, middle - 3 * scale, column + 1, middle + 3 * scale))
               for column in range(int(x - 3 * scale), int(x + 3 * scale) + 1)]
    columns = [color for color in columns if color]
    ring = max(columns, key=lambda color: abs(luma(color) - luma(background))) if columns else None
    edges = box_edges(image, chip_rect, background, ring, scale, margin=3) if ring else None
    label_box = (x + 2 * scale, middle - 8 * scale, x + w - 2 * scale, middle + 8 * scale)
    label = ink(image, label_box, background, pal["alias"], exclude_accent=False)
    # The label's top from ink_top: a thin ascender (the 'b' of "cb") lands
    # on a renderer's own subpixel phase, which ink()'s core alone misreads
    # as the label sitting lower.
    if label is None:
        return {"edges": edges, "label": None}
    lx, ly, _, lh = label["box"]
    top = min(ly, ink_top(image, label_box, background, pal["alias"]) or ly)
    return {
        "edges": edges,
        "label": {"left": lx / scale, "top": top / scale, "height": (ly + lh - top) / scale},
    }


def harness_row_trailing(report, name, capture, row, manifest, native, scale, crops):
    """A native row's trailing parts against their declaration: the kind's
    text ends at the row's right padding, the alias chip lies where it is
    declared, and the key sequence's caps (see compare_key_group)."""
    subject = f"row:{row['title']}"
    fill = native_wash_fill(native, row, scale)
    if fill is None:
        return
    if row.get("kindRect"):
        rect = as_tuple(row["kindRect"])
        found = kind_ink(native, rect, fill, scale, palette(manifest))
        # Text is right-aligned in its box: its ink ends within a pixel of
        # the box's right (a glyph's right side bearing).
        report.check("harness-native", name, capture, subject, "kind ink right",
                     found and found["right"], rect[0] + rect[2], LIMITS["edge_px"] * 1.5, "px",
                     "right-aligned at the row's padding; a glyph's side bearing allowed")
    if row.get("aliasRect"):
        rect = as_tuple(row["aliasRect"])
        measured = measure_alias(native, rect, fill, scale, palette(manifest))
        edges = measured["edges"]
        report.check("harness-native", name, capture, subject, "alias chip width",
                     edges[2] if edges else None, rect[2], LIMITS["edge_px"], "px")
        # Its place: the row's gap before what follows it - the key
        # sequence's first cap, measured, or the kind's declared box. (The
        # caps' widths round to whole pixels in layout, so declared
        # positions drift by fractions of a pixel per cap.)
        following = row["kindRect"]["x"] if row.get("kindRect") else None
        if row.get("keyGroup"):
            first = as_tuple(row["keyGroup"]["caps"][0]["rect"])
            x, y, w, h = scaled(first, scale)
            key_fill = median_color(native, (x + 1.5 * scale, y + 2 * scale, x + 3 * scale, y + h - 3 * scale))
            key_edges = box_edges(native, first, fill, key_fill, scale) if key_fill else None
            following = key_edges[0] if key_edges else None
        gap = manifest["declared"]["geometry"].get("rowGap", 12)
        report.check("harness-native", name, capture, subject, "alias chip gap to what follows",
                     following - (edges[0] + edges[2]) if edges and following is not None else None, gap,
                     LIMITS["edge_px"], "px")
        # What comes before it - a title or reason however long - truncates
        # or wraps short of the gap: no text ink in the gap before the chip.
        if edges:
            _, ry, _, rh = scaled(as_tuple(row["rect"]), scale)
            left = (edges[0] - gap + 1) * scale
            stray = ink(native, (left, ry, edges[0] * scale - 1, ry + rh), fill, palette(manifest)["muted"],
                        exclude_accent=False)
            report.check("harness-native", name, capture, subject, "text ink in the gap before the alias chip",
                         0 if stray is None else stray["count"], 0, 0, "px")
    if row.get("keyGroup"):
        compare_key_group(report, name, capture, row["keyGroup"], manifest, native, scale, None, None, crops)


def compare_row_trailing(report, name, capture, subject, row, ref_row, manifest, native, scale, reference_image,
                         measured, ref_measured, crops):
    """A row's trailing parts, native against reference: the kind's text,
    the alias chip and the key sequence."""
    n_fill = native_wash_fill(native, row, scale)
    r_fill = native_wash_fill(reference_image, ref_row, 1.0)
    if n_fill is None or r_fill is None:
        return
    n_row, r_row = as_tuple(row["rect"]), as_tuple(ref_row["rect"])
    if row.get("kindRect") and ref_row.get("kind"):
        # Each side's kind box across the row's own height: the reference's
        # element is only its text's height.
        kx, _, kw, _ = as_tuple(row["kindRect"])
        rx, _, rw, _ = as_tuple(ref_row["kind"]["rect"])
        mine = kind_ink(native, (kx, n_row[1], kw, n_row[3]), n_fill, scale, palette(manifest))
        theirs = kind_ink(reference_image, (rx, r_row[1], rw, r_row[3]), r_fill)
        report.check("parity", name, capture, subject, "kind", row["kind"], ref_row["kind"]["text"], 0, "")
        if mine and theirs:
            report.check("parity", name, capture, subject, "kind ink right in row", mine["right"] - n_row[0],
                         theirs["right"] - r_row[0], LIMITS["edge_px"], "px")
            report.check("parity", name, capture, subject, "kind ink top in row", mine["top"] - n_row[1],
                         theirs["top"] - r_row[1], LIMITS["edge_px"], "px")
            report.check("parity", name, capture, subject, "kind ink height", mine["height"], theirs["height"],
                         LIMITS["edge_px"], "px")
    if row.get("aliasRect") and ref_row.get("alias"):
        ax, _, aw, _ = as_tuple(row["aliasRect"])
        bx, _, bw, _ = as_tuple(ref_row["alias"]["rect"])
        mine = measure_alias(native, (ax, n_row[1], aw, n_row[3]), n_fill, scale, palette(manifest))
        theirs = measure_alias(reference_image, (bx, r_row[1], bw, r_row[3]), r_fill)
        report.check("parity", name, capture, subject, "alias", row["alias"], ref_row["alias"]["text"], 0, "")
        if mine["edges"] and theirs["edges"]:
            for prop, index, origin in (("left in row", 0, 0), ("top in row", 1, 1), ("width", 2, None),
                                        ("height", 3, None)):
                n_value = mine["edges"][index] - (n_row[origin] if origin is not None else 0)
                r_value = theirs["edges"][index] - (r_row[origin] if origin is not None else 0)
                report.check("parity", name, capture, subject, f"alias chip {prop}", n_value, r_value,
                             LIMITS["edge_px"], "px")
        if mine["label"] and theirs["label"]:
            report.check("parity", name, capture, subject, "alias label ink top in row",
                         mine["label"]["top"] - n_row[1], theirs["label"]["top"] - r_row[1], LIMITS["edge_px"], "px")
        crops.append(("parity-alias-" + row["title"], as_tuple(row["aliasRect"]), as_tuple(ref_row["alias"]["rect"])))
    if row.get("keyGroup") and ref_row.get("keyGroup"):
        compare_key_group(report, name, capture, row["keyGroup"], manifest, native, scale, ref_row["keyGroup"],
                          reference_image, crops, harness=False)


# The reference's blank-query list, labelled with its fixture's own story:
# a pinned strip (#101) and "Suggested · From your recent use" above its
# commands. Pane labels a blank query's list as what it is - its commands,
# in root search's order - and claims no recent use (#100).
SECTION_DISPOSITION = ("accepted (#94, #100): Pane labels a blank query's rows \"Commands\" and claims no recent "
                       "use, so the reference's \"Suggested · From your recent use\" is not shown; both sides show "
                       "the pinned strip's \"Pinned\" label (#101)")


def section_ink(image, rect, scale=1.0, pal=DARK):
    x, y, w, h = scaled(rect, scale)
    background = median_color(image, (x + w * 0.4, y + 2 * scale, x + w * 0.5, y + 5 * scale))
    if background is None:
        return None, None
    left = ink(image, (x, y + 6 * scale, x + w * 0.5, y + h), background, pal["muted"], exclude_accent=False)
    right = ink(image, (x + w * 0.5, y + 6 * scale, x + w + 2 * scale, y + h), background, pal["muted"],
                exclude_accent=False)
    return left, right


def compare_sections(report, name, capture, declared, state, native, scale, reference_image, crops):
    """The section labels: which labels show over the rows, and the
    typography of a label both sides show."""
    native_labels = declared.get("sections") or []
    ref_labels = [label for label in state.get("labels", []) if label.get("visible", True)]
    report.check("parity", name, capture, "sections", "labels",
                 " | ".join(label["label"] for label in native_labels),
                 " | ".join(label["text"] for label in ref_labels), 0, "",
                 accepted=SECTION_DISPOSITION if not state["query"] else None)
    by_text = {label["text"]: label for label in ref_labels}
    for label in native_labels:
        ref = by_text.get(label["label"])
        if not ref:
            continue
        subject = f"section:{label['label']}"
        n_rect, r_rect = as_tuple(label["rect"]), as_tuple(ref["rect"])
        crops.append(("parity-" + subject, n_rect, r_rect))
        n_left, n_right = section_ink(native, n_rect, scale)
        r_left, r_right = section_ink(reference_image, r_rect)
        report.check("parity", name, capture, subject, "note", label.get("note") or "", ref.get("note") or "", 0, "",
                     accepted=SECTION_NOTES.get(label["label"]))
        report.check("parity", name, capture, subject, "label height", n_rect[3], r_rect[3], LIMITS["edge_px"], "px")
        if n_left and r_left:
            for prop, index in (("left", 0), ("top", 1)):
                report.check("parity", name, capture, subject, f"title ink {prop} in label",
                             n_left["box"][index] / scale - n_rect[index], r_left["box"][index] - r_rect[index],
                             LIMITS["edge_px"], "px")
            report.check("parity", name, capture, subject, "title ink height", n_left["box"][3] / scale,
                         r_left["box"][3], LIMITS["edge_px"], "px")
        if n_right and r_right and (label.get("note") or ""):
            report.check("parity", name, capture, subject, "note ink right in label",
                         (n_right["box"][0] + n_right["box"][2]) / scale - n_rect[0],
                         r_right["box"][0] + r_right["box"][2] - r_rect[0], LIMITS["edge_px"] * 1.5, "px",
                         "right-aligned; a glyph's side bearing allowed")


# ------------------------------------------------------ the pinned home (#101)

# One section label and the list's gap after it: what a label the other
# side does not show moves the rows below it by.
LABEL_STEP = 32
EXTRA_LABEL_DISPOSITION = ("accepted (#100): the reference's \"Suggested · From your recent use\" label sits above "
                           "this row; Pane claims no recent use, so its row sits one label higher")
# The slot title's color both sides author (#D9DADD).
SLOT_TITLE = (217, 218, 221)


def extra_labels_above(declared, state, native_y, reference_y):
    """How many more section labels the reference shows above its row at
    reference_y than the native capture declares above its row at
    native_y: each one moves the reference's row down by LABEL_STEP."""
    native = sum(1 for label in declared.get("sections") or [] if as_tuple(label["rect"])[1] < native_y)
    reference = sum(1 for label in (state or {}).get("labels", []) if as_tuple(label["rect"])[1] < reference_y)
    return reference - native


def local_edges(image, approx, scale=1.0, threshold=6.0, out=4, corner=14):
    """A box's edges around approx = (x, y, w, h), image px: each edge is
    where a scan line, walking in from out px outside that edge, first
    leaves the background it started on by threshold luma levels (either
    way: a white inset edge or fill lightens the dark panel, and darkens
    nothing in the light palette's).

    Each scan line keeps its own starting background, so a backdrop that
    changes across or down the panel (the reference's glass) never moves
    an edge. out must stay inside the space around the box (a pinned
    slot's 8px column gap: 4), so a neighbor is never read as the box.
    Scan lines run corner px clear of the box's rounded corners. The
    medians over the scan lines are the edges. Returns (x, y, w, h) or
    None."""
    x, y, w, h = approx
    data = image.load()
    out = out * scale
    corner = corner * scale

    def walk(points):
        start = luma(data[points[0]])
        for index, point in enumerate(points[1:], 1):
            if abs(luma(data[point]) - start) >= threshold:
                return index
        return None

    def inside(px, py):
        return 0 <= px < image.width and 0 <= py < image.height

    lefts, rights, tops, bottoms = [], [], [], []
    for row in range(int(y + corner), int(y + h - corner)):
        begin, end = int(math.floor(x - out)), int(math.ceil(x + w + out))
        if not (inside(begin, row) and inside(end, row)):
            continue
        step = walk([(c, row) for c in range(begin, end)])
        if step is not None:
            lefts.append(begin + step)
        step = walk([(c, row) for c in range(end, begin, -1)])
        if step is not None:
            rights.append(end - step + 1)
    for column in range(int(x + corner), int(x + w - corner)):
        begin, end = int(math.floor(y - out)), int(math.ceil(y + h + out))
        if not (inside(column, begin) and inside(column, end)):
            continue
        step = walk([(column, r) for r in range(begin, end)])
        if step is not None:
            tops.append(begin + step)
        step = walk([(column, r) for r in range(end, begin, -1)])
        if step is not None:
            bottoms.append(end - step + 1)
    if not (lefts and rights and tops and bottoms):
        return None
    left, right = statistics.median(lefts), statistics.median(rights)
    top, bottom = statistics.median(tops), statistics.median(bottoms)
    return (left, top, right - left, bottom - top)


def measure_slot(image, rect, title_rect, tile_rect, app, title_color=SLOT_TITLE, scale=1.0, lighter=True):
    """A pinned slot: its fill (measured as a row's wash is, against the
    strip's padding just above and below it), its box, its title's core
    ink - the center of its extent, its top and height - and its tile.
    title_rect and tile_rect may be None (an empty slot). Returns None
    where the slot cannot be read."""
    wash = row_wash(image, rect, scale, False, lighter)
    if wash is None:
        return None
    result = {"alpha": wash["alpha"], "fill": wash["fill"], "edges": None, "title": None, "tile": None}
    edges = local_edges(image, scaled(rect, scale), scale)
    if edges:
        result["edges"] = tuple(v / scale for v in edges)
    if title_rect is not None:
        x, y, w, h = scaled(as_tuple(title_rect), scale)
        found = ink(image, (x - 2 * scale, y - 3 * scale, x + w + 2 * scale, y + h + 3 * scale), wash["fill"],
                    title_color, exclude_accent=False)
        if found:
            bx, by, bw, bh = found["box"]
            result["title"] = {"center": (bx + bw / 2) / scale, "top": by / scale, "height": bh / scale,
                               "left": bx / scale, "right": (bx + bw) / scale, "color": found["color"]}
    if tile_rect is not None:
        result["tile"] = measure_tile(image, as_tuple(tile_rect), app, APP_GLYPH if app else COMMAND_GLYPH,
                                      wash["fill"], scale)
    return result


def coverage_center(image, rect, background, color, scale=1.0):
    """The center of text's ink extent across rect, by coverage: the first
    and last columns holding a pixel's worth of ink (see coverage_edges).
    Logical px, or None."""
    left, top, right, bottom = clamp_box(image, box_px(rect, scale, pad=2))
    bg, fg = luma(background), luma(color)
    if fg == bg or right <= left:
        return None
    data = image.load()

    def coverage(value):
        amount = min(1.0, max(0.0, (luma(value) - bg) / (fg - bg)))
        return amount if amount >= 0.2 else 0.0

    columns = [sum(coverage(data[x, y]) for y in range(top, bottom)) for x in range(left, right)]
    inked = [i for i, total in enumerate(columns) if total >= 1.0]
    if not inked:
        return None
    return (left + (inked[0] + inked[-1] + 1) / 2) / scale


# GPUI draws every monochrome sprite - text and SVG icons alike - through
# its text's grayscale contrast and gamma correction
# (gpui_render/src/shaders/sprites.rs, fragment_monochrome_sprite), which
# lifts a stroke's partially covered edge pixels; Chrome's Skia leaves them
# linear. A stroke's fully covered pixels match, its edges do not.
SPRITE_CONTRAST = ("accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon "
                   "through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge "
                   "pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against "
                   "211/194 reference at the edges, 255 at the center on both), so the native glyph counts more "
                   "core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails")


def slot_state(slot):
    """A declared slot's state, as its checks name it."""
    if slot.get("title") is None:
        return "empty"
    return "hovered" if slot.get("hovered") else "rest"


def harness_pinned(report, name, capture, declared, manifest, native, scale, pal, crops):
    """The native pinned home against its declaration: each slot's fill
    (white 3.5% at rest, 7% under the pointer, none for an empty slot), its
    box and tile where declared, its title centered across it inside its
    line, and the chords - each slot's and the label's - as key groups."""
    home = declared.get("pinned")
    if not home:
        return
    colors = manifest["declared"]["colors"]
    alphas = {"rest": hex_rgba(colors.get("slotBackground", "#FFFFFF09"))[3],
              "hovered": hex_rgba(colors.get("slotHover", "#FFFFFF12"))[3], "empty": 0}
    title_color = hex_rgba(colors.get("slotTitle", "#D9DADDFF"))[:3]
    # Only what the list shows whole is measured, as with its rows: a
    # scrolled list (the narrow frame's last row, #92) carries the home
    # above its viewport.
    viewport = as_tuple(manifest["list"])
    for slot in home["slots"]:
        subject = f"slot:{slot['number']}"
        rect = as_tuple(slot["rect"])
        if not within(rect, viewport):
            continue
        state = slot_state(slot)
        crops.append((subject, rect, None))
        mine = measure_slot(native, rect, slot.get("titleRect"), slot.get("tile"), slot.get("tone") == "app",
                            title_color, scale, pal["lighter"])
        report.check("harness-native", name, capture, subject, f"fill alpha ({state})", mine and mine["alpha"],
                     alphas[state], LIMITS["flat_fill_levels"], "levels")
        if state == "empty":
            continue
        edges = mine and mine["edges"]
        for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
            report.check("harness-native", name, capture, subject, f"box {prop}", edges[index] if edges else None,
                         rect[index], LIMITS["edge_px"], "px")
        tile = as_tuple(slot["tile"])
        tile_edges = mine and mine["tile"] and mine["tile"]["edges"]
        # A tile's top is read down its middle columns; in a slot too
        # narrow for its key hint to clear the tile (the narrow frame's,
        # 85.6 wide), the hint lies over those columns, so the tile's top
        # and height are not measured there - its left and width are.
        hint = slot.get("keyGroup") and as_tuple(slot["keyGroup"]["rect"])
        middle = tile[0] + tile[2] / 2
        topless = bool(hint) and hint[0] - 1 <= middle <= hint[0] + hint[2] + 1 and hint[1] + hint[3] > tile[1]
        for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
            if topless and prop in ("top", "height"):
                continue
            report.check("harness-native", name, capture, subject, f"tile {prop}",
                         tile_edges[index] if tile_edges else None, tile[index], LIMITS["edge_px"], "px")
        if slot.get("keyGroup"):
            compare_key_group(report, name, capture, slot["keyGroup"], manifest, native, scale, None, None, crops,
                              chained=True, avoid=[tile])
        # An unavailable slot's title is muted, not the title color.
        if slot.get("unavailable"):
            continue
        title = mine and mine["title"]
        line = as_tuple(slot["titleRect"])
        if slot.get("truncated"):
            # An ellipsized title fills its line from the left and ends
            # where its last whole glyph and the ellipsis do: inside the
            # line, not centered.
            inside_x = title and line[0] - LIMITS["edge_px"] <= title["left"] and (
                title["right"] <= line[0] + line[2] + LIMITS["edge_px"])
            report.check("harness-native", name, capture, subject, "truncated title ink inside its line (x)",
                         "yes" if inside_x else "no", "yes", 0, "")
        else:
            report.check("harness-native", name, capture, subject, "title ink center", title and title["center"],
                         rect[0] + rect[2] / 2, LIMITS["edge_px"] * 1.5, "px",
                         "centered across the slot; a glyph's side bearing allowed")
        inside = title and line[1] - LIMITS["edge_px"] <= title["top"] and (
            title["top"] + title["height"] <= line[1] + line[3] + LIMITS["edge_px"])
        report.check("harness-native", name, capture, subject, "title ink inside its line",
                     "yes" if inside else "no", "yes", 0, "")
    if home.get("labelKeys") and within(as_tuple(home["label"]), viewport):
        compare_key_group(report, name, capture, home["labelKeys"], manifest, native, scale, None, None, crops,
                          chained=True)


def within(rect, viewport):
    """Whether rect lies wholly inside viewport (both x, y, w, h)."""
    x, y, w, h = rect
    vx, vy, vw, vh = viewport
    return x >= vx and y >= vy and x + w <= vx + vw and y + h <= vy + vh


def parity_pinned(report, name, capture, declared, manifest, native, scale, state, reference_image, pal, crops):
    """The pinned home, native against reference: shown on both sides or
    neither (a query hides it), the same slots under the pointer, and each
    slot's title, box, fill, title ink, tile and chord, the slot's own
    origin taken on each side."""
    home, ref_home = declared.get("pinned"), state.get("pinned")
    report.check("parity", name, capture, "pinned", "shown", "yes" if home else "no", "yes" if ref_home else "no",
                 0, "", "a blank query shows the strip; a query hides it")
    if not (home and ref_home):
        return
    report.check("parity", name, capture, "pinned", "hovered slots",
                 ", ".join(str(slot["number"]) for slot in home["slots"] if slot.get("hovered")),
                 ", ".join(str(slot["number"]) for slot in ref_home["slots"] if slot.get("hovered")), 0, "")
    if home.get("labelKeys") and ref_home.get("labelKeys"):
        compare_key_group(report, name, capture, home["labelKeys"], manifest, native, scale, ref_home["labelKeys"],
                          reference_image, crops, harness=False, chained=True)
    title_color = hex_rgba(manifest["declared"]["colors"].get("slotTitle", "#D9DADDFF"))[:3]
    ref_slots = {slot["number"]: slot for slot in ref_home["slots"]}
    for slot in home["slots"]:
        ref = ref_slots.get(slot["number"])
        if not ref:
            continue
        subject = f"slot:{slot['number']}"
        report.check("parity", name, capture, subject, "title", slot.get("title") or "", ref.get("title") or "", 0, "")
        if slot.get("title") is None or not ref.get("title"):
            continue
        n_rect, r_rect = as_tuple(slot["rect"]), as_tuple(ref["rect"])
        crops.append(("parity-" + subject, n_rect, r_rect))
        mine = measure_slot(native, n_rect, slot.get("titleRect"), slot.get("tile"), slot.get("tone") == "app",
                            title_color, scale, pal["lighter"])
        theirs = measure_slot(reference_image, r_rect, ref.get("titleRect"), ref.get("tile"), bool(ref.get("tileApp")))
        if not mine or not theirs:
            continue
        report.check("parity", name, capture, subject, "fill alpha", mine["alpha"], theirs["alpha"],
                     LIMITS["flat_fill_levels"], "levels")
        if mine["edges"] and theirs["edges"]:
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                report.check("parity", name, capture, subject, f"box {prop}", mine["edges"][index],
                             theirs["edges"][index], LIMITS["edge_px"], "px")
        n_title, r_title = mine["title"], theirs["title"]
        if n_title and r_title:
            # Centered by coverage: ClearType's colored fringes move a
            # core-pixel extent unevenly at its two ends.
            n_center = coverage_center(native, as_tuple(slot["titleRect"]), mine["fill"], title_color, scale)
            r_center = coverage_center(reference_image, as_tuple(ref["titleRect"]), theirs["fill"], SLOT_TITLE)
            report.check("parity", name, capture, subject, "title ink center in slot",
                         n_center - n_rect[0] if n_center is not None else None,
                         r_center - r_rect[0] if r_center is not None else None, LIMITS["edge_px"], "px",
                         "the extent's center by coverage (a pixel's worth of ink per column)")
            report.check("parity", name, capture, subject, "title ink top in slot", n_title["top"] - n_rect[1],
                         r_title["top"] - r_rect[1], LIMITS["edge_px"], "px")
            report.check("parity", name, capture, subject, "title ink height", n_title["height"], r_title["height"],
                         LIMITS["edge_px"], "px")
            worst = max(abs(a - b) for a, b in zip(n_title["color"], r_title["color"]))
            report.check("parity", name, capture, subject, "title glyph core color (max channel)", worst, 0,
                         LIMITS["glyph_core_levels"], "levels",
                         f"native {fmt_color(n_title['color'])} vs reference {fmt_color(r_title['color'])}")
        n_tile, r_tile = mine["tile"], theirs["tile"]
        if n_tile and r_tile:
            sub = subject + "/tile"
            crops.append(("parity-" + sub, as_tuple(slot["tile"]), as_tuple(ref["tile"])))
            if n_tile["edges"] and r_tile["edges"]:
                for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                    origin = index if index < 2 else None
                    n_value = n_tile["edges"][index] - (n_rect[origin] if origin is not None else 0)
                    r_value = r_tile["edges"][index] - (r_rect[origin] if origin is not None else 0)
                    report.check("parity", name, capture, sub, f"{prop}{' in slot' if index < 2 else ''}", n_value,
                                 r_value, LIMITS["edge_px"], "px")
            for end in ("top", "bottom"):
                if n_tile[end] and r_tile[end]:
                    report.check("parity", name, capture, sub, f"gradient {end} (max channel)",
                                 max(abs(a - b) for a, b in zip(n_tile[end], r_tile[end])), 0,
                                 LIMITS["glyph_core_levels"], "levels",
                                 f"native {fmt_color(n_tile[end])} vs reference {fmt_color(r_tile[end])}")
            if n_tile["glyph"] and r_tile["glyph"]:
                # The glyph's place inside the tile as drawn: layout puts a
                # slot's fractional tile on whole pixels, so its measured
                # edges, not its declared rect, are the origin.
                n_origin, r_origin = (as_tuple(slot["tile"]), as_tuple(ref["tile"]))
                for prop in ("left", "top", "width", "height"):
                    n_value, r_value = n_tile["glyph"][prop], r_tile["glyph"][prop]
                    if prop in ("left", "top") and n_tile["edges"] and r_tile["edges"]:
                        i = 0 if prop == "left" else 1
                        n_value += n_origin[i] - n_tile["edges"][i]
                        r_value += r_origin[i] - r_tile["edges"][i]
                    report.check("parity", name, capture, sub, f"glyph ink {prop}", n_value, r_value,
                                 LIMITS["edge_px"], "px")
                weight = 100 * (n_tile["glyph"]["count"] / max(1, r_tile["glyph"]["count"]) - 1)
                report.check("parity", name, capture, sub, "glyph core pixels (stroke weight)", weight, 0,
                             STROKE_LIMIT, "%",
                             f"{n_tile['glyph']['count']:.0f} vs {r_tile['glyph']['count']:.0f} core pixels",
                             accepted=SPRITE_CONTRAST if weight > 0 else None)
        if slot.get("keyGroup") and ref.get("keyGroup"):
            compare_key_group(report, name, capture, slot["keyGroup"], manifest, native, scale, ref["keyGroup"],
                              reference_image, crops, harness=False, chained=True,
                              avoid=[as_tuple(slot["tile"])], ref_avoid=[as_tuple(ref["tile"])])


# ------------------------------------------------- the footer and Actions (#95)

# Content the reference authors that Pane deliberately does not show: its
# Actions lists operations Pane has no working contract for (#100), and its
# static Actions board's footer carries a tip instead of the hint.
ACTIONS_CONTENT = ("accepted (#95, #100): the reference lists Open New Window, Show in File Manager, Quit and "
                   "Hide from Results; Pane lists only the operations it can perform - the primary action, "
                   "pinning to a quick slot (#101) and an installed command's hotkey and alias")
STATIC_ENTER = ("accepted (#95): the static Actions board draws its footer's Enter as a plain cap; the root "
                "board, live, draws it in the accent, as Pane does (#93)")
ACTIONS_LABEL_COLOR = ("accepted (#95): the reference's boards disagree on the open Actions button's label - the "
                       "static Actions board sets it white (`color: #ffffff`), the root board's live button keeps "
                       ".fbtn's #D9DADD - and Pane draws it white, as the static board does (compared there, it "
                       "passes). Against the root board's dimmer label, the white label's anti-aliased first column "
                       "holds a pixel's worth of ink one column early; the closed button, one color on both "
                       "sides, matches within 1px")
STATIC_HINT = ("accepted (#95): the static Actions board's footer shows a tip ('Every action keeps its shortcut - no "
               "menu needed next time'); Pane's footer shows the hint the root board shows while Actions is open")

# The dimmed list's sample: the list's left padding near its bottom, where
# no row, wash or sheen reaches.
UNDIMMED = {}


def box_px(rect, scale=1.0, pad=0.0):
    """A logical (x, y, w, h) as an image box (left, top, right, bottom),
    grown by pad on every side."""
    x, y, w, h = rect
    return ((x - pad) * scale, (y - pad) * scale, (x + w + pad) * scale, (y + h + pad) * scale)


def ink_extent(image, rect, background, color, scale=1.0, pad=0.0):
    """The core ink of color inside rect (grown by pad): its box in logical
    px (left, top, right, bottom) and its color, or None."""
    found = ink(image, box_px(rect, scale, pad), background, color, exclude_accent=False)
    if not found:
        return None
    left, top, width, height = found["box"]
    return {"left": left / scale, "top": top / scale, "right": (left + width) / scale,
            "bottom": (top + height) / scale, "color": found["color"]}


def brightest(image, box):
    """The color of text's fullest ink inside box (left, top, right,
    bottom): the median of its brightest 2% of pixels, or None."""
    values = sorted(pixels(image, box), key=luma)
    if not values:
        return None
    top = values[-max(1, len(values) // 50):]
    return tuple(statistics.median(channel) for channel in zip(*top))


def coverage_edges(image, rect, background, color, scale=1.0, enough=1.0):
    """Where text's ink begins inside rect, by coverage: each pixel counts
    how far it is from the background toward color (0 to 1), and the left
    edge is the first column, the top the first row, whose pixels add up
    to enough - a pixel's worth of ink. An anti-aliased stem tip or a thin
    diagonal that one renderer leaves just under a core threshold and the
    other just over moves this by a fraction, not a pixel. Logical px
    (left, top), or None."""
    left, top, right, bottom = clamp_box(image, box_px(rect, scale))
    bg, fg = luma(background), luma(color)
    if fg == bg:
        return None
    data = image.load()
    # Below a fifth of the way to the ink, a pixel is the background's own
    # variation (the glass's gradient, the wallpaper's shading), not ink.
    def coverage(value):
        amount = min(1.0, max(0.0, (luma(value) - bg) / (fg - bg)))
        return amount if amount >= 0.2 else 0.0

    cover = [[coverage(data[x, y]) for x in range(left, right)] for y in range(top, bottom)]
    columns = [sum(row[i] for row in cover) for i in range(right - left)]
    rows = [sum(row) for row in cover]
    first_column = next((i for i, total in enumerate(columns) if total >= enough), None)
    first_row = next((i for i, total in enumerate(rows) if total >= enough), None)
    if first_column is None or first_row is None:
        return None
    return {"left": (left + first_column) / scale, "top": (top + first_row) / scale}


def composite(fill_hex, background):
    """The color fill (#RRGGBBAA) paints over background."""
    r, g, b, a = hex_rgba(fill_hex)
    a /= 255
    return tuple(bg * (1 - a) + c * a for c, bg in zip((r, g, b), background))


def color_delta(a, b):
    return max(abs(x - y) for x, y in zip(a, b)) if a and b else None


def footer_strip_background(image, footer, scale=1.0):
    """The footer's own fill: its left padding, which no part reaches."""
    x, y, w, h = footer
    return median_color(image, box_px((x + 2, y + 6, 6, 8), scale))


def measure_footer_parts(image, footer, parts, scale=1.0, pal=DARK, mark_color=(218, 218, 220),
                         divider_color=None):
    """The footer's mark, hint parts, rule and Actions button, measured near
    where parts (the declaration, or the reference's DOM) puts them."""
    background = footer_strip_background(image, footer, scale)
    out = {"background": background}
    if parts.get("mark"):
        out["mark"] = ink_extent(image, as_tuple(parts["mark"]), background, mark_color, scale, pad=2)
    line_top = footer[1] + 1
    line = footer[3] - 1
    hint = []
    for part in parts.get("hint", []):
        x, _, w, _ = as_tuple(part["rect"])
        if part["keys"]:
            cap = (x, line_top + (line - 20) / 2, w, 20)
            measured = measure_cap(image, cap, scale, pal["lighter"])
            edges = measured and measured["edges"]
            hint.append({"text": part["text"], "keys": True, "left": edges[0] if edges else None,
                         "top": edges[1] if edges else None})
        else:
            found = coverage_edges(image, (x - 2, line_top, w + 4, line), background, pal["muted"], scale)
            # The top from the text's interior, clear of a cap's label beside it.
            inner = coverage_edges(image, (x + 3, line_top, max(1, w - 6), line), background, pal["muted"], scale)
            hint.append({"text": part["text"], "keys": False, "left": found and found["left"],
                         "top": inner and inner["top"]})
    out["hint"] = hint
    if parts.get("divider"):
        x, y, w, h = as_tuple(parts["divider"])
        # The column that stands out most from the strip, within 4px of
        # where the rule is declared.
        # The rule lights one column, or two when it falls between them:
        # the strongest neighbouring pair within 3px of its declared place
        # holds it, the nearer pair winning a tie (the open Actions
        # button's wash begins 5px to its right).
        # Against the strip just left of it (the gap before it holds no
        # button): the open panel's shadow darkens the strip unevenly.
        local = median_color(image, box_px((x - 5, y + 3, 2, h - 6), scale)) or background
        columns = []
        for column_x in range(int((x - 3) * scale), int((x + 4) * scale)):
            color = median_color(image, (column_x, (y + 3) * scale, column_x + 1, (y + h - 3) * scale))
            alpha = overlay_alpha(color, local, pal["lighter"]) if color and local else None
            columns.append((max(alpha or 0, 0), column_x / scale))
        pairs = [(a[0] + b[0], -abs(a[1] - x), a[1] if a[0] >= b[0] else b[1])
                 for a, b in zip(columns, columns[1:])]
        alpha, _, found = max(pairs) if pairs else (None, None, None)
        out["divider"] = {"alpha": alpha, "x": found}
    button = parts.get("actionsButton")
    if button:
        rect = as_tuple(button if "x" in button else button["rect"])
        # Against the button's own fill (its open wash, while open).
        inside = median_color(image, box_px((rect[0] + 2, rect[1] + 8, 3, rect[3] - 16), scale)) or background
        area = (rect[0] + 3, rect[1], rect[2] * 0.6, rect[3])
        text = coverage_edges(image, area, inside, (217, 218, 221), scale)
        # The label's own color, the brightest of its ink: white while the
        # static Actions board's panel is open, #D9DADD on the root board's.
        label_color = brightest(image, box_px(area, scale))
        measured = measure_cap(image, rect, scale, pal["lighter"])
        out["actionsButton"] = {"labelLeft": text and text["left"], "labelTop": text and text["top"],
                                "labelColor": label_color,
                                "alpha": measured and measured["alpha"], "edges": measured and measured["edges"]}
    return out


def harness_footer(report, name, capture, declared, manifest, native, scale, pal, crops):
    """The native footer against its declaration: the mark where it is
    declared, each hint part starting where declared, the rule's alpha and
    the Actions button's label and wash."""
    parts = declared.get("footer")
    if not parts:
        return
    colors = manifest["declared"]["colors"]
    footer = as_tuple(manifest["footer"])
    measured = measure_footer_parts(native, footer, parts, scale, pal, hex_rgba(colors["footerMark"])[:3])
    mark = as_tuple(parts["mark"])
    found = measured.get("mark")
    # The filled square's top and right (y 3.5 and x 21 of the mark's 24
    # units: 2.6 and 15.75 of 18px), which the stroked square never
    # reaches: its thin stroke clears the core threshold on one renderer
    # and not on the other, so it decides no edge here.
    report.check("harness-native", name, capture, "footer-mark", "ink top", found and found["top"],
                 mark[1] + 3, LIMITS["edge_px"], "px")
    report.check("harness-native", name, capture, "footer-mark", "ink right", found and found["right"],
                 mark[0] + 15.75, LIMITS["edge_px"], "px")
    crops.append(("footer-mark", mark, None))
    for index, (part, found) in enumerate(zip(parts["hint"], measured["hint"])):
        if part.get("clipped"):
            continue
        subject = f"footer-hint:{index}:{part['text']}"
        x = as_tuple(part["rect"])[0]
        report.check("harness-native", name, capture, subject, "cap left" if part["keys"] else "ink left",
                     found["left"], x, CHAIN_SLACK_PX + (0 if part["keys"] else 0.5), "px",
                     CHAIN_NOTE + ("" if part["keys"] else "; a glyph's side bearing allowed"))
    # The rule stands between two buttons: with nothing selected (fallbacks
    # alone, #96) the fixture shows no primary action, and no rule.
    if declared.get("actionButton"):
        report.check("harness-native", name, capture, "footer-divider", "x", measured.get("divider", {}).get("x"),
                     as_tuple(parts["divider"])[0], CHAIN_SLACK_PX, "px", CHAIN_NOTE)
        report.check("harness-native", name, capture, "footer-divider", "alpha",
                     measured.get("divider", {}).get("alpha"), hex_rgba(colors["footerDivider"])[3],
                     LIMITS["flat_fill_levels"], "levels")
    button = as_tuple(parts["actionsButton"])
    found = measured["actionsButton"]
    report.check("harness-native", name, capture, "footer-actions", "label ink left", found["labelLeft"],
                 button[0] + manifest["declared"]["geometry"].get("actionPaddingX", 8), LIMITS["edge_px"] * 1.5, "px",
                 "a glyph's side bearing allowed")
    expected = hex_rgba(colors["footerButtonOpen"])[3] if parts["actionsPressed"] else 0
    report.check("harness-native", name, capture, "footer-actions",
                 f"wash alpha ({'open' if parts['actionsPressed'] else 'rest'})", found["alpha"], expected,
                 LIMITS["flat_fill_levels"], "levels")
    crops.append(("footer-actions", button, None))


def parity_footer(report, name, capture, declared, manifest, native, scale, state, reference_image, pal, crops):
    """The footer's left side and Actions button, native against reference."""
    parts, ref_parts = declared.get("footer"), state.get("footerParts")
    if not parts or not ref_parts:
        return
    colors = manifest["declared"]["colors"]
    footer = as_tuple(manifest["footer"])
    ref_footer = as_tuple(state["footer"])
    mine = measure_footer_parts(native, footer, parts, scale, pal, hex_rgba(colors["footerMark"])[:3])
    theirs = measure_footer_parts(reference_image, ref_footer, ref_parts)
    if mine.get("mark") and theirs.get("mark"):
        for prop in ("top", "right"):
            report.check("parity", name, capture, "footer-mark", f"ink {prop}", mine["mark"][prop],
                         theirs["mark"][prop], LIMITS["edge_px"], "px")
        crops.append(("parity-footer-mark", as_tuple(parts["mark"]), as_tuple(ref_parts["mark"])))
    # The static Actions board's footer carries one tip where the hint is;
    # the result boards' footers advertise what Pane does not do (#96).
    board_hint = BOARD_HINTS.get(manifest["scenario"].get("board"))
    static = len(ref_parts["hint"]) == 1 or board_hint is not None
    native_text = " ".join(part["text"] for part in parts["hint"])
    ref_text = " ".join(part["text"] for part in ref_parts["hint"])
    report.check("parity", name, capture, "footer-hint", "text", native_text, ref_text, 0, "",
                 accepted=board_hint or (STATIC_HINT if static else None))
    if not static:
        for index, (a, b) in enumerate(zip(mine["hint"], theirs["hint"])):
            subject = f"footer-hint:{index}:{a['text']}"
            for prop in ("left", "top"):
                report.check("parity", name, capture, subject, f"{'cap' if a['keys'] else 'ink'} {prop}",
                             a.get(prop), b.get(prop), LIMITS["edge_px"], "px",
                             "the reference's subset has no ↵: its cap label comes from a fallback face (#93)"
                             if "↵" in a["text"] and prop == "top" else None)
    if ref_parts.get("divider") and parts.get("divider") and declared.get("actionButton"):
        report.check("parity", name, capture, "footer-divider", "x", mine["divider"]["x"],
                     theirs["divider"]["x"], LIMITS["edge_px"], "px")
        report.check("parity", name, capture, "footer-divider", "alpha", mine["divider"]["alpha"],
                     theirs["divider"]["alpha"], LIMITS["flat_fill_levels"], "levels")
    ref_button = ref_parts.get("actionsButton")
    if ref_button:
        a, b = mine["actionsButton"], theirs["actionsButton"]
        # The reference's two boards draw the open Actions label in two
        # colors; a brighter label's anti-aliased first column carries more
        # ink, so its edge is only comparable between labels of one color.
        recolored = bool(a.get("labelColor") and b.get("labelColor")) and abs(
            luma(a["labelColor"]) - luma(b["labelColor"])) > 20
        for prop in ("labelLeft", "labelTop"):
            report.check("parity", name, capture, "footer-actions", f"label ink {prop[5:].lower()}", a[prop], b[prop],
                         LIMITS["edge_px"], "px",
                         accepted=ACTIONS_LABEL_COLOR if recolored and prop == "labelLeft" else None)
        report.check("parity", name, capture, "footer-actions", "pressed", "yes" if parts["actionsPressed"] else "no",
                     "yes" if ref_button["pressed"] else "no", 0, "")
        report.check("parity", name, capture, "footer-actions", "wash alpha", a["alpha"], b["alpha"],
                     LIMITS["flat_fill_levels"], "levels")
        crops.append(("parity-footer-actions", as_tuple(parts["actionsButton"]), as_tuple(ref_button["rect"])))


def action_of(label, index):
    """The kind of an Actions entry, by what the reference names it: the
    first is the primary action."""
    if index == 0:
        return "invoke"
    if "Hotkey" in label:
        return "hotkey"
    if "Alias" in label:
        return "alias"
    if "Quick Slot" in label:
        return "pin"
    return None


def measure_panel(image, panel, scale=1.0, pal=DARK, colors=None):
    """The open Actions panel near panel (the declaration, or the
    reference's DOM, in the same shape): its box, its fill, the header's
    tile and title, each entry's wash, glyph and label, the group labels,
    the rules, the search row's rule and text, and the empty note."""
    rect = as_tuple(panel["rect"])
    x, y, w, h = rect
    background = median_color(image, box_px((x - 8, y + h * 0.5 - 2, 5, 4), scale))
    fill = median_color(image, box_px((x + 1.5, y + h * 0.5 - 2, 2, 4), scale))
    out = {"background": background, "fill": fill}
    out["edges"] = box_edges(image, rect, background, fill, scale) if background and fill else None
    title_color = (142, 143, 148)
    text_color = (228, 228, 231)
    icon_color = (163, 164, 169)
    if colors:
        text_color = hex_rgba(colors["actionText"])[:3]
        icon_color = hex_rgba(colors["actionIcon"])[:3]
        title_color = hex_rgba(colors["textMuted"])[:3]
    header = panel.get("header")
    if header and header.get("tile"):
        tile = as_tuple(header["tile"])
        tile_fill = median_color(image, box_px((tile[0] + 1.5, tile[1] + 7, 2, 4), scale))
        tile_bg = median_color(image, box_px((tile[0] - 6, tile[1] + 7, 3, 4), scale))
        edges = box_edges(image, tile, tile_bg, tile_fill, scale) if tile_fill and tile_bg else None
        hrect = as_tuple(header["rect"])
        title = ink_extent(image, (tile[0] + tile[2] + 2, hrect[1], hrect[2] / 2, hrect[3]), tile_bg, title_color, scale)
        out["header"] = {"tile": edges, "title": title}
    rows = []
    for index, row in enumerate(panel.get("rows", [])):
        r = as_tuple(row["rect"])
        side = median_color(image, box_px((r[0] - 4, r[1] + r[3] * 0.3, 2, r[3] * 0.4), scale))
        wash = median_color(image, box_px((r[0] + 2, r[1] + r[3] * 0.3, 2, r[3] * 0.4), scale))
        alpha = overlay_alpha(wash, side, pal["lighter"]) if wash and side else None
        glyph_box = as_tuple(row["glyph"]) if "glyph" in row else (r[0] + 8, r[1] + 10, 16, 16)
        glyph = ink_extent(image, glyph_box, wash, icon_color, scale, pad=1)
        label = ink_extent(image, (glyph_box[0] + glyph_box[2] + 4, r[1], r[2] * 0.55, r[3]), wash, text_color, scale)
        rows.append({"label": row["label"], "action": row.get("action") or action_of(row["label"], index),
                     "rect": r, "alpha": alpha, "glyph": glyph, "text": label})
    out["rows"] = rows
    out["groups"] = [
        {"text": group["text"], "rect": as_tuple(group["rect"]),
         "ink": ink_extent(image, as_tuple(group["rect"]), fill, title_color, scale)}
        for group in panel.get("groups", [])
    ]
    rules = []
    for rule in panel.get("rules", []):
        rx, ry, rw, rh = as_tuple(rule)
        found = hline(image, (int((rx + 20) * scale), int((rx + rw - 20) * scale)),
                      (int((ry - 3) * scale), int((ry + 4) * scale)), fill, pal["lighter"])
        line = median_color(image, box_px((rx + 20, found / scale if found else ry, rw - 40, 1), scale)) if found else None
        # Against the panel just above it: the popover's sheen fades over
        # its top 40%, so its middle is darker than the rule's surround.
        above = median_color(image, box_px((rx + 20, (found / scale if found else ry) - 3, rw - 40, 1), scale))
        rules.append({"y": found / scale if found else None,
                      "alpha": overlay_alpha(line, above, pal["lighter"]) if line and above else None})
    out["rules"] = rules
    search = as_tuple(panel["search"])
    sx, sy, sw, sh = search
    found = hline(image, (int((sx + 60) * scale), int((sx + sw - 20) * scale)),
                  (int((sy - 3) * scale), int((sy + 4) * scale)), fill, pal["lighter"])
    below = median_color(image, box_px((sx + 60, sy + 4, sw - 80, 4), scale))
    # The field's text, the caret (the accent) apart.
    field_box = box_px((sx + 32, sy + 4, sw - 40, sh - 8), scale)
    field = ink(image, field_box, below, pal["placeholder"]) or ink(image, field_box, below, pal["query"])
    if field:
        left, top, width, height = field["box"]
        field = {"left": left / scale, "top": top / scale, "right": (left + width) / scale,
                 "bottom": (top + height) / scale, "color": field["color"]}
    glass = ink_extent(image, (sx + 8, sy + 8, 24, sh - 16), below, title_color, scale)
    out["search"] = {"rule": found / scale if found else None, "text": field, "glyph": glass}
    empty = panel.get("empty")
    if empty:
        out["empty"] = ink_extent(image, as_tuple(empty["rect"]), fill, title_color, scale)
    return out


def dimmer_sample(image, list_rect, scale=1.0):
    x, y, w, h = list_rect
    return median_color(image, box_px((x + 3, y + h - 12, 5, 6), scale))


def harness_actions(report, name, capture, declared, manifest, native, scale, pal, crops):
    """The native Actions panel against its declaration."""
    panel = declared.get("actions")
    list_rect = as_tuple(manifest["list"])
    sample = dimmer_sample(native, list_rect, scale)
    colors = manifest["declared"]["colors"]
    if not panel:
        if sample:
            UNDIMMED[("native", name)] = sample
        return
    measured = measure_panel(native, panel, scale, pal, colors)
    rect = as_tuple(panel["rect"])
    edges = measured["edges"]
    for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
        report.check("harness-native", name, capture, "actions-panel", prop, edges[index] if edges else None,
                     rect[index], LIMITS["edge_px"], "px")
    crops.append(("actions-panel", rect, None))
    header = panel.get("header")
    if header:
        tile, edges = as_tuple(header["tile"]), measured["header"]["tile"]
        for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
            report.check("harness-native", name, capture, "actions-header", f"tile {prop}",
                         edges[index] if edges else None, tile[index], LIMITS["edge_px"], "px")
        title = measured["header"]["title"]
        report.check("harness-native", name, capture, "actions-header", "title ink left", title and title["left"],
                     tile[0] + tile[2] + 8, LIMITS["edge_px"] * 1.5, "px", "a glyph's side bearing allowed")
    selected_alpha = hex_rgba(colors["actionSelected"])[3]
    for row, found in zip(panel["rows"], measured["rows"]):
        subject = f"action:{row['label']}"
        report.check("harness-native", name, capture, subject, f"wash alpha ({'selected' if row['selected'] else 'rest'})",
                     found["alpha"], selected_alpha if row["selected"] else 0, LIMITS["flat_fill_levels"], "levels")
        glyph = as_tuple(row["glyph"])
        ink_box = found["glyph"]
        report.check("harness-native", name, capture, subject, "glyph ink inside its box",
                     "yes" if ink_box and ink_box["left"] >= glyph[0] - 0.5 and ink_box["right"] <= glyph[0] + glyph[2] + 0.5
                     and ink_box["top"] >= glyph[1] - 0.5 and ink_box["bottom"] <= glyph[1] + glyph[3] + 0.5 else "no",
                     "yes", 0, "")
        report.check("harness-native", name, capture, subject, "label ink left", found["text"] and found["text"]["left"],
                     glyph[0] + glyph[2] + 10, LIMITS["edge_px"] * 1.5, "px", "a glyph's side bearing allowed")
    for group, found in zip(panel["groups"], measured["groups"]):
        rect = as_tuple(group["rect"])
        report.check("harness-native", name, capture, f"action-group:{group['text']}", "ink left",
                     found["ink"] and found["ink"]["left"], rect[0] + 8, LIMITS["edge_px"] * 1.5, "px",
                     "a glyph's side bearing allowed")
    rule_alpha = hex_rgba(colors["actionRule"])[3]
    for rule, found in zip(panel["rules"], measured["rules"]):
        report.check("harness-native", name, capture, "action-rule", "y", found["y"], as_tuple(rule)[1],
                     LIMITS["edge_px"], "px")
        report.check("harness-native", name, capture, "action-rule", "alpha", found["alpha"], rule_alpha,
                     LIMITS["flat_fill_levels"], "levels")
    search = as_tuple(panel["search"])
    report.check("harness-native", name, capture, "actions-search", "rule y", measured["search"]["rule"], search[1],
                 LIMITS["edge_px"], "px")
    text = measured["search"]["text"]
    report.check("harness-native", name, capture, "actions-search", "field ink left", text and text["left"],
                 search[0] + 14 + 15 + 10 + 2, LIMITS["edge_px"] * 2, "px",
                 "after the field's 2px inset; the first glyph's side bearing allowed")
    if panel.get("empty"):
        rect = as_tuple(panel["empty"]["rect"])
        found = measured.get("empty")
        report.check("harness-native", name, capture, "actions-empty", "ink left", found and found["left"],
                     rect[0] + 10, LIMITS["edge_px"] * 1.5, "px", "a glyph's side bearing allowed")
    # The dimmer: the list's padding against the same spot with the panel
    # closed (an earlier capture), or the panel's own color.
    undimmed = UNDIMMED.get(("native", name)) or hex_rgba(colors["panelSolid"])[:3]
    expected = composite(colors["actionsDimmer"], undimmed)
    report.check("harness-native", name, capture, "actions-dimmer", "dimmed list (max channel)",
                 color_delta(sample, expected), 0, LIMITS["flat_fill_levels"], "levels")


def parity_actions(report, name, capture, declared, manifest, native, scale, state, reference_image, pal, crops):
    """The Actions panel, native against reference: placed the same over
    the footer, and each part's anatomy the same. The reference lists more
    entries (see ACTIONS_CONTENT), so its panel is taller: its top and the
    entries are compared relative to the panel's top, entries by what they
    do, and its bottom and right edges where they are."""
    panel, ref_panel = declared.get("actions"), state.get("panel")
    list_rect = as_tuple(state["list"])
    ref_sample = dimmer_sample(reference_image, list_rect)
    if not ref_panel:
        if ref_sample:
            UNDIMMED[("reference", name)] = ref_sample
    report.check("parity", name, capture, "actions-panel", "open", "yes" if panel else "no",
                 "yes" if ref_panel else "no", 0, "")
    if not (panel and ref_panel):
        return
    colors = manifest["declared"]["colors"]
    mine = measure_panel(native, panel, scale, pal, colors)
    theirs = measure_panel(reference_image, ref_panel)
    crops.append(("parity-actions-panel", as_tuple(panel["rect"]), as_tuple(ref_panel["rect"])))
    a, b = mine["edges"], theirs["edges"]
    if a and b:
        report.check("parity", name, capture, "actions-panel", "left", a[0], b[0], LIMITS["edge_px"], "px")
        report.check("parity", name, capture, "actions-panel", "width", a[2], b[2], LIMITS["edge_px"], "px")
        report.check("parity", name, capture, "actions-panel", "bottom", a[1] + a[3], b[1] + b[3], LIMITS["edge_px"], "px")
    native_labels = [row["label"] for row in panel["rows"]]
    ref_labels = [row["label"] for row in ref_panel["rows"]]
    same = native_labels == ref_labels
    report.check("parity", name, capture, "actions-panel", "entries", " | ".join(native_labels),
                 " | ".join(ref_labels), 0, "", accepted=None if same else ACTIONS_CONTENT)
    top, ref_top = as_tuple(panel["rect"])[1], as_tuple(ref_panel["rect"])[1]
    if mine.get("header") and theirs.get("header"):
        ta, tb = mine["header"]["tile"], theirs["header"]["tile"]
        if ta and tb:
            for prop, index in (("left", 0), ("width", 2), ("height", 3)):
                report.check("parity", name, capture, "actions-header", f"tile {prop}", ta[index], tb[index],
                             LIMITS["edge_px"], "px")
            report.check("parity", name, capture, "actions-header", "tile top in panel", ta[1] - top, tb[1] - ref_top,
                         LIMITS["edge_px"], "px")
        na, nb = mine["header"]["title"], theirs["header"]["title"]
        report.check("parity", name, capture, "actions-header", "title", panel["header"]["title"],
                     ref_panel["header"]["title"], 0, "")
        if na and nb:
            report.check("parity", name, capture, "actions-header", "title ink left", na["left"], nb["left"],
                         LIMITS["edge_px"], "px")
            report.check("parity", name, capture, "actions-header", "title ink top in panel", na["top"] - top,
                         nb["top"] - ref_top, LIMITS["edge_px"], "px")
    ref_by_action = {}
    for row in theirs["rows"]:
        if row["action"] and row["action"] not in ref_by_action:
            ref_by_action[row["action"]] = row
    for row in mine["rows"]:
        ref_row = ref_by_action.get(row["action"])
        if not ref_row:
            continue
        subject = f"action:{row['action']}"
        report.check("parity", name, capture, subject, "height", row["rect"][3], ref_row["rect"][3], LIMITS["edge_px"], "px")
        report.check("parity", name, capture, subject, "left", row["rect"][0], ref_row["rect"][0], LIMITS["edge_px"], "px")
        report.check("parity", name, capture, subject, "wash alpha", row["alpha"], ref_row["alpha"],
                     LIMITS["flat_fill_levels"], "levels")
        if row["action"] == "invoke":
            report.check("parity", name, capture, subject, "top in panel", row["rect"][1] - top,
                         ref_row["rect"][1] - ref_top, LIMITS["edge_px"], "px")
        for part in ("glyph", "text"):
            pa, pb = row[part], ref_row[part]
            if pa and pb:
                report.check("parity", name, capture, subject, f"{part} ink left", pa["left"], pb["left"],
                             LIMITS["edge_px"], "px")
                report.check("parity", name, capture, subject, f"{part} ink top in row", pa["top"] - row["rect"][1],
                             pb["top"] - ref_row["rect"][1], LIMITS["edge_px"], "px")
        crops.append((f"parity-action-{row['action']}", row["rect"], ref_row["rect"]))
    # A group label and the rule above it are placed against the first
    # entry below them that both sides list (the "Pane" group's Pin to
    # Quick Slot): the reference's extra entries above them (see
    # ACTIONS_CONTENT) move them down its taller panel. Without such an
    # entry, against the panel's top.
    shared = {row["action"] for row in mine["rows"]} & set(ref_by_action)

    def anchor(rows, y, panel_top):
        below = [row for row in rows if row["action"] in shared and row["rect"][1] > y]
        if not below:
            return panel_top, "panel"
        first = min(below, key=lambda row: row["rect"][1])
        return first["rect"][1], first["action"]

    def placed(y, ref_y):
        (mine_at, mine_of), (theirs_at, theirs_of) = anchor(mine["rows"], y, top), anchor(theirs["rows"], ref_y,
                                                                                          ref_top)
        if mine_of != theirs_of:
            return None, None, None
        if mine_of == "panel":
            return y - mine_at, ref_y - theirs_at, "in panel"
        return mine_at - y, theirs_at - ref_y, f"above action:{mine_of}"

    for ga, gb in zip(mine["groups"], theirs["groups"]):
        report.check("parity", name, capture, "action-group", "text", ga["text"], gb["text"], 0, "")
        if ga["ink"] and gb["ink"]:
            report.check("parity", name, capture, "action-group", "ink left", ga["ink"]["left"], gb["ink"]["left"],
                         LIMITS["edge_px"], "px")
            n_value, r_value, where = placed(ga["ink"]["top"], gb["ink"]["top"])
            report.check("parity", name, capture, "action-group", f"ink top {where or 'placed'}", n_value, r_value,
                         LIMITS["edge_px"], "px")
    if mine["rules"] and theirs["rules"]:
        ra, rb = mine["rules"][0], theirs["rules"][0]
        if ra["y"] is not None and rb["y"] is not None:
            n_value, r_value, where = placed(ra["y"], rb["y"])
            report.check("parity", name, capture, "action-rule", f"y {where or 'placed'}", n_value, r_value,
                         LIMITS["edge_px"], "px")
        report.check("parity", name, capture, "action-rule", "alpha", ra["alpha"], rb["alpha"],
                     LIMITS["flat_fill_levels"], "levels")
    sa, sb = mine["search"], theirs["search"]
    bottom, ref_bottom = top + as_tuple(panel["rect"])[3], ref_top + as_tuple(ref_panel["rect"])[3]
    if sa["rule"] is not None and sb["rule"] is not None:
        report.check("parity", name, capture, "actions-search", "rule above the bottom", bottom - sa["rule"],
                     ref_bottom - sb["rule"], LIMITS["edge_px"], "px")
    for part in ("text", "glyph"):
        if sa[part] and sb[part]:
            report.check("parity", name, capture, "actions-search", f"{part} ink left", sa[part]["left"],
                         sb[part]["left"], LIMITS["edge_px"], "px")
            if sa["rule"] is not None and sb["rule"] is not None:
                report.check("parity", name, capture, "actions-search", f"{part} ink top below the rule",
                             sa[part]["top"] - sa["rule"], sb[part]["top"] - sb["rule"], LIMITS["edge_px"], "px")
    if mine.get("empty") or theirs.get("empty"):
        ea, eb = mine.get("empty"), theirs.get("empty")
        report.check("parity", name, capture, "actions-empty", "shown", "yes" if ea else "no", "yes" if eb else "no", 0, "")
        if ea and eb:
            report.check("parity", name, capture, "actions-empty", "ink left", ea["left"], eb["left"],
                         LIMITS["edge_px"], "px")
            report.check("parity", name, capture, "actions-empty", "ink top in panel", ea["top"] - top,
                         eb["top"] - ref_top, LIMITS["edge_px"], "px")
    # The dimmer, as each side darkens its own list.
    mine_sample = dimmer_sample(native, as_tuple(manifest["list"]), scale)
    native_undimmed, ref_undimmed = UNDIMMED.get(("native", name)), UNDIMMED.get(("reference", name))
    if native_undimmed and ref_undimmed and mine_sample and ref_sample:
        dimmer = manifest["declared"]["colors"]["actionsDimmer"]
        report.check("parity", name, capture, "actions-dimmer", "dimmed list off its own composite (max channel)",
                     color_delta(mine_sample, composite(dimmer, native_undimmed)),
                     color_delta(ref_sample, composite(dimmer, ref_undimmed)), LIMITS["flat_fill_levels"], "levels",
                     "each side's dimmed list against rgba(6,7,8,.34) over its own undimmed list")


def native_wash_fill(image, row, scale):
    """The fill a row's tile sits on: the row's own wash (or the panel),
    just right of the tile."""
    x, y, w, h = scaled(as_tuple(row["rect"]), scale)
    return median_color(image, (x + 2 * scale, y + 4 * scale, x + 8 * scale, y + 6 * scale))


def compare_tiles(report, name, capture, manifest, native, scale, crops):
    """The tile family at each size: each tile where it is declared, and its
    glyph centered in it."""
    data = native.load()
    for index, tile in enumerate(manifest["tiles"]):
        rect = as_tuple(tile["rect"])
        app = tile["tone"] == "app"
        x, y, w, h = scaled(rect, scale)
        background = median_color(native, (x - 6 * scale, y + h / 2 - 2 * scale, x - 3 * scale, y + h / 2 + 2 * scale))
        measured = measure_tile(native, rect, app, APP_GLYPH if app else COMMAND_GLYPH, background, scale)
        subject = f"tile:{tile['size']}-{tile['tone']}"
        edges = measured and measured["edges"]
        for prop, i in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
            report.check("harness-native", name, capture, subject, prop, edges[i] if edges else None, rect[i],
                         LIMITS["edge_px"], "px")
        glyph = measured and measured["glyph"]
        if glyph:
            center = (glyph["left"] + glyph["width"] / 2, glyph["top"] + glyph["height"] / 2)
            report.check("harness-native", name, capture, subject, "glyph centered (x)", center[0], rect[2] / 2,
                         LIMITS["edge_px"] * 1.5, "px", "the glyph's ink, not its viewBox, so allow 1.5px")
            report.check("harness-native", name, capture, subject, "glyph centered (y)", center[1], rect[3] / 2,
                         LIMITS["edge_px"] * 1.5, "px")
        crops.append((subject, rect, None))
    # The same glyph at the same size: the application's 2px stroke against
    # the command's 1.6px.
    by_size = {}
    for tile, cores in measured_cores(manifest, native, scale):
        by_size.setdefault(tile["size"], {})[tile["tone"]] = cores
    for size, tones in by_size.items():
        # At the Actions header's 11px the two strokes are 0.9 and 0.7px:
        # under a pixel, so their cores do not separate.
        if size == "mini":
            continue
        if "app" in tones and "command" in tones and tones["command"]:
            report.check("harness-native", name, capture, f"tile:{size}", "app stroke heavier than command (%)",
                         100 * (tones["app"] / tones["command"] - 1), 25, 15, "%",
                         "same pen glyph and size; 2px against 1.6px is +25% stroke")


def measured_cores(manifest, native, scale):
    for tile in manifest["tiles"]:
        rect = as_tuple(tile["rect"])
        app = tile["tone"] == "app"
        x, y, w, h = scaled(rect, scale)
        background = median_color(native, (x - 6 * scale, y + h / 2 - 2 * scale, x - 3 * scale, y + h / 2 + 2 * scale))
        measured = measure_tile(native, rect, app, APP_GLYPH if app else COMMAND_GLYPH, background, scale)
        yield tile, measured and measured["glyph"] and measured["glyph"]["count"]


# --------------------------------------------------- the Settings shell (#97)

# The board's sections whose glyphs Pane has no counterpart for (its
# palette, shield and info glyphs): the fixture draws a stand-in there, so
# those glyphs are compared by place only.
STAND_IN_GLYPH = "a stand-in for the board's own glyph, which Pane does not have: compared by place only"
SIDEBAR_ROUNDING = ("accepted (#97): both draw the sidebar as black 10% (25.5 levels). Over the page's ~22 levels "
                    "that darkens it by about 2 levels, so one level of rounding is about 11 alpha levels. Pane reads "
                    "its 25.5 within the flat-fill limit; Chrome's glass composites the same overlay 0.2-0.3 of a "
                    "level darker, and reads about 28. Accepted only while Pane matches its declaration and the two "
                    "differ by less than one level of the page")
NAV_GLYPH_LIFT = ("accepted (#97): the same 16px glyph at the same center. Its 1.6 stroke is about 1.07px here, and "
                  "a vertical stroke's round cap covers its end row only partly; GPUI's sprite contrast (see "
                  "SPRITE_CONTRAST) lifts that row past a pixel's worth of coverage (0.64 a pixel against Chrome's "
                  "0.5 on General's sliders), so the native box reads an edge row longer at an end. Accepted only "
                  "where the native box is larger, by at most 2px, around a center within 1px of the reference's")


def vline(image, x_range, y_range, background=None, lighter=True):
    """The x of the most distinct vertical line across y_range within
    x_range (a hairline rule): the column whose median luma stands out most
    from the columns two pixels left and right of it - hline turned on its
    side. A rule between two fills is lighter (darker, in the light
    palette) than both; the edge between the fills is a step, not a
    line."""
    data = image.load()
    rows = range(max(0, y_range[0]), min(image.height, y_range[1]), 3)

    def level(column):
        return statistics.median(luma(data[column, r]) for r in rows)

    best, best_x = 0.0, None
    for column in range(max(2, x_range[0]), min(image.width - 2, x_range[1])):
        left, here, right = level(column - 2), level(column), level(column + 2)
        contrast = min(here - left, here - right) if lighter else min(left - here, right - here)
        if contrast > best:
            best, best_x = contrast, column
    return best_x


def ringed_box(image, rect, background, fill, scale=1.0, ring_lighter=True):
    """A box drawn as a fill under a 1px inset ring (the Settings search
    well: black 24% under a white 6% ring): its fill's edges, as box_edges
    finds them, grown by the ring's logical pixel on every side where the
    ring is lighter than the fill and the background. A ring darker than
    both (the light palette's black one, over a black fill) already counts
    as the fill's, and nothing is grown. Logical (x, y, w, h) or None."""
    edges = box_edges(image, rect, background, fill, scale)
    if edges is None:
        return None
    if not ring_lighter:
        return edges
    x, y, w, h = edges
    return (x - 1, y - 1, w + 2, h + 2)


def native_settings_layout(shell, capture):
    """The fixture's declared Settings shell in one capture, in the shape
    measure_settings reads."""
    hovered = next((h["section"] for h in shell["hovered"] if h["capture"] == capture), None)
    return {
        "rule": shell["titlebar"]["y"] + shell["titlebar"]["height"] - 1,
        "title": as_tuple(shell["title"]["rect"]),
        "sidebar": as_tuple(shell["sidebar"]),
        "search": as_tuple(shell["search"]),
        "placeholder": as_tuple(shell["placeholder"]["rect"]),
        "items": [{
            "label": item["label"],
            "rect": as_tuple(item["rect"]),
            "selected": item["selected"],
            "hovered": index == hovered,
            "glyph": as_tuple(item["glyph"]),
            "boardGlyph": item["boardGlyph"],
            "labelLeft": item["labelBox"]["x"],
            "count": as_tuple(item["count"]["rect"]) if item.get("count") else None,
        } for index, item in enumerate(shell["sections"])],
        "heading": as_tuple(shell["heading"]["rect"]),
        "headingText": shell["heading"].get("text", ""),
        "subtitle": as_tuple(shell["subtitle"]["rect"]),
        "aside": as_tuple(shell["aside"]["rect"]) if shell.get("aside") else None,
    }


def reference_settings_layout(state):
    """The reference Settings board's DOM state, in the same shape. The
    subtitle's box is its first line (the board's wraps to two)."""
    bar = as_tuple(state["titlebar"]["rect"])
    sub = as_tuple(state["subtitle"]["rect"])
    return {
        "rule": bar[1] + bar[3] - 1,
        "title": as_tuple(state["titlebar"]["title"]["rect"]),
        "sidebar": as_tuple(state["sidebar"]["rect"]),
        "search": as_tuple(state["search"]["rect"]),
        "placeholder": as_tuple(state["search"]["input"]),
        "items": [{
            "label": item["label"],
            "rect": as_tuple(item["rect"]),
            "selected": item["selected"],
            "hovered": item["hovered"],
            "glyph": as_tuple(item["glyph"]),
            "boardGlyph": True,
            "labelLeft": item["labelRect"]["x"],
            "count": as_tuple(item["count"]["rect"]) if item.get("count") else None,
            "background": item["background"],
        } for item in state["items"]],
        "heading": as_tuple(state["heading"]["rect"]),
        "subtitle": (sub[0], sub[1], sub[2], min(sub[3], 17)),
        "aside": as_tuple(state["aside"]["rect"]) if state.get("aside") else None,
    }


def sidebar_rows(layout, popover=False):
    """The rows the sidebar's rule and fill are read in (logical px, top and
    bottom): below its last section, where nothing else is drawn - and,
    while a popover is open over the page, the sidebar's last 100 rows, past
    the reach of the popover's shadow (it darkens the sidebar and the page
    beside it unevenly to about 130px below the popover)."""
    sx, sy, sw, sh = layout["sidebar"]
    last = layout["items"][-1]["rect"]
    y0 = last[1] + last[3] + 10
    y1 = min(sy + sh - 10, y0 + 150)
    if popover:
        y1 = sy + sh - 10
        y0 = max(y0, y1 - 100)
    return y0, y1


def measure_settings(image, layout, inks, scale=1.0, lighter=True, popover=False):
    """What an image shows of the Settings shell laid out as layout says
    (logical px): the titlebar's rule and label, the sidebar's rule and
    fill, the search well and its placeholder, each section's wash, label,
    glyph and count, and the page's heading, subtitle and column caption.
    Fills are read as overlay alphas against the background right beside
    them, which survives the reference's glass over its wallpaper. While a
    popover is open over the page (the select's list), its shadow reaches
    the sidebar's rule and darkens both sides of it unevenly well below the
    last section, so the sidebar's fill is read in its bottom rows."""
    result = {}
    sx, sy, sw, sh = layout["sidebar"]
    right = sx + sw
    rule = layout["rule"]
    found = hline(image, (int((right + 40) * scale), int((right + 400) * scale)),
                  (int((rule - 4) * scale), int((rule + 5) * scale)), None, lighter)
    result["rule"] = found / scale if found is not None else None

    # The sidebar's rule and fill, below its last section, where nothing
    # else is drawn: the fill as a black overlay over the page just past the
    # rule.
    y0, y1 = sidebar_rows(layout, popover)
    found = vline(image, (int((right - 5) * scale), int((right + 4) * scale)), (int(y0 * scale), int(y1 * scale)),
                  None, lighter)
    result["separator"] = found / scale if found is not None else None
    # Mean colors over many pixels: a black overlay on these ~22 levels
    # moves them by 2 to 6, so a median's whole levels would round the
    # alpha by about 11 levels each, while the mean keeps the fraction the
    # fixture's dithered gradient carries.
    inside = mean_color(image, box_px((right - 7, y0, 4, y1 - y0), scale))
    beside = mean_color(image, box_px((right + 3, y0, 4, y1 - y0), scale))
    result["sidebarAlpha"] = black_alpha(inside, beside) if inside and beside else None
    result["sidebarLevels"] = min(beside) if beside else None

    # The search well: its fill (between its ring and its magnifier, and
    # inside its right end) as a black overlay over the sidebar right
    # beside each end, at the same rows - the panel's top gradient (and the
    # reference's glass) changes down the sidebar, so the sidebar below the
    # well is darker than the sidebar beside it. Then its edges, and its
    # placeholder's ink.
    x, y, w, h = layout["search"]
    below = median_color(image, box_px((x + 20, y + h + 3, 60, 3), scale))
    fill = median_color(image, box_px((x + 2.5, y + 8, 4, h - 16), scale))
    alphas = []
    for fill_x, beside_x in ((x + 2.5, x - 8), (x + w - 6.5, x + w + 3)):
        end_fill = mean_color(image, box_px((fill_x, y + 8, 4, h - 16), scale))
        end_beside = mean_color(image, box_px((beside_x, y + 8, 5, h - 16), scale))
        if end_fill and end_beside:
            alphas.append(black_alpha(end_fill, end_beside))
    result["search"] = {
        "edges": ringed_box(image, (x, y, w, h), below, fill, scale, lighter) if below and fill else None,
        "alpha": statistics.mean(alphas) if alphas and None not in alphas else None,
    }
    px_left = layout["placeholder"][0]
    result["placeholder"] = ink_extent(image, (px_left - 4, y + 4, 140, h - 8), fill, inks["placeholder"],
                                       scale) if fill else None

    items = []
    for item in layout["items"]:
        rect = item["rect"]
        x, y, w, h = rect
        measured = {"label": item["label"]}
        wash = row_wash(image, rect, scale, lighter=lighter)
        if wash:
            measured["alpha"] = wash["alpha"]
            if item["selected"] or item["hovered"]:
                sign = 1 if lighter else -1
                delta = max(3.0, sign * (luma(wash["fill"]) - luma(wash["background"])) / 2)
                edges = wash_edges(image, scaled(rect, scale), wash["background"], delta, lighter=lighter)
                measured["edges"] = tuple(v / scale for v in edges) if edges else None
            color = inks["selected"] if item["selected"] else inks["hover"] if item["hovered"] else inks["rest"]
            left = item["labelLeft"] - 4
            text = ink(image, box_px((left, y + 4, x + w - 30 - left, h - 8), scale), wash["fill"], color,
                       exclude_accent=False)
            if text:
                bx, by, bw, bh = text["box"]
                measured["text"] = {"left": bx / scale - x, "top": by / scale - y, "height": bh / scale,
                                    "color": text["color"]}
            # The glyph's box by coverage, not core pixels: GPUI lifts an
            # icon's anti-aliased edges (see SPRITE_CONTRAST), so a thin
            # stroke's round cap (the General sliders' ends) is core in
            # GPUI and not in Chrome.
            glyph = coverage_box(image, box_px(item["glyph"], scale, pad=2), inks["icon"])
            if glyph:
                gx, gy, gw, gh = glyph
                measured["glyph"] = (gx / scale - x, gy / scale - y, gw / scale, gh / scale)
            if item["count"]:
                count = ink(image, box_px(item["count"], scale, pad=3), wash["fill"], inks["icon"],
                            exclude_accent=False)
                if count:
                    measured["countRight"] = (count["box"][0] + count["box"][2]) / scale - x
        items.append(measured)
    result["items"] = items

    # The page: its heading, its subtitle's first line and its column
    # caption, against the page's own padding above the heading.
    hx, hy, hw, hh = layout["heading"]
    page = median_color(image, box_px((right + 8, hy - 14, 16, 8), scale))
    if page:
        result["heading"] = ink_extent(image, (hx - 4, hy, min(hw, 300) + 4, hh), page, inks["heading"], scale)
        bx, by, bw, bh = layout["subtitle"]
        result["subtitle"] = ink_extent(image, (bx - 4, by, min(bw, 380) + 4, bh), page, inks["muted"], scale)
        if layout["aside"]:
            ax, ay, aw, ah = layout["aside"]
            result["aside"] = ink_extent(image, (ax - 4, ay, 120, ah), page, inks["muted"], scale)
    tx, ty, tw, th = layout["title"]
    bar = median_color(image, box_px((tx - 60, ty, 40, th), scale))
    result["title"] = ink_extent(image, (tx, ty, tw, th), bar, inks["title"], scale, pad=4) if bar else None
    return result


def compare_settings(report, name, capture, manifest, native, scale, ref_capture, reference_image, crops):
    """The Settings shell (#97): the fixture's against its declaration, the
    reference board's against its DOM, and the two against each other -
    the titlebar's rule and label, the sidebar's rule and fill, the search
    well, every section's wash, label, glyph and count by its label, and
    the page's heading, subtitle and column caption."""
    shell = manifest.get("settings")
    if not shell:
        report.check("harness-native", name, capture, "settings", "declared", "missing", "present", 0, "")
        return
    colors = {key: hex_rgba(value) for key, value in shell["colors"].items()}
    lighter = palette(manifest)["lighter"]
    inks = {
        "rest": colors["navText"][:3], "hover": colors["navHoverText"][:3],
        "selected": colors["navSelectedText"][:3], "icon": colors["navIcon"][:3],
        "heading": colors["headingText"][:3], "muted": colors["textMuted"][:3],
        "placeholder": colors["textPlaceholder"][:3], "title": colors["textBody"][:3],
    }
    edge, flat = LIMITS["edge_px"], LIMITS["flat_fill_levels"]
    bearing = "a glyph's side bearing allowed"

    # ---- the native side against its declaration
    layout = native_settings_layout(shell, capture)
    page = page_of(manifest, capture)
    mine = measure_settings(native, layout, inks, scale, lighter, popover=bool(page and page.get("selectOpen")))
    sidebar = layout["sidebar"]
    rule_x = sidebar[0] + sidebar[2] - 1
    report.check("harness-native", name, capture, "titlebar", "rule y", mine["rule"], layout["rule"], edge, "px")
    report.check("harness-native", name, capture, "sidebar", "rule x", mine["separator"], rule_x, edge, "px")
    report.check("harness-native", name, capture, "sidebar", "fill alpha", mine["sidebarAlpha"],
                 colors["sidebarFill"][3], flat, "levels", "a black overlay over the page beside it")
    search = mine["search"]
    for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
        report.check("harness-native", name, capture, "search", f"well {prop}",
                     search["edges"][index] if search["edges"] else None, layout["search"][index], edge, "px")
    report.check("harness-native", name, capture, "search", "fill alpha", search["alpha"], colors["fieldFill"][3],
                 flat, "levels", "a black overlay over the sidebar below it")
    report.check("harness-native", name, capture, "search", "placeholder ink left",
                 mine["placeholder"] and mine["placeholder"]["left"], layout["placeholder"][0], edge * 1.5, "px",
                 bearing)
    crops.append(("search", layout["search"], None))
    for item, measured in zip(layout["items"], mine["items"]):
        rect = item["rect"]
        subject = f"nav:{item['label']}"
        if item["selected"]:
            expected, state = colors["navSelected"][3], "selected"
        elif item["hovered"]:
            expected, state = colors["navHover"][3], "hovered"
        else:
            expected, state = 0, "rest"
        report.check("harness-native", name, capture, subject, f"nav wash alpha ({state})", measured.get("alpha"),
                     expected, flat, "levels")
        if item["selected"] or item["hovered"]:
            edges = measured.get("edges")
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                report.check("harness-native", name, capture, subject, f"nav wash {prop}",
                             edges[index] if edges else None, rect[index], edge, "px")
        text = measured.get("text")
        report.check("harness-native", name, capture, subject, "label ink left in item", text and text["left"],
                     item["labelLeft"] - rect[0], edge * 1.5, "px", bearing)
        glyph = measured.get("glyph")
        box = item["glyph"]
        report.check("harness-native", name, capture, subject, "glyph ink center x in item",
                     glyph and glyph[0] + glyph[2] / 2, box[0] - rect[0] + box[2] / 2, edge * 1.5, "px",
                     "the glyph's ink, not its viewBox")
        crops.append((subject, rect, None))
    # The heading's ink begins its first glyph's side bearing past its box
    # (Pane's pages head with K, L and E, whose stems sit 1.76px in at 22px).
    heading_bearing = first_glyph_bearing(layout["headingText"])
    report.check("harness-native", name, capture, "page", "heading ink left",
                 mine.get("heading") and mine["heading"]["left"], layout["heading"][0] + heading_bearing,
                 edge * 1.5, "px",
                 f"after the first glyph's side bearing in the font ({heading_bearing:.2f}px); its edge allowed")
    if layout["aside"]:
        report.check("harness-native", name, capture, "page", "aside caption ink left",
                     mine.get("aside") and mine["aside"]["left"], layout["aside"][0], edge * 1.5, "px", bearing)
    title = mine["title"]
    report.check("harness-native", name, capture, "titlebar", "label ink center x",
                 title and (title["left"] + title["right"]) / 2, layout["title"][0] + layout["title"][2] / 2,
                 edge, "px")
    crops.append(("titlebar", (0, 0, manifest["scenario"]["client"][0], layout["rule"] + 1), None))
    crops.append(("heading", layout["heading"], None))

    if ref_capture is None:
        return
    # ---- the reference side against its DOM
    state = ref_capture["state"]
    ref_layout = reference_settings_layout(state)
    theirs = measure_settings(reference_image, ref_layout, inks)
    ref_sidebar = ref_layout["sidebar"]
    report.check("harness-reference", name, capture, "titlebar", "rule y", theirs["rule"], ref_layout["rule"], edge,
                 "px")
    report.check("harness-reference", name, capture, "sidebar", "rule x", theirs["separator"],
                 ref_sidebar[0] + ref_sidebar[2] - 1, edge, "px")
    for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
        edges = theirs["search"]["edges"]
        report.check("harness-reference", name, capture, "search", f"well {prop}", edges[index] if edges else None,
                     ref_layout["search"][index], edge, "px")
    for item, measured in zip(ref_layout["items"], theirs["items"]):
        subject = f"nav:{item['label']}"
        report.check("harness-reference", name, capture, subject, "nav wash alpha vs DOM background",
                     measured.get("alpha"), css_rgba(item["background"])[3], flat, "levels")
        if item["selected"] or item["hovered"]:
            edges = measured.get("edges")
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                report.check("harness-reference", name, capture, subject, f"nav wash {prop}",
                             edges[index] if edges else None, item["rect"][index], edge, "px")

    # ---- parity: the same component in the same state
    selected = lambda items: ", ".join(i["label"] for i in items if i["selected"])  # noqa: E731
    hovered = lambda items: ", ".join(i["label"] for i in items if i["hovered"] and not i["selected"])  # noqa: E731
    report.check("parity", name, capture, "selection", "selected sections", selected(layout["items"]),
                 selected(ref_layout["items"]), 0, "")
    report.check("parity", name, capture, "selection", "hover-only sections", hovered(layout["items"]),
                 hovered(ref_layout["items"]), 0, "", "the pointer washes the section it is over, at once")
    report.check("parity", name, capture, "titlebar", "rule y", mine["rule"], theirs["rule"], edge, "px")
    if mine["title"] and theirs["title"]:
        for prop, value in (("center x", lambda t: (t["left"] + t["right"]) / 2), ("top", lambda t: t["top"])):
            report.check("parity", name, capture, "titlebar", f"label ink {prop}", value(mine["title"]),
                         value(theirs["title"]), edge, "px")
    report.check("parity", name, capture, "sidebar", "rule x", mine["separator"], theirs["separator"], edge, "px")
    # One channel level of the page under the sidebar, in alpha levels: the
    # most a black 10% can be misread by where each renderer rounds it.
    level = 255 / theirs["sidebarLevels"] if theirs.get("sidebarLevels") else 0
    true_native = mine["sidebarAlpha"] is not None and abs(mine["sidebarAlpha"] - colors["sidebarFill"][3]) <= flat
    rounded = (true_native and theirs["sidebarAlpha"] is not None
               and abs(mine["sidebarAlpha"] - theirs["sidebarAlpha"]) < level)
    report.check("parity", name, capture, "sidebar", "fill alpha", mine["sidebarAlpha"], theirs["sidebarAlpha"],
                 flat, "levels", accepted=SIDEBAR_ROUNDING if rounded else None)
    n_edges, r_edges = mine["search"]["edges"], theirs["search"]["edges"]
    for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
        report.check("parity", name, capture, "search", f"well {prop}", n_edges and n_edges[index],
                     r_edges and r_edges[index], edge, "px")
    report.check("parity", name, capture, "search", "fill alpha", mine["search"]["alpha"], theirs["search"]["alpha"],
                 flat, "levels")
    if mine["placeholder"] and theirs["placeholder"]:
        for prop in ("left", "top"):
            report.check("parity", name, capture, "search", f"placeholder ink {prop}", mine["placeholder"][prop],
                         theirs["placeholder"][prop], edge, "px")
    crops.append(("parity-search", layout["search"], ref_layout["search"]))
    by_label = {item["label"]: (item, measured) for item, measured in zip(ref_layout["items"], theirs["items"])}
    for item, measured in zip(layout["items"], mine["items"]):
        if item["label"] not in by_label:
            continue
        ref_item, ref_measured = by_label[item["label"]]
        subject = f"nav:{item['label']}"
        crops.append(("parity-" + subject, item["rect"], ref_item["rect"]))
        report.check("parity", name, capture, subject, "top in client", item["rect"][1], ref_item["rect"][1], edge,
                     "px")
        report.check("parity", name, capture, subject, "nav wash alpha", measured.get("alpha"),
                     ref_measured.get("alpha"), flat, "levels")
        n_edges, r_edges = measured.get("edges"), ref_measured.get("edges")
        if n_edges and r_edges:
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                report.check("parity", name, capture, subject, f"nav wash {prop}", n_edges[index], r_edges[index],
                             edge, "px")
        n_text, r_text = measured.get("text"), ref_measured.get("text")
        if n_text and r_text:
            for prop in ("left", "top", "height"):
                report.check("parity", name, capture, subject, f"label ink {prop} in item", n_text[prop],
                             r_text[prop], edge, "px")
            worst = max(abs(a - b) for a, b in zip(n_text["color"], r_text["color"]))
            report.check("parity", name, capture, subject, "label glyph core color (max channel)", worst, 0,
                         LIMITS["glyph_core_levels"], "levels",
                         f"native {fmt_color(n_text['color'])} vs reference {fmt_color(r_text['color'])}")
        n_glyph, r_glyph = measured.get("glyph"), ref_measured.get("glyph")
        if n_glyph and r_glyph:
            props = (("left", 0), ("top", 1), ("width", 2), ("height", 3)) if item["boardGlyph"] else ()
            for prop, index in props:
                lifted = None
                if index >= 2:
                    # A size: a native box grown by the sprite contrast's lift
                    # at its ends, around the same center.
                    grown = n_glyph[index] - r_glyph[index]
                    shift = (n_glyph[index - 2] + n_glyph[index] / 2) - (r_glyph[index - 2] + r_glyph[index] / 2)
                    if 0 < grown <= 2 * edge and abs(shift) <= edge:
                        lifted = NAV_GLYPH_LIFT
                report.check("parity", name, capture, subject, f"glyph ink {prop} in item", n_glyph[index],
                             r_glyph[index], edge, "px", accepted=lifted)
            if not item["boardGlyph"]:
                report.check("parity", name, capture, subject, "glyph ink center x in item",
                             n_glyph[0] + n_glyph[2] / 2, r_glyph[0] + r_glyph[2] / 2, edge * 1.5, "px",
                             STAND_IN_GLYPH)
        if measured.get("countRight") is not None and ref_measured.get("countRight") is not None:
            report.check("parity", name, capture, subject, "count ink right in item", measured["countRight"],
                         ref_measured["countRight"], edge, "px", "right-aligned at the item's padding")
    for key in ("heading", "subtitle", "aside"):
        n_ink, r_ink = mine.get(key), theirs.get(key)
        if not (n_ink and r_ink):
            continue
        for prop in ("left", "top"):
            report.check("parity", name, capture, "page", f"{key} ink {prop}", n_ink[prop], r_ink[prop], edge, "px")
        if key == "heading":
            report.check("parity", name, capture, "page", "heading ink height", n_ink["bottom"] - n_ink["top"],
                         r_ink["bottom"] - r_ink["top"], edge, "px")
            worst = max(abs(a - b) for a, b in zip(n_ink["color"], r_ink["color"]))
            report.check("parity", name, capture, "page", "heading glyph core color (max channel)", worst, 0,
                         LIMITS["glyph_core_levels"], "levels")
    crops.append(("parity-heading", layout["heading"], ref_layout["heading"]))


# ------------------------------------------------- the Appearance page (#98)

SLIDER_DRAWING = ("the board's range input is the browser's own control; Pane draws its slider family as an "
                  "adaptation, so only its label and value are compared, by place")
PREVIEW_PANEL = ("the board's miniature is glass over the stage's painted wallpaper, which Pane does not paint "
                 "(#100); its fill is compared only where both draw the solid surface")


def appearance_of(manifest, capture):
    """The fixture's declared Appearance page in one capture, or None."""
    return next((page for page in manifest.get("appearancePage", []) if page["capture"] == capture), None)


def page_left_of(image, x, y, h, scale=1.0):
    """The page's own fill just left of a part at x (logical), across its
    middle rows: 4px, 6 to 10px out."""
    return median_color(image, box_px((x - 10, y + h * 0.3, 4, max(1.0, h * 0.4)), scale))


def measure_line(image, rect, color, scale=1.0, opacity=1.0, background=None, slack=8):
    """A line of text at rect (its first line's box), in color at opacity,
    over the page beside it (or background): where its ink begins and
    ends, and its fullest ink's color."""
    x, y, w, h = rect
    h = min(h, 18.2)
    bg = background or page_left_of(image, x, y, h, scale)
    if bg is None:
        return None
    ink_color = tuple(b + (c - b) * opacity for b, c in zip(bg, color))
    found = measure_text(image, (x, y, max(w, 24), h), bg, ink_color, scale, slack)
    if not found:
        return None
    extent = ink_extent(image, (x - slack, y, max(w, 24) + 2 * slack, h), bg, ink_color, scale)
    found["color"] = extent["color"] if extent else None
    return found


def measure_track(image, rect, scale=1.0):
    """A segmented choice's track: its edges and its fill as a black
    overlay over the page beside it, from mean colors (a black 24% over the
    page's ~22 levels moves them by about 5).

    The edges are its ring's, found walking in from 4px outside each edge
    until the luma leaves the page beside it by 3 levels (local_edges):
    the ring is lighter than the page in the dark palette and darker in the
    light one, and each scan line keeps its own page, so the reference's
    glass - 25 levels at the track's left, 38 at its right - moves no edge.
    A disabled track (40%) moves the page by only 2 levels with its fill,
    but its ring still by 4."""
    x, y, w, h = rect
    beside = mean_color(image, box_px((x - 8, y + 8, 4, h - 16), scale))
    fill = mean_color(image, box_px((x + 1, y + 8, 2, h - 16), scale))
    if beside is None or fill is None:
        return None
    edges = local_edges(image, scaled(rect, scale), scale, threshold=3.0, out=4, corner=14)
    return {"edges": tuple(v / scale for v in edges) if edges else None, "fill": fill, "beside": beside,
            "alpha": black_alpha(fill, beside)}


def overlay_limit(background, lighter):
    """An overlay alpha's limit, in alpha levels: the flat-fill limit's 2
    channel levels over the background it is read against. A black overlay
    over b levels moves them one level per 255/b alpha levels (about 10 over
    the dark page's 25), and a white one over b one per 255/(255-b) (about
    7.5 over the light track's 221), so a 1-level rounding of the composite
    reads as that many levels of alpha. The limit stays the channel levels
    LIMITS defines, and never drops below its 2 alpha levels."""
    if background is None:
        return LIMITS["flat_fill_levels"]
    room = 255 - luma(background) if lighter else luma(background)
    return max(LIMITS["flat_fill_levels"], LIMITS["flat_fill_levels"] * 255 / max(1.0, room))


def overlay_note(background, lighter):
    return (f"2 channel levels over a background of {luma(background):.0f}: "
            f"{overlay_limit(background, lighter):.1f} alpha levels") if background else None


def measure_segment(image, segment, color, scale=1.0, lighter=True):
    """A segment: its wash as an overlay over its track's fill (read inside
    its left end, clear of its label, against the track's fill in the 2px
    just left of it - the gap from the previous segment, or the track's
    padding - since the reference's glass shades the track across its
    width), the chosen one's edges, and its label's ink in color (a
    function of the segment's own fill, for a dimmed track)."""
    x, y, w, h = segment["rect"]
    fill = mean_color(image, box_px((x + 4, y + 9, 6, h - 18), scale))
    track_fill = mean_color(image, box_px((x - 2, y + 9, 2, h - 18), scale))
    if fill is None or track_fill is None:
        return {}
    color = color(fill)
    result = {"alpha": overlay_alpha(fill, track_fill, lighter), "trackFill": track_fill}
    if segment["chosen"]:
        sign = 1 if lighter else -1
        delta = max(3.0, sign * (luma(fill) - luma(track_fill)) / 2)
        edges = wash_edges(image, scaled((x, y, w, h), scale), track_fill, delta, margin=2, lighter=lighter)
        result["edges"] = tuple(v / scale for v in edges) if edges else None
    lx, _, lw, _ = segment["labelBox"]
    found = ink_extent(image, (lx - 6, y + 3, lw + 12, h - 6), fill, color, scale)
    if found:
        result["label"] = {"center": (found["left"] + found["right"]) / 2, "top": found["top"],
                           "color": found["color"]}
    return result


def measure_swatch(image, swatch, scale=1.0):
    """An accent swatch's disc (the chosen one's rings reach 4px past it):
    its edges, by its saturated color against the page beside it."""
    x, y, w, h = swatch["rect"]
    grow = 4 if swatch["chosen"] else 0
    background = median_color(image, box_px((x - grow - 9, y + h / 2 - 2, 3, 4), scale))
    fill = median_color(image, box_px((x + w / 2 - 2, y + h / 2 - 2, 4, 4), scale))
    if background is None or fill is None:
        return None
    return box_edges(image, (x - grow, y - grow, w + 2 * grow, h + 2 * grow), background, fill, scale, margin=3,
                     chroma=True)


def measure_toggle(image, toggle, scale=1.0):
    """A toggle that is on: its accent track's edges, and where its white
    knob begins along the track's middle row."""
    x, y, w, h = toggle["rect"]
    background = median_color(image, box_px((x - 10, y + h / 2 - 2, 4, 4), scale))
    fill = median_color(image, box_px((x + 3, y + h / 2 - 1, 2, 2), scale))
    if background is None or fill is None:
        return None
    data = image.load()
    row = int(round((y + h / 2) * scale))
    knob = None
    if 0 <= row < image.height:
        whites = [c for c in range(max(0, int(x * scale)), min(image.width, int((x + w) * scale)))
                  if min(data[c, row]) >= 225]
        knob = whites[0] / scale if whites else None
    return {"edges": box_edges(image, (x, y, w, h), background, fill, scale, margin=3, chroma=True),
            "knob": knob}


def is_caret(color):
    """A caret's green: the lime accent, or the light palette's darkened
    readable one (theme.accent_text, about #5C7A17), which is saturated but
    not bright. ClearType's fringes beside the query's glyphs lean red or
    blue, never green."""
    r, g, b = color[:3]
    return is_accent(color) or (g - b > 60 and g - r > 15)


def caret_left(image, rect, scale=1.0):
    """The first column of caret pixels (is_caret) near a caret's box, or
    None."""
    x, y, w, h = rect
    left, top, right, bottom = clamp_box(image, box_px((x - 4, y, w + 8, h), scale))
    data = image.load()
    for column in range(left, right):
        if any(is_caret(data[column, row]) for row in range(top, bottom)):
            return column / scale
    return None


def mini_row_wash(image, rect, scale=1.0, lighter=True):
    """A miniature row's wash, column by column, like row_wash: the strip
    just under its top edge against the gap right above it in the same
    column, moved by however much the miniature's side padding (4 to 2px
    outside both of the row's ends, clear of the panel's edge) changes
    between that gap's row and the strip's.

    The backdrop shades both ways and neither alone is linear: the light
    miniature darkens toward its top and levels off right at its first row
    (230 levels 26px above it, 246 at it), so the gaps above and below
    interpolate a backdrop 3 levels too dark there; the glass miniature is
    2 levels darker mid-row than at its sides, so the side padding alone
    reads every row as darkened. Returns the median fill, background and
    overlay alpha (levels)."""
    x, y, w, h = scaled(rect, scale)
    data = image.load()
    top = int(round(y))
    strip = top + int(round(4 * scale))
    if top - 1 < 0 or strip + 1 >= image.height:
        return None

    def side(r0, r1):
        boxes = ((x - 4 * scale, r0, x - 2 * scale, r1), (x + w + 2 * scale, r0, x + w + 4 * scale, r1))
        colors = [mean_color(image, box) for box in boxes]
        if None in colors:
            return None
        return tuple(statistics.mean(c) for c in zip(*colors))

    gap_side, strip_side = side(top - 1, top), side(strip - 1, strip + 2)
    if gap_side is None or strip_side is None:
        return None
    shift = tuple(s - g for s, g in zip(strip_side, gap_side))
    alphas, fills, backgrounds = [], [], []
    for column in range(int(x + 14 * scale), int(x + w - 14 * scale)):
        if not 0 <= column < image.width:
            continue
        background = tuple(a + d for a, d in zip(data[column, top - 1], shift))
        fill = tuple(statistics.mean(data[column, r][i] for r in (strip - 1, strip, strip + 1)) for i in range(3))
        alpha = overlay_alpha(fill, background, lighter)
        if alpha is not None:
            alphas.append(alpha)
            fills.append(fill)
            backgrounds.append(background)
    if not alphas:
        return None
    return {
        "fill": tuple(statistics.median(c) for c in zip(*fills)),
        "background": tuple(statistics.median(c) for c in zip(*backgrounds)),
        "alpha": statistics.median(alphas),
    }


def measure_miniature(image, mini, colors, scale=1.0, lighter=True):
    """The preview's miniature laid out as mini says (the fixture's
    declaration or the board's DOM, in the same shape): its rows' washes,
    titles and kinds, its slots' fills, its query, caret and tip, its
    accent key, and its fill where the surface is solid."""
    result = {"rows": [], "pins": []}
    for row in mini["rows"]:
        measured = {"title": row["title"]["text"]}
        wash = mini_row_wash(image, as_tuple(row["rect"]), scale, lighter)
        if wash:
            measured["alpha"] = wash["alpha"]
            title = measure_line(image, as_tuple(row["title"]["rect"]), colors["title"], scale,
                                 background=wash["fill"], slack=4)
            if title:
                measured["titleLeft"], measured["titleTop"] = title["left"], title["top"]
            kind = measure_line(image, as_tuple(row["kind"]["rect"]), colors["muted"], scale,
                                background=wash["fill"], slack=4)
            if kind:
                measured["kindRight"] = kind["right"]
        result["rows"].append(measured)
    for pin in mini["pins"]:
        x, y, w, h = as_tuple(pin)
        background = median_color(image, box_px((x - 4, y + h / 2 - 2, 2, 4), scale))
        fill = median_color(image, box_px((x + 3, y + h / 2 - 2, 5, 4), scale))
        measured = {}
        if background and fill:
            measured["alpha"] = overlay_alpha(fill, background, lighter)
            # Each scan line against its own backdrop (local_edges), 3px out
            # inside the strip's 6px gaps: the reference's glass is lighter
            # above a slot than beside it, which a single background read
            # beside it took for the slot.
            edges = local_edges(image, scaled((x, y, w, h), scale), scale, threshold=4.0, out=3, corner=14)
            measured["edges"] = tuple(v / scale for v in edges) if edges else None
        result["pins"].append(measured)
    query = as_tuple(mini["query"]["rect"])
    search = as_tuple(mini["search"])
    background = median_color(image, box_px((query[0] - 7, search[1] + 8, 3, search[3] - 16), scale))
    result["query"] = measure_line(image, query, colors["query"], scale, background=background, slack=3)
    result["caret"] = caret_left(image, as_tuple(mini["caret"]), scale)
    footer = as_tuple(mini["footer"])
    footer_fill = median_color(image, box_px((footer[0] + 3, footer[1] + 12, 4, 14), scale))
    if mini.get("tip"):
        result["tip"] = measure_line(image, as_tuple(mini["tip"]["rect"]), colors["muted"], scale,
                                     background=footer_fill, slack=3)
    keys = as_tuple(mini["keys"])
    key_fill = median_color(image, box_px((keys[0] + 2, keys[1] + keys[3] / 2 - 1, 2, 2), scale))
    result["keys"] = (box_edges(image, keys, footer_fill, key_fill, scale, margin=3, chroma=True)
                      if footer_fill and key_fill else None)
    rows = mini["rows"]
    if rows:
        x, y, _, _ = as_tuple(rows[-1]["rect"])
        result["panelFill"] = mean_color(image, box_px((x - 5, y + 10, 3, 18), scale))
    return result


def reference_miniature(preview):
    """The board's miniature from its DOM state, in the shape
    measure_miniature reads."""
    return {
        "rows": [{"title": row["title"], "kind": row["kind"], "rect": row["rect"]} for row in preview["rows"]],
        "pins": preview["pins"],
        "query": preview["query"],
        "search": preview["search"],
        "caret": preview["caret"],
        "footer": preview["footer"],
        "tip": preview.get("tip"),
        "keys": preview["keys"],
    }


def measure_appearance(image, page, colors, scale=1.0, lighter=True):
    """What an image shows of the Appearance page laid out as page says
    (the fixture's declaration, or the board's DOM state in the same
    shape): its field labels, tracks and segments, descriptions,
    swatches, sliders, toggles, miniature and link."""
    result = {"labels": [], "tracks": [], "descriptions": [], "swatches": [], "sliders": [], "toggles": []}
    # The chosen segment's wash is white in both palettes (the light one's
    # is white 85% over a black 4% track), so its overlay is read as white.
    on = colors.get("raw", {}).get("segmentOn")
    segments_lighter = lighter or (on is not None and luma(on) > 128)
    for label in page["labels"]:
        result["labels"].append(measure_line(image, as_tuple(label["rect"]), colors["title"], scale,
                                             label.get("opacity", 1.0)))
    for track in page["tracks"]:
        rect = as_tuple(track["rect"])
        measured = measure_track(image, rect, scale) or {}
        segments = []
        for segment in track["segments"]:
            if segment["chosen"]:
                color = colors["onText"]
            elif segment["hovered"]:
                color = colors["hoverText"]
            else:
                color = colors["segmentText"]
            opacity = track.get("opacity", 1.0)

            def segment_color(fill, color=color, opacity=opacity):
                return tuple(f + (c - f) * opacity for f, c in zip(fill, color))

            segments.append(measure_segment(image, {**segment, "rect": as_tuple(segment["rect"]),
                                                    "labelBox": as_tuple(segment["labelBox"])},
                                            segment_color, scale, segments_lighter))
        measured["segments"] = segments
        result["tracks"].append(measured)
    for description in page["descriptions"]:
        color = description.get("ink") or colors["muted"]
        # Its first line's first 160px: tall glyphs enough for its top and
        # left, clear of the reference glass's bright band at the column's
        # right, which a 40% line's faint core threshold would count as ink.
        x, y, w, h = as_tuple(description["rect"])
        result["descriptions"].append(measure_line(image, (x, y, min(w, 160.0), h), color, scale,
                                                   description.get("opacity", 1.0)))
    for swatch in page["swatches"]:
        result["swatches"].append(measure_swatch(image, {**swatch, "rect": as_tuple(swatch["rect"])}, scale))
    for slider in page["sliders"]:
        opacity = slider.get("opacity", 1.0)
        label = slider.get("label")
        value = slider.get("value")
        result["sliders"].append({
            "label": measure_line(image, as_tuple(label["rect"]), colors["title"], scale, opacity) if label else None,
            "value": measure_line(image, as_tuple(value["rect"]), colors["body"], scale, opacity) if value else None,
        })
    for toggle in page["toggles"]:
        rect = as_tuple(toggle["rect"])
        measured = measure_toggle(image, {**toggle, "rect": rect}, scale) or {}
        label = toggle.get("label")
        measured["label"] = (measure_line(image, as_tuple(label["rect"]), colors["title"], scale)
                             if label else None)
        result["toggles"].append(measured)
    if page.get("preview"):
        result["preview"] = measure_miniature(image, page["preview"], colors, scale, lighter)
    if page.get("link"):
        result["link"] = measure_line(image, as_tuple(page["link"]["rect"]), colors["title"], scale)
    return result


def reference_appearance(state):
    """The board's Appearance page from its DOM state, in the shape the
    fixture declares it."""
    def opacity(part):
        return float(part.get("opacity", 1.0))

    preview = state.get("preview")
    return {
        "labels": [{"text": l["text"], "rect": l["rect"], "opacity": opacity(l)} for l in state["labels"]],
        "tracks": [{
            "field": t["field"],
            "rect": t["rect"],
            "opacity": opacity(t),
            "background": t["background"],
            "segments": [{"label": s["label"], "target": "segment-" + s["label"].lower(), "rect": s["rect"],
                          "labelBox": s["labelBox"], "chosen": s["chosen"], "hovered": s["hovered"],
                          "background": s["background"]} for s in t["segments"]],
        } for t in state["tracks"]],
        "descriptions": [{"text": d["text"], "rect": d["rect"], "opacity": opacity(d)} for d in state["descriptions"]],
        "swatches": state["swatches"],
        "sliders": [{"label": s["label"], "value": s["value"], "rect": s["rect"], "opacity": opacity(s)}
                    for s in state["sliders"]],
        "toggles": state["toggles"],
        "preview": reference_miniature(preview) if preview else None,
        "link": state.get("link"),
    }


def appearance_colors(declared):
    """The ink and fills the measures look for, from the fixture's declared
    colors."""
    colors = {key: hex_rgba(value) for key, value in declared["colors"].items()}
    return {
        "title": colors["title"][:3], "muted": colors["muted"][:3], "body": colors["body"][:3],
        "query": colors["query"][:3], "onText": colors["segmentOnText"][:3],
        "hoverText": colors["segmentHoverText"][:3], "segmentText": colors["segmentText"][:3],
        "raw": colors,
    }


def compare_appearance(report, name, capture, manifest, native, scale, ref_capture, reference_image, crops):
    """The Appearance page (#98): the fixture's against its declaration, the
    board's against its DOM, and the two against each other — field by
    field, segment by segment, and the preview's miniature row by row."""
    declared = appearance_of(manifest, capture)
    if declared is None:
        report.check("harness-native", name, capture, "appearance", "declared", "missing", "present", 0, "")
        return
    colors = appearance_colors(declared)
    raw = colors["raw"]
    lighter = palette(manifest)["lighter"]
    # A description declares its own ink (a warning's, where it reports a
    # fallback).
    for description in declared["descriptions"]:
        description["ink"] = hex_rgba(description["color"])[:3]
    edge, flat = LIMITS["edge_px"], LIMITS["flat_fill_levels"]
    bearing = "a glyph's side bearing allowed"
    mine = measure_appearance(native, declared, colors, scale, lighter)
    seg_lighter = lighter or luma(raw["segmentOn"]) > 128

    # ---- the native side against its declaration
    for label, measured in zip(declared["labels"], mine["labels"]):
        report.check("harness-native", name, capture, f"field:{label['text']}", "label ink left",
                     measured and measured["left"], label["rect"]["x"], edge * 1.5, "px", bearing)
    for track, measured in zip(declared["tracks"], mine["tracks"]):
        subject = f"track:{track['field']}"
        rect = as_tuple(track["rect"])
        edges = measured.get("edges")
        for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
            report.check("harness-native", name, capture, subject, f"track {prop}", edges[index] if edges else None,
                         rect[index], edge, "px")
        beside = measured.get("beside")
        report.check("harness-native", name, capture, subject, "track fill alpha", measured.get("alpha"),
                     raw["segmentTrack"][3] * track["opacity"], overlay_limit(beside, False), "levels",
                     "a black overlay over the page beside it; " + (overlay_note(beside, False) or ""))
        for segment, seg in zip(track["segments"], measured["segments"]):
            sub = f"segment:{segment['label']}"
            state = "chosen" if segment["chosen"] else "rest"
            expected = raw["segmentOn"][3] * track["opacity"] if segment["chosen"] else 0
            report.check("harness-native", name, capture, sub, f"segment {'on ' if segment['chosen'] else ''}wash alpha "
                         f"({state})", seg.get("alpha"), expected, overlay_limit(seg.get("trackFill"), seg_lighter),
                         "levels", "over the track's fill; " + (overlay_note(seg.get("trackFill"), seg_lighter) or ""))
            if segment["chosen"]:
                seg_edges = seg.get("edges")
                for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                    report.check("harness-native", name, capture, sub, f"segment wash {prop}",
                                 seg_edges[index] if seg_edges else None, as_tuple(segment["rect"])[index], edge, "px")
            box = as_tuple(segment["labelBox"])
            report.check("harness-native", name, capture, sub, "label ink center x",
                         seg.get("label") and seg["label"]["center"], box[0] + box[2] / 2, edge * 1.5, "px",
                         "centered in its segment")
        crops.append((subject, rect, None))
    for description, measured in zip(declared["descriptions"], mine["descriptions"]):
        report.check("harness-native", name, capture, f"description:{description['text'][:24]}", "ink left",
                     measured and measured["left"], description["rect"]["x"], edge * 1.5, "px", bearing)
    for swatch, edges in zip(declared["swatches"], mine["swatches"]):
        grow = 4 if swatch["chosen"] else 0
        rect = as_tuple(swatch["rect"])
        expected = (rect[0] - grow, rect[1] - grow, rect[2] + 2 * grow, rect[3] + 2 * grow)
        for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
            report.check("harness-native", name, capture, f"swatch:{swatch['name']}", f"disc {prop}",
                         edges[index] if edges else None, expected[index], edge, "px",
                         "with its rings, where chosen")
    for slider, measured in zip(declared["sliders"], mine["sliders"]):
        subject = f"slider:{slider['label']['text']}"
        report.check("harness-native", name, capture, subject, "label ink left",
                     measured["label"] and measured["label"]["left"], slider["label"]["rect"]["x"], edge * 1.5,
                     "px", bearing)
        value = as_tuple(slider["value"]["rect"])
        report.check("harness-native", name, capture, subject, "value ink right",
                     measured["value"] and measured["value"]["right"], value[0] + value[2], edge * 1.5, "px",
                     bearing)
    for toggle, measured in zip(declared["toggles"], mine["toggles"]):
        subject = f"toggle:{toggle['label']['text']}"
        rect = as_tuple(toggle["rect"])
        edges = measured.get("edges")
        for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
            report.check("harness-native", name, capture, subject, f"track {prop}", edges[index] if edges else None,
                         rect[index], edge, "px")
        report.check("harness-native", name, capture, subject, "knob left", measured.get("knob"),
                     toggle["knob"]["x"], edge, "px")
    preview = declared.get("preview")
    if preview:
        stage = as_tuple(preview["stage"])
        found = vline(native, (int((stage[0] - 3) * scale), int((stage[0] + 4) * scale)),
                      (int((stage[1] + 60) * scale), int((stage[1] + 300) * scale)), None, lighter)
        report.check("harness-native", name, capture, "preview", "stage ring x",
                     found / scale if found is not None else None, stage[0], edge, "px", "its 1px inset ring")
        measured = mine["preview"]
        for row, mrow in zip(preview["rows"], measured["rows"]):
            subject = f"mini-row:{row['title']['text']}"
            expected = raw["previewRowSelected"][3] if row["selected"] else 0
            report.check("harness-native", name, capture, subject,
                         f"mini wash alpha ({'selected' if row['selected'] else 'rest'})", mrow.get("alpha"), expected,
                         flat, "levels")
            report.check("harness-native", name, capture, subject, "title ink left", mrow.get("titleLeft"),
                         row["title"]["rect"]["x"], edge * 1.5, "px", bearing)
            kind = as_tuple(row["kind"]["rect"])
            report.check("harness-native", name, capture, subject, "kind ink right", mrow.get("kindRight"),
                         kind[0] + kind[2], edge * 1.5, "px", bearing)
        for index, (pin, mpin) in enumerate(zip(preview["pins"], measured["pins"])):
            report.check("harness-native", name, capture, f"mini-pin:{index + 1}", "fill alpha", mpin.get("alpha"),
                         raw["previewPin"][3], flat, "levels")
        query = measured["query"]
        report.check("harness-native", name, capture, "mini-search", "query ink left", query and query["left"],
                     preview["query"]["rect"]["x"], edge * 1.5, "px", bearing)
        report.check("harness-native", name, capture, "mini-search", "caret left", measured["caret"],
                     preview["caret"]["x"], edge, "px")
        if preview.get("tip"):
            tip = measured.get("tip")
            report.check("harness-native", name, capture, "mini-footer", "tip ink left", tip and tip["left"],
                         preview["tip"]["rect"]["x"], edge * 1.5, "px", bearing)
        keys = measured["keys"]
        report.check("harness-native", name, capture, "mini-footer", "key left", keys and keys[0],
                     preview["keys"]["x"], edge, "px")
        crops.append(("preview", stage, None))
    if declared.get("link"):
        link = mine.get("link")
        report.check("harness-native", name, capture, "link", "ink left", link and link["left"],
                     declared["link"]["rect"]["x"], edge * 1.5, "px", bearing)

    if ref_capture is None or reference_image is None or "appearance" not in ref_capture.get("state", {}):
        return
    # ---- the reference side against its DOM
    ref_page = reference_appearance(ref_capture["state"]["appearance"])
    theirs = measure_appearance(reference_image, ref_page, colors)
    for track, measured in zip(ref_page["tracks"], theirs["tracks"]):
        subject = f"track:{track['field']}"
        edges = measured.get("edges")
        rect = as_tuple(track["rect"])
        for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
            report.check("harness-reference", name, capture, subject, f"track {prop}",
                         edges[index] if edges else None, rect[index], edge, "px")
        for segment, seg in zip(track["segments"], measured["segments"]):
            report.check("harness-reference", name, capture, f"segment:{segment['label']}",
                         "segment wash alpha vs DOM background", seg.get("alpha"),
                         css_rgba(segment["background"])[3] * track["opacity"],
                         overlay_limit(seg.get("trackFill"), True), "levels",
                         overlay_note(seg.get("trackFill"), True))
    for swatch, edges in zip(ref_page["swatches"], theirs["swatches"]):
        grow = 4 if swatch["chosen"] else 0
        rect = as_tuple(swatch["rect"])
        report.check("harness-reference", name, capture, f"swatch:{swatch['name']}", "disc left",
                     edges and edges[0], rect[0] - grow, edge, "px", "with its rings, where chosen")
    for toggle, measured in zip(ref_page["toggles"], theirs["toggles"]):
        report.check("harness-reference", name, capture, f"toggle:{(toggle.get('label') or {}).get('text')}",
                     "knob left",
                     measured.get("knob"), toggle["knob"]["x"], edge, "px")
    if ref_page.get("preview"):
        for row, mrow in zip(ref_page["preview"]["rows"], theirs["preview"]["rows"]):
            report.check("harness-reference", name, capture, f"mini-row:{row['title']['text']}", "title ink left",
                         mrow.get("titleLeft"), row["title"]["rect"]["x"], edge * 1.5, "px", bearing)

    # ---- parity: the same component in the same state
    def pairs(mine_list, theirs_list, declared_list, ref_list, key):
        by_key = {key(item): measured for item, measured in zip(ref_list, theirs_list)}
        for item, measured in zip(declared_list, mine_list):
            if key(item) in by_key:
                yield item, measured, by_key[key(item)]

    def place(subject, prop, n, r, limit=edge, note=None, accepted=None):
        if n is not None and r is not None:
            report.check("parity", name, capture, subject, prop, n, r, limit, "px", note, accepted)

    for label, n, r in pairs(mine["labels"], theirs["labels"], declared["labels"], ref_page["labels"],
                             lambda item: item["text"]):
        subject = f"field:{label['text']}"
        if n and r:
            place(subject, "label ink left", n["left"], r["left"])
            place(subject, "label ink top", n["top"], r["top"])
            if n.get("color") and r.get("color"):
                report.check("parity", name, capture, subject, "label glyph core color (max channel)",
                             color_delta(n["color"], r["color"]), 0, LIMITS["glyph_core_levels"], "levels",
                             f"native {fmt_color(n['color'])} vs reference {fmt_color(r['color'])}")
    chosen = lambda tracks: "; ".join(  # noqa: E731
        f"{t['field']}={','.join(s['label'] for s in t['segments'] if s['chosen'])}" for t in tracks)
    hovered = lambda tracks: ", ".join(  # noqa: E731
        s["label"] for t in tracks for s in t["segments"] if s["hovered"] and not s["chosen"])
    shared = {t["field"] for t in ref_page["tracks"]}
    report.check("parity", name, capture, "segments", "chosen segments",
                 chosen([t for t in declared["tracks"] if t["field"] in shared]), chosen(ref_page["tracks"]), 0, "")
    report.check("parity", name, capture, "segments", "hover-only segments", hovered(declared["tracks"]),
                 hovered(ref_page["tracks"]), 0, "", "the pointer lightens the label it is over, at once")
    for track, n, r in pairs(mine["tracks"], theirs["tracks"], declared["tracks"], ref_page["tracks"],
                             lambda item: item["field"]):
        subject = f"track:{track['field']}"
        crops.append(("parity-" + subject, as_tuple(track["rect"]),
                      as_tuple(next(t for t in ref_page["tracks"] if t["field"] == track["field"])["rect"])))
        n_edges, r_edges = n.get("edges"), r.get("edges")
        if n_edges and r_edges:
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                place(subject, f"track {prop}", n_edges[index], r_edges[index])
        if n.get("alpha") is not None and r.get("alpha") is not None:
            # The reference's page is the coarser: its limit decides.
            report.check("parity", name, capture, subject, "track fill alpha", n["alpha"], r["alpha"],
                         overlay_limit(r.get("beside"), False), "levels", overlay_note(r.get("beside"), False))
        ref_segments = {s["label"]: m for s, m in zip(
            next(t for t in ref_page["tracks"] if t["field"] == track["field"])["segments"], r["segments"])}
        for segment, seg in zip(track["segments"], n["segments"]):
            ref_seg = ref_segments.get(segment["label"])
            if ref_seg is None:
                continue
            sub = f"segment:{segment['label']}"
            if seg.get("alpha") is not None and ref_seg.get("alpha") is not None:
                report.check("parity", name, capture, sub, "segment wash alpha", seg["alpha"], ref_seg["alpha"],
                             overlay_limit(ref_seg.get("trackFill"), True), "levels",
                             overlay_note(ref_seg.get("trackFill"), True))
            if seg.get("edges") and ref_seg.get("edges"):
                for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                    place(sub, f"segment wash {prop}", seg["edges"][index], ref_seg["edges"][index])
            n_label, r_label = seg.get("label"), ref_seg.get("label")
            if n_label and r_label:
                place(sub, "label ink center x", n_label["center"], r_label["center"], edge * 1.5,
                      "the label's own advance rounds per renderer")
                place(sub, "label ink top", n_label["top"], r_label["top"])
                report.check("parity", name, capture, sub, "label glyph core color (max channel)",
                             color_delta(n_label["color"], r_label["color"]), 0, LIMITS["glyph_core_levels"],
                             "levels", f"native {fmt_color(n_label['color'])} vs reference "
                             f"{fmt_color(r_label['color'])}")
    for description, n, r in pairs(mine["descriptions"], theirs["descriptions"], declared["descriptions"],
                                   ref_page["descriptions"], lambda item: item["text"]):
        if n and r:
            subject = f"description:{description['text'][:24]}"
            place(subject, "ink left", n["left"], r["left"])
            place(subject, "ink top", n["top"], r["top"])
    for swatch, n, r in pairs(mine["swatches"], theirs["swatches"], declared["swatches"], ref_page["swatches"],
                              lambda item: item["name"]):
        if n and r:
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                place(f"swatch:{swatch['name']}", f"disc {prop}", n[index], r[index])
    for slider, n, r in pairs(mine["sliders"], theirs["sliders"], declared["sliders"], ref_page["sliders"],
                              lambda item: (item.get("label") or {}).get("text")):
        subject = f"slider:{slider['label']['text']}"
        if n["label"] and r["label"]:
            place(subject, "label ink left", n["label"]["left"], r["label"]["left"], note=SLIDER_DRAWING)
            place(subject, "label ink top", n["label"]["top"], r["label"]["top"], note=SLIDER_DRAWING)
            if n["label"].get("color") and r["label"].get("color"):
                report.check("parity", name, capture, subject, "label glyph core color (max channel)",
                             color_delta(n["label"]["color"], r["label"]["color"]), 0,
                             LIMITS["glyph_core_levels"], "levels", "the field's opacity: 40% while Solid is chosen")
        if n["value"] and r["value"]:
            place(subject, "value ink right", n["value"]["right"], r["value"]["right"], edge * 1.5, SLIDER_DRAWING)
    for toggle, n, r in pairs(mine["toggles"], theirs["toggles"], declared["toggles"], ref_page["toggles"],
                              lambda item: (item.get("label") or {}).get("text")):
        subject = f"toggle:{toggle['label']['text']}"
        if n.get("edges") and r.get("edges"):
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                place(subject, f"track {prop}", n["edges"][index], r["edges"][index])
        place(subject, "knob left", n.get("knob"), r.get("knob"))
        if n.get("label") and r.get("label"):
            place(subject, "label ink left", n["label"]["left"], r["label"]["left"])
            place(subject, "label ink top", n["label"]["top"], r["label"]["top"])
    if preview and ref_page.get("preview"):
        n_mini, r_mini = mine["preview"], theirs["preview"]
        crops.append(("parity-preview", as_tuple(preview["stage"]), as_tuple(ref_capture["state"]["appearance"]
                                                                              ["preview"]["stage"])))
        by_title = {row["title"]["text"]: m for row, m in zip(ref_page["preview"]["rows"], r_mini["rows"])}
        for row, n in zip(preview["rows"], n_mini["rows"]):
            r = by_title.get(row["title"]["text"])
            if r is None:
                continue
            subject = f"mini-row:{row['title']['text']}"
            if n.get("alpha") is not None and r.get("alpha") is not None:
                report.check("parity", name, capture, subject, "mini wash alpha", n["alpha"], r["alpha"], flat,
                             "levels", "over the miniature's own backdrop beside the row")
            place(subject, "title ink left", n.get("titleLeft"), r.get("titleLeft"))
            place(subject, "title ink top", n.get("titleTop"), r.get("titleTop"))
            place(subject, "kind ink right", n.get("kindRight"), r.get("kindRight"), edge * 1.5)
        for index, (n, r) in enumerate(zip(n_mini["pins"], r_mini["pins"])):
            subject = f"mini-pin:{index + 1}"
            if n.get("alpha") is not None and r.get("alpha") is not None:
                report.check("parity", name, capture, subject, "fill alpha", n["alpha"], r["alpha"], flat, "levels")
            if n.get("edges") and r.get("edges"):
                place(subject, "slot left", n["edges"][0], r["edges"][0])
                place(subject, "slot top", n["edges"][1], r["edges"][1])
        if n_mini["query"] and r_mini["query"]:
            place("mini-search", "query ink left", n_mini["query"]["left"], r_mini["query"]["left"])
            place("mini-search", "query ink top", n_mini["query"]["top"], r_mini["query"]["top"])
        place("mini-search", "caret left", n_mini["caret"], r_mini["caret"])
        if n_mini.get("tip") and r_mini.get("tip"):
            place("mini-footer", "tip ink left", n_mini["tip"]["left"], r_mini["tip"]["left"])
            place("mini-footer", "tip ink top", n_mini["tip"]["top"], r_mini["tip"]["top"])
        if n_mini["keys"] and r_mini["keys"]:
            for prop, index in (("left", 0), ("top", 1), ("width", 2)):
                place("mini-footer", f"key {prop}", n_mini["keys"][index], r_mini["keys"][index])
        # The surface: compared where both draw the solid one.
        ref_solid = any(s["chosen"] and s["label"] == "Solid" for t in ref_page["tracks"] for s in t["segments"])
        if not preview["glass"] and ref_solid and n_mini.get("panelFill") and r_mini.get("panelFill"):
            report.check("parity", name, capture, "preview", "solid surface (max channel)",
                         color_delta(n_mini["panelFill"], r_mini["panelFill"]), 0, flat, "levels", PREVIEW_PANEL)
    n_link, r_link = mine.get("link"), theirs.get("link")
    if n_link and r_link:
        place("link", "ink left", n_link["left"], r_link["left"])
        place("link", "ink top", n_link["top"], r_link["top"])


# ------------------------------------- Pane's own pages and the form (#99)

PAGE_NOTE = ("a derived composition of the Settings board's families: no board authors this page, so it is "
             "measured against its own declaration only")
FORM_HEADING = ("the form sits under the launcher's screen heading, whose line is GPUI's default (phi) at 14px: "
                "its parts are declared through that fractional line")


def page_of(manifest, capture):
    """The fixture's declared page (Pane's own Settings page, or the form) in
    one capture, or None."""
    return next((page for page in manifest.get("panePage", []) if page["capture"] == capture), None)


def knob_left(part):
    """Where a toggle's knob begins: 3px in from its left while off, 3px in
    from its right while on (its 40px track, its 18px knob)."""
    x, _, w, _ = as_tuple(part["rect"])
    return x + w - 3 - 18 if part.get("on") else x + 3


def line_inside(found, rect, slack=1.0):
    """Whether a line's measured ink (its top, and its core's height below
    it) lies inside its declared line box, give or take slack."""
    if not found:
        return False
    x, y, w, h = rect
    top, bottom = found["top"], found["top"] + found["height"]
    return top >= y - slack and bottom <= y + h + slack


def measure_page_part(image, part, scale=1.0, lighter=True):
    """What an image shows of one declared part of a page."""
    rect = as_tuple(part["rect"])
    kind = part["kind"]
    if kind in ("label", "text"):
        color = hex_rgba(part["color"])[:3]
        return measure_line(image, rect, color, scale, part.get("opacity", 1.0))
    if kind == "rule":
        x, y, w, _ = rect
        found = hline(image, (int((x + 20) * scale), int((x + min(w, 400)) * scale)),
                      (int((y - 3) * scale), int((y + 4) * scale)), None, lighter)
        return {"y": found / scale if found is not None else None}
    if kind in ("well", "track"):
        return measure_track(image, rect, scale)
    if kind == "button":
        return measure_box(image, rect, scale, lighter)
    if kind == "toggle":
        return measure_toggle(image, {**part, "rect": rect}, scale)
    if kind == "segment":
        x, y, w, h = rect
        fill = mean_color(image, box_px((x + 4, y + 9, 6, h - 18), scale))
        # The track's own fill just left of the segment, at its rows: the
        # track's padding before the first segment, the 2px gap before the
        # others (its first column, clear of the ring at the track's edge).
        track = mean_color(image, box_px((x - 2, y + 9, 1, h - 18), scale))
        if fill is None or track is None:
            return None
        return {"alpha": overlay_alpha(fill, track, lighter)}
    if kind == "item":
        return row_wash(image, rect, scale, lighter=lighter)
    return None


def compare_pane_page(report, name, capture, manifest, native, scale, crops):
    """Pane's own Settings pages and the launcher's form (#99): native-only,
    each part against the fixture's declaration - the settings rows' rules,
    the labels' and notes' ink, the wells' and tracks' boxes and black fills,
    the buttons' boxes and white fills, the switches' tracks and knobs, the
    chosen segment's wash, and the list items' rest."""
    declared = page_of(manifest, capture)
    if declared is None:
        report.check("harness-native", name, capture, "page", "declared", "missing", "present", 0, "")
        return
    colors = {key: hex_rgba(value) for key, value in declared["colors"].items()}
    lighter = palette(manifest)["lighter"]
    edge, flat = LIMITS["edge_px"], LIMITS["flat_fill_levels"]
    form = declared["page"] == "Form"
    vertical = CHAIN_SLACK_PX if form else edge
    vertical_note = FORM_HEADING if form else None
    bearing = "a glyph's side bearing allowed"
    for part in declared["parts"]:
        kind = part["kind"]
        rect = as_tuple(part["rect"])
        subject = f"{kind}:{part['name'][:32]}"
        measured = measure_page_part(native, part, scale, lighter)
        if kind in ("label", "text"):
            report.check("harness-native", name, capture, subject, "ink left", measured and measured["left"],
                         rect[0], edge * 1.5, "px", bearing)
            report.check("harness-native", name, capture, subject, "ink inside its line",
                         "inside" if line_inside(measured, rect, vertical) else "outside", "inside", 0, "",
                         vertical_note)
        elif kind == "rule":
            report.check("harness-native", name, capture, subject, "rule y", measured and measured["y"], rect[1],
                         vertical, "px", vertical_note)
        elif kind in ("well", "track"):
            edges = (measured or {}).get("edges")
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                limit = vertical if index == 1 else edge
                report.check("harness-native", name, capture, subject, f"{kind} {prop}",
                             edges[index] if edges else None, rect[index], limit, "px",
                             vertical_note if index == 1 else None)
            fill = colors["fieldFill"] if kind == "well" else colors["segmentTrack"]
            report.check("harness-native", name, capture, subject, f"{kind} fill alpha",
                         (measured or {}).get("alpha"), fill[3] * part.get("opacity", 1.0), flat, "levels",
                         "a black overlay over the page beside it")
            crops.append((subject, rect, None))
        elif kind == "button":
            edges = (measured or {}).get("edges")
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                limit = vertical if index == 1 else CHAIN_SLACK_PX if index == 2 else edge
                report.check("harness-native", name, capture, subject, f"button {prop}",
                             edges[index] if edges else None, rect[index], limit, "px",
                             CHAIN_NOTE if index == 2 else vertical_note if index == 1 else None)
            report.check("harness-native", name, capture, subject, "button fill alpha",
                         (measured or {}).get("alpha"), colors["buttonFill"][3], flat, "levels",
                         "a white overlay over the page beside it")
            crops.append((subject, rect, None))
        elif kind == "toggle":
            if part.get("opacity", 1.0) < 1.0:
                # A switch not offered is drawn at the disabled opacity: its
                # knob is no longer white, so only its crop is evidence.
                crops.append((subject, rect, None))
                continue
            report.check("harness-native", name, capture, subject, "knob left", (measured or {}).get("knob"),
                         knob_left(part), edge, "px", "3px in from the side its state names")
            if part.get("on"):
                edges = (measured or {}).get("edges")
                for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                    report.check("harness-native", name, capture, subject, f"track {prop}",
                                 edges[index] if edges else None, rect[index], edge, "px")
            crops.append((subject, rect, None))
        elif kind == "segment":
            expected = colors["segmentOn"][3] if part.get("on") else 0
            report.check("harness-native", name, capture, subject,
                         f"segment wash alpha ({'chosen' if part.get('on') else 'rest'})",
                         (measured or {}).get("alpha"), expected, flat, "levels", "over the track's fill")
        elif kind == "item":
            report.check("harness-native", name, capture, subject, "item wash alpha (rest)",
                         (measured or {}).get("alpha"), 0, flat, "levels", PAGE_NOTE)
            crops.append((subject, rect, None))


# ------------------------------------------------- result boards (#96)

# The boards whose fallbacks are drawn static: the first preselected, the
# query plain in their titles.
STATIC_FALLBACK_BOARDS = {"empty"}
FALLBACK_SELECTION = ("accepted (#96, #100): the static empty board preselects its first fallback; Pane's fallbacks "
                      "wait for a deliberate selection, so they show unselected until Down selects the first")
FALLBACK_TITLES = ("accepted (#96): the static empty board draws its fallbacks' titles plain; Pane, like the live "
                   "root board, paints the query's match in a title in the accent")
NOTICE_COPY = ("accepted (#96): Pane's notice says what root search found - 'Nothing matches', not 'Nothing on this "
               "computer matches', since it looks at commands, applications and a granted folder - and offers "
               "installing an extension, with no plugin store to suggest one from")
SECTION_NOTES = {
    "Fallbacks": ("accepted (#96): Pane offers fallbacks in Manage extensions, with no order to change in Settings, "
                  "so its Fallbacks label carries no 'Reorder in Settings' note"),
}
BOARD_HINTS = {
    "calculator": ("accepted (#96, #100): the calculator board's footer advertises pasting the answer into the "
                   "previous app, which Pane does not do; Pane's footer keeps its hint"),
    "empty": ("accepted (#96, #100): the empty board's footer advertises installing from a plugin store, which Pane "
              "does not have; Pane's footer keeps its hint"),
}
TITLE = (237, 237, 239)
SOURCE_VALUE, ANSWER_VALUE = (217, 218, 221), (255, 255, 255)


def measure_box(image, rect, scale=1.0, lighter=True):
    """A filled box near its declared rect - the notice's disc, the answer
    card, its arrow's disc, a chip, a pill, a tile: its edges against what
    lies just left of it, and its fill, sampled 3px inside its left edge at
    mid height (clear of a ring along the edge, and of the glyph or text in
    its middle), as an overlay alpha over that. None off the image."""
    x, y, w, h = rect
    background = median_color(image, box_px((x - 6, y + h / 2 - 2, 3, 4), scale))
    fill = median_color(image, box_px((x + 3, y + h / 2 - 2, 2, 4), scale))
    if background is None or fill is None:
        return None
    return {"edges": box_edges(image, rect, background, fill, scale), "fill": fill,
            "alpha": overlay_alpha(fill, background, lighter)}


def accent_ring(image, rect, scale=1.0):
    """Whether a 1px accent ring runs along a box's top edge (the selected
    answer card's): accent pixels across at least 80% of the edge clear of
    its rounded corners, in its top row or the one below it."""
    x, y, w, h = rect
    for row in (y, y + 1):
        across = box_px((x + 16, row, w - 32, 1), scale)
        if accent_pixels(image, across) >= (across[2] - across[0]) * 0.8:
            return True
    return False


def measure_text(image, rect, background, color, scale=1.0, slack=8):
    """A line of text near its declared line box, widened by slack either
    side (shaping drifts; a box's text may be narrower than the box): where
    its ink begins by coverage (left, top) and its core ink's extent."""
    x, y, w, h = rect
    box = (x - slack, y, w + 2 * slack, h)
    if background is None:
        return None
    edges = coverage_edges(image, box, background, color, scale)
    extent = ink_extent(image, box, background, color, scale)
    if not edges or not extent:
        return None
    return {"left": edges["left"], "top": edges["top"], "right": extent["right"],
            "height": extent["bottom"] - extent["top"],
            "baseline": coverage_baseline(image, box, background, color, scale),
            "center": (extent["left"] + extent["right"]) / 2}


def coverage_baseline(image, rect, background, color, scale=1.0):
    """Where a line of text's baseline lies inside rect, by coverage: the
    bottom of the last row holding at least a quarter of the ink of the
    line's fullest row. A descender's thin rows (g, p) stay under that, so
    two lines of different copy share a baseline; a hinted renderer's half
    pixel at a glyph's end moves it by a fraction, not a pixel. Logical px,
    or None."""
    left, top, right, bottom = clamp_box(image, box_px(rect, scale))
    bg, fg = luma(background), luma(color)
    if fg == bg:
        return None
    data = image.load()

    def coverage(value):
        amount = min(1.0, max(0.0, (luma(value) - bg) / (fg - bg)))
        return amount if amount >= 0.2 else 0.0

    rows = [sum(coverage(data[x, y]) for x in range(left, right)) for y in range(top, bottom)]
    if not rows or max(rows) < 1.0:
        return None
    floor = max(rows) / 4
    last = max(i for i, total in enumerate(rows) if total >= floor)
    return (top + last + 1) / scale


def measure_notice(image, notice, scale=1.0, lighter=True, title=TITLE, muted=MUTED):
    """The no-results notice near where notice (the declaration, or the
    reference's DOM) puts it: its disc, and its two lines' ink against the
    panel between the disc and the text."""
    disc = as_tuple(notice["disc"])
    background = median_color(image, box_px((disc[0] + disc[2] + 5, disc[1] + disc[3] / 2 - 2, 4, 4), scale))
    return {
        "disc": measure_box(image, disc, scale, lighter),
        "title": measure_text(image, as_tuple(notice["title"]["rect"]), background, title, scale),
        "description": measure_text(image, as_tuple(notice["description"]["rect"]), background, muted, scale),
    }


def measure_answer(image, answer, scale=1.0, lighter=True, colors=(SOURCE_VALUE, ANSWER_VALUE), muted=MUTED):
    """The answer card near where answer puts it: its box, fill and ring,
    each value's and caption's ink against the card's fill, the arrow's
    disc, the chips and the rule above them."""
    rect = as_tuple(answer["rect"])
    card = measure_box(image, rect, scale, lighter)
    if card:
        # Its edges against the panel right beside each one: the glass
        # above the card is lighter than the list's padding left of it.
        edges = local_edges(image, scaled(rect, scale), scale, out=2, corner=16)
        card["edges"] = edges and tuple(v / scale for v in edges)
    out = {"card": card, "ring": accent_ring(image, rect, scale)}
    fill = card and card["fill"]
    for key, color in zip(("source", "answer"), colors):
        side = answer[key]
        caption = side.get("caption")
        out[key] = {
            "value": measure_text(image, as_tuple(side["value"]["rect"]), fill, color, scale),
            "caption": caption and measure_text(image, as_tuple(caption["rect"]), fill, muted, scale),
        }
    out["arrow"] = measure_box(image, as_tuple(answer["arrow"]), scale, lighter)
    out["chips"] = [measure_box(image, as_tuple(chip["rect"]), scale, lighter) for chip in answer.get("chips", [])]
    also = answer.get("also")
    out["also"] = also and measure_text(image, as_tuple(also["rect"]), fill, muted, scale)
    rule = answer.get("rule")
    if rule and fill:
        rx, ry, rw, _ = as_tuple(rule)
        found = hline(image, (int((rx + 20) * scale), int((rx + rw - 20) * scale)),
                      (int((ry - 3) * scale), int((ry + 4) * scale)), fill, lighter)
        out["rule"] = found / scale if found is not None else None
    return out


def measure_suggestion(image, row, scale=1.0, lighter=True, title=TITLE):
    """An extension suggestion near where row puts it: its tile, its pill,
    and its title's ink beside the tile."""
    r, tile = as_tuple(row["rect"]), as_tuple(row["tile"])
    background = median_color(image, box_px((tile[0] + tile[2] + 3, r[1] + 4, 4, 4), scale))
    text = (tile[0] + tile[2] + 4, r[1] + 4, r[2] * 0.5, r[3] / 2)
    return {
        "tile": measure_box(image, tile, scale, lighter),
        "pill": measure_box(image, as_tuple(row["pill"]), scale, lighter),
        "title": measure_text(image, text, background, title, scale, slack=0),
    }


def ink_inside(found, rect):
    """Whether text's measured ink lies inside its line box, to half a
    pixel either way."""
    if not found:
        return False
    return rect[1] - 0.5 <= found["top"] and found["top"] + found["height"] <= rect[1] + rect[3] + 0.5


def check_edges(report, section, name, capture, subject, edges, rect, limit=LIMITS["edge_px"], note=None):
    """A box's measured edges against rect (declared, or measured on the
    other side), left, top, width and height."""
    for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
        report.check(section, name, capture, subject, prop, edges[index] if edges else None,
                     rect[index] if rect else None, limit, "px", note)


def harness_board(report, name, capture, declared, manifest, native, scale, pal, crops):
    """A result board's own parts against their declaration (#96)."""
    board = declared.get("board")
    if not board:
        return
    lighter = pal["lighter"]
    title = hex_rgba(manifest["declared"]["colors"]["textTitle"])[:3]
    notice = board.get("notice")
    if notice:
        found = measure_notice(native, notice, scale, lighter, title, pal["muted"])
        disc = as_tuple(notice["disc"])
        check_edges(report, "harness-native", name, capture, "notice-disc", found["disc"] and found["disc"]["edges"],
                    disc)
        report.check("harness-native", name, capture, "notice-disc", "fill alpha",
                     found["disc"] and found["disc"]["alpha"], hex_rgba(notice["discFill"])[3],
                     LIMITS["flat_fill_levels"], "levels")
        for key in ("title", "description"):
            rect = as_tuple(notice[key]["rect"])
            ink_found = found[key]
            report.check("harness-native", name, capture, f"notice-{key}", "ink left", ink_found and ink_found["left"],
                         rect[0], LIMITS["edge_px"] * 1.5, "px", "a glyph's side bearing allowed")
            report.check("harness-native", name, capture, f"notice-{key}", "ink inside its line box",
                         "yes" if ink_inside(ink_found, rect) else "no", "yes", 0, "")
        crops.append(("notice", as_tuple(notice["rect"]), None))
    answer = board.get("answer")
    if answer:
        colors = (hex_rgba(answer["source"]["color"])[:3], hex_rgba(answer["answer"]["color"])[:3])
        found = measure_answer(native, answer, scale, lighter, colors, pal["muted"])
        rect = as_tuple(answer["rect"])
        check_edges(report, "harness-native", name, capture, "answer-card", found["card"] and found["card"]["edges"],
                    rect)
        report.check("harness-native", name, capture, "answer-card", "fill alpha", found["card"] and found["card"]["alpha"],
                     hex_rgba(answer["fill"])[3], LIMITS["flat_fill_levels"], "levels")
        report.check("harness-native", name, capture, "answer-card", "accent ring", "yes" if found["ring"] else "no",
                     "yes" if answer["selected"] else "no", 0, "", "the selected card's 1px accent ring")
        for key in ("source", "answer"):
            value = as_tuple(answer[key]["value"]["rect"])
            ink_found = found[key]["value"]
            report.check("harness-native", name, capture, f"answer-{key}", "value ink center",
                         ink_found and ink_found["center"], value[0] + value[2] / 2, LIMITS["edge_px"] * 1.5, "px",
                         "centered in its column; shaped with the values' tracking")
            report.check("harness-native", name, capture, f"answer-{key}", "value ink inside its line box",
                         "yes" if ink_inside(ink_found, value) else "no", "yes", 0, "")
            caption = answer[key].get("caption")
            if caption:
                crect = as_tuple(caption["rect"])
                cfound = found[key]["caption"]
                report.check("harness-native", name, capture, f"answer-{key}", "caption ink center",
                             cfound and cfound["center"], crect[0] + crect[2] / 2, LIMITS["edge_px"] * 1.5, "px")
        arrow = as_tuple(answer["arrow"])
        check_edges(report, "harness-native", name, capture, "answer-arrow", found["arrow"] and found["arrow"]["edges"],
                    arrow)
        report.check("harness-native", name, capture, "answer-arrow", "fill alpha",
                     found["arrow"] and found["arrow"]["alpha"], hex_rgba(answer["arrowFill"])[3],
                     LIMITS["flat_fill_levels"], "levels")
        for index, (chip, chip_found) in enumerate(zip(answer.get("chips", []), found["chips"])):
            crect = as_tuple(chip["rect"])
            subject = f"answer-chip:{index}:{chip['text']}"
            check_edges(report, "harness-native", name, capture, subject, chip_found and chip_found["edges"], crect,
                        CHAIN_SLACK_PX, CHAIN_NOTE)
            report.check("harness-native", name, capture, subject, "fill alpha", chip_found and chip_found["alpha"],
                         hex_rgba(answer["chipFill"])[3], LIMITS["flat_fill_levels"], "levels")
        if answer.get("also"):
            arect = as_tuple(answer["also"]["rect"])
            report.check("harness-native", name, capture, "answer-also", "ink left",
                         found["also"] and found["also"]["left"], arect[0], CHAIN_SLACK_PX + 0.5, "px",
                         CHAIN_NOTE + "; a glyph's side bearing allowed")
        if answer.get("rule"):
            report.check("harness-native", name, capture, "answer-rule", "y", found.get("rule"),
                         as_tuple(answer["rule"])[1], LIMITS["edge_px"], "px")
        crops.append(("answer-card", rect, None))
    for index, row in enumerate(board.get("suggestions", [])):
        found = measure_suggestion(native, row, scale, lighter, title)
        subject = f"suggestion:{index}:{row['title']}"
        check_edges(report, "harness-native", name, capture, subject + "/tile", found["tile"] and found["tile"]["edges"],
                    as_tuple(row["tile"]))
        check_edges(report, "harness-native", name, capture, subject + "/pill", found["pill"] and found["pill"]["edges"],
                    as_tuple(row["pill"]), CHAIN_SLACK_PX, CHAIN_NOTE)
        report.check("harness-native", name, capture, subject + "/pill", "fill alpha",
                     found["pill"] and found["pill"]["alpha"], hex_rgba(row["pillFill"])[3], LIMITS["flat_fill_levels"],
                     "levels")
        tile = as_tuple(row["tile"])
        report.check("harness-native", name, capture, subject, "title ink left", found["title"] and found["title"]["left"],
                     tile[0] + tile[2] + 12, LIMITS["edge_px"] * 1.5, "px", "a glyph's side bearing allowed")
        crops.append((subject, as_tuple(row["rect"]), None))


def parity_board(report, name, capture, declared, manifest, native, scale, state, reference_image, pal, crops):
    """A result board's own parts, native against reference (#96), and the
    reference's measured against its DOM."""
    board = declared.get("board") or {}
    lighter = pal["lighter"]
    title = hex_rgba(manifest["declared"]["colors"]["textTitle"])[:3]
    notice, ref_notice = board.get("notice"), state.get("notice")
    if notice or ref_notice:
        report.check("parity", name, capture, "notice", "shown", "yes" if notice else "no",
                     "yes" if ref_notice else "no", 0, "")
    if notice and ref_notice:
        mine = measure_notice(native, notice, scale, lighter, title, pal["muted"])
        theirs = measure_notice(reference_image, ref_notice)
        dom_disc = as_tuple(ref_notice["disc"])
        check_edges(report, "harness-reference", name, capture, "notice-disc",
                    theirs["disc"] and theirs["disc"]["edges"], dom_disc)
        report.check("harness-reference", name, capture, "notice-disc", "fill alpha vs DOM background",
                     theirs["disc"] and theirs["disc"]["alpha"], css_rgba(ref_notice["discBackground"])[3],
                     LIMITS["flat_fill_levels"], "levels")
        for key in ("title", "description"):
            report.check("parity", name, capture, f"notice-{key}", "text", notice[key]["text"],
                         ref_notice[key]["text"], 0, "", accepted=NOTICE_COPY)
        check_edges(report, "parity", name, capture, "notice-disc", mine["disc"] and mine["disc"]["edges"],
                    theirs["disc"] and theirs["disc"]["edges"])
        report.check("parity", name, capture, "notice-disc", "fill alpha", mine["disc"] and mine["disc"]["alpha"],
                     theirs["disc"] and theirs["disc"]["alpha"], LIMITS["flat_fill_levels"], "levels")
        # Top and baseline, both by coverage: the two sides' copy differs
        # (NOTICE_COPY), so a descender one line has and the other lacks
        # must not decide its size; Chrome's unhinted glyphs also end half
        # a pixel past DirectWrite's at both ends, which a core-pixel
        # extent doubles.
        for key in ("title", "description"):
            a, b = mine[key], theirs[key]
            for prop in ("left", "top", "baseline"):
                report.check("parity", name, capture, f"notice-{key}", f"ink {prop}", a and a[prop], b and b[prop],
                             LIMITS["edge_px"], "px")
        crops.append(("parity-notice", as_tuple(notice["rect"]), as_tuple(ref_notice["rect"])))
    answer, ref_answer = board.get("answer"), state.get("answer")
    if answer or ref_answer:
        report.check("parity", name, capture, "answer-card", "shown", "yes" if answer else "no",
                     "yes" if ref_answer else "no", 0, "")
    if answer and ref_answer:
        colors = (hex_rgba(answer["source"]["color"])[:3], hex_rgba(answer["answer"]["color"])[:3])
        mine = measure_answer(native, answer, scale, lighter, colors, pal["muted"])
        theirs = measure_answer(reference_image, ref_answer)
        dom = as_tuple(ref_answer["rect"])
        check_edges(report, "harness-reference", name, capture, "answer-card",
                    theirs["card"] and theirs["card"]["edges"], dom)
        report.check("harness-reference", name, capture, "answer-card", "fill alpha vs DOM background",
                     theirs["card"] and theirs["card"]["alpha"], css_rgba(ref_answer["background"])[3],
                     LIMITS["flat_fill_levels"], "levels")
        report.check("harness-reference", name, capture, "answer-card", "accent ring vs DOM",
                     "yes" if theirs["ring"] else "no", "yes" if ref_answer["selected"] else "no", 0, "")
        check_edges(report, "parity", name, capture, "answer-card", mine["card"] and mine["card"]["edges"],
                    theirs["card"] and theirs["card"]["edges"])
        report.check("parity", name, capture, "answer-card", "fill alpha", mine["card"] and mine["card"]["alpha"],
                     theirs["card"] and theirs["card"]["alpha"], LIMITS["flat_fill_levels"], "levels")
        report.check("parity", name, capture, "answer-card", "accent ring", "yes" if mine["ring"] else "no",
                     "yes" if theirs["ring"] else "no", 0, "")
        for key in ("source", "answer"):
            side, ref_side = answer[key], ref_answer[key]
            report.check("parity", name, capture, f"answer-{key}", "value", side["value"]["text"],
                         ref_side["value"]["text"], 0, "")
            a, b = mine[key]["value"], theirs[key]["value"]
            for prop in ("left", "top", "right", "height"):
                report.check("parity", name, capture, f"answer-{key}", f"value ink {prop}", a and a[prop],
                             b and b[prop], LIMITS["edge_px"], "px")
            if side.get("caption") and ref_side.get("caption"):
                report.check("parity", name, capture, f"answer-{key}", "caption", side["caption"]["text"],
                             ref_side["caption"]["text"], 0, "")
                a, b = mine[key]["caption"], theirs[key]["caption"]
                for prop in ("left", "top"):
                    report.check("parity", name, capture, f"answer-{key}", f"caption ink {prop}", a and a[prop],
                                 b and b[prop], LIMITS["edge_px"], "px")
        check_edges(report, "parity", name, capture, "answer-arrow", mine["arrow"] and mine["arrow"]["edges"],
                    theirs["arrow"] and theirs["arrow"]["edges"])
        report.check("parity", name, capture, "answer-arrow", "fill alpha", mine["arrow"] and mine["arrow"]["alpha"],
                     theirs["arrow"] and theirs["arrow"]["alpha"], LIMITS["flat_fill_levels"], "levels")
        chips, ref_chips = answer.get("chips", []), ref_answer.get("chips", [])
        report.check("parity", name, capture, "answer-chips", "labels", " | ".join(c["text"] for c in chips),
                     " | ".join(c["text"] for c in ref_chips), 0, "")
        for index, (a, b) in enumerate(zip(mine["chips"], theirs["chips"])):
            subject = f"answer-chip:{index}:{chips[index]['text']}"
            check_edges(report, "parity", name, capture, subject, a and a["edges"], b and b["edges"])
            report.check("parity", name, capture, subject, "fill alpha", a and a["alpha"], b and b["alpha"],
                         LIMITS["flat_fill_levels"], "levels")
        if mine.get("also") or theirs.get("also"):
            a, b = mine.get("also"), theirs.get("also")
            for prop in ("left", "top"):
                report.check("parity", name, capture, "answer-also", f"ink {prop}", a and a[prop], b and b[prop],
                             LIMITS["edge_px"], "px")
        if "rule" in mine or "rule" in theirs:
            report.check("parity", name, capture, "answer-rule", "y", mine.get("rule"), theirs.get("rule"),
                         LIMITS["edge_px"], "px")
        crops.append(("parity-answer-card", as_tuple(answer["rect"]), dom))
    rows, ref_rows = board.get("suggestions", []), state.get("suggestions") or []
    if rows or ref_rows:
        report.check("parity", name, capture, "suggestions", "titles", " | ".join(r["title"] for r in rows),
                     " | ".join(r["title"] for r in ref_rows), 0, "")
    for index, (row, ref_row) in enumerate(zip(rows, ref_rows)):
        mine = measure_suggestion(native, row, scale, lighter, title)
        theirs = measure_suggestion(reference_image, ref_row)
        subject = f"suggestion:{index}:{row['title']}"
        check_edges(report, "harness-reference", name, capture, subject + "/pill",
                    theirs["pill"] and theirs["pill"]["edges"], as_tuple(ref_row["pill"]))
        report.check("harness-reference", name, capture, subject + "/pill", "fill alpha vs DOM background",
                     theirs["pill"] and theirs["pill"]["alpha"], css_rgba(ref_row["pillBackground"])[3],
                     LIMITS["flat_fill_levels"], "levels")
        for part in ("tile", "pill"):
            check_edges(report, "parity", name, capture, f"{subject}/{part}", mine[part] and mine[part]["edges"],
                        theirs[part] and theirs[part]["edges"])
        report.check("parity", name, capture, subject + "/pill", "fill alpha", mine["pill"] and mine["pill"]["alpha"],
                     theirs["pill"] and theirs["pill"]["alpha"], LIMITS["flat_fill_levels"], "levels")
        a, b = mine["title"], theirs["title"]
        for prop in ("left", "top"):
            report.check("parity", name, capture, subject, f"title ink {prop}", a and a[prop], b and b[prop],
                         LIMITS["edge_px"], "px")
        crops.append(("parity-" + subject, as_tuple(row["rect"]), as_tuple(ref_row["rect"])))


# ------------------------------------------------- the clipboard split view (#102)

# The reference's list scrolls a row the keys select into view with an 8px
# cushion past its edge (componentDidUpdate; the board runtime never runs
# it on the board's frame, so the reference capture applies it the same
# way, __wb.clipboardScroll); GPUI's scroll_to_item scrolls the least that
# shows the row, so a row scrolled into view sits 8px apart.
CUSHION_DISPOSITION = ("accepted (#102): the reference scrolls the selected clip 8px past the list's edge; "
                       "GPUI's scroll_to_item scrolls the least that shows it, as root search's list does")
CUSHION_PX = 8
NO_CLIP_ACTIONS = ("accepted (#102): with no clip listed there is nothing to paste or copy. The approved contract "
                   "(#102: 'zero records means no preview and no primary action') shows only Actions then; the "
                   "reference's footer is static and lists Paste to Obsidian and Copy whatever is selected")


def card_edges(image, rect, scale=1.0):
    """A darkening card's inner edges near rect = (x, y, w, h), logical: its
    left and right along a row 16px above its bottom, its top and bottom
    along a column 16px inside its right edge - where no preview content
    reaches - as the pixels darker than the panel beside it by half the
    card's own darkening. The card's 1px ring is lighter than the panel, so
    the edges found are the ring's inner ones. Returns the inner (x, y, w, h)
    in logical px and the card's fill and the panel's, or None."""
    x, y, w, h = scaled(rect, scale)
    background = median_color(image, (x - 7 * scale, y + h * 0.5 - 2 * scale, x - 3 * scale, y + h * 0.5 + 2 * scale))
    fill = median_color(image, (x + w - 30 * scale, y + h - 30 * scale, x + w - 20 * scale, y + h - 20 * scale))
    if background is None or fill is None:
        return None
    threshold = (luma(background) + luma(fill)) / 2
    if luma(fill) >= luma(background):
        return None
    data = image.load()
    row = int(y + h - 16 * scale)
    column = int(x + w - 16 * scale)
    span_x = range(max(0, int(x - 6 * scale)), min(image.width, int(x + w + 6 * scale)))
    span_y = range(max(0, int(y - 6 * scale)), min(image.height, int(y + h + 6 * scale)))
    dark_x = [c for c in span_x if luma(data[c, row]) <= threshold]
    dark_y = [r for r in span_y if luma(data[column, r]) <= threshold]
    if not dark_x or not dark_y:
        return None
    left, right = min(dark_x), max(dark_x) + 1
    top, bottom = min(dark_y), max(dark_y) + 1
    return {"edges": (left / scale, top / scale, (right - left) / scale, (bottom - top) / scale),
            "fill": fill, "background": background}


def cushioned(scrolled, n_rect, r_rect):
    """Whether a part of a scrolled list lies where the reference's 8px
    cushion puts it: the reference scrolled 8px further, so its part sits
    8px higher than Pane's, to the edge limit."""
    return scrolled and abs((n_rect[1] - r_rect[1]) - CUSHION_PX) <= LIMITS["edge_px"]


def card_box(image, rect, scale=1.0):
    """The preview card around rect = (x, y, w, h), logical, read against
    the panel just outside each edge (local_edges): its 1px white 7% ring
    is its outermost pixel. Its fill (black 24%) as an overlay, read just
    inside its left and right edges against the panel just outside them,
    at the same rows, below its content. The reference's glass shades the
    card's backdrop across and down it, so a fill read far from the panel
    it is compared with misreads it. Returns {"edges", "alpha"} in logical
    px and levels, or None."""
    x, y, w, h = rect
    edges = local_edges(image, scaled(rect, scale), scale)
    alphas = []
    for inside, outside in ((x + 3, x - 8), (x + w - 8, x + w + 3)):
        fill = mean_color(image, box_px((inside, y + h * 0.6, 5, h * 0.3), scale))
        panel = mean_color(image, box_px((outside, y + h * 0.6, 5, h * 0.3), scale))
        if fill and panel:
            alphas.append(black_alpha(fill, panel))
    return {"edges": tuple(v / scale for v in edges) if edges else None,
            "alpha": statistics.mean(alphas) if alphas and None not in alphas else None}


def clip_wash_sides(image, rect, scale=1.0):
    """A clip row's wash left and right, logical px, read against the list's
    padding just beside each end (local_edges, 4px out: the padding is 8px,
    and the panel's own edge ring lies at its outer end), or None."""
    edges = local_edges(image, scaled(rect, scale), scale, out=4)
    if not edges:
        return None
    return edges[0] / scale, (edges[0] + edges[2]) / scale


def measure_clip_row(image, rect, list_rect, washed, title_color, scale=1.0, pal=DARK):
    """A clip row as measure_row reads it, with two differences. A row
    flush with the list's bottom (the keys scrolled it there by the least
    that shows it) has no list gap below it, only the footer's rule, so its
    wash is read against the gap above alone; one flush with the list's top
    (the reference's cushion scrolls the list that far), against the list's
    padding beside it (see row_wash). A washed row's left and right
    are read against the list's padding beside each end (clip_wash_sides):
    the list's 8px padding is narrower than wash_edges' margin, which
    reached the panel's edge ring, and the reference's glass is lighter
    beside the row's right end than beside its left."""
    flush = rect[1] + rect[3] >= list_rect[1] + list_rect[3] - 0.5
    ceiling = rect[1] <= list_rect[1] + 0.5
    measured = measure_row(image, rect, washed, title_color, scale, floor=flush, pal=pal, ceiling=ceiling)
    if washed and measured.get("edges"):
        sides = clip_wash_sides(image, rect, scale)
        left, top, width, height = measured["edges"]
        if sides:
            left, width = sides[0], sides[1] - sides[0]
        # The list's viewport clips a flush row; past it lies the footer's
        # rule, lighter than the panel, which a wash's run would take in.
        height = min(top + height, list_rect[1] + list_rect[3]) - top
        measured["edges"] = (left, top, width, height)
    return measured


def tab_wash(image, rect, scale=1.0, lighter=True):
    """A tab's wash near rect = (x, y, w, h), logical: its fill just under
    its top edge (inside its 1px ring, above its label) against the strip
    just above it - the tabs sit 4px apart, so the strip beside a tab may
    be its chosen neighbour's wash - as overlay alpha, and its edges.
    Returns {"alpha", "edges"} or None."""
    x, y, w, h = scaled(rect, scale)
    background = median_color(image, (x + 4 * scale, y - 5 * scale, x + w - 4 * scale, y - 2 * scale))
    fill = median_color(image, (x + 6 * scale, y + 2 * scale, x + w - 6 * scale, y + 4 * scale))
    if background is None or fill is None:
        return None
    sign = 1 if lighter else -1
    delta = max(3.0, sign * (luma(fill) - luma(background)) / 2)
    edges = wash_edges(image, (x, y, w, h), background, delta, margin=3, lighter=lighter)
    return {"alpha": overlay_alpha(fill, background, lighter),
            "edges": tuple(v / scale for v in edges) if edges else None}


def clip_row_ink(image, rect, wash_fill, pal, scale=1.0):
    """A clip row's title ink (after the 28px mark) and its time's right
    edge (the row's right end, in the muted ink), logical px."""
    x, y, w, h = rect
    title = ink_extent(image, (x + 44, y + 4, w * 0.6, h - 8), wash_fill, (237, 237, 239), scale)
    time = ink_extent(image, (x + w * 0.72, y + 4, w * 0.28, h - 8), wash_fill, pal["muted"], scale)
    return title, time


def compare_clipboard(report, name, capture, declared, manifest, native, scale, ref_capture, reference_image, crops):
    """The split view: the native side against its declaration, then
    against the reference's clipboard board in the same state."""
    clip = declared.get("clipboard")
    if not clip:
        return
    pal = palette(manifest)
    colors = manifest["declared"]["colors"]
    selected_alpha = hex_rgba(colors["rowSelected"])[3]
    hover_alpha = hex_rgba(colors["rowHover"])[3]
    title_color = hex_rgba(colors["textTitle"])[:3]
    width = clip["header"]["width"]
    list_rect = as_tuple(clip["list"])
    footer_rect = as_tuple(clip["footer"])
    strip = as_tuple(clip["tabsStrip"])

    # ---- the native side against its declaration
    header_rule = hline(native, (int((list_rect[2] + 20) * scale), int((width - 160) * scale)),
                        (int((strip[1] - 5) * scale), int((strip[1] + 4) * scale)), lighter=pal["lighter"])
    report.check("harness-native", name, capture, "clip-header", "rule y", header_rule and header_rule / scale,
                 strip[1] - 1, LIMITS["edge_px"], "px")
    strip_rule = hline(native, (int((list_rect[2] + 20) * scale), int((width - 40) * scale)),
                       (int((strip[1] + strip[3] - 5) * scale), int((strip[1] + strip[3] + 4) * scale)),
                       lighter=pal["lighter"])
    report.check("harness-native", name, capture, "clip-tabs", "rule y", strip_rule and strip_rule / scale,
                 strip[1] + strip[3] - 1, LIMITS["edge_px"], "px")
    footer = measure_footer(native, footer_rect[1], scale, pal["lighter"])
    report.check("harness-native", name, capture, "clip-footer", "rule y", footer["rule"], footer_rect[1],
                 LIMITS["edge_px"], "px")
    report.check("harness-native", name, capture, "clip-footer", "tint alpha", footer["alpha"],
                 hex_rgba(colors["footerTint"])[3], LIMITS["flat_fill_levels"], "levels")
    body_bottom = footer_rect[1]
    list_rule = vline(native, (int((list_rect[2] - 6) * scale), int((list_rect[2] + 5) * scale)),
                      (int((body_bottom - 40) * scale), int((body_bottom - 6) * scale)),
                      lighter=pal["lighter"])
    report.check("harness-native", name, capture, "clip-list", "rule x", list_rule and list_rule / scale,
                 list_rect[0] + list_rect[2] - 1, LIMITS["edge_px"], "px")
    native_rows = {}
    for row in clip["rows"]:
        if not row["visible"]:
            continue
        rect = as_tuple(row["rect"])
        washed = row["selected"] or row["hovered"]
        measured = measure_clip_row(native, rect, list_rect, washed, title_color, scale, pal)
        native_rows[row["title"]] = (row, measured)
        subject = f"clip:{row['title']}"
        if row["selected"]:
            expected, state = selected_alpha, "selected"
        elif row["hovered"]:
            expected, state = hover_alpha, "hovered"
        else:
            expected, state = 0, "rest"
        report.check("harness-native", name, capture, subject, f"wash alpha ({state})", measured.get("alpha"),
                     expected, LIMITS["flat_fill_levels"], "levels")
        if row["selected"]:
            edges = measured.get("edges")
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                report.check("harness-native", name, capture, subject, f"wash {prop}", edges[index] if edges else None,
                             rect[index], LIMITS["edge_px"], "px")
        crops.append(("clip-" + row["title"], rect, None))
    preview = clip.get("preview")
    if preview and preview["kind"] in ("code", "text"):
        rect = as_tuple(preview["rect"])
        card = card_edges(native, rect, scale)
        inner = (rect[0] + 1, rect[1] + 1, rect[2] - 2, rect[3] - 2)
        for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
            report.check("harness-native", name, capture, "clip-preview", f"card {prop} (inside its ring)",
                         card and card["edges"][index], inner[index], LIMITS["edge_px"], "px")
        report.check("harness-native", name, capture, "clip-preview", "card fill alpha",
                      card and black_alpha(card["fill"], card["background"]),
                      hex_rgba(clip["colors"]["previewFill"])[3], LIMITS["flat_fill_levels"], "levels")
        crops.append(("clip-preview", rect, None))
    for tab in clip.get("tabs", []):
        rect = as_tuple(tab["rect"])
        measured = tab_wash(native, rect, scale, pal["lighter"])
        expected = hex_rgba(clip["colors"]["tabOn"])[3] if tab["on"] else 0
        report.check("harness-native", name, capture, f"clip-tab:{tab['label']}",
                     f"wash alpha ({'on' if tab['on'] else 'rest'})", measured and measured["alpha"], expected,
                     LIMITS["flat_fill_levels"], "levels")
        if tab["on"]:
            edges = measured and measured["edges"]
            for prop, index in (("left", 0), ("width", 2)):
                report.check("harness-native", name, capture, f"clip-tab:{tab['label']}", f"wash {prop}",
                             edges[index] if edges else None, rect[index], CHAIN_SLACK_PX, "px", CHAIN_NOTE)
        crops.append((f"clip-tab-{tab['label']}", rect, None))
    back = as_tuple(clip["back"])
    measured = measure_cap(native, back, scale, pal["lighter"])
    report.check("harness-native", name, capture, "clip-back", "fill alpha", measured and measured["alpha"],
                 hex_rgba(clip["colors"]["backFill"])[3], LIMITS["flat_fill_levels"], "levels")
    edges = measured and measured["edges"]
    for prop, index in (("left", 0), ("top", 1), ("width", 2)):
        report.check("harness-native", name, capture, "clip-back", prop, edges[index] if edges else None,
                     back[index], LIMITS["edge_px"], "px")
    strip_background = footer_strip_background(native, footer_rect, scale)
    copied = clip["copied"]
    lead = coverage_edges(native, (copied["rect"]["x"] - 2, footer_rect[1] + 1, 300, footer_rect[3] - 1),
                          strip_background, pal["muted"], scale)
    report.check("harness-native", name, capture, "clip-copied", "ink left", lead and lead["left"],
                 copied["rect"]["x"], LIMITS["edge_px"] * 1.5, "px", "a glyph's side bearing allowed")
    for button in clip.get("buttons", []):
        rect = as_tuple(button["rect"])
        label = ink_extent(native, (rect[0], rect[1], rect[2] * 0.6, rect[3]), strip_background,
                           hex_rgba(colors["footerButtonText"])[:3], scale)
        report.check("harness-native", name, capture, f"clip-button:{button['text']}", "label ink left",
                     label and label["left"], rect[0] + manifest["declared"]["geometry"].get("actionPaddingX", 8),
                     CHAIN_SLACK_PX + 0.5, "px", CHAIN_NOTE + "; a glyph's side bearing allowed")
        crops.append((f"clip-button-{button['text']}", rect, None))
    if clip.get("empty"):
        note = as_tuple(clip["empty"]["rect"])
        background = panel_background(native, note[1] + 20, scale)
        found = ink_extent(native, note, background, pal["muted"], scale)
        report.check("harness-native", name, capture, "clip-empty", "ink present", "yes" if found else "no", "yes", 0, "")
        crops.append(("clip-empty", note, None))

    if ref_capture is None:
        return
    # ---- the reference side against its DOM
    state = ref_capture["state"]
    dom_strip = as_tuple(state["strip"])
    ref_strip_rule = hline(reference_image, (int(dom_strip[0] + 380), int(dom_strip[0] + dom_strip[2] - 40)),
                           (int(dom_strip[1] + dom_strip[3] - 5), int(dom_strip[1] + dom_strip[3] + 4)))
    report.check("harness-reference", name, capture, "clip-tabs", "rule y", ref_strip_rule,
                 dom_strip[1] + dom_strip[3] - 1, LIMITS["edge_px"], "px")
    ref_header_rule = hline(reference_image, (380, int(dom_strip[2] - 160)),
                            (int(dom_strip[1] - 5), int(dom_strip[1] + 4)))
    ref_footer = measure_footer(reference_image, as_tuple(state["footer"])[1])
    ref_rows = {}
    for row in state["rows"]:
        if not row["visible"]:
            continue
        rect = as_tuple(row["rect"])
        washed = row["selected"] or row["hovered"]
        measured = measure_clip_row(reference_image, rect, as_tuple(state["list"]), washed, title_color)
        ref_rows[row["title"]] = (row, measured)
        report.check("harness-reference", name, capture, f"clip:{row['title']}", "wash alpha vs DOM background",
                     measured.get("alpha"), css_rgba(row["background"])[3], LIMITS["flat_fill_levels"], "levels",
                     "the DOM serializes .085 as .086")

    # ---- parity: the same component in the same state
    native_selected = ", ".join(row["title"] for row in clip["rows"] if row["selected"])
    ref_selected = ", ".join(row["title"] for row in state["rows"] if row["selected"])
    report.check("parity", name, capture, "clip-selection", "selected clip", native_selected, ref_selected, 0, "",
                 "a click selects, never copies; the keys move within what is listed (#102)")
    report.check("parity", name, capture, "clip-list", "listed clips",
                 " | ".join(row["title"] for row in clip["rows"]), " | ".join(row["title"] for row in state["rows"]),
                 0, "")
    report.check("parity", name, capture, "clip-header", "rule y", header_rule and header_rule / scale,
                 ref_header_rule, LIMITS["edge_px"], "px")
    report.check("parity", name, capture, "clip-tabs", "rule y", strip_rule and strip_rule / scale, ref_strip_rule,
                 LIMITS["edge_px"], "px")
    report.check("parity", name, capture, "clip-footer", "rule y", footer["rule"], ref_footer["rule"],
                 LIMITS["edge_px"], "px")
    report.check("parity", name, capture, "clip-footer", "tint alpha", footer["alpha"], ref_footer["alpha"],
                 LIMITS["flat_fill_levels"], "levels")
    ref_list = as_tuple(state["list"])
    ref_list_rule = vline(reference_image, (int(ref_list[0] + ref_list[2] - 6), int(ref_list[0] + ref_list[2] + 5)),
                          (int(ref_list[1] + ref_list[3] - 40), int(ref_list[1] + ref_list[3] - 6)))
    report.check("parity", name, capture, "clip-list", "rule x", list_rule and list_rule / scale, ref_list_rule,
                 LIMITS["edge_px"], "px")
    scrolled = name == "clipboard-keys" and capture == "last"
    for title, (row, measured) in native_rows.items():
        if title not in ref_rows:
            continue
        ref_row, ref_measured = ref_rows[title]
        subject = f"clip:{title}"
        n_rect, r_rect = as_tuple(row["rect"]), as_tuple(ref_row["rect"])
        crops.append(("parity-clip-" + title, n_rect, r_rect))
        report.check("parity", name, capture, subject, "top in client", n_rect[1], r_rect[1], LIMITS["edge_px"], "px",
                     accepted=CUSHION_DISPOSITION if cushioned(scrolled, n_rect, r_rect) else None)
        report.check("parity", name, capture, subject, "left and width",
                     f"{n_rect[0]:g}/{n_rect[2]:g}", f"{r_rect[0]:g}/{r_rect[2]:g}", 0, "")
        report.check("parity", name, capture, subject, "wash alpha", measured.get("alpha"), ref_measured.get("alpha"),
                     LIMITS["flat_fill_levels"], "levels")
        n_edges, r_edges = measured.get("edges"), ref_measured.get("edges")
        if n_edges and r_edges and row["selected"]:
            for prop, index in (("left", 0), ("width", 2), ("height", 3)):
                report.check("parity", name, capture, subject, f"wash {prop}", n_edges[index], r_edges[index],
                             LIMITS["edge_px"], "px")
        n_fill = native_wash_fill(native, row, scale)
        r_fill = native_wash_fill(reference_image, ref_row, 1.0)
        if n_fill and r_fill:
            n_title, n_time = clip_row_ink(native, n_rect, n_fill, pal, scale)
            r_title, r_time = clip_row_ink(reference_image, r_rect, r_fill, DARK)
            if n_title and r_title:
                for prop in ("left", "top"):
                    report.check("parity", name, capture, subject, f"title ink {prop} in row",
                                 n_title[prop] - n_rect[prop == "top"], r_title[prop] - r_rect[prop == "top"],
                                 LIMITS["edge_px"], "px")
                report.check("parity", name, capture, subject, "title ink height", n_title["bottom"] - n_title["top"],
                             r_title["bottom"] - r_title["top"], LIMITS["edge_px"], "px")
                worst = color_delta(n_title["color"], r_title["color"])
                report.check("parity", name, capture, subject, "title glyph core color (max channel)", worst, 0,
                             LIMITS["glyph_core_levels"], "levels",
                             f"native {fmt_color(n_title['color'])} vs reference {fmt_color(r_title['color'])}")
            if n_time and r_time:
                report.check("parity", name, capture, subject, "time ink right in row",
                             n_time["right"] - n_rect[0], r_time["right"] - r_rect[0], LIMITS["edge_px"] * 1.5, "px",
                             "right-aligned; a glyph's side bearing allowed")
    ref_labels = {label["text"]: label for label in state["labels"] if label["visible"]}
    for label in clip["sections"]:
        ref = ref_labels.get(label["label"])
        if not ref:
            continue
        subject = f"clip-section:{label['label']}"
        n_rect, r_rect = as_tuple(label["rect"]), as_tuple(ref["rect"])
        report.check("parity", name, capture, subject, "top in client", n_rect[1], r_rect[1], LIMITS["edge_px"], "px",
                     accepted=CUSHION_DISPOSITION if cushioned(scrolled, n_rect, r_rect) else None)
        n_left, _ = section_ink(native, n_rect, scale, pal)
        r_left, _ = section_ink(reference_image, r_rect)
        if n_left and r_left:
            for prop, index in (("left", 0), ("top", 1)):
                report.check("parity", name, capture, subject, f"ink {prop} in label",
                             n_left["box"][index] / scale - n_rect[index], r_left["box"][index] - r_rect[index],
                             LIMITS["edge_px"], "px")
    ref_tabs = {tab["label"]: tab for tab in state["tabs"]}
    report.check("parity", name, capture, "clip-tabs", "chosen tab",
                 ", ".join(tab["label"] for tab in clip.get("tabs", []) if tab["on"]),
                 ", ".join(tab["label"] for tab in state["tabs"] if tab["on"]), 0, "")
    for tab in clip.get("tabs", []):
        ref = ref_tabs.get(tab["label"])
        if not ref:
            continue
        subject = f"clip-tab:{tab['label']}"
        n_rect, r_rect = as_tuple(tab["rect"]), as_tuple(ref["rect"])
        crops.append(("parity-" + subject, n_rect, r_rect))
        mine = tab_wash(native, n_rect, scale, pal["lighter"])
        theirs = tab_wash(reference_image, r_rect)
        report.check("parity", name, capture, subject, "wash alpha", mine and mine["alpha"], theirs and theirs["alpha"],
                     LIMITS["flat_fill_levels"], "levels")
        if tab["on"] and mine and theirs and mine["edges"] and theirs["edges"]:
            for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
                report.check("parity", name, capture, subject, f"wash {prop}", mine["edges"][index],
                             theirs["edges"][index], LIMITS["edge_px"], "px")
    if preview or state.get("card"):
        report.check("parity", name, capture, "clip-preview", "branch", preview["kind"] if preview else "none",
                     state["card"]["kind"] if state.get("card") else "none", 0, "",
                     "no clip listed: no preview card on either side")
    if preview and state.get("card") and preview["kind"] in ("code", "text"):
        mine = card_box(native, as_tuple(preview["rect"]), scale)
        theirs = card_box(reference_image, as_tuple(state["card"]["rect"]))
        for prop, index in (("left", 0), ("top", 1), ("width", 2), ("height", 3)):
            report.check("parity", name, capture, "clip-preview", f"card {prop}",
                         mine["edges"] and mine["edges"][index], theirs["edges"] and theirs["edges"][index],
                         LIMITS["edge_px"], "px")
        report.check("parity", name, capture, "clip-preview", "card fill alpha", mine["alpha"], theirs["alpha"],
                     LIMITS["flat_fill_levels"], "levels")
        crops.append(("parity-clip-preview", as_tuple(preview["rect"]), as_tuple(state["card"]["rect"])))
    if state.get("copied"):
        report.check("parity", name, capture, "clip-copied", "text", copied["text"], state["copied"]["text"], 0, "")
        ref_strip_background = footer_strip_background(reference_image, as_tuple(state["footer"]))
        theirs = coverage_edges(reference_image, (state["copied"]["rect"]["x"] - 2, as_tuple(state["footer"])[1] + 1,
                                                  300, as_tuple(state["footer"])[3] - 1),
                                ref_strip_background, MUTED)
        if lead and theirs:
            for prop in ("left", "top"):
                report.check("parity", name, capture, "clip-copied", f"ink {prop}", lead[prop], theirs[prop],
                             LIMITS["edge_px"], "px")
    n_buttons = [button["text"] for button in clip.get("buttons", [])]
    r_buttons = [button["label"] for button in state.get("buttons", [])]
    # With no clip selected Pane keeps only the buttons that need none: the
    # reference's own buttons, less those that act on a clip.
    idle = not preview and n_buttons == r_buttons[len(r_buttons) - len(n_buttons):]
    report.check("parity", name, capture, "clip-footer", "buttons", " | ".join(n_buttons), " | ".join(r_buttons), 0,
                 "", accepted=NO_CLIP_ACTIONS if idle else None)
    if state.get("empty") or clip.get("empty"):
        report.check("parity", name, capture, "clip-empty", "note",
                     clip["empty"]["text"] if clip.get("empty") else "",
                     state["empty"]["text"] if state.get("empty") else "", 0, "")


# ------------------------------------------------------------- evidence


def write_images(out_dir, name, capture, native, reference_image, crops, scale):
    base = Path(out_dir, name)
    base.mkdir(parents=True, exist_ok=True)
    record = {"scenario": name, "capture": capture, "native": f"{name}/{capture}-native.png"}
    native.save(base / f"{capture}-native.png")
    gap = 8
    if reference_image is not None:
        record["reference"] = f"{name}/{capture}-reference.png"
        reference_image.save(base / f"{capture}-reference.png")
        side = Image.new("RGB", (native.width + gap + reference_image.width, max(native.height, reference_image.height)), "#FF00FF")
        side.paste(native, (0, 0))
        side.paste(reference_image, (native.width + gap, 0))
        side.save(base / f"{capture}-side-by-side.png")
        record["sideBySide"] = f"{name}/{capture}-side-by-side.png"
        if native.size == reference_image.size:
            diff = ImageChops.difference(native, reference_image).point(lambda v: min(255, v * 4))
            diff.save(base / f"{capture}-diff-x4.png")
            record["diff"] = f"{name}/{capture}-diff-x4.png"
    overlay = native.copy()
    draw = ImageDraw.Draw(overlay)
    for crop_name, native_rect, ref_rect in crops:
        if native_rect:
            x, y, w, h = scaled(native_rect, scale)
            draw.rectangle((x, y, x + w - 1, y + h - 1), outline="#00FF66")
        if ref_rect:
            x, y, w, h = ref_rect
            draw.rectangle((x, y, x + w - 1, y + h - 1), outline="#FF3399")
    overlay.save(base / f"{capture}-overlay.png")
    record["overlay"] = f"{name}/{capture}-overlay.png"
    record["crops"] = []
    for crop_name, native_rect, ref_rect in crops:
        pad = 8
        if not ref_rect or reference_image is None:
            if crop_name.startswith("parity-"):
                continue
            nx, ny, nw, nh = scaled(native_rect, scale)
            box = clamp_box(native, (nx - pad, ny - pad, nx + nw + pad, ny + nh + pad))
            if not box_visible(box):
                # Scrolled out of the client (the narrow frame's pinned
                # home above its last row): nothing to crop.
                continue
            safe = "".join(c if c.isalnum() or c in "-_" else "-" for c in crop_name)[:60]
            native.crop(box).save(base / f"{capture}-native-crop-{safe}.png")
            record["crops"].append(f"{name}/{capture}-native-crop-{safe}.png")
            continue
        nx, ny, nw, nh = scaled(native_rect, scale)
        rx, ry, rw, rh = ref_rect
        n_box = clamp_box(native, (nx - pad, ny - pad, nx + nw + pad, ny + nh + pad))
        r_box = clamp_box(reference_image, (rx - pad, ry - pad, rx + rw + pad, ry + rh + pad))
        if not (box_visible(n_box) and box_visible(r_box)):
            continue
        n_crop = native.crop(n_box)
        r_crop = reference_image.crop(r_box)
        pair = Image.new("RGB", (max(n_crop.width, r_crop.width), n_crop.height + gap + r_crop.height), "#FF00FF")
        pair.paste(n_crop, (0, 0))
        pair.paste(r_crop, (0, n_crop.height + gap))
        safe = "".join(c if c.isalnum() or c in "-_" else "-" for c in crop_name)[:60]
        pair.save(base / f"{capture}-crop-{safe}.png")
        record["crops"].append(f"{name}/{capture}-crop-{safe}.png")
    return record


def sensitivity(current, baseline):
    """Checks that passed in the baseline and fail now, with both values."""
    before = {check["id"]: check for check in baseline["checks"]}
    flips = []
    for check in current["checks"]:
        old = before.get(check["id"])
        if old and old["passed"] and not check["passed"]:
            value = "measured" if "measured" in check else "native"
            flips.append({
                "id": check["id"],
                "baseline": old.get(value),
                "now": check.get(value),
                "expected": check.get("expected", check.get("reference")),
                "delta": check["delta"],
                "limit": check["limit"],
                "unit": check["unit"],
            })
    return flips


def summary_markdown(result):
    lines = [f"# Visual workbench comparison: {result['meta']['label']}", ""]
    lines.append(f"Reference SHA-256 `{result['meta']['referenceSha256']}`, {result['meta']['chrome']}.")
    lines.append(f"Limits: {result['meta']['limits']}.")
    lines.append("")
    for scenario in result["meta"]["scenarios"]:
        lines.append(f"- `{scenario['name']}`: {scenario['dpi']} DPI, client {scenario['client']}, "
                     f"{scenario['material']}, perturbation {scenario['perturbation']}")
    lines.append("")
    lines.append("| Section | Passed | Failed | Accepted discrepancies |")
    lines.append("|---|---|---|---|")
    for section, totals in result["totals"].items():
        lines.append(f"| {section} | {totals['passed']} | {totals['failed']} | {totals.get('accepted', 0)} |")
    if "sensitivity" in result:
        lines += ["", "## Sensitivity: passed in the baseline, failing now", ""]
        lines.append("| Check | Baseline | Now | Expected | Delta | Limit |")
        lines.append("|---|---|---|---|---|---|")
        for flip in result["sensitivity"]:
            lines.append(f"| `{flip['id']}` | {flip['baseline']} | {flip['now']} | {flip['expected']} | "
                         f"{flip['delta']} | {flip['limit']} {flip['unit']} |")
    for section in ("harness-native", "harness-reference", "parity"):
        failed = [c for c in result["checks"] if c["section"] == section and not c["passed"] and not c.get("accepted")]
        lines += ["", f"## {section}: {len(failed)} failed", ""]
        if failed:
            lines.append("| Check | Native/measured | Reference/expected | Delta | Limit | Note |")
            lines.append("|---|---|---|---|---|---|")
            for check in failed:
                left = check.get("native", check.get("measured"))
                right = check.get("reference", check.get("expected"))
                lines.append(f"| `{check['scenario']}/{check['capture']}/{check['subject']}/{check['property']}` | {left} | "
                             f"{right} | {check['delta']} | {check['limit']} {check['unit']} | {check.get('note', '')} |")
    accepted = [c for c in result["checks"] if c.get("accepted")]
    if accepted:
        lines += ["", f"## Accepted discrepancies: {len(accepted)} (failing, with a recorded disposition)", ""]
        lines.append("| Check | Native/measured | Reference/expected | Delta | Limit | Disposition |")
        lines.append("|---|---|---|---|---|---|")
        for check in accepted:
            left = check.get("native", check.get("measured"))
            right = check.get("reference", check.get("expected"))
            lines.append(f"| `{check['scenario']}/{check['capture']}/{check['subject']}/{check['property']}` | {left} | "
                         f"{right} | {check['delta']} | {check['limit']} {check['unit']} | {check['accepted']} |")
    if result["nativeOnly"]:
        lines += ["", "## Native-only captures (no reference counterpart; not compared)", ""]
        for entry in result["nativeOnly"]:
            lines.append(f"- `{entry['scenario']}`: {', '.join(entry['captures'])}")
    if result["pending"]:
        lines += ["", "## Pending scenarios (reference saved; native fixture not registered yet)", ""]
        for entry in result["pending"]:
            lines.append(f"- `{entry['name']}` ({entry['board']} board, {entry['client'][0]:g}x{entry['client'][1]:g}): {entry['ticket']}")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--native", required=True)
    parser.add_argument("--reference", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--baseline")
    parser.add_argument("--label", default="baseline")
    args = parser.parse_args()
    Path(args.out).mkdir(parents=True, exist_ok=True)
    result = compare(args.native, args.reference, args.out, args.label)
    if args.baseline:
        result["sensitivity"] = sensitivity(result, json.loads(Path(args.baseline).read_text(encoding="utf-8")))
    Path(args.out, "report.json").write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")
    Path(args.out, "summary.md").write_text(summary_markdown(result), encoding="utf-8")
    for section, totals in result["totals"].items():
        print(f"{section}: {totals['passed']} passed, {totals['failed']} failed, {totals.get('accepted', 0)} accepted")
    if "sensitivity" in result:
        print(f"sensitivity: {len(result['sensitivity'])} checks flipped from passed to failed")


if __name__ == "__main__":
    main()
