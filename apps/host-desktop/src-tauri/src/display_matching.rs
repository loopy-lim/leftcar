//! 뷰어(태블릿)가 보고한 화면 메트릭을 macOS 가상 화면 논리 크기로 바꾸는 순수 함수 모음.
//!
//! 매칭 규칙 (docs/plans/2026-09-06-viewer-display-sizing-design.md §1):
//! - 의도는 backing 픽셀을 태블릿 물리 픽셀과 1:1로 맞추는 것이다. HiDPI(scale 2)일 때
//!   논리 = 물리 ÷ 2 (예: 2800×1752 → 논리 1400×876).
//! - HiDPI 후보가 유효 최소(폭·높이 각각 1280×720)에 미달하면 scale 1로 폴백한다
//!   (물리 = 논리, 저해상도 태블릿). 폴백 후보도 미달이면 None.
//! - 결과는 짝수 픽셀로 내림 정렬한다(`(v / 2) * 2`).
//! - 세로로 보고된 메트릭도 같은 픽셀 조합이면 동일한 결과를 내도록 긴 변을 폭으로
//!   정규화한다(orientation 구분 없이 물리 px 기준으로 산출).

/// 뷰어가 보고한 태블릿 화면 메트릭 (물리 픽셀 + 밀도).
///
/// `density_dpi`는 계약 호환을 위해 유지되지만 매칭 산출에는 쓰지 않는다:
/// macOS 가상 화면의 밀도 독립 픽셀은 Android dp와 다른 체계이므로, 승인된 설계는
/// backing 픽셀 1:1(= 논리 ÷ 2)을 기준으로 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewerDisplayMetrics {
    pub physical_width: u32,
    pub physical_height: u32,
    pub density_dpi: u32,
}

/// 가상 화면 HiDPI 배율.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchedScale {
    One,
    Two,
}

impl std::fmt::Display for MatchedScale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchedScale::One => write!(f, "1"),
            MatchedScale::Two => write!(f, "2"),
        }
    }
}

impl From<MatchedScale> for u8 {
    fn from(scale: MatchedScale) -> u8 {
        match scale {
            MatchedScale::One => 1,
            MatchedScale::Two => 2,
        }
    }
}

/// 자동 매칭된 가상 화면 논리 크기와 배율.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchedDisplaySize {
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale: MatchedScale,
}

const MIN_LOGICAL_WIDTH: u32 = 1280;
const MIN_LOGICAL_HEIGHT: u32 = 720;

/// 물리 픽셀을 절반으로 나눈 논리 픽셀을 산출한다. 나눈 값이 홀수면 짝수로
/// 내림 정렬한다 — 논리 크기의 짝수 정렬은 가상 화면 모드 계약의 요구다.
fn halved_px(physical: u64) -> Option<u32> {
    let half = physical / 2;
    let aligned = (half / 2) * 2;
    u32::try_from(aligned).ok()
}

/// 뷰어 화면 메트릭을 가상 화면 논리 크기로 자동 매칭한다.
///
/// HiDPI(scale 2) 논리 후보(물리 ÷ 2)가 유효 범위(폭·높이 각각 1280×720 이상)에
/// 들면 scale 2, 아니면 scale 1(논리 = 물리)로 폴백한다. 폴백한 논리 후보도
/// 범위를 벗어나면 None. 세로로 보고된 메트릭은 긴 변을 폭으로 정규화해 같은
/// 픽셀 조합이면 동일한 결과를 낸다. 입력이 무효하면(물리 px 0, dpi 0) None.
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
        scale: MatchedScale::Two,
    };
    if hidpi.logical_width >= MIN_LOGICAL_WIDTH && hidpi.logical_height >= MIN_LOGICAL_HEIGHT {
        return Some(hidpi);
    }
    let fallback = MatchedDisplaySize {
        logical_width: width,
        logical_height: height,
        scale: MatchedScale::One,
    };
    if fallback.logical_width >= MIN_LOGICAL_WIDTH && fallback.logical_height >= MIN_LOGICAL_HEIGHT
    {
        Some(fallback)
    } else {
        None
    }
}

/// 매칭을 적용할 수 있는 관리 화면 한 개의 크기 스냅샷.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedDisplayMode<'a> {
    pub id: &'a str,
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale: u8,
}

