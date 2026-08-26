import { DeviceEventEmitter, NativeModules } from "react-native";
import type { UsbAccessoryState } from "./usb";

globalThis.__leftcarUsbRuntime = {
  getUsbNative() {
    return NativeModules.UsbAccessory as
      | { getAccessoryState(): Promise<UsbAccessoryState> }
      | undefined;
  },
  subscribeUsbNative(listener: (state: UsbAccessoryState) => void) {
    return DeviceEventEmitter.addListener("leftcarUsbState", listener);
  },
};
