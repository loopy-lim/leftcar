#!/usr/bin/env python3
"""Validate recorded physical full-KM evidence; never infer success from unit tests."""
import argparse
import json
from pathlib import Path
import sys
import unittest

REQUIRED = (
    "mac_to_android_mouse", "mac_to_android_keyboard", "mac_to_android_cursor",
    "mac_to_android_hover", "android_to_mac_mouse", "android_to_mac_keyboard",
    "android_to_mac_cursor", "android_to_mac_hover", "edge_mac_to_android",
    "edge_android_to_mac", "disconnect_release", "revoke_release",
    "watchdog_release", "no_injection_loop", "no_touchscreen_capture",
)


def validate(record):
    if not isinstance(record, dict):
        return ["record must be an object"]
    errors = []
    if record.get("device_model") != "Lenovo TB522FU":
        errors.append("device_model must be Lenovo TB522FU")
    if record.get("android_cursor_authority_approved") is not True:
        errors.append("Android real cursor authority approval missing")
    if record.get("transport") != "direct-wifi":
        errors.append("transport must be direct-wifi")
    evidence = record.get("evidence")
    if not isinstance(evidence, dict):
        return errors + ["evidence must be an object"]
    for key in REQUIRED:
        item = evidence.get(key)
        if not isinstance(item, dict) or item.get("passed") is not True or not isinstance(item.get("observation"), str) or not item["observation"].strip() or not isinstance(item.get("timestamp"), str) or not item["timestamp"].strip():
            errors.append(f"missing passing timestamped observation: {key}")
    return errors


class CheckerTests(unittest.TestCase):
    def test_empty_record_fails_closed(self):
        self.assertTrue(validate({}))

    def test_each_required_observation(self):
        record = {"device_model": "Lenovo TB522FU", "android_cursor_authority_approved": True, "transport": "direct-wifi", "evidence": {key: {"passed": True, "observation": "observed on physical device", "timestamp": "2026-01-01T00:00:00Z"} for key in REQUIRED}}
        self.assertEqual(validate(record), [])
        for key in REQUIRED:
            missing = dict(record, evidence={k: v for k, v in record["evidence"].items() if k != key})
            self.assertTrue(any(key in error for error in validate(missing)))

    def test_unapproved_authority_fails(self):
        self.assertIn("Android real cursor authority approval missing", validate({"evidence": {}}))


def main():
    parser = argparse.ArgumentParser(description="Validate physical bidirectional Wi-Fi KM evidence (JSON); no device actions")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--self-test", action="store_true", help="run checker unit tests")
    group.add_argument("--evidence", type=Path, help="path to recorded physical evidence JSON")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(CheckerTests)
        return 0 if unittest.TextTestRunner().run(suite).wasSuccessful() else 1
    try:
        errors = validate(json.loads(args.evidence.read_text()))
    except (OSError, ValueError) as exc:
        print(f"evidence unavailable or invalid: {exc}", file=sys.stderr)
        return 1
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        return 1
    print("physical KM evidence complete")
    return 0


if __name__ == "__main__":
    sys.exit(main())
