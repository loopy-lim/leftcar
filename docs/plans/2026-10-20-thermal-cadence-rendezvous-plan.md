# Thermal 자원 상한·표시 cadence 계측·랑데부 실험 1·UI/UX 구현 계획

> **For Claude:** REQUIRED SUB-SKILL: Use implement-plan to implement this plan task-by-task.

**Goal:** "전반이 느리다" 재검증 결과(측정 부재 + 알려진 미해결 항목 중첩)에 대응해, (1) 표시 cadence 계측을 진단 NDJSON까지 도달시키고, (2) R-011 완화(thermal 인지 동시 디코더 상한·프로필 강등)를 뷰어 TS 정책 계층에 구현하며, (3) 2디스플레이 랑데부 만료를 늦은 타일 폐기+단독 IDR로 완화(실험 1)하고, (4) 첫 실행 가이드·설정 언어·패널 밀도 회귀 복구를 진행한다.

**Architecture:** cadence는 viewer-decoder 크레이트의 Surface release 공통 지점(output.rs `release_output_at`, decoder.rs render release 3곳)에서 전역 히스토그램으로 집계하고, 유일한 소비자인 DiagnosticsSampler 15초 틱이 sample 레코드에 기록한다(adb 금지 실기에서 NDJSON이 유일 수집 경로). thermal 판단은 전부 뷰어 TS 정책 계층(`thermal-policy.ts`)에 두고, Kotlin은 판단 없는 최신 샘플 전달(`getLatestSample`)만 한다(docs/09 §5). 랑데부는 만료 정상 분기(`DualEncoderPipeline.handlePairCallbackExpiry` :415-417)만 소프트화하고 인코더·정합성 실패 6곳과 연속 2회 하드 밸브는 유지한다. 호스트(control.rs·shim)는 변경하지 않는다 — 뷰어가 낮춘 수치를 그대로 수용하는 현재 계약을 유지한다.

**Tech Stack:** Rust (viewer-decoder, android-viewer), Kotlin (viewer-expo android), TypeScript/React (viewer-expo), Swift (macos-capture-shim).

**Spec:** 본 문서 + ADR-0006(thermal 인지 뷰어 자원 정책) + docs/plans/2026-10-19-viewer-diagnostics.md(진단 계약 확장) + docs/2026-09-08-two-display-lag-research.md(실험 1 설계).

## Global Constraints

- 각 Task는 독립 커밋. 테스트 → 실패 확인 → 구현 → 통과 확인 → 커밋(TDD). 문서 태스크는 예외.
- Kotlin import는 허용목록(`android.*`, `java.lang.*`, `java.util.*`, `java.security.*`, `kotlin.*`) 안에서만. `java.io`/`java.text` 금지 — fully-qualified 사용. Kotlin에 thermal 판단 로직 금지(클로저 DI 전달만).
- 진단 레코드 제약 유지: ≤1,024바이트, 허용 필드(앱/OS 버전, 숫자 지표, 안정적 오류 코드, 생명주기 이벤트 종류), denylist(IP/토큰/경로/원시 예외), 샘플은 매 틱 기록.
- docs/06 측정 위생: 배터리 전력(W) 추정 금지, `/sys/class/thermal` 원시 읽기 금지, Surface release는 물리 표시가 아님(`release_gap_*`로 명명), 필드명은 docs/06 §3·§11 정렬.
- React 변경 후 루트 `npx -y react-doctor@latest . --verbose` 100/100, `bun run typecheck`, 관련 vitest 재실행.
- `bun run test:architecture`는 기존 32건 red가 기준 — **신규 위반 0건**이 보증이다(green 주장 금지).
- 실기 수집은 adb 없이(사용자 실행)만. 실기 미수행 항목은 문서에 미검증으로 명시.
- 호스트 코드(apps/host-desktop/src-tauri, native/macos-capture-shim의 비-Split 경로) 변경 금지. Swift 변경은 Split 소프트 만료 경로 한정.
- thermal 임계(SEVERE 진입·LIGHT 해제·저배터리 15%)는 설계값이다 — `thermal-policy.ts` 상수 표로 고정하고 실기 데이터 후 조정 지점으로 문서에 남긴다.
- 커서 60Hz 폴링(CursorOverlayView.kt)과 HUD 250ms 폴링은 이번 배치에서 변경하지 않는다(033f1da 회귀 주석 의도 존중) — cadence·thermal 데이터로 다음 배치 판단.
- i18n은 ko/en 쌍 필수(i18n.test.ts parity).

