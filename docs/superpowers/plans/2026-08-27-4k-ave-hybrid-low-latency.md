# 4K AVE Hybrid Low-Latency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 4K `video` 세션을 명시적인 AVE H.264 하드웨어 경로로 보내고, 실제 인코더·속성·단계별 성능을 끝까지 계측하여 3840×2160의 Host 출력과 Android 렌더가 180초 평균 59fps 이상이면서 decoder age p95 50ms 이하인지 실기기에서 판정한다.

**Architecture:** 기존 H.264 wire, UDP/FEC/recovery, Android MediaCodec 경로는 유지한다. Swift의 순수 정책이 4K `video`에는 AVE, `interactive`와 sub-4K에는 RTVC를 선택하고, 실제 VideoToolbox capability를 확인해 지원되는 속성만 적용한다. 선택 결과는 Swift stats JSON → Rust 계약/FFI/control snapshot → React 상세 화면으로 전달한다. Host와 Android 누적 카운터 로그는 하나의 분석기로 평균 FPS, 1초 rolling p5, latency percentile, drop delta를 계산한다.

**Tech Stack:** Swift 6 + ScreenCaptureKit + VideoToolbox, Rust + Serde + Tauri, React 19 + TypeScript + Vitest + Tailwind CSS, Android Rust JNI + MediaCodec, shell/TypeScript performance tooling.

**Spec:** `docs/superpowers/specs/2026-08-27-4k-ave-hybrid-low-latency-design.md`

## Global Constraints

- 현재 dirty worktree의 사용자 변경을 보존한다. 구현 시작 전에 `git status --short`를 저장하고 이 계획에 적힌 파일만 의도적으로 수정한다.
- wire packet, Android decoder/reassembly, pairing, input, USB/AOAP, 자동 transport 전환은 변경하지 않는다.
- software encoder fallback, mid-session encoder hot swap, 숨은 환경 변수, 자동 1440p downgrade를 추가하지 않는다.
- 4K encode in-flight 기본값은 첫 후보 측정까지 3으로 유지한다. `MaxFrameDelayCount`는 설정하지 않는다.
- React/TSX 변경 후 저장소 루트에서 `npx -y react-doctor@latest . --verbose`를 실행하고 반드시 `100 / 100`을 확인한다.
- 실기기 고변화 화면 이동은 스트림이 실제로 움직이는 화면임을 확인한 뒤 `Option+0`으로만 수행한다. `Option+Shift+0`은 사용하지 않는다.
- 55fps는 후보 게이트다. 최종 완료 주장은 180초 평균 59fps, 1초 p5 55fps, age/drop/recovery 조건을 모두 통과한 fresh physical evidence가 있을 때만 한다.
- 각 Commit 단계는 사용자가 실행 시점에 명시적으로 커밋을 허용한 경우에만 수행한다. 허용되지 않으면 변경을 그대로 두고 다음 검증 단계로 진행한다.

## File Responsibility Map

| File | Responsibility |
| --- | --- |
| `native/macos-capture-shim/Sources/CaptureShim.swift` | AVE/RTVC 정책, encoder enumeration, capability-aware property application, fallback, structured stats/perf log |
| `native/macos-capture-shim/Tests/EncodePolicyTests.swift` | 순수 encoder 정책·후보 정렬·property plan 회귀 테스트 |
| `crates/control-contract/src/host.rs` | encoder diagnostics의 canonical Rust/JSON schema와 camelCase contract |
| `apps/host-desktop/src-tauri/src/ffi.rs` | Swift stats JSON 파싱 |
| `apps/host-desktop/src-tauri/src/backend.rs` | fake backend의 complete `StatsInfo` fixture |
| `apps/host-desktop/src-tauri/src/control.rs` | fallback fixture 및 `StatsInfo` → `SessionView` 전달 |
| `apps/host-desktop/src-tauri/src/windows_backend/mod.rs` | Windows 기본 diagnostics 값으로 cross-platform compile 유지 |
| `apps/host-desktop/src-tauri/tests/control_e2e.rs` | 실제 `getStatus` JSON에서 encoder diagnostics 왕복 검증 |
| `apps/host-desktop/src/sessionTypes.ts` | React가 소비하는 encoder diagnostics 타입 |
| `apps/host-desktop/src/encoderDiagnostics.ts` | mode/property 결과의 순수 표시 모델 |
| `apps/host-desktop/src/encoderDiagnostics.test.ts` | Host UI 표시 모델의 Vitest 회귀 테스트 |
| `apps/host-desktop/src/SessionInspector.tsx` | 실제 encoder ID/hardware/preset/profile/property/fallback 표시 |
| `tools/perf-matrix/analyze-performance.ts` | Host/Android 누적 카운터와 timestamp 기반 acceptance summary |
| `tools/perf-matrix/analyze-performance.test.ts` | 평균 FPS, rolling p5, percentile, drop delta, stall 판정 테스트 |
| `tools/perf-matrix/collect-1440-4k.sh` | `log stream`과 `adb logcat -v epoch -s LeftcarNative` 동시 수집 |
| `tools/perf-matrix/README.md` | quick/final/soak 명령과 판정 산출물 설명 |
| `docs/11-low-latency-investigation.md` | fresh baseline, AVE 결과, 남은 병목과 proof boundary |

---

### Task 1: Define the pure AVE/RTVC session policy

**Files:**

- Modify: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`

**Interfaces:**

- Consumes: `width`, `height`, `contentMode`, enumerated encoder descriptors, supported property key set.
- Produces: ordered `[EncoderSessionPolicy]`, one preferred hardware encoder ID, and an ordered optional-property plan.
- Does not consume live VideoToolbox sessions; every function in this task stays deterministic and unit-testable.

- [ ] **Step 1: Add failing policy assertions**

Append these assertions near the existing codec/preset assertions in `EncodePolicyTests.main()`:

```swift
let video4K = encoderSessionPolicies(width: 3_840, height: 2_160, contentMode: "video")
precondition(video4K == [
    EncoderSessionPolicy(mode: .ave, codec: .h264),
    EncoderSessionPolicy(mode: .rtvc, codec: .h264),
])
precondition(
    encoderSessionPolicies(width: 3_840, height: 2_160, contentMode: "interactive")
        == [EncoderSessionPolicy(mode: .rtvc, codec: .h264)]
)
precondition(
    encoderSessionPolicies(width: 2_560, height: 1_440, contentMode: "video")
        == [EncoderSessionPolicy(mode: .rtvc, codec: .h264)]
)

