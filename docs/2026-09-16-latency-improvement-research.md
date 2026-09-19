# 1440p60 저지연 개선 조사

조사일: 2026-09-16. 범위: 설치본의 S6 로그·현재 코드, Android MediaCodec, Apple VideoToolbox, Parsec, Moonlight, Tailscale와 IETF 공식 자료. 이번 작업은 기존 기록 분석과 조사 문서 작성이며, 새 제품 코드·설치·실기 측정은 포함하지 않는다.

## 1. 판단

**1440p60에서 지연을 더 줄일 기술적 여지는 있다. 다만 현재 자료만으로 TB710FU에서 Parsec과 같은 체감이나 특정 밀리초를 보장할 수는 없다.** 하드웨어 코덱 선택, 프레임 보관, 네트워크 혼잡 대응, 표시 시점이 각각 개선 지점이다. Parsec도 하드웨어 인코딩·디코딩, GPU 복사 최소화, 프레임 타이밍, 동적 비트레이트를 함께 설명한다. 공식 페이지의 **7ms는 자체 유선 LAN 시험 환경의 수치**이며 TB710FU·macOS·1440p60에 대한 성능 보증이 아니다. [Parsec 기술 설명](https://parsec.app/technology)

60fps의 프레임 간격은 약 16.67ms다. 이것은 처리량 기준이며 전체 경로의 지연 상한은 아니다. 파이프라인이 여러 프레임을 동시에 처리하면 60fps를 유지하면서도 오래된 프레임을 보여줄 수 있다. 따라서 FPS와 함께 프레임별 대기시간·최종 표시 시점·프레임 누락을 판단해야 한다.

**현재 가장 먼저 시도할 대상은 Host의 전송 대기와 복구 과정이다.** 해상도를 1080p로 낮추거나 프로토콜을 통째로 교체하기 전에, 1440p60을 고정하고 이 구간에서 시간이 누적되는 이유를 검증한다. 디코더 교체와 표시 방식은 그다음 비교 대상이다.

### 1.1 현재 실패를 설명하는 로컬 근거

유효한 기준은 S6 `2560×1440@60 / HEVC RTVC / Tailscale direct / UDP auto / 즉시 Surface 출력`이다. 실제 고동작 장면과 태블릿 표시 대응을 확인한 181.31초 기록이며, APK SHA는 `8432ffeb1056c6df042a47fe50c19e57295ec0b6e666c191259c7ac746c45b5d`다. 같은 LAN에서의 Tailscale 직접 연결이므로 외부 인터넷 경로의 성능 증거는 아니다. [S6 측정 결과](/Users/loopy/dev/ll3/leftcar/artifacts/performance-acceptance-2026-09-16/s6-tailnet-1440-auto-fgs-metrics.json), [기존 검증 보고서](/Users/loopy/dev/ll3/leftcar/docs/2026-09-16-adb-acceptance-followup.md)

| 관측 | S6 결과 | 판단 범위 |
| --- | --- | --- |
| 캡처 / 소프트웨어 영상 출력 | 카운터 기준 59.93 / 57.36fps | 캡처는 60fps에 가깝지만 출력 안정성 기준은 실패했다. |
| 캡처→Surface release | 스냅샷 중앙값 56ms, p95 150ms | 물리적인 입력→화면 지연이 아니다. |
| 캡처→인코더 출력 | 각 상태 표본의 구간 p95 중앙값 12.59ms | 전체 프레임 p95와 다르지만, 인코더만으로 150ms를 설명할 근거는 없다. |
| 전송 대기열의 가장 오래된 프레임 | 순간 표본 p95 67.12ms, 최대 117.90ms | 이미 대기열에서 꺼내 전송 중인 프레임은 제외한 값이다. |
| 대기열 길이 | 최대 8프레임, 234,432바이트 | 181개 표본 중 16개에서 가장 오래된 프레임이 33.33ms를 초과했다. |
| 전송 처리 | 전송 전체 시간의 구간 p95 중앙값 18.18ms, 그중 send 호출 합계 p95 중앙값 5.88ms | 서로 다른 분위수를 빼서 순수 대기시간으로 계산하지 않는다. |
| 네트워크 / 복구 | RTT 스냅샷 p95 47ms, 새 복구 키프레임 8회 | 앱의 probe RTT에는 수신 루프 처리 대기도 영향을 줄 수 있다. |
| 누적 카운터 변화 | Host networkQueueDropped +143, decoded output discard +298, decoder input drop 0 | 구간이 다르고 영향이 겹칠 수 있으므로 더해 손실 프레임 수로 삼지 않는다. |

근거: [원본 상태 JSONL](/Users/loopy/dev/ll3/leftcar/artifacts/performance-acceptance-2026-09-16/s6-tailnet-1440-auto-fgs.jsonl), [단계별 스냅샷 재분석](/Users/loopy/dev/ll3/leftcar/artifacts/performance-acceptance-2026-09-16/s6-stage-snapshot-audit.json). 기존 gate를 같은 원본에 다시 적용해 출력 안정성·loss·recovery 실패가 재현됐다.

**관측과 원인 가설은 구분한다.** 33.33ms 초과 대기 표본의 RTT 중앙값은 21ms, 나머지는 25ms였다. 현재 낮은 해상도의 시계열만으로 “무선 RTT가 올라서 모든 대기가 생겼다”라고 단정할 수 없다. queue age, 전송 중인 프레임, 복구 요청, 수신·출력 시각을 프레임별로 연결해야 한다. `maxAuBytes=381,882`는 구간 시작부터 동일한 생애 최대값이므로 S6 중 관측한 키프레임 크기로 해석하지 않는다.

### 1.2 이미 구현된 최적화와 남은 후보

- **전송:** UDP, FEC, 일부 NACK 복구, 묶음 pacing, 큰 프레임 가속과 송신 deadline이 이미 있다. [pacing 정책](/Users/loopy/dev/ll3/leftcar/native/macos-capture-shim/Sources/Transport/UdpPacingPolicy.swift), [UDP 송신](/Users/loopy/dev/ll3/leftcar/native/macos-capture-shim/Sources/Transport/CaptureSession+UdpPacket.swift). 따라서 새 UDP 도입이나 단순 pacing 해제가 해결책은 아니다. `paceUdpDatagram`의 예정 시각·실제 기상 시각·암호화·send 시간을 따로 기록해 스케줄링 초과가 누적되는지 확인한다.
- **대기열:** 영상 모드 UDP는 3칸, 다른 UDP 모드는 8칸이다. S6에서는 실제 최대 8개가 관측됐다. 프레임 개수만으로 지연을 제한하지 못하므로 시간 예산과 인코딩 전 유입 제한을 비교한다. [큐 제한](/Users/loopy/dev/ll3/leftcar/native/macos-capture-shim/Sources/Capture/CaptureSession.swift:180), [참조 관계를 보존하는 큐 처리](/Users/loopy/dev/ll3/leftcar/native/macos-capture-shim/Sources/Transport/CaptureSession+NetworkQueue.swift). 압축된 P-frame 중 최신 것만 남기면 뒤의 영상도 깨질 수 있다. 앞단의 원본 프레임을 건너뛰는 방법과 이미 압축된 참조 연결의 복구를 구분한다.
- **혼잡 대응:** 현재 ABR은 drop, 송신 지체, 손실 또는 큰 RTT/wire 증가를 본다. RTT는 50ms 이상이면서 이전보다 30ms 이상 증가하는 조건이므로 서서히 쌓이는 대기가 이 조건만으로 잡히지 않을 수 있다. 지속적인 높은 기준 RTT 때문에 화질이 계속 내려가던 문제를 다시 만들지 않도록, 최소 RTT 대비 추가 대기와 추세·Host 큐 시간을 함께 시험한다. [ABR 구현](/Users/loopy/dev/ll3/leftcar/native/macos-capture-shim/Sources/Encoder/CaptureSession+AdaptiveBitrate.swift)
- **디코더:** `low-latency=1`, `priority=0`, operating rate와 QTI용 일부 옵션, 이름 지정 실패 시 fallback이 이미 있다. QTI 추가 옵션은 현재 H.264 경로에 적용된다. 새 옵션을 무작정 추가하기보다 실제 선택된 HEVC/H.264 디코더와 input→output 시간을 기록한다. [Android decoder](/Users/loopy/dev/ll3/leftcar/crates/viewer-decoder/src/android/decoder.rs)
- **표시와 시계:** S6은 균형 표시 대기 없이 출력했으므로 그 옵션을 끄는 것은 새 개선안이 아니다. 현재 offset은 왕복 probe의 양방향 지연 대칭을 가정한 추정치다. 비대칭 지연과 loop 대기가 영상 age를 흔들 수 있으므로 offset·probe 표본도 기록한다. 이는 계측 한계이며 측정된 Host 큐 대기 자체를 없애는 설명은 아니다. [시계 추정](/Users/loopy/dev/ll3/leftcar/native/android-viewer/src/input_protocol.rs:324), [age 계산](/Users/loopy/dev/ll3/leftcar/native/android-viewer/src/renderer/single_session/feedback.rs:132)

## 2. 지연 수치를 먼저 같은 의미로 맞추기

| 지표 | 의미와 해석 한계 |
| --- | --- |
| Parsec encode/decode | 각 장치가 한 프레임을 인코딩/디코딩하는 시간이다. 전체 체감 지연이 아니다. |
| Parsec network | 호스트↔클라이언트 **왕복 시간(RTT)**이다. 영상의 단방향 전송시간으로 취급하지 않는다. |
| capture → Surface release | 디코딩한 버퍼를 Surface에 넘기는 호출까지의 시간이라면, 이후 표시 대기를 포함하지 않는다. |
| Surface rendered callback | Surface 렌더 시점을 더 직접적으로 확인할 수 있다. 콜백 도착 시간이 아니라 콜백이 전달하는 렌더 시각을 사용한다. |
| 실제 입력 → 화면 반응 | 입력 전송, 호스트 처리, 영상 경로, 디스플레이 반응을 모두 포함하는 별도 측정이다. |

Parsec 지표 정의는 [공식 오버레이 설명](https://support.parsec.app/hc/en-us/articles/32381603663636-Stream-Overlay-Stats-and-Logging), Surface 반환 의미는 [MediaCodec](https://developer.android.com/reference/android/media/MediaCodec#releaseOutputBuffer(int,%20boolean)), 렌더 콜백 의미는 [OnFrameRenderedListener](https://developer.android.com/reference/android/media/MediaCodec.OnFrameRenderedListener)에 근거한다. 마지막 행은 이 API 경계에 따른 측정 설계상의 구분이다.

**주기적으로 읽은 최신 지연 값의 p95는 모든 프레임의 p95와 다르다.** 같은 오래된 값을 여러 번 읽을 수 있고 중간 프레임의 피크를 놓칠 수도 있다. `snapshot p95 150ms`를 곧바로 “95%의 프레임이 150ms 안에 표시됐다”로 해석하지 않는다. 또한 RTT p95와 영상 지연 p95는 서로 다른 표본이므로 둘을 빼서 디코더 시간을 구할 수 없다.

권장 측정 설계는 프레임 ID/PTS별 `capture → encode callback → 전송 완료 → 재조립 완료 → codec input → codec output → Surface release → rendered timestamp`다. 장치 간 구간에는 시계 동기화 방식과 오차를 함께 기록하고, 장치 내 구간은 같은 monotonic clock으로 계산한다. 렌더 지연은 물리적인 화면 반응의 대체 증거로 과장하지 않는다.

## 3. Android: 저지연 옵션과 디코더 선택

### 3.1 지원 확인과 활성화는 별개

Android의 저지연 디코딩은 API 30부터 제공된다. 실제 `codec + MIME`의 `FEATURE_LowLatency` 지원을 검사하고, 지원 시 `MediaFormat.KEY_LOW_LATENCY = 1`로 요청해야 한다. 기본값은 0이다. 이 모드는 코덱 규격이 요구하는 수준을 넘는 입력·출력 보관을 줄인다. [CodecCapabilities](https://developer.android.com/reference/android/media/MediaCodecInfo.CodecCapabilities#FEATURE_LowLatency), [MediaFormat](https://developer.android.com/reference/android/media/MediaFormat#KEY_LOW_LATENCY)

**Android 16만으로 지원 여부가 확정되지 않는다.** AOSP는 SoC 제조사의 디코더 드라이버 지원이 필요하다고 설명한다. 활성화한 디코더는 불필요하게 다음 입력을 기다리지 않고 가능한 빨리 출력을 반환하며 깨어 있어야 한다. 따라서 저지연 효과와 함께 발열·소비전력도 비교 대상이다. [AOSP 저지연 디코딩](https://source.android.com/docs/core/media/low-latency-media)

적용 전 확인사항:

1. H.264/HEVC 디코더를 모두 열거하고 이름, 하드웨어 여부, `FEATURE_LowLatency`, 2560×1440@60 지원, profile/level을 기록한다.
2. 현재 실제 선택된 디코더 이름을 후보 목록과 대조한다. MIME으로 처음 발견한 디코더가 가장 빠른 구현이라는 보장은 없다.
3. 선택한 구현에서 저지연 설정의 수용 여부와 실제 input→output 시간을 확인한다. 설정 요청 성공만으로 지연 개선을 판정하지 않는다.

Moonlight는 지원되는 모든 디코더를 탐색하면서 저지연 기능이 있는 구현을 먼저 고른다. 기본 구현 뒤에 별도의 저지연 구현이 나오는 기기 사례도 기록한다. 표준 옵션을 우선 쓰고 제조사별 옵션은 예외 처리와 단계적 fallback으로 다룬다. TB710FU에는 해당 기기의 실제 코덱 이름과 지원 결과를 확인한 뒤 적용해야 한다. [Moonlight 디코더 선택·옵션 소스](https://github.com/moonlight-stream/moonlight-android/blob/b48494cb96bff23d8886c4775cc4f39a1075495d/app/src/main/java/com/limelight/binding/video/MediaCodecHelper.java), [디코더별 알려진 제약](https://github.com/moonlight-stream/moonlight-android/blob/b48494cb96bff23d8886c4775cc4f39a1075495d/decoder-errata.txt)

### 3.2 Surface 대기와 화면 표시를 따로 확인

SurfaceView의 시간 지정 출력은 해당 시각 이후 VSYNC에 맞춰 표시될 수 있다. 미래 시각으로 보낸 버퍼는 뒤의 버퍼를 막을 수 있고, 같은 VSYNC에 보낸 여러 버퍼는 마지막 것만 표시될 수 있다. **호스트 PTS와 Android 시스템 시각을 그대로 혼용하는지**, 의도치 않은 미래 예약 또는 Surface 대기가 생기는지 확인할 필요가 있다. [MediaCodec 시간 지정 출력](https://developer.android.com/reference/android/media/MediaCodec#releaseOutputBuffer(int,%20long))

TB710FU의 Android 16에서는 NDK `AMediaCodec_setOnFrameRenderedCallback`을 검토할 수 있다(Android T부터 제공). 콜백은 늦게 도착하거나 묶여 전달될 수 있으므로 `systemNano`를 프레임 PTS와 연결한다. NDK 문서는 콜백 누락 가능성도 명시하므로 콜백 수와 출력 수를 함께 기록한다. 콜백에서는 무거운 작업을 하지 않는다. [Android NDK Media](https://developer.android.com/ndk/reference/group/media#amediacodec_setonframerenderedcallback)

Moonlight Android는 **디코딩 직후 즉시 표시**와 **최대 한 프레임을 버퍼링하는 균형 모드**를 구분한다. 전자는 지연을 줄이고, 후자는 네트워크 흔들림과 호스트·클라이언트 VSYNC 차이를 완충한다. 이 프로젝트에서도 표시 프레임 간격과 지연을 함께 측정해 선택할 후보이지, 무조건 대기 프레임을 추가할 근거는 아니다. [Moonlight 공식 FAQ](https://github.com/moonlight-stream/moonlight-docs/wiki/Frequently-Asked-Questions#what-do-the-frame-pacing-options-on-the-android-client-mean)

## 4. macOS: VideoToolbox 설정과 실제 동작

| 후보 | 공식 의미 | 적용 전 확인사항 |
| --- | --- | --- |
| `EnableLowLatencyRateControl = true` | 저지연 인코더 선택과 모드 활성화. 현재 문서는 frame reorder/lookahead 제거, 초기 IDR 뒤 P-frame으로 이어지는 GOP 등을 설명한다. | session 생성 결과, 실제 encoder ID와 하드웨어 사용, 출력 패턴을 기록한다. [속성 설명](https://developer.apple.com/documentation/videotoolbox/kvtvideoencoderspecification_enablelowlatencyratecontrol) |
| `RealTime = true` | 실시간 처리 권고다. 특정 ms 이내 완료나 60fps 보증이 아니다. | 설정 반환값과 실제 capture→encode callback 분포를 비교한다. [RealTime](https://developer.apple.com/documentation/videotoolbox/kvtcompressionpropertykey_realtime) |
| `AllowFrameReordering = false` | B-frame 인코딩에 필요한 프레임 재정렬을 막는다. | 이미 적용되어 있다면 추가 개선량으로 계산하지 않는다. [AllowFrameReordering](https://developer.apple.com/documentation/videotoolbox/kvtcompressionpropertykey_allowframereordering) |
| `MaxFrameDelayCount` | 인코더가 출력을 내기 전 보관할 수 있는 프레임 수를 제한한다. 기본은 무제한이다. | 지원 범위와 설정 결과를 확인하고, 허용되는 작은 값의 처리량·드롭·callback 지연을 비교한다. 이는 wall-clock 제한이 아니다. [MaxFrameDelayCount](https://developer.apple.com/documentation/videotoolbox/kvtcompressionpropertykey_maxframedelaycount) |

**HEVC를 무조건 배제하지 않는다.** 2021 WWDC 강연은 당시 H.264 저지연 모드를 소개했지만, 현재 Apple 공식 저지연 회의 샘플은 HEVC Main/Main10 설정 분기를 포함한다. 해당 샘플은 인코더마다 지원 속성이 다를 수 있다고 설명하고, `ExpectedFrameRate`는 rate-control 힌트이며 실제 프레임률은 입력에 따른다고 명시한다. 따라서 현행 HEVC RTVC와 H.264를 같은 해상도·FPS·장면에서 비교하되, 이름만 보고 우열을 결정하지 않는다. [현재 Apple 샘플](https://developer.apple.com/documentation/videotoolbox/encoding-video-for-low-latency-conferencing), [2021 발표의 당시 범위](https://developer.apple.com/videos/play/wwdc2021/10158/)

지원 속성은 `VTSessionCopySupportedPropertyDictionary`, 설정 후 값은 `VTSessionCopyProperty`로 확인할 수 있다. 값 요청·지원·수용·실측 효과를 구분해서 남긴다. [VideoToolbox 세션 속성 API](https://developer.apple.com/documentation/videotoolbox/vtsession-api-collection?language=objc)

## 5. 복구 키프레임과 네트워크

Apple은 손실 후 큰 키프레임을 보내면 혼잡이 더 심해질 수 있다고 설명한다. H.264 저지연 모드의 LTR은 수신 확인된 참조 프레임으로부터 더 작은 복구 프레임을 만드는 선택지이지만, 애플리케이션의 프레임 ACK와 인코더 연동이 필요하다. **키프레임 개수만 줄이는 설정 변경으로 취급하면 안 된다.** 현재 세션의 복구 발생 시각, 원인, 키프레임 크기, 전송 소요시간, 다음 정상 출력 시각이 먼저 필요하다. [Apple LTR 설명](https://developer.apple.com/videos/play/wwdc2021/10158/?time=1051)

Parsec은 혼잡 상황에서 비트레이트를 낮추며, 공식 문제 해결 문서는 대역폭 제한을 낮추거나 유선/5GHz 경로를 비교하도록 안내한다. 이 프로젝트의 후보 실험도 코덱 변경과 네트워크 조건 변경을 분리해야 한다. [Parsec 오버레이·혼잡 설명](https://support.parsec.app/hc/en-us/articles/32381603663636-Stream-Overlay-Stats-and-Logging), [Parsec 지연 문제 해결](https://support.parsec.app/hc/en-us/articles/32381352822804-Troubleshooting-Lag-Latency-and-Quality-Issues)

### 5.1 P2P·UDP로 더 공격적으로 보내도 되는가

**직접 P2P는 유리하지만, 그 경로에 여유가 있을 때만 전송 속도를 올리는 것이 맞다.** Tailscale은 직접 연결이 일반적으로 relay보다 지연·처리량에 유리하다고 안내한다. 직접 연결이 무선 링크의 혼잡이나 물리적인 거리까지 없애는 것은 아니다. Linux용 GRO 설정은 Linux subnet router/exit node 조건의 안내이므로 현재 macOS↔Android 단말에 그대로 적용하지 않는다. [Tailscale 성능 지침](https://tailscale.com/docs/reference/best-practices/performance)

UDP 애플리케이션도 burst 크기와 혼잡을 관리해야 하며, 경로 MTU를 넘는 패킷은 피해야 한다. 현재 Tailnet용 plaintext 1200바이트는 IPv4/UDP·앱 암호화 후 1252바이트여서 관측한 터널 MTU 1280 안에 들어간다. P2P라는 이유로 이를 다시 1400바이트로 키우지 않는다. [IETF UDP 지침, §3.1.6·§3.2](https://www.rfc-editor.org/rfc/rfc8085.html), [프로젝트 MTU 검증](/Users/loopy/dev/ll3/leftcar/docs/2026-09-16-adb-acceptance-followup.md:90)

권장 방향은 **경로 여유에 따라 묶음 크기·전송 속도를 올리되, 큐 시간이 늘면 인코딩 비트레이트와 유입량을 조절하는 것**이다. 실시간 혼잡 제어의 목표는 사용할 수 있는 대역폭을 활용하면서 스스로 만든 대기를 줄이는 것이다. 현재의 30ms 단위 급증 감지만으로 충분한지는 실험해야 한다. [IETF 실시간 혼잡 제어 요구사항](https://www.rfc-editor.org/rfc/rfc8836.html)

기존 custom16 오프라인 전송 스케줄 재생 시험의 p95 중앙값은 22.69→11.40ms로 줄었다. 그러나 암호화·실제 소켓·무선·태블릿이 빠져 있다. 과거 S2–S4의 실제 표시 장면 대응도 불충분했으므로 이 결과로 custom16의 실기 우위를 주장하거나 기본값을 바꾸지 않는다. WebRTC/QUIC로의 전면 교체 역시 이 대기 원인을 자동으로 없앤다는 증거가 없어 우선순위가 낮다.

## 6. 제안하는 다음 순서

다음은 공식 자료를 현 상황에 적용한 **조사자의 제안**이다. 효과나 원인이 이미 입증됐다는 뜻은 아니다.

| 순서 | 실험 | 기대하는 이점 | 비용·실패 조건 |
| --- | --- | --- | --- |
| 0 | 프레임별 단계 시각, 실효 codec/속성, probe offset, 복구 원인을 JSON으로 연결 | Host 대기·전송·디코딩·표시를 분리해 변경 효과를 판정 | 기록 자체의 부하도 점검. 기존 스냅샷과 병행해 기준을 바꾸어 통과시키지 않음 |
| 1 | HEVC·1440p60·비트레이트·FEC 고정 후, 현 pacing과 지연 예산이 있는 pacing 비교 | 67–118ms Host 큐 대기 축소가 첫 목표 | 무제한 burst 금지. RTT·불완전 AU·복구가 늘면 채택하지 않음 |
| 2 | 큐 시간에 따른 인코딩 전 유입 제한, 이후 지연 추세를 반영하는 ABR을 각각 비교 | 압축 참조가 깨져 복구 키프레임을 반복 요청하는 상황 예방 | 원본 건너뛰기는 FPS를 낮출 수 있음. 57fps/안정성 기준을 함께 충족해야 함 |
| 3 | 같은 고정 비트레이트에서 LAN과 Tailnet direct 비교, 이후 각 경로에서 비트레이트만 비교 | 네트워크 경로와 송신량이 만드는 지연 분리 | VPN on/off 외 설정·코덱·부하를 같이 바꾸면 인과 비교 불가 |
| 4 | 경로 고정 후 실제 저지연 디코더를 확인한 H.264/HEVC 비교 | 기기에 더 짧은 codec 경로 선택 가능 | 동일 비트레이트와 비슷한 화질 조건을 따로 비교. 글자·색·빠른 장면 품질 저하는 별도 기록 |
| 5 | 디코더 input→output와 rendered 시각에 따라 즉시 표시/한 프레임 균형 표시 비교 | 지연과 화면 간격의 균형 선택 | 균형 모드는 대기가 추가될 수 있음. 평균 FPS만 개선되면 저지연 성공으로 보지 않음 |
| 후속 | 실제 키보드·포인터 입력과 외부 인터넷 Tailnet 경로, 장시간 발열 | 체감·외부 사용·지속 성능 확인 | ADB 입력은 물리 키/IME를 대체하지 않으며 LAN 직접 연결은 WAN 합격이 아님 |

앞 순서에서 이미 목표를 충족하면 효과가 불분명한 변경을 계속 더하지 않는다. 복구가 여전히 주요 원인일 때에만 LTR 같은 참조 복구를 후속 과제로 검토한다. 기존 합성 인코더 시험에서 AVE는 1440p 처리량이 좋았지만 callback p95 38.49ms로 RTVC 11.61ms보다 길었다. 이 H.264 단독 시험을 실제 HEVC 경로의 우열로 확장하지 않고, AVE 교체는 현재 지연 개선의 첫 선택에서 제외한다.

### 6.1 비교 조건과 합격 판단

- 동일한 고동작 장면과 1440p60을 고정하고 한 번에 한 변수만 바꾼다. 유망 후보는 기준/후보 각각 180초 이상, 순서를 번갈아 최소 3회 재현한 뒤 장시간 시험으로 확장한다. 고정 비트레이트 10/14/20Mbps는 탐색 후보이며 제품 기본값이나 화질 합격값이 아니다. 높은 단계는 낮은 단계에서 여유가 확인된 뒤 시도한다.
- Host/APK 해시, 실제 해상도·codec·디코더 이름·표시 모드·경로·MTU·주사율·발열을 기록한다. 모션 페이지가 Host 캡처 대상이고 태블릿에도 실제 전달되는지 최소한의 시작/종료 확인을 남긴다. 이후 반복 측정은 JSON 중심으로 한다.
- 기존 기준인 **57fps 이상인 표본 ≥90%, capture→Surface release 스냅샷 p95 ≤50ms, RTT 스냅샷 p95 ≤20ms, 새 gap·복구·송신 실패·비트레이트 하한 압박 0**을 그대로 함께 평가한다. 새 프레임별 지표가 생겨도 옛 기준과 바꿔치기하지 않는다.
- Host 큐 대기는 줄었지만 RTT/손실이 증가하면 과도한 burst다. 큐·전송이 정상인데 codec input→output이 길면 디코더를 우선한다. 모든 소프트웨어 구간이 짧고 rendered가 늦으면 표시 시점을 조사한다. 네트워크 자체의 지연이 기준을 초과하면 그 경로에서는 소프트웨어 개선과 경로 성능 한계를 분리해 보고한다.
- 실제 한·영 키/IME, 두 손가락 스크롤, 가운데 클릭은 별도 물리 입력 검증을 유지한다. 빠른 영상만으로 입력 호환성을 합격 처리하지 않는다.

최종 비교는 동일 장면·실제 APK/Host 빌드·해상도·주사율·전송 경로를 명시하고, 프레임별 지연 분포·지속 출력률·프레임 누락·복구 횟수·장시간 발열을 함께 기록한다. Parsec과의 체감 동등성은 같은 조건의 물리적 입력/화면 반응 비교가 있어야 주장할 수 있다.
