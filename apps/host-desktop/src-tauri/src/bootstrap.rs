//! Tauri와 무관한 호스트 서비스 기동·종료 순서의 단일 소스. Tauri 앱의
//! `run()`과 사이드카 바이너리(`leftcar-hostd`)가 같은 함수를 호출해 한 벌의
//! 기동 순서(상태 프로필 → Ed25519 신원 → 페어링 서버+토큰 저장소 → 소스 권한
//! 잠금 → 세션 감사 → 공유 설정+실험 환경변수 주입 → 컨트롤 서버 7777 바인딩
//! → AOAP USB 워처 → mDNS 광고 → 가상 디스플레이·UPnP 매니저)를 유지한다.
//!
//! 오류 처리만 호출자가 갈라 쓴다 — Tauri 앱은 대화상자+종료
//! (`fatal_startup_error`), 사이드카는 표준 오류+종료.

use crate::aoap_control;
use crate::audit;
use crate::backend::SharedBackend;
use crate::control;
use crate::identity;
use crate::pairing;
use crate::settings;
use crate::source_grants;
use crate::state_profile;
use crate::upnp;
use crate::virtual_display;
use crate::window_metrics;
use std::sync::Arc;

/// 종료 시퀀스가 UPnP 비활성 퓨처를 완수시키기 위해 호출자에게 맡기는 블로킹
/// 실행기. Tauri 앱은 `tauri::async_runtime::block_on`, 사이드카는 자체 tokio
/// 런타임을 넘긴다 — 런타임 소유를 호출자에 두면 기존 앱의 실행 맥락이
/// 그대로 유지된다.
pub type BlockFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

/// 부트스트랩이 만든 서비스 핸들 한 벌. Tauri 앱은 각각 `app.manage()`로,
/// 사이드카는 패널 서버와 종료 시퀀스에 그대로 넘긴다.
pub struct HostServices {
    pub backend: SharedBackend,
    /// 카탈로그 웜업용 복제 — 기존 앱이 setup에서 하던 시점에 호출자가 쓴다.
    pub warmup_backend: SharedBackend,
    pub server: Arc<control::ControlServer>,
    pub pairing: Arc<pairing::PairingServer>,
    pub audit: Arc<audit::SessionAudit>,
    pub settings: Arc<settings::SharedSettings>,
    pub virtual_display: Arc<virtual_display::VirtualDisplayManager>,
    pub upnp: Arc<upnp::UpnpMappingManager>,
    pub control_port: u16,
    /// 상태 프로필 잠금(프로세스 단위 단일 인스턴스 보장). 필드로 쥐고만
    /// 있으면 잠금이 살아 있다 — drop 시 해제된다.
    _profile_lock: std::fs::File,
}

