# 가상 화면 기능 제거 및 장시간 사용 느려짐 수정 검증 (2026-09-07)

사용자 요청: (1) 가상 화면(가상 디스플레이) 기능을 모두 제거한다. (2) 계속 사용하면 느려지는 원인을 확인한다. 실제 Samsung Galaxy XR 뷰어 연결에서 검증하며, 특히 UDP 버스트(전송) 경로를 본다.

## 1. 제거 범위

| 계층 | 제거한 것 |
| --- | --- |
| Host Rust | `virtual_display.rs`, `display_management.rs`, `display_matching.rs`, `clamshell_mode.rs`, `provider.rs`, `power_assertion.rs` 모듈과 lib.rs의 Tauri 명령 10개(`create_virtual_display`, `remove_virtual_display`, `tablet_display_start/stop/status`, `list/add/remove_managed_display`, `set_managed_display_position`, `resize_managed_display`), control.rs의 `resizeVirtualDisplay` dispatch·`prepare_viewer_display`·관리 레지스트리, ffi.rs의 관리 모드 FFI 3개 |
| 계약 | `StartStreamInput.viewer_display`/`virtual_display_id`, `ViewerDisplayMetricsMsg`, `ResizeVirtualDisplayInput/Output`와 관련 테스트. 생성물(`packages/control-generated`)은 해당 타입이 없어 무변경 (`rustra:generate` 재실행로 확인) |
| Host UI | `DisplayManagerCard.tsx`, App.tsx의 가상 화면 섹션/토글 상태, i18n 키 42개, `virtual-screen`/`virtual-session` CSS 블록 |
| Viewer | 카탈로그 "가상 화면 크기" 카드를 "화면 해상도"(세션 해상도 전환 + XR 창 비율만 유지)로 단순화, `resizeVirtualDisplay` 제어 경로·`viewerDisplay`/`virtualDisplayId` 인자·`getDisplayMetrics` Kotlin 메서드 제거 |
| 도구 | `tools/cgvd-spark/`, `tools/cgvd-shim/`(78MB), `dev-host-macos.zsh`의 CGVD 빌드/검증, `tauri.macos.conf.json`의 CGVD 리소스 번들 |

- 세션 해상도 전환(1080p/1440p/4K/직접 입력, `reconfigureStream`)과 XR 창 비율 프리셋은 그대로 유지했다.
- 구버전 뷰어가 보내는 `viewerDisplay`/`virtualDisplayId` 필드는 serde 기본 동작(알 수 없는 필드 무시)으로 무해하다.

## 2. 장시간 사용 느려짐 — 원인

버스트/UDP pacing·ABR·회복 경로 전수 감사(큐·맵·타이머 모두 상한 존재, 프레임 번호 wraparound 처리 정상, `StreamActivity` holder 누적은 선행 수정으로 해결됨) 결과, **무한 증가 자료구조는 없다**. 느려짐은 세 제어 루프가 한 방향으로만 잠기는 래칫이었다.

### 라이브 증거 (Galaxy XR 실연결, 2026-09-07 22:56–23:11, 848표본)

`tools/stream-stats.py` JSONL: `artifacts/virtual-display-removal-2026-09-07/live-degradation-log.jsonl`

- 15분 동안 `currentBitrate` 7.7Mbps에 **완전 고정**(복구 불가). 회복 IDR 13→52, `captureQueueDropped` 1981→6246(+4.7/s).
- encode/rendered fps 중앙값은 58/56으로 건강하나 **20% 구간이 30fps 미만으로 붕괴**(최저 0–1fps).
- RTT 중앙값 19ms(건강)지만 27% 구간 ≥50ms 스파이크(최대 332ms), wire ≥40ms 12%.

### 원인 1 — 혼잡 판정이 "AU 전체 syscall 합계"를 봄 (버스트 부분 핵심)

`CaptureSession+UdpPacket.swift`의 `sendSyscallUs`는 한 프레임의 모든 데이터그램 sendto 시간을 **더한다**. 165조각 4K 프레임은 조각당 ~0.1ms로 건강해도 합계 ~15ms가 되어 `lastSendBlockUs > 8ms` 혼잡 투표가 **대형 프레임마다 항상 발동**했다. 실측 `sendBlockP95Us=14842`가 정확히 이 오판치다.

