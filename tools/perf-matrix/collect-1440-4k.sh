#!/bin/sh

set -eu

profile=""
duration=30
output_prefix=""
package_name="${LEFTCAR_PACKAGE:-leftcar.ll3.kr}"
clear_log=true
host_pid=""
android_pid=""

usage() {
  cat <<'EOF'
Usage: collect-1440-4k.sh --profile latency|balanced|clarity|video [--duration seconds] [--output report-prefix] [--no-clear]

The stream must already be running. Start the desired Viewer profile, verify
motion, and finish warm-up before starting this collector.
EOF
}

stop_children() {
  for child_pid in "$host_pid" "$android_pid"; do
    if [ -n "$child_pid" ]; then
      kill "$child_pid" >/dev/null 2>&1 || true
    fi
  done
  for child_pid in "$host_pid" "$android_pid"; do
    if [ -n "$child_pid" ]; then
      wait "$child_pid" >/dev/null 2>&1 || true
    fi
  done
  host_pid=""
  android_pid=""
}

trap 'stop_children' EXIT
trap 'stop_children; exit 130' INT
trap 'stop_children; exit 143' TERM

while [ "$#" -gt 0 ]; do
  case "$1" in
    --profile)
      profile="$2"
      shift 2
      ;;
    --duration)
      duration="$2"
      shift 2
      ;;
    --output)
      output_prefix="$2"
      shift 2
      ;;
    --no-clear)
      clear_log=false
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$profile" in
  latency)
    expected_resolution="1920x1080"
    expected_mode="interactive"
    ;;
  balanced)
    expected_resolution="2560x1440"
    expected_mode="interactive"
    ;;
  clarity)
    expected_resolution="3840x2160"
    expected_mode="interactive"
    ;;
  video)
    expected_resolution="3840x2160"
    expected_mode="video"
    ;;
  *)
    echo "--profile must be latency, balanced, clarity, or video" >&2
    exit 2
    ;;
esac

case "$duration" in
  ''|*[!0-9]*)
    echo "--duration must be a positive integer" >&2
    exit 2
    ;;
esac

if [ "$duration" -lt 1 ]; then
  echo "--duration must be at least 1 second" >&2
  exit 2
fi

if ! adb get-state >/dev/null 2>&1; then
  echo "no authorized adb device is available" >&2
  exit 1
fi

if [ -z "$output_prefix" ]; then
  output_prefix="/tmp/leftcar-perf-matrix/$(date +%Y%m%d-%H%M%S)-${profile}"
fi

report="$output_prefix"
host_log="${report}.host.ndjson"
android_log="${report}.android.log"
summary="${report}.summary.json"
gfxinfo="${report}.gfxinfo.txt"
mkdir -p "$(dirname "$report")"

device_state="$(adb get-state 2>/dev/null || true)"
device_model="$(adb shell getprop ro.product.model 2>/dev/null | tr -d '\r' || true)"
android_version="$(adb shell getprop ro.build.version.release 2>/dev/null | tr -d '\r' || true)"

{
  echo "# Leftcar ${profile} performance sample"
  echo
  echo "- collectedAt: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- profile: ${profile}"
  echo "- expectedResolution: ${expected_resolution}"
  echo "- expectedMode: ${expected_mode}"
  echo "- durationSeconds: ${duration}"
  echo "- package: ${package_name}"
  echo "- deviceState: ${device_state}"
  echo "- deviceModel: ${device_model}"
  echo "- androidVersion: ${android_version}"
  echo "- hostLog: ${host_log}"
  echo "- androidLog: ${android_log}"
  echo "- androidLogBufferCleared: ${clear_log}"
  echo "- summary: ${summary}"
  echo "- gfxInfo: ${gfxinfo}"
  echo
  echo "## Required Host confirmation"
  echo
  echo "Confirm the selected encoder mode/ID, hardware acceleration, preset, and applied/unsupported/rejected properties before treating a final 4K result as accepted."
} > "${report}.md"

echo "collecting ${profile} for ${duration}s: ${report}"
/usr/bin/log stream --style ndjson --level info --predicate 'process == "leftcar-host-desktop" AND eventMessage CONTAINS "LeftcarPerf"' > "$host_log" 2>&1 &
host_pid=$!
if [ "$clear_log" = true ]; then
  adb logcat -c
fi
adb logcat -v epoch -s LeftcarNative > "$android_log" 2>&1 &
android_pid=$!

sleep "$duration"
stop_children
trap - EXIT INT TERM

bun tools/perf-matrix/analyze-performance.ts --host "$host_log" --android "$android_log" --duration "$duration" --output "$summary"
adb shell dumpsys gfxinfo "$package_name" framestats > "$gfxinfo" 2>/dev/null || true

echo "saved: ${report}.md"
echo "saved: ${host_log}"
echo "saved: ${android_log}"
echo "saved: ${summary}"
echo "saved: ${gfxinfo}"
