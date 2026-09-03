import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  AppWindow,
  Check,
  ChevronDown,
  ChevronUp,
  Copy,
  Globe,
  HelpCircle,
  Info,
  Laptop,
  Monitor,
  Moon,
  QrCode,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
  Square,
  Sun,
  Tv,
  Wifi,
  X,
} from "lucide-react";
import {
  getTranslation,
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import { trayStatus, type HostSnapshotView } from "./hostState";
import SessionInspector from "./SessionInspector";
import type { SessionRow } from "./sessionTypes";
import PairingPanel from "./PairingPanel";
import {
  createTerminationNotice,
  isTerminalSession,
  type TerminationNotice,
} from "./streamTermination";
import {
  bannerAlertVariants,
  buttonVariants,
  controlToggleVariants,
  statusPillVariants,
  terminationNoticeVariants,
} from "./lib/variants";

function hostErrorMessage(cause: unknown, t: TranslationSchema): string {
  const message = String(cause instanceof Error ? cause.message : cause).toLowerCase();
  if (message.includes("permission") || message.includes("not authorized")) {
    return t.host.screenPermissionError;
  }
  if (message.includes("no lan interface")) {
    return t.host.networkNotFoundError;
  }
  if (message.includes("invoke") || message.includes("initialization")) {
    return t.host.appServiceInitError;
  }
  return t.host.connectionCheckError;
}
interface StatusView {
  sessions: SessionRow[];
}

type ThemeMode = "system" | "light" | "dark";

export default function App() {
  if (window.location.hash === "#/pairing") {
    return (
      <div className="pairing-standalone-view">
        <PairingPanel />
      </div>
    );
  }

  return <Dashboard />;
}

function useHostStatus(t: TranslationSchema) {
  const [banner, setBanner] = useState("Leftcar");
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [terminationNotice, setTerminationNotice] = useState<TerminationNotice | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [inputPermission, setInputPermission] = useState(false);
  const [platform, setPlatform] = useState<HostSnapshotView["platform"]>("macos");
  const [controlPort, setControlPort] = useState(7777);
  const [lanIp, setLanIp] = useState<string | null>(null);
  const [lastUpdated, setLastUpdated] = useState<Date>(new Date());
  const priorActiveSessions = useRef(new Map<number, SessionRow>());
  const seenTerminations = useRef(new Set<string>());
  const hasStatusSnapshot = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const [status, permission, hostPlatform, actualControlPort, actualLanIp] = await Promise.all([
        invoke<StatusView>("get_status"),
        invoke<boolean>("get_input_permission"),
        invoke<HostSnapshotView["platform"]>("get_host_platform"),
        invoke<number>("get_control_port"),
        invoke<string | null>("get_lan_ip").catch(() => null),
      ]);
      const statusSessions = status.sessions || [];
      const activeSessions = statusSessions.filter((session) => !isTerminalSession(session));
      let nextTerminationNotice: TerminationNotice | null = null;

      for (const terminalSession of statusSessions.filter(isTerminalSession)) {
        const notice = createTerminationNotice(terminalSession);
        if (!seenTerminations.current.has(notice.key)) {
          seenTerminations.current.add(notice.key);
          nextTerminationNotice = notice;
        }
      }

      if (hasStatusSnapshot.current) {
        const reportedSessionIds = new Set(statusSessions.map((session) => session.session));
        for (const priorSession of priorActiveSessions.current.values()) {
          if (reportedSessionIds.has(priorSession.session)) continue;
          const notice = createTerminationNotice({
            ...priorSession,
            state: "stopped",
            error: null,
          });
          if (!seenTerminations.current.has(notice.key)) {
            seenTerminations.current.add(notice.key);
            nextTerminationNotice = notice;
          }
        }
      }

      priorActiveSessions.current = new Map(
        activeSessions.map((session) => [session.session, session]),
      );
      hasStatusSnapshot.current = true;
      setSessions(activeSessions);
      if (nextTerminationNotice) setTerminationNotice(nextTerminationNotice);
      setBanner(
        trayStatus({
          hostId: "local",
          platform: hostPlatform,
          pairingState: "connected",
          pairedDevices: [],
          approvedSources: [],
          activeStreamCount: activeSessions.length,
        } satisfies HostSnapshotView),
      );
      setError(null);
      setInputPermission(permission);
      setPlatform(hostPlatform);
      setControlPort(actualControlPort);
      setLanIp(actualLanIp);
      setLastUpdated(new Date());
    } catch (cause) {
      setError(hostErrorMessage(cause, t));
    }
  }, [t]);

  useEffect(() => {
    const refreshWhenVisible = () => {
      if (!document.hidden) void refresh();
    };
    void refresh();
    const timer = setInterval(refreshWhenVisible, 2_000);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    window.addEventListener("focus", refreshWhenVisible);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
      window.removeEventListener("focus", refreshWhenVisible);
    };
  }, [refresh]);

  const dismissTerminationNotice = useCallback(() => setTerminationNotice(null), []);

  return {
    banner,
    sessions,
    terminationNotice,
    dismissTerminationNotice,
    error,
    inputPermission,
    platform,
    controlPort,
    lanIp,
    lastUpdated,
    refresh,
  };
}

