import { beforeEach, describe, expect, it, vi } from "vitest";

// session.ts의 import 체인은 네이티브 경계(control의 tcp-socket, pairing의
// expo 모듈, usb/auto-reconnect)라 전부 모의한다. allocPort/allocPorts는
// 순수 모듈 상태만 다룬다.
vi.mock("./control", () => ({
  connect: vi.fn(),
}));
vi.mock("expo-constants", () => ({
  default: { deviceName: "Android viewer test" },
}));
vi.mock("expo-secure-store", () => {
  const store = new Map<string, string>();
  return {
    getItemAsync: vi.fn(async (key: string) => store.get(key) ?? null),
    setItemAsync: vi.fn(async (key: string, value: string) => {
      store.set(key, value);
    }),
    deleteItemAsync: vi.fn(async (key: string) => {
      store.delete(key);
    }),
    __store: store,
  };
});
vi.mock("./usb", () => ({
  getUsbState: vi.fn(async () => ({ attached: false, controlPort: 0 })),
}));
vi.mock("./auto-reconnect", () => ({
  markConnected: vi.fn(),
}));

beforeEach(async () => {
  // allocPort/allocPorts의 nextPort 모듈 상태를 테스트마다 초기화한다.
  vi.resetModules();
  vi.resetAllMocks();
  const storage = await import("expo-secure-store") as unknown as { __store: Map<string, string> };
  storage.__store.clear();
});

describe("viewer UDP port allocation", () => {
  it("allocPort hands out sequential ports from 5001", async () => {
    const { allocPort } = await import("./session");
    expect(allocPort()).toBe(5001);
    expect(allocPort()).toBe(5002);
    expect(allocPort()).toBe(5003);
  });

  it("allocPorts reserves count consecutive ports", async () => {
    const { allocPorts, allocPort } = await import("./session");
    const base = allocPorts(2);
    expect(base).toBe(5001);
    // The reserved neighbor is never handed out again.
    expect(allocPort()).toBe(5003);
  });

  it("the split + second display scenario never reuses the split right-tile port", async () => {
    const { allocPorts, allocPort } = await import("./session");
    // First display opens as splitVertical: it occupies base and base+1
    // (native/android-viewer prepared_udp.rs split_ports).
    const splitBase = allocPorts(2);
    // Second display must start past BOTH split ports, not land on
    // splitBase+1 (bind EADDRINUSE -> ERR_STREAM_PREPARE).
    const secondDisplay = allocPort();
    expect(secondDisplay).not.toBe(splitBase);
    expect(secondDisplay).not.toBe(splitBase + 1);
    expect(secondDisplay).toBe(splitBase + 2);
  });

  it("a single-mode window keeps its reserved neighbor free for a later split promotion", async () => {
    const { allocPorts, allocPort } = await import("./session");
    // Window A starts single at base; the adaptive reconfigure can promote
    // the SAME base to splitVertical later, claiming base+1 as well.
    const baseA = allocPorts(2);
    // Window B must not sit on A's promotion port.
    const baseB = allocPorts(2);
    expect(baseB).toBe(baseA + 2);
    expect(allocPort()).toBe(baseA + 4);
  });

  it("allocPorts clamps degenerate counts to one port", async () => {
    const { allocPorts } = await import("./session");
    expect(allocPorts(0)).toBe(5001);
    const { allocPort } = await import("./session");
    expect(allocPort()).toBe(5002);
  });
});

// -- 제어 세션 연결 수명 ------------------------------------------------------

import type { ControlClient } from "./control";

/** 테스트가 해제 시점을 고를 수 있는 지연 프라미스. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function makeClient(): ControlClient & { emitClosed: () => void } {
  const closedListeners: (() => void)[] = [];
  return {
    request: vi.fn(async () => ({})) as ControlClient["request"],
    close: vi.fn(),
    hostKey: null,
    whenClosed(listener: () => void) {
      closedListeners.push(listener);
    },
    emitClosed() {
      for (const listener of closedListeners) listener();
    },
  };
}

/** vi.mock 팩토리가 만든 메모리 SecureStore(vi.resetModules로 매 테스트 새로 생성). */
async function secureStore(): Promise<Map<string, string>> {
  const mod = (await import("expo-secure-store")) as unknown as {
    __store: Map<string, string>;
  };
  return mod.__store;
}

