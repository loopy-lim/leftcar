import { describe, expect, it, vi } from "vitest";

// i18n.ts는 expo-secure-store를 정적 import하므로 node 테스트 환경에서는
// 모듈 경계에서 가짜로 대체한다.
vi.mock("expo-secure-store", () => ({
  getItemAsync: vi.fn(async () => null),
  setItemAsync: vi.fn(async () => undefined),
}));

import { resolvePersistedLanguage } from "./i18n";

describe("resolvePersistedLanguage", () => {
  it("저장된 선택이 있으면 로케일보다 우선한다", () => {
    expect(resolvePersistedLanguage("en", "ko-KR")).toBe("en");
    expect(resolvePersistedLanguage("ko", "en-US")).toBe("ko");
  });

  it("저장값이 없으면 영어권 로케일만 영어로 시작한다", () => {
    expect(resolvePersistedLanguage(null, "en-US")).toBe("en");
    expect(resolvePersistedLanguage(null, "EN_sg")).toBe("en");
    expect(resolvePersistedLanguage(null, "ko-KR")).toBe("ko");
    expect(resolvePersistedLanguage(null, "fr-FR")).toBe("ko");
    expect(resolvePersistedLanguage(null, "ja-JP")).toBe("ko");
  });

  it("유효하지 않은 저장값은 로케일 감지로 귀결된다", () => {
    expect(resolvePersistedLanguage("english", "en-US")).toBe("en");
    expect(resolvePersistedLanguage("", "de-DE")).toBe("ko");
  });
});
