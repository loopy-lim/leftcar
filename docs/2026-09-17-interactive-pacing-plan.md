# 인터랙티브 프레임 페이싱 개선 계획 — 1440p60 단일 스트림

작성일: 2026-09-17. 범위: physical display → Android 단일 스트림의 frame pacing·stall·recovery 개선. Virtual Display / 프로토콜 교체 / 신규 UI는 포함하지 않는다.

선행: [1440p60 저지연 개선 조사](2026-09-16-latency-improvement-research.md), [전송 지연 수정](2026-09-16-latency-remediation.md), [전송 시간 예산 수정](2026-09-16-udp-frame-budget-improvement.md), [성능 수락 기록](2026-09-16-performance-acceptance.md), [성능 리뷰](2026-09-11-performance-review.md).

이 문서는 (1) 브리프 가설과 현재 트리 코드의 대조 근거, (2) 이번에 추가한 실험 인프라, (3) 다음 실기 실험 순서를 확정한다. 측정 우선순위는 low latency > stable pacing > 1% low > 지속 60fps > 화질.

## 1. 질문과 판정 요약

**"평균 처리량은 충분한데 왜 single-stream이 Parsec만큼 부드럽지 않은가?"**

판정: **인코더가 아니다. 호스트 전송 큐·pacing·복구 에피소드가 1차 원인이다.** 1440p에서 하드웨어 인코더(RTVC)는 고립 시험에서 59.5fps / callback p95 11.6ms로 여유가 있고, 캡처도 59.93fps를 낸다. 반면 전송 대기열의 가장 오래된 프레임은 p95 67–70ms / 최대 118ms까지 쌓였고(8프레임 × 16.67ms ≈ 133ms 구조 상한), 복구 시에는 9 AU 건너뛰기(recoverySkip)로 127→35ms를 따라잡는 가시적 stall이 관측됐다. 4K 단일 RTVC 44.96fps 실측은 브리프가 기억한 "4K에서 44fps로 떨어지는 1초"와 일치하며, 이는 처리량 한계가 맞다 — 하지만 1440p baseline의 부드러움 문제와는 다른 계층이다.

## 2. 근거 — 브리프 가설 × 현재 코드

