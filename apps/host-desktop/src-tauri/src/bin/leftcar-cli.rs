//! `leftcar-cli` — 실행 중 leftcar 호스트(Tauri GUI 또는 leftcar-hostd)를
//! 루프백 패널 프로토콜로 진단·제어하는 CLI. 호스트 발견은 표준 위치
//! (`${XDG_RUNTIME_DIR}/leftcar/panel-token`·`panel-port`)를 통해 하므로
//! 어떤 호스트 변형이 떠 있든 같은 명령으로 붙는다.
//!
//! 사용 예:
//!   leftcar-cli status                 상태 요약(플랫폼·LAN IP·세션·WAN)
//!   leftcar-cli pair begin             페어링 시작 — 6자리 코드 + 터미널 QR
//!   leftcar-cli pair show              마지막 세션의 QR 다시 표시(세션 유지)
//!   leftcar-cli pair pending           뷰어의 페어링 승인 대기 목록
//!   leftcar-cli pair approve <id>      대기 요청 승인
//!   leftcar-cli devices                페어링된 기기 목록
//!   leftcar-cli wan on                 외부 접속(UPnP WAN) 켜기
//!   leftcar-cli session stop <id>      스트림 세션 강제 종료
//!
//! 종료 코드: 0 성공, 1 호스트·프로토콜 오류, 2 사용법 오류.

use leftcar_host_desktop::panel_client::{discover, PanelClient};
use serde_json::Value;

fn main() {
    // `leftcar-cli pair begin | head`처럼 출력 파이프가 일찍 닫히면 Rust 기본
    // 동작(SIGPIPE 무시 → 쓰기 패닉) 대신 CLI 관례대로 조용히 종료한다.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = match parse_cli(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("leftcar-cli: {message}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    if cli.command == Command::Help {
        print!("{USAGE}");
        return;
    }
    let token_path = cli
        .token_file
        .clone()
        .or_else(leftcar_host_desktop::panel::default_token_path)
        .unwrap_or_else(|| {
            eprintln!(
                "leftcar-cli: 토큰 파일 경로를 알 수 없습니다 — --token-file으로 지정하거나 \
                 XDG_RUNTIME_DIR을 설정하세요"
            );
            std::process::exit(2);
        });
    let port_path = cli
        .port_file
        .clone()
        .or_else(leftcar_host_desktop::panel::default_port_path)
        .unwrap_or_else(|| {
            eprintln!(
                "leftcar-cli: 포트 파일 경로를 알 수 없습니다 — --port-file으로 지정하거나 \
                 XDG_RUNTIME_DIR을 설정하세요"
            );
            std::process::exit(2);
        });
    let (port, token) = discover(&token_path, &port_path).unwrap_or_else(|message| fatal(message));
    let mut client = PanelClient::connect(port, &token).unwrap_or_else(|message| fatal(message));
    // 재표시 캐시(`pair show`)는 토큰 파일과 같은 디렉터리에 둔다 — 커스텀
    // --token-file도 같은 규칙을 따른다.
    let cache_file = token_path
        .parent()
        .map(|dir| dir.join("cli-last-pairing.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("cli-last-pairing.json"));
    if let Err(message) = run(&mut client, &cli, &cache_file) {
        fatal(message);
    }
}

/// 진단 실패 공통 처리(종료 코드 1) — 형제 바이너리 leftcar-hostd의
/// `fatal`과 같은 모양. 사용법 오류(종료 코드 2)는 USAGE·힌트가 달라
/// 여기 쓰지 않는다.
fn fatal(message: String) -> ! {
    eprintln!("leftcar-cli: {message}");
    std::process::exit(1);
}

const USAGE: &str = "\
usage: leftcar-cli [options] <command> [args]

options:
  --token-file <path>   패널 토큰 파일 (기본: $XDG_RUNTIME_DIR/leftcar/panel-token)
  --port-file <path>    패널 포트 파일 (기본: $XDG_RUNTIME_DIR/leftcar/panel-port)
  --json                원시 JSON 응답 출력

commands:
  status                          호스트 상태 요약 (플랫폼·LAN IP·제어 포트·세션·WAN)
  pair begin                      페어링 시작 — 6자리 코드 + 터미널 QR 출력
  pair show                       마지막 세션의 QR 다시 표시(새 offer 아님)
  pair pending                    승인 대기 중인 페어링 요청 목록
  pair approve <offerId>          대기 요청 승인
  pair reject <offerId>           대기 요청 거부
  pair cancel <offerId>           진행 중 페어링 세션 취소
  devices                         페어링된 기기 목록
  devices revoke <deviceId>       기기 한 대의 페어링 폐기
  devices revoke-all              페어링된 기기 전부 폐기
  wan                             외부 접속(UPnP WAN) 상태
  wan on | wan off                외부 접속 켜기/끄기
  sources                         호스트 화면 소스 목록
  session stop <sessionId>        스트림 세션 강제 종료
  help                            이 도움말

QR은 어두운 터미널을 가정해 렌더링한다(밝은 터미널에서는 --json의
qr_payload를 다른 QR 도구에 넣어 찍으세요).";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Status,
    PairBegin,
    PairShow,
    PairPending,
    PairApprove(String),
    PairReject(String),
    PairCancel(String),
    Devices,
    DevicesRevoke(String),
    DevicesRevokeAll,
    WanGet,
    WanOn,
    WanOff,
    Sources,
    SessionStop(u32),
    Help,
}

