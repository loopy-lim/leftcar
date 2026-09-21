import { describe, expect, it } from "vitest";
import {
  capTargetToPanelShortSide,
  CLARITY_STREAM_SHORT_SIDE,
  isStreamingPriority,
  panelShortSideCap,
  RESPONSIVE_STREAM_SHORT_SIDE,
  resolveInitialStreamTarget,
  resolveStreamingTarget,
  STREAMING_PRIORITIES,
  STREAMING_TARGET_MAX_PIXELS,
  STREAMING_TARGET_MAX_WIDTH,
  STREAM_TARGET_FPS,
  streamingPriorityFromProfileId,
} from "./streaming-policy";

describe("streaming priority", () => {
  it("exposes exactly the responsive and clarity priorities", () => {
    expect(STREAMING_PRIORITIES).toEqual(["responsive", "clarity"]);
    expect(isStreamingPriority("responsive")).toBe(true);
    expect(isStreamingPriority("clarity")).toBe(true);
    expect(isStreamingPriority("latency")).toBe(false);
    expect(isStreamingPriority("auto")).toBe(false);
    expect(isStreamingPriority(null)).toBe(false);
    expect(isStreamingPriority(42)).toBe(false);
  });
});

describe("resolveStreamingTarget", () => {
  it("uses the 2560x1440 baseline for responsive 16:9 sources", () => {
    expect(resolveStreamingTarget({ width: 2560, height: 1440 }, "responsive")).toEqual({
      width: 2560,
      height: 1440,
      fps: STREAM_TARGET_FPS,
    });
    expect(RESPONSIVE_STREAM_SHORT_SIDE).toBe(1440);
  });

  it("offers the clarity 4K target to sources with real 4K pixels", () => {
    expect(resolveStreamingTarget({ width: 3840, height: 2160 }, "clarity")).toEqual({
      width: 3840,
      height: 2160,
      fps: STREAM_TARGET_FPS,
    });
    expect(CLARITY_STREAM_SHORT_SIDE).toBe(2160);
  });

  it("offers 5120x1440 to supported 32:9 sources at both priorities", () => {
    const source = { width: 5120, height: 1440 };
    expect(resolveStreamingTarget(source, "responsive")).toEqual({
      width: 5120,
      height: 1440,
      fps: STREAM_TARGET_FPS,
    });
    expect(resolveStreamingTarget(source, "clarity")).toEqual({
      width: 5120,
      height: 1440,
      fps: STREAM_TARGET_FPS,
    });
    // 5120x1440 stays inside the 4K pixel budget even though its width is larger.
    expect(5120 * 1440).toBeLessThanOrEqual(STREAMING_TARGET_MAX_PIXELS);
  });

  it("mirrors targets for portrait equivalents", () => {
    expect(resolveStreamingTarget({ width: 2880, height: 5120 }, "responsive")).toEqual({
      width: 1440,
      height: 2560,
      fps: STREAM_TARGET_FPS,
    });
    expect(resolveStreamingTarget({ width: 2880, height: 5120 }, "clarity")).toEqual({
      width: 2160,
      height: 3840,
      fps: STREAM_TARGET_FPS,
    });
    expect(resolveStreamingTarget({ width: 1440, height: 5120 }, "clarity")).toEqual({
      width: 1440,
      height: 5120,
      fps: STREAM_TARGET_FPS,
    });
  });

  it("never upscales a smaller source, including for clarity", () => {
    expect(resolveStreamingTarget({ width: 1920, height: 1080 }, "clarity")).toEqual({
      width: 1920,
      height: 1080,
      fps: STREAM_TARGET_FPS,
    });
    expect(resolveStreamingTarget({ width: 2560, height: 1440 }, "clarity")).toEqual({
      width: 2560,
      height: 1440,
      fps: STREAM_TARGET_FPS,
    });
    expect(resolveStreamingTarget({ width: 2560, height: 720 }, "responsive")).toEqual({
      width: 2560,
      height: 720,
      fps: STREAM_TARGET_FPS,
    });
  });

  it("keeps even codec dimensions for odd sources", () => {
    const target = resolveStreamingTarget({ width: 3211, height: 1807 }, "responsive");
    expect(target.width % 2).toBe(0);
    expect(target.height % 2).toBe(0);
    expect(target.width / target.height).toBeCloseTo(3211 / 1807, 2);
  });

  it("bounds pixels and long side across the aspect sweep", () => {
    // ratio는 요청된 실제 종횡비(가로/세로)다. sqrt로 소스를 만들어야
    // ratio<1이 세로(portrait), ratio>1이 가로(landscape) 소스가 된다 —
    // 이전 생성식은 ratio<1을 정사각형으로, ratio>1을 ratio² 종횡비로 만들어
    // 요청된 비율을 전혀 검사하지 않았다.
    for (let ratio = 0.25; ratio <= 5.0001; ratio += 0.05) {
      for (const priority of STREAMING_PRIORITIES) {
        const source = {
          width: Math.round(3000 * Math.sqrt(ratio)),
          height: Math.round(3000 / Math.sqrt(ratio)),
        };
        // 소스 자체가 요청 비율을 담고 있는지 먼저 단언한다.
        expect(source.width / source.height).toBeCloseTo(ratio, 1);
        const target = resolveStreamingTarget(source, priority);
        expect(target.width % 2).toBe(0);
        expect(target.height % 2).toBe(0);
        expect(target.fps).toBe(STREAM_TARGET_FPS);
        expect(target.width * target.height).toBeLessThanOrEqual(
          STREAMING_TARGET_MAX_PIXELS,
        );
        expect(Math.max(target.width, target.height)).toBeLessThanOrEqual(
          STREAMING_TARGET_MAX_WIDTH,
        );
        expect(target.width).toBeLessThanOrEqual(source.width);
        expect(target.height).toBeLessThanOrEqual(source.height);
        expect(
          Math.abs(target.width / target.height - source.width / source.height) /
            (source.width / source.height),
        ).toBeLessThan(0.01);
      }
    }
  });

  it("keeps portrait sources portrait across the sweep", () => {
    const source = { width: 1500, height: 6000 };
    expect(resolveStreamingTarget(source, "responsive")).toEqual({
      width: 1280,
      height: 5120,
      fps: STREAM_TARGET_FPS,
    });
    expect(resolveStreamingTarget(source, "clarity")).toEqual({
      width: 1280,
      height: 5120,
      fps: STREAM_TARGET_FPS,
    });
  });

  it("scales an ultrawide clarity request down to the pixel budget", () => {
    const target = resolveStreamingTarget({ width: 5040, height: 2160 }, "clarity");
    expect(target.width * target.height).toBeLessThanOrEqual(
      STREAMING_TARGET_MAX_PIXELS,
    );
    expect(target.width / target.height).toBeCloseTo(5040 / 2160, 2);
  });
});

