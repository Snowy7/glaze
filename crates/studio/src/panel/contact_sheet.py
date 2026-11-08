"""Review fixture PNGs from `cargo test -p glaze-studio panel::tests::render_panel_pages`.

Run with `py -3 crates/studio/src/panel/contact_sheet.py`. No app or settings are opened.
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont
import json
import sys

root = Path(sys.argv[1] if len(sys.argv) > 1 else r"C:\glaze-targets\studio-panel\shots")
files = sorted(p for p in root.glob("*.png") if p.name.endswith(("-dark.png", "-light.png")))
font = ImageFont.truetype("C:/Windows/Fonts/segoeui.ttf", 14)
previews = []
index = []
for path in files:
    source = Image.open(path).convert("RGB")
    # The first viewport of a full render already appears in its companion page PNG.
    start = 700 if "-full-" in path.name else 0
    for top in range(start, source.height, 700):
        panel = Image.new("RGB", (480, 728), "#181818")
        ImageDraw.Draw(panel).text((8, 4), f"{path.name} · {top}px", font=font, fill="white")
        panel.paste(source.crop((0, top, 480, min(top + 700, source.height))), (0, 28))
        previews.append(panel)
        index.append({"file": path.name, "top": top, "sheet": len(previews[:-1]) // 4 + 1})
for start in range(0, len(previews), 4):
    sheet = Image.new("RGB", (960, 1456), "#181818")
    for n, panel in enumerate(previews[start:start + 4]):
        sheet.paste(panel, ((n % 2) * 480, (n // 2) * 728))
    sheet.save(root / f"review-{start // 4 + 1:02}.png")

sheet = Image.new("RGB", (1440, ((len(files) + 5) // 6) * 410), "#181818")
for n, path in enumerate(files):
    panel = Image.open(path).convert("RGB")
    panel.thumbnail((232, 376))
    x, y = (n % 6) * 240, (n // 6) * 410
    ImageDraw.Draw(sheet).text((x + 4, y + 3), path.name, font=font, fill="white")
    sheet.paste(panel, (x + 4, y + 28))
sheet.save(root / "contact-sheet.png")
(root / "review-index.json").write_text(json.dumps(index, indent=2), encoding="utf-8")
print(json.dumps({"renders": len(files), "review_sheets": (len(previews) + 3) // 4}))