/// 기동 순서 한 벌. 실패 메시지는 기존 `run()`의 것을 그대로 쓴다.
pub fn start_host_services() -> Result<HostServices, String> {
    let normal_data = dirs::data_dir();
    let profile = state_profile::StateProfile::from_environment(normal_data.as_deref())?;
    if profile.is_benchmark() {
        eprintln!("Leftcar Host: isolated internal benchmark profile");
    }
    let profile_lock = source_grants::lock_profile(
        &profile
            .file(".source-grants.lock")
            .ok_or("Host state directory unavailable")?,
    )?;
    let backend = platform_backend()?;
    let warmup_backend = backend.clone();
    // 호스트 정체 키: QR과 핸드셰이크 서명의 뿌리. 최초 기동에서 생성·영속된다.
    let identity = Arc::new(identity::load_or_create(
        (if profile.is_benchmark() {
            profile.file("host_identity.json")
        } else {
            identity::default_identity_path()
        })
        .as_deref(),
    )?);
    let pairing_store_path = if profile.is_benchmark() {
        profile.file("paired_devices.json")
    } else {
        pairing::PairingServer::default_store_path()
    };
    let pairing = Arc::new(pairing::PairingServer::new(
        identity.public_key(),
        pairing_store_path.clone(),
        if profile.is_benchmark() {
            pairing::token_store_with_service(pairing_store_path, profile.credential_service())
        } else {
            pairing::token_store(pairing_store_path)
        },
    ));
    pairing.initialize_source_grants(
        profile
            .file("source_grants.json")
            .ok_or("Host state directory unavailable")?,
    )?;
    let audit = Arc::new(audit::SessionAudit::new(if profile.is_benchmark() {
        profile.file("sessions.jsonl")
    } else {
        audit::SessionAudit::default_path()
    }));
    // 파일 공유 게이트: 기본 꺼짐, 승인 토글처럼 영속된다(0600 settings.json).
    let settings = Arc::new(settings::SharedSettings::load_or_default(
        if profile.is_benchmark() {
            profile.file("settings.json")
        } else {
            settings::default_settings_path()
        },
    ));
    // 실험 스위치 주입: 설정값을 프로세스 환경변수로 심는다. shim은 스트림
    // 시작마다 환경변수를 읽으므로(세션 생성이 dylib 로드보다 늦다) 앱 시작
    // 때 한 번 심으면 이후 모든 세션이 같은 값을 본다. 설정 변경 명령도 같은
    // 주입을 반복해 다음 스트림부터 반영된다.
    for (key, value) in settings::experiment_env_vars(&settings.experiment()) {
        std::env::set_var(key, value);
    }
    let server = Arc::new(control::ControlServer::new(
        backend.clone(),
        pairing.clone(),
        identity.clone(),
    ));
    server.set_audit(audit.clone());
    // 기기별 스트림 창 크기(XR 창 크기 유지). 미관 값이라 저장소 개봉 실패가
    // 기능을 막지 않는다 — 메모리 전용으로 낮춘다.
    server.set_window_metrics(Arc::new(window_metrics::WindowMetricsStore::open(
        profile.file("window_metrics.json"),
    )));
    // 클립보드 동기화 호스트 게이트(U5): 같은 0600 settings.json에서 읽고,
    // 손상 시 기본 꺼짐으로 되돌아간다. 토글은 즉시 효력을 가진다.
    server.set_clipboard_share(settings.clipboard_share());
    server.set_settings(settings.clone());
    // WAN 포트 매핑(외부 접속 허용) — 설정이 켜져 있을 때 컨트롤 서버 시작과
    // 함께 등록되고, 토글 off·종료 때 즉시 제거된다. 공개 미디어 엔드포인트는
    // 이 콜백을 통해 상태 스냅숏에 반영된다.
    let upnp = Arc::new(upnp::UpnpMappingManager::new());
    {
        let server_for_upnp = server.clone();
        upnp.set_endpoint_notify(Arc::new(move |endpoint| {
            server_for_upnp.set_public_media_endpoint(endpoint);
        }));
    }
    // 확장(가상) 디스플레이 매니저 — 프로브는 첫 사용 때 shim에서 늦게
    // 확인한다. 심볼 부재 플랫폼/빌드에서는 "지원 안 됨"으로만 존재한다.
    let virtual_display = Arc::new(virtual_display::VirtualDisplayManager::new());
    // 제어 채널의 createVirtualDisplay(태블릿 주도 생성)가 같은 매니저를 쓴다.
    server.set_virtual_display(virtual_display.clone());
    let (control_listener, control_port) = bind_control_listener()?;
    server.set_control_port(control_port);
    start_control_server(
        server.clone(),
        control_listener,
        control_port,
        settings.clone(),
        upnp.clone(),
    );
    // AOAP devices re-enumerate after the accessory handshake. Keep discovery
    // independent from the window lifecycle so a cable connection is available
    // before the viewer asks for its first USB stream.
    aoap_control::start_usb_watcher(server.clone());
    // Advertise independently from the window lifecycle so a viewer can
    // connect while UI initialization is still in progress.
    if let Err(e) = advertise_mdns(control_port) {
        eprintln!("mDNS advertise failed: {e}");
    }
    Ok(HostServices {
        warmup_backend,
        backend,
        server,
        pairing,
        audit,
        settings,
        virtual_display,
        upnp,
        control_port,
        _profile_lock: profile_lock,
    })
}

