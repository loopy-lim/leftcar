//! panel::dispatch 1:1 위임 타입 래퍼(AC-H1). 각 메서드는 [`panel::dispatch`]
//! 표의 같은 이름 분기와 명령 이름·인자(camelCase)·결과 JSON 형태가 정확히
//! 같다 — 원본은 src-tauri `lib.rs`의 `#[tauri::command]`이고, dispatch가
//! 그것을 개행 JSON으로 미러한 표다.
//!
//! 실행 모델(설계 §2.1 원칙 1): [`HostBridge::call`]은 동기 함수이고 UI
//! 스레드만 아니면 어느 스레드에서든 안전하다. 비동기 명령(가상 디스플레이·
//! 커튼·UPnP·파일 대화상자)은 dispatch 내부에서 주입된 런타임 핸들의
//! block_on으로 완수된다 — 패널 프로토콜 서버의 내장 방식과 동일하다.
//! 토큰·포트 파일·TCP 클라이언트는 이 경로에 없다(같은 프로세스 위임).
//!
//! 폴링 규율(설계 §2.1 원칙 1): 상태 폴링은 UI 스레드를 막으면 안 되므로
//! [`HostBridge::request`]가 전용 워커 스레드로 작업을 보내고 채널로 결과를
//! 돌려준다. T-3 대시보드 폴러가 이 경로를 쓴다.
//!
//! 결과 역직렬화: dispatch 결과는 "각 Tauri 명령 반환형의 JSON 표현"이므로
//! `serde_json::from_value`로 실제 뷰 타입으로 돌린다. lib 크레이트 뷰 타입
//! 가운데 Serialize만 구현한 것(PairedDeviceState, RevokeOutcome,
//! VirtualDisplayStatusPublic 등)은 이 크레이트가 같은 JSON 모양의 미러
//! 타입을 정의해 읽는다 — JSON 계약은 그대로고 lib 타입을 건드리지 않는다.

// 47명령 표면은 T-3~T-5 UI가 단계적으로 소비한다 — 그때까지 bin 크레이트의
// 미사용 경고만 끈다(테스트는 이미 전부를 소비한다).
#![allow(dead_code)]

use crate::state::paired_device_state::{
    PairedDevice as PairedDeviceMirror, PairedDeviceStateSnapshot,
    RevokeOutcome as RevokeOutcomeMirror,
};
use crate::state::session_row::SessionRow;
use control_contract::host::DisplayInfo;
use control_contract::km::{KmStatus, KmViewerConsent};
use leftcar_host_desktop::control::StatusViewPublic;
use leftcar_host_desktop::panel::{self, PanelHandles};
use leftcar_host_desktop::settings::{ExperimentSettings, HostLanguage};
use leftcar_host_desktop::source_grants::GrantView;
use leftcar_host_desktop::virtual_display::{DisplayMode, DisplayPosition};
use serde::de::DeserializeOwned;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// lib 크레이트 뷰 타입의 JSON을 그대로 읽는 미러 모음. Serialize 전용인
/// lib 타입의 클라이언트 쪽 상대 타입이다(위 모듈 문서).
pub mod mirrors {
    use serde::{Deserialize, Serialize};

