# 2디스플레이(splitVertical) 상태 보고서 — 2026-09-20 (작성 기준 HEAD = 3878580)

- 입력: `artifacts/decision-two-display.md` (2026-09-22 결정 기록), `docs/2026-09-08-two-display-lag-research.md`, facts-host/facts-viewer 조사 요약
- 검증 방법: 결정 기록이 인용한 file:line 근거를 작업 트리에서 직접 재확인(본 문서의 모든 file:line은 HEAD 3878580 기준 실측)
- 증상 정의: 4K splitVertical(3840×2160 캡처 → 1920×2160 타일 2개) 실사용에서의 렉/스터터/프리즈

## 1. 요약

1. **원인 순위는 확정됐다.** 1차 = H-C(HW 인코더 경합, 듀얼 AVE 콜백 p95 ≈ 40ms), 2차 = H-B 잔여(전송 예산·ABR 하한 — 하한 압박 24회/181s), 3차 = H-A 잔여(랑데부 만료의 페어 연쇄 — 빈도 재측정 전제), 4차 = H-D(갭 경로는 타일별 복구(R2)로 대부분 해소). 별도 트랙 = **세션 수명 결합**(split 렌더 복구가 React/Host 재준비에 의존 → 실사용 "프리즈/검은 화면" 클래스의 현재 1순위 후보).
2. **연구 문서(09-08) 이후 호스트·뷰어 양쪽에서 실제 수정이 반영됐다.** 랑데부 예산 기준점 변경(스큐 기준), soft expiry 경로, 크기 인지 전송 데드라인, 타일별 키프레임(R2), 뷰어 타일별 갭 프리즈, 갭 지연 p95 계측까지 — §2에 커밋 + file:line으로 확정했다.
3. **최근 머지(3878580 / 5a4dbc1 / 4a68d15 / 219eea8)는 split 미디어 경로에 무영향.** 변경 파일이 전부 확장 화면 제어 평면·UI·좌표 계층(`control.rs`, `virtual_display.rs`, `CGVirtualDisplayBridge.m`, 카탈로그/확장 화면 UI)이고 `Split/`·`split_session/` 전송 파일 0건(§4). 단, (a) 확장 화면 진입 빈도를 높인 변화이고 (b) 수명 결함이 바로 그 제어 평면에 있으며 (c) 장시간/잠자기 복귀가 미검증이라 실사용 불만 증가의 비-전송 요인 후보다.
4. **가장 큰 공백은 측정이다.** 마지막 수용 실측(09-16)은 현재 HEAD에서 3커밋 이전(d14996c/94fcb3d/3878580 미포함)이며, 그 실측에서 4K split은 캡처→Surface p95 132ms(단일 1440p 54ms), 하한 압박 24회로 "실패" 판정. **D0 재측정이 모든 후속 판정의 선행 조건이다.**

## 2. 연구 문서(09-08) 이후 반영된 수정 목록 (커밋 + file:line)

### 2.1 호스트 (native/macos-capture-shim)

