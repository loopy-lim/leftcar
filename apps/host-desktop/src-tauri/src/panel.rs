//! 대시보드용 루프백 관리 프로토콜(AC-H1). Tauri 명령 47종을 그대로 개행
//! JSON 요청·응답으로 미러한다 — Flutter 데스크톱 UI가 같은 서비스 계층을
//! 폴링하기 위한 관리 채널이며, 미디어·제어 평면과 완전히 분리된다
//! (docs/plans/2026-11-05-flutter-migration-design.md §2.2).
//! `get_active_pairing`은 Tauri 표면에 없는 패널 전용 확장이다(CLI QR 재표시
//! 검증용 — dispatch 참고).
//!
//! 신뢰 모델은 `control.rs` negotiate의 루프백 평문 선례와 같다: TCP는
//! 127.0.0.1 에페머럴 포트로만 듣고, 기동 때 만든 32바이트 토큰을 0600 파일로
//! 건넨다. 연결의 첫 줄이 토큰과 일치해야 하며, 한 번이라도 틀리면 즉시
//! 끊는다(재시도 없음).
//!
//! 프레이밍: 요청 `{"id":<u64|string>,"command":"...","args":{...}}` 한 줄당
//! 응답 한 줄 — `{"id":...,"ok":true,"result":...}` 또는
//! `{"id":...,"ok":false,"error":"..."}`. 서비스 오류는 문자열로 직렬화하고
//! 결과 값은 각 Tauri 명령 반환형의 JSON 표현이다. `id`는 검사 없이 그대로
//! 돌려준다. 인자 키는 Tauri invoke 표면과 같은 camelCase다(`deviceId` 등).
//!
//! 견고성: JSON 파싱 실패·알 수 없는 명령·인자 오류는 연결을 유지한 채
//! `ok:false` 응답으로 답한다(폴링 클라이언트 사양 — 오탈자 한 번에 세션이
//! 죽지 않는다). 다만 파서를 건너뛸 수 없는 초과 길이 줄(1MiB 상한)은 스트림
//! 정합을 해치므로 `ok:false` 응답 후 연결을 끊는다. 인증 전 첫 줄에는
//! 10초 읽기 제한이 있어 미인증 유휴 연결이 스레드를 점유하지 못하게
//! 한다(인증 뒤 요청 읽기는 폴링 공백을 위해 무제한).
//!
//! 동시성: 연결마다 스레드(`leftcar-panel-conn`), 한 연결 안에서는 요청을
//! 순서대로 처리한다(폴링 클라이언트 의미론). 크레이트의 기존 패턴과 같은
//! 스레드 기반이다(`leftcar-control` 등).

use crate::audit;
use crate::control;
use crate::pairing;
use crate::settings;
use crate::upnp;
use crate::virtual_display;
use std::collections::HashSet;
use std::io::{BufRead, BufWriter, Read, Write};
use std::sync::Arc;

/// 페어링 offer 소각 소속 뷰 식별자. Tauri 앱에서는 창 라벨(`main`,
/// `pairing`)이 이 역할을 한다 — 패널에서는 모든 연결이 같은 UI를 대변하므로
/// 하나의 라벨로 통일한다.
pub const PANEL_VIEW: &str = "panel";

/// 요청 줄 상한. 대시보드 명령의 인자는 전부 수백 바이트 수준이라 넉넉한
/// 1MiB다(제어 평면의 16KiB 핸드셰이크·12MiB 행 한도와 별개 스코프).
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

/// 패널 명령이 위임하는 서비스 핸들 한 벌. 부트스트랩이 만든 것을 사이드카
/// 진입점이 그대로 옮겨 담는다.
pub struct PanelHandles {
    pub server: Arc<control::ControlServer>,
    pub pairing: Arc<pairing::PairingServer>,
    pub audit: Arc<audit::SessionAudit>,
    pub settings: Arc<settings::SharedSettings>,
    pub virtual_display: Arc<virtual_display::VirtualDisplayManager>,
    pub upnp: Arc<upnp::UpnpMappingManager>,
    pub control_port: u16,
    /// 비동기 명령(가상 디스플레이·커튼·UPnP)을 블로킹 스레드에서 완수시킬
    /// 런타임. Tauri 앱은 `tauri::async_runtime`을, 사이드카는 자체 런타임을
    /// 쓴다.
    pub runtime: tokio::runtime::Handle,
}