#[cfg(target_os = "macos")]
fn platform_backend() -> Result<SharedBackend, String> {
    let backend = crate::ffi::FfiBackend::new()?;
    println!("{}", crate::ffi::dylib_report());
    Ok(Arc::new(backend))
}

#[cfg(target_os = "windows")]
fn platform_backend() -> Result<SharedBackend, String> {
    Ok(Arc::new(crate::windows_backend::WindowsBackend::new()?))
}

#[cfg(target_os = "linux")]
fn platform_backend() -> Result<SharedBackend, String> {
    Ok(Arc::new(crate::linux_backend::LinuxBackend::new()?))
}

/// Start the control plane before any UI lifecycle. This keeps the host
/// connectable while UI initialization is slow or blocked by a desktop
/// permission prompt.
pub(crate) fn bind_control_listener() -> Result<(std::net::TcpListener, u16), String> {
    bind_control_listener_at(crate::PREFERRED_CONTROL_PORT)
}

pub(crate) fn bind_control_listener_at(
    preferred_port: u16,
) -> Result<(std::net::TcpListener, u16), String> {
    let listener = match std::net::TcpListener::bind(("0.0.0.0", preferred_port)) {
        Ok(listener) => listener,
        Err(preferred_error) => {
            eprintln!(
                "control port {preferred_port} unavailable ({preferred_error}); selecting a free \
                 port (another Leftcar Host instance may be running)"
            );
            std::net::TcpListener::bind(("0.0.0.0", 0)).map_err(|fallback_error| {
                format!(
                    "port {preferred_port}: {preferred_error}; fallback: {fallback_error}; \
                     another Leftcar Host instance may be running"
                )
            })?
        }
    };
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("control listener nonblocking mode: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("control listener address: {error}"))?
        .port();
    Ok((listener, port))
}

fn start_control_server(
    server: std::sync::Arc<control::ControlServer>,
    listener: std::net::TcpListener,
    port: u16,
    settings: std::sync::Arc<settings::SharedSettings>,
    upnp: std::sync::Arc<upnp::UpnpMappingManager>,
) {
    // Capture startup intent before the thread can be delayed behind a later
    // UI disable or application shutdown.
    let startup_mapping = if settings.wan_access() {
        local_lan_ip().map(|local_ip| upnp.enable(port, local_ip))
    } else {
        None
    };
    std::thread::Builder::new()
        .name("leftcar-control".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("control runtime");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener)
                    .expect("convert control listener to tokio");
                println!("control server on 0.0.0.0:{port}");
                if let Some(mapping) = startup_mapping {
                    tokio::spawn(mapping);
                } else {
                    println!("UPnP: WAN access is off in settings (LAN direct only)");
                }
                server.run(listener).await;
            });
        })
        .expect("control server thread");
}

/// Register `_leftcar._tcp.local.` with the listener's actual control port.
/// The ServiceDaemon is leaked on purpose — it must outlive the app setup.
fn advertise_mdns(port: u16) -> Result<(), String> {
    use std::collections::HashMap;
    let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| format!("mdns daemon: {e:?}"))?;
    let ip = local_lan_ip().ok_or("no LAN interface found")?;
    let info = mdns_sd::ServiceInfo::new(
        "_leftcar._tcp.local.",
        "leftcar-host",
        "leftcar-host.local.",
        &ip,
        port,
        // Android's NSD resolver rejects an empty TXT property set on some
        // vendor builds ("Key cannot be empty"). Keep one stable property so
        // multiple hosts can still be resolved and listed independently.
        HashMap::from([(String::from("product"), String::from("leftcar"))]),
    )
    .map_err(|e| format!("mdns service info: {e:?}"))?;
    daemon
        .register(info)
        .map_err(|e| format!("mdns register: {e:?}"))?;
    println!("mDNS: leftcar-host._leftcar._tcp.local. at {ip}:{port}");
    std::mem::forget(daemon);
    Ok(())
}

/// Best-effort local interface address (UDP connect trick — no packets sent).
pub fn local_lan_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}

