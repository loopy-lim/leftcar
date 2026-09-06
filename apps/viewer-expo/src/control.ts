import TcpSocket from "react-native-tcp-socket";
import type { AdaptiveQualityState } from "./adaptive-resolution";

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
}

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
}

export interface StatusView {
  sessions: SessionView[];
}

export interface ReconfigureStreamInput {
  session: number;
  width: number;
  height: number;
  fps: number;
  qualityState: AdaptiveQualityState;
}

export interface ReconfigureStreamOutput {
  session: number;
  width: number;
  height: number;
  fps: number;
  qualityState: AdaptiveQualityState;
}

export interface ControlClient {
  request<T>(command: string, args?: unknown, onWritten?: () => void): Promise<T>;
  close(): void;
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
  if (!err) return "문제가 발생했습니다. 잠시 후 다시 시도해 주세요.";
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
    return "컴퓨터의 연결 승인이 필요합니다.";
  }
  if (normalized.includes("pairing failed")) {
    return "인증 번호가 맞지 않거나 만료되었습니다. 컴퓨터에서 새 연결 코드를 만들어 주세요.";
  }
  if (normalized.includes("offer not found")) {
    return "연결 코드가 만료되었습니다. 컴퓨터에서 새 연결 코드를 만들어 주세요.";
  }
  if (normalized.includes("timeout")) {
    return "컴퓨터가 응답하지 않습니다. 같은 네트워크인지 확인한 뒤 다시 시도해 주세요.";
  }
  if (
    normalized.includes("connection closed") ||
    normalized.includes("connection error") ||
    normalized.includes("econn")
  ) {
    return "컴퓨터와 연결할 수 없습니다. Leftcar가 실행 중인지 확인해 주세요.";
  }
  return message;
}

export function isControlTransportError(error: unknown): boolean {
  return error instanceof ControlRequestError && error.kind === "transport";
}

/** The host rejected our token (or we never paired) — pairing is required. */
export function isUnauthorizedError(error: unknown): boolean {
  return error instanceof ControlRequestError && error.kind === "unauthorized";
}

/**
 * Supplies the pairing token injected into every request envelope. Polled
 * per-request so a freshly completed pairing is picked up without a reconnect.
 */
export type TokenProvider = () => Promise<string | null>;

export function connect(
  host: string,
  port = 7777,
  timeoutMs = 5000,
  tokenProvider?: TokenProvider,
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

    const socket = TcpSocket.createConnection({ host, port }, () => {
      if (settled) return;
      settled = true;
      resolve({
        request<T>(command: string, args?: unknown, onWritten?: () => void): Promise<T> {
          const issue = (token: string | null) => new Promise<T>((res, rej) => {
            if (terminalError) {
              rej(terminalError);
              return;
            }
            const id = nextId++;
            const envelope = { command, args: args ?? {}, ...(token ? { token } : {}) };
            const payload = JSON.stringify(envelope) + "\n";
            const requestTimeout = command === "startStream"
              ? 25_000
              : command === "getCatalog"
                ? 15_000
                : 5_000;
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
              socket.write(payload, "utf8", (writeError) => {
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
      });
    });

    let buffer = "";

    socket.on("data", (data: Buffer | string) => {
      buffer += typeof data === "string" ? data : data.toString("utf8");
      let nl: number;
      while ((nl = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, nl);
        buffer = buffer.slice(nl + 1);
        if (!line.trim()) continue;
        let parsed: { ok?: boolean; result?: unknown; error?: string };
        try {
          parsed = JSON.parse(line);
        } catch {
          continue;
        }
        // FIFO match: server replies in request order
        const oldest = [...pending.entries()][0];
        if (!oldest) continue;
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
      }
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
