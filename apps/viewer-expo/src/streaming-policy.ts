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

/**
 * Starting short-side cap when the route leaves the LAN (tailnet / public).
 * Outside links start conservatively at 1080p and the adaptive policy still
 * owns the upshift toward the selected maximum.
 */
export const EXTERNAL_ROUTE_START_SHORT_SIDE = 1_080;

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
 * upscaled just to reach a labeled target. `shortSideOverride` replaces the
 * priority anchor (used by the external-route start cap).
 */
export function resolveStreamingTarget(
  source: StreamingTargetSource,
  priority: StreamingPriority,
  shortSideOverride?: number,
): StreamingTarget {
  const shortSide =
    shortSideOverride ??
    (priority === "clarity"
      ? CLARITY_STREAM_SHORT_SIDE
      : RESPONSIVE_STREAM_SHORT_SIDE);
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
 *
 * `options.externalRoute` caps the start at 1080p short side: on tailnet or
 * public routes the first seconds must survive a narrow link, and recovering
 * upward is the adaptive policy's job. It changes the start only — never the
 * user's profile maximum.
 *
 * `options.panelShortSide` replaces the fixed priority anchor with the client
 * panel's physical short side so the stream arrives 1:1 (HiDPI 소스는 패널
 * 네이티브로, 그렇지 않으면 소스 그대로) instead of being upscaled after a
 * blind 1440 downscale. Missing or implausible metrics simply omit the cap
 * and the fixed anchors apply.
 */
export function resolveInitialStreamTarget(
  source: StreamingTargetSource,
  priority: StreamingPriority,
  maximum?: StreamingTargetSource,
  options?: { externalRoute?: boolean; panelShortSide?: number },
): StreamingTarget {
  const priorityAnchor = priority === "clarity"
    ? CLARITY_STREAM_SHORT_SIDE
    : RESPONSIVE_STREAM_SHORT_SIDE;
  const baseAnchor = options?.panelShortSide ?? priorityAnchor;
  const anchor = options?.externalRoute
    ? Math.min(baseAnchor, EXTERNAL_ROUTE_START_SHORT_SIDE)
    : baseAnchor;
  if (!maximum) return resolveStreamingTarget(source, priority, anchor);
  const maxLongSide = Math.max(maximum.width, maximum.height);
  const maxShortSide = Math.min(maximum.width, maximum.height);
  const fitted = fitStreamResolution(source as StreamResolutionSource, {
    maxWidth: Math.min(STREAMING_TARGET_MAX_WIDTH, maxLongSide),
    maxHeight: Math.min(anchor, maxShortSide),
    maxPixels: STREAMING_TARGET_MAX_PIXELS,
  });
  return { width: fitted.width, height: fitted.height, fps: STREAM_TARGET_FPS };
}

/**
 * 클라이언트 패널 단변 캡. 메트릭이 없거나 비정상(단변 1080 미만 — 창 크기를
 * 잘못 보고한 케이스, 2160 초과 — 4K 단변 초과)이면 undefined를 돌려 고정
 * 앵커(1440/2160)로 폴백한다. 캡이 앵커를 낮추는 쪽(1080p 패널 등)은
 * fitStreamResolution이 소스를 절대 업스케일하지 않으므로 결과가 동일하다.
 */
export function panelShortSideCap(
  metrics: StreamingTargetSource | undefined,
): number | undefined {
  if (!metrics) return undefined;
  const { width, height } = metrics;
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return undefined;
  }
  const shortSide = Math.min(width, height);
  if (shortSide < 1080 || shortSide > 2160) return undefined;
  return shortSide;
}

/**
 * 적응 정책의 업시프트 목표(최대)도 패널 단변을 넘지 않게 축소한다 — 패널에
 * 표시되지 않을 픽셀을 인코딩·전송하지 않는다. 소스 크기 이하로는 절대
 * 줄이지 않는다(그 경우 무캡과 동일).
 */
export function capTargetToPanelShortSide<T extends StreamingTargetSource>(
  target: T,
  cap: number | undefined,
): T {
  if (!cap) return target;
  const shortSide = Math.min(target.width, target.height);
  if (shortSide <= cap) return target;
  const scale = cap / shortSide;
  return {
    ...target,
    width: Math.max(2, Math.floor((target.width * scale) / 2) * 2),
    height: Math.max(2, Math.floor((target.height * scale) / 2) * 2),
  };
}

/**
 * Legacy profile intent expressed as a streaming priority. Clarity-oriented
 * legacy profiles (`clarity`, `video`, `smooth`) map to `clarity`;
 * responsiveness- and balance-oriented ones (`latency`, `balanced`, `auto`)
 * map to `responsive`. The legacy identifiers themselves remain valid
 * selections.
 */
export function streamingPriorityFromProfileId(
  profileId: StreamProfileId | "auto",
): StreamingPriority {
  return profileId === "clarity" || profileId === "video" || profileId === "smooth"
    ? "clarity"
    : "responsive";
}