interface DashboardHeaderProps {
  isStreaming: boolean;
  sessionCount: number;
  themeMode: ThemeMode;
  themeLabel: string;
  language: SupportedLanguage;
  t: TranslationSchema;
  onPair: () => void;
  onHelp: () => void;
  onTheme: () => void;
  onToggleLanguage: () => void;
  onRefresh: () => void;
}

function DashboardHeader({
  isStreaming,
  sessionCount,
  themeMode,
  themeLabel,
  language,
  t,
  onPair,
  onHelp,
  onTheme,
  onToggleLanguage,
  onRefresh,
}: DashboardHeaderProps) {
  return (
    <header className="host-header">
      <div className="host-header-left">
        <div className="host-logo-box">
          <Monitor size={17} strokeWidth={2.4} aria-hidden="true" />
        </div>
        <div className="host-title-group">
          <h1>{t.host.headerTitle}</h1>
          <span className="host-version-badge">{t.common.myComputer}</span>
        </div>
      </div>
      <div className="host-header-right">
        <div className={statusPillVariants({ state: isStreaming ? "active" : "idle" })}>
          <span className="status-dot" />
          <span>
            {isStreaming
              ? interpolate(t.host.statusStreaming, { count: sessionCount })
              : t.host.statusIdle}
          </span>
        </div>
        <button
          className={buttonVariants({ variant: "primary" })}
          onClick={onPair}
          title={`${t.host.btnPair} (${t.host.shortcutPair})`}
        >
          <QrCode size={14} />
          <span>{t.host.btnPair}</span>
          <span
            className="kbd-shortcut"
            style={{
              marginLeft: 2,
              opacity: 0.85,
              background: "rgba(255,255,255,0.2)",
              color: "inherit",
              borderColor: "rgba(255,255,255,0.3)",
            }}
          >
            {t.host.shortcutPair}
          </span>
        </button>
        <button
          className={buttonVariants({ variant: "icon" })}
          onClick={onHelp}
          title={`${t.host.btnHelp} (${t.host.shortcutHelp})`}
          aria-label={t.host.btnHelp}
        >
          <HelpCircle size={15} />
        </button>
        <button
          className={buttonVariants({ variant: "icon" })}
          onClick={onToggleLanguage}
          title={t.common.toggleLanguage}
          aria-label={t.common.toggleLanguage}
          style={{ display: "inline-flex", alignItems: "center", gap: 3, padding: "0 6px" }}
        >
          <Globe size={13} />
          <span style={{ fontSize: 11, fontWeight: 700 }}>{language === "ko" ? "EN" : "한국어"}</span>
        </button>
        <button
          className={buttonVariants({ variant: "icon" })}
          onClick={onTheme}
          title={`${t.common.theme}: ${themeLabel}`}
          aria-label={`${t.common.theme}: ${themeLabel}`}
        >
          {themeMode === "light" ? (
            <Sun size={15} />
          ) : themeMode === "dark" ? (
            <Moon size={15} />
          ) : (
            <Laptop size={15} />
          )}
        </button>
        <button
          className={buttonVariants({ variant: "icon" })}
          onClick={onRefresh}
          title={`${t.common.refresh} (${t.host.shortcutRefresh})`}
          aria-label={t.common.refresh}
        >
          <RefreshCw size={14} />
        </button>
      </div>
    </header>
  );
}

interface DashboardFooterProps {
  controlPort: number;
  lanIp: string | null;
  copiedToast: boolean;
  inputPermission: boolean;
  platform: HostSnapshotView["platform"];
  lastUpdated: Date;
  language: SupportedLanguage;
  t: TranslationSchema;
  onCopyAddress: () => void;
  onRequestPermission: () => void;
}

