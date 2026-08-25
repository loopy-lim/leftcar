import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Socket interaction lives inside control.connect(); the test never touches a
// real react-native-tcp-socket — both native modules are mocked at the boundary.

// expo-constants is mocked too: the real package transitively loads
// expo-modules-core's raw TypeScript source, which the vitest/vite SSR
// transform cannot parse (native modules are boundaries in tests anyway).
vi.mock("expo-constants", () => ({
  default: { deviceName: "Android 뷰어 테스트" },
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
  isTrustedHost,
  pairWithHost,
  formatHostEndpoint,
  parseHostEndpoint,
  parseQrPayload,
} from "./pairing";

const store = (SecureStore as unknown as { __store: Map<string, string> }).__store;

const OFFER_ID = "offer-123e4567-e89b-42d3-a456-426614174000";
const OFFER_SECRET = "A".repeat(43);
const validQr =
  `{"v":1,"id":"${OFFER_ID}","s":"${OFFER_SECRET}","h":"192.168.1.5","p":7777}`;

function makePayload(): QrPayload {
  return { id: OFFER_ID, secret: OFFER_SECRET, host: "192.168.1.5", port: 7777 };
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
      id: OFFER_ID,
      secret: OFFER_SECRET,
      host: "192.168.1.5",
      port: 7777,
    });
  });

  it("never trusts a code embedded in a QR payload", () => {
    const qrWithCode =
      `{"v":1,"id":"${OFFER_ID}","s":"${OFFER_SECRET}","h":"192.168.1.5","p":7777,"c":"123456"}`;
    expect(parseQrPayload(qrWithCode)).toEqual({
      id: OFFER_ID,
      secret: OFFER_SECRET,
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

describe("host endpoint", () => {
  it("roundtrips the selected LAN endpoint for the pairing route", () => {
    const routeEndpoint = formatHostEndpoint("192.168.0.134", 7777);
    expect(parseHostEndpoint(routeEndpoint)).toEqual({ host: "192.168.0.134", port: 7777 });
  });

  it("uses the control default port only for an explicit host", () => {
    expect(parseHostEndpoint("192.168.0.134")).toEqual({
      host: "192.168.0.134",
      port: 7777,
    });
    expect(parseHostEndpoint("localhost:7777")).toEqual({ host: "localhost", port: 7777 });
  });

  it("never invents localhost when the target is missing or malformed", () => {
    expect(parseHostEndpoint("")).toBeNull();
    expect(parseHostEndpoint("   ")).toBeNull();
    expect(parseHostEndpoint(":7777")).toBeNull();
    expect(parseHostEndpoint("192.168.0.134:not-a-port")).toBeNull();
    expect(parseHostEndpoint("192.168.0.134:0")).toBeNull();
  });

  it("accepts private and Tailscale targets but rejects public internet hosts", () => {
    expect(isTrustedHost("10.0.0.5")).toBe(true);
    expect(isTrustedHost("100.100.20.30")).toBe(true);
    expect(isTrustedHost("my-mac.example.ts.net")).toBe(true);
    expect(isTrustedHost("8.8.8.8")).toBe(false);
    expect(isTrustedHost("example.com")).toBe(false);
    expect(parseHostEndpoint("8.8.8.8:7777")).toBeNull();
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
      offerId: OFFER_ID,
      secret: OFFER_SECRET,
      code: "123456",
      deviceId: store.get("leftcar.deviceId"),
      deviceName: deviceName(),
    });
    expect(store.get("leftcar.token")).toBe(TOKEN_64HEX);
    expect(closeMock).toHaveBeenCalledTimes(1); // no leaked connection
  });

  it("requires the separately displayed code", async () => {
    await expect(pairWithHost(makePayload(), "")).rejects.toThrow(
      "6자리 인증 코드를 정확히 입력해 주세요",
    );
    expect(connect).not.toHaveBeenCalled();
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

  it("rejects a malformed issued token instead of storing it", async () => {
    requestMock.mockResolvedValueOnce({ token: "not-a-valid-token" });
    await expect(pairWithHost(makePayload(), "123456")).rejects.toThrow(
      "연결 승인 응답을 확인할 수 없습니다",
    );
    expect(store.get("leftcar.token")).toBeUndefined();
    expect(closeMock).toHaveBeenCalledTimes(1);
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
