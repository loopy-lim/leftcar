#!/bin/sh

set -eu

profile="unknown"
duration=30
output_dir="/tmp/leftcar-perf-matrix"
package_name="${LEFTCAR_PACKAGE:-leftcar.ll3.kr}"
clear_log=true

usage() {
  cat <<'EOF'
Usage: collect-1440-4k.sh --profile balanced|video [--duration seconds] [--output directory] [--no-clear]

The stream must already be running. This collector records Android-side logs
and metadata; Host stage metrics must be copied from the Host inspector.
EOF
}

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
      output_dir="$2"
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
  balanced|video) ;;
  *)
    echo "--profile must be balanced or video" >&2
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

mkdir -p "$output_dir"
stamp="$(date +%Y%m%d-%H%M%S)"
report="$output_dir/${stamp}-${profile}"

{
  echo "# Leftcar ${profile} performance sample"
  echo
  echo "- collectedAt: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- profile: ${profile}"
  echo "- durationSeconds: ${duration}"
  echo "- package: ${package_name}"
  echo "- deviceState: $(adb get-state 2>/dev/null || true)"
  echo "- model: $(adb shell getprop ro.product.model 2>/dev/null | tr -d '\r' || true)"
  echo "- android: $(adb shell getprop ro.build.version.release 2>/dev/null | tr -d '\r' || true)"
  echo "- host metrics: copy from Host inspector"
  echo
  echo "## Host metrics"
  echo
  echo "captureFps / encodeSubmitFps / encodeOutputFps / renderedFps:"
  echo "captureToEncodeP95Us / encodeOutputP95Us / encodeOutputIntervalP95Us:"
  echo "sendBlockP95Us / sendPaceP95Us / receiverWireMs:"
  echo "drops / recovery / FEC / udpSendFailures:"
} > "${report}.md"

if [ "$clear_log" = true ]; then
  adb logcat -c
fi

echo "collecting ${profile} for ${duration}s: ${report}"
adb logcat -v threadtime LeftcarNative:I '*:S' > "${report}.android.log" &
logcat_pid=$!
trap 'kill "$logcat_pid" >/dev/null 2>&1 || true; wait "$logcat_pid" >/dev/null 2>&1 || true' EXIT INT TERM
sleep "$duration"
kill "$logcat_pid" >/dev/null 2>&1 || true
wait "$logcat_pid" >/dev/null 2>&1 || true
trap - EXIT INT TERM

adb shell dumpsys gfxinfo "$package_name" framestats > "${report}.gfxinfo.txt" 2>/dev/null || true

echo "saved: ${report}.md"
echo "saved: ${report}.android.log"
echo "saved: ${report}.gfxinfo.txt"
