import { DeviceEventEmitter } from "react-native";

/** 스트림 창의 입력 잠금 배너를 탭했을 때 네이티브가 보내는 보고 —
 * JS가 requestInputEnable 제어 명령으로 호스트에 승인 요청을 전달한다. */
export interface InputEnableRequestEvent {
  port: number;
}

export type InputEnableRequestSubscription = { remove(): void };

export function subscribeInputEnableRequested(
  listener: (event: InputEnableRequestEvent) => void,
): InputEnableRequestSubscription {
  return (
    DeviceEventEmitter?.addListener("leftcarInputEnableRequested", listener) ?? {
      remove() {},
    }
  );
}
