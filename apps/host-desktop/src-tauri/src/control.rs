//! Control server: viewers connect (pull) and exchange newline-delimited
//! JSON `{"command","args"}` / `{"ok":true,"result"}` over TCP (design §제어평면).
//!
//! `addNumbers` delegates to the real rustra host_package so the H02 proof
//! path stays intact; stateful v1 stream commands dispatch locally.

#[path = "session_lifecycle.rs"]
mod session_lifecycle;
#[cfg(test)]
#[path = "source_grants_tests.rs"]
mod source_grants_tests;
use crate::backend::SharedBackend;
use control_contract::host::{
    decode_media_key, CatalogView, EncoderExperiment, EncoderExperimentInfo,
    ReconfigureStreamInput, ReconfigureStreamOutput, SessionView, StartStreamInput,
    StartStreamOutput, StatusView,
};
use control_contract::udp_stability::{
    host_udp_stability_capabilities, resolve_udp_stability, AppliedUdpStability,
};
use session_lifecycle::{Lifecycle, Operation, StartAttempt, Starts};

use base64::Engine as _;
pub use control_contract::host::{StatsInfo, StatusView as StatusViewPublic};
use serde_json::json;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, AtomicU16, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::clipboard::ClipboardBackend;

/// Geometry and transport facts of the live session being reconfigured,
/// captured before the old backend handle is stopped.
#[derive(Clone)]
struct ReconfigureSnapshot {
    transport_attempt: Option<StartAttempt>,
    device_id: Option<String>,
    operation: Operation,
    authorization: Option<crate::pairing::Authorization>,
    handle: u32,
    source_index: u32,
    viewer_host: String,
    viewer_port: u16,
    capture_backend: String,
    media_transport: String,
    content_mode: String,
    encoder_experiment: EncoderExperiment,
    udp_stability: Option<AppliedUdpStability>,
    /// Media-path AEAD key of the live session. The replacement stream keeps
    /// the viewer's original key so the already-sealed media sockets continue
    /// to authenticate across a reconfigure.
    media_key: [u8; 32],
    width: u32,
    height: u32,
    fps_target: u32,
}

#[derive(Clone)]
struct Session {
    transport_owner: Option<StartAttempt>,
    lifecycle: Lifecycle,
    authorization: Option<crate::pairing::Authorization>,
    handle: u32,
    source_index: u32,
    source_name: String,
    /// 이 세션을 연 장치(토큰에서 귀속). revoke 시 즉시 차단에 쓴다.
    device_id: Option<String>,
    width: u32,
    height: u32,
    fps_target: u32,
    quality_state: String,
    capture_backend: String,
    content_mode: String,
    encoder_experiment: EncoderExperiment,
    viewer_addr: String,
    viewer_port: u16,
    media_transport: String,
    udp_stability: Option<AppliedUdpStability>,
    /// Viewer-generated media key sealing every datagram on this session's
    /// media path. Cloned from the startStream request over the encrypted
    /// control plane; never logged, never echoed back.
    media_key: [u8; 32],
    input_enabled: bool,
    input_rate_hz: u32,
    terminal_since: Option<Instant>,
    /// Control-plane tombstone retained long enough for the viewer's 2s
    /// status poll to observe why the host stopped this session.
    terminal_error: Option<String>,
    /// A forced stop removes the handle from the backend immediately. Keep
    /// that fact so terminal-state GC does not try to stop it a second time.
    backend_released: bool,
}

fn normalize_media_transport(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "udp" | "wifi" => Some("udp"),
        "tcp" | "wifitcp" | "wifi-tcp" => Some("tcp"),
        "adbtcp" | "adb-tcp" => Some("adbTcp"),
        "usb" | "aoap" => Some("usb"),
        "auto" | "both" => Some("auto"),
        _ => None,
    }
}

fn normalize_content_mode(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "interactive" | "latency" => Some("interactive"),
        "video" | "movie" => Some("video"),
        _ => None,
    }
}

fn validate_stream_shape(width: u32, height: u32, fps: u32) -> Result<(), String> {
    if width == 0 || height == 0 || width > 8192 || height > 8192 || fps == 0 || fps > 90 {
        return Err("unsupported stream dimensions or fps".into());
    }
    Ok(())
}

fn validate_split_request(
    encoder_experiment: EncoderExperiment,
    width: u32,
    height: u32,
    fps: u32,
    concrete_transport: &str,
    viewer_port: u16,
) -> Result<(), String> {
    if encoder_experiment != EncoderExperiment::SplitVertical {
        return Ok(());
    }
    if (width, height, fps) != (3840, 2160, 60)
        || concrete_transport != "udp"
        || viewer_port == u16::MAX
    {
        return Err(
            "splitVertical requires 3840x2160 at 60fps over direct UDP and two consecutive viewer ports"
                .into(),
        );
    }
    Ok(())
}

fn validate_split_start(input: &StartStreamInput, concrete_transport: &str) -> Result<(), String> {
    validate_split_request(
        input.encoder_experiment,
        input.width,
        input.height,
        input.fps,
        concrete_transport,
        input.viewer_port,
    )
}

/// Validated startStream parameters; transport and content mode are the
/// wire-canonical spellings.
struct StartPlan {
    source: control_contract::host::DisplayInfo,
    name: String,
    transport: &'static str,
    content_mode: &'static str,
    udp_stability: AppliedUdpStability,
    /// Viewer-generated media key decoded from base64url. Every media datagram
    /// is AEAD-sealed with it in both directions; possession replaces the old
    /// plaintext challenge-token suffix authentication.
    media_key: [u8; 32],
}

fn canonical_encoder_experiment(id: EncoderExperiment) -> EncoderExperimentInfo {
    let (label, hint) = match id {
        EncoderExperiment::Auto => ("자동", "호스트가 사용 가능한 인코더 경로를 선택합니다."),
        EncoderExperiment::RateControl => {
            ("레이트 컨트롤", "고정 레이트 컨트롤 경로를 사용합니다.")
        }
        EncoderExperiment::AdaptiveQp => (
            "적응형 QP",
            "화면 변화와 인코더 압력에 따라 Base QP를 조절합니다.",
        ),
        EncoderExperiment::EncoderPool => {
            ("인코더 풀", "인코더가 제공하는 픽셀 버퍼 풀을 사용합니다.")
        }
        EncoderExperiment::SplitVertical => (
            "4K 듀얼 인코더",
            "4K 화면을 좌우 두 하드웨어 인코더와 UDP 포트로 전송합니다.",
        ),
        EncoderExperiment::SplitHorizontal => {
            return EncoderExperimentInfo {
                id,
                label: String::new(),
                hint: String::new(),
                requires_reconnect: true,
            }
        }
    };
    EncoderExperimentInfo {
        id,
        label: label.into(),
        hint: hint.into(),
        requires_reconnect: true,
    }
}

fn advertised_encoder_experiments(
    experiments: Vec<EncoderExperimentInfo>,
) -> Vec<EncoderExperimentInfo> {
    let mut normalized = Vec::new();
    for entry in experiments {
        let id = match entry.id {
            EncoderExperiment::Auto
            | EncoderExperiment::RateControl
            | EncoderExperiment::AdaptiveQp
            | EncoderExperiment::EncoderPool
            | EncoderExperiment::SplitVertical => entry.id,
            EncoderExperiment::SplitHorizontal => continue,
        };
        if normalized
            .iter()
            .any(|existing: &EncoderExperimentInfo| existing.id == id)
        {
            continue;
        }
        normalized.push(canonical_encoder_experiment(id));
    }
    normalized
}

fn encoder_experiment_is_startable(
    advertised: &[EncoderExperimentInfo],
    requested: EncoderExperiment,
    split_diagnostic_enabled: bool,
) -> bool {
    match requested {
        EncoderExperiment::SplitVertical => {
            split_diagnostic_enabled
                || advertised
                    .iter()
                    .any(|entry| entry.id == EncoderExperiment::SplitVertical)
        }
        EncoderExperiment::SplitHorizontal => false,
        _ => advertised.iter().any(|entry| entry.id == requested),
    }
}

fn build_attempts(requested: &str, wifi_candidates: &[String]) -> Vec<(String, &'static str)> {
    let mut attempts = Vec::new();
    match requested {
        "usb" => attempts.push(("127.0.0.1".into(), "usb")),
        "tcp" => attempts.extend(
            wifi_candidates
                .iter()
                .cloned()
                .map(|candidate| (candidate, "tcp")),
        ),
        "udp" => attempts.extend(
            wifi_candidates
                .iter()
                .cloned()
                .map(|candidate| (candidate, "udp")),
        ),
        "adbTcp" => attempts.push(("127.0.0.1".into(), "adbTcp")),
        "auto" => {
            attempts.push(("127.0.0.1".into(), "usb"));
            attempts.extend(
                wifi_candidates
                    .iter()
                    .cloned()
                    .map(|candidate| (candidate, "udp")),
            );
            attempts.extend(
                wifi_candidates
                    .iter()
                    .cloned()
                    .map(|candidate| (candidate, "tcp")),
            );
            attempts.push(("127.0.0.1".into(), "adbTcp"));
        }
        _ => {}
    }
    attempts
}

fn adb_forward(port: u16) -> Result<(), String> {
    let mapping = format!("tcp:{port}");
    let output = Command::new("adb")
        .args(["forward", mapping.as_str(), mapping.as_str()])
        .output()
        .map_err(|error| format!("ADB를 실행하지 못했습니다: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if detail.is_empty() {
            format!("ADB USB 포트 전달에 실패했습니다 (code={})", output.status)
        } else {
            format!("ADB USB 포트 전달에 실패했습니다: {detail}")
        })
    }
}

fn adb_remove_forward(port: u16) {
    let mapping = format!("tcp:{port}");
    let _ = Command::new("adb")
        .args(["forward", "--remove", mapping.as_str()])
        .output();
}

fn cleanup_media_transport(transport: &str, port: u16) {
    if transport == "adbTcp" {
        adb_remove_forward(port);
    } else if transport == "usb" {
        // The USB mux is shared by control and media. Stop only the
        // session-scoped loopback media proxy so the accessory link remains
        // available for the next stream and for control polling.
        crate::aoap_proxy::stop_media_proxy();
    }
}

const TERMINAL_SESSION_RETENTION: Duration = Duration::from_secs(5);
const STARTUP_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const STARTUP_STATS_INTERVAL: Duration = Duration::from_millis(100);
/// A healthy viewer polls `getStatus` every 2s, so a connected control
/// socket should never sit silent this long. Half-open connections (network
/// drop, laptop sleep, no FIN) would otherwise hold their task and socket
/// open forever.
const CONTROL_IDLE_TIMEOUT: Duration = Duration::from_secs(15);
/// 클립보드 텍스트 상한(문자 수, U5 — 256KiB, docs/07 §20).
const CLIPBOARD_MAX_CHARS: usize = 262_144;
/// 클립보드 이미지 상한(base64 문자 수 ≈ 6MiB PNG).
const CLIPBOARD_MAX_IMAGE_BASE64: usize = 8 * 1024 * 1024;
/// 동시 제어 연결 상한(F08). 인증 전 소켓도 작업을 무한 스폰하지 않게
/// accept 루프에서 세마포 허가로 제한한다.
const MAX_CONCURRENT_CONNS: usize = 64;
/// 인증 전 줄 상한(F08). ClientHello/키 확인 JSON은 수백 바이트고 루프백
/// 평문 진단 명령도 이보다 작으므로 16KiB면 충분하다. tokio Lines는 줄 길이
/// 상한이 없어 '\n' 없는 입력을 무한 버퍼로 쌓으므로, 직접 상한을 둔다.
const HANDSHAKE_LINE_LIMIT: usize = 16 * 1024;
/// 인증 후 명령 줄 상한(F08). 가장 큰 정상 명령은 setClipboard 이미지다:
/// 클립보드 이미지 base64 상한 8MiB(CLIPBOARD_MAX_IMAGE_BASE64)를 봉인
/// 프레임(AEAD 태그·논스 ≈ 수십 바이트)으로 감싸 다시 base64url로 인코딩하면
/// 8MiB × 4/3 ≈ 11.2MiB + 프레임·wrapper 여유. 파일 전송 청크(sendFileChunk,
/// MAX_CHUNK_BYTES = 1MiB → 청크 base64 1.4MiB가 봉인+재인코딩으로 ≈ 1.9MiB)
/// 보다도 크다. 이를 모두 담는 12MiB로 둔다.
const COMMAND_LINE_LIMIT: usize = 12 * 1024 * 1024;

