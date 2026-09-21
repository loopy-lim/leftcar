/**
 * 비네이티브 플랫폼(테스트·웹 미리보기)용 폴백 — 입력 허용 요청 이벤트는
 * Android 네이티브의 잠금 배너 탭에서만 발생한다.
 */
import type {
  InputEnableRequestEvent,
  InputEnableRequestSubscription,
} from "./stream-input-request.native";

export type { InputEnableRequestEvent, InputEnableRequestSubscription };

export function subscribeInputEnableRequested(
  _listener: (event: InputEnableRequestEvent) => void,
): InputEnableRequestSubscription {
  return { remove() {} };
}