let preferredAVE = preferredHardwareEncoderID(
    codec: .h264,
    candidates: [
        EncoderCandidateDescriptor(id: "software", codec: .h264, hardware: false, performanceRating: 900),
        EncoderCandidateDescriptor(id: "ave-slow", codec: .h264, hardware: true, performanceRating: 200),
        EncoderCandidateDescriptor(id: "ave-fast", codec: .h264, hardware: true, performanceRating: 400),
        EncoderCandidateDescriptor(id: "hevc-fast", codec: .hevc, hardware: true, performanceRating: 500),
    ]
)
precondition(preferredAVE == "ave-fast")

precondition(
    encoderSpecificationPlan(mode: .ave, encoderID: "ave-fast") == [
        .requireHardware,
        .encoderID("ave-fast"),
    ]
)
precondition(
    encoderSpecificationPlan(mode: .rtvc, encoderID: nil) == [
        .requireHardware,
        .enableLowLatencyRateControl,
    ]
)

let supportedAVE: Set<EncoderOptionalProperty> = [
    .prioritizeSpeed,
    .maximumRealTimeFrameRate,
    .quality,
]
precondition(
    optionalPropertyPlan(mode: .ave, supported: supportedAVE) == [
        .prioritizeSpeed,
        .maximumRealTimeFrameRate,
        .quality,
    ]
)
precondition(optionalPropertyPlan(mode: .rtvc, supported: supportedAVE).isEmpty)
precondition(!EncoderOptionalProperty.allCases.map(\.rawValue).contains("MaxFrameDelayCount"))
```

- [ ] **Step 2: Run the policy test and observe RED**

```bash
swiftc -O -o /tmp/leftcar-encode-policy-tests native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift -framework ScreenCaptureKit -framework VideoToolbox -framework CoreMedia -framework CoreVideo -framework CoreGraphics -framework IOSurface -framework Security -framework Foundation
```

Expected: compile fails because `EncoderSessionPolicy`, `EncoderCandidateDescriptor`, `EncoderSpecificationEntry`, and `EncoderOptionalProperty` do not exist.

- [ ] **Step 3: Implement the minimal pure policy**

Add these types and functions beside the existing encoder policy types in `CaptureShim.swift`:

```swift
enum EncoderMode: String, Equatable {
    case ave
    case rtvc
}

struct EncoderSessionPolicy: Equatable {
    let mode: EncoderMode
    let codec: VideoCodecKind
}

struct EncoderCandidateDescriptor: Equatable {
    let id: String
    let codec: VideoCodecKind
    let hardware: Bool
    let performanceRating: Int
}

enum EncoderSpecificationEntry: Equatable {
    case requireHardware
    case encoderID(String)
    case enableLowLatencyRateControl
}

enum EncoderOptionalProperty: String, CaseIterable {
    case prioritizeSpeed = "PrioritizeEncodingSpeedOverQuality"
    case maximumRealTimeFrameRate = "MaximumRealTimeFrameRate"
    case suggestedLookAheadFrameCount = "SuggestedLookAheadFrameCount"
    case maximizePowerEfficiency = "MaximizePowerEfficiency"
    case quality = "Quality"
}

func encoderSessionPolicies(
    width: UInt32,
    height: UInt32,
    contentMode: String
) -> [EncoderSessionPolicy] {
    let isUltraHD = max(width, height) >= 3_840 && min(width, height) >= 2_160
    if isUltraHD && contentMode.lowercased() == StreamContentMode.video.rawValue {
        return [
            EncoderSessionPolicy(mode: .ave, codec: .h264),
            EncoderSessionPolicy(mode: .rtvc, codec: .h264),
        ]
    }
    return [EncoderSessionPolicy(mode: .rtvc, codec: .h264)]
}

func preferredHardwareEncoderID(
    codec: VideoCodecKind,
    candidates: [EncoderCandidateDescriptor]
) -> String? {
    candidates
        .filter { $0.codec == codec && $0.hardware }
        .sorted {
            if $0.performanceRating == $1.performanceRating { return $0.id < $1.id }
            return $0.performanceRating > $1.performanceRating
        }
        .first?.id
}

func encoderSpecificationPlan(
    mode: EncoderMode,
    encoderID: String?
) -> [EncoderSpecificationEntry] {
    switch mode {
    case .ave:
        guard let encoderID else { return [.requireHardware] }
        return [.requireHardware, .encoderID(encoderID)]
    case .rtvc:
        return [.requireHardware, .enableLowLatencyRateControl]
    }
}

func optionalPropertyPlan(
    mode: EncoderMode,
    supported: Set<EncoderOptionalProperty>
) -> [EncoderOptionalProperty] {
    guard mode == .ave else { return [] }
    return EncoderOptionalProperty.allCases.filter(supported.contains)
}
```

Replace uses of the old `encoderCandidateOrder`/`preferredVideoCodec` path in new code only after the assertions are green; keep their existing tests compiling until Task 2 removes or delegates them.

- [ ] **Step 4: Run the policy test GREEN**

```bash
swiftc -O -o /tmp/leftcar-encode-policy-tests native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift -framework ScreenCaptureKit -framework VideoToolbox -framework CoreMedia -framework CoreVideo -framework CoreGraphics -framework IOSurface -framework Security -framework Foundation
/tmp/leftcar-encode-policy-tests
```

Expected: both commands exit 0.

- [ ] **Step 5: Commit checkpoint, only with explicit authorization**

```bash
git add native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift
git commit -m "feat(stream): AVE RTVC 인코더 정책 분리"
```

---

### Task 2: Create and configure the actual VideoToolbox session

**Files:**

- Modify: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`

**Interfaces:**

- Consumes: `VTCopyVideoEncoderList`, the ordered policies from Task 1, supported property/preset dictionaries.
- Produces: one verified hardware `VTCompressionSession` plus `EncoderConfigurationReport` in session stats.
- Failure contract: AVE create/mandatory/prepare/identity failure records a reason and tries RTVC H.264 once; no software or Baseline/CAVLC fallback.

- [ ] **Step 1: Add failing report and mandatory-profile assertions**

Add to `EncodePolicyTests.main()`:

