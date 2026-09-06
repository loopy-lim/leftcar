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
  requestedFps: number;
  queueAgeUs: number;
  latencyBudgetUs: number;
  recoveryActive: boolean;
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

export function isExact4K(target: AdaptiveTarget): boolean {
  return target.width === 3_840 && target.height === 2_160;
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

export function createAdaptiveResolutionState(
  sourceTarget: AdaptiveTarget,
): AdaptiveResolutionState {
  return {
    sourceTarget: { ...sourceTarget },
    activeTarget: { ...sourceTarget },
    fallbackTarget: fallbackTargetFor(sourceTarget),
    qualityState: "native",
    congestionWindows: 0,
    stableWindows: 0,
    cooldownUntilMs: 0,
  };
}

function isCongested(
  observation: AdaptiveObservation,
): boolean {
  const fpsCollapsed =
    observation.encodedFps < observation.requestedFps * CONGESTION_FPS_RATIO ||
    (observation.transmittedFps !== undefined &&
      observation.transmittedFps < observation.requestedFps * CONGESTION_FPS_RATIO);
  // Receiver loss is useful evidence, but encoder/transport queue pressure is
  // independently actionable when it persists. A low FPS sample by itself is
  // intentionally ignored so static screens remain idle-safe.
  return (observation.floorCollapseDelta ?? 0) > 0 ||
    observation.queueAgeUs > observation.latencyBudgetUs ||
    (observation.receiverLossDelta > 0 && fpsCollapsed);
}

function isHealthy(
  observation: AdaptiveObservation,
): boolean {
  return observation.receiverLossDelta === 0 &&
    observation.encodedFps >= observation.requestedFps * HEALTHY_FPS_RATIO &&
    observation.queueAgeUs <= observation.latencyBudgetUs &&
    !observation.recoveryActive;
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
        qualityState: state.activeTarget.width === state.sourceTarget.width &&
          state.activeTarget.height === state.sourceTarget.height
          ? "native"
          : "fallback",
        congestionWindows: 0,
        stableWindows: 0,
        cooldownUntilMs: nowMs + REBIND_COOLDOWN_MS,
      },
      action: { kind: "keep" },
    };
  }
  const activeTarget = acceptedTarget ? { ...acceptedTarget } : { ...action.target };
  const acceptedNative = activeTarget.width === state.sourceTarget.width &&
    activeTarget.height === state.sourceTarget.height;
  return {
    state: {
      ...state,
      activeTarget,
      qualityState: acceptedNative ? "native" : "fallback",
      congestionWindows: 0,
      stableWindows: 0,
      cooldownUntilMs: nowMs + REBIND_COOLDOWN_MS,
    },
    action: { kind: "keep" },
  };
}
