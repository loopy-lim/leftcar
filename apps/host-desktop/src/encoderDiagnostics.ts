import {
  getTranslation,
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
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
  qualityBasis: string | null;
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

type InspectorStrings = TranslationSchema["host"]["inspector"];

function experimentLabels(t: InspectorStrings): Record<string, string> {
  return {
    auto: t.auto,
    rateControl: t.experimentRateControl,
    adaptiveQp: t.experimentAdaptiveQp,
    encoderPool: t.experimentEncoderPool,
    splitVertical: t.experimentSplitVertical,
  };
}

function finiteNumber(value: number | null | undefined): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function measuredInteger(value: number | null | undefined, measuring: string): string {
  const measured = finiteNumber(value);
  return measured === undefined ? measuring : String(Math.trunc(measured));
}

function measuredMilliseconds(value: number | null | undefined, measuring: string): string {
  const measured = finiteNumber(value);
  return measured === undefined ? measuring : `${(measured / 1000).toFixed(1)}ms`;
}

function experimentId(value: string | null | undefined): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function experimentLabel(
  value: string | undefined,
  labels: Record<string, string>,
  measuring: string,
): string {
  return value === undefined ? measuring : (labels[value] ?? value);
}

function encoderModeLabel(mode: SessionRow["encoderMode"], t: InspectorStrings): string {
  switch (mode) {
    case "ave":
      return "AVE";
    case "rtvc":
      return "RTVC";
    default:
      return t.encoderModeUnknown;
  }
}

function accelerationLabel(
  accelerated: SessionRow["encoderHardwareAccelerated"],
  t: InspectorStrings,
): string {
  switch (accelerated) {
    case true:
      return t.hardware;
    case false:
      return t.software;
    default:
      return t.accelerationChecking;
  }
}

function qpDetail(session: SessionRow, t: InspectorStrings): string {
  if (session.baseFrameQp === null) {
    return t.qpNotApplied;
  }

  const qp = finiteNumber(session.baseFrameQp);
  if (qp === undefined) {
    return t.qpMeasuring;
  }

  const changes = finiteNumber(session.baseFrameQpChanges);
  return changes === undefined
    ? interpolate(t.qpAdjustmentsMeasuring, { qp: Math.trunc(qp) })
    : interpolate(t.qpAdjustments, { qp: Math.trunc(qp), count: Math.trunc(changes) });
}

export function encoderDiagnosticsView(
  session: SessionRow,
  language: SupportedLanguage = "ko",
): EncoderDiagnosticsView {
  const t = getTranslation(language).host.inspector;
  const experimentDiagnosticsAvailable = session.encoderExperimentDiagnosticsAvailable === true;
  const mode = encoderModeLabel(session.encoderMode, t);
  const acceleration = accelerationLabel(session.encoderHardwareAccelerated, t);
  const unavailable = [
    session.encoderUnsupportedProperties?.length
      ? interpolate(t.unsupportedValue, { value: session.encoderUnsupportedProperties.join(", ") })
      : null,
    session.encoderRejectedProperties?.length
      ? interpolate(t.rejectedValue, { value: session.encoderRejectedProperties.join(", ") })
      : null,
  ].filter((value): value is string => value !== null).join(" · ");
  const requestedExperiment = experimentDiagnosticsAvailable
    ? experimentId(session.encoderExperimentRequested)
    : undefined;
  const appliedExperiment = experimentDiagnosticsAvailable
    ? experimentId(session.encoderExperimentApplied)
    : undefined;
  const experimentFallback = experimentDiagnosticsAvailable
    ? (experimentId(session.encoderExperimentFallbackReason) ?? t.noneValue)
    : t.measuring;
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
    identity: session.encoderID || t.encoderIdChecking,
    configuration: `${session.encoderPreset || t.presetChecking} · ${session.encoderProfile || t.profileChecking}`,
    applied: session.encoderAppliedProperties?.join(", ") || t.noneValue,
    unavailable: unavailable || t.noneValue,
    fallback: session.encoderFallbackReason ?? null,
    experiment: experimentLabel(appliedExperiment ?? requestedExperiment, experimentLabels(t), t.measuring),
    experimentDetail: `${interpolate(t.requestedValue, { value: requestedExperiment ?? t.measuring })} · ${interpolate(t.appliedValue, { value: appliedExperiment ?? t.measuring })} · ${experimentDiagnosticsAvailable ? qpDetail(session, t) : t.qpMeasuring}`,
    experimentFallback,
    pressure: `${interpolate(t.dropsCount, { count: measuredInteger(encoderDrops, t.measuring) })} · ${interpolate(t.submitP95Value, { value: measuredMilliseconds(experimentDiagnosticsAvailable ? session.encodeSubmitCallP95Us : undefined, t.measuring) })} · ${interpolate(t.callbackP95Value, { value: measuredMilliseconds(experimentDiagnosticsAvailable ? session.encoderCallbackP95Us : undefined, t.measuring) })}`,
    inFlight: `${t.encoderWord} ${measuredInteger(experimentDiagnosticsAvailable ? session.encodeInFlight : undefined, t.measuring)} · ${t.packetizationWord} ${measuredInteger(experimentDiagnosticsAvailable ? session.packetizationInFlight : undefined, t.measuring)}`,
    validOutputFps: validOutputFps === undefined
      ? t.measuring
      : `${Math.trunc(validOutputFps)} FPS`,
    qualityBasis: experimentDiagnosticsAvailable && appliedExperiment === "adaptiveQp"
      ? t.baseQpBasis
      : null,
    hasEncoderPressure: encoderDrops !== undefined && encoderDrops > 0,
    hasExperimentFallback: experimentDiagnosticsAvailable && experimentFallback !== t.noneValue,
    splitEncode: isSplit
      ? `L ${measuredInteger(session.leftValidEncodeOutputFps, t.measuring)} / R ${measuredInteger(session.rightValidEncodeOutputFps, t.measuring)} FPS`
      : null,
    splitRender: isSplit
      ? `L ${measuredInteger(session.leftRenderedFps, t.measuring)} / R ${measuredInteger(session.rightRenderedFps, t.measuring)} / ${t.joinedWord} ${measuredInteger(session.joinedRenderedFps, t.measuring)} FPS`
      : null,
    splitSync: isSplit
      ? `p95 ${measuredMilliseconds(session.pairReadyDeltaP95Us, t.measuring)} · max ${measuredMilliseconds(session.pairReadyDeltaMaxUs, t.measuring)} · timeout ${measuredInteger(session.pairSyncTimeouts, t.measuring)} · ${t.mismatchWord} ${measuredInteger(session.unmatchedOutputDrops, t.measuring)}`
      : null,
    splitLoss: isSplit
      ? `L ${measuredInteger(session.leftReceiverLoss, t.measuring)} / R ${measuredInteger(session.rightReceiverLoss, t.measuring)}`
      : null,
    splitLatency: isSplit
      ? `${measuredMilliseconds(session.splitPreparationP95Us, t.measuring)} / ${measuredMilliseconds(session.encodedPairCallbackP95Us, t.measuring)}`
      : null,
    splitFlow: isSplit
      ? `lease ${measuredInteger(session.splitFlowActiveLeases, t.measuring)}/${measuredInteger(session.splitFlowCapacity, t.measuring)} · queue ${measuredInteger(session.splitEncodedQueueDepth, t.measuring)} · oldest ${measuredMilliseconds(session.splitEncodedQueueOldestUs, t.measuring)}`
      : null,
    splitRecovery: isSplit
      ? `capture ${measuredInteger(session.splitPreEncodeAdmissionDrops, t.measuring)} · boundary ${measuredInteger(session.splitRecoveryBoundaryDiscards, t.measuring)} · post-encode ${measuredInteger(session.splitPostEncodeDeltaDrops, t.measuring)} · wire ${measuredInteger(session.splitWirePairsAttempted, t.measuring)}/${measuredInteger(session.splitWirePairSendFailures, t.measuring)} · gap IDR ${measuredInteger(session.splitKeyframeGapRecoveries, t.measuring)} / delta ${measuredInteger(session.splitDeltaGapRecoveries, t.measuring)}`
      : null,
  };
}
