# 2디스플레이(splitVertical) 구간 렉 연구 — 2026-09-08

## 0. 요약

2디스플레이(4K splitVertical: 3840×2160 캡처 → 1920×2160 타일 2개)에서 단일 디스플레이보다
렉이 도드라지는 문제를 코드 근거와 기존 실측 문서로 분해했다. 결론부터:

- **렉의 직접 원인은 대역폭 부족이 아니라 "페어(쌍) 단위 동기화와 복구"다.**
  좌/우 인코더 출력을 묶어 기다리는 호스트 랑데부(`EncodedPairAssembler`)와,
  한쪽 타일 손실이 양쪽 프리즈 + 페어 IDR로 번지는 뷰어 복구 정책이 지연·스터터를 증폭시킨다.
- 사용자가 제안한 **"모두 한번에 섞어서 보낸다"는 이미 구현돼 있다** — split 전송은 좌/우
  데이터그램을 하나의 소켓·하나의 페이서로 인터리브한다(`writeSplitPacketPair`).
  빠져 있는 것은 "묶어 보내기"가 아니라 "묶어 기다리지 않기"다.
- **"압축을 잘한다"는 2차 수단**이다. ABR이 이미 2스트림 팩터(×0.65)를 적용하고,
  HEVC 전환(-25~40% 비트레이트)은 유망하지만 렉의 직접 원인(페어 복구 폭풍)을
  해결한 뒤 측정하는 편이 안전하다.
- 권장 순서: **(1) 관측 기준선 → (2) 랑데부 완화 → (3) 전송 데드라인 split 보정 →
  (4) 복구 데커플링 → (5) HEVC 프로브.** 각 단계는 숫자로 합격/불합격을 판정한다.

## 1. 범위

- **splitVertical(제품의 2디스플레이 경로)**: 하나의 CaptureSession이 4K를 캡처해
  좌/우 타일로 나눠 연속 포트 2개로 전송, 뷰어가 두 Surface에 렌더링.
  호스트 검증: `apps/host-desktop/src-tauri/src/control.rs:103-128`
  (3840×2160 / 60fps / direct UDP / 연속 포트 2개 요구).
- 독립 세션 2개(멀티 윈도우)는 ABR의 `withRegistry { $0.count }` 팩터로 이미
  비트레이트가 나뉘고, 세션 큐/소켓도 완전 분리돼 있다. 이 문서의 주제는 splitVertical이지만
  §4 H-C의 자원 경합은 두 모드에 공통 적용된다.

## 2. 현 파이프라인 (코드 근거)

### 호스트 (native/macos-capture-shim)

| 단계 | 구현 | 위치 |
| --- | --- | --- |
| 캡처 | SCStream 4K60 NV12, split은 queueDepth 8 | `Sources/Capture/CaptureSession+Backend.swift:75-171`, `Sources/Encoder/EncoderPolicy.swift:262-270` |
| 분할 | Metal 블리트로 좌/우 픽셀버퍼 풀링 복사 | `Sources/Split/MetalNv12Splitter.swift:25-192` |
| 인코더 | VideoToolbox 타일 인코더 ×2 (하드웨어 필수), aggregate = min(60M, max(30M, w·h·fps·0.085)), 타일당 절반 | `Sources/Split/DualEncoderPipeline.swift:45-121`, `Sources/Split/CaptureSession+Split.swift:157-161` |
| 랑데부 | `EncodedPairAssembler` — 먼저 끝난 타일을 **반대편이 올 때까지 보류**, 만료 = 프레임예산×maxInFlightPairs(60fps: 16.7ms×5 = **83ms**) | `Sources/Split/DualEncoderPipeline.swift:2-23, 299-380` |
| 만료 처리 | 버려지는 게 아니라 **파이프라인 전체 리셋 + 페어 IDR** (`beginPairedRecovery`) | `Sources/Split/DualEncoderPipeline.swift:369-380` |
| 전송 | 좌/우 데이터그램 **인터리브**, 소켓 1개·페이서 1개·마감 1개 공유, 우측 타일은 targetPort+1 | `Sources/Transport/CaptureSession+SplitTransport.swift:21-49, 170-366` |
| 흐름 제어 | `pendingSplitAccessUnits` 초과 시 **세션 정지** ("split encoded queue exceeded flow capacity") | `Sources/Split/CaptureSession+Split.swift:451-486` |

### 뷰어 (native/android-viewer)

