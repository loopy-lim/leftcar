import { describe, expect, it, vi } from "vitest";
import {
  WINDOW_ASPECT_RATIO_PRESETS,
  clampWindowAspectRatio,
  windowAspectRatioPreset,
  type WindowAspectRatioPreset,
  type WindowAspectRatioPresetId,
} from "./window-aspect-ratio";

/**
 * 카드의 비율 프리셋 선택 → launcher 전파 규칙. 모델(handleSelectWindowAspectRatio)과
 * 동일한 계약을 순수 함수로 고정한다: 선택은 세션별로 기록되고, 성공은
 * 선택을 유지하며 실패는 조용히 이전 선택(또는 기록 없음)으로 되돌린다.
 */
function propagateRatioSelection(options: {
  presetId: WindowAspectRatioPresetId;
  session: number;
  port: number;
  previous: WindowAspectRatioPresetId | null;
  setSelection: (session: number, next: WindowAspectRatioPresetId | null) => void;
  setWindowAspectRatio: (instanceId: string, ratio: number) => Promise<void>;
}): void {
  const preset = windowAspectRatioPreset(options.presetId);
  if (!preset) return;
  options.setSelection(options.session, options.presetId);
  options
    .setWindowAspectRatio(`src-${options.port}`, preset.ratio)
    .catch(() => options.setSelection(options.session, options.previous));
}

describe("card ratio selection propagation", () => {
  const presetIds = WINDOW_ASPECT_RATIO_PRESETS.map((preset) => preset.id);
  const SESSION = 7;

  function baseOptions(
    setSelection: (session: number, next: WindowAspectRatioPresetId | null) => void,
  ) {
    return {
      presetId: "16:10" as WindowAspectRatioPresetId,
      session: SESSION,
      port: 5003,
      previous: null,
      setSelection,
    };
  }

  it("propagates the chosen preset ratio to the launcher with src-<port>", async () => {
    const setWindowAspectRatio = vi.fn(async () => undefined);
    const selections: Array<[number, WindowAspectRatioPresetId | null]> = [];
    propagateRatioSelection({
      ...baseOptions((session, next) => selections.push([session, next])),
      setWindowAspectRatio,
    });
    expect(setWindowAspectRatio).toHaveBeenCalledTimes(1);
    expect(setWindowAspectRatio).toHaveBeenCalledWith("src-5003", 1.6);
    expect(selections).toEqual([[SESSION, "16:10"]]);
  });

  it("propagates every preset id with its declared ratio", async () => {
    const calls: Array<[string, number]> = [];
    const setWindowAspectRatio = vi.fn(async (instanceId: string, ratio: number) => {
      calls.push([instanceId, ratio]);
    });
    for (const presetId of presetIds) {
      propagateRatioSelection({
        ...baseOptions(() => undefined),
        presetId,
        setWindowAspectRatio,
      });
    }
    expect(calls).toEqual([
      ["src-5003", 1.6],
      ["src-5003", expect.closeTo(16 / 9, 6)],
      ["src-5003", expect.closeTo(4 / 3, 6)],
      ["src-5003", expect.closeTo(9 / 16, 6)],
    ]);
  });

  it("restores the previous selection when the launcher rejects", async () => {
    const selections: Array<[number, WindowAspectRatioPresetId | null]> = [];
    const previous: WindowAspectRatioPresetId | null = "4:3";
    propagateRatioSelection({
      ...baseOptions((session, next) => selections.push([session, next])),
      presetId: "16:9",
      previous,
      setWindowAspectRatio: async () => {
        throw new Error("not an XR device");
      },
    });
    await Promise.resolve();
    await Promise.resolve();
    expect(selections).toEqual([[SESSION, "16:9"], [SESSION, previous]]);
  });

  it("ignores launcher failures without throwing", async () => {
    const setSelection = vi.fn<(session: number, next: WindowAspectRatioPresetId | null) => void>();
    expect(() =>
      propagateRatioSelection({
        ...baseOptions(setSelection),
        previous: null,
        setWindowAspectRatio: async () => {
          throw new Error("ERR_STREAM_NOT_ACTIVE");
        },
      }),
    ).not.toThrow();
    await Promise.resolve();
    await Promise.resolve();
    expect(setSelection).toHaveBeenCalledTimes(2);
  });

  it("records selections per session so multi-stream cards stay independent", () => {
    const selections: Record<number, WindowAspectRatioPresetId | null> = {};
    const apply = (
      session: number,
      next: WindowAspectRatioPresetId | null,
    ) => {
      selections[session] = next;
    };
    const setWindowAspectRatio = vi.fn(async () => undefined);
    propagateRatioSelection({
      ...baseOptions(apply),
      session: 1,
      setSelection: apply,
      setWindowAspectRatio,
    });
    propagateRatioSelection({
      ...baseOptions(apply),
      session: 2,
      presetId: "9:16",
      setSelection: apply,
      setWindowAspectRatio,
    });
    expect(selections[1]).toBe("16:10");
    expect(selections[2]).toBe("9:16");
    // 두 세션 모두 같은 인스턴스 규칙으로 전파된다.
    expect(setWindowAspectRatio).toHaveBeenNthCalledWith(1, "src-5003", 1.6);
    expect(setWindowAspectRatio).toHaveBeenNthCalledWith(2, "src-5003", 9 / 16);
  });
});

