# 저지연 스트리밍 병목 조사와 개선 방향

문서 상태: 조사 결과 0.1
조사 기준일: 2026-08-26
기준 코드: HEAD `5febc91` (`feat/recovery-fec-abr-zerocopy`)와 조사 시점의 working tree
범위: macOS Host → Android Viewer의 화면 영상 경로
제외 범위: ADB를 이용한 수집과 검증

이 문서는 “왜 화면이 빠르게 바뀔 때 느려지는가”를 현재 구현, 기존 측정값,
실행 가능한 로컬 검사, 공개 구현 사례로 분해한 조사 기록이다. 기존 고모션
표본과 구현 후 정적 smoke 표본을 구분해 기록한다. 따라서 관측값과 추정값을
섞지 않으며, 아래 판정 상태를 유지한다.

## 0. 판정 규칙

| 상태 | 의미 |
| --- | --- |
| `확인됨` | 현재 코드 또는 재현 가능한 기존 측정값으로 직접 확인했다 |
| `반증됨` | 현재 증거가 해당 가설을 주원인으로 보기 어렵게 만든다 |
| `유력` | 코드와 관측값이 일치하지만 단계별 실시간 상관관계가 아직 없다 |
| `계산` | 코드의 상한·비율을 계산한 결과이며 런타임 측정값이 아니다 |
| `미검증` | 실기기, 고속 카메라, 네트워크 계측 등이 없어 아직 판정할 수 없다 |

제품 요구사항의 증거 수준(E0~E7)은 [문서 인덱스](README.md)의
“검증 수준”을 따른다. 빌드, 단위 테스트, APK 생성은 실제 픽셀이 표시된
증거(E6)나 장시간 성능 증거(E7)가 아니다.

## 1. 결론 요약

현재 가장 설명력이 높은 원인은 **4K 입력에서 macOS 캡처와 하드웨어 인코더
출력 속도가 60fps를 따라가지 못하는 것**이다. 기존 4K 기록은 실제 HEVC
디코더와 Surface 경로까지 연결되었지만 Host 출력이 약 34~37fps, 영상 처리
구간이 약 65~81ms였고, 4K 55~60fps에는 도달하지 못했다. 따라서 단순히
대역폭을 늘리는 것만으로는 해결되지 않는다.

그 위에 다음 세 가지 증폭기가 겹칠 가능성이 있다.

1. 현재 pacer가 `encoded bitrate`와 FEC를 분리해 계산하므로 실제 wire
   bitrate보다 느슨하거나 빠르게 동작할 수 있다. 1200-byte 데이터그램의
   250µs 최소 간격은 약 38.4Mbps의 payload 상한이다.
2. 수신 측이 한 번에 3개보다 많은 완성 프레임을 최신 프레임 하나로 접을
   때, 중간 frame id가 건너뛰어 IDR 복구를 유발할 수 있다. 이것은 고정된
   지연 큐를 쌓지 않는 데는 유리하지만, 송신 burst와 결합하면 “burst →
   collapse → gap → IDR → 큰 keyframe burst” 루프가 될 수 있다.
3. `SO_SNDBUF=512KiB`와 각 단계의 큐는 `pendingFrame=0`만으로 보이지 않는
   지연을 만들 수 있다. 큐의 개수보다 **가장 오래된 데이터의 나이와
   바이트 수**를 추적해야 한다.

반대로 Android 디코더는 현재 MediaCodec input을 기다리지 않고, output도
오래된 프레임을 쌓지 않으며, Surface로 직접 내보내도록 설계되어 있다.
따라서 Android 디코더를 첫 번째 범인으로 단정할 근거는 약하다. 다만
`Surface release`가 실제 디스플레이 광자 표시를 증명하지는 않으므로, 최종
표시 지연은 고속 카메라로 별도 검증해야 한다.

## 2. 조사 대상 파이프라인

```text
Host display/window
  │ capture callback
  ▼
latest-frame slot / capture queue
  │ pixel-buffer handoff
  ▼
VideoToolbox hardware encoder
  │ access unit + timestamp
  ▼
AU fragmentation + FEC
  │ queue age / wire-rate pacer
  ▼
UDP socket + LAN
  │ receive burst / reorder / loss
  ▼
Android AU assembly + FEC recovery
  │ live-edge selection / IDR gate
  ▼
MediaCodec input → decoder output
  │ Surface release
  ▼
Android compositor / display photon
```

제어, feedback, RTT probe는 영상과 분리된 경로다. 영상 지연을 줄이기 위해
제어 경로의 신뢰성이나 입력 ACK를 영상 데이터에 섞어서는 안 된다.

단계별로 다음 질문에 답해야 한다.