```swift
var report = EncoderConfigurationReport(mode: .ave)
report.recordApplied("HighSpeed")
report.recordUnsupported("SuggestedLookAheadFrameCount")
report.recordRejected("Quality", status: -12_900)
precondition(report.applied == ["HighSpeed"])
precondition(report.unsupported == ["SuggestedLookAheadFrameCount"])
precondition(report.rejected == ["Quality=-12900"])
precondition(requiredH264Profile(mode: .ave) == .main)
precondition(requiredH264EntropyMode(mode: .ave) == .cabac)
precondition(initialEncoderQuality(mode: .ave, width: 3_840, height: 2_160) == 0.25)
precondition(initialEncoderQuality(mode: .rtvc, width: 3_840, height: 2_160) == nil)
```

- [ ] **Step 2: Run RED**

Run the Task 1 Swift compile command. Expected: missing report and mode-specific profile/quality helpers.

- [ ] **Step 3: Implement report types and VideoToolbox list conversion**

Add the pure report first:

```swift
struct EncoderConfigurationReport {
    let mode: EncoderMode
    private(set) var applied: [String] = []
    private(set) var unsupported: [String] = []
    private(set) var rejected: [String] = []

    mutating func recordApplied(_ key: String) { applied.append(key) }
    mutating func recordUnsupported(_ key: String) { unsupported.append(key) }
    mutating func recordRejected(_ key: String, status: OSStatus) {
        rejected.append("\(key)=\(status)")
    }
}

func requiredH264Profile(mode: EncoderMode) -> H264ProfileKind {
    _ = mode
    return .main
}
func requiredH264EntropyMode(mode: EncoderMode) -> H264EntropyModeKind {
    _ = mode
    return .cabac
}

func initialEncoderQuality(
    mode: EncoderMode,
    width: UInt32,
    height: UInt32
) -> Float? {
    let isUltraHD = max(width, height) >= 3_840 && min(width, height) >= 2_160
    return mode == .ave && isUltraHD ? 0.25 : nil
}
```

Add a runtime adapter that calls `VTCopyVideoEncoderList`, reads `kVTVideoEncoderList_CodecType`, `EncoderID`, `IsHardwareAccelerated`, and `PerformanceRating`, and converts only H.264/HEVC rows into `[EncoderCandidateDescriptor]`. The selection itself must continue to use `preferredHardwareEncoderID` so list ordering cannot choose software or a lower-rated encoder.

```swift
private func availableEncoderDescriptors() -> [EncoderCandidateDescriptor] {
    var rawList: CFArray?
    guard VTCopyVideoEncoderList(nil, &rawList) == noErr,
          let rows = rawList as? [[String: Any]] else {
        return []
    }
    return rows.compactMap { row in
        guard let id = row[kVTVideoEncoderList_EncoderID as String] as? String,
              let codecNumber = row[kVTVideoEncoderList_CodecType as String] as? NSNumber else {
            return nil
        }
        let codecType = CMVideoCodecType(codecNumber.uint32Value)
        let codec: VideoCodecKind
        switch codecType {
        case kCMVideoCodecType_H264: codec = .h264
        case kCMVideoCodecType_HEVC: codec = .hevc
        default: return nil
        }
        return EncoderCandidateDescriptor(
            id: id,
            codec: codec,
            hardware: (row[kVTVideoEncoderList_IsHardwareAccelerated as String] as? NSNumber)?.boolValue ?? false,
            performanceRating: (row[kVTVideoEncoderList_PerformanceRating as String] as? NSNumber)?.intValue ?? 0
        )
    }
}
```

- [ ] **Step 4: Replace the session-creation loop with ordered session policies**

In `setupEncoder(for:)`:

1. Build `policies = encoderSessionPolicies(width: UInt32(w), height: UInt32(h), contentMode: contentMode.rawValue)`.
2. For each AVE policy, resolve `preferredHardwareEncoderID`; if absent, set `lastFailure = "AVE H.264 hardware encoder unavailable"` and continue.
3. Build the `encoderSpecification` from `encoderSpecificationPlan`:

```swift
var specification: [String: Any] = [
    kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder as String: true,
]
switch policy.mode {
case .ave:
    specification[kVTVideoEncoderSpecification_EncoderID as String] = encoderID
case .rtvc:
    if #available(macOS 11.3, *) {
        specification[
            kVTVideoEncoderSpecification_EnableLowLatencyRateControl as String
        ] = true
    }
}
```

Use H.264 for both initial policies. Do not leave HEVC in the automatic fallback list.

- [ ] **Step 5: Apply only supported mode-specific settings**

After session creation, copy `kVTCompressionPropertyKey_SupportedPropertyDictionary` and `kVTCompressionPropertyKey_SupportedPresetDictionaries` once. Apply in this order:

1. AVE `HighSpeed` or RTVC `VideoConferencing` preset when advertised.
2. Mandatory `RealTime=true`, `AllowFrameReordering=false`, H.264 Main, CABAC, `ExpectedFrameRate=fps`.
3. AVE optional settings from `optionalPropertyPlan`: speed priority, maximum real-time frame rate, look-ahead 0, power efficiency false, quality 0.25.
4. Existing calculated `AverageBitRate`, `DataRateLimits`, and keyframe interval after the preset.

For every optional key, record `applied`, `unsupported`, or `rejected(status)`. Remove the `kVTCompressionPropertyKey_MaxFrameDelayCount` setter entirely. RTVC must not call AVE-only setters.

Keep the adaptive bitrate controller active on RTVC by initializing its control value to 0.50, but do not apply `kVTCompressionPropertyKey_Quality` there. On AVE initialize both the control value and the generic quality property to 0.25. `encoderAppliedProperties` is therefore the source of truth for whether generic Quality actually reached VideoToolbox; `qualityHint` remains the ABR/slider control value.

Mandatory setting or `VTCompressionSessionPrepareToEncodeFrames` failure invalidates the session. For AVE, do not execute the current H.264 Main → Baseline profile fallback. After prepare, copy `EncoderID` and `UsingHardwareAcceleratedVideoEncoder`; reject AVE if the ID differs from the requested ID or hardware is not `true`.

- [ ] **Step 6: Persist diagnostics and emit one structured Host sample per rate window**

Add session state fields initialized before setup:

```swift
private var encoderMode = "not_ready"
private var encoderAppliedProperties: [String] = []
private var encoderUnsupportedProperties: [String] = []
private var encoderRejectedProperties: [String] = []
private var encoderFallbackReason: String?
```

On successful setup, copy the report into these fields and preserve the AVE failure reason when RTVC succeeds. Add these JSON keys beside the current encoder fields in `statsJSON()`:

```swift
"encoderMode": encoderMode,
"encoderAppliedProperties": encoderAppliedProperties,
"encoderUnsupportedProperties": encoderUnsupportedProperties,
"encoderRejectedProperties": encoderRejectedProperties,
"encoderFallbackReason": encoderFallbackReason ?? NSNull(),
```

Add `private var lastPerfLogNs: UInt64 = 0`. In `statsJSON()`, after releasing `stateLock` and taking the existing `queueSnapshot`, reacquire only `stateLock` long enough to update this timestamp:

```swift
stateLock.lock()
let shouldLogPerf = nowNs >= lastPerfLogNs && nowNs - lastPerfLogNs >= 1_000_000_000
if shouldLogPerf { lastPerfLogNs = nowNs }
stateLock.unlock()
```

Replace the fixed p95 helper with `percentile(_ samples: [UInt64], quantile: Double)` using nearest rank, then keep `percentile95` as a delegating wrapper and compute `encodeOutputIntervalP50Us` with quantile 0.50. When `shouldLogPerf` is true, emit exactly one parseable line using the already copied scalar snapshots:

```swift
NSLog(
    "LeftcarPerf captureCallbacks=%llu encodeOutputCallbacks=%llu captureFps=%u encodeOutputFps=%u encodeOutputIntervalP50Us=%llu encodeOutputIntervalP95Us=%llu encodeOutputP95Us=%llu queueOldestUs=%llu encoderMode=%@ encoderID=%@",
    captureCallbacks,
    encodeOutputCallbacks,
    lastCaptureFps,
    lastEncodeOutputFps,
    percentile(encodeOutputIntervalSamplesUs, quantile: 0.50),
    percentile95(encodeOutputIntervalSamplesUs),
    percentile95(encodeOutputSamplesUs),
    queueSnapshot.oldestAgeUs,
    encoderMode,
    encoderID
)
```

The log path must not hold `stateLock` and `networkLock` at the same time.

- [ ] **Step 7: Run Swift tests and dylib compile**

```bash
swiftc -O -o /tmp/leftcar-encode-policy-tests native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift -framework ScreenCaptureKit -framework VideoToolbox -framework CoreMedia -framework CoreVideo -framework CoreGraphics -framework IOSurface -framework Security -framework Foundation
/tmp/leftcar-encode-policy-tests
swiftc -O -emit-library native/macos-capture-shim/Sources/CaptureShim.swift -o native/macos-capture-shim/libleftcar_capture.dylib -framework ScreenCaptureKit -framework VideoToolbox -framework CoreMedia -framework CoreVideo -framework CoreGraphics -framework IOSurface -framework Security -framework Foundation
```

Expected: all commands exit 0; no 4K `video` code path inserts `EnableLowLatencyRateControl`; no code sets `MaxFrameDelayCount`.

- [ ] **Step 8: Commit checkpoint, only with explicit authorization**

```bash
git add native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift
git commit -m "feat(stream): AVE 하드웨어 인코더 명시 선택"
```

---

### Task 3: Carry encoder diagnostics through Rust and control JSON

**Files:**

- Modify: `crates/control-contract/src/host.rs`
- Modify: `apps/host-desktop/src-tauri/src/ffi.rs`
- Modify: `apps/host-desktop/src-tauri/src/backend.rs`
- Modify: `apps/host-desktop/src-tauri/src/control.rs`
- Modify: `apps/host-desktop/src-tauri/src/windows_backend/mod.rs`
- Modify: `apps/host-desktop/src-tauri/tests/control_e2e.rs`

**Interfaces:**

- Consumes: Swift JSON keys `encoderMode`, `encoderID`, `encoderHardwareAccelerated`, `encoderPreset`, `encoderProfile`, three property arrays, optional fallback reason.
- Produces: identical camelCase fields in `StatsInfo`, `SessionView`, Tauri `get_status`, and authenticated `getStatus` JSON.

- [ ] **Step 1: Write failing Rust contract assertions**

Extend `stats_info_serializes_keys` and `status_view_serializes` fixtures in `crates/control-contract/src/host.rs` with:

```rust
encoder_mode: "ave".into(),
encoder_id: "com.apple.videotoolbox.videoencoder.ave.avc".into(),
encoder_hardware_accelerated: Some(true),
encoder_preset: "high-speed".into(),
encoder_profile: "main".into(),
encoder_applied_properties: vec!["HighSpeed".into(), "Quality".into()],
encoder_unsupported_properties: vec!["SuggestedLookAheadFrameCount".into()],
encoder_rejected_properties: vec![],
encoder_fallback_reason: None,
```

Assert the serialized JSON contains `"encoderMode":"ave"`, `"encoderHardwareAccelerated":true`, and `"encoderAppliedProperties":["HighSpeed","Quality"]`.

Run:

```bash
cargo test -p control-contract stats_info_serializes_keys
```

Expected: compile fails because the fields do not exist.

- [ ] **Step 2: Add canonical fields to both Rust structs**

Add the same fields to `StatsInfo` and `SessionView` immediately after `current_bitrate`:

```rust
#[serde(default)]
pub encoder_mode: String,
#[serde(default)]
pub encoder_id: String,
#[serde(default)]
pub encoder_hardware_accelerated: Option<bool>,
#[serde(default)]
pub encoder_preset: String,
#[serde(default)]
pub encoder_profile: String,
#[serde(default)]
pub encoder_applied_properties: Vec<String>,
#[serde(default)]
pub encoder_unsupported_properties: Vec<String>,
#[serde(default)]
pub encoder_rejected_properties: Vec<String>,
#[serde(default)]
pub encoder_fallback_reason: Option<String>,
```

Use `"unknown"`, `None`, and empty vectors in fake/Windows/fallback fixtures. Update every `StatsInfo {` and `SessionView {` occurrence listed by:

```bash
rg -n "StatsInfo \{|SessionView \{" crates/control-contract apps/host-desktop/src-tauri --glob '*.rs'
```

- [ ] **Step 3: Extract and test the FFI parser**

Move the inline `serde_json::Value` → `StatsInfo` construction in `ffi.rs` into:

```rust
fn parse_stats_json(json: &str) -> Result<StatsInfo, String>
```

Add a unit test using this payload:

```rust
let stats = parse_stats_json(r#"{
  "frames":1,
  "bytes":2,
  "state":"running",
  "encoderMode":"ave",
  "encoderID":"com.apple.videotoolbox.videoencoder.ave.avc",
  "encoderHardwareAccelerated":true,
  "encoderPreset":"high-speed",
  "encoderProfile":"main",
  "encoderAppliedProperties":["HighSpeed","Quality"],
  "encoderUnsupportedProperties":["SuggestedLookAheadFrameCount"],
  "encoderRejectedProperties":["Quality=-12900"],
  "encoderFallbackReason":null
}"#).unwrap();
assert_eq!(stats.encoder_mode, "ave");
assert_eq!(stats.encoder_hardware_accelerated, Some(true));
assert_eq!(stats.encoder_applied_properties, ["HighSpeed", "Quality"]);
assert_eq!(stats.encoder_rejected_properties, ["Quality=-12900"]);
```

Implement array parsing with `as_array()`, `filter_map(Value::as_str)`, and owned strings. Missing keys must preserve old-shim compatibility through defaults.

Use one helper for all three arrays:

```rust
fn string_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect()
}
```

The new parser assignments are:

```rust
encoder_mode: v["encoderMode"].as_str().unwrap_or("unknown").into(),
encoder_id: v["encoderID"].as_str().unwrap_or("unknown").into(),
encoder_hardware_accelerated: v["encoderHardwareAccelerated"].as_bool(),
encoder_preset: v["encoderPreset"].as_str().unwrap_or("unknown").into(),
encoder_profile: v["encoderProfile"].as_str().unwrap_or("unknown").into(),
encoder_applied_properties: string_array(&v, "encoderAppliedProperties"),
encoder_unsupported_properties: string_array(&v, "encoderUnsupportedProperties"),
encoder_rejected_properties: string_array(&v, "encoderRejectedProperties"),
encoder_fallback_reason: v["encoderFallbackReason"].as_str().map(str::to_owned),
```

- [ ] **Step 4: Prove the control snapshot roundtrip**

Set AVE values in `RecordingBackend::stats` in `control_e2e.rs`. In `test_full_stream_lifecycle`, assert `getStatus` contains the encoder mode, exact ID, hardware flag, and applied property array. Map every field in `ControlServer::snapshot`; do not reconstruct or rename property values.

Run:

```bash
cargo test -p control-contract
cargo test -p leftcar-host-desktop parse_stats_json
cargo test -p leftcar-host-desktop --test control_e2e test_full_stream_lifecycle
```

Expected: all pass.

- [ ] **Step 5: Format and compile all touched Rust targets**

```bash
cargo fmt --all -- --check
cargo check -p control-contract -p leftcar-host-desktop
```

- [ ] **Step 6: Commit checkpoint, only with explicit authorization**

```bash
git add crates/control-contract/src/host.rs apps/host-desktop/src-tauri/src/ffi.rs apps/host-desktop/src-tauri/src/backend.rs apps/host-desktop/src-tauri/src/control.rs apps/host-desktop/src-tauri/src/windows_backend/mod.rs apps/host-desktop/src-tauri/tests/control_e2e.rs
git commit -m "feat(host): 인코더 설정 계측 전달"
```

---

### Task 4: Show attributable encoder configuration in the Host inspector

**Files:**

- Modify: `apps/host-desktop/src/sessionTypes.ts`
- Create: `apps/host-desktop/src/encoderDiagnostics.ts`
- Create: `apps/host-desktop/src/encoderDiagnostics.test.ts`
- Modify: `apps/host-desktop/src/SessionInspector.tsx`

**Interfaces:**

- Consumes: the camelCase `SessionRow` encoder fields from Task 3.
- Produces: stable Korean labels for path/identity/property status and an explicit fallback warning.

- [ ] **Step 1: Add a failing formatter test**

Create `encoderDiagnostics.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import type { SessionRow } from "./sessionTypes";

const session = {
  encoderMode: "ave",
  encoderID: "com.apple.videotoolbox.videoencoder.ave.avc",
  encoderHardwareAccelerated: true,
  encoderPreset: "high-speed",
  encoderProfile: "main",
  encoderAppliedProperties: ["HighSpeed", "Quality"],
  encoderUnsupportedProperties: ["SuggestedLookAheadFrameCount"],
  encoderRejectedProperties: ["Quality=-12900"],
  encoderFallbackReason: null,
} as SessionRow;

describe("encoder diagnostics", () => {
  it("keeps the actual AVE identity and property outcomes visible", () => {
    expect(encoderDiagnosticsView(session)).toEqual({
      path: "AVE · 하드웨어",
      identity: "com.apple.videotoolbox.videoencoder.ave.avc",
      configuration: "high-speed · main",
      applied: "HighSpeed, Quality",
      unavailable: "미지원 SuggestedLookAheadFrameCount · 거부 Quality=-12900",
      fallback: null,
    });
  });
});
```

Run:

```bash
bun run test -- apps/host-desktop/src/encoderDiagnostics.test.ts
```

Expected: module-not-found failure.

- [ ] **Step 2: Add typed fields and the minimal formatter**

Add to `SessionRow`:

```ts
encoderMode?: "ave" | "rtvc" | "unknown" | string;
encoderID?: string;
encoderHardwareAccelerated?: boolean | null;
encoderPreset?: string;
encoderProfile?: string;
encoderAppliedProperties?: string[];
encoderUnsupportedProperties?: string[];
encoderRejectedProperties?: string[];
encoderFallbackReason?: string | null;
```

Implement `encoderDiagnosticsView(session)` as this pure function. It joins arrays without mutating them and preserves exact rejected status strings.

```ts
import type { SessionRow } from "./sessionTypes";

export interface EncoderDiagnosticsView {
  path: string;
  identity: string;
  configuration: string;
  applied: string;
  unavailable: string;
  fallback: string | null;
}

export function encoderDiagnosticsView(session: SessionRow): EncoderDiagnosticsView {
  const mode = session.encoderMode === "ave"
    ? "AVE"
    : session.encoderMode === "rtvc"
      ? "RTVC"
      : "미확인";
  const acceleration = session.encoderHardwareAccelerated === true
    ? "하드웨어"
    : session.encoderHardwareAccelerated === false
      ? "소프트웨어"
      : "가속 확인 중";
  const unavailable = [
    session.encoderUnsupportedProperties?.length
      ? `미지원 ${session.encoderUnsupportedProperties.join(", ")}`
      : null,
    session.encoderRejectedProperties?.length
      ? `거부 ${session.encoderRejectedProperties.join(", ")}`
      : null,
  ].filter((value): value is string => value !== null).join(" · ");

  return {
    path: `${mode} · ${acceleration}`,
    identity: session.encoderID || "인코더 ID 확인 중",
    configuration: `${session.encoderPreset || "preset 확인 중"} · ${session.encoderProfile || "profile 확인 중"}`,
    applied: session.encoderAppliedProperties?.join(", ") || "없음",
    unavailable: unavailable || "없음",
    fallback: session.encoderFallbackReason ?? null,
  };
}
```

