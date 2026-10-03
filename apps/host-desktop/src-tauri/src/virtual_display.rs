//! macOS 확장(가상) 디스플레이 — 호스트 UI가 만들고 없애는 추가 캡처 소스.
//!
//! 설계: docs/2026-09-18-extended-display-design.md. 브리지 심볼
//! (`leftcar_vdisp_*`)은 캡처 shim dylib 안에 산다. 이 매니저는 같은
//! dylib을 별도로 dlopen해(dlopen 참조수 공유) 선택적 심볼만 호출한다 —
//! 심볼이 없거나 프로브가 실패하면 기능 전체가 "지원 안 됨"로 degrade되고
//! 기존 화면 공유 경로는 영향을 받지 않는다.
//!
//! 생성된 디스플레이는 실제 CGDirectDisplayID를 가진 활성 디스플레이라
//! 열거·승인·캡처·입력은 전부 기존 경로를 그대로 지난다. 제거는 객체
//! 해제로 일어나며 시스템 반영은 비동기다 — 카탈로그에서 잠깐 남아 있을 수
//! 있어 상태에 removal_pending으로 드러낸다.

use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

// ===== 순수 모드 매칭 (옛 display_matching.rs 수학 계승, f1a846e^) =====

/// 뷰어가 보고한 자기 화면 메트릭(물리 픽셀 + 밀도). 밀도는 유효성 검사에만
/// 쓴다 — macOS 논리 픽셀과 Android dp는 체계가 다르다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewerDisplayMetrics {
    pub physical_width: u32,
    pub physical_height: u32,
    pub density_dpi: u32,
}

/// 매칭된 가상 디스플레이 논리 크기와 HiDPI 배율.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchedDisplaySize {
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale: u8,
}

const MIN_LOGICAL_WIDTH: u32 = 1280;
const MIN_LOGICAL_HEIGHT: u32 = 720;
/// 논리 장변 상한 — @2x 프리셋 최대(1600×1000)와 정렬. 4K 태블릿이 M1
/// baseline을 넘는 workload를 만들지 않게 한다.
const MAX_LOGICAL_LONG_EDGE: u32 = 1600;

fn halved_px(physical: u64) -> Option<u32> {
    let half = physical / 2;
    let aligned = (half / 2) * 2;
    u32::try_from(aligned).ok()
}

/// 장변이 상한을 넘으면 종횡비를 유지한 채 줄인다(짝수 정렬 유지).
fn clamp_to_long_edge(width: u32, height: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= MAX_LOGICAL_LONG_EDGE {
        return (width, height);
    }
    let short = width.min(height);
    let scaled_short =
        ((u64::from(short) * u64::from(MAX_LOGICAL_LONG_EDGE)) / u64::from(long)) as u32;
    let scaled_short = (scaled_short / 2) * 2;
    if width >= height {
        (MAX_LOGICAL_LONG_EDGE, scaled_short)
    } else {
        (scaled_short, MAX_LOGICAL_LONG_EDGE)
    }
}

/// 뷰어 메트릭을 가상 디스플레이 논리 크기로 매칭한다.
///
/// 의도는 backing 픽셀을 뷰어 물리 픽셀과 1:1로 맞추는 것이다: HiDPI(2x)
/// 후보는 논리 = 물리 ÷ 2. 후보가 최소(1280×720)에 미달하면 scale 1로
/// 폴백, 그래도 미달이면 None. 세로 보고는 장변을 폭으로 정규화하고, 장변
/// 상한(1600)을 넘으면 종횡비를 유지하며 줄인다. 뷰어 값을 그대로 믿지
/// 않는 호스트 측 검증이기도 하다.
pub fn match_display_size(metrics: &ViewerDisplayMetrics) -> Option<MatchedDisplaySize> {
    if metrics.physical_width == 0 || metrics.physical_height == 0 || metrics.density_dpi == 0 {
        return None;
    }
    let (width, height) = if metrics.physical_width >= metrics.physical_height {
        (metrics.physical_width, metrics.physical_height)
    } else {
        (metrics.physical_height, metrics.physical_width)
    };
    let hidpi = MatchedDisplaySize {
        logical_width: halved_px(u64::from(width))?,
        logical_height: halved_px(u64::from(height))?,
        scale: 2,
    };
    if hidpi.logical_width >= MIN_LOGICAL_WIDTH && hidpi.logical_height >= MIN_LOGICAL_HEIGHT {
        let (width, height) = clamp_to_long_edge(hidpi.logical_width, hidpi.logical_height);
        return Some(MatchedDisplaySize {
            logical_width: width,
            logical_height: height,
            scale: 2,
        });
    }
    let fallback = MatchedDisplaySize {
        logical_width: width,
        logical_height: height,
        scale: 1,
    };
    if fallback.logical_width >= MIN_LOGICAL_WIDTH && fallback.logical_height >= MIN_LOGICAL_HEIGHT
    {
        let (width, height) = clamp_to_long_edge(fallback.logical_width, fallback.logical_height);
        return Some(MatchedDisplaySize {
            logical_width: width,
            logical_height: height,
            scale: 1,
        });
    }
    None
}