| 브리프 가설 | 현재 코드 사실 (file:line) | 실측 | 판정 |
| --- | --- | --- | --- |
| P0 capture latest-frame slot, encoder가 못 따라가면 drop | 단일 경로 pending 슬롯 1개, latest-wins. 교체 시 `captureQueueDropped++` (`CaptureSession+PendingCapture.swift:12-26`, `CaptureSession+Encode.swift:117-121`). SCK 큐 depth는 split 8 / 일반 3 (`EncoderPolicy.swift:242-250`) | 캡처 59.93fps, `captureQueueDropped` 병목 흔적 없음 | 구조는 가설대로. 단 1440p에서는 drop 원인이 인코더가 아니라 전송 막힘이었음 |
| P0-1 in-flight 1/2/3 A/B 필요 | RTVC는 컴파일타임 고정: ≥1440p=3, 미만=2 (`EncoderPolicy.swift:342-363`). **knob 없음** → A/B 불가. split만 5 (`DualEncoderPipeline.swift:4-6`). `MaxFrameDelayCount`는 정책 필드만 있고 세션에 적용 안 됨(죽은 필드, `EncoderSetupAttempt.swift` 미적용). RTVC는 옵셔널 튜닝 속성 0개 | AVE 합성: in-flight 2→54.7 / 3→59.7 / 5→59.85fps. 4K RTVC는 in-flight 3에서 44.96fps+107/900 drop | **가설 유효, 실험 불가 상태였음** → 이번에 knob 추가 (§4) |
| P0-2 encoder input direct 우선, cpuCopy는 실패 취급 | 정책: RTVC+1440p+**CGDisplayStream** = `.cpuCopy`(프레임당 plane memcpy, `EncoderPolicy.swift:92-97`, `EncoderPolicy.swift:192-231`). 기본 백엔드는 ScreenCaptureKit이므로 직접 경로는 `.direct`. 단 경로 선택이 세션 문자열 1회 보고뿐 프레임별 카운터 없음, `inputPreparationP95Us`는 shim JSON에만 있고 **Rust 계층에서 드랍되어 getStatus에 없었음** | inputPreparation 분포 미관측 | **부분 일치**. cpuCopy는 프로덕션 기본 경로가 아니지만 잠재 위반. 프레임별 카운터 + P50/P95 getStatus 노출 추가 (§4) |
| P1 network queue 8프레임≈133ms, oldest age가 핵심 | interactive UDP 큐 = **8프레임** (video 모드만 3, `CaptureSession.swift:183-191`). byte 상한 없음. overflow 시 체인 전체 drop+IDR (`CaptureSession+NetworkQueue.swift:75-132`). **age 기반 drop/IDR 없음** — `pendingFrameOldestAgeUs`는 측정만 되고 adaptive-QP 안정 판정(`AdaptiveQpController.swift:20`)에만 사용 | S6: 큐 최대 8프레임/234KB, oldest age p95 67.12ms / 최대 117.90ms, 181표본 중 16개가 >33.33ms | **가설 정확히 일치 — 1차 병목**. age valve 실험 스위치 추가 (§4) |
| P1-1 SO_SNDBUF 512KiB | 정확히 512KiB 하드코딩 (`CaptureSession+Setup.swift:345-346`). knob 없음. EAGAIN/ENOBUFS는 `udpSendFailures`로만 합산 | 미 A/B | **일치**. A/B knob 추가 (§4) |
| P1-2 pacing이 16.67ms 예산 안에 못 보내는지 | usleep 기반 pacer에 **지각 누적 버그**였음(깬 시각 기준 재가산) → `deadline+interval`로 수정 + byte 정확 `UdpFramePacingBudget`(프레임 예산 80% 예약, 120Mbps 상한) — 커밋 c3d4d47. AU deadline 초과 시 나머지를 즉시 보내는 게 아니라 **AU 중단+큐 flush+10s degraded+IDR** (`CaptureSession+UdpPacket.swift:241-257,327-344`) | 수정 전 frame 평균 18.33ms→누적 폭주(시작 지연 p95 최대 524ms), 수정 후 15.40ms/9–15ms (loopback 재현 3회). B2 실기: 큐 대기 p95 69.75ms / 전송 처리 p95 19.005ms. C1(custom burst 16, 2배율): 큐 대기 p95 0.068ms — 단 실험 무효화 | **원인 확인+수정 완료, 실기 재검증 대기**. burst 수요가 남는다(19ms>16.67ms) |
| P2 recovery storm | overflow/deadline → IDR. 약 링크 IDR pacing 캡·2.5s cooldown·10s degraded (c3d4d47). viewer는 gap→freeze+IDR(250ms gate), recoverySkip catch-up. **미커밋 viewer 경화: NACK grace 25→120ms(RTT 15–96ms Wi-Fi에서 NACK 만료→IDR 폭풍 제거), 루프마다 출력 드레인, 3ms input 재시도** | 180초당 복구 2–10회. 복구 직전 수신 지연 127/129ms → recoverySkip 9 AU → 35ms 복귀. XR 추적: rkf≈1/s + networkQueueDropped 증가 폭풍 | **메커니즘 일치**. 브리프 루프(burst→overflow→체인 폐기→IDR→대형 burst)의 각 단계가 코드에 존재하며 경화가 진행 중. 실기 검증이 1차 남은 작업 |
| P3 presentation cadence | freshness 모드 기본(최신 1장 즉시 release, 나머지 discard — `decoder.rs:551-568`). balanced(vsync 지정 release)는 707716f, 기본 off, 전문 설정 토글. 디코더 `c2.qti.hevc.decoder.low_latency` + `low-latency=1` 확인됨. **viewer 표시 간격 p50/p95/p99·최장 gap 미계측**(p50/p95 of age만 40표본) | S6 discard +298 = 디코더 출력 펌프 catch-up(지연 아님, 의도된 최신 프레임 정책). balanced는 bursty 링크에서 p50 58→53fps로 기본 유지 결정 | **계층 갭 확인**. cadence 계측이 P3 실험의 선행 조건 — 별도 배치로 연기(미커밋 트리 충돌 회피) |