### 원인 2 — 높기만 한 지연이 상시 혼잡으로 투표

`CaptureSession+AdaptiveBitrate.swift`는 `RTT≥50ms || wire≥40ms`를 매 창구 혼잡으로 처리했다. 스파이크가 8창구 연속 무혼잡보다 자주 오면 `stableBitrateWindows`가 8에 도달하지 못해 **인하만 가능하고 복구는 영원히 불가**. Wi-Fi 절전 baseline RTT가 높은 XR에서는 절단이 지연을 낫게 하지도 않는데 바닥에 잠긴다(라이브: RTT 205ms에서 15분 고정).

### 원인 3 — 품질 힌트 1회 급락 + 완벽 창구만 복구

`EncoderPolicy.adaptiveEncoderQualityHint`는 손실 1회에 0.50→0.25로 즉시 바닥(비트레이트 ×0.55)이고, 복구는 `renderedFps >= targetFps` **정확히** 등 조건의 완벽 창구만 허용해 55–59fps 뷰어는 영구히 저품질에 머물렀다.

### 결과 증상

북마크: 붕괴된 저비트레이트에서 IDR(최대 225KB)을 제한된 pace로 보내는 동안 대기열 넘침 → 프레임 폭기 → 회복 IDR 재요청(2.6회/분) → 손실·지연 증가 → 더 깊은 절단. 붕괴 구간 20%와 회복 누적이 이 루프의 관측치다.

## 3. 수정

1. **전송 블로킹 측정을 데이터그램 단위로** (`CaptureSession+UdpPacket.swift`): 혼잡 투표 `lastSendBlockUs`를 **한 데이터그램 최대 sendto 시간**으로 변경. AU 합계는 p95 통계용으로 유지. 건강한 대형 프레임은 더 이상 혼잡으로 오판되지 않는다.
2. **지연은 악화될 때만 혼잡** (`CaptureSession+AdaptiveBitrate.swift` + `CaptureSession.swift`): 직전 창구 대비 RTT/wire가 +30ms 이상 **악화**되는 경우만 혼잡 투표. 높지만 안정/개선되는 baseline은 투표하지 않아 복구 인상이 다시 가능하다(과잉 인상 시 악화·손실 신호가 즉시 되돌린다).
3. **품질 힌트 점진 하락 + 복원 밴드 완화** (`EncoderPolicy.swift`): 하락 단계 -0.25→-0.10, 복구 조건을 목표의 90% 밴드(과부하 판정과 동일 기준)로 완화.

`dev-host-macos.zsh`는 옛 리소스(`cgvd-shim`)가 설치 묶음에 남아 codesign 엄격 검증이 실패하는 문제를 고쳤다(설치 전 기존 번들 삭제 — 화면 녹화 승인은 동일 지정 요구사항을 따르므로 유지된다).

## 4. 검증 상태

| 항목 | 결과 |
| --- | --- |
| `bun run typecheck` / `bun run test` / `test:contract` / `test:architecture` | 통과 (462 + 4, 아키텍처 클린) |
| `cargo test --workspace` / `cargo clippy --workspace --tests -- -D warnings` | 통과 |
| `cargo run -p control-contract --bin generate` | 재생성 무변경 |
| React Doctor | 100/100 |
| Swift 심(policy/adaptive/split/cursor 테스트 + library 빌드) | 통과 (종료 코드 직접 확인) |
| 뷰어 Gradle `:app:testReleaseUnitTest` / `:app:assembleRelease` | 통과 (ANDROID_HOME 필요) |

> 테스트 정직성 기록: 첫 Swift 테스트 실행은 파이프(`\| tail`)로 종료 코드가 가려진 거짓 통과였고, 이후 종료 코드 직접 확인에서 품질 힌트 테스트 기대값 오류(50fps 케이스는 복구 불가가 아니라 점진 절단 경로)를 발견해 바로잡고 재통과했다. 구현이 아닌 테스트 기대값 오류였다. 설치된 Host의 dylib는 관리 화면 모드 죽은 코드 제거 전 빌드다(동작 차이 없음; 해당 심볼은 이미 Rust 호출부가 없는 dead code).