| 항목 | 09-08 당시 | 현재 | 근거 (file:line) |
| --- | --- | --- | --- |
| 랑데부 예산 기준점 | 캡처 제출부터(공유 Metal/VT 지연 포함) | **첫 인코더 출력 도착(`firstReadyNs`)부터** — 좌/우 스큐만 예산 소비 | `Sources/Split/EncodedPairAssembler.swift:21,57,88,103-104`, 예산 산식 `Sources/Split/DualEncoderPipeline.swift:17-21,89-90` |
| 만료 처리 | 매 만료마다 전체 리셋 + 페어 IDR | 2연속까지 soft expiry 경로 분리(낡은 시퀀스만 폐기). 단 soft/hard 모두 최종적으로 `expirePairs` → `barrier.reset()` + `beginPairedRecovery()`(generation 범프 + 전 in-flight 해제 + 페어 키프레임)라 **에피소드 비용은 페어 단위 유지** | soft 분기 `Sources/Split/DualEncoderPipeline.swift:405-421`, `Sources/Split/SplitRecoveryPolicy.swift:5-13`, 복구 본체 `Sources/Split/SplitPairLifecycleState.swift:117-127`, 세션 처리 `Sources/Split/CaptureSession+Split.swift:239-268` |
| 흐름 제어 | `pendingSplitAccessUnits` 초과 시 세션 정지 | **소프트 상한**: 낡은 큐 전체 정리 + 페어 키프레임(뷰어 복구 결정론화), 세션은 생존 | `Sources/Split/CaptureSession+Split.swift:529-551` |
| 송신 데드라인 | 단일 스트림 산식 그대로 | **크기 인지 예산**: 좌+우 합산 프래그먼트 수 기반, 첫 버스트 전에도 초과 판정, 초과 시 `splitPairDeadlineExceeds` 별도 카운트 후 복구 | `Sources/Transport/CaptureSession+SplitTransport.swift:278-300,380-395`, 산식 `Sources/Transport/UdpPacingPolicy.swift:301-317` |
| FEC/페이싱 | — | FEC 싱글톤 테일 1회 중복 + 페어 국소성 버스트 제한, 페이싱 예산 80% A/B 확정(1b25805), worst syscall > 8ms 시 10s 경화 윈도우를 split IDR에도 적용 | `Sources/Transport/CaptureSession+SplitTransport.swift:53-82,418-421` |
| 타일별 키프레임(R2) | 없음(페어만) | **추가**: 뷰어 LCF1 side 정보 기반 단일 타일 IDR — generation 범프 없음, 피어 무중단. 조건: per-tile 능력 뷰어 + 요청 타일 신선 손실 + 피어 정상. 그 외(양쪽 버스트 손실 등)는 기존 페어 복구 | `Sources/Split/CaptureSession+Split.swift:145-152`, `Sources/Split/DualEncoderPipeline.swift:197-208`, 능력 플래그 `Sources/Transport/CaptureSession+ViewerControl.swift:541-545`, 분기 `Sources/Transport/CaptureSession+ViewerControl.swift:200-240` |
| 계측 | `paired_idr_resumes`(뷰어) 수준 | `splitPairTimeouts/ConsecutiveTimeouts/DeadlineExceeds/PerTileKeyframes/RecoveryBoundaryDiscards/QueueOverflowDrops` 등 통계 JSON 노출 | `Sources/Metrics/CaptureSession+Stats.swift:519-578` |
| 인코더 코덱 | H.264 고정 | split은 여전히 **H264_Main 고정**(단일 스트림만 HEVC 우선, 93844d6) | `Sources/Split/CaptureSession+Split.swift:76-77` |

### 2.2 뷰어 (native/android-viewer)

| 항목 | 09-08 당시 | 현재 | 근거 (file:line) |
| --- | --- | --- | --- |
| 갭 프리즈 | 한 타일 누락 → 양쪽 프리즈 + 페어 IDR | **타일별**: 해당 타일만 마지막 좋은 프레임 유지, 피어 계속 스트리밍. 단일 타일 갭도 타일별 IDR 요청(R2) | `src/renderer/split_session/gap_policy.rs:48-70`, `src/renderer/split_session.rs:335-345` |
| 복구 게이트 | 페어 750ms 쿨다운만 | 페어 750ms 쿨다운 유지 + **타일별 에피소드 독립 쿨다운**; 디코더 결함은 하드 페어/단일 타일 경로 분기 | `src/renderer/recovery.rs:3,164-170,298-312`, `src/renderer/split_session.rs:392-431` |
| 계측 | `paired_idr_resumes` | `paired_idr_resumes` + `per_tile_idr_resumes` — 1Hz 로그 + LCF1 v3 접미(길이 관용 파싱) | `src/renderer/split_session.rs:44-56`, `src/renderer/stats.rs:93-98` |
| 갭 지연 측정 | 최대값 수준 | `GapEpisodeTracker`: gap→IDR, IDR→첫 출력, gap→첫 출력 구간별 p95 시리즈 | `src/renderer/split_latency.rs:216-225` 이하 |
| 프레젠테이션 | 1ms 홀드 + 4ms 그레이스 | 유지 + vsync 정렬 park 지터 버퍼(092b53c) | `src/renderer/presentation_sync.rs:62-67` |

