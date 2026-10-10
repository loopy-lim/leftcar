import { NativeEventEmitter, NativeModules } from "react-native";
import { KmSession, type KmNative, type KmNativeStatus } from "./km-session";
import { openKmControl } from "./session";
interface KmNativeModule extends KmNative {
  requestShizukuPermission(): Promise<void>;
  openAccessibilitySettings(): Promise<void>;
}
export const kmNative = NativeModules.LeftcarKm as KmNativeModule | undefined;
export const kmSession = kmNative ? new KmSession({ native: kmNative, openPeer: openKmControl }) : null;
export function subscribeKmStatus(listener: (status: KmNativeStatus) => void): () => void {
  if (!kmNative) return () => undefined;
  const emitter = new NativeEventEmitter(kmNative as never);
  const subscription = emitter.addListener("leftcarKmStatus", listener);
  return () => subscription.remove();
}
