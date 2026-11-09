//! GPUI UI 진입(T-3/T-4). 코어 부트스트랩(main.rs)과 분리된 UI 진입점과
//! 창 목록이다.
//!
//! 창 옵션(설계 §2.1 원칙 3):
//! - 대시보드 780×540, 최소 640×440, app_id "leftcar-host"
//! - 페어링 420×560, 같은 app_id(같은 앱의 보조 창이다 — Hyprland
//!   windowrule이 app_id로 건네는 규칙을 공유한다)
//!
//! `gpui_kit::open_window`이 `base::Root`로 감싸므로 대화상자·시트·알림·
//! 메뉴 오버레이가 창 안에서 바로 동작한다.
//!
//! 브리지 접근: 창·모달 어디서든 [`HostBridge`]가 필요하므로 앱 전역
//! ([`BridgeGlobal`])에 둔다. 페어링 기기 병합 엔진도 대시보드 모달과
//! 페어링 창이 같은 상태를 봐야 하므로 [`PairedStoreGlobal`]로 공유한다.
//!
//! 종료 시퀀스: `on_app_quit`에서 `bootstrap::run_exit_shutdown`(Tauri
//! RunEvent::Exit와 같은 공용 시퀀스)을 전용 현재-스레드 런타임으로
//! 완수한다. UI 런타임을 재사용하지 않는 이유는 사이드카의 SIGTERM 경로가
//! 자체 런타임으로 block_on하던 선례와 같다 — 종료 시점에 앱 쪽 실행
//! 맥락이 내려가는 중일 수 있기 때문이다.

pub mod dashboard;
pub mod model;
pub mod pairing;
pub mod qr;
pub mod theme;
pub mod widgets;

use crate::commands::HostBridge;
use gpui_kit::{px, size, App, AppContext, Global, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions};
use leftcar_host_desktop::{audit, bootstrap, control, upnp, virtual_display};
use std::sync::Arc;

mod dashboard_actions {
    // 액션 모듈은 dashboard가 소유한다 — 여기서는 키 바인딩 등록만 본다.
    pub use crate::ui::dashboard::{ToggleHelp, TogglePairing, ToggleSettings, TriggerRefresh};
}

/// 앱 전역 브리지. 창·모달 콜백(&mut App)에서 명령을 보내는 유일한 경로다.
#[derive(Clone)]
pub struct BridgeGlobal(pub Arc<HostBridge>);

impl Global for BridgeGlobal {}

/// 앱 전역 페어링 기기 병합 엔진. React 앱의 모듈 전역 저장소
/// (useSyncExternalStore)와 같은 역할이며, 대시보드의 페어링 모달과 페어링
/// 창이 같은 revision 게이트·tombstone 상태를 본다.
pub struct PairedStoreGlobal(pub crate::state::paired_device_state::PairedDeviceStateStore);

impl Global for PairedStoreGlobal {}

/// 브리지를 전역에서 꺼낸다. init 전에 부르면 패닉이다.
pub fn bridge(cx: &App) -> Arc<HostBridge> {
    cx.global::<BridgeGlobal>().0.clone()
}

/// 정상 종료 때 `bootstrap::run_exit_shutdown`에 넘길 코어 핸들 한 벌.
/// main.rs가 HostServices에서 떼어 넘긴다.
pub struct UiShutdown {
    pub upnp: Arc<upnp::UpnpMappingManager>,
    pub server: Arc<control::ControlServer>,
    pub virtual_display: Arc<virtual_display::VirtualDisplayManager>,
    pub audit: Arc<audit::SessionAudit>,
}

/// GPUI 앱을 띄우고 대시보드 창을 연다. 앱이 끝날 때까지 돌아오지 않는다
/// (Tauri `run()`과 같은 소유 모델).
pub fn run(bridge: HostBridge, shutdown: UiShutdown) {
    gpui_kit::application().run(move |cx: &mut App| {
        // gpui-kit 컴포넌트·테마를 쓰기 전에 한 번 초기화한다.
        gpui_kit::init(cx);

        // 정상 종료 시퀀스 — RunEvent::Exit 선례의 GPUI 대응물.
        let _quit_subscription = cx.on_app_quit(move |_app| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("shutdown runtime");
            bootstrap::run_exit_shutdown(
                shutdown.upnp.clone(),
                &shutdown.server,
                &shutdown.virtual_display,
                &shutdown.audit,
                |future| runtime.block_on(future),
            );
            async {}
        });

        let bridge = Arc::new(bridge);
        cx.set_global(BridgeGlobal(bridge.clone()));
        cx.set_global(PairedStoreGlobal(
            crate::state::paired_device_state::PairedDeviceStateStore::new(),
        ));

        // 대시보드 키보드 단축키(App.tsx의 keydown 핸들러 대응). 컨텍스트
        // 없이 창 전역으로 듣는다 — 액션은 포커스 경로의 루트(대시보드 뷰
        // 컨테이너)의 on_action에서 받는다.
        use dashboard_actions::*;
        cx.bind_keys([
            KeyBinding::new("ctrl-p", TogglePairing, None),
            KeyBinding::new("ctrl-h", ToggleHelp, None),
            KeyBinding::new("ctrl-r", TriggerRefresh, None),
            KeyBinding::new("ctrl-,", ToggleSettings, None),
            KeyBinding::new("cmd-p", TogglePairing, None),
            KeyBinding::new("cmd-h", ToggleHelp, None),
            KeyBinding::new("cmd-r", TriggerRefresh, None),
            KeyBinding::new("cmd-,", ToggleSettings, None),
        ]);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(780.), px(540.)), cx)),
            window_min_size: Some(size(px(640.), px(440.))),
            titlebar: Some(TitlebarOptions {
                title: Some("Leftcar".into()),
                ..Default::default()
            }),
            // Wayland 창 규칙(Hyprland windowrule 등)이 class/app_id로
            // 건넨다 — 비워 두면 T-5 인디케이터의 "always-on-top·작업표시줄
            // 제외" 규칙이 붙을 대상이 없어진다(실측: app_id 미설정 시
            // hyprctl clients의 class가 빈 문자열이었다).
            app_id: Some("leftcar-host".into()),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            dashboard::DashboardView::new(window, cx)
        })
        .expect("failed to open the Leftcar dashboard window");
    });
}