    /// lib `pairing::PairingSessionView`의 미러(begin_pairing). 필드는
    /// snake_case다 — lib 타입에 rename_all이 없다.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct PairingSessionView {
        pub offer_id: String,
        pub qr_payload: String,
        pub code: String,
        pub expires_in_secs: u64,
    }

    /// lib `pairing::PendingPairingView`의 미러(list_pending_pairings).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct PendingPairingView {
        pub offer_id: String,
        pub device_name: String,
        pub requested_at: String,
    }

    /// lib `control::PendingInputRequestView`의 미러(list_input_requests).
    /// React 앱의 InputRequestRow와 같은 camelCase 모양이다.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct InputRequestRow {
        pub session: u32,
        #[serde(default)]
        pub device: Option<String>,
        #[serde(default)]
        pub age_ms: u64,
    }

    /// lib `virtual_display::VirtualDisplayStatusPublic`의 미러.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct VirtualDisplayStatus {
        pub supported: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub live: Option<LiveVirtualDisplay>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub suggested: Option<SuggestedMode>,
        #[serde(default)]
        pub removal_pending: bool,
    }

    /// lib `virtual_display::LiveVirtualDisplayPublic`의 미러.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LiveVirtualDisplay {
        pub display_id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub source_id: Option<String>,
        pub name: String,
        pub logical_width: u32,
        pub logical_height: u32,
        pub scale: u32,
        pub backing_width: u32,
        pub backing_height: u32,
        pub mode_verified: bool,
    }

    /// lib `virtual_display::SuggestedModePublic`의 미러. `source`는
    /// "viewerMetrics"(최근 뷰어 보고 패널 도출) 또는 "fallback"(프리셋
    /// 기본값)이다.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SuggestedMode {
        pub width: u32,
        pub height: u32,
        pub scale: u8,
        pub source: String,
    }

    /// lib `file_transfer::ShareQueueEntry`의 미러(path는 직렬화에서 뺀다).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ShareQueueEntry {
        pub queue_id: String,
        pub name: String,
        pub size: u64,
    }

    /// lib `file_transfer::AddShareFilesResult`의 미러(add_share_files).
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct AddShareFilesResult {
        #[serde(default)]
        pub entries: Vec<ShareQueueEntry>,
        #[serde(default)]
        pub rejected: Vec<RejectedShareFile>,
    }

    /// lib `file_transfer::RejectedShareFile`의 미러.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct RejectedShareFile {
        pub name: String,
        pub reason: String,
    }
}

/// 워커 스레드에 보내는 폴링 작업 한 건. 작업마다 독립 응답 채널을 둔다 —
/// 요청 순서 처리는 폴링 클라이언트마다의 관심사고, 여기서는 응답 지연
/// 결합을 만들지 않는다.
struct Job {
    command: String,
    args: serde_json::Value,
    reply: std::sync::mpsc::SyncSender<Result<serde_json::Value, String>>,
}

/// 전용 폴링 워커. `leftcar-panel-conn` 선례와 같은 스레드 기반이다 — UI
/// 스레드는 절대 block_on하지 않는다.
struct Worker {
    sender: Option<Sender<Job>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    fn spawn(handles: Arc<PanelHandles>) -> Worker {
        let (sender, receiver) = std::sync::mpsc::channel::<Job>();
        let join = std::thread::Builder::new()
            .name("leftcar-gpui-bridge".into())
            .spawn(move || {
                // 요청을 순서대로 완수한다(패널 연결의 폴링 의미론과 같다).
                while let Ok(job) = receiver.recv() {
                    let outcome = panel::dispatch(&handles, &job.command, &job.args);
                    let _ = job.reply.send(outcome);
                }
            })
            .expect("bridge worker thread");
        Worker {
            sender: Some(sender),
            join: Some(join),
        }
    }

