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
//! 해제로 일어나며 비동기다(실측 ~30s) — 카탈로그에서 잠깐 남아 있을 수
//! 있어 상태에 removal_pending으로 드러낸다.

use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 최근 제거 표시를 유지하는 창. 실측 제거 지연(~30s)보다 길게.
const REMOVAL_PENDING_WINDOW: Duration = Duration::from_secs(90);

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
    /// 제거 요청 뒤 시스템 반영(~30s 비동기)이 끝나지 않은 상태.
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
    state: Mutex<ManagerState>,
}

struct ManagerState {
    #[cfg(target_os = "macos")]
    lib: Option<libloading::Library>,
    /// 프로브 결과 캐시 — 심볼 부재/사유는 부팅 후 불변이라도 세션 전제
    /// (활성 디스플레이 ≥1)은 달라질 수 있어 재프로브를 허용한다.
    probed: Option<ProbeOutcome>,
    current: Option<LiveVirtualDisplay>,
    last_removed: Option<(String, Instant)>,
}

#[derive(Debug, Clone)]
struct ProbeOutcome {
    supported: bool,
    reason: Option<String>,
}

impl VirtualDisplayManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(ManagerState {
                #[cfg(target_os = "macos")]
                lib: None,
                probed: None,
                current: None,
                last_removed: None,
            }),
        }
    }

    /// (supported, reason, live) 스냅샷. 프로브는 캐시 없이 매번 도는 게
    /// 싸지만(dylib 열림 여부만 보면 활성 디스플레이 전제를 못 본다)
    /// 매번 프로브하면 UI 폴링마다 shim 호출이 늘어난다 — 성공 이력이 있으면
    /// 캐시하고, 실패는 재시도한다.
    pub fn status(&self) -> (bool, Option<String>, Option<LiveVirtualDisplayPublic>) {
        let mut state = self.state.lock().unwrap();
        self.refresh_probe(&mut state);
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

    /// 최근 제거한 source id — 창 안에 있으면 아직 시스템에 남아 있다.
    pub fn last_removed_pending(&self) -> Option<String> {
        let mut state = self.state.lock().unwrap();
        state.last_removed = state
            .last_removed
            .take_if(|(_, at)| at.elapsed() < REMOVAL_PENDING_WINDOW);
        state.last_removed.as_ref().map(|(id, _)| id.clone())
    }

    pub fn create(
        &self,
        logical_width: u32,
        logical_height: u32,
        scale: u32,
    ) -> Result<LiveVirtualDisplayPublic, String> {
        if scale != 1 && scale != 2 {
            return Err("scale must be 1 or 2".into());
        }
        if !(640..=4096).contains(&logical_width) || !(480..=4096).contains(&logical_height) {
            return Err("logical size out of range".into());
        }
        let mut state = self.state.lock().unwrap();
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

        let json = self.call_create(&state, logical_width, logical_height, scale)?;
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| format!("bad create json: {e}"))?;
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
            return Err("create returned display id 0".into());
        }
        let public = live.public();
        state.current = Some(live);
        Ok(public)
    }

    /// 제거. 반환값은 즉시 소실 여부(false면 시스템 반영이 수 초 뒤,
    /// 실측 ~30s). 브리지 폴링(최대 ~10s)은 결과 보고용일 뿐 — 객체 해제
    /// 자체는 즉시 일어난다.
    pub fn remove(&self) -> Result<bool, String> {
        let mut state = self.state.lock().unwrap();
        let Some(live) = state.current.take() else {
            return Ok(true);
        };
        let vanished = self.call_destroy(&state, live.display_id);
        if let Some(source_id) = &live.source_id {
            state.last_removed = Some((source_id.clone(), Instant::now()));
        }
        Ok(vanished)
    }

    fn refresh_probe(&self, state: &mut ManagerState) {
        let retry = state.probed.as_ref().map_or(true, |probe| !probe.supported);
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

    #[cfg(target_os = "macos")]
    fn call_destroy(&self, state: &ManagerState, display_id: u32) -> bool {
        let Some(lib) = state.lib.as_ref() else {
            return false;
        };
        unsafe {
            let Ok(destroy) =
                lib.get::<unsafe extern "C" fn(u32) -> i32>(b"leftcar_vdisp_destroy_v1")
            else {
                return false;
            };
            // 0 = 목록에서 사라짐, 2 = 폴링 창(~10s) 안에 못 사라짐(곧 사라짐).
            destroy(display_id) == 0
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

// ===== 단위 테스트: 순수 매칭 수학 =====

#[cfg(test)]
mod tests {
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
}
