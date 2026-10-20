import { SAMPLE_INTERVAL_MS } from "./diagnostics";
import type { DiagnosticsRecorder } from "./diagnostics-recorder";
import {
  evaluateThermalCap,
  type ThermalCapDecision,
  type ThermalCapState,
  type ThermalSnapshot,
} from "./thermal-policy";

/**
 * getLatestSample 브리지의 스냅숏. 키는 NDJSON sample 레코드와 같은
 * snake_case다 — 기록과 판단이 같은 원천임을 형식으로 유지한다(ADR-0006).
 * 값 없음은 null/부재다(0 오염 금지).
 */
export interface ThermalNativeSample {
  battery_percent?: number | null;
  charging?: boolean | null;
  plugged?: number | null;
  voltage_mv?: number | null;
  temp_deci_c?: number | null;
  thermal_status?: number | null;
  thermal_headroom?: number | null;
  release_gap_p50_ms?: number | null;
  release_gap_p95_ms?: number | null;
  release_gap_max_ms?: number | null;
  release_gap_count?: number | null;
}

export interface ThermalStateNative {
  getLatestSample(): Promise<ThermalNativeSample | null>;
}

export interface ThermalStatePorts {
  /** 네이티브가 없으면(웹·테스트) 감시 전체가 no-op이다 — 완화 미적용. */
  native: ThermalStateNative | null;
  epochMs: () => number;
  recorder: Pick<
    DiagnosticsRecorder,
    "recordThermalCap" | "recordThermalCapCleared"
  >;
}

export interface ThermalCaps {
  capacityCap?: number;
  startShortSideCap?: number;
}

function toSnapshot(sample: ThermalNativeSample | null): ThermalSnapshot {
  return {
    thermalStatus: typeof sample?.thermal_status === "number" ? sample.thermal_status : null,
    batteryPercent:
      typeof sample?.battery_percent === "number" ? sample.battery_percent : null,
    charging: sample?.charging === true,
  };
}

/**
 * thermal 상태 캐시·전환 기록. 새 폴링 루프를 만들지 않는다 — 소비자는
 * 필요 시점에 refresh()를 불러 15초 샘플러의 최신 값을 읽고, TTL 안에서는
 * 캐시를 쓴다. 상태 변화(apply·clear)만 진단 이벤트가 되고, 배터리 축
 * 변화는 무음 적용이다. 모든 실패는 삼켜 완화 미적용(현행 동작)으로 떨어진다.
 */
export class ThermalStateMonitor {
  private capState: ThermalCapState = "none";
  private caps: ThermalCaps = {};
  private fetchedAtMs = Number.NEGATIVE_INFINITY;
  private inFlight: Promise<ThermalCapDecision | null> | null = null;

  constructor(private readonly ports: ThermalStatePorts) {}

  get available(): boolean {
    return this.ports.native !== null;
  }

  get state(): ThermalCapState {
    return this.capState;
  }

  /** 마지막 판정의 상한. refresh 전엔 빈 객체(완화 미적용)다. */
  currentCaps(): ThermalCaps {
    return this.caps;
  }

  /** 마지막 갱신 이후 샘플 간격(TTL)을 넘었는지. */
  isStale(nowMs: number = this.ports.epochMs()): boolean {
    return nowMs - this.fetchedAtMs > SAMPLE_INTERVAL_MS;
  }

  /**
   * 최신 샘플로 판정을 갱신한다. 진행 중인 갱신이 있으면 그 약속을 재사용해
   * 동시 호출이 이벤트를 중복 기록하지 않게 한다.
   */
  refresh(): Promise<ThermalCapDecision | null> {
    const native = this.ports.native;
    if (!native) return Promise.resolve(null);
    if (this.inFlight) return this.inFlight;
    const pull = this.pull(native).finally(() => {
      this.inFlight = null;
    });
    this.inFlight = pull;
    return pull;
  }

  private async pull(native: ThermalStateNative): Promise<ThermalCapDecision | null> {
    try {
      const sample = await native.getLatestSample();
      this.fetchedAtMs = this.ports.epochMs();
      const snapshot = toSnapshot(sample);
      const decision = evaluateThermalCap(this.capState, snapshot);
      this.capState = decision.state;
      this.caps = {
        capacityCap: decision.capacityCap,
        startShortSideCap: decision.startShortSideCap,
      };
      if (decision.transition === "apply") {
        await this.ports.recorder.recordThermalCap(
          snapshot.thermalStatus ?? 0,
          decision.capacityCap ?? 0,
          decision.startShortSideCap,
        );
      } else if (decision.transition === "clear") {
        await this.ports.recorder.recordThermalCapCleared(snapshot.thermalStatus ?? 0);
      }
      return decision;
    } catch {
      // 진단·판정 실패는 조용히 버린다 — 스트림 흐름이 우선이다.
      return null;
    }
  }
}

/** 네이티브가 없는 환경(웹·테스트 빌드)용 no-op 싱글턴. */
export const thermalState = new ThermalStateMonitor({
  native: null,
  epochMs: () => Date.now(),
  recorder: diagnosticsRecorderStub(),
});

function diagnosticsRecorderStub(): ThermalStatePorts["recorder"] {
  return {
    recordThermalCap: async () => undefined,
    recordThermalCapCleared: async () => undefined,
  };
}