/// 클립보드 텍스트의 sha256(hex). 뷰어의 해시 짧은 폴링과 루프 방지에 쓰인다.
fn clipboard_sha256_hex(text: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

pub struct ControlServer {
    source_operations: crate::source_grants::SourceLease,
    input_changes: Mutex<()>,
    replacement_retired: Mutex<HashMap<u32, u32>>,
    pending_retirements: Mutex<HashMap<u32, String>>,
    backend: SharedBackend,
    pairing: std::sync::Arc<crate::pairing::PairingServer>,
    /// 제어 평면 핸드셰이크의 ServerHello 서명에 쓰이는 호스트 정체 키.
    identity: std::sync::Arc<secure_channel::HostIdentity>,
    control_port: AtomicU16,
    sessions: Mutex<State>,
    starting: Mutex<Starts>,
    /// :7777 토큰 무차별 시도에 대한 IP별 백오프.
    auth_limiter: AuthRateLimiter,
    /// 세션 감사 로그(선택 — set_audit으로 주입).
    audit: std::sync::OnceLock<std::sync::Arc<crate::audit::SessionAudit>>,
    /// 클립보드 텍스트 동기화 호스트 게이트(U5, 기본 꺼짐 — docs/07 §20).
    clipboard_share: AtomicBool,
    clipboard_revision_cache: Mutex<Option<(u64, String)>>,
    /// 클립보드 접근 백엔드(선택 — set_clipboard로 플러그인 구현을 주입;
    /// 미주입 시 pbcopy/pbpaste 폴백).
    clipboard: std::sync::OnceLock<std::sync::Arc<dyn ClipboardBackend>>,
    /// 파일 공유 게이트(선택 — set_settings으로 주입; 없으면 꺼짐).
    settings: std::sync::OnceLock<std::sync::Arc<crate::settings::SharedSettings>>,
    /// 파일 전송 청크 상태와 호스트 공유 대기열. Tauri UI 명령도 같은
    /// 인스턴스를 본다(공유 대기열 추가·삭제).
    file_transfers: crate::file_transfer::FileTransferState,
    /// 장치별 살아 있는 인증 연결. 연결 인증은 소켓 수명당 한 번이므로,
    /// 철회가 즉시 효력을 가지려면 열린 소켓도 함께 깨워야 한다.
    authenticated_conns: Mutex<HashMap<String, Vec<std::sync::Arc<tokio::sync::Notify>>>>,
    /// 화면 잠금 실행부(선택 — set_lock_screen으로 주입; 테스트는 카운터로
    /// 대체한다). 미주입 시 잠금 설정이 켜져 있어도 아무 것도 하지 않는다.
    lock_screen: std::sync::OnceLock<std::sync::Arc<dyn Fn() + Send + Sync>>,
    /// 프라이버시 커튼 실행부(선택 — show/hide; lib.rs가 Tauri 창으로 구현해
    /// 주입한다). 상태 변화 시에만 호출되며, 적용 성공 여부를 돌려준다 —
    /// 커튼 상태 커밋은 성공 뒤에만 하기 위해서다(M3).
    curtain: std::sync::OnceLock<std::sync::Arc<dyn Fn(bool) -> bool + Send + Sync>>,
    /// 커튼 마지막 적용 상태 — 중복 apply를 막는다.
    curtain_state: Mutex<bool>,
    /// 진행 중인 재구성의 세션 id(F02). 같은 세션의 동시 재구성이 stop→swap
    /// 도중에 교차하면 핸들을 덮어써 캡처가 유출되므로 세션당 하나만
    /// 진행한다. 집합 기반이라 다른 세션의 재구성은 막지 않는다.
    reconfiguring: Mutex<std::collections::HashSet<u32>>,
    /// 동시 연결 상한 허가(F08). accept 루프가 연결 작업 하나당 하나씩
    /// 집고, 작업이 끝나면 반납된다.
    conn_permits: std::sync::Arc<tokio::sync::Semaphore>,
}

struct State {
    next: u32,
    live: HashMap<u32, Session>,
}

impl ControlServer {
    pub fn new(
        backend: SharedBackend,
        pairing: std::sync::Arc<crate::pairing::PairingServer>,
        identity: std::sync::Arc<secure_channel::HostIdentity>,
    ) -> Self {
        Self {
            backend,
            pairing,
            identity,
            control_port: AtomicU16::new(crate::PREFERRED_CONTROL_PORT),
            sessions: Mutex::new(State {
                next: 1,
                live: HashMap::new(),
            }),
            starting: Mutex::new(Starts::default()),
            auth_limiter: AuthRateLimiter::new(),
            audit: std::sync::OnceLock::new(),
            clipboard_share: AtomicBool::new(false),
            clipboard_revision_cache: Mutex::new(None),
            clipboard: std::sync::OnceLock::new(),
            settings: std::sync::OnceLock::new(),
            file_transfers: crate::file_transfer::FileTransferState::default(),
            authenticated_conns: Mutex::new(HashMap::new()),
            lock_screen: std::sync::OnceLock::new(),
            curtain: std::sync::OnceLock::new(),
            curtain_state: Mutex::new(false),
            reconfiguring: Mutex::new(std::collections::HashSet::new()),
            source_operations: crate::source_grants::SourceLease::default(),
            input_changes: Mutex::new(()),
            replacement_retired: Mutex::new(HashMap::new()),
            pending_retirements: Mutex::new(HashMap::new()),
            conn_permits: std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_CONNS)),
        }
    }

    pub fn set_control_port(&self, port: u16) {
        self.control_port.store(port, Ordering::Release);
    }

    /// 화면 잠금 실행부 주입(lib.rs setup). 테스트는 카운터를 넣는다.
    pub fn set_lock_screen(&self, lock: std::sync::Arc<dyn Fn() + Send + Sync>) {
        let _ = self.lock_screen.set(lock);
    }

    /// 커튼 실행부 주입(lib.rs setup — 모니터별 검은 오버레이 창).
    pub fn set_curtain_controller(
        &self,
        apply: std::sync::Arc<dyn Fn(bool) -> bool + Send + Sync>,
    ) {
        let _ = self.curtain.set(apply);
    }

    /// 커튼 상태 재계산: 설정이 켜져 있고 라이브 세션이 하나라도 있으면
    /// 오버레이를 띄운다. 상태가 바뀔 때만 실행부를 부른다(세션 시작·종료,
    /// 설정 토글에서 호출).
    pub fn refresh_curtain(&self) {
        self.refresh_curtain_with(false);
    }

    /// 스트림 시작 직전의 커튼 재계산. 이번에 시작할 세션은 아직 live에
    /// 없으므로 pending 세션이 있다는 조건으로 원하는 상태를 계산한다 —
    /// 설정이 켜져 있으면 첫 세션(live가 빈 상태)에서도 커튼 창이
    /// `backend.start`가 필터를 만들기 전에 존재해야 캡처에서 제외된다
    /// (SCK 필터의 제외 목록은 start 시점의 창 스냅샷이다).
    pub fn prepare_curtain_for_start(&self) {
        self.refresh_curtain_with(true);
    }

    fn refresh_curtain_with(&self, pending_session: bool) {
        let desired = self.settings.get().is_some_and(|s| s.privacy_curtain())
            && (pending_session || !self.sessions.lock().unwrap().live.is_empty());
        let mut last = self.curtain_state.lock().unwrap();
        if *last == desired {
            return;
        }
        let Some(apply) = self.curtain.get() else {
            // 실행부가 없으면 적용 자체가 없다 — 상태만 커밋해 기존 동작을
            // 유지한다(테스트 주입 환경 포함).
            *last = desired;
            return;
        };
        // 상태 커밋은 적용이 성공한 뒤에만 한다(M3). 창 생성에 실패했거나
        // 모니터를 못 읽었으면 last를 그대로 둔다 — 다음 refresh 트리거
        // (세션 시작·종료, 설정 토글)에서 다시 시도한다.
        if apply(desired) {
            *last = desired;
            self.audit_log("privacy_curtain", json!({ "shown": desired }));
        }
    }

    /// 커튼 토글이 켜진 직후 살아 있는 세션을 정리한다. SCK 필터의 제외
    /// 목록은 시작 시점의 창 스냅샷이라, 이미 스트리밍 중인 세션은 같은
    /// 형태로 재시작해 필터를 다시 만들어야 커튼이 캡처에서 빠진다(H1).
    /// CGDisplayStream은 창 제외가 아예 없어(M6) 커튼이 켜진 채 계속
    /// 돌리면 검은 화면이므로 종료한다.
    pub async fn restart_sessions_for_curtain(&self) {
        let targets: Vec<u32> = {
            let st = self.sessions.lock().unwrap();
            st.live
                .iter()
                .filter(|(_, s)| !s.backend_released)
                .map(|(id, _)| *id)
                .collect()
        };
        for id in targets {
            let input = {
                let st = self.sessions.lock().unwrap();
                let Some(s) = st.live.get(&id) else {
                    continue;
                };
                if s.capture_backend == "cgDisplayStream" {
                    None
                } else {
                    Some(ReconfigureStreamInput {
                        source_id: None,
                        session: id,
                        width: s.width,
                        height: s.height,
                        fps: s.fps_target,
                        quality_state: s.quality_state.clone(),
                        encoder_experiment: None,
                        source_index: None,
                    })
                }
            };
            match input {
                None => {
                    // 재시작해도 제외가 불가능하다 — 검은 화면 콤비네이션을
                    // 남기지 않고 명확한 종료로 처리한다(M6).
                    let _ = self.force_stop_session(id);
                }
                Some(input) => {
                    // 재시작 실패 시 세션은 reconfigure의 기존 복구 경로
                    // (톰브스톤 또는 이전 스트림 복원)를 따른다.
                    let _ = self.reconfigure_stream(input).await;
                }
            }
        }
    }

    /// 이번 teardown에서 세션이 실제로 제거되어 live가 비게 됐을 때만
    /// (설정 켜짐) 화면을 잠근다. `removed`는 이번에 치운 세션 수 —
    /// reconfigure는 live 맵에서 세션을 치우지 않으므로 이 훅에 걸리지
    /// 않고, 같은 뷰어 주소의 교체(stale 정리)도 의도적으로 제외한다.
    /// 이미 비어 있는 상태의 getStatus 폴링이 잠금을 반복하지 않게 하는
    /// 것도 이 전이 조건의 역할이다.
    fn maybe_lock_after_teardown(&self, removed: usize) {
        if removed == 0 {
            return;
        }
        if !self.sessions.lock().unwrap().live.is_empty() {
            return;
        }
        if !self.settings.get().is_some_and(|s| s.lock_on_disconnect()) {
            return;
        }
        if let Some(lock) = self.lock_screen.get() {
            lock();
            self.audit_log("screen_locked", json!({ "reason": "last_session_ended" }));
        }
    }

    pub fn set_audit(&self, audit: std::sync::Arc<crate::audit::SessionAudit>) {
        let _ = self.audit.set(audit);
    }

    /// 클립보드 텍스트 동기화 호스트 게이트(설정에서 기동 시 주입, 토글로
    /// 즉시 전환된다). 기본값은 꺼짐 — 문서 §20의 이중 잠금 중 호스트 쪽.
    pub fn set_clipboard_share(&self, enabled: bool) {
        self.clipboard_share.store(enabled, Ordering::Release);
        if !enabled {
            *self.clipboard_revision_cache.lock().unwrap() = None;
        }
    }

    pub fn clipboard_share_enabled(&self) -> bool {
        self.clipboard_share.load(Ordering::Acquire)
    }

    /// 플러그인 기반 클립보드 백엔드를 주입한다(lib.rs setup). 미주입 시
    /// pbcopy/pbpaste 폴백이 쓰인다.
    pub fn set_clipboard(&self, backend: std::sync::Arc<dyn ClipboardBackend>) {
        let _ = self.clipboard.set(backend);
    }

    fn clipboard_backend(&self) -> std::sync::Arc<dyn ClipboardBackend> {
        self.clipboard
            .get_or_init(|| std::sync::Arc::new(crate::clipboard::SystemClipboard))
            .clone()
    }

    pub fn set_settings(&self, settings: std::sync::Arc<crate::settings::SharedSettings>) {
        let _ = self.settings.set(settings);
    }

    /// 호스트 UI(공유 대기열 관리)가 같은 파일 전송 상태를 쓰게 한다.
    pub fn file_transfer_state(&self) -> &crate::file_transfer::FileTransferState {
        &self.file_transfers
    }

    /// 파일 공유 게이트. 설정이 주입되지 않았으면 안전 쪽인 꺼짐이다.
    fn file_share_gate(&self) -> bool {
        self.settings.get().is_some_and(|s| s.file_share())
    }

    fn audit_log(&self, event: &str, fields: serde_json::Value) {
        if let Some(audit) = self.audit.get() {
            audit.log(event, fields);
        }
    }

    /// 수집된 세션을 강제 종료하고 미디어 자원을 치운 뒤 잠금·커튼 훅까지
    /// 돌린다. 새 stop 경로가 정리 훅을 빠뜨리지 않게 전골을 한곳에 둔다.
    /// `device`가 Some이면 그 장치의 인증 연결만, None이면 전부 끊는다.
    fn teardown_targets(
        &self,
        targets: Vec<(u32, Session)>,
        reason: &str,
        device: Option<&str>,
    ) -> usize {
        // Disconnect the affected connections before a blocking backend can
        // allow newly authenticated successors to join the registry.
        match device {
            Some(device) => self.disconnect_device_conns(device),
            None => self.disconnect_all_conns(),
        }
        for (id, session) in &targets {
            Self::invalidate_session_source(session);
            if !session.backend_released {
                let _ = self.retire_session_capture(*id, session.handle, Some(2));
            }
            self.cleanup_registered_transport(session.transport_owner.as_ref());
            self.audit_log(
                "session_stopped",
                json!({"session":id,"device":session.device_id,"reason":reason}),
            );
        }
        let removed = targets.len();
        self.maybe_lock_after_teardown(removed);
        self.refresh_curtain();
        removed
    }

    pub fn stop_sessions_for_device(&self, device_id: &str) -> usize {
        let targets = {
            let mut state = self.sessions.lock().unwrap();
            let ids: Vec<_> = state
                .live
                .iter()
                .filter(|(_, s)| s.device_id.as_deref() == Some(device_id))
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter()
                .filter_map(|id| state.live.remove(&id).map(|session| (id, session)))
                .collect()
        };
        self.teardown_targets(targets, "device_revoked", Some(device_id))
    }

    fn invalidate_session_source(session: &Session) {
        if let Some(access) = session
            .authorization
            .as_ref()
            .and_then(|auth| auth.access.as_ref())
        {
            access.lease.invalidate();
        }
    }
    // Only a logical session's old handle can be a known completed retirement.
    // Uncommitted replacement/recovery handles always go through native stop,
    // even if a backend reuses the same numeric handle.
    fn retire_session_capture(
        &self,
        session: u32,
        handle: u32,
        reason: Option<u8>,
    ) -> Result<(), String> {
        if self.replacement_retired.lock().unwrap().get(&session) == Some(&handle) {
            return Ok(());
        }
        self.retire_capture(handle, reason)
    }
    fn retire_capture(&self, handle: u32, reason: Option<u8>) -> Result<(), String> {
        let result = match reason {
            Some(reason) => self.backend.stop_with_reason(handle, reason),
            None => self.backend.stop(handle),
        };
        let mut pending = self.pending_retirements.lock().unwrap();
        match &result {
            Ok(()) => {
                pending.remove(&handle);
            }
            Err(error) => {
                pending.insert(handle, error.clone());
            }
        }
        result
    }
    pub fn host_sources(&self) -> Result<Vec<control_contract::host::DisplayInfo>, String> {
        self.backend.list_displays()
    }
    pub fn set_source_grants(
        &self,
        device: &str,
        sources: Vec<String>,
    ) -> Result<crate::source_grants::GrantView, String> {
        self.set_source_grants_for_credential(device, sources, None)
    }
    pub fn set_source_grants_for_credential(
        &self,
        device: &str,
        sources: Vec<String>,
        expected_credential: Option<&str>,
    ) -> Result<crate::source_grants::GrantView, String> {
        let _operation = self
            .source_operations
            .enter()
            .ok_or("Host is shutting down")?;
        let displays = if sources.is_empty() {
            vec![]
        } else {
            self.backend.list_displays()?
        };
        for source in &sources {
            resolve_display(&displays, Some(source), None)?;
        }
        let (result, leases) = self.pairing.update_source_grants_for_credential(
            device,
            sources,
            expected_credential,
        )?;
        self.stop_sessions_for_device(device);
        for lease in leases {
            lease.wait_idle();
        }
        if let Some(error) = self.pending_retirements.lock().unwrap().values().next() {
            return Err(format!(
                "capture retirement failed; access is blocked: {error}"
            ));
        }
        result
    }
    pub(crate) fn admit_source_admin(&self) -> Option<crate::source_grants::SourceOperation<'_>> {
        self.source_operations.enter()
    }
    pub fn shutdown_source_access(&self) -> Result<(), String> {
        self.source_operations.invalidate();
        let leases = self.pairing.fence_source_access();
        for lease in leases {
            lease.wait_idle();
        }
        self.source_operations.wait_idle();
        let targets: Vec<_> = self.sessions.lock().unwrap().live.drain().collect();
        self.disconnect_all_conns();
        let pending: Vec<_> = self
            .pending_retirements
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect();
        // Admissions are fenced and drained: no handle can acquire a newer
        // incarnation during this shutdown pass. Do not retry a successful
        // pending retirement again through its remaining logical tombstone.
        let mut retired = std::collections::HashSet::new();
        for handle in pending {
            if self.retire_capture(handle, Some(3)).is_ok() {
                retired.insert(handle);
            }
        }
        let mut failure = None;
        for (id, session) in targets {
            if !session.backend_released && !retired.contains(&session.handle) {
                if let Err(error) = self.retire_session_capture(id, session.handle, Some(3)) {
                    failure = Some(error);
                }
            }
            self.cleanup_registered_transport(session.transport_owner.as_ref());
        }
        if failure.is_none() {
            failure = self
                .pending_retirements
                .lock()
                .unwrap()
                .values()
                .next()
                .cloned();
        }
        if let Some(error) = failure {
            return Err(format!(
                "capture retirement failed; grant journal remains dirty: {error}"
            ));
        }
        self.pairing.finish_source_shutdown()
    }

    pub fn revoke_device(&self, device_id: &str) -> crate::pairing::RevokeOutcome {
        let Some(_operation) = self.admit_source_admin() else {
            return crate::pairing::RevokeOutcome {
                persistence_errors: vec!["Host is shutting down".into()],
                ..Default::default()
            };
        };
        let mut outcome = self.pairing.revoke(device_id);
        if !outcome.removed_devices.is_empty() {
            let stopped = self.stop_sessions_for_device(device_id);
            self.audit_log(
                "device_revoked",
                json!({"device":device_id,"stopped_sessions":stopped}),
            );
        }
        for lease in outcome.retired_leases.drain(..) {
            lease.wait_idle();
        }
        outcome.persistence_errors.extend(
            self.pending_retirements
                .lock()
                .unwrap()
                .values()
                .map(|error| format!("capture retirement failed; access is blocked: {error}")),
        );
        outcome
    }
    pub fn revoke_all_devices(&self) -> crate::pairing::RevokeOutcome {
        let Some(_operation) = self.admit_source_admin() else {
            return crate::pairing::RevokeOutcome {
                persistence_errors: vec!["Host is shutting down".into()],
                ..Default::default()
            };
        };
        let mut outcome = self.pairing.revoke_all();
        self.stop_all_sessions();
        self.audit_log(
            "devices_revoked_all",
            json!({"devices":outcome.removed_devices.len()}),
        );
        for lease in outcome.retired_leases.drain(..) {
            lease.wait_idle();
        }
        outcome.persistence_errors.extend(
            self.pending_retirements
                .lock()
                .unwrap()
                .values()
                .map(|error| format!("capture retirement failed; access is blocked: {error}")),
        );
        outcome
    }

    pub fn stop_all_sessions(&self) {
        let targets = self.sessions.lock().unwrap().live.drain().collect();
        self.teardown_targets(targets, "devices_revoked_all", None);
    }

    /// 이 연결을 장치의 살아 있는 인증 연결로 등록한다. 반환된 가드가
    /// 떨어질 때(연결 루프 종료) 자동으로 해지된다.
    fn register_authed_conn(
        &self,
        device: &str,
    ) -> (std::sync::Arc<tokio::sync::Notify>, AuthedConnLease<'_>) {
        let notify = std::sync::Arc::new(tokio::sync::Notify::new());
        self.authenticated_conns
            .lock()
            .unwrap()
            .entry(device.to_owned())
            .or_default()
            .push(std::sync::Arc::clone(&notify));
        let lease = AuthedConnLease {
            server: self,
            device: device.to_owned(),
            notify: std::sync::Arc::clone(&notify),
        };
        (std::sync::Arc::clone(&notify), lease)
    }

    fn unregister_authed_conn(&self, device: &str, notify: &std::sync::Arc<tokio::sync::Notify>) {
        let mut map = self.authenticated_conns.lock().unwrap();
        if let Some(list) = map.get_mut(device) {
            list.retain(|n| !std::sync::Arc::ptr_eq(n, notify));
            if list.is_empty() {
                map.remove(device);
            }
        }
    }

    /// 해당 장치의 인증 연결을 모두 깨운다. notify_one은 대기자가 없어도
    /// 허가를 남기므로, 디스패치 중이라 다음 읽기 대기로 넘어간 루프도
    /// 즉시 종료된다.
    fn disconnect_device_conns(&self, device: &str) {
        let list = self.authenticated_conns.lock().unwrap().remove(device);
        if let Some(list) = list {
            for notify in list {
                notify.notify_one();
            }
        }
    }

    fn disconnect_all_conns(&self) {
        let all: Vec<_> = self
            .authenticated_conns
            .lock()
            .unwrap()
            .drain()
            .flat_map(|(_, list)| list)
            .collect();
        for notify in all {
            notify.notify_one();
        }
    }

    /// Restore the previous capture geometry after a failed reconfigure. On
    /// recovery the session stays live with the restored handle; otherwise it
    /// becomes a terminal tombstone carrying the original failure.
    async fn restore_previous_stream(
        &self,
        session_id: u32,
        previous: &ReconfigureSnapshot,
        failure: &str,
    ) -> Result<u32, String> {
        if !self.replacement_is_current(session_id, previous) {
            return Err("session ended during reconfigure".into());
        }
        let setup = self
            .with_start_transport(previous.transport_attempt.as_ref().unwrap(), || {
                self.prepare_owned_transport(
                    previous.transport_attempt.as_ref().unwrap(),
                    &previous.media_transport,
                    previous.viewer_port,
                )
            })
            .unwrap_or_else(|| Err("recovery superseded".to_owned()));
        let recovered = setup.and_then(|()| {
            self.start_replacement_from_snapshot(
                previous,
                previous.source_index,
                previous.encoder_experiment,
                previous.width,
                previous.height,
                previous.fps_target,
            )
        });
        let recovered = match recovered {
            Ok(handle) => match self.wait_for_first_frame(handle).await {
                Ok(()) => Ok(handle),
                Err(error) => {
                    self.discard_backend(handle);
                    Err(error)
                }
            },
            Err(error) => Err(error),
        };
        let _input_change = self.input_changes.lock().unwrap();
        let current_input = self
            .sessions
            .lock()
            .unwrap()
            .live
            .get(&session_id)
            .is_some_and(|s| s.input_enabled);
        let input_enabled = match recovered {
            Ok(handle) if current_input => self.backend.set_input_enabled(handle, true).is_ok(),
            _ => false,
        };
        let committed = self
            .pairing
            .with_authorization(previous.authorization.as_ref(), || {
                let mut starts = self.starting.lock().unwrap();
                if !previous
                    .transport_attempt
                    .as_ref()
                    .is_some_and(|attempt| starts.accepts(attempt))
                {
                    return false;
                }
                let mut state = self.sessions.lock().unwrap();
                let Some(session) = state
                    .live
                    .get_mut(&session_id)
                    .filter(|s| s.lifecycle.accepts(previous.operation, s.backend_released))
                else {
                    return false;
                };
                match &recovered {
                    Ok(handle) => {
                        let owner = previous.transport_attempt.as_ref().unwrap();
                        starts.register(owner);
                        session.transport_owner = Some(owner.clone());

                        self.replacement_retired.lock().unwrap().remove(&session_id);
                        session.handle = *handle;
                        session.input_enabled = input_enabled;
                    }
                    Err(_) => {
                        session.input_enabled = false;
                        session.terminal_error = Some(failure.to_owned());
                        session.terminal_since = Some(Instant::now());
                        session.backend_released = true;
                        session.lifecycle.invalidate();
                    }
                }
                true
            })
            .unwrap_or(false);
        if !committed || recovered.is_err() {
            self.cleanup_owned_transport(previous.transport_attempt.as_ref().unwrap());
        }
        if !committed {
            if let Ok(handle) = recovered {
                self.discard_backend(handle);
            }
            return Err("session ended during reconfigure".into());
        }
        recovered.map_err(|_| format!("replacement stream failed: {failure}"))
    }

    fn replacement_is_current(&self, session_id: u32, previous: &ReconfigureSnapshot) -> bool {
        self.pairing
            .with_authorization(previous.authorization.as_ref(), || {
                let starts = self.starting.lock().unwrap();
                if !previous
                    .transport_attempt
                    .as_ref()
                    .is_some_and(|attempt| starts.accepts(attempt))
                {
                    return false;
                }
                self.sessions
                    .lock()
                    .unwrap()
                    .live
                    .get(&session_id)
                    .is_some_and(|s| s.lifecycle.accepts(previous.operation, s.backend_released))
            })
            .unwrap_or(false)
    }

    /// All callbacks happen after releasing pairing and session locks, including
    /// cleanup of a result rejected by the final commit predicate.
    fn discard_backend(&self, handle: u32) {
        let _ = self.backend.set_input_enabled(handle, false);
        let _ = self.retire_capture(handle, None);
    }

    /// Logical transport lease: mutations execute without a mutex held, but a
    /// competing start cannot claim this endpoint until the action finishes.
    fn with_start_transport<T>(
        &self,
        attempt: &StartAttempt,
        action: impl FnOnce() -> T,
    ) -> Option<T> {
        if !self
            .starting
            .lock()
            .unwrap()
            .begin_transport_action(attempt)
        {
            return None;
        }
        let _guard = StartTransportGuard {
            starts: &self.starting,
            attempt,
        };
        Some(action())
    }

    /// Transfer the actual resource while holding the logical setup lease.
    /// Publication can happen much later, after first-frame validation.
    fn prepare_owned_transport(
        &self,
        attempt: &StartAttempt,
        transport: &str,
        port: u16,
    ) -> Result<(), String> {
        let predecessor = self
            .starting
            .lock()
            .unwrap()
            .replace_transport(attempt, transport, port);
        if let Some((kind, old_port)) = predecessor {
            cleanup_media_transport(&kind, old_port);
        }
        cleanup_media_transport(transport, port);
        let result = if transport == "adbTcp" {
            adb_forward(port)
        } else if transport == "usb" {
            crate::aoap_proxy::start_media_proxy(port)
        } else {
            Ok(())
        };
        if result.is_err() {
            // Setup may allocate a resource before reporting an error.
            cleanup_media_transport(transport, port);
            self.starting.lock().unwrap().release_transport(attempt);
        }
        result
    }

    /// Cleanup follows physical ownership, not the newest request or the
    /// presence of a Session. A pending start owns its resource immediately.
    fn cleanup_owned_transport(&self, owner: &StartAttempt) {
        let resource = self.starting.lock().unwrap().begin_owned_cleanup(owner);
        if let Some((transport, port)) = resource {
            let _guard = StartTransportGuard {
                starts: &self.starting,
                attempt: owner,
            };
            cleanup_media_transport(&transport, port);
            self.starting.lock().unwrap().release_transport(owner);
        }
    }

    fn cleanup_registered_transport(&self, owner: Option<&StartAttempt>) {
        if let Some(owner) = owner {
            self.cleanup_owned_transport(owner);
            self.starting.lock().unwrap().release_registered(owner);
        }
    }

    fn start_replacement_from_snapshot(
        &self,
        previous: &ReconfigureSnapshot,
        source_index: u32,
        encoder_experiment: EncoderExperiment,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<u32, String> {
        let default_udp_stability = AppliedUdpStability {
            requested: control_contract::udp_stability::UdpStabilityProfile::Auto,
            applied: control_contract::udp_stability::UdpStabilityProfile::Auto,
            burst_datagrams: 0,
            fec_parity_shards: 0,
            adaptive_pacing: false,
            fallback_reason: None,
        };
        let udp_stability = previous
            .udp_stability
            .as_ref()
            .unwrap_or(&default_udp_stability);
        self.backend.start(
            source_index,
            &previous.viewer_host,
            previous.viewer_port,
            width,
            height,
            fps,
            &previous.capture_backend,
            &previous.media_transport,
            &previous.content_mode,
            encoder_experiment,
            udp_stability,
            &previous.media_key,
            previous
                .authorization
                .as_ref()
                .and_then(|auth| auth.access.as_ref()),
        )
    }

    async fn reconfigure_stream(
        &self,
        input: ReconfigureStreamInput,
    ) -> Result<ReconfigureStreamOutput, String> {
        let _source_operation = self
            .source_operations
            .enter()
            .ok_or("Host is shutting down")?;
        // 같은 세션의 동시 재구성은 하나만 진행한다(F02). 교차하면 두 요청이
        // 같은 이전 핸들을 읽어 stop/start가 엇갈리고, 늦게 스왑한 쪽이
        // 상대의 교체 핸들을 덮어써 유출한다. 실패 경로에서도 풀리도록
        // drop 가드로 관리한다.
        if !self.reconfiguring.lock().unwrap().insert(input.session) {
            return Err(format!(
                "reconfigure already in progress for session {}",
                input.session
            ));
        }
        let _guard = ReconfigureGuard {
            in_flight: &self.reconfiguring,
            retired: &self.replacement_retired,
            session: input.session,
        };
        let request = self.starting.lock().unwrap().begin();
        let _request_guard = StartRequestGuard {
            starts: &self.starting,
            request,
        };
        validate_stream_shape(input.width, input.height, input.fps)?;
        if !matches!(
            input.quality_state.as_str(),
            "native" | "fallback" | "downshifting" | "upshifting"
        ) {
            return Err("unsupported quality state".into());
        }
        let settled_quality_state = match input.quality_state.as_str() {
            "fallback" | "downshifting" => "fallback",
            _ => "native",
        };

        let mut previous = {
            let mut state = self.sessions.lock().unwrap();
            let session = state
                .live
                .get_mut(&input.session)
                .filter(|session| !session.backend_released && !session.lifecycle.is_stopped())
                .ok_or_else(|| format!("no such session {}", input.session))?;
            let viewer_host = session
                .viewer_addr
                .rsplit_once(':')
                .map(|(host, _)| host.to_owned())
                .ok_or_else(|| "session viewer address is invalid".to_owned())?;
            ReconfigureSnapshot {
                transport_attempt: None,
                device_id: session.device_id.clone(),
                operation: session.lifecycle.begin(),
                authorization: session.authorization.clone(),
                handle: session.handle,
                source_index: session.source_index,
                viewer_host,
                viewer_port: session.viewer_port,
                capture_backend: session.capture_backend.clone(),
                media_transport: session.media_transport.clone(),
                content_mode: session.content_mode.clone(),
                encoder_experiment: session.encoder_experiment,
                udp_stability: session.udp_stability.clone(),
                media_key: session.media_key,
                width: session.width,
                height: session.height,
                fps_target: session.fps_target,
            }
        };

        let requested_transport = normalize_media_transport(&previous.media_transport)
            .ok_or_else(|| format!("unsupported media transport: {}", previous.media_transport))?;
        // 커튼이 켜진 상태의 CGDisplayStream 재구성도 거부한다 — 새 필터에도
        // 창 제외가 없어 검은 화면만 만든다(M6, plan_start의 시작 경로 가드와
        // 같은 이유다).
        if previous.capture_backend == "cgDisplayStream"
            && self.settings.get().is_some_and(|s| s.privacy_curtain())
        {
            return Err("privacy curtain requires the screenCaptureKit capture backend".into());
        }
        // Resolve the replacement experiment and validate it BEFORE stopping
        // the previous backend: explicit unsupported or malformed input must
        // leave the live stream unchanged.
        let replacement_encoder_experiment = match input.encoder_experiment {
            Some(requested) => {
                let advertised = match self.backend.encoder_experiments() {
                    Ok(experiments) => advertised_encoder_experiments(experiments),
                    Err(error) => return Err(error),
                };
                let split_diagnostic_enabled = std::env::var("LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC")
                    .is_ok_and(|value| value == "1");
                if !encoder_experiment_is_startable(
                    &advertised,
                    requested,
                    split_diagnostic_enabled,
                ) {
                    return Err(format!(
                        "unsupported encoder experiment: {}",
                        requested.as_str()
                    ));
                }
                requested
            }
            None => {
                // Older request without a mode: retain the previous
                // experiment, demoting split to the single path when
                // leaving exact 4K (legacy behavior).
                if previous.encoder_experiment == EncoderExperiment::SplitVertical
                    && (input.width != 3_840 || input.height != 2_160)
                {
                    EncoderExperiment::Auto
                } else {
                    previous.encoder_experiment
                }
            }
        };

        // Split keeps the exact 4K60 direct-UDP consecutive-port
        // restriction; other modes use the supported single path. Both
        // branches resolve the replacement experiment FIRST and share this
        // validation, so legacy mode-omitted requests cannot retain an
        // invalid split shape (e.g. exact 4K at 90/30fps) either.
        validate_split_request(
            replacement_encoder_experiment,
            input.width,
            input.height,
            input.fps,
            requested_transport,
            previous.viewer_port,
        )?;

        // Resolve the requested capture source (cheap display switch) BEFORE
        // stopping the previous backend: an unknown or out-of-range display
        // must leave the live stream untouched, exactly like the shape and
        // experiment validations above. An absent index keeps the session's
        // current display — the legacy wire shape for old viewers.
        let previous_source_id = previous
            .authorization
            .as_ref()
            .and_then(|a| a.access.as_ref())
            .map(|a| a.source_id.clone());
        let requested_source_id = match (input.source_id.clone(), input.source_index) {
            (Some(id), _) => Some(id),
            (None, Some(index)) => Some(
                self.pairing.catalog_source(
                    previous
                        .authorization
                        .as_ref()
                        .ok_or("source_access_denied")?,
                    index,
                )?,
            ),
            (None, None) => previous_source_id.clone(),
        };
        let requested_id = requested_source_id.as_deref();
        let replacement_source = resolve_display(
            &self.backend.list_displays()?,
            requested_id,
            input.source_index.or(Some(previous.source_index)),
        )?;
        let replacement_source_index = replacement_source.index;
        let replacement_source_name = replacement_source.name.clone();
        let source_changed =
            previous_source_id.as_deref() != replacement_source.source_id.as_deref();
        let auth = previous
            .authorization
            .as_ref()
            .ok_or("source_access_denied")?;
        // Rebind from the original credential incarnation, never from device ID.
        let replacement_authorization = Some(
            self.pairing
                .source_authorization(auth, replacement_source.source_id.as_deref().unwrap())?,
        );

        previous.transport_attempt = self.starting.lock().unwrap().claim(
            request,
            previous.device_id.as_deref(),
            &format!("{}:{}", previous.viewer_host, previous.viewer_port),
        );
        if previous.transport_attempt.is_none() {
            return Err("reconfigure superseded or transport operation in progress".into());
        }
        if !self.replacement_is_current(input.session, &previous) {
            return Err(format!(
                "session {} ended during reconfigure",
                input.session
            ));
        }
        let mut backend_stopped = false;
        let setup = self
            .with_start_transport(previous.transport_attempt.as_ref().unwrap(), || {
                {
                    // Serialize actual handle retirement with Host input application.
                    let _input_change = self.input_changes.lock().unwrap();
                    self.retire_capture(previous.handle, None)?;
                    self.replacement_retired
                        .lock()
                        .unwrap()
                        .insert(input.session, previous.handle);
                    backend_stopped = true;
                }
                self.prepare_owned_transport(
                    previous.transport_attempt.as_ref().unwrap(),
                    &previous.media_transport,
                    previous.viewer_port,
                )
            })
            .ok_or_else(|| "reconfigure superseded".to_owned())?;
        if let Err(error) = setup {
            if backend_stopped {
                let _ = self
                    .restore_previous_stream(input.session, &previous, &error)
                    .await;
            }
            return Err(error);
        }

        let replacement_snapshot = ReconfigureSnapshot {
            authorization: replacement_authorization.clone(),
            ..previous.clone()
        };
        let replacement_handle = match self.start_replacement_from_snapshot(
            &replacement_snapshot,
            replacement_source_index,
            replacement_encoder_experiment,
            input.width,
            input.height,
            input.fps,
        ) {
            Ok(handle) => match self.wait_for_first_frame(handle).await {
                Ok(()) => handle,
                Err(error) => {
                    self.discard_backend(handle);
                    self.cleanup_owned_transport(previous.transport_attempt.as_ref().unwrap());
                    let _ = self
                        .restore_previous_stream(input.session, &previous, &error)
                        .await;
                    return Err(format!("replacement stream startup failed: {error}"));
                }
            },
            Err(error) => {
                self.cleanup_owned_transport(previous.transport_attempt.as_ref().unwrap());
                let _ = self
                    .restore_previous_stream(input.session, &previous, &error)
                    .await;
                return Err(format!("replacement stream failed: {error}"));
            }
        };

        let _input_change = self.input_changes.lock().unwrap();
        let input_enabled = !source_changed
            && self
                .sessions
                .lock()
                .unwrap()
                .live
                .get(&input.session)
                .is_some_and(|s| {
                    s.input_enabled && s.lifecycle.accepts(previous.operation, s.backend_released)
                })
            && self
                .backend
                .set_input_enabled(replacement_handle, true)
                .is_ok();

        let committed = self
            .pairing
            .with_authorization(replacement_authorization.as_ref(), || {
                let mut starts = self.starting.lock().unwrap();
                if !previous
                    .transport_attempt
                    .as_ref()
                    .is_some_and(|attempt| starts.accepts(attempt))
                {
                    return false;
                }
                let mut state = self.sessions.lock().unwrap();
                let Some(session) = state
                    .live
                    .get_mut(&input.session)
                    .filter(|s| s.lifecycle.accepts(previous.operation, s.backend_released))
                else {
                    return false;
                };

                let owner = previous.transport_attempt.as_ref().unwrap();
                starts.register(owner);
                session.transport_owner = Some(owner.clone());
                self.replacement_retired
                    .lock()
                    .unwrap()
                    .remove(&input.session);
                session.handle = replacement_handle;
                session.authorization = replacement_authorization.clone();
                session.input_enabled = input_enabled;
                session.source_index = replacement_source_index;
                session.source_name = replacement_source_name.clone();
                session.width = input.width;
                session.height = input.height;
                session.fps_target = input.fps;
                session.quality_state = settled_quality_state.into();
                session.encoder_experiment = replacement_encoder_experiment;
                session.terminal_error = None;
                session.terminal_since = None;
                true
            })
            .unwrap_or(false);
        if !committed {
            self.discard_backend(replacement_handle);
            self.cleanup_owned_transport(previous.transport_attempt.as_ref().unwrap());
            return Err(format!(
                "session {} ended during reconfigure",
                input.session
            ));
        }
        Ok(ReconfigureStreamOutput {
            session: input.session,
            width: input.width,
            height: input.height,
            fps: input.fps,
            quality_state: settled_quality_state.into(),
            encoder_experiment: Some(replacement_encoder_experiment),
            // Echo the accepted source only for switch requests so legacy
            // responses keep their exact old shape.
            source_index: input.source_index.map(|_| replacement_source_index),
            source_name: input.source_index.map(|_| replacement_source_name.clone()),
        })
    }

    async fn wait_for_first_frame(&self, handle: u32) -> Result<(), String> {
        let deadline = Instant::now() + STARTUP_FIRST_FRAME_TIMEOUT;
        loop {
            let stats = self.backend.stats(handle)?;
            // `running` only means that the capture backend accepted the
            // start request. It does not prove that capture, encode, and the
            // media socket produced a frame. Opening the viewer on that state
            // creates a black stream that the recovery loop cannot distinguish
            // from a healthy session. Require the first packet instead.
            if stats.first_send_ms > 0 {
                return Ok(());
            }
            if let Some(error) = stats.error.filter(|error| !error.is_empty()) {
                return Err(error);
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "no video frame produced within {}s",
                    STARTUP_FIRST_FRAME_TIMEOUT.as_secs()
                ));
            }
            tokio::time::sleep(STARTUP_STATS_INTERVAL).await;
        }
    }

    /// Accept loop — runs until the process exits.
    pub async fn run(self: std::sync::Arc<Self>, listener: TcpListener) {
        loop {
            // 연결당 허가 하나(F08). 허가가 바닥나면 accept를 잠시 멈춰
            // 무인증 소켓의 작업 스폰을 제한하고, 연결 작업이 끝나면 반납된다.
            let permit = match std::sync::Arc::clone(&self.conn_permits)
                .acquire_owned()
                .await
            {
                Ok(permit) => permit,
                Err(_) => continue,
            };
            match listener.accept().await {
                Ok((sock, _)) => {
                    let server = self.clone();
                    tokio::spawn(async move {
                        let peer = sock
                            .peer_addr()
                            .map(|a| a.ip().to_string())
                            .unwrap_or_default();
                        handle_conn(sock, &server, &peer).await;
                        drop(permit);
                    });
                }
                Err(e) => {
                    eprintln!("control accept error: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Snapshot for the Tauri UI (`get_status` command reuses this).
    pub fn snapshot(&self) -> StatusView {
        let paired = self.pairing.list_device_views();
        let now = Instant::now();
        // Sample external backends without the state lock. A callback may stop
        // or revoke a session; re-check its handle when applying the sample.
        let handles: Vec<_> = self
            .sessions
            .lock()
            .unwrap()
            .live
            .iter()
            .map(|(id, s)| (*id, s.handle))
            .collect();
        let mut sampled: HashMap<_, _> = handles
            .into_iter()
            .map(|(id, handle)| {
                let metrics = self.backend.stats(handle).unwrap_or_else(|_| StatsInfo {
                    state: "stopped".into(),
                    encoder_experiment_requested: "auto".into(),
                    encoder_experiment_applied: "rateControl".into(),
                    capture_backend: "unknown".into(),
                    media_transport: "unknown".into(),
                    encoder_mode: "unknown".into(),
                    encoder_id: "unknown".into(),
                    encoder_preset: "unknown".into(),
                    encoder_profile: "unknown".into(),
                    quality_adaptation_last_status: "not_checked".into(),
                    error: Some("backend stats unavailable".into()),
                    ..StatsInfo::default()
                });
                (id, (handle, metrics))
            })
            .collect();
        let (mut sessions, expired) = {
            let mut state = self.sessions.lock().unwrap();
            let mut sessions = Vec::with_capacity(state.live.len());
            let mut expired_ids = Vec::new();

            for (id, s) in &mut state.live {
                let Some((handle, mut metrics)) = sampled.remove(id) else {
                    continue;
                };
                if handle != s.handle {
                    continue;
                }

                if let Some(error) = &s.terminal_error {
                    metrics.state = "stopped".into();
                    metrics.fps = 0;
                    metrics.kbps = 0;
                    metrics.pending_frame = 0;
                    metrics.error = Some(error.clone());
                }

                // Android Back/close is an intentional viewer action. Some
                // backends surface it through their error field, but the
                // control contract must expose it as an ordinary stop.
                if metrics.error.as_deref() == Some("viewer closed stream") {
                    metrics.state = "stopped".into();
                }

                let terminal = matches!(metrics.state.as_str(), "error" | "stopped" | "unknown");
                if terminal {
                    let terminal_since = s.terminal_since.get_or_insert(now);
                    if now.duration_since(*terminal_since) >= TERMINAL_SESSION_RETENTION {
                        expired_ids.push(*id);
                        continue;
                    }
                } else {
                    s.terminal_since = None;
                }

                sessions.push(Self::session_view(*id, s, metrics, &paired));
            }

            let expired = expired_ids
                .into_iter()
                .filter_map(|id| {
                    if !self.reconfiguring.lock().unwrap().insert(id) {
                        return None;
                    }
                    state.live.get(&id).cloned().map(|session| (id, session))
                })
                .collect::<Vec<_>>();
            (sessions, expired)
        };

        let mut removed_count = 0;
        for (id, session) in expired {
            Self::invalidate_session_source(&session);
            let _cleanup_guard = ReconfigureGuard {
                in_flight: &self.reconfiguring,
                retired: &self.replacement_retired,
                session: id,
            };
            if !session.backend_released {
                if let Err(error) = self.retire_session_capture(id, session.handle, None) {
                    eprintln!(
                        "failed to release terminal session {}: {error}",
                        session.handle
                    );
                    sessions.push(Self::session_view(
                        id,
                        &session,
                        StatsInfo {
                            state: "stopped".into(),
                            error: session.terminal_error.clone().or(Some(error)),
                            ..StatsInfo::default()
                        },
                        &paired,
                    ));
                    continue;
                }
            }
            let removed = {
                let mut state = self.sessions.lock().unwrap();
                if state.live.get(&id).is_some_and(|current| {
                    current.handle == session.handle && current.lifecycle == session.lifecycle
                }) {
                    state.live.remove(&id);
                    true
                } else {
                    false
                }
            };
            if removed {
                self.cleanup_registered_transport(session.transport_owner.as_ref());
                removed_count += 1;
            }
        }
        self.maybe_lock_after_teardown(removed_count);
        self.refresh_curtain();

        StatusView { sessions }
    }

    /// SessionView construction for one live session; the 1:1 metrics
    /// plumbing lives here so `snapshot` stays focused on retention.
    fn session_view(
        id: u32,
        s: &Session,
        metrics: StatsInfo,
        paired: &[crate::pairing::PairedDeviceView],
    ) -> SessionView {
        SessionView {
            device_name: s.device_id.as_ref().and_then(|id| {
                paired
                    .iter()
                    .find(|d| &d.device_id == id)
                    .map(|d| d.name.clone())
            }),
            session: id,
            source_index: s.source_index,
            source_name: s.source_name.clone(),
            viewer_addr: s.viewer_addr.clone(),
            width: s.width,
            height: s.height,
            quality_state: s.quality_state.clone(),
            udp_stability: s.udp_stability.clone(),
            input_enabled: s.input_enabled,
            input_rate_hz: s.input_rate_hz,
            stats: metrics,
        }
    }

    /// 세션 범위 명령의 소유 검사(F04). 인증된 장치(Some)는 자기 세션에만
    /// 접근할 수 있고, 호스트 내부·테스트 경로(None)는 기존처럼 모든 세션을
    /// 다룬다. 남의 세션은 "no such session"으로 답해 존재 여부를 노출하지
    /// 않는다 — 세션 id는 재사용되지 않으므로 검사 뒤 세션이 끝나더라도
    /// 안전하다.
    fn session_owned_by(&self, session: u32, authenticated_device: Option<&str>) -> bool {
        let Some(device) = authenticated_device else {
            return true;
        };
        self.sessions
            .lock()
            .unwrap()
            .live
            .get(&session)
            .is_some_and(|s| s.device_id.as_deref() == Some(device))
    }

    /// 장치 범위 상태 스냅샷(F04). 인증된 장치는 자기 세션의 상태만 보고,
    /// 호스트 내부 경로(None)는 기존처럼 전체를 본다.
    fn scoped_snapshot(&self, authenticated_device: Option<&str>) -> StatusView {
        let mut view = self.snapshot();
        let Some(device) = authenticated_device else {
            return view;
        };
        let owned: std::collections::HashSet<u32> = {
            let st = self.sessions.lock().unwrap();
            st.live
                .iter()
                .filter(|(_, s)| s.device_id.as_deref() == Some(device))
                .map(|(id, _)| *id)
                .collect()
        };
        view.sessions.retain(|s| owned.contains(&s.session));
        view
    }

    pub fn input_permission(&self) -> Result<bool, String> {
        self.backend.input_permission()
    }

    pub fn platform(&self) -> &'static str {
        self.backend.platform()
    }

    pub fn request_input_permission(&self) -> Result<bool, String> {
        self.backend.request_input_permission()
    }

    pub fn screen_permission(&self) -> Result<bool, String> {
        self.backend.screen_permission()
    }

    pub fn set_session_input(&self, session_id: u32, enabled: bool) -> Result<(), String> {
        let _operation = self
            .source_operations
            .enter()
            .ok_or("Host is shutting down")?;
        let _input_change = self.input_changes.lock().unwrap();
        let (handle, access) = {
            let mut state = self.sessions.lock().unwrap();
            let session = state
                .live
                .get_mut(&session_id)
                .filter(|session| !session.backend_released && !session.lifecycle.is_stopped())
                .ok_or_else(|| format!("no such session {session_id}"))?;
            // Host denial is a logical session decision, even when the old
            // native handle is gone or its disable operation will fail.
            if !enabled {
                session.input_enabled = false;
            }
            (
                session.handle,
                session
                    .authorization
                    .as_ref()
                    .and_then(|auth| auth.access.clone()),
            )
        };
        if self.replacement_retired.lock().unwrap().get(&session_id) == Some(&handle) {
            return if enabled {
                Err("input enable requires a live replacement handle".into())
            } else {
                Ok(())
            };
        }
        let source = access
            .as_ref()
            .map(|access| access.lease.enter().ok_or("source authorization revoked"))
            .transpose()?;
        let applied = self.backend.set_input_enabled(handle, enabled);
        drop(source);
        if let Err(error) = applied {
            if !enabled {
                // A live backend may retain its enabled flag after an error.
                // Retire/fence actual native authority, not only the UI flag.
                if let Some(access) = &access {
                    access.lease.invalidate();
                }
                let retirement = self.force_stop_session(session_id);
                if let Some(access) = &access {
                    access.lease.wait_idle();
                }
                return Err(format!("input disable failed; session access fenced: {error}; retirement: {retirement:?}"));
            }
            return Err(error);
        }
        let mut state = self.sessions.lock().unwrap();
        let session = state
            .live
            .get_mut(&session_id)
            .filter(|session| {
                session.handle == handle
                    && !session.backend_released
                    && !session.lifecycle.is_stopped()
                    && access.as_ref().is_none_or(|access| access.lease.current())
            })
            .ok_or_else(|| format!("session {session_id} ended while changing input"))?;
        session.input_enabled = enabled;
        Ok(())
    }

    pub fn set_session_quality(&self, session_id: u32, quality: Option<f32>) -> Result<(), String> {
        let _operation = self
            .source_operations
            .enter()
            .ok_or("Host is shutting down")?;
        if let Some(value) = quality {
            if !(0.25..=0.5).contains(&value) {
                return Err("quality override must be between 0.25 and 0.50".into());
            }
        }
        let handle = {
            let state = self.sessions.lock().unwrap();
            state
                .live
                .get(&session_id)
                .filter(|session| !session.backend_released && !session.lifecycle.is_stopped())
                .map(|session| session.handle)
                .ok_or_else(|| format!("no such session {session_id}"))?
        };
        self.backend.set_quality_override(handle, quality)
    }

    /// Operator-forced termination: stop the capture session and tell the
    /// still-live viewer (LCT1 code 2) so it closes its window and shows why
    /// instead of waiting for a media timeout or auto-restarting.
    pub fn force_stop_session(&self, session_id: u32) -> Result<(), String> {
        let _operation = self
            .source_operations
            .enter()
            .ok_or("Host is shutting down")?;
        // Publish the tombstone before any callback can re-enter and finish a
        // replacement. A failed backend stop must not revive authorization.
        let (handle, device, owner) = {
            let mut state = self.sessions.lock().unwrap();
            let session = state
                .live
                .get_mut(&session_id)
                .filter(|s| !s.backend_released)
                .ok_or_else(|| format!("no such session {session_id}"))?;
            if let Some(access) = session
                .authorization
                .as_ref()
                .and_then(|a| a.access.as_ref())
            {
                access.lease.invalidate();
            }
            session.lifecycle.stop();
            session.input_enabled = false;
            session.terminal_since = Some(Instant::now());
            session.terminal_error = Some("host operator stopped the stream".into());
            session.backend_released = true;
            (
                session.handle,
                session.device_id.clone(),
                session.transport_owner.clone(),
            )
        };
        let _ = self.backend.set_input_enabled(handle, false);
        let result = self.retire_session_capture(session_id, handle, Some(2));
        if result.is_err()
            && self
                .retire_session_capture(session_id, handle, None)
                .is_err()
        {
            // Keep failed cleanup visible and retryable, but terminal policy
            // must continue to reject replacement/input operations.
            if let Some(session) = self.sessions.lock().unwrap().live.get_mut(&session_id) {
                if session.handle == handle {
                    session.backend_released = false;
                }
            }
        }
        self.cleanup_registered_transport(owner.as_ref());
        self.audit_log(
            "session_stopped",
            json!({"session": session_id, "device": device, "reason": "operator_forced"}),
        );
        result
    }

    /// A viewer restart can leave the old capture handle alive until its TCP
    /// write notices the closed socket. Do not allow two hosts to push into
    /// the same viewer endpoint: their H.264 AU ids would interleave and the
    /// receiver would correctly enter keyframe recovery over and over.
    /// `device`가 Some이면 그 장치의 세션만 치운다(F04) — 시작 요청이 주장한
    /// viewerIps로 남의 세션을 끝내지 못하게 한다. None(호스트 내부 경로)은
    /// 기존 동작 그대로다.
    fn stop_sessions_for_viewer(&self, viewer_addr: &str, device: Option<&str>) {
        let stale = {
            let mut state = self.sessions.lock().unwrap();
            let ids: Vec<u32> = state
                .live
                .iter()
                .filter_map(|(id, session)| {
                    if session.viewer_addr != viewer_addr {
                        return None;
                    }
                    if let Some(device) = device {
                        if session.device_id.as_deref() != Some(device) {
                            return None;
                        }
                    }
                    Some(*id)
                })
                .collect();
            ids.into_iter()
                .filter_map(|id| state.live.remove(&id).map(|session| (id, session)))
                .collect::<Vec<_>>()
        };

        for (id, session) in stale {
            Self::invalidate_session_source(&session);
            if !session.backend_released {
                if let Err(error) = self.retire_session_capture(id, session.handle, None) {
                    eprintln!(
                        "failed to stop stale viewer session {}: {error}",
                        session.handle
                    );
                }
            }
            self.cleanup_registered_transport(session.transport_owner.as_ref());
        }
    }

    /// Validate a startStream request up to transport candidate selection.
    /// Every rejection path — shape, capture backend, encoder experiment,
    /// media transport, UDP stability, split gating, AOAP — lives here in
    /// input order so the dispatch arm stays focused on the attempt loop.
    async fn plan_start(&self, input: &StartStreamInput) -> Result<StartPlan, String> {
        validate_stream_shape(input.width, input.height, input.fps)?;
        // The media path has no plaintext mode. A missing or malformed key is
        // rejected up front so no capture session can start unsealed.
        let media_key = match input.media_key.as_deref() {
            Some(key) => decode_media_key(key)?,
            None => return Err("media encryption key is required — update the viewer app".into()),
        };
        if !self
            .backend
            .supports_capture_backend(&input.capture_backend)
        {
            return Err("unsupported capture backend".into());
        }
        // 프라이버시 커튼이 켜져 있으면 CGDisplayStream으로 시작하지
        // 않는다(M6). 창 제외는 SCK 필터에만 있고 CGDisplayStream은 디스플레이
        // 전체를 캡처하므로, 검은 커튼 오버레이가 그대로 스트리밍된다.
        if input.capture_backend == "cgDisplayStream"
            && self.settings.get().is_some_and(|s| s.privacy_curtain())
        {
            return Err("privacy curtain requires the screenCaptureKit capture backend".into());
        }
        let advertised = advertised_encoder_experiments(self.backend.encoder_experiments()?);
        let split_diagnostic_enabled =
            std::env::var("LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC").is_ok_and(|value| value == "1");
        if !encoder_experiment_is_startable(
            &advertised,
            input.encoder_experiment,
            split_diagnostic_enabled,
        ) {
            return Err(format!(
                "unsupported encoder experiment: {}",
                input.encoder_experiment.as_str()
            ));
        }
        let source = resolve_display(
            &self.backend.list_displays()?,
            input.source_id.as_deref(),
            Some(input.source_index),
        )?;
        let name = source.name.clone();
        let Some(transport) = normalize_media_transport(&input.media_transport) else {
            return Err(format!(
                "unsupported media transport: {}",
                input.media_transport
            ));
        };
        let Some(content_mode) = normalize_content_mode(&input.content_mode) else {
            return Err(format!("unsupported content mode: {}", input.content_mode));
        };
        if input.udp_stability.is_some() && !matches!(transport, "udp" | "auto") {
            return Err("UDP 안정성 설정은 UDP 또는 자동 전송에서만 사용할 수 있습니다".into());
        }
        let udp_stability = resolve_udp_stability(
            input.udp_stability.as_ref(),
            &host_udp_stability_capabilities(),
        )?;
        validate_split_start(input, transport)?;

        // Do not claim a normal Android USB device merely because a
        // cable was attached. AOAP negotiation is an explicit stream
        // request; `auto` may fall back to Wi-Fi, while an explicit
        // USB request reports the negotiation failure to the viewer.
        if input.encoder_experiment != EncoderExperiment::SplitVertical
            && matches!(transport, "usb" | "auto")
        {
            if let Err(error) = crate::aoap_control::ensure_usb_accessory().await {
                if transport == "usb" {
                    return Err(error);
                }
                eprintln!("AOAP unavailable; continuing with transport fallback: {error}");
            }
        }

        Ok(StartPlan {
            source,
            name,
            transport,
            content_mode,
            udp_stability,
            media_key,
        })
    }

    /// Tests may supply a trusted device identity; production TCP/AOAP paths
    /// always use dispatch_with_authorization with the original token proof.
    #[cfg(test)]
    pub(crate) async fn dispatch(
        &self,
        command: &str,
        args: serde_json::Value,
        viewer_ip: &str,
        authenticated_device: Option<&str>,
    ) -> serde_json::Value {
        // Existing lifecycle fixtures use trusted local calls. Production
        // authorization regressions call dispatch_with_authorization directly.
        let source_command = matches!(command, "startStream" | "getCatalog" | "reconfigureStream");
        let fixture_device = authenticated_device.unwrap_or("test-local-host");
        if source_command
            && authenticated_device.is_none()
            && !self.pairing.is_device_paired(fixture_device)
        {
            let offer = self.pairing.begin_pairing("127.0.0.1", 7777);
            self.pairing
                .pair_by_code(&offer.code, fixture_device, "test local")
                .unwrap();
        }
        let authorization = if source_command {
            self.pairing.authorization(fixture_device)
        } else {
            authenticated_device.and_then(|device| self.pairing.authorization(device))
        };
        if source_command {
            let displays = self.backend.list_displays().unwrap_or_default();
            if self
                .pairing
                .list_device_views()
                .iter()
                .find(|d| d.device_id == fixture_device)
                .is_some_and(|d| d.source_grants.review_required)
            {
                let _ = self.pairing.update_source_grants(
                    fixture_device,
                    displays
                        .iter()
                        .filter_map(|d| d.source_id.clone())
                        .collect(),
                );
            }
            if let Some(auth) = &authorization {
                self.pairing.remember_catalog(auth, &displays);
                let accesses: HashMap<_, _> = displays
                    .iter()
                    .filter_map(|d| {
                        d.source_id
                            .as_deref()
                            .and_then(|id| self.pairing.source_authorization(auth, id).ok())
                            .map(|auth| (d.index, auth))
                    })
                    .collect();
                let mut sessions = self.sessions.lock().unwrap();
                for session in sessions
                    .live
                    .values_mut()
                    .filter(|s| s.authorization.is_none())
                {
                    session.authorization = accesses.get(&session.source_index).cloned();
                }
            }
        }
        self.dispatch_with_authorization(
            command,
            args,
            viewer_ip,
            authenticated_device,
            authorization.as_ref(),
        )
        .await
    }

    pub(crate) async fn dispatch_with_authorization(
        &self,
        command: &str,
        args: serde_json::Value,
        viewer_ip: &str,
        authenticated_device: Option<&str>,
        request_authorization: Option<&crate::pairing::Authorization>,
    ) -> serde_json::Value {
        let Some(_source_operation) = self.source_operations.enter() else {
            return err("Host is shutting down");
        };
        if self
            .pairing
            .with_authorization(request_authorization, || ())
            .is_none()
        {
            return err("unauthorized");
        }
        match command {
            "requestUsb" => match crate::aoap_control::ensure_usb_accessory().await {
                Ok(()) => ok(json!({ "attached": true })),
                Err(error) => err(&error),
            },
            "beginPairing" => {
                // Local operator/diagnostic entry point. This exposes the same
                // short-lived pairing offer as the Tauri pairing window, but
                // it is never reachable from a LAN or tailnet peer.
                if !is_loopback_peer(viewer_ip) {
                    return err("unauthorized");
                }
                let Some(host_ip) = crate::local_lan_ip() else {
                    return err("no LAN interface found");
                };
                let port = self.control_port.load(Ordering::Acquire);
                ok(self.pairing.begin_pairing(&host_ip, port))
            }
            "pair" => {
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct PairArgs {
                    #[serde(default)]
                    offer_id: Option<String>,
                    #[serde(default)]
                    secret: Option<String>,
                    code: String,
                    device_id: String,
                    #[serde(default)]
                    device_name: String,
                }
                let input: PairArgs = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(_) => return err("pairing failed"),
                };
                let res = match (input.offer_id.as_deref(), input.secret.as_deref()) {
                    (Some(offer_id), Some(secret)) => self.pairing.pair(
                        offer_id,
                        secret,
                        &input.code,
                        &input.device_id,
                        &input.device_name,
                    ),
                    (None, None) => {
                        self.pairing
                            .pair_by_code(&input.code, &input.device_id, &input.device_name)
                    }
                    _ => Err(crate::pairing::PairingServerError::PairingFailed),
                };
                match res {
                    Ok(token) => ok(json!({ "token": token })),
                    // 승인 기반 페어링: 시크릿은 맞지만 Mac 사용자의 허용이
                    // 아직 없다. 뷰어는 이 상태를 보고 폴링을 계속한다.
                    Err(crate::pairing::PairingServerError::Pending) => {
                        ok(json!({ "status": "pending" }))
                    }
                    Err(crate::pairing::PairingServerError::Rejected) => err("pairing rejected"),
                    Err(_) => err("pairing failed"),
                }
            }
            "getCatalog" => {
                let displays = match self.backend.list_displays() {
                    Ok(displays) => displays,
                    Err(e) => return err(&e),
                };
                let displays: Vec<_> = displays
                    .into_iter()
                    .filter(|display| {
                        let Some(source) = display.source_id.as_deref() else {
                            return false;
                        };
                        request_authorization
                            .is_some_and(|auth| self.pairing.source_allowed(auth, source))
                    })
                    .collect();
                if let Some(auth) = request_authorization {
                    self.pairing.remember_catalog(auth, &displays);
                }
                let encoder_experiments = match self.backend.encoder_experiments() {
                    Ok(experiments) => advertised_encoder_experiments(experiments),
                    Err(e) => return err(&e),
                };
                ok(CatalogView {
                    platform: self.backend.platform().into(),
                    capture_backends: self.backend.capture_backends(),
                    media_host: crate::local_lan_ip().filter(|address| {
                        address
                            .parse::<std::net::Ipv4Addr>()
                            .is_ok_and(|address| address.is_private())
                    }),
                    displays,
                    encoder_experiments,
                    reconfigure_encoder_experiment: Some(true),
                    reconfigure_source: Some(true),
                    udp_stability_capabilities: Some(host_udp_stability_capabilities()),
                })
            }
            "startStream" => {
                let request = self.starting.lock().unwrap().begin();
                let _request_guard = StartRequestGuard {
                    starts: &self.starting,
                    request,
                };

                let mut authorization = request_authorization.cloned();
                if authenticated_device.is_some() && authorization.is_none() {
                    return err("unauthorized");
                }
                let mut input: StartStreamInput = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(e) => return err(&format!("bad args: {e}")),
                };
                if input.source_id.is_none() {
                    let Some(auth) = authorization.as_ref() else {
                        return err("source_access_denied");
                    };
                    input.source_id = match self.pairing.catalog_source(auth, input.source_index) {
                        Ok(id) => Some(id),
                        Err(error) => return err(&error),
                    };
                }
                let Some(auth) = authorization.as_ref() else {
                    return err("source_access_denied: Host에서 이 기기의 화면 접근을 허용한 뒤 다시 시도하세요");
                };
                authorization = match self
                    .pairing
                    .source_authorization(auth, input.source_id.as_deref().unwrap())
                {
                    Ok(auth) => Some(auth),
                    Err(error) => return err(&error),
                };
                let plan = match self.plan_start(&input).await {
                    Ok(plan) => plan,
                    Err(error) => return err(&error),
                };
                if self
                    .pairing
                    .with_authorization(authorization.as_ref(), || ())
                    .is_none()
                {
                    return err("source_access_denied");
                }

                // A non-bypassable VPN can route a local control connection
                // through a LAN subnet router, so its TCP peer is not always
                // the viewer's physical Wi-Fi address. Consider claimed
                // addresses only when they are private and either on the
                // peer's /24 or the authenticated control peer is a tailnet
                // CGNAT address. USB is deliberately different: adb forward
                // terminates on the Host, so the only valid media peer is the
                // Host loopback address.
                let usb_control = viewer_ip == "usb";
                let mut wifi_candidates = input
                    .viewer_ips
                    .iter()
                    .take(4)
                    .filter(|candidate| same_private_lan_candidate(candidate, viewer_ip))
                    .cloned()
                    .collect::<Vec<_>>();
                if !usb_control
                    && !wifi_candidates
                        .iter()
                        .any(|candidate| candidate == viewer_ip)
                {
                    wifi_candidates.push(viewer_ip.to_owned());
                }
                let attempts = if input.encoder_experiment == EncoderExperiment::SplitVertical {
                    build_attempts("udp", &wifi_candidates)
                } else if usb_control {
                    build_attempts("usb", &[])
                } else {
                    build_attempts(plan.transport, &wifi_candidates)
                };
                let mut last_error = None;
                let mut started = None;
                // 첫 프레임 대기(최대 5초) 사이에 장치가 철회됐음을 표시한다.
                // 이 경우 다른 후보 주소를 시도하지 않고 unauthorized로 끝낸다.
                let mut revoked_during_start = false;
                // 커튼 창은 필터 생성(backend.start)보다 먼저 존재해야 캡처에서
                // 제외된다 — 세션 등록 후에 띄우면 오버레이 자체가
                // 스트리밍된다(H1). 시작이 모두 실패하면 아래에서 되돌린다.
                self.prepare_curtain_for_start();
                for (candidate, transport) in attempts {
                    let viewer_addr = format!("{candidate}:{}", input.viewer_port);
                    let claimed = self.starting.lock().unwrap().claim(
                        request,
                        authenticated_device,
                        &viewer_addr,
                    );
                    let Some(attempt) = claimed else {
                        self.refresh_curtain();
                        return err("start superseded or transport operation in progress");
                    };
                    let setup = self.with_start_transport(&attempt, || {
                        self.stop_sessions_for_viewer(&viewer_addr, authenticated_device);
                        self.prepare_owned_transport(&attempt, transport, input.viewer_port)
                    });
                    match setup {
                        Some(Ok(())) => {}
                        Some(Err(error)) => {
                            last_error = Some(error);
                            continue;
                        }
                        None => return err("start superseded"),
                    }
                    match self.backend.start(
                        plan.source.index,
                        &candidate,
                        input.viewer_port,
                        input.width,
                        input.height,
                        input.fps,
                        &input.capture_backend,
                        transport,
                        plan.content_mode,
                        input.encoder_experiment,
                        &plan.udp_stability,
                        &plan.media_key,
                        authorization.as_ref().and_then(|auth| auth.access.as_ref()),
                    ) {
                        Ok(handle) => match self.wait_for_first_frame(handle).await {
                            Ok(()) => {
                                // 등록 직전 재검사(F02): 첫 프레임 대기 동안의
                                // 철회를 잡는다. stop_sessions_for_device는
                                // 등록된 세션만 보므로, 철회 뒤에 등록하면
                                // 철회된 장치의 세션이 살아 남는다.
                                if authenticated_device
                                    .is_some_and(|device| !self.pairing.is_device_paired(device))
                                {
                                    self.discard_backend(handle);
                                    self.cleanup_owned_transport(&attempt);
                                    revoked_during_start = true;
                                    break;
                                }
                                started = Some((handle, candidate, transport, attempt));
                                break;
                            }
                            Err(error) => {
                                self.discard_backend(handle);
                                self.cleanup_owned_transport(&attempt);
                                last_error = Some(format!(
                                    "{candidate} ({transport}) startup failed: {error}"
                                ));
                            }
                        },
                        Err(e) => {
                            self.cleanup_owned_transport(&attempt);
                            last_error = Some(format!("{candidate} ({transport}): {e}"));
                        }
                    }
                }
                match started {
                    Some((handle, candidate, transport, attempt)) => {
                        let viewer_addr = format!("{candidate}:{}", input.viewer_port);
                        // Viewing permission never grants remote input.
                        let input_enabled = false;
                        let session_id = self
                            .pairing
                            .with_authorization(authorization.as_ref(), || {
                                let mut starts = self.starting.lock().unwrap();
                                if !starts.accepts(&attempt) {
                                    return None;
                                }
                                let mut st = self.sessions.lock().unwrap();
                                let id = st.next;
                                st.next += 1;
                                starts.register(&attempt);
                                st.live.insert(
                                    id,
                                    Session {
                                        handle,
                                        source_index: plan.source.index,
                                        source_name: plan.name.clone(),
                                        device_id: authenticated_device.map(str::to_owned),
                                        width: input.width,
                                        height: input.height,
                                        fps_target: input.fps,
                                        quality_state: "native".into(),
                                        capture_backend: input.capture_backend.clone(),
                                        content_mode: plan.content_mode.into(),
                                        encoder_experiment: input.encoder_experiment,
                                        viewer_addr: viewer_addr.clone(),
                                        viewer_port: input.viewer_port,
                                        media_transport: transport.into(),
                                        udp_stability: (transport == "udp")
                                            .then(|| plan.udp_stability.clone()),
                                        media_key: plan.media_key,
                                        input_enabled,
                                        input_rate_hz: input.fps.saturating_mul(2).clamp(30, 240),
                                        terminal_since: None,
                                        terminal_error: None,
                                        backend_released: false,
                                        lifecycle: Lifecycle::default(),
                                        transport_owner: Some(attempt.clone()),
                                        authorization: authorization.clone(),
                                    },
                                );
                                Some(id)
                            })
                            .flatten();
                        let Some(session_id) = session_id else {
                            self.discard_backend(handle);
                            self.cleanup_owned_transport(&attempt);
                            self.refresh_curtain();
                            return err("unauthorized or start superseded");
                        };
                        // 커튼 설정이 켜져 있으면 세션 시작과 함께 오버레이를
                        // 띄운다(창이 필터 생성보다 먼저 있어야 캡처에서
                        // 제외된다).
                        self.refresh_curtain();
                        // 세션 시작 시 입력 중재(U4b): 마지막 세션이 이긴다.
                        // 새 세션의 입력이 실제로 켜졌을 때만 다른 라이브
                        // 세션의 입력을 끊는다 — OS 권한이 없어 입력이 꺼진
                        // 세션 시작은 기존 세션을 건드리지 않는다.
                        if input_enabled {
                            let displaced: Vec<(u32, u32)> = {
                                let mut st = self.sessions.lock().unwrap();
                                let still_enabled = st.live.get(&session_id).is_some_and(|s| {
                                    s.handle == handle && s.input_enabled && !s.backend_released
                                });
                                st.live
                                    .iter_mut()
                                    .filter_map(|(id, session)| {
                                        if !still_enabled {
                                            return None;
                                        }
                                        if *id == session_id || !session.input_enabled {
                                            return None;
                                        }
                                        session.input_enabled = false;
                                        Some((*id, session.handle))
                                    })
                                    .collect()
                            };
                            for (_, other_handle) in &displaced {
                                let _ = self.backend.set_input_enabled(*other_handle, false);
                            }
                            if !displaced.is_empty() {
                                let from: Vec<u32> = displaced.iter().map(|(id, _)| *id).collect();
                                self.audit_log(
                                    "input_reassigned",
                                    json!({ "from": from, "to": session_id }),
                                );
                            }
                        }
                        self.audit_log(
                            "session_started",
                            json!({
                                "session": session_id,
                                "device": authenticated_device.unwrap_or("unknown"),
                                "viewer": viewer_addr.clone(),
                                "transport": transport,
                                "source": plan.name,
                            }),
                        );
                        ok(StartStreamOutput {
                            session: session_id,
                            width: input.width,
                            height: input.height,
                            fps: input.fps,
                            quality_state: "native".into(),
                            udp_stability: (transport == "udp")
                                .then_some(plan.udp_stability.clone()),
                        })
                    }
                    None => {
                        // 시작 전에 띄운 커튼을 되돌린다(세션이 생기지 않았으므로
                        // 원하는 상태는 꺼짐이다).
                        self.refresh_curtain();
                        if revoked_during_start {
                            return err("unauthorized");
                        }
                        err(&format!(
                            "all viewer addresses failed: {}",
                            last_error.unwrap_or_else(|| "no viewer addresses".into())
                        ))
                    }
                }
            }
            "reconfigureStream" => {
                let input: ReconfigureStreamInput = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(e) => return err(&format!("bad args: {e}")),
                };
                // 인증된 장치는 자기 세션만 재구성할 수 있다(F04).
                if !self.session_owned_by(input.session, authenticated_device) {
                    return err(&format!("no such session {}", input.session));
                }
                match self.reconfigure_stream(input).await {
                    Ok(output) => ok(output),
                    Err(error) => err(&error),
                }
            }
            "stopStream" => {
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct StopArgs {
                    session: u32,
                    /// LCT1 wire code the host forwards to the viewer:
                    /// 2 = operator-forced, 3 = ordinary stop. Optional for
                    /// backward compatibility with older viewers.
                    #[serde(default)]
                    reason: Option<u8>,
                }
                let input: StopArgs = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(e) => return err(&format!("bad args: {e}")),
                };
                // 인증된 장치는 자기 세션만 종료할 수 있다(F04) — 남의 세션은
                // 없는 세션과 같은 오류로 답해 존재 여부를 노출하지 않는다.
                if !self.session_owned_by(input.session, authenticated_device) {
                    return err(&format!("no such session {}", input.session));
                }
                let removed = {
                    let mut st = self.sessions.lock().unwrap();
                    st.live.remove(&input.session)
                };
                match removed {
                    Some(s) => {
                        Self::invalidate_session_source(&s);
                        let result = if s.backend_released {
                            Ok(())
                        } else {
                            match input.reason {
                                Some(code) => {
                                    self.retire_session_capture(input.session, s.handle, Some(code))
                                }
                                None => self.retire_session_capture(input.session, s.handle, None),
                            }
                        };
                        match result {
                            Ok(()) => {
                                self.cleanup_registered_transport(s.transport_owner.as_ref());
                                self.audit_log(
                                    "session_stopped",
                                    json!({
                                        "session": input.session,
                                        "device": s.device_id,
                                        "reason": input.reason.unwrap_or(3),
                                    }),
                                );
                                self.maybe_lock_after_teardown(1);
                                self.refresh_curtain();
                                ok(json!({}))
                            }
                            Err(e) => err(&e),
                        }
                    }
                    None => err(&format!("no such session {}", input.session)),
                }
            }
            "getStatus" => ok(self.scoped_snapshot(authenticated_device)),
            "setClipboard" => self.handle_set_clipboard(args, authenticated_device),
            "getClipboard" => self.handle_get_clipboard(args, authenticated_device),
            // -- 파일 전송 v1 (docs/07 §20) --------------------------------
            // 모든 명령은 페어링으로 인증된 장치에만 허용되고, End를 제외한
            // 명령은 file_share 게이트(기본 꺼짐)를 통과해야 한다.
            "sendFileBegin" => {
                let Some(device) = authenticated_device else {
                    return err("unauthorized");
                };
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct BeginArgs {
                    name: String,
                    size: u64,
                }
                let input: BeginArgs = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(_) => return err("bad args"),
                };
                if !self.file_share_gate() {
                    return err("file share disabled");
                }
                match self
                    .file_transfers
                    .begin_incoming(device, &input.name, input.size)
                {
                    Ok(token) => {
                        self.audit_log(
                            "file_send_begin",
                            json!({ "device": device, "name": input.name, "size": input.size }),
                        );
                        ok(json!({ "fileToken": token }))
                    }
                    Err(error) => err(&error),
                }
            }
            "sendFileChunk" => {
                let Some(device) = authenticated_device else {
                    return err("unauthorized");
                };
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct ChunkArgs {
                    file_token: String,
                    data_base64: String,
                    offset: u64,
                }
                let input: ChunkArgs = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(_) => return err("bad args"),
                };
                if !self.file_share_gate() {
                    return err("file share disabled");
                }
                match self.file_transfers.append_incoming(
                    device,
                    &input.file_token,
                    &input.data_base64,
                    input.offset,
                ) {
                    Ok(written) => ok(json!({ "written": written })),
                    Err(error) => err(&error),
                }
            }
            "sendFileEnd" => {
                let Some(device) = authenticated_device else {
                    return err("unauthorized");
                };
                let file_token = match parse_file_token(args) {
                    Ok(token) => token,
                    Err(response) => return response,
                };
                // 게이트는 다시 검사하지 않는다 — 진행 중 전송의 마무리는
                // 게이트를 꺼도 완료할 수 있어야 .part 잔여물이 남지 않는다.
                match self.file_transfers.finish_incoming(device, &file_token) {
                    Ok((name, bytes)) => {
                        self.audit_log(
                            "file_received",
                            json!({ "device": device, "name": name, "bytes": bytes }),
                        );
                        ok(json!({
                            "path": crate::file_transfer::display_path(
                                &crate::file_transfer::sanitize_device_name(device),
                                &name,
                            )
                        }))
                    }
                    Err(error) => err(&error),
                }
            }
            "sendFileCancel" => {
                let Some(device) = authenticated_device else {
                    return err("unauthorized");
                };
                let file_token = match parse_file_token(args) {
                    Ok(token) => token,
                    Err(response) => return response,
                };
                // 게이트와 무관하게 받는다 — 실패한 전송의 스테이징 정리가
                // 게이트 토글에 막혀선 안 된다(만료 스윕의 즉시 버전).
                match self.file_transfers.cancel_incoming(device, &file_token) {
                    Ok(()) => ok(json!({})),
                    Err(error) => err(&error),
                }
            }
            "listShareQueue" => {
                if authenticated_device.is_none() {
                    return err("unauthorized");
                }
                if !self.file_share_gate() {
                    return err("file share disabled");
                }
                ok(json!({ "queue": self.file_transfers.queue_entries() }))
            }
            "fetchFileBegin" => {
                let Some(device) = authenticated_device else {
                    return err("unauthorized");
                };
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct FetchBeginArgs {
                    queue_id: String,
                }
                let input: FetchBeginArgs = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(_) => return err("bad args"),
                };
                if !self.file_share_gate() {
                    return err("file share disabled");
                }
                match self.file_transfers.begin_outgoing(device, &input.queue_id) {
                    Ok((token, name, size)) => {
                        ok(json!({ "fileToken": token, "name": name, "size": size }))
                    }
                    Err(error) => err(&error),
                }
            }
            "fetchFileChunk" => {
                let Some(device) = authenticated_device else {
                    return err("unauthorized");
                };
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct FetchChunkArgs {
                    file_token: String,
                    offset: u64,
                    length: u64,
                }
                let input: FetchChunkArgs = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(_) => return err("bad args"),
                };
                if !self.file_share_gate() {
                    return err("file share disabled");
                }
                match self.file_transfers.read_outgoing(
                    device,
                    &input.file_token,
                    input.offset,
                    input.length,
                ) {
                    Ok((data, size)) => ok(json!({ "data": data, "size": size })),
                    Err(error) => err(&error),
                }
            }
            "fetchFileEnd" => {
                let Some(device) = authenticated_device else {
                    return err("unauthorized");
                };
                let file_token = match parse_file_token(args) {
                    Ok(token) => token,
                    Err(response) => return response,
                };
                match self.file_transfers.finish_outgoing(device, &file_token) {
                    Ok(name) => {
                        self.audit_log("file_fetched", json!({ "device": device, "name": name }));
                        ok(json!({}))
                    }
                    Err(error) => err(&error),
                }
            }
            "fetchFileCancel" => {
                let Some(device) = authenticated_device else {
                    return err("unauthorized");
                };
                let file_token = match parse_file_token(args) {
                    Ok(token) => token,
                    Err(response) => return response,
                };
                match self.file_transfers.cancel_outgoing(device, &file_token) {
                    Ok(()) => ok(json!({})),
                    Err(error) => err(&error),
                }
            }
            _ => {
                // delegate stateless commands to the real rustra package (H02 path)
                match control_contract::host::host_package().invoke_json(command, args) {
                    Ok(v) => ok(v),
                    Err(e) => err(&e.to_string()),
                }
            }
        }
    }

    /// 토큰으로 페어링된 장치 ID를 찾는다. USB 제어 경로(aoap_control)에서
    /// 세션을 장치에 귀속시키기 위해 쓴다 — TCP 경로의
    /// `pairing.authorize_device`와 동일한 규칙이다.
    pub(crate) fn authorize_device_token(
        &self,
        token: &str,
    ) -> Option<crate::pairing::Authorization> {
        self.pairing.authenticate(token)
    }

    /// `setClipboard` 제어 명령(U5, docs/07 §20). 토큰 인증은 연결 레벨에서
    /// 이미 끝난 상태이고, 여기서 호스트 게이트를 추가로 요구한다 — 토글이
    /// 닫혀 있으면 모든 클립보드 명령을 거부한다(뷰어 측 게이트는 없다).
    /// 텍스트는 절대 감사 로그에 쓰지 않는다 — 접근 메타데이터만 남긴다.
    fn handle_set_clipboard(
        &self,
        args: serde_json::Value,
        device: Option<&str>,
    ) -> serde_json::Value {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SetClipboardArgs {
            text: Option<String>,
            image_base64: Option<String>,
        }
        let input: SetClipboardArgs = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return err(&format!("bad args: {e}")),
        };
        if !self.clipboard_share_enabled() {
            return err("clipboard share disabled");
        }
        let backend = self.clipboard_backend();
        match (input.text, input.image_base64) {
            (Some(text), None) => {
                if text.chars().count() > CLIPBOARD_MAX_CHARS {
                    return err("clipboard too large");
                }
                // 루프 방지: 뷰어가 호스트의 현재 내용과 같은 텍스트를 되돌려
                // 보내면(자기 반영 에코) 쓰지 않는다.
                if let Ok(current) = backend.read_text() {
                    if clipboard_sha256_hex(&current) == clipboard_sha256_hex(&text) {
                        return ok(json!({}));
                    }
                }
                match backend.write_text(&text) {
                    Ok(()) => {
                        self.audit_log(
                            "clipboard_write",
                            json!({
                                "device": device.unwrap_or("unknown"),
                                "bytes": text.len(),
                            }),
                        );
                        ok(json!({}))
                    }
                    Err(e) => err(&e),
                }
            }
            (None, Some(image_base64)) => {
                // PNG은 base64 문자 수 기준 상한(≈6MiB 원본).
                if image_base64.len() > CLIPBOARD_MAX_IMAGE_BASE64 {
                    return err("clipboard too large");
                }
                let png_bytes =
                    match base64::engine::general_purpose::STANDARD.decode(&image_base64) {
                        // 디코딩 결과도 상한 안인지 다시 확인한다.
                        Ok(bytes) if bytes.len() <= CLIPBOARD_MAX_IMAGE_BASE64 => bytes,
                        Ok(_) => return err("clipboard too large"),
                        Err(_) => return err("clipboard image is not valid base64"),
                    };
                // 루프 방지: 현재 클립보드 이미지와 같으면 쓰지 않는다.
                if let Ok(Some(current)) = backend.read_image_png() {
                    if current == png_bytes {
                        return ok(json!({}));
                    }
                }
                match backend.write_image_png(&png_bytes) {
                    Ok(()) => {
                        self.audit_log(
                            "clipboard_write",
                            json!({
                                "device": device.unwrap_or("unknown"),
                                "bytes": png_bytes.len(),
                                "kind": "image",
                            }),
                        );
                        ok(json!({}))
                    }
                    Err(e) => err(&e),
                }
            }
            _ => err("bad args: exactly one of text or imageBase64 is required"),
        }
    }

    /// `getClipboard` 제어 명령(U5). 뷰어가 마지막으로 본 해시를 보내면
    /// 호스트가 현재 클립보드 텍스트의 sha256과 비교해 unchanged로 짧게
    /// 끊낸다. 상한 초과는 set과 같은 경로로 거부한다. 실제 텍스트가
    /// 나가는 읽기만 감사 로그에 남긴다 — 2.5초 폴링의 unchanged 응답이
    /// 로그를 흔들지 않게 하기 위해서다.
    fn handle_get_clipboard(
        &self,
        args: serde_json::Value,
        device: Option<&str>,
    ) -> serde_json::Value {
        #[derive(serde::Deserialize)]
        struct GetClipboardArgs {
            hash: String,
        }
        let input: GetClipboardArgs = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return err(&format!("bad args: {e}")),
        };
        if !self.clipboard_share_enabled() {
            return err("clipboard share disabled");
        }
        let backend = self.clipboard_backend();
        let revision = backend.revision().ok().flatten();
        if revision.is_some_and(|revision| {
            self.clipboard_revision_cache
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|(cached, hash)| *cached == revision && *hash == input.hash)
        }) {
            return ok(json!({ "unchanged": true }));
        }
        let remember = |hash: &str| {
            let after = backend.revision().ok().flatten();
            let valid = revision.filter(|before| Some(*before) == after);
            *self.clipboard_revision_cache.lock().unwrap() =
                valid.map(|value| (value, hash.to_owned()));
        };
        let current = match backend.read_text() {
            Ok(text) => text,
            Err(e) => return err(&e),
        };
        if !current.is_empty() {
            if current.chars().count() > CLIPBOARD_MAX_CHARS {
                return err("clipboard too large");
            }
            let hash = clipboard_sha256_hex(&current);
            remember(&hash);
            if input.hash == hash {
                return ok(json!({ "unchanged": true }));
            }
            self.audit_log(
                "clipboard_read",
                json!({ "device": device.unwrap_or("unknown"), "bytes": current.len() }),
            );
            return ok(json!({ "unchanged": false, "text": current, "hash": hash }));
        }
        // 텍스트가 비어 있으면 이미지를 본다. 해시는 "i:" 접두사 + base64
        // 문자열의 sha256 — 뷰어가 같은 규칙으로 계산해 밀어 올린다.
        match backend.read_image_png() {
            Ok(Some(png)) => {
                let image_base64 = base64::engine::general_purpose::STANDARD.encode(&png);
                if image_base64.len() > CLIPBOARD_MAX_IMAGE_BASE64 {
                    return err("clipboard too large");
                }
                let hash = format!("i:{}", clipboard_sha256_hex(&image_base64));
                remember(&hash);
                if input.hash == hash {
                    return ok(json!({ "unchanged": true }));
                }
                self.audit_log(
                    "clipboard_read",
                    json!({
                        "device": device.unwrap_or("unknown"),
                        "bytes": png.len(),
                        "kind": "image",
                    }),
                );
                ok(json!({
                    "unchanged": false,
                    "imageBase64": image_base64,
                    "hash": hash,
                }))
            }
            Ok(None) => {
                // 빈 클립보드 — 빈 텍스트 해시로 unchanged 판정에 맡긴다.
                let hash = clipboard_sha256_hex("");
                remember(&hash);
                if input.hash == hash {
                    ok(json!({ "unchanged": true }))
                } else {
                    ok(json!({ "unchanged": false, "text": "", "hash": hash }))
                }
            }
            Err(e) => err(&e),
        }
    }
}