| 단계 | 확인할 질문 | 필요한 지표 |
| --- | --- | --- |
| Capture | 화면 변화가 들어왔을 때 callback이 밀리는가 | capture FPS, callback age, capture replacement/drop |
| Encode | 인코더가 입력을 소화하지 못하는가 | submit FPS, output FPS, capture→encode p50/p95/p99, in-flight |
| Packetize | 한 AU가 너무 크거나 FEC가 burst를 만드는가 | AU bytes, fragment count, parity count, packetization time |
| Pacer/socket | 송신이 queue와 kernel buffer에서 기다리는가 | queue bytes/age, pace interval, send duration, EAGAIN, socket buffer |
| Network | 실제 손실·재정렬·지터가 있는가 | received, loss, reorder, duplicate, inter-arrival p95 |
| Reassembly | 완성 프레임이 늦거나 incomplete 되는가 | first-fragment age, assembly timeouts, incomplete AU |
| Decoder | input/output 또는 decoder surface가 밀리는가 | input unavailable, output drops, decode time, rendered FPS |
| Compositor | Surface까지 갔지만 표시가 늦는가 | display refresh, frame-present timestamp, 고속 카메라 |

## 3. 현재 증거와 구현 대조

### 3.1 기존 측정값

| 관측 | 판정 | 근거와 한계 |
| --- | --- | --- |
| 1080p에서 Host 59~61fps, capture→encode p95 약 19ms, send block 약 2ms | `확인됨` | [구현 증거](EVIDENCE.md)의 기존 1080p 기록. 짧은 LAN 측정이며 장시간·4K 증거는 아니다 |
| 4K HEVC 설정이 실제 Android 저지연 HEVC decoder로 연결되고 rendered counter가 증가 | `확인됨` | [구현 증거](EVIDENCE.md)의 4K 기록. 화면 광자 표시까지의 증거는 아니다 |
| 4K Host/native 출력 약 34~37fps, 처리 약 65~81ms | `확인됨` | 기존 4K 기록. 현재 주원인 후보를 Host 처리로 좁힌다 |
| 기존 기록에서 UDP send failure가 0 | `확인됨` | `send()`가 실패하지 않았다는 뜻이다. 무선 손실, 수신 지연, kernel queue backlog가 없다는 뜻은 아니다 |
| 4K 55~60fps에 도달하지 못함 | `확인됨` | 기존 기록. 4K 고품질 모드를 기본값으로 삼을 수 없는 근거다 |
| 새 recovery/FEC/ABR 변경 후 4K 장시간 상관관계 | `미검증` | 이전 `SKIP/LOSS` 기록은 새 로직을 대표하지 않으며 재수집이 필요하다 |

### 3.2 현재 코드에서 확인한 구조

| 구현 | 판정 | 의미 |
| --- | --- | --- |
| ScreenCaptureKit `queueDepth=3`, `minimumFrameInterval=1/fps` | `확인됨` | 60fps에서 capture queue만으로 이론상 여러 프레임의 대기가 생길 수 있다. 실제 callback 시각을 측정해야 한다 |
| 고해상도 입력의 encoder in-flight limit이 5 | `확인됨` | 4K에서 최대 다섯 작업이 겹친다. 처리시간이 65~81ms면 오래된 작업이 계속 살아 있을 가능성이 있다 |
| capture는 최신 pending pixel buffer로 교체하고 replacement를 센다 | `확인됨` | 지연 큐를 제한하는 장치다. 이 카운터가 증가하면 “느리지만 모든 프레임 처리”가 아니라 “최신 프레임 우선 드롭” 상태다 |
| UDP 데이터그램 pacing 최소 간격 250µs, 최대 4000µs | `확인됨` | pacing 계산이 실제 wire overhead와 FEC parity를 포함하는지 별도 확인해야 한다 |
| video UDP socket `SO_SNDBUF=512KiB`, nonblocking | `확인됨` | `pendingFrame=0`이어도 kernel 송신 버퍼에 수십~100ms 이상이 존재할 수 있다 |
| 다중 fragment AU를 FEC로 보호하고, full group은 최대 8 data + 2 parity를 사용 | `확인됨` | 짧은 tail이나 reduced delta는 1 parity가 될 수 있다. 최대 parity는 payload 기준 25% 추가이며 수신 복구력과 wire burst 사이의 trade-off다 |
| Android 수신은 `recvmmsg`로 burst를 받고, 완성 AU를 bounded array로 처리 | `확인됨` | syscall 수와 메모리는 제한되어 있다. burst당 처리시간과 live-edge discard를 함께 봐야 한다 |
| Android live-edge는 완성 프레임이 3개보다 많으면 최신 프레임을 선택 | `확인됨` | stale latency를 막지만 frame gap/IDR과 결합될 가능성이 있다 |
| MediaCodec input timeout은 0이고 output은 오래된 renderable을 버리고 최신 것을 Surface에 release | `확인됨` | 고정된 decoder queue를 쌓지 않는 구조다. decoder가 원인인지 별도 카운터가 필요하다 |

