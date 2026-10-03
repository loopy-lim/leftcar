import { beforeEach, describe, expect, it, vi } from "vitest";

// control.ts는 react-native-tcp-socket을 정적 import한다. 소켓 구현은
// Flow 구문이라 node 변환기가 파싱하지 못하므로 모듈 경계를 가짜로 둔다.
vi.mock("react-native-tcp-socket", () => ({
  default: { createConnection: vi.fn() },
}));

vi.mock("./session", () => ({
  bindRequestContext: vi.fn(),
  captureRequestContext: vi.fn(() => null),
  controlHost: vi.fn(() => "10.0.0.1:7777"),
  reconnectHost: vi.fn(),
  isRequestContextCurrent: vi.fn(() => true),
}));

import { catalogErrorMessage, catalogMediaHost, isExternalRouteAddress, requestWithReconnect, requestForCurrentSelection } from "./catalog-helpers";
import { bindRequestContext, captureRequestContext, controlHost, isRequestContextCurrent, reconnectHost } from "./session";
import { AmbiguousControlError } from "./control-error";
import type { SessionRequestContext } from "./session";
import { setCurrentLanguage } from "./language-store";

describe("external route detection", () => {
  it("treats tailnet and public addresses as external", () => {
    expect(isExternalRouteAddress("100.80.133.120")).toBe(true);
    expect(isExternalRouteAddress("mac.example.ts.net")).toBe(true);
    expect(isExternalRouteAddress("203.0.113.9")).toBe(true);
  });

  it("treats LAN, loopback, and mDNS addresses as local", () => {
    expect(isExternalRouteAddress("192.168.0.7")).toBe(false);
    expect(isExternalRouteAddress("10.1.2.3")).toBe(false);
    expect(isExternalRouteAddress("leftcar-host.local")).toBe(false);
    expect(isExternalRouteAddress("localhost")).toBe(false);
    expect(isExternalRouteAddress(undefined)).toBe(false);
    expect(isExternalRouteAddress(null)).toBe(false);
    expect(isExternalRouteAddress("")).toBe(false);
  });
});

describe("catalog media route", () => {
  it("uses the connected numeric peer for MagicDNS and VPN-routed LAN control", () => {
    expect(catalogMediaHost("mac.tailnet.ts.net:7777", "192.168.0.134", "100.80.133.120")).toBe("100.80.133.120");
    expect(catalogMediaHost("192.168.0.134:7777", "192.168.0.134", "100.80.133.120")).toBe("100.80.133.120");
  });
  it.each(["100.80.133.120:7777", "100.64.0.1:7777", "100.127.255.254:7777", "mac.tailnet.ts.net:7777", "MAC.TAILNET.TS.NET.:7777"])(
    "keeps the selected Tailscale endpoint %s instead of the advertised LAN address", (endpoint) => {
      expect(catalogMediaHost(endpoint, "192.168.0.134")).toBe(endpoint.split(":")[0]);
    },
  );
  it.each(["192.168.0.134:7777", "127.0.0.1:7777", "mac.local:7777"])(
    "preserves the advertised media route for %s", (endpoint) => {
      expect(catalogMediaHost(endpoint, "192.168.0.134")).toBe("192.168.0.134");
    },
  );
  it("preserves local route over publicMediaEndpoint when connected on LAN", () => {
    expect(catalogMediaHost("192.168.0.134:7777", "192.168.0.134", undefined, "1.217.35.59:5001")).toBe("192.168.0.134");
  });
  it.each(["1.217.35.59:7777", "100.128.0.1:7777", "custom.host.com:7777"])(
    "keeps public WAN route %s instead of unreachable internal LAN advertisedHost", (endpoint) => {
      expect(catalogMediaHost(endpoint, "192.168.0.134")).toBe(endpoint.split(":")[0]);
    },
  );
  it("uses publicMediaEndpoint for public WAN connections", () => {
    expect(catalogMediaHost("1.217.35.59:7777", "192.168.0.134", undefined, "1.217.35.59:5001")).toBe("1.217.35.59");
    expect(catalogMediaHost("myhost.ddns.net:7777", "192.168.0.134", undefined, "1.217.35.59:5001")).toBe("1.217.35.59");
  });
  it("falls back to the control endpoint without an advertised address", () => {
    expect(catalogMediaHost("192.168.0.134:7777", " ")).toBe("192.168.0.134");
  });
});

