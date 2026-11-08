"""Read-only component probe: opens faces in the real helper, never sends vendor actions."""
import argparse
import ctypes
from ctypes import wintypes
import json
import hashlib
from datetime import datetime, timezone
from pathlib import Path
import queue
import shutil
import struct
import subprocess
import tempfile
import threading
import time
import os


def integer(value):
    data = bytearray()
    while value >= 128:
        data.append((value & 127) | 128)
        value >>= 7
    data.append(value)
    return bytes(data)


def string(value):
    data = value.encode("utf-8")
    return integer(len(data)) + data


class Frame:
    def __init__(self, data):
        self.data = data
        self.offset = 0

    def integer(self):
        result, shift = 0, 0
        while True:
            value = self.data[self.offset]
            self.offset += 1
            result |= (value & 127) << shift
            if value < 128:
                return result
            shift += 7

    def blob(self):
        length = self.integer()
        result = self.data[self.offset:self.offset + length]
        self.offset += length
        return result

    def string(self):
        return self.blob().decode("utf-8")


class Memory(ctypes.Structure):
    _fields_ = [("cb", wintypes.DWORD), ("faults", wintypes.DWORD)] + [
        (name, ctypes.c_size_t) for name in
        ("peak_ws", "ws", "peak_paged", "paged", "peak_nonpaged", "nonpaged", "pagefile", "peak_pagefile", "private")
    ]


def memory(process):
    result = Memory()
    result.cb = ctypes.sizeof(result)
    psapi = ctypes.WinDLL("psapi", use_last_error=True)
    psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD]
    if not psapi.GetProcessMemoryInfo(wintypes.HANDLE(int(process._handle)), ctypes.byref(result), result.cb):
        raise ctypes.WinError(ctypes.get_last_error())
    return {"private_mib": result.private / 1048576, "working_set_mib": result.ws / 1048576}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--seconds", type=int, default=20)
    args = parser.parse_args()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    plugin = Path(__file__).resolve().parents[1]
    scratch = Path(tempfile.mkdtemp(prefix="glaze-tailscale-probe-"))
    env = dict(os.environ, APPDATA=str(scratch / "roaming"), LOCALAPPDATA=str(scratch / "local"))
    messages = queue.Queue()
    stderr = (args.out_dir / "helper.log").open("w", encoding="utf-8")
    process = subprocess.Popen([str(args.exe.resolve())], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=stderr, env=env, creationflags=subprocess.CREATE_NO_WINDOW)

    def send(payload):
        process.stdin.write(struct.pack("<I", len(payload)) + payload)
        process.stdin.flush()

    def read():
        try:
            while header := process.stdout.read(4):
                size, = struct.unpack("<I", header)
                data = process.stdout.read(size)
                if len(data) != size:
                    raise RuntimeError("truncated helper frame")
                messages.put(Frame(data))
        except Exception as error:
            messages.put(error)

    thread = threading.Thread(target=read, daemon=True)
    thread.start()
    try:
        time.sleep(0.2)
        baseline = memory(process)
        for face in ("connection", "peers", "traffic"):
            # HostMsg::Open (0), SizeClass::Large (2), postcard length-prefixed strings.
            send(integer(0) + string("tailscale") + string(str(plugin)) + string(face) + string(face) + integer(2) + string("{}"))
        faces = {}
        observed = {}
        stats = []
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            try:
                frame = messages.get(timeout=max(.1, deadline - time.monotonic()))
            except queue.Empty:
                break
            if isinstance(frame, Exception):
                raise frame
            kind = frame.integer()
            if kind == 0:
                instance = frame.string()
                face = json.loads(frame.blob())
                observed[instance] = face
                if face.get("footer") == "Snapshot · refresh for peers and traffic" and face.get("body", {}).get("kind") != "empty":
                    faces[instance] = face
                if len(faces) == 3:
                    break
            elif kind == 1:
                plugin_id, status = frame.string(), frame.integer()
                if status in (1, 2):
                    raise RuntimeError(f"{plugin_id}: {frame.string()}")
        if len(faces) != 3:
            (args.out_dir / "failed-faces.json").write_text(json.dumps(observed, indent=2), encoding="utf-8")
            raise RuntimeError(f"only received {len(faces)} loaded faces: {[f.get('footer') for f in observed.values()]}")
        active = memory(process)
        send(integer(5))  # HostMsg::Stats, not a plugin action.
        idle_started = None
        while len(stats) < 2:
            frame = messages.get(timeout=10)
            if isinstance(frame, Exception):
                raise frame
            if frame.integer() == 3:
                stats.append({"plugin": frame.string(), "linear_memory_bytes": frame.integer(),
                              "cpu_us": frame.integer(), "events": frame.integer()})
                if len(stats) == 1:
                    idle_started = time.monotonic()
                    time.sleep(args.seconds)
                    send(integer(5))
        summary = {"read_only": True, "measured_at_utc": datetime.now(timezone.utc).isoformat(),
                   "helper_sha256": hashlib.sha256(args.exe.read_bytes()).hexdigest(),
                   "component_sha256": hashlib.sha256((plugin / "tailscale.wasm").read_bytes()).hexdigest(),
                   "faces": list(faces), "baseline": baseline, "active": active,
                   "added_private_mib": active["private_mib"] - baseline["private_mib"],
                   "idle_seconds": time.monotonic() - idle_started,
                   "idle_plugin_events": stats[1]["events"] - stats[0]["events"],
                   "idle_plugin_cpu_us": stats[1]["cpu_us"] - stats[0]["cpu_us"], "stats": stats}
        (args.out_dir / "faces.json").write_text(json.dumps(faces, indent=2), encoding="utf-8")
        (args.out_dir / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
        print(json.dumps(summary, indent=2))
    finally:
        try:
            send(integer(6))  # HostMsg::Shutdown.
            process.communicate(timeout=10)
        except Exception:
            process.kill()
            process.wait()
        stderr.close()
        shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    main()