describe("preset catalogue shape shared with the native clamp", () => {
  it("keeps every ratio inside the 0.5~2.0 native clamp window", () => {
    const presets: WindowAspectRatioPreset[] = WINDOW_ASPECT_RATIO_PRESETS;
    for (const preset of presets) {
      expect(clampWindowAspectRatio(preset.ratio)).toBeCloseTo(preset.ratio, 6);
    }
  });
});

describe("windowAspectRatioPresets", () => {
  it("exposes the four approved presets with correct ratios", () => {
    expect(WINDOW_ASPECT_RATIO_PRESETS.map((preset) => preset.id)).toEqual([
      "16:10",
      "16:9",
      "4:3",
      "9:16",
    ]);
    const byId = (id: WindowAspectRatioPresetId) =>
      WINDOW_ASPECT_RATIO_PRESETS.find((preset) => preset.id === id)?.ratio;
    expect(byId("16:10")).toBeCloseTo(1.6, 6);
    expect(byId("16:9")).toBeCloseTo(16 / 9, 6);
    expect(byId("4:3")).toBeCloseTo(4 / 3, 6);
    expect(byId("9:16")).toBeCloseTo(9 / 16, 6);
  });

  it("marks 9:16 as the only portrait preset and keeps all ratios clampable", () => {
    for (const preset of WINDOW_ASPECT_RATIO_PRESETS) {
      const clamped = clampWindowAspectRatio(preset.ratio);
      expect(clamped).toBeCloseTo(preset.ratio, 6);
      expect(preset.label.length).toBeGreaterThan(0);
    }
    const portrait = windowAspectRatioPreset("9:16");
    expect(portrait?.ratio).toBeLessThan(1);
  });

  it("resolves presets by id and rejects unknown ids", () => {
    expect(windowAspectRatioPreset("16:10")?.ratio).toBeCloseTo(1.6, 6);
    expect(windowAspectRatioPreset("21:9" as WindowAspectRatioPresetId)).toBeNull();
  });
});

describe("clampWindowAspectRatio", () => {
  it("clamps into the 0.5~2.0 native window", () => {
    expect(clampWindowAspectRatio(0.1)).toBe(0.5);
    expect(clampWindowAspectRatio(9)).toBe(2);
    expect(clampWindowAspectRatio(1.6)).toBeCloseTo(1.6, 6);
  });

  it("rejects non-finite input by clamping to the nearest bound", () => {
    // Math.min/max 규칙상 NaN은 NaN으로 남는다 — 네이티브로 전달하지 않는다.
    expect(Number.isNaN(clampWindowAspectRatio(Number.NaN))).toBe(true);
  });
});
