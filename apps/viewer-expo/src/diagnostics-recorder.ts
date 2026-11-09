import { DiagnosticsRecorder } from "./diagnostics-recorder.core";

export {
  DiagnosticsRecorder,
  type AppStateListener,
  type DiagnosticsRecorderNative,
  type DiagnosticsRecorderPorts,
} from "./diagnostics-recorder.core";

/** 네이티브가 없는 환경(웹·테스트 빌드)용 no-op 싱글턴. */
export const diagnosticsRecorder = new DiagnosticsRecorder({
  native: null,
  epochMs: () => Date.now(),
  uptimeMs: async () => 0,
  subscribeAppState: () => () => undefined,
  currentState: () => "unknown",
});