describe("control session connect lifecycle", () => {
  it("reads the persisted pin before any handshake or credential delivery", async () => {
    const key = "B".repeat(43);
    const store = await secureStore();
    store.set("leftcar.recent_hosts", JSON.stringify([
      { host: "10.0.0.5", port: 7777, hostKey: key, lastConnected: 1 },
    ]));
    store.set("leftcar.token.v2.10.0.0.5.7777", "a".repeat(64));
    const { connect } = await import("./control");
    const delivered: Array<string | null> = [];
    vi.mocked(connect).mockImplementation(async (_host, _port, _timeout, provider, options) => {
      if (options?.pinnedHostKey === key) throw new Error("host identity mismatch");
      const attackerKey = "C".repeat(43);
      options?.onHostKey?.(attackerKey);
      delivered.push(await provider?.() ?? null);
      return { ...makeClient(), hostKey: attackerKey };
    });

    const session = await import("./session");
    await expect(session.connectHost("10.0.0.5", 7777)).rejects.toThrow("host identity mismatch");
    expect(delivered).toEqual([]);
    expect(store.get("leftcar.token.v2.10.0.0.5.7777")).toBe("a".repeat(64));
  });

  it("does not open a socket when persisted pin storage is unreadable", async () => {
    const storage = await import("expo-secure-store");
    vi.mocked(storage.getItemAsync).mockRejectedValueOnce(new Error("pin storage unavailable"));
    const { connect } = await import("./control");
    vi.mocked(connect).mockClear().mockResolvedValue(makeClient());
    const session = await import("./session");

    await expect(session.connectHost("10.0.0.5", 7777)).rejects.toThrow("pin storage unavailable");
    expect(connect).not.toHaveBeenCalled();
  });

  it("never sends an unpinned endpoint credential to a newly proved peer", async () => {
    const token = "a".repeat(64);
    const store = await secureStore();
    store.set("leftcar.token.v2.10.0.0.5.7777", token);
    const peerKey = "C".repeat(43);
    const { connect } = await import("./control");
    const delivered: Array<string | null> = [];
    vi.mocked(connect).mockImplementation(async (_host, _port, _timeout, provider, options) => {
      options?.onHostKey?.(peerKey);
      delivered.push(await provider?.() ?? null);
      return { ...makeClient(), hostKey: peerKey };
    });
    const session = await import("./session");

    await session.connectHost("10.0.0.5", 7777);
    expect(delivered).toEqual([null]);
    expect(store.get("leftcar.token.v2.10.0.0.5.7777")).toBe(token);
    expect(store.get(`leftcar.token.v3.${peerKey}`)).toBeUndefined();
  });

  it("pins a saved alternate route before the handshake", async () => {
    const key = "B".repeat(43);
    (await secureStore()).set("leftcar.recent_hosts", JSON.stringify([
      { host: "10.0.0.5", aliases: ["mac.tail.ts.net"], port: 7777, hostKey: key, lastConnected: 1 },
    ]));
    const { connect } = await import("./control");
    vi.mocked(connect).mockClear().mockResolvedValue({ ...makeClient(), hostKey: key });
    const session = await import("./session");

    await session.connectHost("mac.tail.ts.net", 7777);
    expect(vi.mocked(connect).mock.calls[0]?.[4]?.pinnedHostKey).toBe(key);
  });

  it("a live focus signal cannot let a superseded selection persist first-use trust", async () => {
    const key = "C".repeat(43);
    const ready = deferred<ControlClient>();
    const nextClient = { ...makeClient(), hostKey: key };
    const { connect } = await import("./control");
    vi.mocked(connect).mockImplementationOnce(async (_host, _port, _timeout, _provider, options) => {
      const socket = await ready.promise;
      options?.onHostKey?.(key);
      return socket;
    });
    const session = await import("./session");
    const focus = new AbortController();
    const selection = session.beginHostSelection();
    const pending = session.connectHost("10.0.0.5", 7777, { selection, signal: focus.signal });
    await vi.waitFor(() => expect(connect).toHaveBeenCalledTimes(1));

    session.beginHostSelection();
    ready.resolve(nextClient);
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect(focus.signal.aborted).toBe(false);
    expect((await secureStore()).get("leftcar.recent_hosts")).toBeUndefined();
    expect((await import("./pinned-host-keys")).getPinnedHostKey("10.0.0.5", 7777)).toBeNull();
    expect(nextClient.close).toHaveBeenCalledOnce();
  });

  it.each([false, true])("reconnect cannot persist trust after its selection is retired (focus signal: %s)", async (withFocus) => {
    const first = makeClient();
    const key = "C".repeat(43);
    const nextClient = { ...makeClient(), hostKey: key };
    const ready = deferred<ControlClient>();
    const { connect } = await import("./control");
    vi.mocked(connect).mockResolvedValueOnce(first).mockImplementationOnce(
      async (_host, _port, _timeout, _provider, options) => {
        const socket = await ready.promise;
        options?.onHostKey?.(key);
        return socket;
      },
    );
    const session = await import("./session");
    await session.connectHost("10.0.0.5", 7777);
    const origin = session.captureRequestContext();
    first.emitClosed();
    const focus = new AbortController();
    const pending = session.reconnectHost(origin, withFocus ? focus.signal : undefined);
    await vi.waitFor(() => expect(connect).toHaveBeenCalledTimes(2));

    session.disconnectHost();
    ready.resolve(nextClient);
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect((await secureStore()).get("leftcar.recent_hosts")).toBeUndefined();
    expect((await import("./pinned-host-keys")).getPinnedHostKey("10.0.0.5", 7777)).toBeNull();
    expect(nextClient.close).toHaveBeenCalledOnce();
  });

  it.each(["broken", "{}", '[{"host":"10.0.0.5","port":7777,"hostKey":3,"lastConnected":1}]'])(
    "fails closed before a handshake on corrupt persisted identities: %s",
    async (raw) => {
      (await secureStore()).set("leftcar.recent_hosts", raw);
      const { connect } = await import("./control");
      vi.mocked(connect).mockClear().mockResolvedValue(makeClient());
      const session = await import("./session");

      await expect(session.connectHost("10.0.0.5", 7777)).rejects.toThrow();
      expect(connect).not.toHaveBeenCalled();
    },
  );

  it("closes a new verified socket when credential migration fails and keeps the active session", async () => {
    const { connect } = await import("./control");
    const clientA = makeClient();
    const clientB = { ...makeClient(), hostKey: "K".repeat(43) };
    vi.mocked(connect).mockResolvedValueOnce(clientA).mockResolvedValueOnce(clientB);

    const session = await import("./session");
    await session.connectHost("10.0.0.1", 7777);
    (await secureStore()).set(`leftcar.token.v3.${"K".repeat(43)}`, "b".repeat(64));
    const secureStoreModule = await import("expo-secure-store");
    vi.mocked(secureStoreModule.setItemAsync).mockRejectedValueOnce(
      new Error("endpoint alias write failed"),
    );

    await expect(session.connectHost("10.0.0.2", 7777)).rejects.toThrow(
      "endpoint alias write failed",
    );

    expect(clientB.close).toHaveBeenCalledTimes(1);
    expect(clientA.close).not.toHaveBeenCalled();
    expect(session.controlClient()).toBe(clientA);
    expect(session.controlHost()).toBe("10.0.0.1:7777");
  });

  it("a reconnect bound to A cannot overtake a newer B user selection", async () => {
    const { connect } = await import("./control");
    const clientA = makeClient();
    const clientB = makeClient();
    const gateB = deferred<ControlClient>();
    vi.mocked(connect)
      .mockResolvedValueOnce(clientA)
      .mockImplementationOnce(() => gateB.promise);

    const session = await import("./session");
    await session.connectHost("10.0.0.1", 7777);
    const requestA = session.captureRequestContext();
    expect(requestA).not.toBeNull();

    const selectionB = session.beginHostSelection();
    const pendingB = session.connectHost("10.0.0.2", 7777, { selection: selectionB });
    await expect(session.reconnectHost(requestA!)).rejects.toMatchObject({ name: "AbortError" });

    gateB.resolve(clientB);
    await expect(pendingB).resolves.toBe(clientB);
    expect(session.controlHost()).toBe("10.0.0.2:7777");
  });

  it("uses a verified host identity token after the same host changes address", async () => {
    const hostKey = "K".repeat(43);
    (await secureStore()).set(`leftcar.token.v3.${hostKey}`, "a".repeat(64));

    const { connect } = await import("./control");
    let provider: (() => Promise<string | null>) | undefined;
    vi.mocked(connect).mockImplementation(async (_host, _port, _timeoutMs, tokenProvider) => {
      provider = tokenProvider;
      return { ...makeClient(), hostKey };
    });

    const session = await import("./session");
    await session.connectHost("10.0.0.42", 7777);

    expect(await provider?.()).toBe("a".repeat(64));
    expect(session.captureRequestContext()?.identity).toBe(hostKey);
    expect((await secureStore()).get("leftcar.token.v2.10.0.0.42.7777")).toBe(
      "a".repeat(64),
    );
  });

  it("sends each host its own stored token across an A→B→A round-trip", async () => {
    (await secureStore()).set("leftcar.token.v2.10.0.0.1.7777", "a".repeat(64));
    (await secureStore()).set("leftcar.token.v2.10.0.0.2.7777", "b".repeat(64));
    (await secureStore()).set("leftcar.recent_hosts", JSON.stringify([
      { host: "10.0.0.1", port: 7777, hostKey: "A".repeat(43), lastConnected: 1 },
      { host: "10.0.0.2", port: 7777, hostKey: "B".repeat(43), lastConnected: 1 },
    ]));

    const { connect } = await import("./control");
    const providers: Array<() => Promise<string | null>> = [];
    vi.mocked(connect).mockImplementation(async (_host, _port, _timeoutMs, tokenProvider) => {
      providers.push(tokenProvider as () => Promise<string | null>);
      return makeClient();
    });

    const session = await import("./session");
    await session.connectHost("10.0.0.1", 7777);
    await session.connectHost("10.0.0.2", 7777);
    await session.connectHost("10.0.0.1", 7777);

    expect(await providers[0]()).toBe("a".repeat(64));
    expect(await providers[1]()).toBe("b".repeat(64));
    expect(await providers[2]()).toBe("a".repeat(64));
    expect(session.controlTarget()).toEqual({ host: "10.0.0.1", port: 7777 });
  });

  it("the USB loopback path still reads the selected network target's token", async () => {
    const { getUsbState } = await import("./usb");
    vi.mocked(getUsbState).mockResolvedValue({ attached: true, controlPort: 6200 });
    (await secureStore()).set("leftcar.token.v2.10.0.0.5.7777", "e".repeat(64));

    const { connect } = await import("./control");
    const calls: Array<{ host: string; port: number; token: string | null }> = [];
    vi.mocked(connect).mockImplementation(async (host, port, _timeoutMs, tokenProvider) => {
      calls.push({ host, port: port ?? 0, token: (await tokenProvider?.()) ?? null });
      return makeClient();
    });

    const session = await import("./session");
    await session.connectHost("10.0.0.5", 7777);

    expect(calls).toEqual([{ host: "127.0.0.1", port: 6200, token: "e".repeat(64) }]);
  });

  it("a connect that settles after disconnectHost does not resurrect the session", async () => {
    const { connect } = await import("./control");
    const clientA = makeClient();
    const gate = deferred<ControlClient>();
    vi.mocked(connect).mockImplementationOnce(() => gate.promise);

    const session = await import("./session");
    const pending = session.connectHost("10.0.0.1", 7777);
    await vi.waitFor(() => expect(connect).toHaveBeenCalledTimes(1));
    session.disconnectHost();
    gate.resolve(clientA);

    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect(clientA.close).toHaveBeenCalledTimes(1);
    expect(session.controlClient()).toBeNull();
    expect(session.controlHost()).toBe("");
    expect(session.controlTarget()).toBeNull();
  });

  it("a newer connectHost wins — the late socket closes and B's state stands", async () => {
    const { connect } = await import("./control");
    const clientA = makeClient();
    const clientB = makeClient();
    const gateA = deferred<ControlClient>();
    vi.mocked(connect).mockImplementationOnce(() => gateA.promise);
    vi.mocked(connect).mockImplementationOnce(async () => clientB);

    const session = await import("./session");
    const pendingA = session.connectHost("10.0.0.1", 7777);
    await vi.waitFor(() => expect(connect).toHaveBeenCalledTimes(1));
    const wonB = await session.connectHost("10.0.0.2", 7778);
    expect(wonB).toBe(clientB);
    expect(session.controlClient()).toBe(clientB);
    expect(session.controlHost()).toBe("10.0.0.2:7778");
    expect(clientA.close).not.toHaveBeenCalled();

    gateA.resolve(clientA);
    await expect(pendingA).rejects.toMatchObject({ name: "AbortError" });
    expect(clientA.close).toHaveBeenCalledTimes(1);
    expect(clientB.close).not.toHaveBeenCalled();
    expect(session.controlClient()).toBe(clientB);
    expect(session.controlHost()).toBe("10.0.0.2:7778");
  });
});