인코더 1440p 비병목 근거: V3 고립 시험 RTVC 59.49fps / callback p95 11.61ms, 캡처 59.93fps. 반면 같은 실행에서 capture→Surface 스냅샷 p50 56ms / p95 150ms — 차이의 주범이 전송 큐 age(67–118ms)라는 것이 양측 정합.

## 3. 병목 서열

1. **호스트 전송 큐 + pacing 처리량** — 측정된 1위. interactive 8프레임 상한, age 정책 부재, (수정 전) 지각 누적. c3d4d47로 처리량 절반은 회복, 실기 재검증 + burst 수요 잔여(19ms) 처리 필요.
2. **복구 에피소드 빈도와 크기** — 180초당 2–10회, 각 에피소드는 recoverySkip catch-up stall을 동반. 미커밋 NACK grace 경화가 이 겨냥.
3. **표시 cadence 계측 부재** — viewer p99/최장 gap이 없어 P3 판정 자체가 불가. balanced on/off A/B도 계측 선행.
4. **인코더 세부** — 1440p는 여유. 단 in-flight A/B 불가, cpuCopy 잠재 경로, MaxFrameDelayCount 미적용은 브리프 P0 요건으로 실험 준비만 해둠.

## 4. 이번에 추가한 실험 인프라 (미커밋)

하나의 A/B가 정확히 하나의 변수만 바꾸도록 전부 기본값=기존 동작인 스위치로 추가했다. 게이트: shim `policy-test` 통과, 최적화 shim 빌드 통과, `cargo test -p control-contract` 20 passed, `cargo test --lib`(leftcar-host-desktop) 230 passed(신규 parse 테스트 포함).

| 스위치 | 범위/기본값 | 목적 |
| --- | --- | --- |
| `LEFTCAR_MAX_ENCODE_IN_FLIGHT` | 1–5 clamp, 미설정=기존(≥1440p 3) | P0-1 in-flight A/B |
| `LEFTCAR_QUEUE_MAX_AGE_MS` | ms, 0=off(기존) | P1 oldest-age valve: 초과 시 체인 drop+IDR(overflow와 동일 의미론, 큐에 복구 키프레임 보존) |
| `LEFTCAR_SO_SNDBUF_BYTES` | 64KiB–2MiB clamp, 미설정=512KiB | P1-1 kernel 큐 A/B |

새 getStatus 필드(`StatsInfo`, serde default로 구버전 shim 호환): `encoderMaxInFlightLimit`, `inputPreparationP50Us`, `inputPreparationP95Us`(Rust 노출 신규), `inputFramesDirect`/`inputFramesPixelTransfer`/`inputFramesCpuCopy`, `networkQueueAgeDropped`, `networkQueueMaxAgeMs`, `udpSendBufferBytes`.

기존 인프라 재활용: `LEFTCAR_FRAME_TRACE=1`(프레임별 전송 JSON, `artifacts/.../debug/summarize_trace.py`), `tools/stream-stats.py --log` JSONL, `tools/stream-gate.py`(counter-continuity 게이트 포함).

연기(명시): viewer 표시 간격 p50/p95/p99·최장 gap 계측 — `native/android-viewer`와 Kotlin stream 파일에 다른 세션의 미커밋 경화가 있어 충돌을 피하기 위해 별도 배치로 진행한다. LTR/frames-ACK는 복구가 여전히 1위 이후에만 검토.

## 5. 실험 계획 (브리프 Phase 2–7 매핑)

공통 고정: Lenovo TB710FU, 2560×1440@60, 동일 WebGL 고운동 장면(전 화면 변화), audio OFF, LAN UDP, 180초 이상, `stream-stats.py --log` + gate + (필요 시) FRAME_TRACE. 한 번에 한 변수. 순서를 바꿔가며 최소 반복. 태블릿 독점 확인이 각 실행의 선행 조건이다(지난 C1 무효화 사例).

