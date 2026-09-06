import { describe, expect, it } from "vitest";
import { displaySizePresets, normalizeCustomSize } from "./display-size";

describe("displaySizePresets", () => {
  it("offers the tablet-matched candidate computed from physical pixels", () => {
    const presets = displaySizePresets({ width: 2800, height: 1752 }, null);
    const tabletMatch = presets.find((preset) => preset.label === "태블릿 크기");
    // 논리 = 물리 ÷ 2, 짝수 정렬. 1400×876 ≥ 1280×720이므로 scale 2.
    expect(tabletMatch).toEqual({ width: 1400, height: 876, scale: 2, label: "태블릿 크기" });
  });

  it("falls back to scale 1 when the matched logical size is below 1280×720", () => {
    const presets = displaySizePresets({ width: 1600, height: 1000 }, null);
    const tabletMatch = presets.find((preset) => preset.label === "태블릿 크기");
    // 논리 800×500 < 1280×720 → scale 1 (논리 = 물리).
    expect(tabletMatch).toEqual({ width: 800, height: 500, scale: 1, label: "태블릿 크기" });
  });

  it("omits the tablet candidate without tablet metrics", () => {
    const presets = displaySizePresets(null, null);
    expect(presets.find((preset) => preset.label.includes("태블릿"))).toBeUndefined();
    // 표준 프리셋은 항상 존재
    expect(presets.map((preset) => preset.label)).toEqual(
      expect.arrayContaining(["1080p", "1440p", "4K"]),
    );
  });

  it("aligns the matched logical size to even pixels (physical / 2)", () => {
    const presets = displaySizePresets({ width: 2805, height: 1751 }, null);
    const tabletMatch = presets.find((preset) => preset.label === "태블릿 크기");
    // 2805/2 = 1402.5 → 1402, 1751/2 = 875.5 → 875 → 짝수 정렬 874.
    expect(tabletMatch).toEqual({ width: 1402, height: 874, scale: 2, label: "태블릿 크기" });
  });

  it("keeps scale 2 only when the preset logical size reaches 1280×720", () => {
    // 4K 논리(물리÷2) = 1920×1080 → scale 2 허용
    const presets = displaySizePresets(null, null);
    const fourK = presets.find((preset) => preset.label === "4K");
    expect(fourK?.scale).toBe(2);
  });

  it("drops candidates identical to the current size and scale", () => {
    const presets = displaySizePresets(null, {
      width: 1920,
      height: 1080,
      scale: 2,
    });
    expect(presets.find((preset) => preset.label === "1080p")).toBeUndefined();
    // 논리 1920×1080에 scale 1을 얹으면 backing이 1920×1080이 되어 유효한 대안이므로
    // 남는다(scale이 다르면 다른 후보).
    const remaining = presets.filter((preset) => preset.label === "1080p");
    expect(remaining).toHaveLength(0);
    const fourK = presets.find((preset) => preset.label === "4K");
    expect(fourK).toBeDefined();
  });

  it("keeps a preset whose scale differs from the current size", () => {
    const presets = displaySizePresets(null, {
      width: 1920,
      height: 1080,
      scale: 1,
    });
    // 후보의 scale은 논리 크기에서 자동 판정된다(1920×1080 ≥ 1280×720 → scale 2).
    // 현재 scale 1과 backing이 달라 유효한 대안이므로 후보가 남는다.
    expect(presets.find((preset) => preset.label === "1080p")).toMatchObject({
      width: 1920,
      height: 1080,
      scale: 2,
    });
  });

  it("drops the tablet candidate when it equals the current size and scale", () => {
    const presets = displaySizePresets(
      { width: 2800, height: 1752 },
      { width: 1400, height: 876, scale: 2 },
    );
    expect(presets.find((preset) => preset.label === "태블릿 크기")).toBeUndefined();
  });

  it("keeps the tablet candidate when only the scale differs", () => {
    const presets = displaySizePresets(
      { width: 2800, height: 1752 },
      { width: 1400, height: 876, scale: 1 },
    );
    expect(presets.find((preset) => preset.label === "태블릿 크기")).toBeDefined();
  });
});

describe("normalizeCustomSize", () => {
  it("accepts and even-aligns valid custom sizes", () => {
    expect(normalizeCustomSize(1921, 1080)).toEqual({ width: 1920, height: 1080 });
    expect(normalizeCustomSize(640, 480)).toEqual({ width: 640, height: 480 });
    expect(normalizeCustomSize(4096, 4096)).toEqual({ width: 4096, height: 4096 });
    expect(normalizeCustomSize(2160, 3840)).toEqual({ width: 2160, height: 3840 });
  });

  it("rejects sizes below the 640×480 minimum", () => {
    expect(normalizeCustomSize(638, 480)).toBeNull();
    expect(normalizeCustomSize(640, 478)).toBeNull();
  });

  it("rejects sizes above the 4096×4096 maximum", () => {
    expect(normalizeCustomSize(4098, 2160)).toBeNull();
    expect(normalizeCustomSize(3840, 4100)).toBeNull();
  });

  it("rejects non-positive and non-finite input", () => {
    expect(normalizeCustomSize(0, 1080)).toBeNull();
    expect(normalizeCustomSize(-1920, 1080)).toBeNull();
    expect(normalizeCustomSize(Number.NaN, 1080)).toBeNull();
    expect(normalizeCustomSize(1920, Number.POSITIVE_INFINITY)).toBeNull();
  });

  it("rejects sizes that even-alignment drops below the minimum", () => {
    // 639 → 638(짝수) < 640 최소
    expect(normalizeCustomSize(639, 480)).toBeNull();
  });
});
