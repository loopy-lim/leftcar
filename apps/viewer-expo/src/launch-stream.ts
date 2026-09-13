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
  SPLIT_4K_HEIGHT,
  SPLIT_4K_WIDTH,
  type AdaptiveQualityState,
  type AdaptiveTarget,
} from "./adaptive-resolution";
import {
  availableEncoderExperiments,
  resolveEncoderExperimentForStream,
  selectAutomaticEncoderExperiment,
  type EncoderExperimentId,
} from "./encoder-experiment";
import { randomBytes, bytesToBase64Url } from "./secure-channel";
import { STREAM_TARGET_FPS } from "./streaming-policy";
import {
  VIEWER_UDP_CAPABILITIES,
  availableUdpStabilityOptions,
  resolveUdpStabilitySelection,
  type UdpStabilitySelection,
} from "./udp-stability";
import {
  planAdmission,
  type AdmissionStream,
  type DecoderDemand,
  type DecoderCapabilityHint,
} from "./decoder-budget";

export interface StreamLauncher {
  getDecoderCapabilityHint?(): Promise<DecoderCapabilityHint>;
  setBalancedPresentation?(instanceId: string, enabled: boolean): Promise<void>;
  /** Exact native Activity incarnation; empty means no opened renderer. */
  getStreamGeneration?(instanceId: string): Promise<string>;
  /** Resolves only after this incarnation's native decoder cleanup finishes. */
  closeStream?(instanceId: string, generation: string): Promise<void>;
  getLocalIpv4Addresses?(): Promise<string[]>;
  prepareStream(
    port: number,
    host: string,
    mediaTransport: string,
    encoderExperiment: EncoderExperimentId,
    language?: string,
    /** 뷰어가 생성한 32바이트 세션 미디어 키(base64url). 네이티브 리스너가
     * 봉인된 LCH1 도전에 startStream 이전에 응답할 수 있게 한다. 네이티브
     * 계층은 키가 없으면 fail-closed이므로 런처는 항상 전달해야 한다. */
    mediaKey?: string,
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
  openStreamWithPresentation?(
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
    balancedPresentation?: boolean,
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
  setOpusAudio?(instanceId: string, enabled: boolean): Promise<void>;
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

/** Negotiate the additive native method; old modules keep their exact arity. */
async function openConfiguredStream(launcher: StreamLauncher,
  ...args: Parameters<NonNullable<StreamLauncher["openStreamWithPresentation"]>>
): Promise<void> {
  if (launcher.openStreamWithPresentation) {
    await launcher.openStreamWithPresentation(...args);
  } else {
    const [port,host,width,height,fps,encoder,display,showFps,cursor,language,audio] = args;
    await launcher.openStream(port,host,width,height,fps,encoder,display,showFps,cursor,language,audio);
  }
}

export interface StartStreamArgs {
  sourceId?: string;
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
  opusAudio?: boolean;
  balancedPresentation?: boolean;
  contentMode?: StreamContentMode;
  viewerIps?: string[];
  udpStability?: UdpStabilitySelection;
}

export interface StartedStream {
  opusAudio?: boolean;
  /** Effective native mode, distinct from the saved preference. */
  balancedPresentation?: boolean;
  session: number;
  /** 뷰어가 생성한 세션 미디어 키 — 재구성(reconfigure) 시 그대로 재사용된다. */
  mediaKey: string;
  width?: number;
  height?: number;
  fps?: number;
  qualityState?: AdaptiveQualityState;
  /** 소스 전환 후 실제로 스트리밍 중인 캡처 소스(호스트 에코, 전환 요청 때만). */
  sourceIndex?: number;
  sourceId?: string;
  sourceName?: string;
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

/**
 * Viewer-side prepare failures surface from the native `prepareStream` call
 * as ERR_STREAM_PREPARE. The prepare bind happens BEFORE the Host startStream
 * request exists, so when this error escapes startPreparedStream the Host has
 * no session to clean up and the caller can safely retry the launch on a
 * freshly allocated port (a stale port neighbor can still hold the old bind).
 */
export function isStreamPrepareError(cause: unknown): boolean {
  const code = (cause as { code?: unknown } | null)?.code;
  if (typeof code === "string" && code.includes("ERR_STREAM_PREPARE")) {
    return true;
  }
  return String(cause).includes("ERR_STREAM_PREPARE");
}

function assertDecoderReservation(
  reservation: DecoderDemand | undefined,
  encoderExperiment: EncoderExperimentId,
  target: AdaptiveTarget,
): void {
  if (!reservation) return;
  const exceedsInstances = encoderExperiment === "splitVertical" && !reservation.split;
  const invalidTarget = ![target.width, target.height, target.fps].every((value) => Number.isFinite(value) && value > 0);
  const pixelRate = target.width * target.height * target.fps;
  const reservedRate = reservation.target.width * reservation.target.height * reservation.target.fps;
  if (exceedsInstances || invalidTarget || pixelRate > reservedRate) {
    throw new Error("Host stream exceeds the decoder reservation");
  }
}

interface StartPreparedStreamInput {
  decoderReservation?: DecoderDemand;
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

/**
 * USB 액세서리는 스트림 시작 전에 먼저 붙을 수 있다. 봉인 경로가 유지되도록
 * 생성된 세션 키를 라이브 USB 브리지에 미리 등록한다. 네이티브 모듈이 구버전
 * 이면 이 등록이 없을 수 있고, 그때는 prepareStream 경로의 키 전달만으로
 * 동작한다(테스트 환경처럼 react-native를 못 부르는 경우 조용히 건너뛴다).
 */
function registerUsbMediaKey(mediaKey: string): void {
  try {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const { NativeModules } = require("react-native");
    const native = NativeModules?.UsbAccessory as
      | { setSessionMediaKey?(key: string): Promise<void> }
      | undefined;
    native?.setSessionMediaKey?.(mediaKey).catch(() => undefined);
  } catch {
    // react-native를 불러올 수 없는 환경(단위 테스트)에서는 생략한다.
  }
}

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
  decoderReservation,
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
    // 뷰어가 세션 미디어 키를 생성한다. 암호화된 제어 평면으로 호스트에
    // 전달되고, 네이티브 준비 리스너는 같은 키로 봉인된 도전에 응답한다.
    // USB는 액세서리가 키 생성보다 먼저 붙을 수 있으므로 라이브 브리지에
    // 같은 키를 미리 등록한다(구버전 모듈은 없을 수 있어 best-effort).
    const mediaKey = bytesToBase64Url(randomBytes(32));
    registerUsbMediaKey(mediaKey);
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
    assertDecoderReservation(decoderReservation, encoderExperiment, args);
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
        mediaKey,
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
        mediaKey,
      );
    }
    const { udpStability: _requestedUdpStability, balancedPresentation: _localPresentation, ...baseArgs } = args;
    const startArgs = {
      ...baseArgs,
      ...(viewerIps.length > 0 ? { viewerIps } : {}),
      mediaTransport,
      encoderExperiment,
      mediaKey,
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
    assertDecoderReservation(decoderReservation, encoderExperiment, {width, height, fps});
    await openConfiguredStream(launcher,
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
      args.balancedPresentation ?? false,
    );
    await launcher.setOpusAudio?.(`src-${args.viewerPort}`, args.opusAudio ?? false);
    return {
      session,
      opusAudio: Boolean(launcher.setOpusAudio && args.opusAudio),
      balancedPresentation: Boolean(launcher.openStreamWithPresentation && args.balancedPresentation),
      // Optional fields stay undefined when the Host omitted them;
      // JSON drops undefined keys on the wire.
      width: started.width,
      height: started.height,
      fps: started.fps,
      qualityState: started.qualityState,
      viewerIps,
      mediaTransport,
      encoderExperiment,
      mediaKey,
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
  request?: StreamControlRequest;
  /** Native preparation must not exceed the already-owned reservation. */
  decoderReservation?: DecoderDemand;
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
  /**
   * Target capture source for a cheap display switch (R7). Only sent when the
   * catalog capability `reconfigureSource` is present; the Host restarts the
   * capture backend on that display under the same session id while the
   * viewer address, media port, and this prepared receiver stay untouched.
   */
  sourceIndex?: number;
  sourceId?: string;
  /** Catalog capability `reconfigureSource`. Without it `sourceIndex` is ignored. */
  reconfigureSource?: boolean;
  /** Catalog `encoderExperiments` advertisement, for availability checks. */
  advertisedEncoderExperiments?: unknown;
  /**
   * Live viewer streams for the decoder-instance budget (M4/R8), including
   * `active` itself. Optional so legacy callers (tests, older hooks) keep the
   * ungated promotion behavior; the catalog hook always supplies it.
   */
  decoderBudget?: {
    currentStreams: readonly AdmissionStream[];
  };
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
export function resolveReconfigureExperiment(
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
    availableEncoderExperiments(
      input.advertisedEncoderExperiments,
      SPLIT_4K_WIDTH,
      SPLIT_4K_HEIGHT,
    )
      .some((experiment) => experiment.id === "splitVertical");
  return promote ? "splitVertical" : active.encoderExperiment;
}

export async function reconfigurePreparedStream({
  control,
  request = control.request.bind(control),
  launcher,
  host,
  active,
  target,
  qualityState,
  reconfigureEncoderExperiment,
  sourceIndex,
  sourceId,
  reconfigureSource,
  advertisedEncoderExperiments,
  decoderBudget,
  decoderReservation,
}: ReconfigurePreparedStreamInput): Promise<StartedStream> {
  // Cheap display switch (R7): a source change rides the exact same prepared
  // listener as a resolution reconfigure. The receiver is bound BEFORE the
  // reconfigure request so it captures the replacement backend's single LCH1
  // challenge; the media port never changes, so the open window just sees a
  // new IDR on the same socket. Without the host capability the field is
  // never sent — an old host would silently ignore it and keep the old source.
  const switchSource = reconfigureSource === true ? sourceIndex : undefined;
  const reconfigureArgs = (
    requestedExperiment: EncoderExperimentId | undefined,
  ) => ({
    session: active.session,
    width: target.width,
    height: target.height,
    fps: target.fps,
    qualityState,
    ...(requestedExperiment ? { encoderExperiment: requestedExperiment } : {}),
    ...(switchSource !== undefined ? { sourceIndex: switchSource, ...(sourceId ? { sourceId } : {}) } : {}),
  });
  const resolvedExperiment = resolveReconfigureExperiment(active, target, {
    reconfigureEncoderExperiment,
    advertisedEncoderExperiments,
  });
  // Read before the branch below narrows active.encoderExperiment.
  const activeIsSplit = active.encoderExperiment === "splitVertical";
  // Decoder budget (M4/R8): a split promotion claims a second decoder
  // instance (+1 net — the stream keeps its single-mode slot until the
  // replacement). Projecting over capacity refuses the promotion and keeps
  // the single path; staying single never projects over capacity because the
  // replaced slot is released first. Admission control elsewhere guarantees
  // the total never exceeds capacity without this request, so "block" is not
  // reachable here — any non-allow plan keeps single. Without a budget the
  // ungated legacy behavior is preserved.
  let desiredExperiment = resolvedExperiment;
  if (
    resolvedExperiment === "splitVertical" &&
    resolvedExperiment !== active.encoderExperiment &&
    decoderBudget !== undefined
  ) {
    const budget = planAdmission(decoderBudget.currentStreams, {
      target,
      split: true,
      replacing: { split: activeIsSplit },
    });
    if (budget.action !== "allow") {
      desiredExperiment = "auto";
    }
  }
  // Only a capability-backed split promotion is requested explicitly; demotion
  // and same-mode retention keep the legacy omission wire shape so older
  // hosts never receive a field they do not know.
  const promotion = desiredExperiment === "splitVertical" &&
    desiredExperiment !== active.encoderExperiment;
  assertDecoderReservation(decoderReservation, desiredExperiment, target);
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
      active.mediaKey,
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
      active.mediaKey,
    );
  }
  try {
    const acceptedOnce = await request<ReconfigureStreamOutput>(
      "reconfigureStream",
      reconfigureArgs(requestedExperiment),
    );
    // Open (and report) the mode the Host actually accepted — it may differ
    // from the request; capability-less hosts omit the field entirely.
    let accepted: ReconfigureStreamOutput = acceptedOnce;
    if (
      accepted.encoderExperiment === "splitVertical" &&
      accepted.encoderExperiment !== preparedExperiment
    ) {
      if (decoderReservation && !decoderReservation.split) {
        throw new Error("Host split mode exceeds the decoder reservation");
      }
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
        active.mediaKey,
      );
      accepted = await request<ReconfigureStreamOutput>(
        "reconfigureStream",
        reconfigureArgs("splitVertical"),
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
    assertDecoderReservation(decoderReservation, encoderExperiment, accepted);
    await openConfiguredStream(launcher,
      active.port,
      host,
      accepted.width,
      accepted.height,
      accepted.fps,
      encoderExperiment,
      // After a source switch the Host echo carries the new display name;
      // plain reconfigures keep the window's existing name.
      accepted.sourceName ?? active.sourceName,
      active.showFps ?? false,
      active.localCursor ?? true,
      currentLanguage(),
      active.localAudio ?? true,
      active.balancedPresentation ?? false,
    );
    await launcher.setOpusAudio?.(`src-${active.port}`, active.opusAudio ?? false);
    return {
      session: accepted.session,
      opusAudio: Boolean(launcher.setOpusAudio && active.opusAudio),
      balancedPresentation: Boolean(launcher.openStreamWithPresentation && active.balancedPresentation),
      width: accepted.width,
      height: accepted.height,
      fps: accepted.fps,
      qualityState: accepted.qualityState,
      // A source switch reports the source the Host actually accepted, falling
      // back to the requested index when an older capability host omits the
      // echo. JSON drops undefined keys, so plain reconfigures stay unchanged.
      ...(switchSource !== undefined
        ? {
            sourceIndex: accepted.sourceIndex ?? switchSource,
            ...(sourceId ? { sourceId } : {}),
            ...(accepted.sourceName ? { sourceName: accepted.sourceName } : {}),
          }
        : {}),
      viewerIps: active.viewerIps,
      mediaTransport: active.mediaTransport,
      encoderExperiment,
      mediaKey: active.mediaKey,
      udpStability: active.udpStability,
    };
  } catch (error) {
    await launcher
      .cancelPreparedStream(active.port, preparedExperiment)
      .catch(() => undefined);
    throw error;
  }
}