세부 구현은 [시스템 아키텍처](03-system-architecture.md),
[성능 측정 계획](06-benchmark-device-validation.md),
[구현 증거](EVIDENCE.md), 그리고 현재 작업 중인
[60fps 계측/1440p 계획](superpowers/plans/2026-08-26-60fps-instrumentation-and-1440-baseline.md)을 함께 본다.

## 4. 가설 레지스터

### H-01. 4K macOS capture/encode가 60fps를 따라가지 못한다 — `유력`

기존 4K에서 Host output이 34~37fps이고 processing이 65~81ms인 점이 직접적인
근거다. 60fps의 frame budget은 16.67ms이므로, 평균 처리시간이 그보다 몇 배
긴 상태에서는 네트워크를 빠르게 만들어도 새 프레임이 제때 생성되지 않는다.

검증:

- 같은 장면에서 1080p H264, 1440p H264, 4K HEVC를 각각 측정한다.
- `captureFps`, encoder submit/output FPS, capture→encode p95/p99,
  in-flight, `captureQueueDropped`를 같은 run에 기록한다.
- 4K in-flight를 1/2/5로 비교한다. output FPS가 오르지만 drop 또는 queue age가
  악화되면 병렬성의 이득보다 backlog 비용이 크다.

판정 기준: Host encoded FPS가 55 이상이고 capture→encode p95가 안정적으로
frame budget에 접근하기 전에는 4K 60을 지원 모드로 광고하지 않는다.

### H-02. pacing이 FEC와 실제 wire rate를 제대로 반영하지 못한다 — `계산`, `유력`

현재 interval은 대략 다음 구조다.

```text
interval_us = clamp(payload_bytes * 8 * 1e6 / configured_bitrate,
                    250, 4000)
```

1200-byte 데이터그램을 250µs 간격으로 보내면 payload 기준 약 38.4Mbps다.
FEC full group이 8 data + 2 parity라면 parity만 25% 추가된다. 짧은 tail이나
reduced delta는 1 parity가 될 수 있다. 예를 들어 encoded payload가 36Mbps이면
full-group parity를 포함한 wire payload는 약 45Mbps가 되고, UDP/IP/
미디어 헤더는 더해진다. 실제 코드가 parity를 `configured_bitrate`에 넣지
않으면 송신 예산과 수신이 보는 양이 불일치한다.

검증:

- AU별 data bytes, parity bytes, 실제 datagram count, 첫 전송~마지막 전송
  duration을 함께 기록한다.
- `configured bitrate`, `data bitrate`, `wire bitrate`, queue age를 분리한다.
- 현재 기본 구현은 FEC를 끄는 profile이 없으므로, off/8+1/8+2 A/B는 실험용
  feature/profile을 먼저 추가한 뒤 동일 장면에서 비교한다.

개선 방향: pacer의 기준을 encoded bitrate가 아니라 `wire budget`으로 만들고,
FEC 비율 변경을 ABR 입력에 반영한다. 전송률을 높이는 것보다 먼저 queue age가
frame budget을 넘을 때 drop/recovery 정책을 적용한다.

### H-03. 송신 burst와 live-edge collapse가 frame gap/IDR 루프를 만든다 — `유력`

수신기는 완성 프레임 batch가 크면 최신 프레임만 남긴다. 오래된 delta들을
버리는 선택 자체는 stale latency를 막는 올바른 안전장치지만, 다음 frame id가
연속되지 않아 decoder resync와 IDR 요청을 유발할 수 있다. 큰 IDR은 다시
burst를 만들 수 있다.

검증:

- 같은 run에서 `completedBatch`, `liveEdgeBatch`, `frameGaps`,
  `recoveryKeyframes`, `staleDrops`, `decoder input drops`를 시간순으로
  내보낸다.
- “실제 네트워크 loss”와 “수신기가 의도적으로 오래된 프레임을 버림”을
  별도 그래프로 그린다.
- gap 직전 송신 AU bytes/fragment count와 IDR 크기를 연결한다.

개선 방향: live-edge 선택 시 연속 delta를 무조건 여러 개 쌓지 않되, 한 번의
collapse로 decoder가 불필요하게 IDR을 요구하지 않도록 “새 epoch/keyframe
경계”와 “의도적 latest skip”을 명시적으로 전달하거나, 안전한 다음 keyframe
까지 delta를 폐기하는 상태를 구분한다.