/// 연결 등록 해지 가드 — 연결 루프가 어떤 경로로 끝나도 레지스트리에서
/// 자기 알림 핸들을 지운다(레지스트리가 죽은 연결을 기억하지 않게).
struct AuthedConnLease<'a> {
    server: &'a ControlServer,
    device: String,
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl Drop for AuthedConnLease<'_> {
    fn drop(&mut self) {
        self.server
            .unregister_authed_conn(&self.device, &self.notify);
    }
}

/// 재구성 임계구역 가드(F02). reconfigure_stream이 시작 직전에 세션 id를
/// 집합에 넣고, 실패·성공 어느 경로로 끝나도 drop에서 뺀다. 첫 프레임 대기
/// 전체를 포함한 재구성 구간 내내 쥔다 — 중간에 풀었다면 두 번째 재구성이
/// 아직 스왑되지 않은 이전 핸들 스냅샷을 다시 읽게 된다.
struct StartRequestGuard<'a> {
    starts: &'a Mutex<Starts>,
    request: u64,
}
impl Drop for StartRequestGuard<'_> {
    fn drop(&mut self) {
        self.starts.lock().unwrap().finish(self.request);
    }
}
struct StartTransportGuard<'a> {
    starts: &'a Mutex<Starts>,
    attempt: &'a StartAttempt,
}
impl Drop for StartTransportGuard<'_> {
    fn drop(&mut self) {
        loop {
            let deferred = self
                .starts
                .lock()
                .unwrap()
                .end_transport_action(self.attempt);
            let Some((transport, port)) = deferred else {
                break;
            };
            cleanup_media_transport(&transport, port);
        }
    }
}

