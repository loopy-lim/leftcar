import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("react-native", () => ({ Alert: { alert: vi.fn() } }));
vi.mock("expo-router", () => ({ router: { push: vi.fn(), replace: vi.fn() } }));
vi.mock("expo-constants", () => ({ default: { deviceName: "Android viewer test" } }));
vi.mock("./auto-reconnect", () => ({ markConnected: vi.fn(), markPairingStale: vi.fn() }));
vi.mock("./usb", () => ({
  getUsbState: vi.fn(async () => ({ attached: false, controlPort: 0 })),
}));
vi.mock("expo-secure-store", () => {
  const store = new Map<string, string>();
  return {
    getItemAsync: vi.fn(async (key: string) => store.get(key) ?? null),
    setItemAsync: vi.fn(async (key: string, value: string) => { store.set(key, value); }),
    deleteItemAsync: vi.fn(async (key: string) => { store.delete(key); }),
    __store: store,
  };
});

const { connectMock } = vi.hoisted(() => ({ connectMock: vi.fn() }));
vi.mock("./control", () => ({
  connect: connectMock,
  ControlRequestError: class extends Error {
    constructor(message: string, readonly kind: string) {
      super(message);
      this.name = "ControlRequestError";
    }
  },
}));

import * as SecureStore from "expo-secure-store";
import { router } from "expo-router";
import type { ConnectOptions, ControlClient } from "./control";
import {
  PairingWorkflow,
  handleUnauthorized,
  runPinPairingWorkflow,
  runQrPairingWorkflow,
} from "./connect-flow";
import {
  connectHost,
  controlClient,
  controlHost,
  captureRequestContext,
  disconnectHost,
} from "./session";
import { getPinnedHostKey, resetPinnedHostKeysForTests } from "./pinned-host-keys";
import type { QrPayload } from "./pairing";
import { getUsbState } from "./usb";

const store = (SecureStore as unknown as { __store: Map<string, string> }).__store;
const HOST_KEY_A = "A".repeat(43);
const HOST_KEY_B = "B".repeat(43);
const TOKEN_A = "a".repeat(64);
const TOKEN_B = "b".repeat(64);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

function makeClient(
  hostKey: string | null = null,
  request: ControlClient["request"] = vi.fn(async () => ({})) as ControlClient["request"],
): ControlClient {
  return { request, close: vi.fn(), hostKey };
}

function payload(id: string, host: string, hostKey: string): QrPayload {
  return { id, secret: hostKey, hostKey, host, port: 7777 };
}

beforeEach(() => {
  disconnectHost();
  resetPinnedHostKeysForTests();
  store.clear();
  vi.clearAllMocks();
  connectMock.mockReset();
  vi.mocked(SecureStore.getItemAsync).mockImplementation(async (key) => store.get(key) ?? null);
  vi.mocked(SecureStore.setItemAsync).mockImplementation(async (key, value) => {
    store.set(key, value);
  });
  vi.mocked(SecureStore.deleteItemAsync).mockImplementation(async (key) => {
    store.delete(key);
  });
  vi.mocked(getUsbState).mockReset();
  vi.mocked(getUsbState).mockResolvedValue({ attached: false, controlPort: 0 });
});

