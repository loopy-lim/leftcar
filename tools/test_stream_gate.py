"""Regression cases for misleading acceptance results, through the real CLI."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

TOOL = Path(__file__).with_name("stream-gate.py")


def samples():
    return [{"time": 1000 + i, "sessions": [{
        "session": 7, "state": "running", "width": 2560, "height": 1440,
        "fpsTarget": 60, "captureFps": 60, "renderedFps": 60,
        "encoderExperimentApplied": "rateControl", "receiverIncompleteAus": 0,
        "receiverFrameGaps": 0, "recoveryKeyframes": 0,
        "bitrateFloorCollapseCount": 0, "udpSendFailures": 0,
    }]} for i in range(12)]


class StreamGateTest(unittest.TestCase):
    def run_gate(self, records):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "samples.jsonl"
            log.write_text("\n".join(json.dumps(row) for row in records))
            return subprocess.run([sys.executable, str(TOOL), "--log", str(log)],
                                  capture_output=True, text=True)

    def test_complete_motion_sample_passes(self):
        self.assertEqual(self.run_gate(samples()).returncode, 0)

    def test_1080p_cannot_pass_the_minimum_resolution(self):
        rows = samples()
        for row in rows:
            row["sessions"][0].update(width=1920, height=1080)
        self.assertNotEqual(self.run_gate(rows).returncode, 0)

    def test_disappearing_session_cannot_pass_liveness(self):
        rows = samples()
        rows[5]["sessions"] = []
        self.assertNotEqual(self.run_gate(rows).returncode, 0)

    def test_missing_receiver_samples_cannot_pass_render(self):
        rows = samples()
        for row in rows[1:]:
            row["sessions"][0]["renderedFps"] = None
        self.assertNotEqual(self.run_gate(rows).returncode, 0)

    def test_counter_reset_cannot_count_as_negative_recovery(self):
        rows = samples()
        rows[0]["sessions"][0]["recoveryKeyframes"] = 5
        self.assertNotEqual(self.run_gate(rows).returncode, 0)

    def test_missing_time_interval_cannot_pass(self):
        rows = samples()
        for row in rows[6:]:
            row["time"] += 20
        self.assertNotEqual(self.run_gate(rows).returncode, 0)

    def test_idle_is_unknown_instead_of_a_performance_pass(self):
        rows = samples()
        for row in rows:
            row["sessions"][0]["captureFps"] = 0
            row["sessions"][0]["renderedFps"] = 0
        self.assertEqual(self.run_gate(rows).returncode, 2)

    def test_mode_change_requires_separate_comparison_segments(self):
        rows = samples()
        rows[-1]["sessions"][0]["width"] = 3840
        self.assertNotEqual(self.run_gate(rows).returncode, 0)

    def test_unknown_configuration_cannot_be_a_comparable_pass(self):
        rows = samples()
        for row in rows:
            del row["sessions"][0]["encoderExperimentApplied"]
        self.assertNotEqual(self.run_gate(rows).returncode, 0)

    def test_half_target_fps_no_longer_passes_by_default(self):
        rows = samples()
        for row in rows:
            row["sessions"][0]["renderedFps"] = 30
        self.assertNotEqual(self.run_gate(rows).returncode, 0)

    def test_poll_failure_is_retained_as_incomplete_evidence(self):
        rows = samples() + [{"time": 1012, "pollError": "TimeoutError"}]
        self.assertNotEqual(self.run_gate(rows).returncode, 0)

    def test_json_report_preserves_missing_latency_and_snapshot_percentiles(self):
        rows = samples()
        for i, row in enumerate(rows):
            row["sessions"][0]["receiverInputRttMs"] = i
        with tempfile.TemporaryDirectory() as directory:
            log, report = Path(directory)/"log.jsonl", Path(directory)/"report.json"
            log.write_text("\n".join(json.dumps(row) for row in rows))
            process = subprocess.run([sys.executable, str(TOOL), "--log", str(log), "--report", str(report)], capture_output=True)
            self.assertEqual(process.returncode, 0)
            data = json.loads(report.read_text())
            self.assertEqual(data["sampledTelemetry"]["receiverInputRttMs"], {"samples": 12, "p50": 5, "p95": 11, "max": 11})
            self.assertIsNone(data["sampledTelemetry"]["receiverSplitCaptureAgeMs"]["p95"])
            self.assertEqual(data["counterDeltas"]["receiverIncompleteAus"], 0)


if __name__ == "__main__":
    unittest.main()
