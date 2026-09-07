import {
  is4KResolution,
  STREAM_PROFILES,
  type StreamProfileId,
} from "./stream-profile";
import {
  isStreamingPriority,
  resolveStreamingTarget,
  streamingPriorityFromProfileId,
  type StreamingPriority,
} from "./streaming-policy";
import { resolveStreamResolution } from "./stream-resolution";

export type { StreamingPriority };

export const VIEWER_PREFERENCES_KEY = "leftcar.viewerPreferences";

export interface ViewerPreferences {
  profileId: ViewerProfileSelection;
  streamingPriority: StreamingPriority;
  showFps: boolean;
  localCursor: boolean;
}

export type ViewerProfileSelection = StreamProfileId | "auto";

export const DEFAULT_VIEWER_PREFERENCES: ViewerPreferences = {
  profileId: "auto",
  streamingPriority: "responsive",
  showFps: true,
  localCursor: false,
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

export function resolveViewerProfileId(
  selection: ViewerProfileSelection,
  display: DisplaySize,
): StreamProfileId {
  return selection === "auto" ? recommendedStreamProfileId(display) : selection;
}

/**
 * Actual stream maximum for a display under the user's profile selection —
 * the adaptive policy's upshift goal and the catalog preview's 최대 figure.
 *
 * `auto` resolves to the real clarity-first streaming target
 * (`resolveStreamingTarget(display, "clarity")`), so the maximum follows the
 * actual source size and the 4K pixel budget — a 32:9 display gets 5120×1440
 * instead of a legacy profile's crushed box fit. An explicit manual profile
 * keeps its own legacy cap: the user pinned that intent, and no priority
 * ever widens it.
 */
export function resolveStreamMaximum(
  display: DisplaySize,
  selection: ViewerProfileSelection,
): { width: number; height: number; fps: number } {
  if (selection === "auto") {
    return resolveStreamingTarget(display, "clarity");
  }
  const profile = STREAM_PROFILES.find((candidate) => candidate.id === selection) ??
    STREAM_PROFILES[0];
  return resolveStreamResolution(display, profile);
}

function isViewerProfileSelection(value: unknown): value is ViewerProfileSelection {
  return value === "auto"
    || (typeof value === "string"
      && STREAM_PROFILES.some((profile) => profile.id === value));
}

export function parseViewerPreferences(raw: string | null): ViewerPreferences {
  if (!raw) return { ...DEFAULT_VIEWER_PREFERENCES };
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const profileId = isViewerProfileSelection(parsed.profileId)
      ? parsed.profileId
      : DEFAULT_VIEWER_PREFERENCES.profileId;
    // Migrate legacy profile intent into streamingPriority. An explicitly
    // stored priority wins; otherwise it is derived from the stored profile
    // so older installs keep their clarity/responsiveness intent while
    // profileId, showFps and localCursor are preserved untouched.
    const streamingPriority = isStreamingPriority(parsed.streamingPriority)
      ? parsed.streamingPriority
      : streamingPriorityFromProfileId(profileId);
    return {
      profileId,
      streamingPriority,
      showFps: typeof parsed.showFps === "boolean"
        ? parsed.showFps
        : DEFAULT_VIEWER_PREFERENCES.showFps,
      localCursor: typeof parsed.localCursor === "boolean"
        ? parsed.localCursor
        : DEFAULT_VIEWER_PREFERENCES.localCursor,
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
      streamingPriority: preferences.streamingPriority,
      showFps: preferences.showFps,
      localCursor: preferences.localCursor,
    }),
  );
}