| 단계 | 구현 | 위치 |
| --- | --- | --- |
| 수신/디코드 | 타일 워커 ×2 — 소켓·디코더 각각 독립 | `renderer/split_session/tile_worker.rs:49-720` |
| 프레젠테이션 | `PairPresentationCoordinator` — PTS 매칭, 선행 타일 **1ms만 홀드**, 지각 피어 4ms 그레이스 후 단독 진행 | `renderer/presentation_sync.rs:5-12` |
| 갭 복구 | **한쪽 타일이라도 프레임 누락이면 양쪽 마지막 좋은 프레임 프리즈 + 페어 IDR**, 750ms 재시도 | `renderer/split_session/gap_policy.rs:36-100`, `split_session.rs:536` |

핵심 비대칭: **뷰어는 이미 느슨하다**(선행 타일 1ms 홀드 후 단독 진행). 그런데 **호스트는
83ms까지 묶어 기다렸다가, 못 기다리면 전체를 리셋한다.** 인위적 직렬화의 무게중심이 호스트에 있다.

## 3. 측정된 사실

1. **듀얼 하드웨어 인코더 콜백 p95 = 38ms** (좌 38.03ms / 우 37.80ms, 유효 59.88fps).
   단일 인코더 타일 프로브는 10.41ms — 인스턴스 2개가 콜백 지연을 3배 이상로 만든다.
   `docs/sidecar-quality-validation.md:17-21`
2. **4K split 기기 실측**: 평균 59.6fps, 그러나 1초 구간 최저 44fps, 180초에 gap 복구 5회,
   최대 gap→첫 출력 159ms, **2.895초 복구 정지 1회**. `docs/responsive-streaming-validation.md:58-73`
3. **AU 전송 데드라인 초과 → 체인 폐기 + IDR**이 "버스트 → 프레임 갭 → IDR 루프"로
   붕괴시키는 패턴은 문서화된 기존 가설 H-03이다. split은 프레임당 데이터그램이 2배라
   같은 링크에서 데드라인 진입 확률도 대략 2배다. `docs/11-low-latency-investigation.md`,
   `Sources/Transport/UdpPacingPolicy.swift:255-281`
4. **ABR은 2스트림에서 ×0.65로 시작**하고, 컷은 2연속 혼잡 윈도우, 레이즈는 8안정 윈도우라
   회복이 수 초 단위다. `Sources/Encoder/CaptureSession+AdaptiveBitrate.swift:105-135`
5. 뷰어도 창 하나가 수신을 멈추면 호스트 최신프레임 큐 넘침으로 **반대편 스트림이 열화**한다는
   주석이 코드에 있다. `renderer/single_session/runtime/worker.rs:226-236`

## 4. 병목 가설 (우선순위)

| # | 가설 | 근거 | 예상 기여 |
| --- | --- | --- | --- |
| H-A | **호스트 랑데부가 느린 쪽 인코더에 전 구간을 발목 잡는다.** 듀얼 p95 38ms 환경에서 스큐가 83ms 만료에 닿으면 전체 리셋 + 페어 IDR 폭풍 → 관측된 2.9s 정지 | §2 랑데부·만료 처리, §3-1, §3-2 | 최대. 스터터 에피소드의 상당 부분 |
| H-B | **전송 예산.** 좌/우 인터리브가 하나의 직렬 네트워크 큐·페이서를 공유. Wi-Fi 에어타임 2배, AU 데드라인 초과 → IDR 루프 진입 확률 2배 | §3-3 | 중. 무선 환경 의존 |
| H-C | **HW 인코더/디코더 경합.** VTCompressionSession 2개 경합이 38ms 콜백의 근본 원인일 수 있고, 뷰어도 저지연 MediaCodec 2개 동시 구동 | §3-1, §3-5 | 중. H-A의 밑바탕 |
| H-D | **페어 단위 복구 증폭.** 한 타일 손실 → 양쪽 프리즈 + 페어 IDR + 750ms 재시도. 손실률이 낮아도 복구 비용이 페어 단위로 2배 | §2 뷰어 갭 복구, §3-2 | 중. H-A와 결합 시 큼 |

## 5. 아이디어 평가

### "압축을 잘한다" (압축 효율 개선)

- 현재: aggregate = min(60M, max(30M, w·h·fps·0.085)) → 4K60 기준 약 42M, 타일당 21M.
  ABR이 2스트림 팩터 ×0.65를 추가로 적용한다. 대역폭이 렉의 1차 원인이라는 증거는 아직 없다
  (평균 fps는 59.6으로 나온다 — 링크가 평시엔 따라잡는다).
