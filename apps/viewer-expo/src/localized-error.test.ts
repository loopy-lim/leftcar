import { describe, expect, it, vi } from "vitest";

// control.ts는 react-native-tcp-socket을 정적 import한다. 소켓 구현은
// Flow 구문이라 node 변환기가 파싱하지 못하므로 모듈 경계를 가짜로 둔다.
vi.mock("react-native-tcp-socket", () => ({
  default: { createConnection: vi.fn() },
}));

import { LocalizedError } from "./localized-error";
import { setCurrentLanguage } from "./language-store";
import { formatErrorMessage } from "./control";

describe("LocalizedError", () => {
  it("표시 시점 언어로 포맷된다", () => {
    setCurrentLanguage("ko");
    expect(new LocalizedError("errPairingCodeInvalid").format()).toBe(
      "6자리 인증 코드를 정확히 입력해 주세요.",
    );
    setCurrentLanguage("en");
    expect(new LocalizedError("errPairingCodeInvalid").format()).toBe(
      "Enter the 6-digit pairing code correctly.",
    );
  });

  it("파라미터와 원인 상세를 반영한다", () => {
    setCurrentLanguage("en");
    expect(
      new LocalizedError("errResizeFailed", { detail: "timeout" }).format(),
    ).toBe("Resolution change failed: timeout");

    expect(
      new LocalizedError("errUdpApply", { detail: "code=7" }).format(),
    ).toBe("Could not apply UDP settings: code=7");
  });
});

describe("formatErrorMessage 언어 전환", () => {
  it("기존 휴리스틱 분기도 현재 언어를 따른다", () => {
    setCurrentLanguage("ko");
    expect(formatErrorMessage(new Error("request unauthorized"))).toBe(
      "컴퓨터의 연결 승인이 필요합니다.",
    );
    expect(formatErrorMessage(new Error("connect timeout to 1.2.3.4:7777"))).toContain(
      "컴퓨터가 응답하지 않습니다",
    );

    setCurrentLanguage("en");
    expect(formatErrorMessage(new Error("request unauthorized"))).toBe(
      "Pairing approval from the computer is required.",
    );
    expect(formatErrorMessage(new Error("connect timeout to 1.2.3.4:7777"))).toContain(
      "The computer is not responding",
    );
    expect(formatErrorMessage(null)).toBe("Something went wrong. Please try again in a moment.");
  });

  it("한글 원문 패스스루와 LocalizedError 우선순위를 유지한다", () => {
    setCurrentLanguage("en");
    // 호스트가 내려준 한국어 안내는 큐레이된 문구로 그대로 노출한다.
    expect(formatErrorMessage(new Error("네트워크 상태가 불안정합니다"))).toBe(
      "네트워크 상태가 불안정합니다",
    );
    // LocalizedError는 휴리스틱보다 우선한다.
    const localized = new LocalizedError("errGeneric");
    expect(formatErrorMessage(localized)).toBe(
      "Something went wrong. Please try again in a moment.",
    );
  });
});