- [ ] **Step 3: Render the diagnostics without hidden state**

Call the formatter once at the start of `SessionInspector` and add inspector rows for:

- `인코더 경로`: path and exact ID.
- `인코더 설정`: preset/profile and applied properties.
- `미지원/거부 속성`: unsupported/rejected summary, or `없음`.
- `인코더 fallback`: render only when the fallback reason is non-null.

Use existing `inspector-grid`, `inspector-item`, and typography classes; do not introduce inline color/style objects or a second diagnostics state.

- [ ] **Step 4: Run UI tests, React Doctor, and typechecks**

```bash
bun run test -- apps/host-desktop/src/encoderDiagnostics.test.ts apps/host-desktop/src/hostState.test.ts apps/host-desktop/src/streamTermination.test.ts
npx -y react-doctor@latest . --verbose
bun run typecheck
bun run --cwd apps/host-desktop build
```

Expected: tests/typecheck/build pass and React Doctor prints `100 / 100`. Fix every source finding before proceeding.

- [ ] **Step 5: Commit checkpoint, only with explicit authorization**

```bash
git add apps/host-desktop/src/sessionTypes.ts apps/host-desktop/src/encoderDiagnostics.ts apps/host-desktop/src/encoderDiagnostics.test.ts apps/host-desktop/src/SessionInspector.tsx
git commit -m "feat(host): 실제 인코더 경로 진단 표시"
```

---

### Task 5: Make physical performance evidence machine-checkable

**Files:**

- Create: `tools/perf-matrix/analyze-performance.ts`
- Create: `tools/perf-matrix/analyze-performance.test.ts`
- Modify: `tools/perf-matrix/collect-1440-4k.sh`
- Modify: `tools/perf-matrix/README.md`

**Interfaces:**

- Consumes: macOS `LeftcarPerf` cumulative counters and Android `Rendered N frames` diagnostic logs with epoch timestamps.
- Produces: JSON containing Host/Android average FPS, rolling-window p5, max sample gap, latency p50/p95, drop deltas, and threshold booleans.

- [ ] **Step 1: Write failing counter-analysis tests**

Create this exact fixture. Host counters advance by 30 each half-second; Android parser fixtures use ages 25/30/40/50ms.

```ts
const samples = [
  { timestampMs: 0, frames: 0 },
  { timestampMs: 500, frames: 30 },
  { timestampMs: 1_000, frames: 60 },
  { timestampMs: 1_500, frames: 90 },
];
expect(summarizeCounterSeries(samples, 1_500)).toMatchObject({
  averageFps: 60,
  rollingOneSecondP5Fps: 60,
  zeroFpsStallDetected: false,
});
expect(percentile([25, 30, 40, 50], 0.5)).toBe(30);
expect(percentile([25, 30, 40, 50], 0.95)).toBe(50);
```

Add this stalled series and assert `zeroFpsStallDetected: true` and rolling p5 `0`:

```ts
const stalled = [
  { timestampMs: 0, frames: 0 },
  { timestampMs: 1_000, frames: 60 },
  { timestampMs: 3_000, frames: 60 },
];
```

Parse this Android line and assert timestamp, frames, drops, gaps, and age exactly:

```text
1777777777.500  1234  1234 I LeftcarNative: Rendered 120 frames; outputDrops=2 staleInputs=0 staleInputDrops=0 outputBurst=0 fecRecovered=0 decoderInputsQueued=120 decoderInputDrops=3 completedBatch=1 liveEdgeBatch=1 maxCompletedBatch=1 frameGaps=4 intentionalLiveEdgeGaps=0 recoverySkippedFrames=0 feedUs=100 maxFeedUs=200 captureAgeMs=Some(41) encodeAgeMs=Some(12) wireAgeMs=Some(7)
```

Run:

```bash
bun run test -- tools/perf-matrix/analyze-performance.test.ts
```

Expected: module-not-found failure.

- [ ] **Step 2: Implement pure parsers and summaries**

Export these exact functions:

```ts
export function parseHostLog(text: string): HostPerfSample[];
export function parseAndroidLog(text: string): AndroidPerfSample[];
export function percentile(values: number[], quantile: number): number;
export function summarizeCounterSeries(
  samples: Array<{ timestampMs: number; frames: number }>,
  expectedDurationMs: number,
): CounterSummary;
export function summarizePerformance(
  host: HostPerfSample[],
  android: AndroidPerfSample[],
  expectedDurationMs: number,
): PerformanceSummary;
```

`averageFps` uses `(last.frames - first.frames) / elapsed`; 1-second windows use the oldest sample within 800–1,200ms of each endpoint. `rollingOneSecondP5Fps` uses the nearest-rank percentile. A counter that does not advance for at least 1,000ms, a sample gap over 1,500ms, or a missing tail longer than 1,500ms is a zero-FPS stall. Drop/gap results are `last - first`, never the absolute lifetime counter.

`parseHostLog` parses each NDJSON object, uses `Date.parse(row.timestamp)` for the clock, and tokenizes `row.eventMessage` after `LeftcarPerf`. `parseAndroidLog` converts the leading epoch seconds to milliseconds. The performance summary also carries Host `encodeOutputIntervalP50Us`, `encodeOutputIntervalP95Us`, `encodeOutputP95Us`, and queue-age trend; Android `captureAgeMs` p50/p95; and output/input/gap deltas. Existing `UDP access-unit gap detected at id=N` and `Received IDR access unit id=N` events are paired when present. A zero gap delta automatically satisfies recovery; a nonzero delta requires the paired recovery distance to be at most two frame IDs or remains failed/unverified.

The CLI must accept:

```text
--host <host.ndjson> --android <android.log> --duration <seconds> --output <summary.json>
```

and write the full summary plus booleans for the 55fps candidate and final 4K criteria. It must return a parse error for fewer than two counter samples instead of manufacturing a passing zero.

- [ ] **Step 3: Collect both clocks and both cumulative counters**

Update `collect-1440-4k.sh`:

- Accept `latency|balanced|clarity|video` profile IDs.
- Keep the stream-start prerequisite.
- Capture Android with `adb logcat -v epoch -s LeftcarNative`.
- Capture Host with `/usr/bin/log stream --style ndjson --level info --predicate 'process == "leftcar-host-desktop" AND eventMessage CONTAINS "LeftcarPerf"'`.
- Stop and wait for both child processes in the trap.
- Run the analyzer with the requested duration and write `${report}.summary.json`.
- Record profile, expected resolution/mode, device model, duration, and exact artifact paths in `${report}.md`.

