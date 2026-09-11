import { describe, expect, it } from "vitest";
import {
  DEFAULT_DECODER_CAPACITY,
  SPLIT_DECODER_SLOTS,
  admissionStreamsFrom,
  decoderSlots,
  downgradedStreamTarget,
  planAdmission,
  requestedDecoderShape,
} from "./decoder-budget";

const fourK = { width: 3840, height: 2160, fps: 60 };

function plan(
  streams: readonly { split: boolean }[],
  request: { split: boolean; replacing?: { split: boolean } },
) {
  return planAdmission(streams, { target: fourK, ...request });
}

describe("decoderSlots", () => {
  it("charges two instances for split and one for a regular window", () => {
    expect(decoderSlots({ split: true })).toBe(2);
    expect(decoderSlots({ split: false })).toBe(1);
    expect(SPLIT_DECODER_SLOTS).toBe(2);
  });
});

describe("admissionStreamsFrom", () => {
  it("derives split flags from live stream encoder modes", () => {
    expect(
      admissionStreamsFrom([
        { encoderExperiment: "splitVertical" },
        { encoderExperiment: "auto" },
        { encoderExperiment: "adaptiveQp" },
      ]),
    ).toEqual([{ split: true }, { split: false }, { split: false }]);
  });
});

describe("planAdmission", () => {
  it("admits into an empty budget", () => {
    const result = plan([], { split: false });
    expect(result).toMatchObject({
      allowed: true,
      action: "allow",
      currentSlots: 0,
      projectedSlots: 1,
      capacity: DEFAULT_DECODER_CAPACITY,
    });
  });

  it("admits a single window next to one live single window", () => {
    expect(plan([{ split: false }], { split: false })).toMatchObject({
      allowed: true,
      action: "allow",
      currentSlots: 1,
      projectedSlots: 2,
    });
  });

  it("admits a split pair into an empty budget", () => {
    expect(plan([], { split: true })).toMatchObject({
      allowed: true,
      action: "allow",
      currentSlots: 0,
      projectedSlots: 2,
    });
  });

  it("admits one extra window next to a live split pair", () => {
    expect(plan([{ split: true }], { split: false })).toMatchObject({
      allowed: true,
      action: "allow",
      currentSlots: 2,
      projectedSlots: 3,
    });
  });

  it("downgrades a further split when a split pair and a window are live", () => {
    // split(2) + window(1) = 3; +2 would be 5 > 4, but a single fits (3+1=4).
    expect(plan([{ split: true }, { split: false }], { split: true })).toMatchObject({
      allowed: true,
      action: "downgradeResolution",
      currentSlots: 3,
      projectedSlots: 4,
    });
  });

  it("blocks a split request only when even a single instance does not fit", () => {
    // split(2) + window(1) + window(1) = 4 leaves no room at all.
    const full = [{ split: true }, { split: false }, { split: false }];
    expect(plan(full, { split: true })).toMatchObject({
      allowed: false,
      action: "block",
      currentSlots: 4,
      projectedSlots: 5,
    });
  });

  it("blocks a regular window at full capacity instead of downgrading", () => {
    // 해상도 강등은 디코더 인스턴스 수를 줄이지 못하므로(슬롯은 split
    // 여부에만 의존) 초과 일반 창은 같은 창 모양 그대로 거절된다.
    const full = [{ split: true }, { split: false }, { split: false }];
    expect(plan(full, { split: false })).toMatchObject({
      allowed: false,
      action: "block",
      currentSlots: 4,
      projectedSlots: 5,
    });
  });

  it("blocks a split request at capacity with the same outcome as a window", () => {
    const full = [{ split: true }, { split: false }, { split: false }];
    expect(plan(full, { split: true })).toMatchObject({
      allowed: false,
      action: "block",
      currentSlots: 4,
      projectedSlots: 5,
    });
  });

  it("still allows replace-in-place at full capacity", () => {
    // 대체 시작은 자기 슬롯을 먼저 반납하므로 만원 상태에서도 들어간다.
    const full = [{ split: true }, { split: false }, { split: false }];
    expect(
      plan(full, { split: false, replacing: { split: false } }),
    ).toMatchObject({
      allowed: true,
      action: "allow",
      currentSlots: 3,
      projectedSlots: 4,
    });
  });

  it("releases the replaced stream's slots before projecting a promotion", () => {
    // split(2) + this single(1) + window(1) = 4. Promotion releases this
    // stream's single slot, so split needs 3 + 2 = 5 > 4 → refuse (downgrade).
    const streams = [
      { split: true },
      { split: false },
      { split: false },
    ];
    expect(
      plan(streams, { split: true, replacing: { split: false } }),
    ).toMatchObject({
      allowed: true,
      action: "downgradeResolution",
      currentSlots: 3,
      projectedSlots: 4,
    });
  });

  it("allows a promotion that stays inside capacity after release", () => {
    // split(2) + this single(1) = 3. Release → 2, +2 = 4 ≤ 4 → allow.
    expect(
      plan([{ split: true }, { split: false }], {
        split: true,
        replacing: { split: false },
      }),
    ).toMatchObject({
      allowed: true,
      action: "allow",
      currentSlots: 2,
      projectedSlots: 4,
    });
  });

  it("honors an injected capacity for a future native probe", () => {
    const refused = planAdmission([{ split: true }], {
      target: fourK,
      split: true,
    }, { capacity: 2 });
    expect(refused).toMatchObject({
      allowed: false,
      action: "block",
      capacity: 2,
    });
    const downgraded = planAdmission([{ split: true }], {
      target: fourK,
      split: true,
    }, { capacity: 3 });
    expect(downgraded).toMatchObject({
      allowed: true,
      action: "downgradeResolution",
      capacity: 3,
    });
  });
});

