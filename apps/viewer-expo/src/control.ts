import TcpSocket from "react-native-tcp-socket";

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
  displays: DisplayInfo[];
}

export interface SessionView {
  session: number;
  sourceIndex: number;
  sourceName: string;
  viewerAddr: string;
  state: string;
  fps: number;
  kbps: number;
  fpsTarget: number;
  dropped: number;
  networkDropped?: number;
  captureQueueDropped?: number;
  captureToEncodeUs: number;
  maxCaptureToEncodeUs: number;
  captureQueueWaitUs?: number;
  maxCaptureQueueWaitUs?: number;
  encodeOutputUs?: number;
  maxEncodeOutputUs?: number;
  sendBlockUs: number;
  maxSendBlockUs: number;
  pendingFrame: number;
  error?: string | null;
}

export interface StatusView {
  sessions: SessionView[];
}

export interface ControlClient {
  request<T>(command: string, args?: unknown): Promise<T>;
  close(): void;
}

export type ControlErrorKind = "remote" | "timeout" | "transport";

export class ControlRequestError extends Error {
  constructor(
    message: string,
    readonly kind: ControlErrorKind,
  ) {
    super(message);
    this.name = "ControlRequestError";
  }
}

export function isControlTransportError(error: unknown): boolean {
  return error instanceof ControlRequestError && error.kind === "transport";
}

export function connect(host: string, port = 7777, timeoutMs = 5000): Promise<ControlClient> {
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

    const socket = TcpSocket.createConnection({ host, port }, () => {
      if (settled) return;
      settled = true;
      resolve({
        request<T>(command: string, args?: unknown): Promise<T> {
          return new Promise<T>((res, rej) => {
            if (terminalError) {
              rej(terminalError);
              return;
            }
            const id = nextId++;
            const payload = JSON.stringify({ command, args: args ?? {} }) + "\n";
            const requestTimeout =
              command === "getCatalog" || command === "startStream" ? 15_000 : 5_000;
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
                if (!writeError || !pending.has(id)) return;
                const handler = pending.get(id);
                pending.delete(id);
                clearTimeout(handler?.timer ?? timer);
                (handler?.reject ?? rej)(
                  new ControlRequestError(`control write error: ${writeError.message}`, "transport"),
                );
              });
            } catch (e) {
              const handler = pending.get(id);
              pending.delete(id);
              clearTimeout(handler?.timer ?? timer);
              rej(new ControlRequestError(`control write error: ${String(e)}`, "transport"));
            }
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
          handlers.reject(new ControlRequestError(parsed.error ?? "control error", "remote"));
        }
      }
    });

    socket.on("error", (err: Error) => {
      const e = new ControlRequestError(`control connection error: ${err.message}`, "transport");
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
