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
interface PendingPairingView {
  offer_id: string;
  device_name: string;
  requested_at: string;
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

function PairingIpBanner({
  lanIp,
  controlPort,
  copiedIp,
  t,
  onCopyIp,
}: {
  lanIp: string;
  controlPort: number;
  copiedIp: boolean;
  t: TranslationSchema;
  onCopyIp: () => void;
}) {
  return (
    <div className="pairing-ip-banner">
      <div className="pairing-ip-info">
        <span className="pairing-ip-label">{t.host.computerAddressLabel}</span>
        <span className="pairing-ip-value">{lanIp}:{controlPort}</span>
      </div>
      <button
        type="button"
        className={buttonVariants({ variant: "ghost", size: "sm" })}
        onClick={onCopyIp}
        title={t.host.computerAddressLabel}
      >
        {copiedIp ? (
          <span style={{ display: "inline-flex", alignItems: "center", gap: 3, fontWeight: 600 }}>
            <Check size={13} /> {t.host.copied}
          </span>
        ) : (
          <span style={{ display: "inline-flex", alignItems: "center", gap: 3 }}>
            <Copy size={13} /> {t.common.myComputer} IP 복사
          </span>
        )}
      </button>
    </div>
  );
}

interface PairingQrCardProps {
  session: ActiveSession | null;
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

function PairingQrCard({
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
          <button onClick={onStartPairing} className={buttonVariants({ variant: "primary", size: "lg" })} disabled={starting}>
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
          <button onClick={onStartPairing} className={buttonVariants({ variant: "primary" })} disabled={starting}>
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
          <span className="code-value">{session.code.replace(/(\d{3})(\d{3})/, "$1 $2")}</span>
          <button
            type="button"
            className="clickable-chip"
            onClick={onCopyCode}
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
        <p style={{ margin: 0, fontSize: 12, opacity: 0.7 }}>{t.host.pairingQrScanHint}</p>
        <div className="countdown-container">
          <div className="countdown-badge" style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
            <Clock size={12} /> {t.host.remainingTime} {formatCountdown(remainingMs)}
          </div>
          <div className="time-decay-track" aria-hidden="true">
            <div className="time-decay-bar" style={{ width: `${progressPercent}%` }} />
          </div>
        </div>
        <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
          <button onClick={onStartPairing} className={buttonVariants({ variant: "ghost", size: "sm" })} title={t.host.pairingNewCode}>
            <RefreshCw size={12} /> {t.host.pairingNewCode}
          </button>
          <button onClick={onCancelPairing} className={buttonVariants({ variant: "ghost", size: "sm" })}>
            {t.host.pairingCancel}
          </button>
        </div>
      </div>
    </div>
  );
}

function PendingApprovalCard({
  requests,
  busyId,
  onApprove,
  onDeny,
  t,
}: {
  requests: PendingPairingView[];
  busyId: string | null;
  onApprove: (offerId: string) => void;
  onDeny: (offerId: string) => void;
  t: TranslationSchema;
}) {
  if (requests.length === 0) return null;
  return (
    <div className="paired-devices-section" role="alert">
      <div className="section-title-row">
        <div className="section-title-left">
          <Smartphone size={15} />
          <h4>{t.host.pairApprovalCardTitle}</h4>
          <span className="count-pill">{requests.length}</span>
        </div>
      </div>
      <p style={{ margin: "4px 0 8px", fontSize: 12, opacity: 0.75 }}>
        {t.host.pairApprovalCardHint}
      </p>
      <div className="device-rows-container">
        {requests.map((request) => (
          <div key={request.offer_id} className="device-row-item">
            <div className="device-row-main">
              <span className="device-row-name" style={{ display: "inline-flex", alignItems: "center", gap: 7 }}>
                <Smartphone size={15} strokeWidth={2} />
                {request.device_name}
              </span>
            </div>
            <div style={{ display: "flex", gap: 6 }}>
              <button
                onClick={() => onApprove(request.offer_id)}
                className={buttonVariants({ variant: "primary", size: "sm" })}
                disabled={busyId !== null}
              >
                <Check size={13} /> {t.host.pairApprovalAllow}
              </button>
              <button
                onClick={() => onDeny(request.offer_id)}
                className="btn-danger-outline btn-sm"
                disabled={busyId !== null}
              >
                {t.host.pairApprovalDeny}
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function PairedDevicesSection({
  devices,
  revoking,
  language,
  t,
  onRevoke,
  onRevokeAll,
}: {
  devices: PairedDevice[];
  revoking: string | null;
  language: SupportedLanguage;
  t: TranslationSchema;
  onRevoke: (id: string) => void;
  onRevokeAll: () => void;
}) {
  return (
    <div className="paired-devices-section">
      <div className="section-title-row">
        <div className="section-title-left">
          <ShieldCheck size={15} />
          <h4>{t.host.pairedDevicesSection}</h4>
          <span className="count-pill">{devices.length}</span>
        </div>
        {devices.length > 0 && (
          <button
            onClick={onRevokeAll}
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
                onClick={() => onRevoke(device.device_id)}
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
  );
}

export default function PairingPanel({ language: propLanguage }: { language?: SupportedLanguage }) {
  const language = propLanguage || (localStorage.getItem("leftcar_lang") as SupportedLanguage) || "ko";
  const t = getTranslation(language);

  const [session, setSession] = useState<ActiveSession | null>(null);
  const [starting, setStarting] = useState(false);
  const [devices, setDevices] = useState<PairedDevice[]>([]);
  const [pendingRequests, setPendingRequests] = useState<PendingPairingView[]>([]);
  const [decidingId, setDecidingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [copiedCode, setCopiedCode] = useState(false);
  const [copiedIp, setCopiedIp] = useState(false);
  const [lanIp, setLanIp] = useState<string | null>(null);
  const [controlPort, setControlPort] = useState(7777);
  const [now, setNow] = useState(Date.now());
  const deviceCountRef = useRef(devices.length);

  useEffect(() => {
    void invoke<string | null>("get_lan_ip").then(setLanIp).catch(() => {});
    void invoke<number>("get_control_port").then(setControlPort).catch(() => {});
  }, []);

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

  const copyIp = useCallback(() => {
    if (!lanIp) return;
    void navigator.clipboard.writeText(`${lanIp}:${controlPort}`);
    setCopiedIp(true);
    setTimeout(() => setCopiedIp(false), 2000);
  }, [lanIp, controlPort]);

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

  const refreshPending = useCallback(async () => {
    try {
      setPendingRequests(await invoke<PendingPairingView[]>("list_pending_pairings"));
    } catch {
      // best effort
    }
  }, []);

  const approveRequest = useCallback(async (offerId: string) => {
    setDecidingId(offerId);
    try {
      await invoke("approve_pending_pairing", { offerId });
      setPendingRequests((current) => current.filter((request) => request.offer_id !== offerId));
    } catch {
      // 다음 폴링이 목록을 정리한다
    } finally {
      setDecidingId(null);
    }
  }, []);

  const denyRequest = useCallback(async (offerId: string) => {
    setDecidingId(offerId);
    try {
      await invoke("reject_pending_pairing", { offerId });
      setPendingRequests((current) => current.filter((request) => request.offer_id !== offerId));
    } catch {
      // 다음 폴링이 목록을 정리한다
    } finally {
      setDecidingId(null);
    }
  }, []);

  useEffect(() => {
    refreshDevices();
    const interval = setInterval(refreshDevices, 2000);
    return () => clearInterval(interval);
  }, [refreshDevices]);

  useEffect(() => {
    refreshPending();
    const interval = setInterval(refreshPending, 1500);
    return () => clearInterval(interval);
  }, [refreshPending]);

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

      {lanIp && (
        <PairingIpBanner
          lanIp={lanIp}
          controlPort={controlPort}
          copiedIp={copiedIp}
          t={t}
          onCopyIp={copyIp}
        />
      )}

      {error && (
        <div className={bannerAlertVariants({ tone: "danger" })}>
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <AlertTriangle size={15} /> {error}
          </span>
        </div>
      )}

      <PendingApprovalCard
        requests={pendingRequests}
        busyId={decidingId}
        onApprove={(offerId) => void approveRequest(offerId)}
        onDeny={(offerId) => void denyRequest(offerId)}
        t={t}
      />

      <PairingQrCard
        session={session}
        expired={expired}
        starting={starting}
        remainingMs={remainingMs}
        progressPercent={progressPercent}
        copiedCode={copiedCode}
        t={t}
        onStartPairing={startPairing}
        onCancelPairing={cancelPairing}
        onCopyCode={copyCode}
      />

      <PairedDevicesSection
        devices={devices}
        revoking={revoking}
        language={language}
        t={t}
        onRevoke={revoke}
        onRevokeAll={revokeAll}
      />
    </div>
  );
}
