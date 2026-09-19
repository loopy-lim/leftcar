import { Check, Clock, Copy, KeyRound, QrCode, RefreshCw } from "lucide-react";
import { buttonVariants } from "../lib/variants";
import type { TranslationSchema } from "@leftcar/ui-tokens";

export interface ActivePairingSession {
  qrDataUrl: string;
  code: string;
  expiresAt: number;
}

function formatCountdown(remainingMs: number): string {
  const total = Math.max(0, Math.ceil(remainingMs / 1000));
  const mm = String(Math.floor(total / 60)).padStart(2, "0");
  const ss = String(total % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

interface PairingQrCardProps {
  session: ActivePairingSession | null;
  expired: boolean;
  starting: boolean;
  remainingMs: number;
  progressPercent: number;
  copiedCode: boolean;
  t: TranslationSchema;
  onStartPairing: () => void;
  onCancelPairing: () => void;
  onCopyCode: () => void;
}

export default function PairingQrCard({
  session,
  expired,
  starting,
  remainingMs,
  progressPercent,
  copiedCode,
  t,
  onStartPairing,
  onCancelPairing,
  onCopyCode,
}: PairingQrCardProps) {
  if (!session) {
    return (
      <div className="pairing-qr-card">
        <div className="pairing-idle-state">
          <div className="idle-icon-box">
            <QrCode size={22} strokeWidth={2} />
          </div>
          <p className="idle-title">{t.host.btnPair}</p>
          <p className="idle-sub">{t.host.idleHint}</p>
          <button
            onClick={onStartPairing}
            className={buttonVariants({ variant: "primary", size: "lg" })}
            disabled={starting}
          >
            <KeyRound size={15} />
            {starting ? "…" : t.host.btnCreatePairing}
          </button>
        </div>
      </div>
    );
  }

  if (expired) {
    return (
      <div className="pairing-qr-card">
        <div className="pairing-idle-state">
          <div className="idle-icon-box">
            <Clock size={22} strokeWidth={2} />
          </div>
          <p className="idle-title">{t.host.pairingExpiredTitle}</p>
          <p className="idle-sub">{t.host.pairingExpiredSub}</p>
          <button
            onClick={onStartPairing}
            className={buttonVariants({ variant: "primary" })}
            disabled={starting}
          >
            <RefreshCw size={14} />
            {starting ? "…" : t.host.regenerateCode}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="pairing-qr-card">
      <div className="pairing-active-state">
        <div className="qr-image-frame">
          <img
            src={session.qrDataUrl}
            alt="QR Code"
            width={190}
            height={190}
            className="qr-img"
          />
        </div>
        <div className="code-display-box">
          <span className="code-label">{t.host.pairingCodeLabel}</span>
          <span className="code-value">
            {session.code.replace(/(\d{3})(\d{3})/, "$1 $2")}
          </span>
          <button
            type="button"
            className="clickable-chip"
            onClick={onCopyCode}
            title={t.host.pairingCodeLabel}
            style={{ marginLeft: 4 }}
          >
            {copiedCode ? (
              <span
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 3,
                  fontSize: 12,
                  fontWeight: 600,
                }}
              >
                <Check size={13} /> {t.host.copied}
              </span>
            ) : (
              <Copy size={13} style={{ opacity: 0.8 }} />
            )}
          </button>
        </div>
        <p style={{ margin: 0, fontSize: 12, opacity: 0.7 }}>
          {t.host.pairingQrScanHint}
        </p>
        <div className="countdown-container">
          <div
            className="countdown-badge"
            style={{ display: "inline-flex", alignItems: "center", gap: 5 }}
          >
            <Clock size={12} /> {t.host.remainingTime}{" "}
            {formatCountdown(remainingMs)}
          </div>
          <div className="time-decay-track" aria-hidden="true">
            <div
              className="time-decay-bar"
              style={{ width: `${progressPercent}%` }}
            />
          </div>
        </div>
        <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
          <button
            onClick={onStartPairing}
            className={buttonVariants({ variant: "ghost", size: "sm" })}
            title={t.host.pairingNewCode}
          >
            <RefreshCw size={12} /> {t.host.pairingNewCode}
          </button>
          <button
            onClick={onCancelPairing}
            className={buttonVariants({ variant: "ghost", size: "sm" })}
          >
            {t.host.pairingCancel}
          </button>
        </div>
      </div>
    </div>
  );
}