주요 커밋(작업 트리 git log로 확인): b419f06(09-12, 전송 판정 계약 통일·split 폐기 키프레임 요청·NACK 게이트), 93844d6(09-15, 단일 스트림 HEVC 우선), 1b25805(09-18, pacing 예산 80% 확정), 092b53c(09-18, park-5 지터 버퍼), c3d4d47(09-17, 복구 폭풍 완화), 04c1fa9/d14996c(09-19, NACK 사이드 키·워치독 IDR·전송 배치), 94fcb3d(09-20, retransmit 링 보강).

## 3. 현재 남은 병목 순위와 근거

| 순위 | 병목 | 근거 |
| --- | --- | --- |
| **별도 1순위** | **세션 수명 결합**: split 렌더 복구가 메인 화면(React/Host) 재준비에 의존 → 프리즈/검은 화면, 수동 재오픈으로만 복구 | 09-16 실기 재현: 메인 창 제거 후 영상 창 검은 화면 지속, 로그 `split render recovery requires React/Host re-preparation` (`docs/2026-09-16-final-test-followup.md` §3, :70-72) |
| **1차** | **H-C: HW 인코더 경합(듀얼 AVE 콜백 지연)** | 복구 폭풍 제거 후에도 지연 격차 잔존 — 4K split 캡처→Surface p95 132ms/최대 148ms vs 같은 링크 1440p 단일 41ms/54ms, RTT p95 10ms(링크 여유) (`docs/2026-09-16-performance-acceptance.md:30-36`). 듀얼 AVE 4K 타일 콜백 p95 40.24/39.67ms vs 단일 RTVC 타일 11.61ms (`docs/2026-09-16-performance-acceptance.md:56-61`, `docs/sidecar-quality-validation.md:17-21`). 격차(~78ms) ≈ 듀얼 콜백 p95(~40ms) + 페어 표시 정렬로 정합. 증상은 프레임 드롭형이 아니라 **지연·지터형 렉 + 인터랙션 반응 저하**로 이동 |
| **2차** | **H-B 잔여: 전송 예산·ABR 하한** | `bitrateFloorCollapseCount` 24회/181s — 4K split 비트레이트 하한이 링크 혼잡 예산을 지속 초과(`Sources/Encoder/CaptureSession+AdaptiveBitrate.swift:268-283`). 09-16 프레임 예산 개선은 split 전송 제외 명시(`docs/2026-09-16-udp-frame-budget-improvement.md:45`). 무선 환경에서 IDR 루프 재진입 위험 |
| **3차** | **H-A 잔여: 랑데부 만료의 페어 연쇄** | 예산이 스큐 기준으로 바뀌어(§2.1) 빈도는 크게 낮아졌을 것으로 추정되나 **현재 HEAD 실측 없음**. 구조상 만료 1회가 여전히 전 in-flight 해제 + 큐 wipe + 페어 IDR로 연쇄(`SplitPairLifecycleState.swift:117-127`) — 1회 발동 비용은 페어 전체 |
| **4차** | **H-D: 페어 복구 증폭** | 갭 경로는 타일별 프리즈 + 타일별 IDR(R2)로 데커플링 완료(§2.2). 잔여 페어 경로(전송 결함·양쪽 버스트 손실)는 참조 체인 정합을 위한 설계 선택으로 유지 |

순위 판정의 한계: 09-16 실측이 HEAD - 3커밋이므로 H-A 잔여 빈도(`splitPairTimeouts`)와 하한 압박 원본 신호는 D0 재측정 전까지 중간 버전 기준 값이다.

## 4. 최근 머지의 split 경로 영향 (코드 근거 확정)

| 커밋 | 변경 파일 | split 미디어 경로 영향 |
| --- | --- | --- |
| 3878580(머지) / 5a4dbc1 | host `control.rs`·`lib.rs`·`virtual_display.rs`, viewer catalog/확장 화면 UI(`ExtensionDisplayCard.tsx`, `use-extension-display.ts` 등), `control-contract`, UI 회귀 도구 | **없음** — `Split/`·`split_session/` 전송 파일 0건. 확장 화면 열기·배치·크기(제어 평면) 개선 |
| 4a68d15 | shim `CaptureSession+Cursor.swift`·`CaptureSession+Input.swift`·`CursorStreamCoordinator.swift`·`CGVirtualDisplayBridge.m` | **없음** — ARC 소유권·모드 재적용·좌표 갱신 수정. 미디어 페이서·소켓·큐 무관 |
| 219eea8 | 검증 문서 2건 | 없음(문서만) |

