# 성능 전면 검토 — 지연 최소화·60fps 유지·멀티디스플레이 구독 (2026-09-11)

## 0. 요약

7개 병렬 조사(호스트 Swift 파이프라인, 전송/FEC/암호, 뷰어 네이티브 디코딩·렌더, 뷰어 RN/JS,
멀티디스플레이 구독 아키텍처, 입력·계측, 경쟁사 웹 리서치)를 종합했다. 코드 미변경, 읽기 전용.

핵심 결론 4가지:

1. **"잃으면 전부 버리고 IDR"이 만능 실패 대응으로 박혀 있다.** 오버플로·데드라인 초과·갭·
   랑데부 만료가 전부 "체인 폐기 + IDR 폭풍"으로 수렴하고, IDR은 와이어에서 가장 크고
   잃기 쉬운 객체(전송 70–120ms)라 손실 시 에피소드가 재시작된다. 에피소드당 프리즈
   150–400ms. LAN RTT 2–5ms에서는 **NACK 재전송과 LTR이 FEC+IDR보다 구조적으로 유리**하다.
2. **splitVertical(4K 2타일)의 페일 세 가지가 미해결.** 2026-09-08 연구 문서의 실험 1~4는
   **전부 미반영** — ① 랑데부 만료 시 전체 리셋+페어 IDR(83ms 예산), ② split 전송에 AU
   데드라인 부재(단일 경로엔 있음), ③ 한 타일 손실이 양쪽 프리즈+페어 IDR(750ms 재시도),
   ④ HEVC 미착수. 게다가 **신규 발견: `allocPort()`가 split의 base+1 포트를 예약하지 않아
   "4K split + 두 번째 디스플레이" 시나리오가 EADDRINUSE로 하드 실패**한다.
3. **멀티디스플레이는 "세션 N개"로는 확장되지만 조율이 없다.** 호스트 세션 상한 없음,
   ABR·페이서·소켓이 세션별 독립이라 두 스트림이 Wi-Fi 에어타임을 다투며 진동할 수 있고,
   입력은 단일 소유자(새 세션 시작 시 기존 세션 입력 자동 차단), 뷰어는 디코더 상한
   사전 확인 없음(4 인스턴스 지연 실패), 창당 WifiLock·커서 폴링·오디오 스레드가 N배.
4. **지연 바닥값은 이미 낮지만(최신 프레임 우선, 1ms 홀드, 저지연 디코더), 평시 60fps가
   아니라 복구 에피소드와 복구 후 회복 속도가 체감을 결정한다.** ABR 반응 2–4초, 상승은
   8안정 윈도우 — 이 "회복이 느린" 구간이 개선 여지가 가장 크다.

기준선(기존 실측): 4K split 평균 59.6fps, 1초 구간 최저 44fps, gap→첫 출력 최대 159ms,
2.895초 복구 정지 1회(`docs/responsive-streaming-validation.md:58-73`). 듀얼 인코더 콜백
p95 38ms(단일 10.4ms) — 웹 리서치 결과 **베이스 M1/M2는 인코더 엔진이 1개**라 2 세션이
한 엔진을 타임슬라이스하는 것이 이 수치의 정체일 가능성이 높다(M1 Pro/Max는 2개).

---

## 1. 조사 방법

| 에이전트 | 범위 | 산출 |
| --- | --- | --- |
| 호스트 파이프라인 | native/macos-capture-shim (캡처→인코더→페이싱) | §3.1 |
| 전송·FEC·암호 | 와이어 포맷, FEC, AEAD, 혼잡 피드백, 소켓 | §3.2 |
| 뷰어 네이티브 | MediaCodec, 프레젠테이션, 갭 복구, JNI | §3.3 |
| 뷰어 RN/JS | Expo 앱, 브리지, 폴링, 서멀 리스크 | §3.4 |
| 멀티디스플레이 | 구독 아키텍처, 세션 비용, 전환 지연 | §3.6 |
| 입력·계측 | 입력 왕복 예산, 카운터 인벤토리 | §3.5, §4 |
| 웹 리서치 | Moonlight/Sunshine, Parsec, WWDC, Android, Wi-Fi, GCC/SCReAM | §5 |

---

## 2. 아키텍처 스냅샷 — "멀티디스플레이"의 두 얼굴

- **모드 A — 독립 세션**: (디스플레이, 뷰어 포트)마다 제어 세션 1 + CaptureSession 1
  (SCStream 1, VT 인코더 1, UDP 소켓 1). 뷰어는 세션당 OS 프리폼 윈도우 1개.
- **모드 B — splitVertical**: 디스플레이 **1개**를 4K 캡처해 Metal로 좌/우 타일 분할,
  VT 인코더 2개, 연속 포트 2개로 인터리브 전송. 뷰어는 **한 윈도우 안에** Surface 2개 +
  디코더 2개. "두 화면"이 아니라 "한 화면, 두 배선"이다.

