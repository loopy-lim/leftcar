# 성능 측정과 실기기 검증 계획

문서 상태: 제안안 0.1  
목적: “빠르게 잘 보인다”를 재현 가능한 수치와 화면 품질 증거로 바꾼다.

## 1. 측정 원칙

1. 최종 지표는 glass-to-glass다.
2. 단계별 software timestamp는 병목 분석용이다.
3. 서로 다른 장치 clock을 동기화 없이 직접 빼지 않는다.
4. 평균만 보고하지 않는다. p50, p95, p99, max, sample count를 기록한다.
5. warm-up과 steady-state를 분리한다.
6. 한 stream과 여러 stream을 따로 측정한다.
7. 발열 전과 60분 후 결과를 따로 기록한다.
8. 목표 미달 결과도 삭제하지 않고 환경 정보와 함께 보관한다.
9. 공식 장치 사양을 실제 Leftcar 성능으로 바꿔 말하지 않는다.

## 2. 핵심 지표

### 2.1 지연

| metric | 정의 |
| --- | --- |
| capture callback delay | 화면 변화 시점부터 Host capture callback까지 |
| encode latency | encoder submit부터 access unit output까지 |
| sender queue time | encode output부터 socket handoff까지 |
| network transit estimate | 송신/수신 timestamp와 clock uncertainty로 추정 |
| assembly latency | 첫 fragment부터 완성 frame까지 |
| decoder input wait | frame 완성부터 codec input queue까지 |
| decode latency | codec input부터 output callback까지 |
| present handoff | codec output부터 Surface release까지 |
| glass-to-glass | Host display photon 변화부터 Galaxy XR display photon 변화까지 |

### 2.2 안정성

- capture FPS
- encoded FPS
- received complete FPS
- rendered FPS
- capture drop
- encode drop
- network loss/reorder/duplicate
- assembly timeout
- decoder drop
- stale epoch drop
- IDR request rate
- reconnect count/time
- latency creep slope

### 2.3 자원

Host:

- process RSS/private bytes
- CPU by thread/process
- GPU utilization if available
- encoder utilization
- network throughput
- capture/encoder queue memory

Viewer:

- RSS/PSS
- native heap와 Java heap
- CPU/GPU
- decoder instance identity/count
- Surface count
- network throughput
- battery drain
- thermal status
- dropped frames and frame time

### 2.4 품질

- source와 viewer screenshot의 crop/scale alignment
- luma PSNR/SSIM 보조 지표
- OCR character accuracy
- 작은 글꼴 edge contrast
- chroma fringing visual inspection
- color bar delta
- resize 후 aspect ratio

screen capture 결과를 repo에 넣지 않는다. synthetic test pattern만 영구 artifact로 저장한다.

## 3. 기준 장비 기록

모든 결과에 다음을 포함한다.

```yaml
run_id:
date:
git_commit:
build_type: release
host:
  model:
  cpu:
  gpu:
  memory:
  os_version:
  display_resolution:
  display_refresh_rate:
viewer:
  model: Samsung Galaxy XR
  os_build:
  app_version:
  battery_percent:
  charging:
  initial_thermal_status:
network:
  access_point:
  band:
  channel_width:
  rssi:
  host_link:
  distance_m:
  competing_traffic:
media:
  source_kind:
  source_resolution:
  capture_fps:
  codec:
  profile:
  bitrate:
  keyframe_interval:
  transport:
windows:
  count:
  dimensions:
  focus_pattern:
```

## 4. 테스트 패턴

### 4.1 Latency Flash

Host 화면을 검정/흰색으로 전환하고 frame ID를 표시한다.

- 변화 주기: random 300-900ms
- 최소 200회
- Host와 Viewer가 같은 frame ID를 보이게 한다.
- 외부 고속 카메라 frame에서 두 화면 변화 간격을 센다.

화면 전체 flash는 자동 brightness나 display persistence 영향을 받을 수 있어 작은 고대비 patch도 같이 측정한다.

### 4.2 Moving Bar

- frame마다 한 칸 이동하는 세로 막대
- frame number와 monotonic counter
- frame duplication, reorder, drop을 눈과 OCR로 확인

### 4.3 Text Grid

- 8, 10, 12, 14, 16px 영문/한글 monospace
- 빨강/파랑 chroma edge
- light/dark theme
- thin/bold font
- 100%, 125%, 150%, 200% scale

### 4.4 Resize Grid

- aspect ratio marker
- corner coordinate
- source resize sequence
- 640x480 -> 1920x1080 -> 2560x1440 -> portrait -> original

### 4.5 Multi-source Identity

source마다 다른 color, large letter, increment rate를 사용한다. 잘못된 source routing과 Activity/task identity 오류를 찾는다.

## 5. Glass-to-glass 측정

### 5.1 장비

- 240fps 이상 고속 카메라 권장
- Host 디스플레이와 Galaxy XR lens/display가 한 frame에 들어오는 고정 rig
- 가능한 경우 photodiode/LED trigger
- 삼각대, 고정 노출/셔터/초점

Galaxy XR 내부 화면 촬영은 optics 때문에 난도가 높다. 장비가 불가능하면 다음을 명확히 분리한다.

