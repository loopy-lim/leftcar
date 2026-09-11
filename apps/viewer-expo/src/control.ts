import TcpSocket from "react-native-tcp-socket";
import type { AdaptiveQualityState } from "./adaptive-resolution";
import {
  StreamSealer,
  base64UrlToBytes,
  bytesToBase64Url,
  clientFinish,
  createClientHello,
  encodeClientHello,
  parseSealedLine,
  parseServerHello,
  sealedLine,
  type ClientHello,
} from "./secure-channel";
import type { EncoderExperimentId } from "./encoder-experiment";
import { currentTranslation } from "./language-store";
import { LocalizedError } from "./localized-error";
import { DEFAULT_CONTROL_PORT } from "./defaults";


/** 명령별 요청 타임아웃: 스트림 시작은 첫 프레임 대기까지, 카탈로그는
 * 나열 조회까지의 실측 여유를 담는다. 나머지는 짧은 기본값을 쓴다.
 * 파일 청크 1개는 최대 1 MiB(base64 ≈ 1.4 MB)라 느린 링크 대비 여유를 둔다.
 * 클립보드는 256 KiB 텍스트 왕복이 있어 기본 5초보다 여유를 둔다 — 실패 시
 * 공용 제어 소켓이 파괴되므로 타임아웃이 오탐이면 안 된다. */
const REQUEST_TIMEOUT_MS: Record<string, number> = {
  startStream: 25_000,
  getCatalog: 15_000,
  setClipboard: 15_000,
  getClipboard: 15_000,
  sendFileBegin: 15_000,
  sendFileChunk: 30_000,
  sendFileEnd: 15_000,
  sendFileCancel: 10_000,
  listShareQueue: 15_000,
  fetchFileBegin: 15_000,
  fetchFileChunk: 30_000,
  fetchFileEnd: 15_000,
  fetchFileCancel: 10_000,
};
const DEFAULT_REQUEST_TIMEOUT_MS = 5_000;

/**
 * Control-plane client (design §제어평면): viewer pulls from the host's
 * TCP JSON server. Newline-delimited {"command","args"} / {"ok",...}.
 */

export interface DisplayInfo {
  index: number;
  name: string;
  width: number;
  height: number;
}

export interface CatalogView {
  platform: "macos" | "windows" | "linux" | string;
  captureBackends: CaptureBackendInfo[];
  mediaHost?: string | null;
  displays: DisplayInfo[];
  encoderExperiments?: unknown;
  udpStabilityCapabilities?: unknown;
  /**
   * Host accepts an optional `encoderExperiment` on reconfigureStream and
   * reports the actually accepted mode in the response. Older hosts omit this
   * flag; viewers must never send a mode transition without it.
   */
  reconfigureEncoderExperiment?: boolean;
  /**
   * Host accepts an optional `sourceIndex` on reconfigureStream to move the
   * live session to another display (cheap display switch). Older hosts omit
   * this flag; viewers must keep using stop+start to change sources.
   */
  reconfigureSource?: boolean;
};

export interface CaptureBackendInfo {
  id: string;
  label: string;
  hint: string;
}

export function preferredCaptureBackend(
  catalog: Pick<CatalogView, "captureBackends"> | null | undefined,
  current?: string,
): string {
  const available = catalog?.captureBackends ?? [];
  if (current && available.some((backend) => backend.id === current)) return current;
  return available[0]?.id ?? "screenCaptureKit";
}