## Tasks

### T0.1: 계약 문서 + ADR

Files:
- Create: `docs/plans/2026-10-20-thermal-cadence-rendezvous-plan.md` (본 문서)
- Create: `docs/decisions/0006-thermal-aware-viewer-resource-policy.md`

- [x] 자원 판단 계층(뷰어 TS)·Kotlin 패스스루·보수 임계+히스테리시스·호스트 수용 원칙을 ADR로 기록.
- [x] 커밋: `docs(viewer): R-011 완화와 랑데부 실험 1 구현 계획, thermal 자원 정책 ADR`

### T1.1: viewer-decoder cadence 히스토그램 + release 계측 (Rust)

Files:
- Create: `crates/viewer-decoder/src/android/cadence.rs`
- Modify: `crates/viewer-decoder/src/android/mod.rs`, `output.rs`(`release_output_at` 성공 후 기록), `decoder.rs`(render=true release 3곳 성공 후 기록)

- 히스토그램(1ms 버킷 + overflow) + count/max/last_release_ns, 전역 `PRESENTATION_CADENCE`(Mutex), `record_gap_ns` / `read_and_reset -> Option<CadenceWindow{count,p50,p95,p99,max}>`.
- [x] 단위 테스트: 알려진 gap 시퀀스의 분위수, read_and_reset 후 비움, overflow 버킷, 최초 release(간격 없음) 스킵, 버킷 경계.
- [x] `cargo test -p viewer-decoder` green.
- [x] 커밋: `feat(viewer-decoder): 표면 릴리스 간격 히스토그램 계측`

### T1.2: cadence JNI 게터 (Rust)

Files:
- Modify: `native/android-viewer/src/jni_exports.rs`(`leftcar_jni_stream_cadence` + u16 pack: count|p50|p95|max, count 65535 캡, 전부 0=데이터 없음), `jni_wrappers.rs`

- [x] 테스트: pack/센티널, read_and_reset 1회성.
- [x] `cargo test -p android-viewer` green.
- [x] 커밋: `feat(android-viewer): cadence 윈도우 JNI 게터`

### T1.3: 진단 sample에 cadence 기록 (Kotlin) + 계약 확장

