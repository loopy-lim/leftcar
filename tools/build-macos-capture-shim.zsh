#!/bin/zsh

set -euo pipefail

tool_dir=${0:A:h}
repo_root=${tool_dir:h}
shim_root="$repo_root/native/macos-capture-shim"
mode=${1:?"usage: build-macos-capture-shim.zsh <library|vdisp-test|capture-start-export-test|policy-test|adaptive-policy-test|split-test|split-policy-test|cursor-test|audio-ownership-test|media-sealer-test|media-interop-test|retransmit-ring-test|retransmit-policy-test|tile-throughput-probe|udp-send-probe|udp-frame-budget-test|udp-packet-size-test|peer-congestion-test|recovery-pacing-test|congestion-cut-test|pacing-clock-test|input-language-test|mouse-button-test> <output>"}
output=${2:?"missing output path"}

typeset -a shim_sources frameworks framework_args
shim_sources=("$shim_root"/Sources/**/*.swift(N))
frameworks=(
  AppKit
  Carbon
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

# 가상 디스플레이 브리지(ObjC) — library/vdisp-test 모드에서만 링크된다.
# 나머지 단일 파일 테스트 빌드는 이 오브젝트 없이도 컴파일된다: Swift 측
# 통합(VirtualDisplayCatalog.swift)이 심볼을 dlsym으로 늦게 찾기 때문이다.
vdisp_bridge="$shim_root/Sources/VirtualDisplay/CGVirtualDisplayBridge.m"
vdisp_object="${TMPDIR:-/tmp}/leftcar-vdisp-bridge.o"

build_vdisp_object() {
  /usr/bin/xcrun clang -c -fobjc-arc -o "$vdisp_object" "$vdisp_bridge"
}

case "$mode" in
  tcp-connection-test|terminal-notice-test|input-heartbeat-test)
    if [[ "$mode" == tcp-connection-test ]]; then
      test_source="$shim_root/Tests/TcpConnectionTests.swift"
    elif [[ "$mode" == terminal-notice-test ]]; then
      test_source="$shim_root/Tests/TerminalNoticeTests.swift"
    else
      test_source="$shim_root/Tests/InputStatusHeartbeatTests.swift"
    fi
    /usr/bin/xcrun swiftc -Onone \
      -module-cache-path "${TMPDIR:-/tmp}/leftcar-connection-module-cache" \
      "${shim_sources[@]}" "$test_source" \
      -o "$output" "${framework_args[@]}"
    ;;
  udp-frame-budget-test)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$shim_root/Tests/UdpFramePacingBudgetTests.swift" \
      -o "$output" "${framework_args[@]}"
    ;;
  udp-send-probe)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$repo_root/tools/network-probe/UdpSendReplayProbe.swift" \
      -o "$output" "${framework_args[@]}"
    ;;
  pacing-clock-test)
    /usr/bin/xcrun swiftc -Onone \
      "${shim_sources[@]}" \
      "$shim_root/Tests/UdpPacingClockTests.swift" \
      -o "$output" "${framework_args[@]}"
    ;;
  udp-packet-size-test)
    /usr/bin/xcrun swiftc -Onone \
      "${shim_sources[@]}" \
      "$shim_root/Tests/UdpPacketSizeTests.swift" \
      -o "$output" "${framework_args[@]}"
    ;;
  peer-congestion-test)
    /usr/bin/xcrun swiftc -Onone \
      "${shim_sources[@]}" \
      "$shim_root/Tests/PeerCongestionMemoryTests.swift" \
      -o "$output" "${framework_args[@]}"
    ;;
  recovery-pacing-test)
    /usr/bin/xcrun swiftc -Onone \
      "${shim_sources[@]}" \
      "$shim_root/Tests/RecoveryPacingTests.swift" \
      -o "$output" "${framework_args[@]}"
    ;;
  mouse-button-test)
    /usr/bin/xcrun swiftc -Onone \
      "${shim_sources[@]}" \
      "$shim_root/Tests/MouseButtonMappingTests.swift" \
      -o "$output" "${framework_args[@]}"
    ;;
  congestion-cut-test)
    /usr/bin/xcrun swiftc -Onone \
      "${shim_sources[@]}" \
      "$shim_root/Tests/AdaptiveCongestionCutTests.swift" \
      -o "$output" "${framework_args[@]}"
    ;;
  input-language-test)
    /usr/bin/xcrun swiftc -Onone \
      "${shim_sources[@]}" \
      "$shim_root/Tests/InputLanguageTests.swift" \
      -o "$output" "${framework_args[@]}"
    ;;
  library)
    build_vdisp_object
    /usr/bin/xcrun swiftc -O -emit-library \
      "${shim_sources[@]}" \
      "$vdisp_object" \
      -o "$output" \
      "${framework_args[@]}" \
      -framework ColorSync
    ;;
  vdisp-test)
    # 브리지 물리 검증 CLI(GUI 터미널에서 실행). 브리지 + 탐침만 링크.
    build_vdisp_object
    /usr/bin/xcrun swiftc -O \
      "$shim_root/Tests/VirtualDisplayBridgeProbe.swift" \
      "$vdisp_object" \
      -o "$output" -framework Foundation -framework CoreGraphics -framework ColorSync -lobjc
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
  media-interop-test)
    # In-memory encryption plus a fixture-only Rust peer; no capture or sockets.
    /usr/bin/xcrun swiftc -O \
      -module-cache-path "${TMPDIR:-/tmp}/leftcar-media-interop-module-cache" \
      "$shim_root/Sources/Transport/MediaSealer.swift" \
      "$shim_root/Tests/MediaCryptoInteropTests.swift" \
      -o "$output" -framework Foundation
    ;;
  audio-ownership-test)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$shim_root/Tests/SystemAudioOwnershipTests.swift" \
      -o "$output" \
      "${framework_args[@]}"
    ;;
  media-sealer-test)
    # Pure crypto: only the sealer itself plus the shared cross-language vector.
    /usr/bin/xcrun swiftc -O \
      -module-cache-path "${TMPDIR:-/tmp}/leftcar-media-sealer-module-cache" \
      "$shim_root/Sources/Transport/MediaSealer.swift" \
      "$shim_root/Tests/MediaSealerTests.swift" \
      -o "$output" -framework Foundation -framework CryptoKit
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