세션 = 제어 평면 엔트리 1:1 (`apps/host-desktop/src-tauri/src/control.rs:54-84`),
shim 핸들 = 등록 딕셔너리 (상한 없음, `native/macos-capture-shim/Sources/CaptureShim.swift:27`).
오디오는 뷰어 기기당 소유자 1개(최저 핸들 세션, `SystemAudioOwnership.swift:17-25`),
소유 이전은 live `updateConfiguration`으로 무중단.

**디스플레이 전환 비용**: 재구성은 소스 변경 불가(`control.rs:700-713`) → 항상
정지+신규 시작(새 SCStream·소켓·LCH1·IDR). 웜 0.5–1.5초, 콜드 5–16초
(SCStream 콜드 스타트 8초+ 문서화, 15초 타임아웃 `CaptureSession+Backend.swift:144-153`).

---

## 3. 병목 발견 (계층별, 근거 포함)

약어: `SRC/` = `native/macos-capture-shim/Sources/`, `AV/` = `native/android-viewer/src/`.

### 3.1 호스트 (캡처·인코더·페이싱)

| # | 발견 | 근거 | 영향 |
| --- | --- | --- | --- |
| H1 | **split 전송에 AU 데드라인 부재** — 단일 경로 `writePacket`은 버스트마다 데드라인 검사·중단(`SRC/Transport/CaptureSession+UdpPacket.swift:196-218`), split `writeSplitPacketPair`는 검사 없음(`SRC/Transport/CaptureSession+SplitTransport.swift:273-306`). 느린 링크가 직렬 networkQueue를 AU 전체만큼 점유 | 전송 조사 | flow-capacity 킬(#H2)의 직접 연료 |
| H2 | **split 큐 오버플로 = 세션 종료** — `pendingSplitAccessUnits` 5 초과 시 `markStopped("split encoded queue exceeded flow capacity")` (`SRC/Split/CaptureSession+Split.swift:458-486`) | 전송 조사 | 열악 링크에서 세션 자체 사망 |
| H3 | **랑데부 만료 = 전체 리셋+페어 IDR (여전히)** — 만료 예산 16.67ms×5=83ms(`SRC/Split/DualEncoderPipeline.swift:12-22`), 만료 시 `beginPairedRecovery`가 인플라이트 전부 폐기+강제 페어 키프레임(`:369-379`). 완화 2건 추가됨: 인코더 세션 유지, 만료 기준을 첫 출력으로 시프트(`:349-356`), 유휴 화면 IDR 캐리어 시딩(`SRC/Capture/CaptureSession+PendingCapture.swift:78-86`) — 그러나 "늦은 타일만 폐기"는 미구현 | 2-display 문서 | 스터터 에피소드 최대 기여 |
| H4 | **복구 게이트가 의도적으로 스트림을 얼린다** — 강제 키프레임 후 IDR이 뷰어에 도달할 때까지 델타 제출 중지(`SRC/Capture/CaptureSession.swift:46-51`), 750ms 타임아웃 | 호스트 조사 | 손실당 1 RTT+IDR 프리즈 |
| H5 | **키프레임 간격 3600 ≈ 무한** — 모든 복구가 강제 IDR 버스트(64Mbps 상한, 750ms 예산) (`SRC/Encoder/CaptureSession+EncoderSetupAttempt.swift:268-283`, `UdpPacingPolicy.swift:57-72`) | 호스트 조사 | IDR 슈퍼프레임 손실 자석 |
| H6 | **ABR 반응 2–4초** — 컷 2연속 혼잡 윈도우 ×0.80, 레이즈 8안정 윈도우 4→30%(`SRC/Encoder/CaptureSession+AdaptiveBitrate.swift:104-110,234-241`), 피드백이 1Hz 누적 카운터 | 전송 조사 | 붕괴 후 회복 수 초 |
| H7 | **단일 4K 인코더 벽 48.6fps** — RTVC 21ms/프레임, split 듀얼 AVE만 59.8fps(단일 경로 4K는 구조적으로 <60) | doc11 §14-17 | split이 사실상 필수 |
| H8 | **프레임당 4번의 직렬 큐 핸드오프** (split: 캡처→encode→(Metal 완료 복귀)→packetize→network, 전부 userInteractive 직렬) + 페이싱 핫 루프의 per-버스트 stateLock | `SRC/Split/DualEncoderPipeline.swift:162`, `SRC/Transport/UdpStabilityPolicy.swift:238-247` | 소규모지만 누적 |
| H9 | **세션당 6개 userInteractive 직렬 큐** — 2+ 세션이 VT/Metal/WindowServer를 동일 우선순위로 경합(듀얼 RTVC 37–48fps 붕괴가 실측 증거). 배경 세션이 포커스 세션과 경쟁 | doc11 §17.1, H-11 | 멀티세션 품질 |

### 3.2 전송 (FEC·암호·피드백)

| # | 발견 | 근거 | 영향 |
| --- | --- | --- | --- |
| T1 | **NACK/재전송 전무** — 수리 = FEC + IDR뿐. LAN RTT 2–5ms에서 격리 손실 1패킷 재전송이 구조적으로 최저비용 | 전송 조사 전역 | "1패킷 손실 → 300ms 프리즈"의 근원 |
| T2 | **FEC 순차 그룹, 인터리빙 없음** — RS k≤8, 기본 패리티 2(+25%), AU 내 8프래그먼트 순차 그룹. 한 그룹에 패리티 초과 손실 → AU 전사 → 갭 → (단일) MediaCodec **flush** + IDR. IDR 전송 70–120ms(165–317 프래그먼트) | `crates/fec-core/src/lib.rs:237-246`, `SRC/Split/CaptureSession+SplitTransport.swift:139-157`, `AV/media_datagram.rs:1045-1050` | 에피소드당 150–400ms |
| T3 | **뷰어 FEC 디코드가 테이블 없는 gf_mul** — 호스트는 64KB 곱셈표, 뷰어는 비트 루프+pow-254 역원 → 회복 그룹당 모바일 CPU 0.3–1ms (수신 핫 패스) | `crates/fec-core/src/lib.rs:299-323` | RX 루프 지연 |
| T4 | **데이터그램당 복사 3회+할당 2회 양측** — 호스트: fragment Data+FEC shard+seal 출력, 뷰어: open Vec+FEC to_vec+슬랩 복사. sendmmsg 없음, split 뷰어는 recvmmsg도 없음 | `AV/secure-channel/src/lib.rs:401-434`, `AV/media_datagram.rs:369,668-712` | 4K split ~3,600 dg/s에서 체감 |
| T5 | **LCF1이 1Hz 누적 카운터** — 첫 손실 즉시 보고 없음. 단, **AEAD 카운터가 매 데이터그램에 이미 실려 있어 무료 시퀀스로 쓸 수 있음** | `AV/single_session/feedback.rs:68-104` | 혼잡 감지 지연 |
| T6 | **16bit AU id, 랩 가드 64엔트리** — 고fps에서 수 분마다 랩 | `AV/media_datagram.rs:794-802` | 잠재 오탐 |
| T7 | **수신 버퍼 비대칭** — 단일 세션 SO_RCVBUF 512KB(split 4MB), 호스트는 SO_RCVBUF 미설정. 512KB는 4K 복구 버스트 50ms 미만 | `AV/socket_tuning.rs:6-75`, `SRC/Capture/CaptureSession+Setup.swift:302-322` | 커널 드롭 → 갭 |
| T8 | **재정렬 시계 8ms vs FEC 회복** — 패리티가 8ms/3프레임 창 안에 오지 않으면 AU 스킵. 적응형 아님 | `AV/media_datagram.rs:45-46` | 느린 패리티 = 낭비 갭 |
| T9 | **단일 경로는 갭마다 MediaCodec flush, split은 무 flush 프리즈** — split 쪽이 저렴하고 단일 쪽 드리프트 | `AV/single_session/decoder.rs:20-34` vs `AV/renderer/split_session/gap_policy.rs:61-73` | 단일 세션 복구 비용 불필요하게 큼 |

### 3.3 뷰어 네이티브 (디코딩·렌더)

| # | 발견 | 근거 | 영향 |
| --- | --- | --- | --- |
| V1 | **한 타일 손실 → 양쪽 프리즈 + 페어 IDR(750ms 재시도)** — 페어 재개는 양쪽 타일이 같은 IDR 세대 보고 시에만. 타일별 디커플 복구 미구현(호스트 PLI coalescing 제약, 의도적) | `AV/renderer/split_session/gap_policy.rs:61-73`, `recovery.rs:3-16,106-137` | 멀티디스플레이 체감 1위 |
| V2 | **PresentAt 명령이 루프 상단에서만 처리** — 타일 워커가 poll(2ms) 후 루프당 4 데이터그램 처리, 타이밍 릴리스가 폴 틱 대기. 최악 프레임당 2–5ms 추가 | `AV/renderer/split_session/tile_worker.rs:124-165`, `helpers.rs:11-19` | split 지연 바닥 상승 |
| V3 | **디코더 재구성 = 완전 재생성** — SPS/PPS 변경 시 stop/delete/create/start + 검은 화면. split 입력 압박 17ms → DecoderFailure → **양쪽 flush** + 페어 IDR | `tile_worker.rs:448-460`, `split_session.rs:290-330` | 재바인드/회복 수백 ms 블랙 |
| V4 | **CursorOverlayView가 매 디스플레이 프레임 JNI 폴링** — Choreographer마다 `cursorState` JNI(전역 라이프사이클 뮤텍스+Arc 클론), 커서 비가시에도, 창당 60–165Hz | `CursorOverlayView.kt:76-93`, `jni.rs:459-465` | 서멀·전력, 메인 루프 점유 |
| V5 | **HUD가 250ms마다 JNI 5콜** + 오디오 스레드 MAX_PRIORITY 12ms 폴링(창당) | `StreamHudController.kt:103-123`, `StreamAudioPlayer.kt:21` | N창 곱연산 |
| V6 | **split 그레이스 고정 4ms** — 측정된 ready-delta p95로 스케일 가능한데 고정값 | `AV/renderer/presentation_sync.rs:11-12` | 소액 |
| V7 | **vsync 무시 "now+1ms" 릴리스** — 의도적(지연 우선)이나 120Hz 패널 마이크로스터터 여지. Moonlight는 Choreographer 정렬 + 80% 규칙 or nanoTime 모드 병행 | `presentation_sync.rs:301-306` | 스무스니스 |

### 3.4 뷰어 RN/JS

| # | 발견 | 근거 | 영향 |
| --- | --- | --- | --- |
| J1 | **getStatus 1Hz 폴링이 JS JSON+ChaCha seal/open + ~80필드 파싱** — 프리폼/XR에서 RN 윈도우가 resume 상태라 스트리밍 중 계속 돌고, 세션 N개면 페이로드 N배. 태블릿 서멀→스로틀 소용돌이의 주범 | `apps/viewer-expo/src/use-stream-controller.ts:83-90`, `control.ts:407-410` | JS 스레드·전력 |
| J2 | **토글이 boolean 하나에 startActivity 전체 왕복** — setCursor/setAudio/setWindowAspectRatio | `StreamLauncherModule.kt:317-363` | 무겁지만 저빈도 |
| J3 | **적응 리바인드가 1Hz 샘플마다 판단** — 5s 쿨다운 게이트 있음. 링크가 힘들 때 정확히 JS 버스트 | `use-stream-controller.ts:302-356` | 중간 |
| J4 | **SessionView/StatusView 수작업 복제** — 드리프트가 `undefined`+`??`로 은폐 | `control.ts:85-181` | 신뢰성 |

### 3.5 입력 왕복

한 윕 예산: 터치→JNI 서브 ms → 스케줄러(포인터는 2×fps 합체, 60fps에서 최대 ~8.3ms 대기;
신뢰 이벤트는 단일 플라이트 — 미ACK 키 뒤 클릭이 20ms 재시도 대기, head-of-line) → 전송
(단일 세션은 전용 EF 소켓 분리 ✅, **split은 미디어 소켓 공유+왼쪽 타일 워커가 전송**) → 호스트
(`inputQueue` 직렬 — **커서 CGEvent 탭·커서 폴링·주입이 같은 큐 직렬화**, `remoteKeyRemap`이
키 입력마다 NSGlobalDomain 파싱 `SRC/Transport/CaptureSession+Input.swift:259-330`) → ACK.
측정: **입력 RTT 계측 전무** — 타임스탬프가 경로 어디에도 없음.

### 3.6 멀티디스플레이 구독 특이사항 (사용자 핵심 질문)

| # | 발견 | 근거 | 영향 |
| --- | --- | --- | --- |
| M1 | **🚨 `allocPort()`가 split base+1을 예약 안 함** — split이 5001+5002 점유, 다음 `openDisplay`가 5002 할당 → bind EADDRINUSE → `ERR_STREAM_PREPARE`. **"4K split + 두 번째 디스플레이"가 정확히 하드 실패** | `apps/viewer-expo/src/session.ts:108-111`, `AV/prepared_udp.rs:48-53` | 시나리오 사망 |
| M2 | **세션 간 혼잡 조율 없음** — ABR·페이서·소켓 세션별 독립, 레지스트리 팩터(1/count×1.3)는 시작점만 낮춤. 두 스트림이 에어타임을 다투며 진동 가능 | `SRC/Encoder/CaptureSession+AdaptiveBitrate.swift:122-155` | 멀티스트림 안정성 |
| M3 | **입력 단일 소유자** — 입력 활성 새 세션이 기존 세션 입력을 무음 차단. "여러 화면 동시 조작" 불가 | `control.rs:1448-1477` | UX 제약 |
| M4 | **뷰어 디코더 상한 사전 확인 없음** — split 보장 2 인스턴스뿐, split(2)+창 2개 = 4 인스턴스 → 생성 실패가 늦게 회복 루프로 | `SplitDecoderCapability.kt:10-32` | 지연 실패 |
| M5 | **창당 상주 비용 N배** — WifiLock(FULL_LOW_LATENCY), 커서 폴링, 오디오 스레드, HUD 폴링이 창마다 | `StreamActivity.kt:294-307` 등 | 서멀 |
| M6 | **표시 전환 = 해체+신규** — 웜 0.5–1.5초+블랙 플래시. 뷰어 리스너·윈도우 재사용 의미의 `switchSource` 없음 | `control.rs:815-843`, `launch-stream.ts:483-535` | 전환 체감 |
| M7 | 대역폭 산술 — 4K split 시작 42Mbps(집계, 타일당 21M) + 1080p60 추가 ~5.6M → 실측 45–80Mbps+FEC. 좋은 Wi-Fi 5/6에서 가능하나 split의 에어타임 2배 + 버스트 비조율 | `SRC/Split/CaptureSession+Split.swift:155-161` | 링크 포화 여지 |

---

## 4. 계측 격차 (성능 작업의 전제)

1. **split 경로 E2E 캡처 나이 부재** — L2 타임스탬프가 와이어에 있지만 split은 호스트 시계
   오프셋 미수립이라 무시됨(`AV/renderer/split_latency.rs:1-21`). LCP1/LCP2 오프셋을
   split으로 확장하면 2.9초 프리즈 조사가 포렌식에서 직접 지표로 바뀐다. **최고 가치.**
2. **입력 RTT 전무** — `InputScheduler.push` 모노토닉 타임스탬프 → LCA1 ACK에 왕복 에코
   (또는 1Hz LCP 편승). 스케줄러 자체 드롭도 텔레메트리에 안 보임.
3. **`paired_idr_resumes` 로그 전용** — 와이어 스냅샷·stream-stats 콘솔에 없음
   (`split_session.rs:50`, `tools/stream-stats.py:50-135`). 합격 판정 전에 와이어화 필요.
4. **퍼프 로그가 폴링 게이트** — 1Hz LeftcarPerf 라인이 statsJSON 내부라 아무도 폴링 안 하면
   흔적 없음(`SRC/Metrics/CaptureSession+Stats.swift:286-315`).
5. **타일 스큐 히스토그램 부재** — p95/max만 있음. N타일 확장에 분포 필요.
6. 단일 세션은 시계 보정 나이(capture→decoder 등) EWMA 있음 ✅ — split이 따라가면 됨.

---

## 5. 외부 기법 조사 (적용 가능 순)

| 기법 | 기대 | 비용 | 출처 |
| --- | --- | --- | --- |
| **LTR로 IDR 대체** — VideoToolbox `EnableLTR`+LTR ACK 토큰(WWDC21 저지연 모드 일부), 손실 시 `ForceLTRRefresh`. IDR 폭풍·슈퍼프레임 소멸 | 큼 | 중 | WWDC21 10158 |
| **NACK 재전송 + FEC 축소** — LAN RTT 2–5ms에선 재전송(~1 RTT)이 20% FEC 상시 오버헤드보다 저렴. 뷰어가 이미 (au_id, group_base) 정확 파악 → NACK 명령 추가, 호스트는 ~2프레임 링 버퍼 | 큼 | 중 | libwebrtc hybrid, moonlight-common-c |
| **단일 프레임 VBV** — DataRateLimits=1프레임(현재 125%/1s), MaxFrameDelayCount=0, AllowFrameReordering=false, RealTime, ExpectedFrameDuration | 버스트 상한·인코더 큐잉 제거 | 설정만 | Apple 키 문서, Sunshine nvenc |
| **듀얼 인코더 재평가** — 베이스 M 실리콘은 엔진 1개(Pro/Max 2개). 38ms p95의 정체. 베이스 호스트면 단일 인코더+클라이언트 분할(또는 합성 캔버스)이 낫다 | 큼(하드웨어 의존) | 테스트 소, 재구조 중 | channels/handbrake 문서, stash#4945 |
| **Choreographer 정렬 릴리스** — 2-딥 출력 큐 + `releaseOutputBuffer(idx, frameTimeNanos-vsyncOffset)` + 80% 간격 스킵 규칙(최저지연 모드는 `System.nanoTime()`) | 마이크로스터터 제거 | 소 | moonlight-android |
| **교차 스트림 페이싱** — 링크율 ~80% 상한으로 두 스트림을 한 페이서가 인터리브(SCReAM식 self-clocking) | 에어타임 소용돌이 제거 | 중 | SCReAM RFC 8298 |
| **빠른 회복 ABR** — 손실 시 0.85x, 회복은 측정 ACK 레이트 향해 프로브(GCC 손실 밴드 <2% +5% / >10% 반감) | 회복 수 초→수백 ms | 중 | GCC draft, SCReAM |
| **뷰어 디코더 레시피 보강** — 현재 qti 키 보유✅. `getSupportedVendorParameters` 라이브 프로브, MTK `vdec-lowlatency`, `FEATURE_LowLatency` 우선 선택 | 기기 커버리지 | 소 | moonlight-android |
| WMM — 비디오 AF41→AC_VI(유니캐스트), 입력 EF 유지, 호스트 `SO_NET_SERVICE_TYPE` | 무선 큐 우선 | 소 | Aruba/Interline |
| 하지 말 것: true AU 봉입(헤더·FEC·갭폴리시 전면 수정 대비 이득 미미), QUIC-datagram(LAN 최저 지연엔 raw UDP 우위), AV1(저지연 HW 프로브 리스크, HEVC-SCC 도구는 VideoToolbox 미노출) | — | — | — |

---

## 6. 권장 로드맵 (숫자 합격 기준)

공통 원칙: 각 단계 전후 `tools/stream-stats.py` + perf-matrix로 동일 시나리오(4K split 60,
고움직임, 의도적 손실) 3회 측정. 먼저 §4의 1·3번(측정 구멍)을 막는다.

### 6.1 즉시 배치 (각 0.5–2일, 저위험)

| # | 작업 | 합격 기준 |
| --- | --- | --- |
| Q1 | **allocPort가 split base+1 예약**(또는 prepare에서 양 포트 bind 프루브) — M1 | split+2디스플레이 시나리오 시작 성공률 100% |
| Q2 | **split AU 데드라인 중단** — `writeSplitPacketPair`에 단일 경로의 검사 이식 — H1 | 의도적 손실에서 IDR/s 상승 없이 fps 하락 완화, flow-capacity 종료 0회 |
| Q3 | **flow-capacity 오버플로 완화** — markStopped 대신 최쌍 폐기+페어 IDR — H2 | 오버플로 에피소드에도 세션 생존 |
| Q4 | **랑데부 만료 완화(문서 실험 1)** — 늦은 타일 AU만 폐기+해당 타일 IDR, 정상 스큐 emit 유지 — H3 | 페어 리셋 ≥90% 감소, gap→첫 출력 p95 ≤100ms, 2.9s급 정지 0회 |
| Q5 | **`paired_idr_resumes` 와이어화+stream-stats 노출** | 콘솔에서 카운터 확인 |
| Q6 | split E2E 캡처 나이(LCP 오프셋 확장) + 입력 RTT(LCA1 에코) | 두 지표가 HUD/stats에 실시간 표시 |
| Q7 | sendmmsg(호스트), 뷰어 gf_mul 테이블화, 뷰어 AEAD 제자리 복호화, split recvmmsg, 단일 SO_RCVBUF 4MB | RX 루프 CPU·최악 지연 감소(로그 p95) |
| Q8 | 단일 세션 갭 정책을 split의 무-flush로 통일 — T9 | 단일 세션 손실 복구 시간 split 대비 열화 해소 |
| Q9 | 커서 폴링 30Hz 조절+비가시 스킵, 오디오 폴 백오프, RN 비가시 시 getStatus 일시정지 — V4/V5/J1 | 창당 유휴 웨이크 감소(서멀 여유) |
| Q10 | LCF1 첫 손실 즉시 보고 + AEAD 카운터를 데이터그램 시퀀스로 — T5 | 혼잡 감지 1s→즉시 |

### 6.2 중기 배치 (각 1–5일, 프로토콜 변경 수반)

| # | 작업 | 합격 기준 |
| --- | --- | --- |
| R1 | **NACK/RTX** — 짧은 재전송 캐시+뷰어 NACK 명령. FEC는 버스트 방어로 축소 | 격리 손실 에피소드 150–400ms→≤RTT+수 ms, IDR/s 대폭 감소 |
| R2 | **타일별 디커플 복구(문서 실험 3)** — 뷰어 갭폴리시 타일별 프리즈, 호스트 타일별 IDR(인코더는 이미 독립 forceKeyframe 가능) | 단일 타일 손실 시 반대편 프리즈 0 |
| R3 | **LTR 도입(단일+타일)** — WWDC 저지연 모드 위에서 | 복구 시 IDR 슈퍼프레임 0, 복구 중 대역폭 스파이크 제거 |
| R4 | **교차 세션 혼잡 스칼라** — 레지스트리 전역 손실/블록 플래그 공유(AdaptiveBitrate의 withRegistry 지점 확장) — M2 | 2세션 동시 스트림에서 진동 에피소드 감소 |
| R5 | **빠른 회복 ABR** — ACK 레이트 프로브 | 붕괴→원래 비트레이트 회복 수 초→≤1s |
| R6 | IDR 전용 인터리브/레이어드 FEC | 의도적 버스트 손실에서 IDR 에피소드 재시작률 감소 |
| R7 | cheap display switch — 뷰어 리스너·윈도우 유지 `switchSource` — M6 | 웜 전환 ≤0.5s, 블랙 플래시 제거 |
| R8 | 뷰어 디코더 어드미션 — 라이브 창 점유 인스턴스 차감 후 해상도 강등 — M4 | 상한 초과 시 늦은 크래시 0 |

### 6.3 구조 변경 (주 단위, 방향 결정 후)

1. **단일/split 렌더러 N-타일 수렴** — 피드백·시계·복구 드리프트 표(§3.3) 소멸. 멀티디스플레이
   확장의 토대.
2. **수신자 주도 페이서(리키 버킷+큐 나이 정책)** — 직렬 usleep 페이서 대체. 단일 최대
   구조 지연 레버(§3.1 H1 계열). AU 나이/바이트 텔레메트리 이미 존재.
3. **HEVC 타일 프로브(문서 실험 4)** — 6.1~6.2 이후. 듀얼 콜백 p95 열화 없고 -20% 비트레이트
   이상일 때만.
4. **단일 인코더 4K60 또는 합성 캔버스** — 호스트 실리콘이 베이스 M이면 우선 검토. 랑데부+
   페어IDR 세금 자체를 소멸.
5. **스트리밍 헬스 피드의 JS 탈출** — 네이티브 집계 이벤트 푸시로 전환, JS는 사용자 가시
   상태만(§3.4 J1 소멸).
6. **멀티윈도우 자원 공유** — WifiLock·오디오 파이프라인 단일화, 세션 우선순위(포커스/배경).

### 참고 목표 수치 (업계)

Parsec 인코딩+디코딩 ≈10ms, CloudXR 권장 네트워크 20–50ms, 모션투포톤 ≤20ms(Oculus).
LAN 데스크톱 환산: 캡처 ≤3ms / 인코딩 ≤6ms / 네트워크 ≤3ms / 디코딩 ≤8ms / 합성 ≤4ms →
**글라스투글라스 ~25ms 현실 목표, ~16ms 엘리트**. 현재 파이프라인의 평시 스테이지 합은
이 예산 안에 들어온다 — 남은 것은 복구 에피소드 제거와 멀티스트림 조율이다.

---

## 7. 실행 결과 (2026-09-11, 같은 날 구현)

§6 로드맵과 24시간 리스크 리뷰 15건을 작업 트리에 구현 완료(미커밋). 전체 게이트 통과:
cargo test workspace 383 passed / Tauri 154 passed / android-viewer 223 passed (host target) /
aarch64-linux-android check clean / tsc pass / viewer-expo vitest 364 passed / contract 4 passed /
react-doctor 100/100 / gradle 단위테스트 BUILD SUCCESSFUL / shim 전체 빌드 OK.

### 리스크 리뷰 15건 (risk-review-2026-09-11)

| 건 | 수정 | 요점 |
| --- | --- | --- |
| C1 | 이미 트리에 있음 | split 재바인드 공유 crypto 인스턴스(검증됨) |
| H1 | control.rs+lib.rs | 세션 시작 전 커튼 선raise, 토글 시 SCK 세션 재시작으로 필터 재스냅샷 |
| H2 | input_protocol.rs | 스타일러스 압력 이동을 lossy 최신우선 경로로 되돌림(신뢰 큐误삽입 해제) |
| H3 | clipboard.rs | read_text ContentNotAvailable → 빈 텍스트 처리, 이미지 분기 도달 |
| H4 | clipboard.rs | PNG set_limits+8192²/64MiB 사전 검사, 할당 전 거절 |
| M1 | control.rs+pairing.rs | 명령 디스패치 직전 페어링 재확인, 철회 시 즉시 차단 |
| M2 | file_transfer.rs | created_at→last_activity, 청크마다 갱신 |
| M3 | control.rs+lib.rs | 커튼 상태 apply 성공 후 커밋, 실패 시 재시도 |
| M4 | StreamActivity.kt | rebuildStreamSurfaces에서 zoom 리셋 |
| M5 | file-io.native.ts | .part 스테이징+finalize rename |
| M6 | control.rs | 커튼 ON + CGDisplayStream 조합 거절/강제중단 |
| M7 | secure-channel+media_crypto | 동일키 재시작 시 유효 LCH1 인증 자체가 RX 워터마크 리셋 근거 |
| L1 | clipboard-sync.ts | 게이트 오류 래치(3회)+in-flight 가드 |
| L2 | StreamZoomState.kt | split 줌 타일별 seam 피벗 |
| L3 | Backend.swift | SCK setup async 홉 stopRequested 재검사 |

### 성능 로드맵

| 항목 | 상태 | 요점 |
| --- | --- | --- |
| Q1 allocPort | ✅ | allocPorts(2) 연속 예약+재시도, session.test.ts |
| Q2 split AU 데드라인 | ✅ | writeSplitPacketPair 버스트마다 검사·조기 중단, splitPairDeadlineExceeds |
| Q3 flow-capacity 완화 | ✅ | markStopped→큐 드롭+페어 IDR 복구 경로 |
| Q4 랑데부 만료 완화 | ✅ | 만료=해당 페어만 폐기(톰스톤), 3연속 시 기존 하드 리셋 밸브 |
| Q5 paired_idr_resumes 와이어 | ✅ | LCF1 v3 접미(양방향 길이 호환)→statsJSON→contract→stream-stats 콘솔 |
| Q6 split E2E 캡처 나이 | ✅ | 타일 소켓 LCP1/LCP2 시계 동기+capture/wire age EWMA, 입력 RTT(뷰어 측정) |
| Q7 RX 핫패스 | ✅ | fec-core 64KB 곱셈표(65536쌍 비트동등 증명), AEAD 제자리 복호화, split recvmmsg, SO_RCVBUF 4MB. 호스트 sendmmsg는 macOS 미지원으로 각하 |
| Q8 단일 갭 정책 통일 | ✅ | 갭=무 flush 프리즈+IDR 대기, flush는 fatal 전용 |
| Q9 웨이크 감소 | ✅ | 커서 30Hz/비가시 2Hz, 오디오 2단 백오프, getStatus 2s(적응 로직 2s 샘플 수용 확인) |
| Q10 즉시 손실 피드백 | ✅ | 양 경로 첫 손실 시 ≥100ms 간격 즉시 LCF1 |
| R1 NACK/RTX | ✅ | NAK 명령(au_id+frag≤20), 호스트 링(사이드별 au 8개/512항목), 뷰어 grace=clamp(2×RTT+5ms, 8–25ms) — 격리 손실 150–400ms 프리즈→RTT 수리. 신구 호스트/뷰어 4분면 호환 |
| R2 타일별 디커플 복구 | ✅ | 호스트 사이드어웨어 키프레임(피어 손실 신선할 때만 paired), 뷰어 타일별 resume. 구형 뷰어=기존 paired, 구형 호스트=타일 resume+피어 IDR 무해 수용 |
| R3 LTR | 보류 | Apple 특화 ACK 흐름이 실기기 검증 없이 안전 검증 불가 — 후속 |
| R4 교차 세션 혼잡 공유 | ✅ | 레지스트리 3s 마크, 자기 감지만 마크 갱신(에코 봉쇄), 2연속 규칙 유지 |
| R5 빠른 회복 램프 | ✅ | 컷 후 2 clean 자격+3윈도우 ×30% 램프(90% 상한), 순수 함수화 |
| R6 IDR 인터리브 FEC | 대체 | 그룹 배치가 와이어 비호환(구현 APK가 산술로 그룹 계산, 협상 없음). 목적은 R1 NACK가 IDR 프래그먼트 수리로 대신 수행 |
| R7 switchSource | ✅ | reconfigure에 source_index(검증→정지 전 거절, 실패 시 원본 복원), 뷰어 동일 포트 재바인드=윈도우 유지. UI 진입점은 기존 UX 보존 위해 미연결(handleSwitchSessionSource 노출) |
| R8 디코더 어드미션 | ✅ | decoder-budget.ts(용량 4, split 2슬롯), split 요청 초과 시 강등/거절, 프로모션 게이트 |
| 구조 1–6 | 미착수 | 방향 결정 필요: N-타일 렌더러 수렴, 수신자 주도 페이서, HEVC 타일(실험 4), 단일 인코더 4K, 헬스 피드 JS 탈출, 멀티윈도우 자원 공유 |

계측 신규: splitPairConsecutiveTimeouts/QueueOverflowDrops/DeadlineExceeds, nacksServed/Missed,
splitPerTileKeyframes, receiverPerTileIdrResumes, receiverInputRttMs, crossSessionCongestion*,
hostOffsetMs/captureAgeMs(split 1Hz 로그+LCF1 v3). 실기기 검증(§6.1 시나리오 3회 실측)은
사용자 실행 체크리스트로 남는다.

## 8. 기존 문서와의 관계

- `docs/2026-09-08-two-display-lag-research.md`의 실험 1~4는 **전부 미반영**으로 재확인됨
  (랑데부 완화의 부분 완화 2건 — 인코더 유지, 만료 기준 시프트, 캐리어 시딩 — 만 추가됨).
  이 문서의 §6.1 Q2–Q4가 그 실험 1·2에 해당하고, §6.2 R2가 실험 3, §6.3-3이 실험 4다.
- `docs/11-low-latency-investigation.md`의 종결 항목(사이즈어웨어 데드라인, AVE 지연 prepare,
  160Mbps 페이싱 등)은 코드에서 수정 확인. 미종결: 4K60 장기 수용, 1440p→4K 전환 6s 피드백
  타임아웃.
- 신규 발견(기존 문서에 없음): M1 allocPort 충돌, H1 split 데드라인 부재의 정확한 위치,
  H2 flow-capacity 세션 킬, M2–M6 멀티세션 조율 부재, T3 FEC 테이블 비대칭, V4 커서 폴링,
  J1 getStatus 서멀, 입력 RTT·split 시계 부재.