### 실기기 잔여 확인 (호스트 재설치 후 사용자가 XR에서 수행)

1. XR에서 기존 스트림이 끊어졌으므로 카탈로그에서 화면을 다시 연다(구버전 뷰어 앱 그대로 호환됨). **2026-09-07 23:24 XR이 재연결했고 아래 초기 관측을 확보했다.**
2. `python3 tools/stream-stats.py`로 15분 이상 관찰: `cap`(currentBitrate Mbps)이 7.7에 갇히지 않고 상승·유지되는지, `recov`(회복 누적) 증가 속도가 둔화되는지, 30fps 미만 붕괴 구간이 사라지는지.
3. 구버전 뷰어의 "가상 화면 크기" 카드는 이제 오류를 낸다(예상 동작). 새 APK 설치 후 "화면 해상도" 카드로 대체된다.

### 수정 후 초기 관측 (2026-09-07 23:24, Galaxy XR 재연결 세션, 새 Host 빌드)

| 지표 | 수정 전 (래칫 발동 상태) | 수정 직후 (~1분) |
| --- | --- | --- |
| currentBitrate | 7.74 Mbps (15분+ 고정) | **30.97 Mbps** |
| sendBlockP95Us | 14,842 (AU 합계 오판) | **652** (데이터그램 단위) |
| receiverRttMs / wireMs | 205 / 32 | **12 / 3** |
| recoveryKeyframes | 63+ (2.6회/분 증가) | **1** (최초 키프레임) |
| captureQueueDropped | 6,246 (+4.7/s) | **5** |
| encodeOutputP95Us / encodeSubmitCallP95Us | 241,983 / 296,035 | **10,745 / 11,254** |
| encode·rendered fps | 중앙 58/56, 20% 구간 <30fps | **59–61 / 59–61 안정** |

인코더 콜백 p95(242ms→10.7ms)도 함께 정상화됐다. 즉 구버전의 `rateControl`(rtvc) 인코더 "정체"는 인코더 자체 문제가 아니라 **전송 큐 역압의 증상**이었다 — 비트레이트가 바닥에서 IDR이 제한된 속도로 길게 흐르는 동안 네트워크 큐가 막히고 인코더 출력 콜백가 뒤에서 대기한 것이다. 아래 "미해결 후보" 중 인코더 항목은 이 관측으로 약화되었다(장시간 추이로 최종 판단).

15분 연속 로그: `artifacts/virtual-display-removal-2026-09-07/post-fix-15min-log.jsonl`.

### 수정 전후 15분 정량 비교 (같은 Galaxy XR, 같은 2560×1440@60)

| 지표 | 수정 전 15분 (래칫 발동) | 수정 후 15분 |
| --- | --- | --- |
| encode fps | 중앙 58, **20% 구간 <30fps**, 최저 0 | 중앙 60, **0% (<30fps 없음)**, 최저 59 |
| rendered fps | 중앙 56, 최저 1 | 중앙 60, 최저 54 |
| recoveryKeyframes 증가 | +39 (2.6회/분) | **+1** |
| captureQueueDropped 증가 | +4,265 (4.7/s) | **+1** |
| currentBitrate | 7.7 Mbps 고정(상승 불가) | **31.0 Mbps 안정 유지** |
| RTT / wire | 중앙 19/7ms, 스파이크 332/423ms | 중앙 10/5ms, **최대 24/27ms** |

**해석의 한계**: 수정 전 세션은 수 시간 사용 후(래칫이 이미 발동)이고, 수정 후 관측은 재연결 직후 15분이다. 구세션도 초기엔 건강했으므로, "장시간 사용에도 다시는 내려가지 않는다"의 최종 증명은 수 시간 실사용이 필요하다. 다만 수정 전 로그에서 보인 병리(건강한 중앙값에도 비트레이트가 상승 불능으로 갇힘)의 원인 메커니즘을 직접 제거했고, 수정 후 창구에서 해당 신호(오판 혼잡 투표)가 더 이상 발생하지 않음을 확인했다.

