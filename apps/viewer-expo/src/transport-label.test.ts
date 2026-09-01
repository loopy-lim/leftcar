import { describe, expect, it } from "vitest";
import { transportBadgeLabel } from "./transport-label";

describe("transportBadgeLabel", () => {
  it("usb이면 USB를 반환한다", () => {
    expect(transportBadgeLabel("usb")).toBe("USB");
  });

  it("udp이면 Wi-Fi를 반환한다", () => {
    expect(transportBadgeLabel("udp")).toBe("Wi-Fi");
  });

  it("tcp이면 Wi-Fi (TCP)를 반환한다", () => {
    expect(transportBadgeLabel("tcp")).toBe("Wi-Fi (TCP)");
  });

  it("adbTcp와 기타 값은 ADB를 반환한다", () => {
    expect(transportBadgeLabel("adbTcp")).toBe("ADB");
    expect(transportBadgeLabel("anything-else")).toBe("ADB");
  });
});
