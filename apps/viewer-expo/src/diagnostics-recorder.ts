import {
  formatSession,
  type DiagnosticsSessionEvent,
  type DiagnosticsSessionRecord,
} from "./diagnostics";

/** Diagnostics 네이티브 모듈 표면. 세션 레코드 기록과 내보내기·요약. */
export interface DiagnosticsRecorderNative {
  appendRecord(line: string): Promise<boolean>;
  uptimeMs(): Promise<number>;
  getSummary(): Promise<Record<string, number>>;
  exportToDownloads(): Promise<string>;
}

export type AppStateListener = (state: string) => void | Promise<void>;

export interface DiagnosticsRecorderPorts {
  /** 네이티브가 없으면(웹·테스트) 모든 기록이 no-op이다. */
  native: DiagnosticsRecorderNative | null;
  epochMs: () => number;
  uptimeMs: () => Promise<number>;
  subscribeAppState: (listener: AppStateListener) => () => void;
  currentState: () => string;
}

/**
 * 세션 이벤트 기록기. sample·crash·session_start 레코드는 Kotlin 쪽이
 * JS 수명과 무관하게 기록하며, 이 기록기는 살아 있는 TS가 알 수 있는
 * stream 시작/종료와 포그라운드 전환만 세션 레코드로 붙인다. 모든 실패는
 * 삼킨다 — 진단이 스트림 흐름을 방해하지 않는다.
 */
export class DiagnosticsRecorder {
  constructor(private readonly ports: DiagnosticsRecorderPorts) {}

  get available(): boolean {
    return this.ports.native !== null;
  }

  recordStreamStart(profile: string, decoderSlots: number): Promise<void> {
    return this.append({
      ts: this.ports.epochMs(),
      event: "stream_start",
      profile,
      decoderSlots,
    });
  }

  recordStreamStop(): Promise<void> {
    return this.append({ ts: this.ports.epochMs(), event: "stream_stop" });
  }

  /** thermal 상한 진입·격상(상태 변화 edge에서만 1회). */
  recordThermalCap(
    thermalStatus: number,
    capacityCap: number,
    startShortSideCap?: number,
  ): Promise<void> {
    return this.append({
      ts: this.ports.epochMs(),
      event: "thermal_cap",
      thermalStatus,
      capacityCap,
      startShortSideCap,
    });
  }

  /** thermal 상한 해제(edge에서만 1회). */
  recordThermalCapCleared(thermalStatus: number): Promise<void> {
    return this.append({
      ts: this.ports.epochMs(),
      event: "thermal_cap_cleared",
      thermalStatus,
    });
  }

  /** AppState 구독을 시작하고 구독 해제 함수를 돌려준다. */
  startLifecycle(): () => void {
    let previous = this.ports.currentState();
    return this.ports.subscribeAppState((state) => {
      if (state === previous) return;
      const event = lifecycleEvent(state);
      previous = state;
      if (!event) return;
      return this.append({ ts: this.ports.epochMs(), event });
    });
  }

  async summary(): Promise<Record<string, number> | null> {
    const native = this.ports.native;
    if (!native) return null;
    return native.getSummary();
  }

  async exportToDownloads(): Promise<string | null> {
    const native = this.ports.native;
    if (!native) return null;
    return native.exportToDownloads();
  }

  private async append(record: Omit<DiagnosticsSessionRecord, "upMs">): Promise<void> {
    const native = this.ports.native;
    if (!native) return;
    try {
      // up_ms의 원본은 네이티브 시계(SystemClock)다 — 재부팅 증명의 기준.
      const upMs = await this.ports.uptimeMs().catch(() => 0);
      await native.appendRecord(formatSession({ ...record, upMs }));
    } catch {
      // 진단 실패는 조용히 버린다.
    }
  }
}

function lifecycleEvent(state: string): DiagnosticsSessionEvent | null {
  if (state === "active") return "foreground";
  if (state === "background") return "background";
  return null;
}

/** 네이티브가 없는 환경(웹·테스트 빌드)용 no-op 싱글턴. */
export const diagnosticsRecorder = new DiagnosticsRecorder({
  native: null,
  epochMs: () => Date.now(),
  uptimeMs: async () => 0,
  subscribeAppState: () => () => undefined,
  currentState: () => "unknown",
});
