"""Compare a bare wallpaper region, excluding the unchanged user windows and live widgets."""

import argparse
import json
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageStat


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("captures", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    # Physical DISPLAY2 pixels: the gap between the left calendar and weather cards, above
    # the user's open Studio window. This is wallpaper throughout both proof runs.
    region = (460, 70, 940, 350)
    names = [
        name for name in ["hold-start", "before-release", "released", "resumed"]
        if (args.captures / f"{name}.png").exists()
    ]
    crops = {}
    sheet = Image.new("RGB", (480 * len(names), 310), "#121519")
    draw = ImageDraw.Draw(sheet)
    for index, name in enumerate(names):
        path = args.captures / f"{name}.png"
        with Image.open(path) as image:
            crop = image.convert("RGB").crop(region)
        crops[name] = crop
        sheet.paste(crop, (480 * index, 30))
        draw.text((480 * index + 8, 8), name, fill="white")
    comparisons = {}
    for a, b in [("hold-start", "before-release"), ("hold-start", "released"), ("released", "resumed")]:
        if a not in crops or b not in crops:
            continue
        diff = ImageChops.difference(crops[a], crops[b])
        changed = sum(pixel != (0, 0, 0) for pixel in diff.get_flattened_data())
        comparisons[f"{a} -> {b}"] = {
            "changed_pixels": changed,
            "total_pixels": diff.width * diff.height,
            "mean_absolute_channel_difference": sum(ImageStat.Stat(diff).mean) / 3,
            "max_channel_difference": max(high for low, high in diff.getextrema()),
        }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(args.output.with_suffix(".png"))
    result = {"captures": str(args.captures), "crop_physical_pixels": region, "comparisons": comparisons}
    args.output.with_suffix(".json").write_text(json.dumps(result, indent=2) + "\n")
    resume_paths = sorted(args.captures.glob("resume-*.png"))
    if resume_paths:
        resumes = Image.new("RGB", (1920, 390 * ((len(resume_paths) + 2) // 3)), "#121519")
        labels = ImageDraw.Draw(resumes)
        for index, path in enumerate(resume_paths):
            x, y = index % 3 * 640, index // 3 * 390
            with Image.open(path) as image:
                resumes.paste(image.convert("RGB").resize((640, 360)), (x, y + 30))
            labels.text((x + 8, y + 8), path.stem, fill="white")
        resumes.save(args.output.with_name(args.output.stem + "-resumes.png"))
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