/// 기동 때 한 번 만드는 루프백 인증 토큰. UUIDv4 두 개(각 122비트, getrandom
/// 기반 — 크레이트가 쓰는 유일한 난수원)로 32바이트를 채우고 파일에 쓰기
/// 좋은 base64url로 인코딩한다.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 비밀 파일 기록(0600). 부모 디렉터리를 만들고 unix에서는 생성 시점부터
/// 0600으로 둔다(느슨한 모드 창을 남기지 않는다). 기존 파일이 더 느슨하게
/// 만들어진 경우도 0600으로 되돌린다. non-unix는 베스트 에포트(모드 개념
/// 부재). 패널 토큰·포트 파일과 `leftcar-cli`의 페어링 재표시 캐시가 같은
/// 규칙을 공유한다.
pub fn write_secret_file(path: &std::path::Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("secret file dir {}: {error}", parent.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("secret file {}: {error}", path.display()))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| format!("secret file write {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// 토큰 파일 기록 — [`write_secret_file`]의 특수 케이스.
pub fn write_token_file(path: &std::path::Path, token: &str) -> Result<(), String> {
    write_secret_file(path, token)
}

/// 포트 파일 기록. 포트는 비밀이 아니지만 토큰 파일과 같은 0600으로 둔다 —
/// 완만한 권한 차등이 관측·추측 지표가 되는 걸 피한다.
pub fn write_port_file(path: &std::path::Path, port: u16) -> Result<(), String> {
    write_secret_file(path, &port.to_string())
}

/// 실행 중 호스트의 패널 파일 표준 위치. 플랫폼별 유저 전용 디렉터리를
/// 골라 그 아래 `leftcar/`를 만든다:
///
/// - 리눅스: `${XDG_RUNTIME_DIR}/leftcar` — 세션 수명이라 스테일 파일이
///   재부팅 사이에 남지 않고 0700/유저 전용 권한이 보장된다.
/// - macOS: `${TMPDIR}/leftcar` — 유저별 `/var/folders/…/T/` 디렉터리.
///   XDG_RUNTIME_DIR이 있으면(개발 중 설정) 그것이 우선한다.
/// - Windows: `%LOCALAPPDATA%\leftcar` — 유저 프로필 ACL로 보호되고
///   TEMP와 달리 정리되지 않아 발견 파일이 안정적이다.
pub fn default_panel_dir() -> Option<std::path::PathBuf> {
    panel_dir_from_env(
        HOST_PANEL_OS,
        std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
        std::env::var_os("TMPDIR").as_deref(),
        std::env::var_os("LOCALAPPDATA").as_deref(),
    )
}

/// [`default_panel_dir`]의 순수 결정 근사 — 어떤 OS에서 실행하든 세 플랫폼
/// 규칙을 결정론적으로 테스트할 수 있게 env만 받는다.
fn panel_dir_from_env(
    os: PanelOs,
    xdg_runtime_dir: Option<&std::ffi::OsStr>,
    tmp_dir: Option<&std::ffi::OsStr>,
    local_app_data: Option<&std::ffi::OsStr>,
) -> Option<std::path::PathBuf> {
    if let Some(base) = xdg_runtime_dir {
        return Some(std::path::PathBuf::from(base).join("leftcar"));
    }
    match os {
        PanelOs::Linux => None,
        PanelOs::Mac => tmp_dir.map(|base| std::path::PathBuf::from(base).join("leftcar")),
        PanelOs::Windows => local_app_data
            .map(|base| std::path::PathBuf::from(base).join("leftcar")),
    }
}

// 호스트 OS가 아닌 변형은 이 크레이트의 비(非)테스트 코드에서 만들어지지
// 않는다 — 세 플랫폼 규칙을 한 OS에서 테스트하기 위한 값들이다.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelOs {
    Linux,
    Mac,
    Windows,
}

#[cfg(target_os = "macos")]
const HOST_PANEL_OS: PanelOs = PanelOs::Mac;
#[cfg(target_os = "windows")]
const HOST_PANEL_OS: PanelOs = PanelOs::Windows;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const HOST_PANEL_OS: PanelOs = PanelOs::Linux;

/// 표준 토큰 파일 경로(`default_panel_dir`이 없으면 None).
pub fn default_token_path() -> Option<std::path::PathBuf> {
    default_panel_dir().map(|dir| dir.join("panel-token"))
}

/// 표준 포트 파일 경로(`default_panel_dir`이 없으면 None).
pub fn default_port_path() -> Option<std::path::PathBuf> {
    default_panel_dir().map(|dir| dir.join("panel-port"))
}

/// GUI 호스트(Tauri)가 기동 때 부르는 원스텝 패널 게시: 토큰 생성 → 표준
/// 경로 기록 → 루프백 리스너 바인딩 → 서브 스레드로 `serve`. 성공 시 패널
/// 포트를 돌려준다. 사이드카(leftcar-hostd)는 이 함수를 쓰지 않는다 —
/// `--panel-token-file` 등 명시적 경로 재정의를 지원하고 기록 시점(토큰을
/// 먼저 쓴다)과 serve 진입 방식(메인 스레드 블로킹)을 직접 통제한다.
pub fn publish_default_panel(handles: Arc<PanelHandles>) -> Result<u16, String> {
    // 두 파일은 항상 같은 표준 디렉터리에서 나온다 — env은 한 번만 읽는다.
    let dir = default_panel_dir()
        .ok_or_else(|| "XDG_RUNTIME_DIR is not set; cannot publish the panel files".to_string())?;
    let token_path = dir.join("panel-token");
    let port_path = dir.join("panel-port");
    // 포트를 먼저 확정하고 파일을 쓴다 — CLI가 반쯤 쓰인 포트 파일을 읽는
    // 창을 최소화한다. 토큰이 뒤바뀐 순간 이전 토큰은 어차피 무효다.
    let (listener, port) = bind_panel_listener()?;
    let token = generate_token();
    write_token_file(&token_path, &token)?;
    write_port_file(&port_path, port)?;
    std::thread::Builder::new()
        .name("leftcar-panel-serve".into())
        .spawn(move || serve(listener, token, handles))
        .map_err(|error| format!("panel serve thread: {error}"))?;
    Ok(port)
}

/// 관리 채널은 루프백 에페머럴 포트 하나다 — UI는 포트 파일로 실제 포트를
/// 건네받는다.
pub fn bind_panel_listener() -> Result<(std::net::TcpListener, u16), String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("panel listener bind: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("panel listener address: {error}"))?
        .port();
    Ok((listener, port))
}

/// 접속 수용 루프. 연결마다 스레드를 하나 돌리고 첫 줄 인증을 맡긴다.
pub fn serve(listener: std::net::TcpListener, token: String, handles: Arc<PanelHandles>) {
    loop {
        let (stream, _peer) = match listener.accept() {
            Ok(accepted) => accepted,
            Err(error) => {
                eprintln!("panel accept failed: {error}");
                continue;
            }
        };
        let token = token.clone();
        let handles = handles.clone();
        let spawned = std::thread::Builder::new()
            .name("leftcar-panel-conn".into())
            .spawn(move || handle_connection(stream, &token, handles));
        if let Err(error) = spawned {
            eprintln!("panel connection thread unavailable: {error}");
        }
    }
}

pub(crate) enum LineError {
    Io(std::io::Error),
    TooLong,
}

/// 개행 단위 읽기에 상한을 둔다. 상한을 넘는 줄은 파서를 건너뛸 방법이
/// 없어 스트림 정합이 깨지므로 호출자가 연결을 닫는다. 패널 클라이언트
/// ([`crate::panel_client`])가 같은 규칙으로 응답 줄을 읽는다 — 상한만 다르다.
pub(crate) fn read_capped_line(
    reader: &mut impl BufRead,
    out: &mut Vec<u8>,
    cap: usize,
) -> Result<usize, LineError> {
    out.clear();
    let mut limited = reader.take((cap + 1) as u64);
    let read = limited.read_until(b'\n', out).map_err(LineError::Io)?;
    if out.len() > cap {
        return Err(LineError::TooLong);
    }
    Ok(read)
}

/// 개행(CRLF 포함)만 떼어낸 조각 — [u8] 슬라이스라 문자열 치환 없이 비교한다.
fn trim_eol(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b'\r' || bytes[end - 1] == b'\n') {
        end -= 1;
    }
    &bytes[..end]
}

