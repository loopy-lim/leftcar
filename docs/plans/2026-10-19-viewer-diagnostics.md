# Viewer Diagnostics Capture + Export Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** adb 없이 Galaxy XR 기기 꺼짐의 원인(방전/전압 강하/패닉 재부팅/크래시)을 가릴 수 있는 뷰어 내부 진단 기록(배터리·thermal·세션 이벤트)과 사용자 주도 내보내기(Downloads)를 구현한다.

**Architecture:** Kotlin은 플랫폼 접착만 담당한다(배터리/thermal 읽기, append·5MiB 회전·내보내기, 크래시 마커, 15초 샘플러 상수). 스키마·redaction·세션 이벤트 형식은 TS(vitest 검증). 샘플러와 크래시 마커는 JS가 죽어도 동작해야 하므로 Kotlin이 직접 기록하며, 그 결과 sample·crash 레코드의 JSON 형식은 JVM 테스트로 고정하고 TS 형식은 vitest로 고정한다. Rust/Rustra는 이 뷰어 로컬 기능에 필요 없다(ADR-0002 경계 유지).

**Tech Stack:** TypeScript (viewer-expo), Kotlin (viewer-expo android).

**Spec:** 본 문서 + `docs/07-security-privacy.md` §16·§18 (사용자 승인 2026-10-19 — 내보내기 위치 Downloads, 샘플링 앱 실행 내내 15초).

## Global Constraints

- `docs/07` §16 명세를 그대로 따른다: local-only 저장, 허용 필드만(앱/OS 버전, 숫자 지표, 안정적 오류 코드, 생명주기 이벤트 종류), 5 MiB 회전 + 한 세대 백업, 레코드 ≤1,024바이트(줄바꿈 제외), 기록 전 크기 확인, 같은 프로세스 append/회전 직렬화, 0600 권한(모드 `MODE_PRIVATE` + 소유자 전용 강제), 파일 시스템 오류 시 기록 중단·스트림 유지.
- denylist: window title, QR/pairing offer, IP/MAC(`SessionView.viewerAddr` 포함), 토큰, 파일 경로, raw 예외 메시지. 크래시 레코드는 예외 **클래스명만**.
- §18 필수 테스트명 `diagnostics_redact_title_path_token_and_ip`을 TS 측에 구현한다.
- Kotlin import는 허용목록(`android.*`, `java.lang.*`, `java.util.*`, `java.security.*`, `kotlin.*`) 안에서만. `java.io`/`java.text` import 금지 — fully-qualified 사용. `java.text.SimpleDateFormat` 대신 `java.util.Calendar` + `String.format`.
- 콘텐츠 금지 규칙 준수: 네트워크·코덱 관련 문자열 미사용.
- 아키텍처 검사(`bun run test:architecture`)는 현재 HEAD에서 기존 32건(km/)으로 red다. 이번 변경 후에도 red가 예상이며, 보증은 **신규 위반 0건**이다.
- 배터리 전력(W) 추정 금지, `/sys/class/thermal` 원시 읽기 금지(docs/06 측정 위생). thermal은 `PowerManager.getCurrentThermalStatus()`(API 29+)와 `getThermalHeadroom(int)`(API 30+)만.
- 충전 판정은 `BatteryManager.BATTERY_STATUS_CHARGING(2) || BATTERY_STATUS_FULL(5)`.
- 레코드 필드명은 docs/06 §3·§11과 정렬(`battery_percent`, `charging`, thermal status timeline 등)해 이후 벤치마크 재사용이 가능하게 한다.
- 샘플은 **매 틱 기록**(변화 시만 기록 금지) — 기록 공백 자체가 사망 증거다.
- 내부 빌드 전용 기능이다(docs/completion-and-support.md 단계 사다리). 공개 릴리스 포함 주장 금지. 실기 미검증 항목은 문서에 명시한다.
- 각 Task는 독립 커밋. 테스트 → 실패 확인 → 구현 → 통과 확인 → 커밋(TDD).
- React 변경 후 루트 `npx -y react-doctor@latest . --verbose` 100/100, `bun run typecheck`, 관련 vitest 재실행.

### Task 1: 구현 계약 문서

Files:
- Create: `docs/plans/2026-10-19-viewer-diagnostics.md` (본 문서)

- [x] 계획 문서 작성.
- [x] 커밋: `docs(plan): 뷰어 진단 캡처와 사용자 내보내기 구현 계약`

### Task 2: 진단 레코드 스키마와 §16 redaction 포맷터 (TS)

Files:
- Create: `apps/viewer-expo/src/diagnostics.ts` (순수 로직 + 비네이티브 fallback)
- Test: `apps/viewer-expo/src/diagnostics.test.ts`