Files:
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/diagnostics/DiagnosticsSampler.kt`(cadence 클로저 DI), `DiagnosticsRuntime.kt`(ViewerNative.streamCadence 배선, 부재·실패 시 null), `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/ViewerNative.kt`(external fun)
- Modify: `docs/plans/2026-10-19-viewer-diagnostics.md`(sample 레코드 `release_gap_p50_ms/p95_ms/max_ms/count` nullable 필드 계약)

- [x] JVM 테스트(DiagnosticsFormatsTest): cadence 필드 포함 포맷·null 필드·≤1024B.
- [x] `./gradlew :app:testDebugUnitTest --no-daemon` green.
- [x] 커밋: `feat(viewer): 진단 샘플에 표면 릴리스 간격 기록`

### T2.1: 최신 샘플 조회 네이티브 메서드 (Kotlin)

Files:
- Modify: `DiagnosticsRuntime.kt`(`@Volatile latestSample` 갱신), `DiagnosticsModule.kt`(`getLatestSample(promise)` — JavaOnlyMap, 없으면 resolve(null))

- [x] Robolectric 테스트(DiagnosticsModuleTest): 샘플 후 최신값, 값 없음 null, thermalStatus null 전달.
- [x] gradlew green. Kotlin은 판단 없이 보관·전달만.
- [x] 커밋: `feat(viewer): 진단 최신 샘플 조회 네이티브 메서드`

### T2.2: thermal 순수 정책 (TS)

Files:
- Create: `apps/viewer-expo/src/thermal-policy.ts`
- Test: `apps/viewer-expo/src/thermal-policy.test.ts`

- 상수 표: 진입 SEVERE(3)→capacityCap 2 + 시작 단변 1080 / CRITICAL(4)→cap 1 + 720 / 해제 LIGHT(1) 이하(MODERATE 유지 구간 히스테리시스). charging은 thermal 완화 없음, `batteryPercent≤15 && !charging` → cap −1(하한 1). 첫 값 없음 = 미적용.
- [x] 테스트: SEVERE 진입 cap 2, CRITICAL cap 1, LIGHT 해제, MODERATE 유지, charging 배터리 축 스킵, 저배터리 −1, 첫 값 없음 미적용, 상수 표 고정.
- [x] 루트 `bun run test -- src/thermal-policy.test.ts` green.
- [x] 커밋: `feat(viewer): thermal 자원 상한 순수 정책`

### T2.3: thermal 진단 이벤트 + 캐시 브리지 (TS)

Files:
- Modify: `src/diagnostics.ts`(세션 이벤트 `thermal_cap`/`thermal_cap_cleared` 추가, 숫자 필드 thermalStatus·capacityCap·startShortSideCap), `src/diagnostics-recorder.ts`(recordThermalCap/recordThermalCapCleared)
- Create: `src/thermal-state.ts`(TTL=SAMPLE_INTERVAL_MS 캐시, refresh, transition 구독·edge 기록)
- Test: `diagnostics.test.ts`, `diagnostics-recorder.test.ts` 확장, `thermal-state.test.ts` 신설

- [x] 테스트: 알 수 없는 이벤트 거절 유지, thermal_cap 레코드 필드·크기, edge 1회, TTL 캐시, 네이티브 부재 no-op.
- [x] 루트 `bun run test` green.
- [x] docs/plans/2026-10-19-viewer-diagnostics.md에 세션 이벤트 2종 계약 추가.
- [x] 커밋: `feat(viewer): thermal 상한 전환 진단 기록과 캐시 브리지`

### T2.4: 상한 적용 지점 순수 확장 (TS)

Files:
- Modify: `src/decoder-budget.ts`(DecoderReservations capacity overlay — `capacity()=min(hint, overlay)`), `src/streaming-policy.ts`(`resolveInitialStreamTarget`에 `thermalShortSideCap` 옵션 — externalRoute·panelShortSide와 min 체인), `src/adaptive-resolution.ts`(`AdaptiveObservation.thermalCapShortSide` — upshift 대상 클램프)
- Test: `decoder-reservations.test.ts`, `streaming-policy.test.ts`, `adaptive-resolution.test.ts` 확장

- [x] 테스트: overlay 차단·해제 복원, split 승격 차단(3840×2160@60 요구가 overlay로 먼저 막힘), 기존 엔트리 유지, 캡 min 체인, upshift 클램프·downshift 무영향·cap 없을 때 무영향.
- [x] 대상 3종 vitest green.
- [x] 커밋: `feat(viewer): 디코더 용량 오버레이와 thermal 시작·업시프트 캡`

### T2.5: openDisplay·적응 정책 배선 (TS/React)

Files:
- Modify: `src/use-catalog-model.ts`(openDisplay에서 thermal-state 캡 읽기 → plan·resolveInitialStreamTarget 전달), `src/use-stream-controller.ts`(관측 주입부에 cap 전달), `app/index.tsx`, `app/catalog.tsx`(마운트 예열 refresh)
- Test: `launch-stream.test.ts`, adaptive-launch 테스트 확장

- [x] 테스트: thermal 케이스 추가, 네이티브 부재 시 기존 동작 회귀 없음.
- [x] `bun run typecheck && bun run test` + react-doctor 100/100 + `bun run test:architecture`(기존 32 red 유지·신규 0).
- [x] 커밋: `feat(viewer): 창 열기·적응 정책에 thermal 상한 반영`

### T3.1: 랑데부 순수 계층 (Swift)

Files:
- Modify: `native/macos-capture-shim/Sources/Split/EncodedPairAssembler.swift`(`takeExpiredSequences` → `[(sequence, missingSide, readyValue?)]`), `SplitPairLifecycleState.swift`(`expireSinglePair(sequence:)` — generation 불변, 해당 시퀀스 in-flight·lease만 제거)
- Test: `Tests/SplitPolicyTests.swift`

- [ ] 테스트: 만료 시퀀스의 미도착 면 식별, 예산 내 반쪽 보존, expireSinglePair 후 generation·타 in-flight 불변·lease 반환.
- [ ] `tools/build-macos-capture-shim.zsh split-policy-test <out> && <out>` exit 0.
- [ ] 커밋: `feat(capture): 페어 만료의 타일별 폐기 준비`

### T3.2: 랑데부 소프트 만료 경로 (Swift)

Files:
- Modify: `native/macos-capture-shim/Sources/Split/DualEncoderPipeline.swift`(handlePairCallbackExpiry 정상 분기 교체, cancelledSequences 등록, publish 가드 — 늦은 AU lease 회수·폐기, `tileEmittedSolo` 이벤트), `CaptureSession+Split.swift`(이벤트 소비), `SplitRecoveryPolicy.swift`(필요 시 헬퍼)
- Test: `Tests/SplitPipelineTests.swift`

- [ ] 테스트: 오른쪽 타일 지연 합성 → pairExpired + dropped(리셋) 부재 + 왼쪽 단독 emit + 오른쪽 requestTileKeyframe + 늦은 오른쪽 AU 폐기 + 정상 페어 지속 + 연속 2회 만료 하드 밸브 + generation 불변 + fault injection 상호(fault 주입 시 기존 경로 유지).
- [ ] `split-test` + `split-policy-test` 재실행 exit 0.
- [ ] 커밋: `feat(capture): 랑데부 만료를 늦은 타일 폐기+단독 IDR로 완화(실험 1)`

### T4.1: 첫 실행 가이드 강화 (React)

Files:
- Modify: `apps/viewer-expo/src/components/SetupGuideCard.tsx`(행동 체크리스트: Mac 호스트 앱 실행 → 페어링 Action → 창 열기, 완료 상태·완료 시 축소), `app/index.tsx`(props: lastHost·isConnected 전달), `packages/ui-tokens/src/i18n.ts`(viewer 네임스페이스 ko/en)

- [ ] i18n parity 테스트 통과, 완료 판정 순수 로직 테스트.
- [ ] `bun run test` + `bun run typecheck` + react-doctor 100/100.
- [ ] 커밋: `feat(viewer): 첫 실행 가이드를 행동 가능한 체크리스트로 강화`

### T4.2: 설정 언어 전환 + 치환 정리 (React)

Files:
- Modify: `app/catalog.tsx`(SettingsContent 언어 섹션 — index.tsx 토글 로직 이식, :470 `.replace` → `interpolate`), `packages/ui-tokens/src/i18n.ts`
- 검토: `apps/host-desktop/src/streamTermination.ts` 한국어 — host i18n 인프라 부재 시 이번 배치 제외·문서화

- [ ] `bun run test` + typecheck + react-doctor.
- [ ] 커밋: `refactor(viewer): 설정에 언어 전환 추가와 치환 헬퍼 정리`

### T4.3: 패널 밀도 재적용 (React)

Files:
- Modify: `app/index.tsx`, `app/catalog.tsx`, `app/host.tsx`, `app/pairing.tsx`(useWindowDimensions + `panelDensityScale` + `applyPanelDensity` — 27e33e4 패턴 복원)

- [ ] `panel-density.test.ts` 유지 green, react-doctor 100/100.
- [ ] 커밋: `feat(viewer): 4 화면에 패널 밀도 곡선 재적용`

### T4.4: 수정 화면 토큰 일관성 (React)

Files:
- Modify: T4.1–T4.3이 건드린 화면·컴포넌트 한정(타이포·간격·색 토큰 감사)

- [ ] react-doctor 100/100, 신규 범위 확장 없음.
- [ ] 커밋: `style(viewer): 수정 화면 토큰 일관성 정리`

### T5.1: 실기 수집 절차 문서화

Files:
- Modify: 본 문서(수동 검증 절차 섹션), 필요 시 `tools/perf-matrix/collect.mjs`·`tools/stream-stats.py` 신규 필드 점검

- [ ] 시나리오별 체크리스트 + 합격 기준 기록.
- [ ] 커밋: `docs: 실기 수집 절차와 합격 기준`

### T5.2: 실기 수집 (사용자 실행, adb 금지)

- 시나리오: (1) 4창 thermal/battery, (2) 4K split 60 고움직임 3회, (3) 단일 baseline.
- 결과는 `docs/2026-10-XX-…md`로 기록(측정 위생 준수). 미수행 시 미검증 명시.

### T6.1: 문서 마무리

Files:
- Modify: `docs/09-risk-register.md`(R-011 상태·charger-mode 해석·실기 미검증), `docs/2026-09-08-two-display-lag-research.md`(실험 1 결과 칸), `docs/decisions/0006-…`(실기 데이터로 임계 확정), `docs/2026-09-17-interactive-pacing-plan.md`(P3 계측 연기 해소), 커서 폴링 재검토 후보 기록

- [ ] 커밋: `docs: R-011 완화 결과와 랑데부 실험 1 판정 기록`

## 수동 검증 절차(adb 없음, 사용자 실행)

사전: `bun run build android-internal` → MTP로 Galaxy XR에 사이드로드. 호스트는 Mac에서 leftcar 실행.

**시나리오 1 — 4창 thermal/battery (R-011)**
1. 네 창을 열고 장시간 사용해 thermal이 오르는 상태를 만든다(충전 없이).
2. 기기가 꺼지거나 세션을 종료한 뒤 재부팅 → 앱 실행 → 설정 맨 아래 진단 기록 → 요약 확인 → 진단 내보내기 → 파일명 확인.
3. MTP로 Downloads의 `leftcar-diagnostics-*.ndjson`을 Mac에 복사.
4. 판독: `thermal_status` 시계열에서 SEVERE(3) 도달 시 `thermal_cap` 이벤트와 이후 `battery_percent`·`temp_deci_c` 추이 확인. `release_gap_p50_ms/p95_ms/max_ms`로 창 전환·부하 구간의 표면 릴리스 간격 확인. docs/plans/2026-10-19 판독표로 꺼짐 원인 후보 병기.
5. 비교: thermal cap 적용 전(이전 빌드) 대비 창 수·프로필 변화가 진단 세션 이벤트로 흔적 남는지.

**시나리오 2 — 4K split 랑데부 (실험 1)**
1. 4K 확장 디스플레이 split 스트림을 고움직임 콘텐츠(예: 영상 재생)로 3회, 회당 5분 이상 실행.
2. 호스트 측 `tools/stream-stats.py`로 `idrRes`·`dropped`(리셋) 이벤트 관찰 — 실험 1 합격 기준: 페어 리셋 ≥90% 감소, gap→첫 출력 p95 ≤100ms, 2.9초급 정지 0회.
3. 뷰어 체감 정지 발생 시 시각·상황을 기록해 이벤트 시계열과 대조.

**시나리오 3 — 단일 baseline**
1. 창 1개, 평소 프로필로 10분 사용 → 진단 내보내기.
2. 시나리오 1·2 결과와의 비교 기준선으로 사용.

호스트 측 정밀 측정이 필요하면 `bun tools/perf-matrix/collect.mjs --context <run-context.json> --duration 1800 --output <dir>` 병행.

**미검증 한계(수행 전):** thermal 임계·감폭 효과, 랑데부 소프트 경로의 실기 안정성, 단독 emit의 뷰어 흡수(PTS 매칭+1ms 홀드+4ms 그레이스 의존), cadence 값의 분포 — 모두 T5.2 수행 전까지 미검증이다.