### 미해결 후보 (별도 조사 필요)

- `maxSendBlockUs` 3.75s 단발 관측(논블로킹 소켓이라 원인 미상 — 시스템 레벨 정체 가능성).
- 장시간(수 시간) 사용 시 비트레이트·fps 유지 여부 — 이번 관측은 재연결 직후 기준이다.

## 6. "갑자기 4fps" 추가 조사 (2026-09-08 00:00, 사용자 후속 보고)

사용자 보고 "처음엔 괜찮다가 갑자기 4fps". 0.5초 간격 감시(`sudden-collapse-watch.jsonl`, 4.3분)로 에피소드를 실시간 포착했다.

### 관측된 에피소드 해부

| 구간 | 지속 | 증상 |
| --- | --- | --- |
| 23:57:15–45 | 30s | enc 4–30fps, RTT 중앙 102/최대 317ms, wire 최대 288ms, 비트레이트 31→7.7Mbps (ABR 정상 절단) |
| 23:59:28–00:00:14 | ~46s | enc≈24–26fps — **정상**: 화면 변화량 기반 캡처(cgDisplayStream)라 저움직임 구간은 캡처 자체가 24fps. 건강(RTT 9–11ms) |
| 00:00:15–51 | 36s | **enc 최저 2–3fps** — RTT 중앙 119/최대 330ms, wire 최대 345ms, 회복 +1뿐(IDR 루프 아님) |
| 00:01:44–00:02:18 | 34s | enc 5–18fps, RTT 91–332ms 재발 |

### 원인 (측정으로 확정)

- **링크 자체 용량 붕괴**: RTT가 9ms→100–330ms, wire 5ms→345ms. LAN에서 RTT 300ms는 우선순위 큐잉이 아니라 전파 시간 자체가 남지 않는 상태(간섭/거리/헤드셋 Wi-Fi 절전 중 어느 것이든 호스트에서는 구분 불가).
- **느린 sendto가 파이프라인을 얼림**: 저하 구간에 개별 논블로킹 `sendto`가 13–80ms씩 걸리고(누적 최대 2.24초 1회), `udpSendFailures=0` — 버퍼 만석(EAGAIN)이 아니라 **"느리게 성공"만** 한다. 작은 AU(5–12KB) 하나를 보내는데 100–700ms. 직렬 네트워크 큐가 이 뒤에 밀려 인코더·캡처까지 역압 → enc 2–4fps.
- 실제 전송량 1.4Mbps ≪ pacing 목표 5Mbps — pacing이 느린 게 아니라(간격 상한 4ms) syscall이 느린 것.
- 에피소드 후 회복은 정상(비트레이트 4.5→20Mbps, RTT→12ms) — 1차 래칫 수정이 회복 경로를 되살린 것 확인.

### 수정 — AU 전송 마감 (`UdpPacingPolicy.swift` + `CaptureSession+UdpPacket.swift`)

느리게 성공하는 전송 뒤에 큐가 얼지 않도록, AU 단위 전송에 벽시계 예산을 둔다:

- 델타 AU: 프레임 예산 × 2(60fps에서 33.3ms, 하한 20ms). 초과 시 나머지 조각 전송을 중단하고 기존 실패 경로(스테일 체인 폭기 + 회복 IDR 요청)로 전환한다. 인코더·캡처는 계속 돈다.
- 회복 키프레임: 750ms(IDR 재시도 주기와 동일). 165조각 4K IDR의 합법적 pacing(~160ms@24Mbps)은 중단하지 않는다.

효과: 링크 붕괴 중 "파이프라인 동결 + 붕괴 구간 누적"이 "스테일 프레임 폭기 + 링크 회복 즉시 재개"로 바뀐다. 링크가 나쁜 동안 화질/프레임이 낮아지는 것 자체는 물리적으로 불가피하다.

### 1차 시행의 과보정과 2차 수정 (2026-09-08 00:11–00:25)

