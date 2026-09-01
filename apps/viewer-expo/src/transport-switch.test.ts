import { describe, expect, it } from "vitest";
import { shouldSwitchTransport } from "./transport-switch";

describe("shouldSwitchTransport", () => {
  it("keeps UDP when no USB accessory is attached", () => {
    expect(shouldSwitchTransport("udp", { attached: false })).toBe(false);
  });

  it("switches UDP to USB when an accessory is attached", () => {
    expect(shouldSwitchTransport("udp", { attached: true })).toBe(true);
  });

  it("keeps a 4K split stream on its required UDP pair", () => {
    expect(
      shouldSwitchTransport("udp", { attached: true }, "splitVertical"),
    ).toBe(false);
  });

  it("keeps USB while the accessory remains attached", () => {
    expect(shouldSwitchTransport("usb", { attached: true })).toBe(false);
  });

  it("switches USB back to UDP after detach", () => {
    expect(shouldSwitchTransport("usb", { attached: false })).toBe(true);
  });

  it("does not rewrite explicit diagnostic TCP", () => {
    expect(shouldSwitchTransport("tcp", { attached: false })).toBe(false);
  });
});