describe("unauthorized request origin", () => {
  it("screen departure during credential retirement prevents late navigation", async () => {
    store.set("leftcar.token.v2.10.0.0.1.7777", TOKEN_A);
    const client = makeClient();
    connectMock.mockResolvedValueOnce(client);
    await connectHost("10.0.0.1", 7777);
    const paused = deferred<void>();
    vi.mocked(SecureStore.deleteItemAsync).mockImplementation(async key => {
      await paused.promise;
      store.delete(key);
    });
    const action = new AbortController();
    const beforeNavigate = vi.fn();
    const result = handleUnauthorized({ context: captureRequestContext(), signal: action.signal,
      beforeNavigate, navigate: { endpoint: "10.0.0.1:7777" } });
    await vi.waitFor(() => expect(SecureStore.deleteItemAsync).toHaveBeenCalled());
    action.abort();
    paused.resolve();
    await result;
    expect(beforeNavigate).not.toHaveBeenCalled();
    expect(router.push).not.toHaveBeenCalled();
    expect(controlClient()).toBe(client);
  });

  it("uses the actual A to B session transition before retiring only A", async () => {
    store.set("leftcar.token.v2.10.0.0.1.7777", TOKEN_A);
    store.set("leftcar.token.v2.10.0.0.2.7777", TOKEN_B);
    const clientA = makeClient();
    const clientB = makeClient();
    connectMock.mockResolvedValueOnce(clientA).mockResolvedValueOnce(clientB);

    await connectHost("10.0.0.1", 7777);
    const oldA = captureRequestContext();
    await connectHost("10.0.0.2", 7777);
    await handleUnauthorized({ context: oldA, navigate: { endpoint: "10.0.0.1:7777" } });

    expect(store.get("leftcar.token.v2.10.0.0.1.7777")).toBeUndefined();
    expect(store.get("leftcar.token.v2.10.0.0.2.7777")).toBe(TOKEN_B);
    expect(controlClient()).toBe(clientB);
    expect(controlHost()).toBe("10.0.0.2:7777");
    expect(router.push).not.toHaveBeenCalled();
  });

  it("a tokenless old A context cannot delete A's newly paired incarnation", async () => {
    const oldClient = makeClient();
    const pairClient = makeClient(
      HOST_KEY_A,
      vi.fn(async () => ({ token: TOKEN_A })) as ControlClient["request"],
    );
    const newClient = makeClient(HOST_KEY_A);
    connectMock
      .mockResolvedValueOnce(oldClient)
      .mockResolvedValueOnce(pairClient)
      .mockResolvedValueOnce(newClient);

    await connectHost("10.0.0.1", 7777);
    const tokenlessOldA = captureRequestContext();
    expect(tokenlessOldA?.credential).toBeNull();

    const workflow = new PairingWorkflow();
    const run = workflow.beginPin();
    await runPinPairingWorkflow({
      run,
      target: { host: "10.0.0.1", port: 7777 },
      code: "123456",
      navigate: vi.fn(),
    });
    workflow.finish(run);
    expect(captureRequestContext()?.credential?.token).toBe(TOKEN_A);

    await handleUnauthorized({ context: tokenlessOldA });

    expect(store.get("leftcar.token.v2.10.0.0.1.7777")).toBe(TOKEN_A);
    expect(store.get(`leftcar.token.v3.${HOST_KEY_A}`)).toBe(TOKEN_A);
    expect(controlClient()).toBe(newClient);
  });

  it("explicitly unknown origin does not resolve or disconnect a mutable current target", async () => {
    store.set("leftcar.token.v2.10.0.0.2.7777", TOKEN_B);
    const clientB = makeClient();
    connectMock.mockResolvedValueOnce(clientB);
    await connectHost("10.0.0.2", 7777);

    await handleUnauthorized({ context: null, navigate: { endpoint: "10.0.0.1:7777" } });

    expect(store.get("leftcar.token.v2.10.0.0.2.7777")).toBe(TOKEN_B);
    expect(controlClient()).toBe(clientB);
    expect(router.push).not.toHaveBeenCalled();
  });
});