function DashboardFooter(props: DashboardFooterProps) {
  const { t, language } = props;
  const platformLabel =
    props.platform === "macos"
      ? t.common.myMac
      : props.platform === "windows"
        ? t.common.windowsPc
        : t.common.myComputer;

  const addressText = props.lanIp
    ? `${props.lanIp}:${props.controlPort}`
    : `:${props.controlPort}`;

  return (
    <footer className="host-footer">
      <div className="footer-status-info">
        <button
          type="button"
          className="clickable-chip"
          onClick={props.onCopyAddress}
          title={t.host.computerAddressLabel}
        >
          {t.host.computerAddressLabel} <strong>{addressText}</strong>{" "}
          {props.copiedToast ? (
            <span style={{ display: "inline-flex", alignItems: "center", gap: 3, fontWeight: 600 }}>
              <Check size={13} strokeWidth={2.5} />{" "}
              {interpolate(t.host.addressCopied, { address: addressText })}
            </span>
          ) : (
            <Copy size={12} style={{ opacity: 0.7 }} />
          )}
        </button>
        <span className="footer-divider">·</span>
        <button
          type="button"
          className="clickable-chip"
          onClick={props.onRequestPermission}
          title={props.inputPermission ? t.host.permApproved : t.host.permRequired}
        >
          {t.host.remoteControlLabel}{" "}
          {props.inputPermission ? (
            <strong style={{ display: "inline-flex", alignItems: "center", gap: 3 }}>
              <ShieldCheck size={13} /> {t.host.permApproved}
            </strong>
          ) : (
            <strong style={{ display: "inline-flex", alignItems: "center", gap: 3 }}>
              <ShieldAlert size={13} /> {t.host.permRequired}
            </strong>
          )}
        </button>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span className="footer-timestamp">
          {platformLabel} · {t.host.lastUpdated} {props.lastUpdated.toLocaleTimeString(language === "ko" ? "ko-KR" : "en-US")}
        </span>
      </div>
    </footer>
  );
}

function TroubleshootingModal({
  onClose,
  t,
}: {
  onClose: () => void;
  t: TranslationSchema;
}) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-window" onClick={(event) => event.stopPropagation()} style={{ maxWidth: 480 }}>
        <div className="modal-title-bar">
          <h3>{t.host.troubleshootTitle}</h3>
          <button className={buttonVariants({ variant: "close" })} onClick={onClose} aria-label={t.host.troubleshootCloseAria}>
            <X size={15} />
          </button>
        </div>
        <div className="modal-scroll-area" style={{ padding: 16, display: "flex", flexDirection: "column", gap: 12 }}>
          <div className="troubleshoot-card">
            <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 13, color: "var(--text-primary)" }}>
              <Wifi size={16} />
              <span>{t.host.troubleshootWifi}</span>
            </div>
            <p style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 4, lineHeight: 1.5 }}>
              {t.host.troubleshootWifiDesc}
            </p>
          </div>

          <div className="troubleshoot-card">
            <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 13, color: "var(--text-primary)" }}>
              <AlertTriangle size={16} />
              <span>{t.host.troubleshootAp}</span>
            </div>
            <p style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 4, lineHeight: 1.5 }}>
              {t.host.troubleshootApDesc}
            </p>
          </div>

          <div className="troubleshoot-card">
            <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 13, color: "var(--text-primary)" }}>
              <ShieldCheck size={16} />
              <span>{t.host.troubleshootFirewall}</span>
            </div>
            <p style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 4, lineHeight: 1.5 }}>
              {t.host.troubleshootFirewallDesc}
            </p>
          </div>

          <div className="troubleshoot-card">
            <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 13, color: "var(--text-primary)" }}>
              <Monitor size={16} />
              <span>{t.host.troubleshootPerm}</span>
            </div>
            <p style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 4, lineHeight: 1.5 }}>
              {t.host.troubleshootPermDesc}
            </p>
          </div>

          <div className="troubleshoot-card">
            <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 13, color: "var(--text-primary)" }}>
              <AppWindow size={16} />
              <span>{t.host.troubleshootHiddenWindow}</span>
            </div>
            <p style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 4, lineHeight: 1.5 }}>
              {t.host.troubleshootHiddenWindowDesc}
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}

function PairingModal({
  onClose,
  language,
  t,
}: {
  onClose: () => void;
  language: SupportedLanguage;
  t: TranslationSchema;
}) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-window" onClick={(event) => event.stopPropagation()}>
        <div className="modal-title-bar">
          <h3>{t.host.pairingModalTitle}</h3>
          <button className={buttonVariants({ variant: "close" })} onClick={onClose} aria-label={t.host.pairingModalCloseAria}>
            <X size={15} />
          </button>
        </div>
        <div className="modal-scroll-area">
          <PairingPanel language={language} />
        </div>
      </div>
    </div>
  );
}

