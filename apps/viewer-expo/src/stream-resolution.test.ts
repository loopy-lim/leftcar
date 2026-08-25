import { describe, expect, it } from "vitest";

import { resolveStreamResolution } from "./stream-resolution";

describe("resolveStreamResolution", () => {
  it("requests a real 4K60 stream from a 16:9 HiDPI logical size", () => {
    expect(
      resolveStreamResolution(
        { width: 1920, height: 1080 },
        { maxWidth: 3840, maxHeight: 2160, fps: 60, allowUpscale: true },
      ),
    ).toEqual({ width: 3840, height: 2160, fps: 60 });
  });

  it("does not upscale latency-oriented profiles", () => {
    expect(
      resolveStreamResolution(
        { width: 1280, height: 720 },
        { maxWidth: 1920, maxHeight: 1080, fps: 60 },
      ),
    ).toEqual({ width: 1280, height: 720, fps: 60 });
  });

  it("caps sources larger than 4K while preserving aspect ratio", () => {
    expect(
      resolveStreamResolution(
        { width: 5120, height: 2880 },
        { maxWidth: 3840, maxHeight: 2160, fps: 60, allowUpscale: true },
      ),
    ).toEqual({ width: 3840, height: 2160, fps: 60 });
  });
});
