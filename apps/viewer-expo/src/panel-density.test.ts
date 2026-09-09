import { describe, expect, it } from "vitest";
import { applyPanelDensity, panelDensityScale } from "./panel-density";

describe("panelDensityScale", () => {
  it("600dp 이하에서는 1.0을 유지한다", () => {
    expect(panelDensityScale(360)).toBe(1);
    expect(panelDensityScale(412)).toBe(1);
    expect(panelDensityScale(600)).toBe(1);
  });

  it("600dp를 넘으면 완만한 선형 곡선으로 커진다", () => {
    // 태블릿 대역: 1.0 → 1.08 수준 — 폰 UI 정체감 유지
    expect(panelDensityScale(750)).toBeCloseTo(1.0417, 3);
    expect(panelDensityScale(900)).toBeCloseTo(1.0833, 3);
    expect(panelDensityScale(1200)).toBeCloseTo(1.1667, 3);
  });

  it("XR 대형 패널에서도 1.25로 상한이 걸린다", () => {
    expect(panelDensityScale(1500)).toBe(1.25);
    expect(panelDensityScale(1600)).toBe(1.25);
    expect(panelDensityScale(2560)).toBe(1.25);
  });

  it("비정상 입력은 1.0으로 방어한다", () => {
    expect(panelDensityScale(0)).toBe(1);
    expect(panelDensityScale(-10)).toBe(1);
    expect(panelDensityScale(Number.NaN)).toBe(1);
  });
});

describe("applyPanelDensity", () => {
  const styles = {
    card: {
      padding: 12,
      borderRadius: 10,
      gap: 6,
      flex: 1,
      opacity: 0.75,
      width: "70%",
    },
    label: {
      fontSize: 9,
      lineHeight: 16,
      letterSpacing: 0.04,
      fontWeight: "700",
      transform: [{ scale: 0.98 }],
    },
  } as const;

  it("scale 1에서는 원본 객체를 재사용한다", () => {
    expect(applyPanelDensity(styles, 1)).toBe(styles);
  });

  it("크기 속성만 곱하고 무차원 값과 문자열 값은 그대로 둔다", () => {
    const scaled = applyPanelDensity(styles, 1.5);
    expect(scaled.card.padding).toBe(18);
    expect(scaled.card.borderRadius).toBe(15);
    expect(scaled.card.gap).toBe(9);
    expect(scaled.card.flex).toBe(1);
    expect(scaled.card.opacity).toBe(0.75);
    expect(scaled.card.width).toBe("70%");
    expect(scaled.label.fontSize).toBe(14); // 9 * 1.5 = 13.5 → 반올림
    expect(scaled.label.lineHeight).toBe(24);
    // letterSpacing 같은 미세 소수 값은 스케일하지 않는다 (최소 1 반올림이 25배 점프를 만든다)
    expect(scaled.label.letterSpacing).toBe(0.04);
    expect(scaled.label.fontWeight).toBe("700");
    expect(scaled.label.transform).toEqual([{ scale: 0.98 }]);
  });

  it("축소 없음 — 작은 값의 최소 1을 보장한다", () => {
    const scaled = applyPanelDensity({ chip: { paddingVertical: 1 } }, 1.25);
    expect(scaled.chip.paddingVertical).toBe(1); // 1.25 → 반올림 1
    const scaled2 = applyPanelDensity({ chip: { paddingVertical: 1 } }, 1.5);
    expect(scaled2.chip.paddingVertical).toBe(2);
  });
});
