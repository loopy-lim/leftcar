import { AppState, NativeModules } from "react-native";
// 같은 basename(`./diagnostics-recorder`)을 여기서 import하면 Metro 플랫폼
// 해석이 이 파일 자신을 되돌려 순환 참조가 된다. 공용 클래스는 core에서.
import {
  DiagnosticsRecorder,
  type DiagnosticsRecorderNative,
} from "./diagnostics-recorder.core";

export {
  DiagnosticsRecorder,
  type AppStateListener,
  type DiagnosticsRecorderNative,
  type DiagnosticsRecorderPorts,
} from "./diagnostics-recorder.core";

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
