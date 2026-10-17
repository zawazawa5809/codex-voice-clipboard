"""Verify real tray icon files: sizes, transparency, state distinctions and contrast."""

import pathlib
import unittest
from PIL import Image

ICONS = pathlib.Path(__file__).resolve().parents[1] / "src-tauri" / "icons"
TRAY = ICONS / "tray"
STATES = ("active", "paused", "error")
THEMES = ("dark", "light")
SIZES = (32, 64)


def tray(state, theme, size):
    return Image.open(TRAY / f"{state}-{theme}-{size}.png").convert("RGBA")


class IconTests(unittest.TestCase):
    def test_every_state_theme_and_size_exists_at_its_size(self):
        for state in STATES:
            for theme in THEMES:
                for size in SIZES:
                    with self.subTest(state=state, theme=theme, size=size):
                        self.assertEqual((size, size), tray(state, theme, size).size)

    def test_every_tray_icon_is_visible_and_has_transparent_corners(self):
        for file in TRAY.glob("*.png"):
            with self.subTest(icon=file.name):
                frame = Image.open(file).convert("RGBA")
                self.assertIsNotNone(frame.getbbox())
                w, h = frame.size
                for point in ((0, 0), (0, h - 1), (w - 1, 0), (w - 1, h - 1)):
                    self.assertEqual(0, frame.getpixel(point)[3])

    def test_states_remain_distinct_at_the_smallest_size(self):
        for theme in THEMES:
            frames = {tray(state, theme, 32).tobytes() for state in STATES}
            self.assertEqual(3, len(frames))

    def test_normal_icon_has_contrast_for_each_menu_bar_theme(self):
        for theme in THEMES:
            frame = tray("active", theme, 32)
            pixels = list(frame.getdata())
            opaque = [sum(p[:3]) / 3 for p in pixels if p[3] > 240]
            self.assertGreater(len(opaque), 10)
            luminance = sum(opaque) / len(opaque)
            if theme == "dark":
                self.assertGreater(luminance, 200)
            else:
                self.assertLess(luminance, 80)

    def test_paused_and_error_are_colored_not_monochrome(self):
        for state in ("paused", "error"):
            for theme in THEMES:
                pixels = [p for p in tray(state, theme, 64).getdata() if p[3] > 240]
                spread = max(max(p[:3]) - min(p[:3]) for p in pixels)
                self.assertGreater(spread, 60, f"{state}-{theme}")

    def test_app_icon_source_is_square_and_large(self):
        with Image.open(ICONS / "app-source.png") as source:
            self.assertEqual((1024, 1024), source.size)


if __name__ == "__main__":
    unittest.main()