### H-04. kernel socket buffer가 숨은 지연을 만든다 — `계산`, `미검증`

512KiB는 payload rate가 80Mbps일 때 약 52ms, 36Mbps일 때 약 116ms에 해당하는
비트 수다. 실제 커널 동작은 플랫폼과 overhead에 따라 달라지므로 상한 계산일
뿐이지만, 애플리케이션의 `pendingFrame=0`만으로는 이 대기를 알 수 없다.

검증:

- 송신 시점마다 queue bytes/age와 `send()` duration/EAGAIN을 기록한다.
- 가능하면 `getsockopt(SO_SNDBUF)` 실제 값을 기록하고 128KiB/512KiB A/B를
  비교한다.
- buffer를 줄였을 때 loss가 아닌 queue age만 줄어드는지 확인한다.

개선 방향: socket buffer를 무작정 키우지 말고, video queue의 최대 age를
정책으로 둔다. 오래된 delta를 폐기하고 keyframe 복구하는 것이 수백 ms의
오래된 화면을 계속 운반하는 것보다 interactive 제품에 적합하다.

### H-05. Android MediaCodec 또는 Surface가 주 병목이다 — `미검증`, 현재는 낮은 우선순위

현재 decoder feed는 nonblocking이고 output도 최대 최신 몇 개만 유지한다.
따라서 구조상 고정 queue가 주 병목일 가능성은 낮지만, 특정 Galaxy XR
펌웨어/HEVC 프로파일/열 상태에서는 달라질 수 있다.

검증:

- Host encoded FPS가 55 이상인 run에서만 decoder input unavailable,
  output drop, rendered FPS, decode output age를 비교한다.
- 같은 bitstream을 H264 baseline과 HEVC로 비교한다.
- Surface release 시각과 고속 카메라의 실제 표시 시각을 별도 기록한다.

Host output 자체가 34~37fps인 현재 4K 기록만으로 Android decoder를 원인으로
판정하지 않는다.

### H-06. ScreenCaptureKit `queueDepth=3`이 capture latency를 늘린다 — `계산`, `미검증`

60fps에서 세 프레임은 nominal frame period 기준 약 50ms다. 실제 queueDepth가
항상 그만큼 지연된다는 뜻은 아니며, callback 처리와 framework scheduling을
같이 봐야 한다.

검증: queueDepth 2/3, 동일 pixel format·display·fps로 capture callback age,
replacement/drop, output FPS를 비교한다. queueDepth를 줄여 drop만 증가하고
지연이 줄지 않으면 원인이 아니다.

### H-07. FEC 계산/복구가 CPU와 burst를 과도하게 사용한다 — `미검증`

8+2에 가까운 FEC는 손실 복구에는 유리하지만 parity 생성과 Android 복구 모두 비용이
있다. 특히 고해상도 AU가 여러 fragment로 나뉘면 매 AU parity burst가 커진다.

검증: FEC 작업 시간, host/Android CPU, parity bytes, recovered fragment,
reassembly timeout을 실험용 FEC off/8+1/8+2로 비교한다. clean LAN에서 복구가 0인데
CPU와 wire bytes만 늘면 기본값으로 부적합하다.

### H-08. HEVC/encoder 설정이 4K 처리량을 떨어뜨린다 — `미검증` (HEVC 수신 경로는 `확인됨`)

Android에서는 실제 저지연 HEVC decoder 연결과 output 증가가 확인되었으므로
“HEVC를 전혀 decode하지 못한다”는 가설은 약해졌다. 그러나 Host VideoToolbox
설정, pixel format 변환, profile, bitrate, keyframe recovery 설정이 4K
처리시간을 높이는지는 아직 분리하지 않았다.

검증: 같은 source에서 H264/HEVC, hardware-only, bitrate, in-flight, keyframe
주기를 바꾸고 capture→encode와 output FPS를 비교한다. CPU fallback이 발생하면
즉시 실패로 세고, 소프트웨어 fallback으로 수치를 채우지 않는다.

### H-09. 무선 LAN 용량 또는 손실이 주원인이다 — `미검증`

현재 `UDP send failure=0`은 네트워크가 정상이라는 증거가 아니다. UDP는 수신
손실을 송신자에게 알려주지 않으며, nonblocking socket은 kernel queue에 넣은
뒤에도 무선 재전송·지터·수신 처리 지연이 생길 수 있다.

검증: ADB 없이 Host 계측과 Viewer 화면/HUD 또는 안전한 진단 export를
사용하고, received/loss/reorder/duplicate/inter-arrival p95를 기록한다.
가능하면 별도 승인된 packet capture와 고정 AP 조건을 사용한다. controlled
loss/latency를 넣을 수 있는 테스트 harness가 있으면 실제 LAN과 분리한다.

