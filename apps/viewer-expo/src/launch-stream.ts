import type { ControlClient, ReconfigureStreamOutput } from "./control";
import { currentLanguage } from "./language-store";
import { LocalizedError } from "./localized-error";
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
  availableEncoderExperiments,
  resolveEncoderExperimentForStream,
  selectAutomaticEncoderExperiment,
  type EncoderExperimentId,
} from "./encoder-experiment";
import { STREAM_TARGET_FPS } from "./streaming-policy";
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
    language?: string,
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
    language?: string,
    localAudio?: boolean,
  ): Promise<string>;
  cancelPreparedStream(
    port: number,
    encoderExperiment: EncoderExperimentId,
  ): Promise<void>;
  setCursorStream?(instanceId: string, enabled: boolean): Promise<void>;
  /**
   * 활성 스트림 창의 시스템 소리 전달(SNDON/SNDOFF)을 켜고 끈다. 네이티브
   * 모듈이 구버전이면 setAudioStream가 없을 수 있고, 호출부는 best-effort로
   * 무시한다.
   */
  setAudioStream?(instanceId: string, enabled: boolean): Promise<void>;
  /**
   * XR 창 비율 프리셋을 활성 스트림 창에 적용한다. 컴퓨터 화면 해상도는
   * 변경하지 않는다. 네이티브 모듈이 없거나 XR이 아닌 기기에서는 실패하며,
   * 호출부는 best-effort로 이를 무시한다.
   */
  setWindowAspectRatio?(instanceId: string, ratio: number): Promise<void>;
  /**
   * XR 창 비율 프리셋 지원 여부. StreamActivity의 XR 검사와 같은 시스템
   * 피처를 본다. 구버전 네이티브 모듈엔 없을 수 있고, 호출부는 그 경우
   * 기존처럼 비율 행을 보여 준다.
   */
  isXrWindowRatioSupported?(): Promise<boolean>;
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
  localAudio?: boolean;
  contentMode?: StreamContentMode;
  viewerIps?: string[];
  udpStability?: UdpStabilitySelection;
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
      throw new LocalizedError("errUdpUnsupported");
    }
    try {
      await launcher.prepareStream(
        args.viewerPort,
        host,
        mediaTransport,
        encoderExperiment,
        currentLanguage(),
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
        currentLanguage(),
      );
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
      args.showFps ?? false,
      // 커서 오버레이는 입력 피드백이라 기본(true) — 네이티브 기본값과 정합.
      args.localCursor ?? true,
      currentLanguage(),
      // 오디오는 기본 전달(true) — 네이티브 기본값과 정합.
      args.localAudio ?? true,
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
  /**
   * Catalog capability `reconfigureEncoderExperiment`. When true the Host
   * accepts an optional `encoderExperiment` request and reports the actually
   * accepted mode. Without it the viewer must never request a mode transition:
   * older hosts cannot honor one mid-session.
   */
  reconfigureEncoderExperiment?: boolean;
  /** Catalog `encoderExperiments` advertisement, for availability checks. */
  advertisedEncoderExperiments?: unknown;
}

/**
 * Encoder mode the reconfigure should run under. Demotion (split at a
 * non-4K target) falls back to the automatic single path and stays on the
 * legacy omission wire shape. A promotion (auto → splitVertical at exact 4K
 * 60fps) requires the capability, an advertised split experiment, the UDP
 * transport, and an automatic (not user-pinned) current mode. 90/30fps 4K
 * targets stay on the single path: the split pair is sized for the 4K60
 * contract only.
 */
function resolveReconfigureExperiment(
  active: ActiveStream,
  target: AdaptiveTarget,
  input: Pick<
    ReconfigurePreparedStreamInput,
    "reconfigureEncoderExperiment" | "advertisedEncoderExperiments"
  >,
): EncoderExperimentId {
  if (active.encoderExperiment === "splitVertical" && !isExact4K(target)) {
    return "auto";
  }
  const promote = input.reconfigureEncoderExperiment === true &&
    active.encoderExperiment === "auto" &&
    active.mediaTransport === "udp" &&
    isExact4K(target) &&
    target.fps === STREAM_TARGET_FPS &&
    availableEncoderExperiments(input.advertisedEncoderExperiments, 3840, 2160)
      .some((experiment) => experiment.id === "splitVertical");
  return promote ? "splitVertical" : active.encoderExperiment;
}

