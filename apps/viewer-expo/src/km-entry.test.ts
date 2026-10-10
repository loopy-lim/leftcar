import { describe, expect, it } from "vitest";
import { shouldShowKmEntry } from "./km-entry";
describe("input-sharing Stop access", () => {
  it("keeps the input panel reachable after video control disconnects while native sharing is active", () => {
    expect(shouldShowKmEntry(false, "armed")).toBe(true);
    expect(shouldShowKmEntry(false, "macToAndroid")).toBe(true);
    expect(shouldShowKmEntry(false, "androidToMac")).toBe(true);
    expect(shouldShowKmEntry(false, "preparing")).toBe(true);
  });
  it("offers sharing on a selected host without claiming an unknown disconnected native session", () => {
    expect(shouldShowKmEntry(true, null)).toBe(true);
    expect(shouldShowKmEntry(false, null)).toBe(false);
    expect(shouldShowKmEntry(false, "off")).toBe(false);
  });
});