인터페이스:
```ts
export const SAMPLE_INTERVAL_MS = 15_000;
export const MAX_RECORD_BYTES = 1024;
export const MAX_FILE_BYTES = 5 * 1024 * 1024;
export const BACKUP_GENERATIONS = 1;
export const BOOT_DETECTION_UPTIME_MS = 120_000;

// 세션 레코드는 TS가 살아 있을 때만 발생한다. sample·crash 레코드는
// JS 사망 후에도 기록되어야 하므로 Kotlin 측이 같은 스키마로 직접 기록한다.
export type DiagnosticsSessionEvent =
  | "session_start" | "foreground" | "background"
  | "stream_start" | "stream_stop";
export interface DiagnosticsSessionRecord {
  ts: number; upMs: number; event: DiagnosticsSessionEvent;
  appVersion?: string; osApi?: number; bootDetected?: boolean;
  profile?: string; decoderSlots?: number;
}
export function formatSession(record: DiagnosticsSessionRecord): string;
export function summarizeCounts(counts: Readonly<Record<string, number>>): string;
```

- [x] §18 필수 테스트 `diagnostics_redact_title_path_token_and_ip` 포함 red 테스트 작성(`bun run test -- src/diagnostics.test.ts` 실패 확인): 제목·IP·토큰·파일 경로 주입 시 부재, `profile`은 `[a-z0-9_-]{1,40}` 이외 `"unknown"` 치환, 알 수 없는 이벤트 종류 거절, 레코드 ≤1024바이트, 요약 형식.
- [x] 포맷터 구현(snake_case 키, docs/06 필드명 정렬). green 확인.
- [x] 커밋: `feat(viewer): 진단 세션 레코드 스키마와 §16 redaction 포맷터`

### Task 3: 진단 스토어 append·회전·내보내기 (Kotlin)

Files:
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/diagnostics/DiagnosticsStore.kt`
- Test: `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/diagnostics/DiagnosticsStoreTest.kt`

인터페이스:
```kotlin
interface ExportSink { fun open(name: String): java.io.OutputStream }
class DiagnosticsStore(
  baseDir: java.io.File,
  nowMs: () -> Long,
  upMs: () -> Long,
) {
  fun append(line: String): Boolean      // 기록 전 ≤1024B 검사, 5MiB 회전(.ndjson → .ndjson.1), FS 오류 래치
  fun appendCrash(exceptionClass: String): Boolean
  fun exportTo(sink: ExportSink): String // 백업(.1) 먼저, 활성 나중 — 시간순 병합
  fun counts(): Map<String, Int>         // 이번 실행 성공 기록 개수(종류별)
  fun stopWriting(): Unit                // 래치 수동 유발(테스트·오류 경로)
}
```
- crash 레코드 JSON은 JVM 테스트로 형식 고정(클래스명만, 메시지 부재).
- [x] red 테스트 작성(`cd apps/viewer-expo/android && ./gradlew :app:testDebugUnitTest --no-daemon` 실패 확인): append 내용, 5MiB 회전, 1024B 초과 거절(false), FS 오류 래치 후 무음 거절, crash 형식(denylist 문자열 부재), export 시간순 병합.
- [x] 스토어 구현. green 확인.
- [x] 커밋: `feat(android): 진단 스토어 append·5MiB 회전·단일 백업 세대`

### Task 4: Diagnostics 네이티브 모듈·샘플러·크래시 마커 (Kotlin)

Files:
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/diagnostics/DiagnosticsModule.kt`, `DiagnosticsPackage.kt`, `DiagnosticsSampler.kt`, `CrashMarker.kt`, `DiagnosticsRuntime.kt`
- Test: `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/diagnostics/DiagnosticsModuleTest.kt` (Robolectric)

인터페이스:
```kotlin
class DiagnosticsModule(ctx: ReactApplicationContext) : ReactContextBaseJavaModule() {
  override fun getName() = "Diagnostics"
  @ReactMethod fun appendRecord(line: String, promise: Promise)
  @ReactMethod fun getSummary(promise: Promise)          // 종류별 개수 — 사용자 확인용 요약
  @ReactMethod fun exportToDownloads(promise: Promise)   // Downloads insert, 표시 이름 resolve
}
class DiagnosticsSampler(
  intervalMs: Long = 15_000,
  readSnapshot: () -> Bundle?,           // 기본: sticky ACTION_BATTERY_CHANGED extras (테스트 주입)
  thermalStatus: () -> Int?,             // 기본: PowerManager.getCurrentThermalStatus (API 29+ 가드)
  thermalHeadroom: () -> Double?,        // 기본: getThermalHeadroom (API 30+ 가드)
  append: (String) -> Boolean,
  schedule: (Runnable, Long) -> Unit,    // HandlerThread postDelayed 주입점
)
object CrashMarker { fun install(store: DiagnosticsStore): Unit }
```
- sample 레코드 JSON은 Kotlin이 직렬화하고 JVM/Robolectric 테스트로 형식 고정(`{"ts":..,"up_ms":..,"kind":"sample","battery_percent":..,"charging":..,"plugged":..,"voltage_mv":..,"temp_deci_c":..,"thermal_status":..,"thermal_headroom":..}`).
- MediaStore.Downloads insert: `IS_PENDING` 플립, 단일 `java.util.concurrent.Executor`(메인 스레드 금지). 실제 MediaStore 경로는 Robolectric 한계로 fake sink 테스트 + 실기 수동 검증으로 나눈다.
- [x] Robolectric red 테스트: wiring, appendRecord resolve, fresh 환경 boot_detected, 주입 스냅샷으로 샘플러 1틱→1줄, fake sink export, 크래시 마커가 클래스명만 기록.
- [x] 구현. `./gradlew :app:testDebugUnitTest` green.
- [x] 커밋: `feat(android): Diagnostics 네이티브 모듈과 15초 샘플러·크래시 마커`

