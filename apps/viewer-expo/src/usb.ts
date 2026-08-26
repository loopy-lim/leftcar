import "./usb-runtime";

interface UsbRuntime {
  getUsbNative(): { getAccessoryState(): Promise<UsbAccessoryState> } | undefined;
  subscribeUsbNative(listener: (state: UsbAccessoryState) => void): { remove: () => void };
}

declare global {
  var __leftcarUsbRuntime: UsbRuntime | undefined;
}

function usbRuntime(): UsbRuntime {
  return globalThis.__leftcarUsbRuntime ?? {
    getUsbNative: () => undefined,
    subscribeUsbNative: () => ({ remove: () => undefined }),
  };
}

export interface UsbAccessoryState {
  attached: boolean;
  controlPort: number;
  accessoryPresent?: boolean;
  permissionPending?: boolean;
}

export type ResolvedTransport = "usb" | "udp" | "tcp" | "adbTcp";

export function resolveTransport(
  state: Pick<UsbAccessoryState, "attached">,
  requested = "auto",
): ResolvedTransport {
  const normalized = requested.trim().toLowerCase();
  // USB is the product default when present, even when the catalog supplied
  // its ordinary Wi-Fi default (`udp`). Without USB, use the low-latency UDP
  // media path; its recovery/FEC/ABR policy is responsible for handling loss.
  // Explicit TCP/UDP/ADB TCP requests remain explicit so diagnostics and
  // compatibility tools cannot be rewritten.
  if (state.attached && (normalized === "auto" || normalized === "udp" || normalized === "wifi")) {
    return "usb";
  }
  if (normalized === "usb" || normalized === "aoap") return "usb";
  if (normalized === "tcp" || normalized === "wifitcp" || normalized === "wifi-tcp") {
    return "tcp";
  }
  if (normalized === "adbtcp" || normalized === "adb-tcp") return "adbTcp";
  if (normalized === "udp" || normalized === "wifi") return "udp";
  return state.attached ? "usb" : "udp";
}

export async function getUsbState(): Promise<UsbAccessoryState> {
  const native = usbRuntime().getUsbNative();
  if (!native) return { attached: false, controlPort: 0 };
  try {
    const state = await native.getAccessoryState();
    return {
      attached: state.attached === true
        && Number.isInteger(state.controlPort)
        && state.controlPort > 0,
      controlPort: Number.isInteger(state.controlPort) ? state.controlPort : 0,
      accessoryPresent: state.accessoryPresent === true,
      permissionPending: state.permissionPending === true,
    };
  } catch {
    return { attached: false, controlPort: 0 };
  }
}

export function subscribeUsbState(
  listener: (state: UsbAccessoryState) => void,
): { remove: () => void } {
  return usbRuntime().subscribeUsbNative(listener);
}
