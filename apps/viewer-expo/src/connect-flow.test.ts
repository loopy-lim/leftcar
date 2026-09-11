import { beforeEach, describe, expect, it, vi } from "vitest";

// handleUnauthorized는 Alert(react-native)·router(expo-router)·세션 상태에
// 의존한다 — 그 경계들만 모의하고 pairing 토큰 로직은 메모리 SecureStore로
// 실제 모듈을 쓴다. ./control도 모의한다(pairing의 전이 의존인 tcp-socket이
// 노드 환경에서 로드되지 않도록 — pairing.test.ts와 같은 경계다).
vi.mock("react-native", () => ({
  Alert: { alert: vi.fn() },
}));
vi.mock("expo-router", () => ({
  router: { push: vi.fn(), replace: vi.fn() },
}));
// pairing.ts는 expo-constants를 정적 import한다 — 실제 패키지는
// expo-modules-core(네이티브 전역)를 끌어오므로 경계에서 모의한다.
vi.mock("expo-constants", () => ({
  default: { deviceName: "Android viewer test" },
}));
vi.mock("./auto-reconnect", () => ({
  markPairingStale: vi.fn(),
}));
vi.mock("./session", () => ({
  controlTarget: vi.fn(() => null),
  disconnectHost: vi.fn(),
}));
vi.mock("./control", () => ({
  connect: vi.fn(),
  ControlRequestError: class extends Error {
    constructor(
      message: string,
      readonly kind: string,
    ) {
      super(message);
    }
  },
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

import * as SecureStore from "expo-secure-store";
import { router } from "expo-router";
import { Alert } from "react-native";
import { controlTarget, disconnectHost } from "./session";
import { handleUnauthorized } from "./connect-flow";

const store = (SecureStore as unknown as { __store: Map<string, string> }).__store;

beforeEach(() => {
  store.clear();
  vi.clearAllMocks();
  vi.mocked(controlTarget).mockReturnValue(null);
});

describe("handleUnauthorized token scoping", () => {
  it("clears only the current control target's token", async () => {
    store.set("leftcar.token.v2.10.0.0.1.7777", "a".repeat(64));
    store.set("leftcar.token.v2.10.0.0.2.7777", "b".repeat(64));
    vi.mocked(controlTarget).mockReturnValue({ host: "10.0.0.2", port: 7777 });

    await handleUnauthorized();

    expect(store.get("leftcar.token.v2.10.0.0.2.7777")).toBeUndefined();
    // 다른 호스트(B가 아니라 A)의 토큰은 살아 있어야 한다.
    expect(store.get("leftcar.token.v2.10.0.0.1.7777")).toBe("a".repeat(64));
    expect(disconnectHost).toHaveBeenCalledTimes(1);
  });

  it("skips token clearing when there is no control session", async () => {
    vi.mocked(controlTarget).mockReturnValue(null);

    await handleUnauthorized();

    expect(SecureStore.deleteItemAsync).not.toHaveBeenCalled();
    expect(disconnectHost).toHaveBeenCalledTimes(1);
  });

  it("alerts and navigates to the pairing screen with the caller's endpoint", async () => {
    vi.mocked(controlTarget).mockReturnValue({ host: "10.0.0.2", port: 7777 });

    await handleUnauthorized({ navigate: { endpoint: "10.0.0.2:7777" } });

    expect(Alert.alert).toHaveBeenCalledTimes(1);
    expect(router.push).toHaveBeenCalledWith({
      pathname: "/pairing",
      params: { endpoint: "10.0.0.2:7777" },
    });
    expect(store.get("leftcar.token.v2.10.0.0.2.7777")).toBeUndefined();
  });
});
