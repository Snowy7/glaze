"""Composite owned-window WGC captures on neutral grey and show real corner pixels."""
from pathlib import Path
import struct
import sys
from PIL import Image, ImageDraw

root = Path(sys.argv[1])
out = Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)

def load(name):
    data = (root / f"{name}.bmp").read_bytes()
    width, height = struct.unpack_from("<ii", data, 18)
    rgba = Image.frombytes("RGBA", (width, abs(height)), data[54:], "raw", "BGRA")
    bg = Image.new("RGBA", rgba.size, (100, 100, 100, 255))
    return Image.alpha_composite(bg, rgba).convert("RGB")

sheet = Image.new("RGB", (960, 420), "#242424")
draw = ImageDraw.Draw(sheet)
for i, name in enumerate(["default", "round", "small", "square"]):
    im = load(name)
    im.save(out / f"{name}.png")
    x = i * 240
    draw.text((x + 12, 14), name.title() + " · 96 DPI", fill="white")
    for j, box in enumerate([(0, 0, 40, 40), (im.width-40, 0, im.width, 40)]):
        sheet.paste(im.crop(box).resize((192, 192), Image.Resampling.NEAREST), (x + 12, 40 + j * 190))
sheet.save(out / "corner-modes.png")

if (root / "files.bmp").exists():
    files, native = load("files"), load("default")
    # A layered app capture includes the invisible native resize frame. Its drawn body
    # starts at the client origin, unlike DWM's cropped opaque-window capture.
    import json
    geometry = json.loads((root / "files-geometry.json").read_text(encoding="utf-8-sig"))
    x, y, w, h = (geometry[key] for key in ["client_x", "client_y", "client_width", "client_height"])
    files = files.crop((x, y, x+w, y+h))
    # Only Glaze's title band and the owned native frame. Window content below it is omitted.
    comparison = Image.new("RGB", (1120, 480), "#242424")
    d = ImageDraw.Draw(comparison)
    for i, (im, label) in enumerate([(files, "Glaze Files · 96 DPI"), (native, "Owned native Windows window · Default · 96 DPI")]):
        x = i * 560
        d.text((x + 12, 14), label, fill="white")
        band = im.crop((0, 0, min(536, im.width), min(46, im.height)))
        comparison.paste(band, (x + 12, 40))
        for j, box in enumerate([(0, 0, 40, 40), (im.width-40, 0, im.width, 40)]):
            comparison.paste(im.crop(box).resize((240, 240), Image.Resampling.NEAREST), (x + 12 + j*266, 126))
        d.text((x + 12, 380), "Top-left / top-right · 6x actual pixels", fill="#bbbbbb")
    comparison.save(out / "files-vs-windows.png")

if (root / "fixture-files-normal.bmp").exists():
    states = Image.new("RGB", (960, 460), "#242424")
    d = ImageDraw.Draw(states)
    for row, app in enumerate(["files", "studio"]):
        for col, state in enumerate(["normal", "edge", "maximized"]):
            im = load(f"fixture-{app}-{state}")
            x = 8 if app == "files" else 0
            y = 8 if app == "files" and state == "maximized" else 0
            d.text((col*320 + 12, row*230 + 10), f"{app.title()} · {state} · 96 DPI", fill="white")
            states.paste(im.crop((x, y, x+40, y+40)).resize((192, 192), Image.Resampling.NEAREST), (col*320+12, row*230+32))
    states.save(out / "app-corner-states.png")
