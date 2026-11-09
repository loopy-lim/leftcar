//! `host-gpui` — Tauri 웹뷰를 GPUI로 갈아끼우는 호스트 앱(2026-11-06 GPUI
//! 전환 계획 T-1). Tauri 앱(`leftcar-host-desktop::run`)과 같은 단일
//! 프로세스 구조를 재현한다: 이 바이너리가 코어 부트스트랩을 직접 기동하고
//! UI 폴러가 `panel::dispatch` 1:1 위임 표를 백그라운드 스레드에서 호출한다
//! (설계 §2.1 개정 — 사이드카·토큰·TCP 클라이언트 없음).
//!
//! 기동 순서는 Tauri 앱의 `run()`과 사이드카(`leftcar-hostd`)가 공유하는
//! `bootstrap::start_host_services()` 한 벌을 그대로 쓴다. 실패 처리만
//! 호출자의 몫이다 — 이 앱은 표준 오류 + 종료로 간다(Tauri 앱의
//! `fatal_startup_error` 대화상자에 해당).
//!
//! 종료는 두 갈래다. 정상 종료는 gpui `on_app_quit`에서
//! `bootstrap::run_exit_shutdown`(UPnP ≤3s → 승인 종료 → 가상 디스플레이
//! ≤2s + 감사)을 수행하고, SIGTERM/SIGINT는 Tauri 앱의 업데이터 경로와
//! 같이 같은 시퀀스를 돌리고 프로세스를 끝낸다. 두 경로가 같은 시퀀스를
//! 공유하는 것은 사이드카 선례와 동일하다.

mod commands;
mod i18n;
mod state;
mod ui;

use commands::HostBridge;
use leftcar_host_desktop::{bootstrap, panel};
use std::sync::Arc;

fn main() {
    // Tauri와 무관한 기동 순서: 상태 프로필 → Ed25519 신원 → 페어링 서버 →
    // 소스 권한 잠금 → 세션 감사 → 공유 설정 → 컨트롤 7777 → AOAP 워처 →
    // mDNS 광고 → 가상 디스플레이·UPnP 매니저.
    let services = bootstrap::start_host_services().unwrap_or_else(|message| {
        eprintln!("host-gpui startup failed: {message}");
        std::process::exit(1);
    });

    // G-3: 호스트 시작 때 이전 프로세스가 남긴 좀비 가상 디스플레이를
    // 탐지·복구한다 — Tauri setup과 leftcar-hostd가 워커 스레드로 미루는
    // 것과 같은 시점이다.
    std::thread::Builder::new()
        .name("leftcar-vd-recovery".into())
        .spawn({
            let virtual_display = services.virtual_display.clone();
            move || virtual_display.recover_stale_if_due()
        })
        .expect("virtual display recovery thread");

    // 첫 폴링 전에 디스플레이 카탈로그를 데운다 — Tauri setup의
    // warm_display_catalog와 같은 시점이다.
    bootstrap::warm_display_catalog(services.warmup_backend.clone());

    #[cfg(unix)]
    spawn_sigterm_handler(&services);

    // 런타임은 브리지가 소유한다 — panel::serve 주변의 leftcar-hostd가
    // 런타임을 main 스코프에 두는 것과 같은 이유다.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("host-gpui bridge runtime");
    let handles = Arc::new(panel::PanelHandles {
        server: services.server.clone(),
        pairing: services.pairing.clone(),
        audit: services.audit.clone(),
        settings: services.settings.clone(),
        virtual_display: services.virtual_display.clone(),
        upnp: services.upnp.clone(),
        control_port: services.control_port,
        runtime: runtime.handle().clone(),
    });
    let bridge = HostBridge::with_runtime(handles, runtime);
    println!("host-gpui: control server on port {}", bridge.handles().control_port);

    // UI 진입은 코어 부트스트랩과 분리되어 있다 — 실제 대시보드 창은 T-3+
    // 에서 온다. 지금은 앱이 살아 있음을 증명하는 자리 표시자 창 하나다.
    ui::run(
        bridge,
        ui::UiShutdown {
            upnp: services.upnp.clone(),
            server: services.server.clone(),
            virtual_display: services.virtual_display.clone(),
            audit: services.audit.clone(),
        },
    );
}

/// SIGTERM·SIGINT 경로는 Tauri 앱과 같은 관측 결과를 만든다: 업데이터가
/// SIGTERM을 보내면 `bootstrap::run_exit_shutdown` 한 벌을 공유하고
/// 프로세스를 끝낸다(leftcar-hostd의 사이드카 경로와 동일). gpui 앱 핸들은
/// 스레드 건너 볼 수 없으니(Rc 기반) 자체 스레드에서 시퀀스를 완수한다.
#[cfg(unix)]
fn spawn_sigterm_handler(services: &bootstrap::HostServices) {
    let server = services.server.clone();
    let upnp = services.upnp.clone();
    let virtual_display = services.virtual_display.clone();
    let audit = services.audit.clone();
    std::thread::Builder::new()
        .name("leftcar-gpui-signal".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("host-gpui signal runtime unavailable: {error}");
                    return;
                }
            };
            let terminated = runtime.block_on(async {
                let mut terminate =
                    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        Ok(signal) => signal,
                        Err(error) => {
                            eprintln!("host-gpui SIGTERM hook unavailable: {error}");
                            return false;
                        }
                    };
                let mut interrupt =
                    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    {
                        Ok(signal) => signal,
                        Err(error) => {
                            eprintln!("host-gpui SIGINT hook unavailable: {error}");
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
            println!("host-gpui: shutting down on termination signal");
            bootstrap::run_exit_shutdown(
                upnp,
                &server,
                &virtual_display,
                &audit,
                |future| runtime.block_on(future),
            );
            std::process::exit(0);
        })
        .expect("signal handler thread");
}