struct Cli {
    json: bool,
    token_file: Option<std::path::PathBuf>,
    port_file: Option<std::path::PathBuf>,
    command: Command,
}

fn parse_cli<I: Iterator<Item = String>>(mut args: I) -> Result<Cli, String> {
    let mut json = false;
    let mut token_file = None;
    let mut port_file = None;
    let mut rest: Vec<String> = Vec::new();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--json" => json = true,
            "--token-file" | "--port-file" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("{flag} needs a value"))?;
                if flag == "--token-file" {
                    token_file = Some(value.into());
                } else {
                    port_file = Some(value.into());
                }
            }
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option: {other}"));
            }
            other => rest.push(other.to_string()),
        }
    }
    let mut words = rest.iter().map(String::as_str);
    let command = match words.next() {
        Some("status") => Command::Status,
        Some("help") | None => Command::Help,
        Some("pair") => match words.next() {
            Some("begin") => Command::PairBegin,
            Some("show") => Command::PairShow,
            Some("pending") => Command::PairPending,
            Some("approve") => Command::PairApprove(take_id(words.next(), "offerId")?),
            Some("reject") => Command::PairReject(take_id(words.next(), "offerId")?),
            Some("cancel") => Command::PairCancel(take_id(words.next(), "offerId")?),
            other => return Err(format!("unknown pair subcommand: {}", other.unwrap_or(""))),
        },
        Some("devices") => match words.next() {
            None => Command::Devices,
            Some("revoke") => Command::DevicesRevoke(take_id(words.next(), "deviceId")?),
            Some("revoke-all") => Command::DevicesRevokeAll,
            Some(other) => return Err(format!("unknown devices subcommand: {other}")),
        },
        Some("wan") => match words.next() {
            None => Command::WanGet,
            Some("on") => Command::WanOn,
            Some("off") => Command::WanOff,
            Some(other) => return Err(format!("wan expects on|off, got: {other}")),
        },
        Some("sources") => Command::Sources,
        Some("session") => match words.next() {
            Some("stop") => {
                let id = take_id(words.next(), "sessionId")?;
                id.parse::<u32>()
                    .map(Command::SessionStop)
                    .map_err(|_| format!("sessionId must be a number, got: {id}"))?
            }
            other => return Err(format!("unknown session subcommand: {}", other.unwrap_or(""))),
        },
        Some(other) => return Err(format!("unknown command: {other}")),
    };
    if let Some(extra) = words.next() {
        return Err(format!("unexpected extra argument: {extra}"));
    }
    Ok(Cli {
        json,
        token_file,
        port_file,
        command,
    })
}