### Task 5: 패키지 등록과 크래시 핸들러 설치

Files:
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/MainApplication.kt`

- [x] packageList에 `dev.leftcar.viewer.diagnostics.DiagnosticsPackage()` 추가, `onCreate`에서 `super.onCreate()` 후 `loadReactNative` 전 `CrashMarker.install(...)` 호출.
- [x] `bun run test:architecture` — 기존 32건 외 신규 위반 0건 확인(green 주장 금지). `./gradlew :app:compileDebugKotlin` 성공.
- [x] 커밋: `feat(android): Diagnostics 패키지 등록과 크래시 핸들러 설치`

### Task 6: TS 브리지·설정 UI·세션 이벤트 (TS/React)

Files:
- Create: `apps/viewer-expo/src/diagnostics.native.ts`
- Modify: `apps/viewer-expo/src/use-catalog-model.ts` (stream 시작/종료 경로에서 stream_start/stop 기록 — profile + `deviceDecoderReservations` 슬롯), `apps/viewer-expo/app/catalog.tsx` (SettingsContent에 진단 섹션: 상시 기록 안내 Notice + "진단 내보내기" Action + 저장 파일명·종류별 개수 요약), `packages/ui-tokens/src/i18n.ts` (viewer 네임스페이스 ko·en 쌍)
- Test: 기존 모델 테스트 확장

i18n 키: `diagnosticsTitle`, `diagnosticsNote`, `diagnosticsExport`, `diagnosticsExporting`, `diagnosticsExportDone({name})`, `diagnosticsExportFailed`, `diagnosticsSummaryLabel`, `diagnosticsUnavailable`.

- [x] 세션 이벤트 기록 경로 vitest 확장. UI 구현. 루트에서 `npx -y react-doctor@latest . --verbose` 100/100, `bun run typecheck && bun run test`.
- [x] 커밋: `feat(viewer): 진단 내보내기 설정 UI와 세션 이벤트 기록`

### Task 7: 문서 갱신과 최종 검증

Files:
- Modify: `docs/07-security-privacy.md` §16(사용자 주도 내보내기·요약 제공으로 경계 문장 갱신, "공유 전 확인" 유지, 네이티브 크래시 미기록 한계 명시), `docs/README.md` (상태 표 동기), `docs/completion-and-support.md` (내부 전용 표기)

- [x] 전체 검증: `bun run typecheck && bun run test && bun run test:architecture`(32 기준) && `./gradlew :app:testDebugUnitTest` && `npx -y react-doctor@latest . --verbose`. Cargo 미변경.
- [x] 수동 검증 절차 문서화(내부 APK 설치 → 세션 → 기기 꺼짐 → 재부팅 → 설정에서 내보내기 → MTP 복사 → 패턴 판독: 방전/전압 강하/패닉 재부팅/JS 크래시/프로세스 종료). 실기 미수행 항목은 미검증으로 명시.
- [x] 커밋: `docs: 진단 내보내기 경계 문장과 상태 표 갱신`

## 수동 검증 절차(adb 없음)

1. `bun run build android-internal` → MTP로 Galaxy XR에 사이드로드.
2. 평소처럼 세션을 사용해 기기가 꺼질 때까지 실행한다. 꺼진 뒤 기기를 다시 켜고 앱을 연다.
3. 설정 화면 맨 아래 **진단 기록** → 요약 확인(이번 실행 기록) → **진단 내보내기** → 저장된 파일명 확인.
4. MTP로 Downloads의 `leftcar-diagnostics-*.ndjson`을 Mac에 복사해 판독한다.

판독 가이드(레코드는 `ts`(epoch)·`up_ms`(부팅 후 경과)를 함께 가진다):

| 파일 패턴 | 판정 |
| --- | --- |
| `battery_percent` 단조 감소 → ≤5%에서 종료, `charging:false` | 배터리 방전 |
| percent 15-20% 이상 유지, 마지막 샘플들에서 `voltage_mv` 붕괴 | 부하 전압 강하(전력 W 추정 금지) |
| 정상 값에서 급단종 + 다음 `session_start`가 `boot_detected:true`·`up_ms` 극소, crash 레코드 없음 | 기기 패닉 재부팅(시스템/드라이버) |
| 마지막 session 기록 후에도 sample 계속 + (Java 기원이면) `kind:"crash"` | RN JS 크래시 — 프로세스 생존 |
| 시계열 끊김 + 다음 `boot_detected:false`·큰 `up_ms` | 재부팅 없는 프로세스 사망(네이티브 크래시 한계 지점) |
| 15초 초과 공백 | 샘플러 정지 — 판단 유보 |

실기기 수집은 아직 수행되지 않았으며, 수행 전까지 이 기능의 기기 수용은 미검증이다.
