import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// control.ts talks to react-native-tcp-socket; the socket is faked at the
// module boundary with an EventEmitter-style mock. No real sockets.

type PendingWrite = { payload: string; cb?: (e: Error | null) => void };

interface FakeSocket {
  on(event: string, handler: (...args: unknown[]) => void): void;
  write(payload: string, _enc: string, cb?: (e: Error | null) => void): void;
  destroy(): void;
  emit(event: string, ...args: unknown[]): void;
  written: PendingWrite[];
}

const sockets: FakeSocket[] = [];

function makeSocket(): FakeSocket {
  const handlers = new Map<string, ((...args: unknown[]) => void)[]>();
  const s: FakeSocket = {
    on(event, handler) {
      const list = handlers.get(event) ?? [];
      list.push(handler);
      handlers.set(event, list);
    },
    write(payload, _enc, cb) {
      s.written.push({ payload, cb });
    },
    destroy() {
      /* lifecycle is controlled by the test */
    },
    emit(event, ...args) {
      for (const h of handlers.get(event) ?? []) h(...args);
    },
    written: [],
  };
  return s;
}

vi.mock("react-native-tcp-socket", () => ({
  default: {
    createConnection(
      _opts: { host: string; port: number },
      onConnect: () => void,
    ) {
      const s = makeSocket();
      sockets.push(s);
      // connect asynchronously like the real native module
      setTimeout(onConnect, 0);
      return s;
    },
  },
}));

import { connect, isUnauthorizedError, preferredCaptureBackend } from "./control";

function lastSocket(): FakeSocket {
  return sockets[sockets.length - 1];
}

/** Server-style reply: newline-delimited JSON. */
function reply(socket: FakeSocket, line: unknown) {
  socket.emit("data", `${JSON.stringify(line)}\n`);
}