fn take_id(word: Option<&str>, name: &str) -> Result<String, String> {
    word.map(str::to_string)
        .ok_or_else(|| format!("{name} is required"))
}

fn run(
    client: &mut PanelClient,
    cli: &Cli,
    cache_path: &std::path::Path,
) -> Result<(), String> {
    match &cli.command {
        Command::Help => unreachable!("help is handled before connecting"),
        Command::Status => {
            let platform = client.call0("get_host_platform")?;
            let lan_ip = client.call0("get_lan_ip")?;
            let port = client.call0("get_control_port")?;
            let wan = client.call0("get_wan_access")?;
            let status = client.call0("get_status")?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "platform": platform,
                        "lanIp": lan_ip,
                        "controlPort": port,
                        "wanAccess": wan,
                        "status": status,
                    })
                );
                return Ok(());
            }
            println!("플랫폼       {}", value_str(&platform));
            println!(
                "LAN IP       {}",
                lan_ip.as_str().unwrap_or("(없음)")
            );
            println!("제어 포트    {}", port.as_u64().unwrap_or_default());
            println!(
                "외부 접속    {}",
                if wan.as_bool().unwrap_or(false) { "켜짐 (UPnP WAN)" } else { "꺼짐 (LAN 직접 연결만)" }
            );
            let sessions = status["sessions"].as_array().cloned().unwrap_or_default();
            if sessions.is_empty() {
                println!("세션         없음");
            } else {
                println!("세션         {}개", sessions.len());
                for session in &sessions {
                    println!(
                        "  #{} {} → {} ({}x{}) from {} [{}]",
                        session["session"].as_u64().unwrap_or_default(),
                        field(session, "device_name", "deviceName").as_str().unwrap_or("(이름 없음)"),
                        field(session, "source_name", "sourceName").as_str().unwrap_or("?"),
                        session["width"].as_u64().unwrap_or_default(),
                        session["height"].as_u64().unwrap_or_default(),
                        field(session, "viewer_addr", "viewerAddr").as_str().unwrap_or("?"),
                        field(session, "quality_state", "qualityState").as_str().unwrap_or("?"),
                    );
                }
            }
            Ok(())
        }
        Command::PairBegin => {
            let view = client.call0("begin_pairing")?;
            // 재표시 캐시는 best-effort다 — 실패해도 QR은 이미 확보했으니
            // 세션 시작을 막지 않는다(이후 `pair show`만 불가).
            if let Err(message) = save_pairing_cache(cache_path, &view, now_unix()) {
                eprintln!(
                    "leftcar-cli: 경고 — 페어링 캐시 기록 실패로 `pair show`를 쓸 수 없습니다: {message}"
                );
            }
            if cli.json {
                println!("{view}");
                return Ok(());
            }
            let expires = field(&view, "expires_in_secs", "expiresInSecs")
                .as_u64()
                .unwrap_or_default();
            println!("페어링 세션 시작 — {expires}초 후 만료");
            println!("  코드: {}", view["code"].as_str().unwrap_or("?"));
            println!(
                "  offer: {}",
                field(&view, "offer_id", "offerId").as_str().unwrap_or("?")
            );
            let payload = field(&view, "qr_payload", "qrPayload").as_str().map(str::to_string);
            if let Some(payload) = payload {
                println!();
                for line in render_qr(&payload)? {
                    println!("{line}");
                }
                println!();
                println!("뷰어에서 QR을 스캔한 뒤 `leftcar-cli pair pending`으로 승인하세요.");
            }
            Ok(())
        }
        Command::PairShow => {
            // `begin`을 다시 돌리면 새 offer가 열리고 대기 중 요청까지
            // 무효화된다. `show`는 캐시한 세션을 그대로 다시 띄운다 — 서버가
            // QR 페이로드를 보관하지 않기 때문에(시크릿 제로화 원칙) 재표시는
            // 이 캐시로만 가능하다.
            let now = now_unix();
            let cache = load_pairing_cache(cache_path, now)?;
            // 활성 offer와 대조: 다른 터미널·GUI에서 begin을 다시 돌리거나
            // 승인/취소로 offer가 소진됐으면 캐시된 QR은 이미 죽어 있다.
            let active = client.call0("get_active_pairing")?;
            if active.as_str() != Some(cache.offer_id.as_str()) {
                return Err(format!(
                    "캐시된 세션({})은 더 활성 상태가 아닙니다 — 다른 곳에서 새 세션이 \
                     시작됐거나 승인·취소로 소진됐습니다. `leftcar-cli pair begin`으로 \
                     새 세션을 여세요",
                    cache.offer_id
                ));
            }
            let remaining = cache.expires_at_unix.saturating_sub(now);
            if cli.json {
                // `begin`의 원시 뷰(PairingSessionView 직렬화)와 같은 모양
                // (snake_case)으로 돌려준다 — 소비자가 명령별로 키를 갈아낄
                // 필요가 없게. remaining은 이미 흐른 TTL을 반영한다.
                println!(
                    "{}",
                    serde_json::json!({
                        "offer_id": cache.offer_id,
                        "code": cache.code,
                        "qr_payload": cache.qr_payload,
                        "expires_in_secs": remaining,
                    })
                );
                return Ok(());
            }
            println!("페어링 세션 다시 표시 — {remaining}초 후 만료 (기존 세션 유지, 새 offer 아님)");
            println!("  코드: {}", cache.code);
            println!("  offer: {}", cache.offer_id);
            println!();
            for line in render_qr(&cache.qr_payload)? {
                println!("{line}");
            }
            println!();
            println!("뷰어에서 QR을 스캔한 뒤 `leftcar-cli pair pending`으로 승인하세요.");
            Ok(())
        }
        Command::PairPending => {
            let pending = client.call0("list_pending_pairings")?;
            if cli.json {
                println!("{pending}");
                return Ok(());
            }
            let list = pending.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                println!("승인 대기 중인 페어링 요청이 없습니다.");
                return Ok(());
            }
            for item in &list {
                println!(
                    "{}  {}  ({}에 요청)",
                    field(item, "offer_id", "offerId").as_str().unwrap_or("?"),
                    field(item, "device_name", "deviceName").as_str().unwrap_or("(이름 없음)"),
                    field(item, "requested_at", "requestedAt").as_str().unwrap_or("?"),
                );
            }
            println!("승인: leftcar-cli pair approve <offerId>");
            Ok(())
        }
        Command::PairApprove(id) => offer_action(client, "approve_pending_pairing", "승인", id),
        Command::PairReject(id) => offer_action(client, "reject_pending_pairing", "거부", id),
        Command::PairCancel(id) => offer_action(client, "cancel_pairing", "취소", id),
        Command::Devices => {
            let devices = client.call0("list_paired_devices")?;
            if cli.json {
                println!("{devices}");
                return Ok(());
            }
            let list = devices.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                println!("페어링된 기기가 없습니다.");
                return Ok(());
            }
            for device in &list {
                println!(
                    "{}  {}  {}  ({} 페어링)",
                    field(device, "device_id", "deviceId").as_str().unwrap_or("?"),
                    device["name"].as_str().unwrap_or("(이름 없음)"),
                    if device["connected"].as_bool().unwrap_or(false) { "연결됨" } else { "오프라인" },
                    field(device, "paired_at", "pairedAt").as_str().unwrap_or("?"),
                );
            }
            Ok(())
        }
        Command::DevicesRevoke(id) => {
            client.call("revoke_paired_device", serde_json::json!({ "deviceId": id }))?;
            println!("폐기했습니다: {id}");
            Ok(())
        }
        Command::DevicesRevokeAll => {
            client.call0("revoke_all_devices")?;
            println!("모든 기기 페어링을 폐기했습니다.");
            Ok(())
        }
        Command::WanGet => {
            let wan = client.call0("get_wan_access")?;
            if cli.json {
                println!("{wan}");
                return Ok(());
            }
            println!(
                "외부 접속(UPnP WAN): {}",
                if wan.as_bool().unwrap_or(false) { "켜짐" } else { "꺼짐" }
            );
            Ok(())
        }
        Command::WanOn | Command::WanOff => {
            let enabled = matches!(cli.command, Command::WanOn);
            client.call("set_wan_access", serde_json::json!({ "enabled": enabled }))?;
            println!(
                "외부 접속(UPnP WAN)을 {}습니다. 매핑 반영까지 수 초 걸릴 수 있습니다.",
                if enabled { "켰" } else { "껐" }
            );
            Ok(())
        }
        Command::Sources => {
            let sources = client.call0("list_host_sources")?;
            if cli.json {
                println!("{sources}");
                return Ok(());
            }
            let list = sources.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                println!("호스트 화면 소스가 없습니다.");
                return Ok(());
            }
            for source in &list {
                println!(
                    "{}  {}  ({}x{})",
                    field(source, "source_id", "sourceId").as_str().unwrap_or("?"),
                    source["name"].as_str().unwrap_or("(이름 없음)"),
                    source["width"].as_u64().unwrap_or_default(),
                    source["height"].as_u64().unwrap_or_default(),
                );
            }
            Ok(())
        }
        Command::SessionStop(id) => {
            client.call(
                "force_stop_session",
                serde_json::json!({ "session": id }),
            )?;
            println!("세션 {id}을(를) 종료했습니다.");
            Ok(())
        }
    }
}

