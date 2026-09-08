import { describe, expect, it } from "vitest";
import {
  displaySizePresets,
  normalizeCustomSize,
} from "./display-size";

describe("displaySizePresets", () => {
  it("always offers the fixed resolution presets", () => {
    const presets = displaySizePresets(null);
    expect(presets.map((preset) => preset.label)).toEqual(["1080p", "1440p", "4K"]);
    expect(presets.find((preset) => preset.label === "4K")).toEqual({
      width: 3840,
      height: 2160,
      label: "4K",
    });
  });

  it("drops the candidate identical to the current size", () => {
    const presets = displaySizePresets({ width: 1920, height: 1080 });
    expect(presets.find((preset) => preset.label === "1080p")).toBeUndefined();
    const fourK = presets.find((preset) => preset.label === "4K");
    expect(fourK).toBeDefined();
  });

  it("keeps every preset when the current size matches none", () => {
    const presets = displaySizePresets({ width: 1600, height: 1000 });
    expect(presets.map((preset) => preset.label)).toEqual(["1080p", "1440p", "4K"]);
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
