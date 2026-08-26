import type { ControlClient } from "./control";
import {
  getUsbState,
  resolveTransport,
  type ResolvedTransport,
  type UsbAccessoryState,
} from "./usb";
import type { StreamContentMode } from "./stream-profile";

export interface StreamLauncher {
  getLocalIpv4Addresses?(): Promise<string[]>;
  prepareStream(port: number, host: string, mediaTransport: string): Promise<void>;
  openStream(
    port: number,
    host: string,
    width: number,
    height: number,
    fps: number,
  ): Promise<string>;
  cancelPreparedStream(port: number): Promise<void>;
}

export interface StartStreamArgs {
  sourceIndex: number;
  viewerPort: number;
  width: number;
  height: number;
  fps: number;
  captureBackend: string;
  mediaTransport: "udp" | "adbTcp" | "auto" | string;
  contentMode?: StreamContentMode;
  viewerIps?: string[];
}

export interface StartedStream {
  session: number;
  viewerIps: string[];
  mediaTransport: ResolvedTransport;
}

export type StreamControlRequest = <T>(command: string, args?: unknown) => Promise<T>;

interface StartPreparedStreamInput {
  control: ControlClient;
  request?: StreamControlRequest;
  launcher: StreamLauncher;
  host: string;
  args: StartStreamArgs;
}

const USB_ATTACH_TIMEOUT_MS = 5_000;
const USB_ATTACH_POLL_MS = 100;

async function waitForUsbAccessory(initial: UsbAccessoryState): Promise<UsbAccessoryState> {
  let state = initial;
  const deadline = Date.now() + USB_ATTACH_TIMEOUT_MS;
  while (!state.attached && Date.now() < deadline) {
    await new Promise<void>((resolve) => setTimeout(resolve, USB_ATTACH_POLL_MS));
    state = await getUsbState();
  }
  return state;
}

/**
 * Bind the viewer port before Host reachability proof, then display the native
 * window only after the Host has created a real capture session. A failure at
 * either boundary rolls both sides back instead of leaving a black document.
 */
export async function startPreparedStream({
  control,
  request = control.request.bind(control),
  launcher,
  host,
  args,
}: StartPreparedStreamInput): Promise<StartedStream> {
  let session: number | null = null;
  try {
    const discoveredIps = launcher.getLocalIpv4Addresses
      ? await launcher.getLocalIpv4Addresses().catch(() => [])
      : [];
    const viewerIps = [...new Set(discoveredIps)]
      .filter((address) => typeof address === "string" && address.length > 0)
      .slice(0, 4);
    let usbState = await getUsbState();
    const requestedTransport = args.mediaTransport.trim().toLowerCase();
    let usbRequestError: unknown;
    if (
      !usbState.attached &&
      (requestedTransport === "auto" || requestedTransport === "usb" || requestedTransport === "aoap")
    ) {
      try {
        await request("requestUsb");
      } catch (error) {
        usbRequestError = error;
      }
      if (usbRequestError === undefined) {
        usbState = await waitForUsbAccessory(usbState);
      }
      if (!usbState.attached && requestedTransport !== "auto") {
        const detail = usbRequestError instanceof Error
          ? `: ${usbRequestError.message}`
          : "";
        throw new Error(`USB 액세서리 권한을 허용하지 않아 USB 스트림을 시작하지 못했습니다${detail}`);
      }
    }
    const mediaTransport = resolveTransport(usbState, args.mediaTransport);
    const startArgs = {
      ...(viewerIps.length > 0 ? { ...args, viewerIps } : args),
      mediaTransport,
    };
    await launcher.prepareStream(args.viewerPort, host, mediaTransport);
    const started = await request<{ session: number }>("startStream", startArgs);
    session = started.session;
    await launcher.openStream(
      args.viewerPort,
      host,
      args.width,
      args.height,
      args.fps,
    );
    return { session, viewerIps, mediaTransport };
  } catch (error) {
    if (session !== null) {
      await request("stopStream", { session }).catch(() => undefined);
    }
    await launcher.cancelPreparedStream(args.viewerPort).catch(() => undefined);
    throw error;
  }
}
