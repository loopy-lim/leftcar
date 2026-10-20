import { DEFAULT_DECODER_CAPACITY } from "./decoder-budget";

/**
 * Thermal 인지 뷰어 자원 상한의 순수 정책 (ADR-0006).
 *
 * 판단은 전부 이 계층에서 한다 — Kotlin은 판단 없는 최신 샘플 전달자고,
 * 호스트는 뷰어 수치를 수용한다. 임계 표는 설계값이며 실기 thermal 분포
 * 데이터(docs/plans/2026-10-20-thermal-cadence-rendezvous-plan.md T5.2)로
 * 조정한다. 보수 원칙: 완화는 SEVERE부터, 해제는 LIGHT 이하 — MODERATE는
 * 유지 구간으로 쓰는 히스테리시스(15초 샘플이라 플래핑 비용이 크다).
 */
export const THERMAL_POLICY = {
  /** 진입: PowerManager.THERMAL_STATUS_SEVERE. */
  entryStatus: 3,
  /** 해제: THERMAL_STATUS_LIGHT 이하. */
  releaseStatus: 1,
  criticalStatus: 4,
  severeCapacityCap: 2,
  criticalCapacityCap: 1,
  severeStartShortSide: 1_080,
  criticalStartShortSide: 720,
  /** 배터리 축: 이 퍼센트 이하 + 방전 중이면 용량 상한을 1 더 낮춘다. */
  lowBatteryPercent: 15,
  /** 배터리 축만 켜질 때의 감축 기준 용량(decoder-budget 상한과 동일). */
  baseCapacity: DEFAULT_DECODER_CAPACITY,
} as const;

export type ThermalCapState = "none" | "severe" | "critical";

/** getLatestSample 브리지의 스냅셧. 값 없음은 null이다(0 오염 금지). */
export interface ThermalSnapshot {
  thermalStatus: number | null;
  batteryPercent: number | null;
  charging: boolean;
}

export type ThermalCapTransition = "hold" | "apply" | "clear";

export interface ThermalCapDecision {
  state: ThermalCapState;
  /** 디코더 동시 용량 상한. 활성 캡이 없으면 undefined(힌트 값 그대로). */
  capacityCap?: number;
  /** 새 스트림 시작 단변 상한. 배터리 축만으로는 걸지 않는다. */
  startShortSideCap?: number;
  /** 상태 변화만 이벤트가 된다 — 배터리 축 변화는 무음 적용이다. */
  transition: ThermalCapTransition;
}

function thermalStatusOrNull(value: number | null | undefined): number | null {
  if (value == null || !Number.isFinite(value)) return null;
  const rounded = Math.round(value);
  // PowerManager thermal status는 0..6 범위다. 범위 밖은 관측 불가로 취급한다.
  return rounded >= 0 && rounded <= 6 ? rounded : null;
}

function batteryPercentOrNull(value: number | null | undefined): number | null {
  if (value == null || !Number.isFinite(value)) return null;
  const rounded = Math.round(value);
  return rounded >= 0 && rounded <= 100 ? rounded : null;
}

/**
 * 직전 상태와 최신 스냅샷으로 다음 상한을 판정한다. 상태 기계:
 * - thermalStatus ≥ 4 → critical, ≥ 3 → severe, ≤ 1 → 해제, 2(MODERATE)는 유지.
 * - 데이터 공백(스냅샷 없음·status null)은 직전 상태를 유지한다 — 공백으로
 *   해제하면 샘플러 지연마다 플래핑이 되고, 유지가 안전 방향이다.
 * - 배터리 축: `batteryPercent ≤ 15 && !charging`일 때 용량 상한을 1 더
 *   낮춘다(하한 1). 충전 중에는 발열을 가중하므로 완화하지 않는다(ADR-0006).
 */
export function evaluateThermalCap(
  previous: ThermalCapState,
  snapshot: ThermalSnapshot | null | undefined,
): ThermalCapDecision {
  let state = previous;
  const status = thermalStatusOrNull(snapshot?.thermalStatus);
  if (status != null) {
    if (status >= THERMAL_POLICY.criticalStatus) {
      state = "critical";
    } else if (status >= THERMAL_POLICY.entryStatus) {
      state = "severe";
    } else if (status <= THERMAL_POLICY.releaseStatus) {
      state = "none";
    }
  }

  let capacityCap: number | undefined;
  let startShortSideCap: number | undefined;
  if (state === "critical") {
    capacityCap = THERMAL_POLICY.criticalCapacityCap;
    startShortSideCap = THERMAL_POLICY.criticalStartShortSide;
  } else if (state === "severe") {
    capacityCap = THERMAL_POLICY.severeCapacityCap;
    startShortSideCap = THERMAL_POLICY.severeStartShortSide;
  }

  const percent = batteryPercentOrNull(snapshot?.batteryPercent);
  const discharging = snapshot?.charging !== true;
  if (percent != null && percent <= THERMAL_POLICY.lowBatteryPercent && discharging) {
    capacityCap = Math.max(1, (capacityCap ?? THERMAL_POLICY.baseCapacity) - 1);
  }

  const transition: ThermalCapTransition =
    state === previous ? "hold" : state === "none" ? "clear" : "apply";
  return { state, capacityCap, startShortSideCap, transition };
}
