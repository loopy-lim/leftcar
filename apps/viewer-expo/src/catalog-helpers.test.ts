import { describe, expect, it, vi } from "vitest";

// control.ts는 react-native-tcp-socket을 정적 import한다. 소켓 구현은
// Flow 구문이라 node 변환기가 파싱하지 못하므로 모듈 경계를 가짜로 둔다.
vi.mock("react-native-tcp-socket", () => ({
  default: { createConnection: vi.fn() },
}));

vi.mock("./session", () => ({
  bindRequestContext: vi.fn(),
  captureRequestContext: vi.fn(() => null),
  reconnectHost: vi.fn(),
}));

import { catalogErrorMessage, requestWithReconnect } from "./catalog-helpers";
import { bindRequestContext, captureRequestContext } from "./session";
import { setCurrentLanguage } from "./language-store";

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
      client: { request: vi.fn(async () => { throw unauthorized; }), close: vi.fn(), hostKey: null },
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
