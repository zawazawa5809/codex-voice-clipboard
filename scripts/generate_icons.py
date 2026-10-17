"""Render the approved A concept from editable geometry; no image service at runtime.

Development only: Python 3 + Pillow. Checked-in ICO files are embedded in the EXE.
Each Windows size is rendered independently before supersampling, rather than
letting Windows scale a single large PNG down to sixteen pixels.
"""

import argparse
import io
import pathlib
import struct
from PIL import Image, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parents[1]
SIZES = (16, 20, 24, 32, 40, 48, 64, 256)
PALE = "#f4faf8"
INK = "#253136"


def render(size, state="active", theme="dark", app=False):
    scale = 8
    canvas = Image.new("RGBA", (size * scale, size * scale))
    draw = ImageDraw.Draw(canvas)
    unit = size * scale / 24
    inset = 0
    if app:
        draw.rounded_rectangle(
            (unit, unit, 23 * unit, 23 * unit), radius=5.5 * unit, fill="#123a3c"
        )
        unit *= 0.78
        inset = size * scale * 0.11

    def box(coords):
        return tuple(inset + value * unit for value in coords)

    def rounded(coords, radius, fill):
        draw.rounded_rectangle(box(coords), radius=radius * unit, fill=fill)

    def line(points, color, width):
        values = [(inset + x * unit, inset + y * unit) for x, y in points]
        stroke = max(1, round(width * unit))
        draw.line(values, fill=color, width=stroke, joint="curve")
        for x, y in (values[0], values[-1]):
            r = stroke / 2
            draw.ellipse((x - r, y - r, x + r, y + r), fill=color)

    color = PALE if theme == "dark" or app else INK
    accent = "#70e2c6" if app else color
    if state == "paused":
        color = "#ffc765" if theme == "dark" else "#936000"
        accent = color
    elif state == "error":
        color = "#ff8989" if theme == "dark" else "#be303b"
        accent = color

    # Clipboard body, with a broad gap under its clip. Every coordinate is in
    # an editable 24-unit grid and the interior stays genuinely transparent.
    draw.rounded_rectangle(
        box((3.5, 4.4, 20.5, 22.4)),
        radius=2.5 * unit,
        outline=color,
        width=round(2.1 * unit),
    )
    draw.rectangle(box((7.2, 3.2, 16.8, 6.2)), fill="#123a3c" if app else (0, 0, 0, 0))
    rounded((7.8, 3.1, 16.2, 6.6), 1.0, color)
    draw.ellipse(box((10, 1.4, 14, 5.1)), fill=color)

    # The microphone cradle and stem are invariant across all three states.
    draw.arc(
        box((7.8, 9.5, 16.2, 17.8)),
        start=0,
        end=180,
        fill=color,
        width=round(1.7 * unit),
    )
    line(((7.8, 13.6), (7.8, 12.9)), color, 1.6)
    line(((16.2, 13.6), (16.2, 12.9)), color, 1.6)
    line(((12, 17.3), (12, 19.9)), color, 1.6)
    line(((9.8, 19.9), (14.2, 19.9)), color, 1.6)
    if state == "active":
        rounded((9.8, 8.6, 14.2, 15.0), 2.1, accent)
    elif state == "paused":
        rounded((9.7, 8.6, 11.4, 15.0), 0.65, accent)
        rounded((12.6, 8.6, 14.3, 15.0), 0.65, accent)
    else:
        rounded((11.0, 8.4, 13.0, 12.5), 0.7, accent)
        draw.ellipse(box((10.9, 13.6, 13.1, 15.8)), fill=accent)
    result = canvas.resize((size, size), Image.Resampling.LANCZOS)
    # Remove almost-invisible resampling halos so transparent edges stay clean.
    result.putalpha(
        result.getchannel("A").point(lambda alpha: 0 if alpha < 4 else alpha)
    )
    return result


def write_ico(path, state="active", theme="dark", app=False):
    frames = []
    for size in SIZES:
        buffer = io.BytesIO()
        render(size, state, theme, app).save(buffer, format="PNG")
        frames.append((size, buffer.getvalue()))
    offset = 6 + 16 * len(frames)
    entries = []
    for size, data in frames:
        entries.append(
            struct.pack(
                "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset
            )
        )
        offset += len(data)
    path.write_bytes(
        struct.pack("<HHH", 0, 1, len(frames))
        + b"".join(entries)
        + b"".join(data for _, data in frames)
    )


def preview(path):
    sheet = Image.new("RGB", (1080, 680), "#f2f4f3")
    draw = ImageDraw.Draw(sheet)
    font_path = pathlib.Path("C:/Windows/Fonts/segoeui.ttf")
    title = ImageFont.truetype(str(font_path), 30)
    normal = ImageFont.truetype(str(font_path), 20)
    small = ImageFont.truetype(str(font_path), 15)
    draw.text((38, 25), "A  /  CLIPBOARD + MICROPHONE", font=title, fill=INK)
    draw.text(
        (38, 71),
        "Production icon preview   |   actual pixels below, enlarged views above",
        font=normal,
        fill="#596569",
    )
    for col, (label, state) in enumerate(
        (("WATCHING", "active"), ("PAUSED", "paused"), ("ERROR", "error"))
    ):
        x = 38 + col * 272
        draw.text((x, 126), label, font=normal, fill=INK)
        for row, theme in enumerate(("dark", "light")):
            y = 164 + row * 212
            draw.rounded_rectangle(
                (x, y, x + 238, y + 190),
                radius=15,
                fill="#25292e" if theme == "dark" else "#ffffff",
            )
            image = render(104, state, theme)
            sheet.paste(image, (x + 67, y + 12), image)
            for index, size in enumerate((16, 20, 24, 32)):
                image = render(size, state, theme)
                px = x + 30 + index * 49
                sheet.paste(
                    image, (px + (32 - size) // 2, y + 128 + (32 - size) // 2), image
                )
                draw.text(
                    (px + 6, y + 166),
                    str(size),
                    font=small,
                    fill="#acb5ba" if theme == "dark" else "#737d82",
                )
    draw.text((871, 126), "APP / EXE", font=normal, fill=INK)
    image = render(144, app=True)
    sheet.paste(image, (866, 174), image)
    draw.text((867, 336), "16 / 20 / 24 / 32", font=small, fill="#737d82")
    draw.text((867, 360), "40 / 48 / 64 / 256", font=small, fill="#737d82")
    draw.text(
        (38, 620),
        "Transparent tray marks. Theme variants. State changes in both color and microphone shape.",
        font=normal,
        fill="#596569",
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(path)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--preview",
        type=pathlib.Path,
        default=ROOT / "artifacts" / "icon-preview-a.png",
    )
    args = parser.parse_args()
    assets = ROOT / "src" / "CodexVoiceClipboard" / "Assets"
    assets.mkdir(parents=True, exist_ok=True)
    write_ico(assets / "app.ico", app=True)
    for state in ("active", "paused", "error"):
        for theme in ("dark", "light"):
            write_ico(assets / f"{state}-{theme}.ico", state, theme)
    preview(args.preview)
    print(f"Generated 7 multi-resolution ICO assets and {args.preview}")
