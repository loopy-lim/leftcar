import type { ControlClient } from "./control";
import {
  getUsbState,
  resolveTransport,
  type ResolvedTransport,
  type UsbAccessoryState,
} from "./usb";
import type { StreamContentMode } from "./stream-profile";
import {
  resolveEncoderExperimentForStream,
  type EncoderExperimentId,
} from "./encoder-experiment";
import {
  VIEWER_UDP_CAPABILITIES,
  availableUdpStabilityOptions,
  resolveUdpStabilitySelection,
  type UdpStabilitySelection,
} from "./udp-stability";

export interface StreamLauncher {
  getLocalIpv4Addresses?(): Promise<string[]>;
  prepareStream(
    port: number,
    host: string,
    mediaTransport: string,
    encoderExperiment: EncoderExperimentId,
  ): Promise<void>;
  openStream(
    port: number,
    host: string,
    width: number,
    height: number,
    fps: number,
    encoderExperiment: EncoderExperimentId,
    displayName?: string,
    showFps?: boolean,
  ): Promise<string>;
  cancelPreparedStream(
    port: number,
    encoderExperiment: EncoderExperimentId,
  ): Promise<void>;
}

export interface StartStreamArgs {
  sourceIndex: number;
  viewerPort: number;
  width: number;
  height: number;
  fps: number;
  captureBackend: string;
  mediaTransport: "udp" | "adbTcp" | "auto" | string;
  encoderExperiment: EncoderExperimentId;
  displayName?: string;
  showFps?: boolean;
  contentMode?: StreamContentMode;
  viewerIps?: string[];
  udpStability?: UdpStabilitySelection;
}

export interface StartedStream {
  session: number;
  viewerIps: string[];
  mediaTransport: ResolvedTransport;
  encoderExperiment: EncoderExperimentId;
  udpStability?: UdpStabilitySelection;
}

export interface RestartedStreamState extends StartedStream {
  captureBackend: string;
}

export function replaceRestartedStreamState<
  T extends RestartedStreamState & { startedAt: number },
>(
  streams: readonly T[],
  previousSession: number,
  restarted: RestartedStreamState,
  startedAt: number,
  endUnownedRestart?: (session: number) => void,
): T[] {
  let replaced = false;
  const next = streams.map((stream) => {
    if (stream.session !== previousSession) return stream;
    replaced = true;
    return { ...stream, ...restarted, startedAt };
  });
  if (!replaced) endUnownedRestart?.(restarted.session);
  return next;
}

export type StreamControlRequest = <T>(command: string, args?: unknown) => Promise<T>;

interface StartPreparedStreamInput {
  control: ControlClient;
  request?: StreamControlRequest;
  launcher: StreamLauncher;
  host: string;
  advertisedEncoderExperiments: unknown;
  advertisedUdpStabilityCapabilities?: unknown;
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
  advertisedEncoderExperiments,
  advertisedUdpStabilityCapabilities,
  args,
}: StartPreparedStreamInput): Promise<StartedStream> {
  let session: number | null = null;
  const encoderExperiment = resolveEncoderExperimentForStream(
    args.encoderExperiment,
    advertisedEncoderExperiments,
    args.width,
    args.height,
  );
  try {
    const discoveredIps = launcher.getLocalIpv4Addresses
      ? await launcher.getLocalIpv4Addresses().catch(() => [])
      : [];
    const viewerIps = [...new Set(discoveredIps)]
      .filter((address) => typeof address === "string" && address.length > 0)
      .slice(0, 4);
    let usbState = await getUsbState();
    const requestedTransport = encoderExperiment === "splitVertical"
      ? "udp"
      : args.mediaTransport.trim().toLowerCase();
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
    const mediaTransport = resolveTransport(usbState, requestedTransport);
    const udpOptions = availableUdpStabilityOptions(
      advertisedUdpStabilityCapabilities,
    );
    const udpStability = args.udpStability
      ? resolveUdpStabilitySelection(
          args.udpStability,
          advertisedUdpStabilityCapabilities,
        )
      : null;
    if (args.udpStability && udpOptions && !udpStability) {
      throw new Error("선택한 UDP 안정성 설정을 이 컴퓨터에서 지원하지 않습니다.");
    }
    const { udpStability: _requestedUdpStability, ...baseArgs } = args;
    const startArgs = {
      ...baseArgs,
      ...(viewerIps.length > 0 ? { viewerIps } : {}),
      mediaTransport,
      encoderExperiment,
      ...(mediaTransport === "udp" && udpStability
        ? {
            udpStability: {
              ...udpStability,
              viewer: VIEWER_UDP_CAPABILITIES,
            },
          }
        : {}),
    };
    await launcher.prepareStream(
      args.viewerPort,
      host,
      mediaTransport,
      encoderExperiment,
    );
    const started = await request<{ session: number }>("startStream", startArgs);
    session = started.session;
    await launcher.openStream(
      args.viewerPort,
      host,
      args.width,
      args.height,
      args.fps,
      encoderExperiment,
      args.displayName,
      args.showFps ?? true,
    );
    return {
      session,
      viewerIps,
      mediaTransport,
      encoderExperiment,
      ...(udpStability ? { udpStability } : {}),
    };
  } catch (error) {
    if (session !== null) {
      await request("stopStream", { session }).catch(() => undefined);
    }
    await launcher
      .cancelPreparedStream(args.viewerPort, encoderExperiment)
      .catch(() => undefined);
    throw error;
  }
}
