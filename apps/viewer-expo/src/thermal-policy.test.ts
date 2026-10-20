import { describe, expect, it } from "vitest";

import {
  THERMAL_POLICY,
  evaluateThermalCap,
  type ThermalSnapshot,
} from "./thermal-policy";

const snapshot = (overrides: Partial<ThermalSnapshot> = {}): ThermalSnapshot => ({
  thermalStatus: null,
  batteryPercent: null,
  charging: false,
  ...overrides,
});

describe("thermal-policy", () => {
  it("freezes the conservative threshold table", () => {
    // ADR-0006 — 설계값이다. 변경은 실기 데이터(T5.2) 근거와 함께 문서로.
    expect(THERMAL_POLICY).toEqual({
      entryStatus: 3,
      releaseStatus: 1,
      criticalStatus: 4,
      severeCapacityCap: 2,
      criticalCapacityCap: 1,
      severeStartShortSide: 1_080,
      criticalStartShortSide: 720,
      lowBatteryPercent: 15,
      baseCapacity: 4,
    });
  });

  it("enters the severe cap at thermal status 3", () => {
    const decision = evaluateThermalCap("none", snapshot({ thermalStatus: 3 }));
    expect(decision.state).toBe("severe");
    expect(decision.capacityCap).toBe(2);
    expect(decision.startShortSideCap).toBe(1_080);
    expect(decision.transition).toBe("apply");
  });

  it("escalates to the critical cap at thermal status 4", () => {
    const decision = evaluateThermalCap("severe", snapshot({ thermalStatus: 4 }));
    expect(decision.state).toBe("critical");
    expect(decision.capacityCap).toBe(1);
    expect(decision.startShortSideCap).toBe(720);
    expect(decision.transition).toBe("apply");
  });

  it("releases the cap only at or below the light status", () => {
    const cleared = evaluateThermalCap("severe", snapshot({ thermalStatus: 1 }));
    expect(cleared.state).toBe("none");
    expect(cleared.capacityCap).toBeUndefined();
    expect(cleared.startShortSideCap).toBeUndefined();
    expect(cleared.transition).toBe("clear");

    // MODERATE(2)는 유지 구간이다 — 히스테리시스.
    const held = evaluateThermalCap("severe", snapshot({ thermalStatus: 2 }));
    expect(held.state).toBe("severe");
    expect(held.capacityCap).toBe(2);
    expect(held.transition).toBe("hold");
  });

  it("holds the previous state while samples are missing", () => {
    // 데이터 공백으로 해제하면 플래핑이 된다 — 유지가 안전 방향이다.
    expect(evaluateThermalCap("severe", null).state).toBe("severe");
    expect(evaluateThermalCap("severe", snapshot({ thermalStatus: null })).state).toBe("severe");
    expect(evaluateThermalCap("none", null).capacityCap).toBeUndefined();
  });

  it("de-escalates from critical back to severe without a clear event", () => {
    const decision = evaluateThermalCap("critical", snapshot({ thermalStatus: 3 }));
    expect(decision.state).toBe("severe");
    expect(decision.capacityCap).toBe(2);
    expect(decision.transition).toBe("apply");
  });

  it("applies the battery-axis reduction only while discharging", () => {
    // 15% 이하 방전 — 기본 용량 4에서 1 감축.
    const lowBattery = evaluateThermalCap("none", snapshot({ batteryPercent: 15, charging: false }));
    expect(lowBattery.capacityCap).toBe(3);
    expect(lowBattery.startShortSideCap).toBeUndefined();
    expect(lowBattery.transition).toBe("hold");

    // 충전 중에는 배터리 축 완화가 없다(발열 가중 — ADR-0006).
    const charging = evaluateThermalCap("none", snapshot({ batteryPercent: 10, charging: true }));
    expect(charging.capacityCap).toBeUndefined();
  });

  it("compounds the battery reduction with thermal caps at a floor of one", () => {
    const severe = evaluateThermalCap(
      "severe",
      snapshot({ thermalStatus: 3, batteryPercent: 8, charging: false }),
    );
    expect(severe.capacityCap).toBe(1);

    const critical = evaluateThermalCap(
      "critical",
      snapshot({ thermalStatus: 5, batteryPercent: 8, charging: false }),
    );
    expect(critical.capacityCap).toBe(1);
  });

  it("ignores implausible sample values", () => {
    expect(
      evaluateThermalCap("none", snapshot({ thermalStatus: Number.NaN, batteryPercent: 999 })).capacityCap,
    ).toBeUndefined();
    expect(
      evaluateThermalCap("none", snapshot({ thermalStatus: -1 })).capacityCap,
    ).toBeUndefined();
  });

  it("reports held transitions without a state change", () => {
    const decision = evaluateThermalCap("severe", snapshot({ thermalStatus: 3 }));
    expect(decision.state).toBe("severe");
    expect(decision.capacityCap).toBe(2);
    expect(decision.transition).toBe("hold");
  });
});
