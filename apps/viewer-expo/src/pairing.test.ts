import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Socket interaction lives inside control.connect(); the test never touches a
// real react-native-tcp-socket — both native modules are mocked at the boundary.

// expo-constants is mocked too: the real package transitively loads
// expo-modules-core's raw TypeScript source, which the vitest/vite SSR
// transform cannot parse (native modules are boundaries in tests anyway).
vi.mock("expo-constants", () => ({
  default: { deviceName: "Galaxy XR 테스트" },
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

const requestMock = vi.fn();
const closeMock = vi.fn();

vi.mock("./control", () => ({
  connect: vi.fn(async () => ({
    request: requestMock,
    close: closeMock,
  })),
  isUnauthorizedError: vi.fn(
    (e: unknown) =>
      e instanceof Error && e.name === "ControlRequestError" && e.message.includes("unauthorized"),
  ),
}));

import * as SecureStore from "expo-secure-store";
import { connect } from "./control";
import type { QrPayload } from "./pairing";
import {
  clearToken,
  deviceName,
  getDeviceId,
  getStoredToken,
  pairWithHost,
  parseQrPayload,
} from "./pairing";

const store = (SecureStore as unknown as { __store: Map<string, string> }).__store;

const validQr =
  '{"v":1,"id":"offer-x","s":"abc","h":"192.168.1.5","p":7777}';

function makePayload(): QrPayload {
  return { id: "offer-x", secret: "abc", host: "192.168.1.5", port: 7777 };
}

const TOKEN_64HEX = "a".repeat(64);

beforeEach(() => {
  store.clear();
  vi.clearAllMocks();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("parseQrPayload", () => {
  it("parseQrPayload_valid: extracts id/secret/host/port", () => {
    expect(parseQrPayload(validQr)).toEqual({
      id: "offer-x",
      secret: "abc",
      host: "192.168.1.5",
      port: 7777,
    });
  });

  it("parseQrPayload_wrong_version_and_missing_fields → null", () => {
    expect(parseQrPayload('{"v":2,"id":"o","s":"s","h":"1.2.3.4","p":7777}')).toBeNull();
    expect(
      parseQrPayload('{"v":1,"id":"o","h":"1.2.3.4","p":7777}'),
    ).toBeNull(); // missing s
    expect(
      parseQrPayload('{"v":1,"id":"o","s":"s","p":7777}'),
    ).toBeNull(); // missing h
    expect(parseQrPayload('{"v":1,"id":"o","s":"s","h":"1.2.3.4"}')).toBeNull(); // missing p
    expect(parseQrPayload('{"v":1,"id":"o","s":"s","h":"1.2.3.4","p":"7777"}')).toBeNull(); // port not number
    expect(parseQrPayload('{"v":1,"id":"o","s":"s","h":"1.2.3.4","p":7777.5}')).toBeNull(); // non-integer
    expect(parseQrPayload('{"v":1,"id":"o","s":"s","h":"1.2.3.4","p":0}')).toBeNull(); // out of range (low)
    expect(parseQrPayload('{"v":1,"id":"o","s":"s","h":"1.2.3.4","p":70000}')).toBeNull(); // out of range (high)
    expect(parseQrPayload("not json")).toBeNull();
  });
});

describe("getDeviceId", () => {
  it("generates and persists: second call returns the same id without re-creating", async () => {
    const first = await getDeviceId();
    expect(first).toBeTruthy();
    expect(store.get("leftcar.deviceId")).toBe(first);
    const second = await getDeviceId();
    expect(second).toBe(first);
    expect(SecureStore.setItemAsync).toHaveBeenCalledTimes(1);
  });
});

describe("pairWithHost", () => {
  it("success: stores token and returns it", async () => {
    requestMock.mockResolvedValueOnce({ token: TOKEN_64HEX });
    const token = await pairWithHost(makePayload(), "123456");
    expect(token).toBe(TOKEN_64HEX);
    expect(connect).toHaveBeenCalledWith("192.168.1.5", 7777);
    expect(requestMock).toHaveBeenCalledWith("pair", {
      offerId: "offer-x",
      secret: "abc",
      code: "123456",
      deviceId: store.get("leftcar.deviceId"),
      deviceName: deviceName(),
    });
    expect(store.get("leftcar.token")).toBe(TOKEN_64HEX);
    expect(closeMock).toHaveBeenCalledTimes(1); // no leaked connection
  });

  it("failure: throws and stores nothing", async () => {
    requestMock.mockRejectedValueOnce(new Error("pairing failed"));
    await expect(pairWithHost(makePayload(), "000000")).rejects.toThrow("pairing failed");
    expect(store.get("leftcar.token")).toBeUndefined();
    expect(closeMock).toHaveBeenCalledTimes(1); // closed even on failure
  });

  it("failure clears any previously stored token (defensive)", async () => {
    store.set("leftcar.token", "stale");
    requestMock.mockRejectedValueOnce(new Error("pairing failed"));
    await expect(pairWithHost(makePayload(), "000000")).rejects.toThrow("pairing failed");
    expect(store.get("leftcar.token")).toBeUndefined();
  });
});

describe("token storage", () => {
  it("getStoredToken_none_returns_null", async () => {
    expect(await getStoredToken()).toBeNull();
  });

  it("getStoredToken_returns_stored_value", async () => {
    store.set("leftcar.token", TOKEN_64HEX);
    expect(await getStoredToken()).toBe(TOKEN_64HEX);
  });

  it("clearToken_removes", async () => {
    store.set("leftcar.token", TOKEN_64HEX);
    await clearToken();
    expect(store.get("leftcar.token")).toBeUndefined();
    expect(await getStoredToken()).toBeNull();
  });
});

describe("deviceName", () => {
  it("is a sane non-empty string", () => {
    const name = deviceName();
    expect(typeof name).toBe("string");
    expect(name.length).toBeGreaterThan(0);
  });
});
