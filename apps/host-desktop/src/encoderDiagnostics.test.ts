import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import SessionInspector from "./SessionInspector";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import type { SessionRow } from "./sessionTypes";

const session = {
  encoderMode: "ave",
  encoderID: "com.apple.videotoolbox.videoencoder.ave.avc",
  encoderHardwareAccelerated: true,
  encoderPreset: "high-speed",
  encoderProfile: "main",
  encoderAppliedProperties: ["HighSpeed", "Quality"],
  encoderUnsupportedProperties: ["SuggestedLookAheadFrameCount"],
  encoderRejectedProperties: ["Quality=-12900"],
  encoderFallbackReason: null,
} as SessionRow;

const adaptiveSession = {
  ...session,
  encoderExperimentDiagnosticsAvailable: true,
  encoderExperimentRequested: "adaptiveQp",
  encoderExperimentApplied: "adaptiveQp",
  encoderExperimentFallbackReason: null,
  encoderFrameDrops: 3,
  encoderFrameDropFps: 1,
  validEncodeOutputFps: 58,
  encodeSubmitCallP50Us: 8_600,
  encodeSubmitCallP95Us: 17_200,
  encoderCallbackP50Us: 9_100,
  encoderCallbackP95Us: 18_100,
  encodeInFlight: 0,
  packetizationInFlight: 1,
  baseFrameQp: 34,
  baseFrameQpChanges: 2,
} as SessionRow;

function renderInspector(currentSession: SessionRow): string {
  return renderToStaticMarkup(
    createElement(SessionInspector, {
      session: currentSession,
      transportLabel: "Wi-Fi UDP",
      qualitySupported: true,
      qualityPercent: 40,
      qualityBusy: false,
      onSetQuality: async () => {},
    }),
  );
}

