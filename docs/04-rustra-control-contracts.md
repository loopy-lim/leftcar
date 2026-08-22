# Rustra 제어 계약

문서 상태: 설계용 의사 코드  
목적: Rustra가 담당하는 범위를 고정하고 구현자가 contract-first TDD를 시작할 수 있게 한다.

## 1. 경계

Rustra는 다음 로컬 호출을 연결한다.

```text
Host Tauri TypeScript UI <-> Host Rust Core
Viewer TypeScript Hub UI <-> Viewer Rust Core    # RN shell 선택 시
```

Rustra는 다음을 연결하지 않는다.

```text
Host <-> Viewer network protocol
Encoder <-> network packetizer
Network receiver <-> MediaCodec
Compressed frame <-> TypeScript
```

## 2. package 분리

제안 package:

- `leftcar.host.control`
- `leftcar.viewer.control`
- 공통 타입 crate는 가능하지만 생성 package는 host/viewer 권한에 따라 분리한다.

Viewer가 Host 전용 command를 로컬로 호출할 수 있는 API surface를 생성하지 않는다.

## 3. 공통 ID

모든 ID는 로그에 원문을 남기지 않는 opaque newtype이다.

```rust
#[bridge_type]
pub struct HostId(pub String);

#[bridge_type]
pub struct DeviceId(pub String);

#[bridge_type]
pub struct SourceId(pub String);

#[bridge_type]
pub struct StreamInstanceId(pub String);

#[bridge_type]
pub struct SessionId(pub String);
```

요구:

- UUID 또는 128-bit random 기반
- 화면 제목, bundle ID, HWND를 ID에 인코딩하지 않음
- network source ID와 platform native handle을 직접 동일시하지 않음
- stale ID 재사용 금지

## 4. 핵심 타입

```rust
#[bridge_type]
pub enum HostPlatform {
    Macos,
    Windows,
    Linux,
}

#[bridge_type]
pub enum SourceKind {
    Display,
    Window,
}

#[bridge_type]
pub struct SourceDescriptor {
    pub id: SourceId,
    pub kind: SourceKind,
    pub display_name: String,
    pub application_name: Option<String>,
    pub width_px: u32,
    pub height_px: u32,
    pub is_approved: bool,
    pub is_available: bool,
    pub revision: u64,
}
```

`display_name`은 UI에 필요하지만 diagnostic log 기본값에서는 redaction한다.

```rust
#[bridge_type]
pub enum PairingState {
    Unpaired,
    Advertising,
    AwaitingHostApproval,
    PairedOffline,
    Connecting,
    Connected,
    Revoked,
}

#[bridge_type]
pub enum StreamPhase {
    Idle,
    Negotiating,
    WaitingKeyframe,
    Playing,
    Degraded,
    Reconnecting,
    Suspended,
    SourceUnavailable,
    PermissionRevoked,
    DecoderFailed,
    Stopped,
}

#[bridge_type]
pub enum QualityProfile {
    Focus,
    Normal,
    BackgroundVisible,
    Suspended,
    Custom(CustomQualityProfile),
}

#[bridge_type]
pub struct CustomQualityProfile {
    pub max_width: u32,
    pub max_height: u32,
    pub max_fps: u16,
    pub max_bitrate_bps: u64,
}
```

## 5. Host commands

### 5.1 permission과 source

```rust
#[command]
async fn get_capture_permission_state(
    input: GetCapturePermissionStateInput,
) -> Result<CapturePermissionState>;

#[command]
async fn request_source_selection(
    input: RequestSourceSelectionInput,
) -> Result<SourceDescriptor>;

#[command]
async fn list_approved_sources(
    input: ListApprovedSourcesInput,
) -> Result<SourceCatalogSnapshot>;

#[command]
async fn revoke_source(
    input: RevokeSourceInput,
) -> Result<MutationReceipt>;
```

규칙:

- source selection은 사용자 gesture에서 시작한다.
- TS가 native handle이나 picker 내부 ID를 제공하지 않는다.
- `list_approved_sources`는 현재 캡처 허용 source만 반환한다.
- source revoke는 해당 stream만 중단하고 event를 발생시킨다.

### 5.2 pairing

