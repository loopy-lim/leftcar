import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setCurrentLanguage } from "./language-store";

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

import {
  connect,
  formatErrorMessage,
  isUnauthorizedError,
  preferredCaptureBackend,
} from "./control";
import { ed25519, x25519 } from "@noble/curves/ed25519";
import {
  StreamSealer,
  base64UrlToBytes,
  bytesToBase64Url,
  deriveKeys,
  parseSealedLine,
  sealedLine,
  signedTranscript,
} from "./secure-channel";

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

// 포맷 문구는 테스트에서 한국어로 고정한다.
setCurrentLanguage("ko");

describe("connect token injection", () => {
  it("adds the provider token to every request envelope", async () => {
    const client = await connect("127.0.0.1", 7777, 1000, async () => "tok-123");
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
    const client = await connect("127.0.0.1", 7777, 1000, async () => null);
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
    const client = await connect("127.0.0.1", 7777, 1000);
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
    const client = await connect("127.0.0.1", 7777, 1000, async () => {
      throw new Error("secure store unavailable");
    });
    await expect(client.request("getCatalog")).rejects.toThrow("secure store unavailable");
    expect(lastSocket().written.length).toBe(0);
    client.close();
  });

  it("caches the token so a stream request is written in the same JS turn", async () => {
    const tokenProvider = vi.fn(async () => "tok-cached");
    const client = await connect("127.0.0.1", 7777, 1000, tokenProvider);
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
    const client = await connect("127.0.0.1", 7777, 1000, async () => "tok");
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
    const client = await connect("127.0.0.1", 7777, 1000, async () => "bad-token");
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
    const client = await connect("127.0.0.1", 7777, 1000, async () => "tok");
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

describe("formatErrorMessage and socket error handling", () => {
  it("formats Error objects, strings, error code objects, and null/undefined without undefined", () => {
    // 매핑되지 않은 영어 원문은 친절한 안내문 뒤 괄호로 붙는다.
    expect(formatErrorMessage(new Error("custom error"))).toBe(
      "문제가 발생했습니다. 잠시 후 다시 시도해 주세요. (custom error)",
    );
    expect(formatErrorMessage("string error")).toBe(
      "문제가 발생했습니다. 잠시 후 다시 시도해 주세요. (string error)",
    );
    expect(formatErrorMessage({ code: "ECONNREFUSED" })).toBe(
      "컴퓨터와 연결할 수 없습니다. Leftcar가 실행 중인지 확인해 주세요.",
    );
    expect(formatErrorMessage({ message: "msg error" })).toBe(
      "문제가 발생했습니다. 잠시 후 다시 시도해 주세요. (msg error)",
    );
    expect(formatErrorMessage({ error: "err property" })).toBe(
      "문제가 발생했습니다. 잠시 후 다시 시도해 주세요. (err property)",
    );
    expect(formatErrorMessage(undefined)).toBe("문제가 발생했습니다. 잠시 후 다시 시도해 주세요.");
    expect(formatErrorMessage(null)).toBe("문제가 발생했습니다. 잠시 후 다시 시도해 주세요.");
  });

  it("keeps already-curated Korean messages untouched", () => {
    expect(formatErrorMessage(new Error("신뢰하는 같은 Wi-Fi의 컴퓨터만 연결할 수 있습니다"))).toBe(
      "신뢰하는 같은 Wi-Fi의 컴퓨터만 연결할 수 있습니다",
    );
  });

  it("maps host screen-recording permission failures to a settings guide", () => {
    expect(
      formatErrorMessage(
        new Error("startStream failed: screen-recording permission is not granted to Leftcar Host"),
      ),
    ).toBe("컴퓨터에서 화면 공유 권한이 꺼져 있습니다. Mac 시스템 설정에서 Leftcar를 허용해 주세요.");
    expect(formatErrorMessage(new Error("Screen Recording permission required"))).toBe(
      "컴퓨터에서 화면 공유 권한이 꺼져 있습니다. Mac 시스템 설정에서 Leftcar를 허용해 주세요.",
    );
  });

  it("maps an explicit pairing denial to the declined guide", () => {
    expect(formatErrorMessage(new Error("pairing rejected"))).toBe(
      "컴퓨터에서 연결 요청을 거절했습니다. 다시 시도하려면 QR을 다시 스캔해 주세요.",
    );
  });

  it("handles non-Error socket errors without producing 'control connection error: undefined'", async () => {
    const connectPromise = connect("127.0.0.1", 7777, 1000);
    const socket = lastSocket();
    socket.emit("error", { code: "ECONNREFUSED" });
    await expect(connectPromise).rejects.toThrow(
      "control connection error: 컴퓨터와 연결할 수 없습니다. Leftcar가 실행 중인지 확인해 주세요.",
    );
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

  it("selects the host's first automatic backend before the user chooses one", () => {
    const catalog = {
      captureBackends: [
        { id: "cgDisplayStream", label: "Automatic", hint: "pickerless" },
      ],
    };
    expect(preferredCaptureBackend(catalog, "")).toBe("cgDisplayStream");
  });
});

// -- 봉인(secure) 경로: 스크립트 서버로 핸드셰이크·봉인 프레임을 검증한다 ----

const b64u = bytesToBase64Url;
const enc = new TextEncoder();
const dec = new TextDecoder();

interface ScriptedServer {
  serverKeys: { c2s: Uint8Array; s2c: Uint8Array };
  spkB64: string;
}

/** ClientHello를 읽어 ServerHello로 답하고 키 확인 프레임을 소비한다. */
async function handshakeAsServer(socket: FakeSocket): Promise<ScriptedServer> {
  const hello = JSON.parse(socket.written[0].payload.trim()) as {
    nc: string;
    xk: string;
  };
  const nc = base64UrlToBytes(hello.nc);
  const clientXk = base64UrlToBytes(hello.xk);
  const serverSecret = new Uint8Array(32).fill(11);
  const serverXk = x25519.getPublicKey(serverSecret);
  const serverSeed = new Uint8Array(32).fill(12);
  const spk = ed25519.getPublicKey(serverSeed);
  const ns = new Uint8Array(32).fill(13);
  const sig = ed25519.sign(signedTranscript(nc, ns, clientXk, serverXk, spk), serverSeed);
  socket.emit(
    "data",
    `${JSON.stringify({
      v: 1,
      hello: "s",
      ns: b64u(ns),
      xk: b64u(serverXk),
      spk: b64u(spk),
      sig: b64u(sig),
    })}\n`,
  );
  // 키 확인 프레임(두 번째 쓰기)을 서버 키로 연다.
  await vi.waitFor(() => expect(socket.written.length).toBeGreaterThanOrEqual(2));
  const confirm = parseSealedLine(socket.written[1].payload.trim());
  const shared = x25519.getSharedSecret(serverSecret, clientXk);
  const serverKeys = deriveKeys(shared, nc, ns);
  const plaintext = new StreamSealer(serverKeys.c2s).open(confirm as Uint8Array);
  expect(JSON.parse(dec.decode(plaintext))).toEqual({ hello: "ok", nc: hello.nc });
  return { serverKeys, spkB64: b64u(spk) };
}

describe("secure control channel", () => {
  it("handshakes with a pinned key and seals every frame", async () => {
    const pending = connect("192.168.1.10", 7777, 5000, async () => "tok");
    await vi.waitFor(() => expect(lastSocket().written.length).toBeGreaterThanOrEqual(1));
    const server = await handshakeAsServer(lastSocket());
    const client = await pending;
    expect(client.hostKey).toBe(server.spkB64);

    const reply = JSON.stringify({ ok: true, result: { displays: [1, 2] } });
    const tx = new StreamSealer(server.serverKeys.s2c);
    const pendingRequest = client.request<{ displays: number[] }>("getCatalog", {});
    await vi.waitFor(() => expect(lastSocket().written.length).toBeGreaterThanOrEqual(3));
    // 요청은 봉인 봉투로 나가야 한다.
    const envelope = JSON.parse(lastSocket().written.at(-1)?.payload.trim() as string);
    expect(typeof envelope.e).toBe("string");
    lastSocket().emit("data", `${sealedLine(tx.seal(enc.encode(reply)))}\n`);
    await expect(pendingRequest).resolves.toEqual({ displays: [1, 2] });

    // 서버가 평문 JSON을 보내면 연결이 끊긴다(다운그레이드 거부).
    const pending2 = connect("192.168.1.10", 7777, 5000, async () => "tok");
    await vi.waitFor(() => {
      expect(sockets.length).toBeGreaterThanOrEqual(2);
      expect(sockets[sockets.length - 1].written.length).toBeGreaterThanOrEqual(1);
    });
    await handshakeAsServer(sockets[sockets.length - 1]);
    const client2 = await pending2;
    const pendingRequest2 = client2.request("getStatus", {});
    sockets[sockets.length - 1].emit(
      "data",
      `${JSON.stringify({ ok: true, result: {} })}\n`,
    );
    await expect(pendingRequest2).rejects.toThrow("sealed frame");
  });

  it("rejects a server whose key does not match the pin", async () => {
    const pending = connect("192.168.1.10", 7777, 5000, async () => null, {
      pinnedHostKey: b64u(new Uint8Array(32).fill(99)),
    });
    // connect()는 소켓 연결 콜백 이후 ClientHello를 쓴다.
    await vi.waitFor(() => expect(lastSocket().written.length).toBeGreaterThanOrEqual(1));
    const hello = JSON.parse(lastSocket().written[0].payload.trim()) as {
      nc: string;
      xk: string;
    };
    const serverSecret = new Uint8Array(32).fill(11);
    const serverXk = x25519.getPublicKey(serverSecret);
    const serverSeed = new Uint8Array(32).fill(12);
    const spk = ed25519.getPublicKey(serverSeed);
    const ns = new Uint8Array(32).fill(13);
    const sig = ed25519.sign(
      signedTranscript(
        base64UrlToBytes(hello.nc),
        ns,
        base64UrlToBytes(hello.xk),
        serverXk,
        spk,
      ),
      serverSeed,
    );
    lastSocket().emit(
      "data",
      `${JSON.stringify({
        v: 1,
        hello: "s",
        ns: b64u(ns),
        xk: b64u(serverXk),
        spk: b64u(spk),
        sig: b64u(sig),
      })}\n`,
    );
    await expect(pending).rejects.toThrow("보안 키");
    // 핀 불일치는 키 확인 전에 연결을 끊는다 — 확인 프레임을 쓰면 안 된다.
    expect(lastSocket().written.length).toBe(1);
  });
});