describe("connection-lost invalidation (stale connected badge)", () => {
  it("retains the selected target for a native window after transport loss and watches replacement sockets", async () => {
    const { connect } = await import("./control");
    const first = makeClient();
    const second = makeClient();
    const third = makeClient();
    vi.mocked(connect).mockResolvedValueOnce(first).mockResolvedValueOnce(second).mockResolvedValueOnce(third);
    const session = await import("./session");
    await session.connectHost("10.0.0.1", 7777);
    const origin = session.captureRequestContext();
    first.emitClosed();
    expect(session.controlClient()).toBeNull();
    expect(session.controlHost()).toBe("");
    expect(session.captureRequestContext()).toBe(origin);
    await expect(session.reconnectHost()).resolves.toBe(second);
    expect(session.controlHost()).toBe("10.0.0.1:7777");
    first.emitClosed();
    expect(session.controlClient()).toBe(second);
    second.emitClosed();
    expect(session.controlClient()).toBeNull();
    await expect(session.reconnectHost()).resolves.toBe(third);
  });

  it.each(["disconnect", "select"])("transport-loss recovery cannot undo an explicit %s", async (action) => {
    const { connect } = await import("./control");
    const first = makeClient();
    vi.mocked(connect).mockResolvedValueOnce(first);
    const session = await import("./session");
    await session.connectHost("10.0.0.1", 7777);
    const origin = session.captureRequestContext();
    first.emitClosed();
    if (action === "disconnect") session.disconnectHost();
    else session.beginHostSelection();
    await expect(session.reconnectHost(origin)).rejects.toMatchObject({ name: "AbortError" });
    expect(session.controlClient()).toBeNull();
  });

  it("a closed socket invalidates the session so the home badge stops claiming connected", async () => {
    const { connect } = await import("./control");
    const client = makeClient();
    vi.mocked(connect).mockResolvedValue(client);

    const session = await import("./session");
    await session.connectHost("10.0.0.1", 7777);
    expect(session.controlClient()).not.toBeNull();

    // 호스트 재시작·네트워크 전환 등으로 소켓이 닫히면 연결 객체가 남아
    // "컴퓨터 연결됨" 배지가 거짓말을 하던 회귀를 잠근다.
    client.emitClosed();
    expect(session.controlClient()).toBeNull();
    expect(session.controlHost()).toBe("");
  });

  it("a stale older socket closing never tears down the newer session", async () => {
    const { connect } = await import("./control");
    const clientA = makeClient();
    const clientB = makeClient();
    vi.mocked(connect).mockResolvedValueOnce(clientA).mockResolvedValueOnce(clientB);

    const session = await import("./session");
    await session.connectHost("10.0.0.1", 7777);
    await session.connectHost("10.0.0.2", 7777);
    expect(session.controlClient()).toBe(clientB);

    clientA.emitClosed();
    expect(session.controlClient()).toBe(clientB);
  });

  it("an unexpected close frees the auto-reconnect gate (hasClient false)", async () => {
    const { connect } = await import("./control");
    // auto-reconnect는 이 파일에서 목(mock)으로 대체되므로 실물 판정은 importActual로.
    const { shouldAutoReconnect } = (await vi.importActual(
      "./auto-reconnect",
    )) as typeof import("./auto-reconnect");
    const client = makeClient();
    vi.mocked(connect).mockResolvedValue(client);

    const session = await import("./session");
    await session.connectHost("10.0.0.1", 7777);
    client.emitClosed();

    expect(
      shouldAutoReconnect({
        hasClient: session.controlClient() !== null,
        hasRecentHost: true,
        userDisconnected: false,
        pairingStale: false,
        lastAttemptAt: null,
        now: Date.now(),
      }),
    ).toBe(true);
  });
});