export interface SessionView {
  session: number;
  sourceIndex: number;
  sourceName: string;
  viewerAddr: string;
  width?: number;
  height?: number;
  state: string;
  fps: number;
  kbps: number;
  fpsTarget: number;
  qualityState?: AdaptiveQualityState;
  udpStability?: unknown;
  captureFps?: number;
  encodeSubmitFps?: number;
  encodeOutputFps?: number;
  renderedFps?: number | null;
  /** Non-null only while the accepted encoder experiment is splitVertical. */
  splitDirection?: string | null;
  /** Raw split-decoder receiver output fps (0 while frozen; Host maps the aggregate 0 to null). */
  leftRenderedFps?: number;
  rightRenderedFps?: number;
  joinedRenderedFps?: number;
  captureCallbacks?: number;
  encodeOutputCallbacks?: number;
  encodeSubmitFailures?: number;
  encodeInFlight?: number;
  dropped: number;
  networkDropped?: number;
  networkQueueDropped?: number;
  udpSendFailures?: number;
  udpSendRetries?: number;
  recoveryKeyframes?: number;
  recoveryRequestsSuppressed?: number;
  captureQueueDropped?: number;
  captureToEncodeUs: number;
  maxCaptureToEncodeUs: number;
  captureQueueWaitUs?: number;
  maxCaptureQueueWaitUs?: number;
  encodeOutputUs?: number;
  maxEncodeOutputUs?: number;
  sendBlockUs: number;
  maxSendBlockUs: number;
  sendPaceUs?: number;
  maxSendPaceUs?: number;
  pendingFrame: number;
  pendingFrameBytes?: number;
  pendingFrameOldestAgeUs?: number;
  /** Oldest age in the split encoded queue (macOS split pipeline). */
  splitEncodedQueueOldestUs?: number;
  /** Oldest age in the split capture queue (pending capture age). */
  splitCaptureQueueOldestUs?: number;
  frames: number;
  bytes: number;
  captureBackend: "screenCaptureKit" | "cgDisplayStream" | "windowsGraphicsCapture" | string;
  mediaTransport: "udp" | string;
  firstCaptureMs: number;
  firstEncodeMs: number;
  firstSendMs: number;
  currentBitrate: number;
  captureIntervalP95Us: number;
  captureToEncodeP95Us: number;
  captureQueueWaitP95Us: number;
  encodeOutputP95Us: number;
  encodeOutputIntervalP95Us?: number;
  sendBlockP95Us: number;
  sendPaceP95Us?: number;
  error?: string | null;
  receiverFrameGaps?: number;
  receiverInputDrops?: number;
  receiverIncompleteAus?: number;
  receiverStaleFrames?: number;
  receiverStaleInputDrops?: number | null;
  receiverOutputBurstDiscards?: number;
  receiverRttMs?: number | null;
  receiverWireMs?: number | null;
  receiverFeedbackAgeMs?: number | null;
  bitrateFloorCollapseCount?: number;
  bitrateFloorCollapseLastReason?: string;
  udpStabilityProfile?: string;
  udpBurstDatagrams?: number;
  udpPacingRateMultiplier?: number;
  udpFecParityShards?: number;
  udpAdaptivePacing?: boolean;
  udpBurstReason?: string;
  receiverMediaDatagrams?: number;
  receiverDataDatagrams?: number;
  receiverParityDatagrams?: number;
  receiverFecRestoredFragments?: number;
  receiverUnrecoverableFecGroups?: number;
  receiverMaxMissingDataFragments?: number;
  receiverOneFrameGapEvents?: number;
  receiverMultiFrameGapEvents?: number;
  receiverPairedIdrEpisodes?: number;
  receiverSuppressedRecoveryRequests?: number;
  receiverFecDecodeFailures?: number;
  /** Split recovery episodes where both tiles resumed from the same IDR generation. */
  receiverPairedIdrResumes?: number;
  /** Split clock-corrected send->decoder age in ms; null until clock sync converges. */
  receiverSplitWireMs?: number | null;
  /** Split clock-corrected capture->decoder age in ms; null until clock sync converges. */
  receiverSplitCaptureAgeMs?: number | null;
  /** Viewer-measured reliable-input send->ack RTT (EWMA, ms); null until the first ack. */
  receiverInputRttMs?: number | null;
}

export interface StatusView {
  sessions: SessionView[];
}

export interface ReconfigureStreamOutput {
  session: number;
  width: number;
  height: number;
  fps: number;
  qualityState: AdaptiveQualityState;
  /** The mode actually accepted by the Host (capability-backed hosts). */
  encoderExperiment?: EncoderExperimentId;
  /**
   * Echo of the capture source the replacement stream runs on. Present only
   * when the request carried a `sourceIndex` (capability-backed hosts).
   */
  sourceIndex?: number;
  /** Host-side display name of the switched source; only present with a source echo. */
  sourceName?: string;
}

export interface ControlClient {
  request<T>(command: string, args?: unknown, onWritten?: () => void): Promise<T>;
  close(): void;
  /** 서버가 서명으로 증명한 호스트 공개키(b64url). 평문(루프백) 모드면 없다. */
  readonly hostKey?: string | null;
}

