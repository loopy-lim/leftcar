import { Info, Square, Tv } from "lucide-react";
import { interpolate, type TranslationSchema } from "@leftcar/ui-tokens";
import type { SessionRow } from "../sessionTypes";
import SessionInspector from "../SessionInspector";
import { buttonVariants, controlToggleVariants } from "../lib/variants";

export interface SessionCardProps {
  session: SessionRow;
  inputPermission: boolean;
  inputBusy: boolean;
  showInspector: boolean;
  /** 이 세션의 뷰어가 잠금 배너 탭으로 입력 허용을 요청 중이다. */
  inputRequested?: boolean;
  t: TranslationSchema;
  onToggleInput: (session: SessionRow) => Promise<void>;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
  qualityBusy: boolean;
  onForceStop: (session: SessionRow) => void;
}

function sessionTransportLabel(session: SessionRow, t: TranslationSchema) {
  const labels: Record<string, string> = { usb: t.host.cableUsb, udp: t.host.wifiWireless };
  const transport = session.mediaTransport;
  return transport ? labels[transport] ?? transport : t.host.unknownTransport;
}

function remoteInputLabel(busy: boolean, enabled: boolean, t: TranslationSchema) {
  if (busy) return t.host.remoteInputProcessing;
  return enabled ? t.host.remoteInputAllowed : t.host.remoteInputOff;
}

function sessionStateLabel(state: string, t: TranslationSchema, badge = false) {
  if (state === "running") return badge ? t.host.liveBadge : t.host.statusRunning;
  return t.host.statusChecking;
}

function DroppedFrames({ dropped = 0, t }: { dropped?: number; t: TranslationSchema }) {
  if (dropped > 0) return <span className="font-rose">{interpolate(t.host.droppedFrames, { count: dropped })}</span>;
  return <span className="font-emerald">{t.host.stabilityStable}</span>;
}

export function SessionCard({
  session,
  inputPermission,
  inputBusy,
  showInspector,
  inputRequested = false,
  t,
  onToggleInput,
  onSetQuality,
  qualityBusy,
  onForceStop,
}: SessionCardProps) {
  const bitrateMbps = Math.max(0, session.kbps / 1000).toFixed(1);
  const encodeOutputFps = session.encodeOutputFps ?? session.fps;
  const transportLabel = sessionTransportLabel(session, t);
  const qualitySupported = session.qualityHint != null;
  const qualityPercent = Math.round((session.qualityOverride ?? session.qualityHint ?? 0.5) * 100);

  return (
    <div className="stream-card-item">
      <div className="stream-card-top-row">
        <div className="stream-card-identity">
          <div className="stream-card-icon">
            <Tv size={20} strokeWidth={2} />
          </div>
          <div className="stream-card-name-group">
            <div className="stream-name-badge-row">
              <h3>{session.sourceName}</h3>
              {session.deviceName && <p>{session.deviceName}</p>}
            </div>
            <span className="stream-card-target">
              {interpolate(t.host.connectedDevice, { addr: session.viewerAddr })}
            </span>
          </div>
        </div>

        <div className="stream-card-action">
          {inputRequested && !session.inputEnabled && (
            <span className="input-request-badge" title={t.host.remoteInputApprovalHint}>
              {t.host.remoteInputRequestedBadge}
            </span>
          )}
          <button
            className={controlToggleVariants({ active: session.inputEnabled })}
            disabled={(!inputPermission && !session.inputEnabled) || session.state !== "running" || inputBusy}
            onClick={() => void onToggleInput(session)}
            title={session.inputEnabled ? t.host.remoteInputAllowed : t.host.remoteInputApprovalHint}
          >
            {remoteInputLabel(inputBusy, session.inputEnabled, t)}
          </button>
          <button
            className={buttonVariants({ variant: "stop" })}
            disabled={inputBusy || qualityBusy}
            onClick={() => onForceStop(session)}
            title={t.host.stopThisStream}
            aria-label={`${session.sourceName} ${t.host.stopShare}`}
          >
            <Square size={12} fill="currentColor" />
            {t.host.stopShare}
          </button>
        </div>
      </div>

      <div className="stream-card-metrics-grid">
        <div className="metric-card">
          <span className="metric-card-label">{t.host.encoderOutput}</span>
          <span className="metric-card-value font-emerald">{encodeOutputFps} FPS</span>
        </div>

        <div className="metric-card">
          <span className="metric-card-label">{t.host.bitrate}</span>
          <span className="metric-card-value">{bitrateMbps} Mbps</span>
        </div>

        <div className="metric-card">
          <span className="metric-card-label">{t.host.connectionStatus}</span>
          <span className="metric-card-value font-blue">
            {sessionStateLabel(session.state, t)}
          </span>
        </div>

        <div className="metric-card">
          <span className="metric-card-label">{t.host.transferStability}</span>
          <span className="metric-card-value">
            <DroppedFrames dropped={session.dropped} t={t} />
          </span>
        </div>
      </div>

      {showInspector && (
        <SessionInspector
          session={session}
          transportLabel={transportLabel}
          qualitySupported={qualitySupported}
          qualityPercent={qualityPercent}
          qualityBusy={qualityBusy}
          onSetQuality={onSetQuality}
        />
      )}

      <div className="stream-card-footer">
        <div className="stream-termination-policy">
          <Info size={12} />
          <span>{t.host.autoCleanupPolicy}</span>
        </div>
        <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: "var(--text-dim)" }}>
          {sessionStateLabel(session.state, t, true)}
        </span>
      </div>
    </div>
  );
}

export default SessionCard;
