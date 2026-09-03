# 리서치: OpenDisplay(opendisplay.app) vs Leftcar — 무엇을 배울 것인가

작성일: 2026-09-03
기준 커밋: `8ae714c12c1d2ad8ecfe2b34212dfde6c295f10a` (branch `main`)
방법: opendisplay.app 웹사이트 분석 + GitHub 저장소(peetzweg/opendisplay) 코드/PROTOCOL.md 전수 조사 + Leftcar 코드베이스(docs/, native/, apps/) 조사 (병렬 서브에이전트 3건)
근거 정책: OpenDisplay 측 사실은 저장소 코드와 PROTOCOL.md, 공식 사이트만 근거로 쓴다. Leftcar 측 사실은 커밋 `8ae714c` 시점 코드만 근거로 쓴다.

## 연구 질문

https://opendisplay.app/ 를 보고 Leftcar에서 더 배울만한 점은 무엇인가 — 특히 가상 디스플레이 생성 구현과 비교.

## 요약

OpenDisplay는 iPhone/iPad/여분 Mac을 Mac의 확장 디스플레이로 만드는 무료 오픈소스(GPL-3.0, v0.4.x 이전 MIT) 앱이다. 핵심 발견:

1. **Leftcar의 캡처→인코딩→전송 파이프라인은 OpenDisplay보다 성숙하다.** OpenDisplay는 단일 TCP만 지원하지만 Leftcar는 UDP+FEC / TCP / USB-AOAP 3종 전송, 적응 비트레이트, 듀얼 인코더 split, IDR 복구, PIN 페어링+토큰 인증을 갖췄다. 반대로 **가상 디스플레이가 Leftcar의 유일한 미완 영역**이고, OpenDisplay는 그 부분이 가장 잘 담긴 앱이다.
2. **OpenDisplay는 BetterDisplay 대신 private `CGVirtualDisplay` API를 직접 호출한다** (`Mac/VirtualDisplay.swift`, `Mac/CGVirtualDisplayPrivate.h`). 이는 DeskPad/BetterDisplay와 동일 계보의 역공학 API로, Leftcar의 "자체 구현 = DriverKit = 큰 새 프로젝트"라는 기존 전제(ADR-0003/0005, `docs/09-risk-register.md` R-008/R-012/R-015)를 바꿀 수 있는 선택지다. **Leftcar 저장소 어디에도 CGVirtualDisplay 검토 기록은 없다** — 기존 ADR들이 커버하지 않는 미개척 영역.
3. **가장 배울 가치가 큰 것은 개별 기법이 아니라 운영 지식이다**: 생성 후 200ms~2s 간격으로 미러 해제·HiDPI 모드 재적용·배치 복원을 영구 감시하는 "연속 집행 루프", serial 기반 배치 기억, 회전=재생성 아닌 모드 재적용, "Stop Extending" 오염 복구. macOS 가상 디스플레이는 "만들고 잊는" 객체가 아니라는 점 — 이것은 BetterDisplay CLI 실험에서는 절대 관측되지 않는 내부 동작이다.
4. **라이선스 주의**: GPL-3.0이라 코드 발췌/복사 금지. 단 PROTOCOL.md는 인터오퍼레이션 규격으로 공개된 문서이고, CGVirtualDisplay private 헤더는 Khaos Tian(VirtualDisplayExp)→DeskPad로 이어지는 공통 선임계 지식이라 시그니처 목록 참고 후 직접 작성은 통상 문제없다 (파일 텍스트 복사는 금지).
5. 그 외 배울 점: 커서 분리+로컬 에코(Leftcar는 `showsCursor=true`로 미달 — 기존 리서치 `docs/research/2026-08-25` 결론과 일치), pre-encode 백프레셔 드롭+드롭 시 강제 키프레임 금지, 정적 화면 IDR 리플레이, SCK 60Hz rate-limiter 우회(120 요청), USB 역할 배정("수신기가 리슨, 송신기가 접속").

## 상세 분석

### 1. 두 앱의 구조 비교

