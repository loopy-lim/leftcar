import { NativeModules } from "react-native";

import { diagnosticsRecorder } from "./diagnostics-recorder";
// 같은 basename(`./thermal-state`)을 여기서 import하면 Metro 플랫폼 해석이
// 이 파일 자신을 되돌려 순환 참조가 된다. 공용 클래스는 core에서.
import {
  ThermalStateMonitor,
  type ThermalStateNative,
} from "./thermal-state.core";

export {
  ThermalStateMonitor,
  type ThermalCaps,
  type ThermalNativeSample,
  type ThermalStateNative,
  type ThermalStatePorts,
} from "./thermal-state.core";

const diagnostics = NativeModules.Diagnostics as
  | (ThermalStateNative & Record<string, unknown>)
  | undefined;

export const thermalState = new ThermalStateMonitor({
  native: diagnostics ?? null,
  epochMs: () => Date.now(),
  recorder: diagnosticsRecorder,
});