function StopStreamModal({
  session,
  busy,
  t,
  onCancel,
  onConfirm,
}: {
  session: SessionRow;
  busy: boolean;
  t: TranslationSchema;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <dialog
      open
      className="modal-overlay"
      aria-labelledby="stop-stream-title"
      onClose={onCancel}
    >
      <div className="modal-window stop-stream-modal">
        <div className="modal-title-bar">
          <h3 id="stop-stream-title">{t.host.stopModalTitle}</h3>
          <button className={buttonVariants({ variant: "close" })} disabled={busy} onClick={onCancel} aria-label={t.common.close}>
            <X size={15} />
          </button>
        </div>
        <div className="stop-stream-modal-body">
          <div className="stop-stream-target">
            <strong>{session.sourceName}</strong>
            <span>{interpolate(t.host.stopModalConnectedDevice, { addr: session.viewerAddr })}</span>
          </div>
          <p className="stop-stream-summary">
            {t.host.stopModalSummary}
          </p>
          <div className="stop-stream-actions">
            <button className={buttonVariants({ variant: "ghost" })} disabled={busy} onClick={onCancel}>{t.host.btnKeepStreaming}</button>
            <button className={buttonVariants({ variant: "danger" })} disabled={busy} onClick={onConfirm}>
              <Square size={13} fill="currentColor" />
              {busy ? t.host.stopping : t.host.btnConfirmStop}
            </button>
          </div>
        </div>
      </div>
    </dialog>
  );
}

function TerminationBanner({
  notice,
  language,
  t,
  onDismiss,
}: {
  notice: TerminationNotice;
  language: SupportedLanguage;
  t: TranslationSchema;
  onDismiss: () => void;
}) {
  return (
    <section
      className={terminationNoticeVariants({ tone: notice.tone === "danger" ? "danger" : "default" })}
      role={notice.tone === "danger" ? "alert" : "status"}
      aria-label={notice.title}
    >
      <span className="termination-notice-icon" aria-hidden="true">
        {notice.tone === "danger" ? (
          <AlertTriangle size={14} />
        ) : (
          <Square size={12} fill="currentColor" />
        )}
      </span>
      <div className="termination-notice-content">
        <div className="termination-notice-heading">
          <strong>{notice.title}</strong>
          <time dateTime={notice.observedAt.toISOString()}>
            {notice.observedAt.toLocaleTimeString(language === "ko" ? "ko-KR" : "en-US")}
          </time>
        </div>
        <span className="termination-notice-target">
          {notice.sourceName} · {interpolate(t.host.connectedDevice, { addr: notice.viewerAddr })}
        </span>
        <p><b>{t.host.terminationReasonLabel}</b> {notice.detail}</p>
      </div>
      <button className={buttonVariants({ variant: "close" })} onClick={onDismiss} aria-label={t.common.close}>
        <X size={15} />
      </button>
    </section>
  );
}

interface VirtualDisplayExperimentSectionProps {
  platform: HostSnapshotView["platform"];
  enabled: boolean;
  t: TranslationSchema;
  onToggle: () => void;
}

/// Opt-in gate for the BetterDisplay experiment (design flow step 1: toggle,
/// default off). When `enabled` is false the VirtualDisplayCard is not
/// rendered at all, so no `create_virtual_display`/`remove_virtual_display`
/// command can be invoked.
function VirtualDisplayExperimentSection({
  platform,
  enabled,
  t,
  onToggle,
}: VirtualDisplayExperimentSectionProps) {
  return (
    <>
      <section className="troubleshoot-card" aria-label={t.host.virtualDisplayExperiment}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 13, color: "var(--text-primary)" }}>
            <Monitor size={16} />
            <span>{t.host.virtualDisplayExperiment}</span>
          </div>
          <button
            className={controlToggleVariants({ active: enabled })}
            onClick={onToggle}
            aria-pressed={enabled}
            aria-label={t.host.virtualDisplayExperiment}
            title={enabled ? t.host.virtualDisplayToggleOn : t.host.virtualDisplayToggleOff}
          >
            {enabled ? t.host.virtualDisplayToggleOn : t.host.virtualDisplayToggleOff}
          </button>
        </div>
        <p style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 4, lineHeight: 1.5 }}>
          {t.host.virtualDisplayToggleDesc}
        </p>
      </section>
      {enabled && <VirtualDisplayCard platform={platform} t={t} />}
    </>
  );
}

interface VirtualDisplayCardProps {
  platform: HostSnapshotView["platform"];
  t: TranslationSchema;
}

