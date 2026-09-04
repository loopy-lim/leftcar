import { describe, expect, it } from "vitest";
import {
  DEFAULT_VIEWER_PREFERENCES,
  parseViewerPreferences,
  recommendedStreamProfileId,
  readViewerPreferences,
  resolveViewerProfileId,
  writeViewerPreferences,
  type ViewerProfileSelection,
  type ViewerPreferencesStore,
} from "./viewer-preferences";

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
  });

  it("keeps valid stored choices while recovering invalid fields independently", () => {
    expect(parseViewerPreferences('{"profileId":"clarity","showFps":"yes"}')).toEqual({
      profileId: "clarity",
      showFps: true,
      localCursor: false,
    });
    expect(parseViewerPreferences('{"profileId":"unknown","showFps":false}')).toEqual({
      profileId: "auto",
      showFps: false,
      localCursor: false,
    });
  });

  it("round-trips preferences through the persistent store", async () => {
    const store = memoryStore();
    const preferences = { profileId: "video" as const, showFps: false, localCursor: true };

    await writeViewerPreferences(store, preferences);

    expect(await readViewerPreferences(store)).toEqual(preferences);
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
