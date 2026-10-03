import { Check, Clock, Copy, KeyRound, QrCode, RefreshCw } from "lucide-react";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import { Button, Surface, Text } from "../ui/primitives";
export interface ActivePairingSession {
  offerId: string;
  qrDataUrl: string;
  code: string;
  expiresAt: number;
  durationMs: number;
}
function formatCountdown(remainingMs: number): string {
  const total = Math.max(0, Math.ceil(remainingMs / 1000));
  return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}
interface PairingQrCardProps {
  session: ActivePairingSession | null;
  expired: boolean;
  starting: boolean;
  cancelling?: boolean;
  remainingMs: number;
  progressPercent: number;
  copiedCode: boolean;
  t: TranslationSchema;
  onStartPairing: () => void;
  onCancelPairing: () => void;
  onCopyCode: () => void;
}
function PairingIdleCard({
  expired,
  starting,
  cancelling = false,
  t,
  onStartPairing,
}: Pick<
  PairingQrCardProps,
  "expired" | "starting" | "cancelling" | "t" | "onStartPairing"
>) {
  const Icon = expired ? Clock : QrCode;
  return (
    <Surface
      variant="card"
      className="flex flex-col items-center gap-3 py-6 text-center"
    >
      <Icon size={24} className="text-muted" />
      <Text variant="title">
        {expired ? t.host.pairingExpiredTitle : t.host.btnPair}
      </Text>
      <Text tone="muted" className="max-w-sm">
        {expired ? t.host.pairingExpiredSub : t.host.idleHint}
      </Text>
      <Button busy={starting} disabled={cancelling} onClick={onStartPairing}>
        {expired ? <RefreshCw size={16} /> : <KeyRound size={16} />}
        {starting
          ? t.host.pairingStarting
          : expired
            ? t.host.regenerateCode
            : t.host.btnCreatePairing}
      </Button>
    </Surface>
  );
}
function ActivePairingCard({
  session,
  starting,
  cancelling = false,
  remainingMs,
  progressPercent,
  copiedCode,
  t,
  onStartPairing,
  onCancelPairing,
  onCopyCode,
}: Omit<PairingQrCardProps, "session" | "expired"> & {
  session: ActivePairingSession;
}) {
  return (
    <Surface
      variant="card"
      className="flex flex-col items-center gap-3 text-center"
    >
      <div className="rounded-md border border-outline bg-qr p-2">
        <img
          src={session.qrDataUrl}
          alt={t.host.pairingQrScanHint}
          width={190}
          height={190}
          className="block"
        />
      </div>
      <div className="flex max-w-full flex-wrap items-center justify-center gap-2 rounded-md border border-outline bg-subtle px-3 py-1">
        <Text variant="caption" tone="muted">
          {t.host.pairingCodeLabel}
        </Text>
        <Text
          variant="heading"
          className="font-mono tracking-widest tabular-nums"
          aria-label={`${t.host.pairingCodeLabel}: ${session.code.split("").join(" ")}`}
        >
          {session.code.replace(/(\d{3})(\d{3})/, "$1 $2")}
        </Text>
        <Button
          variant="ghost"
          size="icon"
          onClick={onCopyCode}
          aria-label={t.host.copyCode}
        >
          {copiedCode ? <Check size={16} /> : <Copy size={16} />}
        </Button>
        {copiedCode && (
          <Text variant="caption" role="status">
            {t.host.copied}
          </Text>
        )}
      </div>
      <Text variant="caption" tone="muted">
        {t.host.pairingQrScanHint}
      </Text>
      <div className="w-full max-w-56 space-y-2">
        <Text
          variant="code"
          tone="muted"
          className="flex items-center justify-center gap-1"
        >
          <Clock size={16} />
          {t.host.remainingTime} {formatCountdown(remainingMs)}
        </Text>
        <div
          className="h-1 overflow-hidden rounded-sm bg-subtle"
          aria-hidden="true"
        >
          <div
            className="h-full bg-ink motion-reduce:transition-none"
            style={{ width: `${progressPercent}%` }}
          />
        </div>
      </div>
      <div className="flex flex-wrap justify-center gap-2">
        <Button
          variant="secondary"
          size="compact"
          busy={starting}
          disabled={cancelling}
          onClick={onStartPairing}
        >
          <RefreshCw size={16} />
          {starting ? t.host.pairingStarting : t.host.pairingNewCode}
        </Button>
        <Button
          variant="ghost"
          size="compact"
          busy={cancelling}
          disabled={starting}
          onClick={onCancelPairing}
        >
          {cancelling ? t.host.remoteInputProcessing : t.host.pairingCancel}
        </Button>
      </div>
    </Surface>
  );
}
export default function PairingQrCard(props: PairingQrCardProps) {
  if (!props.session || props.expired) return <PairingIdleCard {...props} />;
  return <ActivePairingCard {...props} session={props.session} />;
}