struct ReconfigureGuard<'a> {
    in_flight: &'a Mutex<std::collections::HashSet<u32>>,
    retired: &'a Mutex<HashMap<u32, u32>>,
    session: u32,
}

impl Drop for ReconfigureGuard<'_> {
    fn drop(&mut self) {
        self.retired.lock().unwrap().remove(&self.session);
        self.in_flight.lock().unwrap().remove(&self.session);
    }
}

#[derive(Debug)]
enum ControlLineReadError {
    TooLong,
    Io(std::io::Error),
}

/// 상한 있는 한 줄 읽기(F08). '\n' 전에 `cap`을 넘기면 Err — 호출자는
/// 연결을 끊는다. tokio Lines는 줄 길이 상한이 없어 인증 전 소켓이라도
/// '\n' 없는 입력을 무한히 버퍼링할 수 있으므로 fill_buf 루프로 직접 읽는다.
/// Ok(None)은 EOF이며, 소켓 오류와 상한 초과는 구분해 반환한다.
async fn read_bounded_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    cap: usize,
) -> Result<Option<String>, ControlLineReadError> {
    let mut line: Vec<u8> = Vec::new();
    loop {
        let available = reader.fill_buf().await.map_err(ControlLineReadError::Io)?;
        if available.is_empty() {
            return Ok(None);
        }
        if let Some(pos) = available.iter().position(|&byte| byte == b'\n') {
            if line.len() + pos > cap {
                return Err(ControlLineReadError::TooLong);
            }
            line.extend_from_slice(&available[..pos]);
            reader.consume(pos + 1);
            // tokio Lines와 같은 \r\n 관습을 맞춘다.
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
        }
        if line.len() + available.len() > cap {
            return Err(ControlLineReadError::TooLong);
        }
        let buffered = available.len();
        line.extend_from_slice(available);
        reader.consume(buffered);
    }
}

/// 다음 명령 줄을 읽는다. 무활동 15초(하프오픈 소켓 회수)와 연결 등록
/// 해지(장치 철회) 둘 다 종료 사유가 된다. 줄 상한 초과는 Err(연결 종료).
async fn read_authed_line(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    killed: Option<&std::sync::Arc<tokio::sync::Notify>>,
) -> Result<Option<String>, ControlLineReadError> {
    let cap = if killed.is_some() {
        COMMAND_LINE_LIMIT
    } else {
        HANDSHAKE_LINE_LIMIT
    };
    let read = tokio::time::timeout(CONTROL_IDLE_TIMEOUT, read_bounded_line(reader, cap));
    match killed {
        Some(notify) => {
            tokio::pin!(read);
            tokio::select! {
                outcome = &mut read => match outcome {
                    Ok(inner) => inner,
                    Err(_) => Ok(None),
                },
                _ = notify.notified() => Ok(None),
            }
        }
        None => match read.await {
            Ok(inner) => inner,
            Err(_) => Ok(None),
        },
    }
}

async fn handle_conn(sock: TcpStream, server: &ControlServer, peer: &str) {
    let (rd, mut wr) = sock.into_split();
    let mut reader = BufReader::new(rd);
    // 첫 줄은 인증 전이다 — 핸드셰이크 상한을 넘기면 응답 없이 끊는다(F08).
    let first_line = match tokio::time::timeout(
        CONTROL_IDLE_TIMEOUT,
        read_bounded_line(&mut reader, HANDSHAKE_LINE_LIMIT),
    )
    .await
    {
        Ok(Ok(Some(line))) => line,
        Ok(Err(ControlLineReadError::TooLong)) => {
            eprintln!(
                "control handshake line exceeded {HANDSHAKE_LINE_LIMIT} bytes from {peer}; closing"
            );
            return;
        }
        Ok(Err(ControlLineReadError::Io(error))) => {
            eprintln!(
                "control handshake read failed ({:?}); closing",
                error.kind()
            );
            return;
        }
        _ => return,
    };
    if !server.auth_limiter.allow(peer) {
        // 무차별 시도가 누적된 주소 — 응답조차 주지 않는다(백오프).
        return;
    }
    let Some((mut crypto, mut pending)) =
        negotiate(&first_line, &mut reader, &mut wr, server, peer).await
    else {
        return;
    };
    let mut device: Option<String> = None;
    let mut authorization: Option<crate::pairing::Authorization> = None;
    // 철회 즉시 차단: 인증을 통과한 연결은 레지스트리에 등록되고, 장치가
    // 철회되면 깨워져 루프가 다음 줄을 기다리지 않고 종료한다.
    let mut killed: Option<std::sync::Arc<tokio::sync::Notify>> = None;
    let mut _lease: Option<AuthedConnLease<'_>> = None;
    loop {
        let raw = match pending.take() {
            Some(line) => line,
            None => {
                // Reap half-open connections: a peer that vanished without FIN never
                // unblocks this read otherwise, and its task leaks for the process
                // lifetime. The viewer's 2s status poll makes 15s a generous budget.
                match read_authed_line(&mut reader, killed.as_ref()).await {
                    Ok(Some(line)) => line,
                    Ok(None) => break,
                    Err(ControlLineReadError::TooLong) => {
                        let cap = if device.is_some() {
                            COMMAND_LINE_LIMIT
                        } else {
                            HANDSHAKE_LINE_LIMIT
                        };
                        eprintln!("control command line exceeded {cap} bytes from {peer}; closing");
                        break;
                    }
                    Err(ControlLineReadError::Io(error)) => {
                        eprintln!("control command read failed ({:?}); closing", error.kind());
                        break;
                    }
                }
            }
        };
        if raw.trim().is_empty() {
            continue;
        }
        let line = match crypto.decode(raw) {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let parsed: Result<(String, serde_json::Value, Option<String>), String> =
            serde_json::from_str(&line)
                .map(|v: Envelope| (v.command, v.args, v.token))
                .map_err(|e| format!("bad request: {e}"));
        let resp = match parsed {
            Ok((cmd, args, token)) => {
                // Auth gate: `pair` is the only command reachable without a
                // token over the network. The loopback-only beginPairing
                // command is an operator diagnostic equivalent to opening the
                // local Tauri pairing panel. Anything else requires a token
                // issued by a completed pairing; on failure the connection is
                // closed, not just the request rejected (design §2).
                let local_pairing = cmd == "beginPairing" && is_loopback_peer(peer);
                if device.is_none() && cmd != "pair" && !local_pairing {
                    match server.pairing.authenticate(token.as_deref().unwrap_or("")) {
                        Some(auth) => {
                            let device_id = auth.device_id().to_owned();
                            authorization = Some(auth);
                            let (notify, lease) = server.register_authed_conn(&device_id);
                            killed = Some(notify);
                            _lease = Some(lease);
                            device = Some(device_id);
                            server.auth_limiter.record_success(peer);
                        }
                        None => {
                            server.auth_limiter.record_failure(peer);
                            let _ = write_response(
                                &mut wr,
                                &mut crypto,
                                &serde_json::to_string(&err("unauthorized")).unwrap_or_default(),
                            )
                            .await;
                            break;
                        }
                    }
                }
                // 실행 직전 재검사(M1): 연결 인증은 소켓 수명당 한 번이므로,
                // 철회가 명령 실행 중에 일어나면 그 명령은 철회 뒤에도
                // 완주된다(startStream은 첫 프레임까지 몇 초를 기다린다).
                // 디스패치 바로 앞에서 장치의 페어링 유효성을 다시 확인해
                // 철회된 장치의 명령 유리시간을 없앤다.
                if server
                    .pairing
                    .with_authorization(authorization.as_ref(), || ())
                    .is_none()
                {
                    let _ = write_response(
                        &mut wr,
                        &mut crypto,
                        &serde_json::to_string(&err("unauthorized")).unwrap_or_default(),
                    )
                    .await;
                    break;
                }
                let out = server
                    .dispatch_with_authorization(
                        &cmd,
                        args,
                        peer,
                        device.as_deref(),
                        authorization.as_ref(),
                    )
                    .await;
                serde_json::to_string(&out).unwrap_or_else(|_| "{\"ok\":false}".into())
            }
            Err(e) => format!("{{\"ok\":false,\"error\":{}}}", json!(e)),
        };
        if write_response(&mut wr, &mut crypto, &resp).await.is_err() {
            break;
        }
    }
}

/// 한 연결의 봉인 상태. 루프백 진단 클라이언트(tools/, 테스트)는 평문을
/// 유지하고, 그 외 피어는 secure-channel 핸드셰이크가 필수다.
enum ConnCrypto {
    Plain,
    Secure {
        tx: secure_channel::StreamSealer,
        rx: secure_channel::StreamSealer,
    },
}

impl ConnCrypto {
    /// 수신 줄을 평문 JSON으로 되돌린다. 봉인 프레임은 `{"e":"<b64url>"}`.
    fn decode(&mut self, raw: String) -> Result<String, ()> {
        match self {
            ConnCrypto::Plain => Ok(raw),
            ConnCrypto::Secure { rx, .. } => {
                let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|_| ())?;
                let encoded = parsed.get("e").and_then(|v| v.as_str()).ok_or(())?;
                let frame = base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(encoded)
                    .map_err(|_| ())?;
                let plaintext = rx.open(&frame).map_err(|_| ())?;
                String::from_utf8(plaintext).map_err(|_| ())
            }
        }
    }

    /// 응답 JSON을 전송 형식으로 감싼다.
    fn encode(&mut self, body: &str) -> Result<String, ()> {
        match self {
            ConnCrypto::Plain => Ok(body.to_owned()),
            ConnCrypto::Secure { tx, .. } => {
                let frame = tx.seal(body.as_bytes()).map_err(|_| ())?;
                let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(frame);
                Ok(json!({ "e": encoded }).to_string())
            }
        }
    }
}

async fn write_response(
    wr: &mut tokio::net::tcp::OwnedWriteHalf,
    crypto: &mut ConnCrypto,
    body: &str,
) -> Result<(), ()> {
    let wire = crypto.encode(body)?;
    let _ = wr.write_all(wire.as_bytes()).await;
    let _ = wr.write_all(b"\n").await;
    Ok(())
}

async fn refuse_handshake(wr: &mut tokio::net::tcp::OwnedWriteHalf) {
    write_line(
        wr,
        &serde_json::to_string(&err("handshake required")).unwrap_or_default(),
    )
    .await;
}

#[derive(serde::Deserialize)]
struct ClientHelloLine {
    /// 필수 필드 — 평문 명령 JSON과 구조 자체가 겹치지 않게 한다.
    hello: String,
    nc: String,
    xk: String,
}

/// 첫 줄로 전송 모드를 정한다. ClientHello면 봉인 핸드셰이크를 끝까지 마치고
/// Secure를, 루프백 평문이면 Plain(첫 줄을 되돌려 명령 루프가 처리)을, 그
/// 외엔 오류 한 줄과 함께 None(연결 종료)을 돌려준다.
async fn negotiate(
    first_line: &str,
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    wr: &mut tokio::net::tcp::OwnedWriteHalf,
    server: &ControlServer,
    peer: &str,
) -> Option<(ConnCrypto, Option<String>)> {
    let hello: ClientHelloLine = match serde_json::from_str::<ClientHelloLine>(first_line) {
        Ok(v) if v.hello == "c" => v,
        _ => {
            if is_loopback_peer(peer) {
                return Some((ConnCrypto::Plain, Some(first_line.to_owned())));
            }
            refuse_handshake(wr).await;
            return None;
        }
    };
    let (Some(nc), Some(xk)) = (b64_bytes32(&hello.nc), b64_bytes32(&hello.xk)) else {
        refuse_handshake(wr).await;
        return None;
    };
    let client = secure_channel::ClientHello { nc, xk };
    let (hello_out, keys) = secure_channel::accept_client(&server.identity, &client);
    let reply = json!({
        "v": 1,
        "hello": "s",
        "ns": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hello_out.ns),
        "xk": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hello_out.xk),
        "spk": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hello_out.spk),
        "sig": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hello_out.sig),
    });
    write_line(wr, &reply.to_string()).await;
    let mut rx = secure_channel::StreamSealer::new(keys.c2s);
    let tx = secure_channel::StreamSealer::new(keys.s2c);
    // 키 확인: 클라이언트의 첫 봉인 프레임은 {"hello":"ok","nc":<에코>}여야
    // 한다 — 핸드셰이크 유래 값의 되돌림으로 두 방향 키를 모두 증명한다.
    let confirmation = match tokio::time::timeout(
        CONTROL_IDLE_TIMEOUT,
        read_bounded_line(reader, HANDSHAKE_LINE_LIMIT),
    )
    .await
    {
        Ok(Ok(Some(line))) => line,
        _ => return None,
    };
    let expected_nc = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(nc);
    let confirmed = (|| {
        let parsed: serde_json::Value = serde_json::from_str(&confirmation).ok()?;
        let frame = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parsed.get("e")?.as_str()?)
            .ok()?;
        let plaintext = rx.open(&frame).ok()?;
        let value: serde_json::Value = serde_json::from_slice(&plaintext).ok()?;
        Some(value.get("hello")?.as_str()? == "ok" && value.get("nc")?.as_str()? == expected_nc)
    })() == Some(true);
    if !confirmed {
        write_line(
            wr,
            &serde_json::to_string(&err("handshake failed")).unwrap_or_default(),
        )
        .await;
        return None;
    }
    Some((ConnCrypto::Secure { tx, rx }, None))
}

fn b64_bytes32(encoded: &str) -> Option<[u8; 32]> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()?;
    bytes.try_into().ok()
}

/// :7777 토큰 무차별 대입에 대한 IP별 백오프. 60초 창에 5회 실패하면 60초
/// 차단한다. 루프백은 대상에서 제외한다(진단 도구·테스트).
struct AuthRateLimiter {
    failures: Mutex<HashMap<String, FailState>>,
}

#[derive(Clone, Copy)]
struct FailState {
    count: u32,
    window_start: Instant,
    blocked_until: Option<Instant>,
}

const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(60);
const AUTH_FAILURE_LIMIT: u32 = 5;
const AUTH_BLOCK: Duration = Duration::from_secs(60);

impl AuthRateLimiter {
    fn new() -> Self {
        Self {
            failures: Mutex::new(HashMap::new()),
        }
    }

    fn allow(&self, peer: &str) -> bool {
        if is_loopback_peer(peer) {
            return true;
        }
        let mut failures = self.failures.lock().unwrap();
        let now = Instant::now();
        failures.retain(|_, state| {
            state.blocked_until.map(|until| until > now).unwrap_or(true)
                || now.duration_since(state.window_start) < AUTH_FAILURE_WINDOW
        });
        match failures.get_mut(peer) {
            Some(state) => !state
                .blocked_until
                .map(|until| until > now)
                .unwrap_or(false),
            None => true,
        }
    }

    fn record_failure(&self, peer: &str) {
        if is_loopback_peer(peer) {
            return;
        }
        let mut failures = self.failures.lock().unwrap();
        let now = Instant::now();
        let state = failures.entry(peer.to_owned()).or_insert(FailState {
            count: 0,
            window_start: now,
            blocked_until: None,
        });
        if now.duration_since(state.window_start) >= AUTH_FAILURE_WINDOW {
            state.count = 0;
            state.window_start = now;
        }
        state.count += 1;
        if state.count >= AUTH_FAILURE_LIMIT {
            state.blocked_until = Some(now + AUTH_BLOCK);
        }
    }

    fn record_success(&self, peer: &str) {
        if is_loopback_peer(peer) {
            return;
        }
        self.failures.lock().unwrap().remove(peer);
    }
}

#[cfg(test)]
mod rate_limiter_tests {
    use super::*;

    #[test]
    fn five_failures_block_then_success_clears() {
        let limiter = AuthRateLimiter::new();
        let peer = "192.168.0.77";
        assert!(limiter.allow(peer));
        for _ in 0..4 {
            limiter.record_failure(peer);
            assert!(limiter.allow(peer));
        }
        limiter.record_failure(peer);
        assert!(!limiter.allow(peer), "5th failure must block the peer");
        // 루프백은 차단 대상이 아니다.
        assert!(limiter.allow("127.0.0.1"));
        // 성공은 카운터를 지운다.
        limiter.record_success(peer);
        assert!(limiter.allow(peer));
    }
}

fn is_loopback_peer(peer: &str) -> bool {
    peer.parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

fn same_private_lan_candidate(candidate: &str, peer: &str) -> bool {
    let Ok(candidate) = candidate.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let Ok(peer) = peer.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let candidate_octets = candidate.octets();
    let peer_octets = peer.octets();
    let same_private_subnet = peer.is_private() && candidate_octets[..3] == peer_octets[..3];
    let tailnet_peer = peer_octets[0] == 100 && (peer_octets[1] & 0b1100_0000) == 64;
    candidate.is_private() && (same_private_subnet || tailnet_peer)
}

async fn write_line(wr: &mut tokio::net::tcp::OwnedWriteHalf, body: &str) {
    let _ = wr.write_all(body.as_bytes()).await;
    let _ = wr.write_all(b"\n").await;
}

#[derive(serde::Deserialize)]
struct Envelope {
    command: String,
    #[serde(default)]
    args: serde_json::Value,
    #[serde(default)]
    token: Option<String>,
}

/// file_token 하나만 받는 파일 명령(End/Cancel 계열)의 인자 파싱.
/// 실패 시 뷰어로 돌아가는 err 값이 그대로 Err가 된다.
fn parse_file_token(args: serde_json::Value) -> Result<String, serde_json::Value> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TokenArgs {
        file_token: String,
    }
    serde_json::from_value::<TokenArgs>(args)
        .map(|input| input.file_token)
        .map_err(|_| err("bad args"))
}

fn ok<T: serde::Serialize>(result: T) -> serde_json::Value {
    json!({ "ok": true, "result": result })
}