평면 예산(2프레임 = 60fps에서 33ms)을 깔았더니 **과보정**이 확인됐다: 호스트 파이프라인은 더 이상 얼지 않았으나(enc 56–60fps, RTT 9–11ms 유지 — 동결 해결 자체는 성공), 움직임 많은 큰 델타 AU(20–57KB)는 합법적인 pacing만으로 28–44ms가 걸려 마감이 계속 발동, 10분간 폭기 7,064개·회복 IDR 300개의 자해성 폭풍이 생기고 ABR이 이를 혼잡으로 읽어 21→8Mbps 재절단, 렌더가 다시 6fps까지 떨어졌다. 사용자 보고("붕괴가 멈추지 않는다")가 정확했다.

2차 수정(`UdpPacingPolicy.swift`): 마감을 **AU 크기 인지**로 변경 — `max(프레임 예산×2, pacing 구간 수×4ms×2 + 20ms)`. 작은 AU(7조각)는 36ms, 큰 AU(43조각)는 108ms. 검증: Swift 4종 통과. 재설치 후 재감시로 (a) 회복 IDR 폭풍 소멸, (b) 진짜 링크 열화 시에만 마감 발동, (c) 열화 중에도 enc 유지를 확인한다.

### 3차 반복 — 상승 상한(절단 기억) (2026-09-08 00:30–00:45)

v2 관측(00:28–00:32): 진짜 링크 열화(RTT≥60ms) 창구는 00:29:02–15 한 번뿐이었으나, 회복 후에도 비트레이트가 8Mbps 바닥에 갇히고 폭기가 0.5/s로 이어져 상승 창구가 리셋 — **상승→붕괴→절단 톱니**의 구조적 문제가 남았다. 붕괴의 트리거(움직임 많은 앱 사용 중 colorsync 40%·WindowServer 58% CPU 관측)와 결합하면: 큰 프레임 → 비트레이트 21–31Mbps 상승 → 한계 링크 붕괴 → 절단 → 회복 → 다시 상승 → 반복.

수정(`EncoderPolicy.swift` + `CaptureSession+AdaptiveBitrate.swift`): **절단 기억**. 확정 혼잡 절단 시 그때의 실패 비트레이트 90%를 상승 상한으로 기록하고, 인상(일반·고움직임 모두)은 상한까지만 오르며, 30창구(30초) 연속 무혼잡마다 상한을 10%씩 완화(정책 상한 이하). 같은 벽에 반복해서 부딪히는 톱니를 끊는다. 검증: Swift 4종 통과(신규 순수 함수 `adaptiveRaiseCeilingAfterCongestion`·`nextAdaptiveRaiseCeiling` 단정 포함).

검증: Swift 4종 테스트 + library 빌드 통과(종료 코드 직접 확인). 새 Host 설치 후 에피소드 재발 시 `AU send deadline exceeded` 로그와 enc fps 유지 여부로 확인한다.

### 4차 반복 — 뷰어 렌더 스톨 창구의 폭기 혼잡 투표 제외 + USB 키보드 사고 (2026-09-08 00:45–00:55)

**렌더 스톨 게이트**: 대시보드 접근 테스트 중 발견 — 링크(RTT 9–13ms)와 호스트(enc 56–60)가 건강해도 뷰어 렌더만 3–8초 멈추는 창구가 반복되고(앱 접근/Surface 재생성 시 XR 뷰어의 렌더 파이프라인 스톨; 대시보드를 열기 전에도 발생), 그동안의 로컬 폭기가 혼잡으로 투표돼 절단이 누적됐다. `receiverRenderedFps == nil`(뷰어가 0 렌더 보고)인 창구의 폭기는 혼잡 투표에서 제외했다(`CaptureSession+AdaptiveBitrate.swift`). 실링크 붕괴는 여전히 손실·악화 지연 투표로 절단된다.

