#!/usr/bin/env python3
"""Draws the MusicCraft icon and writes every size the packages need.

The icon is original artwork made for MusicCraft: five rounded bars of a sound wave, the middle
ones rising, on a tile that goes from deep teal to indigo. Drawn with Pillow shapes; no fonts, no
third-party art. Outputs, committed:

  assets/app-icon/musiccraft.svg          reference drawing (same geometry)
  assets/app-icon/hicolor/<n>x<n>/apps/it.mauromazzieri.musiccraft.png
  assets/app-icon/musiccraft-1024.png     source of the macOS .icns
  assets/app-icon/musiccraft.ico          Windows (16-256)
  assets/app-icon/window-64.rgba          raw RGBA for the window icon (include_bytes!)
  mobile/android/app/src/main/res/mipmap-*/ic_launcher.png   Android launcher
  mobile/ios/MusicCraft/Assets.xcassets/AppIcon.appiconset/icon-1024.png   iOS (square, opaque:
                                          the system rounds the corners)

Run from the repository root: python3 tools/icons.py
"""
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "app-icon"
APP_ID = "it.mauromazzieri.musiccraft"
S = 1024
TOP, BOTTOM = (14, 138, 150), (58, 50, 138)
# centre x, half height, colour
BARS = [
    (232, 120, (255, 255, 255)),
    (372, 250, (255, 255, 255)),
    (512, 330, (255, 209, 102)),
    (652, 210, (255, 255, 255)),
    (792, 130, (255, 255, 255)),
]
HALF = 46


def draw(size_px: int, margin: float = 0.0) -> Image.Image:
    """The icon at `size_px`, drawn larger and reduced for smooth edges."""
    big = S * 2
    m = int(big * margin)
    side = big - 2 * m
    u = side / S
    ramp = Image.new("RGBA", (1, S))
    for y in range(S):
        t = y / (S - 1)
        ramp.putpixel((0, y), tuple(int(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)) + (255,))
    tile = ramp.resize((big, big))
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle([m, m, m + side, m + side], radius=int(226 * u), fill=255)
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    img.paste(tile, (0, 0), mask)
    d = ImageDraw.Draw(img)
    for x, h, c in BARS:
        box = [m + (x - HALF) * u, m + (512 - h) * u, m + (x + HALF) * u, m + (512 + h) * u]
        d.rounded_rectangle(box, radius=int(HALF * u), fill=c + (255,))
    img.putalpha(ImageChops.darker(img.getchannel("A"), mask))
    return img.resize((size_px, size_px), Image.LANCZOS)


def square(size_px: int) -> Image.Image:
    """The icon without its rounded corners or transparency, for systems that cut their own."""
    img = Image.new("RGB", (S, S))
    for y in range(S):
        t = y / (S - 1)
        ImageDraw.Draw(img).line([(0, y), (S, y)], fill=tuple(int(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)))
    big = img.resize((S * 2, S * 2))
    d = ImageDraw.Draw(big)
    for x, h, c in BARS:
        d.rounded_rectangle([(x - HALF) * 2, (512 - h) * 2, (x + HALF) * 2, (512 + h) * 2], radius=HALF * 2, fill=c)
    return big.resize((size_px, size_px), Image.LANCZOS)


def mobile() -> None:
    res = ROOT / "mobile" / "android" / "app" / "src" / "main" / "res"
    for name, n in (("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)):
        d = res / f"mipmap-{name}"
        d.mkdir(parents=True, exist_ok=True)
        draw(n, margin=0.04).save(d / "ic_launcher.png")
    ios = ROOT / "mobile" / "ios" / "MusicCraft" / "Assets.xcassets" / "AppIcon.appiconset"
    ios.mkdir(parents=True, exist_ok=True)
    square(1024).save(ios / "icon-1024.png")


def svg() -> str:
    rgb = lambda c: "#%02X%02X%02X" % c
    bars = "\n".join(
        f'  <rect x="{x - HALF}" y="{512 - h}" width="{2 * HALF}" height="{2 * h}" rx="{HALF}" fill="{rgb(c)}"/>'
        for x, h, c in BARS
    )
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {S} {S}">
  <!-- MusicCraft icon: reference drawing of tools/icons.py (MIT OR Apache-2.0). -->
  <defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{rgb(TOP)}"/><stop offset="1" stop-color="{rgb(BOTTOM)}"/></linearGradient></defs>
  <rect width="{S}" height="{S}" rx="226" fill="url(#g)"/>
{bars}
</svg>
"""


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "musiccraft.svg").write_text(svg(), newline="\n")
    for n in (16, 24, 32, 48, 64, 128, 256, 512):
        d = OUT / "hicolor" / f"{n}x{n}" / "apps"
        d.mkdir(parents=True, exist_ok=True)
        draw(n).save(d / f"{APP_ID}.png")
    draw(1024, margin=0.09).save(OUT / "musiccraft-1024.png")
    draw(256).save(OUT / "musiccraft.ico", sizes=[(n, n) for n in (16, 20, 24, 32, 40, 48, 64, 128, 256)])
    (OUT / "window-64.rgba").write_bytes(draw(64).tobytes())
    mobile()


if __name__ == "__main__":
    main()