describe("requestedDecoderShape", () => {
  const splitAdvertised = [
    { id: "auto", label: "auto", hint: "h", requiresReconnect: true },
    {
      id: "splitVertical",
      label: "split",
      hint: "h",
      requiresReconnect: true,
    },
  ] as const;

  it("predicts split for a pinned splitVertical at 4K", () => {
    expect(
      requestedDecoderShape({
        encoderExperiment: "splitVertical",
        width: 3840,
        height: 2160,
        advertisedEncoderExperiments: splitAdvertised,
      }),
    ).toBe(true);
  });

  it("predicts split for auto at 4K when split is advertised", () => {
    expect(
      requestedDecoderShape({
        encoderExperiment: "auto",
        width: 3840,
        height: 2160,
        advertisedEncoderExperiments: splitAdvertised,
      }),
    ).toBe(true);
  });

  it("keeps sub-4K requests single even when split is advertised", () => {
    expect(
      requestedDecoderShape({
        encoderExperiment: "auto",
        width: 2560,
        height: 1440,
        advertisedEncoderExperiments: splitAdvertised,
      }),
    ).toBe(false);
  });

  it("keeps requests single when split is not advertised", () => {
    expect(
      requestedDecoderShape({
        encoderExperiment: "auto",
        width: 3840,
        height: 2160,
        advertisedEncoderExperiments: [
          { id: "auto", label: "auto", hint: "h", requiresReconnect: true },
        ],
      }),
    ).toBe(false);
  });
});

describe("downgradedStreamTarget", () => {
  it("steps 4K down to the responsive 2K baseline", () => {
    expect(downgradedStreamTarget(fourK)).toEqual({
      width: 2560,
      height: 1440,
      fps: 60,
    });
  });

  it("keeps stepping through 1080 and 720", () => {
    expect(downgradedStreamTarget({ width: 2560, height: 1440, fps: 60 })).toEqual({
      width: 1920,
      height: 1080,
      fps: 60,
    });
    expect(downgradedStreamTarget({ width: 1920, height: 1080, fps: 60 })).toEqual({
      width: 1280,
      height: 720,
      fps: 60,
    });
  });

  it("returns null at the bottom of the ladder", () => {
    expect(downgradedStreamTarget({ width: 1280, height: 720, fps: 60 })).toBeNull();
    expect(downgradedStreamTarget({ width: 640, height: 480, fps: 60 })).toBeNull();
  });

  it("mirrors portrait sources and preserves fps", () => {
    expect(
      downgradedStreamTarget({ width: 2160, height: 3840, fps: 30 }),
    ).toEqual({ width: 1440, height: 2560, fps: 30 });
  });

  it("downgrades an off-ladder short side to the next lower anchor", () => {
    expect(downgradedStreamTarget({ width: 3200, height: 1600, fps: 60 })).toEqual({
      width: 2160,
      height: 1080,
      fps: 60,
    });
  });
});
