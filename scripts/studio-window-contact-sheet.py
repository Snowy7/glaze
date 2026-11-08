"""Make uncropped contact sheets of the current renderer-written Studio fixtures."""

import argparse
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("shots", nargs="?", type=Path, default=Path(r"C:\glaze-targets\studio-window\shots"))
    args = parser.parse_args()
    baseline = Path(__file__).resolve().parents[1] / "crates/studio/tests/golden/window"
    files = [args.shots / path.name for path in sorted(baseline.glob("*.png"))]
    files.extend(path for name in ("fit-stills-dark.png", "fit-stills-light.png") if (path := args.shots / name).is_file())
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
    # Readable pages make it possible to review every fixture without a giant downscale.
    page_height = cell_height * 3
    for index, top in enumerate(range(0, sheet.height, page_height), 1):
        sheet.crop((0, top, sheet.width, min(top + page_height, sheet.height))).save(
            args.shots / f"contact-sheet-{index:02}.png"
        )
    print(f"{destination}: {len(files)} renders")


if __name__ == "__main__":
    main()
