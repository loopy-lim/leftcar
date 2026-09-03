#!/bin/zsh
# CGVirtualDisplay 스파크 프로브 실행 스크립트.
#
# 디스플레이 생성/모드 실측은 WindowServer(GUI) 세션을 필요로 한다.
# SSH·자동화 셸에서는 활성 디스플레이가 0으로 보여 실측이 불가하다(세션 제약).
# 반드시 터미널 앱(Ghostty/Terminal 등)에서 이 스크립트를 실행할 것.
#
# 사용: zsh tools/cgvd-spark/run-probe.zsh
set -euo pipefail

dir=${0:A:h}

# 0) 세션 무관 실측: private 클래스 존재 여부 (어떤 셸에서든 가능)
print -- "== 1/2 세션 무관 실측: 클래스 존재 =="
swiftc -o "$dir/cgvd-exist" "$dir/cgvd-exist.swift"
"$dir/cgvd-exist"

# 1) GUI 세션 실측: 생성 → 1x 모드 → 120Hz 시도 → HiDPI @2x → SCK → 회전 → 60초 관찰 → 소멸
print -- "== 2/2 GUI 세션 실측: 생성/모드/캡처 정합 =="
swiftc -import-objc-header "$dir/CGVD.h" "$dir/main.swift" -o "$dir/cgvd-spark" \
  -framework AppKit -framework CoreGraphics -framework ScreenCaptureKit
"$dir/cgvd-spark"