- camera-based true glass-to-glass
- viewer screen recording 기반 근사치
- software timestamp 기반 pipeline estimate

후자의 두 수치를 glass-to-glass라고 부르지 않는다.

### 5.2 절차

1. Host display refresh rate를 고정하거나 기록한다.
2. Viewer brightness/refresh/환경을 고정한다.
3. 앱과 stream을 5분 warm-up한다.
4. 200회 random flash를 촬영한다.
5. 각 event에서 Host와 Viewer 최초 변화 frame을 표시한다.
6. camera FPS로 시간 차이를 계산한다.
7. 측정 오차를 `± 1 camera frame` 이상으로 기록한다.
8. p50/p95/p99와 histogram을 생성한다.
9. 같은 profile을 cold, warm, 60분 thermal 상태에서 반복한다.

### 5.3 통과 기준

S1 기준:

- p50 50ms 이하
- p95 80ms 이하
- 200 sample 중 99%가 120ms 이하
- 60분 뒤 p50 증가 16ms 이하
- 연속적으로 증가하는 queue latency 없음

stretch:

- p50 35ms 이하
- p95 50ms 이하

## 6. Software timestamp

### 6.1 frame trace ID

sampling된 frame 1/30 또는 configurable 비율에 `trace_id`를 부여한다. 모든 frame에 verbose log를 남기지 않는다.

### 6.2 clock sync

Host와 Viewer 사이 ping/pong으로 다음을 추정한다.

- RTT
- offset interval
- uncertainty

NTP-like 최소 RTT sample을 사용할 수 있지만 Wi-Fi 비대칭이 있다. 결과에는 point offset뿐 아니라 uncertainty를 포함한다.

```text
estimated_network_and_viewer_time = viewer_receive_clock
                                  - mapped_host_send_clock
                                  ± clock_uncertainty
```

uncertainty가 5ms보다 크면 세부 stage 합을 단정하지 않는다.

## 7. 다중 창/다중 decoder matrix

### 7.1 창 개수

| case | 창 | source | 목적 |
| --- | --- | --- | --- |
| W1 | 1 | 1 | baseline |
| W2 | 2 | 2 | 기본 다중 앱 |
| W3 | 3 | 3 | Android XR tidy up와 일반 사용 |
| W4 | 4 | 4 | v1 수용 기준 |
| W6 | 6 | 6 | 탐색 상한 |
| W8 | 8 | 8 | capability 탐색, release 요구 아님 |

### 7.2 profile

[제품 요구사항](01-product-requirements.md) 7.1의 기준 profile ID를 그대로 사용한다.

| profile | 구성 |
| --- | --- |
| S1 | 1080p60 x1 |
| S2 | 1440p60 x1 |
| M2 | 1080p30 x2 |
| M4 | 1080p30 x4 |
| F4 | 1440p60 x1 + 720p15 x3 |
| M6 | 720p15 x6, 탐색 전용이며 v1 요구가 아님 |

각 profile에서 decoder create 성공만 보지 않는다. 10분 rendered FPS, drop, Surface visibility, resource 사용을 측정한다.

### 7.3 focus/size pattern

- 모두 같은 크기
- 한 창 크게, 세 창 작게
- focus를 2초마다 순환
- 한 창을 최소 크기로 축소
- Hub를 앞에 놓아 stream window가 비초점
- 한 창 close/open 반복

quality controller가 hysteresis 없이 thrash하지 않는지 확인한다.

## 8. codec capability probe

Viewer 진단 tool은 JSON으로 다음을 내보낸다.

```json
{
  "codec": "redacted-vendor-codec-name",
  "mime": "video/avc",
  "hardwareAccelerated": true,
  "softwareOnly": false,
  "lowLatency": true,
  "maxSupportedInstancesHint": 8,
  "performancePoints": [],
  "tested": [
    {"width": 1920, "height": 1080, "fps": 30, "instances": 4, "passed": true}
  ]
}
```

vendor codec name 자체는 민감하지 않지만 raw device fingerprint가 되지 않도록 외부 telemetry에는 보내지 않는다.

probe 단계:

1. enumerate
2. capability query
3. single instance decode
4. concurrent instance ramp 1 -> 2 -> 3 -> 4 -> 6 -> 8
5. Surface output visibility
6. 10분 sustain
7. 60분 selected profile

한 단계 실패 뒤 더 높은 단계를 강행하지 않는다.

## 9. transport bake-off

동일 조건에서 WebRTC와 QUIC 후보를 비교한다.

### 9.1 고정 조건

- capture source: generated moving pattern
- encoder output: 동일 pre-encoded access unit replay와 live encode 두 종류
- decoder: 동일 Rust `AMediaCodec` path
- window count: 1, 2, 4
- profile: S1, M4, F4
- network profile: clean, normal, busy, bad, outage

### 9.2 결과 표

| metric | WebRTC | QUIC | 판정 |
| --- | --- | --- | --- |
| clean p95 | | | |
| 1% loss p95 | | | |
| outage recovery | | | |
| IDR recovery | | | |
| 4-stream CPU | | | |
| 4-stream battery | | | |
| 60m creep | | | |
| binary size | | | |
| operational complexity | | | |