function VirtualDisplayCard({ platform, t }: VirtualDisplayCardProps) {
  const [name, setName] = useState("Leftcar Virtual");
  const [busy, setBusy] = useState(false);
  const [created, setCreated] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const runDisplayCommand = async (
    command: "create_virtual_display" | "remove_virtual_display",
    onDone: (output: string) => void,
  ) => {
    setBusy(true);
    try {
      const output = command === "create_virtual_display"
        ? await invoke<string>("create_virtual_display", {
            name: name.trim(),
            width: 1920,
            height: 1200,
          })
        : await invoke<string>("remove_virtual_display", { name: name.trim() });
      onDone(output);
      setFailure(null);
    } catch (cause) {
      setCreated(null);
      setFailure(
        interpolate(t.host.virtualDisplayFailed, {
          error: String(cause instanceof Error ? cause.message : cause),
        }),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="troubleshoot-card" aria-label={t.host.virtualDisplayExperiment}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 13, color: "var(--text-primary)" }}>
        <Monitor size={16} />
        <span>{t.host.virtualDisplayExperiment}</span>
      </div>
      <p style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 4, lineHeight: 1.5 }}>
        {t.host.virtualDisplayHint}
      </p>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 10, flexWrap: "wrap" }}>
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          disabled={busy}
          placeholder="Leftcar Virtual"
          aria-label={t.host.virtualDisplayExperiment}
          style={{
            flex: "1 1 160px",
            minWidth: 140,
            padding: "6px 10px",
            fontSize: 12,
            color: "var(--text-primary)",
            background: "var(--bg-surface)",
            border: "1px solid var(--border-card)",
            borderRadius: 8,
            outline: "none",
          }}
        />
        <button
          className={buttonVariants({ variant: "ghost", size: "sm" })}
          disabled={busy || platform !== "macos"}
          onClick={() => void runDisplayCommand("create_virtual_display", (output) => {
            setCreated(output || t.host.virtualDisplayCreated);
          })}
        >
          {busy ? t.host.statusChecking : t.host.virtualDisplayCreate}
        </button>
        <button
          className={buttonVariants({ variant: "ghost", size: "sm" })}
          disabled={busy || platform !== "macos"}
          onClick={() => void runDisplayCommand("remove_virtual_display", (output) => {
            setCreated(output || t.host.virtualDisplayRemoved);
          })}
        >
          {busy ? t.host.statusChecking : t.host.virtualDisplayRemove}
        </button>
      </div>
      {created && (
        <p className="font-emerald" style={{ fontSize: 11, marginTop: 8, display: "flex", alignItems: "center", gap: 4 }}>
          <Check size={13} strokeWidth={2.5} /> {created}
        </p>
      )}
      {failure && (
        <p className="font-rose" style={{ fontSize: 11, marginTop: 8, display: "flex", alignItems: "center", gap: 4 }}>
          <AlertTriangle size={13} /> {failure}
        </p>
      )}
    </section>
  );
}

interface SystemAlertBannersProps {
  error: string | null;
  inputActionError: string | null;
  inputPermission: boolean;
  platform: HostSnapshotView["platform"];
  inputBusy: number | "permission" | null;
  t: TranslationSchema;
  onRequestPermission: () => void;
  onOpenAccessibility: () => void;
}

function SystemAlertBanners({
  error,
  inputActionError,
  inputPermission,
  platform,
  inputBusy,
  t,
  onRequestPermission,
  onOpenAccessibility,
}: SystemAlertBannersProps) {
  return (
    <>
      {error && (
        <div className={bannerAlertVariants({ tone: "danger" })}>
          <div className="banner-text">
            <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
              <AlertTriangle size={16} /> {error}
            </span>
          </div>
          {platform === "macos" && error.includes("Remote Desktop") && (
            <button
              className={buttonVariants({ variant: "ghost", size: "sm" })}
              onClick={() => void invoke("open_system_settings", { pane: "remote_desktop" })}
            >
              {t.host.openRemoteDesktopSettings}
            </button>
          )}
          {platform === "macos" && error.includes("권한") && !error.includes("Remote Desktop") && (
            <button
              className={buttonVariants({ variant: "ghost", size: "sm" })}
              onClick={() => void invoke("open_system_settings", { pane: "screencapture" })}
            >
              {t.host.openScreenCaptureSettings}
            </button>
          )}
        </div>
      )}

      {inputActionError && (
        <div className={bannerAlertVariants({ tone: "danger" })}>
          <div className="banner-text">
            <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
              <AlertTriangle size={16} /> {inputActionError}
            </span>
          </div>
          {platform === "macos" && (
            <button className={buttonVariants({ variant: "ghost", size: "sm" })} onClick={onOpenAccessibility}>
              {t.host.btnOpenSettings}
            </button>
          )}
        </div>
      )}

      {!inputPermission && platform === "macos" && (
        <div className={bannerAlertVariants({ tone: "warning" })}>
          <div className="banner-text">
            <strong>{t.host.inputPermBannerTitle}</strong>
            <p>{t.host.inputPermBannerDesc}</p>
          </div>
          <div className="banner-actions">
            <button
              className={buttonVariants({ variant: "primary", size: "sm" })}
              disabled={inputBusy === "permission"}
              onClick={onRequestPermission}
            >
              {inputBusy === "permission" ? t.host.checkingPerm : t.host.btnGrantPerm}
            </button>
            <button
              className={buttonVariants({ variant: "ghost", size: "sm" })}
              onClick={onOpenAccessibility}
              title={t.host.btnOpenSettings}
            >
              {t.host.btnOpenSettings}
            </button>
          </div>
        </div>
      )}
    </>
  );
}

