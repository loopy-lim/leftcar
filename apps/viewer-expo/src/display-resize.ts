import type { ActiveStream } from "./catalog-model-types";
import { fallbackTargetFor, type AdaptiveTarget } from "./adaptive-resolution";

export interface VirtualResizeTarget {
  width: number;
  height: number;
  fps: number;
}

/**
 * Next stream state after a managed virtual display resize. The session keeps
 * its identity while the logical source moves to the accepted display size;
 * the adaptive state is re-seeded so the new size becomes the "native" target
 * and the fallback is recomputed with the existing downscale policy.
 */
export function streamTargetAfterVirtualResize(
  active: ActiveStream,
  target: VirtualResizeTarget,
): ActiveStream {
  const nextTarget: AdaptiveTarget = {
    width: target.width,
    height: target.height,
    fps: target.fps,
  };
  return {
    ...active,
    width: nextTarget.width,
    height: nextTarget.height,
    fps: nextTarget.fps,
    sourceTarget: nextTarget,
    activeTarget: nextTarget,
    fallbackTarget: fallbackTargetFor(nextTarget),
  };
}
