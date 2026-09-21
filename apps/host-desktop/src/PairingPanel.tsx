import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import QRCode from "qrcode";
import {
  getPairedDeviceState,
  confirmSourceGrants,
  receivePairedDeviceState,
  receiveRevokeOutcome,
  subscribePairedDeviceState,
  type PairedDevice,
  type PairedDeviceState,
  type RevokeOutcome,
  type SourceGrantView,
} from "./paired-device-state";
import {
  AlertTriangle,
  Check,
  Copy,
  ShieldCheck,
} from "lucide-react";
import { bannerAlertVariants, buttonVariants } from "./lib/variants";
import { formatHostAddress } from "./hostState";
import RevokeConfirmDialog, { type RevokeConfirm } from "./RevokeConfirmDialog";
import {
  getTranslation,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import PairingQrCard, { type ActivePairingSession } from "./pairing/PairingQrCard";
import PendingApprovalSection, { type PendingPairingView } from "./pairing/PendingApprovalSection";
import PairedDeviceRow from "./pairing/PairedDeviceRow";

interface PairingSessionView {
  qr_payload: string;
  code: string;
  expires_in_secs: number;
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

function PairingNetworkCard({
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
  const [showAdvanced, setShowAdvanced] = useState(false);

  return (
    <div className="pairing-network-card">
      <div className="pairing-network-status-row">
        <div className="pairing-network-status-indicator">
          <span className="pairing-status-dot" />
          <strong style={{ fontSize: 13, color: "var(--text-primary)" }}>
            {t.host.remoteReady}
          </strong>
        </div>
        <button
          type="button"
          onClick={() => setShowAdvanced((prev) => !prev)}
          style={{
            background: "none",
            border: "none",
            fontSize: 12,
            color: "var(--text-secondary)",
            cursor: "pointer",
            display: "inline-flex",
            alignItems: "center",
            gap: 4,
            padding: "2px 6px",
          }}
        >
          {t.host.advancedNetworkToggle}
          <span style={{ transform: showAdvanced ? "rotate(180deg)" : "none", transition: "transform 0.2s" }}>
            ⌄
          </span>
        </button>
      </div>

      <p className="pairing-network-card-desc">
        {t.host.remoteReadyDesc}
      </p>

      {showAdvanced && (
        <div className="pairing-advanced-details">
          <span style={{ color: "var(--text-secondary)" }}>
            {t.host.localLanAddress}:{" "}
            <code style={{ color: "var(--text-primary)", fontWeight: 600 }}>
              {lanIp}:{controlPort}
            </code>
          </span>
          <button
            type="button"
            className={buttonVariants({ variant: "ghost", size: "sm" })}
            onClick={onCopyIp}
            title={t.host.computerAddressLabel}
          >
            {copiedIp ? (
              <span style={{ display: "inline-flex", alignItems: "center", gap: 4, fontWeight: 600 }}>
                <Check size={13} /> {t.host.copied}
              </span>
            ) : (
              <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                <Copy size={13} /> {t.host.copyAddress}
              </span>
            )}
          </button>
        </div>
      )}
    </div>
  );
}

function PairedDevicesSection({
  devices,
  revoking,
  approvingScreens,
  language,
  t,
  onRevoke,
  onRevokeAll,
  onApproveScreens,
}: {
  devices: PairedDevice[];
  revoking: string | null;
  approvingScreens: string | null;
  language: SupportedLanguage;
  t: TranslationSchema;
  onRevoke: (id: string) => void;
  onRevokeAll: () => void;
  onApproveScreens: (device: PairedDevice) => void;
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
            type="button"
            onClick={onRevokeAll}
            className={buttonVariants({ variant: "outlineDanger", size: "sm" })}
            disabled={revoking !== null}
          >
            {t.host.revokeAll}
          </button>
        )}
      </div>

      {devices.length > 0 ? (
        <div className="device-rows-container">
          {devices.map((device) => (
            <PairedDeviceRow
              key={device.source_grants.credentialId}
              device={device}
              language={language}
              t={t}
              revoking={revoking}
              approvingScreens={approvingScreens}
              onRevoke={onRevoke}
              onApproveScreens={onApproveScreens}
            />
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

  const [session, setSession] = useState<ActivePairingSession | null>(null);
  const [starting, setStarting] = useState(false);
  const pairedState = useSyncExternalStore(subscribePairedDeviceState, getPairedDeviceState);
  const devices = pairedState.devices;
  const [pendingRequests, setPendingRequests] = useState<PendingPairingView[]>([]);
  const [decidingId, setDecidingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [approvingScreens, setApprovingScreens] = useState<string | null>(null);
  const [revokeConfirm, setRevokeConfirm] = useState<RevokeConfirm | null>(null);
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
      const snapshot = await invoke<PairedDeviceState>("list_paired_device_state");
      if (snapshot.devices.length > deviceCountRef.current) {
        setSession(null);
      }
      receivePairedDeviceState(snapshot);
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
    navigator.clipboard.writeText(session.code).then(
      () => {
        setCopiedCode(true);
        setTimeout(() => setCopiedCode(false), 2000);
      },
      () => setError(t.host.copyFailed),
    );
  }, [session, t]);

  const copyIp = useCallback(() => {
    if (!lanIp) return;
    navigator.clipboard.writeText(formatHostAddress(lanIp, controlPort)).then(
      () => {
        setCopiedIp(true);
        setTimeout(() => setCopiedIp(false), 2000);
      },
      () => setError(t.host.copyFailed),
    );
  }, [lanIp, controlPort, t]);

  const revoke = useCallback(
    async (deviceId: string) => {
      setRevoking(deviceId);
      setError(null);
      try {
        receiveRevokeOutcome(await invoke<RevokeOutcome>("revoke_paired_device", { deviceId }));
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
      receiveRevokeOutcome(await invoke<RevokeOutcome>("revoke_all_devices"));
      await refreshDevices();
    } catch (e) {
      setError(connectionErrorMessage(e, t));
    } finally {
      setRevoking(null);
    }
  }, [refreshDevices, t]);

  // 승인 때 화면 목록이 비어 있었던 기기(화면기록 권한 전 페어링 등)를 위한
  // 수동 화면 허용 — "승인 대기"가 뜬 그 자리에서 결정한다.
  const approveScreens = useCallback(
    async (device: PairedDevice) => {
      setApprovingScreens(device.device_id);
      setError(null);
      try {
        const displays = await invoke<{ sourceId: string | null }[]>("list_host_sources");
        const sourceIds = displays
          .map((display) => display.sourceId)
          .filter((sourceId): sourceId is string => sourceId !== null);
        const grants = await invoke<SourceGrantView>("set_source_grants", {
          deviceId: device.device_id,
          sourceIds,
          credentialId: device.source_grants.credentialId,
        });
        confirmSourceGrants(device.device_id, grants);
        await refreshDevices();
      } catch (e) {
        setError(connectionErrorMessage(e, t));
      } finally {
        setApprovingScreens(null);
      }
    },
    [refreshDevices, t],
  );

  const refreshPending = useCallback(async () => {
    try {
      setPendingRequests(await invoke<PendingPairingView[]>("list_pending_pairings"));
    } catch {
      // best effort
    }
  }, []);

  const decideRequest = useCallback(
    async (command: "approve_pending_pairing" | "reject_pending_pairing", offerId: string) => {
      setDecidingId(offerId);
      try {
        await invoke(command, { offerId });
        setPendingRequests((current) => current.filter((request) => request.offer_id !== offerId));
      } catch (e) {
        setError(connectionErrorMessage(e, t));
      } finally {
        setDecidingId(null);
      }
    },
    [t],
  );

  const approveRequest = useCallback(
    (offerId: string) => decideRequest("approve_pending_pairing", offerId),
    [decideRequest],
  );

  const denyRequest = useCallback(
    (offerId: string) => decideRequest("reject_pending_pairing", offerId),
    [decideRequest],
  );

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
        <p className="pairing-guide-sub">{t.host.pairingPanelGuide}</p>
      </div>

      {lanIp && (
        <PairingNetworkCard
          lanIp={lanIp}
          controlPort={controlPort}
          copiedIp={copiedIp}
          t={t}
          onCopyIp={copyIp}
        />
      )}

      {(error || pairedState.administrativeError) && (
        <div role="alert" className={bannerAlertVariants({ tone: "danger" })}>
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <AlertTriangle size={15} /> {[error, pairedState.administrativeError].filter(Boolean).join(" · ")}
          </span>
        </div>
      )}

      {revokeConfirm && (
        <RevokeConfirmDialog
          confirm={revokeConfirm}
          revoking={revoking}
          t={t}
          onCancel={() => setRevokeConfirm(null)}
          onConfirm={() => {
            const deviceId = revokeConfirm.deviceId;
            setRevokeConfirm(null);
            if (deviceId) void revoke(deviceId);
            else void revokeAll();
          }}
        />
      )}

      <PendingApprovalSection
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
        approvingScreens={approvingScreens}
        language={language}
        t={t}
        onRevoke={(deviceId) =>
          setRevokeConfirm({
            deviceId,
            name: devices.find((device) => device.device_id === deviceId)?.name ?? null,
          })
        }
        onRevokeAll={() => setRevokeConfirm({ deviceId: null, name: null })}
        onApproveScreens={(device) => void approveScreens(device)}
      />
    </div>
  );
}