### 9.3 선택 규칙

- security/correctness fail: 즉시 탈락
- S1 p95 또는 복구 목표 fail: 탈락
- 4 stream 불안정: 탈락 또는 제품 범위 축소 검토
- 둘 다 통과: 코드와 배포 복잡도가 낮은 후보
- 결과 차이가 측정 오차 이내: WebRTC 우선 검토

QUIC custom path를 선택하면 3% loss에서의 congestion behavior와 network fairness review가 필수다.

## 10. 네트워크 실제 환경

### N1 ideal desk

- Host 유선 Ethernet
- Galaxy XR Wi-Fi 6E/7
- AP 1-2m
- 동일 방, line of sight

### N2 normal home

- Host Wi-Fi
- Galaxy XR Wi-Fi
- AP 3-5m
- 일반 background traffic

### N3 busy

- 동시 4K streaming/다운로드
- 5GHz/6GHz contention
- packet loss와 jitter 기록

### N4 roam/outage

- AP 연결 재협상 또는 Wi-Fi 5초 off/on
- Host sleep/wake
- Viewer sleep/wake

### N5 unsupported boundary

- 서로 다른 LAN/NAT
- VPN
- LTE tethering

N5는 관측만 할 수 있으며 v1 지원 목표가 아니다.

## 11. 장시간/thermal soak

### 11.1 60분 필수

- S1과 F4
- 5분마다 metric snapshot
- 10분마다 focus 변경
- 20분마다 한 stream 재시작
- 30분에 Wi-Fi 5초 단절
- 종료 시 모든 자원 release 확인

### 11.2 2시간 beta

Galaxy XR 공식 일반 사용 battery 시간이 약 2시간인 점을 고려해 beta 후보는 외부 배터리 연결/미연결 두 경우를 조사한다. 공식 battery 사양을 Leftcar 지속 시간 보장으로 사용하지 않는다.

측정:

- battery delta
- thermal status timeline
- FPS/latency timeline
- decoder restart
- memory slope
- network reconnect

## 12. resize와 lifecycle stress

자동/반자동 시나리오:

- 각 stream window 100회 resize
- aspect ratio 4:3, 16:9, portrait
- Surface destroy/create 100회
- Hub open/close 50회
- stream task close/open 50회
- Viewer process kill/restore 10회
- Host source resize 50회
- Host app window close/reselect 20회

통과:

- crash/ANR 없음
- double free/invalid handle 없음
- orphan source lease 없음
- 다른 창 재생 중단 없음
- memory가 반복 횟수에 비례해 증가하지 않음

## 13. 화질 평가

### 13.1 OCR

고정 text grid에서 source text와 viewer capture OCR 결과를 비교한다.

- 정확도는 pipeline 간 상대 비교에 사용한다.
- headset optics/카메라 촬영 오차를 별도 기록한다.
- 한글과 영문을 나눈다.

### 13.2 사람 검수

다음 질문을 5점 척도로 기록한다.

- 10px monospace 코드가 읽히는가
- 빨강/파랑 경계에서 색 번짐이 거슬리는가
- scroll 중 글자가 유지되는가
- resize 직후 blur가 회복되는가
- 작은 비초점 창이 상태 관찰에 충분한가

### 13.3 codec profile 결정

H.264가 text 품질 목표를 만족하면 v1 baseline을 유지한다. 부족하면 HEVC를 같은 latency/power 조건으로 비교한다. 무손실/4:4:4은 device capability와 bandwidth 증거 없이는 scope에 추가하지 않는다.

## 14. 결과 저장

제안 위치:

```text
artifacts/benchmarks/<date>/<run_id>/
  manifest.yaml
  summary.md
  metrics.jsonl
  latency.csv
  histogram.svg
  capability.json
  redacted.log
  checksums.txt
```

실제 화면 video는 기본 저장하지 않는다. 필요한 광학 영상은 접근 제한 저장소에 두고 repo artifact에는 derived timestamp와 redacted thumbnail만 넣는다.

## 15. 회귀 판정

PR benchmark:

- copy/allocation count 증가: hard fail
- queue bound 위반: hard fail
- microbenchmark median 10% 이상 악화: review

release benchmark:

- NFR 필수 목표 미달: block
- p95 10% 이상 회귀: block 또는 명시적 waiver
- thermal severe 지속: block 또는 profile 하향
- W4 실패: v1 요구 재승인 필요

waiver에는 원인, 사용자 영향, 만료 버전, 회복 계획이 있어야 한다.

## 16. 수동 검증 결과 문구

좋은 예:

> Galaxy XR OS build X, Mac model Y, H.264 1080p60 단일 창, clean-lan 조건에서 240fps 카메라 220 sample의 glass-to-glass p50 46ms, p95 72ms를 관측했다. 60분 뒤 p50은 8ms 증가했다.

나쁜 예:

> Rustra가 빠르므로 10ms다.

> Galaxy XR는 8K60이라 4개 창도 문제없다.

> 로그상 decode가 4ms라 전체 지연도 4ms다.