describe("encoder diagnostics", () => {
  it("keeps the actual AVE identity and property outcomes visible", () => {
    expect(encoderDiagnosticsView(session)).toMatchObject({
      path: "AVE · 하드웨어",
      identity: "com.apple.videotoolbox.videoencoder.ave.avc",
      configuration: "high-speed · main",
      applied: "HighSpeed, Quality",
      unavailable: "미지원 SuggestedLookAheadFrameCount · 거부 Quality=-12900",
      fallback: null,
    });
  });

  it("shows the applied adaptive QP profile and true encoder pressure", () => {
    expect(encoderDiagnosticsView(adaptiveSession)).toMatchObject({
      experiment: "적응형 QP",
      experimentDetail: "요청 adaptiveQp · 적용 adaptiveQp · QP 34 (2회 조정)",
      pressure: "드롭 3 · 제출 p95 17.2ms · callback p95 18.1ms",
      inFlight: "인코더 0 · 패킷화 1",
      validOutputFps: "58 FPS",
      qualityBasis: "Base QP 기반",
    });
  });

  it.each([
    ["auto", "자동"],
    ["rateControl", "레이트 컨트롤"],
    ["adaptiveQp", "적응형 QP"],
    ["encoderPool", "인코더 풀"],
    ["splitVertical", "4K 수직 분할"],
  ])("maps the known %s experiment label", (experimentId, expectedLabel) => {
    expect(encoderDiagnosticsView({
      ...adaptiveSession,
      encoderExperimentApplied: experimentId,
    }).experiment).toBe(expectedLabel);
  });

  it("keeps the existing quality wording outside adaptive QP", () => {
    expect(encoderDiagnosticsView({
      ...adaptiveSession,
      encoderExperimentApplied: "rateControl",
    }).qualityBasis).toBeNull();
  });

  it("preserves unknown requested and applied experiment IDs", () => {
    expect(encoderDiagnosticsView({
      ...adaptiveSession,
      encoderExperimentRequested: "futureRequested",
      encoderExperimentApplied: "futureApplied",
    })).toMatchObject({
      experiment: "futureApplied",
      experimentDetail: "요청 futureRequested · 적용 futureApplied · QP 34 (2회 조정)",
    });
  });

  it("shows measuring states for old-shim defaults when diagnostics are unavailable", () => {
    expect(encoderDiagnosticsView({
      ...session,
      encoderExperimentDiagnosticsAvailable: false,
      encoderExperimentRequested: "auto",
      encoderExperimentApplied: "rateControl",
      encoderExperimentFallbackReason: null,
      encoderFrameDrops: 0,
      validEncodeOutputFps: 0,
      encodeSubmitCallP95Us: 0,
      encoderCallbackP95Us: 0,
      encodeInFlight: 0,
      packetizationInFlight: 0,
      baseFrameQp: null,
      baseFrameQpChanges: 0,
    } as SessionRow)).toMatchObject({
      experiment: "측정 중",
      experimentDetail: "요청 측정 중 · 적용 측정 중 · QP 측정 중",
      experimentFallback: "측정 중",
      pressure: "드롭 측정 중 · 제출 p95 측정 중 · callback p95 측정 중",
      inFlight: "인코더 측정 중 · 패킷화 측정 중",
      validOutputFps: "측정 중",
      qualityBasis: null,
      hasEncoderPressure: false,
      hasExperimentFallback: false,
    });
  });

  it("renders legitimate zero measurements when diagnostics are available", () => {
    expect(encoderDiagnosticsView({
      ...adaptiveSession,
      encoderExperimentRequested: "rateControl",
      encoderExperimentApplied: "rateControl",
      encoderFrameDrops: 0,
      validEncodeOutputFps: 0,
      encodeSubmitCallP95Us: 0,
      encoderCallbackP95Us: 0,
      encodeInFlight: 0,
      packetizationInFlight: 0,
      baseFrameQp: null,
      baseFrameQpChanges: 0,
    })).toMatchObject({
      experiment: "레이트 컨트롤",
      experimentDetail: "요청 rateControl · 적용 rateControl · QP 적용 안 됨",
      experimentFallback: "없음",
      pressure: "드롭 0 · 제출 p95 0.0ms · callback p95 0.0ms",
      inFlight: "인코더 0 · 패킷화 0",
      validOutputFps: "0 FPS",
      qualityBasis: null,
      hasEncoderPressure: false,
      hasExperimentFallback: false,
    });
  });

  it("shows no experiment fallback when the Host reports none", () => {
    expect(encoderDiagnosticsView(adaptiveSession).experimentFallback).toBe("없음");
  });

  it("shows an explicit experiment fallback independently from encoder identity fallback", () => {
    expect(encoderDiagnosticsView({
      ...adaptiveSession,
      encoderExperimentFallbackReason: "RTVC adaptive QP unavailable",
      encoderFallbackReason: "AVE identity unavailable",
    })).toMatchObject({
      experimentFallback: "RTVC adaptive QP unavailable",
      fallback: "AVE identity unavailable",
    });
  });

  it("never folds network or recovery loss into encoder drops", () => {
    expect(encoderDiagnosticsView({
      ...adaptiveSession,
      encoderFrameDrops: 0,
      networkDropped: 99,
      networkQueueDropped: 88,
      recoveryFramesDropped: 77,
    }).pressure).toBe("드롭 0 · 제출 p95 17.2ms · callback p95 18.1ms");
  });

  it("renders the experiment and encoder-pressure rows as read-only diagnostics", () => {
    const html = renderInspector({
      ...adaptiveSession,
      qualityOverride: 0.4,
    });

    expect(html).toContain("실험 프로필");
    expect(html).toContain("적응형 QP");
    expect(html).toContain("실험 fallback 없음");
    expect(html).toContain("인코더 압력");
    expect(html).toContain("드롭 3 · 제출 p95 17.2ms · callback p95 18.1ms");
    expect(html).toContain("인코더/패킷화 in-flight");
    expect(html).toContain("인코더 0 · 패킷화 1");
    expect(html).toContain("유효 출력 FPS");
    expect(html).toContain("58 FPS");
    expect(html).toContain("40% 고정 · Base QP 기반");
  });

  it("keeps the existing quality result wording for non-adaptive profiles", () => {
    const html = renderInspector({
      ...adaptiveSession,
      encoderExperimentApplied: "rateControl",
      qualityOverride: 0.4,
    });

    expect(html).toContain("40% 고정");
    expect(html).not.toContain("Base QP 기반");
  });

  it("shows both tile pipelines and pair synchronization for a split stream", () => {
    const splitSession = {
      ...adaptiveSession,
      encoderExperimentRequested: "splitVertical",
      encoderExperimentApplied: "splitVertical",
      splitDirection: "vertical",
      leftValidEncodeOutputFps: 60,
      rightValidEncodeOutputFps: 59,
      leftRenderedFps: 60,
      rightRenderedFps: 59,
      joinedRenderedFps: 59,
      leftReceiverLoss: 2,
      rightReceiverLoss: 3,
      pairReadyDeltaP95Us: 2_300,
      pairReadyDeltaMaxUs: 9_800,
      pairSyncTimeouts: 1,
      unmatchedOutputDrops: 4,
      splitPreparationP95Us: 1_800,
      encodedPairCallbackP95Us: 41_200,
      splitFlowActiveLeases: 3,
      splitFlowCapacity: 5,
      splitPreEncodeAdmissionDrops: 8,
      splitEncodedQueueDepth: 1,
      splitEncodedQueueOldestUs: 12_300,
      splitRecoveryBoundaryDiscards: 4,
      splitPostEncodeDeltaDrops: 0,
      splitWirePairsAttempted: 600,
      splitWirePairSendFailures: 0,
      splitKeyframeGapRecoveries: 1,
      splitDeltaGapRecoveries: 1,
    } as SessionRow;

    expect(encoderDiagnosticsView(splitSession)).toMatchObject({
      experiment: "4K 수직 분할",
      splitEncode: "L 60 / R 59 FPS",
      splitRender: "L 60 / R 59 / 결합 59 FPS",
      splitSync: "p95 2.3ms · max 9.8ms · timeout 1 · 불일치 4",
      splitLoss: "L 2 / R 3",
      splitLatency: "1.8ms / 41.2ms",
      splitFlow: "lease 3/5 · queue 1 · oldest 12.3ms",
      splitRecovery: "capture 8 · boundary 4 · post-encode 0 · wire 600/0 · gap IDR 1 / delta 1",
    });
    const html = renderInspector(splitSession);
    expect(html).toContain("타일 출력/표시");
    expect(html).toContain("L 60 / R 59 FPS");
    expect(html).toContain("결합 59 FPS");
    expect(html).toContain("타일 동기화");
    expect(html).toContain("분할 준비 / pair callback p95");
    expect(html).toContain("1.8ms / 41.2ms");
    expect(html).toContain("분할 flow");
    expect(html).toContain("lease 3/5 · queue 1 · oldest 12.3ms");
    expect(html).toContain("분할 복구");
    expect(html).toContain("capture 8 · boundary 4 · post-encode 0");
  });
});
