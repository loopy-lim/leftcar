import { describe, expect, it } from "vitest";
import {
  DEFAULT_VIEWER_PREFERENCES,
  parseViewerPreferences,
  recommendedStreamProfileId,
  readViewerPreferences,
  resolveStreamMaximum,
  resolveViewerProfileId,
  writeViewerPreferences,
  type ViewerProfileSelection,
  type ViewerPreferencesStore,
} from "./viewer-preferences";
import { resolveInitialStreamTarget } from "./streaming-policy";

function memoryStore(initial: string | null = null): ViewerPreferencesStore & {
  value: string | null;
} {
  return {
    value: initial,
    async getItemAsync() {
      return this.value;
    },
    async setItemAsync(_key, value) {
      this.value = value;
    },
  };
}

describe("viewer preferences", () => {
  it("uses per-display recommendations and visible FPS overlay by default", () => {
    expect(parseViewerPreferences(null)).toEqual(DEFAULT_VIEWER_PREFERENCES);
    expect(DEFAULT_VIEWER_PREFERENCES.profileId).toBe("auto");
    expect(DEFAULT_VIEWER_PREFERENCES.streamingPriority).toBe("responsive");
  });

  it("keeps valid stored choices while recovering invalid fields independently", () => {
    expect(parseViewerPreferences('{"profileId":"clarity","showFps":"yes"}')).toEqual({
      profileId: "clarity",
      streamingPriority: "clarity",
      showFps: true,
      localCursor: false,
    });
    expect(parseViewerPreferences('{"profileId":"unknown","showFps":false}')).toEqual({
      profileId: "auto",
      streamingPriority: "responsive",
      showFps: false,
      localCursor: false,
    });
  });

  it("migrates legacy profile intent into streamingPriority without dropping fields", () => {
    expect(
      parseViewerPreferences(
        '{"profileId":"clarity","showFps":false,"localCursor":true}',
      ),
    ).toEqual({
      profileId: "clarity",
      streamingPriority: "clarity",
      showFps: false,
      localCursor: true,
    });
    expect(parseViewerPreferences('{"profileId":"latency"}').streamingPriority).toBe(
      "responsive",
    );
    expect(parseViewerPreferences('{"profileId":"balanced"}').streamingPriority).toBe(
      "responsive",
    );
    expect(parseViewerPreferences('{"profileId":"video"}').streamingPriority).toBe(
      "clarity",
    );
    expect(parseViewerPreferences('{"profileId":"auto"}').streamingPriority).toBe(
      "responsive",
    );
    expect(parseViewerPreferences("{}").streamingPriority).toBe("responsive");
  });

  it("honors an explicitly stored streamingPriority over the legacy derivation", () => {
    expect(
      parseViewerPreferences('{"profileId":"latency","streamingPriority":"clarity"}')
        .streamingPriority,
    ).toBe("clarity");
    expect(
      parseViewerPreferences('{"profileId":"clarity","streamingPriority":"responsive"}')
        .streamingPriority,
    ).toBe("responsive");
    expect(
      parseViewerPreferences('{"streamingPriority":"cheapest"}').streamingPriority,
    ).toBe("responsive");
  });

  it("round-trips preferences through the persistent store", async () => {
    const store = memoryStore();
    const preferences = {
      profileId: "video" as const,
      streamingPriority: "clarity" as const,
      showFps: false,
      localCursor: true,
    };

    await writeViewerPreferences(store, preferences);

    expect(await readViewerPreferences(store)).toEqual(preferences);
    expect(JSON.parse(store.value ?? "{}")).toMatchObject({
      streamingPriority: "clarity",
    });
  });

  it("defaults localCursor to false and persists toggles", async () => {
    expect(DEFAULT_VIEWER_PREFERENCES.localCursor).toBe(false);
    expect(parseViewerPreferences(null).localCursor).toBe(false);
    expect(parseViewerPreferences('{"showFps":true}').localCursor).toBe(false);
    expect(
      parseViewerPreferences('{"localCursor":true,"showFps":true}').localCursor,
    ).toBe(true);
    expect(
      parseViewerPreferences('{"localCursor":"yes"}').localCursor,
    ).toBe(false);
  });

  it("recommends a profile from each display's actual pixel size", () => {
    expect(recommendedStreamProfileId({ width: 1920, height: 1080 })).toBe("latency");
    expect(recommendedStreamProfileId({ width: 2560, height: 1440 })).toBe("balanced");
    expect(recommendedStreamProfileId({ width: 3840, height: 2160 })).toBe("video");
  });

  it("uses the display recommendation only until the user makes a manual choice", () => {
    const display = { width: 3840, height: 2160 };
    expect(resolveViewerProfileId("auto", display)).toBe("video");
    expect(resolveViewerProfileId("balanced", display)).toBe("balanced");

    const selection: ViewerProfileSelection = "latency";
    expect(resolveViewerProfileId(selection, { width: 2560, height: 1440 })).toBe("latency");
  });
});