| 영역 | Leftcar (Mac→Android) | OpenDisplay (Mac→iOS/Mac) |
|---|---|---|
| 가상 디스플레이 | BetterDisplay CLI 셸아웃, opt-in 실험 (`apps/host-desktop/src-tauri/src/virtual_display.rs:90-127`) | private CGVirtualDisplay 직접 호출 (`Mac/VirtualDisplay.swift:43-68`) |
| 캡처 | ScreenCaptureKit 420v IOSurface (CGDisplayStream 폴백) | ScreenCaptureKit 420v (동일, +픽셀 해상도 주의사항) |
| 인코딩 | VideoToolbox H.264, AVE/RTVC 하드웨어 ID 명시 검증, 적응 비트레이트 6–80Mbps, split 듀얼 인코더 | VideoToolbox H.264 RealTime, 고정 프리셋 6/10/18Mbps |
| 전송 | UDP(1200B+FEC RS(8,10)) / TCP / USB-AOAP 3종, 제어 평면 분리 | 단일 TCP만 (WiFi=Bonjour, USB=usbmux 터널) |
| 디코딩 | Android MediaCodec low-latency (c2.qti 선호), Surface 직접 출력 | AVSampleBufferDisplayLayer (DisplayImmediately) |
| 입력 | 정규화 좌표→CGEvent, 무브 latest-wins/나머지 ACK 재시도 | CGEvent + Apple Pencil 합성 태블릿 이벤트 |
| 커서 | 비디오에 포함 (`showsCursor=true`) | **비디오에서 분리, 120Hz JSON + UDP 9001 사이드 채널 로컬 렌더** |
| 페어링 | 6자리 PIN + 32B 토큰 | 없음 (같은 네트워크/Bonjour 발견, USB 물리 연결) |

Leftcar가 뒤처진 것은 사실상 **가상 디스플레이와 커서 처리** 두 가지뿐이다.

### 2. 가상 디스플레이: OpenDisplay의 구현 (핵심 조사 대상)

#### 2.1 API와 생성

`Mac/CGVirtualDisplayPrivate.h`(74줄)는 역공학 private API 4개 클래스를 선언한다: `CGVirtualDisplayMode`(width/height/refreshRate), `CGVirtualDisplaySettings`(modes 배열 + `hiDPI` 플래그), `CGVirtualDisplay`(descriptor 초기화 + `applySettings:`), `CGVirtualDisplayDescriptor`(queue, name, maxPixelsWide/High, sizeInMillimeters, serialNum, productID, vendorID, terminationHandler).

생성 흐름 (`Mac/VirtualDisplay.swift:43-68`):

```swift
let descriptor = CGVirtualDisplayDescriptor()
descriptor.setDispatchQueue(DispatchQueue.main)
descriptor.name = name
descriptor.maxPixelsWide = UInt32(maxPointsPerAxis * 2)   // 회전 대비 여유
descriptor.maxPixelsHigh = UInt32(maxPointsPerAxis * 2)
descriptor.productID = 0x4F53; descriptor.vendorID = 0x5043
descriptor.serialNum = serialNum   // 기기별 안정 + 동시 디스플레이 간 유일
display = CGVirtualDisplay(descriptor: descriptor)
settings = CGVirtualDisplaySettings()
settings.hiDPI = 1
settings.modes = [CGVirtualDisplayMode(width: pointsWide, height: pointsHigh, refreshRate: 60)]
guard display.apply(settings) else { return nil }
```

설계 원리:

- **Point 기반 + @2x**: 패널 물리 픽셀 W×H → 가상 디스플레이는 (W/2)×(H/2) 포인트 @2x. 캡처도 2×points = W×H로 인코더까지 1:1 래스터. Retina pixel-for-pixel의 실현 방식.
- **serialNum 안정성**: vendor/product/serial로 디스플레이 배치를 기억하므로 기기별 고정 serial이 세션 간 화면 배치 유지의 열쇠.
- **60Hz 상한**: private API 특성상 60Hz가 상한 (README 명시).
- **maxPixels에 긴 축 ×2 여유**: 회전 시에도 같은 디스플레이에서 모드 전환이 가능하도록.
- **회전 = 모드 재적용**: `resize()`가 같은 가상 모니터에 새 모드만 `apply`한다. 재생성하면 WindowServer가 창을 다른 디스플레이로 재배치함.

#### 2.2 연속 집행 루프 — "만들고 잊으면 안 된다" (가장 배울 가치가 큰 부분)

