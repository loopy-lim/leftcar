export type AdaptiveQualityState =
  | "native"
  | "downshifting"
  | "fallback"
  | "upshifting";

export interface AdaptiveTarget {
  width: number;
  height: number;
  fps: number;
}

export interface AdaptiveObservation {
  nowMs: number;
  receiverLossDelta: number;
  encodedFps: number;
  /** Measured transmitted frame rate when a transport counter is available. */
  transmittedFps?: number;
  /**
   * Receiver-measured output frame rate from fresh receiver feedback.
   * Deliberately named separately from `transmittedFps`/`encodedFps`: a
   * receiver can be frozen while Host-side encoders still report 60. Callers
   * must omit the field (not send 0) when feedback is unavailable or stale so
   * older hosts and idle links stay idle-safe.
   */
  renderedFps?: number;
  requestedFps: number;
  queueAgeUs: number;
  latencyBudgetUs: number;
  /**
   * A recovery burst is actively in progress right now (frames are being
   * regenerated/suppressed). Window measurements are invalid while true, so
   * the policy pauses and resets its congestion/stability accumulators.
   * Callers must pass cumulative-counter deltas here; episode observations
   * belong in `recoveryObserved`.
   */
  recoveryActive: boolean;
  /**
   * Completed recovery episodes were observed inside this window (counter
   * delta). This is evidence of a struggling link, not a measurement
   * invalidator: it never blocks downshift on its own, counts as congestion
   * evidence when the frame rate also collapsed, and disqualifies the window
   * from upshift stability.
   */
  recoveryObserved?: boolean;
  rebindInFlight: boolean;
  floorCollapseDelta?: number;
}

export interface AdaptiveResolutionState {
  sourceTarget: AdaptiveTarget;
  activeTarget: AdaptiveTarget;
  fallbackTarget: AdaptiveTarget | null;
  qualityState: AdaptiveQualityState;
  congestionWindows: number;
  stableWindows: number;
  cooldownUntilMs: number;
}

export type AdaptiveResolutionAction =
  | { kind: "keep" }
  | { kind: "downshift"; target: AdaptiveTarget }
  | { kind: "upshift"; target: AdaptiveTarget };

export interface AdaptiveResolutionObservation {
  state: AdaptiveResolutionState;
  action: AdaptiveResolutionAction;
}

const DOWNSHIFT_WINDOWS = 2;
const UPSHIFT_WINDOWS = 4;
const REBIND_COOLDOWN_MS = 5_000;
const CONGESTION_FPS_RATIO = 0.9;
const HEALTHY_FPS_RATIO = 0.95;
const FALLBACK_MAX_WIDTH = 2_560;
const FALLBACK_MAX_HEIGHT = 1_440;
const FALLBACK_MIN_DIMENSION = 720;

/** Split-4K 계약 해상도 — Host가 (width, height, fps) 정확 일치를 강제한다. */
export const SPLIT_4K_WIDTH = 3_840;
export const SPLIT_4K_HEIGHT = 2_160;

export function isExact4K(target: AdaptiveTarget): boolean {
  return target.width === SPLIT_4K_WIDTH && target.height === SPLIT_4K_HEIGHT;
}

export function fallbackTargetFor(sourceTarget: AdaptiveTarget): AdaptiveTarget | null {
  const portrait = sourceTarget.height > sourceTarget.width;
  const maxWidth = portrait ? FALLBACK_MAX_HEIGHT : FALLBACK_MAX_WIDTH;
  const maxHeight = portrait ? FALLBACK_MAX_WIDTH : FALLBACK_MAX_HEIGHT;
  const scaleToFit = Math.min(
    maxWidth / sourceTarget.width,
    maxHeight / sourceTarget.height,
    1,
  );
  if (scaleToFit >= 1) return null;

  // Keep the source aspect ratio while aligning both dimensions for codecs.
  // The minimum is a readability guard for unusually wide/tall sources.
  const shortSide = Math.min(sourceTarget.width, sourceTarget.height);
  const scaleToMinimum = FALLBACK_MIN_DIMENSION / shortSide;
  const scale = Math.min(1, Math.max(scaleToFit, scaleToMinimum));
  const width = Math.max(2, Math.floor(sourceTarget.width * scale / 2) * 2);
  const height = Math.max(2, Math.floor(sourceTarget.height * scale / 2) * 2);
  if (width > maxWidth || height > maxHeight) return null;
  if (width >= sourceTarget.width && height >= sourceTarget.height) return null;
  return { width, height, fps: sourceTarget.fps };
}

/**
 * Seed adaptive state from what is actually running instead of assuming the
 * stream sits at its source maximum. Used when a session appears (responsive
 * starts open below the maximum) and after an explicit target change; never
 * per sample, so hysteresis keeps accumulating between real changes.
 */
export function seedAdaptiveResolutionState(
  sourceTarget: AdaptiveTarget,
  activeTarget: AdaptiveTarget,
  options: { nowMs?: number } = {},
): AdaptiveResolutionState {
  const state: AdaptiveResolutionState = {
    sourceTarget: { ...sourceTarget },
    activeTarget: { ...activeTarget },
    fallbackTarget: fallbackTargetFor(sourceTarget),
    qualityState: "native",
    congestionWindows: 0,
    stableWindows: 0,
    cooldownUntilMs: 0,
  };
  if (
    activeTarget.width !== sourceTarget.width ||
    activeTarget.height !== sourceTarget.height
  ) {
    state.qualityState = "fallback";
    if (options.nowMs !== undefined) {
      // Keep the rebind cooldown alive across the seed so ownership-sensitive
      // boundaries (start, explicit resize) never flip-flop immediately.
      state.cooldownUntilMs = options.nowMs + REBIND_COOLDOWN_MS;
    }
  }
  return state;
}