export async function reconfigurePreparedStream({
  control,
  launcher,
  host,
  active,
  target,
  qualityState,
  reconfigureEncoderExperiment,
  advertisedEncoderExperiments,
}: ReconfigurePreparedStreamInput): Promise<StartedStream> {
  const desiredExperiment = resolveReconfigureExperiment(active, target, {
    reconfigureEncoderExperiment,
    advertisedEncoderExperiments,
  });
  // Only a capability-backed split promotion is requested explicitly; demotion
  // and same-mode retention keep the legacy omission wire shape so older
  // hosts never receive a field they do not know.
  const promotion = desiredExperiment === "splitVertical" &&
    desiredExperiment !== active.encoderExperiment;
  let preparedExperiment = desiredExperiment;
  let requestedExperiment: EncoderExperimentId | undefined = promotion
    ? desiredExperiment
    : undefined;
  try {
    await launcher.prepareStream(
      active.port,
      host,
      active.mediaTransport,
      desiredExperiment,
      currentLanguage(),
    );
  } catch (error) {
    if (!promotion) {
      throw error;
    }
    await launcher
      .cancelPreparedStream(active.port, desiredExperiment)
      .catch(() => undefined);
    preparedExperiment = "auto";
    requestedExperiment = "auto";
    await launcher.prepareStream(
      active.port,
      host,
      active.mediaTransport,
      preparedExperiment,
      currentLanguage(),
    );
  }
  try {
    const acceptedOnce = await control.request<ReconfigureStreamOutput>(
      "reconfigureStream",
      {
        session: active.session,
        width: target.width,
        height: target.height,
        fps: target.fps,
        qualityState,
        ...(requestedExperiment ? { encoderExperiment: requestedExperiment } : {}),
      },
    );
    // Open (and report) the mode the Host actually accepted — it may differ
    // from the request; capability-less hosts omit the field entirely.
    let accepted: ReconfigureStreamOutput = acceptedOnce;
    if (
      accepted.encoderExperiment === "splitVertical" &&
      accepted.encoderExperiment !== preparedExperiment
    ) {
      // The Host accepted split while the receiver was prepared for a single
      // mode. A split attach claims two fresh prepared listeners — openStream
      // assumes a matching preparation and never prepares itself. But the
      // reconfiguration above already issued its LCH1 challenge, and a
      // streaming replacement never issues a second one on its own: a split
      // preflight bound now would hand the renderer an empty token (device
      // session-3 feedback-timeout failure). So cancel the single
      // preparation, re-prepare split, and RECONFIGURE AGAIN with an explicit
      // split request — the fresh listeners capture the replacement
      // session's new challenge. Any failure lands in the catch below, which
      // cancels whatever is still prepared and propagates the error.
      await launcher
        .cancelPreparedStream(active.port, preparedExperiment)
        .catch(() => undefined);
      // A failed prepare can already own resources in the accepted mode.
      preparedExperiment = "splitVertical";
      await launcher.prepareStream(
        active.port,
        host,
        active.mediaTransport,
        preparedExperiment,
        currentLanguage(),
      );
      accepted = await control.request<ReconfigureStreamOutput>(
        "reconfigureStream",
        {
          session: active.session,
          width: target.width,
          height: target.height,
          fps: target.fps,
          qualityState,
          encoderExperiment: "splitVertical",
        },
      );
      if (
        accepted.encoderExperiment !== undefined &&
        accepted.encoderExperiment !== "splitVertical"
      ) {
        // The second request still did not land on split (e.g. the Host lost
        // the split encoder mid-transition). The freshly prepared split
        // listeners cannot serve a single stream; report the mismatch
        // instead of attaching with an empty token.
        throw new LocalizedError("errSplitEncodeRejected");
      }
    }
    // An accepted single mode keeps the existing preparation: it was bound
    // before the Host reconfigured, so its worker already captured the
    // replacement session's challenge token. Re-creating the receiver now —
    // after the Host change — would hand the renderer an empty token, and a
    // streaming replacement never issues a second challenge. The single
    // attach/rebind claims this exact prepared socket. In both cases the mode
    // opened is the one the prepared listeners can actually authenticate.
    const encoderExperiment = accepted.encoderExperiment ?? preparedExperiment;
    await launcher.openStream(
      active.port,
      host,
      accepted.width,
      accepted.height,
      accepted.fps,
      encoderExperiment,
      active.sourceName,
      active.showFps ?? false,
      active.localCursor ?? true,
      currentLanguage(),
      active.localAudio ?? true,
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
      .cancelPreparedStream(active.port, preparedExperiment)
      .catch(() => undefined);
    throw error;
  }
}