| # | 실험 | 스위치/변수 | 판정 |
| --- | --- | --- | --- |
| E1 | 신규 baseline 2종 | (a) 현재 기본(auto, HEVC 우선), (b) H.264 RTVC 명시 — 브리프 명시 baseline | gate PASS(57fps≥90%, capture→Surface p95≤50ms, RTT p95≤20ms, gap/복구/송신실패 0) + frame trace 큐 대기 p95. c3d4d47 후 기준선 확립이 목적 |
| E2 | in-flight A/B | `LEFTCAR_MAX_ENCODE_IN_FLIGHT` = 1/2/3 | fps가 아니라: captureQueueDropped, encoderCallbackP95, capture→Surface p95, 프레임 간격 분포. 예: 2에서 fps 유지+지연 감소면 채택 |
| E3 | 입력 경로 확인 | 스위치 없음 — `inputFrames*` 카운터 + `inputPreparationP50/P95` 관측 | SCK 기본에서 direct 확인. cpuCopy가 관측되면( CG 백엔드) direct 전환 별도 실험으로 승격. p50이 수 ms면 경로 자체가 의심 |
| E4 | 큐 age valve | `LEFTCAR_QUEUE_MAX_AGE_MS` = 33 (vs off) | oldest age p95/최대, networkQueueAgeDropped, recoveryKeyframes, frame gap. valve가 복구를 늘리면 채택 금지 |
| E5 | SO_SNDBUF | `LEFTCAR_SO_SNDBUF_BYTES` = 128KiB (vs 512KiB) | udpSendFailures, gap, 복구, oldest age. E4와 순차(동시 변경 금지) |
| E6 | burst 수요 잔여 | auto vs custom burst 16(기존 UI 경로) — B2 재현 후 | 큐 대기 p95 <10ms 달성 여부. 무선 손실/RTT/복구 증가 시 기각. C1의 유효 재현 |
| E7 | 복구 경화 검증 | 미커밋 viewer 빌드(NACK grace 120ms 등) on/off 비교 불가 — 채택 판정 | recoveryKeyframes/에피소드당 recoverySkip AU 수/최장 gap 감소. 이후 cadence 계측 배치 + balanced A/B(Phase 6) |
| E8 | sustained | 채택 구성으로 10/30분 | latency creep(p50+16ms 이하), 발열, 메모리, 복구 빈도 |

Phase 8(VirtualDisplayProvider)은 E8 통과 전까지 금지 — 기존 합의 유지.

## 6. 합격 기준

브리프 최종 기준을 그대로 채택하되, 기존 gate(2026-09-16 수락 기준)와 병기한다 — 어느 하나를 다른 하나로 대체하지 않는다.

- encode output / viewer present 평균 ≥59fps, 1% low ≥55 목표
- 50ms 초과 unexplained stall 최소화, 100ms+ 반복 금지 — `stream-gate` 미지원 항목이므로 FRAME_TRACE + 신규 cadence 계측으로 보강
- capture→encode p95 ≤20ms(기존: capture→Surface 스냅샷 p95 ≤50ms)
- glass-to-glass p50 ≤50ms / p95 ≤80ms — 물리 광학 미측, capture→Surface+present 추정으로 명시
- RTT p95 ≤20ms(LAN), 복구·gap·송신 실패·비트레이트 하한 압박 0(기존 gate와 동일)
- CPU encoder fallback 0, 메모리·발열 sustained 이상 없음

## 7. 다음 단계

1. 태블릿 독점 확인 → E1 baseline 2종(이것이 c3d4d47·UdpFramePacingBudget의 첫 유효 실기 재검증이다).
2. E1 결과에서 큐 대기가 사라졌는지 확인 → 남는 지연의 소유자(디코더/표시) 판정.
3. 미커밋 viewer 경화 + 이번 스위치를 함께 실기 검증한 뒤, 사용자 확인을 받아 커밋 분할(스위치 인프라 / viewer 경화 / 계획 문서).
4. viewer cadence 계측 배치를 별도 세션 트리 정리 후 착수.
