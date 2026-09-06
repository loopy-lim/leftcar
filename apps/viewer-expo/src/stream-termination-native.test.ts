import { beforeAll, describe, expect, it, vi } from "vitest";

vi.mock("react-native", () => ({
  DeviceEventEmitter: {
    addListener: () => ({ remove: () => undefined }),
  },
}));

let nativeTermination: typeof import("./stream-termination.native");

beforeAll(async () => {
  nativeTermination = await import("./stream-termination.native");
});

describe("native stream termination entrypoint", () => {
  it("exports the viewer-close classifier selected by Android bundling", () => {
    expect(typeof nativeTermination.classifyHostTermination).toBe("function");
    expect(nativeTermination.classifyHostTermination("viewer closed stream")).toBe(
      "viewerClosed",
    );
  });
});