describe("resolveStreamMaximum", () => {
  it("gives AUTO the real clarity streaming target, not a legacy profile fit", () => {
    // 32:9 소스는 4K 픽셀 예산 안에서 5120x1440을 최대로 가진다. 구 프로필
    // (balanced 추천) 끼워 맞춤이었다면 2560x720으로 부서졌을 것이다.
    expect(resolveStreamMaximum({ width: 5120, height: 1440 }, "auto")).toEqual({
      width: 5120,
      height: 1440,
      fps: 60,
    });
  });

  it("opens a REAL AUTO 5120x1440 stream at the full maximum", () => {
    // 실제 자동(AUTO) 최대와 시작 크기가 모두 5120x1440이다 — 시작은
    // 우선순위와 무관하게 소스 전체를 쓴다(줄여 눈금을 맞출 필요 없음).
    const display = { width: 5120, height: 1440 };
    const maximum = resolveStreamMaximum(display, "auto");
    expect(resolveInitialStreamTarget(display, "responsive", maximum)).toEqual({
      width: 5120,
      height: 1440,
      fps: 60,
    });
    expect(resolveInitialStreamTarget(display, "clarity", maximum)).toEqual({
      width: 5120,
      height: 1440,
      fps: 60,
    });
  });

  it("mirrors the AUTO maximum and start for portrait displays", () => {
    const display = { width: 1440, height: 5120 };
    const maximum = resolveStreamMaximum(display, "auto");
    expect(maximum).toEqual({ width: 1440, height: 5120, fps: 60 });
    expect(resolveInitialStreamTarget(display, "responsive", maximum)).toEqual({
      width: 1440,
      height: 5120,
      fps: 60,
    });
  });

  it("keeps a manual balanced profile's legacy cap on a 32:9 display", () => {
    // 수동 균형 프로필의 사용자 의도는 기존 2560x1440 상자 그대로다 —
    // AUTO 최대 확장이 수동 상한을 넓히지 않는다.
    const display = { width: 5120, height: 1440 };
    const maximum = resolveStreamMaximum(display, "balanced");
    expect(maximum).toEqual({ width: 2560, height: 720, fps: 60 });
    expect(resolveInitialStreamTarget(display, "responsive", maximum)).toEqual({
      width: 2560,
      height: 720,
      fps: 60,
    });
  });

  it("keeps manual profiles on their legacy fits for 16:9 displays", () => {
    const display = { width: 3840, height: 2160 };
    expect(resolveStreamMaximum(display, "latency")).toEqual({
      width: 1920,
      height: 1080,
      fps: 60,
    });
    expect(resolveStreamMaximum(display, "video")).toEqual({
      width: 3840,
      height: 2160,
      fps: 60,
    });
    expect(resolveStreamMaximum(display, "clarity")).toEqual({
      width: 3840,
      height: 2160,
      fps: 60,
    });
  });

  it("matches the clarity target for AUTO on common display sizes", () => {
    expect(resolveStreamMaximum({ width: 3840, height: 2160 }, "auto")).toEqual({
      width: 3840,
      height: 2160,
      fps: 60,
    });
    expect(resolveStreamMaximum({ width: 2560, height: 1440 }, "auto")).toEqual({
      width: 2560,
      height: 1440,
      fps: 60,
    });
    // 작은 소스는 최대로 올리지 않는다(업스케일 금지).
    expect(resolveStreamMaximum({ width: 1920, height: 1080 }, "auto")).toEqual({
      width: 1920,
      height: 1080,
      fps: 60,
    });
  });
});