interface IdleStudioViewProps {
  controlPort: number;
  t: TranslationSchema;
  onOpenPairing: () => void;
}

function IdleStudioView({ t, onOpenPairing }: IdleStudioViewProps) {
  return (
    <div className="idle-center-container">
      <div className="idle-center-card">
        <div className="idle-center-icon-box">
          <Monitor size={26} strokeWidth={2} />
        </div>
        <div className="idle-center-text">
          <h2>{t.host.idleTitle}</h2>
          <p>{t.host.idleDesc}</p>
        </div>

        <div className="idle-status-chip">
          <span className="status-dot" />
          <span>{t.host.idleStatusReady}</span>
        </div>

        <button className={buttonVariants({ variant: "primary", size: "lg" })} onClick={onOpenPairing} title={`${t.host.btnCreatePairing} (${t.host.shortcutPair})`}>
          <QrCode size={15} />
          <span>{t.host.btnCreatePairing}</span>
          <span className="kbd-shortcut" style={{ marginLeft: 4, background: "rgba(255,255,255,0.2)", color: "inherit", borderColor: "rgba(255,255,255,0.3)" }}>{t.host.shortcutPair}</span>
        </button>

        <span className="idle-center-hint">
          <Info size={12} />
          {t.host.idleHint}
        </span>
      </div>
    </div>
  );
}

interface StreamsListViewProps {
  sessions: SessionRow[];
  inputPermission: boolean;
  inputBusy: number | "permission" | null;
  showInspector: boolean;
  t: TranslationSchema;
  onToggleInspector: () => void;
  onToggleInput: (session: SessionRow) => Promise<void>;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
  qualityBusy: number | null;
  onForceStop: (session: SessionRow) => void;
}

function StreamsListView({
  sessions,
  inputPermission,
  inputBusy,
  showInspector,
  t,
  onToggleInspector,
  onToggleInput,
  onSetQuality,
  qualityBusy,
  onForceStop,
}: StreamsListViewProps) {
  return (
    <div className="streams-section">
      <div className="streams-section-header">
        <div className="streams-header-left">
          <h2>{interpolate(t.host.activeSectionTitle, { count: sessions.length })}</h2>
          <span className="live-badge-pulse">
            <span className="status-dot" /> {t.host.liveBadge}
          </span>
        </div>
        <button
          className={buttonVariants({ variant: "link" })}
          onClick={onToggleInspector}
          style={{ display: "inline-flex", alignItems: "center", gap: 4 }}
        >
          {showInspector ? (
            <>{t.host.hideMetrics} <ChevronUp size={14} /></>
          ) : (
            <>{t.host.showMetrics} <ChevronDown size={14} /></>
          )}
        </button>
      </div>

      <div className="stream-cards-container">
        {sessions.map((session) => (
          <SessionCard
            key={session.session}
            session={session}
            inputPermission={inputPermission}
            inputBusy={inputBusy === session.session}
            showInspector={showInspector}
            t={t}
            onToggleInput={onToggleInput}
            onSetQuality={onSetQuality}
            qualityBusy={qualityBusy === session.session}
            onForceStop={onForceStop}
          />
        ))}
      </div>
    </div>
  );
}

