import { DeviceEventEmitter } from "react-native";
import { subscribeStreamTermination as subscribeWithoutNative } from "./stream-termination-policy";
export {
  claimStreamRestore,
  reduceRestartFailure,
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
  return DeviceEventEmitter?.addListener(
    "leftcarStreamTerminated",
    listener,
  ) ?? subscribeWithoutNative(listener);
}