/// offer 대상 승인·거부·취소 — 같은 인자(`offerId`)와 같은 출력 형식의
/// 대칭 명령이라 한 자리에서 처리한다.
fn offer_action(
    client: &mut PanelClient,
    command: &str,
    verb: &str,
    offer_id: &str,
) -> Result<(), String> {
    client.call(command, serde_json::json!({ "offerId": offer_id }))?;
    println!("{verb}했습니다: {offer_id}");
    Ok(())
}

fn value_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// 응답 필드 조회 — 뷰 타입마다 serde 케이스가 다르다(패널 결과는 각 Tauri
/// 명령 반환형의 직렬화를 그대로 미러하기 때문). snake_case와 camelCase를
/// 순서대로 시도한다.
fn field<'a>(value: &'a Value, snake: &str, camel: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    match value.get(snake) {
        Some(found) if !found.is_null() => found,
        _ => value.get(camel).unwrap_or(&NULL),
    }
}

/// `pair show` 재표시용 마지막 페어링 세션 캐시. QR 페이로드에 offer
/// 시크릿(base64url)이 들어 있으므로 패널 토큰과 같은 0600 파일로 저장한다.
/// 서버는 페이로드를 보관하지 않으므로(시크릿 제로화 원칙) 재표시는 이
/// 캐시가 유일한 출처다.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PairingCache {
    offer_id: String,
    code: String,
    qr_payload: String,
    expires_at_unix: u64,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// `begin_pairing` 응답을 TTL 절대 시각과 함께 캐시에 기록한다. 퍼미션·부모