fn err(error: &str) -> serde_json::Value {
    json!({ "ok": false, "error": error })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canonical base64url spelling of bytes 0..32, used by startStream tests.
    const TEST_MEDIA_KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";

    #[test]
    fn split_vertical_requires_exact_4k60_udp_and_safe_base_port() {
        let parse = |width, height, fps, media_transport: &str, viewer_port| {
            serde_json::from_value::<StartStreamInput>(serde_json::json!({
                "sourceIndex": 0,
                "viewerPort": viewer_port,
                "width": width,
                "height": height,
                "fps": fps,
                "mediaTransport": media_transport,
                "encoderExperiment": "splitVertical"
            }))
            .unwrap()
        };
        assert!(validate_split_start(&parse(3840, 2160, 60, "udp", 5002), "udp").is_ok());
        assert!(validate_split_start(&parse(3840, 2160, 60, "tcp", 5002), "tcp").is_err());
        assert!(validate_split_start(&parse(3840, 2160, 60, "udp", 65535), "udp").is_err());
        assert!(validate_split_start(&parse(2560, 1440, 60, "udp", 5002), "udp").is_err());
    }

    #[test]
    fn product_normalization_keeps_verified_split_and_diagnostic_is_fallback() {
        let advertised = advertised_encoder_experiments(vec![
            canonical_encoder_experiment(EncoderExperiment::Auto),
            canonical_encoder_experiment(EncoderExperiment::SplitVertical),
            canonical_encoder_experiment(EncoderExperiment::SplitHorizontal),
        ]);

        assert_eq!(
            advertised.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            vec![EncoderExperiment::Auto, EncoderExperiment::SplitVertical]
        );
        assert!(encoder_experiment_is_startable(
            &advertised,
            EncoderExperiment::SplitVertical,
            false
        ));
        assert!(encoder_experiment_is_startable(
            &advertised,
            EncoderExperiment::SplitVertical,
            true
        ));
        assert!(!encoder_experiment_is_startable(
            &[canonical_encoder_experiment(EncoderExperiment::Auto)],
            EncoderExperiment::SplitHorizontal,
            true
        ));
        assert!(encoder_experiment_is_startable(
            &[canonical_encoder_experiment(EncoderExperiment::Auto)],
            EncoderExperiment::SplitVertical,
            true
        ));
    }
    use crate::backend::{CaptureBackend, FakeBackend};
    use control_contract::host::DisplayInfo;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };

    fn backend() -> SharedBackend {
        Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                source_id: Some("test:display:0".into()),
                index: 0,
                name: "Main".into(),
                width: 1920,
                height: 1080,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        })
    }

    struct TerminalBackend {
        stopped: Arc<AtomicUsize>,
    }

    impl CaptureBackend for TerminalBackend {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            backend().list_displays()
        }

        fn start(
            &self,
            _source_index: u32,
            _ip: &str,
            _port: u16,
            _w: u32,
            _h: u32,
            _fps: u32,
            _capture_backend: &str,
            _media_transport: &str,
            _content_mode: &str,
            _encoder_experiment: EncoderExperiment,
            _udp_stability: &AppliedUdpStability,
            _media_key: &[u8; 32],
            _access: Option<&crate::source_grants::CaptureAccess>,
        ) -> Result<u32, String> {
            Ok(7)
        }

        fn stop(&self, _handle: u32) -> Result<(), String> {
            self.stopped.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
            Ok(StatsInfo {
                frames: 10,
                bytes: 1_000,
                state: "error".into(),
                fps: 0,
                kbps: 0,
                fps_target: 60,
                encoder_experiment_diagnostics_available: true,
                encoder_experiment_requested: "auto".into(),
                encoder_experiment_applied: "rateControl".into(),
                encoder_experiment_fallback_reason: None,
                encoder_frame_drops: 0,
                encoder_frame_drop_fps: 0,
                valid_encode_output_fps: 0,
                encode_submit_call_p50_us: 0,
                encode_submit_call_p95_us: 0,
                encoder_callback_p50_us: 0,
                encoder_callback_p95_us: 0,
                packetization_in_flight: 0,
                base_frame_qp: None,
                base_frame_qp_changes: 0,
                capture_fps: 0,
                encode_submit_fps: 0,
                encode_output_fps: 0,
                rendered_fps: None,
                capture_callbacks: 0,
                encode_output_callbacks: 0,
                encode_submit_failures: 0,
                encode_in_flight: 0,
                dropped: 0,
                network_dropped: 0,
                network_queue_dropped: 0,
                recovery_frames_dropped: 0,
                udp_send_failures: 0,
                udp_send_retries: 0,
                recovery_keyframes: 0,
                recovery_requests_suppressed: 0,
                capture_queue_dropped: 0,
                capture_to_encode_us: 0,
                max_capture_to_encode_us: 0,
                capture_queue_wait_us: 0,
                max_capture_queue_wait_us: 0,
                encode_output_us: 0,
                max_encode_output_us: 0,
                packetization_us: 0,
                max_packetization_us: 0,
                send_block_us: 0,
                max_send_block_us: 0,
                send_pace_us: 0,
                max_send_pace_us: 0,
                pending_frame: 0,
                pending_frame_bytes: 0,
                pending_frame_oldest_age_us: 0,
                capture_backend: "screenCaptureKit".into(),
                media_transport: "udp".into(),
                first_capture_ms: 20,
                first_encode_ms: 25,
                first_send_ms: 26,
                current_bitrate: 12_000_000,
                bitrate_floor_collapse_count: 7,
                bitrate_floor_collapse_last_reason: "resolution_fallback_floor_reached".into(),
                encoder_mode: "unknown".into(),
                encoder_id: "unknown".into(),
                encoder_hardware_accelerated: None,
                encoder_preset: "unknown".into(),
                encoder_profile: "unknown".into(),
                encoder_applied_properties: Vec::new(),
                encoder_unsupported_properties: Vec::new(),
                encoder_rejected_properties: Vec::new(),
                encoder_fallback_reason: None,
                quality_hint: None,
                quality_override: None,
                quality_adaptation_checks: 0,
                quality_adaptation_changes: 0,
                quality_adaptation_rejections: 0,
                quality_adaptation_last_status: "not_checked".into(),
                capture_interval_p95_us: 16_667,
                capture_to_encode_p95_us: 8_000,
                capture_queue_wait_p95_us: 1_000,
                encode_output_p95_us: 7_000,
                packetization_p95_us: 0,
                encode_output_interval_p95_us: 0,
                send_block_p95_us: 1_000,
                send_pace_p95_us: 0,
                last_au_bytes: 0,
                last_au_fragments: 0,
                last_au_parity: 0,
                last_au_datagrams: 0,
                last_au_expected_datagrams: 0,
                last_au_send_us: 0,
                last_au_is_keyframe: false,
                max_au_bytes: 0,
                max_au_fragments: 0,
                sent_datagrams: 0,
                sent_parity_datagrams: 0,
                error: Some("viewer closed stream".into()),
                receiver_frame_gaps: 0,
                receiver_input_drops: 0,
                receiver_incomplete_aus: 0,
                receiver_stale_frames: 0,
                receiver_stale_input_drops: None,
                receiver_output_burst_discards: 0,
                receiver_rtt_ms: None,
                receiver_wire_ms: None,
                receiver_feedback_age_ms: None,
                ..StatsInfo::default()
            })
        }
    }

    async fn spawn_server() -> std::net::SocketAddr {
        spawn_server_with_pairing(test_pairing()).await
    }

    fn test_pairing() -> std::sync::Arc<crate::pairing::PairingServer> {
        std::sync::Arc::new(crate::pairing::PairingServer::new(
            [7u8; 32],
            None,
            Box::new(crate::pairing::FileTokenStore::new(None)),
        ))
    }

    fn test_identity() -> std::sync::Arc<secure_channel::HostIdentity> {
        std::sync::Arc::new(secure_channel::HostIdentity::from_seed([42u8; 32]))
    }

    async fn spawn_server_with_pairing(
        pairing: std::sync::Arc<crate::pairing::PairingServer>,
    ) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::sync::Arc::new(ControlServer::new(backend(), pairing, test_identity()));
        server.set_control_port(addr.port());
        tokio::spawn(async move { server.run(listener).await });
        addr
    }

    async fn request(
        sock: &mut tokio::net::TcpStream,
        cmd: &str,
        args: &str,
        token: &str,
    ) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        sock.write_all(
            format!("{{\"command\":\"{cmd}\",\"args\":{args},\"token\":\"{token}\"}}\n").as_bytes(),
        )
        .await
        .unwrap();
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            sock.read_exact(&mut byte).await.unwrap();
            if byte[0] == b'\n' {
                break;
            }
            buf.push(byte[0]);
        }
        String::from_utf8(buf).unwrap()
    }

    // -- 봉인 제어 평면 (secure-channel 핸드셰이크) ---------------------------

    #[tokio::test]
    async fn secure_handshake_seals_commands_and_pairing_token() {
        use tokio::io::AsyncWriteExt;

        let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let unb64 = |encoded: &str| -> [u8; 32] {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(encoded)
                .unwrap()
                .try_into()
                .unwrap()
        };
        async fn read_line(sock: &mut tokio::net::TcpStream) -> String {
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                use tokio::io::AsyncReadExt;
                sock.read_exact(&mut byte).await.unwrap();
                if byte[0] == b'\n' {
                    break;
                }
                buf.push(byte[0]);
            }
            String::from_utf8(buf).unwrap()
        }

        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        // 핸드셰이크: ClientHello → ServerHello(핀된 키로 검증) → 키 확인.
        let mut nc = [0u8; 32];
        secure_channel::random_bytes(&mut nc);
        let mut secret_seed = [0u8; 32];
        secure_channel::random_bytes(&mut secret_seed);
        let secret = x25519_dalek::StaticSecret::from(secret_seed);
        let client = secure_channel::ClientHello {
            nc,
            xk: x25519_dalek::PublicKey::from(&secret).to_bytes(),
        };
        sock.write_all(
            format!(
                "{}\n",
                json!({ "v": 1, "hello": "c", "nc": b64(&nc), "xk": b64(&client.xk) })
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let hello_value: serde_json::Value =
            serde_json::from_str(&read_line(&mut sock).await).unwrap();
        let server_hello = secure_channel::ServerHello {
            ns: unb64(hello_value["ns"].as_str().unwrap()),
            xk: unb64(hello_value["xk"].as_str().unwrap()),
            spk: unb64(hello_value["spk"].as_str().unwrap()),
            sig: base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(hello_value["sig"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
        };
        let pinned = test_identity().public_key();
        let (keys, spk) =
            secure_channel::client_finish(&secret, &client, &server_hello, Some(&pinned)).unwrap();
        assert_eq!(spk, pinned);
        let mut send = secure_channel::StreamSealer::new(keys.c2s);
        let mut recv = secure_channel::StreamSealer::new(keys.s2c);

        let confirm = json!({ "hello": "ok", "nc": b64(&nc) });
        let frame = send.seal(confirm.to_string().as_bytes()).unwrap();
        sock.write_all(format!("{}\n", json!({ "e": b64(&frame) })).as_bytes())
            .await
            .unwrap();

        // 봉인된 pair 요청 — 페어링 토큰이 이제 암호화 채널로만 이동한다.
        let view = pairing.begin_pairing("127.0.0.1", 7777);
        let payload: serde_json::Value = serde_json::from_str(&view.qr_payload).unwrap();
        let args = json!({
            "offerId": payload["id"],
            "secret": payload["s"],
            "code": "",
            "deviceId": "sealed-viewer",
            "deviceName": "Sealed Viewer",
        });
        let pair_request = json!({ "command": "pair", "args": args, "token": null });
        let frame = send.seal(pair_request.to_string().as_bytes()).unwrap();
        sock.write_all(format!("{}\n", json!({ "e": b64(&frame) })).as_bytes())
            .await
            .unwrap();

        // 서버의 pending 응답을 먼저 회수한다(비동기 경합 제거).
        let first_response: serde_json::Value = serde_json::from_slice(
            &recv
                .open(&{
                    let line = read_line(&mut sock).await;
                    let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
                    base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(parsed["e"].as_str().unwrap())
                        .unwrap()
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            first_response["result"]["status"], "pending",
            "{first_response}"
        );

        // 승인 → 같은 봉인 채널로 픽업 폴링.
        pairing
            .approve_pending(payload["id"].as_str().unwrap())
            .unwrap();
        let frame = send.seal(pair_request.to_string().as_bytes()).unwrap();
        sock.write_all(format!("{}\n", json!({ "e": b64(&frame) })).as_bytes())
            .await
            .unwrap();
        let response_value: serde_json::Value = serde_json::from_slice(
            &recv
                .open(&{
                    let line = read_line(&mut sock).await;
                    let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
                    base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(parsed["e"].as_str().unwrap())
                        .unwrap()
                })
                .unwrap(),
        )
        .unwrap();
        let token = response_value["result"]["token"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(token.len(), 64, "{response_value}");

        // 루프백 평문 경로는 진단 호환을 위해 살아 있어야 한다.
        let mut plain = tokio::net::TcpStream::connect(addr).await.unwrap();
        let line = request(&mut plain, "getStatus", "{}", &token).await;
        assert!(line.contains("\"ok\":true"), "{line}");
    }

    /// Pair via the real pairing flow and return the issued token.
    async fn pair_token(
        sock: &mut tokio::net::TcpStream,
        pairing: &crate::pairing::PairingServer,
    ) -> String {
        let view = pairing.begin_pairing("127.0.0.1", 7777);
        let payload: serde_json::Value = serde_json::from_str(&view.qr_payload).unwrap();
        let args = json!({
            "offerId": payload["id"],
            "secret": payload["s"],
            "code": view.code,
            "deviceId": "test-viewer",
            "deviceName": "Test Viewer",
        });
        let line = request(sock, "pair", &args.to_string(), "").await;
        let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
        let token = resp["result"]["token"].as_str().unwrap().to_owned();
        pairing
            .update_source_grants("test-viewer", vec!["test:display:0".into()])
            .0
            .unwrap();
        pairing.remember_catalog(
            &pairing.authenticate(&token).unwrap(),
            &backend().list_displays().unwrap(),
        );
        token
    }

    #[tokio::test]
    async fn catalog_start_status_stop_roundtrip() {
        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let token = pair_token(&mut sock, &pairing).await;

        let line = request(&mut sock, "getCatalog", "{}", &token).await;
        assert!(line.contains("\"displays\""), "{line}");
        assert!(line.contains("\"platform\":\"test\""), "{line}");
        assert!(line.contains("\"captureBackends\""), "{line}");

        let line = request(
            &mut sock,
            "startStream",
            r#"{"mediaKey":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8","sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90}"#,
            &token,
        )
        .await;
        assert!(line.contains("\"session\":1"), "{line}");
        assert!(line.contains("\"width\":1920"), "{line}");
        assert!(line.contains("\"height\":1080"), "{line}");
        assert!(line.contains("\"qualityState\":\"native\""), "{line}");

        let line = request(
            &mut sock,
            "reconfigureStream",
            r#"{"session":1,"width":2560,"height":1440,"fps":90,"qualityState":"fallback"}"#,
            &token,
        )
        .await;
        assert!(line.contains("\"session\":1"), "{line}");
        assert!(line.contains("\"width\":2560"), "{line}");
        assert!(line.contains("\"height\":1440"), "{line}");
        assert!(line.contains("\"qualityState\":\"fallback\""), "{line}");

        let line = request(
            &mut sock,
            "reconfigureStream",
            r#"{"session":1,"width":0,"height":1440,"fps":90,"qualityState":"fallback"}"#,
            &token,
        )
        .await;
        assert!(line.contains("\"ok\":false"), "{line}");

        let line = request(&mut sock, "getStatus", "{}", &token).await;
        assert!(line.contains("\"state\":\"running\""), "{line}");
        assert!(line.contains("\"width\":2560"), "{line}");
        assert!(line.contains("\"qualityState\":\"fallback\""), "{line}");
        assert!(line.contains("\"inputEnabled\":false"), "{line}");
        assert!(line.contains("\"bitrateFloorCollapseCount\":7"), "{line}");
        assert!(line.contains("\"inputRateHz\":180"), "{line}");

        let line = request(&mut sock, "stopStream", r#"{"session":1}"#, &token).await;
        assert!(line.contains("\"ok\":true"), "{line}");
    }

    #[tokio::test]
    async fn add_numbers_delegates_to_rustra() {
        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let token = pair_token(&mut sock, &pairing).await;
        let line = request(&mut sock, "addNumbers", r#"{"a":20,"b":22}"#, &token).await;
        assert!(line.contains("\"value\":42"), "{line}");
    }

    #[tokio::test]
    async fn unknown_command_and_restart_session_ids() {
        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let token = pair_token(&mut sock, &pairing).await;

        let line = request(&mut sock, "nope", "{}", &token).await;
        assert!(line.contains("\"ok\":false"), "{line}");

        for i in 1..=2 {
            let line = request(
                &mut sock,
                "startStream",
                r#"{"mediaKey":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8","sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90}"#,
                &token,
            )
            .await;
            assert!(line.contains(&format!("\"session\":{i}")), "{line}");
            let line = request(
                &mut sock,
                "stopStream",
                &format!("{{\"session\":{i}}}"),
                &token,
            )
            .await;
            assert!(line.contains("\"ok\":true"), "{line}");
        }
    }

    #[tokio::test]
    async fn unauthenticated_getcatalog_is_rejected() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        let line = request(&mut sock, "getCatalog", "{}", "").await;
        assert!(line.contains("\"error\":\"unauthorized\""), "{line}");
        assert!(line.contains("\"ok\":false"), "{line}");

        // connection is closed: the next request hits EOF or reset
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _ = sock
            .write_all(b"{\"command\":\"getCatalog\",\"args\":{}}\n")
            .await;
        let mut buf = [0u8; 16];
        let n = sock.read(&mut buf).await.unwrap_or(0);
        assert_eq!(n, 0, "connection must be closed after unauthorized");
    }

    #[tokio::test]
    async fn loopback_operator_can_begin_pairing_without_a_token() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        let line = request(&mut sock, "beginPairing", "{}", "").await;
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["ok"], true, "{line}");
        assert_eq!(response["result"]["expires_in_secs"], 120, "{line}");
        assert_eq!(response["result"]["code"].as_str().unwrap().len(), 6);
        let payload: serde_json::Value =
            serde_json::from_str(response["result"]["qr_payload"].as_str().unwrap()).unwrap();
        assert_eq!(payload["v"], 2);
        assert_eq!(payload["p"], addr.port());
    }

    #[tokio::test]
    async fn six_digit_code_alone_pairs_against_the_live_host_offer() {
        let pairing = test_pairing();
        let view = pairing.begin_pairing("127.0.0.1", 7777);
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let args = json!({
            "code": view.code,
            "deviceId": "code-only-viewer",
            "deviceName": "Code Only Viewer",
        });

        let line = request(&mut sock, "pair", &args.to_string(), "").await;
        assert!(line.contains("\"ok\":true"), "{line}");
        assert!(line.contains("\"token\":"), "{line}");
        assert_eq!(pairing.list_devices().len(), 1);
    }

    #[tokio::test]
    async fn approval_pairing_returns_pending_then_token_after_approval() {
        let pairing = test_pairing();
        let view = pairing.begin_pairing("127.0.0.1", 7777);
        let payload: serde_json::Value = serde_json::from_str(&view.qr_payload).unwrap();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let args = json!({
            "offerId": payload["id"],
            "secret": payload["s"],
            "code": "",
            "deviceId": "approval-viewer",
            "deviceName": "Approval Viewer",
        });

        // Mac 승인 전: 오류가 아니라 pending 상태로 답한다.
        let line = request(&mut sock, "pair", &args.to_string(), "").await;
        assert!(line.contains("\"ok\":true"), "{line}");
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["result"]["status"], "pending", "{line}");
        assert_eq!(pairing.list_devices().len(), 0);

        // Mac 사용자가 [허용]을 누른 뒤 같은 요청은 토큰으로 답한다.
        pairing
            .approve_pending(payload["id"].as_str().unwrap())
            .unwrap();
        let line = request(&mut sock, "pair", &args.to_string(), "").await;
        assert!(line.contains("\"ok\":true"), "{line}");
        assert!(line.contains("\"token\":"), "{line}");
        assert_eq!(pairing.list_devices().len(), 1);
    }

    #[test]
    fn operator_pairing_is_strictly_loopback_only() {
        assert!(is_loopback_peer("127.0.0.1"));
        assert!(is_loopback_peer("::1"));
        assert!(!is_loopback_peer("192.168.0.18"));
        assert!(!is_loopback_peer("100.77.109.50"));
        assert!(!is_loopback_peer("localhost"));
    }

    #[test]
    fn media_candidate_must_be_private_and_on_the_control_peers_lan() {
        assert!(same_private_lan_candidate("192.168.0.18", "192.168.0.170"));
        assert!(same_private_lan_candidate("192.168.0.18", "100.80.133.120"));
        assert!(!same_private_lan_candidate("192.168.1.18", "192.168.0.170"));
        assert!(!same_private_lan_candidate("1.2.3.4", "192.168.0.170"));
        assert!(!same_private_lan_candidate("192.168.0.18", "100.128.0.1"));
    }

    #[test]
    fn normalize_media_transport_accepts_usb_without_reinterpreting_adb() {
        assert_eq!(normalize_media_transport("usb"), Some("usb"));
        assert_eq!(normalize_media_transport("AOAP"), Some("usb"));
        assert_eq!(normalize_media_transport("adbTcp"), Some("adbTcp"));
    }

    #[test]
    fn advertised_encoder_experiments_keeps_vertical_split_and_hides_horizontal_split() {
        let normalized = advertised_encoder_experiments(vec![
            EncoderExperimentInfo {
                id: EncoderExperiment::Auto,
                label: "wrong auto".into(),
                hint: "wrong auto hint".into(),
                requires_reconnect: false,
            },
            EncoderExperimentInfo {
                id: EncoderExperiment::SplitHorizontal,
                label: "reserved".into(),
                hint: "reserved".into(),
                requires_reconnect: false,
            },
            EncoderExperimentInfo {
                id: EncoderExperiment::AdaptiveQp,
                label: "wrong qp".into(),
                hint: "wrong qp hint".into(),
                requires_reconnect: false,
            },
            EncoderExperimentInfo {
                id: EncoderExperiment::Auto,
                label: "duplicate auto".into(),
                hint: "duplicate auto hint".into(),
                requires_reconnect: false,
            },
            EncoderExperimentInfo {
                id: EncoderExperiment::SplitVertical,
                label: "reserved".into(),
                hint: "reserved".into(),
                requires_reconnect: false,
            },
            EncoderExperimentInfo {
                id: EncoderExperiment::AdaptiveQp,
                label: "duplicate qp".into(),
                hint: "duplicate qp hint".into(),
                requires_reconnect: false,
            },
        ]);

        assert_eq!(
            normalized,
            vec![
                EncoderExperimentInfo {
                    id: EncoderExperiment::Auto,
                    label: "자동".into(),
                    hint: "호스트가 사용 가능한 인코더 경로를 선택합니다.".into(),
                    requires_reconnect: true,
                },
                EncoderExperimentInfo {
                    id: EncoderExperiment::AdaptiveQp,
                    label: "적응형 QP".into(),
                    hint: "화면 변화와 인코더 압력에 따라 Base QP를 조절합니다.".into(),
                    requires_reconnect: true,
                },
                EncoderExperimentInfo {
                    id: EncoderExperiment::SplitVertical,
                    label: "4K 듀얼 인코더".into(),
                    hint: "4K 화면을 좌우 두 하드웨어 인코더와 UDP 포트로 전송합니다.".into(),
                    requires_reconnect: true,
                },
            ]
        );
    }

    #[test]
    fn advertised_encoder_experiments_keeps_empty_input_empty() {
        assert!(advertised_encoder_experiments(Vec::new()).is_empty());
    }

    #[test]
    fn auto_attempts_usb_then_udp_then_tcp() {
        let attempts = build_attempts("auto", &["192.168.1.50".into()]);
        assert_eq!(
            attempts,
            vec![
                ("127.0.0.1".into(), "usb"),
                ("192.168.1.50".into(), "udp"),
                ("192.168.1.50".into(), "tcp"),
                ("127.0.0.1".into(), "adbTcp"),
            ]
        );
    }

    #[tokio::test]
    async fn wrong_token_is_rejected() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        let line = request(&mut sock, "getCatalog", "{}", "deadbeef").await;
        assert!(line.contains("\"error\":\"unauthorized\""), "{line}");
    }

    #[test]
    fn terminal_sessions_are_retained_briefly_then_released() {
        let stopped = Arc::new(AtomicUsize::new(0));
        let server = ControlServer::new(
            Arc::new(TerminalBackend {
                stopped: stopped.clone(),
            }),
            test_pairing(),
            test_identity(),
        );
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                device_id: None,
                source_index: 0,
                source_name: "Main".into(),
                width: 1_920,
                height: 1_080,
                fps_target: 60,
                quality_state: "native".into(),
                capture_backend: "screenCaptureKit".into(),
                content_mode: "interactive".into(),
                encoder_experiment: EncoderExperiment::Auto,
                viewer_addr: "192.168.0.2:5001".into(),
                viewer_port: 5001,
                media_transport: "udp".into(),
                udp_stability: None,
                media_key: [0u8; 32],
                input_enabled: false,
                input_rate_hz: 120,
                terminal_since: None,
                terminal_error: None,
                backend_released: false,
                lifecycle: Lifecycle::default(),
                transport_owner: None,
                authorization: None,
            },
        );

        let first = server.snapshot();
        assert_eq!(first.sessions.len(), 1);
        assert_eq!(
            first.sessions[0].stats.error.as_deref(),
            Some("viewer closed stream")
        );
        assert_eq!(first.sessions[0].stats.state, "stopped");

        server
            .sessions
            .lock()
            .unwrap()
            .live
            .get_mut(&1)
            .unwrap()
            .terminal_since =
            Some(Instant::now() - TERMINAL_SESSION_RETENTION - Duration::from_millis(1));

        let second = server.snapshot();
        assert!(second.sessions.is_empty());
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn last_session_teardown_locks_the_screen_only_when_enabled() {
        // 설정 켜짐 + 잠금 카운터 주입.
        let server = ControlServer::new(
            Arc::new(TerminalBackend {
                stopped: Arc::new(AtomicUsize::new(0)),
            }),
            test_pairing(),
            test_identity(),
        );
        let settings = crate::settings::SharedSettings::in_memory();
        settings.set_lock_on_disconnect(true).unwrap();
        server.set_settings(settings);
        let locks = Arc::new(AtomicUsize::new(0));
        let counter = locks.clone();
        server.set_lock_screen(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        insert_live_session(&server, 1, None);
        server
            .sessions
            .lock()
            .unwrap()
            .live
            .get_mut(&1)
            .unwrap()
            .terminal_since =
            Some(Instant::now() - TERMINAL_SESSION_RETENTION - Duration::from_millis(1));

        // 만료 세션 GC가 live를 비우면 잠금이 정확히 한 번 불린다.
        let _ = server.snapshot();
        assert_eq!(locks.load(Ordering::SeqCst), 1);
        // 빈 live에 대한 이후 폴링은 중복 잠금하지 않는다(전이 조건).
        let _ = server.snapshot();
        assert_eq!(locks.load(Ordering::SeqCst), 1);

        // 설정 꺼짐: 세션 제거가 잠금을 부르지 않는다.
        let server_off = ControlServer::new(backend(), test_pairing(), test_identity());
        server_off.set_settings(crate::settings::SharedSettings::in_memory());
        let locks_off = Arc::new(AtomicUsize::new(0));
        let counter_off = locks_off.clone();
        server_off.set_lock_screen(Arc::new(move || {
            counter_off.fetch_add(1, Ordering::SeqCst);
        }));
        insert_live_session(&server_off, 1, None);
        server_off.sessions.lock().unwrap().live.remove(&1);
        server_off.maybe_lock_after_teardown(1);
        assert_eq!(
            locks_off.load(Ordering::SeqCst),
            0,
            "disabled setting must not lock"
        );
    }

    #[test]
    fn privacy_curtain_follows_session_lifecycle() {
        let server = ControlServer::new(backend(), test_pairing(), test_identity());
        server.set_settings(crate::settings::SharedSettings::in_memory());
        server
            .settings
            .get()
            .unwrap()
            .set_privacy_curtain(true)
            .unwrap();
        let states: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = states.clone();
        server.set_curtain_controller(Arc::new(move |show| {
            recorder.lock().unwrap().push(show);
            true
        }));

        // 세션이 살아 있으면 커튼이 뜬다.
        insert_live_session(&server, 1, None);
        server.refresh_curtain();
        // 마지막 세션이 정리되면 커튼이 내려간다.
        server.sessions.lock().unwrap().live.remove(&1);
        server.refresh_curtain();
        assert_eq!(*states.lock().unwrap(), vec![true, false]);

        // 설정 꺼짐: 상태 변화가 있어도 실행부를 부르지 않는다.
        server
            .settings
            .get()
            .unwrap()
            .set_privacy_curtain(false)
            .unwrap();
        insert_live_session(&server, 2, None);
        server.refresh_curtain();
        server.sessions.lock().unwrap().live.remove(&2);
        server.refresh_curtain();
        assert_eq!(
            states.lock().unwrap().len(),
            2,
            "disabled curtain must not touch the controller"
        );
    }

    /// 적용이 실패한 토글은 상태로 커밋되지 않는다 — 다음 refresh가 같은
    /// desired로 실행부를 다시 부른다(M3). 성공해 커밋된 뒤에는 중복 호출이
    /// 없어야 한다.
    #[test]
    fn curtain_apply_failure_stays_uncommitted_and_retries() {
        let server = ControlServer::new(backend(), test_pairing(), test_identity());
        let settings = crate::settings::SharedSettings::in_memory();
        settings.set_privacy_curtain(true).unwrap();
        server.set_settings(settings);
        insert_live_session(&server, 1, None);

        let calls = Arc::new(AtomicUsize::new(0));
        let fail = Arc::new(AtomicBool::new(true));
        let (calls_c, fail_c) = (calls.clone(), fail.clone());
        server.set_curtain_controller(Arc::new(move |_show| {
            calls_c.fetch_add(1, Ordering::SeqCst);
            !fail.load(Ordering::SeqCst)
        }));

        // 창 생성 실패 — 상태가 커밋되지 않아 원하는 상태가 그대로 남는다.
        server.refresh_curtain();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // 다음 refresh 트리거가 재시도하고, 성공한 뒤에야 커밋된다.
        fail_c.store(false, Ordering::SeqCst);
        server.refresh_curtain();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        // 커밋된 뒤의 refresh는 실행부를 다시 부르지 않는다.
        server.refresh_curtain();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// 커튼 창은 필터 생성(backend.start)보다 먼저 존재해야 캡처에서
    /// 제외된다(H1) — live가 비어 있어도 시작 직전에는 띄우고, 시작이 모두
    /// 실패해 세션이 생기지 않았다면 되돌린다.
    #[test]
    fn curtain_is_raised_before_the_stream_starts_and_lowered_when_it_fails() {
        let server = ControlServer::new(backend(), test_pairing(), test_identity());
        let settings = crate::settings::SharedSettings::in_memory();
        settings.set_privacy_curtain(true).unwrap();
        server.set_settings(settings);
        let states: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = states.clone();
        server.set_curtain_controller(Arc::new(move |show| {
            recorder.lock().unwrap().push(show);
            true
        }));

        // 시작 직전: 첫 세션(live가 빈 상태)에서도 커튼을 먼저 띄운다.
        server.prepare_curtain_for_start();
        assert_eq!(*states.lock().unwrap(), vec![true]);

        // 세션이 등록된 뒤의 refresh는 desired가 같아 중복 apply가 없다.
        insert_live_session(&server, 1, None);
        server.refresh_curtain();
        assert_eq!(*states.lock().unwrap(), vec![true]);

        // 시작이 모두 실패한 경로 — 세션 없이는 커튼이 내려간다.
        server.sessions.lock().unwrap().live.clear();
        server.refresh_curtain();
        assert_eq!(*states.lock().unwrap(), vec![true, false]);
    }

    /// 토글이 켜진 뒤의 정리: SCK 세션은 같은 형태로 재시작돼 필터를 다시
    /// 만들고(커튼 제외), CGDisplayStream 세션은 창 제외가 불가능하므로
    /// 종료된다(H1, M6).
    #[tokio::test]
    async fn curtain_toggle_restarts_sck_sessions_and_stops_cg_ones() {
        let stopped = Arc::new(AtomicUsize::new(0));
        let server = ControlServer::new(
            Arc::new(TerminalBackend {
                stopped: stopped.clone(),
            }),
            test_pairing(),
            test_identity(),
        );
        insert_live_session(&server, 1, None);
        server.sessions.lock().unwrap().live.insert(
            2,
            Session {
                handle: 9,
                device_id: None,
                source_index: 0,
                source_name: "Main".into(),
                width: 1_920,
                height: 1_080,
                fps_target: 60,
                quality_state: "native".into(),
                capture_backend: "cgDisplayStream".into(),
                content_mode: "interactive".into(),
                encoder_experiment: EncoderExperiment::Auto,
                viewer_addr: "192.168.0.2:5001".into(),
                viewer_port: 5001,
                media_transport: "udp".into(),
                udp_stability: None,
                media_key: [0u8; 32],
                input_enabled: false,
                input_rate_hz: 120,
                terminal_since: None,
                terminal_error: None,
                backend_released: false,
                lifecycle: Lifecycle::default(),
                transport_owner: None,
                authorization: None,
            },
        );

        server.restart_sessions_for_curtain().await;

        let state = server.sessions.lock().unwrap();
        let sck = state.live.get(&1).unwrap();
        assert!(
            !sck.backend_released,
            "SCK session must survive the filter rebuild"
        );
        let cg = state.live.get(&2).unwrap();
        assert!(
            cg.backend_released,
            "CGDisplayStream session must not keep running under the curtain"
        );
        assert_eq!(
            cg.terminal_error.as_deref(),
            Some("host operator stopped the stream")
        );
        drop(state);
        // CG 강제 종료 1회 + SCK 재시작의 이전 핸들 정리 1회.
        assert_eq!(stopped.load(Ordering::SeqCst), 2);
    }

    fn insert_live_session(server: &ControlServer, id: u32, device: Option<&str>) {
        if let Some(device) = device {
            if !server.pairing.is_device_paired(device) {
                direct_pair_token(&server.pairing, device);
            }
        }
        let fixture_device = device.unwrap_or("test-local-host");
        if !server.pairing.is_device_paired(fixture_device) {
            direct_pair_token(&server.pairing, fixture_device);
        }
        let authorization = server
            .pairing
            .authorization(fixture_device)
            .and_then(|auth| {
                server
                    .pairing
                    .source_authorization(&auth, "test:display:0")
                    .ok()
            });
        server.sessions.lock().unwrap().live.insert(
            id,
            Session {
                handle: 7 + id,
                device_id: device.map(str::to_owned),
                source_index: 0,
                source_name: "Main".into(),
                width: 1_920,
                height: 1_080,
                fps_target: 60,
                quality_state: "native".into(),
                capture_backend: "screenCaptureKit".into(),
                content_mode: "interactive".into(),
                encoder_experiment: EncoderExperiment::Auto,
                viewer_addr: "192.168.0.2:5001".into(),
                viewer_port: 5001,
                media_transport: "udp".into(),
                udp_stability: None,
                media_key: [1u8; 32],
                input_enabled: false,
                input_rate_hz: 120,
                terminal_since: None,
                terminal_error: None,
                backend_released: false,
                lifecycle: Lifecycle::default(),
                transport_owner: None,
                authorization,
            },
        );
    }

    #[test]
    fn forced_stop_is_retained_as_a_tombstone_without_double_stopping() {
        let stopped = Arc::new(AtomicUsize::new(0));
        let server = ControlServer::new(
            Arc::new(TerminalBackend {
                stopped: stopped.clone(),
            }),
            test_pairing(),
            test_identity(),
        );
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                device_id: None,
                source_index: 0,
                source_name: "Main".into(),
                width: 1_920,
                height: 1_080,
                fps_target: 60,
                quality_state: "native".into(),
                capture_backend: "screenCaptureKit".into(),
                content_mode: "interactive".into(),
                encoder_experiment: EncoderExperiment::Auto,
                viewer_addr: "192.168.0.2:5001".into(),
                viewer_port: 5001,
                media_transport: "udp".into(),
                udp_stability: None,
                media_key: [0u8; 32],
                input_enabled: true,
                input_rate_hz: 120,
                terminal_since: None,
                terminal_error: None,
                backend_released: false,
                lifecycle: Lifecycle::default(),
                transport_owner: None,
                authorization: None,
            },
        );

        server.force_stop_session(1).unwrap();
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
        let retained = server.snapshot();
        assert_eq!(retained.sessions.len(), 1);
        assert_eq!(retained.sessions[0].stats.state, "stopped");
        assert!(!retained.sessions[0].input_enabled);
        assert_eq!(
            retained.sessions[0].stats.error.as_deref(),
            Some("host operator stopped the stream")
        );

        server
            .sessions
            .lock()
            .unwrap()
            .live
            .get_mut(&1)
            .unwrap()
            .terminal_since =
            Some(Instant::now() - TERMINAL_SESSION_RETENTION - Duration::from_millis(1));

        assert!(server.snapshot().sessions.is_empty());
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn set_session_input_toggles_a_live_session() {
        let server = ControlServer::new(backend(), test_pairing(), test_identity());
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                device_id: None,
                source_index: 0,
                source_name: "Main".into(),
                width: 1_920,
                height: 1_080,
                fps_target: 60,
                quality_state: "native".into(),
                capture_backend: "screenCaptureKit".into(),
                content_mode: "interactive".into(),
                encoder_experiment: EncoderExperiment::Auto,
                viewer_addr: "192.168.0.2:5001".into(),
                viewer_port: 5001,
                media_transport: "udp".into(),
                udp_stability: None,
                media_key: [0u8; 32],
                input_enabled: false,
                input_rate_hz: 120,
                terminal_since: None,
                terminal_error: None,
                backend_released: false,
                lifecycle: Lifecycle::default(),
                transport_owner: None,
                authorization: None,
            },
        );

        assert!(server.input_permission().unwrap());
        assert!(!server.snapshot().sessions[0].input_enabled);
        server.set_session_input(1, true).unwrap();
        let session = server.snapshot().sessions.remove(0);
        assert!(session.input_enabled);
        assert_eq!(session.input_rate_hz, 120);
    }

    #[test]
    fn screen_permission_passes_the_backend_answer_through() {
        // The fake has no TCC gate, so the trait default reports granted.
        let server = ControlServer::new(backend(), test_pairing(), test_identity());
        assert!(server.screen_permission().unwrap());

        struct ScreenDeniedBackend;
        impl CaptureBackend for ScreenDeniedBackend {
            fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
                Ok(Vec::new())
            }

            fn start(
                &self,
                _source_index: u32,
                _ip: &str,
                _port: u16,
                _w: u32,
                _h: u32,
                _fps: u32,
                _capture_backend: &str,
                _media_transport: &str,
                _content_mode: &str,
                _encoder_experiment: EncoderExperiment,
                _udp_stability: &AppliedUdpStability,
                _media_key: &[u8; 32],
                _access: Option<&crate::source_grants::CaptureAccess>,
            ) -> Result<u32, String> {
                Err("not under test".into())
            }

            fn stop(&self, _handle: u32) -> Result<(), String> {
                Ok(())
            }

            fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
                Err("not under test".into())
            }

            fn screen_permission(&self) -> Result<bool, String> {
                Ok(false)
            }
        }

        let server = ControlServer::new(
            Arc::new(ScreenDeniedBackend),
            test_pairing(),
            test_identity(),
        );
        assert!(!server.screen_permission().unwrap());
    }

    fn input_test_backend(permission: bool) -> Arc<FakeBackend> {
        Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                source_id: Some("test:display:0".into()),
                index: 0,
                name: "Main".into(),
                width: 1920,
                height: 1080,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: permission,
            input_calls: Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn task10_start_stream_requires_host_input_approval_even_with_os_permission() {
        let fake = input_test_backend(true);
        let server = Arc::new(ControlServer::new(
            fake.clone(),
            test_pairing(),
            test_identity(),
        ));

        let resp = server
            .dispatch(
                "startStream",
                serde_json::json!({
                    "sourceIndex": 0,
                    "viewerPort": 5001,
                    "width": 1920,
                    "height": 1080,
                    "fps": 60,
                    "mediaTransport": "udp",
                    "mediaKey": TEST_MEDIA_KEY
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");

        let state = server.sessions.lock().unwrap();
        let session = state.live.values().next().unwrap();
        assert!(
            !session.input_enabled,
            "OS permission is not Host approval: {resp}"
        );
        assert!(
            fake.input_calls.lock().unwrap().is_empty(),
            "must not enable input: {resp}"
        );
    }

    #[tokio::test]
    async fn revoking_a_device_stops_its_live_sessions_immediately() {
        let fake = input_test_backend(true);
        // 시작 등록 직전의 페어링 재검사(F02)가 있으므로 장치를 실제로 페어링한다.
        let pairing = test_pairing();
        let _ = direct_pair_token(&pairing, "viewer-1");
        let server = Arc::new(ControlServer::new(fake.clone(), pairing, test_identity()));

        let resp = server
            .dispatch(
                "startStream",
                serde_json::json!({
                    "sourceIndex": 0,
                    "viewerPort": 5001,
                    "width": 1920,
                    "height": 1080,
                    "fps": 60,
                    "mediaTransport": "udp",
                    "mediaKey": TEST_MEDIA_KEY
                }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(server.sessions.lock().unwrap().live.len(), 1);

        // 철회 즉시 세션이 사라지고 백엔드 stop(사유 2)이 불린다.
        let stopped = server.stop_sessions_for_device("viewer-1");
        assert_eq!(stopped, 1);
        assert_eq!(server.sessions.lock().unwrap().live.len(), 0);
        assert!(fake.stops.load(std::sync::atomic::Ordering::SeqCst) >= 1);
        // 다른 장치 철회는 세션을 건드리지 않는다.
        assert_eq!(server.stop_sessions_for_device("viewer-2"), 0);
    }

    #[tokio::test]
    async fn start_stream_leaves_input_off_without_permission() {
        let fake = input_test_backend(false);
        let server = Arc::new(ControlServer::new(
            fake.clone(),
            test_pairing(),
            test_identity(),
        ));

        let resp = server
            .dispatch(
                "startStream",
                serde_json::json!({
                    "sourceIndex": 0,
                    "viewerPort": 5001,
                    "width": 1920,
                    "height": 1080,
                    "fps": 60,
                    "mediaTransport": "udp",
                    "mediaKey": TEST_MEDIA_KEY
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");

        let state = server.sessions.lock().unwrap();
        let session = state.live.values().next().unwrap();
        assert!(!session.input_enabled, "{resp}");
        assert!(fake.input_calls.lock().unwrap().is_empty(), "{resp}");
    }

    #[tokio::test]
    async fn second_start_stream_does_not_steal_explicit_host_input_approval() {
        // 입력 중재(U4b): 마지막 세션이 이긴다. 두 번째 startStream이 입력을
        // 얻으면 첫 세션의 입력은 세션 상태와 백엔드 양쪽에서 꺼진다.
        let fake = input_test_backend(true);
        // 시작 등록 직전의 페어링 재검사(F02)가 있으므로 장치를 실제로 페어링한다.
        let pairing = test_pairing();
        let _ = direct_pair_token(&pairing, "viewer-2");
        let server = Arc::new(ControlServer::new(fake.clone(), pairing, test_identity()));
        {
            // 기존 시드 패턴: 입력이 켜진 라이브 세션을 직접 심는다. 다음
            // startStream이 id 2를 받도록 카운터도 맞춘다.
            let mut st = server.sessions.lock().unwrap();
            st.live.insert(
                1,
                Session {
                    handle: 7,
                    device_id: Some("viewer-1".into()),
                    source_index: 0,
                    source_name: "Main".into(),
                    width: 1_920,
                    height: 1_080,
                    fps_target: 60,
                    quality_state: "native".into(),
                    capture_backend: "screenCaptureKit".into(),
                    content_mode: "interactive".into(),
                    encoder_experiment: EncoderExperiment::Auto,
                    viewer_addr: "192.168.0.9:5002".into(),
                    viewer_port: 5002,
                    media_transport: "udp".into(),
                    udp_stability: None,
                    media_key: [0u8; 32],
                    input_enabled: true,
                    input_rate_hz: 120,
                    terminal_since: None,
                    terminal_error: None,
                    backend_released: false,
                    lifecycle: Lifecycle::default(),
                    transport_owner: None,
                    authorization: None,
                },
            );
            st.next = 2;
        }

        let resp = server
            .dispatch(
                "startStream",
                serde_json::json!({
                    "sourceIndex": 0,
                    "viewerPort": 5001,
                    "width": 1920,
                    "height": 1080,
                    "fps": 60,
                    "mediaTransport": "udp",
                    "mediaKey": TEST_MEDIA_KEY
                }),
                "192.168.0.9",
                Some("viewer-2"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");

        let state = server.sessions.lock().unwrap();
        assert_eq!(state.live.len(), 2, "{resp}");
        assert!(
            state.live.get(&1).unwrap().input_enabled,
            "older session preserves Host approval: {resp}"
        );
        assert!(
            !state.live.get(&2).unwrap().input_enabled,
            "new session starts input-off: {resp}"
        );
        drop(state);
        // 새 세션 활성화(true) 뒤 이전 세션도 백엔드에서 꺼진다(false).
        assert_eq!(*fake.input_calls.lock().unwrap(), vec![], "{resp}");
    }

    #[tokio::test]
    async fn start_stream_without_input_permission_leaves_other_sessions_enabled() {
        // OS 입력 권한이 없어 새 세션 입력이 꺼졌다면 기존 세션은 그대로다.
        let fake = input_test_backend(false);
        // 시작 등록 직전의 페어링 재검사(F02)가 있으므로 장치를 실제로 페어링한다.
        let pairing = test_pairing();
        let _ = direct_pair_token(&pairing, "viewer-2");
        let server = Arc::new(ControlServer::new(fake.clone(), pairing, test_identity()));
        {
            let mut st = server.sessions.lock().unwrap();
            st.live.insert(
                1,
                Session {
                    handle: 7,
                    device_id: Some("viewer-1".into()),
                    source_index: 0,
                    source_name: "Main".into(),
                    width: 1_920,
                    height: 1_080,
                    fps_target: 60,
                    quality_state: "native".into(),
                    capture_backend: "screenCaptureKit".into(),
                    content_mode: "interactive".into(),
                    encoder_experiment: EncoderExperiment::Auto,
                    viewer_addr: "192.168.0.9:5002".into(),
                    viewer_port: 5002,
                    media_transport: "udp".into(),
                    udp_stability: None,
                    media_key: [0u8; 32],
                    input_enabled: true,
                    input_rate_hz: 120,
                    terminal_since: None,
                    terminal_error: None,
                    backend_released: false,
                    lifecycle: Lifecycle::default(),
                    transport_owner: None,
                    authorization: None,
                },
            );
            st.next = 2;
        }

        let resp = server
            .dispatch(
                "startStream",
                serde_json::json!({
                    "sourceIndex": 0,
                    "viewerPort": 5001,
                    "width": 1920,
                    "height": 1080,
                    "fps": 60,
                    "mediaTransport": "udp",
                    "mediaKey": TEST_MEDIA_KEY
                }),
                "192.168.0.9",
                Some("viewer-2"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");

        let state = server.sessions.lock().unwrap();
        assert!(
            state.live.get(&1).unwrap().input_enabled,
            "others must stay untouched: {resp}"
        );
        assert!(!state.live.get(&2).unwrap().input_enabled, "{resp}");
        drop(state);
        assert!(fake.input_calls.lock().unwrap().is_empty(), "{resp}");
    }

    // -- 클립보드 텍스트 동기화(U5, docs/07 §20) -----------------------------

    struct FakeClipboard {
        text: Mutex<String>,
        writes: AtomicUsize,
        image: Mutex<Option<Vec<u8>>>,
        image_writes: AtomicUsize,
        revision: AtomicUsize,
        reads: AtomicUsize,
        image_reads: AtomicUsize,
    }

    impl FakeClipboard {
        fn new(text: &str) -> Self {
            Self {
                text: Mutex::new(text.to_owned()),
                writes: AtomicUsize::new(0),
                image: Mutex::new(None),
                image_writes: AtomicUsize::new(0),
                revision: AtomicUsize::new(0),
                reads: AtomicUsize::new(0),
                image_reads: AtomicUsize::new(0),
            }
        }
    }

    impl ClipboardBackend for FakeClipboard {
        fn revision(&self) -> Result<Option<u64>, String> {
            Ok(match self.revision.load(Ordering::SeqCst) {
                0 => None,
                value => Some(value as u64),
            })
        }
        fn read_text(&self) -> Result<String, String> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.text.lock().unwrap().clone())
        }

        fn write_text(&self, text: &str) -> Result<(), String> {
            *self.text.lock().unwrap() = text.to_owned();
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn read_image_png(&self) -> Result<Option<Vec<u8>>, String> {
            self.image_reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.image.lock().unwrap().clone())
        }

        fn write_image_png(&self, png: &[u8]) -> Result<(), String> {
            *self.image.lock().unwrap() = Some(png.to_vec());
            self.image_writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn clipboard_server(gate: bool) -> (Arc<ControlServer>, Arc<FakeClipboard>) {
        let server = Arc::new(ControlServer::new(
            backend(),
            test_pairing(),
            test_identity(),
        ));
        server.set_clipboard_share(gate);
        let clipboard = Arc::new(FakeClipboard::new(""));
        server.set_clipboard(clipboard.clone());
        (server, clipboard)
    }

    #[test]
    fn native_clipboard_revision_skips_payload_only_for_validated_matching_hash() {
        let (server, clipboard) = clipboard_server(true);
        clipboard.revision.store(1, Ordering::SeqCst);
        *clipboard.image.lock().unwrap() = Some(vec![1, 2, 3]);
        let first = server.handle_get_clipboard(json!({ "hash": "unknown" }), None);
        let hash = first["result"]["hash"].as_str().unwrap();
        let second = server.handle_get_clipboard(json!({ "hash": hash }), None);
        assert_eq!(second["result"]["unchanged"], true);
        assert_eq!(clipboard.reads.load(Ordering::SeqCst), 1);
        assert_eq!(clipboard.image_reads.load(Ordering::SeqCst), 1);
        let different_caller = server.handle_get_clipboard(json!({ "hash": "other" }), None);
        assert_eq!(different_caller["result"]["unchanged"], false);
        assert_eq!(clipboard.image_reads.load(Ordering::SeqCst), 2);
        clipboard.revision.store(2, Ordering::SeqCst);
        server.handle_get_clipboard(json!({ "hash": hash }), None);
        assert_eq!(clipboard.image_reads.load(Ordering::SeqCst), 3);
        clipboard.revision.store(0, Ordering::SeqCst);
        server.handle_get_clipboard(json!({ "hash": hash }), None);
        server.handle_get_clipboard(json!({ "hash": hash }), None);
        assert_eq!(clipboard.image_reads.load(Ordering::SeqCst), 5);
        server.set_clipboard_share(false);
        assert_eq!(
            server.handle_get_clipboard(json!({ "hash": hash }), None)["error"],
            "clipboard share disabled"
        );
        assert_eq!(clipboard.reads.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn clipboard_commands_are_rejected_while_the_host_gate_is_closed() {
        let (server, clipboard) = clipboard_server(false);
        let set = server
            .dispatch(
                "setClipboard",
                json!({ "text": "hello" }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(set["error"], "clipboard share disabled", "{set}");
        let get = server
            .dispatch(
                "getClipboard",
                json!({ "hash": "00" }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(get["error"], "clipboard share disabled", "{get}");
        assert_eq!(clipboard.writes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn oversize_clipboard_text_is_rejected() {
        let (server, clipboard) = clipboard_server(true);
        let oversized = "가".repeat(CLIPBOARD_MAX_CHARS + 1);
        let resp = server
            .dispatch(
                "setClipboard",
                json!({ "text": oversized }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["error"], "clipboard too large", "{resp}");
        assert_eq!(clipboard.writes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn set_then_get_clipboard_round_trips_with_hash_short_circuit() {
        let (server, clipboard) = clipboard_server(true);
        let resp = server
            .dispatch(
                "setClipboard",
                json!({ "text": "클립보드 동기화" }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");

        let unknown_hash = server
            .dispatch(
                "getClipboard",
                json!({ "hash": "deadbeef" }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(unknown_hash["result"]["unchanged"], false, "{unknown_hash}");
        assert_eq!(
            unknown_hash["result"]["text"], "클립보드 동기화",
            "{unknown_hash}"
        );
        let hash = unknown_hash["result"]["hash"].as_str().unwrap().to_owned();

        // 같은 해시로 다시 물으면 unchanged로 짧게 끊낸다.
        let short = server
            .dispatch(
                "getClipboard",
                json!({ "hash": hash }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(short["result"]["unchanged"], true, "{short}");

        // 루프 방지: 호스트 현재 내용과 같은 setClipboard은 쓰지 않고 무시한다.
        let echo = server
            .dispatch(
                "setClipboard",
                json!({ "text": "클립보드 동기화" }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(echo["ok"], true, "{echo}");
        assert_eq!(clipboard.writes.load(Ordering::SeqCst), 1, "{echo}");
    }

    #[tokio::test]
    async fn clipboard_image_round_trips_and_echoes_are_suppressed() {
        let (server, clipboard) = clipboard_server(true);
        let png = vec![0x89, b'P', b'N', b'G', 1, 2, 3, 4];
        let image_base64 = base64::engine::general_purpose::STANDARD.encode(&png);

        // 최초 get: 이미지가 없으면 빈 텍스트 해시로 응답한다.
        let first = server
            .dispatch(
                "getClipboard",
                json!({ "hash": "00" }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(first["result"]["text"], "", "{first}");

        // 이미지 set → 정확히 한 번 쓴다.
        let set = server
            .dispatch(
                "setClipboard",
                json!({ "imageBase64": image_base64 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(set["ok"], true, "{set}");
        assert_eq!(clipboard.image_writes.load(Ordering::SeqCst), 1);

        // 같은 이미지 재전송(에코)은 쓰지 않는다.
        let echo = server
            .dispatch(
                "setClipboard",
                json!({ "imageBase64": image_base64 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(echo["ok"], true, "{echo}");
        assert_eq!(clipboard.image_writes.load(Ordering::SeqCst), 1);

        // get은 이미지를 base64로 돌려주고, 해시 제출에는 unchanged로
        // 짧게 끊난다.
        let got = server
            .dispatch(
                "getClipboard",
                json!({ "hash": "00" }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(got["result"]["imageBase64"], image_base64, "{got}");
        let hash = got["result"]["hash"].as_str().unwrap().to_owned();
        assert!(hash.starts_with("i:"), "{hash}");
        let unchanged = server
            .dispatch(
                "getClipboard",
                json!({ "hash": hash }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(unchanged["result"]["unchanged"], true, "{unchanged}");
    }

    #[tokio::test]
    async fn clipboard_rejects_ambiguous_and_oversized_image_args() {
        let (server, _clipboard) = clipboard_server(true);
        for args in [json!({ "text": "a", "imageBase64": "aGk=" }), json!({})] {
            let resp = server
                .dispatch("setClipboard", args, "192.168.0.9", Some("viewer-1"))
                .await;
            assert_eq!(resp["ok"], false, "{resp}");
            assert!(
                resp["error"].as_str().unwrap().contains("exactly one"),
                "{resp}"
            );
        }
        // base64가 아니면 거부.
        let resp = server
            .dispatch(
                "setClipboard",
                json!({ "imageBase64": "!!!not-base64!!!" }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
    }

    #[tokio::test]
    async fn clipboard_write_audits_metadata_only() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "leftcar-clipboard-audit-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let (server, _) = clipboard_server(true);
        server.set_audit(Arc::new(crate::audit::SessionAudit::new(Some(
            path.clone(),
        ))));

        let secret = "감사 로그에 남으면 안 되는 본문";
        let resp = server
            .dispatch(
                "setClipboard",
                json!({ "text": secret }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");

        let body = std::fs::read_to_string(&path).unwrap();
        let record: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(record["event"], "clipboard_write", "{body}");
        assert_eq!(record["bytes"], secret.len(), "{body}");
        let device = record["device"].as_str().unwrap();
        let pseudonym = device.strip_prefix("dev:").unwrap();
        assert_eq!(pseudonym.len(), 24, "{body}");
        assert!(
            pseudonym
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "{body}"
        );
        assert_ne!(device, "viewer-1", "{body}");
        assert!(!body.contains("viewer-1"), "{body}");
        assert!(
            !body.contains(secret),
            "audit must never contain clipboard text: {body}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn reconfigure_stream_carries_input_enablement_to_replacement() {
        let fake = input_test_backend(true);
        let server = Arc::new(ControlServer::new(
            fake.clone(),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);
        server.set_session_input(1, true).unwrap();
        assert_eq!(*fake.input_calls.lock().unwrap(), vec![(7, true)]);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 3840,
                    "height": 2160,
                    "fps": 60,
                    "qualityState": "native"
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");

        // The replacement backend handle must re-receive the enablement the
        // viewer already had; a resolution switch cannot silently strip
        // control.
        assert_eq!(
            *fake.input_calls.lock().unwrap(),
            vec![(7, true), (7, true)],
            "{resp}"
        );
        let state = server.sessions.lock().unwrap();
        assert!(state.live.get(&1).unwrap().input_enabled, "{resp}");
    }

    /// Seed one live UDP session directly (the dispatcher's split start
    /// validation requires the diagnostic flag; these tests target the
    /// reconfigure path with a session already established).
    fn seed_live_session(
        server: &ControlServer,
        encoder_experiment: EncoderExperiment,
        width: u32,
        height: u32,
    ) {
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                device_id: None,
                source_index: 0,
                source_name: "Main".into(),
                width,
                height,
                fps_target: 60,
                quality_state: "native".into(),
                capture_backend: "screenCaptureKit".into(),
                content_mode: "video".into(),
                encoder_experiment,
                viewer_addr: "192.168.0.9:5002".into(),
                viewer_port: 5002,
                media_transport: "udp".into(),
                udp_stability: None,
                media_key: [0u8; 32],
                input_enabled: false,
                input_rate_hz: 180,
                terminal_since: None,
                terminal_error: None,
                backend_released: false,
                lifecycle: Lifecycle::default(),
                transport_owner: None,
                authorization: None,
            },
        );
    }

    /// Backend whose capture stats report a positive split capture queue age.
    struct CaptureQueueAgeBackend;

    impl CaptureBackend for CaptureQueueAgeBackend {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            Ok(Vec::new())
        }

        fn start(
            &self,
            _source_index: u32,
            _ip: &str,
            _port: u16,
            _w: u32,
            _h: u32,
            _fps: u32,
            _capture_backend: &str,
            _media_transport: &str,
            _content_mode: &str,
            _encoder_experiment: EncoderExperiment,
            _udp_stability: &AppliedUdpStability,
            _media_key: &[u8; 32],
            _access: Option<&crate::source_grants::CaptureAccess>,
        ) -> Result<u32, String> {
            Ok(9)
        }

        fn stop(&self, _handle: u32) -> Result<(), String> {
            Ok(())
        }

        fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
            Ok(StatsInfo {
                state: "running".into(),
                split_capture_queue_oldest_us: Some(45_600),
                ..StatsInfo::default()
            })
        }
    }

    #[tokio::test]
    async fn status_response_forwards_split_capture_queue_oldest_us() {
        // 네이티브 캡처 stats의 splitCaptureQueueOldestUs가 status 응답까지
        // 생존해야 한다 (파싱 → SessionView 전달 → Host JSON).
        let server = Arc::new(ControlServer::new(
            Arc::new(CaptureQueueAgeBackend),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::Auto, 3840, 2160);

        let resp = server
            .dispatch("getStatus", serde_json::json!({}), "192.168.0.9", None)
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(
            resp["result"]["sessions"][0]["splitCaptureQueueOldestUs"], 45_600,
            "{resp}"
        );
    }

    /// Backend that records every start attempt and can fail specific
    /// experiments, to exercise the reconfigure rollback path.
    struct ReplacingBackend {
        starts: Mutex<Vec<(EncoderExperiment, u32, u32)>>,
        fail_experiments: Vec<EncoderExperiment>,
        stops: AtomicUsize,
    }

    impl CaptureBackend for ReplacingBackend {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            Ok(vec![DisplayInfo {
                source_id: Some("test:display:0".into()),
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }])
        }

        fn encoder_experiments(
            &self,
        ) -> Result<Vec<control_contract::host::EncoderExperimentInfo>, String> {
            let mut experiments = control_contract::host::phase_a_encoder_experiments();
            experiments.push(control_contract::host::EncoderExperimentInfo {
                id: EncoderExperiment::SplitVertical,
                label: String::new(),
                hint: String::new(),
                requires_reconnect: true,
            });
            Ok(experiments)
        }

        fn start(
            &self,
            _source_index: u32,
            _ip: &str,
            _port: u16,
            width: u32,
            height: u32,
            _fps: u32,
            _capture_backend: &str,
            _media_transport: &str,
            _content_mode: &str,
            encoder_experiment: EncoderExperiment,
            _udp_stability: &AppliedUdpStability,
            _media_key: &[u8; 32],
            _access: Option<&crate::source_grants::CaptureAccess>,
        ) -> Result<u32, String> {
            self.starts
                .lock()
                .unwrap()
                .push((encoder_experiment, width, height));
            if self.fail_experiments.contains(&encoder_experiment) {
                return Err("simulated replacement failure".into());
            }
            Ok(7)
        }

        fn stop(&self, _handle: u32) -> Result<(), String> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
            Ok(StatsInfo {
                state: "running".into(),
                first_send_ms: 26,
                ..StatsInfo::default()
            })
        }
    }

    #[tokio::test]
    async fn reconfigure_stream_explicit_split_targets_exact_4k_and_reports_actual_mode() {
        let fake = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                source_id: Some("test:display:0".into()),
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: true,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        });
        let server = Arc::new(ControlServer::new(
            fake.clone(),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 3840,
                    "height": 2160,
                    "fps": 60,
                    "qualityState": "native",
                    "encoderExperiment": "splitVertical"
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        // 교체 백엔드는 요청된 split 모드로 시작한다.
        assert_eq!(
            *fake.encoder_experiment.lock().unwrap(),
            EncoderExperiment::SplitVertical,
            "{resp}"
        );
        // 라이브 기록과 수락 응답이 실제 모드로 일치한다.
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::SplitVertical);
        assert_eq!((session.width, session.height), (3840, 2160));
        drop(state);
        assert_eq!(
            resp["result"]["encoderExperiment"], "splitVertical",
            "{resp}"
        );
    }

    #[tokio::test]
    async fn reconfigure_stream_explicit_auto_leaves_split_for_single_path() {
        let fake = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                source_id: Some("test:display:0".into()),
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::SplitVertical),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        });
        let server = Arc::new(ControlServer::new(
            fake.clone(),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::SplitVertical, 3840, 2160);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 2560,
                    "height": 1440,
                    "fps": 60,
                    "qualityState": "native",
                    "encoderExperiment": "auto"
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(
            *fake.encoder_experiment.lock().unwrap(),
            EncoderExperiment::Auto,
            "{resp}"
        );
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::Auto);
        assert_eq!((session.width, session.height), (2560, 1440));
        drop(state);
        assert_eq!(resp["result"]["encoderExperiment"], "auto", "{resp}");
    }

    #[tokio::test]
    async fn reconfigure_stream_rejects_unsupported_split_shape_before_stopping() {
        let fake = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                source_id: Some("test:display:0".into()),
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: true,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        });
        let server = Arc::new(ControlServer::new(
            fake.clone(),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 2560,
                    "height": 1440,
                    "fps": 60,
                    "qualityState": "native",
                    "encoderExperiment": "splitVertical"
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        let message = resp["error"].as_str().unwrap_or_default();
        assert!(message.contains("splitVertical"), "{resp}");
        // 검증은 이전 백엔드 stop 전에 수행된다: 라이브 스트림은 그대로다.
        assert_eq!(fake.stops.load(Ordering::SeqCst), 0, "{resp}");
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::Auto);
        assert_eq!((session.width, session.height), (2560, 1440));
        assert_eq!(session.handle, 7);
        assert!(!session.backend_released);
        assert!(session.terminal_error.is_none());
    }

    #[tokio::test]
    async fn reconfigure_stream_rejects_unsupported_requested_mode_before_stopping() {
        let fake = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                source_id: Some("test:display:0".into()),
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        });
        let server = Arc::new(ControlServer::new(
            fake.clone(),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 3840,
                    "height": 2160,
                    "fps": 60,
                    "qualityState": "native",
                    "encoderExperiment": "splitHorizontal"
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        let message = resp["error"].as_str().unwrap_or_default();
        assert!(message.contains("unsupported encoder experiment"), "{resp}");
        assert_eq!(fake.stops.load(Ordering::SeqCst), 0, "{resp}");
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::Auto);
        assert_eq!(session.handle, 7);
        assert!(!session.backend_released);
        assert!(session.terminal_error.is_none());
    }

    #[tokio::test]
    async fn reconfigure_stream_failure_restores_previous_actual_mode() {
        let backend = Arc::new(ReplacingBackend {
            starts: Mutex::new(Vec::new()),
            fail_experiments: vec![EncoderExperiment::SplitVertical],
            stops: AtomicUsize::new(0),
        });
        let server = Arc::new(ControlServer::new(
            backend.clone(),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 3840,
                    "height": 2160,
                    "fps": 60,
                    "qualityState": "native",
                    "encoderExperiment": "splitVertical"
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        // 교체 실패 시 정확히 이전 실제 모드/형태로 복구된다.
        assert_eq!(
            *backend.starts.lock().unwrap(),
            vec![
                (EncoderExperiment::SplitVertical, 3840, 2160),
                (EncoderExperiment::Auto, 2560, 1440),
            ]
        );
        assert_eq!(backend.stops.load(Ordering::SeqCst), 1);
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::Auto);
        assert_eq!((session.width, session.height), (2560, 1440));
        assert_eq!(session.handle, 7);
        assert!(!session.backend_released);
        assert!(session.terminal_error.is_none());
    }

    /// Backend with two displays that records the source_index of every
    /// start, to exercise the reconfigure cheap-display-switch path.
    struct SourceSwitchBackend {
        starts: Mutex<Vec<(u32, u32, u32)>>,
        fail_sources: Vec<u32>,
        stops: AtomicUsize,
    }

    impl CaptureBackend for SourceSwitchBackend {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            Ok(vec![
                DisplayInfo {
                    source_id: Some("test:display:0".into()),
                    index: 0,
                    name: "Main".into(),
                    width: 1920,
                    height: 1080,
                },
                DisplayInfo {
                    source_id: Some("test:display:1".into()),
                    index: 1,
                    name: "Side".into(),
                    width: 2560,
                    height: 1440,
                },
            ])
        }

        fn start(
            &self,
            source_index: u32,
            _ip: &str,
            _port: u16,
            width: u32,
            height: u32,
            _fps: u32,
            _capture_backend: &str,
            _media_transport: &str,
            _content_mode: &str,
            _encoder_experiment: EncoderExperiment,
            _udp_stability: &AppliedUdpStability,
            _media_key: &[u8; 32],
            _access: Option<&crate::source_grants::CaptureAccess>,
        ) -> Result<u32, String> {
            self.starts
                .lock()
                .unwrap()
                .push((source_index, width, height));
            if self.fail_sources.contains(&source_index) {
                return Err("simulated source switch failure".into());
            }
            Ok(7)
        }

        fn stop(&self, _handle: u32) -> Result<(), String> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
            Ok(StatsInfo {
                state: "running".into(),
                first_send_ms: 26,
                ..StatsInfo::default()
            })
        }
    }

    #[tokio::test]
    async fn reconfigure_stream_switches_source_under_the_same_session() {
        // Cheap display switch (R7): the reconfigure path restarts the capture
        // backend on the requested display while the session id, viewer
        // address, and media port stay untouched.
        let backend = Arc::new(SourceSwitchBackend {
            starts: Mutex::new(Vec::new()),
            fail_sources: Vec::new(),
            stops: AtomicUsize::new(0),
        });
        let server = Arc::new(ControlServer::new(
            backend.clone(),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 1920,
                    "height": 1080,
                    "fps": 60,
                    "qualityState": "native",
                    "sourceIndex": 1
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        // The replacement capture started on the requested display...
        assert_eq!(
            *backend.starts.lock().unwrap(),
            vec![(1, 1920, 1080)],
            "{resp}"
        );
        assert_eq!(backend.stops.load(Ordering::SeqCst), 1, "{resp}");
        // ...and the live session now reports the new source.
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.source_index, 1);
        assert_eq!(session.source_name, "Side");
        assert_eq!((session.width, session.height), (1920, 1080));
        assert_eq!(session.handle, 7);
        assert!(!session.backend_released);
        drop(state);
        // The response echoes the accepted source for switching viewers.
        assert_eq!(resp["result"]["session"], 1, "{resp}");
        assert_eq!(resp["result"]["sourceIndex"], 1, "{resp}");
        assert_eq!(resp["result"]["sourceName"], "Side", "{resp}");
    }

    #[tokio::test]
    async fn reconfigure_stream_rejects_unknown_source_before_stopping() {
        // An out-of-range display is a pre-stop validation error: the live
        // stream must be left completely untouched.
        let backend = Arc::new(SourceSwitchBackend {
            starts: Mutex::new(Vec::new()),
            fail_sources: Vec::new(),
            stops: AtomicUsize::new(0),
        });
        let server = Arc::new(ControlServer::new(
            backend.clone(),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 1920,
                    "height": 1080,
                    "fps": 60,
                    "qualityState": "native",
                    "sourceIndex": 5
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        let message = resp["error"].as_str().unwrap_or_default();
        assert!(message.contains("source_refresh_required"), "{resp}");
        assert_eq!(backend.stops.load(Ordering::SeqCst), 0, "{resp}");
        assert!(backend.starts.lock().unwrap().is_empty(), "{resp}");
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.source_index, 0);
        assert_eq!(session.source_name, "Main");
        assert_eq!(session.handle, 7);
        assert!(!session.backend_released);
        assert!(session.terminal_error.is_none());
    }

    #[tokio::test]
    async fn reconfigure_stream_source_failure_restores_previous_source() {
        // If the replacement capture on the new display fails to produce a
        // frame, the rollback restarts the ORIGINAL source under the same
        // session id — never leaves the session pointing at the new display.
        let backend = Arc::new(SourceSwitchBackend {
            starts: Mutex::new(Vec::new()),
            fail_sources: vec![1],
            stops: AtomicUsize::new(0),
        });
        let server = Arc::new(ControlServer::new(
            backend.clone(),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 1920,
                    "height": 1080,
                    "fps": 60,
                    "qualityState": "native",
                    "sourceIndex": 1
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        // First attempt on the new source, rollback on the previous one.
        assert_eq!(
            *backend.starts.lock().unwrap(),
            vec![(1, 1920, 1080), (0, 2560, 1440)],
            "{resp}"
        );
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.source_index, 0);
        assert_eq!(session.source_name, "Main");
        assert_eq!((session.width, session.height), (2560, 1440));
        assert_eq!(session.handle, 7);
        assert!(!session.backend_released);
        assert!(session.terminal_error.is_none());
    }

    // split 재구성 회귀 테스트(구 viewer_display_matching 모듌에서 이전)
    #[tokio::test]
    async fn reconfigure_stream_demotes_split_replacement_for_non_4k_target() {
        // 4K split 세션을 1080p로 재구성하면 교체 캡처가 Auto 단일 인코더로
        // 시작해야 한다 — 뷰어는 이미 demote된 단일 prepare/rebind 경로로
        // 전환했으므로, 교체 세션이 splitVertical로 시작되면 (엔진 검증
        // 실패 또는 포트 기하 불일치로) 복구된 이전 세션은 뷰어 피드백을
        // 영원히 받지 못하고 feedback timeout으로 죽는다 (실기 2026-09-07
        // 4K split → 1080p 프리셋 실패 재현).
        let fake = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                source_id: Some("test:display:0".into()),
                index: 0,
                name: "Main".into(),
                width: 1920,
                height: 1080,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        });
        let server = Arc::new(ControlServer::new(
            fake.clone(),
            test_pairing(),
            test_identity(),
        ));
        // dispatcher의 split 시작 검증은 진단 플래그를 요구하므로 라이브
        // split 세션을 직접 시드한다 — 이 테스트의 대상은 reconfigure의
        // 교체 세션 실험 선택이다.
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                device_id: None,
                source_index: 0,
                source_name: "Main".into(),
                width: 3840,
                height: 2160,
                fps_target: 60,
                quality_state: "native".into(),
                capture_backend: "screenCaptureKit".into(),
                content_mode: "video".into(),
                encoder_experiment: EncoderExperiment::SplitVertical,
                viewer_addr: "192.168.0.9:5002".into(),
                viewer_port: 5002,
                media_transport: "udp".into(),
                udp_stability: None,
                media_key: [0u8; 32],
                input_enabled: false,
                input_rate_hz: 180,
                terminal_since: None,
                terminal_error: None,
                backend_released: false,
                lifecycle: Lifecycle::default(),
                transport_owner: None,
                authorization: None,
            },
        );

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 1920,
                    "height": 1080,
                    "fps": 60,
                    "qualityState": "native"
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(resp["result"]["width"], 1920, "{resp}");
        // 구 요청(mode 생략)도 실제 수락 모드를 응답에 보고한다.
        assert_eq!(resp["result"]["encoderExperiment"], "auto", "{resp}");
        // 교체 캡처 백엔드는 demote된 Auto로 시작해야 한다.
        assert_eq!(
            *fake.encoder_experiment.lock().unwrap(),
            EncoderExperiment::Auto,
            "replacement capture must start with the demoted encoder experiment",
        );
        // 세션 기록도 교체 결과와 일치한다.
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.width, 1920);
        assert_eq!(session.height, 1080);
        assert_eq!(session.encoder_experiment, EncoderExperiment::Auto);
        assert_eq!(session.handle, 7);
    }

    #[tokio::test]
    async fn reconfigure_stream_omitted_mode_rejects_split_4k_90fps_before_stopping() {
        // mode 생략(구 뷰어) 요청이 정확한 4K를 유지하면 이전 split 실험이
        // 유지되지만, fps가 60이 아니면 split 제약 위반이다. 교체 실험을
        // 검증하기 전에는 이전 백엔드를 멈추지 않는다: 거부 시 라이브
        // 스트림과 기록은 그대로여야 한다.
        let fake = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                source_id: Some("test:display:0".into()),
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        });
        let server = Arc::new(ControlServer::new(
            fake.clone(),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::SplitVertical, 3840, 2160);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 3840,
                    "height": 2160,
                    "fps": 90,
                    "qualityState": "native"
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        let message = resp["error"].as_str().unwrap_or_default();
        assert!(message.contains("splitVertical"), "{resp}");
        assert_eq!(fake.stops.load(Ordering::SeqCst), 0, "{resp}");
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::SplitVertical);
        assert_eq!((session.width, session.height), (3840, 2160));
        assert_eq!(session.fps_target, 60);
        assert_eq!(session.handle, 7);
        assert!(!session.backend_released);
        assert!(session.terminal_error.is_none());
    }

    #[tokio::test]
    async fn reconfigure_stream_omitted_mode_rejects_split_4k_30fps_before_stopping() {
        let fake = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                source_id: Some("test:display:0".into()),
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        });
        let server = Arc::new(ControlServer::new(
            fake.clone(),
            test_pairing(),
            test_identity(),
        ));
        seed_live_session(&server, EncoderExperiment::SplitVertical, 3840, 2160);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 3840,
                    "height": 2160,
                    "fps": 30,
                    "qualityState": "native"
                }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        let message = resp["error"].as_str().unwrap_or_default();
        assert!(message.contains("splitVertical"), "{resp}");
        assert_eq!(fake.stops.load(Ordering::SeqCst), 0, "{resp}");
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::SplitVertical);
        assert_eq!((session.width, session.height), (3840, 2160));
        assert_eq!(session.fps_target, 60);
        assert_eq!(session.handle, 7);
        assert!(!session.backend_released);
        assert!(session.terminal_error.is_none());
    }

    // -- 파일 전송 v1 (게이트·검증·라운드트립) -----------------------------

    use std::path::PathBuf;

    /// 파일 전송 테스트용 서버: 임시 incoming 루트와 주입 가능한 게이트.
    fn file_test_server(tag: &str) -> (ControlServer, PathBuf) {
        use crate::settings::SharedSettings;
        let server = ControlServer::new(backend(), test_pairing(), test_identity());
        let mut root = std::env::temp_dir();
        root.push(format!(
            "leftcar-ft-dispatch-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        server
            .file_transfers
            .set_incoming_root_for_tests(root.clone());
        server.set_settings(SharedSettings::in_memory());
        (server, root)
    }

    fn enable_file_share(server: &ControlServer) {
        server.settings.get().unwrap().set_file_share(true).unwrap();
    }

    #[tokio::test]
    async fn file_commands_require_an_authenticated_device() {
        let (server, root) = file_test_server("unauth");
        enable_file_share(&server);
        for command in ["sendFileBegin", "listShareQueue", "fetchFileBegin"] {
            let resp = server
                .dispatch(
                    command,
                    serde_json::json!({ "name": "a.txt" }),
                    "192.168.0.9",
                    None,
                )
                .await;
            assert_eq!(resp["error"], "unauthorized", "{command}: {resp}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn file_share_gate_defaults_off_and_rejects_uploads() {
        let (server, root) = file_test_server("gateoff");
        let resp = server
            .dispatch(
                "sendFileBegin",
                serde_json::json!({ "name": "a.txt", "size": 3 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        assert_eq!(resp["error"], "file share disabled", "{resp}");

        enable_file_share(&server);
        let resp = server
            .dispatch(
                "sendFileBegin",
                serde_json::json!({ "name": "a.txt", "size": 3 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert!(
            resp["result"]["fileToken"].as_str().unwrap().len() == 32,
            "{resp}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn file_upload_rejects_unsafe_names_and_oversize() {
        let (server, root) = file_test_server("names");
        enable_file_share(&server);
        for name in ["../x", "a/b", "a\\b", ".hidden", "..", ""] {
            let resp = server
                .dispatch(
                    "sendFileBegin",
                    serde_json::json!({ "name": name, "size": 3 }),
                    "192.168.0.9",
                    Some("viewer-1"),
                )
                .await;
            assert_eq!(resp["ok"], false, "{name}: {resp}");
        }
        let resp = server
            .dispatch(
                "sendFileBegin",
                serde_json::json!({ "name": "big.bin", "size": crate::file_transfer::MAX_FILE_SIZE + 1 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn file_chunk_enforcement_sequence_oversize_and_ownership() {
        let (server, root) = file_test_server("chunks");
        enable_file_share(&server);
        let resp = server
            .dispatch(
                "sendFileBegin",
                serde_json::json!({ "name": "seq.bin", "size": 6 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        let token = resp["result"]["fileToken"].as_str().unwrap().to_owned();

        // 순서가 어긋난 오프셋은 거부된다.
        let resp = server
            .dispatch(
                "sendFileChunk",
                serde_json::json!({ "fileToken": token, "dataBase64": "eHg=", "offset": 2 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");

        // 선언 크기 초과는 거부된다("xyzxyzx" = 7바이트 > size 6).
        let resp = server
            .dispatch(
                "sendFileChunk",
                serde_json::json!({ "fileToken": token, "dataBase64": "eHl6enl6eg==", "offset": 0 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");

        // 1 MiB를 넘는 청크는 거부된다.("AAA" 349,525개 + 2바이트 = 1 MiB + 1B)
        let resp = server
            .dispatch(
                "sendFileBegin",
                serde_json::json!({ "name": "cap.bin", "size": 20 * 1024 * 1024 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        let cap_token = resp["result"]["fileToken"].as_str().unwrap().to_owned();
        let oversized = format!("{}QQI=", "QUFB".repeat(349_525));
        let resp = server
            .dispatch(
                "sendFileChunk",
                serde_json::json!({
                    "fileToken": cap_token,
                    "dataBase64": oversized,
                    "offset": 0
                }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        assert_eq!(resp["error"], "chunk is too large", "{resp}");

        // 다른 장치의 토큰은 소유 검사에 걸린다.
        let resp = server
            .dispatch(
                "sendFileChunk",
                serde_json::json!({ "fileToken": token, "dataBase64": "eHg=", "offset": 0 }),
                "192.168.0.9",
                Some("viewer-2"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        assert_eq!(resp["error"], "unknown file token", "{resp}");

        // 정상 순차 전송은 written 누적을 돌려준다.
        let resp = server
            .dispatch(
                "sendFileChunk",
                serde_json::json!({ "fileToken": token, "dataBase64": "eHl6", "offset": 0 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["result"]["written"], 3, "{resp}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn file_roundtrip_upload_then_fetch_with_byte_equality() {
        let (server, root) = file_test_server("roundtrip");
        enable_file_share(&server);

        // 업로드: begin → chunks → end("Hello world\n" = 12바이트).
        let resp = server
            .dispatch(
                "sendFileBegin",
                serde_json::json!({ "name": "roundtrip.txt", "size": 12 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        let token = resp["result"]["fileToken"].as_str().unwrap().to_owned();
        for (data, offset) in [("SGVsbG8g", 0u64), ("d29ybGQK", 6u64)] {
            let resp = server
                .dispatch(
                    "sendFileChunk",
                    serde_json::json!({ "fileToken": token, "dataBase64": data, "offset": offset }),
                    "192.168.0.9",
                    Some("viewer-1"),
                )
                .await;
            assert_eq!(resp["ok"], true, "{resp}");
        }
        let resp = server
            .dispatch(
                "sendFileEnd",
                serde_json::json!({ "fileToken": token }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(
            resp["result"]["path"], "Downloads/leftcar/viewer-1/roundtrip.txt",
            "{resp}"
        );
        let uploaded = std::fs::read(root.join("viewer-1").join("roundtrip.txt")).unwrap();
        assert_eq!(uploaded, b"Hello world\n");

        // 다운로드: 대기열 등록 → fetchBegin → chunks → fetchEnd.
        let source = root.join("viewer-1").join("roundtrip.txt");
        let entry = server
            .file_transfers
            .add_share_file(source.clone())
            .unwrap();
        let resp = server
            .dispatch(
                "listShareQueue",
                serde_json::json!({}),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(
            resp["result"]["queue"][0]["name"], "roundtrip.txt",
            "{resp}"
        );

        let resp = server
            .dispatch(
                "fetchFileBegin",
                serde_json::json!({ "queueId": entry.queue_id }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(resp["result"]["size"], 12, "{resp}");
        let fetch_token = resp["result"]["fileToken"].as_str().unwrap().to_owned();

        let mut fetched = Vec::new();
        for (offset, length) in [(0u64, 6u64), (6, 6)] {
            let resp = server
                .dispatch(
                    "fetchFileChunk",
                    serde_json::json!({
                        "fileToken": fetch_token,
                        "offset": offset,
                        "length": length
                    }),
                    "192.168.0.9",
                    Some("viewer-1"),
                )
                .await;
            assert_eq!(resp["ok"], true, "{resp}");
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(resp["result"]["data"].as_str().unwrap())
                .unwrap();
            fetched.extend(decoded);
        }
        assert_eq!(fetched, uploaded, "fetched bytes must match the upload");

        let resp = server
            .dispatch(
                "fetchFileEnd",
                serde_json::json!({ "fileToken": fetch_token }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn fetch_rejects_missing_entries_and_bad_ranges() {
        let (server, root) = file_test_server("fetchbad");
        enable_file_share(&server);
        let source = root.join("gone.txt");
        std::fs::write(&source, b"abc").unwrap();
        let entry = server.file_transfers.add_share_file(source).unwrap();
        std::fs::remove_file(root.join("gone.txt")).unwrap();

        // 파일이 사라졌으면 fetchFileBegin은 실패한다.
        let resp = server
            .dispatch(
                "fetchFileBegin",
                serde_json::json!({ "queueId": entry.queue_id }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");

        // 존재하지 않는 큐 항목도 오류.
        let resp = server
            .dispatch(
                "fetchFileBegin",
                serde_json::json!({ "queueId": "deadbeef" }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");

        // 1 MiB를 넘는 length 요청은 거부된다.
        let live = root.join("live.txt");
        std::fs::write(&live, b"abc").unwrap();
        let entry = server.file_transfers.add_share_file(live).unwrap();
        let resp = server
            .dispatch(
                "fetchFileBegin",
                serde_json::json!({ "queueId": entry.queue_id }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        let token = resp["result"]["fileToken"].as_str().unwrap().to_owned();
        let resp = server
            .dispatch(
                "fetchFileChunk",
                serde_json::json!({
                    "fileToken": token,
                    "offset": 0,
                    "length": 1024 * 1024 + 1
                }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // -- 세션 범위 명령의 장치 소유 검사(F04) ---------------------------------

    /// 테스트용 직접 페어링: 소켓 없이 장치 토큰을 발금한다.
    fn direct_pair_token(pairing: &crate::pairing::PairingServer, device_id: &str) -> String {
        let view = pairing.begin_pairing("127.0.0.1", 7777);
        let token = pairing
            .pair_by_code(&view.code, device_id, device_id)
            .unwrap();
        pairing
            .update_source_grants(
                device_id,
                vec!["test:display:0".into(), "test:display:1".into()],
            )
            .0
            .unwrap();
        if let Some(auth) = pairing.authenticate(&token) {
            pairing.remember_catalog(
                &auth,
                &[control_contract::host::DisplayInfo {
                    source_id: Some("test:display:0".into()),
                    index: 0,
                    name: "test".into(),
                    width: 1920,
                    height: 1080,
                }],
            );
        }
        token
    }

    #[tokio::test]
    async fn cross_device_stop_stream_is_denied_without_existence_leak() {
        let stopped = Arc::new(AtomicUsize::new(0));
        let server = Arc::new(ControlServer::new(
            Arc::new(TerminalBackend {
                stopped: stopped.clone(),
            }),
            test_pairing(),
            test_identity(),
        ));
        insert_live_session(&server, 1, Some("viewer-1"));

        // 남의 세션은 없는 세션과 같은 오류로 답한다.
        let resp = server
            .dispatch(
                "stopStream",
                serde_json::json!({ "session": 1 }),
                "192.168.0.9",
                Some("viewer-2"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        assert_eq!(resp["error"], "no such session 1", "{resp}");
        assert_eq!(server.sessions.lock().unwrap().live.len(), 1);

        // 소유자는 종료할 수 있다.
        let resp = server
            .dispatch(
                "stopStream",
                serde_json::json!({ "session": 1 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert!(server.sessions.lock().unwrap().live.is_empty());
    }

    #[tokio::test]
    async fn cross_device_reconfigure_is_denied_before_touching_the_backend() {
        let fake = input_test_backend(true);
        let server = Arc::new(ControlServer::new(
            fake.clone(),
            test_pairing(),
            test_identity(),
        ));
        insert_live_session(&server, 1, Some("viewer-1"));

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 2560,
                    "height": 1440,
                    "fps": 60,
                    "qualityState": "native"
                }),
                "192.168.0.9",
                Some("viewer-2"),
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        assert_eq!(resp["error"], "no such session 1", "{resp}");
        // 거부는 이전 백엔드 stop 전에 일어난다 — 라이브 스트림은 그대로다.
        assert_eq!(fake.stops.load(Ordering::SeqCst), 0, "{resp}");
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.handle, 8);
        assert_eq!((session.width, session.height), (1920, 1080));
    }

    #[tokio::test]
    async fn get_status_is_scoped_to_the_calling_device() {
        let server = Arc::new(ControlServer::new(
            backend(),
            test_pairing(),
            test_identity(),
        ));
        insert_live_session(&server, 1, Some("viewer-1"));
        insert_live_session(&server, 2, Some("viewer-2"));
        insert_live_session(&server, 3, None);

        let view = server
            .dispatch(
                "getStatus",
                serde_json::json!({}),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        let ids: Vec<u64> = view["result"]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["session"].as_u64().unwrap())
            .collect();
        assert_eq!(ids, vec![1], "{view}");

        // 호스트 내부 경로(None)는 기존처럼 전체를 본다.
        let view = server
            .dispatch("getStatus", serde_json::json!({}), "192.168.0.9", None)
            .await;
        let mut ids: Vec<u64> = view["result"]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["session"].as_u64().unwrap())
            .collect();
        ids.sort();
        assert_eq!(ids, vec![1, 2, 3], "{view}");
    }

    #[tokio::test]
    async fn none_device_path_keeps_full_session_access() {
        let stopped = Arc::new(AtomicUsize::new(0));
        let server = Arc::new(ControlServer::new(
            Arc::new(TerminalBackend {
                stopped: stopped.clone(),
            }),
            test_pairing(),
            test_identity(),
        ));
        insert_live_session(&server, 1, Some("viewer-1"));

        // 장치 미귀속 경로(호스트 UI·테스트)는 남의 세션도 다룬다 — 기존 동작.
        let resp = server
            .dispatch(
                "stopStream",
                serde_json::json!({ "session": 1 }),
                "192.168.0.9",
                None,
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert!(server.sessions.lock().unwrap().live.is_empty());
    }

    // -- 시작·재구성 임계구역의 핸들 유출 정리(F02) ---------------------------

    /// 첫 프레임을 릴리스할 때까지 보류하는 백엔드 — 시작 대기 창(최대 5초)을
    /// 테스트에서 잡을 수 있게 한다. FakeBackend는 first_send_ms=26이라 즉시
    /// 통과하므로 대기 창 테스트에는 이 백엔드가 필요하다.
    struct LatchBackend {
        stopped_handles: Mutex<Vec<u32>>,
        input_changes: Mutex<Vec<(u32, bool)>>,
        released: AtomicBool,
        starts: AtomicUsize,
        stops: AtomicUsize,
    }

    impl LatchBackend {
        fn new() -> Self {
            Self {
                stopped_handles: Mutex::new(Vec::new()),
                input_changes: Mutex::new(Vec::new()),
                released: AtomicBool::new(false),
                starts: AtomicUsize::new(0),
                stops: AtomicUsize::new(0),
            }
        }

        fn release(&self) {
            self.released.store(true, Ordering::SeqCst);
        }
    }

    impl CaptureBackend for LatchBackend {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            Ok(vec![DisplayInfo {
                source_id: Some("test:display:0".into()),
                index: 0,
                name: "Main".into(),
                width: 1920,
                height: 1080,
            }])
        }

        fn start(
            &self,
            _source_index: u32,
            _ip: &str,
            _port: u16,
            _w: u32,
            _h: u32,
            _fps: u32,
            _capture_backend: &str,
            _media_transport: &str,
            _content_mode: &str,
            _encoder_experiment: EncoderExperiment,
            _udp_stability: &AppliedUdpStability,
            _media_key: &[u8; 32],
            _access: Option<&crate::source_grants::CaptureAccess>,
        ) -> Result<u32, String> {
            Ok(11 + self.starts.fetch_add(1, Ordering::SeqCst) as u32)
        }

        fn set_input_enabled(&self, handle: u32, enabled: bool) -> Result<(), String> {
            self.input_changes.lock().unwrap().push((handle, enabled));
            Ok(())
        }

        fn stop(&self, handle: u32) -> Result<(), String> {
            self.stopped_handles.lock().unwrap().push(handle);
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
            Ok(StatsInfo {
                state: "running".into(),
                first_send_ms: if self.released.load(Ordering::SeqCst) {
                    26
                } else {
                    0
                },
                ..StatsInfo::default()
            })
        }
    }

    /// 조건이 잠깐 안에 참이 될 때까지 폴린다(테스트를 빠르게 유지).
    async fn wait_until(condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !condition() {
            assert!(Instant::now() < deadline, "condition not met in time");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn revoked_during_first_frame_wait_is_rejected_and_the_handle_is_stopped() {
        let pairing = test_pairing();
        let latch = Arc::new(LatchBackend::new());
        let server = Arc::new(ControlServer::new(
            latch.clone(),
            pairing.clone(),
            test_identity(),
        ));
        let token = direct_pair_token(&pairing, "viewer-1");
        assert_eq!(
            pairing.authorize_device(&token).as_deref(),
            Some("viewer-1")
        );

        let task = {
            let server = server.clone();
            tokio::spawn(async move {
                server
                    .dispatch(
                        "startStream",
                        serde_json::json!({
                            "sourceIndex": 0,
                            "viewerPort": 5001,
                            "width": 1920,
                            "height": 1080,
                            "fps": 60,
                            "mediaTransport": "udp",
                            "mediaKey": TEST_MEDIA_KEY
                        }),
                        "192.168.0.9",
                        Some("viewer-1"),
                    )
                    .await
            })
        };

        // 첫 프레임 대기에 들어간 뒤 철회하고 나서 프레임을 푼다 — 등록 직전
        // 재검사(F02)가 이 순서의 철회를 잡아야 한다.
        wait_until(|| latch.starts.load(Ordering::SeqCst) == 1).await;
        assert!(!pairing.revoke("viewer-1").removed_devices.is_empty());
        latch.release();

        let resp = task.await.unwrap();
        assert_eq!(resp["ok"], false, "{resp}");
        assert_eq!(resp["error"], "unauthorized", "{resp}");
        // 백엔드 핸들은 등록 전에 멈추고 live 맵은 비어 있어야 한다.
        assert_eq!(latch.stops.load(Ordering::SeqCst), 1, "{resp}");
        assert!(server.sessions.lock().unwrap().live.is_empty());
    }

    #[tokio::test]
    async fn stop_stream_during_reconfigure_wait_stops_the_replacement_handle() {
        let latch = Arc::new(LatchBackend::new());
        let server = Arc::new(ControlServer::new(
            latch.clone(),
            test_pairing(),
            test_identity(),
        ));
        insert_live_session(&server, 1, Some("viewer-1"));

        let task = {
            let server = server.clone();
            tokio::spawn(async move {
                server
                    .dispatch(
                        "reconfigureStream",
                        serde_json::json!({
                            "session": 1,
                            "width": 2560,
                            "height": 1440,
                            "fps": 60,
                            "qualityState": "native"
                        }),
                        "192.168.0.9",
                        Some("viewer-1"),
                    )
                    .await
            })
        };

        // 교체 백엔드가 시작해 첫 프레임을 기다리는 동안 세션을 종료한다.
        wait_until(|| latch.starts.load(Ordering::SeqCst) == 1).await;
        let stop = server
            .dispatch(
                "stopStream",
                serde_json::json!({ "session": 1 }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(stop["ok"], true, "{stop}");
        latch.release();

        let resp = task.await.unwrap();
        assert_eq!(resp["ok"], false, "{resp}");
        assert!(
            resp["error"]
                .as_str()
                .unwrap()
                .contains("ended during reconfigure"),
            "{resp}"
        );
        assert_eq!(
            *latch.stopped_handles.lock().unwrap(),
            vec![8, 11],
            "old and rejected replacement each retire once; cancellation cannot leak either handle"
        );
        assert!(server.sessions.lock().unwrap().live.is_empty());
    }

    #[tokio::test]
    async fn concurrent_reconfigures_of_one_session_serialize() {
        let latch = Arc::new(LatchBackend::new());
        let server = Arc::new(ControlServer::new(
            latch.clone(),
            test_pairing(),
            test_identity(),
        ));
        insert_live_session(&server, 1, Some("viewer-1"));

        let task = {
            let server = server.clone();
            tokio::spawn(async move {
                server
                    .dispatch(
                        "reconfigureStream",
                        serde_json::json!({
                            "session": 1,
                            "width": 2560,
                            "height": 1440,
                            "fps": 60,
                            "qualityState": "native"
                        }),
                        "192.168.0.9",
                        Some("viewer-1"),
                    )
                    .await
            })
        };

        // 첫 재구성이 stop→swap 임계구역(첫 프레임 대기 포함)에 진입한 뒤
        // 두 번째를 시도한다 — "진행 중"으로 명확히 거부된다.
        wait_until(|| latch.stops.load(Ordering::SeqCst) == 1).await;
        let second = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 1280,
                    "height": 720,
                    "fps": 60,
                    "qualityState": "native"
                }),
                "192.168.0.9",
                Some("viewer-1"),
            )
            .await;
        assert_eq!(second["ok"], false, "{second}");
        assert!(
            second["error"]
                .as_str()
                .unwrap()
                .contains("already in progress"),
            "{second}"
        );

        latch.release();
        let first = task.await.unwrap();
        assert_eq!(first["ok"], true, "{first}");
        // stop(이전 핸들 1회) == start(교체 핸들 1회) — 유출·이중 시작이 없다.
        assert_eq!(latch.stops.load(Ordering::SeqCst), 1);
        assert_eq!(latch.starts.load(Ordering::SeqCst), 1);
        let state = server.sessions.lock().unwrap();
        assert_eq!(state.live.len(), 1);
        assert_eq!(state.live.get(&1).unwrap().handle, 11);
    }

    // -- 인증 전 입력·연결 상한(F08) ------------------------------------------

    #[tokio::test]
    async fn bounded_line_preserves_socket_error_instead_of_reporting_overflow() {
        let socket = tokio_test::io::Builder::new()
            .read(b"partial request")
            .read_error(std::io::Error::from(std::io::ErrorKind::ConnectionReset))
            .build();
        let mut reader = BufReader::new(socket);
        assert!(matches!(
            read_bounded_line(&mut reader, HANDSHAKE_LINE_LIMIT).await,
            Err(ControlLineReadError::Io(error))
                if error.kind() == std::io::ErrorKind::ConnectionReset
        ));
    }

    #[tokio::test]
    async fn oversized_preauth_line_is_dropped_and_server_stays_responsive() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        // 핸드셰이크 줄 상한을 넘는 바이트를 '\n' 없이 보낸다.
        sock.write_all(&vec![b'a'; HANDSHAKE_LINE_LIMIT + 1])
            .await
            .unwrap();
        // 서버는 응답 없이 즉시 연결을 닫는다 — 초과 직후 EOF.
        let mut leftover = Vec::new();
        let closed = tokio::time::timeout(Duration::from_secs(5), sock.read_to_end(&mut leftover))
            .await
            .is_ok();
        assert!(
            closed,
            "server must close an oversized pre-auth connection promptly"
        );

        // 닫힌 뒤에도 정상 요청은 계속 처리된다.
        let mut fresh = tokio::net::TcpStream::connect(addr).await.unwrap();
        let line = request(&mut fresh, "getStatus", "{}", "").await;
        assert!(line.contains("\"ok\":false"), "{line}");
    }

    #[tokio::test]
    async fn connection_limit_does_not_wedge_the_server() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let addr = spawn_server().await;

        // 상한만큼 아무 것도 보내지 않는 연결을 열어 허가를 모두 점유한다.
        let mut idle = Vec::new();
        for _ in 0..MAX_CONCURRENT_CONNS {
            idle.push(tokio::net::TcpStream::connect(addr).await.unwrap());
        }

        // 상한을 넘은 연결의 요청은 잠시 대기하지만, 점유 연결 하나가
        // 끊기면 허가가 반납돼 처리된다 — 서버가 영구 막히지 않는다.
        let mut pending = tokio::net::TcpStream::connect(addr).await.unwrap();
        pending
            .write_all(b"{\"command\":\"getStatus\",\"args\":{},\"token\":\"\"}\n")
            .await
            .unwrap();
        drop(idle.remove(0));

        let response = tokio::time::timeout(Duration::from_secs(5), async move {
            let mut response = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                pending.read_exact(&mut byte).await.unwrap();
                response.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            response
        })
        .await;
        let response = response.expect("server must recover once a connection slot frees");
        let line = String::from_utf8(response).unwrap();
        assert!(line.contains("\"ok\":false"), "{line}");
    }
    #[tokio::test]
    async fn reaudit_operator_stop_during_reconfigure_must_not_revive() {
        let latch = Arc::new(LatchBackend::new());
        let server = Arc::new(ControlServer::new(
            latch.clone(),
            test_pairing(),
            test_identity(),
        ));
        insert_live_session(&server, 1, Some("viewer-1"));
        let task = {
            let server = server.clone();
            tokio::spawn(async move {
                server.dispatch("reconfigureStream", serde_json::json!({"session":1,"width":2560,"height":1440,"fps":60,"qualityState":"native"}), "192.168.0.9",Some("viewer-1")).await
            })
        };
        wait_until(|| latch.starts.load(Ordering::SeqCst) == 1).await;
        server.force_stop_session(1).unwrap();
        assert!(
            server
                .sessions
                .lock()
                .unwrap()
                .live
                .get(&1)
                .unwrap()
                .backend_released
        );
        latch.release();
        let response = task.await.unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert!(
            latch.stopped_handles.lock().unwrap().contains(&11),
            "replacement handle must be stopped after operator cancellation"
        );
        assert!(
            latch.input_changes.lock().unwrap().contains(&(11, false)),
            "rejected replacement input must be disabled"
        );
        let state = server.sessions.lock().unwrap();
        let s = state.live.get(&1).unwrap();
        println!(
            "response={response}, backend_released={}, terminal_error={:?}, starts={}, stops={}",
            s.backend_released,
            s.terminal_error,
            latch.starts.load(Ordering::SeqCst),
            latch.stops.load(Ordering::SeqCst)
        );
        assert!(
            s.backend_released,
            "operator-stopped tombstone was revived by replacement"
        );
    }

    struct RevokeOnInputCheck {
        latch: LatchBackend,
        pairing: Arc<crate::pairing::PairingServer>,
        target: &'static str,
        repair: bool,
        fired: AtomicBool,
        server: std::sync::OnceLock<std::sync::Weak<ControlServer>>,
    }
    impl CaptureBackend for RevokeOnInputCheck {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            self.latch.list_displays()
        }
        fn start(
            &self,
            source: u32,
            ip: &str,
            port: u16,
            w: u32,
            h: u32,
            fps: u32,
            capture: &str,
            transport: &str,
            content: &str,
            experiment: EncoderExperiment,
            stability: &AppliedUdpStability,
            key: &[u8; 32],
            _access: Option<&crate::source_grants::CaptureAccess>,
        ) -> Result<u32, String> {
            self.latch.start(
                source, ip, port, w, h, fps, capture, transport, content, experiment, stability,
                key, _access,
            )
        }
        fn set_input_enabled(&self, handle: u32, enabled: bool) -> Result<(), String> {
            self.latch.set_input_enabled(handle, enabled)
        }

        fn stop(&self, h: u32) -> Result<(), String> {
            self.latch.stop(h)
        }
        fn stats(&self, h: u32) -> Result<StatsInfo, String> {
            self.revoke_from_stats();
            self.latch.stats(h)
        }
    }
    impl RevokeOnInputCheck {
        fn revoke_from_stats(&self) {
            if self.fired.swap(true, Ordering::SeqCst) {
                return;
            }
            assert!(!self.pairing.revoke(self.target).removed_devices.is_empty());
            self.server
                .get()
                .unwrap()
                .upgrade()
                .unwrap()
                .stop_sessions_for_device(self.target);
            if self.repair {
                direct_pair_token(&self.pairing, self.target);
            }
        }
    }
    #[tokio::test]
    async fn reaudit_revoke_after_check_before_registration_must_stop_start() {
        let pairing = test_pairing();
        let _token = direct_pair_token(&pairing, "viewer-1");
        let backend = Arc::new(RevokeOnInputCheck {
            latch: LatchBackend::new(),
            pairing: pairing.clone(),
            target: "viewer-1",
            repair: false,
            fired: AtomicBool::new(false),
            server: std::sync::OnceLock::new(),
        });
        backend.latch.release();
        let server = Arc::new(ControlServer::new(
            backend.clone(),
            pairing.clone(),
            test_identity(),
        ));
        backend.server.set(Arc::downgrade(&server)).unwrap();
        let response=server.dispatch("startStream",serde_json::json!({"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":60,"mediaTransport":"udp","mediaKey":TEST_MEDIA_KEY}),"192.168.0.9",Some("viewer-1")).await;
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(backend.latch.stops.load(Ordering::SeqCst), 1);
        assert!(
            backend
                .latch
                .input_changes
                .lock()
                .unwrap()
                .contains(&(11, false)),
            "revoked candidate input must be disabled"
        );
        println!(
            "response={response}, still_paired={}, live_sessions={}, stops={}",
            pairing.is_device_paired("viewer-1"),
            server.sessions.lock().unwrap().live.len(),
            backend.latch.stops.load(Ordering::SeqCst)
        );
        assert!(
            server.sessions.lock().unwrap().live.is_empty(),
            "revoked device was registered after final auth check"
        );
    }
    struct StopDuringRollback {
        inner: LatchBackend,
        server: std::sync::OnceLock<std::sync::Weak<ControlServer>>,
    }
    impl CaptureBackend for StopDuringRollback {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            self.inner.list_displays()
        }
        fn start(
            &self,
            source: u32,
            ip: &str,
            port: u16,
            w: u32,
            h: u32,
            fps: u32,
            capture: &str,
            transport: &str,
            content: &str,
            experiment: EncoderExperiment,
            stability: &AppliedUdpStability,
            key: &[u8; 32],
            _access: Option<&crate::source_grants::CaptureAccess>,
        ) -> Result<u32, String> {
            let result = self.inner.start(
                source, ip, port, w, h, fps, capture, transport, content, experiment, stability,
                key, _access,
            );
            if self.inner.starts.load(Ordering::SeqCst) == 2 {
                self.server
                    .get()
                    .unwrap()
                    .upgrade()
                    .unwrap()
                    .force_stop_session(1)
                    .unwrap();
            }
            result
        }
        fn stop(&self, handle: u32) -> Result<(), String> {
            self.inner.stop(handle)
        }
        fn stats(&self, handle: u32) -> Result<StatsInfo, String> {
            if self.inner.starts.load(Ordering::SeqCst) == 1 {
                Err("replacement failed".into())
            } else {
                self.inner.stats(handle)
            }
        }
    }
    #[tokio::test]
    async fn reaudit_operator_stop_during_rollback_must_not_revive() {
        let backend = Arc::new(StopDuringRollback {
            inner: LatchBackend::new(),
            server: std::sync::OnceLock::new(),
        });
        backend.inner.release();
        let server = Arc::new(ControlServer::new(
            backend.clone(),
            test_pairing(),
            test_identity(),
        ));
        backend.server.set(Arc::downgrade(&server)).unwrap();
        insert_live_session(&server, 1, None);
        let response = server
            .dispatch(
                "reconfigureStream",
                json!({"session":1,"width":2560,"height":1440,"fps":60,"qualityState":"native"}),
                "127.0.0.1",
                None,
            )
            .await;
        assert_eq!(response["ok"], false);
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert!(
            session.backend_released,
            "rollback resurrected an operator-stopped session"
        );
        assert_eq!(
            session.terminal_error.as_deref(),
            Some("host operator stopped the stream")
        );
        assert_eq!(
            *backend.inner.stopped_handles.lock().unwrap(),
            vec![8, 11, 12],
            "old, failed replacement and rejected recovery each retire once; operator stop does not repeat old retirement"
        );
    }

    #[tokio::test]
    async fn reaudit_pairing_command_does_not_grant_large_command_budget() {
        use tokio::io::AsyncReadExt;
        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut socket = TcpStream::connect(addr).await.unwrap();
        let token = pair_token(&mut socket, &pairing).await;
        // Possessing a returned token is not authenticating this connection.
        socket.write_all(&vec![b' '; 16 * 1024 + 1]).await.unwrap();
        let mut byte = [0];
        let closed = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte)).await;
        assert!(
            matches!(closed, Ok(Ok(0)) | Ok(Err(_))),
            "pre-token command retained a large buffer: {closed:?}"
        );
        let mut socket = TcpStream::connect(addr).await.unwrap();
        let valid = request(&mut socket, "getStatus", "{}", &token).await;
        assert!(valid.contains("\"ok\":true"), "{valid}");
        let large = " ".repeat(20 * 1024);
        let accepted = request(&mut socket, "getStatus", &format!("{{{large}}}"), &token).await;
        assert!(accepted.contains("\"ok\":true"), "{accepted}");
    }

    #[tokio::test]
    async fn reaudit_repair_cannot_validate_old_start_but_other_device_revoke_is_independent() {
        for (target, repair, expected_success) in
            [("viewer-1", true, false), ("viewer-2", false, true)]
        {
            let pairing = test_pairing();
            direct_pair_token(&pairing, "viewer-1");
            direct_pair_token(&pairing, "viewer-2");
            let backend = Arc::new(RevokeOnInputCheck {
                latch: LatchBackend::new(),
                pairing: pairing.clone(),
                target,
                repair,
                fired: AtomicBool::new(false),
                server: std::sync::OnceLock::new(),
            });
            backend.latch.release();
            let server = Arc::new(ControlServer::new(
                backend.clone(),
                pairing,
                test_identity(),
            ));
            backend.server.set(Arc::downgrade(&server)).unwrap();
            let response = server.dispatch("startStream", json!({"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":60,"mediaTransport":"udp","mediaKey":TEST_MEDIA_KEY}), "192.168.0.9", Some("viewer-1")).await;
            assert_eq!(response["ok"], expected_success, "{response}");
            assert_eq!(
                server.sessions.lock().unwrap().live.len(),
                usize::from(expected_success)
            );
        }
    }

    struct StopFromStats {
        server: std::sync::OnceLock<std::sync::Weak<ControlServer>>,
    }
    impl CaptureBackend for StopFromStats {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            Ok(vec![])
        }
        fn start(
            &self,
            _: u32,
            _: &str,
            _: u16,
            _: u32,
            _: u32,
            _: u32,
            _: &str,
            _: &str,
            _: &str,
            _: EncoderExperiment,
            _: &AppliedUdpStability,
            _: &[u8; 32],
            _access: Option<&crate::source_grants::CaptureAccess>,
        ) -> Result<u32, String> {
            Ok(7)
        }
        fn stop(&self, _: u32) -> Result<(), String> {
            Ok(())
        }
        fn stats(&self, _: u32) -> Result<StatsInfo, String> {
            self.server
                .get()
                .unwrap()
                .upgrade()
                .unwrap()
                .force_stop_session(1)?;
            Ok(StatsInfo {
                state: "running".into(),
                ..StatsInfo::default()
            })
        }
    }
    #[test]
    fn reaudit_status_backend_can_reenter_operator_stop() {
        let backend = Arc::new(StopFromStats {
            server: std::sync::OnceLock::new(),
        });
        let server = Arc::new(ControlServer::new(
            backend.clone(),
            test_pairing(),
            test_identity(),
        ));
        backend.server.set(Arc::downgrade(&server)).unwrap();
        insert_live_session(&server, 1, None);
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            send.send(server.snapshot()).unwrap();
        });
        let snapshot = receive
            .recv_timeout(Duration::from_secs(2))
            .expect("stats callback deadlocked on session state");
        assert_eq!(
            snapshot.sessions[0].stats.error.as_deref(),
            Some("host operator stopped the stream")
        );
    }
    struct OrderedStarts {
        fail_stop: bool,
        fail_old: bool,
        starts: AtomicUsize,
        old_ready: AtomicBool,
        stopped: Mutex<Vec<u32>>,
    }
    impl CaptureBackend for OrderedStarts {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            LatchBackend::new().list_displays()
        }
        fn start(
            &self,
            _: u32,
            _: &str,
            _: u16,
            _: u32,
            _: u32,
            _: u32,
            _: &str,
            _: &str,
            _: &str,
            _: EncoderExperiment,
            _: &AppliedUdpStability,
            _: &[u8; 32],
            _access: Option<&crate::source_grants::CaptureAccess>,
        ) -> Result<u32, String> {
            Ok(self.starts.fetch_add(1, Ordering::SeqCst) as u32 + 1)
        }
        fn stop(&self, handle: u32) -> Result<(), String> {
            self.stopped.lock().unwrap().push(handle);
            if self.fail_stop {
                return Err("backend still owns handle".into());
            }

            Ok(())
        }
        fn stats(&self, handle: u32) -> Result<StatsInfo, String> {
            if handle == 1 && self.old_ready.load(Ordering::SeqCst) && self.fail_old {
                return Err("delayed replacement failure".into());
            }
            Ok(StatsInfo {
                state: "running".into(),
                first_send_ms: if handle > 1 || self.old_ready.load(Ordering::SeqCst) {
                    1
                } else {
                    0
                },
                ..StatsInfo::default()
            })
        }
    }
    #[tokio::test]
    async fn reaudit_newer_start_supersedes_only_the_same_viewer_endpoint() {
        for newer_port in [5001, 5002] {
            let backend = Arc::new(OrderedStarts {
                fail_stop: false,
                fail_old: false,
                starts: AtomicUsize::new(0),
                old_ready: AtomicBool::new(false),
                stopped: Mutex::new(vec![]),
            });
            let server = Arc::new(ControlServer::new(
                backend.clone(),
                test_pairing(),
                test_identity(),
            ));
            let old = {
                let server = server.clone();
                tokio::spawn(async move {
                    server.dispatch("startStream",json!({"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":60,"mediaTransport":"udp","mediaKey":TEST_MEDIA_KEY}),"192.168.0.9",None).await
                })
            };
            wait_until(|| backend.starts.load(Ordering::SeqCst) == 1).await;
            let newer = server.dispatch("startStream",json!({"sourceIndex":0,"viewerPort":newer_port,"width":1920,"height":1080,"fps":60,"mediaTransport":"udp","mediaKey":TEST_MEDIA_KEY}),"192.168.0.9",None).await;
            assert_eq!(newer["ok"], true, "{newer}");
            backend.old_ready.store(true, Ordering::SeqCst);
            let older = old.await.unwrap();
            assert_eq!(
                older["ok"],
                newer_port != 5001,
                "older start published after its successor: {older}"
            );
            let state = server.sessions.lock().unwrap();
            assert_eq!(state.live.len(), if newer_port == 5001 { 1 } else { 2 });
            assert!(
                state.live.values().any(|s| s.handle == 2),
                "newer session removed by stale completion"
            );
            assert!(
                !backend.stopped.lock().unwrap().contains(&2),
                "stale cleanup stopped successor backend"
            );
            if newer_port == 5001 {
                assert_eq!(*backend.stopped.lock().unwrap(), vec![1]);
            }
        }
    }
    #[cfg(unix)]
    #[test]
    fn reaudit_stale_start_does_not_remove_successors_adb_mapping() {
        const CHILD: &str = "LEFTCAR_TASK1_TRANSPORT_CHILD";
        if let Ok(state_file) = std::env::var(CHILD) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                for replacement in [false, true] {
                let backend = Arc::new(OrderedStarts { fail_stop: false, fail_old: replacement, starts:AtomicUsize::new(0), old_ready:AtomicBool::new(false), stopped:Mutex::new(vec![]) });
                let server = Arc::new(ControlServer::new(backend.clone(),test_pairing(),test_identity()));
                if replacement {
                    insert_live_session(&server, 1, None);
                    {
                        let mut state = server.sessions.lock().unwrap();
                        let session = state.live.get_mut(&1).unwrap();
                        session.viewer_addr = "127.0.0.1:5001".into();
                        session.media_transport = "adbTcp".into();
                        state.next = 2;
                    }
                    adb_forward(5001).unwrap();
                }
                let old = { let server=server.clone(); tokio::spawn(async move {
                    if replacement {
                        return server.dispatch("reconfigureStream",json!({"session":1,"width":2560,"height":1440,"fps":60,"qualityState":"native"}),"127.0.0.1",None).await;
                    }

                    server.dispatch("startStream",json!({"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":60,"mediaTransport":"adbTcp","mediaKey":TEST_MEDIA_KEY}),"127.0.0.1",None).await
                })};
                wait_until(|| backend.starts.load(Ordering::SeqCst)==1).await;
                let newer = server.dispatch("startStream",json!({"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":60,"mediaTransport":"adbTcp","mediaKey":TEST_MEDIA_KEY}),"127.0.0.1",None).await;
                assert_eq!(newer["ok"],true,"{newer}");
                backend.old_ready.store(true,Ordering::SeqCst);
                assert_eq!(old.await.unwrap()["ok"],false);
                assert!(std::path::Path::new(&state_file).exists(),"stale cleanup removed the successor's ADB forwarding resource");
                }
            });
            return;
        }
        // Isolate PATH in a child test process. No connected device or global
        // process environment is changed; the production adb command runs.
        let _fixture = crate::source_grants::profile_process_fixture();
        let root = std::env::temp_dir().join(format!("leftcar-transport-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("adb");
        std::fs::write(&executable, "#!/bin/sh\nif [ \"$2\" = \"--remove\" ]; then /bin/rm -f \"$LEFTCAR_TASK1_TRANSPORT_CHILD\"; else /usr/bin/touch \"$LEFTCAR_TASK1_TRANSPORT_CHILD\"; fi\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "control::tests::reaudit_stale_start_does_not_remove_successors_adb_mapping",
                "--nocapture",
            ])
            .env(CHILD, root.join("forward-live"))
            .env("PATH", &root)
            .output()
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[tokio::test]
    async fn reaudit_failed_operator_stop_keeps_cleanup_ownership_without_reviving() {
        let backend = Arc::new(OrderedStarts {
            fail_stop: true,
            fail_old: false,
            starts: AtomicUsize::new(0),
            old_ready: AtomicBool::new(false),
            stopped: Mutex::new(vec![]),
        });
        let server = ControlServer::new(backend.clone(), test_pairing(), test_identity());
        insert_live_session(&server, 1, None);
        assert!(server.force_stop_session(1).is_err());
        assert!(
            !server
                .sessions
                .lock()
                .unwrap()
                .live
                .get(&1)
                .unwrap()
                .backend_released,
            "failed stop discarded ownership of an unreleased backend"
        );
        let result = server
            .dispatch(
                "reconfigureStream",
                json!({"session":1,"width":2560,"height":1440,"fps":60,"qualityState":"native"}),
                "127.0.0.1",
                None,
            )
            .await;
        assert_eq!(result["ok"], false);
        assert_eq!(
            backend.starts.load(Ordering::SeqCst),
            0,
            "terminal session became startable after stop failure"
        );
        server
            .sessions
            .lock()
            .unwrap()
            .live
            .get_mut(&1)
            .unwrap()
            .terminal_since =
            Some(Instant::now() - TERMINAL_SESSION_RETENTION - Duration::from_millis(1));
        server.snapshot();
        assert!(
            server.sessions.lock().unwrap().live.contains_key(&1),
            "failed terminal cleanup lost its retryable handle"
        );
    }
    #[tokio::test]
    async fn fix1_authenticated_socket_cannot_adopt_repaired_device_generation() {
        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut old = TcpStream::connect(addr).await.unwrap();
        let token_a = pair_token(&mut old, &pairing).await;
        let authenticated = request(&mut old, "getStatus", "{}", &token_a).await;
        assert!(authenticated.contains("\"ok\":true"));
        let token_b = direct_pair_token(&pairing, "test-viewer");
        assert!(!pairing.authorize(&token_a));
        let args = json!({"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":60,"mediaTransport":"udp","mediaKey":TEST_MEDIA_KEY}).to_string();
        let rejected = request(&mut old, "startStream", &args, &token_a).await;
        let rejected: serde_json::Value = serde_json::from_str(&rejected).unwrap();
        assert_eq!(
            rejected["ok"], false,
            "old socket adopted the new credential: {rejected}"
        );
        let mut fresh = TcpStream::connect(addr).await.unwrap();
        let accepted = request(&mut fresh, "startStream", &args, &token_b).await;
        let accepted: serde_json::Value = serde_json::from_str(&accepted).unwrap();
        assert_eq!(
            accepted["ok"], true,
            "new credential connection must work: {accepted}"
        );
    }

    #[cfg(unix)]
    struct TeardownBarrier {
        inner: OrderedStarts,
        armed: AtomicBool,
        entered: AtomicBool,
        released: Mutex<bool>,
        wake: std::sync::Condvar,
        terminal: AtomicBool,
        successor_ready: AtomicBool,
        fail_successor: AtomicBool,
    }
    #[cfg(unix)]
    impl CaptureBackend for TeardownBarrier {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            self.inner.list_displays()
        }
        fn start(
            &self,
            source: u32,
            ip: &str,
            port: u16,
            w: u32,
            h: u32,
            fps: u32,
            capture: &str,
            transport: &str,
            content: &str,
            experiment: EncoderExperiment,
            stability: &AppliedUdpStability,
            key: &[u8; 32],
            _access: Option<&crate::source_grants::CaptureAccess>,
        ) -> Result<u32, String> {
            self.inner.start(
                source, ip, port, w, h, fps, capture, transport, content, experiment, stability,
                key, _access,
            )
        }
        fn stop(&self, handle: u32) -> Result<(), String> {
            if handle == 1 && self.armed.swap(false, Ordering::SeqCst) {
                self.entered.store(true, Ordering::SeqCst);
                let mut released = self.released.lock().unwrap();
                while !*released {
                    released = self.wake.wait(released).unwrap();
                }
            }
            self.inner.stop(handle)
        }
        fn stats(&self, handle: u32) -> Result<StatsInfo, String> {
            let mut stats = self.inner.stats(handle)?;
            if handle == 2 {
                if !self.successor_ready.load(Ordering::SeqCst) {
                    stats.first_send_ms = 0;
                } else if self.fail_successor.load(Ordering::SeqCst) {
                    return Err("controlled successor first-frame failure".into());
                }
            }
            if handle == 1 && self.terminal.load(Ordering::SeqCst) {
                stats.state = "stopped".into();
            }
            Ok(stats)
        }
    }
    #[cfg(unix)]
    #[test]
    fn fix1_registered_teardown_cannot_remove_successors_adb_mapping() {
        const CHILD: &str = "LEFTCAR_TASK1_TEARDOWN_CHILD";
        if let Ok(state_file) = std::env::var(CHILD) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                for kind in ["forced","stopStream","gc","device","all","viewer"] {
                    let backend=Arc::new(TeardownBarrier {
                        inner:OrderedStarts { fail_stop:false,fail_old:false,starts:AtomicUsize::new(0),old_ready:AtomicBool::new(true),stopped:Mutex::new(vec![]) },
                        armed:AtomicBool::new(false),entered:AtomicBool::new(false),released:Mutex::new(false),wake:std::sync::Condvar::new(),terminal:AtomicBool::new(false),successor_ready:AtomicBool::new(true),fail_successor:AtomicBool::new(false),
                    });
                    let pairing=test_pairing();direct_pair_token(&pairing,"viewer-1");
                    let server=Arc::new(ControlServer::new(backend.clone(),pairing,test_identity()));
                    let args=json!({"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":60,"mediaTransport":"adbTcp","mediaKey":TEST_MEDIA_KEY});
                    let first=server.dispatch("startStream",args.clone(),"127.0.0.1",Some("viewer-1")).await;
                    assert_eq!(first["ok"],true,"{first}");
                    if kind=="gc" {
                        backend.terminal.store(true,Ordering::SeqCst);
                        server.sessions.lock().unwrap().live.get_mut(&1).unwrap().terminal_since=Some(Instant::now()-TERMINAL_SESSION_RETENTION-Duration::from_millis(1));
                    }
                    backend.armed.store(true,Ordering::SeqCst);
                    let stop={let server=server.clone();std::thread::spawn(move || {
                        match kind {
                            "forced" => { server.force_stop_session(1).unwrap(); },
                            "stopStream" => { tokio::runtime::Runtime::new().unwrap().block_on(server.dispatch("stopStream",json!({"session":1}),"127.0.0.1",Some("viewer-1"))); },
                            "gc" => { server.snapshot(); },
                            "device" => { server.stop_sessions_for_device("viewer-1"); },
                            "all" => server.stop_all_sessions(),
                            "viewer" => server.stop_sessions_for_viewer("127.0.0.1:5001",Some("viewer-1")),
                            _=>unreachable!(),
                        }
                    })};
                    wait_until(||backend.entered.load(Ordering::SeqCst)).await;
                    let successor=server.dispatch("startStream",args,"127.0.0.1",Some("viewer-1")).await;
                    *backend.released.lock().unwrap()=true;backend.wake.notify_all();stop.join().unwrap();
                    assert_eq!(successor["ok"],true,"{kind}: {successor}");
                    assert!(server.sessions.lock().unwrap().live.values().any(|s|s.handle==2),"{kind}: successor lost registration");
                    assert!(std::path::Path::new(&state_file).exists(),"{kind}: old registered teardown deleted successor transport");
                    assert!(!backend.inner.stopped.lock().unwrap().contains(&2),"{kind}: successor backend stopped");
                    server.force_stop_session(2).unwrap();
                    assert!(!std::path::Path::new(&state_file).exists(), "{kind}: current owner failed to clean its own mapping");

                }
            });
            return;
        }
        let _fixture = crate::source_grants::profile_process_fixture();
        let root = std::env::temp_dir().join(format!("leftcar-teardown-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("adb");
        std::fs::write(&executable,"#!/bin/sh\nif [ \"$2\" = \"--remove\" ]; then /bin/rm -f \"$LEFTCAR_TASK1_TEARDOWN_CHILD\"; else /usr/bin/touch \"$LEFTCAR_TASK1_TEARDOWN_CHILD\"; fi\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "control::tests::fix1_registered_teardown_cannot_remove_successors_adb_mapping",
                "--nocapture",
            ])
            .env(CHILD, root.join("forward-live"))
            .env("PATH", &root)
            .output()
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(unix)]
    fn run_fix2_transport_case(scenario: &str, test_name: &str) {
        const CHILD: &str = "LEFTCAR_TASK1_FIX2_CHILD";
        if let Ok(state_file) = std::env::var(CHILD) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let kinds: &[&str] = match scenario {
                    "pending" => &["forced", "stopStream", "gc", "device", "all", "viewer"],
                    "failure" => &["first_frame"],
                    "setup_failure" => &["setup"],
                    "deferred" => &["deferred"],
                    "switch" => &["udp", "tcp"],
                    "reconfigure" => &["success", "rollback", "setup_rollback", "setup_terminal"],
                    _ => unreachable!(),
                };
                for &kind in kinds {
                    let backend = Arc::new(TeardownBarrier {
                        inner: OrderedStarts { fail_stop:scenario=="deferred", fail_old:false, starts:AtomicUsize::new(0), old_ready:AtomicBool::new(true), stopped:Mutex::new(vec![]) },
                        armed:AtomicBool::new(false), entered:AtomicBool::new(false), released:Mutex::new(false), wake:std::sync::Condvar::new(), terminal:AtomicBool::new(false),
                        successor_ready:AtomicBool::new(false), fail_successor:AtomicBool::new(scenario=="failure" || scenario=="setup_failure" || kind=="rollback"),
                    });
                    let pairing=test_pairing(); direct_pair_token(&pairing,"viewer-1");
                    let server=Arc::new(ControlServer::new(backend.clone(),pairing,test_identity()));
                    let mut args=json!({"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":60,"mediaTransport":"adbTcp","mediaKey":TEST_MEDIA_KEY});
                    let first=server.dispatch("startStream",args.clone(),"127.0.0.1",Some("viewer-1")).await;
                    assert_eq!(first["ok"],true,"{first}");
                    assert!(std::path::Path::new(&state_file).exists());
                    if scenario=="deferred" {
                        backend.armed.store(true,Ordering::SeqCst);
                        let reconfigure={let server=server.clone();std::thread::spawn(move || tokio::runtime::Runtime::new().unwrap().block_on(server.dispatch("reconfigureStream",json!({"session":1,"width":2560,"height":1440,"fps":60,"qualityState":"native"}),"127.0.0.1",Some("viewer-1"))))};
                        wait_until(||backend.entered.load(Ordering::SeqCst)).await;
                        assert!(server.force_stop_session(1).is_err());
                        assert!(std::path::Path::new(&state_file).exists(),"teardown must be deferred while setup owns the lease");
                        *backend.released.lock().unwrap()=true; backend.wake.notify_all();
                        assert_eq!(reconfigure.join().unwrap()["ok"],false);
                        assert!(!std::path::Path::new(&state_file).exists(),"aborted setup silently dropped deferred predecessor cleanup");
                        continue;
                    }
                    if scenario=="reconfigure" {
                        if kind=="setup_rollback" {std::fs::write(format!("{state_file}.fail_once"),b"fail once").unwrap();}
                        if kind=="setup_terminal" {std::fs::write(format!("{state_file}.fail"),b"fail always").unwrap();}
                        let successor={let server=server.clone();tokio::spawn(async move {server.dispatch("reconfigureStream",json!({"session":1,"width":2560,"height":1440,"fps":60,"qualityState":"native"}),"127.0.0.1",Some("viewer-1")).await})};
                        if kind!="setup_terminal" {
                            wait_until(||backend.inner.starts.load(Ordering::SeqCst)==2).await;
                            assert!(std::path::Path::new(&state_file).exists(),"{kind}: reconfigure removed its own ADB mapping before first frame");
                            backend.successor_ready.store(true,Ordering::SeqCst);
                        }
                        let response=successor.await.unwrap();
                        assert_eq!(response["ok"],kind=="success","{kind}: {response}");
                        if kind=="setup_terminal" {
                            assert!(server.sessions.lock().unwrap().live.get(&1).unwrap().backend_released);
                            assert!(!std::path::Path::new(&state_file).exists());
                            std::fs::remove_file(format!("{state_file}.fail")).unwrap();
                        } else {
                            let expected=if kind=="rollback" {3} else {2};
                            assert_eq!(server.sessions.lock().unwrap().live.get(&1).unwrap().handle,expected);
                            assert!(std::path::Path::new(&state_file).exists(),"{kind}: committed reconfigure/recovery lost its mapping");
                            if kind=="rollback" {assert!(backend.inner.stopped.lock().unwrap().contains(&2));}
                            server.force_stop_session(1).unwrap();
                            assert!(!std::path::Path::new(&state_file).exists());
                        }
                        continue;
                    }
                    let stop = if scenario=="pending" || kind=="first_frame" {
                        if kind=="gc" {
                            backend.terminal.store(true,Ordering::SeqCst);
                            server.sessions.lock().unwrap().live.get_mut(&1).unwrap().terminal_since=Some(Instant::now()-TERMINAL_SESSION_RETENTION-Duration::from_millis(1));
                        }
                        backend.armed.store(true,Ordering::SeqCst);
                        let server=server.clone();
                        let stop=std::thread::spawn(move || match kind {
                            "forced" => {server.force_stop_session(1).unwrap();},
                            "gc" => {server.snapshot();},
                            "device" => {server.stop_sessions_for_device("viewer-1");},
                            "all" => server.stop_all_sessions(),
                            "viewer" => server.stop_sessions_for_viewer("127.0.0.1:5001",Some("viewer-1")),
                            _ => {tokio::runtime::Runtime::new().unwrap().block_on(server.dispatch("stopStream",json!({"session":1}),"127.0.0.1",Some("viewer-1")));},
                        });
                        wait_until(||backend.entered.load(Ordering::SeqCst)).await;
                        Some(stop)
                    } else {None};
                    if scenario=="switch" { args["mediaTransport"]=json!(kind); }
                    if kind=="setup" { std::fs::write(format!("{state_file}.fail"), b"fail after allocating mapping").unwrap(); }
                    let successor={let server=server.clone();tokio::spawn(async move {server.dispatch("startStream",args,"127.0.0.1",Some("viewer-1")).await})};
                    if kind!="setup" {
                        wait_until(||backend.inner.starts.load(Ordering::SeqCst)==2).await;
                        assert!(!server.sessions.lock().unwrap().live.values().any(|s|s.handle==2),"test must release teardown BEFORE successor registration");
                        if let Some(stop)=stop {
                            *backend.released.lock().unwrap()=true; backend.wake.notify_all(); stop.join().unwrap();
                        }
                        if scenario=="pending" || kind=="first_frame" {
                            assert!(std::path::Path::new(&state_file).exists(),"{kind}: predecessor teardown deleted pending successor mapping");
                        } else {
                            assert!(!std::path::Path::new(&state_file).exists(),"{kind}: transport switch orphaned predecessor ADB mapping");
                        }
                        backend.successor_ready.store(true,Ordering::SeqCst);
                    }
                    let response=successor.await.unwrap();
                    if scenario=="failure" || scenario=="setup_failure" {
                        assert_eq!(response["ok"],false,"{kind}: {response}");
                        assert!(!std::path::Path::new(&state_file).exists(),"{kind}: failed successor leaked mapping");
                        assert!(server.sessions.lock().unwrap().live.is_empty(),"failed successor must not register");
                        if kind=="first_frame" { assert!(backend.inner.stopped.lock().unwrap().contains(&2)); }
                        if kind=="setup" { std::fs::remove_file(format!("{state_file}.fail")).unwrap(); }
                    } else {
                        assert_eq!(response["ok"],true,"{kind}: {response}");
                        assert!(!backend.inner.stopped.lock().unwrap().contains(&2));
                        server.force_stop_session(2).unwrap();
                        assert!(!std::path::Path::new(&state_file).exists(),"{kind}: current owner did not clean mapping");
                    }
                }
            });
            return;
        }
        let _fixture = crate::source_grants::profile_process_fixture();
        let root = std::env::temp_dir().join(format!("leftcar-fix2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("adb");
        std::fs::write(&executable, r##"#!/bin/sh
if [ "$2" = "--remove" ]; then
  /bin/rm -f "$LEFTCAR_TASK1_FIX2_CHILD"
else
  /usr/bin/touch "$LEFTCAR_TASK1_FIX2_CHILD"
  if [ -f "$LEFTCAR_TASK1_FIX2_CHILD.fail_once" ]; then /bin/rm -f "$LEFTCAR_TASK1_FIX2_CHILD.fail_once"; exit 1; fi
  if [ -f "$LEFTCAR_TASK1_FIX2_CHILD.fail" ]; then exit 1; fi
fi
"##).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name, "--nocapture"])
            .env(CHILD, root.join("forward-live"))
            .env("PATH", &root)
            .output()
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(unix)]
    #[test]
    fn fix2_pending_successor_owns_adb_before_first_frame() {
        run_fix2_transport_case(
            "pending",
            "control::tests::fix2_pending_successor_owns_adb_before_first_frame",
        );
    }
    #[cfg(unix)]
    #[test]
    fn fix2_failed_successor_releases_transport() {
        run_fix2_transport_case(
            "failure",
            "control::tests::fix2_failed_successor_releases_transport",
        );
    }
    #[cfg(unix)]
    #[test]
    fn fix2_transport_switch_cleans_predecessor_kind() {
        run_fix2_transport_case(
            "switch",
            "control::tests::fix2_transport_switch_cleans_predecessor_kind",
        );
    }
    #[cfg(unix)]
    #[test]
    fn fix2_partial_setup_failure_releases_transport() {
        run_fix2_transport_case(
            "setup_failure",
            "control::tests::fix2_partial_setup_failure_releases_transport",
        );
    }

    #[cfg(unix)]
    #[test]
    fn fix2_reconfigure_and_rollback_keep_owned_adb_transport() {
        run_fix2_transport_case(
            "reconfigure",
            "control::tests::fix2_reconfigure_and_rollback_keep_owned_adb_transport",
        );
    }

    #[cfg(unix)]
    #[test]
    fn fix2_aborted_setup_drains_deferred_predecessor_cleanup() {
        run_fix2_transport_case(
            "deferred",
            "control::tests::fix2_aborted_setup_drains_deferred_predecessor_cleanup",
        );
    }
}

fn resolve_display(
    displays: &[control_contract::host::DisplayInfo],
    source: Option<&str>,
    index: Option<u32>,
) -> Result<control_contract::host::DisplayInfo, String> {
    let selected: Vec<_> = displays
        .iter()
        .filter(|display| match source {
            Some(source) => !source.is_empty() && display.source_id.as_deref() == Some(source),
            None => Some(display.index) == index,
        })
        .collect();
    let display =
        match selected.as_slice() {
            [display] => *display,
            _ => return Err(
                "source_unavailable: 화면이 없거나 식별자가 중복되었습니다. 목록을 새로 고치세요"
                    .into(),
            ),
        };
    let id = display
        .source_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or("source_unavailable: stable display identity missing")?;
    if displays
        .iter()
        .filter(|display| display.source_id.as_deref() == Some(id))
        .count()
        != 1
    {
        return Err("source_unavailable: ambiguous display identity".into());
    }
    Ok(display.clone())
}