describe("production pairing workflow", () => {
  it("deduplicates the same offer during approval and authenticated connect", async () => {
    const qr = payload("offer-123e4567-e89b-42d3-a456-426614174000", "10.0.0.1", HOST_KEY_A);
    const approval = deferred<{ token: string }>();
    const approvalClient = makeClient(
      HOST_KEY_A,
      vi.fn(() => approval.promise) as ControlClient["request"],
    );
    const connected = deferred<ControlClient>();
    const sessionClient = makeClient(HOST_KEY_A);
    connectMock.mockResolvedValueOnce(approvalClient).mockImplementationOnce(() => connected.promise);
    const navigate = vi.fn();
    const workflow = new PairingWorkflow();
    const run = workflow.beginQr(qr)!;
    const pending = runQrPairingWorkflow({ run, payload: qr, navigate });
    await vi.waitFor(() => expect(approvalClient.request).toHaveBeenCalledTimes(1));

    expect(workflow.beginQr(qr)).toBeNull();
    expect(run.attempt.signal.aborted).toBe(false);
    approval.resolve({ token: TOKEN_A });
    await vi.waitFor(() => expect(connectMock).toHaveBeenCalledTimes(2));

    expect(workflow.beginQr(qr)).toBeNull();
    expect(run.attempt.signal.aborted).toBe(false);
    connected.resolve(sessionClient);
    await pending;

    expect(workflow.finish(run)).toBe(true);
    expect(navigate).toHaveBeenCalledTimes(1);
    expect(store.get(`leftcar.token.v3.${HOST_KEY_A}`)).toBe(TOKEN_A);
  });

  it("a different offer supersedes the old request without storing its late token", async () => {
    const qrA = payload("offer-123e4567-e89b-42d3-a456-426614174000", "10.0.0.1", HOST_KEY_A);
    const qrB = payload("offer-223e4567-e89b-42d3-a456-426614174000", "10.0.0.2", HOST_KEY_B);
    const approvalA = deferred<{ token: string }>();
    const approvalClientA = makeClient(
      HOST_KEY_A,
      vi.fn(() => approvalA.promise) as ControlClient["request"],
    );
    const approvalClientB = makeClient(
      HOST_KEY_B,
      vi.fn(async () => ({ token: TOKEN_B })) as ControlClient["request"],
    );
    const sessionClientB = makeClient(HOST_KEY_B);
    connectMock
      .mockResolvedValueOnce(approvalClientA)
      .mockResolvedValueOnce(approvalClientB)
      .mockResolvedValueOnce(sessionClientB);
    const workflow = new PairingWorkflow();
    const navigateA = vi.fn();
    const navigateB = vi.fn();
    const runA = workflow.beginQr(qrA)!;
    const pendingA = runQrPairingWorkflow({ run: runA, payload: qrA, navigate: navigateA });
    await vi.waitFor(() => expect(approvalClientA.request).toHaveBeenCalledTimes(1));

    const runB = workflow.beginQr(qrB)!;
    const pendingB = runQrPairingWorkflow({ run: runB, payload: qrB, navigate: navigateB });
    await pendingB;
    approvalA.resolve({ token: TOKEN_A });
    await expect(pendingA).rejects.toMatchObject({ name: "AbortError" });

    expect(workflow.finish(runA)).toBe(false);
    expect(workflow.finish(runB)).toBe(true);
    expect(navigateA).not.toHaveBeenCalled();
    expect(navigateB).toHaveBeenCalledTimes(1);
    expect(store.get(`leftcar.token.v3.${HOST_KEY_A}`)).toBeUndefined();
    expect(store.get(`leftcar.token.v3.${HOST_KEY_B}`)).toBe(TOKEN_B);
  });

  it("a new PIN run supersedes an approval request and owns the resulting session", async () => {
    const qrA = payload("offer-123e4567-e89b-42d3-a456-426614174000", "10.0.0.1", HOST_KEY_A);
    const approvalA = deferred<{ token: string }>();
    const approvalClientA = makeClient(
      HOST_KEY_A,
      vi.fn(() => approvalA.promise) as ControlClient["request"],
    );
    const pinClientB = makeClient(
      HOST_KEY_B,
      vi.fn(async () => ({ token: TOKEN_B })) as ControlClient["request"],
    );
    const sessionClientB = makeClient(HOST_KEY_B);
    connectMock
      .mockResolvedValueOnce(approvalClientA)
      .mockResolvedValueOnce(pinClientB)
      .mockResolvedValueOnce(sessionClientB);
    const workflow = new PairingWorkflow();
    const navigateA = vi.fn();
    const navigateB = vi.fn();
    const runA = workflow.beginQr(qrA)!;
    const pendingA = runQrPairingWorkflow({ run: runA, payload: qrA, navigate: navigateA });
    await vi.waitFor(() => expect(approvalClientA.request).toHaveBeenCalledTimes(1));

    const runB = workflow.beginPin();
    await runPinPairingWorkflow({
      run: runB,
      target: { host: "10.0.0.2", port: 7777 },
      code: "123456",
      navigate: navigateB,
    });
    approvalA.resolve({ token: TOKEN_A });
    await expect(pendingA).rejects.toMatchObject({ name: "AbortError" });

    expect(workflow.finish(runA)).toBe(false);
    expect(workflow.finish(runB)).toBe(true);
    expect(navigateA).not.toHaveBeenCalled();
    expect(navigateB).toHaveBeenCalledTimes(1);
    expect(controlClient()).toBe(sessionClientB);
    expect(store.get(`leftcar.token.v3.${HOST_KEY_A}`)).toBeUndefined();
    expect(store.get(`leftcar.token.v3.${HOST_KEY_B}`)).toBe(TOKEN_B);
  });

  it("PIN cleanup during authenticated connect rolls back storage and closes the late socket", async () => {
    const pairClient = makeClient(
      HOST_KEY_A,
      vi.fn(async () => ({ token: TOKEN_A })) as ControlClient["request"],
    );
    const sessionClient = makeClient(HOST_KEY_A);
    const connected = deferred<ControlClient>();
    connectMock.mockResolvedValueOnce(pairClient).mockImplementationOnce(() => connected.promise);
    const workflow = new PairingWorkflow();
    const run = workflow.beginPin();
    const navigate = vi.fn();
    const pending = runPinPairingWorkflow({
      run,
      target: { host: "10.0.0.1", port: 7777 },
      code: "123456",
      navigate,
    });
    await vi.waitFor(() => expect(connectMock).toHaveBeenCalledTimes(2));

    await workflow.cancel();
    connected.resolve(sessionClient);
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });

    expect(store.get("leftcar.token.v2.10.0.0.1.7777")).toBeUndefined();
    expect(store.get(`leftcar.token.v3.${HOST_KEY_A}`)).toBeUndefined();
    expect(store.get("leftcar.recent_hosts")).toBeUndefined();
    expect(sessionClient.close).toHaveBeenCalledTimes(1);
    expect(navigate).not.toHaveBeenCalled();
  });

  it("QR cleanup during authenticated connect rolls back the approval result", async () => {
    const qr = payload("offer-123e4567-e89b-42d3-a456-426614174000", "10.0.0.1", HOST_KEY_A);
    const approvalClient = makeClient(
      HOST_KEY_A,
      vi.fn(async () => ({ token: TOKEN_A })) as ControlClient["request"],
    );
    const sessionClient = makeClient(HOST_KEY_A);
    const connected = deferred<ControlClient>();
    connectMock.mockResolvedValueOnce(approvalClient).mockImplementationOnce(() => connected.promise);
    const workflow = new PairingWorkflow();
    const run = workflow.beginQr(qr)!;
    const navigate = vi.fn();
    const pending = runQrPairingWorkflow({ run, payload: qr, navigate });
    await vi.waitFor(() => expect(connectMock).toHaveBeenCalledTimes(2));

    await workflow.cancel();
    connected.resolve(sessionClient);
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });

    expect(store.get("leftcar.token.v2.10.0.0.1.7777")).toBeUndefined();
    expect(store.get(`leftcar.token.v3.${HOST_KEY_A}`)).toBeUndefined();
    expect(store.get("leftcar.recent_hosts")).toBeUndefined();
    expect(sessionClient.close).toHaveBeenCalledTimes(1);
    expect(navigate).not.toHaveBeenCalled();
  });

  it("PIN cleanup during USB discovery cannot start a network handshake or restore its pin", async () => {
    const pairClient = makeClient(
      HOST_KEY_A,
      vi.fn(async () => ({ token: TOKEN_A })) as ControlClient["request"],
    );
    const sessionClient = makeClient(HOST_KEY_A);
    const discovery = deferred<{ attached: boolean; controlPort: number }>();
    vi.mocked(getUsbState).mockImplementationOnce(() => discovery.promise);
    connectMock
      .mockResolvedValueOnce(pairClient)
      .mockImplementationOnce(
        async (
          _host: string,
          _port: number,
          _timeoutMs: number,
          _tokenProvider: unknown,
          options?: ConnectOptions,
        ) => {
          options?.onHostKey?.(HOST_KEY_A);
          return sessionClient;
        },
      );
    const workflow = new PairingWorkflow();
    const run = workflow.beginPin();
    const navigate = vi.fn();
    const pending = runPinPairingWorkflow({
      run,
      target: { host: "10.0.0.1", port: 7777 },
      code: "123456",
      navigate,
    });
    await vi.waitFor(() => expect(getUsbState).toHaveBeenCalledTimes(1));
    expect(getPinnedHostKey("10.0.0.1", 7777)).toBe(HOST_KEY_A);

    await workflow.cancel();
    expect(getPinnedHostKey("10.0.0.1", 7777)).toBeNull();
    discovery.resolve({ attached: false, controlPort: 0 });
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });

    expect(connectMock).toHaveBeenCalledTimes(1);
    expect(getPinnedHostKey("10.0.0.1", 7777)).toBeNull();
    expect(store.get("leftcar.recent_hosts")).toBeUndefined();
    expect(sessionClient.close).not.toHaveBeenCalled();
    expect(navigate).not.toHaveBeenCalled();
  });

  it("a canceled PIN handshake callback cannot publish a newly verified pin", async () => {
    const pairClient = makeClient(
      null,
      vi.fn(async () => ({ token: TOKEN_A })) as ControlClient["request"],
    );
    const sessionClient = makeClient(HOST_KEY_A);
    const connected = deferred<ControlClient>();
    const handshakeStarted = deferred<ConnectOptions>();
    connectMock
      .mockResolvedValueOnce(pairClient)
      .mockImplementationOnce(
        (
          _host: string,
          _port: number,
          _timeoutMs: number,
          _tokenProvider: unknown,
          options?: ConnectOptions,
        ) => {
          if (!options) throw new Error("expected secure connection options");
          handshakeStarted.resolve(options);
          return connected.promise;
        },
      );
    const workflow = new PairingWorkflow();
    const run = workflow.beginPin();
    const navigate = vi.fn();
    const pending = runPinPairingWorkflow({
      run,
      target: { host: "10.0.0.1", port: 7777 },
      code: "123456",
      navigate,
    });
    const options = await handshakeStarted.promise;
    expect(options.pinnedHostKey).toBeNull();
    expect(options.onHostKey).toBeTypeOf("function");

    await workflow.cancel();
    options.onHostKey!(HOST_KEY_A);
    connected.resolve(sessionClient);
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });

    expect(getPinnedHostKey("10.0.0.1", 7777)).toBeNull();
    expect(store.get("leftcar.token.v2.10.0.0.1.7777")).toBeUndefined();
    expect(store.get("leftcar.recent_hosts")).toBeUndefined();
    expect(sessionClient.close).toHaveBeenCalledTimes(1);
    expect(navigate).not.toHaveBeenCalled();
  });
});
