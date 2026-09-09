import type { ReactNode } from "react";
import { diagnosticValueVariants } from "./diagnosticStyles";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import { cn } from "@leftcar/ui-tokens";
import type { SessionRow } from "./sessionTypes";

function Metric({ label, children, tone = "default" }: { label: string; children: ReactNode; tone?: "default" | "warning" }) {
  return (
    <div className="inspector-item">
      <span className="inspector-item-label">{label}</span>
      <span className={tone === "default" ? "inspector-item-value" : cn(diagnosticValueVariants({ tone }))}>{children}</span>
    </div>
  );
}

function milliseconds(value: number | undefined, fallback: string): string {
  return value === undefined ? fallback : `${(value / 1000).toFixed(1)}ms`;
}

function SplitDiagnostics({ session, diagnostics }: { session: SessionRow; diagnostics: ReturnType<typeof encoderDiagnosticsView> }) {
  if (diagnostics.splitEncode === null) return null;
  const recoveryTone = (session.splitPostEncodeDeltaDrops ?? 0) > 0 || (session.splitWirePairSendFailures ?? 0) > 0 ? "warning" : "default";
  const syncTone = (session.pairSyncTimeouts ?? 0) > 0 ? "warning" : "default";
  return <>
    <Metric label="타일 출력/표시">{diagnostics.splitEncode} · {diagnostics.splitRender}</Metric>
    <Metric label="분할 준비 / pair callback p95">{diagnostics.splitLatency}</Metric>
    <Metric label="분할 flow">{diagnostics.splitFlow}</Metric>
    <Metric label="분할 복구" tone={recoveryTone}>{diagnostics.splitRecovery}</Metric>
    <Metric label="타일 동기화" tone={syncTone}>{diagnostics.splitSync} · 수신 손실 {diagnostics.splitLoss}</Metric>
  </>;
}

function PipelineRates({ session, diagnostics }: { session: SessionRow; diagnostics: ReturnType<typeof encoderDiagnosticsView> }) {
  return <>
    <Metric label="단계별 FPS (캡처 / 제출 / 출력)">
      {session.captureFps ?? "측정 중"} / {session.encodeSubmitFps ?? session.fps} / {session.encodeOutputFps ?? "측정 중"}
    </Metric>
    <SplitDiagnostics session={session} diagnostics={diagnostics} />
    <Metric label="실제 Android 렌더 FPS">
      {session.renderedFps != null ? `${session.renderedFps} FPS` : "feedback 대기 중"}
    </Metric>
    <Metric label="인코더 제출 실패 / in-flight">
      {session.encodeSubmitFailures ?? 0} / {session.encodeInFlight ?? 0}
    </Metric>
  </>;
}

function TimingDiagnostics({ session }: { session: SessionRow }) {
  return <>
    <Metric label="인코더 출력 간격 p95">{milliseconds(session.encodeOutputIntervalP95Us, "측정 중")}</Metric>
    <Metric label="화면 가져오기">{milliseconds(session.captureToEncodeUs, "<2ms")}</Metric>
    <Metric label="처리 대기">{milliseconds(session.captureQueueWaitUs, "0.1ms")}</Metric>
    <Metric label="영상 처리">{milliseconds(session.encodeOutputUs, "<2ms")}</Metric>
    <Metric label="인코더 출력 / 패킷화">
      {milliseconds(session.encodeOutputP95Us, "측정 중")} / {milliseconds(session.packetizationP95Us, "측정 중")}
    </Metric>
    <Metric label="네트워크 전송">{milliseconds(session.sendBlockUs, "0.2ms")}</Metric>
  </>;
}

function TailTimingDiagnostics({ session }: { session: SessionRow }) {
  return <>
    <Metric label="P95 처리 / 전송 지연">
      {milliseconds(session.captureToEncodeP95Us, "1.2ms")} / {milliseconds(session.sendBlockP95Us, "0.5ms")}
    </Metric>
    <Metric label="UDP pacing p95">{milliseconds(session.sendPaceP95Us, "측정 중")}</Metric>
  </>;
}

