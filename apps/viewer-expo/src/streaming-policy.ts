import type { StreamProfileId } from "./stream-profile";
import {
  fitStreamResolution,
  type StreamResolutionSource,
} from "./stream-resolution";

/**
 * Response-first vs clarity-first user intent. `responsive` is the default:
 * it keeps the 2K (2560×1440 at 16:9) baseline so input latency stays low.
 */
export type StreamingPriority = "responsive" | "clarity";

export const STREAMING_PRIORITIES = ["responsive", "clarity"] as const;

/** Requested frame rate for priority-derived stream targets. */
export const STREAM_TARGET_FPS = 60;

/** Long-side cap; 32:9 sources reach the planned 5120×1440 target. */
export const STREAMING_TARGET_MAX_WIDTH = 5_120;

/**
 * Pixel budget equals one 4K frame. Width alone is not a load budget: a
 * 5120×1440 frame holds fewer pixels than 3840×2160 and fits inside this
 * ceiling, while e.g. a hypothetical 7680×2160 would not.
 */
export const STREAMING_TARGET_MAX_PIXELS = 3_840 * 2_160;

/** Short-side anchor of the responsive 2K baseline (2560×1440 at 16:9). */
export const RESPONSIVE_STREAM_SHORT_SIDE = 1_440;

/** Short-side anchor of the clarity 4K target (3840×2160 at 16:9). */
export const CLARITY_STREAM_SHORT_SIDE = 2_160;

export interface StreamingTargetSource {
  width: number;
  height: number;
}

export interface StreamingTarget {
  width: number;
  height: number;
  fps: number;
}

export function isStreamingPriority(value: unknown): value is StreamingPriority {
  return value === "responsive" || value === "clarity";
}

/**
 * Source-aspect-aware stream target for a priority. 16:9 sources resolve to
 * 2560×1440 (responsive) or 3840×2160 (clarity); supported 32:9 sources reach
 * 5120×1440; portrait sources get the mirrored equivalents. Dimensions are
 * even, pixels stay inside the 4K budget, and smaller sources are never
 * upscaled just to reach a labeled target.
 */
export function resolveStreamingTarget(
  source: StreamingTargetSource,
  priority: StreamingPriority,
): StreamingTarget {
  const shortSide =
    priority === "clarity"
      ? CLARITY_STREAM_SHORT_SIDE
      : RESPONSIVE_STREAM_SHORT_SIDE;
  const fitted = fitStreamResolution(source as StreamResolutionSource, {
    maxWidth: STREAMING_TARGET_MAX_WIDTH,
    maxHeight: shortSide,
    maxPixels: STREAMING_TARGET_MAX_PIXELS,
  });
  return { width: fitted.width, height: fitted.height, fps: STREAM_TARGET_FPS };
}

/**
 * Starting stream target for a new stream. The priority picks the short-side
 * anchor (responsive 1440, clarity 2160); an explicit source/user maximum
 * (fitted manual profile or the source itself) only ever shrinks the start —
 * it never widens it. The maximum stays the adaptive policy's upshift goal
 * and is deliberately independent from this initial target.
 */
export function resolveInitialStreamTarget(
  source: StreamingTargetSource,
  priority: StreamingPriority,
  maximum?: StreamingTargetSource,
): StreamingTarget {
  if (!maximum) return resolveStreamingTarget(source, priority);
  const maxLongSide = Math.max(maximum.width, maximum.height);
  const maxShortSide = Math.min(maximum.width, maximum.height);
  const anchor = priority === "clarity"
    ? CLARITY_STREAM_SHORT_SIDE
    : RESPONSIVE_STREAM_SHORT_SIDE;
  const fitted = fitStreamResolution(source as StreamResolutionSource, {
    maxWidth: Math.min(STREAMING_TARGET_MAX_WIDTH, maxLongSide),
    maxHeight: Math.min(anchor, maxShortSide),
    maxPixels: STREAMING_TARGET_MAX_PIXELS,
  });
  return { width: fitted.width, height: fitted.height, fps: STREAM_TARGET_FPS };
}

/**
 * Legacy profile intent expressed as a streaming priority. Clarity-oriented
 * legacy profiles (`clarity`, `video`) map to `clarity`; responsiveness- and
 * balance-oriented ones (`latency`, `balanced`, `auto`) map to `responsive`.
 * The legacy identifiers themselves remain valid selections.
 */
export function streamingPriorityFromProfileId(
  profileId: StreamProfileId | "auto",
): StreamingPriority {
  return profileId === "clarity" || profileId === "video"
    ? "clarity"
    : "responsive";
}
