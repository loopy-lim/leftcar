import { describe, expect, it } from "vitest";

import type { DiagnosticsRecorder } from "./diagnostics-recorder";
import {
  ThermalStateMonitor,
  type ThermalNativeSample,
  type ThermalStatePorts,
} from "./thermal-state";

type RecorderCalls = Pick<
  DiagnosticsRecorder,
  "recordThermalCap" | "recordThermalCapCleared"
> & {
  caps: Array<{ status: number; capacityCap: number; startShortSideCap?: number }>;
  clears: number[];
};

function fakeRecorder(): RecorderCalls {
  const calls = {
    caps: [] as Array<{ status: number; capacityCap: number; startShortSideCap?: number }>,
    clears: [] as number[],
  };
  return {
    caps: calls.caps,
    clears: calls.clears,
    recordThermalCap: async (thermalStatus, capacityCap, startShortSideCap) => {
      calls.caps.push({ status: thermalStatus, capacityCap, startShortSideCap });
    },
    recordThermalCapCleared: async (thermalStatus) => {
      calls.clears.push(thermalStatus);
    },
  };
}

function ports(
  overrides: Partial<ThermalStatePorts> = {},
): ThermalStatePorts {
  return {
    native: { getLatestSample: async () => null },
    epochMs: () => 1_000_000,
    recorder: fakeRecorder(),
    ...overrides,
  };
}

const severeSample: ThermalNativeSample = {
  thermal_status: 3,
  battery_percent: 80,
  charging: false,
};

describe("ThermalStateMonitor", () => {
  it("maps the latest native sample onto the policy decision", async () => {
    const state = new ThermalStateMonitor(
      ports({ native: { getLatestSample: async () => severeSample } }),
    );
    const decision = await state.refresh();
    expect(decision?.state).toBe("severe");
    expect(state.currentCaps()).toEqual({ capacityCap: 2, startShortSideCap: 1_080 });
    expect(state.state).toBe("severe");
  });

  it("records the transition edge exactly once per state change", async () => {
    const recorder = fakeRecorder();
    let sample: ThermalNativeSample | null = severeSample;
    const state = new ThermalStateMonitor(
      ports({ recorder, native: { getLatestSample: async () => sample } }),
    );
    await state.refresh();
    await state.refresh(); // 같은 상태 유지 — 무음
    sample = { thermal_status: 1 };
    await state.refresh(); // 해제 edge
    expect(recorder.caps).toEqual([
      { status: 3, capacityCap: 2, startShortSideCap: 1_080 },
    ]);
    expect(recorder.clears).toEqual([1]);
  });

  it("treats missing native as a no-op and reports unavailability", async () => {
    const state = new ThermalStateMonitor(ports({ native: null }));
    await expect(state.refresh()).resolves.toBeNull();
    expect(state.currentCaps()).toEqual({});
    expect(state.available).toBe(false);
  });

  it("swallows native failures and leaves the caps untouched", async () => {
    const state = new ThermalStateMonitor(
      ports({
        native: {
          getLatestSample: async () => {
            throw new Error("jni");
          },
        },
      }),
    );
    await expect(state.refresh()).resolves.toBeNull();
    expect(state.currentCaps()).toEqual({});
    expect(state.state).toBe("none");
  });

  it("merges concurrent refreshes into one native read", async () => {
    let reads = 0;
    const state = new ThermalStateMonitor(
      ports({
        native: {
          getLatestSample: async () => {
            reads += 1;
            return severeSample;
          },
        },
      }),
    );
    await Promise.all([state.refresh(), state.refresh(), state.refresh()]);
    expect(reads).toBe(1);
  });

  it("reports staleness past the sample interval after a refresh", async () => {
    let epoch = 1_000_000;
    const state = new ThermalStateMonitor(
      ports({ epochMs: () => epoch, native: { getLatestSample: async () => severeSample } }),
    );
    expect(state.isStale()).toBe(true);
    await state.refresh();
    epoch = 1_010_000;
    expect(state.isStale()).toBe(false);
    epoch = 1_016_000;
    expect(state.isStale()).toBe(true);
  });

  it("starts with no caps so the first refresh decides mitigation", () => {
    const state = new ThermalStateMonitor(ports({}));
    expect(state.state).toBe("none");
    expect(state.currentCaps()).toEqual({});
  });

  it("keeps refreshing usable after a native rejection", async () => {
    let shouldFail = true;
    const state = new ThermalStateMonitor(
      ports({
        native: {
          getLatestSample: async () => {
            if (shouldFail) throw new Error("jni");
            return severeSample;
          },
        },
      }),
    );
    await expect(state.refresh()).resolves.toBeNull();
    shouldFail = false;
    const decision = await state.refresh();
    expect(decision?.state).toBe("severe");
  });
});