/// 디렉터리 규칙은 패널의 0600 비밀 파일 헬퍼를 그대로 쓴다.
fn save_pairing_cache(path: &std::path::Path, view: &Value, now: u64) -> Result<(), String> {
    let cache = PairingCache {
        offer_id: field(view, "offer_id", "offerId")
            .as_str()
            .ok_or("응답에 offer_id가 없습니다")?
            .to_string(),
        code: view["code"].as_str().unwrap_or("").to_string(),
        qr_payload: field(view, "qr_payload", "qrPayload")
            .as_str()
            .ok_or("응답에 qr_payload가 없습니다")?
            .to_string(),
        expires_at_unix: now.saturating_add(
            field(view, "expires_in_secs", "expiresInSecs")
                .as_u64()
                .unwrap_or_default(),
        ),
    };
    let body = serde_json::to_string(&cache)
        .map_err(|error| format!("페어링 캐시 직렬화 실패: {error}"))?;
    leftcar_host_desktop::panel::write_secret_file(path, &body)
        .map_err(|error| format!("페어링 캐시 기록 실패: {error}"))
}

/// 캐시를 읽어 만료 전이면 돌려준다. 파일이 없거나 파손됐거나 만료됐으면
/// 처방이 같은 안내로 바꾼다 — `pair show`의 모든 실패 경로가 새 `begin`으로
/// 귀결된다.
fn load_pairing_cache(path: &std::path::Path, now: u64) -> Result<PairingCache, String> {
    let begin_hint = "`leftcar-cli pair begin`으로 새 세션을 여세요";
    let body = std::fs::read_to_string(path)
        .map_err(|error| format!("재표시할 페어링 세션이 없습니다({error}) — {begin_hint}"))?;
    let cache: PairingCache = serde_json::from_str(body.trim())
        .map_err(|error| format!("페어링 캐시를 읽을 수 없습니다({error}) — {begin_hint}"))?;
    if cache.expires_at_unix <= now {
        return Err(format!("마지막 페어링 세션이 만료되었습니다 — {begin_hint}"));
    }
    Ok(cache)
}

