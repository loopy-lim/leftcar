import { DeviceEventEmitter, NativeModules } from "react-native";

/** 스트림 창의 입력 잠금 배너를 탭했을 때 네이티브가 보내는 보고 —
 * JS가 requestInputEnable 제어 명령으로 호스트에 승인 요청을 전달한다. */
export interface InputEnableRequestEvent {
  port: number;
  instanceId: string;
  generation: string;
  requestId: string;
}

/** The native side rejects replies to replaced windows or superseded requests. */
export function reportInputEnableRequestResult(
  event: InputEnableRequestEvent,
  error: string | null,
): Promise<void> {
  const native = NativeModules.StreamLauncher as
    | {
        reportInputRequestResult?(
          instanceId: string,
          generation: string,
          requestId: string,
          error: string | null,
        ): Promise<void>;
      }
    | undefined;
  return (
    native?.reportInputRequestResult?.(
      event.instanceId,
      event.generation,
      event.requestId,
      error,
    ) ?? Promise.resolve()
  );
}

export type InputEnableRequestSubscription = { remove(): void };

export function subscribeInputEnableRequested(
  listener: (event: InputEnableRequestEvent) => void,
): InputEnableRequestSubscription {
  return (
    DeviceEventEmitter?.addListener(
      "leftcarInputEnableRequested",
      listener,
    ) ?? {
      remove() {},
    }
  );
}
