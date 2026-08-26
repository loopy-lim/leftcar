import { describe, expect, it } from "vitest";
import { resolveTransport } from "./usb";

describe("resolveTransport", () => {
  it("prefers USB when an accessory is attached", () => {
    expect(resolveTransport({ attached: true })).toBe("usb");
  });

  it("selects UDP when USB is unavailable", () => {
    expect(resolveTransport({ attached: false })).toBe("udp");
  });

  it("preserves an explicitly requested transport", () => {
    expect(resolveTransport({ attached: false }, "udp")).toBe("udp");
    expect(resolveTransport({ attached: false }, "tcp")).toBe("tcp");
    expect(resolveTransport({ attached: true }, "adbTcp")).toBe("adbTcp");
  });

  it("keeps explicit TCP as a diagnostic transport", () => {
    expect(resolveTransport({ attached: false }, "wifitcp")).toBe("tcp");
  });
});
