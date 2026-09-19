#!/usr/bin/env python3
"""Conservative gates for stream-stats JSONL; missing evidence never passes.

Rates and latency distributions describe sampled telemetry, not every frame or
physical input-to-photon latency. Use --report to save a comparable JSON receipt.
"""
import argparse
import json
import math
from pathlib import Path

DELTA_FIELDS = ["receiverFrameGaps", "receiverPairedIdrEpisodes",
    "receiverPairedIdrResumes", "receiverIncompleteAus", "udpSendFailures",
    "recoveryKeyframes", "recoveryRequestsSuppressed", "bitrateFloorCollapseCount"]
REQUIRED_COUNTERS = {"receiverFrameGaps", "recoveryKeyframes", "udpSendFailures", "bitrateFloorCollapseCount"}


def number(value):
    return not isinstance(value, bool) and isinstance(value, (int, float)) and math.isfinite(value) and value >= 0


def distribution(values):
    values = sorted(v for v in values if number(v))
    if not values:
        return {"samples": 0, "p50": None, "p95": None, "max": None}
    rank = lambda p: values[max(0, math.ceil(len(values) * p) - 1)]
    return {"samples": len(values), "p50": rank(.5), "p95": rank(.95), "max": values[-1]}


def analyze(records, args):
    gates = []
    def gate(name, ok, detail):
        gates.append({"name": name, "verdict": "PASS" if ok else "FAIL", "detail": detail})
    valid_records = all(isinstance(r, dict) and isinstance(r.get("sessions"), list) for r in records)
    if not records or not valid_records:
        raise ValueError("Every sample must contain a sessions array")
    if not all(isinstance(v, dict) for r in records for v in r["sessions"]):
        raise ValueError("Session samples must be objects")
    ids = {v.get("session") for r in records for v in r["sessions"]}
    sid = args.session if args.session is not None else next(iter(ids)) if len(ids) == 1 else None
    selected = [[v for v in r["sessions"] if sid is not None and v.get("session") == sid] for r in records]
    views = [v[0] for v in selected if len(v) == 1]
    gate("liveness", bool(views) and all(len(v) == 1 and v[0].get("state") == "running" for v in selected),
         f"session={sid}; present={len(views)}/{len(records)}; every state must be running")
    times = [r.get("monotonicTime", r.get("time")) for r in records]
    valid_time = all(number(t) for t in times)
    intervals = [b-a for a,b in zip(times,times[1:])] if valid_time else []
    elapsed = times[-1]-times[0] if valid_time and len(times)>1 else 0
    gate("coverage", len(records)>1 and valid_time and all(0 < t <= args.max_sample_gap for t in intervals) and elapsed >= args.min_duration,
         f"elapsed={elapsed:.3f}s; max gap={max(intervals, default=0):.3f}s; required={args.min_duration}s")
    configurations = {(v.get("width"),v.get("height"),v.get("fpsTarget") or v.get("fps"),v.get("encoderExperimentApplied")) for v in views}
    complete_configuration = all(number(v.get("width")) and v["width"]>0 and
        number(v.get("height")) and v["height"]>0 and number(v.get("fpsTarget")) and v["fpsTarget"]>0 and
        isinstance(v.get("encoderExperimentApplied"), str) and bool(v["encoderExperimentApplied"])
        for v in views)
    gate("configuration", len(configurations)==1 and complete_configuration,
         "One complete resolution, target FPS and encoder mode per comparison segment")
    gate("minimum-resolution", bool(views) and all(
        number(v.get("width")) and number(v.get("height")) and
        min(v["width"], v["height"]) >= args.min_short_side for v in views),
        f"Every sample must have a short side of at least {args.min_short_side} pixels")
    motion = [v for v in views if number(v.get("captureFps")) and v["captureFps"]>0]
    telemetry = all(number(v.get("captureFps")) and number(v.get("renderedFps")) for v in views)
    gate("telemetry", bool(views) and telemetry, "Capture and receiver FPS required in every selected sample")
    targets = [args.fps_target or v.get("fpsTarget") or v.get("fps") for v in motion]
    passes = sum(number(v.get("renderedFps")) and number(t) and t>0 and v["renderedFps"] >= args.min_rend_ratio*t for v,t in zip(motion,targets))
    if len(motion)<args.min_motion_samples:
        gates.append({"name":"render", "verdict":"UNKNOWN", "detail":f"Only {len(motion)} motion samples; require {args.min_motion_samples}"})
    else:
        gate("render", passes/len(motion)>=.9, f"{passes}/{len(motion)} motion samples meet target ratio {args.min_rend_ratio}")
    deltas = {}
    invalid_counters = []
    for field in DELTA_FIELDS:
        values = [v.get(field) for v in views]
        if not any(v is not None for v in values) and field not in REQUIRED_COUNTERS:
            continue
        valid = len(values)>1 and all(number(v) and int(v)==v for v in values)
        valid = valid and all(b>=a for a,b in zip(values,values[1:]))
        deltas[field] = values[-1]-values[0] if valid else None
        if not valid:
            invalid_counters.append(field)
    gate("counter-continuity", not invalid_counters and bool(views), f"Missing/reset counters: {invalid_counters}")
    for name, field, limit in [("loss","receiverFrameGaps",0),("recovery-budget","recoveryKeyframes",args.max_recovery_delta),
                                ("bitrate-floor","bitrateFloorCollapseCount",0),("send-health","udpSendFailures",0)]:
        delta = deltas.get(field)
        gate(name, delta is not None and 0<=delta<=limit, f"{field} delta={delta}; allowed={limit}")
    metrics = {field: distribution(v.get(field) for v in views) for field in
        ["captureFps","encodeOutputFps","renderedFps","receiverInputRttMs","receiverSplitWireMs","receiverSplitCaptureAgeMs"]}
    return {"schema":1,"scope":"stream-status-only; excludes source workload, Surface latency and physical input acceptance",
        "session":sid,"sampleCount":len(records),"elapsedSeconds":elapsed,
        "configuration":list(next(iter(configurations))) if len(configurations)==1 else None,
        "gates":gates,"counterDeltas":deltas,"sampledTelemetry":metrics,
        "measurementBoundary":"Percentiles of status snapshots (input RTT may already be smoothed). Not per-event percentiles, panel presentation, or physical input-to-photon latency.",
        "verdict":"FAIL" if any(g["verdict"]=="FAIL" for g in gates) else "UNKNOWN" if any(g["verdict"]=="UNKNOWN" for g in gates) else "PASS"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", required=True)
    parser.add_argument("--report", help="JSON comparison receipt path")
    parser.add_argument("--session", type=int, help="Required when other sessions share the log")
    parser.add_argument("--min-rend-ratio", type=float, default=.95)
    parser.add_argument("--min-motion-samples", type=int, default=10)
    parser.add_argument("--max-recovery-delta", type=int, default=0)
    parser.add_argument("--fps-target", type=int)
    parser.add_argument("--max-sample-gap", type=float, default=2.5)
    parser.add_argument("--min-duration", type=float, default=10)
    parser.add_argument("--min-short-side", type=int, default=1440)
    args = parser.parse_args()
    if not 0 < args.min_rend_ratio <= 1 or args.min_motion_samples < 1 or args.max_recovery_delta < 0 or not number(args.max_sample_gap) or args.max_sample_gap == 0 or not number(args.min_duration) or args.min_short_side <= 0 or (args.fps_target is not None and args.fps_target <= 0):
        parser.error("Invalid measurement thresholds")
    try:
        records = [json.loads(line) for line in Path(args.log).read_text().splitlines() if line.strip()]
        report = analyze(records,args)
    except (OSError,ValueError,TypeError) as error:
        report = {"schema":1,"verdict":"FAIL","error":str(error),"gates":[]}
    if args.report:
        Path(args.report).write_text(json.dumps(report,indent=2,allow_nan=False)+"\n")
    for g in report["gates"]:
        print(f"{g['verdict']:7} {g['name']:20} {g['detail']}")
    print(report["verdict"] + (": " + report["error"] if "error" in report else ""))
    return {"PASS":0,"FAIL":1,"UNKNOWN":2}[report["verdict"]]


if __name__ == "__main__":
    raise SystemExit(main())