// ===== 공개 상태(직렬화) =====

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualDisplayStatusPublic {
    pub supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<LiveVirtualDisplayPublic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested: Option<SuggestedModePublic>,
    /// 제거 요청 뒤 시스템 반영이 끝나지 않은 상태.
    pub removal_pending: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveVirtualDisplayPublic {
    pub display_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    pub name: String,
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale: u32,
    pub backing_width: u32,
    pub backing_height: u32,
    pub mode_verified: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedModePublic {
    pub width: u32,
    pub height: u32,
    pub scale: u8,
    /// "viewerMetrics" — 최근 뷰어 보고 패널에서 도출, "fallback" — 프리셋 기본값.
    pub source: &'static str,
}

/// 도출 실패 시 UI가 보여 줄 폴백 프리셋(설계 §3 기본).
pub const FALLBACK_MODE: MatchedDisplaySize = MatchedDisplaySize {
    logical_width: 1280,
    logical_height: 800,
    scale: 2,
};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayPosition {
    Right,
    Left,
    Above,
    Below,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub scale: u32,
}

impl DisplayMode {
    pub fn validate(self) -> Result<Self, String> {
        if !matches!(self.scale, 1 | 2)
            || !(640..=4096).contains(&self.width)
            || !(480..=4096).contains(&self.height)
            || !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
        {
            return Err(
                "invalid display size: use even dimensions 640–4096 by 480–4096, scale 1 or 2"
                    .into(),
            );
        }
        Ok(self)
    }
}

// ===== 매니저 =====

#[derive(Debug, Clone)]
struct LiveVirtualDisplay {
    display_id: u32,
    source_id: Option<String>,
    name: String,
    logical_width: u32,
    logical_height: u32,
    scale: u32,
    mode_verified: bool,
}

impl LiveVirtualDisplay {
    fn public(&self) -> LiveVirtualDisplayPublic {
        LiveVirtualDisplayPublic {
            display_id: self.display_id,
            source_id: self.source_id.clone(),
            name: self.name.clone(),
            logical_width: self.logical_width,
            logical_height: self.logical_height,
            scale: self.scale,
            backing_width: self.logical_width * self.scale,
            backing_height: self.logical_height * self.scale,
            mode_verified: self.mode_verified,
        }
    }
}

pub struct VirtualDisplayManager {
    operation: Mutex<()>,
    state: Mutex<ManagerState>,
    /// remove()의 실제 진입 횟수 — 세션 수명 훅 회귀 검사가 "해제 요청 →
    /// 화면 해제" 전이를 관측하는 수단이다(프로덕션 빌드에는 없다).
    #[cfg(test)]
    remove_requests: AtomicUsize,
}

#[cfg(all(test, target_os = "macos"))]
type FindStaleDisplay = fn() -> Vec<u32>;
#[cfg(test)]
type DestroyDisplay = fn(u32) -> Result<bool, String>;

struct ManagerState {
    #[cfg(target_os = "macos")]
    lib: Option<libloading::Library>,
    /// 프로브 결과 캐시 — 심볼 부재/사유는 부팅 후 불변이라도 세션 전제
    /// (활성 디스플레이 ≥1)은 달라질 수 있어 재프로브를 허용한다.
    probed: Option<ProbeOutcome>,
    current: Option<LiveVirtualDisplay>,
    last_removed: Option<(u32, String)>,
    pending_resize: Option<(LiveVirtualDisplay, DisplayMode)>,
    /// 마지막 좀비 스캔 시각 — 상태 조회 폴링(2s)마다 find_stale 브리지
    /// 호출이 반복되지 않게 하는 백오프 근거(G-3).
    stale_scan_at: Option<std::time::Instant>,
    /// 회귀 검사용 find 스텁 — 테스트가 실제 dylib·브리지 없이 "좀비 존재
    /// → 탐지·복구" 전이를 재현하는 수단. 프로덕션 빌드에는 없다.
    #[cfg(all(test, target_os = "macos"))]
    test_find_stale: std::sync::Mutex<Option<FindStaleDisplay>>,
    /// 회귀 검사용 destroy 스텁 — control.rs의 해제 재시도 테스트가
    /// "실패 → 폴링 재시도 → 성공" 전이를 브리지 없이 재현하는 수단.
    /// 프로덕션 빌드에는 존재하지 않는다.
    #[cfg(test)]
    test_destroy: std::sync::Mutex<Option<DestroyDisplay>>,
}

#[derive(Debug, Clone)]
struct ProbeOutcome {
    supported: bool,
    reason: Option<String>,
}

impl VirtualDisplayManager {
    pub fn public_status(
        &self,
        metrics: Option<ViewerDisplayMetrics>,
    ) -> VirtualDisplayStatusPublic {
        let (supported, reason, live) = self.status();
        let matched = metrics.as_ref().and_then(match_display_size);
        let pending = self
            .state
            .lock()
            .unwrap()
            .pending_resize
            .as_ref()
            .map(|(_, mode)| *mode);
        let size = pending
            .map(|m| MatchedDisplaySize {
                logical_width: m.width,
                logical_height: m.height,
                scale: m.scale as u8,
            })
            .unwrap_or_else(|| matched.unwrap_or(FALLBACK_MODE));
        VirtualDisplayStatusPublic {
            supported,
            reason,
            live,
            suggested: Some(SuggestedModePublic {
                width: size.logical_width,
                height: size.logical_height,
                scale: size.scale,
                source: if pending.is_some() {
                    "pendingResize"
                } else if matched.is_some() {
                    "viewerMetrics"
                } else {
                    "fallback"
                },
            }),
            removal_pending: self.last_removed_pending().is_some(),
        }
    }
    pub fn new() -> Self {
        Self {
            operation: Mutex::new(()),
            state: Mutex::new(ManagerState {
                #[cfg(target_os = "macos")]
                lib: None,
                probed: None,
                current: None,
                last_removed: None,
                pending_resize: None,
                stale_scan_at: None,
                #[cfg(all(test, target_os = "macos"))]
                test_find_stale: std::sync::Mutex::new(None),
                #[cfg(test)]
                test_destroy: std::sync::Mutex::new(None),
            }),
            #[cfg(test)]
            remove_requests: AtomicUsize::new(0),
        }
    }

    /// (supported, reason, live) 스냅샷. 프로브는 캐시 없이 매번 도는 게
    /// 싸지만(dylib 열림 여부만 보면 활성 디스플레이 전제를 못 본다)
    /// 매번 프로브하면 UI 폴링마다 shim 호출이 늘어난다 — 성공 이력이 있으면
    /// 캐시하고, 실패는 재시도한다.
    pub fn status(&self) -> (bool, Option<String>, Option<LiveVirtualDisplayPublic>) {
        let mut state = self.state.lock().unwrap();
        self.refresh_probe(&mut state);
        Self::refresh_removal(&mut state);
        let probe = state.probed.clone().unwrap_or(ProbeOutcome {
            supported: false,
            reason: Some("probe unavailable".into()),
        });
        (
            probe.supported,
            probe.reason,
            state.current.as_ref().map(LiveVirtualDisplay::public),
        )
    }

    /// Actual OS lifetime, not a timer. A retry must never reuse an identity
    /// while WindowServer still owns its previous display.
    pub fn last_removed_pending(&self) -> Option<String> {
        let mut state = self.state.lock().unwrap();
        Self::refresh_removal(&mut state);
        state.last_removed.as_ref().map(|(_, id)| id.clone())
    }

    fn refresh_removal(state: &mut ManagerState) {
        #[cfg(target_os = "macos")]
        if let (Some(lib), Some((display_id, _))) = (&state.lib, &state.last_removed) {
            unsafe {
                if let Ok(active) =
                    lib.get::<unsafe extern "C" fn(u32) -> i32>(b"leftcar_vdisp_is_active_v1")
                {
                    if active(*display_id) == 0 {
                        state.last_removed = None;
                    }
                }
            }
        }
    }

    pub fn create(
        &self,
        logical_width: u32,
        logical_height: u32,
        scale: u32,
    ) -> Result<LiveVirtualDisplayPublic, String> {
        let _operation = self.operation.lock().unwrap();
        self.create_inner(logical_width, logical_height, scale)
    }

    fn create_inner(
        &self,
        logical_width: u32,
        logical_height: u32,
        scale: u32,
    ) -> Result<LiveVirtualDisplayPublic, String> {
        DisplayMode {
            width: logical_width,
            height: logical_height,
            scale,
        }
        .validate()?;
        let mut state = self.state.lock().unwrap();
        Self::refresh_removal(&mut state);
        if state.last_removed.is_some() {
            return Err("virtual display removal pending".into());
        }
        if state.current.is_some() {
            return Err("a Leftcar display already exists".into());
        }
        self.refresh_probe(&mut state);
        let probe = state.probed.clone().unwrap_or(ProbeOutcome {
            supported: false,
            reason: Some("probe unavailable".into()),
        });
        if !probe.supported {
            return Err(format!(
                "virtual display unavailable: {}",
                probe.reason.as_deref().unwrap_or("unknown")
            ));
        }

        // Recover objects left by a previous bridge/process before creating. The
        // bridge registry excludes objects owned by this process.
        self.recover_stale(&mut state);

        let json = self.call_create(&state, logical_width, logical_height, scale)?;
        let value: serde_json::Value = match serde_json::from_str(&json) {
            Ok(value) => value,
            Err(error) => {
                self.recover_stale(&mut state);
                return Err(format!("bad create json: {error}"));
            }
        };
        if value["ok"].as_bool() != Some(true) {
            let stage = value["stage"].as_str().unwrap_or("unknown");
            let error = value["error"].as_str().unwrap_or("unknown");
            return Err(format!("create failed at {stage}: {error}"));
        }
        let live = LiveVirtualDisplay {
            display_id: value["displayId"].as_u64().unwrap_or_default() as u32,
            source_id: value["sourceId"].as_str().map(str::to_owned),
            name: value["name"]
                .as_str()
                .unwrap_or("Leftcar Display")
                .to_owned(),
            logical_width: value["logicalWidth"].as_u64().unwrap_or_default() as u32,
            logical_height: value["logicalHeight"].as_u64().unwrap_or_default() as u32,
            scale: value["scale"].as_u64().unwrap_or(scale as u64) as u32,
            mode_verified: value["modeVerified"].as_bool().unwrap_or(false),
        };
        if live.display_id == 0 {
            self.recover_stale(&mut state);
            return Err("create returned display id 0".into());
        }
        let public = live.public();
        state.current = Some(live);
        state.pending_resize = None;
        Ok(public)
    }

    /// remove()의 실제 진입 횟수(회귀 검사 전용 관측).
    #[cfg(test)]
    pub fn remove_request_count(&self) -> usize {
        self.remove_requests.load(Ordering::SeqCst)
    }

    /// 세션 수명 훅 회귀 검사용 라이브 디스플레이 설치 — control.rs 테스트가
    /// 브리지 없는 환경에서 remove 실패·재시도 경로를 재현하는 수단이다.
    #[cfg(test)]
    pub(crate) fn install_test_display(&self, display_id: u32) {
        self.state.lock().unwrap().current = Some(LiveVirtualDisplay {
            display_id,
            source_id: Some(format!("display:test:{display_id}")),
            name: "Leftcar Display".into(),
            logical_width: 1280,
            logical_height: 800,
            scale: 2,
            mode_verified: true,
        });
    }

    /// 회귀 검사용 destroy 스텁 교체. Some이면 실제 dylib 대신 이 함수로
    /// destroy 결과를 만든다(실패→성공 전이 재현용).
    #[cfg(test)]
    pub(crate) fn set_test_destroy(&self, destroy: Option<DestroyDisplay>) {
        *self.state.lock().unwrap().test_destroy.lock().unwrap() = destroy;
    }

    /// 회귀 검사용 find_stale 스텁 교체. Some이면 실제 dylib 스캔 대신 이
    /// 함수가 좀비 display id 목록을 반환한다.
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn set_test_find_stale(&self, find: Option<FindStaleDisplay>) {
        *self.state.lock().unwrap().test_find_stale.lock().unwrap() = find;
    }

    /// 좀비(이전 프로세스 유산) 가상 디스플레이의 탐지·복구 진입점 —
    /// create 진입 외에 호스트 시작(setup)과 상태 조회 경로에서도 호출한다
    /// (G-3, artifacts/stale-connected-state-2026-09-27/report.md §2).
    ///
    /// 비용 통제: (1) 최소 간격 백오프로 반복 폴링이 브리지 스캔을 폴링
    /// 주파수로 반복하지 않게 하고, (2) 이 프로세스 소유 디스플레이가 살아
    /// 있으면 건너뛴다(브리지 레지스트리는 자기 프로세스 객체를 제외하므로
    /// 이때 좀비는 생길 수 없다), (3) 다른 create/remove가 진행 중이면
    /// 기다리지 않고 다음 폴링으로 미룬다. 완전히 사라지지 못한 좀비는
    /// last_removed로 승격되어 removalPending으로 보고된다.
    pub fn recover_stale_if_due(&self) {
        let Ok(_operation) = self.operation.try_lock() else {
            return;
        };
        let mut state = self.state.lock().unwrap();
        if state.current.is_some() {
            return;
        }
        if let Some(last) = state.stale_scan_at {
            if last.elapsed() < STALE_SCAN_MIN_INTERVAL {
                return;
            }
        }
        state.stale_scan_at = Some(std::time::Instant::now());
        // find 스텁이 설치된 회귀 검사는 실제 dylib 프로브 없이 좀비 경로만
        // 재현한다 — state.lib이 None으로 남아 refresh_removal 같은 실제
        // 브리지 호출도 차단된다.
        #[cfg(all(test, target_os = "macos"))]
        if state.test_find_stale.lock().unwrap().is_some() {
            self.recover_stale(&mut state);
            return;
        }
        self.refresh_probe(&mut state);
        self.recover_stale(&mut state);
    }

    /// 프로브·브리지 호출 없이 라이브 디스플레이 존재만 싸게 본다 — 세션
    /// 수명 훅의 감사 로그 게이트용. 읽는 순간의 스냅샷이라 훅의 remove와
    /// 생성 경로가 끼어드는 창은 남는다(감사 §9.6-3, v1 수용).
    pub fn has_live_display(&self) -> bool {
        self.state.lock().unwrap().current.is_some()
    }

    /// 종료 훅용 동기 해제 — remove를 별도 스레드에서 실행하고 상한만큼만
    /// 기다린다. 브리지 destroy 폴링(최대 ~9.5s)이 프로세스 종료를 붙잡지
    /// 않게 한다. None은 상한 내 완료 없음: 해제 호출은 이미 보장됐고 잔여
    /// 정리는 프로세스 사망 시 WindowServer 회수(design.md E-1)가 마무리한다.
    pub fn remove_blocking_within(
        self: &std::sync::Arc<Self>,
        timeout: std::time::Duration,
    ) -> Option<Result<bool, String>> {
        let manager = std::sync::Arc::clone(self);
        let (done, received) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(manager.remove());
        });
        received.recv_timeout(timeout).ok()
    }

    /// false means the object was released but WindowServer still lists it.
    pub fn remove(&self) -> Result<bool, String> {
        let _operation = self.operation.lock().unwrap();
        #[cfg(test)]
        self.remove_requests.fetch_add(1, Ordering::SeqCst);
        self.state.lock().unwrap().pending_resize = None;
        self.remove_inner()
    }

    fn remove_inner(&self) -> Result<bool, String> {
        let mut state = self.state.lock().unwrap();
        Self::refresh_removal(&mut state);
        let Some(live) = state.current.clone() else {
            return Ok(state.last_removed.is_none());
        };
        let vanished = self.call_destroy(&state, live.display_id)?;
        state.current = None;
        if !vanished {
            if let Some(source_id) = &live.source_id {
                state.last_removed = Some((live.display_id, source_id.clone()));
            }
        }
        Ok(vanished)
    }

    pub fn resize(&self, mode: DisplayMode) -> Result<LiveVirtualDisplayPublic, String> {
        let mode = mode.validate()?;
        let _operation = self.operation.lock().unwrap();
        let old = {
            let state = self.state.lock().unwrap();
            state
                .current
                .clone()
                .or_else(|| state.pending_resize.as_ref().map(|(old, _)| old.clone()))
                .ok_or("no extended display")?
        };
        if self.state.lock().unwrap().current.is_some()
            && (old.logical_width, old.logical_height, old.scale)
                == (mode.width, mode.height, mode.scale)
        {
            return Ok(old.public());
        }
        self.remove_inner()?;
        // Keep the requested size available if WindowServer outlives this request.
        self.state.lock().unwrap().pending_resize = Some((old.clone(), mode));
        if !wait_for_removal(
            || self.last_removed_pending().is_some(),
            || std::thread::sleep(std::time::Duration::from_millis(100)),
            REMOVAL_WAIT_ATTEMPTS,
        ) {
            return Err("virtual display removal pending; requested size saved, retry when removal completes".into());
        }
        match self.create_inner(mode.width, mode.height, mode.scale) {
            Ok(live) => Ok(live),
            Err(error) => {
                let restored = self
                    .create_inner(old.logical_width, old.logical_height, old.scale)
                    .is_ok();
                Err(format!(
                    "resize failed: {error}; previous size restored: {restored}"
                ))
            }
        }
    }

    pub fn arrange(&self, position: DisplayPosition) -> Result<(), String> {
        let _operation = self.operation.lock().unwrap();
        let state = self.state.lock().unwrap();
        let live = state.current.as_ref().ok_or("no extended display")?;
        #[cfg(target_os = "macos")]
        unsafe {
            let lib = state.lib.as_ref().ok_or("virtual display unavailable")?;
            let arrange = lib
                .get::<unsafe extern "C" fn(u32, u32) -> i32>(b"leftcar_vdisp_arrange_v1")
                .map_err(|_| "update the Host capture library to arrange displays")?;
            let result = arrange(live.display_id, position as u32);
            if result != 0 {
                return Err(format!("display arrangement failed: {result}"));
            }
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (live, position);
            Err("unsupported platform".into())
        }
    }

    fn refresh_probe(&self, state: &mut ManagerState) {
        let retry = state.probed.as_ref().is_none_or(|probe| !probe.supported);
        if !retry {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            match self.probe_once(state) {
                Ok(outcome) => state.probed = Some(outcome),
                Err(error) => {
                    state.probed = Some(ProbeOutcome {
                        supported: false,
                        reason: Some(error),
                    })
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            state.probed = Some(ProbeOutcome {
                supported: false,
                reason: Some("unsupported platform".into()),
            });
        }
    }

    #[cfg(target_os = "macos")]
    fn probe_once(&self, state: &mut ManagerState) -> Result<ProbeOutcome, String> {
        let lib = Self::ensure_lib(state)?;
        let json = {
            use libloading::Symbol;
            unsafe {
                let probe: Symbol<unsafe extern "C" fn() -> *mut std::ffi::c_char> = lib
                    .get(b"leftcar_vdisp_probe_v1")
                    .map_err(|e| format!("probe symbol unavailable: {e}"))?;
                take_json_with(lib, || probe())
            }
        };
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| format!("bad probe json: {e}"))?;
        Ok(ProbeOutcome {
            supported: value["supported"].as_bool().unwrap_or(false),
            reason: value["reason"]
                .as_str()
                .filter(|r| !r.is_empty())
                .map(str::to_owned),
        })
    }

    #[cfg(target_os = "macos")]
    fn call_create(
        &self,
        state: &ManagerState,
        logical_width: u32,
        logical_height: u32,
        scale: u32,
    ) -> Result<String, String> {
        use libloading::Symbol;
        use std::ffi::CString;
        let lib = state
            .lib
            .as_ref()
            .ok_or_else(|| "library not loaded".to_string())?;
        let name = CString::new("Leftcar Display").map_err(|e| e.to_string())?;
        unsafe {
            let create: Symbol<
                unsafe extern "C" fn(
                    u32,
                    u32,
                    u32,
                    *const std::ffi::c_char,
                ) -> *mut std::ffi::c_char,
            > = lib
                .get(b"leftcar_vdisp_create_v1")
                .map_err(|e| format!("create symbol unavailable: {e}"))?;
            Ok(take_json_with(lib, || {
                create(logical_width, logical_height, scale, name.as_ptr())
            }))
        }
    }

    fn recover_stale(&self, state: &mut ManagerState) {
        #[cfg(all(test, target_os = "macos"))]
        {
            // fn 포인터는 Copy라 가드를 먼저 떨어뜨리고 state를 다시 빌릴 수 있다.
            let find = *state.test_find_stale.lock().unwrap();
            if let Some(find) = find {
                self.recover_stale_ids(state, find());
                return;
            }
        }
        #[cfg(target_os = "macos")]
        {
            let Some(lib) = state.lib.as_ref() else {
                return;
            };
            let result = unsafe {
                let Ok(find) = lib.get::<unsafe extern "C" fn() -> *mut std::ffi::c_char>(
                    b"leftcar_vdisp_find_stale_v1",
                ) else {
                    return;
                };
                take_json_with(lib, || find())
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&result) else {
                eprintln!("leftcar: virtual display stale scan returned invalid JSON");
                return;
            };
            let ids: Vec<u32> = value["displays"]
                .as_array()
                .map(|displays| {
                    displays
                        .iter()
                        .filter_map(|display| {
                            display["displayId"]
                                .as_u64()
                                .and_then(|id| u32::try_from(id).ok())
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.recover_stale_ids(state, ids);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = state;
    }

    /// 발견된 좀비를 하나씩 해제한다. 즉시 소실 확인(destroy → 0)되면 상태
    /// 기록 없이 끝나고, WindowServer에 아직 남으면(2, 오류) last_removed로
    /// 승격해 removalPending으로 보고하고 백오프 만료 뒤 재시도한다.
    #[cfg(target_os = "macos")]
    fn recover_stale_ids(&self, state: &mut ManagerState, ids: Vec<u32>) {
        for id in ids {
            match self.call_destroy(state, id) {
                Ok(true) => eprintln!(
                    "leftcar: virtual_display_zombie_recovered display_id={id} vanished=true"
                ),
                Ok(false) => {
                    state.last_removed = Some((id, format!("stale:{id}")));
                    eprintln!("leftcar: virtual_display_zombie_recovery_pending display_id={id}");
                }
                Err(error) => {
                    state.last_removed = Some((id, format!("stale:{id}")));
                    eprintln!(
                        "leftcar: virtual_display_zombie_recovery_failed display_id={id}: {error}"
                    );
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn call_destroy(&self, state: &ManagerState, display_id: u32) -> Result<bool, String> {
        #[cfg(test)]
        if let Some(destroy) = *state.test_destroy.lock().unwrap() {
            return destroy(display_id);
        }
        let Some(lib) = state.lib.as_ref() else {
            return Err("virtual display library unavailable".into());
        };
        unsafe {
            let Ok(destroy) =
                lib.get::<unsafe extern "C" fn(u32) -> i32>(b"leftcar_vdisp_destroy_v1")
            else {
                return Err("virtual display destroy symbol unavailable".into());
            };
            match destroy(display_id) {
                0 => Ok(true),
                2 => Ok(false),
                code => Err(format!("virtual display removal failed: {code}")),
            }
        }
    }

    /// 캡처 shim dylib을 후보 경로에서 연다(FfiBackend와 같은 경로 규칙).
    /// dlopen 참조수만 공유하므로 이중 로드는 안전하다.
    #[cfg(target_os = "macos")]
    fn ensure_lib(state: &mut ManagerState) -> Result<&libloading::Library, String> {
        if state.lib.is_none() {
            let mut last_err = "no dylib candidates".to_string();
            for path in crate::ffi::dylib_candidates() {
                if !path.exists() {
                    last_err = format!("dylib not found at {}", path.display());
                    continue;
                }
                match unsafe { libloading::Library::new(&path) } {
                    Ok(lib) => {
                        state.lib = Some(lib);
                        break;
                    }
                    Err(e) => last_err = format!("dlopen {}: {e}", path.display()),
                }
            }
            if state.lib.is_none() {
                return Err(last_err);
            }
        }
        Ok(state
            .lib
            .as_ref()
            .expect("ensure_lib just populated the library"))
    }
}

impl Default for VirtualDisplayManager {
    fn default() -> Self {
        Self::new()
    }
}

/// shim 문자열 규약: NULL 아니면 UTF-8 JSON, 호출자가
/// leftcar_capture_free_string으로 해제한다.
#[cfg(target_os = "macos")]
fn take_json_with(
    lib: &libloading::Library,
    call: impl FnOnce() -> *mut std::ffi::c_char,
) -> String {
    use std::ffi::CStr;
    let ptr = call();
    if ptr.is_null() {
        return String::new();
    }
    let owned = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    if let Ok(free) = unsafe {
        lib.get::<unsafe extern "C" fn(*mut std::ffi::c_char)>(b"leftcar_capture_free_string")
    } {
        unsafe { free(ptr) };
    }
    owned
}

/// resize가 이전 디스플레이의 실제 소실(활성 열거에서 사라짐)을 기다리는
/// 폴링 상한(시도 횟수 × 100ms). 실측 제거 반영은 최대 ~30s(design.md
/// E-5)라 35s로 둔다 — 이전 20s(200회)는 실측 지연보다 짧아 resize가
/// "removal pending" 오류로 자주 끝났다.
const REMOVAL_WAIT_ATTEMPTS: usize = 350;

/// 좀비 스캔(recover_stale_if_due)의 최소 재실행 간격. 상태 조회 폴링(2s)이
/// find_stale 브리지 호출을 폴링 주파수로 반복하지 않게 한다. 좀비는 이전
/// 프로세스 사망 시점에만 새로 생기므로 30s면 즉각성 충분하다.
const STALE_SCAN_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

fn wait_for_removal(
    mut pending: impl FnMut() -> bool,
    mut pause: impl FnMut(),
    attempts: usize,
) -> bool {
    for _ in 0..attempts {
        if !pending() {
            return true;
        }
        pause();
    }
    !pending()
}

// ===== 단위 테스트 =====

#[cfg(test)]
mod tests {
    #[test]
    fn resize_waits_through_delayed_removal() {
        let mut polls = 0;
        assert!(super::wait_for_removal(
            || {
                polls += 1;
                polls < 4
            },
            || {},
            5
        ));
        assert_eq!(polls, 4);
        assert!(!super::wait_for_removal(|| true, || {}, 5));
    }
    #[test]
    fn failed_removal_keeps_the_current_display() {
        let manager = super::VirtualDisplayManager::new();
        manager.state.lock().unwrap().current = Some(super::LiveVirtualDisplay {
            display_id: 123,
            source_id: Some("display:kept".into()),
            name: "Leftcar Display".into(),
            logical_width: 1280,
            logical_height: 800,
            scale: 2,
            mode_verified: true,
        });
        assert!(manager.remove().is_err());
        assert_eq!(
            manager
                .state
                .lock()
                .unwrap()
                .current
                .as_ref()
                .unwrap()
                .display_id,
            123
        );
    }

    #[test]
    fn invalid_resize_preserves_current_display() {
        let manager = super::VirtualDisplayManager::new();
        manager.state.lock().unwrap().current = Some(super::LiveVirtualDisplay {
            display_id: 123,
            source_id: Some("display:kept".into()),
            name: "Leftcar Display".into(),
            logical_width: 1280,
            logical_height: 800,
            scale: 2,
            mode_verified: true,
        });
        assert!(manager
            .resize(super::DisplayMode {
                width: 1,
                height: 800,
                scale: 2
            })
            .is_err());
        assert_eq!(
            manager
                .state
                .lock()
                .unwrap()
                .current
                .as_ref()
                .unwrap()
                .display_id,
            123
        );
        assert!(manager.state.lock().unwrap().last_removed.is_none());
    }

    #[test]
    fn unchanged_resize_reuses_display_and_identity() {
        let manager = super::VirtualDisplayManager::new();
        manager.state.lock().unwrap().current = Some(super::LiveVirtualDisplay {
            display_id: 123,
            source_id: Some("display:kept".into()),
            name: "Leftcar Display".into(),
            logical_width: 1280,
            logical_height: 800,
            scale: 2,
            mode_verified: true,
        });
        assert_eq!(
            manager
                .resize(super::DisplayMode {
                    width: 1280,
                    height: 800,
                    scale: 2
                })
                .unwrap()
                .display_id,
            123
        );
    }
    #[test]
    fn creation_waits_for_previous_display_to_disappear() {
        let manager = super::VirtualDisplayManager::new();
        manager.state.lock().unwrap().last_removed = Some((123, "old-display".into()));
        let error = manager.create(1280, 800, 2).unwrap_err();
        assert!(
            error.contains("removal pending"),
            "unexpected result: {error}"
        );
    }
    use super::*;

    fn metrics(w: u32, h: u32, dpi: u32) -> ViewerDisplayMetrics {
        ViewerDisplayMetrics {
            physical_width: w,
            physical_height: h,
            density_dpi: dpi,
        }
    }

    #[test]
    fn hidpi_tablet_matches_half_logical() {
        // TB710FU급 2560×1600 패널 → 1280×800 @2x.
        let matched = match_display_size(&metrics(2560, 1600, 278)).unwrap();
        assert_eq!(
            matched,
            MatchedDisplaySize {
                logical_width: 1280,
                logical_height: 800,
                scale: 2
            }
        );
    }

    #[test]
    fn odd_pixels_align_even() {
        let matched = match_display_size(&metrics(2815, 1751, 240)).unwrap();
        assert_eq!(matched.logical_width % 2, 0);
        assert_eq!(matched.logical_height % 2, 0);
        assert_eq!(matched.logical_width, 1406);
        assert_eq!(matched.logical_height, 874);
        assert_eq!(matched.scale, 2);
    }

    #[test]
    fn portrait_reports_landscape_normalized() {
        assert_eq!(
            match_display_size(&metrics(1600, 2560, 278)),
            match_display_size(&metrics(2560, 1600, 278))
        );
    }

    #[test]
    fn sixteen_by_nine_keeps_aspect() {
        // 16:9 태블릿(예: 2560×1440)은 16:10과 다른 결과를 내야 한다.
        let matched = match_display_size(&metrics(2560, 1440, 300)).unwrap();
        assert_eq!(matched.logical_width, 1280);
        assert_eq!(matched.logical_height, 720);
        assert_eq!(matched.scale, 2);
    }

    #[test]
    fn oversize_panel_clamps_to_long_edge() {
        // 4K급 3840×2400 → 논리 1920×1200이 아니라 1600×1000으로 클램프.
        let matched = match_display_size(&metrics(3840, 2400, 280)).unwrap();
        assert_eq!(matched.logical_width, 1600);
        assert_eq!(matched.logical_height, 1000);
        assert_eq!(matched.scale, 2);
    }

    #[test]
    fn low_res_tablet_falls_back_to_scale_one() {
        // 1600×900 물리 → @2x 후보 800×450 미달 → scale 1.
        let matched = match_display_size(&metrics(1600, 900, 200)).unwrap();
        assert_eq!(matched.scale, 1);
        assert_eq!(matched.logical_width, 1600);
        assert_eq!(matched.logical_height, 900);
    }

    #[test]
    fn invalid_metrics_return_none() {
        assert!(match_display_size(&metrics(0, 1600, 278)).is_none());
        assert!(match_display_size(&metrics(2560, 1600, 0)).is_none());
        // scale-1 폴백도 최소 이하면 매칭 없음.
        assert!(match_display_size(&metrics(1024, 600, 160)).is_none());
    }

    #[test]
    fn fallback_mode_is_the_design_default() {
        assert_eq!(FALLBACK_MODE.logical_width, 1280);
        assert_eq!(FALLBACK_MODE.logical_height, 800);
        assert_eq!(FALLBACK_MODE.scale, 2);
    }

    // -- 세션 스코프 소유 정책(2026-09-22 종료 감사 §9) 회귀 검사 ----------

    #[test]
    fn removal_wait_budget_covers_measured_window() {
        // 실측 제거 반영 ~30s(E-5)를 resize 폴링 상한(100ms × 시도 횟수)이
        // 덮는지 자기검사한다 — 상한이 실측보다 짧아지면 resize가 pending
        // 오류로 되돌아가는 회귀다.
        assert!(REMOVAL_WAIT_ATTEMPTS as u64 * 100 >= 30_000);
    }

    #[test]
    fn exit_release_runs_remove_within_the_deadline() {
        let manager = std::sync::Arc::new(VirtualDisplayManager::new());
        let outcome = manager.remove_blocking_within(std::time::Duration::from_secs(2));
        // 디스플레이가 없으면 no-op 성공 — 종료 경로가 여전히 remove를
        // 호출했음을 카운터로 확인한다.
        assert_eq!(outcome, Some(Ok(true)));
        assert_eq!(manager.remove_request_count(), 1);
    }

    #[test]
    fn exit_release_caps_the_wait_and_finishes_in_background() {
        let manager = std::sync::Arc::new(VirtualDisplayManager::new());
        // remove의 operation 락을 붙잡아 진입을 막는다 — 종료 경로가 브리지
        // 폴링만큼 끌려가지 않고 상한에서 잘려야 한다.
        let operation = manager.operation.lock().unwrap();
        let started = std::time::Instant::now();
        let outcome = manager.remove_blocking_within(std::time::Duration::from_millis(200));
        assert!(
            outcome.is_none(),
            "the capped release must report a timeout"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(manager.remove_request_count(), 0);
        drop(operation);
        // 락이 풀리면 백그라운드 해제가 마무리된다(사망 회수 백스톱 전에).
        for _ in 0..500 {
            if manager.remove_request_count() == 1 {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(manager.remove_request_count(), 1);
    }

    // -- G-3: 시작/상태 조회 경로의 좀비 복구
    //    (artifacts/stale-connected-state-2026-09-27/report.md §2) -------

    /// 스텁별 독립 카운터 — 테스트 병렬 실행에서도 정확한 호출 수를 단언한다.
    static VANISH_CALLS: AtomicUsize = AtomicUsize::new(0);
    static LINGER_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn destroy_vanishes(_display_id: u32) -> Result<bool, String> {
        VANISH_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }

    fn destroy_lingers(_display_id: u32) -> Result<bool, String> {
        LINGER_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(false)
    }

    #[test]
    fn startup_and_status_paths_detect_and_recover_zombies() {
        let manager = VirtualDisplayManager::new();
        manager.set_test_find_stale(Some(|| vec![777]));
        manager.set_test_destroy(Some(destroy_vanishes));
        // 호스트 시작(setup)과 상태 조회가 쓰는 진입점: 좀비를 탐지하고
        // 즉시 해제한다.
        manager.recover_stale_if_due();
        assert_eq!(VANISH_CALLS.load(Ordering::SeqCst), 1);
        assert!(manager.state.lock().unwrap().last_removed.is_none());
        // 백오프: 최소 간격 안의 반복 상태 조회는 재스캔하지 않는다.
        manager.recover_stale_if_due();
        assert_eq!(VANISH_CALLS.load(Ordering::SeqCst), 1);
        // 백오프가 지나면 다시 스캔한다(복구 실패 좀비의 재시도 경로).
        manager.state.lock().unwrap().stale_scan_at = None;
        manager.recover_stale_if_due();
        assert_eq!(VANISH_CALLS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn zombie_that_has_not_vanished_surfaces_as_removal_pending() {
        let manager = VirtualDisplayManager::new();
        manager.set_test_find_stale(Some(|| vec![555]));
        manager.set_test_destroy(Some(destroy_lingers));
        manager.recover_stale_if_due();
        assert_eq!(LINGER_CALLS.load(Ordering::SeqCst), 1);
        // WindowServer에 아직 남은 좀비는 last_removed로 승격되고,
        // public_status는 이를 removalPending으로 보고한다 — 확장 디스플레이
        // 카드가 '만들기' 상태로 잘못 보이지 않게 하는 매핑의 원천.
        assert_eq!(manager.last_removed_pending().as_deref(), Some("stale:555"));
        // 좀비가 남아 있는 동안 create는 'removal pending'으로 거절된다.
        let error = manager.create(1280, 800, 2).unwrap_err();
        assert!(error.contains("removal pending"), "unexpected: {error}");
    }
}