/// 스트림 시작 전 가상 화면 준비 결정.
///
/// 이 결정은 절대 화면 생성을 요구하지 않는다(설계 §4 소유권 안전): 생성은 호스트
/// UI의 명시적 사용자 동작으로 남고, 제어 채널의 자동 매칭은 기존 관리 화면의
/// 재사용·리사이즈와 로그까지만 담당한다. 리사이즈 실패가 스트림을 막지 않는
/// best-effort 정책은 실행 단계(`ControlServer::prepare_viewer_display`)에서
/// 강제된다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualDisplayPreparation {
    /// 뷰어 메트릭이 없거나 매칭에 실패해 준비할 것이 없다.
    NotNeeded,
    /// 요청된 관리 화면을 매칭 크기로 맞춘다.
    Resize {
        id: String,
        width: u32,
        height: u32,
        scale: MatchedScale,
    },
    /// 관리 화면이 이미 매칭 크기라 그대로 쓴다.
    Reuse { id: String },
    /// 준비할 수 없어 매칭 결과만 로그로 남긴다. 스트림은 기존 소스로 계속된다.
    LogOnly {
        width: u32,
        height: u32,
        scale: MatchedScale,
    },
}

/// 관리 화면 이름에 붙는 소유권 마커(` display_management::add`)에서 관리 ID를
/// 뽑는다. 카탈로그의 가상 화면 소스 이름이 곧 관리 ID 참조가 된다.
fn managed_name_marker_id(name: &str) -> Option<&str> {
    name.rsplit_once("[leftcar:").and_then(|(_, rest)| {
        let end = rest.find(']')?;
        let id = &rest[..end];
        (!id.is_empty()).then_some(id)
    })
}

/// 스트림 시작 시 뷰어 메트릭으로 가상 화면 준비를 결정한다 (순수 — 부작용 없음).
///
/// - `viewer_display`가 없거나 매칭 실패면 `NotNeeded`: 스트림은 그대로 시작된다.
/// - `virtual_display_id`가 있으면 그 화면만 대상으로 한다. 없는 ID여도 오류를
///   내지 않고 `LogOnly`로 폴백한다.
/// - ID가 없는데 소스가 관리 화면(이름 마커)이면, 매칭 크기와 같은 관리 화면을
///   재사용한다. 없으면 생성하지 않고 `LogOnly`.
/// - 소스가 관리 화면이 아니면 준비 자체가 없다(`LogOnly` 로그만).
pub fn prepare_virtual_display(
    viewer_display: Option<&ViewerDisplayMetrics>,
    virtual_display_id: Option<&str>,
    source_name: Option<&str>,
    managed: &[ManagedDisplayMode<'_>],
) -> VirtualDisplayPreparation {
    let Some(metrics) = viewer_display else {
        return VirtualDisplayPreparation::NotNeeded;
    };
    let Some(matched) = match_display_size(metrics) else {
        return VirtualDisplayPreparation::NotNeeded;
    };
    let same_size = |entry: &ManagedDisplayMode<'_>| {
        entry.logical_width == matched.logical_width
            && entry.logical_height == matched.logical_height
            && entry.scale == scale_u8(matched.scale)
    };
    if let Some(id) = virtual_display_id {
        return managed
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| {
                if same_size(entry) {
                    VirtualDisplayPreparation::Reuse {
                        id: entry.id.to_owned(),
                    }
                } else {
                    VirtualDisplayPreparation::Resize {
                        id: entry.id.to_owned(),
                        width: matched.logical_width,
                        height: matched.logical_height,
                        scale: matched.scale,
                    }
                }
            })
            .unwrap_or(VirtualDisplayPreparation::LogOnly {
                width: matched.logical_width,
                height: matched.logical_height,
                scale: matched.scale,
            });
    }
    let Some(source_id) = source_name.and_then(managed_name_marker_id) else {
        return VirtualDisplayPreparation::LogOnly {
            width: matched.logical_width,
            height: matched.logical_height,
            scale: matched.scale,
        };
    };
    if let Some(entry) = managed.iter().find(|entry| entry.id == source_id) {
        if same_size(entry) {
            return VirtualDisplayPreparation::Reuse {
                id: entry.id.to_owned(),
            };
        }
        return VirtualDisplayPreparation::Resize {
            id: entry.id.to_owned(),
            width: matched.logical_width,
            height: matched.logical_height,
            scale: matched.scale,
        };
    }
    // 매칭 크기와 같은 관리 화면이 있으면 재사용, 없으면 절대 생성하지 않는다.
    managed
        .iter()
        .find(|entry| same_size(entry))
        .map(|entry| VirtualDisplayPreparation::Reuse {
            id: entry.id.to_owned(),
        })
        .unwrap_or(VirtualDisplayPreparation::LogOnly {
            width: matched.logical_width,
            height: matched.logical_height,
            scale: matched.scale,
        })
}