### H-10. Android compositor/display refresh가 60fps를 숨긴다 — `미검증`

Surface에 최신 frame을 release한 것과 사용자가 실제로 본 시점은 다르다.
refresh rate, compositor scheduling, window resize, thermal 상태가 영향을 줄
수 있다.

검증: 표시 영역에 단조 증가하는 frame counter를 그린 synthetic pattern을
넣고, 고속 카메라로 Host와 Viewer 화면을 동시에 촬영한다. software timestamp는
단계 진단에만 쓰고 glass-to-glass 주장에는 카메라 증거를 사용한다.

### H-11. 여러 stream이 CPU/GPU/encoder 자원을 서로 빼앗는다 — `미검증`

단일 stream의 4K와 네 개 stream의 1440p는 전혀 다른 문제다. 제품 목표도
focus stream과 background stream을 같은 품질로 보내는 것이 아니다.

검증: 1/2/4 visible stream을 같은 장면·전원·발열 조건에서 비교하고, 각
source의 capture/output/render FPS와 resource budget을 기록한다.

개선 방향: focus는 1440p60 또는 가능한 최고 프로파일, background는 720p15~30
또는 일시 중지로 내려가는 명시적 profile을 둔다. focus 전환에는 hysteresis를
둬 encoder 재설정 thrash를 막는다.

### H-12. timestamp clock이 서로 달라 지연 판단을 틀린다 — `확인됨`

Host와 Android의 wall clock을 동기화 없이 직접 빼면 잘못된 지연이 된다.
현재의 RTT/clock offset probe와 각 장치 내부 stage age는 원인 분석용으로
사용할 수 있지만, 광자 지연의 대체물이 아니다.

개선 방향: 각 장치 단조 시계의 local stage metric과 nonce 기반 RTT/offset
불확실성을 함께 기록하고, cross-device latency는 범위로 보고한다.

### H-13. cursor가 영상에 섞여 interactive 체감이 늦다 — `미검증`, 4K 주원인은 아님

현재 capture 설정은 cursor를 포함한다. 화면 내용과 cursor가 같은 codec/GOP/
network queue를 공유하면 화면이 바쁜 순간 pointer도 같이 늦어진다.

개선 방향: 1차 throughput 문제가 안정된 뒤 cursor를 별도 authenticated
control/overlay plane으로 분리하고, frame-relative position과 visibility를
보낸다. 보안과 coordinate mapping을 먼저 설계한 후 적용한다.

### H-14. WebRTC/QUIC로 바꾸면 자동으로 해결된다 — `미검증`, 4K 주원인으로는 확인되지 않음

전송 교체는 queue, pacing, loss recovery를 개선할 수 있지만, Host가 34~37fps
밖에 만들지 못하는 문제를 해결하지는 않는다. 먼저 현재 경로에서 stage별
budget을 얻은 다음 동일한 source/profile 조건으로 bake-off 해야 한다.

## 5. ADB 없는 실제 검증 절차

이번 조사에서 ADB 수집은 실행하지 않았다. 기존 `tools/perf-matrix`의 일부
collector는 ADB를 전제로 하므로 이번 범위의 실행 명령에서 제외한다. 대신
Host 진단 패널/파일과 Viewer의 화면에 표시되는 HUD 또는 진단 export를 하나의
run id로 묶는다. Viewer 로그가 화면에 노출되지 않는 빌드라면 그것은 “수집
불가”로 기록하며 추정값으로 채우지 않는다.

### 5.1 공통 조건

각 run에는 [성능 측정 계획](06-benchmark-device-validation.md)의 장비 정보와
다음을 함께 남긴다.

```yaml
run_id:
git_commit:
host_build:
viewer_build:
source_resolution:
source_refresh_hz:
codec:
configured_bitrate_mbps:
fec: off | 8+1 | 8+2
visible_streams:
focus_stream:
network_ap:
network_band:
warmup_seconds: 60
steady_seconds: 180
soak_seconds: 3600
adb_used: false
```

화면 종류는 정적 문서, 빠른 스크롤, 창 이동/resize, 동영상 재생을 구분한다.
정적 화면만으로는 capture callback과 encoder throughput 문제를 재현할 수
없다.

### 5.2 실행 매트릭스