beforeEach(() => {
  sockets.length = 0;
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("connect token injection", () => {
  it("adds the provider token to every request envelope", async () => {
    const client = await connect("1.2.3.4", 7777, 1000, async () => "tok-123");
    const promise = client.request<{ n: number }>("addNumbers", { a: 1, b: 2 });
    const socket = lastSocket();
    // The token is awaited before the write happens.
    await vi.waitFor(() => expect(socket.written.length).toBe(1));
    expect(JSON.parse(socket.written[0].payload)).toEqual({
      command: "addNumbers",
      args: { a: 1, b: 2 },
      token: "tok-123",
    });
    reply(socket, { ok: true, result: { n: 3 } });
    await expect(promise).resolves.toEqual({ n: 3 });
    client.close();
  });

  it("omits the token field when the provider returns null", async () => {
    const client = await connect("1.2.3.4", 7777, 1000, async () => null);
    const promise = client.request("getCatalog");
    const socket = lastSocket();
    await vi.waitFor(() => expect(socket.written.length).toBe(1));
    expect(JSON.parse(socket.written[0].payload)).toEqual({
      command: "getCatalog",
      args: {},
    });
    reply(socket, { ok: true, result: { displays: [] } });
    await expect(promise).resolves.toEqual({ displays: [] });
    client.close();
  });

  it("sends no token when no provider is given (pair command)", async () => {
    const client = await connect("1.2.3.4", 7777, 1000);
    const promise = client.request("pair", { offerId: "o", code: "123456" });
    const socket = lastSocket();
    await vi.waitFor(() => expect(socket.written.length).toBe(1));
    const envelope = JSON.parse(socket.written[0].payload);
    expect(envelope.token).toBeUndefined();
    expect(envelope.command).toBe("pair");
    reply(socket, { ok: true, result: { token: "t".repeat(64) } });
    await expect(promise).resolves.toEqual({ token: "t".repeat(64) });
    client.close();
  });

  it("provider failure rejects the request without writing anything", async () => {
    const client = await connect("1.2.3.4", 7777, 1000, async () => {
      throw new Error("secure store unavailable");
    });
    await expect(client.request("getCatalog")).rejects.toThrow("secure store unavailable");
    expect(lastSocket().written.length).toBe(0);
    client.close();
  });

  it("caches the token so a stream request is written in the same JS turn", async () => {
    const tokenProvider = vi.fn(async () => "tok-cached");
    const client = await connect("1.2.3.4", 7777, 1000, tokenProvider);
    const socket = lastSocket();

    const probe = client.request("getStatus");
    await vi.waitFor(() => expect(socket.written.length).toBe(1));
    reply(socket, { ok: true, result: { sessions: [] } });
    await probe;

    const start = client.request("startStream", { viewerPort: 5001 });
    // No await here: opening StreamActivity immediately after this call must
    // not suspend JS before the native socket write has been queued.
    expect(socket.written.length).toBe(2);
    expect(JSON.parse(socket.written[1].payload).token).toBe("tok-cached");
    expect(tokenProvider).toHaveBeenCalledTimes(1);
    reply(socket, { ok: true, result: { session: 1 } });
    await expect(start).resolves.toEqual({ session: 1 });
    client.close();
  });

  it("acknowledges the native socket write before the server response", async () => {
    const client = await connect("1.2.3.4", 7777, 1000, async () => "tok");
    const socket = lastSocket();
    const onWritten = vi.fn();

    const request = client.request("startStream", { viewerPort: 5001 }, onWritten);
    await vi.waitFor(() => expect(socket.written.length).toBe(1));
    expect(onWritten).not.toHaveBeenCalled();

    socket.written[0].cb?.(null);
    expect(onWritten).toHaveBeenCalledOnce();

    reply(socket, { ok: true, result: { session: 1 } });
    await expect(request).resolves.toEqual({ session: 1 });
    client.close();
  });
});

describe("unauthorized error handling", () => {
  it("rejects with kind unauthorized and beats the close event", async () => {
    const client = await connect("1.2.3.4", 7777, 1000, async () => "bad-token");
    const promise = client.request<{ displays: [] }>("getCatalog");
    const socket = lastSocket();
    await vi.waitFor(() => expect(socket.written.length).toBe(1));
    // Host behavior: reply unauthorized, then close the connection.
    reply(socket, { ok: false, error: "unauthorized" });
    socket.emit("close");
    const error = await promise.catch((e: Error) => e);
    expect(isUnauthorizedError(error)).toBe(true);
    expect(error instanceof Error && error.message).toBe("unauthorized");
    client.close();
  });

  it("other remote errors keep the remote kind", async () => {
    const client = await connect("1.2.3.4", 7777, 1000, async () => "tok");
    const promise = client.request("nope");
    const socket = lastSocket();
    await vi.waitFor(() => expect(socket.written.length).toBe(1));
    reply(socket, { ok: false, error: "unknown command" });
    const error = await promise.catch((e: unknown) => e);
    expect(isUnauthorizedError(error)).toBe(false);
    expect(error instanceof Error && error.message).toBe("unknown command");
    client.close();
  });
});

describe("host capture capabilities", () => {
  it("selects Windows Graphics Capture instead of the macOS legacy default", () => {
    expect(preferredCaptureBackend({
      captureBackends: [{
        id: "windowsGraphicsCapture",
        label: "Windows Graphics Capture",
        hint: "hardware H.264",
      }],
    }, "screenCaptureKit")).toBe("windowsGraphicsCapture");
  });

  it("keeps a supported selection and falls back for legacy hosts", () => {
    const catalog = {
      captureBackends: [
        { id: "screenCaptureKit", label: "SCK", hint: "default" },
        { id: "cgDisplayStream", label: "CGDS", hint: "compat" },
      ],
    };
    expect(preferredCaptureBackend(catalog, "cgDisplayStream")).toBe("cgDisplayStream");
    expect(preferredCaptureBackend(undefined)).toBe("screenCaptureKit");
  });
});
