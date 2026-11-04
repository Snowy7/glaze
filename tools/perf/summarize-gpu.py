"""Summarize engine JSONL timestamps without substituting CPU time for GPU time.
Usage: py -3 tools/perf/summarize-gpu.py timing.run0.jsonl [more.jsonl ...]
Skip warmup with --warmup-seconds (default 6). Outputs per-screen median/p95/max,
combined plates per engine frame, and geometry-only publish-to-submit delivery.
"""
import argparse
import json
import statistics
from collections import defaultdict
from pathlib import Path


def stats(values):
    values = sorted(values)
    if not values:
        return None
    return {"n": len(values), "median": statistics.median(values),
            "p95": values[min(len(values)-1, int(len(values)*0.95))], "max": max(values)}


def summarize(paths, warmup, window=None):
    windows = json.loads(window.read_text(encoding="utf8")) if window else []
    rows = []
    for path in paths:
        run = [json.loads(line) for line in path.read_text(encoding="utf8").splitlines() if line.strip()]
        if not run:
            continue
        frequency = run[0]["qpc_frequency"]
        since = run[0]["submit_qpc"] + warmup*frequency
        sample = next((s for s in windows if s.get("timing_file") and Path(s["timing_file"]).resolve()==path.resolve()),None)
        lower = sample["sample_started_qpc"] if sample else since
        upper = sample["sample_ended_qpc"] if sample else float("inf")
        rows.extend(dict(row, run=str(path)) for row in run if lower<=row["submit_qpc"]<=upper)
    result = {"frames": len(rows), "screens": {}}
    for screen in sorted({r["screen"] for r in rows}):
        group = [r for r in rows if r["screen"] == screen]
        result["screens"][screen] = {key: stats([r[key] for r in group if key != "wallpaper_ms" or r["wallpaper_drawn"]])
                                      for key in ["wallpaper_ms", "restore_ms", "glass_ms"]}
        result["screens"][screen]["frame_ms"] = stats([
            r["wallpaper_ms"]+r["restore_ms"]+r["glass_ms"] for r in group])
        motion = [r for r in group if not r["wallpaper_drawn"]]
        result["screens"][screen]["glass_only_frames"] = len(motion)
        result["screens"][screen]["glass_only_publish_to_submit_ms"] = stats([r["publish_to_submit_ms"] for r in motion])
        result["screens"][screen]["glass_only_publish_to_composition_ms"] = stats([r["publish_to_composition_ms"] for r in motion if r.get("publish_to_composition_ms") is not None])
    frames = defaultdict(list)
    for row in rows:
        frames[(row["run"],row["frame"])].append(row)
    screens = {r["screen"] for r in rows}
    complete = [group for group in frames.values() if {r["screen"] for r in group} == screens]
    result["all_screens_glass_ms"] = stats([sum(r["glass_ms"] for r in group) for group in complete])
    result["incomplete_frames"] = len(frames)-len(complete)
    result["wallpaper_submissions"] = sum(r["wallpaper_drawn"] for r in rows)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", type=Path, nargs="+")
    parser.add_argument("--warmup-seconds", type=float, default=6)
    parser.add_argument("--window",type=Path,help="native sample JSON; excludes startup and teardown exactly")
    args = parser.parse_args()
    print(json.dumps(summarize(args.paths,args.warmup_seconds,args.window),indent=2))
