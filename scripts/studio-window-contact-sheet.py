"""Make an uncropped contact sheet of Studio renders and isolated live captures."""

import argparse
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("shots", nargs="?", type=Path, default=Path(r"C:\mypaper-targets\studio-window\shots"))
    args = parser.parse_args()
    baseline = Path(__file__).resolve().parents[1] / "crates/studio/tests/golden/window"
    files = [args.shots / path.name for path in sorted(baseline.glob("*.png"))]
    files.extend(path for name in ("live-home.png", "live-look.png") if (path := args.shots / name).is_file())
    columns, cell_width, cell_height = 4, 480, 328
    sheet = Image.new("RGB", (columns * cell_width, ((len(files) + columns - 1) // columns) * cell_height), "#202020")
    draw = ImageDraw.Draw(sheet)
    font = ImageFont.truetype("C:/Windows/Fonts/segoeui.ttf", 16)
    for index, path in enumerate(files):
        x, y = index % columns * cell_width, index // columns * cell_height
        with Image.open(path) as source:
            picture = source.convert("RGB")
            picture.thumbnail((464, 292), Image.Resampling.LANCZOS)
            sheet.paste(picture, (x + (cell_width - picture.width) // 2, y + 28))
        draw.text((x + 8, y + 5), path.stem, font=font, fill="#eeeeee")
    destination = args.shots / "contact-sheet.png"
    sheet.save(destination)
    print(f"{destination}: {len(files)} renders")


if __name__ == "__main__":
    main()
