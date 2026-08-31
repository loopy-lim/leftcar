import { describe, expect, it } from "vitest";

import {
  availableEncoderExperiments,
  availableEncoderExperimentsForStreams,
  normalizeEncoderExperiments,
  resolveEncoderExperiment,
  resolveEncoderExperimentForStream,
  type EncoderExperimentInfo,
} from "./encoder-experiment";
import { STREAM_PROFILES } from "./stream-profile";
import { resolveStreamResolution } from "./stream-resolution";

const advertised: EncoderExperimentInfo[] = [
  {
    id: "auto",
    label: "자동",
    hint: "호스트가 사용 가능한 인코더 경로를 선택합니다.",
    requiresReconnect: true,
  },
  {
    id: "rateControl",
    label: "레이트 컨트롤",
    hint: "고정 레이트 컨트롤 경로를 사용합니다.",
    requiresReconnect: true,
  },
  {
    id: "adaptiveQp",
    label: "적응형 QP",
    hint: "화면 변화와 인코더 압력에 따라 Base QP를 조절합니다.",
    requiresReconnect: true,
  },
  {
    id: "encoderPool",
    label: "인코더 풀",
    hint: "인코더가 제공하는 픽셀 버퍼 풀을 사용합니다.",
    requiresReconnect: true,
  },
  {
    id: "splitVertical",
    label: "4K 수직 분할",
    hint: "두 하드웨어 디코더와 Surface를 사용합니다.",
    requiresReconnect: true,
  },
];

describe("encoder experiment selector", () => {
  it("shows only the advertised automatic option below 4K", () => {
    expect(availableEncoderExperiments(advertised, 2560, 1440)).toEqual([
      advertised[0],
    ]);
  });

  it("hides the split diagnostic wire ID at every product resolution", () => {
    expect(availableEncoderExperiments(advertised, 3840, 2160)).toEqual(
      advertised.filter((experiment) => experiment.id !== "splitVertical"),
    );
    expect(availableEncoderExperiments(advertised, 7680, 4320)).toEqual(
      advertised.filter((experiment) => experiment.id !== "splitVertical"),
    );
  });

  it("does not synthesize profiles that the Host did not advertise", () => {
    expect(availableEncoderExperiments(undefined, 3840, 2160)).toEqual([]);
    expect(availableEncoderExperiments([advertised[0]], 3840, 2160)).toEqual([
      advertised[0],
    ]);
  });

  it("fails closed for malformed top-level capability payloads", () => {
    for (const malformed of [{}, "adaptiveQp", null, 42, false]) {
      expect(normalizeEncoderExperiments(malformed)).toEqual([]);
      expect(availableEncoderExperiments(malformed, 3840, 2160)).toEqual([]);
      expect(
        resolveEncoderExperimentForStream(
          "adaptiveQp",
          malformed,
          3840,
          2160,
        ),
      ).toBe("auto");
    }
  });

  it("resolves a stale selection and old-host capability to automatic", () => {
    expect(resolveEncoderExperiment("adaptiveQp", [advertised[0]])).toBe("auto");
    expect(resolveEncoderExperiment("adaptiveQp", undefined)).toBe("auto");
  });

  it("keeps a selected profile when the Host still advertises it", () => {
    expect(resolveEncoderExperiment("adaptiveQp", advertised)).toBe("adaptiveQp");
  });

  it("forces an advertised adaptive selection to automatic for an actual 1440p stream", () => {
    expect(
      resolveEncoderExperimentForStream(
        "adaptiveQp",
        advertised,
        2560,
        1440,
      ),
    ).toBe("auto");
  });

  it("preserves an advertised adaptive selection for an actual 4K stream", () => {
    expect(
      resolveEncoderExperimentForStream(
        "adaptiveQp",
        advertised,
        3840,
        2160,
      ),
    ).toBe("adaptiveQp");
  });

  it("rejects the split diagnostic wire ID even at exact 4K", () => {
    expect(
      resolveEncoderExperimentForStream(
        "splitVertical",
        advertised,
        3840,
        2160,
      ),
    ).toBe("auto");
    expect(
      resolveEncoderExperimentForStream(
        "splitVertical",
        advertised,
        4096,
        2160,
      ),
    ).toBe("auto");
  });

  it("uses fitted dimensions instead of a 4K profile maximum", () => {
    const profile = STREAM_PROFILES.find((candidate) => candidate.id === "video");
    expect(profile).toBeDefined();
    const fitted = resolveStreamResolution(
      { width: 2560, height: 1440 },
      profile ?? STREAM_PROFILES[0],
    );

    expect(fitted).toEqual({ width: 2560, height: 1440, fps: 60 });
    expect(
      resolveEncoderExperimentForStream(
        "adaptiveQp",
        advertised,
        fitted.width,
        fitted.height,
      ),
    ).toBe("auto");
  });

  it("shows the global selector when at least one actual target is 4K", () => {
    expect(
      availableEncoderExperimentsForStreams(advertised, [
        { width: 2560, height: 1440 },
        { width: 3840, height: 2160 },
      ]),
    ).toEqual(advertised.filter((experiment) => experiment.id !== "splitVertical"));
    expect(
      availableEncoderExperimentsForStreams(advertised, [
        { width: 1920, height: 1080 },
        { width: 2560, height: 1440 },
      ]),
    ).toEqual([advertised[0]]);
  });

  it("uses actual active dimensions when restoring or replacing transport", () => {
    expect(
      resolveEncoderExperimentForStream(
        "encoderPool",
        advertised,
        3840,
        2160,
      ),
    ).toBe("encoderPool");
    expect(
      resolveEncoderExperimentForStream(
        "encoderPool",
        advertised,
        2560,
        1440,
      ),
    ).toBe("auto");
  });

  it("filters unknown reserved malformed and duplicate runtime entries in Host order", () => {
    const raw: unknown[] = [
      advertised[2],
      { ...advertised[2], label: "duplicate adaptive" },
      {
        id: "splitHorizontal",
        label: "reserved",
        hint: "reserved",
        requiresReconnect: true,
      },
      {
        id: "futureExperiment",
        label: "future",
        hint: "future",
        requiresReconnect: true,
      },
      null,
      {
        id: "rateControl",
        label: 42,
        hint: "malformed label",
        requiresReconnect: true,
      },
      {
        id: "encoderPool",
        label: "non-deferred pool",
        hint: "must not be selectable",
        requiresReconnect: false,
      },
      {
        id: "rateControl",
        label: "malformed reconnect",
        hint: "must not be selectable",
        requiresReconnect: "true",
      },
      advertised[0],
      advertised[1],
      advertised[3],
      advertised[4],
    ];

    expect(normalizeEncoderExperiments(raw)).toEqual([
      advertised[2],
      advertised[0],
      advertised[1],
      advertised[3],
      advertised[4],
    ]);
  });

  it("rejects a catalog whose Phase A entries do not explicitly require reconnect", () => {
    expect(
      normalizeEncoderExperiments(
        advertised.map((entry) => ({
          ...entry,
          requiresReconnect: false,
        })),
      ),
    ).toEqual([]);
  });
});
