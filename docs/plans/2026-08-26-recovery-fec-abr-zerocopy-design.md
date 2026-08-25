# 설계: 프레임 드랍 원인 제거 — 회복 정책 재설계 + FEC/ABR + Windows 제로카피

작성일: 2026-08-26
상태: 승인됨 (접근법 A, 2026-08-26 대화)
관련 리서치: `docs/research/2026-08-25_remote-screen-display-pipelines.md` (외부 원격 프로그램 화면 표시 방식)
관련 ADR: ADR-0004 (전송 bake-off 전 확정 금지 — 본 설계는 위반하지 않음: FEC는 전송 위 계층)
기준 커밋: `d6b0161` (feat/rn-tauri-rebuild)
병행: 사용자가 실측 스파이크(stale 연쇄 확인 계측)를 별도 진행 — 결과는 미해결 항목 판정에 반영

## 문제

macOS Host 세션에서 관측된 심각한 프레임 드랍(E15 표본: 55 FPS, SKIP 461, LOSS 32)의 본체는 캡처/인코딩 GPU 경로가 아니라 **Viewer의 프레임 회복 정책이 꼬리 지연을 프레임 폭포로 증폭**시키는 구조다.

근거 (코드 조사, 2026-08-25/26):
- Host는 60 FPS로 인코딩 중, capture→encode p95 약 19ms, queue wait p95 0.7ms, send block p95 2ms, pendingFrame=0 — 호스트는 정시 생산·송신
- Viewer SKIP = `stale_outputs`(`native/android-viewer/src/jni.rs:796`)은 ① 입력 stale drop(`capture_age > 80ms+RTT/2, 상한 200ms` 델타 폐기, `media_datagram.rs:42`, `jni.rs:1368-1378`) + ② 출력 burst 폐기(3개 초과 시 `render=false`, `viewer-decoder/src/lib.rs:660-695`)의 합
- 3분 세션의 부족분 ~900프레임 중 SKIP 461이 지배, 진짜 네트워크 유실은 32
- 증폭 루프: stale 프레임 1개 → `awaiting_keyframe=true` → IDR 도착까지 모든 델타 폐기 → UDP GOP가 `max(1, fps)`(60fps→1초 1회, `CaptureShim.swift:1922-1928`)이므로 최대 1초 연쇄 + IDR 요청 debounce 750ms(`media_datagram.rs:36`)가 추가 지연

외부 벤치마크(리서치 문서): Moonlight/Parsec은 같은 문제를 FEC(재전송 없는 손실 복구) + 무한 GOP(재동기화 대기 부재)로 회피한다. Leftcar의 macOS 캡처→인코더 경로는 이미 IOSurface 제로카피(`CaptureShim.swift:1292→1647`)로 이 계열과 동등하며, 제로카피 격차는 Windows WGC의 CPU readback(`apps/host-desktop/src-tauri/src/windows_backend/capture.rs:111-144`)에만 존재한다.

## 목표

1. 꼬리 지연 1프레임이 프레임 폭포로 증폭되지 않게 한다 (SKIP 두 자릿수, 59-60fps 유지, IDR 후 복구 ~50ms)
2. 유실 32개 수준의 가벼운 손실이 IDR 재요청 없이 복구되게 한다 (FEC)
3. 네트워크/뷰어 상태가 Host 비트레이트에 적응하게 한다 (ABR)
4. Windows 캡처→인코더 경로의 GPU 상주화로 매 프레임 CPU 복사·색변환을 제거한다 (제로카피)
5. 라이선스는 MIT 유지 — GPL-3.0 코드(Sunshine, moonlight-common-c)는 복사 금지, 사양 참조만. 직접 사용은 MIT 컴포넌트(nanors 등)만

## 설계

### 파트 1 — 회복 정책 재설계 (원인 제거, 최우선)

