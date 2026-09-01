import { describe, expect, it } from "vitest";
import { resolveStreamResolution } from "./stream-resolution";
import {
  createAdaptiveResolutionState,
  observeAdaptiveResolution,
  recordAdaptiveResolutionResult,
  type AdaptiveObservation,
} from "./adaptive-resolution";

const sourceTarget = { width: 3840, height: 2160, fps: 60 };

function observation(
  nowMs: number,
  overrides: Partial<AdaptiveObservation> = {},
): AdaptiveObservation {
  return {
    nowMs,
    receiverLossDelta: 0,
    encodedFps: 60,
    transmittedFps: 60,
    requestedFps: 60,
    queueAgeUs: 10_000,
    latencyBudgetUs: 100_000,
    recoveryActive: false,
    rebindInFlight: false,
    ...overrides,
  };
}

describe("source-compatible adaptive resolution", () => {
  it("fits exact 4K and does not upscale smaller sources", () => {
    expect(resolveStreamResolution(
      { width: 3840, height: 2160 },
      { maxWidth: 3840, maxHeight: 2160, fps: 60 },
    )).toEqual(sourceTarget);
    expect(resolveStreamResolution(
      { width: 1920, height: 1080 },
      { maxWidth: 3840, maxHeight: 2160, fps: 60 },
    )).toEqual({ width: 1920, height: 1080, fps: 60 });
  });

  it("downshifts on the second congested window", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    const first = observeAdaptiveResolution(
      state,
      observation(1_000, {
        receiverLossDelta: 3,
        encodedFps: 48,
        transmittedFps: 48,
      }),
    );
    expect(first.action.kind).toBe("keep");
    state = first.state;
    const second = observeAdaptiveResolution(
      state,
      observation(2_000, {
        receiverLossDelta: 2,
        encodedFps: 50,
        transmittedFps: 49,
      }),
    );
    expect(second.action).toEqual({
      kind: "downshift",
      target: { width: 2560, height: 1440, fps: 60 },
    });
  });

  it("ignores recovery-burst loss and never downshifts a 1440p source", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    const recovering = observeAdaptiveResolution(
      state,
      observation(1_000, {
        receiverLossDelta: 20,
        encodedFps: 20,
        transmittedFps: 20,
        recoveryActive: true,
      }),
    );
    expect(recovering.action.kind).toBe("keep");
    expect(recovering.state.congestionWindows).toBe(0);

    state = createAdaptiveResolutionState({ width: 2560, height: 1440, fps: 60 });
    state = observeAdaptiveResolution(
      state,
      observation(1_000, { receiverLossDelta: 3, encodedFps: 40, transmittedFps: 40 }),
    ).state;
    expect(observeAdaptiveResolution(
      state,
      observation(2_000, { receiverLossDelta: 2, encodedFps: 40, transmittedFps: 40 }),
    ).action.kind).toBe("keep");
  });

  it("upshifts once after four stable windows and the five-second cooldown", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    const downshift = observeAdaptiveResolution(
      observeAdaptiveResolution(
        state,
        observation(1_000, { receiverLossDelta: 1, encodedFps: 40, transmittedFps: 40 }),
      ).state,
      observation(2_000, { receiverLossDelta: 1, encodedFps: 40, transmittedFps: 40 }),
    );
    state = recordAdaptiveResolutionResult(
      downshift.state,
      downshift.action,
      true,
      2_000,
    ).state;
    for (const nowMs of [3_000, 4_000, 5_000, 6_000]) {
      const stable = observeAdaptiveResolution(state, observation(nowMs));
      state = stable.state;
      expect(stable.action.kind).toBe("keep");
    }
    const afterCooldown = observeAdaptiveResolution(state, observation(7_000));
    expect(afterCooldown.action).toEqual({
      kind: "upshift",
      target: sourceTarget,
    });
  });

  it("keeps fallback after a failed upshift and resets the stability window", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    const downshift = observeAdaptiveResolution(
      observeAdaptiveResolution(
        state,
        observation(1_000, { receiverLossDelta: 1, encodedFps: 40, transmittedFps: 40 }),
      ).state,
      observation(2_000, { receiverLossDelta: 1, encodedFps: 40, transmittedFps: 40 }),
    );
    state = recordAdaptiveResolutionResult(
      downshift.state,
      downshift.action,
      true,
      2_000,
    ).state;
    state = { ...state, cooldownUntilMs: 0 };
    for (const nowMs of [3_000, 4_000, 5_000]) {
      state = observeAdaptiveResolution(state, observation(nowMs)).state;
    }
    const upshift = observeAdaptiveResolution(state, observation(6_000));
    expect(upshift.action.kind).toBe("upshift");
    const failed = recordAdaptiveResolutionResult(upshift.state, upshift.action, false, 6_000);
    expect(failed.state.activeTarget).toEqual({ width: 2560, height: 1440, fps: 60 });
    expect(failed.state.qualityState).toBe("fallback");
    expect(failed.state.stableWindows).toBe(0);
  });
});