| ID | 프로파일 | 목적 |
| --- | --- | --- |
| B1 | 1440p H264 60, 기본 FEC, 1 stream | 제품의 첫 interactive baseline |
| B2 | 1440p H264 60, 실험 FEC off/8+1/8+2 | FEC overhead와 recovery 비용 분리 |
| B3 | 4K HEVC 60, 기본 FEC | 순수 capture/encode throughput 확인 |
| B4 | 4K HEVC 60, FEC 8+2 | high-bandwidth wire/pacer 상호작용 확인 |
| B5 | 4K H264 또는 낮은 bitrate HEVC | codec/bitrate가 주원인인지 확인 |
| B6 | 1440p 또는 720p, 2/4 streams | resource fairness와 focus/background 정책 확인 |
| B7 | 최종 profile 60분 | thermal, memory, latency creep 확인 |

각 B run은 같은 장면에서 최소 60초 warm-up 후 180초 steady를 측정한다.
B1/B3/B4의 결과는 `capture`, `encoded`, `received`, `rendered` FPS가 모두
필요하다. 하나라도 없으면 해당 단계의 판정은 미검증으로 남긴다.

### 5.3 판정 기준

제품 목표는 [제품 요구사항](01-product-requirements.md)의 NFR-001/NFR-002와
호환되게 적용한다.

- 1440p baseline은 clean LAN에서 Host/Viewer 모두 지속 55fps 이상, 영상 queue
  age p95가 한 frame budget 근처에서 안정되고, 80ms 이상의 stale chain이
  반복되지 않아야 한다.
- 4K는 Host encoded FPS가 55 이상이고 queue age가 증가하지 않을 때만
  `capable`로 표시한다. 현재의 34~37fps 관측만으로는 이 조건을 충족하지
  않는다.
- `pendingFrame=0`, `send failure=0`, APK 설치 성공만으로 pass하지 않는다.
- 60분 동안 latency creep가 16ms를 넘거나 thermal 상태가 상승하면 E7 목표
  미달로 기록한다.
- 고속 카메라 없이 p50/p95를 계산해도 그것은 software pipeline latency일
  뿐이며 glass-to-glass pass가 아니다.

## 6. 개선 방향과 우선순위

### P0 — 먼저 계측하고 stale latency를 차단한다

1. **큐 개수에서 queue age/bytes로 계측을 바꾼다.**
   Capture pending, encoder in-flight, AU queue, socket handoff, Android
   incomplete AU, live-edge batch 각각에 `oldest_age_us`, `bytes`, `count`를
   추가한다. 1Hz 요약과 run 종료 histogram을 제공한다.
2. **wire bitrate를 별도 계산한다.**
   `encoded bytes`, `parity bytes`, protocol/IP/UDP overhead, datagram count를
   분리하고 pacer는 실제 wire budget을 사용한다. FEC를 켰을 때 ABR가 실제
   비용을 보도록 한다.
3. **queue-time pacer를 도입한다.**
   WebRTC처럼 leaky-bucket/priority pacer를 참고하되, Leftcar의 bounded
   latest-frame 정책에 맞춘다. queue age가 frame budget을 넘으면 delta를
   계속 쌓지 않고 skip 또는 recovery로 전환한다.
4. **recovery 원인을 분리한다.**
   네트워크 loss, reordering timeout, live-edge intentional skip,
   decoder input unavailable, encoder/capture drop을 서로 다른 카운터와
   frame id로 남긴다. IDR 요청률 하나만 보고 “네트워크가 나쁘다”고 결론내리지
   않는다.
5. **1440p H264를 첫 acceptance baseline으로 고정한다.**
   4K capability를 기다리느라 interactive 기본 모드까지 느리게 만들지 않는다.

### P1 — Host 4K 경로와 수신 burst를 최적화한다

1. **4K profile을 capability-gated opt-in으로 둔다.**
   1440p60 H264, 4K HEVC, 4K H264/저비트레이트 순서로 capability를 측정하고
   output FPS/queue age 조건을 만족하는 경우에만 노출한다.
2. **in-flight 1/2/5와 ScreenCaptureKit queueDepth 2/3을 A/B한다.**
   숫자를 크게 해서 병렬성을 올리는 것이 항상 빠르지 않다. 처리시간보다 오래
   살아 있는 작업을 최신 frame으로 대체할 수 있어야 한다.
3. **GPU zero-copy 경계를 유지하되 실제 변환을 측정한다.**
   pixel format 변환, IOSurface handoff, VideoToolbox submit/output을 각각
   계측한다. “GPU 경로”라는 이름만으로 변환 비용이 사라졌다고 가정하지 않는다.
4. **IDR은 recovery budget 안에서 보낸다.**
   연속 recovery 요청을 debounce하고, keyframe 크기와 pacing duration을
   별도 관리한다. IDR을 무조건 즉시 보내는 정책은 손실 뒤의 burst를 키울 수
   있다.
5. **FEC는 profile별로 조절한다.**
   clean LAN interactive baseline에서는 실험용 off 또는 낮은 parity를 비교하고,
   손실이 실제로 관측될 때만 8+1/8+2를 선택한다. FEC는 control plane에 넣지
   않는다.