export type ControlErrorKind = "remote" | "timeout" | "transport" | "unauthorized";

export class ControlRequestError extends Error {
  constructor(
    message: string,
    readonly kind: ControlErrorKind,
  ) {
    super(message);
    this.name = "ControlRequestError";
  }
}

export function formatErrorMessage(err: unknown): string {
  const t = currentTranslation().viewer;
  if (err instanceof LocalizedError) return err.format();
  if (!err) return t.errGeneric;
  let message = "";
  if (err instanceof Error && err.message) message = err.message;
  if (!message && typeof err === "string") message = err;
  if (typeof err === "object") {
    const o = err as Record<string, unknown>;
    if (!message && typeof o.message === "string" && o.message) message = o.message;
    if (!message && typeof o.error === "string" && o.error) message = o.error;
    if (!message && typeof o.err === "string" && o.err) message = o.err;
    if (!message && typeof o.code === "string" && o.code) message = o.code;
  }
  if (!message) message = String(err);

  const normalized = message.toLowerCase();
  if (normalized.includes("unauthorized")) {
    return t.errUnauthorized;
  }
  if (normalized.includes("pinned key")) {
    return t.errHostKeyMismatch;
  }
  if (normalized.includes("pairing failed")) {
    return t.errPairingRejected;
  }
  if (normalized.includes("pairing rejected")) {
    return t.errPairingDeclined;
  }
  if (normalized.includes("offer not found")) {
    return t.errCodeExpired;
  }
  if (
    normalized.includes("screen-recording permission") ||
    normalized.includes("screen recording permission") ||
    message.includes("화면 공유 권한")
  ) {
    return t.errHostScreenPermission;
  }
  if (normalized.includes("timeout")) {
    return t.errHostTimeout;
  }
  if (
    normalized.includes("connection closed") ||
    normalized.includes("connection error") ||
    normalized.includes("econn")
  ) {
    return t.errConnectFailed;
  }
  // 이 코드베이스의 안내 문구는 한국어로 작성되므로 한글이 섞인 메시지는
  // 이미 큐레이된 것이다. 매핑되지 않은 영어 원문은 개발자 진단용 콘솔로만
  // 남기고, UI는 친절한 안내문 하나만 보여 준다.
  if (/[가-힣]/.test(message)) {
    return message;
  }
  console.warn("[leftcar] unmapped control error:", message);
  return t.errGeneric;
}

export function isControlTransportError(error: unknown): boolean {
  return error instanceof ControlRequestError && error.kind === "transport";
}

/** The host rejected our token (or we never paired) — pairing is required. */
export function isUnauthorizedError(error: unknown): boolean {
  return error instanceof ControlRequestError && error.kind === "unauthorized";
}

/** 루프백 제어 경로(USB 네이티브 프록시, 진단 도구)는 평문 JSON을 유지한다. */
export function isLoopbackHost(host: string): boolean {
  const normalized = host.trim().toLowerCase();
  return normalized === "127.0.0.1" || normalized === "::1" || normalized === "localhost";
}

/**
 * Supplies the pairing token injected into every request envelope. Polled
 * per-request so a freshly completed pairing is picked up without a reconnect.
 */
export type TokenProvider = () => Promise<string | null>;

export interface ConnectOptions {
  /**
   * QR(v2)에서 핀한 호스트 Ed25519 공개키(b64url 32B). ServerHello의 서명
   * 키가 이것과 다르면 중간자로 간주하고 연결을 끊는다. 없으면 TOFU —
   * 검증된 키를 onHostKey로 되돌려 호출자가 핀하게 한다.
   */
  pinnedHostKey?: string | null;
  /** 핸드셰이크로 증명된 호스트 공개키(b64url). TOFU 핀 저장용. */
  onHostKey?: (hostKey: string) => void;
}

