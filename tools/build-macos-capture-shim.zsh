#!/bin/zsh

set -euo pipefail

tool_dir=${0:A:h}
repo_root=${tool_dir:h}
shim_root="$repo_root/native/macos-capture-shim"
mode=${1:?"usage: build-macos-capture-shim.zsh <library|policy-test|split-test|tile-throughput-probe> <output>"}
output=${2:?"missing output path"}

typeset -a shim_sources frameworks framework_args
shim_sources=("$shim_root"/Sources/**/*.swift(N))
frameworks=(
  AppKit
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
  policy-test)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$shim_root/Tests/EncodePolicyTests.swift" \
      -o "$output" \
      "${framework_args[@]}"
    ;;
  split-test)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$shim_root/Tests/SplitPipelineTests.swift" \
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