### P1 — Android live-edge와 표시 경계를 다듬는다

1. 의도적으로 최신 프레임을 선택한 경우와 네트워크 gap을 구분한다.
2. batch가 큰 경우 전체를 버리더라도 다음 decodable keyframe 경계를 명시해
   불필요한 IDR 연쇄를 줄인다.
3. recvmmsg burst를 한 번에 처리하는 시간이 frame budget을 넘지 않도록
   assembly/FEC 작업의 time slice와 카운터를 둔다.
4. MediaCodec input unavailable, output discard, Surface release를 각각
   표시하고, decoder가 정상인데 compositor가 늦는 경우를 분리한다.

### P2 — 제품 체감과 장기 확장

1. **focus/background quality policy:** focus 1440p60, background 720p15~30,
   hidden suspend를 사용한다. focus 전환은 hysteresis와 최소 체류시간을 둔다.
2. **cursor overlay:** 영상과 별도 인증 plane으로 cursor를 전송해 바쁜 화면에서도
   pointer 체감을 유지한다.
3. **WebRTC/QUIC bake-off:** 같은 B1~B7 조건에서 queue age, loss recovery,
   CPU, 구현 복잡도를 비교한 뒤 선택한다. 전송 교체를 4K encoder 병목의
   해결책으로 선행하지 않는다.
4. **immersive/XR 후속:** 장기적으로 focus/ROI/foveated quality와 prediction을
   검토한다. 현재 Leftcar는 여러 2D 창 제품이므로 먼저 창별 profile과 공정한
   pixel budget을 완성한다.

## 7. 참고한 공개 구현과 적용 범위

다른 제품의 수치를 그대로 Leftcar의 목표로 복사하지 않고, 구조적 아이디어만
참고한다.