Do not clear macOS unified logs. `adb logcat -c` remains optional and scoped to the connected test device.

- [ ] **Step 4: Test the analyzer and shell syntax**

```bash
bun run test -- tools/perf-matrix/analyze-performance.test.ts
sh -n tools/perf-matrix/collect-1440-4k.sh
```

Expected: pass.

- [ ] **Step 5: Document quick, final, and soak invocations**

Add these exact examples to the README:

```bash
tools/perf-matrix/collect-1440-4k.sh --profile video --duration 30 --output /tmp/leftcar-4k-ave-quick
tools/perf-matrix/collect-1440-4k.sh --profile video --duration 180 --output /tmp/leftcar-4k-ave-final
tools/perf-matrix/collect-1440-4k.sh --profile video --duration 600 --output /tmp/leftcar-4k-ave-soak
```

State that warm-up happens before starting the collector: 10 seconds for quick runs and 60 seconds for final/soak runs.

- [ ] **Step 6: Commit checkpoint, only with explicit authorization**

```bash
git add tools/perf-matrix/analyze-performance.ts tools/perf-matrix/analyze-performance.test.ts tools/perf-matrix/collect-1440-4k.sh tools/perf-matrix/README.md
git commit -m "test(perf): 4K 실기기 판정 자동화"
```

---

### Task 6: Run static gates, build the signed Host, and establish the quick A/B

**Files:**

- Verify only; source changes are allowed only to fix a failing gate in already-touched files.

**Interfaces:**

- Consumes: Tasks 1–5 code and the existing installed Viewer APK.
- Produces: one same-binary RTVC 4K control, one AVE H.264 4K candidate, and one 1440p RTVC regression summary.

- [ ] **Step 1: Run all pre-device gates**

```bash
/tmp/leftcar-encode-policy-tests
cargo fmt --all -- --check
cargo test -p control-contract -p leftcar-host-desktop -p viewer-decoder -p android-viewer
bun run test -- apps/host-desktop/src/encoderDiagnostics.test.ts tools/perf-matrix/analyze-performance.test.ts
npx -y react-doctor@latest . --verbose
bun run typecheck
git diff --check
```

Expected: every command passes and React Doctor is `100 / 100`.

- [ ] **Step 2: Rebuild the dylib and signed Host in place**

```bash
swiftc -O -emit-library native/macos-capture-shim/Sources/CaptureShim.swift -o native/macos-capture-shim/libleftcar_capture.dylib -framework ScreenCaptureKit -framework VideoToolbox -framework CoreMedia -framework CoreVideo -framework CoreGraphics -framework IOSurface -framework Security -framework Foundation
bun run dev:host:macos
```

Expected: the stable `/Applications/Leftcar Host.app` identity is preserved, code signing verifies, and the Host launches. Do not rebuild/reinstall the APK unless an Android regression is observed, because no Android source or ABI changes are in this plan.

- [ ] **Step 3: Confirm physical prerequisites before collecting**

```bash
adb get-state
adb shell getprop ro.product.model
```

Expected: one authorized device and model `TB710FU`. In the Host inspector confirm the selected source is 3840×2160 and the transport is the intended Wi-Fi UDP path.

- [ ] **Step 4: Measure same-binary 4K RTVC control A**

On the Viewer select `선명한 화면` (`clarity`, 3840×2160, interactive). Confirm the Host shows `encoderMode=rtvc`, exact RTVC encoder ID, hardware true, and no AVE-only applied properties. Confirm the Android stream is rendering, use Computer Use to verify motion, press `Option+0`, wait 10 seconds, then run:

```bash
tools/perf-matrix/collect-1440-4k.sh --profile clarity --duration 30 --output /tmp/leftcar-4k-rtvc-quick
```

- [ ] **Step 5: Measure AVE H.264 candidate B**

Restart from the Viewer with `동영상 우선` (`video`, 3840×2160). Confirm the Host shows `encoderMode=ave`, exact AVE H.264 ID, hardware true, HighSpeed, and the property results. Verify moving content before `Option+0`, wait 10 seconds, then run:

```bash
tools/perf-matrix/collect-1440-4k.sh --profile video --duration 30 --output /tmp/leftcar-4k-ave-quick
```

Candidate gate: Host average encode output and Android average render must both be at least 55fps; capture/decoder age p95 must be at most 50ms; no monotonic queue-age growth; decoder input drops must be zero. If this gate passes, proceed to Task 7. If it fails, proceed to Task 8 without claiming 4K60. If H.264 later passes this quick gate but fails Task 9 final acceptance, execute Task 8 once with that final summary as the baseline.

- [ ] **Step 6: Measure 1440p RTVC regression C**

Select `균형` (`balanced`, 2560×1440, interactive), warm for 10 seconds, and run:

```bash
tools/perf-matrix/collect-1440-4k.sh --profile balanced --duration 30 --output /tmp/leftcar-1440-rtvc-quick
```

Require Host/Android average at least 55fps, age p95 at most 50ms, and decoder input drops zero.

- [ ] **Step 7: Record the A/B decision before tuning**

Copy the three summary JSON paths and their exact metrics into `docs/11-low-latency-investigation.md`. Separate capture, encode, packetization/pacing, network, reassembly, decoder, and Surface observations. Do not infer network or decoder causality when Host output is already below target.

---

### Task 7: Tune only an AVE H.264 candidate that clears 55fps

**Files:**

