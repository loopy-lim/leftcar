import { describe, expect, it } from "vitest";
import { shouldSwitchTransport } from "./transport-switch";

describe("shouldSwitchTransport", () => {
  it("keeps UDP when no USB accessory is attached", () => {
    expect(shouldSwitchTransport("udp", "udp")).toBe(false);
  });

  it("switches UDP to USB when an accessory is attached", () => {
    expect(shouldSwitchTransport("udp", "usb")).toBe(true);
  });

  it("keeps a 4K split stream on its required UDP pair", () => {
    expect(
      shouldSwitchTransport("udp", "usb", "splitVertical"),
    ).toBe(false);
  });

  it("keeps USB while the accessory remains attached", () => {
    expect(shouldSwitchTransport("usb", "usb")).toBe(false);
  });

  it("switches USB back to UDP after detach", () => {
    expect(shouldSwitchTransport("usb", "udp")).toBe(true);
  });

  it("does not rewrite explicit diagnostic TCP", () => {
    expect(shouldSwitchTransport("tcp", "udp")).toBe(false);
  });
});
