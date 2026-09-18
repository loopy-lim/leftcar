#!/usr/bin/env python3
"""Realtime stream statistics console (Parsec-style), driven by the host
control plane.

Polls `getStatus` once per second and prints one live line per session:

    time  session  WxH@fps  mode  kbps  cap  enc  rend  gaps  recov  idrRes  wire  age

`rend` is the receiver-side rendered FPS reported through feedback — the same
number the adaptive controller sees. `idrRes` counts paired-IDR split recovery
resumes; `wire`/`age` are the split clock-corrected send/capture-to-decoder
ages in ms and `inRtt` the viewer-measured input send->ack RTT (all None until
they converge). Transitions (resolution or encoder mode changes) are detected and summarized with their recovery
duration: time from the accepted change until rendered FPS is back at/above
half the target.

Also appends every sample to a JSONL log so a run can be summarized offline.

Usage:
  python3 tools/stream-stats.py [--host 127.0.0.1] [--port 7777]
      [--interval 1.0] [--log PATH] [--duration SEC]
"""

import argparse
import json
import math
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

PAIRING_PATH = Path.home() / "Library/Application Support/leftcar-host/paired_devices.json"
KEYCHAIN_SERVICE = "leftcar-host"


def load_token(device=None):
    """Paired-device control token. Since 2026-09-10 tokens live in the macOS
    login Keychain (service "leftcar-host", account = the device's
    credential_id) and paired_devices.json holds metadata only. The first CLI
    read may pop a one-time Keychain consent dialog — allow it once."""
    try:
        devices = json.loads(PAIRING_PATH.read_text())
    except (OSError, ValueError):
        return None
    for entry in devices:
        if not isinstance(entry, dict):
            continue
        name = str(entry.get("name", ""))
        device_id = str(entry.get("device_id", ""))
        if device and device.lower() not in name.lower() and device != device_id:
            continue
        account = entry.get("credential_id") or device_id
        try:
            found = subprocess.run(
                ["security", "find-generic-password", "-s", KEYCHAIN_SERVICE, "-a", account, "-w"],
                capture_output=True,
                text=True,
                timeout=30,
            )
        except (OSError, subprocess.TimeoutExpired):
            continue
        if found.returncode == 0 and found.stdout.strip():
            return found.stdout.strip()
    return None


def get_status(host, port, token, timeout=3.0):
    with socket.create_connection((host, port), timeout=timeout) as connection:
        connection.sendall(
            (json.dumps({"command": "getStatus", "args": {}, "token": token}) + "\n").encode()
        )
        response = json.loads(connection.makefile().readline())
    if response.get("ok") is not True:
        raise RuntimeError(str(response.get("error", "unknown response")))
    return response["result"]


class TransitionTracker:
    """Tracks one session's resolution/mode transitions and recovery time."""

    def __init__(self, session):
        self.session = session
        self.key = None
        self.pending = None  # (key, changed_at, target_fps)

    def observe(self, session_view, now):
        key = (session_view.get("width"), session_view.get("height"), session_view.get("encoderExperimentApplied"))
        fps_target = session_view.get("fpsTarget") or session_view.get("fps") or 60
        rendered = session_view.get("renderedFps")
        events = []
        if self.key is not None and key != self.key:
            self.pending = (key, now, fps_target)
            events.append(
                f"transition session={self.session} -> {key[0]}x{key[1]} mode={key[2]}"
            )
        self.key = key
        if self.pending is not None and rendered is not None and rendered >= max(1, fps_target // 2):
            _, changed_at, _ = self.pending
            self.pending = None
            events.append(
                f"recovered session={self.session} renderedFps={rendered} "
                f"in {now - changed_at:.1f}s"
            )
        return events


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=7777)
    parser.add_argument("--interval", type=float, default=1.0)
    parser.add_argument("--log")
    parser.add_argument("--duration", type=float)
    parser.add_argument("--token")
    parser.add_argument(
        "--device",
        help="paired device name/id whose token to use when several devices are paired",
    )
    args = parser.parse_args()

    token = args.token or load_token(args.device)
    if not token:
        raise SystemExit("no pairing token: pass --token or pair the host first")

    log = open(args.log, "a") if args.log else None
    trackers = {}
    if not math.isfinite(args.interval) or args.interval <= 0 or args.interval > 60 or (args.duration is not None and (not math.isfinite(args.duration) or args.duration <= 0)):
        parser.error("interval must be 0..60 seconds and duration must be positive")
    deadline = time.monotonic() + args.duration if args.duration else None
    stopped = False

    def stop(_sig, _frame):
        nonlocal stopped
        stopped = True

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)

    print(f"polling {args.host}:{args.port} every {args.interval}s — Ctrl-C to stop")
    while not stopped and (deadline is None or time.monotonic() < deadline):
        try:
            status = get_status(args.host, args.port, token)
        except (OSError, RuntimeError, ValueError) as error:
            if log:
                # Preserve failed acquisition attempts so a quiet tail cannot
                # masquerade as a shorter successful run. No credential data.
                log.write(json.dumps({"time": time.time(), "monotonicTime": time.monotonic(),
                                      "pollError": type(error).__name__}) + "\n")
                log.flush()
            print(f"{time.strftime('%H:%M:%S')} host unreachable: {error}")
            time.sleep(args.interval)
            continue
        now = time.time()
        if log:
            log.write(json.dumps({"time": now, "monotonicTime": time.monotonic(), **status}, ensure_ascii=False) + "\n")
            log.flush()
        line = time.strftime("%H:%M:%S")
        for view in status.get("sessions", []):
            sid = view.get("session")
            tracker = trackers.setdefault(sid, TransitionTracker(sid))
            for event in tracker.observe(view, now):
                print(f"  ** {event}")
            rendered = view.get("renderedFps")
            split_wire = view.get("receiverSplitWireMs")
            capture_age = view.get("receiverSplitCaptureAgeMs")
            input_rtt = view.get("receiverInputRttMs")
            line += (
                f" | s{sid} {view.get('width')}x{view.get('height')}@{view.get('fps')}"
                f" {view.get('encoderExperimentApplied') or '-'}"
                f" {view.get('kbps', 0) // 1000}Mbps"
                f" cap={view.get('captureFps', 0)} enc={view.get('encodeOutputFps', 0)}"
                f" rend={rendered if rendered is not None else '-'}"
                f" gaps={view.get('receiverFrameGaps', 0)}"
                f" recov={view.get('recoveryKeyframes', 0)}"
                f" idrRes={view.get('receiverPairedIdrResumes', 0)}"
                f" auP95={view.get('auBytesP95', 0) // 1024}K"
                f" idrP95={view.get('idrBytesP95', 0) // 1024}K"
                f" nack={view.get('nacksServed', 0)}/{view.get('nacksMissed', 0)}"
                f" drlW={view.get('dataRateLimitWindowMs', 0)}ms"
                f" wire={split_wire if split_wire is not None else '-'}"
                f" age={capture_age if capture_age is not None else '-'}"
                f" inRtt={input_rtt if input_rtt is not None else '-'}"
                f" {view.get('state', '')}"
            )
        print(line)
        time.sleep(args.interval)
    if log:
        log.close()


if __name__ == "__main__":
    sys.exit(main())
