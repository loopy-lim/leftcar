import {
  ThermalStateMonitor,
  type ThermalStatePorts,
} from "./thermal-state.core";

export {
  ThermalStateMonitor,
  type ThermalCaps,
  type ThermalNativeSample,
  type ThermalStateNative,
  type ThermalStatePorts,
} from "./thermal-state.core";

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
