# GXR 34fps dip 원인 규명과 UDP 안정성 자동화 단순화 (2026-11-22)

요청: Galaxy XR(SM-I610) 뷰어에서 표시 fps가 약 34까지 떨어지는 현상의 원인
확인(Mac↔공유기 1Gbps 유선, 공유기↔GXR Wi-Fi). 원인 규명 후 사용자 결정으로
UDP 안정성 설정 단순화와 auto 적응 강화를 구현했다.

## 결론

**dip의 원인은 Wi-Fi 대역폭 부족이 아니라, 저장된 옛 프로파일 'stable'
(burst 2 + FEC parity 4, 적응 페이싱 꺼짐)에서 큰 프레임의 IDR 복구 배수가
느려져 자가 유지 복구 루프에 빠진 것이다.** 5GHz 마지막 홉의 순간 유실은
트리거지만, dip의 크기와 지속은 복구 정책이 만든 것이다.

측정(60초 시계열, `getStatus` 1Hz, 세션 30, 2226×1438@60 목표):

- 'stable' 적용 시: 호스트 파이프라인(캡처/인코딩/송출)은 49~60fps로 정상,
  인코더 드롭 0, UDP 전송 실패 0. 그러나 뷰어 렌더 fps가 25/33/35/36/37로
  반복 급락했고 그 시점마다 `networkDropped`가 +30~40씩 증가(60초 +232),
  `recoveryKeyframes` 동반 증가. 급락 직전에 13~33Mbps 비트레이트 스파이크.
- `networkDropped` 1085 == `recoveryFramesDropped` 1085: 전부 "IDR 올 때까지
  델타 폐기" 복구 경계 드롭이다(호스트 송신 큐 폐기,
  CaptureSession+NetworkQueue.swift).
- 적응 페이싱 전환(custom burst 4 + pacing on) 직후 60초: `networkDropped`
  +0, `recoveryKeyframes` +0, 렌더 저점 45, 21~22Mbps 스파이크에도 손실 없음.
- 경로 확인: en9 1000baseT(유선) → 공유기 → Wi-Fi → GXR(192.168.0.8).
  ping 0% 손실, 지터 min 3 / avg 20 / max 44ms.

아티팩트: `artifacts/udp-stability-gxr-2026-11-22/`
- `leftcar-fps-34fps.jsonl` — stream-stats.py 원시 로그('stable' 시대 스냅샷)
- `leftcar-fps-timeseries.json` — 전(-before) 60초 시계열
- `leftcar-fps-after.json` — 후(after) 60초 시계열

기존 근거와의 연결: 안정 프리셋 조합의 IDR 복구 루프는
`native/macos-capture-shim/Sources/Transport/UdpStabilityPolicy.swift`의
회복 경계 주석("parity four plus burst two made 4K IDRs take 70-120ms and
triggered a self-sustaining recovery loop")과 docs/11-low-latency-investigation
H-01 계열과 같은 메커니즘이다.

## 구현 (사용자 승인 계획, 2026-11-22)

사용자 방향: 고급 섹션은 유지하되 세부 수동 조정·중복은 제거하고 auto가
연결 상태를 보고 더 스스로 최적화하게. 저장된 옛 설정은 한 번 auto로 초기화.

1. **계약**(`crates/control-contract/src/udp_stability.rs`): 뷰어가 값을
   생략해도 Auto 요청으로 취급. viewer caps가 없으면 기존 폴백 체인이
   Balanced(4/2/꺼짐)를 적용한다(적응 페이싱은 feedback v2 카운터를 소비하므로
   caps 없는 뷰어에게 강제하지 않는다). balanced/custom은 구버전 협상·폴백용으로
   계약에 유지.
2. **Shim 연결 적응 사다리**(`UdpStabilityPolicy.swift`): auto 세션의 유실
   에피소드가 반복되면 tier 0(기본선 4/2) → 1(parity 3) → 2(parity 4) →
   3(parity 4 + burst 2, hold 동안만)로 보호를 올리고, 클린 5s마다 한 단계씩
   내려가 30s 클린에 완전 리셋한다. tier 3 조합이 루프 유발 조합이라 상시
   유지하지 않는 것이 핵심이다. 진단용 reason `escalated`/`standingDown` 추가.
3. **뷰어 마이그레이션**(`apps/viewer-expo/src/udp-stability.ts`): 저장 키를
   `leftcar.udpStability.v2`로 회전하고 레거시 키를 best-effort 삭제해 모든
   기존 저장값(stable/balanced/custom 포함)이 한 번 auto로 초기화된다.
4. **뷰어 UI 단순화**(`UdpStabilityControls.tsx`, `packages/ui-tokens/src/i18n.ts`):
   프리셋 3개(자동 권장/저지연/안정성 우선)만 남기고 세부 패널(버스트/FEC/
   페이싱 개별 토글, custom 진입)과 중복 프리셋 'balanced'(표준)을 제거했다.

## 검증

- `cargo test -p control-contract` 20 passed. 워크스페이스 296 passed.
- shim: `policy-test` 포함 관련 6개 모드 빌드·실행 통과(사다리 상승/하강,
  에피소드 창 리셋, 기존 시퀀스 호환).
- 뷰어: typecheck 통과, udp-stability/launch-stream/native-settings 59 passed.
- React Doctor 100/100.
- 기존 실패(본 변경과 무관, 깨끗한 main에서 동일 재현):
  `tools/vitest.inventory.test.mjs`(temp 경로 상대화),
  `architecture-check` `real_workspace_and_standalone_host_pass`
  (src-tauri Cargo.lock `--locked` 불일치).
- 실기기: 설치 호스트가 v0.1.11이라 호스트 측 변경(사다리·생략 기본값)의
  라이브 검증은 dev 호스트(`tools/dev-host-macos.zsh`) 빌드 후 별도 수행한다.

## 남은 항목

- RTVC 프리셋 인코더 콜백 p50 약 13ms가 상시 출력을 ~54fps에 묶는 배경 한계
  (dip과 무관). docs/11-low-latency-investigation H-01 계열 후속.
- `AppliedUdpStability.fallback_reason`이 뷰어 상태에 노출되지 않아 생략
  폴백이 조용히 일어난다. 호스트 로그 노출 후속 후보.
