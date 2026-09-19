/**
 * 비네이티브 플랫폼(테스트·웹 미리보기)용 폴백 — 창 크기 보고 이벤트는
 * Android 네이티브에서만 발생한다.
 */
import type {
  WindowSizeReport,
  WindowSizeSubscription,
} from "./stream-window-size.native";

export type { WindowSizeReport, WindowSizeSubscription };

export function subscribeWindowSizeChanged(
  _listener: (report: WindowSizeReport) => void,
): WindowSizeSubscription {
  return { remove() {} };
}
