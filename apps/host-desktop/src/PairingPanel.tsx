import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import QRCode from "qrcode";
import {
  AlertTriangle,
  Check,
  Clock,
  Copy,
  KeyRound,
  QrCode,
  RefreshCw,
  ShieldCheck,
  Smartphone,
  Laptop,
} from "lucide-react";
import { bannerAlertVariants, buttonVariants } from "./lib/variants";
import {
  getTranslation,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";

interface PairingSessionView {
  qr_payload: string;
  code: string;
  expires_in_secs: number;
}
interface PairedDevice {
  device_id: string;
  name: string;
  paired_at: string;
}

interface ActiveSession {
  qrDataUrl: string;
  code: string;
  expiresAt: number;
}

function formatPairedAt(pairedAt: string, language: SupportedLanguage): string {
  const secs = Number(pairedAt.replace(/^unix:/, ""));
  if (!Number.isFinite(secs) || secs <= 0) return pairedAt;
  return new Date(secs * 1000).toLocaleString(language === "ko" ? "ko-KR" : "en-US", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatCountdown(remainingMs: number): string {
  const total = Math.max(0, Math.ceil(remainingMs / 1000));
  const mm = String(Math.floor(total / 60)).padStart(2, "0");
  const ss = String(total % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

function connectionErrorMessage(cause: unknown, t: TranslationSchema): string {
  const message = String(cause instanceof Error ? cause.message : cause).toLowerCase();
  if (message.includes("no lan interface")) {
    return t.host.networkNotFoundError;
  }
  if (message.includes("persistence")) {
    return t.host.appServiceInitError;
  }
  return t.host.connectionCheckError;
}

export default function PairingPanel({ language: propLanguage }: { language?: SupportedLanguage }) {
  const language = propLanguage || (localStorage.getItem("leftcar_lang") as SupportedLanguage) || "ko";
  const t = getTranslation(language);

  const [session, setSession] = useState<ActiveSession | null>(null);
  const [starting, setStarting] = useState(false);
  const [devices, setDevices] = useState<PairedDevice[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [copiedCode, setCopiedCode] = useState(false);
  const [now, setNow] = useState(Date.now());
  const deviceCountRef = useRef(devices.length);

  useEffect(() => {
    deviceCountRef.current = devices.length;
  }, [devices.length]);

  const refreshDevices = useCallback(async () => {
    try {
      const list = await invoke<PairedDevice[]>("list_paired_devices");
      if (list.length > deviceCountRef.current) {
        setSession(null);
      }
      setDevices(list);
    } catch {
      // best effort
    }
  }, []);

  const startPairing = useCallback(async () => {
    setStarting(true);
    setError(null);
    try {
      const view = await invoke<PairingSessionView>("begin_pairing");
      const qrDataUrl = await QRCode.toDataURL(view.qr_payload, {
        width: 220,
        margin: 1,
        color: {
          dark: "#09090b",
          light: "#ffffff",
        },
      });
      setSession({
        qrDataUrl,
        code: view.code,
        expiresAt: Date.now() + view.expires_in_secs * 1000,
      });
      refreshDevices();
    } catch (e) {
      setError(connectionErrorMessage(e, t));
    } finally {
      setStarting(false);
    }
  }, [refreshDevices, t]);

  const cancelPairing = useCallback(async () => {
    try {
      await invoke("cancel_pairing");
    } catch {
      // ignore
    }
    setSession(null);
  }, []);

  const copyCode = useCallback(() => {
    if (!session) return;
    void navigator.clipboard.writeText(session.code);
    setCopiedCode(true);
    setTimeout(() => setCopiedCode(false), 2000);
  }, [session]);

  const revoke = useCallback(
    async (deviceId: string) => {
      setRevoking(deviceId);
      setError(null);
      try {
        await invoke("revoke_paired_device", { deviceId });
        await refreshDevices();
      } catch (e) {
        setError(connectionErrorMessage(e, t));
      } finally {
        setRevoking(null);
      }
    },
    [refreshDevices, t],
  );

  const revokeAll = useCallback(async () => {
    setRevoking("all");
    setError(null);
    try {
      await invoke("revoke_all_devices");
      await refreshDevices();
    } catch (e) {
      setError(connectionErrorMessage(e, t));
    } finally {
      setRevoking(null);
    }
  }, [refreshDevices, t]);

  useEffect(() => {
    refreshDevices();
    const interval = setInterval(refreshDevices, 2000);
    return () => clearInterval(interval);
  }, [refreshDevices]);

  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, []);

  useEffect(
    () => () => {
      // Closing this screen invalidates any QR that may still be visible in a
      // screenshot or camera preview.
      void invoke("cancel_pairing");
    },
    [],
  );

  const expired = session !== null && now >= session.expiresAt;
  const remainingMs = session ? Math.max(0, session.expiresAt - now) : 0;
  const progressPercent = session ? Math.max(0, Math.min(100, (remainingMs / (120 * 1000)) * 100)) : 0;

  useEffect(() => {
    if (session && expired) {
      invoke("cancel_pairing").catch(() => {});
    }
  }, [session, expired]);

  return (
    <div className="pairing-wrapper">
      <div className="pairing-guide">
        <p className="pairing-guide-title">{t.host.pairingModalTitle}</p>
        <p className="pairing-guide-sub">
          {t.host.pairingPanelGuide}
        </p>
      </div>

      {error && (
        <div className={bannerAlertVariants({ tone: "danger" })}>
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <AlertTriangle size={15} /> {error}
          </span>
        </div>
      )}

      <div className="pairing-qr-card">
        {!session ? (
          <div className="pairing-idle-state">
            <div className="idle-icon-box">
              <QrCode size={22} strokeWidth={2} />
            </div>
            <p className="idle-title">{t.host.btnPair}</p>
            <p className="idle-sub">{t.host.idleHint}</p>
            <button onClick={startPairing} className={buttonVariants({ variant: "primary", size: "lg" })} disabled={starting}>
              <KeyRound size={15} />
              {starting ? "…" : t.host.btnCreatePairing}
            </button>
          </div>
        ) : expired ? (
          <div className="pairing-idle-state">
            <div className="idle-icon-box">
              <Clock size={22} strokeWidth={2} />
            </div>
            <p className="idle-title">{t.host.pairingExpiredTitle}</p>
            <p className="idle-sub">{t.host.pairingExpiredSub}</p>
            <button onClick={startPairing} className={buttonVariants({ variant: "primary" })} disabled={starting}>
              <RefreshCw size={14} />
              {starting ? "…" : t.host.regenerateCode}
            </button>
          </div>
        ) : (
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
              <span className="code-value">{session.code.replace(/(\d{3})(\d{3})/, "$1 $2")}</span>
              <button
                type="button"
                className="clickable-chip"
                onClick={copyCode}
                title={t.host.pairingCodeLabel}
                style={{ marginLeft: 4 }}
              >
                {copiedCode ? (
                  <span style={{ display: "inline-flex", alignItems: "center", gap: 3, fontSize: 11, fontWeight: 600 }}>
                    <Check size={13} /> {t.host.copied}
                  </span>
                ) : (
                  <Copy size={13} style={{ opacity: 0.8 }} />
                )}
              </button>
            </div>
            <div className="countdown-container">
              <div className="countdown-badge" style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
                <Clock size={12} /> {t.host.remainingTime} {formatCountdown(remainingMs)}
              </div>
              <div className="time-decay-track" aria-hidden="true">
                <div className="time-decay-bar" style={{ width: `${progressPercent}%` }} />
              </div>
            </div>
            <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
              <button onClick={startPairing} className={buttonVariants({ variant: "ghost", size: "sm" })} title={t.host.pairingNewCode}>
                <RefreshCw size={12} /> {t.host.pairingNewCode}
              </button>
              <button onClick={cancelPairing} className={buttonVariants({ variant: "ghost", size: "sm" })}>
                {t.host.pairingCancel}
              </button>
            </div>
          </div>
        )}
      </div>

      <div className="paired-devices-section">
        <div className="section-title-row">
          <div className="section-title-left">
            <ShieldCheck size={15} />
            <h4>{t.host.pairedDevicesSection}</h4>
            <span className="count-pill">{devices.length}</span>
          </div>
          {devices.length > 0 && (
            <button
              onClick={revokeAll}
              className="btn-danger-outline btn-sm"
              disabled={revoking !== null}
            >
              {t.host.revokeAll}
            </button>
          )}
        </div>

        {devices.length > 0 ? (
          <div className="device-rows-container">
            {devices.map((device) => (
              <div key={device.device_id} className="device-row-item">
                <div className="device-row-main">
                  <span className="device-row-name" style={{ display: "inline-flex", alignItems: "center", gap: 7 }}>
                    {device.name.toLowerCase().includes("pc") || device.name.toLowerCase().includes("mac") ? (
                      <Laptop size={15} strokeWidth={2} />
                    ) : (
                      <Smartphone size={15} strokeWidth={2} />
                    )}
                    {device.name}
                  </span>
                  <span className="device-row-date">{formatPairedAt(device.paired_at, language)}</span>
                </div>
                <button
                  onClick={() => revoke(device.device_id)}
                  className="btn-danger-outline"
                  disabled={revoking === device.device_id}
                  title={t.host.revoke}
                >
                  {t.host.revoke}
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div className="empty-devices-box">
            <p>{t.host.noPairedDevices}</p>
          </div>
        )}
      </div>
    </div>
  );
}
