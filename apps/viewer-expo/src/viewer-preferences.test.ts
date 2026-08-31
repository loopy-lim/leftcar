import { describe, expect, it } from "vitest";
import {
  DEFAULT_VIEWER_PREFERENCES,
  parseViewerPreferences,
  recommendedStreamProfileId,
  readViewerPreferences,
  writeViewerPreferences,
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
  it("uses the balanced profile and visible FPS overlay by default", () => {
    expect(parseViewerPreferences(null)).toEqual(DEFAULT_VIEWER_PREFERENCES);
  });

  it("keeps valid stored choices while recovering invalid fields independently", () => {
    expect(parseViewerPreferences('{"profileId":"clarity","showFps":"yes"}')).toEqual({
      profileId: "clarity",
      showFps: true,
    });
    expect(parseViewerPreferences('{"profileId":"unknown","showFps":false}')).toEqual({
      profileId: "balanced",
      showFps: false,
    });
  });

  it("round-trips preferences through the persistent store", async () => {
    const store = memoryStore();
    const preferences = { profileId: "video" as const, showFps: false };

    await writeViewerPreferences(store, preferences);

    expect(await readViewerPreferences(store)).toEqual(preferences);
  });

  it("recommends a profile from each display's actual pixel size", () => {
    expect(recommendedStreamProfileId({ width: 1920, height: 1080 })).toBe("latency");
    expect(recommendedStreamProfileId({ width: 2560, height: 1440 })).toBe("balanced");
    expect(recommendedStreamProfileId({ width: 3840, height: 2160 })).toBe("video");
  });
});