describe("resolveInitialStreamTarget", () => {
  it("starts responsive streams at the 1440 short side while clarity starts at the source maximum", () => {
    const display = { width: 3840, height: 2160 };
    expect(resolveInitialStreamTarget(display, "responsive")).toEqual({
      width: 2560,
      height: 1440,
      fps: STREAM_TARGET_FPS,
    });
    expect(resolveInitialStreamTarget(display, "clarity")).toEqual({
      width: 3840,
      height: 2160,
      fps: STREAM_TARGET_FPS,
    });
  });

  it("caps the initial target at the manual profile maximum for both priorities", () => {
    const display = { width: 3840, height: 2160 };
    // 수동 '빠른 반응'(1080p) 프로필이 최대를 제한하면 두 우선순위 모두
    // 1080p에서 시작한다 — 우선순위는 최대를 넓히지 않는다.
    const maximum = { width: 1920, height: 1080 };
    expect(resolveInitialStreamTarget(display, "responsive", maximum)).toEqual({
      width: 1920,
      height: 1080,
      fps: STREAM_TARGET_FPS,
    });
    expect(resolveInitialStreamTarget(display, "clarity", maximum)).toEqual({
      width: 1920,
      height: 1080,
      fps: STREAM_TARGET_FPS,
    });
  });

  it("keeps the manual 4K maximum distinct from the responsive 1440 start", () => {
    // 수동 4K 프로필(또는 자동 추천)은 최대로 남고, responsive 시작점은
    // 2560x1440이다 — 이후 안정 구간에서 적응 정책이 최대로 올린다.
    const display = { width: 3840, height: 2160 };
    const maximum = { width: 3840, height: 2160 };
    const initial = resolveInitialStreamTarget(display, "responsive", maximum);
    expect(initial).toEqual({ width: 2560, height: 1440, fps: STREAM_TARGET_FPS });
  });

  it("handles wide and portrait sources with an explicit maximum", () => {
    const wide = { width: 5120, height: 1440 };
    expect(resolveInitialStreamTarget(wide, "responsive", wide)).toEqual({
      width: 5120,
      height: 1440,
      fps: STREAM_TARGET_FPS,
    });
    const portrait = { width: 2160, height: 3840 };
    expect(resolveInitialStreamTarget(portrait, "responsive", portrait)).toEqual({
      width: 1440,
      height: 2560,
      fps: STREAM_TARGET_FPS,
    });
    expect(resolveInitialStreamTarget(portrait, "clarity", portrait)).toEqual({
      width: 2160,
      height: 3840,
      fps: STREAM_TARGET_FPS,
    });
  });

  it("never upscales a smaller source for the initial target", () => {
    const display = { width: 1920, height: 1080 };
    expect(resolveInitialStreamTarget(display, "clarity")).toEqual({
      width: 1920,
      height: 1080,
      fps: STREAM_TARGET_FPS,
    });
    expect(resolveInitialStreamTarget(display, "responsive")).toEqual({
      width: 1920,
      height: 1080,
      fps: STREAM_TARGET_FPS,
    });
  });

  it("keeps the initial target inside the maximum across the sweep", () => {
    for (let ratio = 0.25; ratio <= 5.0001; ratio += 0.05) {
      const source = {
        width: Math.round(3000 * Math.sqrt(ratio)),
        height: Math.round(3000 / Math.sqrt(ratio)),
      };
      const maximum = { width: 3840, height: 2160 };
      for (const priority of STREAMING_PRIORITIES) {
        const initial = resolveInitialStreamTarget(source, priority, maximum);
        const maxLong = Math.max(maximum.width, maximum.height);
        expect(Math.max(initial.width, initial.height)).toBeLessThanOrEqual(maxLong);
        expect(initial.width).toBeLessThanOrEqual(source.width);
        expect(initial.height).toBeLessThanOrEqual(source.height);
        expect(initial.width % 2).toBe(0);
        expect(initial.height % 2).toBe(0);
      }
    }
  });
});