export function connect(
  host: string,
  port = DEFAULT_CONTROL_PORT,
  timeoutMs = 5000,
  tokenProvider?: TokenProvider,
  options?: ConnectOptions,
): Promise<ControlClient> {
  return new Promise((resolve, reject) => {
    let settled = false;
    const pending = new Map<
      number,
      {
        resolve: (v: unknown) => void;
        reject: (e: Error) => void;
        timer: ReturnType<typeof setTimeout>;
      }
    >();
    let terminalError: ControlRequestError | null = null;
    let nextId = 1;
    // The issued token is immutable for the lifetime of this socket. After
    // the first authenticated probe, keep it in memory so a subsequent
    // startStream reaches socket.write without waiting for another
    // SecureStore/JS turn. Opening a native viewer activity can pause React Native
    // immediately after the call site.
    let cachedToken: string | null | undefined;
    // 루프백이 아닌 피어는 secure-channel 핸드셰이크가 필수다.
    const secure = !isLoopbackHost(host);
    let mode: "handshake" | "plain" | "sealed" = secure ? "handshake" : "plain";
    let tx: StreamSealer | null = null;
    let rx: StreamSealer | null = null;
    let clientHostKey: string | null = null;
    let helloSecret: Uint8Array | null = null;
    let hello: ClientHello | null = null;
    let buffer = "";
    const textEncoder = new TextEncoder();
    const textDecoder = new TextDecoder();

    const fail = (message: string) => {
      const e = new ControlRequestError(message, "transport");
      terminalError = e;
      if (!settled) {
        settled = true;
        reject(e);
      }
      for (const [, h] of pending) {
        clearTimeout(h.timer);
        h.reject(e);
      }
      pending.clear();
      socket.destroy();
    };

    const socket = TcpSocket.createConnection({ host, port }, () => {
      if (settled) return;
      if (!secure) {
        settled = true;
        resolve(makeClient());
        return;
      }
      try {
        const handshake = createClientHello();
        helloSecret = handshake.secret;
        hello = handshake.hello;
        socket.write(`${encodeClientHello(handshake.hello)}\n`, "utf8", (writeError) => {
          if (writeError) fail(`secure handshake write error: ${formatErrorMessage(writeError)}`);
        });
      } catch (e) {
        // 네이티브 콜백 안의 동기 예외는 앱 전체를 강제종료시킨다 — 반드시
        // connect() 프로미스의 rejection으로 바꾼다.
        fail(`secure handshake failed: ${formatErrorMessage(e)}`);
      }
    });

    const makeClient = (): ControlClient => ({
      request<T>(command: string, args?: unknown, onWritten?: () => void): Promise<T> {
        const issue = (token: string | null) => new Promise<T>((res, rej) => {
          if (terminalError) {
            rej(terminalError);
            return;
          }
          const id = nextId++;
          const envelope = { command, args: args ?? {}, ...(token ? { token } : {}) };
          const payload = JSON.stringify(envelope) + "\n";
          const requestTimeout = REQUEST_TIMEOUT_MS[command] ?? DEFAULT_REQUEST_TIMEOUT_MS;
          const timer = setTimeout(() => {
            const handler = pending.get(id);
            if (!handler) return;
            pending.delete(id);
            handler.reject(new ControlRequestError(`control request timeout: ${command}`, "timeout"));
            // Responses do not carry request ids. Once one request times
            // out, a delayed response could otherwise be matched to the
            // next request on this socket.
            socket.destroy();
          }, requestTimeout);
          pending.set(id, {
            resolve: res as (v: unknown) => void,
            reject: rej,
            timer,
          });
          try {
            const wire =
              mode === "sealed" && tx
                ? sealedLine(tx.seal(textEncoder.encode(payload))) + "\n"
                : payload;
            socket.write(wire, "utf8", (writeError) => {
              if (!writeError) {
                onWritten?.();
                return;
              }
              if (!pending.has(id)) return;
              const handler = pending.get(id);
              pending.delete(id);
              clearTimeout(handler?.timer ?? timer);
              const msg = formatErrorMessage(writeError);
              (handler?.reject ?? rej)(
                new ControlRequestError(`control write error: ${msg}`, "transport"),
              );
            });
          } catch (e) {
            const handler = pending.get(id);
            pending.delete(id);
            clearTimeout(handler?.timer ?? timer);
            const msg = formatErrorMessage(e);
            rej(new ControlRequestError(`control write error: ${msg}`, "transport"));
          }
        });
        if (cachedToken !== undefined) {
          return issue(cachedToken);
        }
        if (!tokenProvider) {
          cachedToken = null;
          return issue(cachedToken);
        }
        // The initial lookup may be asynchronous, but a rejection occurs
        // before an id is allocated. Every later request is issued
        // synchronously through the cached branch above.
        return tokenProvider().then((token) => {
          cachedToken = token;
          return issue(token);
        });
      },
      close() {
        terminalError = new ControlRequestError("control connection closed", "transport");
        socket.destroy();
      },
      hostKey: clientHostKey,
    });

    /** 한 줄의 평문 JSON을 응답 매칭으로 처리한다. */
    const handlePlainTextLine = (line: string) => {
      if (!line.trim()) return;
      let parsed: { ok?: boolean; result?: unknown; error?: string };
      try {
        parsed = JSON.parse(line);
      } catch {
        return;
      }
      // FIFO match: server replies in request order
      const oldest = [...pending.entries()][0];
      if (!oldest) return;
      const [id, handlers] = oldest;
      pending.delete(id);
      clearTimeout(handlers.timer);
      if (parsed.ok) {
        handlers.resolve(parsed.result);
      } else {
        if (parsed.error === "unauthorized") {
          cachedToken = undefined;
        }
        // The host closes the connection right after "unauthorized"; this
        // rejection is registered (and pending cleared) before the close
        // handler runs, so the specific error wins over the generic one.
        handlers.reject(
          new ControlRequestError(
            parsed.error ?? "control error",
            parsed.error === "unauthorized" ? "unauthorized" : "remote",
          ),
        );
      }
    };

    const drain = () => {
      while (true) {
        const nl = buffer.indexOf("\n");
        if (nl < 0) return;
        const raw = buffer.slice(0, nl);
        buffer = buffer.slice(nl + 1);
        if (mode === "handshake") {
          try {
            const serverHello = parseServerHello(raw);
            const pinned = options?.pinnedHostKey
              ? base64UrlToBytes(options.pinnedHostKey)
              : null;
            const result = clientFinish(helloSecret as Uint8Array, hello as ClientHello, serverHello, pinned);
            tx = new StreamSealer(result.keys.c2s);
            rx = new StreamSealer(result.keys.s2c);
            clientHostKey = bytesToBase64Url(result.spk);
            mode = "sealed";
            const confirm = JSON.stringify({
              hello: "ok",
              nc: bytesToBase64Url((hello as ClientHello).nc),
            });
            socket.write(
              `${sealedLine(tx.seal(textEncoder.encode(confirm)))}\n`,
              "utf8",
              (writeError) => {
                if (writeError) fail(`secure handshake write error: ${formatErrorMessage(writeError)}`);
              },
            );
            options?.onHostKey?.(clientHostKey);
            settled = true;
            resolve(makeClient());
          } catch (e) {
            fail(`secure handshake failed: ${formatErrorMessage(e)}`);
          }
          continue;
        }
        if (mode === "sealed") {
          const frame = parseSealedLine(raw);
          if (!frame) {
            fail("control channel expected a sealed frame");
            return;
          }
          let plaintext: Uint8Array;
          try {
            plaintext = (rx as StreamSealer).open(frame);
          } catch (e) {
            fail(`sealed frame rejected: ${formatErrorMessage(e)}`);
            return;
          }
          handlePlainTextLine(textDecoder.decode(plaintext));
          continue;
        }
        handlePlainTextLine(raw);
      }
    };

    socket.on("data", (data: Buffer | string) => {
      buffer += typeof data === "string" ? data : data.toString("utf8");
      drain();
    });

    socket.on("error", (err: unknown) => {
      const msg = formatErrorMessage(err);
      const e = new ControlRequestError(`control connection error: ${msg}`, "transport");
      terminalError = e;
      if (!settled) {
        settled = true;
        reject(e);
      }
      for (const [, h] of pending) {
        clearTimeout(h.timer);
        h.reject(e);
      }
      pending.clear();
    });

    socket.on("close", () => {
      const e = new ControlRequestError("control connection closed", "transport");
      terminalError = e;
      for (const [, h] of pending) {
        clearTimeout(h.timer);
        h.reject(e);
      }
      pending.clear();
    });

    setTimeout(() => {
      if (!settled) {
        settled = true;
        socket.destroy();
        reject(new ControlRequestError(`connect timeout to ${host}:${port}`, "timeout"));
      }
    }, timeoutMs);
  });
}