생성 후 무한 Task가 200ms(안정 후 2s) 간격으로 세 가지를 영구 감시·복구한다 (`Mac/VirtualDisplay.swift:78-91`):

1. **미러 해제** (`ensureNotMirrored`): macOS가 가상 디스플레이를 미러 세트에 넣는 경우가 있다(TV로 오분류 시 기본값이 Mirror). `CGConfigureDisplayOrigin`/config로 **양방향 detach하며, 스코프는 반드시 `.forSession`** — `.permanently`는 `kCGErrorIllegalArgument`로 거부되는데 성공을 반환하는 은닉 버그가 있다.
2. **HiDPI 모드 재적용** (`selectHiDPIMode`): macOS는 새 디스플레이를 1x 모드로 기본하고, 이 serial의 오래된 저장 모드를 수 초 후 비동기 복원하거나 모드 리스트를 통째로 교체한다. `CGDisplayCopyAllDisplayModes`에 `kCGDisplayShowDuplicateLowResolutionModes: true`로 `pixelWidth == width*2` 모드를 찾아 `CGConfigureDisplayWithDisplayMode`로 재적용.
3. **배치(origin) 관리**: 생성 후 수 초간 macOS가 자체 저장 배치를 비동기 복원하므로 6초 복원 창 동안 target origin을 강제하고, 이후 origin 변화는 "사용자가 System Settings에서 드래그"한 것으로 콜백해 영속화.

이 세 가지는 BetterDisplay CLI 실험에서는 관측 불가능한 내부 동작이다 — CLI가 전부 대행해주기 때문. 자체 구현 시 반드시 이식해야 할 운영 지식이다.

#### 2.3 실패 복구 — "독성 아이덴티티"

시스템 UI의 "Stop Extending"(연결 해제)이 저장 상태를 오염시켜 **생성은 성공하는데 디스플레이가 active 목록에 안 뜨는** 상태가 있다. OpenDisplay의 대응 (`Mac/MacSender.swift:471-546`): serial+productID를 함께 +1 bump한 새 아이덴티티로 재시도(최대 3 probe), 성공한 오프셋을 영속화. 그리고 `findSCDisplay`가 최대 20회 × 250ms 폴링하며 SCShareableContent에 뜨길 기다린다.

#### 2.4 Leftcar 현재 구현과의 정합

Leftcar가 실기기에서 배운 HiDPI 교훈(`docs/EVIDENCE.md` 245행 정정: 비율 숫자 16x9가 6400x4000 백킹 스토어를 만든 사건 → 픽셀 지정 + HiDPI off + multiplier 1x 고정, `virtual_display.rs:69-81`)은 OpenDisplay의 접근과 같은 문제를 다른 방향으로 푼 것이다. OpenDisplay는 "포인트 @2x + 재적용 루프"로 HiDPI를 살리고, Leftcar는 HiDPI를 꺼서 함정을 회피했다. CGVirtualDisplay 자체 구현 시 Leftcar의 정정 기록이 "스케일 팩터 처리에서 동일 함정 재현 가능성"의 1차 참고 자료가 된다 (docs 조사 에이전트 결론).

### 3. 캡처/인코딩/전송에서 배울 점

Leftcar 파이프라인 조사 결과와 대조한 개선 후보:

1. **커서 분리 (가장 명확한 격차)**. OpenDisplay는 캡처에서 커서를 숨기고(`showsCursor = !localCursor`) 120Hz 폴링으로 정규화 좌표를 JSON으로 보내 수신측이 로컬 렌더링한다. WiFi에선 head-of-line blocking 회피용 UDP 9001 커서 전용 사이드 채널(시퀀스 번호 재정렬/드롭)까지 쓴다. 근거: 비디오에 구운 커서는 풀 파이프라인 지연(~30ms)을 타지만 로컬 에코는 ~2ms. 이는 `docs/research/2026-08-25`가 "업계 표준인데 Leftcar 미달"로 결론낸 항목과 정확히 일치하며, OpenDisplay 구현(`MacSender.swift:1467-1641`)이 참고 구현 사례가 된다.
2. **백프레셔 = pre-encode 드롭**. `pendingEncodes ≤ 1`(latest frame wins), `pendingSends ≤ 3`(TCP in-flight) 두 카운터로 드롭 지점을 분리. 핵심: **인코딩 전 드롭은 참조 체인이 유효하므로 드롭 시 강제 키프레임을 하지 않는다**(IDR pulsing/blockiness 유발). 드롭 후 30ms 디바운스로 마지막 프레임을 재인코딩해 스킵 구간을 메운다. Leftcar는 디코더 백프레셔→드롭 구조라, 호스트측 pre-encode 드롭 정책은 비교 검토 가치가 있다.
3. **정적 화면 IDR 리플레이**: 정적 화면에선 SCK가 프레임을 안 주므로, 키프레임 복구 요청 시 `lastPixelBuffer`를 호스트시간 PTS로 재인코딩해 즉시 IDR을 보낸다. 재접속 복구 체감 개선용.
4. **SCK 60Hz rate-limiter 우회**: 60Hz 디스플레이에 정확히 1/60을 요청하면 SCK가 살짝 이른 프레임을 스킵해 실측 51fps가 된다 → 1/120을 요청. Leftcar의 `minimumFrameInterval = 1/fps` 정책에 대한 실측 기반 반례.
5. **인코더**: `EnableLowLatencyRateControl` 스펙(미지원 기기 대비 폴백 재시도 필요 — AMD 전용 Mac 사례), `PrioritizeEncodingSpeedOverQuality`, 주기 키프레임 OFF(TCP에선 비트레이트 스파이크가 히컵으로 체감). Leftcar는 UDP 3600프레임 GOP로 이미 유사하나 TCP 경로 GOP는 fps*60.
6. **캡처 해상도는 픽셀 기준**: `SCDisplay.width`는 포인트라 Retina에서 절반 래스터 손실 → `CGDisplayCopyDisplayMode().pixelWidth` 사용.
7. **디코더 한계 광고(decode ceiling)**: 수신기가 hello에 `maxEncodeWide/High`를 광고하면 송신기가 데스크톱 크기는 유지한 채 스트림만 축소. "H.264 하드웨어 디코드는 5120px 미만에서 막힘"(모든 테스트 Mac 공통). Leftcar의 프로파일 협상(1080p/1440p/4K60)과 결합할 아이디어.
8. **USB 역할 배정**: "수신기가 리슨하고 송신기가 접속한다"는 역할이 WiFi/USB 동일 코드 경로를 가능케 하는 최대 공헌 설계 (PROTOCOL.md §1). Apple에선 usbmuxd 직접 클라이언트(외부 의존 없음, `/var/run/usbmuxd` Unix 소켓, plist 프레이밍, Connect 후 같은 소켓이 바이트 파이프로 변신)로 구현. Leftcar는 이미 AOAP로 USB를 갖췄고 오히려 더 범용이라 이 항목은 "검증"에 가깝다. Android 비공식 수신기에 대한 PROTOCOL.md Appendix B는 `adb reverse`면 충분다고 명시.

### 4. 라이선스/재사용 경계

- **GPL-3.0** (v0.5부터, 이전 릴리스 MIT): 코드 발췌·복사 시 Leftcar 전체가 파생물이 되어 소스 공개 의무. 클로즈드/상용 부분이 있다면 **절대 코드 복사 금지**.
- **가능한 것**: (a) PROTOCOL.md는 서드파티 구현을 예상하고 공개된 인터오퍼레이션 규격 — 와이어 포맷/시맨틱 준수 재구현은 저작권 대상 아님 (Bonjour 타입 `_opensidecar._tcp`까지 쓸 필요는 없음). (b) CGVirtualDisplay 시그니처 목록은 VirtualDisplayExp→DeskPad로 이어지는 공통 선임계 지식 — 참고 후 직접 작성 가능 (헤더 파일 텍스트 복사 금지). (c) 정성적 발견(60Hz 상한, `.forSession`, 연속 집행, 120Hz SCK 요청, 드롭 시 강제키 금지)은 사실/아이디어로 자유 인용 가능.
- **권고 경로**: 설계·운영 지식은 흡수, 코드는 0부터 작성.

### 5. 도입 결정 관점 (ADR 업데이트 필요성)

