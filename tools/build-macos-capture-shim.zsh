#!/bin/zsh

set -euo pipefail

tool_dir=${0:A:h}
repo_root=${tool_dir:h}
shim_root="$repo_root/native/macos-capture-shim"
mode=${1:?"usage: build-macos-capture-shim.zsh <library|capture-start-export-test|policy-test|adaptive-policy-test|split-test|split-policy-test|cursor-test|audio-ownership-test|retransmit-ring-test|retransmit-policy-test|tile-throughput-probe> <output>"}
output=${2:?"missing output path"}

typeset -a shim_sources frameworks framework_args
shim_sources=("$shim_root"/Sources/**/*.swift(N))
frameworks=(
  AppKit
  AudioToolbox
  CoreGraphics
  CoreMedia
  CoreVideo
  Foundation
  IOSurface
  Metal
  ScreenCaptureKit
  Security
  VideoToolbox
)

for framework in "${frameworks[@]}"; do
  framework_args+=(-framework "$framework")
done

case "$mode" in
  library)
    /usr/bin/xcrun swiftc -O -emit-library \
      "${shim_sources[@]}" \
      -o "$output" \
      "${framework_args[@]}"
    ;;
  capture-start-export-test)
    # Real versioned exports + pure parsers; only final OS constructor is replaced.
    # No screen/audio capture, sockets, native input, pixel-buffer or codec adapters.
    /usr/bin/xcrun swiftc -O \
      -module-cache-path "${TMPDIR:-/tmp}/leftcar-start-export-module-cache" \
      "$shim_root/Sources/Capture/CaptureStartOptions.swift" \
      "$shim_root/Sources/Capture/SourceAuthorization.swift" \
      "$shim_root/Sources/Encoder/EncoderExperimentParsing.swift" \
      "$shim_root/Sources/Transport/CaptureTransportPolicy.swift" \
      "$shim_root/Sources/CaptureShim+UdpStabilityExports.swift" \
      "$shim_root/Tests/CaptureStartExportTests.swift" \
      -o "$output" -framework Foundation
    ;;
  policy-test)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$shim_root/Tests/EncodePolicyTests.swift" \
      -o "$output" \
      "${framework_args[@]}"
    ;;
  adaptive-policy-test)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$shim_root/Tests/AdaptiveResolutionPolicyTests.swift" \
      -o "$output" \
      "${framework_args[@]}"
    ;;
  split-policy-test)
    # Explicit allowlist: no CaptureSession, screen capture, codec or socket paths.
    /usr/bin/xcrun swiftc -O \
      -module-cache-path "${TMPDIR:-/tmp}/leftcar-split-policy-module-cache" \
      "$shim_root/Sources/Split/SplitGeometry.swift" \
      "$shim_root/Sources/Split/SplitFlowControlState.swift" \
      "$shim_root/Sources/Split/SplitPairLifecycleState.swift" \
      "$shim_root/Sources/Split/SplitRecoveryPolicy.swift" \
      "$shim_root/Sources/Split/EncodedPairAssembler.swift" \
      "$shim_root/Tests/SplitPolicyTests.swift" \
      -o "$output" -framework Foundation
    ;;
  split-test)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$shim_root/Tests/SplitPipelineTests.swift" \
      -o "$output" \
      "${framework_args[@]}"
    ;;
  cursor-test)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$shim_root/Tests/CursorStreamTests.swift" \
      -o "$output" \
      "${framework_args[@]}"
    ;;
  audio-ownership-test)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$shim_root/Tests/SystemAudioOwnershipTests.swift" \
      -o "$output" \
      "${framework_args[@]}"
    ;;
  retransmit-policy-test)
    # Allowlisted pure cache, geometry types and in-memory crypto only.
    # No CaptureSession, screen/audio sources, socket or encoder adapters.
    /usr/bin/xcrun swiftc -O \
      -module-cache-path "${TMPDIR:-/tmp}/leftcar-retransmit-policy-module-cache" \
      "$shim_root/Sources/Transport/MediaRetransmitRing.swift" \
      "$shim_root/Sources/Split/SplitGeometry.swift" \
      "$shim_root/Sources/Transport/MediaSealer.swift" \
      "$shim_root/Tests/RetransmitRingTests.swift" \
      -o "$output" -framework Foundation -framework CryptoKit
    ;;
  retransmit-ring-test)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$shim_root/Tests/RetransmitRingTests.swift" \
      -o "$output" \
      "${framework_args[@]}"
    ;;
  tile-throughput-probe)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$repo_root/tools/codec-probe/TileEncoderThroughputProbe.swift" \
      -o "$output" \
      "${framework_args[@]}"
    ;;
  *)
    print -u2 "unknown build mode: $mode"
    exit 2
    ;;
esac
