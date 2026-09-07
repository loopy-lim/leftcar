import { describe, expect, it } from "vitest";
import { resolveStreamResolution } from "./stream-resolution";
import {
  createAdaptiveResolutionState,
  fallbackTargetFor,
  observeAdaptiveResolution,
  recordAdaptiveResolutionResult,
  seedAdaptiveResolutionState,
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
  it("creates proportional even-pixel fallbacks for landscape and portrait sources", () => {
    expect(fallbackTargetFor({ width: 3200, height: 2000, fps: 60 })).toEqual({
      width: 2304,
      height: 1440,
      fps: 60,
    });
    expect(fallbackTargetFor({ width: 2000, height: 3200, fps: 60 })).toEqual({
      width: 1440,
      height: 2304,
      fps: 60,
    });
    expect(fallbackTargetFor({ width: 4000, height: 500, fps: 60 })).toBeNull();
    expect(fallbackTargetFor({ width: 1920, height: 1080, fps: 60 })).toBeNull();
  });

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

  it("counts chronic completed recovery episodes as congestion evidence", () => {
    // Regression: recurring recovery episodes used to reset congestion windows
    // forever, permanently blocking downshift on a struggling link.
    let state = createAdaptiveResolutionState(sourceTarget);
    for (const nowMs of [1_000, 2_000, 3_000]) {
      const result = observeAdaptiveResolution(
        state,
        observation(nowMs, {
          receiverLossDelta: 0,
          encodedFps: 40,
          transmittedFps: 40,
          recoveryActive: false,
          recoveryObserved: true,
        }),
      );
      state = result.state;
    }
    const second = observeAdaptiveResolution(
      state,
      observation(4_000, {
        receiverLossDelta: 0,
        encodedFps: 40,
        transmittedFps: 40,
        recoveryObserved: true,
      }),
    );
    expect(second.action).toEqual({
      kind: "downshift",
      target: { width: 2560, height: 1440, fps: 60 },
    });
  });

  it("does not treat completed recovery episodes alone as congestion", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    for (const nowMs of [1_000, 2_000, 3_000]) {
      const result = observeAdaptiveResolution(
        state,
        observation(nowMs, { recoveryObserved: true }),
      );
      state = result.state;
      expect(result.action.kind).toBe("keep");
    }
    expect(state.congestionWindows).toBe(0);
  });

  it("still pauses measurements while a recovery burst is actively in progress", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    state = observeAdaptiveResolution(
      state,
      observation(1_000, { queueAgeUs: 120_000 }),
    ).state;
    const burst = observeAdaptiveResolution(
      state,
      observation(2_000, {
        queueAgeUs: 120_000,
        recoveryActive: true,
      }),
    );
    expect(burst.action.kind).toBe("keep");
    expect(burst.state.congestionWindows).toBe(0);
  });

  it("does not count a recovery-observed window as stable for upshift", () => {
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
    const disrupted = observeAdaptiveResolution(
      state,
      observation(6_000, { recoveryObserved: true }),
    );
    expect(disrupted.action.kind).toBe("keep");
    expect(disrupted.state.stableWindows).toBe(0);
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

  it("downshifts on sustained queue pressure even without receiver packet loss", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    state = observeAdaptiveResolution(
      state,
      observation(1_000, { queueAgeUs: 120_000 }),
    ).state;
    const second = observeAdaptiveResolution(
      state,
      observation(2_000, { queueAgeUs: 140_000 }),
    );
    expect(second.action.kind).toBe("downshift");
  });

  it("uses a Host bitrate-floor collapse as explicit downshift evidence", () => {
    let state = createAdaptiveResolutionState({ width: 3200, height: 2000, fps: 60 });
    state = observeAdaptiveResolution(
      state,
      observation(1_000, { floorCollapseDelta: 1 }),
    ).state;
    const second = observeAdaptiveResolution(
      state,
      observation(2_000, { floorCollapseDelta: 1 }),
    );
    expect(second.action).toEqual({
      kind: "downshift",
      target: { width: 2304, height: 1440, fps: 60 },
    });
  });

  it("does not treat an idle low-fps source as congestion", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    for (const nowMs of [1_000, 2_000, 3_000]) {
      const result = observeAdaptiveResolution(
        state,
        observation(nowMs, { encodedFps: 10, transmittedFps: 10 }),
      );
      state = result.state;
      expect(result.action.kind).toBe("keep");
    }
  });

  it("honors cooldown after a failed downshift", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    const first = observeAdaptiveResolution(state, observation(1_000, { queueAgeUs: 120_000 }));
    const second = observeAdaptiveResolution(first.state, observation(2_000, { queueAgeUs: 120_000 }));
    state = recordAdaptiveResolutionResult(second.state, second.action, false, 2_000).state;
    const blocked = observeAdaptiveResolution(state, observation(3_000, { queueAgeUs: 120_000 }));
    expect(blocked.action.kind).toBe("keep");
    expect(blocked.state.congestionWindows).toBe(0);
  });

  it("stores the target actually accepted by the renderer", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    const action = { kind: "downshift" as const, target: { width: 2560, height: 1440, fps: 60 } };
    const result = recordAdaptiveResolutionResult(
      state,
      action,
      true,
      2_000,
      { width: 2304, height: 1440, fps: 60 },
    );
    expect(result.state.activeTarget).toEqual({ width: 2304, height: 1440, fps: 60 });
    expect(result.state.qualityState).toBe("fallback");
  });

  it("reports native when a downshift is accepted at the source target", () => {
    const state = createAdaptiveResolutionState(sourceTarget);
    const result = recordAdaptiveResolutionResult(
      state,
      { kind: "downshift", target: { width: 2560, height: 1440, fps: 60 } },
      true,
      2_000,
      sourceTarget,
    );
    expect(result.state.activeTarget).toEqual(sourceTarget);
    expect(result.state.qualityState).toBe("native");
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

  it("does not upscale while measured encoder output remains slow", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    state = recordAdaptiveResolutionResult(
      state,
      { kind: "downshift", target: { width: 2560, height: 1440, fps: 60 } },
      true,
      0,
    ).state;
    for (const nowMs of [5_000, 6_000, 7_000, 8_000, 9_000]) {
      const result = observeAdaptiveResolution(
        state,
        observation(nowMs, { encodedFps: 30, transmittedFps: 60 }),
      );
      state = result.state;
      expect(result.action.kind).toBe("keep");
    }
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

describe("receiver renderedFps pressure", () => {
  it("downshifts when the receiver freezes while Host-side encoding still reports 60", () => {
    // 회귀: encodedFps는 60을 유지하지만 수신기 renderedFps가 얼어 붙은
    // 상태(손실 동반)는 혼잡 증거가 되어야 한다.
    let state = createAdaptiveResolutionState(sourceTarget);
    state = observeAdaptiveResolution(
      state,
      observation(1_000, {
        encodedFps: 60,
        transmittedFps: 60,
        renderedFps: 0,
        receiverLossDelta: 4,
      }),
    ).state;
    const second = observeAdaptiveResolution(
      state,
      observation(2_000, {
        encodedFps: 60,
        transmittedFps: 60,
        renderedFps: 2,
        receiverLossDelta: 2,
      }),
    );
    expect(second.action).toEqual({
      kind: "downshift",
      target: { width: 2560, height: 1440, fps: 60 },
    });
  });

  it("counts sustained renderedFps collapse with completed recovery episodes as congestion", () => {
    let state = createAdaptiveResolutionState(sourceTarget);
    const first = observeAdaptiveResolution(
      state,
      observation(1_000, {
        encodedFps: 60,
        renderedFps: 8,
        recoveryObserved: true,
      }),
    );
    expect(first.action.kind).toBe("keep");
    const second = observeAdaptiveResolution(
      first.state,
      observation(2_000, {
        encodedFps: 60,
        renderedFps: 8,
        recoveryObserved: true,
      }),
    );
    expect(second.action.kind).toBe("downshift");
  });

  it("ignores receiver loss when no renderedFps observation exists (older hosts)", () => {
    // renderedFps를 알 수 없으면 기존 동작을 유지한다: 손실만으로는
    // encodedFps 60에서 혼잡으로 보지 않는다.
    let state = createAdaptiveResolutionState(sourceTarget);
    for (const nowMs of [1_000, 2_000, 3_000]) {
      const result = observeAdaptiveResolution(
        state,
        observation(nowMs, {
          encodedFps: 60,
          transmittedFps: 60,
          receiverLossDelta: 3,
        }),
      );
      state = result.state;
      expect(result.action.kind).toBe("keep");
    }
  });

  it("does not treat a low renderedFps sample by itself as congestion", () => {
    // 정적 화면에서 renderedFps가 낮게 측정되더라도 손실·복구·큐 압력이
    // 없으면 혼잡이 아니다 (idle-safe).
    let state = createAdaptiveResolutionState(sourceTarget);
    for (const nowMs of [1_000, 2_000, 3_000]) {
      const result = observeAdaptiveResolution(
        state,
        observation(nowMs, {
          encodedFps: 60,
          transmittedFps: 60,
          renderedFps: 5,
        }),
      );
      state = result.state;
      expect(result.action.kind).toBe("keep");
      expect(state.congestionWindows).toBe(0);
    }
  });

  it("withholds upshift while the fresh receiver frame rate stays deficit", () => {
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
      state = observeAdaptiveResolution(
        state,
        observation(nowMs, { renderedFps: 30 }),
      ).state;
    }
    // encodedFps는 60에 도달했지만 수신기 renderedFps가 부족하면
    // 안정 구간으로 계산하지 않는다.
    const withheld = observeAdaptiveResolution(
      state,
      observation(6_000, { renderedFps: 30 }),
    );
    expect(withheld.action.kind).toBe("keep");
    expect(withheld.state.stableWindows).toBe(0);
  });
});

describe("seedAdaptiveResolutionState", () => {
  it("seeds the actual accepted start target while keeping the source maximum", () => {
    // responsive 시작(2560x1440)과 소스/사용자 최대(4K)를 분리해 저장한다.
    const state = seedAdaptiveResolutionState(
      sourceTarget,
      { width: 2560, height: 1440, fps: 60 },
      { nowMs: 10_000 },
    );
    expect(state.sourceTarget).toEqual(sourceTarget);
    expect(state.activeTarget).toEqual({ width: 2560, height: 1440, fps: 60 });
    expect(state.fallbackTarget).toEqual(fallbackTargetFor(sourceTarget));
    expect(state.qualityState).toBe("fallback");
    // 시작 직후 즉시 리바인드되는 소유권 경합을 피하기 위해 히스테리시스
    // 시작 시점에 쿨다운을 둔다.
    expect(state.cooldownUntilMs).toBeGreaterThan(10_000);
    expect(state.congestionWindows).toBe(0);
    expect(state.stableWindows).toBe(0);
  });

  it("seeds native with no cooldown when the stream starts at its maximum", () => {
    const state = seedAdaptiveResolutionState(sourceTarget, sourceTarget);
    expect(state.qualityState).toBe("native");
    expect(state.cooldownUntilMs).toBe(0);
    expect(state.activeTarget).toEqual(sourceTarget);
  });

  it("upshifts a responsive start to the source maximum after stable windows", () => {
    // 적응 초기 폴백 → 업시프트 회귀: responsive 1440으로 시작한 스트림은
    // 안정 구간이 쌓이면 소스 최대(4K)로 복귀해야 한다.
    let state = seedAdaptiveResolutionState(
      sourceTarget,
      { width: 2560, height: 1440, fps: 60 },
      { nowMs: 0 },
    );
    for (const nowMs of [6_000, 7_000, 8_000]) {
      state = observeAdaptiveResolution(state, observation(nowMs)).state;
      expect(state.stableWindows).toBeGreaterThan(0);
    }
    const upshift = observeAdaptiveResolution(state, observation(9_000));
    expect(upshift.action).toEqual({ kind: "upshift", target: sourceTarget });
  });

  it("re-seeding for an explicit target change resets counters without touching sample logic", () => {
    // 명시적 해상도 변경 후 동기화는 상태를 다시 심는다 — 매 샘플마다
    // 히스테리시스를 무효화하지 않는다 (호출부가 변경 시에만 호출).
    let state = createAdaptiveResolutionState(sourceTarget);
    state = observeAdaptiveResolution(
      state,
      observation(1_000, { queueAgeUs: 120_000 }),
    ).state;
    expect(state.congestionWindows).toBe(1);
    state = seedAdaptiveResolutionState(
      { width: 1920, height: 1080, fps: 60 },
      { width: 1920, height: 1080, fps: 60 },
      { nowMs: 5_000 },
    );
    expect(state.congestionWindows).toBe(0);
    expect(state.qualityState).toBe("native");
    expect(state.fallbackTarget).toBeNull();
  });
});
