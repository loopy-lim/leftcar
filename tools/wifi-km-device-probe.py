#!/usr/bin/env python3
"""Read-only connected-device probe for the Leftcar Wi-Fi KM validation.

Collects, without modifying anything:
  - macOS host version and Leftcar host app state
  - USB/ADB device list (read-only `adb devices` only — never connect/tcpip/pair)
  - LAN interfaces and ARP neighbours (direct Wi-Fi addressing evidence)
  - mDNS browse for `_leftcar._tcp` and `_adb-tls-connect._tcp` (discovery only)
  - Leftcar host pairing/session audit files (read-only)
  - macOS TCC permission rows for leftcar/KeyBridge (read-only sqlite query)

Standard library only. All external commands are read-only diagnostics
(sw_vers, ifconfig, arp, dns-sd, adb devices, sqlite3 select, ping -c).
It never starts wireless ADB, changes settings, or injects input.

Usage:
    python3 tools/wifi-km-device-probe.py [--mdns-timeout SECONDS] [--json]
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import pathlib
import re
import socket
import subprocess
import sys

LEFTCAR_APP_SUPPORT = pathlib.Path.home() / "Library/Application Support/leftcar-host"
TCC_DB = pathlib.Path("/Library/Application Support/com.apple.TCC/TCC.db")
MDNS_SERVICES = ("_leftcar._tcp", "_adb-tls-connect._tcp")
TCC_CLIENTS = ("leftcar.ll3.kr", "kr.ll3.keybridge")


def run(cmd: list[str], timeout: float = 10.0) -> str:
    """Run a read-only command and return stdout+stderr, or an error string."""
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=timeout, check=False
        )
        out = (proc.stdout + proc.stderr).strip()
        return out if out else f"(no output, exit={proc.returncode})"
    except FileNotFoundError:
        return f"(command not found: {cmd[0]})"
    except subprocess.TimeoutExpired:
        return "(command timed out)"


def browse_mdns(service: str, timeout: float) -> str:
    """Browse one mDNS service type. Discovery only; no connection is made."""
    try:
        proc = subprocess.Popen(
            ["dns-sd", "-B", service, "local."],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        try:
            out, _ = proc.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            proc.kill()
            out, _ = proc.communicate()
    except FileNotFoundError:
        return "(dns-sd not found)"
    lines = [ln for ln in out.splitlines() if " Add " in ln or "Rmv" in ln]
    return "\n".join(lines) if lines else "(no instances seen)"


def local_addresses() -> list[str]:
    """Best-effort list of this host's non-loopback IPv4 addresses."""
    addrs: set[str] = set()
    ifout = run(["ifconfig"])
    for m in re.finditer(r"inet (\d+\.\d+\.\d+\.\d+) ", ifout):
        ip = m.group(1)
        if not ip.startswith("127."):
            addrs.add(ip)
    return sorted(addrs)


def arp_neighbours() -> list[str]:
    out = run(["arp", "-a"])
    return sorted({m.group(1) for m in re.finditer(r"\((\d+\.\d+\.\d+\.\d+)\)", out)})


def load_json_file(path: pathlib.Path):
    try:
        return json.loads(path.read_text())
    except FileNotFoundError:
        return f"(missing: {path})"
    except (OSError, ValueError) as exc:
        return f"(unreadable: {exc})"


def unix_to_iso(value) -> str:
    prefix = "unix:"
    if isinstance(value, str) and value.startswith(prefix):
        value = int(value[len(prefix):])
    if isinstance(value, (int, float)):
        return datetime.datetime.fromtimestamp(value).strftime("%Y-%m-%d %H:%M:%S")
    return str(value)


def tcc_rows() -> str:
    clients = ", ".join(f"'{c}'" for c in TCC_CLIENTS)
    query = (
        "select service, client, auth_value, "
        "datetime(last_modified,'unixepoch') from access "
        f"where client in ({clients});"
    )
    return run(["sqlite3", str(TCC_DB), query])


def session_summary(limit: int = 5) -> list[str]:
    path = LEFTCAR_APP_SUPPORT / "sessions.jsonl"
    try:
        lines = path.read_text().strip().splitlines()
    except OSError as exc:
        return [f"(unreadable: {exc})"]
    out = []
    for line in lines[-limit:]:
        try:
            event = json.loads(line)
        except ValueError:
            continue
        event["ts"] = unix_to_iso(event.get("ts"))
        out.append(json.dumps(event, ensure_ascii=False))
    return out


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Read-only Leftcar connected-device diagnostics (no state changes)."
    )
    parser.add_argument(
        "--mdns-timeout",
        type=float,
        default=6.0,
        help="seconds to wait per mDNS browse (default: 6)",
    )
    parser.add_argument(
        "--json", action="store_true", help="emit machine-readable JSON report"
    )
    args = parser.parse_args()

    paired = load_json_file(LEFTCAR_APP_SUPPORT / "paired_devices.json")
    report = {
        "generated_at": datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
        "mode": "read-only diagnostics; wireless ADB never used",
        "macos_version": run(["sw_vers"]),
        "adb_devices": run(["adb", "devices", "-l"]),
        "local_ipv4": local_addresses(),
        "arp_neighbours": arp_neighbours(),
        "mdns": {
            svc: browse_mdns(svc, args.mdns_timeout) for svc in MDNS_SERVICES
        },
        "paired_devices": paired,
        "recent_sessions": session_summary(),
        "tcc_permissions": tcc_rows(),
    }

    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        for key, value in report.items():
            print(f"== {key} ==")
            if isinstance(value, (list, dict)):
                print(json.dumps(value, ensure_ascii=False, indent=2))
            else:
                print(value)
            print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
