import { describe, expect, it } from "vitest";
import { is4KResolution, STREAM_PROFILES } from "./stream-profile";

describe("stream profiles", () => {
  it("provides a video profile that preserves 4K at the minimum 60fps", () => {
    const video = STREAM_PROFILES.find((profile) => profile.id === "video");

    expect(video).toMatchObject({
      id: "video",
      contentMode: "video",
      detail: "최대 4K 60fps",
      maxWidth: 3840,
      maxHeight: 2160,
      fps: 60,
      allowUpscale: false,
    });
  });

  it("keeps interactive profiles on the low-latency content mode", () => {
    expect(STREAM_PROFILES.find((profile) => profile.id === "latency")).toMatchObject({
      contentMode: "interactive",
    });
  });

  it("marks 1440p60 as the measurable fallback baseline", () => {
    expect(STREAM_PROFILES.find((profile) => profile.id === "balanced")).toMatchObject({
      id: "balanced",
      detail: "1440p 60fps",
      maxWidth: 2560,
      maxHeight: 1440,
      fps: 60,
      contentMode: "interactive",
      role: "fallback",
    });
  });

  it("uses the existing non-rotating width and height for 4K visibility", () => {
    expect(is4KResolution(3840, 2160)).toBe(true);
    expect(is4KResolution(7680, 4320)).toBe(true);
    expect(is4KResolution(2560, 1440)).toBe(false);
    expect(is4KResolution(2160, 3840)).toBe(false);
  });

  it("does not request clarity upscaling without verified source pixels", () => {
    expect(STREAM_PROFILES.find((profile) => profile.id === "clarity")).toMatchObject({
      allowUpscale: false,
    });
  });
});