/// 페이로드를 터미널 QR로 렌더링한다. 어두운 배경을 가정한 극성(밝은 모듈을
/// 블록으로 찍는다)이고 좌우 여백 2모듈을 붙여 콰이어트 존을 보장한다.
/// QR 렌더러가 페이로드를 인코딩할 수 없으면 원문을 담은 오류로 알린다.
fn render_qr(payload: &str) -> Result<Vec<String>, String> {
    let code = match qrcode::QrCode::with_error_correction_level(
        payload.as_bytes(),
        qrcode::EcLevel::L,
    ) {
        Ok(code) => code,
        Err(error) => {
            return Err(format!(
                "QR 인코딩 실패({error}) — qr_payload를 수동으로 입력하세요: {payload}"
            ))
        }
    };
    // 밝은 모듈을 블록(터미널 전경색)으로, 어두운 모듈을 공백(터미널 배경색)
    // 으로 찍는다 — 어두운 터미널에서 정극성 QR로 읽힌다. 렌더러에 여백
    // 옵션이 없으므로 콰이어트 존(2모듈)을 직접 붙인다.
    let body = code
        .render::<qrcode::render::unicode::Dense1x2>()
        .dark_color(qrcode::render::unicode::Dense1x2::Light)
        .light_color(qrcode::render::unicode::Dense1x2::Dark)
        .build();
    let quiet_zone = "    "; // 가로 2모듈(Dense1x2는 모듈당 2칸 폭)
    let mut lines = vec![String::new()]; // 세로 2모듈(줄당 2모듈)
    lines.extend(
        body.lines()
            .map(|line| format!("{quiet_zone}{line}{quiet_zone}")),
    );
    lines.push(String::new());
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> std::vec::IntoIter<String> {
        list.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn parses_global_options_before_command() {
        let cli = parse_cli(args(&["--json", "--token-file", "/tmp/t", "status"])).unwrap();
        assert!(cli.json);
        assert_eq!(cli.token_file.as_deref(), Some(std::path::Path::new("/tmp/t")));
        assert_eq!(cli.command, Command::Status);
    }

    #[test]
    fn parses_pair_and_devices_subcommands() {
        assert_eq!(parse_cli(args(&["pair", "begin"])).unwrap().command, Command::PairBegin);
        assert_eq!(parse_cli(args(&["pair", "show"])).unwrap().command, Command::PairShow);
        assert_eq!(
            parse_cli(args(&["pair", "approve", "offer-1"])).unwrap().command,
            Command::PairApprove("offer-1".into())
        );
        assert_eq!(
            parse_cli(args(&["devices", "revoke", "dev-9"])).unwrap().command,
            Command::DevicesRevoke("dev-9".into())
        );
        assert_eq!(
            parse_cli(args(&["devices", "revoke-all"])).unwrap().command,
            Command::DevicesRevokeAll
        );
    }

    #[test]
    fn parses_wan_and_session() {
        assert_eq!(parse_cli(args(&["wan", "on"])).unwrap().command, Command::WanOn);
        assert_eq!(
            parse_cli(args(&["session", "stop", "7"])).unwrap().command,
            Command::SessionStop(7)
        );
        assert!(parse_cli(args(&["session", "stop", "x"])).is_err());
    }

    #[test]
    fn rejects_unknown_and_missing_pieces() {
        assert!(parse_cli(args(&["nope"])).is_err());
        assert!(parse_cli(args(&["pair"])).is_err());
        assert!(parse_cli(args(&["pair", "approve"])).is_err());
        assert!(parse_cli(args(&["--wat", "status"])).is_err());
        assert!(parse_cli(args(&["status", "extra"])).is_err());
        assert!(parse_cli(args(&["--token-file"])).is_err());
    }

    #[test]
    fn bare_invocation_is_help() {
        assert_eq!(parse_cli(args(&[])).unwrap().command, Command::Help);
    }

    #[test]
    fn qr_renders_with_quiet_zone() {
        let lines = render_qr("{\"v\":1}").unwrap();
        // 앞뒤 빈 줄(세로 콰이어트 존) + 본문 줄은 좌우 여백을 가진다.
        assert_eq!(lines.first().map(String::as_str), Some(""));
        assert_eq!(lines.last().map(String::as_str), Some(""));
        assert!(lines.len() > 4);
        assert!(lines[1..lines.len() - 1]
            .iter()
            .all(|line| line.starts_with("    ")));
    }

    /// 고유 임시 디렉터리(크레이트에 tempfile이 없어 std로 만든다).
    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "leftcar-cli-cache-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn pairing_view() -> Value {
        serde_json::json!({
            "offer_id": "offer-abc",
            "code": "123456",
            "expires_in_secs": 120,
            "qr_payload": "{\"v\":2,\"id\":\"offer-abc\"}",
        })
    }

    #[test]
    fn pairing_cache_round_trips_until_expiry() {
        let dir = TempDir::new("roundtrip");
        let path = dir.0.join("cli-last-pairing.json");
        save_pairing_cache(&path, &pairing_view(), 1_000).unwrap();

        let cache = load_pairing_cache(&path, 1_050).unwrap();
        assert_eq!(cache.offer_id, "offer-abc");
        assert_eq!(cache.code, "123456");
        assert_eq!(cache.qr_payload, "{\"v\":2,\"id\":\"offer-abc\"}");
        assert_eq!(cache.expires_at_unix, 1_120);

        // 만료 순간(==)에도 재표시는 거절된다 — 경계는 닫혀 있다.
        let expired = load_pairing_cache(&path, 1_120).unwrap_err();
        assert!(expired.contains("만료"), "{expired}");
    }

    #[test]
    fn pairing_cache_file_is_user_only() {
        let dir = TempDir::new("perm");
        let path = dir.0.join("cli-last-pairing.json");
        save_pairing_cache(&path, &pairing_view(), 1_000).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn pairing_cache_missing_or_corrupt_points_to_begin() {
        let dir = TempDir::new("missing");
        let path = dir.0.join("cli-last-pairing.json");
        let missing = load_pairing_cache(&path, 1_000).unwrap_err();
        assert!(missing.contains("pair begin"), "{missing}");

        std::fs::write(&path, "not-json").unwrap();
        let corrupt = load_pairing_cache(&path, 1_000).unwrap_err();
        assert!(corrupt.contains("pair begin"), "{corrupt}");
    }
}
