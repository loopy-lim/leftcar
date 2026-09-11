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

beforeEach(() => {
  // allocPort/allocPorts의 nextPort 모듈 상태를 테스트마다 초기화한다.
  vi.resetModules();
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

function makeClient(): ControlClient {
  return {
    request: vi.fn(async () => ({})) as ControlClient["request"],
    close: vi.fn(),
    hostKey: null,
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
  it("sends each host its own stored token across an A→B→A round-trip", async () => {
    (await secureStore()).set("leftcar.token.v2.10.0.0.1.7777", "a".repeat(64));
    (await secureStore()).set("leftcar.token.v2.10.0.0.2.7777", "b".repeat(64));

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
    session.disconnectHost();
    gate.resolve(clientA);

    await expect(pending).rejects.toMatchObject({ name: "LocalizedError" });
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
    const wonB = await session.connectHost("10.0.0.2", 7778);
    expect(wonB).toBe(clientB);
    expect(session.controlClient()).toBe(clientB);
    expect(session.controlHost()).toBe("10.0.0.2:7778");
    expect(clientA.close).not.toHaveBeenCalled();

    gateA.resolve(clientA);
    await expect(pendingA).rejects.toMatchObject({ name: "LocalizedError" });
    expect(clientA.close).toHaveBeenCalledTimes(1);
    expect(clientB.close).not.toHaveBeenCalled();
    expect(session.controlClient()).toBe(clientB);
    expect(session.controlHost()).toBe("10.0.0.2:7778");
  });
});
