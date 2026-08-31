import type { SessionRow } from "./sessionTypes";

export interface EncoderDiagnosticsView {
  path: string;
  identity: string;
  configuration: string;
  applied: string;
  unavailable: string;
  fallback: string | null;
  experiment: string;
  experimentDetail: string;
  experimentFallback: string;
  pressure: string;
  inFlight: string;
  validOutputFps: string;
  qualityBasis: "Base QP 기반" | null;
  hasEncoderPressure: boolean;
  hasExperimentFallback: boolean;
  splitEncode: string | null;
  splitRender: string | null;
  splitSync: string | null;
  splitLoss: string | null;
  splitLatency: string | null;
  splitFlow: string | null;
  splitRecovery: string | null;
}

const experimentLabels: Record<string, string> = {
  auto: "자동",
  rateControl: "레이트 컨트롤",
  adaptiveQp: "적응형 QP",
  encoderPool: "인코더 풀",
  splitVertical: "4K 수직 분할",
};

function finiteNumber(value: number | null | undefined): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function measuredInteger(value: number | null | undefined): string {
  const measured = finiteNumber(value);
  return measured === undefined ? "측정 중" : String(Math.trunc(measured));
}

function measuredMilliseconds(value: number | null | undefined): string {
  const measured = finiteNumber(value);
  return measured === undefined ? "측정 중" : `${(measured / 1000).toFixed(1)}ms`;
}

function experimentId(value: string | null | undefined): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function experimentLabel(value: string | undefined): string {
  return value === undefined ? "측정 중" : (experimentLabels[value] ?? value);
}

function qpDetail(session: SessionRow): string {
  if (session.baseFrameQp === null) {
    return "QP 적용 안 됨";
  }

  const qp = finiteNumber(session.baseFrameQp);
  if (qp === undefined) {
    return "QP 측정 중";
  }

  const changes = finiteNumber(session.baseFrameQpChanges);
  return changes === undefined
    ? `QP ${Math.trunc(qp)} (조정 횟수 측정 중)`
    : `QP ${Math.trunc(qp)} (${Math.trunc(changes)}회 조정)`;
}