describe("streamingPriorityFromProfileId", () => {
  it("maps legacy clarity-intent profiles to clarity", () => {
    expect(streamingPriorityFromProfileId("clarity")).toBe("clarity");
    expect(streamingPriorityFromProfileId("video")).toBe("clarity");
    expect(streamingPriorityFromProfileId("smooth")).toBe("clarity");
  });

  it("maps legacy responsiveness-intent profiles to responsive", () => {
    expect(streamingPriorityFromProfileId("latency")).toBe("responsive");
    expect(streamingPriorityFromProfileId("balanced")).toBe("responsive");
    expect(streamingPriorityFromProfileId("auto")).toBe("responsive");
  });

  it("keeps the long side cap constant for downstream wiring", () => {
    expect(STREAMING_TARGET_MAX_WIDTH).toBe(5120);
  });
});

describe("resolveInitialStreamTarget external route", () => {
  const display = { width: 3840, height: 2160 };

  it("caps the start at the 1080p short side on tailnet/public routes", () => {
    const responsive = resolveInitialStreamTarget(display, "responsive", undefined, {
      externalRoute: true,
    });
    expect(responsive).toEqual({ width: 1920, height: 1080, fps: STREAM_TARGET_FPS });
    // clarity 역시 같은 상한에서 시작한다 — 최대(4K)는 그대로 적응의 상승 목표다.
    const clarity = resolveInitialStreamTarget(display, "clarity", undefined, {
      externalRoute: true,
    });
    expect(clarity.height).toBe(1080);
  });

  it("never widens a manual maximum but keeps the external clamp", () => {
    const maximum = { width: 1920, height: 1080 };
    expect(
      resolveInitialStreamTarget(display, "responsive", maximum, { externalRoute: true }).height,
    ).toBe(1080);
    const small = { width: 1280, height: 720 };
    expect(
      resolveInitialStreamTarget(display, "responsive", small, { externalRoute: true }).height,
    ).toBe(720);
  });

  it("keeps the LAN start unchanged without the flag", () => {
    expect(resolveInitialStreamTarget(display, "responsive").height).toBe(
      RESPONSIVE_STREAM_SHORT_SIDE,
    );
    expect(
      resolveInitialStreamTarget(display, "responsive", undefined, { externalRoute: false })
        .height,
    ).toBe(RESPONSIVE_STREAM_SHORT_SIDE);
  });
});