describe("catalogErrorMessage 언어 전환", () => {
  it("SCShareableContent 지연 안내를 현재 언어로 내린다", () => {
    setCurrentLanguage("ko");
    expect(catalogErrorMessage(new Error("getCatalog failed: SCShareableContent timed out"))).toBe(
      "화면 소스 조회가 지연되고 있습니다. 잠시 후 새로고침을 눌러 주세요.",
    );
    setCurrentLanguage("en");
    expect(catalogErrorMessage(new Error("getCatalog failed: SCShareableContent timed out"))).toBe(
      "The screen source list is slow to load. Try refreshing again in a moment.",
    );
  });

  it("화면 공유 권한 안내를 현재 언어로 내린다", () => {
    setCurrentLanguage("ko");
    expect(
      catalogErrorMessage(new Error("getCatalog failed: screen-recording permission is not granted")),
    ).toBe("컴퓨터에서 화면 공유 권한이 꺼져 있습니다. Mac 시스템 설정에서 허용해 주세요.");
    setCurrentLanguage("en");
    expect(
      catalogErrorMessage(new Error("getCatalog failed: screen-recording permission is not granted")),
    ).toBe("Screen sharing is turned off on the computer. Allow it in macOS System Settings.");
  });

  it("매핑되지 않은 원문은 그대로 둔다", () => {
    setCurrentLanguage("en");
    expect(catalogErrorMessage(new Error("host said no"))).toBe("host said no");
  });
});

describe("requestWithReconnect 미연결 오류", () => {
  it("LocalizedError로 던지며 표시 시점 언어로 포맷된다", async () => {
    setCurrentLanguage("en");
    const error = await requestWithReconnect("getCatalog").then(
      () => null,
      (caught) => caught,
    );
    expect(error).toBeInstanceOf(Error);
    expect(catalogErrorMessage(error)).toBe("Not connected to the computer.");
    setCurrentLanguage("ko");
    expect(catalogErrorMessage(error)).toBe("컴퓨터에 연결되어 있지 않습니다");
  });

  it("binds an unauthorized response to the client and target that issued it", async () => {
    const unauthorized = new Error("unauthorized");
    const context = {
      client: { request: vi.fn(async () => { throw unauthorized; }), close: vi.fn(), whenClosed: vi.fn(), hostKey: null },
      target: { host: "10.0.0.1", port: 7777 },
      selectionGeneration: 7,
      identity: null,
      credential: null,
    };
    vi.mocked(captureRequestContext).mockReturnValue(context);

    await expect(requestWithReconnect("getCatalog")).rejects.toBe(unauthorized);

    expect(bindRequestContext).toHaveBeenCalledWith(unauthorized, context);
  });
});

it("source permission denial gives an actionable Host review and retry message", () => {
  setCurrentLanguage("ko");
  expect(catalogErrorMessage(new Error("source_access_denied"))).toBe("Host에서 이 기기의 화면 접근을 허용한 뒤 목록을 새로 고치세요.");
  setCurrentLanguage("en");
  expect(catalogErrorMessage(new Error("source_refresh_required"))).toContain("refresh the list");
});

it("a retained window starts a fresh operation after reconnecting to the same Host, while old operations stay cancelled", async () => {
  let requests = 0;
  const client = { request: async <T>() => { requests += 1; return "accepted" as T; }, close: vi.fn(), whenClosed: vi.fn(), hostKey: null };
  const context = { client, target: { host: "10.0.0.1", port: 7777 }, selectionGeneration: 10,
    identity: null, credential: null };
  vi.mocked(controlHost).mockReturnValue("10.0.0.1:7777");
  vi.mocked(captureRequestContext).mockReturnValue(context);
  const pending = requestForCurrentSelection("10.0.0.1:7777");
  vi.mocked(captureRequestContext).mockReturnValue({ ...context, selectionGeneration: 11 });
  await expect(pending("startStream")).rejects.toThrow("selection changed");
  await expect(requestForCurrentSelection("10.0.0.1:7777")("startStream")).resolves.toBe("accepted");
  const retained = requestForCurrentSelection("10.0.0.1:7777");
  vi.mocked(controlHost).mockReturnValue("10.0.0.2:7777");
  vi.mocked(captureRequestContext).mockReturnValue({ ...context, target: {host: "10.0.0.2", port: 7777}, selectionGeneration: 12 });
  await expect(retained("startStream")).rejects.toThrow("selection changed");
  await expect(requestForCurrentSelection("10.0.0.1:7777")("startStream")).rejects.toThrow("selection changed");
  expect(requests).toBe(1);
});