판정: split 미디어 파이프라인 회귀 가능성은 코드상 배제. 다만 이 머지는 (a) 확장 화면 사용 빈도를 높였고, (b) §3 별도 1순위 수명 결함이 바로 이 제어 평면에 있으며, (c) 219eea8 검증 문서 기준 장시간/잠자기 복귀가 미검증 — 실사용 불만 증가의 비-전송 요인으로 D4 트랙의 근거가 된다.

## 5. 수정 순서와 수용 기준

공통 시나리오: 4K split 60fps, 고움직임 콘텐츠, 181s × 3회, 동일 링크. 도구: `tools/stream-stats.py`(idrRes/gaps/recov/wire/age) + 호스트 통계 JSON(`CaptureSession+Stats.swift:519-578` 항목).

### D0 — 기준선 재측정 (관측만, 선행 조건) — **최우선**
- 내용: 현재 HEAD에서 §2 계측 항목 전부 수집. 특히 `splitPairTimeouts`(H-A 잔여 빈도), 하한 압박 시의 원본 혼잡 신호(pacing p95 / worstDatagramSyscallUs / RTT / 수신 지연 중 무엇이 바닥을 누르는지), 시작 구간 52-60fps 변동 원인.
- 합격: 3회 측정 모두 카운터 누락 없이 수집, 렉 에피소드-카운터 상관을 문서로 특정. 이후 모든 단계의 전·후 비교 기준선.

### D4 — 세션 수명 결합 제거 (별도 트랙, D0과 병행 가능)
- 내용: split 렌더 복구가 메인 화면(React/Host) 재준비에 의존하지 않고 영상 창이 스스로 재연결. 확장 화면 열기/닫기/재오픈(3878580가 고친 계층)과의 상호작용 포함 실기 검증.
- 합격: 메인 창 제거 시나리오 5회 연속 검은 화면 0, 자동 복구 ≤ 3s, 30분 장시간·잠자기 복귀 후 스트림 지속.

### D1 — 전송 예산·ABR 하한 잔여 (H-B)
- 내용: (관측) D0 결과로 하한 압박 신호 규명 → (구현) split에 09-16 프레임 예산 계열 판정 기준 적용 + 필요 시 4K split 하한/해상도 사다리 조정(강하 단계 신설 등). IDR 데드라인 클래스(750ms, `UdpPacingPolicy.swift:303-305`) 재검토 포함.
- 합격: 181s × 3에서 하한 압박 ≤ 2, 새 복구 키프레임 ≤ 2, 구간 최저 fps ≥ 55, 송신 실패 0, 의도적 손실 환경에서 IDR/s 상승 없이 fps 하락 감소.

### D2 — 랑데부 만료의 진짜 타일별화 (H-A 잔여)
- 내용: `expirePairs`가 만료 시퀀스만 폐기하고 건강한 반쌍/리스를 유지하며, 늦은 타일에만 IDR(`requestTileKeyframe`) 발급. D0에서 `splitPairTimeouts`가 0에 수렴하면 본 단계는 보류 가능.
- 트레이드오프: (a) 최소 변형(만료 타일만 폐기+타일 IDR) — 참조 체인 국소화, 비용 작음 / (b) 랑데부 제거(선착 즉시 전송) — H-A 근절이나 gap_policy·프레젠테이션 재검증 필요. (a) 먼저, (b)는 (a) 후 수치로 판단.
- 합격: 단일 만료 에피소드에서 반대 타일 프레임 중단 0(`per_tile_idr_resumes` 수렴), 페어 IDR 발생률(`paired_idr_resumes` ÷ `splitPairTimeouts`) ≤ 0.2, 2.9s급 정지 0회, gap→첫 출력 p95 ≤ 100ms.