fn handle_connection(stream: std::net::TcpStream, token: &str, handles: Arc<PanelHandles>) {
    let _ = stream.set_nodelay(true);
    // 인증 전 첫 줄에만 읽기 제한시간을 둔다 — 토큰 줄을 끝내 보내지 않는
    // 로컬 연결이 스레드를 영구 주차시키는 일을 막는다(정상 클라이언트는
    // 연결 즉시 토큰을 보낸다).
    const AUTH_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
    let _ = stream.set_read_timeout(Some(AUTH_READ_TIMEOUT));
    let mut writer = match stream.try_clone() {
        Ok(writer) => BufWriter::new(writer),
        Err(_) => return,
    };
    let mut reader = std::io::BufReader::new(stream);
    let mut line = Vec::new();

    // 첫 줄은 반드시 토큰이다. 한 번이라도 틀리면 재시도 없이 끊는다 —
    // 루프백에서 무차별 대입을 막는 최소 방어다.
    match read_capped_line(&mut reader, &mut line, MAX_REQUEST_BYTES) {
        Ok(0) | Err(_) => return,
        Ok(_) => {}
    }
    if trim_eol(&line) != token.as_bytes() {
        return;
    }
    // 인증됐다 — 이후 요청 읽기는 무제한으로 돌린다. 대시보드 폴링
    // 클라이언트는 한 연결을 유지한 채 요청 사이에 긴 공백을 둔다("느긋한
    // 연결" 의미론). reader와 writer는 같은 소켓의 복제라 reader 쪽 핸들로
    // 걸어도 양쪽에 적용된다.
    let _ = reader.get_ref().set_read_timeout(None);

    loop {
        match read_capped_line(&mut reader, &mut line, MAX_REQUEST_BYTES) {
            Ok(0) => return,
            Err(LineError::Io(error)) => {
                eprintln!("panel connection io error: {error}");
                return;
            }
            Err(LineError::TooLong) => {
                let _ = write_response(
                    &mut writer,
                    &serde_json::Value::Null,
                    Err(format!(
                        "request line exceeds the {MAX_REQUEST_BYTES} byte limit"
                    )),
                );
                return;
            }
            Ok(_) => {}
        }
        // 파싱 실패는 연결을 살려 둔다 — id를 모르므로 null로 답한다.
        let request = match serde_json::from_slice::<PanelRequest>(&line) {
            Ok(request) => request,
            Err(error) => {
                if write_response(
                    &mut writer,
                    &serde_json::Value::Null,
                    Err(format!("malformed request: {error}")),
                )
                .is_err()
                {
                    return;
                }
                continue;
            }
        };
        let outcome = dispatch(&handles, &request.command, &request.args);
        if write_response(&mut writer, &request.id, outcome).is_err() {
            return;
        }
    }
}

#[derive(serde::Deserialize)]
struct PanelRequest {
    #[serde(default)]
    id: serde_json::Value,
    command: String,
    #[serde(default)]
    args: serde_json::Value,
}

fn write_response(
    writer: &mut BufWriter<std::net::TcpStream>,
    id: &serde_json::Value,
    outcome: Result<serde_json::Value, String>,
) -> std::io::Result<()> {
    let response = match outcome {
        Ok(result) => serde_json::json!({ "id": id, "ok": true, "result": result }),
        Err(error) => serde_json::json!({ "id": id, "ok": false, "error": error }),
    };
    writer.write_all(response.to_string().as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn json<T: serde::Serialize>(value: T) -> Result<serde_json::Value, String> {
    serde_json::to_value(value).map_err(|error| format!("response serialization failed: {error}"))
}

/// Tauri 명령의 인자 추출과 같은 규칙(camelCase 키, 타입 불일치는 명령 오류).
fn arg<T: serde::de::DeserializeOwned>(args: &serde_json::Value, key: &str) -> Result<T, String> {
    let value = args
        .get(key)
        .ok_or_else(|| format!("missing argument: {key}"))?;
    serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid argument {key}: {error}"))
}

fn opt_arg<T: serde::de::DeserializeOwned>(
    args: &serde_json::Value,
    key: &str,
) -> Result<Option<T>, String> {
    match args.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| format!("invalid argument {key}: {error}")),
    }
}

