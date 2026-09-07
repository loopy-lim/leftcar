import { describe, expect, it } from "vitest";
import type { SessionView } from "./control";
import {
  hostQueuePressureUs,
  receiverRenderedFps,
  RENDERED_FPS_STALE_MS,
} from "./receiver-telemetry";

function session(overrides: Partial<SessionView> = {}): SessionView {
  return {
    session: 1,
    sourceIndex: 0,
    sourceName: "Main",
    viewerAddr: "127.0.0.1:50000",
    state: "running",
    fps: 60,
    kbps: 8_000,
    fpsTarget: 60,
    dropped: 0,
    captureToEncodeUs: 0,
    maxCaptureToEncodeUs: 0,
    sendBlockUs: 0,
    maxSendBlockUs: 0,
    pendingFrame: 0,
    frames: 0,
    bytes: 0,
    captureBackend: "screenCaptureKit",
    mediaTransport: "udp",
    firstCaptureMs: 0,
    firstEncodeMs: 0,
    firstSendMs: 0,
    currentBitrate: 0,
    captureIntervalP95Us: 0,
    captureToEncodeP95Us: 0,
    captureQueueWaitP95Us: 0,
    encodeOutputP95Us: 0,
    sendBlockP95Us: 0,
    ...overrides,
  };
}

describe("receiverRenderedFps", () => {
  it("returns a fresh receiver frame rate, including a frozen zero", () => {
    // 신선도는 알려진 피드백 나이로 증명한다 — 새 계약의 필수 요소다.
    expect(
      receiverRenderedFps(session({ renderedFps: 0, receiverFeedbackAgeMs: 120 })),
    ).toBe(0);
    expect(
      receiverRenderedFps(session({ renderedFps: 58, receiverFeedbackAgeMs: 120 })),
    ).toBe(58);
  });

  it("treats null and missing receiver output as unavailable", () => {
    expect(receiverRenderedFps(session({ renderedFps: null }))).toBeUndefined();
    expect(receiverRenderedFps(session())).toBeUndefined();
  });

  it("ignores a stale receiver feedback sample", () => {
    // 수신기 피드백이 멈춘 뒤의 마지막 값은 현재 상태가 아니다 — 폐기한다.
    expect(
      receiverRenderedFps(session({
        renderedFps: 60,
        receiverFeedbackAgeMs: RENDERED_FPS_STALE_MS + 1,
      })),
    ).toBeUndefined();
    expect(
      receiverRenderedFps(session({
        renderedFps: 60,
        receiverFeedbackAgeMs: RENDERED_FPS_STALE_MS,
      })),
    ).toBe(60);
  });

  it("requires a known feedback age — missing or null is not a fresh sample", () => {
    // 피드백 나이를 알 수 없으면 신선도를 증명할 수 없다 — 구 Host라도
    // 측정값을 0으로 만들거나 임의로 받아들이지 않고 생략한다.
    expect(receiverRenderedFps(session({ renderedFps: 55 }))).toBeUndefined();
    expect(
      receiverRenderedFps(session({ renderedFps: 55, receiverFeedbackAgeMs: null })),
    ).toBeUndefined();
  });

  it("rejects NaN and negative feedback ages", () => {
    expect(
      receiverRenderedFps(session({
        renderedFps: 60,
        receiverFeedbackAgeMs: Number.NaN,
      })),
    ).toBeUndefined();
    expect(
      receiverRenderedFps(session({ renderedFps: 60, receiverFeedbackAgeMs: -1 })),
    ).toBeUndefined();
  });

  it("rejects non-finite values", () => {
    expect(
      receiverRenderedFps(session({ renderedFps: Number.NaN })),
    ).toBeUndefined();
    expect(
      receiverRenderedFps(session({ renderedFps: -3 })),
    ).toBeUndefined();
  });
});

describe("receiverRenderedFps split mode", () => {
  // 분할 모드에서 Host는 집계 renderedFps의 0을 null로 바꿔 보낸다(0→nil).
  // 얼어 붙은 수신기를 계속 보이게 하려면 원시 분할 필드(joined 또는
  // 좌/우 최솟값)를 사용해야 한다.
  const split = (overrides: Partial<SessionView> = {}): SessionView => session({
    splitDirection: "vertical",
    renderedFps: null,
    receiverFeedbackAgeMs: 120,
    ...overrides,
  });

  it("keeps a frozen split receiver visible through the reported joined fps", () => {
    expect(
      receiverRenderedFps(split({
        joinedRenderedFps: 0,
        leftRenderedFps: 0,
        rightRenderedFps: 0,
      })),
    ).toBe(0);
    expect(
      receiverRenderedFps(split({
        joinedRenderedFps: 57,
        leftRenderedFps: 57,
        rightRenderedFps: 58,
      })),
    ).toBe(57);
  });

  it("falls back to the valid minimum of both decoders without a joined value", () => {
    expect(
      receiverRenderedFps(split({ leftRenderedFps: 60, rightRenderedFps: 45 })),
    ).toBe(45);
  });

  it("never invents a split sample when the per-decoder fields are absent", () => {
    expect(receiverRenderedFps(split())).toBeUndefined();
    expect(receiverRenderedFps(split({ leftRenderedFps: 60 }))).toBeUndefined();
  });

  it("applies the feedback-age budget to split samples", () => {
    expect(
      receiverRenderedFps(split({
        joinedRenderedFps: 60,
        receiverFeedbackAgeMs: RENDERED_FPS_STALE_MS + 1,
      })),
    ).toBeUndefined();
  });

  it("ignores split decoder fields outside split mode", () => {
    expect(
      receiverRenderedFps(session({
        splitDirection: null,
        renderedFps: 58,
        receiverFeedbackAgeMs: 120,
        joinedRenderedFps: 0,
        leftRenderedFps: 0,
        rightRenderedFps: 0,
      })),
    ).toBe(58);
  });
});

describe("hostQueuePressureUs", () => {
  it("takes the oldest pending age across Host queues", () => {
    expect(
      hostQueuePressureUs(session({
        pendingFrameOldestAgeUs: 40_000,
        splitEncodedQueueOldestUs: 120_000,
        splitCaptureQueueOldestUs: 90_000,
      })),
    ).toBe(120_000);
  });

  it("falls back to the encode pending queue alone", () => {
    expect(
      hostQueuePressureUs(session({ pendingFrameOldestAgeUs: 30_000 })),
    ).toBe(30_000);
  });

  it("reports zero when no queue age metric exists", () => {
    expect(hostQueuePressureUs(session())).toBe(0);
  });
});