### D3 — HEVC split 타일 프로브 (H-C 완화)
- 내용: 단일 스트림의 HEVC 우선(93844d6)을 split 타일로 확장. split 현재는 H264_Main 고정(`CaptureSession+Split.swift:76-77`). 듀얼 콜백 p95 재검증 + 동일 화질 비트레이트 비교. 비트레이트 −25~40%는 D1 하한 압박을 직접 완화.
- 합격: 듀얼 콜백 p95가 H.264 듀얼(≈40ms) 대비 +5% 이내, 동일 화질 비트레이트 −20% 이상일 때만 채택 검토. 채택 시 캡처→Surface p95 132ms → ≤ 100ms 목표.
- 유의: HEVC는 콜백 지연 자체를 줄이지 않는다(비트레이트 경감 경로). 콜백 지연 직접 완화(AVE 세션 우선순위·인코더 예산 재조정)는 별도 실험으로 열어두고, 지연 목표는 표시 단(park 버퍼·홀드)과 함께 종합 판정. 채택 전 뷰어 split 디코더 동시 구동(H.264 하드웨어 2개 가정 경로) 검증 선행.

### 전체 수용 기준 ("2디스플레이 잘 됨" 판정)
181s × 3회에서: 캡처→Surface p95 ≤ 100ms·최대 ≤ 150ms, 구간 최저 fps ≥ 55, gap→첫 출력 p95 ≤ 100ms, 500ms 초과 복구 정지 0회, `paired_idr_resumes` ≤ 2, 하한 압박 ≤ 2.

## 6. 실물 측정이 필요한 항목 체크리스트

- [ ] **D0 기준선**: 현재 HEAD에서 4K split 181s × 3회 — `splitPairTimeouts`/`splitPairConsecutiveTimeouts`/`splitPairDeadlineExceeds`/`splitPerTileKeyframes`/`splitRecoveryBoundaryDiscards`/`splitPairQueueOverflowDrops` 전 수집
- [ ] 캡처→Surface 지연(중앙값/p95/최대) — 09-16 값(86/132/148ms)과 HEAD 3커밋 차이 재확인
- [ ] 같은 링크 1440p 단일 대조(중앙값 41ms / p95 54ms 기준) — H-C 격차 정량화
- [ ] 하한 압박 24회 재현 시의 원본 신호: pacing p95 / worstDatagramSyscallUs / RTT / 수신 지연 중 지배 항목 특정
- [ ] 시작 구간 52-60fps 변동의 원인(캡처 웜업 vs 인코더 재협상 vs ABR 초기화) 분리
- [ ] H-A 잔여 빈도: `splitPairTimeouts` 발생 여부와 렉 에피소드 상관 (D2 보류/진행 판정 입력)
- [ ] 세션 수명 시나리오: 메인 창 제거 → split 영상 창 상태 5회, 자동 복구 시간 측정 (D4 합격 입력)
- [ ] 30분 장시간 스트리밍 + 잠자기 복귀 후 스트림 지속 여부(현재 미검증, 219eea8 검증 문서 공백)
- [ ] 확장 화면 열기/닫기/재오픈 반복(3878580 계층) 중 split 스트림 품질 — 스트리밍 품질은 이 머지 후 실기 미검증
- [ ] 무선 환경 변수 통제: 동일 AP·거리·대역에서 전·후 비교(D1 판정 전제)
- [ ] (D3 선행) HEVC split 인코딩 프로브: 듀얼 콜백 p95 + 동일 화질 비트레이트
- [ ] (D3 선행) 뷰어 split HEVC 디코더 동시 구동 가능 여부(하드웨어 2개 가정 경로 확인)

## 7. 리스크·미해결

- 09-16 실측과 HEAD의 3커밋 차이 — D0 없이는 모든 순위 판정이 중간 버전 기준.
- 무선 대역·거리·AP 환경이 하한 압박에 개입 — D1 합격 기준은 동일 링크 전후 비교로만 판정.
- HEVC 채택 시 뷰어 split 디코더 동시 구동 검증이 선행되지 않으면 D3는 착수 불가.
- D4는 전송 품질 지표로 측정되지 않는 사용자 체감(검은 화면) 클래스 — 수용 기준을 시나리오 재현 횟수로 별도 관리.
