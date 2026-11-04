"""Measure decoded video boundaries, motion and internal cuts; never edits media.

Requires Python, numpy, scikit-image, FFmpeg and ffprobe. Example:
  py -3 themes/verify-loops.py --ffmpeg C:/tools/ffmpeg.exe --ffprobe C:/tools/ffprobe.exe --output C:/temp/loops.json
Scores use 640x360 boundary frames and 160x90 temporal/flow analysis. A high
SSIM alone cannot prove motion continuity; inspect the registration/flow data
and the largest internal step as well. Procedural themes have no file restart.
"""

import argparse
import json
import pathlib
import subprocess

import numpy as np
from skimage.metrics import peak_signal_noise_ratio, structural_similarity
from skimage.registration import optical_flow_ilk, phase_cross_correlation


def decode(ffmpeg, path, filters):
    result = subprocess.run(
        [ffmpeg, "-v", "error", "-threads", "2", "-i", str(path), "-an",
         "-vf", filters, "-fps_mode", "passthrough", "-pix_fmt", "rgb24", "-f", "rawvideo", "-"],
        capture_output=True, check=True,
    )
    return result.stdout


def measure(ffmpeg, ffprobe, path):
    metadata = json.loads(subprocess.check_output(
        [ffprobe, "-v", "error", "-show_streams", "-show_format", "-of", "json", str(path)], text=True,
    ))
    stream = next(s for s in metadata["streams"] if s["codec_type"] == "video")
    frames = np.frombuffer(decode(ffmpeg, path, "scale=160:90"), np.uint8).reshape(-1, 90, 160, 3)
    pixels = frames.astype(np.float32)
    differences = np.mean(np.abs(np.diff(pixels, axis=0)), axis=(1, 2, 3))
    boundary = float(np.abs(pixels[-1] - pixels[0]).mean())
    adjacent = float(np.mean(np.r_[differences[-3:], differences[:3]]))
    # Full-stream decode finds errors and internal cuts; this second decode
    # measures the actual first/last frames at a useful inspection resolution.
    selected = f"select='lt(n,2)+gte(n,{len(frames)-2})',scale=640:360"
    large = np.frombuffer(decode(ffmpeg, path, selected), np.uint8).reshape(4, 360, 640, 3)
    pairs = [(pixels[-2], pixels[-1]), (pixels[-1], pixels[0]), (pixels[0], pixels[1])]
    motions = []
    for before, after in pairs:
        a, b = before.mean(2) / 255, after.mean(2) / 255
        flow = optical_flow_ilk(a, b, radius=5, num_warp=4)
        registration, _, _ = phase_cross_correlation(a, b, upsample_factor=10)
        motions.append({
            "mean_flow_yx": [float(v) for v in flow.mean(axis=(1, 2))],
            "mean_flow_speed": float(np.linalg.norm(flow, axis=0).mean()),
            "registration_yx": [float(v) for v in registration],
        })
    expected = (np.array(motions[0]["registration_yx"]) + np.array(motions[2]["registration_yx"])) / 2
    registration_error = float(np.linalg.norm(np.array(motions[1]["registration_yx"]) - expected))
    # Also inspect the overlap's motion; a good file boundary can hide a cut
    # or a camera step elsewhere inside the fade.
    manifest = json.loads((path.parent / "theme.json").read_text(encoding="utf-8"))
    catalog = json.loads((path.parent.parent / "catalog.json").read_text(encoding="utf-8"))
    recipe = next(t["loop"] for t in catalog["themes"] if t["id"] == path.parent.name)
    numerator, denominator = map(int, stream["avg_frame_rate"].split("/"))
    rate = numerator / denominator
    overlap_start = recipe["source_duration_seconds"] - 2 * recipe["crossfade_seconds"]
    overlap_motion = []
    for fraction in [0, 0.25, 0.5, 0.75, 1]:
        frame = min(len(frames) - 2, max(0, round((overlap_start + fraction * recipe["crossfade_seconds"]) * rate)))
        flow = optical_flow_ilk(pixels[frame].mean(2) / 255, pixels[frame+1].mean(2) / 255, radius=5, num_warp=4)
        overlap_motion.append({"frame": frame, "mean_flow_yx": [float(v) for v in flow.mean(axis=(1, 2))],
                               "mean_flow_speed": float(np.linalg.norm(flow, axis=0).mean())})
    fps = stream["avg_frame_rate"]
    return {
        "id": path.parent.name, "frames": len(frames), "duration": float(metadata["format"]["duration"]),
        "width": stream["width"], "height": stream["height"], "fps": fps,
        "codec": stream["codec_name"], "pixel_format": stream["pix_fmt"],
        "audio_tracks": sum(s["codec_type"] == "audio" for s in metadata["streams"]),
        "boundary_ssim_640x360": float(structural_similarity(large[-1], large[0], channel_axis=2, data_range=255)),
        "boundary_psnr_db_640x360": float(peak_signal_noise_ratio(large[-1], large[0], data_range=255)),
        "adjacent_ssim_before_640x360": float(structural_similarity(large[-2], large[-1], channel_axis=2, data_range=255)),
        "adjacent_ssim_after_640x360": float(structural_similarity(large[0], large[1], channel_axis=2, data_range=255)),
        "boundary_MAD_255_160x90": boundary, "adjacent_MAD_255_160x90": adjacent,
        "boundary_to_adjacent_MAD_ratio": boundary / max(adjacent, 1e-8),
        "largest_internal_MAD_255_160x90": float(differences.max()),
        "median_internal_MAD_255_160x90": float(np.median(differences)),
        "largest_internal_step_frame": int(differences.argmax()),
        "motion_before_at_after_boundary_160x90": motions,
        "boundary_registration_error_px_160x90": registration_error,
        "overlap_motion_samples_160x90": overlap_motion,
        "configured_fps": manifest["wallpaper"]["fps"],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--ffprobe", default="ffprobe")
    parser.add_argument("--output", type=pathlib.Path)
    args = parser.parse_args()
    root = pathlib.Path(__file__).resolve().parent
    results = [measure(args.ffmpeg, args.ffprobe, path) for path in sorted(root.glob("*/loop.mp4"))]
    text = json.dumps({"analysis": "Decoded RGB; motion/registration in pixels per frame at 160x90.", "videos": results}, indent=2) + "\n"
    if args.output:
        args.output.write_text(text, encoding="utf-8")
    print(text)


if __name__ == "__main__":
    main()
