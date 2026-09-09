import type { TextStyle, ViewStyle } from "react-native";

/**
 * 패널 폭 기반 UI 밀도. 좁은 창(폰)은 오늘의 다이어트 UI를 그대로 두고,
 * 태블릿·XR 대형 패널에서만 폰트·터치 타깃을 확대한다. 네이티브 HUD는
 * StreamPanelDensity.kt가 같은 곡선을 미러링한다.
 *
 * 곡선: 600dp까지 1.0, 600→1500dp에 걸쳐 선형 상승해 1500dp 이상은
 * 1.25 상한. 태블릿(≈900dp)은 1.08 수준으로 폰 정체감을 유지하고, XR
 * 대형 패널에서만 최대 배율 — 최대 폰트 12px → 15dp로 XR 권장(1.75m에서
 * 14dp)을 계속 충족한다.
 */
const BASE_WIDTH_DP = 600;
const RAMP_WIDTH_DP = 900;
const MAX_SCALE = 1.25;

export function panelDensityScale(widthDp: number): number {
  if (!Number.isFinite(widthDp) || widthDp <= 0) return 1;
  if (widthDp <= BASE_WIDTH_DP) return 1;
  const ramp = Math.min((widthDp - BASE_WIDTH_DP) / RAMP_WIDTH_DP, 1);
  return 1 + (MAX_SCALE - 1) * ramp;
}

/**
 * 크기 성격을 가진 스타일 속성만 곱한다. flex·opacity·transform 같은
 * 무차원 값과 문자열 값("70%")은 건드리지 않는다.
 */
const SCALED_PROPERTIES: ReadonlySet<string> = new Set([
  "fontSize",
  "lineHeight",
  "gap",
  "rowGap",
  "columnGap",
  "borderRadius",
  "borderWidth",
  "borderBottomWidth",
  "borderTopWidth",
  "borderLeftWidth",
  "borderRightWidth",
  "width",
  "height",
  "minWidth",
  "minHeight",
  "maxWidth",
  "maxHeight",
  "padding",
  "paddingHorizontal",
  "paddingVertical",
  "paddingTop",
  "paddingBottom",
  "paddingLeft",
  "paddingRight",
  "margin",
  "marginHorizontal",
  "marginVertical",
  "marginTop",
  "marginBottom",
  "marginLeft",
  "marginRight",
  "top",
  "bottom",
  "left",
  "right",
  "elevation",
]);

/** 숫자 스타일 값을 밀도 배율만큼 키운다. 최소 1은 보장한다. */
function scaleValue(value: number, scale: number): number {
  return Math.max(1, Math.round(value * scale));
}

type NamedStyles = Record<string, ViewStyle | TextStyle>;

/** 스타일 번들 전체에 패널 밀도를 적용한다. scale이 1이면 원본을 재사용한다. */
export function applyPanelDensity<S extends NamedStyles>(styles: S, scale: number): S {
  if (scale === 1) return styles;
  const scaled: Record<string, ViewStyle | TextStyle> = {};
  for (const [name, style] of Object.entries(styles)) {
    const entry: Record<string, number | string | object> = {};
    for (const [property, value] of Object.entries(style)) {
      if (
        typeof value === "number" &&
        SCALED_PROPERTIES.has(property)
      ) {
        entry[property] = scaleValue(value, scale);
      } else {
        entry[property] = value;
      }
    }
    scaled[name] = entry as ViewStyle | TextStyle;
  }
  return scaled as S;
}
