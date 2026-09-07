import { describe, expect, it } from "vitest";

import {
  evenCodecDimension,
  fitStreamResolution,
  resolveStreamResolution,
} from "./stream-resolution";

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

describe("evenCodecDimension", () => {
  it("floors to even codec-safe values with a floor of two", () => {
    expect(evenCodecDimension(1080)).toBe(1080);
    expect(evenCodecDimension(1081)).toBe(1080);
    expect(evenCodecDimension(3)).toBe(2);
    expect(evenCodecDimension(0)).toBe(2);
    expect(evenCodecDimension(-10)).toBe(2);
  });
});

describe("fitStreamResolution", () => {
  it("applies orientation-aware caps for portrait sources", () => {
    expect(
      fitStreamResolution(
        { width: 2000, height: 4000 },
        { maxWidth: 5120, maxHeight: 1440 },
      ),
    ).toEqual({ width: 1440, height: 2880 });
  });

  it("scales down to the pixel budget after the box fit", () => {
    const fitted = fitStreamResolution(
      { width: 5040, height: 2160 },
      { maxWidth: 5120, maxHeight: 2160, maxPixels: 3840 * 2160 },
    );
    expect(fitted.width * fitted.height).toBeLessThanOrEqual(3840 * 2160);
    expect(fitted.width / fitted.height).toBeCloseTo(5040 / 2160, 2);
    expect(fitted.width % 2).toBe(0);
    expect(fitted.height % 2).toBe(0);
  });

  it("does not upscale unless explicitly allowed", () => {
    expect(
      fitStreamResolution(
        { width: 1280, height: 720 },
        { maxWidth: 5120, maxHeight: 1440 },
      ),
    ).toEqual({ width: 1280, height: 720 });
    expect(
      fitStreamResolution(
        { width: 1280, height: 720 },
        { maxWidth: 5120, maxHeight: 1440, allowUpscale: true },
      ),
    ).toEqual({ width: 2560, height: 1440 });
  });

  it("floors odd source dimensions to even codec values", () => {
    expect(
      fitStreamResolution(
        { width: 1441, height: 1001 },
        { maxWidth: 5120, maxHeight: 1440 },
      ),
    ).toEqual({ width: 1440, height: 1000 });
  });
});
