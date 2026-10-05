"""Offline tests for the visual workbench's measurement math; never launch
anything. They check the measuring itself on synthetic images with known
answers - not token values, which the captures check against the real
renders.

Run: python -m unittest discover -s scripts/visual-workbench -p test_compare.py
"""
import unittest

from PIL import Image, ImageDraw

import compare


def over_white(background, alpha):
    """`background` under a white overlay of `alpha` (0-1), as a browser or
    GPUI composites it."""
    return tuple(round(b + (255 - b) * alpha) for b in background)


class OverlayAlpha(unittest.TestCase):
    def test_white_alpha_recovers_the_overlay_over_any_background(self):
        for background in [(22, 23, 26), (40, 44, 50), (90, 90, 90)]:
            fill = over_white(background, 0.085)
            self.assertAlmostEqual(compare.white_alpha(fill, background), 0.085 * 255, delta=1.5)

    def test_black_alpha_recovers_a_darkening_overlay(self):
        background = (40, 41, 45)
        fill = tuple(round(b * (1 - 0.14)) for b in background)
        self.assertAlmostEqual(compare.black_alpha(fill, background), 0.14 * 255, delta=2.0)


class Runs(unittest.TestCase):
    def test_a_single_dark_sample_does_not_split_a_run(self):
        lit = [False, True, True, False, True, True, False, False, True]
        self.assertEqual(compare.lit_run(lit, 4), (1, 6))

    def test_a_two_sample_gap_separates_neighbors(self):
        # Two rows' washes with the list's 2px gap between them.
        lit = [True] * 5 + [False] * 2 + [True] * 5
        self.assertEqual(compare.lit_run(lit, 9), (7, 12))


class WashEdges(unittest.TestCase):
    def panel(self):
        image = Image.new("RGB", (200, 120), (22, 23, 26))
        draw = ImageDraw.Draw(image)
        # A rule above, a neighbor's wash 2px above, and the wash itself
        # with a dark tile shadow and bright text inside it.
        draw.line((0, 10, 199, 10), fill=(40, 41, 44))
        draw.rectangle((11, 14, 188, 37), fill=over_white((22, 23, 26), 0.085))
        draw.rounded_rectangle((11, 40, 188, 83), radius=10, fill=over_white((22, 23, 26), 0.085))
        draw.rectangle((19, 48, 23, 75), fill=(5, 5, 6))      # a tile's drop shadow
        draw.rectangle((24, 48, 51, 75), fill=(200, 90, 40))  # the tile
        draw.text((60, 55), "Title", fill=(237, 237, 239))
        return image

    def test_edges_ignore_the_neighbor_the_rule_and_the_content(self):
        image = self.panel()
        edges = compare.wash_edges(image, (11, 40, 178, 44), (22, 23, 26), 9)
        self.assertEqual(edges, (11, 40, 178, 44))

    def test_a_shifted_wash_is_measured_where_it_is(self):
        image = Image.new("RGB", (200, 80), (22, 23, 26))
        ImageDraw.Draw(image).rounded_rectangle((15, 20, 184, 63), radius=10, fill=(41, 42, 45))
        # Looked for at its declared place, 4px to the left.
        edges = compare.wash_edges(image, (11, 20, 178, 44), (22, 23, 26), 9)
        self.assertEqual(edges[0], 15)
        self.assertEqual(edges[2], 170)


