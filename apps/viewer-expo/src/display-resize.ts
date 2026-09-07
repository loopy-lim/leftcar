import type { ActiveStream } from "./catalog-model-types";
import { fallbackTargetFor, type AdaptiveTarget } from "./adaptive-resolution";

export interface VirtualResizeTarget {
  width: number;
  height: number;
  fps: number;
  /** HiDPI scale the host confirmed for the new mode, when known. */
  scale?: 1 | 2;
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
  accepted: Pick<ActiveStream, "encoderExperiment"> & Partial<Pick<ActiveStream, "qualityState">>,
): ActiveStream {
  const nextTarget: AdaptiveTarget = {
    width: target.width,
    height: target.height,
    fps: target.fps,
  };
  return {
    ...active,
    encoderExperiment: accepted.encoderExperiment,
    qualityState: accepted.qualityState ?? "native",
    width: nextTarget.width,
    height: nextTarget.height,
    fps: nextTarget.fps,
    scale: target.scale ?? active.scale,
    sourceTarget: nextTarget,
    activeTarget: nextTarget,
    fallbackTarget: fallbackTargetFor(nextTarget),
  };
}