**1a. 무한 GOP + IDR on-request**
- `CaptureShim.swift`의 UDP GOP `max(1, fps)`를 `MaxKeyFrameInterval = 3600`(사실상 무한)으로 변경. TCP 경로는 기존 `fps*60` 유지
- 회복은 기존 인증 `IDR` 요청 경로(`request_idr` → `request_idr_debounced` → Host `kVTEncodeFrameOptionKey_ForceKeyFrame`, `CaptureShim.swift:1643`)로만 수행
- 새 Surface/중간 참여의 초기 IDR(`jni.rs:967`)는 기존 동작 유지 — 정기 IDR 제거의 영향 없음
- 위험: IDR 요청 데이터그램 자체가 유실되면 회복 불가 — debounce 단축(1c)과 FEC(파트 2)가 상호 보완. 최악의 경우 다음 요청(debounce 후)이 회복

**1b. stale 판정 히스테리시스**
- 현재: `!keyframe && capture_age > budget` 즉시 폐기+재동기화
- 변경: **연속 K=3프레임 초과 시에만** `awaiting_keyframe` 진입. K-1 이하의 꼬리 지연 프레임은 늦게라도 렌더(디코더 투입)하여 연쇄 차단
- `stale_frame_budget_ms` 자체(80ms+RTT/2, 상한 200ms)는 유지 — 예산이 문제가 아니라 단일 샘플로 재동기화하는 것이 문제
- K와 예산은 상수로 두고 단위 테스트로 고정(측정 스파이크 결과로 조정 가능)

**1c. debounce 단축**
- `RECOVERY_REQUEST_COOLDOWN` 750ms → 250ms (RTT ~13ms 관측 기준 여유 충분)
- `RecoveryRequestGate` 로직(`media_datagram.rs:397-433`)은 변경 없이 상수만 교체, 기존 게이트 단위 테스트 갱신

**1d. SKIP 카운터 분리**
- `stale_outputs` 합산(`jni.rs:796`)을 `stale_input_drops`/`output_burst_discards` 두 AtomicU64로 분리
- HUD 패킹(`pack_stream_stats`, `jni.rs:1790-1798`)의 64비트 비트필드는 하위 호환을 위해 기존 자리는 합계 유지, 로그(`jni.rs:780-790`)에 두 값 분리 출력
- 목적: 측정 스파이크가 연쇄 원인(입력 정책 vs 출력 burst)을 즉시 판정 가능하게

### 파트 2 — FEC (Reed-Solomon, MIT nanors)

**범위**: 미디어 데이터그램(비디오 조각)의 유실 복구. 제어·입력·ACK·프로브는 기존 신뢰 경로(ACK/재시도) 유지.

- **구성**: AU별 인터리빙 RS(n,k). 안: k=8 데이터 조각당 패리티 2개(20% 오버헤드) — LOSS 32/~10,800 (0.3%) 수준의 산발 유실엔 과잉이지만 버스트에 대비한 시작값. 조각 수가 k 미만인 마지막 AU는 축소된 그룹 적용
- **CRITICAL: AU 경계와 FEC 그룹의 정렬** — fragment 재조립기(`media_datagram.rs` `FrameReassembler`)와 FEC 복호 사이의 순서(先 FEC 복호 → 후 재조립)를 프로토콜 테스트로 고정. 조각 헤더에 FEC 그룹 id/인덱스 추가 시 기존 `L2` 헤더 버전과의 하위 호환(구 Host/Viewer 혼합 시 명시적 거부) 규칙을 wire 버전 관례에 맞춰 정의
- **의존성**: `nanors`(MIT)를 vendored 의존성으로 추가. GPL 코드 불포함 확인
- **동작**: 복구 성공 시 frame gap 없음 → IDR 요청 없음. 복구 실패(패리티 부족) 시 기존 gap 경로 → IDR(파트 1의 단축된 경로)
- **Host 송신 측**: `wire.rs` 조각화 뒤 패리티 생성 송신. 인코딩 스레드에 추가하는 계산은 k=8에 RS 인코딩 수십 µs 수준(측정 후 `encodeOutputSamplesUs`에 반영 확인)

### 파트 3 — ABR (적응 비트레이트)