class RowWash(unittest.TestCase):
    def test_the_alpha_holds_over_a_backdrop_that_varies_across_and_down(self):
        image = Image.new("RGB", (300, 120))
        data = image.load()
        for y in range(120):
            for x in range(300):
                data[x, y] = (20 + x // 15, 21 + x // 15, 25 + x // 15 - y // 30)
        row = (10, 30, 280, 44)
        for y in range(30, 74):
            for x in range(10, 290):
                data[x, y] = over_white(data[x, y], 0.085)
        wash = compare.row_wash(image, row)
        self.assertAlmostEqual(wash["alpha"], 0.085 * 255, delta=1.0)

    def test_an_unwashed_row_measures_no_alpha(self):
        image = Image.new("RGB", (300, 120), (22, 23, 26))
        self.assertAlmostEqual(compare.row_wash(image, (10, 30, 280, 44))["alpha"], 0, delta=0.5)


class Lines(unittest.TestCase):
    def test_a_rule_is_found_beside_a_wash_edge(self):
        image = Image.new("RGB", (300, 100), (22, 23, 26))
        draw = ImageDraw.Draw(image)
        draw.line((0, 64, 299, 64), fill=(37, 38, 41))                    # the rule
        draw.rectangle((10, 68, 289, 99), fill=over_white((22, 23, 26), 0.085))  # a wash below
        self.assertEqual(compare.hline(image, (0, 300), (60, 69)), 64)


class Ink(unittest.TestCase):
    def test_core_pixels_exclude_antialiased_edges_and_the_caret(self):
        image = Image.new("RGB", (100, 40), (22, 23, 26))
        draw = ImageDraw.Draw(image)
        draw.rectangle((20, 10, 29, 25), fill=(237, 237, 239))  # a glyph's core
        draw.line((19, 10, 19, 25), fill=(100, 100, 102))       # its antialiased edge
        draw.line((40, 8, 40, 30), fill=(201, 238, 106))        # the lime caret
        found = compare.ink(image, (0, 0, 100, 40), (22, 23, 26), (237, 237, 239))
        self.assertEqual(found["box"], (20, 10, 10, 16))
        self.assertEqual(found["color"], (237, 237, 239))

    def test_a_stem_split_across_two_columns_still_marks_the_ink_top(self):
        # A 'b': its ascender's stem falls half in each of two columns, so
        # neither pixel is core; its bowl below is.
        background, text = (22, 23, 26), (185, 186, 190)
        half = tuple(round((b + t) / 2) for b, t in zip(background, text))
        image = Image.new("RGB", (40, 30), background)
        draw = ImageDraw.Draw(image)
        draw.rectangle((10, 5, 11, 6), fill=half)   # the split stem
        draw.rectangle((10, 7, 16, 12), fill=text)  # the bowl
        self.assertEqual(compare.ink(image, (0, 0, 40, 30), background, text)["box"][1], 7)
        self.assertEqual(compare.ink_top(image, (0, 0, 40, 30), background, text), 5)

    def test_a_faint_ring_is_not_ink(self):
        background, text = (22, 23, 26), (185, 186, 190)
        image = Image.new("RGB", (40, 30), background)
        draw = ImageDraw.Draw(image)
        draw.rectangle((4, 2, 30, 20), outline=over_white(background, 0.14))  # the chip's ring
        draw.rectangle((10, 8, 16, 12), fill=text)
        self.assertEqual(compare.ink_top(image, (0, 0, 40, 30), background, text), 8)
        self.assertIsNone(compare.ink_top(image, (0, 0, 40, 6), background, text))


class Frame(unittest.TestCase):
    def panel(self, radius=0):
        """A 760x518 dark panel with a 1px inset ring at 7.5%, a top
        highlight at 10%, optionally rounded over a darker backdrop."""
        backdrop, base = (8, 9, 10), (22, 23, 26)
        image = Image.new("RGB", (760, 518), backdrop if radius else base)
        draw = ImageDraw.Draw(image)
        ring = over_white(base, 0.075)
        draw.rounded_rectangle((0, 0, 759, 517), radius=radius, fill=ring)
        draw.rounded_rectangle((1, 1, 758, 516), radius=max(0, radius - 1), fill=base)
        top = over_white(ring, 0.10)
        draw.line((radius, 0, 759 - radius, 0), fill=top)
        return image

    def test_the_inset_ring_measures_its_alpha_on_each_side(self):
        image = self.panel()
        for edge in ("left", "right", "bottom"):
            self.assertAlmostEqual(compare.edge_alpha(image, edge), 0.075 * 255, delta=1.5, msg=edge)
        combined = 255 * (1 - 0.925 * 0.9)
        self.assertAlmostEqual(compare.edge_alpha(image, "top"), combined, delta=1.5)

    def test_a_missing_ring_measures_none(self):
        image = Image.new("RGB", (760, 518), (22, 23, 26))
        self.assertAlmostEqual(compare.edge_alpha(image, "left"), 0, delta=0.5)

    def test_a_square_corner_has_no_inset_and_a_rounded_one_its_radius(self):
        self.assertEqual(compare.corner_inset(self.panel()), 0)
        inset = compare.corner_inset(self.panel(radius=18))
        # r(1 - 1/sqrt 2) = 5.3 for 18px, within the diagonal's pixel steps.
        self.assertTrue(4 <= inset <= 6, inset)


def over_black(background, alpha):
    return tuple(round(b * (1 - alpha)) for b in background)


class Keycaps(unittest.TestCase):
    def cap(self, x=20, y=20, w=36, h=20, label=True):
        """A reference .kbd on a dark panel: a white 7% fill, the black 35%
        bottom line under a white 8% ring, a bright label in the middle."""
        panel = (22, 23, 26)
        image = Image.new("RGB", (120, 70), panel)
        draw = ImageDraw.Draw(image)
        fill = over_white(panel, 0.07)
        draw.rounded_rectangle((x, y, x + w - 1, y + h - 1), radius=5, fill=over_white(fill, 0.08))
        draw.rounded_rectangle((x + 1, y + 1, x + w - 2, y + h - 2), radius=4, fill=fill)
        bottom = over_white(over_black(fill, 0.35), 0.08)
        draw.line((x + 5, y + h - 1, x + w - 6, y + h - 1), fill=bottom)
        if label:
            draw.rectangle((x + 8, y + 6, x + w - 9, y + 13), fill=(201, 202, 206))
        return image

    def test_edges_of_a_rounded_cap_with_a_label(self):
        image = self.cap()
        measured = compare.measure_key(image, (20, 20, 36, 20), False, (201, 202, 206))
        left, top, width, _ = measured["edges"]
        self.assertEqual((left, top, width), (20, 20, 36))
        self.assertAlmostEqual(measured["alpha"], 0.07 * 255, delta=1.5)

    def test_the_bottom_line_is_recovered_from_under_the_ring(self):
        measured = compare.measure_key(self.cap(), (20, 20, 36, 20), False, (201, 202, 206))
        self.assertAlmostEqual(measured["bottom"], 0.35 * 255, delta=4)

    def test_a_cap_off_whole_pixels_reports_no_bottom_line(self):
        self.assertIsNone(compare.bottom_line_alpha(self.cap(), (20, 20.5, 36, 20), (40, 41, 44)))


class ResultBoards(unittest.TestCase):
    """The no-results notice's disc and the answer card (#96)."""

    PANEL = (22, 23, 26)
    ACCENT = (201, 238, 106)

    def disc(self, x=20, y=20, size=44, ring=True, glyph=True):
        """The notice's disc: white 6% under a white 8% ring, a lighter
        glyph in its middle."""
        image = Image.new("RGB", (120, 90), self.PANEL)
        draw = ImageDraw.Draw(image)
        fill = over_white(self.PANEL, 0.06)
        box = (x, y, x + size - 1, y + size - 1)
        draw.ellipse(box, fill=over_white(fill, 0.08) if ring else fill)
        draw.ellipse((x + 1, y + 1, x + size - 2, y + size - 2), fill=fill)
        if glyph:
            draw.ellipse((x + 14, y + 14, x + 28, y + 28), outline=(163, 164, 169), width=2)
        return image

    def card(self, ring):
        """A 300x120 card: white 6%, a 1px accent ring when selected, a
        white value in the middle of its left half."""
        image = Image.new("RGB", (340, 160), self.PANEL)
        draw = ImageDraw.Draw(image)
        fill = over_white(self.PANEL, 0.06)
        draw.rounded_rectangle((20, 20, 319, 139), radius=14, fill=self.ACCENT if ring else fill)
        draw.rounded_rectangle((21, 21, 318, 138), radius=13, fill=fill)
        draw.rectangle((70, 60, 109, 89), fill=(255, 255, 255))  # the value's core
        return image

    def test_a_disc_with_a_glyph_measures_its_box_and_fill(self):
        measured = compare.measure_box(self.disc(), (20, 20, 44, 44))
        left, top, width, height = measured["edges"]
        # The circle's chord across its middle rows is its diameter, or a
        # pixel short of it on the rows either side.
        self.assertTrue(19 <= left <= 21 and 43 <= width <= 45, measured["edges"])
        self.assertEqual((top, height), (20, 44))
        self.assertAlmostEqual(measured["alpha"], 0.06 * 255, delta=1.5)

    def test_a_shifted_disc_is_measured_where_it_is(self):
        measured = compare.measure_box(self.disc(x=24), (20, 20, 44, 44))
        self.assertTrue(23 <= measured["edges"][0] <= 25, measured["edges"])

    def test_the_accent_ring_marks_only_the_selected_card(self):
        self.assertTrue(compare.accent_ring(self.card(ring=True), (20, 20, 300, 120)))
        self.assertFalse(compare.accent_ring(self.card(ring=False), (20, 20, 300, 120)))
        # Measured a row off, it is found in the row below.
        self.assertTrue(compare.accent_ring(self.card(ring=True), (20, 19, 300, 120)))

    def test_a_cards_edges_and_fill_hold_under_its_ring(self):
        for ring in (True, False):
            measured = compare.measure_box(self.card(ring), (20, 20, 300, 120))
            self.assertEqual(measured["edges"], (20, 20, 300, 120), ring)
            self.assertAlmostEqual(measured["alpha"], 0.06 * 255, delta=1.5)

    def test_a_values_ink_is_found_inside_its_widened_line_box(self):
        image = self.card(ring=True)
        fill = over_white(self.PANEL, 0.06)
        # Declared 4px off its ink, as shaping may leave it.
        found = compare.measure_text(image, (74, 56, 40, 40), fill, (255, 255, 255))
        self.assertEqual((found["left"], found["top"], found["right"]), (70, 60, 110))
        self.assertEqual((found["height"], found["center"]), (30, 90))
        self.assertIsNone(compare.measure_text(image, (74, 56, 40, 40), None, (255, 255, 255)))


class SplitView(unittest.TestCase):
    """The clipboard split view's measures (#102)."""

    panel = (22, 23, 26)

    def test_a_vertical_rule_is_found_beside_a_dark_card(self):
        image = Image.new("RGB", (200, 120), self.panel)
        draw = ImageDraw.Draw(image)
        draw.line((59, 0, 59, 119), fill=over_white(self.panel, 0.06))      # the list's rule
        draw.rectangle((72, 10, 190, 110), fill=over_black(self.panel, 0.24))  # a card beyond it
        self.assertEqual(compare.vline(image, (54, 66), (20, 100)), 59)

    def test_a_cards_edges_are_found_inside_its_ring_past_its_content(self):
        image = Image.new("RGB", (300, 200), self.panel)
        draw = ImageDraw.Draw(image)
        fill = over_black(self.panel, 0.24)
        # The card at (40, 20, 220, 160): a lighter 1px ring, then the fill,
        # with bright text across its upper part.
        draw.rectangle((40, 20, 259, 179), fill=over_white(fill, 0.07))
        draw.rectangle((41, 21, 258, 178), fill=fill)
        draw.rectangle((60, 40, 240, 70), fill=(217, 218, 221))
        found = compare.card_edges(image, (40, 20, 220, 160))
        self.assertEqual(found["edges"], (41, 21, 218, 158))
        self.assertAlmostEqual(compare.black_alpha(found["fill"], found["background"]), 0.24 * 255, delta=2)

    def test_a_lighter_card_has_no_dark_edges(self):
        image = Image.new("RGB", (300, 200), self.panel)
        ImageDraw.Draw(image).rectangle((40, 20, 259, 179), fill=(201, 238, 106))  # a color preview
        self.assertIsNone(compare.card_edges(image, (40, 20, 220, 160)))

    def test_a_tabs_wash_is_read_against_the_strip_above_not_its_neighbour(self):
        image = Image.new("RGB", (200, 60), self.panel)
        draw = ImageDraw.Draw(image)
        on = over_white(self.panel, 0.10)
        # A chosen tab at (10, 15, 40, 30), its ring at 6%, its label; an
        # unchosen neighbour 4px to its right.
        draw.rounded_rectangle((10, 15, 49, 44), radius=8, fill=over_white(on, 0.06))
        draw.rounded_rectangle((11, 16, 48, 43), radius=7, fill=on)
        draw.rectangle((20, 24, 38, 34), fill=(255, 255, 255))
        draw.rectangle((60, 24, 80, 34), fill=(154, 155, 160))
        chosen = compare.tab_wash(image, (10, 15, 40, 30))
        self.assertAlmostEqual(chosen["alpha"], 0.10 * 255, delta=1.5)
        self.assertEqual((chosen["edges"][0], chosen["edges"][2]), (10, 40))
        rest = compare.tab_wash(image, (53, 15, 34, 30))
        self.assertAlmostEqual(rest["alpha"], 0, delta=0.5)


class PinnedSlots(unittest.TestCase):
    PANEL = (22, 23, 26)

    def slot(self, alpha=0.035, x=10, y=30, w=142, h=100):
        """A reference .slot on a dark panel: a white fill under a white 5%
        inset ring, a 42px gradient tile centered across it and a bright
        title line centered below the tile."""
        image = Image.new("RGB", (180, 160), self.PANEL)
        draw = ImageDraw.Draw(image)
        fill = over_white(self.PANEL, alpha)
        draw.rounded_rectangle((x, y, x + w - 1, y + h - 1), radius=12, fill=over_white(fill, 0.05))
        draw.rounded_rectangle((x + 1, y + 1, x + w - 2, y + h - 2), radius=11, fill=fill)
        tile_x, tile_y = x + (w - 42) // 2, y + 18
        for row in range(42):
            shade = (74 - row, 77 - row, 85 - row)
            draw.line((tile_x, tile_y + row, tile_x + 41, tile_y + row), fill=shade)
        draw.rectangle((x + 46, tile_y + 42 + 12, x + w - 47, tile_y + 42 + 21), fill=(217, 218, 221))
        return image, (x, y, w, h), (tile_x, tile_y, 42, 42), (x + 8, tile_y + 51, w - 16, 16.25)

    def test_a_slot_measures_its_fill_box_title_and_tile(self):
        image, rect, tile, title = self.slot()
        measured = compare.measure_slot(image, rect, title, tile, True)
        self.assertAlmostEqual(measured["alpha"], 0.035 * 255, delta=1.5)
        self.assertEqual(measured["edges"], rect)
        self.assertAlmostEqual(measured["title"]["center"], rect[0] + rect[2] / 2, delta=0.5)
        self.assertEqual(measured["title"]["top"], tile[1] + 42 + 12)
        self.assertEqual(measured["tile"]["edges"][:2], tile[:2])

    def test_the_hover_wash_measures_twice_the_rest(self):
        image, rect, tile, title = self.slot(alpha=0.07)
        self.assertAlmostEqual(compare.measure_slot(image, rect, title, tile, True)["alpha"], 0.07 * 255, delta=1.5)

    def test_an_empty_slot_measures_no_fill(self):
        image = Image.new("RGB", (180, 160), self.PANEL)
        measured = compare.measure_slot(image, (10, 30, 142, 100), None, None, False)
        self.assertAlmostEqual(measured["alpha"], 0, delta=0.5)
        self.assertIsNone(measured["title"])

    def test_a_label_only_the_reference_shows_counts_above_its_rows(self):
        declared = {"sections": [{"rect": {"x": 10, "y": 68, "width": 740, "height": 30}},
                                 {"rect": {"x": 10, "y": 210, "width": 740, "height": 30}}]}
        state = {"labels": [{"rect": {"x": 10, "y": 68, "width": 740, "height": 30}},
                            {"rect": {"x": 10, "y": 210, "width": 740, "height": 30}},
                            {"rect": {"x": 10, "y": 426, "width": 740, "height": 30}}]}
        # Above the fourth row both sides show the same two labels; above
        # the fifth the reference shows its third.
        self.assertEqual(compare.extra_labels_above(declared, state, 380, 380), 0)
        self.assertEqual(compare.extra_labels_above(declared, state, 426, 458), 1)


class Accepted(unittest.TestCase):
    def test_a_failing_check_with_a_disposition_is_accepted_not_failed(self):
        report = compare.Report()
        report.check("parity", "s", "c", "frame", "corner", 0, 5, 1, "px", accepted="platform limit")
        report.check("parity", "s", "c", "frame", "edge", 0, 5, 1, "px")
        report.check("parity", "s", "c", "frame", "ok", 5, 5, 1, "px", accepted="unused")
        self.assertEqual([c.get("accepted") for c in report.checks], ["platform limit", None, None])
        self.assertEqual([c["passed"] for c in report.checks], [False, False, True])


class Sensitivity(unittest.TestCase):
    def test_only_checks_that_passed_before_and_fail_now_flip(self):
        def check(identifier, passed, value):
            return {"id": identifier, "passed": passed, "measured": value, "expected": 11, "delta": value - 11,
                    "limit": 1, "unit": "px"}

        baseline = {"checks": [check("a", True, 11), check("b", False, 20), check("c", True, 11)]}
        current = {"checks": [check("a", False, 15), check("b", False, 20), check("c", True, 11.5)]}
        flips = compare.sensitivity(current, baseline)
        self.assertEqual([flip["id"] for flip in flips], ["a"])
        self.assertEqual((flips[0]["baseline"], flips[0]["now"], flips[0]["delta"]), (11, 15, 4))


class SettingsShell(unittest.TestCase):
    """The Settings shell's measures (#97)."""

    def sidebar(self):
        # The sidebar's black 10% fill left of x 100, its 1px white rule at
        # x 99, and the page to the right.
        image = Image.new("RGB", (200, 120), (25, 26, 29))
        draw = ImageDraw.Draw(image)
        draw.rectangle((0, 0, 98, 119), fill=(22, 23, 26))
        draw.line((99, 0, 99, 119), fill=over_white((22, 23, 26), 0.06))
        return image

    def test_vline_finds_the_rule_not_the_step_between_two_fills(self):
        image = self.sidebar()
        self.assertEqual(compare.vline(image, (90, 110), (10, 110)), 99)

    def test_vline_finds_a_darker_rule_in_the_light_palette(self):
        image = Image.new("RGB", (200, 120), (240, 240, 242))
        ImageDraw.Draw(image).line((60, 0, 60, 119), fill=(225, 225, 227))
        self.assertEqual(compare.vline(image, (50, 70), (10, 110), lighter=False), 60)

    def test_a_ringed_box_is_its_fill_and_its_ring(self):
        # The search well: black 24% under a white 6% ring at 10,60 211x34,
        # with its magnifier and placeholder lighter than the fill inside.
        background = (22, 23, 26)
        fill = tuple(round(c * 0.76) for c in background)
        image = Image.new("RGB", (240, 120), background)
        draw = ImageDraw.Draw(image)
        draw.rectangle((10, 60, 220, 93), fill=over_white(fill, 0.06))
        draw.rectangle((11, 61, 219, 92), fill=fill)
        draw.rectangle((20, 70, 33, 83), fill=(142, 143, 148))   # the magnifier
        draw.rectangle((42, 70, 140, 84), fill=(134, 135, 140))  # the placeholder
        self.assertEqual(compare.ringed_box(image, (10, 60, 211, 34), background, fill), (10, 60, 211, 34))

    def test_a_ringed_box_is_measured_where_it_is(self):
        background = (22, 23, 26)
        fill = tuple(round(c * 0.76) for c in background)
        image = Image.new("RGB", (240, 120), background)
        draw = ImageDraw.Draw(image)
        draw.rectangle((12, 61, 222, 94), fill=over_white(fill, 0.06))
        draw.rectangle((13, 62, 221, 93), fill=fill)
        # Looked for at its declared place, 2px left of and 1px above it.
        edges = compare.ringed_box(image, (10, 60, 211, 34), background, fill)
        self.assertEqual(edges, (12, 61, 211, 34))


class AppearanceControls(unittest.TestCase):
    """The Appearance page's measures (#98)."""

    PAGE = (22, 23, 26)
    LIME = (201, 238, 106)

    def track(self, chosen=0, count=3, x=20, y=20, w=388):
        """A .segwrap at x,y: black 24% under a white 6% ring, its segments
        3px in and 2px apart, the chosen one white 12%, each with a label
        block in its middle; returns the image, the track and the
        segments."""
        image = Image.new("RGB", (w + 60, 80), self.PAGE)
        draw = ImageDraw.Draw(image)
        fill = over_black(self.PAGE, 0.24)
        draw.rectangle((x, y, x + w - 1, y + 35), fill=over_white(fill, 0.06))
        draw.rectangle((x + 1, y + 1, x + w - 2, y + 34), fill=fill)
        width = (w - 6 - 2 * (count - 1)) / count
        segments = []
        for index in range(count):
            left = x + 3 + index * (width + 2)
            rect = (left, y + 3, width, 30)
            if index == chosen:
                draw.rectangle((round(left), y + 3, round(left + width) - 1, y + 32), fill=over_white(fill, 0.12))
            label = (255, 255, 255) if index == chosen else (154, 155, 160)
            middle = left + width / 2
            draw.rectangle((round(middle - 12), y + 13, round(middle + 12), y + 21), fill=label)
            segments.append({"rect": rect, "labelBox": (middle - 12, y + 9.5, 25, 17), "chosen": index == chosen,
                             "hovered": False})
        return image, (x, y, w, 36), segments

    def test_a_track_measures_its_ring_box_and_black_fill(self):
        image, rect, _ = self.track()
        measured = compare.measure_track(image, rect)
        self.assertEqual(measured["edges"], rect)
        self.assertAlmostEqual(measured["alpha"], 0.24 * 255, delta=4)

    @staticmethod
    def ink(color):
        return lambda fill: color

    def test_the_chosen_segment_measures_its_wash_edges_and_label(self):
        image, rect, segments = self.track(chosen=1)
        chosen = compare.measure_segment(image, segments[1], self.ink((255, 255, 255)))
        self.assertAlmostEqual(chosen["alpha"], 0.12 * 255, delta=1.5)
        left, top, width, height = chosen["edges"]
        self.assertEqual((top, height), (23, 30))
        self.assertTrue(abs(left - segments[1]["rect"][0]) <= 1 and abs(width - 126) <= 1, chosen["edges"])
        middle = segments[1]["rect"][0] + 63
        self.assertAlmostEqual(chosen["label"]["center"], middle, delta=1)
        rest = compare.measure_segment(image, segments[0], self.ink((154, 155, 160)))
        self.assertAlmostEqual(rest["alpha"], 0, delta=0.5)
        self.assertNotIn("edges", rest)

    def shade_across(self, image, left, right):
        """Shades image's page from left levels at its left edge to right at
        its right, keeping every pixel's offset from the page: the
        reference's glass behind a track."""
        data = image.load()
        for x in range(image.width):
            shift = left + (right - left) * x / (image.width - 1) - self.PAGE[0]
            for y in range(image.height):
                data[x, y] = tuple(int(round(c + shift)) for c in data[x, y])

    def test_a_track_over_a_shaded_page_keeps_its_ring_box(self):
        # The reference's glass: 25 levels at the track's left, 38 at its
        # right, which a single page read beside the left end lost.
        image, rect, segments = self.track(chosen=1)
        self.shade_across(image, 24, 40)
        self.assertEqual(compare.measure_track(image, rect)["edges"], rect)
        # Each segment against the track's fill just left of it.
        far = compare.measure_segment(image, segments[2], self.ink((154, 155, 160)))
        self.assertAlmostEqual(far["alpha"], 0, delta=1.5)
        chosen = compare.measure_segment(image, segments[1], self.ink((255, 255, 255)))
        self.assertAlmostEqual(chosen["alpha"], 0.12 * 255, delta=2.5)

    def test_a_disabled_track_is_found_by_its_ring(self):
        # At 40% the fill moves the page by 2 levels; the ring by 4.
        image = Image.new("RGB", (448, 80), self.PAGE)
        draw = ImageDraw.Draw(image)
        fill = over_black(self.PAGE, 0.24 * 0.4)
        draw.rectangle((20, 20, 407, 55), fill=over_white(fill, 0.06 * 0.4))
        draw.rectangle((21, 21, 406, 54), fill=fill)
        self.assertEqual(compare.measure_track(image, (20, 20, 388, 36))["edges"], (20, 20, 388, 36))

    def test_an_overlays_limit_is_two_channel_levels_over_its_background(self):
        self.assertAlmostEqual(compare.overlay_limit((25, 25, 25), False), 20.4, delta=0.1)
        self.assertAlmostEqual(compare.overlay_limit((221, 221, 221), True), 15.0, delta=0.1)
        self.assertAlmostEqual(compare.overlay_limit((19, 19, 19), True), 2.16, delta=0.01)
        self.assertEqual(compare.overlay_limit(None, True), 2.0)

    def test_the_light_caret_is_found_by_its_darkened_green(self):
        image = Image.new("RGB", (60, 30), (237, 237, 238))
        ImageDraw.Draw(image).rectangle((31, 5, 32, 21), fill=(92, 122, 23))
        self.assertEqual(compare.caret_left(image, (30, 5, 1.5, 17)), 31)
        # ClearType's fringes beside a glyph lean red or blue.
        fringes = Image.new("RGB", (60, 30), (237, 237, 238))
        ImageDraw.Draw(fringes).rectangle((31, 5, 31, 21), fill=(171, 111, 35))
        ImageDraw.Draw(fringes).rectangle((32, 5, 32, 21), fill=(72, 143, 198))
        self.assertIsNone(compare.caret_left(fringes, (30, 5, 1.5, 17)))

    def test_a_miniature_row_follows_its_side_paddings_shade(self):
        # The light miniature shades from its top and levels off at its
        # first row; a selected row (black 8.6%) is read against the gap
        # above it moved by what its side padding does down to the strip.
        image = Image.new("RGB", (120, 80), (246, 246, 246))
        data = image.load()
        for y in range(0, 26):
            for x in range(120):
                data[x, y] = (230 + y * 16 // 26,) * 3
        wash = over_black((246, 246, 246), 22 / 255)
        ImageDraw.Draw(image).rectangle((10, 22, 109, 59), fill=wash)
        found = compare.mini_row_wash(image, (10, 22, 100, 38), lighter=False)
        self.assertAlmostEqual(found["alpha"], 22, delta=1.5)
        # A backdrop darker mid-row than at its sides (the glass miniature)
        # leaves an unwashed row unwashed.
        glass = Image.new("RGB", (120, 80), (20, 21, 24))
        ImageDraw.Draw(glass).rectangle((20, 0, 99, 79), fill=(18, 19, 22))
        self.assertAlmostEqual(compare.mini_row_wash(glass, (10, 22, 100, 38))["alpha"], 0, delta=0.5)

    def test_a_chosen_swatch_is_measured_with_its_rings(self):
        image = Image.new("RGB", (120, 80), self.PAGE)
        draw = ImageDraw.Draw(image)
        draw.ellipse((16, 16, 53, 53), fill=self.LIME)        # the 2px ring, 4px out
        draw.ellipse((18, 18, 51, 51), fill=(26, 27, 30))     # the 2px gap
        draw.ellipse((20, 20, 49, 49), fill=self.LIME)        # the disc
        edges = compare.measure_swatch(image, {"rect": (20, 20, 30, 30), "chosen": True})
        self.assertTrue(abs(edges[0] - 16) <= 1 and abs(edges[2] - 38) <= 1, edges)

    def test_a_toggle_measures_its_track_and_where_its_knob_begins(self):
        image = Image.new("RGB", (120, 60), self.PAGE)
        draw = ImageDraw.Draw(image)
        draw.rounded_rectangle((40, 10, 79, 33), radius=12, fill=self.LIME)
        draw.ellipse((59, 13, 76, 30), fill=(255, 255, 255))
        measured = compare.measure_toggle(image, {"rect": (40, 10, 40, 24)})
        self.assertEqual(measured["edges"][:3], (40, 10, 40))
        self.assertAlmostEqual(measured["knob"], 59, delta=1)

    def test_the_caret_is_found_by_its_accent(self):
        image = Image.new("RGB", (60, 30), self.PAGE)
        ImageDraw.Draw(image).rectangle((31, 5, 32, 21), fill=self.LIME)
        self.assertEqual(compare.caret_left(image, (30, 5, 1.5, 17)), 31)
        self.assertIsNone(compare.caret_left(Image.new("RGB", (60, 30), self.PAGE), (30, 5, 1.5, 17)))

    def test_the_boards_state_takes_the_fixtures_shape(self):
        rect = {"x": 10, "y": 20, "width": 30, "height": 18}
        state = {
            "labels": [{"text": "Material", "rect": rect, "opacity": 1}],
            "tracks": [{"field": "material", "rect": rect, "opacity": 0.4, "background": "rgba(0, 0, 0, 0.24)",
                        "segments": [{"label": "Solid", "rect": rect, "labelBox": rect, "chosen": True,
                                      "hovered": False, "background": "rgba(255, 255, 255, 0.12)"}]}],
            "descriptions": [], "swatches": [], "sliders": [], "toggles": [],
        }
        page = compare.reference_appearance(state)
        self.assertEqual(page["tracks"][0]["segments"][0]["target"], "segment-solid")
        self.assertEqual(page["tracks"][0]["opacity"], 0.4)
        self.assertIsNone(page["preview"])
        # Measuring a page that shows nothing finds nothing, without failing.
        colors = {"title": (237, 237, 239), "muted": (142, 143, 148), "body": (163, 164, 169),
                  "query": (243, 243, 245), "onText": (255, 255, 255), "hoverText": (237, 237, 239),
                  "segmentText": (154, 155, 160)}
        measured = compare.measure_appearance(Image.new("RGB", (80, 60), self.PAGE), page, colors)
        self.assertIsNone(measured["labels"][0])


class PanePages(unittest.TestCase):
    """The measures of Pane's own Settings pages and the form (#99)."""

    PAGE = (22, 23, 26)

    def test_a_knob_begins_3px_in_from_the_side_its_state_names(self):
        rect = {"x": 100, "y": 10, "width": 40, "height": 24}
        self.assertEqual(compare.knob_left({"rect": rect, "on": False}), 103)
        self.assertEqual(compare.knob_left({"rect": rect, "on": True}), 119)

    def test_a_line_is_inside_its_box_within_the_slack(self):
        rect = (10, 20, 100, 18)
        self.assertTrue(compare.line_inside({"top": 24, "height": 10}, rect))
        self.assertTrue(compare.line_inside({"top": 19.5, "height": 10}, rect))
        self.assertFalse(compare.line_inside({"top": 30, "height": 10}, rect))
        self.assertFalse(compare.line_inside(None, rect))

    def test_a_settings_rows_rule_is_found_where_it_runs(self):
        # A 1px white 6% rule along a row's top at y 40, the page around it.
        image = Image.new("RGB", (480, 100), self.PAGE)
        ImageDraw.Draw(image).line((10, 40, 470, 40), fill=over_white(self.PAGE, 0.06))
        part = {"kind": "rule", "rect": {"x": 10, "y": 40, "width": 460, "height": 1}}
        self.assertEqual(compare.measure_page_part(image, part)["y"], 40)

    def test_a_button_measures_its_box_and_white_fill(self):
        # A .pill: white 8% at 20,20 120x30 with its label inside.
        image = Image.new("RGB", (200, 80), self.PAGE)
        draw = ImageDraw.Draw(image)
        draw.rectangle((20, 20, 139, 49), fill=over_white(self.PAGE, 0.08))
        draw.rectangle((40, 30, 120, 40), fill=(237, 237, 239))
        part = {"kind": "button", "rect": {"x": 20, "y": 20, "width": 120, "height": 30}}
        measured = compare.measure_page_part(image, part)
        self.assertEqual(measured["edges"], (20, 20, 120, 30))
        self.assertAlmostEqual(measured["alpha"], 0.08 * 255, delta=3)

    def test_a_well_measures_its_ring_box_and_black_fill(self):
        image = Image.new("RGB", (240, 80), self.PAGE)
        draw = ImageDraw.Draw(image)
        fill = over_black(self.PAGE, 0.24)
        draw.rectangle((40, 20, 199, 49), fill=over_white(fill, 0.06))
        draw.rectangle((41, 21, 198, 48), fill=fill)
        part = {"kind": "well", "rect": {"x": 40, "y": 20, "width": 160, "height": 30}}
        measured = compare.measure_page_part(image, part)
        self.assertEqual(measured["edges"], (40, 20, 160, 30))
        self.assertAlmostEqual(measured["alpha"], 0.24 * 255, delta=4)

    def test_a_chosen_segment_reads_its_wash_over_the_tracks_padding(self):
        image = Image.new("RGB", (240, 80), self.PAGE)
        draw = ImageDraw.Draw(image)
        fill = over_black(self.PAGE, 0.24)
        draw.rectangle((20, 20, 219, 55), fill=fill)
        draw.rectangle((23, 23, 120, 52), fill=over_white(fill, 0.12))
        chosen = {"kind": "segment", "rect": {"x": 23, "y": 23, "width": 98, "height": 30}}
        rest = {"kind": "segment", "rect": {"x": 123, "y": 23, "width": 94, "height": 30}}
        self.assertAlmostEqual(compare.measure_page_part(image, chosen)["alpha"], 0.12 * 255, delta=2)
        self.assertAlmostEqual(compare.measure_page_part(image, rest)["alpha"], 0, delta=0.5)

    def test_a_headings_ink_begins_its_first_glyphs_side_bearing_in(self):
        # Geist SemiBold's stems sit 80 units in, its G 43 and its A 19: at
        # the heading's 22px, 1.76, 0.95 and 0.42px.
        for text, bearing in (("Keyboard", 1.76), ("Launcher", 1.76), ("Extensions", 1.76),
                              ("General", 0.95), ("Appearance", 0.42)):
            self.assertAlmostEqual(compare.first_glyph_bearing(text), bearing, delta=0.03, msg=text)
        self.assertEqual(compare.first_glyph_bearing(""), 0.0)

    def test_an_open_popover_moves_the_sidebars_fill_below_its_shadow(self):
        layout = {"sidebar": (0, 47, 233, 673),
                  "items": [{"rect": (8, 330, 216, 36)}]}
        self.assertEqual(compare.sidebar_rows(layout), (376, 526))
        self.assertEqual(compare.sidebar_rows(layout, popover=True), (610, 710))
        # A sidebar too short for both keeps its rows below the last section.
        short = {"sidebar": (0, 47, 233, 400), "items": [{"rect": (8, 330, 216, 36)}]}
        self.assertEqual(compare.sidebar_rows(short, popover=True), (376, 437))


class ReferenceGuards(unittest.TestCase):
    """#103: a reference board drawn in a fallback face, a pointer left
    over a board by an earlier scenario, or two sides at different scales
    each fail a check."""

    # Two unicode-range subsets per weight, as the reference embeds them.
    FACES = [
        {"family": "Geist", "weight": "400", "status": "unloaded"},
        {"family": "Geist", "weight": "400", "status": "loaded"},
        {"family": "Geist", "weight": "600", "status": "unloaded"},
        {"family": "Geist", "weight": "600", "status": "unloaded"},
        {"family": "Geist Mono", "weight": "500", "status": "error"},
        {"family": "Geist Mono", "weight": "100 900", "status": "loaded"},
    ]

    def test_a_weight_is_loaded_when_one_of_its_subsets_loaded(self):
        self.assertEqual(compare.face_status(self.FACES, "Geist", 400), "loaded")

    def test_a_weight_no_subset_of_which_loaded_is_reported(self):
        self.assertEqual(compare.face_status(self.FACES, "Geist", 600), "not loaded")
        self.assertEqual(compare.face_status(self.FACES, "Geist", 500), "absent")

    def test_a_variable_face_covers_the_weights_in_its_range(self):
        self.assertEqual(compare.face_status(self.FACES, "Geist Mono", 400), "loaded")
        failed = [{"family": "Geist Mono", "weight": "500", "status": "error"}]
        self.assertEqual(compare.face_status(failed, "Geist Mono", 500), "error")

    def test_the_font_check_fails_a_board_missing_a_face_or_its_record(self):
        report = compare.Report()
        compare.check_reference_fonts(report, "s", "c", "settings", self.FACES)
        verdicts = {c["property"]: c["passed"] for c in report.checks}
        self.assertTrue(verdicts["Geist 400 loaded"])
        self.assertFalse(verdicts["Geist 500 loaded"])
        self.assertFalse(verdicts["Geist 600 loaded"])
        self.assertFalse(verdicts["faces failed"])
        report = compare.Report()
        compare.check_reference_fonts(report, "s", "c", "root", None)
        self.assertFalse(report.checks[0]["passed"])

    def test_a_pointer_parked_off_the_board_or_on_the_field_rests_where_it_was_put(self):
        parked = {"page": [2, 2], "board": None, "steps": 0, "fieldClick": False}
        field = {"page": [400, 300], "board": [380, 32], "steps": 0, "fieldClick": True}
        for pointer in (parked, field):
            measured, expected = compare.pointer_rest(pointer)
            self.assertEqual(measured, expected)

    def test_a_pointer_over_a_board_before_any_step_fails(self):
        leftover = {"page": [400, 500], "board": [380, 200], "steps": 0, "fieldClick": False}
        measured, expected = compare.pointer_rest(leftover)
        self.assertNotEqual(measured, expected)
        # The rest's click leaves it on the field, not below the header.
        below = {"page": [400, 500], "board": [380, 200], "steps": 0, "fieldClick": True}
        self.assertNotEqual(*compare.pointer_rest(below))
        self.assertNotEqual(*compare.pointer_rest(None))

    def test_a_pointer_step_must_land_on_the_board(self):
        self.assertEqual(*compare.pointer_rest({"board": [10, 100], "steps": 1}))
        self.assertNotEqual(*compare.pointer_rest({"board": None, "steps": 1}))

    def test_parity_needs_the_same_device_scale(self):
        self.assertEqual(compare.scale_parity({"dpi": 96}, {"deviceScaleFactor": 1}), (1.0, 1))
        native, reference = compare.scale_parity({"dpi": 120}, {"deviceScaleFactor": 1})
        report = compare.Report()
        self.assertFalse(report.check("parity", "s", "c", "client", "device scale", native, reference, 0, "")["passed"])
        self.assertEqual(compare.scale_parity({}, {"deviceScaleFactor": 1}), (None, 1))


if __name__ == "__main__":
    unittest.main()
