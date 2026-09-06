//! 뷰어(태블릿)가 보고한 화면 메트릭을 macOS 가상 화면 논리 크기로 바꾸는 순수 함수 모음.
//!
//! 매칭 규칙 (docs/plans/2026-09-06-viewer-display-sizing.md Task 1):
//! - 밀도 스케일 = `density_dpi / 160` (Android 기준 밀도). 논리 후보 = 물리 px ÷ 밀도 스케일.
//! - HiDPI 논리 = 논리 후보 ÷ 2. 결과는 짝수 픽셀로 내림 정렬한다(`(v / 2) * 2`).
//! - HiDPI 후보가 1280×720(폭·높이 각각) 미만이면 scale 1(논리 후보)로 폴백하고,
//!   폴백한 논리 후보마저 1280×720 미만이면 None.
//! - 세로로 보고된 메트릭도 같은 픽셀 조합이면 동일한 결과를 내도록 긴 변을 폭으로
//!   정규화한다(orientation 구분 없이 물리 px 기준으로 산출).

/// 뷰어가 보고한 태블릿 화면 메트릭 (물리 픽셀 + 밀도).
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

const MIN_LOGICAL_WIDTH: u64 = 1280;
const MIN_LOGICAL_HEIGHT: u64 = 720;

/// 물리 픽셀과 밀도로 정수 논리 픽셀을 계산하고 짝수로 내림 정렬한다.
///
/// `hidpi`가 true면 논리 후보를 다시 2로 나눈 HiDPI 값(`물리 px × 80 / dpi`)을,
/// false면 논리 후보(`물리 px × 160 / dpi`)를 정수로 내린 뒤 짝수 정렬한다.
/// 결과가 u32 범위를 벗어나면 None.
fn scaled_px(physical: u32, density_dpi: u32, hidpi: bool) -> Option<u32> {
    let candidate = u64::from(physical) * 160 / u64::from(density_dpi);
    let scaled = if hidpi { candidate / 2 } else { candidate };
    let aligned = (scaled / 2) * 2;
    u32::try_from(aligned).ok()
}

/// 뷰어 화면 메트릭을 가상 화면 논리 크기로 자동 매칭한다.
///
/// HiDPI(scale 2) 논리 후보가 유효 범위(폭·높이 각각 1280×720 이상)에 들면 scale 2,
/// 아니면 scale 1(논리 후보)로 폴백한다. 폴백한 논리 후보도 범위를 벗어나면 None.
/// 세로로 보고된 메트릭은 긴 변을 폭으로 정규화해 같은 픽셀 조합이면 동일한 결과를
/// 낸다. 입력이 무효하면(물리 px 0, dpi 0) None.
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
        logical_width: scaled_px(width, metrics.density_dpi, true)?,
        logical_height: scaled_px(height, metrics.density_dpi, true)?,
        scale: MatchedScale::Two,
    };
    if u64::from(hidpi.logical_width) >= MIN_LOGICAL_WIDTH
        && u64::from(hidpi.logical_height) >= MIN_LOGICAL_HEIGHT
    {
        return Some(hidpi);
    }
    let fallback = MatchedDisplaySize {
        logical_width: scaled_px(width, metrics.density_dpi, false)?,
        logical_height: scaled_px(height, metrics.density_dpi, false)?,
        scale: MatchedScale::One,
    };
    if u64::from(fallback.logical_width) >= MIN_LOGICAL_WIDTH
        && u64::from(fallback.logical_height) >= MIN_LOGICAL_HEIGHT
    {
        return Some(fallback);
    }
    None
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
    fn tablet_at_reference_density_matches_hidpi_scale_two() {
        // 2800×1752 @ 160dpi(밀도 1.0): 논리 후보 2800×1752 → HiDPI 1400×876.
        // 설계 문서 예시(물리 px ÷ 2 = 1400×876, scale 2)와 같다.
        let m = match_display_size(&metrics(2800, 1752, 160)).expect("태블릿 매칭 성공");
        assert_eq!(m.logical_width, 1400);
        assert_eq!(m.logical_height, 876);
        assert_eq!(m.scale, MatchedScale::Two);
    }

    #[test]
    fn portrait_and_landscape_same_pixels_are_equivalent() {
        let landscape = match_display_size(&metrics(2800, 1752, 160)).expect("가로 매칭 성공");
        let portrait = match_display_size(&metrics(1752, 2800, 160)).expect("세로 매칭 성공");
        // 세로/가로는 같은 픽셀 조합이므로 방향과 무관하게 동일한 크기 결정을 내려야 한다.
        assert_eq!(landscape, portrait);
    }

    #[test]
    fn hidpi_below_floor_falls_back_to_scale_one() {
        // 2560×1600 @ 320dpi: 논리 후보 1280×800 → HiDPI 640×400은 폭 기준 미달이므로
        // scale 1 폴백, 논리 후보 1280×800은 유효.
        let fallback = match_display_size(&metrics(2560, 1600, 320)).expect("폴백 매칭 성공");
        assert_eq!(
            fallback,
            MatchedDisplaySize {
                logical_width: 1280,
                logical_height: 800,
                scale: MatchedScale::One
            }
        );
    }

    #[test]
    fn fallback_still_below_floor_returns_none() {
        // 2800×1752 @ 420dpi(밀도 2.625): 논리 후보 1066×666, HiDPI 532×332 — 둘 다
        // 1280×720 미만이므로 매칭 실패. 계획 문서 예시의 "1344×836 scale 2"는 이
        // 공식으로는 산출되지 않는다(비율 1.6077 ≠ 2800×1752의 1.5982) — 보고서 참고.
        let none = match_display_size(&metrics(2800, 1752, 420));
        assert_eq!(none, None);
        // 저해상도 입력도 폴백 후 미달이면 None.
        let low = match_display_size(&metrics(2000, 1200, 320));
        assert_eq!(low, None);
    }

    #[test]
    fn logical_sizes_are_even_aligned() {
        // 2843×1771 @ 160dpi: HiDPI 1421×885 → 짝수 내림 1420×884.
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
