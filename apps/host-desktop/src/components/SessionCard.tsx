import { Square } from "lucide-react";
import { interpolate, type TranslationSchema } from "@leftcar/ui-tokens";
import type { SessionRow } from "../sessionTypes";
import QualityOverride from "../QualityOverride";
import SessionInspector from "../SessionInspector";
import { Button, Notice, Text } from "../ui/primitives";
export interface SessionCardProps {
  session: SessionRow;
  inputPermission: boolean;
  inputBusy: boolean;
  showInspector: boolean;
  inputRequested?: boolean;
  t: TranslationSchema;
  onToggleInput: (session: SessionRow) => Promise<void>;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
  qualityBusy: boolean;
  stopping?: boolean;
  actionError?: string;
  onRetry?: () => void;
  onForceStop: (session: SessionRow) => void;
}
function sessionTransportLabel(session: SessionRow, t: TranslationSchema) {
  const labels: Record<string, string> = {
    usb: t.host.cableUsb,
    udp: t.host.wifiWireless,
  };
  return session.mediaTransport
    ? (labels[session.mediaTransport] ?? session.mediaTransport)
    : t.host.unknownTransport;
}
function SessionIdentity({
  session,
  t,
}: Pick<SessionCardProps, "session" | "t">) {
  return (
    <div className="min-w-0 flex-1 space-y-1">
      <h3 className="text-title text-ink break-words font-semibold">
        {session.sourceName}
      </h3>
      {session.deviceName && (
        <Text tone="muted" className="block break-words">
          {session.deviceName}
        </Text>
      )}
      <Text variant="code" tone="muted" className="block break-all">
        {interpolate(t.host.connectedDevice, { addr: session.viewerAddr })}
      </Text>
    </div>
  );
}

function SessionActions({
  session,
  inputPermission,
  inputBusy,
  inputRequested,
  t,
  onToggleInput,
  onForceStop,
  busy,
}: Pick<
  SessionCardProps,
  | "session"
  | "inputPermission"
  | "inputBusy"
  | "inputRequested"
  | "t"
  | "onToggleInput"
  | "onForceStop"
> & { busy: boolean }) {
  const inputLabel = inputBusy
    ? t.host.remoteInputProcessing
    : session.inputEnabled
      ? t.host.remoteInputAllowed
      : t.host.remoteInputOff;
  return (
    <div className="flex flex-wrap items-center gap-2">
      {inputRequested && !session.inputEnabled && (
        <Text variant="caption" role="status" className="font-semibold">
          {t.host.remoteInputRequestedBadge}
        </Text>
      )}
      <Button
        variant={session.inputEnabled ? "primary" : "secondary"}
        size="compact"
        aria-pressed={session.inputEnabled}
        busy={inputBusy}
        disabled={
          (!inputPermission && !session.inputEnabled) ||
          session.state !== "running" ||
          busy
        }
        onClick={() => void onToggleInput(session)}
        title={
          session.inputEnabled
            ? t.host.remoteInputAllowed
            : t.host.remoteInputApprovalHint
        }
      >
        {inputLabel}
      </Button>
      <Button
        variant="secondary"
        size="compact"
        disabled={busy}
        onClick={() => onForceStop(session)}
        aria-label={`${session.sourceName}: ${t.host.stopShare}`}
      >
        <Square size={16} fill="currentColor" />
        {t.host.stopShare}
      </Button>
    </div>
  );
}

function SessionActionError({
  error,
  onRetry,
  busy,
  t,
}: {
  error?: string;
  onRetry?: () => void;
  busy: boolean;
  t: TranslationSchema;
}) {
  if (!error) return null;
  return (
    <Notice
      tone="error"
      className="flex flex-wrap items-center justify-between gap-2"
    >
      <Text>{error}</Text>
      {onRetry && (
        <Button
          variant="secondary"
          size="compact"
          disabled={busy}
          onClick={onRetry}
        >
          {t.common.retry}
        </Button>
      )}
    </Notice>
  );
}

function SessionDetails({
  session,
  busy,
  qualityBusy,
  t,
  onSetQuality,
}: Pick<SessionCardProps, "session" | "t" | "onSetQuality" | "qualityBusy"> & {
  busy: boolean;
}) {
  const qualitySupported = session.qualityHint != null;
  const qualityPercent = Math.round(
    (session.qualityOverride ?? session.qualityHint ?? 0.5) * 100,
  );
  return (
    <div className="space-y-4 border-t border-outline pt-4">
      {qualitySupported && (
        <QualityOverride
          session={session}
          qualitySupported={qualitySupported}
          qualityPercent={qualityPercent}
          qualityBusy={qualityBusy}
          blocked={busy}
          onSetQuality={onSetQuality}
        />
      )}
      <SessionInspector
        session={session}
        transportLabel={sessionTransportLabel(session, t)}
      />
    </div>
  );
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
  stopping = false,
  actionError,
  onRetry,
  onForceStop,
}: SessionCardProps) {
  const busy = inputBusy || qualityBusy || stopping;
  return (
    <article
      className="space-y-3 border-t border-outline py-4 first:border-0"
      aria-label={session.sourceName}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <SessionIdentity session={session} t={t} />
        <SessionActions
          session={session}
          inputPermission={inputPermission}
          inputBusy={inputBusy}
          inputRequested={inputRequested}
          t={t}
          onToggleInput={onToggleInput}
          onForceStop={onForceStop}
          busy={busy}
        />
      </div>
      {!inputPermission && !session.inputEnabled && (
        <Text variant="caption" tone="muted" className="block">
          {t.host.inputOptionalHint}
        </Text>
      )}
      {session.state !== "running" && <Notice>{t.host.statusChecking}</Notice>}
      <SessionActionError
        error={actionError}
        onRetry={onRetry}
        busy={busy}
        t={t}
      />
      {showInspector && (
        <SessionDetails
          session={session}
          qualityBusy={qualityBusy}
          busy={busy}
          t={t}
          onSetQuality={onSetQuality}
        />
      )}
    </article>
  );
}
export default SessionCard;