- **신호원**: Viewer가 이미 1Hz 피드백(`LCF1`)으로 전송 중인 통계에 LOSS/SKIP/RTT/decoder feed ms 추가
- **Host 정책**: 기존 dynamic clamp(`CaptureShim.swift` bitrate clamp) 위에 계단식 조정 — 연속 M개 피드백에서 SKIP/LOSS 임계 초과 시 비트레이트 ×0.7 (하한 기존 minRate), N개 세션 정상 시 ×1.15씩 회복 (상한 기존 maxRate). 다중 세션 시 각 세션 독립 적용
- **측정 지표**: Host `getStatus`의 kbps 시계열과 Viewer HUD LOSS/SKIP으로 수렴 확인
- **명시적 비목표**: 해상도/FPS 동적 변경, SVC/시뮬캐스트 — 비트레이트만

### 파트 4 — Windows WGC 제로카피

- **현재**: WGC 텍스처 → `CopyResource` → STAGING → `Map(CPU_READ)` → CPU BGRA→NV12 → MF 인코더 (`capture.rs:111-144`)
- **목표**: GPU 상태 경로 — WGC D3D11 텍스처 → GPU 컴퓨트셰이더(또는 MF 자체 변환) BGRA→NV12 → 인코더 입력 텍스처 직접 전달. Sunshine `display_vram.cpp` 구조 사양 참조(GPL — 코드 복사 금지, MIT 재구현)
- **방법**: Media Foundation 하드웨어 MFT의 D3D11 `IMFDXGIDeviceManager` 텍스처 입력 경로 사용. MF가 BGRA→NV12 변환을 지원하면 셰이더 없이 MFT 입력 attribute로 해결되는지 우선 확인(구현 계획 단계에서 스파이크로 판정)
- **순서**: E13(Windows 물리 검증)과 동일 게이트 — 이 파트의 수용 기준은 Windows 실기기에서만 종단 검증 가능. 구현은 macOS 교차 컴파일 + Windows CI로 소스 수준 유지
- **명시적 비목표**: macOS 제로카피 개선(이미 완성 상태), 멀티 GPU/eGPU 최적화

## 데이터 흐름 (변경 후)

```
[Host] SCK IOSurface → VT H.264(무한 GOP) → 조각화 → RS 패리티 추가 → 인증 UDP
                                                                ↑ ABR clamp (피드백 기반)
[Viewer] UDP 수신 → FEC 복호 → 재조립 → stale 판정(히스테리시스 K=3)
       → AMediaCodec → ANativeWindow 직접 렌더
       → 1Hz 피드백(LOSS/SKIP/RTT) ──────────────────────┘
```

## 오류 처리

- IDR 요청 유실: debounce(250ms) 후 재요청. FEC가 프로브/요청 데이터그램 자체는 복구하지 않으므로(제어 경로는 FEC 범위 밖) 기존 ACK/재시도에 의존
- FEC 복호 실패: 기존 frame gap → IDR 경로(파트 1 단축본). 새 패리티 미수신 AU는 폐기
- 구형 Host/Viewer 혼합: FEC 헤더 추가로 wire 버전 불일치 시 기존 관례(명시적 거부) 따름
- Windows 제로카피 실패(디바이스 경로 미지원): 기존 CPU readback 경로를 폴백이 아닌 **명시적 세션 오류**로 유지(무인 소프트웨어 폴백 금지 원칙, E13과 동일)

## 테스트 전략

각 파트 독립 검증(접근법 A 원칙):

