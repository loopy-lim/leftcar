#!/usr/bin/env python3
"""Realtime stream statistics console (Parsec-style), driven by the host
control plane.

Polls `getStatus` once per second and prints one live line per session:

    time  session  WxH@fps  mode  kbps  cap  enc  rend  drop  recvGaps  recov

`rend` is the receiver-side rendered FPS reported through feedback — the same
number the adaptive controller sees. Transitions (resolution or encoder mode
changes) are detected and summarized with their recovery duration: time from
the accepted change until rendered FPS is back at/above half the target.

Also appends every sample to a JSONL log so a run can be summarized offline.

Usage:
  python3 tools/stream-stats.py [--host 127.0.0.1] [--port 7777]
      [--interval 1.0] [--log PATH] [--duration SEC]
"""

import argparse
import json
import signal
import socket
import sys
import time
from pathlib import Path

PAIRING_PATH = Path.home() / "Library/Application Support/leftcar-host/paired_devices.json"


def load_token():
    try:
        return json.loads(PAIRING_PATH.read_text())[0]["token_hex"]
    except (OSError, IndexError, KeyError, ValueError):
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
    args = parser.parse_args()

    token = args.token or load_token()
    if not token:
        raise SystemExit("no pairing token: pass --token or pair the host first")

    log = open(args.log, "a") if args.log else None
    trackers = {}
    deadline = time.time() + args.duration if args.duration else None
    stopped = False

    def stop(_sig, _frame):
        nonlocal stopped
        stopped = True

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)

    print(f"polling {args.host}:{args.port} every {args.interval}s — Ctrl-C to stop")
    while not stopped and (deadline is None or time.time() < deadline):
        try:
            status = get_status(args.host, args.port, token)
        except (OSError, RuntimeError, ValueError) as error:
            print(f"{time.strftime('%H:%M:%S')} host unreachable: {error}")
            time.sleep(args.interval)
            continue
        now = time.time()
        if log:
            log.write(json.dumps({"time": now, **status}, ensure_ascii=False) + "\n")
            log.flush()
        line = time.strftime("%H:%M:%S")
        for view in status.get("sessions", []):
            sid = view.get("session")
            tracker = trackers.setdefault(sid, TransitionTracker(sid))
            for event in tracker.observe(view, now):
                print(f"  ** {event}")
            rendered = view.get("renderedFps")
            line += (
                f" | s{sid} {view.get('width')}x{view.get('height')}@{view.get('fps')}"
                f" {view.get('encoderExperimentApplied') or '-'}"
                f" {view.get('kbps', 0) // 1000}Mbps"
                f" cap={view.get('captureFps', 0)} enc={view.get('encodeOutputFps', 0)}"
                f" rend={rendered if rendered is not None else '-'}"
                f" gaps={view.get('receiverFrameGaps', 0)}"
                f" recov={view.get('recoveryKeyframes', 0)}"
                f" {view.get('state', '')}"
            )
        print(line)
        time.sleep(args.interval)
    if log:
        log.close()


if __name__ == "__main__":
    sys.exit(main())
