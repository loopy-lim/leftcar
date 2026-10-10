#!/usr/bin/env python3
"""Leftcar live connection checker (read-only, standard library only).

Checks whether a live Leftcar host <-> viewer Wi-Fi session actually exists
right now, using only observational evidence:

  1. Host process   : leftcar-host-desktop running on this Mac
  2. Host transport : TCP control-plane port (default 7777) reachable
  3. ADB transport  : at least one authorized Android device attached
  4. Fresh session  : sessions.jsonl shows session_started newer than
                      --fresh-window seconds with no later session_stopped

A running process / open port alone is NOT success: the verdict is
"connected" only when a fresh app-level session record exists.

--self-test feeds synthetic states through the same evaluation function
(a disconnected synthetic case must fail, a connected one must pass) and
performs no system inspection at all.
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

HOST_SESSIONS_PATH = os.path.expanduser(
    "~/Library/Application Support/leftcar-host/sessions.jsonl"
)
DEFAULT_PORT = 7777
DEFAULT_FRESH_WINDOW_SEC = 120


# --------------------------------------------------------------------------
# Evidence collection (all read-only)
# --------------------------------------------------------------------------

def run_command(cmd: list[str], timeout: float = 8.0) -> str | None:
    """Run a read-only command and return stdout, or None on failure."""
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=timeout, check=False
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if proc.returncode != 0:
        return None
    return proc.stdout


def check_host_process() -> dict:
    out = run_command(["pgrep", "-f", "leftcar-host-desktop"])
    pids = [line for line in (out or "").split() if line.strip().isdigit()]
    return {"check": "host_process", "ok": bool(pids), "pids": pids}


def check_host_tcp(port: int, hosts: list[str]) -> dict:
    reachable = []
    for host in hosts:
        try:
            with socket.create_connection((host, port), timeout=2.0):
                reachable.append(f"{host}:{port}")
        except OSError:
            continue
    return {"check": "host_tcp", "ok": bool(reachable), "reachable": reachable}


def check_adb_device() -> dict:
    out = run_command(["adb", "devices", "-l"])
    devices = []
    if out:
        for line in out.splitlines():
            parts = line.split()
            # "<serial>\tdevice product:... model:... device:... transport_id:..."
            if len(parts) >= 2 and parts[1] == "device":
                model = next(
                    (p.split(":", 1)[1] for p in parts if p.startswith("model:")),
                    None,
                )
                devices.append({"serial": parts[0], "model": model})
    return {"check": "adb_device", "ok": bool(devices), "devices": devices}


def read_session_records(path: str) -> list[dict]:
    """Parse sessions.jsonl into epoch-timestamped records."""
    records: list[dict] = []
    try:
        text = Path(path).read_text(encoding="utf-8")
    except OSError:
        return records
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rec = json.loads(line)
        except json.JSONDecodeError:
            continue
        ts = rec.get("ts", "")
        if isinstance(ts, str) and ts.startswith("unix:"):
            try:
                rec["_epoch"] = float(ts.split(":", 1)[1])
            except ValueError:
                continue
        else:
            continue
        records.append(rec)
    return records


def latest_session_state(records: list[dict]) -> dict | None:
    """Return the newest session_started record plus any later stop, if any."""
    starts = [r for r in records if r.get("event") == "session_started"]
    if not starts:
        return None
    latest = max(starts, key=lambda r: r["_epoch"])
    later_stops = [
        r
        for r in records
        if r.get("event") == "session_stopped"
        and r.get("session") == latest.get("session")
        and r["_epoch"] > latest["_epoch"]
    ]
    return {"start": latest, "stopped_after": bool(later_stops)}


# --------------------------------------------------------------------------
# Evaluation (pure; exercised by --self-test with synthetic states)
# --------------------------------------------------------------------------

def evaluate(
    checks: dict,
    now: float,
    fresh_window_sec: int = DEFAULT_FRESH_WINDOW_SEC,
) -> dict:
    """Decide the live-session verdict from collected evidence.

    checks keys:
      host_process: bool
      host_tcp: bool
      adb_device: bool
      session: None | {"start": {"_epoch": float, ...}, "stopped_after": bool}
    """
    session = checks.get("session")
    fresh_active = bool(
        session
        and not session.get("stopped_after")
        and (now - session["start"]["_epoch"]) <= fresh_window_sec
    )
    connected = all(
        [checks.get("host_process"), checks.get("host_tcp"), fresh_active]
    )
    verdict = "connected" if connected else "disconnected"
    evidence = {
        "host_process": bool(checks.get("host_process")),
        "host_tcp": bool(checks.get("host_tcp")),
        "adb_device": bool(checks.get("adb_device")),
        "fresh_active_session": fresh_active,
        "session_age_sec": (
            round(now - session["start"]["_epoch"], 1) if session else None
        ),
    }
    return {
        "verdict": verdict,
        "fresh_session_verified": fresh_active,
        "evidence": evidence,
        "ok": verdict == "connected",
    }


def collect_state(
    sessions_path: str, port: int, hosts: list[str], want_adb: bool
) -> dict:
    host_proc = check_host_process()
    host_tcp = check_host_tcp(port, hosts)
    adb = check_adb_device() if want_adb else {"ok": False}
    records = read_session_records(sessions_path)
    session = latest_session_state(records)
    return {
        "host_process": host_proc["ok"],
        "host_tcp": host_tcp["ok"],
        "adb_device": adb["ok"],
        "session": session,
        "details": {
            "host_process": host_proc,
            "host_tcp": host_tcp,
            "adb_device": adb,
            "session_records_total": len(records),
        },
    }


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------

def self_test() -> int:
    now = 1_900_000_000.0
    # Synthetic connected case: fresh session 20s old, nothing stopped after.
    connected_state = {
        "host_process": True,
        "host_tcp": True,
        "adb_device": True,
        "session": {
            "start": {"_epoch": now - 20, "event": "session_started", "session": 11},
            "stopped_after": False,
        },
    }
    connected = evaluate(connected_state, now)
    assert connected["verdict"] == "connected", connected
    assert connected["fresh_session_verified"] is True, connected

    # Synthetic disconnected case: last session stopped (stale) -> must fail.
    disconnected_state = {
        "host_process": True,
        "host_tcp": True,
        "adb_device": True,
        "session": {
            "start": {"_epoch": now - 90_000, "event": "session_started", "session": 10},
            "stopped_after": True,
        },
    }
    disconnected = evaluate(disconnected_state, now)
    assert disconnected["verdict"] == "disconnected", disconnected
    assert disconnected["ok"] is False, disconnected

    # Synthetic edge case: recent start but already stopped -> still fails.
    just_stopped_state = {
        "host_process": True,
        "host_tcp": True,
        "adb_device": True,
        "session": {
            "start": {"_epoch": now - 5, "event": "session_started", "session": 12},
            "stopped_after": True,
        },
    }
    just_stopped = evaluate(just_stopped_state, now)
    assert just_stopped["verdict"] == "disconnected", just_stopped

    print("self-test: 3 synthetic cases OK (connected passes, disconnected fails)")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Read-only check for a live Leftcar host/viewer Wi-Fi session."
    )
    parser.add_argument(
        "--sessions-path",
        default=HOST_SESSIONS_PATH,
        help=f"Path to host sessions.jsonl (default: {HOST_SESSIONS_PATH})",
    )
    parser.add_argument(
        "--port", type=int, default=DEFAULT_PORT,
        help=f"Host control-plane TCP port (default: {DEFAULT_PORT})",
    )
    parser.add_argument(
        "--hosts",
        default="127.0.0.1,192.168.0.133,192.168.0.154",
        help="Comma-separated host addresses to probe (default: local LAN addrs)",
    )
    parser.add_argument(
        "--fresh-window", type=int, default=DEFAULT_FRESH_WINDOW_SEC,
        help="Max age (seconds) of session_started for a fresh session",
    )
    parser.add_argument(
        "--no-adb", action="store_true",
        help="Skip the ADB device check",
    )
    parser.add_argument(
        "--json", action="store_true", help="Print result as JSON"
    )
    parser.add_argument(
        "--self-test", action="store_true",
        help="Run synthetic self-test (no system inspection) and exit",
    )
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    now = time.time()
    state = collect_state(
        args.sessions_path,
        args.port,
        [h.strip() for h in args.hosts.split(",") if h.strip()],
        want_adb=not args.no_adb,
    )
    result = evaluate(state, now, args.fresh_window)
    result["checked_at_epoch"] = now
    result["details"] = state["details"]

    if args.json:
        print(json.dumps(result, indent=2, ensure_ascii=False))
    else:
        print(f"verdict: {result['verdict']}")
        for key, value in result["evidence"].items():
            print(f"  {key}: {value}")
        if result["verdict"] != "connected":
            print("  -> no fresh live session; see evidence above")
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
