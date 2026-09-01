import { describe, expect, it } from "vitest";
import { transportBadgeLabel } from "./transport-label";

describe("transportBadgeLabel", () => {
  it("returns USB for the usb transport", () => {
    expect(transportBadgeLabel("usb")).toBe("USB");
  });

  it("returns Wi-Fi for the udp transport", () => {
    expect(transportBadgeLabel("udp")).toBe("Wi-Fi");
  });

  it("returns Wi-Fi (TCP) for the tcp transport", () => {
    expect(transportBadgeLabel("tcp")).toBe("Wi-Fi (TCP)");
  });

  it("falls back to ADB for adbTcp and unknown values", () => {
    expect(transportBadgeLabel("adbTcp")).toBe("ADB");
    expect(transportBadgeLabel("anything-else")).toBe("ADB");
  });
});
