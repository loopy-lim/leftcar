import { DeviceEventEmitter, NativeModules } from "react-native";
import { subscribeStreamTermination as subscribeWithoutNative } from "./stream-termination-policy";
export {
  claimStreamRestore,
  classifyHostTermination,
  releaseStreamRestore,
  selectRecoverableStream,
  type LocalStreamTerminationReason,
  type RestartRequest,
  type StreamTerminationEvent,
  type StreamTerminationSubscription,
} from "./stream-termination-policy";
import type {
  StreamTerminationEvent,
  StreamTerminationSubscription,
} from "./stream-termination-policy";

export function subscribeStreamTermination(
  listener: (event: StreamTerminationEvent) => void,
): StreamTerminationSubscription {
  const eventName = "leftcarStreamTerminated";
  // Install JS first: native addListener may immediately flush one pending
  // termination emitted while the React bridge was not available.
  const subscription = DeviceEventEmitter?.addListener(eventName, listener);
  if (!subscription) return subscribeWithoutNative(listener);
  const native = NativeModules.StreamLauncher;
  native?.addListener?.(eventName);
  let removed = false;
  return {
    remove() {
      if (removed) return;
      removed = true;
      subscription.remove();
      native?.removeListeners?.(1);
    },
  };
}