/// 이 호스트의 오버레이 네트워크(100.64.0.0/10 — Tailscale) 자기 주소.
/// 같은 UDP connect 트릭으로 CGNAT 대역 라우트가 고르는 인터페이스 주소를
/// 읽는데, 테일넷이 내려가 있으면 기본 라우트의 LAN 주소가 돌아오므로
/// 결과가 100.64/10일 때만 채택한다. 연결 트릭은 패킷을 보내지 않는다.
pub fn tailscale_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("100.100.100.100:80").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    let octets = match ip {
        std::net::IpAddr::V4(v4) => v4.octets(),
        std::net::IpAddr::V6(_) => return None,
    };
    (octets[0] == 100 && (64..=127).contains(&octets[1])).then(|| ip.to_string())
}

/// Prime the display catalog before the first viewer opens it. On macOS this
/// is a synchronous CoreGraphics metadata read; ScreenCaptureKit consent is
/// requested only when the viewer starts a stream.
pub fn warm_display_catalog(backend: SharedBackend) {
    let _ = std::thread::Builder::new()
        .name("leftcar-catalog-warmup".into())
        .spawn(move || {
            // Let the AppKit event loop become live before the first catalog
            // probe so a subsequent system picker can be presented cleanly.
            std::thread::sleep(std::time::Duration::from_millis(500));
            match backend.list_displays() {
                Ok(displays) => println!("display catalog warm: {} display(s)", displays.len()),
                Err(error) => eprintln!("display catalog warmup deferred: {error}"),
            }
        });
}

/// 종료 공용 시퀀스 — Tauri 앱의 `RunEvent::Exit`와 사이드카의 SIGTERM 경로가
/// 같은 함수를 쓴다. WAN 포트 매핑을 즉시 치우고(3초 상한), 저장된 기기
/// 승인을 닫고, 가상 디스플레이를 해제한다(2초 상한 + 감사).
pub fn run_exit_shutdown(
    upnp: Arc<upnp::UpnpMappingManager>,
    server: &Arc<control::ControlServer>,
    virtual_display: &Arc<virtual_display::VirtualDisplayManager>,
    audit: &Arc<audit::SessionAudit>,
    block_on: impl Fn(BlockFuture),
) {
    // WAN 포트 매핑을 종료 때 즉시 치운다. 응답 없는 게이트웨이 앞에서
    // 종료가 붙잡히지 않게 3초로 잘라내고, 못 치운 매핑은 lease(1시간)가
    // 만료시킨다.
    block_on(Box::pin(async move {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), upnp.disable()).await;
    }));
    if let Err(error) = server.shutdown_source_access() {
        eprintln!("Host grant shutdown uncertain: {error}");
    }
    // 확장(가상) 디스플레이도 종료 시점에 확실히 끈다(세션 스코프
    // 소유 정책, 2026-09-22 종료 감사 §9.2). 브리지 destroy 폴링
    // (최대 ~9.5s)이 종료를 붙잡지 않게 2초 상한만 두고, 상한 내
    // 못 끝낸 해제는 프로세스 사망 시 WindowServer 회수(E-1)가
    // 백스톱한다. tokio 런타임이 내려가는 시점이라 전용 스레드로
    // 돌린다(remove_blocking_within). 해제 결과는 감사로 남겨
    // 종료 경로의 관측 공백을 닫는다(report §6 수정안 3).
    let had_display = virtual_display.has_live_display();
    match virtual_display.remove_blocking_within(std::time::Duration::from_secs(2)) {
        Some(Ok(vanished)) => {
            if had_display {
                audit.log(
                    "virtual_display_released",
                    serde_json::json!({"reason": "exit", "vanished": vanished}),
                );
            }
        }
        Some(Err(error)) => {
            eprintln!("virtual display release on exit failed: {error}");
            audit.log(
                "virtual_display_release_failed",
                serde_json::json!({"reason": "exit", "error": error}),
            );
        }
        None => {
            eprintln!("virtual display release on exit timed out; relying on WindowServer reclaim");
            audit.log(
                "virtual_display_release_failed",
                serde_json::json!({
                    "reason": "exit_timeout",
                    "error": "release timed out after 2s"
                }),
            );
        }
    }
}
