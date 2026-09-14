import type { ReactNode } from "react";
import {
  cn,
  getTranslation,
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import { diagnosticValueVariants } from "./diagnosticStyles";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import type { SessionRow } from "./sessionTypes";

type InspectorStrings = TranslationSchema["host"]["inspector"];

function Metric({ label, children, tone }: { label: string; children: ReactNode; tone?: "default" | "warning" }) {
  return (
    <div className="inspector-item">
      <span className="inspector-item-label">{label}</span>
      <span className={cn(diagnosticValueVariants({ tone }))}>{children}</span>
    </div>
  );
}

function milliseconds(value: number | undefined, fallback: string): string {
  return value === undefined ? fallback : `${(value / 1000).toFixed(1)}ms`;
}

function SplitDiagnostics({ session, diagnostics, t }: { session: SessionRow; diagnostics: ReturnType<typeof encoderDiagnosticsView>; t: InspectorStrings }) {
  if (diagnostics.splitEncode === null) return null;
  const recoveryTone = (session.splitPostEncodeDeltaDrops ?? 0) > 0 || (session.splitWirePairSendFailures ?? 0) > 0 ? "warning" : "default";
  const syncTone = (session.pairSyncTimeouts ?? 0) > 0 ? "warning" : "default";
  return <>
    <Metric label={t.tileOutputLabel}>{diagnostics.splitEncode} · {diagnostics.splitRender}</Metric>
    <Metric label={t.splitPreparationLabel}>{diagnostics.splitLatency}</Metric>
    <Metric label={t.splitFlowLabel}>{diagnostics.splitFlow}</Metric>
    <Metric label={t.splitRecoveryLabel} tone={recoveryTone}>{diagnostics.splitRecovery}</Metric>
    <Metric label={t.tileSyncLabel} tone={syncTone}>{diagnostics.splitSync} · {t.receiverLossWord} {diagnostics.splitLoss}</Metric>
  </>;
}

function PipelineRates({ session, diagnostics, t }: { session: SessionRow; diagnostics: ReturnType<typeof encoderDiagnosticsView>; t: InspectorStrings }) {
  return <>
    <Metric label={t.stageFpsLabel}>
      {session.captureFps ?? t.measuring} / {session.encodeSubmitFps ?? session.fps} / {session.encodeOutputFps ?? t.measuring}
    </Metric>
    <SplitDiagnostics session={session} diagnostics={diagnostics} t={t} />
    <Metric label={t.androidRenderFpsLabel}>
      {session.renderedFps != null ? `${session.renderedFps} FPS` : t.awaitingFeedback}
    </Metric>
    <Metric label={t.encoderSubmitFailuresLabel}>
      {session.encodeSubmitFailures ?? 0} / {session.encodeInFlight ?? 0}
    </Metric>
  </>;
}

function TimingDiagnostics({ session, t }: { session: SessionRow; t: InspectorStrings }) {
  return <>
    <Metric label={t.encoderOutputIntervalLabel}>{milliseconds(session.encodeOutputIntervalP95Us, t.measuring)}</Metric>
    <Metric label={t.captureFetchLabel}>{milliseconds(session.captureToEncodeUs, "<2ms")}</Metric>
    <Metric label={t.processingWaitLabel}>{milliseconds(session.captureQueueWaitUs, "0.1ms")}</Metric>
    <Metric label={t.videoProcessingLabel}>{milliseconds(session.encodeOutputUs, "<2ms")}</Metric>
    <Metric label={t.encoderOutputPacketizationLabel}>
      {milliseconds(session.encodeOutputP95Us, t.measuring)} / {milliseconds(session.packetizationP95Us, t.measuring)}
    </Metric>
    <Metric label={t.networkSendLabel}>{milliseconds(session.sendBlockUs, "0.2ms")}</Metric>
  </>;
}

function TailTimingDiagnostics({ session, t }: { session: SessionRow; t: InspectorStrings }) {
  return <>
    <Metric label={t.tailLatencyLabel}>
      {milliseconds(session.captureToEncodeP95Us, "1.2ms")} / {milliseconds(session.sendBlockP95Us, "0.5ms")}
    </Metric>
    <Metric label={t.udpPacingLabel}>{milliseconds(session.sendPaceP95Us, t.measuring)}</Metric>
  </>;
}

function TransportDiagnostics({ session, transportLabel, t }: { session: SessionRow; transportLabel: string; t: InspectorStrings }) {
  return <>
    <Metric label={t.transportPathLabel}>{transportLabel}</Metric>
    <Metric label={t.udpStabilityLabel}>
      {session.udpStabilityProfile || "legacy"} / {session.udpBurstDatagrams ?? 8} / {session.udpFecParityShards ?? 2}
      {session.udpAdaptivePacing ? ` · ${interpolate(t.adaptiveAuto, { reason: session.udpBurstReason || "initial" })}` : ` · ${t.fixedPacing}`}
    </Metric>
    <Metric label={t.receiverRttLabel}>
      {session.receiverRttMs != null ? `${session.receiverRttMs}ms` : t.measuring} / {session.receiverWireMs != null ? `${session.receiverWireMs}ms` : t.measuring}
    </Metric>
    <Metric label={t.receiverLossFeedbackLabel}>
      {(session.receiverFrameGaps ?? 0) + (session.receiverIncompleteAus ?? 0)} / {session.receiverFeedbackAgeMs != null ? interpolate(t.msAgo, { ms: session.receiverFeedbackAgeMs }) : t.pendingShort}
    </Metric>
  </>;
}

function QueueDiagnostics({ session, t }: { session: SessionRow; t: InspectorStrings }) {
  return <>
    <Metric label={t.hostQueueDropsLabel}>
      {t.dropNormal} {Math.max(0, (session.networkQueueDropped ?? 0) - (session.recoveryFramesDropped ?? 0))} / {t.dropRecovery} {session.recoveryFramesDropped ?? 0} / {t.dropCapture} {session.captureQueueDropped ?? 0}
    </Metric>
    <Metric label={t.hostQueueOccupancyLabel}>
      {session.pendingFrameBytes ?? 0}B / {((session.pendingFrameOldestAgeUs ?? 0) / 1000).toFixed(1)}ms
    </Metric>
    <Metric label={t.recentAuBurstLabel}>
      {session.lastAuBytes !== undefined
        ? `${(session.lastAuBytes / 1024).toFixed(0)}KB · ${session.lastAuFragments ?? 0} + ${interpolate(t.countUnit, { count: session.lastAuParity ?? 0 })} · ${((session.lastAuSendUs ?? 0) / 1000).toFixed(1)}ms`
        : t.measuring}
      {session.lastAuIsKeyframe ? " · IDR" : ""}
    </Metric>
  </>;
}

function FecDiagnostics({ session, t }: { session: SessionRow; t: InspectorStrings }) {
  return <>
    <Metric label={t.udpDatagramLabel}>
      {interpolate(t.sentCount, { count: session.sentDatagrams ?? 0 })} · {interpolate(t.failedCount, { count: session.udpSendFailures ?? 0 })} · parity {session.sentParityDatagrams ?? 0}
    </Metric>
    <Metric label={t.viewerFecLabel}>
      data {session.receiverDataDatagrams ?? 0} · parity {session.receiverParityDatagrams ?? 0} · {interpolate(t.restoredCount, { count: session.receiverFecRestoredFragments ?? 0 })}
    </Metric>
    <Metric label={t.fecUnrecoveredLabel}>
      {session.receiverUnrecoverableFecGroups ?? 0} / {interpolate(t.countUnit, { count: session.receiverMaxMissingDataFragments ?? 0 })}
    </Metric>
    <Metric label={t.frameGapLabel}>
      {session.receiverOneFrameGapEvents ?? 0} / {session.receiverMultiFrameGapEvents ?? 0}
    </Metric>
  </>;
}

function RecoveryDiagnostics({ session, t }: { session: SessionRow; t: InspectorStrings }) {
  return <Metric label={t.recoveryRequestsLabel}>
    {interpolate(t.suppressedCount, { count: session.recoveryRequestsSuppressed ?? 0 })} / {interpolate(t.timesCount, { count: session.recoveryKeyframes ?? 0 })}
  </Metric>;
}

function EncoderTarget({ session, t }: { session: SessionRow; t: InspectorStrings }) {
  return <Metric label={t.encoderTargetLabel}>
    {session.currentBitrate !== undefined ? `${(session.currentBitrate / 1_000_000).toFixed(1)}Mbps` : t.measuring}
  </Metric>;
}

export default function SessionPipelineDiagnostics({ session, transportLabel, language }: { session: SessionRow; transportLabel: string; language: SupportedLanguage }) {
  const t = getTranslation(language).host.inspector;
  const diagnostics = encoderDiagnosticsView(session, language);
  return (
    <>
      <PipelineRates session={session} diagnostics={diagnostics} t={t} />
      <TimingDiagnostics session={session} t={t} />
      <TransportDiagnostics session={session} transportLabel={transportLabel} t={t} />
      <QueueDiagnostics session={session} t={t} />
      <FecDiagnostics session={session} t={t} />
      <RecoveryDiagnostics session={session} t={t} />
      <TailTimingDiagnostics session={session} t={t} />
      <EncoderTarget session={session} t={t} />
    </>
  );
}
