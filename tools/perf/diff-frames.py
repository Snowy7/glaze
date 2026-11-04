"""Compare two frame-lab captures (crates/wallpaper/src/lab.rs) pixel by pixel.
Usage: py -3 tools/perf/diff-frames.py BEFORE_DIR AFTER_DIR [--png OUT_DIR]
For every frame in BEFORE_DIR/frames.json: the largest per-channel difference (of 255),
the share of identical pixels, and the mean absolute difference. --png writes, for frames
that differ, the after frame and a difference image amplified 32x for inspection.
Exits 1 when any channel of any frame differs by more than --max (default 1).
"""
import argparse
import json
import sys
from pathlib import Path

import numpy as np


def load(folder, frame):
    pixels = np.fromfile(folder / frame["file"], np.uint8)
    return pixels.reshape(frame["height"], frame["width"], 4)[..., :3].astype(np.int16)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--png", type=Path)
    parser.add_argument("--max", type=int, default=1)
    args = parser.parse_args()
    worst = 0
    for frame in json.loads((args.before / "frames.json").read_text(encoding="utf8")):
        before, after = load(args.before, frame), load(args.after, frame)
        difference = np.abs(before - after)
        largest = int(difference.max())
        worst = max(worst, largest)
        same = 100.0 * float((difference.max(axis=2) == 0).mean())
        print(f"{frame['file']:>18}: max {largest}/255, {same:.4f}% identical, "
              f"mean {float(difference.mean()):.6f}")
        if args.png and largest:
            from PIL import Image
            args.png.mkdir(parents=True, exist_ok=True)
            stem = Path(frame["file"]).stem
            Image.fromarray(after[..., ::-1].astype(np.uint8)).save(args.png / f"{stem}-after.png")
            amplified = np.clip(difference * 32, 0, 255).astype(np.uint8)[..., ::-1]
            Image.fromarray(amplified).save(args.png / f"{stem}-diff.png")
    print(f"largest difference: {worst}/255")
    sys.exit(1 if worst > args.max else 0)


if __name__ == "__main__":
    main()
