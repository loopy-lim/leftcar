/**
 * XR 창 비율 프리셋 — 태블릿 "화면 비율" 카드에서 선택해 활성
 * StreamActivity의 SpatialWindow 비율로 실시간 재적용한다. 컴퓨터 화면
 * 해상도는 이 값의 영향을 받지 않는다.
 *
 * `ratio`는 가로÷세로이며 0.5~2.0으로 clamp된다. 9:16은 세로 창 지정이므로
 * 1보다 작은 값을 가진다.
 */

export type WindowAspectRatioPresetId = "16:10" | "16:9" | "4:3" | "9:16";

export interface WindowAspectRatioPreset {
  id: WindowAspectRatioPresetId;
  /** 가로÷세로. 세로 프리셋은 1 미만. */
  ratio: number;
  /** 카드 버튼에 표시할 한국어 라벨. */
  label: string;
}

export const WINDOW_ASPECT_RATIO_PRESETS: WindowAspectRatioPreset[] = [
  { id: "16:10", ratio: 1.6, label: "16:10" },
  { id: "16:9", ratio: 16 / 9, label: "16:9" },
  { id: "4:3", ratio: 4 / 3, label: "4:3" },
  { id: "9:16", ratio: 9 / 16, label: "9:16" },
];

/** 프리셋 id로 비율을 찾는다. 없으면 null. */
export function windowAspectRatioPreset(
  id: WindowAspectRatioPresetId,
): WindowAspectRatioPreset | null {
  return WINDOW_ASPECT_RATIO_PRESETS.find((preset) => preset.id === id) ?? null;
}

/**
 * XR 창 비율을 0.5~2.0으로 clamp한다. 네이티브 Kotlin 정규화와 동일한
 * 범위 규칙으로 TS 미리보기·테스트에 쓰인다.
 */
export function clampWindowAspectRatio(ratio: number): number {
  return Math.min(2, Math.max(0.5, ratio));
}
