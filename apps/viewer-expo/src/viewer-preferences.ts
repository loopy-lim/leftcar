import {
  is4KResolution,
  STREAM_PROFILES,
  type StreamProfileId,
} from "./stream-profile";

export const VIEWER_PREFERENCES_KEY = "leftcar.viewerPreferences";

export interface ViewerPreferences {
  profileId: StreamProfileId;
  showFps: boolean;
}

export const DEFAULT_VIEWER_PREFERENCES: ViewerPreferences = {
  profileId: "balanced",
  showFps: true,
};

export interface ViewerPreferencesStore {
  getItemAsync(key: string): Promise<string | null>;
  setItemAsync(key: string, value: string): Promise<void>;
}

interface DisplaySize {
  width: number;
  height: number;
}

export function recommendedStreamProfileId(
  display: DisplaySize,
): StreamProfileId {
  if (is4KResolution(display.width, display.height)) return "video";
  if (display.width >= 2560 && display.height >= 1440) return "balanced";
  return "latency";
}

function isStreamProfileId(value: unknown): value is StreamProfileId {
  return typeof value === "string"
    && STREAM_PROFILES.some((profile) => profile.id === value);
}

export function parseViewerPreferences(raw: string | null): ViewerPreferences {
  if (!raw) return { ...DEFAULT_VIEWER_PREFERENCES };
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    return {
      profileId: isStreamProfileId(parsed.profileId)
        ? parsed.profileId
        : DEFAULT_VIEWER_PREFERENCES.profileId,
      showFps: typeof parsed.showFps === "boolean"
        ? parsed.showFps
        : DEFAULT_VIEWER_PREFERENCES.showFps,
    };
  } catch {
    return { ...DEFAULT_VIEWER_PREFERENCES };
  }
}

export async function readViewerPreferences(
  store: ViewerPreferencesStore,
): Promise<ViewerPreferences> {
  try {
    return parseViewerPreferences(await store.getItemAsync(VIEWER_PREFERENCES_KEY));
  } catch {
    return { ...DEFAULT_VIEWER_PREFERENCES };
  }
}

export async function writeViewerPreferences(
  store: ViewerPreferencesStore,
  preferences: ViewerPreferences,
): Promise<void> {
  await store.setItemAsync(
    VIEWER_PREFERENCES_KEY,
    JSON.stringify({
      profileId: preferences.profileId,
      showFps: preferences.showFps,
    }),
  );
}