describe("selection-owned shared reconnect", () => {
  it.each([true, false])("cancels only the signaled waiter (first waiter: %s)", async cancelFirst => {
    const first = makeClient();
    const replacement = makeClient();
    const ready = deferred<ControlClient>();
    const { connect } = await import("./control");
    vi.mocked(connect).mockResolvedValueOnce(first).mockImplementationOnce(() => ready.promise);
    const session = await import("./session");
    await session.connectHost("10.0.0.5");
    const origin = session.captureRequestContext();
    first.emitClosed();
    const cancellation = new AbortController();
    let cancelled: unknown;
    const signaled = cancelFirst ? session.reconnectHost(origin, cancellation.signal) : undefined;
    const survivor = session.reconnectHost(origin);
    const cancelledWaiter = (signaled ?? session.reconnectHost(origin, cancellation.signal)).catch(error => {
      cancelled = error;
      return error;
    });
    await vi.waitFor(() => expect(connect).toHaveBeenCalledTimes(2));
    cancellation.abort();
    for (let i = 0; i < 12; i++) await Promise.resolve();
    const beforeCompletion = cancelled;
    // Even after an early cancellation, a third waiter joins the same attempt.
    const joined = session.reconnectHost(origin).catch(error => error);
    ready.resolve(replacement);
    const survivors = await Promise.all([survivor.catch(error => error), joined]);
    await cancelledWaiter;

    expect(beforeCompletion).toMatchObject({ name: "AbortError" });
    expect(survivors).toEqual([replacement, replacement]);
    expect(connect).toHaveBeenCalledTimes(2);
    expect(session.controlClient()).toBe(replacement);
    expect(replacement.close).not.toHaveBeenCalled();
    session.disconnectHost();
  });
});