    fn send(&self, job: Job) -> Result<(), Job> {
        match self.sender.as_ref() {
            Some(sender) => sender.send(job).map_err(|error| error.0),
            None => Err(job),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // sender를 먼저 떨어뜨려 워커 루프를 종료시키고 스레드를 기다린다.
        self.sender = None;
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Tauri 앱과 같은 단일 프로세스 브리지. 코어 핸들 한 벌과 폴링 워커,
/// 그 런타임을 소유한다.
pub struct HostBridge {
    handles: Arc<PanelHandles>,
    _worker: Worker,
    /// PanelHandles의 비동기 명령이 이 런타임 핸들로 block_on하므로 브리지가
    /// 살아 있는 동안 런타임도 살아 있어야 한다 — 소유권으로 그 불변을 닫는다.
    _runtime: tokio::runtime::Runtime,
}

impl HostBridge {
    /// 자체 런타임을 만드는 편의 생성자. 실제 앱은 [`HostBridge::with_runtime`]
    /// 으로 런타임을 명시한다.
    pub fn new(handles: Arc<PanelHandles>) -> HostBridge {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("host-gpui bridge runtime");
        HostBridge::with_runtime(handles, runtime)
    }

    /// 런타임을 소유권으로 받는 생성자.
    pub fn with_runtime(handles: Arc<PanelHandles>, runtime: tokio::runtime::Runtime) -> HostBridge {
        let worker = Worker::spawn(handles.clone());
        HostBridge {
            handles,
            _worker: worker,
            _runtime: runtime,
        }
    }

    pub fn handles(&self) -> &Arc<PanelHandles> {
        &self.handles
    }

    /// 동기 호출. UI 스레드만 아니면 어느 스레드에서든 안전하다(비동기
    /// 명령은 내부 런타임 핸들이 block_on한다 — 런타임 워커 스레드 안에서
    /// 부르면 안 된다는 뜻이기도 하다). 상시 폴링은 [`HostBridge::request`]
    /// 로 워커에 맡긴다.
    pub fn call(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        panel::dispatch(&self.handles, command, args)
    }

    /// 전용 워커 스레드로 보내고 결과 채널을 돌려준다. 2초 폴링 같은 상시
    /// 조회는 이 경로로 UI 스레드 밖에서 돌아간다.
    pub fn request(
        &self,
        command: String,
        args: serde_json::Value,
    ) -> Receiver<Result<serde_json::Value, String>> {
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
        let job = Job {
            command,
            args,
            reply: reply_tx,
        };
        if self._worker.send(job).is_err() {
            // 워커가 내려갔다 — 오류를 그대로 결과 채널로 돌려준다.
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let _ = tx.send(Err("bridge worker is shut down".into()));
            return rx;
        }
        reply_rx
    }

    /// dispatch 결과를 실제 뷰 타입으로 역직렬화하는 공용 헬퍼.
    fn typed<T: DeserializeOwned>(&self, command: &str, args: &serde_json::Value) -> Result<T, String> {
        let value = self.call(command, args)?;
        serde_json::from_value(value).map_err(|error| format!("response decode failed: {error}"))
    }

    /// json(()) 결과(빈 단위 응답)를 받는 헬퍼.
    fn call_unit(&self, command: &str, args: &serde_json::Value) -> Result<(), String> {
        self.call(command, args).map(|_| ())
    }

    // ===== 아래 47개는 panel::dispatch 표와 1:1 위임 메서드다 =====

    pub fn get_status(&self) -> Result<StatusViewPublic, String> {
        self.typed("get_status", &serde_json::json!({}))
    }

    /// get_status 세션을 UI용 SessionRow(JSON 계약 동일)로 읽는 편의.
    pub fn sessions_as_rows(&self) -> Result<Vec<SessionRow>, String> {
        let status = self.get_status()?;
        let value = serde_json::to_value(&status)
            .map_err(|error| format!("response serialization failed: {error}"))?;
        serde_json::from_value(value["sessions"].clone())
            .map_err(|error| format!("session decode failed: {error}"))
    }

    pub fn list_input_requests(&self) -> Result<Vec<mirrors::InputRequestRow>, String> {
        self.typed("list_input_requests", &serde_json::json!({}))
    }

    pub fn get_host_platform(&self) -> Result<String, String> {
        self.typed("get_host_platform", &serde_json::json!({}))
    }

    pub fn get_control_port(&self) -> Result<u16, String> {
        self.typed("get_control_port", &serde_json::json!({}))
    }

    pub fn get_lan_ip(&self) -> Result<Option<String>, String> {
        self.typed("get_lan_ip", &serde_json::json!({}))
    }

    pub fn get_input_permission(&self) -> Result<bool, String> {
        self.typed("get_input_permission", &serde_json::json!({}))
    }

    pub fn get_km_status(&self, device_id: &str) -> Result<KmStatus, String> {
        self.typed("get_km_status", &serde_json::json!({ "deviceId": device_id }))
    }

    pub fn get_km_consent(&self, device_id: &str) -> Result<KmViewerConsent, String> {
        self.typed("get_km_consent", &serde_json::json!({ "deviceId": device_id }))
    }

    pub fn set_km_consent(
        &self,
        device_id: &str,
        mac_to_android: bool,
        android_to_mac: bool,
    ) -> Result<KmViewerConsent, String> {
        self.typed(
            "set_km_consent",
            &serde_json::json!({
                "deviceId": device_id,
                "macToAndroid": mac_to_android,
                "androidToMac": android_to_mac,
            }),
        )
    }

    pub fn request_input_permission(&self) -> Result<bool, String> {
        self.typed("request_input_permission", &serde_json::json!({}))
    }

    pub fn get_screen_permission(&self) -> Result<bool, String> {
        self.typed("get_screen_permission", &serde_json::json!({}))
    }

    /// macOS가 아니면 시스템 설정 패널이 없다 — dispatch와 같이 no-op 성공.
    pub fn open_system_settings(&self, pane: Option<&str>) -> Result<(), String> {
        self.call_unit("open_system_settings", &serde_json::json!({ "pane": pane }))
    }

    pub fn set_session_input(&self, session: u32, enabled: bool) -> Result<(), String> {
        self.call_unit(
            "set_session_input",
            &serde_json::json!({ "session": session, "enabled": enabled }),
        )
    }

    pub fn set_session_quality(&self, session: u32, quality: Option<f32>) -> Result<(), String> {
        self.call_unit(
            "set_session_quality",
            &serde_json::json!({ "session": session, "quality": quality }),
        )
    }

    pub fn force_stop_session(&self, session: u32) -> Result<(), String> {
        self.call_unit("force_stop_session", &serde_json::json!({ "session": session }))
    }

    pub fn begin_pairing(&self) -> Result<mirrors::PairingSessionView, String> {
        self.typed("begin_pairing", &serde_json::json!({}))
    }

    pub fn cancel_pairing(&self, offer_id: &str) -> Result<bool, String> {
        self.typed("cancel_pairing", &serde_json::json!({ "offerId": offer_id }))
    }

    pub fn list_pending_pairings(&self) -> Result<Vec<mirrors::PendingPairingView>, String> {
        self.typed("list_pending_pairings", &serde_json::json!({}))
    }

    pub fn approve_pending_pairing(&self, offer_id: &str) -> Result<(), String> {
        self.call_unit("approve_pending_pairing", &serde_json::json!({ "offerId": offer_id }))
    }

    pub fn reject_pending_pairing(&self, offer_id: &str) -> Result<(), String> {
        self.call_unit("reject_pending_pairing", &serde_json::json!({ "offerId": offer_id }))
    }

    pub fn list_paired_devices(&self) -> Result<Vec<PairedDeviceMirror>, String> {
        self.typed("list_paired_devices", &serde_json::json!({}))
    }

    pub fn list_paired_device_state(&self) -> Result<PairedDeviceStateSnapshot, String> {
        self.typed("list_paired_device_state", &serde_json::json!({}))
    }

    pub fn list_host_sources(&self) -> Result<Vec<DisplayInfo>, String> {
        self.typed("list_host_sources", &serde_json::json!({}))
    }

    pub fn set_source_grants(
        &self,
        device_id: &str,
        source_ids: &[String],
        credential_id: &str,
    ) -> Result<GrantView, String> {
        self.typed(
            "set_source_grants",
            &serde_json::json!({
                "deviceId": device_id,
                "sourceIds": source_ids,
                "credentialId": credential_id,
            }),
        )
    }

    pub fn virtual_display_status(&self) -> Result<mirrors::VirtualDisplayStatus, String> {
        self.typed("virtual_display_status", &serde_json::json!({}))
    }

    pub fn virtual_display_arrange(&self, position: DisplayPosition) -> Result<(), String> {
        self.call_unit(
            "virtual_display_arrange",
            &serde_json::json!({ "position": position }),
        )
    }

    pub fn virtual_display_resize(&self, mode: DisplayMode) -> Result<mirrors::LiveVirtualDisplay, String> {
        self.typed(
            "virtual_display_resize",
            &serde_json::json!({ "width": mode.width, "height": mode.height, "scale": mode.scale }),
        )
    }

    /// 확장 디스플레이 생성(수 초 걸림 — 워커 스레드에서 request()로 부른다).
    pub fn virtual_display_create(
        &self,
        width: u32,
        height: u32,
        scale: u32,
    ) -> Result<mirrors::VirtualDisplayStatus, String> {
        self.typed(
            "virtual_display_create",
            &serde_json::json!({ "width": width, "height": height, "scale": scale }),
        )
    }

    pub fn virtual_display_remove(&self) -> Result<mirrors::VirtualDisplayStatus, String> {
        self.typed("virtual_display_remove", &serde_json::json!({}))
    }

    pub fn revoke_paired_device(&self, device_id: &str) -> Result<RevokeOutcomeMirror, String> {
        self.typed("revoke_paired_device", &serde_json::json!({ "deviceId": device_id }))
    }

    pub fn revoke_all_devices(&self) -> Result<RevokeOutcomeMirror, String> {
        self.typed("revoke_all_devices", &serde_json::json!({}))
    }

    pub fn get_clipboard_share(&self) -> Result<bool, String> {
        self.typed("get_clipboard_share", &serde_json::json!({}))
    }

    pub fn set_clipboard_share(&self, enabled: bool) -> Result<bool, String> {
        self.typed("set_clipboard_share", &serde_json::json!({ "enabled": enabled }))
    }

    pub fn get_file_share(&self) -> Result<bool, String> {
        self.typed("get_file_share", &serde_json::json!({}))
    }

    pub fn set_file_share(&self, enabled: bool) -> Result<(), String> {
        self.call_unit("set_file_share", &serde_json::json!({ "enabled": enabled }))
    }

    pub fn get_privacy_settings(&self) -> Result<bool, String> {
        self.typed("get_privacy_settings", &serde_json::json!({}))
    }

    pub fn get_experiments(&self) -> Result<ExperimentSettings, String> {
        self.typed("get_experiments", &serde_json::json!({}))
    }

    pub fn set_experiments(&self, experiment: ExperimentSettings) -> Result<ExperimentSettings, String> {
        self.typed("set_experiments", &serde_json::json!({ "experiment": experiment }))
    }

    pub fn set_privacy_curtain(&self, enabled: bool) -> Result<(), String> {
        self.call_unit("set_privacy_curtain", &serde_json::json!({ "enabled": enabled }))
    }

    /// 파일 대화상자를 워커 스레드에서 띄운다(dispatch 내장) — 인자 없음.
    pub fn add_share_files(&self) -> Result<mirrors::AddShareFilesResult, String> {
        self.typed("add_share_files", &serde_json::json!({}))
    }

    pub fn list_share_queue(&self) -> Result<Vec<mirrors::ShareQueueEntry>, String> {
        self.typed("list_share_queue", &serde_json::json!({}))
    }

    pub fn remove_share_file(&self, queue_id: &str) -> Result<bool, String> {
        self.typed("remove_share_file", &serde_json::json!({ "queueId": queue_id }))
    }

    pub fn set_language(&self, language: HostLanguage) -> Result<(), String> {
        self.call_unit("set_language", &serde_json::json!({ "language": language.as_str() }))
    }

    pub fn get_streaming_badge(&self) -> Result<bool, String> {
        self.typed("get_streaming_badge", &serde_json::json!({}))
    }

    pub fn set_streaming_badge(&self, enabled: bool) -> Result<bool, String> {
        self.typed("set_streaming_badge", &serde_json::json!({ "enabled": enabled }))
    }

    pub fn get_wan_access(&self) -> Result<bool, String> {
        self.typed("get_wan_access", &serde_json::json!({}))
    }

    pub fn set_wan_access(&self, enabled: bool) -> Result<bool, String> {
        self.typed("set_wan_access", &serde_json::json!({ "enabled": enabled }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leftcar_host_desktop::audit;
    use leftcar_host_desktop::backend::FakeBackend;
    use leftcar_host_desktop::control;
    use leftcar_host_desktop::pairing;
    use leftcar_host_desktop::settings::{self, SharedSettings};
    use leftcar_host_desktop::upnp;
    use leftcar_host_desktop::virtual_display::VirtualDisplayManager;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    /// panel.rs TestHarness와 같은 조립: FakeBackend + 메모리 저장소.
    /// 패널 서버 대신 브리지를 세운다 — 같은 dispatch 표를 스레드 경계
    /// 없이 부른다.
    fn bridge() -> HostBridge {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("bridge test runtime");
        let backend = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                index: 0,
                source_id: Some("test:display:0".into()),
                name: "Main Display".into(),
                width: 1920,
                height: 1080,
            }],
            encoder_experiment: Mutex::new(control_contract::host::EncoderExperiment::Auto),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
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
            settings: SharedSettings::in_memory(),
            virtual_display: Arc::new(VirtualDisplayManager::new()),
            upnp: Arc::new(upnp::UpnpMappingManager::new()),
            control_port: 7777,
            runtime: runtime.handle().clone(),
        });
        HostBridge::with_runtime(handles, runtime)
    }

    #[test]
    fn typed_wrappers_round_trip_control_port_and_platform() {
        let bridge = bridge();
        let expected_port = bridge.handles().control_port;
        assert_eq!(bridge.get_control_port().unwrap(), expected_port);
        assert_eq!(bridge.get_host_platform().unwrap(), "test");
        // LAN IP 조회는 환경에 따라 Some/None이 갈린다 — 형태만 고정한다.
        let lan_ip = bridge.get_lan_ip().unwrap();
        assert!(lan_ip.as_ref().is_none_or(|ip| !ip.is_empty()));
    }

    #[test]
    fn call_works_from_a_plain_worker_thread() {
        let bridge = Arc::new(bridge());
        let handle = std::thread::spawn(move || bridge.get_control_port());
        assert_eq!(handle.join().unwrap().unwrap(), 7777);
    }

    #[test]
    fn request_delivers_results_through_the_worker_channel() {
        let bridge = bridge();
        let receiver = bridge.request("get_control_port".into(), serde_json::json!({}));
        assert_eq!(receiver.recv().unwrap().unwrap(), serde_json::json!(7777));
    }

    #[test]
    fn experiments_round_trip_including_frame_trace() {
        let bridge = bridge();
        let experiment = ExperimentSettings {
            max_encode_in_flight: Some(3),
            frame_trace: true,
            ..Default::default()
        };
        let saved = bridge.set_experiments(experiment).unwrap();
        assert_eq!(saved.max_encode_in_flight, Some(3));
        assert!(saved.frame_trace);
        let loaded = bridge.get_experiments().unwrap();
        assert_eq!(loaded, saved);
    }

    #[test]
    fn set_language_persists_and_rejects_unknown_codes() {
        let bridge = bridge();
        bridge.set_language(HostLanguage::En).unwrap();
        assert_eq!(
            bridge.handles().settings.language(),
            settings::HostLanguage::En
        );
        let error = bridge
            .call("set_language", &serde_json::json!({ "language": "fr" }))
            .unwrap_err();
        assert!(error.contains("unsupported language"), "error: {error}");
    }

    #[test]
    fn unknown_command_surfaces_the_dispatch_error_string() {
        let bridge = bridge();
        let error = bridge
            .call("not_a_real_command", &serde_json::json!({}))
            .unwrap_err();
        assert_eq!(error, "unknown command: not_a_real_command");
    }

    #[test]
    fn missing_arguments_are_command_errors_not_bridge_errors() {
        let bridge = bridge();
        let error = bridge
            .call("get_km_status", &serde_json::json!({}))
            .unwrap_err();
        assert_eq!(error, "missing argument: deviceId");
    }

    #[test]
    fn list_share_queue_starts_empty() {
        let bridge = bridge();
        assert_eq!(bridge.list_share_queue().unwrap(), Vec::new());
    }

    #[test]
    fn clipboard_and_badge_toggles_round_trip() {
        let bridge = bridge();
        assert!(!bridge.get_clipboard_share().unwrap());
        assert!(bridge.set_clipboard_share(true).unwrap());
        assert!(bridge.get_clipboard_share().unwrap());
        assert!(!bridge.get_file_share().unwrap());
        bridge.set_file_share(true).unwrap();
        assert!(bridge.get_file_share().unwrap());
        assert!(!bridge.get_streaming_badge().unwrap());
        assert!(bridge.set_streaming_badge(true).unwrap());
        assert!(bridge.get_streaming_badge().unwrap());
    }

    #[test]
    fn km_reads_and_the_pairing_gate_round_trip() {
        let bridge = bridge();
        // 페어링되지 않은 기기에는 동의 저장이 막힌다(서비스 게이트가 문자열
        // 오류로 답한다 — dispatch의 오류 경로).
        let error = bridge
            .set_km_consent("device-1", true, false)
            .unwrap_err();
        assert!(
            error.contains("Pair the device"),
            "error: {error}"
        );
        // 읽기는 페어링 전 기본값으로 답한다.
        let consent = bridge.get_km_consent("device-1").unwrap();
        assert_eq!(
            consent,
            KmViewerConsent {
                mac_to_android: false,
                android_to_mac: false,
            }
        );
        let status = bridge.get_km_status("device-1").unwrap();
        assert!(!status.armed);
    }

    #[test]
    fn paired_device_state_round_trips_through_the_merge_mirror() {
        let bridge = bridge();
        let snapshot = bridge.list_paired_device_state().unwrap();
        assert!(snapshot.revision >= 0);
        assert!(snapshot.devices.is_empty());
        assert_eq!(bridge.list_paired_devices().unwrap(), Vec::new());
        // lib Serialize 전용 타입의 JSON이 미러 타입과 정확히 같은 모양인지
        // 실물 라운드트립으로 고정한다.
        let raw = serde_json::to_value(bridge.handles().pairing.list_device_state()).unwrap();
        let mirror: PairedDeviceStateSnapshot = serde_json::from_value(raw).unwrap();
        assert_eq!(mirror, snapshot);
    }

    #[test]
    fn revoke_outcome_round_trips_with_empty_state() {
        let bridge = bridge();
        let outcome = bridge.revoke_all_devices().unwrap();
        assert!(outcome.removed_devices.is_empty());
        assert!(outcome.persistence_errors.is_empty());
    }

    #[test]
    fn virtual_display_status_reports_shape() {
        let bridge = bridge();
        let status = bridge.virtual_display_status().unwrap();
        // 만든 적 없는 프로세스에서 제거 진행 상태는 없다.
        assert!(!status.removal_pending);
    }

    #[test]
    fn session_rows_decode_from_get_status() {
        let bridge = bridge();
        assert!(bridge.sessions_as_rows().unwrap().is_empty());
    }

    #[test]
    fn open_system_settings_is_a_noop_off_macos() {
        let bridge = bridge();
        bridge.open_system_settings(Some("screen_capture")).unwrap();
    }

    #[test]
    fn source_grants_error_surfaces_for_unknown_device() {
        let bridge = bridge();
        let outcome = bridge.set_source_grants("device-1", &["test:display:0".into()], "cred-1");
        assert!(outcome.is_err(), "페어링되지 않은 기기에는 권한을 못 준다");
    }

    #[test]
    fn unit_responses_decode_into_unit() {
        let bridge = bridge();
        bridge.set_privacy_curtain(false).unwrap();
        bridge.set_privacy_curtain(true).unwrap();
        assert!(bridge.get_privacy_settings().unwrap());
    }
}