fn scale_u8(scale: MatchedScale) -> u8 {
    u8::from(scale)
}

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
    fn tablet_matches_hidpi_scale_two() {
        // 2800×1752 → 논리 1400×876, scale 2. 설계 문서 예시(물리 px ÷ 2)와 같다.
        let m = match_display_size(&metrics(2800, 1752, 420)).expect("태블릿 매칭 성공");
        assert_eq!(m.logical_width, 1400);
        assert_eq!(m.logical_height, 876);
        assert_eq!(m.scale, MatchedScale::Two);
    }

    #[test]
    fn portrait_and_landscape_same_pixels_are_equivalent() {
        let landscape = match_display_size(&metrics(2800, 1752, 420)).expect("가로 매칭 성공");
        let portrait = match_display_size(&metrics(1752, 2800, 420)).expect("세로 매칭 성공");
        // 세로/가로는 같은 픽셀 조합이므로 방향과 무관하게 동일한 크기 결정을 내려야 한다.
        assert_eq!(landscape, portrait);
    }

    #[test]
    fn low_resolution_falls_back_to_scale_one() {
        // 2560×1600은 HiDPI 1280×800으로 최소를 충족한다. 1920×1080은 HiDPI 960×540
        // 미달 → scale 1 폴백(논리 = 물리).
        let hidpi = match_display_size(&metrics(2560, 1600, 160)).expect("HiDPI 매칭 성공");
        assert_eq!(hidpi.scale, MatchedScale::Two);
        let fallback = match_display_size(&metrics(1920, 1080, 160)).expect("폴백 매칭 성공");
        assert_eq!(
            fallback,
            MatchedDisplaySize {
                logical_width: 1920,
                logical_height: 1080,
                scale: MatchedScale::One
            }
        );
    }

    #[test]
    fn fallback_still_below_floor_returns_none() {
        // 2000×1200은 scale 1 폴백이 유효(2000×1200). 폴백 후에도 1280×720 미달인
        // 1999×719(홀수 소스)는 None이다.
        let fallback = match_display_size(&metrics(2000, 1200, 160)).expect("폴백 매칭 성공");
        assert_eq!(fallback.scale, MatchedScale::One);
        // 경계값 1280×720은 유효하고, 그 아래는 None이다.
        assert_eq!(
            match_display_size(&metrics(1280, 720, 160)),
            Some(MatchedDisplaySize {
                logical_width: 1280,
                logical_height: 720,
                scale: MatchedScale::One
            })
        );
        assert_eq!(match_display_size(&metrics(1999, 719, 160)), None);
    }

    #[test]
    fn logical_sizes_are_even_aligned() {
        // 2843×1771: 물리 2842×1770 절반 1421×885 → 논리 짝수 정렬 1420×884.
        let m = match_display_size(&metrics(2843, 1771, 160)).expect("홀수 픽셀 매칭 성공");
        assert_eq!(m.logical_width, 1420);
        assert_eq!(m.logical_height, 884);
        assert_eq!(m.logical_width % 2, 0);
        assert_eq!(m.logical_height % 2, 0);
    }

    #[test]
    fn invalid_inputs_return_none() {
        assert_eq!(match_display_size(&metrics(0, 1200, 160)), None);
        assert_eq!(match_display_size(&metrics(1920, 0, 160)), None);
        assert_eq!(match_display_size(&metrics(1920, 1200, 0)), None);
    }

    fn managed<'a>(id: &'a str, w: u32, h: u32, scale: u8) -> ManagedDisplayMode<'a> {
        ManagedDisplayMode {
            id,
            logical_width: w,
            logical_height: h,
            scale,
        }
    }

    #[test]
    fn explicit_display_id_is_resized_to_the_matched_size() {
        let managed = [managed("abc", 1600, 1000, 2)];
        let preparation =
            prepare_virtual_display(Some(&metrics(2800, 1752, 420)), Some("abc"), None, &managed);
        assert_eq!(
            preparation,
            VirtualDisplayPreparation::Resize {
                id: "abc".into(),
                width: 1400,
                height: 876,
                scale: MatchedScale::Two
            }
        );
    }

    #[test]
    fn explicit_display_id_already_matching_size_is_reused() {
        let managed = [managed("abc", 1400, 876, 2)];
        let preparation =
            prepare_virtual_display(Some(&metrics(2800, 1752, 420)), Some("abc"), None, &managed);
        assert_eq!(
            preparation,
            VirtualDisplayPreparation::Reuse { id: "abc".into() }
        );
    }

    #[test]
    fn unknown_explicit_display_id_is_logged_not_created() {
        // 요청한 관리 화면이 없으면 오류로 스트림을 막지 않는다 — 로그만 남긴다.
        let managed = [managed("abc", 1600, 1000, 2)];
        let preparation = prepare_virtual_display(
            Some(&metrics(2800, 1752, 420)),
            Some("ghost"),
            None,
            &managed,
        );
        assert_eq!(
            preparation,
            VirtualDisplayPreparation::LogOnly {
                width: 1400,
                height: 876,
                scale: MatchedScale::Two
            }
        );
    }

    #[test]
    fn extension_mode_source_reuses_same_size_managed_display() {
        let managed = [managed("abc", 1400, 876, 2), managed("def", 1920, 1080, 1)];
        let preparation = prepare_virtual_display(
            Some(&metrics(2800, 1752, 420)),
            None,
            Some("Leftcar VD [leftcar:0b1e-4242]"),
            &managed,
        );
        assert_eq!(
            preparation,
            VirtualDisplayPreparation::Reuse { id: "abc".into() }
        );
    }

    #[test]
    fn extension_mode_source_without_match_never_creates_or_resizes() {
        // 소스가 관리 화면이어도 매칭 크기의 관리 화면이 없으면 생성하지 않고
        // 로그만 남긴다 — 생성은 호스트 UI의 명시적 사용자 동작으로 남는다.
        let managed = [managed("abc", 1920, 1080, 1)];
        let preparation = prepare_virtual_display(
            Some(&metrics(2800, 1752, 420)),
            None,
            Some("Leftcar VD [leftcar:0b1e-4242]"),
            &managed,
        );
        assert_eq!(
            preparation,
            VirtualDisplayPreparation::LogOnly {
                width: 1400,
                height: 876,
                scale: MatchedScale::Two
            }
        );
    }

    #[test]
    fn viewer_metrics_without_an_explicit_id_change_nothing_but_the_log() {
        // 관리 화면이 아니어도 매칭 결과는 로그로 남는다(준비 동작은 없음).
        let managed = [managed("abc", 1400, 876, 2)];
        let preparation = prepare_virtual_display(
            Some(&metrics(2800, 1752, 420)),
            None,
            Some("Main"),
            &managed,
        );
        assert_eq!(
            preparation,
            VirtualDisplayPreparation::LogOnly {
                width: 1400,
                height: 876,
                scale: MatchedScale::Two
            }
        );
    }

    #[test]
    fn missing_or_unmatchable_metrics_need_no_preparation() {
        let managed = [managed("abc", 1400, 876, 2)];
        assert_eq!(
            prepare_virtual_display(None, Some("abc"), None, &managed),
            VirtualDisplayPreparation::NotNeeded
        );
        assert_eq!(
            prepare_virtual_display(Some(&metrics(1999, 719, 160)), Some("abc"), None, &managed),
            VirtualDisplayPreparation::NotNeeded
        );
    }

    #[test]
    fn managed_name_marker_id_extracts_the_managed_id() {
        assert_eq!(
            managed_name_marker_id("Leftcar VD [leftcar:0b1e-4242]"),
            Some("0b1e-4242")
        );
        assert_eq!(managed_name_marker_id("Main"), None);
        assert_eq!(managed_name_marker_id("Leftcar VD [leftcar:]"), None);
    }
}