- ADR-0003/0005가 자체 구현을 기각한 근거는 "DriverKit 기반 신규 프로젝트 = 큰 새 프로젝트"였다. 그러나 **CGVirtualDisplay 경로는 DriverKit이 아니며**, OpenDisplay 사례상 단일 Swift 파일 252줄 + 74줄 헤더 + 감시 루프로 구성된다. DriverKit 드라이버 서명/배포 문제가 없고, 코드량은 BetterDisplay CLI 래퍼와 한 자릿수 배수 차이. 다만 새로운 비용이 있다: private API라 macOS 업데이트 시 파손 가능(README가 명시하는 구조적 리스크, Mac App Store 배포 불가 → 직접 배포), 60Hz 상한, 그리고 위 2.2/2.3의 macOS 세션 상태와의 싸움.
- R-015("실제 virtual display로 scope 팽창 금지, 다음 evidence 전에는 시작하지 않는다")의 트리거 조건(최소화/가림이 workflow를 반복적으로 막음 + display capture로 미해결 + 사용자가 OS-level 별도 desktop을 실제 요구)은 그대로 유효. 이 리서치는 "evidence 수집" 단계에 해당하며, CGVirtualDisplay 스파크(하루 분량, 스트리밍 캡처 연동까지) 결과를 evidence로 삼아 별도 ADR로 판단하는 것이 기존 문서 체계와 정합적이다.
- 참고: 형제 프로젝트 Andra의 검증 문서(`Andra/docs/verification/w2-virtual-display-2026-09-01.md`)가 Android 측 public `DisplayManager.createVirtualDisplay`의 제약(SDK 36에서 서드파티 앱의 public VD로 액티비티 런치 차단)을 확정해둠 — "가상 디스플레이 자체 구현의 플랫폼 제약" 사례.

## 코드 참조