**USB 키보드/마우스 끊김 (사용자 보고)**: `handshake_candidate`가 인터페이스가 있는 **모든** USB 장치에 AOAP 핸드셰이크를 시도했다(open + interface claim + vendor 제어 전송). `auto` 전송 스트림 시작마다 뷰어가 `requestUsb`를 호출하므로, 이 밤의 잦은 재연결마다 키보드(Lofree Flow2)·마우스 동글·허브·RTL9210B NVMe 브리지가 스윕 대상이 됐다. macOS에서 interface claim은 활성 드라이버를 떼므로 HID 장치가 재연결되고, 허브 뒤 장치들이 함께 재열거된다. 수정: Android 형태 인터페이스(0xFF vendor-specific/0x06 MTP/0xE0 wireless)가 있는 장치만 시도(`aoap.rs interface_class_suggests_android`, 회귀 테스트 추가). HID(0x03)·허브(0x09)·대용량저장(0x08) 등은 절대 open/claim하지 않는다.

정직성 기록: 수정 후 단발 `requestUsb` 재현 시험과 3분 USB 감시(호스트 재시작 포함)에서는 키보드 끊김을 재현하지 못했다 — claim이 HID에서 실패로 끝나는 경우도 있는 듯하다. 그래도 스윕이 비-Android 장치를 열지 않는 것은 올바른 방어 수정이며 설치 완료(00:52). 끊김이 지속되면 다음 의심은 하드웨어(NVMe 브리지가 공유 허브를 재설정)이고, 브리지 분리 시험으로 가린다.

## 8. 최종 상태 (2026-09-08 01:15)

### 최종 스택 검증 (새 Host 00:52 + 새 뷰어 APK 태블릿 설치 00:57, 14분 관측)

| 지표 | 수정 전(래칫 발동 15분) | 구뷰어+새호스트 | **최종 스택** |
| --- | --- | --- | --- |
| 30fps 미만 붕괴 구간 | 20% | ~10% | **1.8% (세션 시작 워밍업뿐)** |
| enc fps 중앙/최저 | 58 / 0 | 59 / 0 | 58 / 0 |
| rend fps 중앙/최저 | 56 / 1 | 50 / 2 | 39 / 1 |
| RTT 중앙/최대 | 19 / 332ms | 12 / 627ms | 12 / 45ms |

- 회복 IDR 88회/14분(≈6/분, 시작기 제외하면 더 낮음), captureQueueDropped 1.5/s(고움직임 영상 재생 중 인코더 처리량 한계, 2.5%).
- rend 중앙 39: 태블릿 디코더/패널 렌더 페이싱으로 추정 — 갭·붕괴는 사실상 소멸했고 체감 개선은 사용자 판단 필요.
- USB: 최종 호스트에서 키보드·마우스 재열거 **0회**(6분+ 0.15~0.2s 감시 2회). 사용자가 여전히 키 입력 씹힘을 보고했으므로(01:0x) USB 레벨이 아닌 시스템 부하(WindowServer 58%+colorsyncd 42% 화면 영상 재생+빌드) 또는 하드웨어(NVMe 브리지/허브) 여지가 남아 있다 — 비디오 정지 상태에서 재시험 권고.

설치된 Host에 반영된 것: 가상 화면 제거, ABR 래칫 3종 수정, AU 전송 마감(크기 인지), 상승 상한(절단 기억), 렌더 스톨 게이트, USB AOAP 필터. 게이트: cargo test/clippy 70 통과, Swift 4종 통과(종료 코드 직접 확인), React Doctor 100/100, TS 462/계약/아키텍처 통과. 태블릿 뷰어 APK `artifacts/leftcar-viewer-virtual-display-removed-0b7f21e.apk` 설치 완료(00:57, adb 192.168.0.19:5555). XR은 같은 APK로 사용자 설치 권고.

## 7. 상태 요약 (2026-09-08 00:1x)

- 커밋 전 상태. 변경 묶음: 가상 화면 제거(45파일 −7,112라인) + ABR/품질/전송측정 3종 수정 + AU 전송 마감.
- 최종 호스트(전송 마감 포함) 설치 후 XR 재연결 대기 중. 에피소드 재발 시 감시 로그로 마감 동작 확인 필요.

## 5. 커밋 상태

아직 커밋하지 않았다. 사용자 확인 후 기존 6개 묶음 계획(`artifacts/responsive-streaming-2026-09-07/commit-plan.md`)에 이 변경들을 반영해 분리 커밋을 제안한다.
