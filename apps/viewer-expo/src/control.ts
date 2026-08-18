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
}

export interface StatusView {
  sessions: SessionView[];
}

export interface ControlClient {
  request<T>(command: string, args?: unknown): Promise<T>;
  close(): void;
}

export function connect(host: string, port = 7777, timeoutMs = 5000): Promise<ControlClient> {
  return new Promise((resolve, reject) => {
    let settled = false;
    const pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
    let nextId = 1;
    // Line buffering: responses are matched in order (server processes
    // requests sequentially per connection).
    const queue: Array<(v: unknown) => void | Promise<void>> = [];

    const socket = TcpSocket.createConnection({ host, port }, () => {
      if (settled) return;
      settled = true;
      resolve({
        request<T>(command: string, args?: unknown): Promise<T> {
          return new Promise<T>((res, rej) => {
            const id = nextId++;
            pending.set(id, { resolve: res as (v: unknown) => void, reject: rej });
            socket.write(
              JSON.stringify({ command, args: args ?? {} }) + "\n",
            );
          });
        },
        close() {
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
        if (parsed.ok) {
          handlers.resolve(parsed.result);
        } else {
          handlers.reject(new Error(parsed.error ?? "control error"));
        }
      }
    });

    socket.on("error", (err: Error) => {
      const e = new Error(`control connection error: ${err.message}`);
      if (!settled) {
        settled = true;
        reject(e);
      }
      for (const [, h] of pending) h.reject(e);
      pending.clear();
    });

    socket.on("close", () => {
      const e = new Error("control connection closed");
      for (const [, h] of pending) h.reject(e);
      pending.clear();
    });

    setTimeout(() => {
      if (!settled) {
        settled = true;
        socket.destroy();
        reject(new Error(`connect timeout to ${host}:${port}`));
      }
    }, timeoutMs);
  });
}
