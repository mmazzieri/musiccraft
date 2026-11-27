#!/usr/bin/env python3
"""Writes every size of the MusicCraft icon that the packages need.

The artwork is `assets/app-icon/musiccraft-source.png` (1024 x 1024, a rounded tile with transparent
corners): an engraved nightingale singing, on a blue tile, made for MusicCraft by Mauro Mazzieri with Google Gemini and released under
MIT OR Apache-2.0 like the code (see ATTRIBUTION.md). This script only scales it. Outputs, committed:

  assets/app-icon/hicolor/<n>x<n>/apps/it.mauromazzieri.musiccraft.png
  assets/app-icon/musiccraft-1024.png        source of the macOS .icns (Apple's grid: a transparent margin)
  assets/app-icon/musiccraft.ico             Windows (16-256)
  assets/app-icon/window-64.rgba        raw RGBA for the window icon (include_bytes!)
  mobile/android/app/src/main/res/mipmap-*/ic_launcher.png   Android launcher
  mobile/ios/MusicCraft/Assets.xcassets/AppIcon.appiconset/icon-1024.png   iOS (square, opaque:
                                        the system rounds the corners)

Run from the repository root: python3 tools/icons.py
"""
from collections import Counter
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "app-icon"
APP_ID = "it.mauromazzieri.musiccraft"
SOURCE = OUT / "musiccraft-source.png"
S = 1024


def draw(size_px: int, margin: float = 0.0) -> Image.Image:
    """The icon at `size_px`; `margin` is a transparent border as a fraction of the side."""
    src = Image.open(SOURCE).convert("RGBA")
    inner = max(1, round(size_px * (1 - 2 * margin)))
    tile = src.resize((inner, inner), Image.LANCZOS)
    img = Image.new("RGBA", (size_px, size_px), (0, 0, 0, 0))
    off = (size_px - inner) // 2
    img.paste(tile, (off, off))
    return img


def tile_colour(img: Image.Image) -> tuple:
    """The tile's background: the commonest opaque colour along its edges."""
    px = img.load()
    seen = Counter()
    for i in range(0, S, 4):
        for x, y in ((i, 12), (i, S - 13), (12, i), (S - 13, i)):
            r, g, b, a = px[x, y]
            if a == 255:
                seen[(r // 4 * 4, g // 4 * 4, b // 4 * 4)] += 1
    return seen.most_common(1)[0][0] if seen else (0, 0, 0)


def mobile() -> None:
    res = ROOT / "mobile" / "android" / "app" / "src" / "main" / "res"
    for name, n in (("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)):
        d = res / f"mipmap-{name}"
        d.mkdir(parents=True, exist_ok=True)
        draw(n, margin=0.04).save(d / "ic_launcher.png", optimize=True)
    ios = ROOT / "mobile" / "ios" / "MusicCraft" / "Assets.xcassets" / "AppIcon.appiconset"
    ios.mkdir(parents=True, exist_ok=True)
    icon = draw(S)
    square = Image.new("RGB", (S, S), tile_colour(icon))
    # Only the tile's inside: its rounded rim is left out, the corners take the tile's colour.
    d = next((i for i in range(S // 2) if icon.getpixel((i, i))[3] == 255), 0)
    radius = round(d / 0.2929)
    inset = 8
    keep = Image.new("L", (S, S), 0)
    ImageDraw.Draw(keep).rounded_rectangle([inset, inset, S - 1 - inset, S - 1 - inset], radius=max(0, radius - inset), fill=255)
    square.paste(icon, mask=ImageChops.multiply(icon.getchannel("A"), keep))
    square.save(ios / "icon-1024.png", optimize=True)


def main() -> None:
    for n in (16, 24, 32, 48, 64, 128, 256, 512):
        d = OUT / "hicolor" / f"{n}x{n}" / "apps"
        d.mkdir(parents=True, exist_ok=True)
        draw(n).save(d / f"{APP_ID}.png", optimize=True)
    draw(S, margin=100 / 1024).save(OUT / "musiccraft-1024.png", optimize=True)
    draw(256).save(OUT / "musiccraft.ico", sizes=[(n, n) for n in (16, 20, 24, 32, 40, 48, 64, 128, 256)])
    (OUT / "window-64.rgba").write_bytes(draw(64).tobytes())
    mobile()


if __name__ == "__main__":
    main()