- Modify if evidence requires: `native/macos-capture-shim/Sources/CaptureShim.swift`
- Modify if policy value changes: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`
- Modify: `docs/11-low-latency-investigation.md`

**Interfaces:**

- Consumes: Task 6 AVE H.264 summary with both Host and Android at least 55fps.
- Produces: one evidence-selected quality/in-flight setting; no runtime codec switch.

- [ ] **Step 1: Compare quality 0.25 and 0.50 without changing transport**

Use the existing Host quality slider while the AVE stream is running. Collect separate 30-second high-change runs at 25% and 50%. Retain 0.50 only if Host/Android FPS gates remain satisfied and age p95 does not regress by more than 5ms; otherwise keep 0.25.

- [ ] **Step 2: Compare in-flight 3 against 2 only after throughput passes**

Add `precondition(encoderLatencyPolicy(width: 3_840, height: 2_160).maxEncodeInFlight == 2)` and observe RED, change only the 4K `maxEncodeInFlight` result to 2, rebuild/install the same signed Host, and rerun the 30-second AVE sample. Keep 2 only if average output/render do not decrease, rolling p5 does not decrease, and age p95 improves. Otherwise restore 3, restore the assertion to `== 3`, and rerun the Swift policy test.

- [ ] **Step 3: Re-run the quick AVE gate after the selected tuning**

Use a new output directory and preserve both candidate summaries. Do not overwrite `/tmp/leftcar-4k-ave-quick`.

- [ ] **Step 4: Commit checkpoint, only with explicit authorization**

Commit only the evidence-selected setting and its test:

```bash
git add native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift docs/11-low-latency-investigation.md
git commit -m "perf(stream): AVE 4K 지연 처리량 조정"
```

Proceed to Task 9.

---

### Task 8: Evaluate AVE HEVC only when H.264 misses a required gate

**Files:**

- Modify: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`
- Modify: `docs/11-low-latency-investigation.md`

**Interfaces:**

- Consumes: an AVE H.264 quick summary below the 55fps candidate gate, or a final summary that clears 55fps but misses the 59fps/latency acceptance.
- Produces: a separately built AVE HEVC measurement; final production policy remains whichever codec wins both throughput and latency criteria.

- [ ] **Step 1: Change the explicit 4K video primary codec under test to HEVC**

First change the pure assertion for 4K `video` primary policy from `.h264` to `.hevc` and observe RED. Then change only the AVE primary policy to HEVC; preserve RTVC H.264 as fallback. `preferredHardwareEncoderID(codec: .hevc, candidates: enumeratedEncoders)` must still require hardware and highest performance rating. Existing `CF2` HEVC wire/decoder path is reused.

- [ ] **Step 2: Build/install and measure D independently**

Run all Swift/Rust/React static gates, rebuild the dylib and signed Host, then collect a fresh `video` quick run into `/tmp/leftcar-4k-ave-hevc-quick`. Confirm the Host exact ID is AVE HEVC and Android reports a 3840×2160 hardware low-latency decoder.

- [ ] **Step 3: Select by evidence, not codec preference**

Keep HEVC only if it exceeds H.264 in both Host output and Android render without worsening age p95 or drops. Otherwise restore the H.264 policy and rerun the policy test/build. If neither codec reaches the 55fps candidate gate, stop single-session tuning and open a separate tiled/multi-session SPEC; do not implement tiling inside this plan.

- [ ] **Step 4: Commit checkpoint, only with explicit authorization**

Commit only the selected final codec policy, its assertions, and evidence:

```bash
git add native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift docs/11-low-latency-investigation.md
git commit -m "perf(stream): 4K AVE 코덱 경로 확정"
```

Proceed to Task 9 only if one candidate cleared 55fps; otherwise report the measured blocker.

---

### Task 9: Run final 4K60 acceptance and latency-creep soak

**Files:**

- Modify: `docs/11-low-latency-investigation.md`

**Interfaces:**

- Consumes: the selected AVE codec/settings and machine-generated summaries.
- Produces: final pass/fail receipt with explicit unverified photon-to-photon boundary.

- [ ] **Step 1: Re-run every static gate from clean process state**

Quit any stale development Host, rebuild the dylib and signed app, launch one installed Host, then run the full Task 6 Step 1 command set. Record command exit codes and React Doctor `100 / 100`.

- [ ] **Step 2: Collect the 180-second final run**

Start `동영상 우선`, verify 3840×2160 and the intended AVE encoder in Host/Android, confirm moving content, press `Option+0`, warm for 60 seconds, then run:

```bash
tools/perf-matrix/collect-1440-4k.sh --profile video --duration 180 --output /tmp/leftcar-4k-ave-final
```

Require all of:

- Host encode-output average ≥59fps.
- Android render average ≥59fps.
- Both one-second rolling-window p5 values ≥55fps and no zero-FPS stall.
- Host output interval median ≤16.7ms and p95 ≤18.5ms.
- Host encode-output latency p95 ≤18.5ms.
- Android capture timestamp → decoder age p95 ≤50ms.
- Queue oldest age does not remain above 16.67ms or rise monotonically.
- Android output-drop delta and decoder-input-drop delta are 0.
- 30-second high-change frame-gap delta ≤2 and recovery occurs within two output frames.
- Exact AVE hardware ID, preset, profile, applied/unsupported/rejected settings are present.

- [ ] **Step 3: Collect the 10-minute latency-creep soak**

Restart the session, warm for 60 seconds, then run:

```bash
tools/perf-matrix/collect-1440-4k.sh --profile video --duration 600 --output /tmp/leftcar-4k-ave-soak
```

Require no zero-FPS stall and no sustained upward trend in capture age or Host queue oldest age. A soak pass does not override a failed 180-second FPS gate.

- [ ] **Step 4: Re-run the 1440p RTVC regression after the final build**

Collect a fresh 30-second `balanced` run. Require ≥55fps Host/Android, age p95 ≤50ms, and decoder input drops 0.

- [ ] **Step 5: Write the final evidence boundary**

Update `docs/11-low-latency-investigation.md` with:

- exact app build/install time and device model;
- selected encoder ID/hardware/preset/profile/property report;
- quick A/B, final, soak, and 1440p summary paths;
- each numerical acceptance result and pass/fail;
- the remaining physical limit: photon-to-photon latency is unverified without high-speed-camera measurement.

If any final criterion fails for H.264 AVE, state `4K60 low-latency not yet achieved`, identify the first failing stage from measured evidence, and execute Task 8 once before considering tiled/multi-session work. If the selected final codec is already HEVC, do not loop back to Task 8.

- [ ] **Step 6: Final repository verification**

```bash
cargo fmt --all -- --check
cargo test -p control-contract -p leftcar-host-desktop -p viewer-decoder -p android-viewer
bun run test
npx -y react-doctor@latest . --verbose
bun run typecheck
bun run --cwd apps/host-desktop build
sh -n tools/perf-matrix/collect-1440-4k.sh
git diff --check
git status --short
```

Expected: all automated gates pass, React Doctor is `100 / 100`, and `git status` contains no accidental artifact/log files.

- [ ] **Step 7: Final commit checkpoint, only with explicit authorization**

```bash
git add docs/11-low-latency-investigation.md
git commit -m "docs(perf): 4K60 AVE 실기기 검증 기록"
```