- **HEVC 타일 인코딩**(동화질 -25~40% 비트레이트): VideoToolbox가 지원하고 뷰어 디코더도
  지원 가능. 단 (a) 저지단 튜닝 재검증, (b) 뷰어의 split 디코더 검증 경로가
  "동시 하드웨어 **H.264** 디코더"로 고정돼 있는지 확인(`splitDecoderName` 계열),
  (c) H-A 해결 없이 바꾸면 효과가 묻힌다. → **후순위 프로브로 분리.**
- 프레임 스킵/양자화 조정 같은 수단은 "평시 품질"을 깎는다. 렉은 평시 fps가 아니라
  **복구 에피소드**에서 나오므로 우선순위가 낮다.

### "모두 한번에 섞어서 보낸다" (통합 전송)

- 이미 인터리브되어 있다: `writeSplitPacketPair`가 좌/우 데이터그램을 하나의 페이서
  예산으로 섞어 보낸다(`CaptureSession+SplitTransport.swift:21-49, 242-306`).
- 좌/우를 **하나의 AU에 봉입**하는 진짜 통합은 G/L2 헤더·리어셈블러·FEC·gap_policy를
  전부 고쳐야 하고, 얻는 것은 패킷 헤더 절감 정도다. 비용 대비 이득이 없다.
- 오히려 반대 방향이 맞다: **묶어 기다리지 말고 각자 즉시 보내라.** 뷰어는 PTS 매칭 +
  1ms 홀드 + 4ms 그레이스로 이미 비대칭 도착을 흡수할 수 있다.

## 6. 권장 실험 계획 (숫자로 합격 판정)

공통: `tools/stream-stats.py`로 실시간 지표를 보며, 각 단계 전후로 같은 시나리오
(4K split 60fps, 고움직임 콘텐츠, 의도적 손실 유발 포함)를 3회씩 측정한다.

1. **기준선 관측 (변경 없음)**
   - 수집: 페어 IDR 에피소드/분(`paired_idr_resumes`, `split_session.rs:49-50`),
     IDR/s, AU 데드라인 초과율, gap→첫 출력 p95, 구간 최저 fps.
   - 합격 기준(문서화): 렉 에피소드가 어느 카운터와 상관되는지 특정.
     이미 §3 수치가 있으므로 재검증 수준.
2. **실험 1 — 랑데부 완화 (H-A, 최우선)**
   - 최소 변형(권장, 하루 단위): 만료 시 `beginPairedRecovery`(전체 리셋+페어 IDR) 대신
     **늦은 타일 AU만 폐기 + 해당 타일에만 IDR 요청**으로 완화.
     정상 스큐(만료 미만)는 지금처럼 emit한다.
   - 큰 변형(후속): 랑데부 제거 — 먼저 끝난 타일 즉시 전송, 페어 불일치 검사만 유지.
     `gap_policy`의 페어 가정 재검토가 따라온다.
   - 합격: 페어 리셋 횟수 ≥ 90% 감소, gap→첫 출력 p95 ≤ 100ms, 2.9s급 정지 0회.
3. **실험 2 — split 전송 데드라인 보정 (H-B)**
   - AU 데드라인 산식(`udpAccessUnitSendDeadlineUs`)에 좌+우 합산 크기를 반영하거나
     split 모드 상수를 보정. IDR 루프(300 keyframes/10min 사건의 split판) 방지가 목적.
   - 합격: 의도적 손실 환경에서 IDR/s 상승 없이 fps 하락 감소.
4. **실험 3 — 복구 데커플링 (H-D, 프로토콜 변경 수반)**
   - 한 타일 갭 → 해당 타일만 프리즈, IDR은 타일별 발급. 페어 동기는 presentation_sync가
     유지. L2/gap_policy/호스트 측 IDR 발급 경로 변경을 수반하므로 실험 1~2 이후.
   - 합격: 단일 타일 손실 시 반대편 타일 프리즈 0.
5. **실험 4 — HEVC 타일 프로브 (압축)**
   - `sidecar-quality-validation.md`의 듀얼 인코더 프로브를 HEVC로 재실행:
     콜백 p95, 화질(VMAF 또는 주관), 동일 화질 기준 비트레이트.
   - 합격: p95가 H.264 듀얼 대비 열화 없고 비트레이트 -20% 이상일 때만 채택 검토.

## 7. 다음 단계

- 실험 1의 최소 변형(만료 시 늦은 타일만 폐기)이 H-A를 가장 싸게 검증한다.
  다음 구현 세션 후보.
- 기준선 카운터(`paired_idr_resumes` 등)가 stream-stats 콘솔에 노출돼 있는지 확인하고
  없으면 추가 — 측정이 모든 합격 판정의 전제다.