export function encoderDiagnosticsView(session: SessionRow): EncoderDiagnosticsView {
  const experimentDiagnosticsAvailable = session.encoderExperimentDiagnosticsAvailable === true;
  const mode = session.encoderMode === "ave"
    ? "AVE"
    : session.encoderMode === "rtvc"
      ? "RTVC"
      : "미확인";
  const acceleration = session.encoderHardwareAccelerated === true
    ? "하드웨어"
    : session.encoderHardwareAccelerated === false
      ? "소프트웨어"
      : "가속 확인 중";
  const unavailable = [
    session.encoderUnsupportedProperties?.length
      ? `미지원 ${session.encoderUnsupportedProperties.join(", ")}`
      : null,
    session.encoderRejectedProperties?.length
      ? `거부 ${session.encoderRejectedProperties.join(", ")}`
      : null,
  ].filter((value): value is string => value !== null).join(" · ");
  const requestedExperiment = experimentDiagnosticsAvailable
    ? experimentId(session.encoderExperimentRequested)
    : undefined;
  const appliedExperiment = experimentDiagnosticsAvailable
    ? experimentId(session.encoderExperimentApplied)
    : undefined;
  const experimentFallback = experimentDiagnosticsAvailable
    ? (experimentId(session.encoderExperimentFallbackReason) ?? "없음")
    : "측정 중";
  const encoderDrops = experimentDiagnosticsAvailable
    ? finiteNumber(session.encoderFrameDrops)
    : undefined;
  const validOutputFps = experimentDiagnosticsAvailable
    ? finiteNumber(session.validEncodeOutputFps)
    : undefined;
  const isSplit = appliedExperiment === "splitVertical"
    || session.splitDirection === "vertical";

  return {
    path: `${mode} · ${acceleration}`,
    identity: session.encoderID || "인코더 ID 확인 중",
    configuration: `${session.encoderPreset || "preset 확인 중"} · ${session.encoderProfile || "profile 확인 중"}`,
    applied: session.encoderAppliedProperties?.join(", ") || "없음",
    unavailable: unavailable || "없음",
    fallback: session.encoderFallbackReason ?? null,
    experiment: experimentLabel(appliedExperiment ?? requestedExperiment),
    experimentDetail: `요청 ${requestedExperiment ?? "측정 중"} · 적용 ${appliedExperiment ?? "측정 중"} · ${experimentDiagnosticsAvailable ? qpDetail(session) : "QP 측정 중"}`,
    experimentFallback,
    pressure: `드롭 ${measuredInteger(encoderDrops)} · 제출 p95 ${measuredMilliseconds(experimentDiagnosticsAvailable ? session.encodeSubmitCallP95Us : undefined)} · callback p95 ${measuredMilliseconds(experimentDiagnosticsAvailable ? session.encoderCallbackP95Us : undefined)}`,
    inFlight: `인코더 ${measuredInteger(experimentDiagnosticsAvailable ? session.encodeInFlight : undefined)} · 패킷화 ${measuredInteger(experimentDiagnosticsAvailable ? session.packetizationInFlight : undefined)}`,
    validOutputFps: validOutputFps === undefined
      ? "측정 중"
      : `${Math.trunc(validOutputFps)} FPS`,
    qualityBasis: experimentDiagnosticsAvailable && appliedExperiment === "adaptiveQp"
      ? "Base QP 기반"
      : null,
    hasEncoderPressure: encoderDrops !== undefined && encoderDrops > 0,
    hasExperimentFallback: experimentDiagnosticsAvailable && experimentFallback !== "없음",
    splitEncode: isSplit
      ? `L ${measuredInteger(session.leftValidEncodeOutputFps)} / R ${measuredInteger(session.rightValidEncodeOutputFps)} FPS`
      : null,
    splitRender: isSplit
      ? `L ${measuredInteger(session.leftRenderedFps)} / R ${measuredInteger(session.rightRenderedFps)} / 결합 ${measuredInteger(session.joinedRenderedFps)} FPS`
      : null,
    splitSync: isSplit
      ? `p95 ${measuredMilliseconds(session.pairReadyDeltaP95Us)} · max ${measuredMilliseconds(session.pairReadyDeltaMaxUs)} · timeout ${measuredInteger(session.pairSyncTimeouts)} · 불일치 ${measuredInteger(session.unmatchedOutputDrops)}`
      : null,
    splitLoss: isSplit
      ? `L ${measuredInteger(session.leftReceiverLoss)} / R ${measuredInteger(session.rightReceiverLoss)}`
      : null,
    splitLatency: isSplit
      ? `${measuredMilliseconds(session.splitPreparationP95Us)} / ${measuredMilliseconds(session.encodedPairCallbackP95Us)}`
      : null,
    splitFlow: isSplit
      ? `lease ${measuredInteger(session.splitFlowActiveLeases)}/${measuredInteger(session.splitFlowCapacity)} · queue ${measuredInteger(session.splitEncodedQueueDepth)} · oldest ${measuredMilliseconds(session.splitEncodedQueueOldestUs)}`
      : null,
    splitRecovery: isSplit
      ? `capture ${measuredInteger(session.splitPreEncodeAdmissionDrops)} · boundary ${measuredInteger(session.splitRecoveryBoundaryDiscards)} · post-encode ${measuredInteger(session.splitPostEncodeDeltaDrops)} · wire ${measuredInteger(session.splitWirePairsAttempted)}/${measuredInteger(session.splitWirePairSendFailures)} · gap IDR ${measuredInteger(session.splitKeyframeGapRecoveries)} / delta ${measuredInteger(session.splitDeltaGapRecoveries)}`
      : null,
  };
}
