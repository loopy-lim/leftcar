import { describe, expect, it, vi } from "vitest";
vi.mock("react-native-tcp-socket", () => ({ default: { createConnection: vi.fn() } }));
vi.mock("./session", () => ({ bindRequestContext: vi.fn(), captureRequestContext: vi.fn(), reconnectHost: vi.fn(), isRequestContextCurrent: vi.fn() }));
import { openExtendedDisplay, extensionDisplayFor } from "./extension-display";
import type { CatalogView } from "./control";

const display = { sourceId: "display:extension", index: 4, name: "Leftcar Display", width: 2560, height: 1600 };
describe("extended display flow", () => {
  it("reuses the stable source without creating a second monitor", async () => {
    const opened: string[] = [];
    await openExtendedDisplay({
      existing: display, isCurrent: () => true,
      create: async () => { throw new Error("should reuse"); },
      refresh: async () => [display], open: async source => { opened.push(source.sourceId!); },
    });
    expect(opened).toEqual([display.sourceId]);
  });
  it("does not open a late creation on a different selected Host", async () => {
    let current = true;
    const opened: string[] = [];
    await openExtendedDisplay({
      isCurrent: () => current,
      create: async () => { current = false; return { sourceId: display.sourceId }; },
      refresh: async () => [display], open: async source => { opened.push(source.sourceId!); },
    });
    expect(opened).toEqual([]);
  });
  it("does not mistake a physical display's name for managed identity on a new Host", () => {
    const catalog: CatalogView = { platform: "macos", captureBackends: [], displays: [display], virtualDisplayControl: true, virtualDisplaySourceId: "display:other" };
    expect(extensionDisplayFor(catalog)).toBeUndefined();
  });
  it("never opens a display that is being removed", () => {
    const catalog: CatalogView = { platform: "macos", captureBackends: [], displays: [display], virtualDisplaySourceId: display.sourceId, virtualDisplayPendingRemoval: display.sourceId };
    expect(extensionDisplayFor(catalog)).toBeUndefined();
  });
});