function isCongested(
  observation: AdaptiveObservation,
): boolean {
  const fpsCollapsed =
    observation.encodedFps < observation.requestedFps * CONGESTION_FPS_RATIO ||
    (observation.transmittedFps !== undefined &&
      observation.transmittedFps < observation.requestedFps * CONGESTION_FPS_RATIO) ||
    (observation.renderedFps !== undefined &&
      observation.renderedFps < observation.requestedFps * CONGESTION_FPS_RATIO);
  // Receiver loss is useful evidence, but encoder/transport queue pressure is
  // independently actionable when it persists. A low FPS sample by itself is
  // intentionally ignored so static screens remain idle-safe — including a
  // low receiver `renderedFps` with no loss/recovery/queue evidence. Completed
  // recovery episodes plus a collapsed frame rate are the same kind of
  // sustained-pressure evidence as loss; without this, a link that keeps
  // recovering could suppress downshifting forever.
  return (observation.floorCollapseDelta ?? 0) > 0 ||
    observation.queueAgeUs > observation.latencyBudgetUs ||
    (observation.receiverLossDelta > 0 && fpsCollapsed) ||
    ((observation.recoveryObserved ?? false) && fpsCollapsed);
}

function isHealthy(
  observation: AdaptiveObservation,
): boolean {
  const receiverHealthy = observation.renderedFps === undefined ||
    observation.renderedFps >= observation.requestedFps * HEALTHY_FPS_RATIO;
  return observation.receiverLossDelta === 0 &&
    observation.encodedFps >= observation.requestedFps * HEALTHY_FPS_RATIO &&
    receiverHealthy &&
    observation.queueAgeUs <= observation.latencyBudgetUs &&
    !observation.recoveryActive &&
    !(observation.recoveryObserved ?? false);
}

/**
 * Pure one-window transition function. The caller owns rebind execution and
 * calls recordAdaptiveResolutionResult after the Host/renderer acknowledges
 * the target.
 */
export function observeAdaptiveResolution(
  previous: AdaptiveResolutionState,
  observation: AdaptiveObservation,
): AdaptiveResolutionObservation {
  // Only an actively in-flight recovery burst or rebind invalidates window
  // measurements. Merely observing completed recovery episodes must not reset
  // the accumulators, or chronic recovery would block downshift forever.
  if (observation.recoveryActive || observation.rebindInFlight) {
    return {
      state: {
        ...previous,
        congestionWindows: 0,
        stableWindows: 0,
      },
      action: { kind: "keep" },
    };
  }

  if (previous.activeTarget.width === previous.sourceTarget.width &&
    previous.activeTarget.height === previous.sourceTarget.height) {
    if (observation.nowMs < previous.cooldownUntilMs) {
      return {
        state: { ...previous, congestionWindows: 0, stableWindows: 0 },
        action: { kind: "keep" },
      };
    }
    const congestionWindows = isCongested(observation)
      ? previous.congestionWindows + 1
      : 0;
    const nextState = {
      ...previous,
      congestionWindows,
      stableWindows: 0,
    };
    if (previous.fallbackTarget && congestionWindows >= DOWNSHIFT_WINDOWS) {
      return {
        state: { ...nextState, qualityState: "downshifting" },
        action: { kind: "downshift", target: { ...previous.fallbackTarget } },
      };
    }
    return { state: nextState, action: { kind: "keep" } };
  }

  const stableWindows = isHealthy(observation)
    ? previous.stableWindows + 1
    : 0;
  const nextState = {
    ...previous,
    congestionWindows: 0,
    stableWindows,
  };
  if (
    previous.fallbackTarget &&
    stableWindows >= UPSHIFT_WINDOWS &&
    observation.nowMs >= previous.cooldownUntilMs
  ) {
    return {
      state: { ...nextState, qualityState: "upshifting" },
      action: { kind: "upshift", target: { ...previous.sourceTarget } },
    };
  }
  return { state: nextState, action: { kind: "keep" } };
}

export function deriveQualityState(
  acceptedTarget: AdaptiveTarget,
  sourceTarget: AdaptiveTarget,
  reported?: AdaptiveQualityState,
): AdaptiveQualityState {
  if (reported) return reported;
  return acceptedTarget.width === sourceTarget.width &&
    acceptedTarget.height === sourceTarget.height
    ? "native"
    : "fallback";
}

export function recordAdaptiveResolutionResult(
  state: AdaptiveResolutionState,
  action: AdaptiveResolutionAction,
  success: boolean,
  nowMs: number,
  acceptedTarget?: AdaptiveTarget,
): AdaptiveResolutionObservation {
  if (action.kind === "keep") {
    return { state, action };
  }
  if (!success) {
    return {
      state: {
        ...state,
      qualityState: deriveQualityState(state.activeTarget, state.sourceTarget),
        congestionWindows: 0,
        stableWindows: 0,
        cooldownUntilMs: nowMs + REBIND_COOLDOWN_MS,
      },
      action: { kind: "keep" },
    };
  }
  const activeTarget = acceptedTarget ? { ...acceptedTarget } : { ...action.target };
  return {
    state: {
      ...state,
      activeTarget,
      qualityState: deriveQualityState(activeTarget, state.sourceTarget),
      congestionWindows: 0,
      stableWindows: 0,
      cooldownUntilMs: nowMs + REBIND_COOLDOWN_MS,
    },
    action: { kind: "keep" },
  };
}