1. **파트 1**: 히스테리시스 단위 테스트(연속 K-1 stale는 렌더, K 연속은 재동기화), debounce 상수 테스트 갱신, 카운터 분리 왕복 테스트. 실기기: 동일 시나리오(3분 4K/1080p) SKIP/복구 시간 비교 — 측정 스파이크와 지표 공유
2. **파트 2**: RS 인코딩/복호 왕복, 1/2 조각 유실 복구, 패리티 부족 시 폐기+gap 경로, AU 경계 정렬 property 테스트(기존 `media-model` fragment 테스트 패턴 확장), wire 버전 혼합 거부 테스트
3. **파트 3**: 계단식 정책 단위 테스트(임계 초과 감소/정상 회복/하상하한 클램프), 다중 세션 독립성. 실기기: 인위적 대역폭 제한에서 kbps 수렴 곡선
4. **파트 4**: MSVC 교차 `cargo check --lib`, GPU 경로 단위 모의(D3D11 타입 수준), Windows CI 통과. 물리 검증은 E13 게이트 명시
5. **공통**: `cargo test --workspace` + host Tauri 테스트 + `bun run test:contract`(wire 변경 반영) + `test:architecture` + React 미변경 영역 react-doctor 100/100 유지

## 성공 기준

- [ ] 동일 3분 시나리오에서 SKIP < 50, 렌더 FPS ≥ 59 (측정 스파이크 지표와 동일 수집 방식)
- [ ] IDR 후 복구 p95 ≤ 100ms (구: 최대 ~1s + 750ms)
- [ ] 3% 인위적 데이터그램 유실에서 IDR 요청 없이 스트림 유지(FEC 복구)
- [ ] 인위적 대역폭 제한 시 Host kbps가 설정 상하한 내에서 수렴(오버슛 없이 단조 회복)
- [ ] Windows 백엔드가 GPU 상주 경로로 컴파일되고 기존 readback 경로 제거(소스 수준; 물리 성능은 E13)
- [ ] `cargo test --workspace`/host 테스트/contract/architecture 전 통과, react-doctor 100/100
- [ ] 저장소에 GPL-3.0 코드 없음(의존성·vendor 감사)

## 범위 제한 (하지 않는 것)

- WebRTC/QUIC 전송 교체 및 bake-off 실행 (ADR-0004 유지)
- 커서 분리 채널, 4:4:4/HEVC/AV1 코덱 협상 — 리서치에서 확인된 후보 후속 과제로 별도 SPEC
- macOS 제로카피 변경(이미 IOSurface 경로), 가상 디스플레이/동적 해상도
- 해상도/FPS 동적 변경, SVC, 다중 트랙 시뮬캐스트
- Linux Host
- 정기 IDR 재도입, stale 예산값 자체 변경(히스테리시스만 추가)

## 미해결 (측정 스파이크 결과로 판정)

- SKIP 461 중 입력 정책 연쇄 vs 클록 오프셋 왜곡(L2 wall-clock 보정, `jni.rs:488`) 비중 — 카운터 분리(1d) 후 실측
- K=3, debounce 250ms, RS(10,8) 초기값의 실측 기반 조정
- MF 하드웨어 MFT의 BGRA 직접 입력 지원 여부(파트 4 스파이크)

## 참고 자료

- 리서치: `docs/research/2026-08-25_remote-screen-display-pipelines.md` (Moonlight FEC/무한 GOP, Parsec 제로카피, 라이선스 조사)
- 코드: `native/android-viewer/src/jni.rs:780-800,1360-1410` (stale/갭/재동기화), `native/android-viewer/src/media_datagram.rs:36-45,397-433` (예산/게이트), `crates/viewer-decoder/src/lib.rs:660-700` (burst 폐기), `native/macos-capture-shim/Sources/CaptureShim.swift:1192,1504-1660,1834-1932` (GOP/드레인/인코더), `apps/host-desktop/src-tauri/src/windows_backend/capture.rs:111-144` (readback), `apps/host-desktop/src-tauri/src/wire.rs` (조각화/HMAC — v0.2 계획과 조정 필요)
- 문서: `docs/EVIDENCE.md` E15 (관측 표본), ADR-0002/0004, `docs/plans/2026-08-25-v0.2-hardening.md` (HMAC 작업과 wire 충돌 조정)
- 외부: Moonlight `RtpVideoQueue.c`(RS FEC 설계), Sunshine `display_vram.cpp`(제로카피 구조, GPL — 참조만), nanors(MIT), Parsec technology(7ms 참고)
