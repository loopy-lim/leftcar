import type { ControlClient, ReconfigureStreamOutput } from "./control";
import {
  getUsbState,
  resolveTransport,
  type ResolvedTransport,
  type UsbAccessoryState,
} from "./usb";
import type { StreamContentMode } from "./stream-profile";
import type { ActiveStream } from "./catalog-model-types";
import {
  isExact4K,
  type AdaptiveQualityState,
  type AdaptiveTarget,
} from "./adaptive-resolution";
import {
  resolveEncoderExperimentForStream,
  selectAutomaticEncoderExperiment,
  type EncoderExperimentId,
} from "./encoder-experiment";
import {
  VIEWER_UDP_CAPABILITIES,
  availableUdpStabilityOptions,
  resolveUdpStabilitySelection,
  type UdpStabilitySelection,
} from "./udp-stability";

/**
 * Physical metrics of the viewer's own screen, reported at stream start so
 * the Host can size a virtual display to match the tablet.
 */
export interface ViewerDisplayMetrics {
  physicalWidth: number;
  physicalHeight: number;
  densityDpi: number;
}

export interface StreamLauncher {
  getLocalIpv4Addresses?(): Promise<string[]>;
  getDisplayMetrics?(): Promise<ViewerDisplayMetrics>;
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
    localCursor?: boolean,
  ): Promise<string>;
  cancelPreparedStream(
    port: number,
    encoderExperiment: EncoderExperimentId,
  ): Promise<void>;
  setCursorStream?(instanceId: string, enabled: boolean): Promise<void>;
  /**
   * XR 창 비율 프리셋을 활성 스트림 창에 적용한다. Mac 가상 화면 해상도는
   * 변경하지 않는다. 네이티브 모듈이 없거나 XR이 아닌 기기에서는 실패하며,
   * 호출부는 best-effort로 이를 무시한다.
   */
  setWindowAspectRatio?(instanceId: string, ratio: number): Promise<void>;
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
  localCursor?: boolean;
  contentMode?: StreamContentMode;
  viewerIps?: string[];
  udpStability?: UdpStabilitySelection;
  viewerDisplay?: ViewerDisplayMetrics;
}

export interface StartedStream {
  session: number;
  width?: number;
  height?: number;
  fps?: number;
  qualityState?: AdaptiveQualityState;
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
  const selectedEncoderExperiment = resolveEncoderExperimentForStream(
    args.encoderExperiment,
    advertisedEncoderExperiments,
    args.width,
    args.height,
  );
  let encoderExperiment = selectedEncoderExperiment;
  try {
    const discoveredIps = launcher.getLocalIpv4Addresses
      ? await launcher.getLocalIpv4Addresses().catch(() => [])
      : [];
    const viewerIps = [...new Set(discoveredIps)]
      .filter((address) => typeof address === "string" && address.length > 0)
      .slice(0, 4);
    let usbState = await getUsbState();
    const requestedTransport = selectedEncoderExperiment === "splitVertical"
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
    const resolvedMediaTransport = resolveTransport(usbState, requestedTransport);
    encoderExperiment = selectAutomaticEncoderExperiment(
      selectedEncoderExperiment,
      advertisedEncoderExperiments,
      args.width,
      args.height,
      resolvedMediaTransport,
    );
    let mediaTransport = encoderExperiment === "splitVertical"
      ? "udp"
      : resolvedMediaTransport;
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
    try {
      await launcher.prepareStream(
        args.viewerPort,
        host,
        mediaTransport,
        encoderExperiment,
      );
    } catch (error) {
      const canFallBackToSingleEncoder = selectedEncoderExperiment === "auto" &&
        encoderExperiment === "splitVertical";
      if (!canFallBackToSingleEncoder) {
        throw error;
      }
      await launcher
        .cancelPreparedStream(args.viewerPort, encoderExperiment)
        .catch(() => undefined);
      encoderExperiment = "auto";
      mediaTransport = resolvedMediaTransport;
      await launcher.prepareStream(
        args.viewerPort,
        host,
        mediaTransport,
        encoderExperiment,
      );
    }
    const { udpStability: _requestedUdpStability, ...baseArgs } = args;
    const startArgs = {
      ...baseArgs,
      ...(viewerIps.length > 0 ? { viewerIps } : {}),
      // Omitted entirely for legacy Hosts — the contract treats a missing
      // field as the historical wire shape.
      ...(args.viewerDisplay ? { viewerDisplay: args.viewerDisplay } : {}),
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
    const started = await request<{
      session: number;
      width?: number;
      height?: number;
      fps?: number;
      qualityState?: AdaptiveQualityState;
    }>("startStream", startArgs);
    session = started.session;
    const width = started.width ?? args.width;
    const height = started.height ?? args.height;
    const fps = started.fps ?? args.fps;
    await launcher.openStream(
      args.viewerPort,
      host,
      width,
      height,
      fps,
      encoderExperiment,
      args.displayName,
      args.showFps ?? true,
      // 미옵트인 기본(false)과 정합 — 네이티브 인자 수 계약을 채우는 파이프.
      args.localCursor ?? false,
    );
    return {
      session,
      // Optional fields stay undefined when the Host omitted them;
      // JSON drops undefined keys on the wire.
      width: started.width,
      height: started.height,
      fps: started.fps,
      qualityState: started.qualityState,
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

interface ReconfigurePreparedStreamInput {
  control: ControlClient;
  launcher: StreamLauncher;
  host: string;
  active: ActiveStream;
  target: AdaptiveTarget;
  qualityState: AdaptiveQualityState;
}

export async function reconfigurePreparedStream({
  control,
  launcher,
  host,
  active,
  target,
  qualityState,
}: ReconfigurePreparedStreamInput): Promise<StartedStream> {
  // splitVertical requires an exact-4K source; a demoted target falls back
  // to the automatic single-encoder path.
  const encoderExperiment = active.encoderExperiment === "splitVertical" &&
    !isExact4K(target)
    ? "auto"
    : active.encoderExperiment;
  await launcher.prepareStream(
    active.port,
    host,
    active.mediaTransport,
    encoderExperiment,
  );
  try {
    const accepted = await control.request<ReconfigureStreamOutput>(
      "reconfigureStream",
      {
        session: active.session,
        width: target.width,
        height: target.height,
        fps: target.fps,
        qualityState,
      },
    );
    await launcher.openStream(
      active.port,
      host,
      accepted.width,
      accepted.height,
      accepted.fps,
      encoderExperiment,
      active.sourceName,
      active.showFps ?? true,
      active.localCursor ?? false,
    );
    return {
      session: accepted.session,
      width: accepted.width,
      height: accepted.height,
      fps: accepted.fps,
      qualityState: accepted.qualityState,
      viewerIps: active.viewerIps,
      mediaTransport: active.mediaTransport,
      encoderExperiment,
      udpStability: active.udpStability,
    };
  } catch (error) {
    await launcher
      .cancelPreparedStream(active.port, encoderExperiment)
      .catch(() => undefined);
    throw error;
  }
}
