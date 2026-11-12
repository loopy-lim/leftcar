//! `leftcar-hostd` — Tauri 없는 호스트 사이드카(AC-H1). 기존 코어를 그대로
//! 기동(제어 7777·mDNS·AOAP USB 워처)하고, 루프백 관리 프로토콜
//! ([`leftcar_host_desktop::panel`])로 Tauri 대시보드 명령 47종에 응답한다.
//! Flutter 데스크톱 UI가 폴링 클라이언트로 붙는 대상이다.
//!
//! 인자:
//!   --panel-token-file <경로>   (필수) 기동 때 만든 0600 인증 토큰을 기록한다.
//!   --panel-port-file <경로>    (선택) 바인딩된 루프백 포트를 기록한다.
//!
//! 상태 프로필 잠금 때문에 같은 프로필에서는 Tauri 호스트와 동시에 뜰 수
//! 없다(제어 평면 7777 단일 인스턴스 전제 — 설계 §2.3의 사용 규칙).

use leftcar_host_desktop::{bootstrap, panel};

fn main() {
    // hostd has no event loop to set DPI awareness on our behalf (the
    // Tauri/tao shell does it for the GUI exe). Without per-monitor-v2
    // awareness GetMonitorInfoW reports virtualized coordinates on scaled
    // displays, so every negotiated stream size mismatches the capture
    // surface. Best effort: older Windows keeps the default context.
    #[cfg(target_os = "windows")]
    unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
        .ok();
    }
    let args = parse_args(std::env::args().skip(1)).unwrap_or_else(|message| {
        eprintln!("leftcar-hostd: {message}\n{}", USAGE);
        std::process::exit(2);
    });
    let services = bootstrap::start_host_services().unwrap_or_else(|message| {
        eprintln!("leftcar-hostd startup failed: {message}");
        std::process::exit(1);
    });
    // G-3: 호스트 시작 시 이전 프로세스가 남긴 좀비 가상 디스플레이를
    // 탐지·복구한다 — Tauri 앱의 setup이 하던 것과 같은 워커 스레드 기동이다.
    std::thread::Builder::new()
        .name("leftcar-vd-recovery".into())
        .spawn({
            let virtual_display = services.virtual_display.clone();
            move || virtual_display.recover_stale_if_due()
        })
        .expect("virtual display recovery thread");

    let token = panel::generate_token();
    panel::write_token_file(&args.panel_token_file, &token)
        .unwrap_or_else(|message| fatal(message));
    let (listener, panel_port) =
        panel::bind_panel_listener().unwrap_or_else(|message| fatal(message));
    if let Some(port_file) = &args.panel_port_file {
        if let Err(error) = std::fs::write(port_file, panel_port.to_string()) {
            fatal(format!("panel port file {}: {error}", port_file.display()));
        }
    }
    println!("panel on 127.0.0.1:{panel_port}");

    #[cfg(unix)]
    spawn_sigterm_handler(&services);

    // 런타임은 serve가 돌는 동안 살아 있어야 한다 — main 스코프에 두고 핸들만
    // 패널에 넘긴다(비동기 명령 완수용).
    let panel_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("panel runtime");
    let handles = std::sync::Arc::new(panel::PanelHandles {
        server: services.server.clone(),
        pairing: services.pairing.clone(),
        audit: services.audit.clone(),
        settings: services.settings.clone(),
        virtual_display: services.virtual_display.clone(),
        upnp: services.upnp.clone(),
        control_port: services.control_port,
        runtime: panel_runtime.handle().clone(),
    });
    panel::serve(listener, token, handles);
}

const USAGE: &str = "usage: leftcar-hostd --panel-token-file <path> [--panel-port-file <path>]";

struct HostdArgs {
    panel_token_file: std::path::PathBuf,
    panel_port_file: Option<std::path::PathBuf>,
}

fn parse_args<I: Iterator<Item = String>>(mut args: I) -> Result<HostdArgs, String> {
    let mut parsed = HostdArgs {
        panel_token_file: std::path::PathBuf::new(),
        panel_port_file: None,
    };
    while let Some(flag) = args.next() {
        let (flag, inline_value) = match flag.split_once('=') {
            Some((name, value)) => (name.to_string(), Some(value.to_string())),
            None => (flag, None),
        };
        let mut take_value = |name: &str| -> Result<String, String> {
            inline_value
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--panel-token-file" => {
                parsed.panel_token_file = take_value("--panel-token-file")?.into();
            }
            "--panel-port-file" => {
                parsed.panel_port_file = Some(take_value("--panel-port-file")?.into());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if parsed.panel_token_file.as_os_str().is_empty() {
        return Err("--panel-token-file is required".into());
    }
    Ok(parsed)
}

fn fatal(message: String) -> ! {
    eprintln!("leftcar-hostd startup failed: {message}");
    std::process::exit(1);
}

/// SIGTERM 경로는 Tauri 앱과 같은 관측 결과를 만든다: 업데이터가 SIGTERM을
/// 보내면 UPnP 매핑을 치우고(3초 상한) 저장된 기기 승인을 닫고 가상
/// 디스플레이를 해제한다(2초 상한 + 감사) — `bootstrap::run_exit_shutdown`
/// 한 벌을 공유한다. SIGINT(포그라운드 종료)도 같은 시퀀스로 처리한다.
#[cfg(unix)]
fn spawn_sigterm_handler(services: &bootstrap::HostServices) {
    let server = services.server.clone();
    let upnp = services.upnp.clone();
    let virtual_display = services.virtual_display.clone();
    let audit = services.audit.clone();
    std::thread::Builder::new()
        .name("leftcar-hostd-signal".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("leftcar-hostd signal runtime unavailable: {error}");
                    return;
                }
            };
            let terminated = runtime.block_on(async {
                let mut terminate =
                    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        Ok(signal) => signal,
                        Err(error) => {
                            eprintln!("leftcar-hostd SIGTERM hook unavailable: {error}");
                            return false;
                        }
                    };
                let mut interrupt =
                    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    {
                        Ok(signal) => signal,
                        Err(error) => {
                            eprintln!("leftcar-hostd SIGINT hook unavailable: {error}");
                            return false;
                        }
                    };
                tokio::select! {
                    _ = terminate.recv() => true,
                    _ = interrupt.recv() => true,
                }
            });
            if !terminated {
                return;
            }
            println!("leftcar-hostd: shutting down on termination signal");
            bootstrap::run_exit_shutdown(upnp, &server, &virtual_display, &audit, |future| {
                runtime.block_on(future)
            });
            std::process::exit(0);
        })
        .expect("signal handler thread");
}
