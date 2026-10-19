import { AppState, NativeModules } from "react-native";
import {
  DiagnosticsRecorder,
  type DiagnosticsRecorderNative,
} from "./diagnostics-recorder";

const native = NativeModules.Diagnostics as DiagnosticsRecorderNative | undefined;

export const diagnosticsRecorder = new DiagnosticsRecorder({
  native: native ?? null,
  epochMs: () => Date.now(),
  uptimeMs: () => (native ? native.uptimeMs() : Promise.resolve(0)),
  subscribeAppState: (listener) => {
    const subscription = AppState.addEventListener("change", listener);
    return () => subscription.remove();
  },
  currentState: () => AppState.currentState ?? "unknown",
});