function Dashboard() {
  const [language, setLanguage] = useState<SupportedLanguage>(() => {
    const saved = localStorage.getItem("leftcar_lang") as SupportedLanguage | null;
    if (saved === "ko" || saved === "en") return saved;
    const navLang = navigator.language?.toLowerCase() || "ko";
    return navLang.startsWith("en") ? "en" : "ko";
  });

  useEffect(() => {
    localStorage.setItem("leftcar_lang", language);
    document.documentElement.lang = language;
  }, [language]);

  const toggleLanguage = useCallback(() => {
    setLanguage((prev) => (prev === "ko" ? "en" : "ko"));
  }, []);

  const t = getTranslation(language);

  const {
    sessions,
    terminationNotice,
    dismissTerminationNotice,
    error,
    inputPermission,
    platform,
    controlPort,
    lanIp,
    lastUpdated,
    refresh,
  } = useHostStatus(t);
  const [inputActionError, setInputActionError] = useState<string | null>(null);
  const [inputBusy, setInputBusy] = useState<number | "permission" | null>(null);
  const [qualityBusy, setQualityBusy] = useState<number | null>(null);
  const [showInspector, setShowInspector] = useState(false);
  const [showPairingModal, setShowPairingModal] = useState(false);
  const [showHelpModal, setShowHelpModal] = useState(false);
  const [pendingStopSession, setPendingStopSession] = useState<SessionRow | null>(null);
  const [copiedToast, setCopiedToast] = useState(false);
  const [theme, setTheme] = useState<ThemeMode>(() => {
    return (localStorage.getItem("leftcar_theme") as ThemeMode) || "system";
  });
  // Opt-in experiment gate: default off, persisted like the other host settings.
  const [virtualDisplayExperiment, setVirtualDisplayExperiment] = useState<boolean>(() => {
    return localStorage.getItem("leftcar_virtual_display_experiment") === "on";
  });

  const toggleVirtualDisplayExperiment = useCallback(() => {
    setVirtualDisplayExperiment((prev) => !prev);
  }, []);

  useEffect(() => {
    localStorage.setItem(
      "leftcar_virtual_display_experiment",
      virtualDisplayExperiment ? "on" : "off",
    );
  }, [virtualDisplayExperiment]);

  const isStreaming = sessions.length > 0;

  useEffect(() => {
    const root = document.documentElement;
    if (theme === "system") {
      root.removeAttribute("data-theme");
    } else {
      root.setAttribute("data-theme", theme);
    }
    localStorage.setItem("leftcar_theme", theme);
  }, [theme]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setShowPairingModal(false);
        setShowHelpModal(false);
        if (inputBusy === null) setPendingStopSession(null);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "p") {
        e.preventDefault();
        setShowPairingModal((prev) => !prev);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "h") {
        e.preventDefault();
        setShowHelpModal((prev) => !prev);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
        e.preventDefault();
        void refresh();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [inputBusy, refresh]);

  const openAccessibilitySettings = async () => {
    try {
      await invoke("open_system_settings", { pane: "accessibility" });
    } catch (cause) {
      setInputActionError(hostErrorMessage(cause, t));
    }
  };

  const requestInputPermission = async () => {
    setInputBusy("permission");
    try {
      const granted = await invoke<boolean>("request_input_permission");
      if (!granted) {
        await invoke("open_system_settings", { pane: "accessibility" }).catch(() => {});
        setInputActionError(t.host.permissionErrorGuide);
      } else {
        setInputActionError(null);
      }
      await refresh();
    } catch (cause) {
      setInputActionError(hostErrorMessage(cause, t));
    } finally {
      setInputBusy(null);
    }
  };

  const toggleSessionInput = async (session: SessionRow) => {
    setInputBusy(session.session);
    try {
      await invoke("set_session_input", {
        session: session.session,
        enabled: !session.inputEnabled,
      });
      setInputActionError(null);
      await refresh();
    } catch (cause) {
      setInputActionError(hostErrorMessage(cause, t));
    } finally {
      setInputBusy(null);
    }
  };

  const setSessionQuality = async (session: SessionRow, quality: number | null) => {
    setQualityBusy(session.session);
    try {
      await invoke("set_session_quality", {
        session: session.session,
        quality,
      });
      setInputActionError(null);
      await refresh();
    } catch (cause) {
      setInputActionError(hostErrorMessage(cause, t));
    } finally {
      setQualityBusy(null);
    }
  };

  const forceStopSession = async (session: SessionRow) => {
    setInputBusy(session.session);
    try {
      await invoke("force_stop_session", { session: session.session });
      setInputActionError(null);
      await refresh();
      setPendingStopSession(null);
    } catch (cause) {
      setInputActionError(hostErrorMessage(cause, t));
    } finally {
      setInputBusy(null);
    }
  };

  const copyAddressInfo = () => {
    const textToCopy = lanIp ? `${lanIp}:${controlPort}` : `:${controlPort}`;
    void navigator.clipboard.writeText(textToCopy);
    setCopiedToast(true);
    setTimeout(() => setCopiedToast(false), 2000);
  };

  const toggleTheme = () => {
    setTheme((prev) => {
      if (prev === "system") return "light";
      if (prev === "light") return "dark";
      return "system";
    });
  };

  const themeLabel =
    theme === "light"
      ? t.common.themeLight
      : theme === "dark"
        ? t.common.themeDark
        : t.common.themeSystem;

  return (
    <div className="host-window">
      <DashboardHeader
        isStreaming={isStreaming}
        sessionCount={sessions.length}
        themeMode={theme}
        themeLabel={themeLabel}
        language={language}
        t={t}
        onPair={() => setShowPairingModal(true)}
        onHelp={() => setShowHelpModal(true)}
        onTheme={toggleTheme}
        onToggleLanguage={toggleLanguage}
        onRefresh={() => void refresh()}
      />

      <main className="host-body">
        {terminationNotice && (
          <TerminationBanner
            notice={terminationNotice}
            language={language}
            t={t}
            onDismiss={dismissTerminationNotice}
          />
        )}

        <SystemAlertBanners
          error={error}
          inputActionError={inputActionError}
          inputPermission={inputPermission}
          platform={platform}
          inputBusy={inputBusy}
          t={t}
          onRequestPermission={requestInputPermission}
          onOpenAccessibility={openAccessibilitySettings}
        />

        {isStreaming ? (
          <StreamsListView
            sessions={sessions}
            inputPermission={inputPermission}
            inputBusy={inputBusy}
            showInspector={showInspector}
            t={t}
            onToggleInspector={() => setShowInspector((prev) => !prev)}
            onToggleInput={toggleSessionInput}
            onSetQuality={setSessionQuality}
            qualityBusy={qualityBusy}
            onForceStop={setPendingStopSession}
          />
        ) : (
          <IdleStudioView
            controlPort={controlPort}
            t={t}
            onOpenPairing={() => setShowPairingModal(true)}
          />
        )}

        <VirtualDisplayExperimentSection
          platform={platform}
          enabled={virtualDisplayExperiment}
          t={t}
          onToggle={toggleVirtualDisplayExperiment}
        />
      </main>

      <DashboardFooter
        controlPort={controlPort}
        lanIp={lanIp}
        copiedToast={copiedToast}
        inputPermission={inputPermission}
        platform={platform}
        lastUpdated={lastUpdated}
        language={language}
        t={t}
        onCopyAddress={copyAddressInfo}
        onRequestPermission={requestInputPermission}
      />

      {showPairingModal && (
        <PairingModal
          language={language}
          t={t}
          onClose={() => setShowPairingModal(false)}
        />
      )}
      {showHelpModal && (
        <TroubleshootingModal
          t={t}
          onClose={() => setShowHelpModal(false)}
        />
      )}
      {pendingStopSession && (
        <StopStreamModal
          session={pendingStopSession}
          busy={inputBusy === pendingStopSession.session}
          t={t}
          onCancel={() => setPendingStopSession(null)}
          onConfirm={() => void forceStopSession(pendingStopSession)}
        />
      )}
    </div>
  );
}

