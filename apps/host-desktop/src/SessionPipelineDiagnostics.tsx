import {
  getTranslation,
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import { DiagnosticMetric as Metric } from "./DiagnosticMetric";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import type { SessionRow } from "./sessionTypes";

type InspectorStrings = TranslationSchema["host"]["inspector"];

function milliseconds(value: number | undefined, fallback: string): string {
  return measured(value) ? `${(value / 1000).toFixed(1)}ms` : fallback;
}

function measured(value: number | undefined): value is number {
  return value !== undefined && Number.isFinite(value) && value >= 0;
}

function count(value: number | undefined, fallback: string): string | number {
  return measured(value) ? value : fallback;
}

function receiverLoss(session: SessionRow, fallback: string): string | number {
  if (
    !measured(session.receiverFrameGaps) ||
    !measured(session.receiverIncompleteAus)
  )
    return fallback;
  return session.receiverFrameGaps + session.receiverIncompleteAus;
}

function normalQueueDrops(
  session: SessionRow,
  fallback: string,
): string | number {
  if (
    !measured(session.networkQueueDropped) ||
    !measured(session.recoveryFramesDropped)
  )
    return fallback;
  return Math.max(
    0,
    session.networkQueueDropped - session.recoveryFramesDropped,
  );
}

function pacingStatus(session: SessionRow, t: InspectorStrings): string {
  if (session.udpAdaptivePacing === undefined) return t.measuring;
  if (!session.udpAdaptivePacing) return t.fixedPacing;
  return interpolate(t.adaptiveAuto, {
    reason: session.udpBurstReason || t.measuring,
  });
}

function SplitDiagnostics({
  session,
  diagnostics,
  t,
}: {
  session: SessionRow;
  diagnostics: ReturnType<typeof encoderDiagnosticsView>;
  t: InspectorStrings;
}) {
  if (diagnostics.splitEncode === null) return null;
  const recoveryTone =
    (session.splitPostEncodeDeltaDrops ?? 0) > 0 ||
    (session.splitWirePairSendFailures ?? 0) > 0
      ? "warning"
      : "default";
  const syncTone = (session.pairSyncTimeouts ?? 0) > 0 ? "warning" : "default";
  return (
    <>
      <Metric label={t.tileOutputLabel}>
        {diagnostics.splitEncode} · {diagnostics.splitRender}
      </Metric>
      <Metric label={t.splitPreparationLabel}>
        {diagnostics.splitLatency}
      </Metric>
      <Metric label={t.splitFlowLabel}>{diagnostics.splitFlow}</Metric>
      <Metric label={t.splitRecoveryLabel} tone={recoveryTone}>
        {diagnostics.splitRecovery}
      </Metric>
      <Metric label={t.tileSyncLabel} tone={syncTone}>
        {diagnostics.splitSync} · {t.receiverLossWord} {diagnostics.splitLoss}
      </Metric>
    </>
  );
}

function PipelineRates({
  session,
  diagnostics,
  t,
}: {
  session: SessionRow;
  diagnostics: ReturnType<typeof encoderDiagnosticsView>;
  t: InspectorStrings;
}) {
  return (
    <>
      <Metric label={t.stageFpsLabel}>
        {count(session.captureFps, t.measuring)} /{" "}
        {count(session.encodeSubmitFps, t.measuring)} /{" "}
        {count(session.encodeOutputFps, t.measuring)}
      </Metric>
      <SplitDiagnostics session={session} diagnostics={diagnostics} t={t} />
      <Metric label={t.androidRenderFpsLabel}>
        {session.renderedFps != null
          ? `${session.renderedFps} FPS`
          : t.awaitingFeedback}
      </Metric>
      <Metric label={t.encoderSubmitFailuresLabel}>
        {count(session.encodeSubmitFailures, t.measuring)} /{" "}
        {count(session.encodeInFlight, t.measuring)}
      </Metric>
    </>
  );
}

function TimingDiagnostics({
  session,
  t,
}: {
  session: SessionRow;
  t: InspectorStrings;
}) {
  return (
    <>
      <Metric label={t.encoderOutputIntervalLabel}>
        {milliseconds(session.encodeOutputIntervalP95Us, t.measuring)}
      </Metric>
      <Metric label={t.captureFetchLabel}>
        {milliseconds(session.captureToEncodeUs, t.measuring)}
      </Metric>
      <Metric label={t.processingWaitLabel}>
        {milliseconds(session.captureQueueWaitUs, t.measuring)}
      </Metric>
      <Metric label={t.videoProcessingLabel}>
        {milliseconds(session.encodeOutputUs, t.measuring)}
      </Metric>
      <Metric label={t.encoderOutputPacketizationLabel}>
        {milliseconds(session.encodeOutputP95Us, t.measuring)} /{" "}
        {milliseconds(session.packetizationP95Us, t.measuring)}
      </Metric>
      <Metric label={t.networkSendLabel}>
        {milliseconds(session.sendBlockUs, t.measuring)}
      </Metric>
    </>
  );
}

function TailTimingDiagnostics({
  session,
  t,
}: {
  session: SessionRow;
  t: InspectorStrings;
}) {
  return (
    <>
      <Metric label={t.tailLatencyLabel}>
        {milliseconds(session.captureToEncodeP95Us, t.measuring)} /{" "}
        {milliseconds(session.sendBlockP95Us, t.measuring)}
      </Metric>
      <Metric label={t.udpPacingLabel}>
        {milliseconds(session.sendPaceP95Us, t.measuring)}
      </Metric>
    </>
  );
}

function TransportDiagnostics({
  session,
  transportLabel,
  t,
}: {
  session: SessionRow;
  transportLabel: string;
  t: InspectorStrings;
}) {
  return (
    <>
      <Metric label={t.transportPathLabel}>{transportLabel}</Metric>
      <Metric label={t.udpStabilityLabel}>
        {session.udpStabilityProfile || t.measuring} /{" "}
        {count(session.udpBurstDatagrams, t.measuring)} /{" "}
        {count(session.udpFecParityShards, t.measuring)}
        {` · ${pacingStatus(session, t)}`}
      </Metric>
      <Metric label={t.receiverRttLabel}>
        {session.receiverRttMs != null
          ? `${session.receiverRttMs}ms`
          : t.measuring}{" "}
        /{" "}
        {session.receiverWireMs != null
          ? `${session.receiverWireMs}ms`
          : t.measuring}
      </Metric>
      <Metric label={t.receiverLossFeedbackLabel}>
        {receiverLoss(session, t.measuring)} /{" "}
        {session.receiverFeedbackAgeMs != null
          ? interpolate(t.msAgo, { ms: session.receiverFeedbackAgeMs })
          : t.pendingShort}
      </Metric>
    </>
  );
}

function QueueDiagnostics({
  session,
  t,
}: {
  session: SessionRow;
  t: InspectorStrings;
}) {
  return (
    <>
      <Metric label={t.hostQueueDropsLabel}>
        {t.dropNormal} {normalQueueDrops(session, t.measuring)} /{" "}
        {t.dropRecovery} {count(session.recoveryFramesDropped, t.measuring)} /{" "}
        {t.dropCapture} {count(session.captureQueueDropped, t.measuring)}
      </Metric>
      <Metric label={t.hostQueueOccupancyLabel}>
        {measured(session.pendingFrameBytes)
          ? `${session.pendingFrameBytes}B`
          : t.measuring}{" "}
        / {milliseconds(session.pendingFrameOldestAgeUs, t.measuring)}
      </Metric>
      <Metric label={t.recentAuBurstLabel}>
        {measured(session.lastAuBytes)
          ? `${(session.lastAuBytes / 1024).toFixed(0)}KB · ${count(session.lastAuFragments, t.measuring)} + ${interpolate(t.countUnit, { count: count(session.lastAuParity, t.measuring) })} · ${milliseconds(session.lastAuSendUs, t.measuring)}`
          : t.measuring}
        {session.lastAuIsKeyframe ? " · IDR" : ""}
      </Metric>
    </>
  );
}

function FecDiagnostics({
  session,
  t,
}: {
  session: SessionRow;
  t: InspectorStrings;
}) {
  return (
    <>
      <Metric label={t.udpDatagramLabel}>
        {interpolate(t.sentCount, {
          count: count(session.sentDatagrams, t.measuring),
        })}{" "}
        ·{" "}
        {interpolate(t.failedCount, {
          count: count(session.udpSendFailures, t.measuring),
        })}{" "}
        · parity {count(session.sentParityDatagrams, t.measuring)}
      </Metric>
      <Metric label={t.viewerFecLabel}>
        data {count(session.receiverDataDatagrams, t.measuring)} · parity{" "}
        {count(session.receiverParityDatagrams, t.measuring)} ·{" "}
        {interpolate(t.restoredCount, {
          count: count(session.receiverFecRestoredFragments, t.measuring),
        })}
      </Metric>
      <Metric label={t.fecUnrecoveredLabel}>
        {count(session.receiverUnrecoverableFecGroups, t.measuring)} /{" "}
        {interpolate(t.countUnit, {
          count: count(session.receiverMaxMissingDataFragments, t.measuring),
        })}
      </Metric>
      <Metric label={t.frameGapLabel}>
        {count(session.receiverOneFrameGapEvents, t.measuring)} /{" "}
        {count(session.receiverMultiFrameGapEvents, t.measuring)}
      </Metric>
    </>
  );
}

function RecoveryDiagnostics({
  session,
  t,
}: {
  session: SessionRow;
  t: InspectorStrings;
}) {
  return (
    <Metric label={t.recoveryRequestsLabel}>
      {interpolate(t.suppressedCount, {
        count: count(session.recoveryRequestsSuppressed, t.measuring),
      })}{" "}
      /{" "}
      {interpolate(t.timesCount, {
        count: count(session.recoveryKeyframes, t.measuring),
      })}
    </Metric>
  );
}

function EncoderTarget({
  session,
  t,
}: {
  session: SessionRow;
  t: InspectorStrings;
}) {
  return (
    <Metric label={t.encoderTargetLabel}>
      {measured(session.currentBitrate)
        ? `${(session.currentBitrate / 1_000_000).toFixed(1)}Mbps`
        : t.measuring}
    </Metric>
  );
}

export default function SessionPipelineDiagnostics({
  session,
  transportLabel,
  language,
}: {
  session: SessionRow;
  transportLabel: string;
  language: SupportedLanguage;
}) {
  const t = getTranslation(language).host.inspector;
  const diagnostics = encoderDiagnosticsView(session, language);
  return (
    <>
      <PipelineRates session={session} diagnostics={diagnostics} t={t} />
      <TimingDiagnostics session={session} t={t} />
      <TransportDiagnostics
        session={session}
        transportLabel={transportLabel}
        t={t}
      />
      <QueueDiagnostics session={session} t={t} />
      <FecDiagnostics session={session} t={t} />
      <RecoveryDiagnostics session={session} t={t} />
      <TailTimingDiagnostics session={session} t={t} />
      <EncoderTarget session={session} t={t} />
    </>
  );
}