- [RustDesk video service](https://github.com/rustdesk/rustdesk/blob/master/src/server/video_service.rs):
  client의 frame-fetched 신호를 이용해 Host가 계속 앞서가지 않게 하는
  receiver-driven 흐름이 있다. 전체 영상을 hard-block하는 기본값보다는
  interactive profile의 optional feedback으로 적용할 가치가 있다.
- [RustDesk video QoS](https://raw.githubusercontent.com/rustdesk/rustdesk/master/src/server/video_qos.rs):
  delay와 화면 상태를 기준으로 FPS/품질을 조정하고 hysteresis를 둔다. Leftcar도
  queue age와 focus 상태를 ABR 입력에 넣어야 한다.
- [Sunshine configuration](https://github.com/LizardByte/Sunshine/blob/master/docs/configuration.md):
  빠른 NVENC preset, 낮은 VBV, 4K split encode, FEC percentage, access-unit
  크기 제한 등에서 “hardware encoder가 있다”보다 실제 latency/bitrate 설정을
  관리하는 것이 중요하다는 점을 보여 준다.
- [WebRTC pacer design](https://webrtc.googlesource.com/src/+/dab50c6fe8f13a19f4dee31a5338ad70a81f4f74/modules/pacing/g3doc/index.md):
  burst를 평탄화하고 queue time limit을 두며, queue가 길어지면 pacing을
  overdrive하거나 drop/recovery를 선택한다. Leftcar의 queue-age 정책에 가장
  직접적으로 참고할 수 있다.
- [Microsoft untethered VR remote rendering](https://www.microsoft.com/en-us/research/publication/cutting-the-cord-designing-a-high-quality-untethered-vr-system-with-low-latency-remote-rendering/),
  [Fraunhofer immersive streaming](https://www.hhi.fraunhofer.de/en/departments/vca/projects/immersive-streaming.html):
  병렬 render/encode/transmit/decode, prediction, server-side view rendering,
  hardware encode의 중요성을 보여 준다. 연구 장비·해상도·제품 조건이 다르므로
  수치를 Leftcar 성능으로 인용하지 않는다.

## 8. 권장 목표 구조

```text
                ┌──────────── focus profile ────────────┐
capture ───────▶│ latest input → HW encode → AU budget   │
                └──────────────────┬────────────────────┘
                                   ▼
                       wire-rate / queue-age pacer
                                   │
                 ┌─────────────────┴─────────────────┐
                 │                                   │
           video UDP + FEC                    control/feedback
                 │                                   │
       bounded reorder/assembly                RTT + queue report
                 │                                   │
       live-edge + keyframe gate               profile/ABR decision
                 │
       nonblocking MediaCodec → Surface
                 │
             display measurement
```

핵심 불변식은 다음과 같다.

- 영상 큐는 프레임 수가 아니라 최대 age와 bytes로 제한한다.
- Host가 새 frame을 만들 수 없는 문제를 transport 교체로 숨기지 않는다.
- FEC와 IDR은 resilience 예산 안에서 보내며, 실제 wire bytes에 반영한다.
- stale 화면을 보존하기보다 latest decodable 화면을 선택한다.
- `rendered`와 `photon-presented`를 같은 의미로 쓰지 않는다.
- 여러 창은 동일한 품질로 경쟁하지 않고 focus/background 정책을 따른다.

## 9. 다음 실행 순서

1. B1 1440p H264 clean-LAN run을 ADB 없이 Host/Viewer 진단으로 재수집한다.
2. B3 4K HEVC에서 capture/encode/packetize/socket의 age와 bytes를 추가해
   34~37fps 병목을 재현한다.
3. H-02/H-03/H-04의 상관관계를 같은 run timeline으로 확인한다.
4. queue-age pacer와 recovery reason 계측을 먼저 구현하고, 1440p baseline의
   pass/fail을 고정한다.
5. 그 뒤에 queueDepth/in-flight/FEC/codec 조합을 A/B한다.
6. Host output이 충분히 빠른 경우에만 Android decoder/Surface와 고속 카메라
   검증을 진행한다.
7. 1/2/4 stream과 60분 soak를 거쳐 focus/background profile을 확정한다.
8. 마지막으로 WebRTC/QUIC bake-off를 수행한다.

## 10. 이번 조사에서 실행한 로컬 검사

ADB를 호출하지 않고 2026-08-26 현재 working tree에서 실행했다. 이 검사는
문서·순수 로직·빌드 경계를 검증하며, 실기기 E6/E7을 대신하지 않는다.

| 검사 | 결과 |
| --- | --- |
| `git diff --check` | 통과 |
| `cargo fmt --all -- --check` | 통과 |
| `bun run test` | 12 files, 73 tests 통과 |
| `bun run test:contract` | 1 file, 4 tests 통과 |
| `bun run test:architecture` | TS/Kotlin rules clean |
| `cargo test --workspace` | 실패 0, 전체 crate/doc test 통과 |
| `npx -y react-doctor@latest . --verbose` | `100 / 100`, issues 0 |

이번 문서 작업 자체는 React/React Native 소스나 런타임 동작을 변경하지
않았다. 따라서 위 React Doctor 결과는 현재 working tree의 React 변경까지
포함한 저장소 게이트 확인이며, 영상 실기기 검증 결과로 해석하지 않는다.

## 11. 구현 후 재검증 (2026-08-26)

- **H-03 구현**: Android 수신기의 frame-id 점프를 `networkLoss`,
  `liveEdgeDiscard`, `recoverySkip`으로 분리했다. live-edge에서 중간 AU를
  버린 경우는 네트워크 손실로 Host에 보고하지 않으며, 선택된 delta는 참조
  체인 안전을 위해 다음 IDR까지 버린다. 이미 keyframe을 기다리는 동안의
  점프는 새 복구 요청이나 새 네트워크 손실로 중복 집계하지 않는다.
- **H-04 계측**: Host의 userspace encoded-frame 큐에 `pendingFrameBytes`와
  `pendingFrameOldestAgeUs`를 추가했다. 이는 커널 `SO_SNDBUF` 잔량을
  의미하지 않으므로, 이 두 값이 0이어도 kernel socket backlog가 0이라고
  해석하지 않는다.
- **순수 로직 검증**: Android viewer native 51개 테스트, frame-gap 분리
  회귀 6개, viewer-decoder gap 테스트, Swift encode policy 테스트가
  통과했다. Android `aarch64-linux-android` release native build와 release
  APK assemble도 통과했다.
- **정적 실기기 smoke**: `HA2D6EMP`에 APK를 덮어 설치하고
  `2560×1440@60`, H.264, `CgDisplayStream`, Wi-Fi UDP로 연결했다.
  `actualCodec=c2.qti.avc.decoder.low_latency`, `Rendered 150`,
  `decoderInputDrops=0`, `outputDrops=0`, `frameGaps=0`,
  `intentionalLiveEdgeGaps=0`을 확인했다. 이 표본은 짧은 정적 장면이므로
  고모션 7~15fps 문제가 해결됐다는 증거가 아니다.
- **남은 검증**: 사용자가 직접 고변화 장면을 재생한 뒤 Android 로그에서
  `frameGaps`와 `intentionalLiveEdgeGaps`가 분리되고, Host의 queue age가
  frame budget을 넘지 않는지 확인해야 한다. 이후에도 `encodeOutputFps`가
  55 미만이면 encoder/capture 병목이고, output이 유지되면서 rendered만
  떨어지면 Android/recovery 경로를 별도로 본다.
