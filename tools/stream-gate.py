#!/usr/bin/env python3
"""Trust-tier acceptance gates for a recorded stream-stats JSONL run.

Only metrics the 2026-09-14 trust analysis rated safe as gates are used:
windowed rates (cap/enc/rend), monotonic counters read as deltas, and
liveness state. Known traps are accounted for: cumulative viewer counters
are diffed (a static value is a lower bound, 0 != "no loss"), static
screens legitimately drive rates to ~0 (motion is judged on cap > 0
samples), and split-only fields (idrRes/FEC/inRtt) are simply absent in
single sessions.

Usage:
  python3 tools/stream-gate.py --log run.jsonl
  python3 tools/stream-gate.py --log run.jsonl --min-rend-ratio 0.6
Exit code 0 = all gates pass, 1 = at least one FAIL.
"""

import argparse
import json
import sys
from pathlib import Path

DELTA_FIELDS = [
    "receiverFrameGaps",
    "receiverPairedIdrEpisodes",
    "receiverPairedIdrResumes",
    "receiverIncompleteAUs",
    "udpSendFailures",
    "recoveryKeyframes",
    "recoveryRequestsSuppressed",
    "bitrateFloorCollapseCount",
]


def load_samples(path: Path):
    samples = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            record = json.loads(line)
        except ValueError:
            continue
        sessions = record.get("sessions")
        if sessions is None:
            continue  # polling header lines before the first session appeared
        samples.append(sessions)
    return samples


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--log", required=True, help="JSONL from stream-stats.py --log")
    parser.add_argument("--min-rend-ratio", type=float, default=0.5,
                        help="rend >= ratio * fpsTarget on samples where cap > 0")
    parser.add_argument("--min-motion-samples", type=int, default=10,
                        help="cap > 0 samples required to judge the render gate")
    parser.add_argument("--max-recovery-delta", type=int, default=0,
                        help="allowed new recoveryKeyframes during the run")
    parser.add_argument("--fps-target", type=int, help="override fps target")
    args = parser.parse_args()

    samples = load_samples(Path(args.log))
    if not samples:
        print("no session samples in log")
        sys.exit(1)

    session_ids: set = set()
    state_values: set = set()
    fps_target = args.fps_target
    cap_positive = 0
    rend_values: list[int] = []
    rend_pass = 0
    counter_first: dict[str, int] = {}
    counter_last: dict[str, int] = {}

    for sessions in samples:
        for view in sessions:
            session_ids.add(view.get("session"))
            state_values.add(view.get("state"))
            if fps_target is None:
                fps_target = view.get("fpsTarget") or view.get("fps") or 60
            for field in DELTA_FIELDS:
                value = view.get(field)
                if value is None:
                    continue
                counter_first.setdefault(field, value)
                counter_last[field] = value
            cap = view.get("captureFps") or 0
            rend = view.get("renderedFps")
            if cap > 0:
                cap_positive += 1
                if rend is not None:
                    rend_values.append(rend)
                    if rend >= fps_target * args.min_rend_ratio:
                        rend_pass += 1
            state = view.get("state")
            if state not in (None, "running"):
                state_values.add(state)

    results: list[tuple[str, str, str]] = []

    def gate(name: str, ok: bool, detail: str):
        results.append((name, "PASS" if ok else "FAIL", detail))

    single_session = len(session_ids) == 1
    states_clean = state_values <= {None, "running"}
    gate("liveness", single_session and states_clean and bool(session_ids),
         f"sessions={sorted(session_ids)} states={sorted(s for s in state_values if s)}")

    ratio = (rend_pass / len(rend_values)) if rend_values else 0.0
    if cap_positive >= args.min_motion_samples:
        rend_lo = min(rend_values) if rend_values else -1
        rend_hi = max(rend_values) if rend_values else -1
        gate("render", ratio >= 0.9,
             f"rend >= {args.min_rend_ratio}*{fps_target} on {ratio:.0%} of "
             f"{len(rend_values)} motion samples (min={rend_lo} max={rend_hi})")
    else:
        results.append(("render", "WARN",
                        f"only {cap_positive} motion samples (< {args.min_motion_samples}); "
                        "re-run during real screen motion for a verdict"))

    deltas = {f: counter_last[f] - counter_first[f]
              for f in DELTA_FIELDS if f in counter_first}

    gaps = deltas.get("receiverFrameGaps")
    gate("loss", gaps == 0, f"receiverFrameGaps delta={gaps} (lower bound; NACK-healed loss is invisible)")
    recov = deltas.get("recoveryKeyframes")
    gate("recovery-budget", recov is not None and recov <= args.max_recovery_delta,
         f"recoveryKeyframes delta={recov} (allowed <= {args.max_recovery_delta})")
    floor_delta = deltas.get("bitrateFloorCollapseCount")
    gate("bitrate-floor", floor_delta == 0, f"bitrateFloorCollapseCount delta={floor_delta}")
    send_fail = deltas.get("udpSendFailures")
    gate("send-health", send_fail == 0, f"udpSendFailures delta={send_fail}")

    failed = [name for name, verdict, _ in results if verdict == "FAIL"]
    for name, verdict, detail in results:
        print(f"{verdict:4}  {name:16}  {detail}")
    print(f"\n{len(results) - len(failed)}/{len(results)} gates pass"
          + ("" if not failed else f"; FAILED: {', '.join(failed)}"))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
