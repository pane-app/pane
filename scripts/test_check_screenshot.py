"""Offline regression tests for smoke image assertions; never launch Pane.

Run: python -m unittest discover -s scripts -p test_check_screenshot.py
"""
import contextlib
import io
from pathlib import Path
import tempfile
import unittest

from PIL import Image, ImageDraw

import check_screenshot as check


class ScreenshotChecks(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.output = contextlib.redirect_stdout(io.StringIO())
        self.output.__enter__()
        self.addCleanup(self.output.__exit__, None, None, None)

    def panel(self):
        image = Image.new("RGB", (820, 520), "#abcdef")
        draw = ImageDraw.Draw(image)
        draw.rectangle((30, 30, 789, 489), fill="#16171a")
        # A gradient and footer must not be cropped away with the old flat
        # background strategy. All coordinates below are screenshot pixels.
        for y in range(165):
            r = 34 - round(12 * y / 164)
            draw.line((30, 30 + y, 789, 30 + y), fill=(r, r + 1, r + 4))
        draw.rectangle((30, 440, 789, 489), fill="#131416")
        return image, draw

    def save(self, image, name="frame.png", scale=1):
        path = str(Path(self.directory.name) / name)
        image.resize((image.width * scale, image.height * scale), Image.Resampling.NEAREST).save(path)
        return path

    def test_whole_panel_and_desktop_isolation(self):
        image, draw = self.panel()
        draw.rectangle((2, 2, 12, 12), fill="#16171a")
        draw.rectangle((40, 460, 100, 463), fill="#9fd8a8")
        first = self.save(image)
        self.assertEqual(check.window_box(first)[1], (30, 30, 790, 490))
        check.main(first, "success")
        with self.assertRaises(SystemExit):
            check.absent(first, "success")
        draw.rectangle((0, 0, 20, 20), fill="red")
        check.same(first, self.save(image, "desktop-changed.png"))
        draw.rectangle((40, 460, 100, 463), fill="#131416")
        missing = self.save(image, "missing-result.png")
        check.absent(missing, "success")
        with self.assertRaises(SystemExit):
            check.main(missing, "success")
        with self.assertRaises(SystemExit):
            check.same(first, missing)
        check.distinct([first, missing])

    def test_selection_rejects_sheen_hover_and_tiles(self):
        image, draw = self.panel()
        draw.rectangle((42, 100, 777, 143), fill="#252629")  # hover
        draw.rectangle((52, 108, 79, 135), fill="#303134")  # neutral tile
        with self.assertRaises(SystemExit):
            check.selected(self.save(image))
        draw.rectangle((42, 100, 777, 143), fill="#2a2b2e")
        check.selected(self.save(image))

    def test_same_allows_only_one_pixel_of_rounding_noise(self):
        image, _ = self.panel()
        image.putpixel((330, 300), (178, 92, 46))
        first = self.save(image, "original.png")
        image.putpixel((330, 300), (178, 92, 47))
        check.same(first, self.save(image, "one-level.png"))
        image.putpixel((330, 300), (178, 92, 48))
        with self.assertRaises(SystemExit):
            check.same(first, self.save(image, "two-levels.png"))
        image.putpixel((330, 300), (178, 92, 47))
        image.putpixel((331, 300), (23, 23, 26))
        with self.assertRaises(SystemExit):
            check.same(first, self.save(image, "two-pixels.png"))

    def test_same_rejects_changed_row_hover(self):
        image, draw = self.panel()
        first = self.save(image, "no-hover.png")
        draw.rectangle((42, 143, 777, 186), fill="#1e1f22")
        with self.assertRaises(SystemExit):
            check.same(first, self.save(image, "hover.png"))

    def test_preview_requires_metadata_in_header_band_at_each_scale(self):
        image, draw = self.panel()
        draw.rectangle((90, 52, 250, 65), fill="#86878c")  # root placeholder
        draw.rectangle((90, 110, 250, 112), fill="#a3a4a9")  # lower text
        for scale in (1, 2):
            with self.assertRaises(SystemExit):
                check.preview(self.save(image, scale=scale))
        draw.rectangle((50, 80, 230, 83), fill="#a3a4a9")
        for scale in (1, 2):
            check.preview(self.save(image, scale=scale))

    def test_progress_and_subtitle_cannot_pass_from_wrong_region(self):
        image, draw = self.panel()
        draw.rectangle((80, 110, 260, 113), fill="#d6a36a")  # unavailable reason
        draw.rectangle((50, 460, 230, 463), fill="#8e8f94")  # idle hint
        path = self.save(image)
        for role in ("progress", "subtitle"):
            with self.assertRaises(SystemExit):
                check.main(path, role)
        draw.rectangle((50, 460, 230, 463), fill="#d6a36a")
        draw.rectangle((80, 110, 260, 113), fill="#8e8f94")
        path = self.save(image)
        for role in ("progress", "subtitle"):
            check.main(path, role)

    def test_guest_swatches_are_unchanged_and_locatable(self):
        for color in ("1e88e5", "8e24aa", "1b5e20"):
            image, draw = self.panel()
            draw.rectangle((130, 150, 229, 199), fill="#" + color)
            path = self.save(image)
            check.main(path, color, 3000)
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                check.locate(path, color)
            self.assertEqual(output.getvalue().strip(), "179 174")
            with self.assertRaises(SystemExit):
                check.absent(path, color)

    def test_linux_native_preview_with_antialiased_metadata(self):
        fixtures = Path(__file__).parent / "fixtures" / "linux-preview"
        check.preview(str(fixtures / "package.png"))
        with self.assertRaises(SystemExit):
            check.preview(str(fixtures / "root.png"))


if __name__ == "__main__":
    unittest.main()