/// Tauri 명령 47종의 위임을 한 명령씩 그대로 옮긴 표. 각 분기의 원본은
/// lib.rs의 같은 이름 `#[tauri::command]`다 — 동작 차이가 생기지 않게 원문
/// 주석도 함께 옮겼다.
pub fn dispatch(
    handles: &PanelHandles,
    command: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match command {
        "get_status" => json(handles.server.snapshot()),
        "list_input_requests" => json(handles.server.pending_input_requests()),
        "get_host_platform" => json(handles.server.platform()),
        "get_control_port" => json(handles.control_port),
        "get_lan_ip" => json(crate::bootstrap::local_lan_ip()),
        "get_input_permission" => json(handles.server.input_permission()?),
        "get_km_status" => {
            let device_id: String = arg(args, "deviceId")?;
            json(handles.server.km_status(&device_id))
        }
        "get_km_consent" => {
            let device_id: String = arg(args, "deviceId")?;
            json(handles.server.km_consent(&device_id))
        }
        "set_km_consent" => {
            let device_id: String = arg(args, "deviceId")?;
            let mac_to_android: bool = arg(args, "macToAndroid")?;
            let android_to_mac: bool = arg(args, "androidToMac")?;
            handles.server.set_km_consent(
                &device_id,
                control_contract::km::KmViewerConsent {
                    mac_to_android,
                    android_to_mac,
                },
            )?;
            json(handles.server.km_consent(&device_id))
        }
        "request_input_permission" => json(handles.server.request_input_permission()?),
        "get_screen_permission" => json(handles.server.screen_permission()?),
        // macOS가 아니면 시스템 설정 패널이 없다 — no-op 성공(원본과 같다).
        "open_system_settings" => {
            let pane: Option<String> = opt_arg(args, "pane")?;
            #[cfg(target_os = "macos")]
            {
                let url = match pane.as_deref() {
                    Some("screen_capture") | Some("screencapture") => "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
                    Some("inputMonitoring") => {
                        "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
                    }
                    Some("remote_desktop") | Some("remotedesktop") => {
                        "x-apple.systempreferences:com.apple.preference.security?Privacy_RemoteDesktop"
                    }
                    _ => {
                        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
                    }
                };
                std::process::Command::new("open")
                    .arg(url)
                    .spawn()
                    .map_err(|e| format!("failed to open macOS settings: {e}"))?;
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = pane;
            }
            json(())
        }
        "set_session_input" => {
            let session: u32 = arg(args, "session")?;
            let enabled: bool = arg(args, "enabled")?;
            json(handles.server.set_session_input(session, enabled)?)
        }
        "set_session_quality" => {
            let session: u32 = arg(args, "session")?;
            let quality: Option<f32> = opt_arg(args, "quality")?;
            json(handles.server.set_session_quality(session, quality)?)
        }
        "force_stop_session" => {
            let session: u32 = arg(args, "session")?;
            json(handles.server.force_stop_session(session)?)
        }
        "begin_pairing" => {
            let ip = crate::bootstrap::local_lan_ip().ok_or("no LAN interface found")?;
            json(handles.pairing.begin_pairing_for_view(
                &ip,
                handles.control_port,
                crate::bootstrap::tailscale_ip().as_deref(),
                PANEL_VIEW,
            ))
        }
        "cancel_pairing" => {
            let offer_id: String = arg(args, "offerId")?;
            json(handles.pairing.cancel_for_view(&offer_id, PANEL_VIEW))
        }
        "list_pending_pairings" => json(handles.pairing.list_pending_views()),
        // 패널 전용 확장(Tauri 표면 밖): `leftcar-cli pair show`가 캐시한 QR이
        // 아직 활성 offer인지 대조한다. id만 돌려준다 — QR 페이로드/시크릿은
        // 서버가 보관하지 않는다(페어링 서버의 제로화 원칙).
        "get_active_pairing" => json(handles.pairing.active_offer_id()),
        "approve_pending_pairing" => {
            let offer_id: String = arg(args, "offerId")?;
            let approved_device_id = handles
                .pairing
                .approve_pending(&offer_id)
                .map_err(|e| e.to_string())?;
            // 기본적으로 모두 허용: 사용자가 호스트에서 기기 연결을 승인하면 그 기기
            // 한 대에 모든 화면 접근 권한을 기본으로 부여한다. 다른 기기는 이 명령이
            // 절대 승격하지 않는다 — 기기별 승인은 그 기기의 승인 플로우에서만 결정되고,
            // 검토 후 비워 둔 권한(거부)은 그대로 남는다.
            let device = handles
                .pairing
                .list_device_views()
                .into_iter()
                .find(|device| device.device_id == approved_device_id);
            let Some(device) = device else {
                return json(());
            };
            let fresh =
                device.source_grants.review_required || device.source_grants.source_ids.is_empty();
            if !fresh {
                // 재승인이라도 사용자가 좁혀 둔 권한을 넓히지 않는다.
                return json(());
            }
            let displays = handles.server.backend().list_displays().unwrap_or_default();
            let display_ids: Vec<String> =
                displays.into_iter().filter_map(|d| d.source_id).collect();
            if !display_ids.is_empty() {
                let _ = handles.server.set_source_grants_for_credential(
                    &device.device_id,
                    display_ids,
                    Some(&device.source_grants.credential_id),
                );
            }
            json(())
        }
        "reject_pending_pairing" => {
            let offer_id: String = arg(args, "offerId")?;
            handles
                .pairing
                .reject_pending(&offer_id)
                .map_err(|e| e.to_string())?;
            json(())
        }
        "list_paired_devices" => json(handles.pairing.list_device_views()),
        "list_paired_device_state" => {
            // 순수 읽기다 — 이 폴링 명령이 승인을 기록하지 않는다. 화면 승인은 해당
            // 기기의 승인 플로우(approve_pending_pairing)에서만 결정된다.
            let mut state = handles.pairing.list_device_state();
            // 뷰어 배지와 같은 사실을 말하게 한다: 인증된 제어 연결이 살아 있는
            // 기기만 "연결됨"으로 표시한다.
            let connected: HashSet<String> =
                handles.server.connected_device_ids().into_iter().collect();
            for device in state.devices.iter_mut() {
                device.connected = connected.contains(&device.device_id);
            }
            json(state)
        }
        "list_host_sources" => json(handles.server.host_sources()?),
        "set_source_grants" => {
            let device_id: String = arg(args, "deviceId")?;
            let source_ids: Vec<String> = arg(args, "sourceIds")?;
            let credential_id: String = arg(args, "credentialId")?;
            json(handles.server.set_source_grants_for_credential(
                &device_id,
                source_ids,
                Some(&credential_id),
            )?)
        }
        // 상태 폴링은 워커 스레드에서 — 원본 Tauri 명령의 spawn_blocking과 같다.
        "virtual_display_status" => {
            let server = handles.server.clone();
            let manager = handles.virtual_display.clone();
            handles.runtime.block_on(async move {
                json(
                    tokio::task::spawn_blocking(move || {
                        crate::build_virtual_display_status(&server, &manager)
                    })
                    .await
                    .map_err(|e| e.to_string())?,
                )
            })
        }
        "virtual_display_arrange" => {
            let position: virtual_display::DisplayPosition = arg(args, "position")?;
            let _change = handles.server.begin_virtual_display_change()?;
            let manager = handles.virtual_display.clone();
            handles.runtime.block_on(async move {
                json(
                    tokio::task::spawn_blocking(move || manager.arrange(position))
                        .await
                        .map_err(|e| e.to_string())?,
                )
            })
        }
        "virtual_display_resize" => {
            let width: u32 = arg(args, "width")?;
            let height: u32 = arg(args, "height")?;
            let scale: u32 = arg(args, "scale")?;
            json(
                handles
                    .runtime
                    .block_on(handles.server.resize_virtual_display(
                        virtual_display::DisplayMode {
                            width,
                            height,
                            scale,
                        },
                    ))?,
            )
        }
        // 확장 디스플레이 생성. 이미 화면 승인된 기기에 새 소스만 추가한다.
        // 권한 추가는 기존 스트림을 유지하며 미검토·거부 기기는 변경하지 않는다.
        "virtual_display_create" => {
            let width: u32 = arg(args, "width")?;
            let height: u32 = arg(args, "height")?;
            let scale: u32 = arg(args, "scale")?;
            let _change = handles.server.begin_virtual_display_change()?;
            let manager = handles.virtual_display.clone();
            // 생성은 활성화+모드 폴링로 수 초 걸린다 — 워커 스레드에서 돌린다.
            let live = handles.runtime.block_on(async move {
                tokio::task::spawn_blocking(move || manager.create(width, height, scale))
                    .await
                    .map_err(|e| e.to_string())?
            })?;
            let Some(source_id) = live.source_id.clone() else {
                return Err("virtual display has no stable source id".into());
            };
            handles.audit.log(
                "virtual_display_created",
                serde_json::json!({
                    "sourceId": source_id,
                    "displayId": live.display_id,
                    "logicalWidth": live.logical_width,
                    "logicalHeight": live.logical_height,
                    "scale": live.scale,
                    "modeVerified": live.mode_verified,
                }),
            );
            // 이미 화면 승인된 기기에만 새 소스를 추가한다.
            for device in handles.pairing.list_device_views() {
                let Some(ids) = crate::grants_with_virtual_source(
                    &device.source_grants.source_ids,
                    device.source_grants.review_required,
                    &source_id,
                ) else {
                    continue;
                };
                let _ = handles.server.set_source_grants_for_credential(
                    &device.device_id,
                    ids,
                    Some(&device.source_grants.credential_id),
                );
            }
            json(crate::build_virtual_display_status(
                &handles.server,
                &handles.virtual_display,
            ))
        }
        // 확장 디스플레이 제거. 시스템 반영은 비동기(~30s)라 상태가 제거 중으로
        // 잠깐 표시될 수 있다.
        "virtual_display_remove" => {
            let _change = handles.server.begin_virtual_display_change()?;
            let manager = handles.virtual_display.clone();
            let _vanished = handles.runtime.block_on(async move {
                tokio::task::spawn_blocking(move || manager.remove())
                    .await
                    .map_err(|e| e.to_string())?
            })?;
            handles
                .audit
                .log("virtual_display_removed", serde_json::json!({}));
            json(crate::build_virtual_display_status(
                &handles.server,
                &handles.virtual_display,
            ))
        }
        "revoke_paired_device" => {
            let device_id: String = arg(args, "deviceId")?;
            json(handles.server.revoke_device(&device_id))
        }
        "revoke_all_devices" => json(handles.server.revoke_all_devices()),
        "get_clipboard_share" => json(handles.server.clipboard_share_enabled()),
        "set_clipboard_share" => {
            let enabled: bool = arg(args, "enabled")?;
            // 토글은 즉시 효력을 가지고 file_share와 함께 0600 settings.json에 남는다.
            handles.settings.set_clipboard_share(enabled)?;
            handles.server.set_clipboard_share(enabled);
            json(enabled)
        }
        "get_file_share" => json(handles.settings.file_share()),
        "set_file_share" => {
            let enabled: bool = arg(args, "enabled")?;
            handles.settings.set_file_share(enabled)?;
            handles.audit.log(
                "file_share_changed",
                serde_json::json!({ "enabled": enabled }),
            );
            json(())
        }
        "get_privacy_settings" => json(handles.settings.privacy_curtain()),
        "get_experiments" => json(handles.settings.experiment()),
        "set_experiments" => {
            let experiment: settings::ExperimentSettings = arg(args, "experiment")?;
            // 실험 스위치 저장 + 프로세스 환경변수 재주입. shim이 스트림 시작마다
            // 환경변수를 읽으므로 앱 재시작 없이 다음 스트림부터 적용된다.
            handles.settings.set_experiment(experiment)?;
            for (key, value) in settings::experiment_env_vars(&handles.settings.experiment()) {
                std::env::set_var(key, value);
            }
            json(handles.settings.experiment())
        }
        "set_privacy_curtain" => {
            let enabled: bool = arg(args, "enabled")?;
            handles.settings.set_privacy_curtain(enabled)?;
            // 즉시 반영: 켜면 살아 있는 세션이 있을 때 오버레이를 띄우고, 끄면
            // 치운다.
            handles.server.refresh_curtain();
            if enabled {
                // 이미 스트리밍 중인 세션의 SCK 필터는 시작 시점의 창 스냅샷으로
                // 제외 목록을 만들었다 — 방금 띄운 커튼이 캡처에서 빠지게 같은
                // 형태로 재시작해 필터를 다시 만든다(H1).
                handles
                    .runtime
                    .block_on(handles.server.restart_sessions_for_curtain());
            }
            handles.audit.log(
                "privacy_curtain_changed",
                serde_json::json!({ "enabled": enabled }),
            );
            json(())
        }
        // 다이얼로그 취소는 no-op(빈 목록)이다. rfd의 동기 패널은 호출 스레드를
        // 막을 수 있으므로 워커 스레드에서 띄운다(원본 Tauri 명령과 같다).
        "add_share_files" => {
            let server = handles.server.clone();
            let dialog_title = match handles.settings.language() {
                settings::HostLanguage::Ko => "Leftcar — 파일 공유",
                settings::HostLanguage::En => "Leftcar — File Sharing",
            };
            let picked = handles.runtime.block_on(async move {
                tokio::task::spawn_blocking(move || {
                    rfd::FileDialog::new()
                        .set_title(dialog_title)
                        .pick_files()
                        .unwrap_or_default()
                })
                .await
                .map_err(|error| format!("file dialog failed: {error:?}"))
            })?;
            json(server.file_transfer_state().add_share_files(picked))
        }
        "list_share_queue" => json(handles.server.file_transfer_state().queue_entries()),
        "remove_share_file" => {
            let queue_id: String = arg(args, "queueId")?;
            json(
                handles
                    .server
                    .file_transfer_state()
                    .remove_share_file(&queue_id),
            )
        }
        // 웹뷰(→Flutter)의 언어 토글을 네이티브 표면(트레이 메뉴·파일 대화상자)
        // 와 공유하기 위해 settings.json에 남긴다. 다음 기동 때 트레이가 이 값을 읽는다.
        "set_language" => {
            let language: String = arg(args, "language")?;
            let parsed = match language.as_str() {
                "ko" => settings::HostLanguage::Ko,
                "en" => settings::HostLanguage::En,
                other => return Err(format!("unsupported language: {other}")),
            };
            handles.settings.set_language(parsed)?;
            json(())
        }
        "get_streaming_badge" => json(handles.settings.streaming_badge()),
        "set_streaming_badge" => {
            let enabled: bool = arg(args, "enabled")?;
            // 배지 표시 토글 — 개인 기기 조합 기본 꺼짐, 0600 settings.json에 영속.
            // Indicator 라우트가 2초 폴링으로 이 값을 따라 show/hide한다.
            handles.settings.set_streaming_badge(enabled)?;
            json(enabled)
        }
        "get_wan_access" => json(handles.settings.wan_access()),
        "set_wan_access" => {
            let enabled: bool = arg(args, "enabled")?;
            // 외부 접속 토글 — 0600 settings.json에 영속되고 매핑 변화는 즉시 효력을
            // 가진다. 매핑 작업은 수 초 걸릴 수 있으니 백그라운드에서 돌린다.
            handles.settings.set_wan_access(enabled)?;
            handles.audit.log(
                "wan_access_changed",
                serde_json::json!({ "enabled": enabled }),
            );
            if enabled {
                let upnp = handles.upnp.clone();
                let port = handles.control_port;
                if let Some(local_ip) = crate::bootstrap::local_lan_ip() {
                    handles.runtime.spawn(upnp.enable(port, local_ip));
                } else {
                    eprintln!("UPnP: local LAN IP not found");
                    handles.runtime.spawn(upnp.disable());
                }
            } else {
                let upnp = handles.upnp.clone();
                handles.runtime.spawn(upnp.disable());
            }
            json(enabled)
        }
        other => Err(format!("unknown command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::FakeBackend;
    use control_contract::host::DisplayInfo;
    use std::io::Write;

    #[test]
    fn panel_dir_prefers_xdg_runtime_dir_on_every_os() {
        for os in [PanelOs::Linux, PanelOs::Mac, PanelOs::Windows] {
            assert_eq!(
                panel_dir_from_env(
                    os,
                    Some(std::ffi::OsStr::new("/run/user/1000")),
                    Some(std::ffi::OsStr::new("/var/folders/T/")),
                    Some(std::ffi::OsStr::new("C:\\Users\\u\\AppData\\Local")),
                ),
                Some(std::path::PathBuf::from("/run/user/1000").join("leftcar"))
            );
        }
    }

    #[test]
    fn panel_dir_falls_back_per_platform() {
        // 리눅스는 XDG가 없으면 발견 불가 — 명시적 경로를 쓰게 한다.
        assert_eq!(
            panel_dir_from_env(PanelOs::Linux, None, Some(std::ffi::OsStr::new("/tmp")), None),
            None
        );
        // macOS는 유저별 TMPDIR로.
        assert_eq!(
            panel_dir_from_env(PanelOs::Mac, None, Some(std::ffi::OsStr::new("/var/folders/T/")), None),
            Some(std::path::PathBuf::from("/var/folders/T/").join("leftcar"))
        );
        assert_eq!(panel_dir_from_env(PanelOs::Mac, None, None, None), None);
        // Windows는 LOCALAPPDATA로.
        assert_eq!(
            panel_dir_from_env(
                PanelOs::Windows,
                None,
                None,
                Some(std::ffi::OsStr::new("C:\\Users\\u\\AppData\\Local")),
            ),
            Some(std::path::PathBuf::from("C:\\Users\\u\\AppData\\Local").join("leftcar"))
        );
        assert_eq!(panel_dir_from_env(PanelOs::Windows, None, None, None), None);
    }

    /// 기존 테스트(control_e2e.rs)와 같은 조립: FakeBackend + 메모리 저장소.
    struct TestHarness {
        handles: Arc<PanelHandles>,
        #[allow(dead_code)] // 런타임을 테스트 동안 살려 둔다
        runtime: tokio::runtime::Runtime,
    }

    fn harness() -> TestHarness {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("panel test runtime");
        let backend = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                index: 0,
                source_id: Some("test:display:0".into()),
                name: "Main Display".into(),
                width: 1920,
                height: 1080,
            }],
            encoder_experiment: std::sync::Mutex::new(
                control_contract::host::EncoderExperiment::Auto,
            ),
            advertise_split_vertical: false,
            stops: std::sync::atomic::AtomicUsize::new(0),
            input_permission: true,
            input_calls: std::sync::Mutex::new(Vec::new()),
        });
        let pairing = Arc::new(pairing::PairingServer::new(
            [7u8; 32],
            None,
            Box::new(pairing::FileTokenStore::new(None)),
        ));
        let server = Arc::new(control::ControlServer::new(
            backend,
            pairing.clone(),
            Arc::new(secure_channel::HostIdentity::from_seed([42u8; 32])),
        ));
        let handles = Arc::new(PanelHandles {
            server,
            pairing,
            audit: Arc::new(audit::SessionAudit::new(None)),
            settings: settings::SharedSettings::in_memory(),
            virtual_display: Arc::new(virtual_display::VirtualDisplayManager::new()),
            upnp: Arc::new(upnp::UpnpMappingManager::new()),
            control_port: 7777,
            runtime: runtime.handle().clone(),
        });
        TestHarness { handles, runtime }
    }

    /// 패널 서버를 127.0.0.1:0에 띄고 (포트, 토큰)을 돌려준다.
    fn spawn_server(harness: &TestHarness) -> (u16, String) {
        let token = generate_token();
        let (listener, port) = bind_panel_listener().unwrap();
        // 스레드가 'static이어야 하므로 참조가 아닌 Arc 소유권만 넘긴다.
        let handles = harness.handles.clone();
        let server_token = token.clone();
        std::thread::Builder::new()
            .name("leftcar-panel-test".into())
            .spawn(move || serve(listener, server_token, handles))
            .unwrap();
        (port, token)
    }

    fn connect(port: u16, token: &str) -> std::net::TcpStream {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        writeln!(stream, "{token}").unwrap();
        stream
    }

    fn round_trip(stream: &mut std::net::TcpStream, request: &str) -> serde_json::Value {
        writeln!(stream, "{request}").unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    fn expect_ok(response: serde_json::Value) -> serde_json::Value {
        assert_eq!(
            response["ok"],
            serde_json::json!(true),
            "response: {response}"
        );
        response["result"].clone()
    }

    fn expect_error(response: serde_json::Value) -> String {
        assert_eq!(
            response["ok"],
            serde_json::json!(false),
            "response: {response}"
        );
        response["error"].as_str().unwrap().to_string()
    }

    #[test]
    fn wrong_token_closes_the_connection_without_a_response() {
        let harness = harness();
        let (port, _token) = spawn_server(&harness);
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        writeln!(stream, "definitely-not-the-token").unwrap();
        let mut reader = std::io::BufReader::new(stream);
        let mut line = String::new();
        let read = reader.read_line(&mut line).unwrap();
        assert_eq!(read, 0, "실패한 인증은 응답 없이 연결이 닫힌다");
    }

    #[test]
    fn correct_token_allows_platform_query() {
        let harness = harness();
        let (port, token) = spawn_server(&harness);
        let mut stream = connect(port, &token);
        // FakeBackend의 platform 값 — Tauri 명령 get_host_platform과 같은 위임.
        let result = expect_ok(round_trip(
            &mut stream,
            r#"{"id":7,"command":"get_host_platform"}"#,
        ));
        assert_eq!(result, serde_json::json!("test"));
    }

    #[test]
    fn id_is_echoed_verbatim_for_strings_and_numbers() {
        let harness = harness();
        let (port, token) = spawn_server(&harness);
        let mut stream = connect(port, &token);
        let response = round_trip(
            &mut stream,
            r#"{"id":"poll-42","command":"get_control_port"}"#,
        );
        assert_eq!(response["id"], serde_json::json!("poll-42"));
        assert_eq!(response["result"], serde_json::json!(7777));
        let response = round_trip(&mut stream, r#"{"id":99,"command":"get_control_port"}"#);
        assert_eq!(response["id"], serde_json::json!(99));
    }

    #[test]
    fn clipboard_share_round_trips_through_the_service_gate() {
        let harness = harness();
        let (port, token) = spawn_server(&harness);
        let mut stream = connect(port, &token);
        assert_eq!(
            expect_ok(round_trip(
                &mut stream,
                r#"{"id":1,"command":"get_clipboard_share"}"#
            )),
            serde_json::json!(false)
        );
        assert_eq!(
            expect_ok(round_trip(
                &mut stream,
                r#"{"id":2,"command":"set_clipboard_share","args":{"enabled":true}}"#
            )),
            serde_json::json!(true)
        );
        assert_eq!(
            expect_ok(round_trip(
                &mut stream,
                r#"{"id":3,"command":"get_clipboard_share"}"#
            )),
            serde_json::json!(true)
        );
    }

    #[test]
    fn experiments_round_trip_and_env_injection() {
        let harness = harness();
        let (port, token) = spawn_server(&harness);
        let mut stream = connect(port, &token);
        let result = expect_ok(round_trip(
            &mut stream,
            r#"{"id":1,"command":"set_experiments","args":{"experiment":{"maxEncodeInFlight":3,"frameTrace":true}}}"#,
        ));
        assert_eq!(result["maxEncodeInFlight"], serde_json::json!(3));
        assert_eq!(result["frameTrace"], serde_json::json!(true));
        assert_eq!(
            expect_ok(round_trip(
                &mut stream,
                r#"{"id":2,"command":"get_experiments"}"#
            )),
            result,
            "저장된 실험 스위치가 폴링 조회와 같다"
        );
        // 환경변수 재주입 — shim이 스트림 시작마다 읽는 값.
        assert_eq!(std::env::var("LEFTCAR_MAX_ENCODE_IN_FLIGHT").unwrap(), "3");
        assert_eq!(std::env::var("LEFTCAR_FRAME_TRACE").unwrap(), "1");
    }

    #[test]
    fn share_queue_starts_empty() {
        let harness = harness();
        let (port, token) = spawn_server(&harness);
        let mut stream = connect(port, &token);
        assert_eq!(
            expect_ok(round_trip(
                &mut stream,
                r#"{"id":1,"command":"list_share_queue"}"#
            )),
            serde_json::json!([])
        );
    }

    #[test]
    fn file_share_toggle_round_trips() {
        let harness = harness();
        let (port, token) = spawn_server(&harness);
        let mut stream = connect(port, &token);
        expect_ok(round_trip(
            &mut stream,
            r#"{"id":1,"command":"set_file_share","args":{"enabled":true}}"#,
        ));
        assert_eq!(
            expect_ok(round_trip(
                &mut stream,
                r#"{"id":2,"command":"get_file_share"}"#
            )),
            serde_json::json!(true)
        );
    }

    #[test]
    fn language_persists_and_rejects_unknown_codes() {
        let harness = harness();
        let (port, token) = spawn_server(&harness);
        let mut stream = connect(port, &token);
        expect_ok(round_trip(
            &mut stream,
            r#"{"id":1,"command":"set_language","args":{"language":"en"}}"#,
        ));
        assert_eq!(
            harness.handles.settings.language(),
            settings::HostLanguage::En
        );
        let error = expect_error(round_trip(
            &mut stream,
            r#"{"id":2,"command":"set_language","args":{"language":"fr"}}"#,
        ));
        assert!(error.contains("unsupported language"), "error: {error}");
    }

    #[test]
    fn unknown_command_is_ok_false_and_the_connection_survives() {
        let harness = harness();
        let (port, token) = spawn_server(&harness);
        let mut stream = connect(port, &token);
        let error = expect_error(round_trip(
            &mut stream,
            r#"{"id":1,"command":"not_a_real_command"}"#,
        ));
        assert_eq!(error, "unknown command: not_a_real_command");
        let result = expect_ok(round_trip(
            &mut stream,
            r#"{"id":2,"command":"get_streaming_badge"}"#,
        ));
        assert_eq!(result, serde_json::json!(false));
    }

    #[test]
    fn malformed_json_reports_ok_false_and_the_connection_survives() {
        let harness = harness();
        let (port, token) = spawn_server(&harness);
        let mut stream = connect(port, &token);
        let error = expect_error(round_trip(&mut stream, "this is not json"));
        assert!(error.starts_with("malformed request"), "error: {error}");
        assert_eq!(
            round_trip(&mut stream, r#"{"id":1,"command":"get_control_port"}"#)["ok"],
            serde_json::json!(true),
            "파싱 실패 한 번으로 폴링 세션이 끊기지 않는다"
        );
    }

    #[test]
    fn missing_arguments_are_command_errors_not_protocol_errors() {
        let harness = harness();
        let (port, token) = spawn_server(&harness);
        let mut stream = connect(port, &token);
        let error = expect_error(round_trip(
            &mut stream,
            r#"{"id":1,"command":"get_km_status"}"#,
        ));
        assert_eq!(error, "missing argument: deviceId");
        let error = expect_error(round_trip(
            &mut stream,
            r#"{"id":2,"command":"set_session_input","args":{"session":1,"enabled":"yes"}}"#,
        ));
        assert!(error.contains("invalid argument enabled"), "error: {error}");
    }

    #[test]
    fn token_file_is_private_and_existing_files_are_rehardened() {
        let dir = std::env::temp_dir().join(format!("leftcar-panel-test-{}", std::process::id()));
        let path = dir.join("nested").join("panel.token");
        let _ = std::fs::remove_dir_all(&dir);
        write_token_file(&path, "first-token").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first-token");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "토큰 파일은 0600이어야 한다");
        }
        write_token_file(&path, "second-token").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second-token");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_request_line_is_rejected_and_closes_the_connection() {
        let harness = harness();
        let (port, token) = spawn_server(&harness);
        let mut stream = connect(port, &token);
        let oversized = format!(
            "{{\"id\":1,\"command\":\"get_control_port\",\"pad\":\"{}\"}}",
            "x".repeat(MAX_REQUEST_BYTES + 10)
        );
        writeln!(stream, "{oversized}").unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["ok"], serde_json::json!(false));
        assert!(response["error"].as_str().unwrap().contains("byte limit"));
        let read = reader.read_line(&mut String::new()).unwrap();
        assert_eq!(read, 0, "초과 줄은 스트림 정합을 해치므로 연결이 닫힌다");
    }
}
