import { describe, expect, it } from "vitest";
import { getTranslation, interpolate, translations } from "./i18n";

describe("i18n module", () => {
  it("provides complete translations for Korean and English", () => {
    const ko = getTranslation("ko");
    const en = getTranslation("en");

    expect(ko.host.headerTitle).toBe("화면 공유 호스트");
    expect(en.host.headerTitle).toBe("Screen Sharing Host");
  });

  it("has exact parity in keys between Korean and English dictionaries", () => {
    const checkKeysMatch = (objA: Record<string, unknown>, objB: Record<string, unknown>, path = "") => {
      const keysA = Object.keys(objA).sort();
      const keysB = Object.keys(objB).sort();

      expect(keysA, `Keys mismatch at ${path}`).toEqual(keysB);

      for (const key of keysA) {
        const valA = objA[key];
        const valB = objB[key];
        if (typeof valA === "object" && valA !== null) {
          expect(typeof valB).toBe("object");
          checkKeysMatch(
            valA as Record<string, unknown>,
            valB as Record<string, unknown>,
            path ? `${path}.${key}` : key,
          );
        } else {
          expect(typeof valB).toBe("string");
        }
      }
    };

    checkKeysMatch(translations.ko, translations.en);
  });

  it("interpolates template parameters correctly", () => {
    expect(interpolate("Port :{port} Copied!", { port: 7777 })).toBe("Port :7777 Copied!");
    expect(interpolate("Expires in {seconds}s", { seconds: 120 })).toBe("Expires in 120s");
    expect(interpolate("No params")).toBe("No params");
    expect(interpolate("Missing {param}")).toBe("Missing {param}");
  });
});
