import type { ActiveStream } from "./catalog-model-types";
import { fallbackTargetFor, type AdaptiveTarget } from "./adaptive-resolution";

export interface SessionResizeTarget {
  width: number;
  height: number;
  fps: number;
}

/**
 * Next stream state after an explicit session resolution change. The session
 * keeps its identity while the stream target moves to the accepted size; the
 * adaptive state is re-seeded so the new size becomes the "native" target and
 * the fallback is recomputed with the existing downscale policy.
 */
export function streamTargetAfterResize(
  active: ActiveStream,
  target: SessionResizeTarget,
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
    sourceTarget: nextTarget,
    activeTarget: nextTarget,
    fallbackTarget: fallbackTargetFor(nextTarget),
  };
}