describe("client panel fitting", () => {
  it("caps the short side at the client panel so HiDPI sources stream 1:1", () => {
    const vdisp = { width: 2560, height: 1600 };
    expect(
      resolveInitialStreamTarget(vdisp, "responsive", vdisp, { panelShortSide: 1600 }),
    ).toEqual({ width: 2560, height: 1600, fps: STREAM_TARGET_FPS });
  });

  it("fits larger sources to the panel short side without upscaling smaller ones", () => {
    const display4K = { width: 3840, height: 2160 };
    expect(
      resolveInitialStreamTarget(display4K, "responsive", display4K, { panelShortSide: 1600 }),
    ).toEqual({ width: 2844, height: 1600, fps: STREAM_TARGET_FPS });
    const small = { width: 1280, height: 800 };
    expect(
      resolveInitialStreamTarget(small, "responsive", small, { panelShortSide: 1600 }),
    ).toEqual({ width: 1280, height: 800, fps: STREAM_TARGET_FPS });
  });

  it("still applies the external route clamp below the panel cap", () => {
    const display4K = { width: 3840, height: 2160 };
    expect(
      resolveInitialStreamTarget(display4K, "responsive", display4K, {
        panelShortSide: 1600,
        externalRoute: true,
      }),
    ).toEqual({ width: 1920, height: 1080, fps: STREAM_TARGET_FPS });
  });
});

describe("panelShortSideCap", () => {
  it("accepts sane panels and rejects missing or implausible metrics", () => {
    expect(panelShortSideCap(undefined)).toBeUndefined();
    expect(panelShortSideCap({ width: 2560, height: 1600 })).toBe(1600);
    expect(panelShortSideCap({ width: 1920, height: 1080 })).toBe(1080);
    expect(panelShortSideCap({ width: 864, height: 891 })).toBeUndefined();
    expect(panelShortSideCap({ width: 4096, height: 2304 })).toBeUndefined();
    expect(panelShortSideCap({ width: 0, height: 0 })).toBeUndefined();
  });
});

describe("capTargetToPanelShortSide", () => {
  it("shrinks the adaptive maximum to the panel and leaves smaller targets alone", () => {
    expect(capTargetToPanelShortSide({ width: 3840, height: 2160, fps: 60 }, 1600)).toEqual({
      width: 2844,
      height: 1600,
      fps: 60,
    });
    expect(capTargetToPanelShortSide({ width: 2560, height: 1600, fps: 60 }, 1600)).toEqual({
      width: 2560,
      height: 1600,
      fps: 60,
    });
    expect(
      capTargetToPanelShortSide({ width: 3840, height: 2160, fps: 60 }, undefined),
    ).toEqual({ width: 3840, height: 2160, fps: 60 });
  });
});