it("a retained window uses its selected target while the connected badge is cleared", async () => {
  let requests = 0;
  const request = async <T>() => { requests += 1; return "accepted" as T; };
  const context = { client: { request, close: vi.fn(), whenClosed: vi.fn(), hostKey: null },
    target: {host: "10.0.0.1", port: 7777}, selectionGeneration: 13, identity: null, credential: null };
  vi.mocked(controlHost).mockReturnValue("");
  vi.mocked(captureRequestContext).mockReturnValue(context);
  await expect(requestForCurrentSelection("10.0.0.1:7777")("getCatalog")).resolves.toBe("accepted");
  vi.mocked(isRequestContextCurrent).mockReturnValueOnce(false);
  await expect(requestForCurrentSelection("10.0.0.1:7777")("startStream")).rejects.toThrow("selection changed");
  expect(requests).toBe(1);
});


describe("transport replay admission", () => {
  beforeEach(() => {
    vi.mocked(isRequestContextCurrent).mockReturnValue(true);
    vi.mocked(reconnectHost).mockReset();
  });

  async function failedConnection() {
    const { ControlRequestError } = await import("./control");
    const failure = new ControlRequestError("control connection closed", "transport");
    const origin: SessionRequestContext = {
      client: { request: vi.fn(async () => { throw failure; }), close: vi.fn(), whenClosed: vi.fn(), hostKey: null },
      target: { host: "10.0.0.1", port: 7777 }, selectionGeneration: 22, identity: null, credential: null,
    };
    const replacement: SessionRequestContext = { ...origin, client: { ...origin.client,
      request: async <T>() => ({ accepted: true }) as T } };
    vi.spyOn(replacement.client, "request");
    vi.mocked(captureRequestContext).mockReturnValue(origin);
    vi.mocked(reconnectHost).mockImplementation(async () => {
      vi.mocked(captureRequestContext).mockReturnValue(replacement);
      return replacement.client;
    });
    return { failure, origin, replacement };
  }

  it.each(["startStream", "createVirtualDisplay", "sendFileBegin", "fetchFileBegin", "reconfigureStream", "unknownCommand"])(
    "does not silently repeat %s after an ambiguous connection failure", async command => {
      const { origin, replacement } = await failedConnection();
      setCurrentLanguage("en");
      const failure = await requestWithReconnect(command).catch(error => error);
      expect(failure).toBeInstanceOf(Error);
      expect(catalogErrorMessage(failure)).toContain("whether the operation completed");
      expect(replacement.client.request).not.toHaveBeenCalled();
      expect(origin.client.request).toHaveBeenCalledOnce();
      expect(bindRequestContext).toHaveBeenCalledWith(failure, origin);
    },
  );

  it("classifies a lost start acknowledgement timeout as uncertain without replay", async () => {
    const { origin, replacement } = await failedConnection();
    const { ControlRequestError } = await import("./control");
    const timeout = new ControlRequestError("control request timeout: startStream", "timeout");
    vi.mocked(origin.client.request).mockRejectedValueOnce(timeout);
    setCurrentLanguage("en");
    const error = await requestWithReconnect("startStream").catch(error => error);
    expect(catalogErrorMessage(error)).toContain("whether the operation completed");
    if (!(error instanceof AmbiguousControlError)) throw new Error("Expected uncertain completion");
    expect(error.cause).toBe(timeout);
    expect(reconnectHost).not.toHaveBeenCalled();
    expect(replacement.client.request).not.toHaveBeenCalled();
  });

  it.each(["getCatalog", "getStatus", "getClipboard", "listShareQueue", "fetchFileChunk"])(
    "replays safe read %s once on the verified replacement connection", async command => {
      const { origin, replacement } = await failedConnection();
      await expect(requestWithReconnect(command, { offset: 8 })).resolves.toEqual({ accepted: true });
      expect(reconnectHost).toHaveBeenCalledWith(origin);
      expect(replacement.client.request).toHaveBeenCalledExactlyOnceWith(command, { offset: 8 });
    },
  );
});
