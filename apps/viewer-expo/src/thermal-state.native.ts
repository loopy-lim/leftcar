import { NativeModules } from "react-native";

import { diagnosticsRecorder } from "./diagnostics-recorder";
import {
  ThermalStateMonitor,
  type ThermalStateNative,
} from "./thermal-state";

const diagnostics = NativeModules.Diagnostics as
  | (ThermalStateNative & Record<string, unknown>)
  | undefined;

export const thermalState = new ThermalStateMonitor({
  native: diagnostics ?? null,
  epochMs: () => Date.now(),
  recorder: diagnosticsRecorder,
});