- `apps/host-desktop/src-tauri/src/virtual_display.rs:27-42` — BetterDisplay create argv (픽셀 + HiDPI off + 1x 고정)
- `apps/host-desktop/src-tauri/src/virtual_display.rs:90-127` — CLI 셸아웃 (create/connect/discard)
- `apps/host-desktop/src-tauri/src/App.tsx:570-698` — 가상 디스플레이 실험 UI (게이트: localStorage `leftcar_virtual_display_experiment`)
- `native/macos-capture-shim/Sources/Capture/CaptureSession+Backend.swift:75-171` — SCK 캡처 설정 (showsCursor=true 포함)
- `native/macos-capture-shim/Sources/Encoder/` — VideoToolbox 설정/정책 (AVE/RTVC 검증, 적응 비트레이트)
- `apps/host-desktop/src-tauri/src/aoap.rs:167-260` — USB AOAP mux 2채널
- `apps/host-desktop/src-tauri/src/aoap_proxy.rs:90-168` — 루프백 TCP→USB 브리지
- `native/android-viewer/src/usb_bridge.rs:36-65, 159-214` — USB 수신 → shim datagram 재분할
- `native/android-viewer/src/renderer/single_session/` — MediaCodec low-latency 렌더 루프
- `docs/decisions/0005-virtual-display-via-betterdisplay-cli.md` — 현재 가상 디스플레이 결정 (CLI 래퍼)
- `docs/decisions/0003-window-streams-before-virtual-displays.md` — 가상 디스플레이 재검토 조건 3가지
- `docs/09-risk-register.md` R-015 — scope 팽창 경고와 시작 금지 트리거
- `docs/EVIDENCE.md:245` — BetterDisplay HiDPI 6400x4000 사건 정정 기록
- `docs/research/2026-08-25_remote-screen-display-pipelines.md` — 12개 원격 화면 제품 비교 (커서 분리 미달 결론)
- OpenDisplay 측 (로컬 사본 `/tmp/opendisplay/`, 원본 https://github.com/peetzweg/opendisplay ):
  - `Mac/CGVirtualDisplayPrivate.h` — private API 시그니처 74줄
  - `Mac/VirtualDisplay.swift:43-68, 78-91, 152-174, 225-251` — 생성/감시 루프/HiDPI 재적용/미러 해제
  - `Mac/MacSender.swift:471-546, 677-723, 1853-1917, 155-209` — 독성 아이덴티티 복구/SCK 설정/VT 설정/백프레셔
  - `Mac/Usbmux.swift` — usbmuxd 직접 클라이언트
  - `Shared/StreamReceiver.swift:1002-1169` — Annex B 파서→AVSampleBufferDisplayLayer
  - `PROTOCOL.md` — 단일 TCP 프로토콜 규격 (pv3, 698줄)

## 아키텍처 인사이트

1. **Leftcar의 강점은 전송·페어링·적응성**이고 OpenDisplay는 단일 TCP로 단순함을 택했다. OpenDisplay의 단순함은 "수신기가 리슨"이라는 역할 배정 하나로 WiFi/USB를 통일한 데서 나온다 — 추상화의 힘.
2. **가상 디스플레이 자체 구현의 실체는 "생성"이 아니라 "유지"**다. 생성 코드는 252줄이지만, macOS 세션 상태(미러링/모드 롤백/배치 복원/Stop Extending 오염)와의 싸움이 대부분이다. BetterDisplay CLI는 이 싸움을 대행해주는 것이 본질이었다.
3. **Leftcar의 HiDPI off 우회와 OpenDisplay의 @2x 재적용 루프는 같은 문제의 두 해법**이다. 전자는 안전하지만 Retina 선명도를 포기했고, 후자는 선명하지만 운영 복잡도를 감수했다. CGVirtualDisplay 도입 시 후자의 감시 루프가 사실상 필수다.
4. **커서 분리는 이제 "미달 사항"이 아니라 "참고 구현이 확보된 과제"**가 됐다. 기존 리서치의 결론(업계 표준, Leftcar 미달)에 OpenDisplay의 실제 구현 사례(120Hz JSON + UDP 사이드 채널)가 추가됐다.

## 히스토리 컨텍스트 (thoughts/ 디렉토리)

leftcar에는 `thoughts/` 디렉토리가 없다. 동일 역할 문서는 전부 `docs/` 아래에 있으며 위 "코드 참조"에 반영했다. 기존 문서가 남긴 핵심 결정/경고:

1. 자체 가상 디스플레이는 두 ADR(0003/0005)을 거쳐 **의도적으로 회피**된 선택 — 전환은 별도 ADR이 사실상 필수다.
2. 기존 "자체 구현" 논의는 전부 **DriverKit** 전제이며 **CGVirtualDisplay 검토는 저장소에 0건** — 이 리서치가 최초의 기록이다.
3. BetterDisplay CLI에서 배운 HiDPI 정정(EVIDENCE.md)은 CGVirtualDisplay 전환 시 동일 함정 경고로 이식 가능하다.
4. R-015의 v1 논골 유지 원칙 — 이 리서치는 시작이 아니라 evidence 수집 단계다.

## 관련 리서치

- `docs/research/2026-08-25_remote-screen-display-pipelines.md` — 12개 원격 화면 프로그램 파이프라인 비교 (본 리서치의 선행 문서, 커서 분리/4:4:4/이중 채널 결론)

## 미해결 질문

1. CGVirtualDisplay가 현재 macOS(Sonoma~이후)에서 여전히 동작하는가 — OpenDisplay는 macOS 14+ 요구. Leftcar 대상 OS 버전에서 실기기 스파크로 검증 필요 (60Hz 상한, `.forSession` 미러 해제, HiDPI 재적용 포함).
2. Leftcar 스트리밍 파이프라인과 CGVirtualDisplay의 정합 — 가상 디스플레이가 SCK `SCShareableContent`에 뜨는 것까지는 확인됐으나(OpenDisplay `findSCDisplay`), 420v 캡처→VT 인코딩→AOAP/UDP 전송 경로에서의 실측 지연/품질은 미측정.
3. `EnableLowLatencyRateControl` 스펙이 Leftcar 대상 기기군(Apple silicon 전반)에서 지원 범위가 어느 정도인지 — OpenDisplay의 AMD 전용 Mac 폴백 사례가 시사하는 바.
4. 커서 분리 채택 시 Leftcar의 LCS1/LCI1 채널 설계에 커서 좌표 역방향 스트림을 어떻게 넣을지 (UDP 사이드 채널 vs 기존 제어 채널 확장).
5. GPL-3.0 경계에서 "PROTOCOL.md 규격 참고 재구현"의 조직적 리스크 허용선 — 필요 시 변호사 검토 대상.
