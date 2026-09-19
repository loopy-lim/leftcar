import { DeviceEventEmitter } from "react-native";

/** XR 스트림 창이 핸들 리사이즈를 마쳤을 때 네이티브가 보내는 보고. */
export interface WindowSizeReport {
  port: number;
  widthPx: number;
  heightPx: number;
}

export type WindowSizeSubscription = { remove(): void };

export function subscribeWindowSizeChanged(
  listener: (report: WindowSizeReport) => void,
): WindowSizeSubscription {
  return (
    DeviceEventEmitter?.addListener("leftcarWindowSizeChanged", listener) ?? {
      remove() {},
    }
  );
}