```rust
#[command]
async fn begin_pairing(input: BeginPairingInput) -> Result<PairingOfferView>;

#[command]
async fn cancel_pairing(input: CancelPairingInput) -> Result<MutationReceipt>;

#[command]
async fn approve_pairing(input: ApprovePairingInput) -> Result<PairedDeviceView>;

#[command]
async fn reject_pairing(input: RejectPairingInput) -> Result<MutationReceipt>;

#[command]
async fn list_paired_devices(
    input: ListPairedDevicesInput,
) -> Result<Vec<PairedDeviceView>>;

#[command]
async fn revoke_device(input: RevokeDeviceInput) -> Result<MutationReceipt>;
```

`PairingOfferView`는 QR 렌더링에 필요한 public/ephemeral 정보만 포함한다. private key와 raw long-term token을 반환하지 않는다.

### 5.3 stream과 진단

```rust
#[command]
async fn get_host_snapshot(input: GetHostSnapshotInput) -> Result<HostSnapshot>;

#[command]
async fn set_source_policy(input: SetSourcePolicyInput) -> Result<SourcePolicyView>;

#[command]
async fn stop_source(input: StopSourceInput) -> Result<MutationReceipt>;

#[command]
async fn stop_all_streams(input: StopAllStreamsInput) -> Result<MutationReceipt>;

#[command]
async fn export_redacted_diagnostics(
    input: ExportDiagnosticsInput,
) -> Result<ExportDiagnosticsOutput>;
```

## 6. Viewer commands

### 6.1 Host와 pairing

```rust
#[command]
async fn consume_pairing_offer(
    input: ConsumePairingOfferInput,
) -> Result<PairingRequestView>;

#[command]
async fn list_hosts(input: ListHostsInput) -> Result<Vec<HostView>>;

#[command]
async fn connect_host(input: ConnectHostInput) -> Result<SessionSnapshot>;

#[command]
async fn disconnect_host(input: DisconnectHostInput) -> Result<MutationReceipt>;
```

### 6.2 source와 stream window

```rust
#[command]
async fn list_remote_sources(
    input: ListRemoteSourcesInput,
) -> Result<SourceCatalogSnapshot>;

#[command]
async fn create_stream_launch(
    input: CreateStreamLaunchInput,
) -> Result<StreamLaunchView>;

#[command]
async fn list_stream_windows(
    input: ListStreamWindowsInput,
) -> Result<Vec<StreamWindowView>>;

#[command]
async fn request_close_stream_window(
    input: RequestCloseStreamWindowInput,
) -> Result<MutationReceipt>;

#[command]
async fn set_stream_profile(
    input: SetStreamProfileInput,
) -> Result<StreamSnapshot>;
```

`create_stream_launch`는 Activity를 직접 생성하지 않을 수 있다. TS UI는 결과의 opaque launch handle을 native launcher module에 넘긴다. handle은 짧게 만료되고 source 권한을 다시 검증한다.

### 6.3 capability와 metric

```rust
#[command]
async fn get_viewer_capabilities(
    input: GetViewerCapabilitiesInput,
) -> Result<ViewerCapabilities>;

#[command]
async fn get_stream_snapshot(
    input: GetStreamSnapshotInput,
) -> Result<StreamSnapshot>;

#[command]
async fn get_benchmark_readiness(
    input: GetBenchmarkReadinessInput,
) -> Result<BenchmarkReadiness>;
```

## 7. event 계약

저빈도 UI event만 Rustra sink로 보낸다.

```rust
#[bridge_type]
pub enum HostEvent {
    PermissionChanged(CapturePermissionState),
    SourceCatalogChanged { revision: u64 },
    PairingRequestCreated(PairingRequestView),
    PairingStateChanged(PairingStateView),
    StreamSummaryChanged(StreamSummaryView),
}

#[bridge_type]
pub enum ViewerEvent {
    HostStateChanged(HostStateView),
    SourceCatalogChanged { host_id: HostId, revision: u64 },
    StreamWindowChanged(StreamWindowView),
    SessionAlert(UserFacingAlert),
}
```

규칙:

- frame마다 event 금지
- metric은 최대 1Hz summary
- event는 snapshot 조회를 대체하는 유일한 진실이 아니다.
- UI가 event를 놓쳐도 `get_*_snapshot`으로 복구할 수 있다.
- event version과 payload shape를 contract test한다.

## 8. 오류 계약

```rust
#[bridge_type]
pub struct LeftcarErrorView {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub scope: ErrorScope,
    pub action: Option<RecoveryAction>,
    pub incident_id: Option<String>,
}

#[bridge_type]
pub enum ErrorScope {
    Command,
    Source,
    Stream,
    Session,
    Host,
    Viewer,
}

#[bridge_type]
pub enum RecoveryAction {
    Retry,
    RePair,
    ReSelectSource,
    GrantScreenRecording,
    CloseStreamWindow,
    UpdateHost,
    UpdateViewer,
    OpenDiagnostics,
}
```

초기 stable error code:

| code | retryable | action |
| --- | --- | --- |
| `pairing.offer_expired` | true | RePair |
| `pairing.host_rejected` | false | RePair |
| `auth.device_revoked` | false | RePair |
| `protocol.version_mismatch` | false | UpdateHost 또는 UpdateViewer |
| `capture.permission_required` | true | GrantScreenRecording |
| `capture.source_unavailable` | true | ReSelectSource |
| `capture.protected_content` | false | ReSelectSource |
| `encoder.hardware_unavailable` | 조건부 | OpenDiagnostics |
| `transport.disconnected` | true | Retry |
| `decoder.profile_unsupported` | 조건부 | Retry with fallback |
| `decoder.resource_exhausted` | true | CloseStreamWindow |
| `stream.launch_expired` | true | Retry |
| `stream.stale_catalog_revision` | true | Retry after refresh |

native error 문자열에 secret, 경로, 창 제목을 넣지 않는다.

## 9. 멱등성과 경쟁 조건

mutation command는 `request_id`를 받는다.

```rust
pub struct CommandMeta {
    pub request_id: String,
    pub expected_revision: Option<u64>,
}
```

규칙:

- 같은 `request_id` 재시도는 같은 결과를 반환하거나 안전한 already-applied 결과를 준다.
- source catalog mutation은 revision compare-and-set을 사용한다.
- close가 open보다 먼저 도착해도 orphan lease를 만들지 않는다.
- device revoke는 이후 모든 command보다 우선한다.
- `stop_all_streams`는 여러 번 호출해도 안전하다.

## 10. contract version

세 버전을 구분한다.

1. Rustra generated contract hash: 로컬 Rust/TS 일치
2. network protocol version: Host/Viewer 호환
3. media stream epoch: source pipeline 재구성

하나의 숫자로 합치지 않는다.

배포 규칙:

- generated output은 CI에서 clean regeneration을 확인한다.
- local contract hash mismatch면 UI command를 시작하지 않는다.
- network protocol은 최소/최대 range로 negotiate한다.
- wire field 제거는 최소 한 compatibility window 뒤에 한다.

## 11. TDD 시작 테스트

계약 구현 전에 다음 테스트 이름부터 만든다.

```text
host_contract_generates_stable_source_descriptor
viewer_contract_does_not_expose_high_rate_input_commands
video_payload_type_is_absent_from_generated_typescript
pairing_offer_view_never_contains_private_key
create_stream_launch_rejects_stale_catalog_revision
duplicate_request_id_is_idempotent
revoke_device_invalidates_existing_session_commands
event_loss_recovers_from_snapshot
all_stable_errors_have_user_recovery_mapping
generated_contract_hash_matches_runtime
```

## 12. 금지 구조 검사

CI에서 정적 검사 또는 architecture test로 다음을 막는다.

- generated TS에 `EncodedFrame`, `NalUnit`, `VideoPacket`, raw `bytes` command가 나타남
- Viewer Rustra contract에 `sendKey`, `sendMouse`, `injectInput`, `clipboard` command가 나타남. 키보드/포인터는 세션 토큰으로 인증한 네이티브 데이터그램 평면만 사용한다.
- pairing output에 `private`, `secret`, `token` 필드가 장기 값으로 나타남
- domain crate가 Tauri, React Native, Android, Apple, Windows SDK를 import함
- control event가 10Hz를 넘는 publish loop에 사용됨
