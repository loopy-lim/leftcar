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
}