interface SessionCardProps {
  session: SessionRow;
  inputPermission: boolean;
  inputBusy: boolean;
  showInspector: boolean;
  t: TranslationSchema;
  onToggleInput: (session: SessionRow) => Promise<void>;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
  qualityBusy: boolean;
  onForceStop: (session: SessionRow) => void;
}

function SessionCard({
  session,
  inputPermission,
  inputBusy,
  showInspector,
  t,
  onToggleInput,
  onSetQuality,
  qualityBusy,
  onForceStop,
}: SessionCardProps) {
  const bitrateMbps = session.kbps > 0 ? (session.kbps / 1000).toFixed(1) : "0.0";
  const encodeOutputFps = session.encodeOutputFps ?? session.fps;
  const transportLabel =
    session.mediaTransport === "usb"
      ? t.host.cableUsb
      : session.mediaTransport === "udp"
        ? t.host.wifiWireless
        : session.mediaTransport || t.host.unknownTransport;
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
              <span className="session-tag">#{session.session}</span>
            </div>
            <span className="stream-card-target">
              {interpolate(t.host.connectedDevice, { addr: session.viewerAddr })}
            </span>
          </div>
        </div>

        <div className="stream-card-action">
          <button
            className={controlToggleVariants({ active: session.inputEnabled })}
            disabled={(!inputPermission && !session.inputEnabled) || session.state !== "running" || inputBusy}
            onClick={() => void onToggleInput(session)}
            title={session.inputEnabled ? t.host.remoteInputAllowed : t.host.remoteInputOff}
          >
            {inputBusy
              ? t.host.remoteInputProcessing
              : session.inputEnabled
                ? t.host.remoteInputAllowed
                : t.host.remoteInputOff}
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
          <span className="metric-card-value font-emerald">
            <span className="signal-bars" aria-hidden="true">
              <span className="bar bar-1 active" />
              <span className="bar bar-2 active" />
              <span className="bar bar-3 active" />
            </span>
            {encodeOutputFps} FPS
          </span>
        </div>

        <div className="metric-card">
          <span className="metric-card-label">{t.host.bitrate}</span>
          <span className="metric-card-value">{bitrateMbps} Mbps</span>
        </div>

        <div className="metric-card">
          <span className="metric-card-label">{t.host.connectionStatus}</span>
          <span className="metric-card-value font-blue">
            {session.state === "running" ? t.host.statusRunning : t.host.statusChecking}
          </span>
        </div>

        <div className="metric-card">
          <span className="metric-card-label">{t.host.transferStability}</span>
          <span className="metric-card-value">
            {session.dropped ? (
              <span className="font-rose">{interpolate(t.host.droppedFrames, { count: session.dropped })}</span>
            ) : (
              <span className="font-emerald">{t.host.stabilityStable}</span>
            )}
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
        <span style={{ fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--text-dim)" }}>
          {session.state === "running" ? t.host.liveBadge : t.host.statusChecking}
        </span>
      </div>
    </div>
  );
}
