"""Verify real icon files, all Windows sizes, transparency and state distinctions."""

import pathlib
import unittest
from PIL import Image

ASSETS = (
    pathlib.Path(__file__).resolve().parents[1]
    / "src"
    / "CodexVoiceClipboard"
    / "Assets"
)
SIZES = {(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 256)}


class IconTests(unittest.TestCase):
    def test_all_icons_have_all_windows_sizes(self):
        for name in ["app"] + [
            f"{state}-{theme}"
            for state in ("active", "paused", "error")
            for theme in ("dark", "light")
        ]:
            with self.subTest(icon=name), Image.open(ASSETS / f"{name}.ico") as icon:
                self.assertEqual(SIZES, icon.ico.sizes())

    def test_every_frame_is_visible_and_has_transparent_corners(self):
        for file in ASSETS.glob("*.ico"):
            with Image.open(file) as icon:
                for size in SIZES:
                    with self.subTest(icon=file.name, size=size):
                        frame = icon.ico.getimage(size).convert("RGBA")
                        self.assertIsNotNone(frame.getbbox())
                        for point in (
                            (0, 0),
                            (0, size[1] - 1),
                            (size[0] - 1, 0),
                            (size[0] - 1, size[1] - 1),
                        ):
                            self.assertEqual(0, frame.getpixel(point)[3])

    def test_states_remain_distinct_at_sixteen_pixels(self):
        for theme in ("dark", "light"):
            frames = []
            for state in ("active", "paused", "error"):
                with Image.open(ASSETS / f"{state}-{theme}.ico") as icon:
                    frames.append(icon.ico.getimage((16, 16)).tobytes())
            self.assertEqual(3, len(set(frames)))

    def test_normal_icon_has_contrast_for_each_taskbar_theme(self):
        for theme in ("dark", "light"):
            with Image.open(ASSETS / f"active-{theme}.ico") as icon:
                frame = icon.ico.getimage((16, 16)).convert("RGBA")
                pixels = [frame.getpixel((x, y)) for y in range(16) for x in range(16)]
                opaque = [sum(pixel[:3]) / 3 for pixel in pixels if pixel[3] > 240]
                self.assertGreater(len(opaque), 10)
                luminance = sum(opaque) / len(opaque)
                if theme == "dark":
                    self.assertGreater(luminance, 200)
                else:
                    self.assertLess(luminance, 80)


if __name__ == "__main__":
    unittest.main(verbosity=2)