function TransportDiagnostics({ session, transportLabel }: { session: SessionRow; transportLabel: string }) {
  return <>
    <Metric label="실제 전송 경로">{transportLabel}</Metric>
    <Metric label="UDP 안정성 (모드 / burst / FEC)">
      {session.udpStabilityProfile || "legacy"} / {session.udpBurstDatagrams ?? 8} / {session.udpFecParityShards ?? 2}
      {session.udpAdaptivePacing ? ` · 자동 (${session.udpBurstReason || "initial"})` : " · 고정"}
    </Metric>
    <Metric label="수신 RTT / 디코더">
      {session.receiverRttMs != null ? `${session.receiverRttMs}ms` : "측정 중"} / {session.receiverWireMs != null ? `${session.receiverWireMs}ms` : "측정 중"}
    </Metric>
    <Metric label="수신 손실 / feedback">
      {(session.receiverFrameGaps ?? 0) + (session.receiverIncompleteAus ?? 0)} / {session.receiverFeedbackAgeMs != null ? `${session.receiverFeedbackAgeMs}ms 전` : "대기 중"}
    </Metric>
  </>;
}

function QueueDiagnostics({ session }: { session: SessionRow }) {
  return <>
    <Metric label="Host 큐 드롭">
      일반 {Math.max(0, (session.networkQueueDropped ?? 0) - (session.recoveryFramesDropped ?? 0))} / 복구 {session.recoveryFramesDropped ?? 0} / 캡처 {session.captureQueueDropped ?? 0}
    </Metric>
    <Metric label="Host 큐 점유 / oldest">
      {session.pendingFrameBytes ?? 0}B / {((session.pendingFrameOldestAgeUs ?? 0) / 1000).toFixed(1)}ms
    </Metric>
    <Metric label="최근 AU burst">
      {session.lastAuBytes !== undefined
        ? `${(session.lastAuBytes / 1024).toFixed(0)}KB · ${session.lastAuFragments ?? 0} + ${session.lastAuParity ?? 0}개 · ${((session.lastAuSendUs ?? 0) / 1000).toFixed(1)}ms`
        : "측정 중"}
      {session.lastAuIsKeyframe ? " · IDR" : ""}
    </Metric>
  </>;
}

function FecDiagnostics({ session }: { session: SessionRow }) {
  return <>
    <Metric label="UDP datagram">
      {session.sentDatagrams ?? 0}개 전송 · 실패 {session.udpSendFailures ?? 0} · parity {session.sentParityDatagrams ?? 0}
    </Metric>
    <Metric label="Viewer FEC 수신 / 복구">
      data {session.receiverDataDatagrams ?? 0} · parity {session.receiverParityDatagrams ?? 0} · 복원 {session.receiverFecRestoredFragments ?? 0}
    </Metric>
    <Metric label="FEC 미복구 / 최대 누락">
      {session.receiverUnrecoverableFecGroups ?? 0} / {session.receiverMaxMissingDataFragments ?? 0}개
    </Metric>
    <Metric label="단일 / 다중 프레임 Gap">
      {session.receiverOneFrameGapEvents ?? 0} / {session.receiverMultiFrameGapEvents ?? 0}
    </Metric>
  </>;
}

function RecoveryDiagnostics({ session }: { session: SessionRow }) {
  return <Metric label="복구 요청 / IDR">
    {session.recoveryRequestsSuppressed ?? 0} 억제 / {session.recoveryKeyframes ?? 0}회
  </Metric>;
}

function EncoderTarget({ session }: { session: SessionRow }) {
  return <Metric label="현재 인코더 목표">
    {session.currentBitrate !== undefined ? `${(session.currentBitrate / 1_000_000).toFixed(1)}Mbps` : "측정 중"}
  </Metric>;
}

export default function SessionPipelineDiagnostics({ session, transportLabel }: { session: SessionRow; transportLabel: string }) {
  const diagnostics = encoderDiagnosticsView(session);
  return (
    <>
      <PipelineRates session={session} diagnostics={diagnostics} />
      <TimingDiagnostics session={session} />
      <TransportDiagnostics session={session} transportLabel={transportLabel} />
      <QueueDiagnostics session={session} />
      <FecDiagnostics session={session} />
      <RecoveryDiagnostics session={session} />
      <TailTimingDiagnostics session={session} />
      <EncoderTarget session={session} />
    </>
  );
}
