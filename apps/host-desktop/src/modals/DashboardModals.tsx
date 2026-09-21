import {
  AlertTriangle,
  AppWindow,
  ClipboardCheck,
  Eye,
  EyeOff,
  Globe,
  Lock,
  Monitor,
  QrCode,
  Settings,
  ShieldCheck,
  Square,
  Wifi,
  X,
} from "lucide-react";
import {
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import type { SessionRow } from "../sessionTypes";
import Modal from "../Modal";
import PairingPanel from "../PairingPanel";
import { buttonVariants } from "../lib/variants";
import { ExperimentsSection } from "../ExperimentsSection";
import FileShareCard from "../components/FileShareCard";
import ExtendedDisplayCard from "../components/ExtendedDisplayCard";

export interface HostSettingsModalProps {
  onClose: () => void;
  t: TranslationSchema;
  clipboardShare: boolean;
  lockOnDisconnect: boolean;
  privacyCurtain: boolean;
  streamingBadge: boolean;
  wanAccess: boolean;
  clipboardPending: boolean;
  lockPending: boolean;
  curtainPending: boolean;
  badgePending: boolean;
  wanPending: boolean;
  clipboardError: string | null;
  lockError: string | null;
  curtainError: string | null;
  badgeError: string | null;
  wanError: string | null;
  onToggleClipboardShare: () => void;
  onToggleLockOnDisconnect: () => void;
  onTogglePrivacyCurtain: () => void;
  onToggleStreamingBadge: () => void;
  onToggleWanAccess: () => void;
  retryClipboard: () => void;
  retryLock: () => void;
  retryCurtain: () => void;
  retryBadge: () => void;
  retryWan: () => void;
}

export function HostSettingsModal(props: HostSettingsModalProps) {
  const { t, onClose } = props;

  const settingsItems = [
    {
      icon: ClipboardCheck,
      title: t.host.clipboardShareLabel,
      desc: t.host.clipboardShareDesc,
      active: props.clipboardShare,
      pending: props.clipboardPending,
      error: props.clipboardError,
      onToggle: props.onToggleClipboardShare,
      onRetry: props.retryClipboard,
    },
    {
      icon: Lock,
      title: t.host.lockOnDisconnectLabel,
      desc: t.host.lockOnDisconnectDesc,
      active: props.lockOnDisconnect,
      pending: props.lockPending,
      error: props.lockError,
      onToggle: props.onToggleLockOnDisconnect,
      onRetry: props.retryLock,
    },
    {
      icon: EyeOff,
      title: t.host.privacyCurtainLabel,
      desc: t.host.privacyCurtainDesc,
      active: props.privacyCurtain,
      pending: props.curtainPending,
      error: props.curtainError,
      onToggle: props.onTogglePrivacyCurtain,
      onRetry: props.retryCurtain,
    },
    {
      icon: Eye,
      title: t.host.streamingBadgeLabel,
      desc: t.host.streamingBadgeDesc,
      active: props.streamingBadge,
      pending: props.badgePending,
      error: props.badgeError,
      onToggle: props.onToggleStreamingBadge,
      onRetry: props.retryBadge,
    },
    {
      icon: Globe,
      title: t.host.wanAccessLabel,
      desc: t.host.wanAccessDesc,
      active: props.wanAccess,
      pending: props.wanPending,
      error: props.wanError,
      onToggle: props.onToggleWanAccess,
      onRetry: props.retryWan,
    },
  ];

  return (
    <Modal ariaLabel={t.host.settingsTitle} onClose={onClose} closeOnOverlayClick>
      <div className="modal-window" onClick={(event) => event.stopPropagation()} style={{ maxWidth: 540 }}>
        <div className="modal-title-bar">
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <div
              style={{
                width: 28,
                height: 28,
                borderRadius: 7,
                background: "var(--bg-surface-subtle)",
                border: "1px solid var(--border-subtle)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                color: "var(--text-primary)",
              }}
            >
              <Settings size={15} />
            </div>
            <h3 style={{ margin: 0, fontSize: 14, fontWeight: 700 }}>{t.host.settingsTitle}</h3>
          </div>
          <button
            className={buttonVariants({ variant: "close" })}
            onClick={onClose}
            aria-label={t.host.settingsModalClose}
          >
            <X size={15} />
          </button>
        </div>

        <div className="modal-scroll-area" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 18 }}>
          <div className="settings-section">
            <span className="settings-section-title">{t.host.privacySection}</span>
            <div className="settings-group-container">
              {settingsItems.map((item) => {
                const Icon = item.icon;
                return (
                  <div
                    className="settings-item-row"
                    key={item.title}
                    role="button"
                    tabIndex={item.pending ? -1 : 0}
                    aria-disabled={item.pending}
                    onClick={() => {
                      if (!item.pending) item.onToggle();
                    }}
                    onKeyDown={(e) => {
                      if ((e.key === "Enter" || e.key === " ") && !item.pending) {
                        e.preventDefault();
                        item.onToggle();
                      }
                    }}
                  >
                    <div className="settings-item-icon-box">
                      <Icon size={16} />
                    </div>
                    <div className="settings-item-info">
                      <span className="settings-item-name">{item.title}</span>
                      <p className="settings-item-desc">{item.desc}</p>
                      {item.error && (
                        <div
                          className="settings-item-error"
                          role="alert"
                          onClick={(e) => e.stopPropagation()}
                        >
                          <span>{item.error}</span>
                          <button
                            type="button"
                            className={buttonVariants({ variant: "ghost", size: "sm" })}
                            onClick={item.onRetry}
                            disabled={item.pending}
                          >
                            {t.common.retry}
                          </button>
                        </div>
                      )}
                    </div>
                    <button
                      type="button"
                      disabled={item.pending}
                      aria-busy={item.pending}
                      aria-label={`${item.title} ${item.active ? t.host.clipboardShareOn : t.host.clipboardShareOff}`}
                      className={`ui-switch ${item.active ? "switch-active" : ""}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        item.onToggle();
                      }}
                      aria-pressed={item.active}
                    >
                      <span className="ui-switch-thumb" />
                    </button>
                  </div>
                );
              })}
            </div>
          </div>

          <div className="settings-section">
            <span className="settings-section-title">{t.host.extDisplaySection}</span>
            <ExtendedDisplayCard t={t} />
          </div>

          <div className="settings-section">
            <span className="settings-section-title">{t.host.fileShareSection}</span>
            <FileShareCard t={t} />
          </div>

          <ExperimentsSection t={t} />
        </div>
        <div
          style={{
            padding: "0 18px 16px",
            display: "flex",
            alignItems: "center",
            gap: 6,
            color: "var(--text-muted)",
            fontSize: 12,
          }}
        >
          <ShieldCheck size={14} style={{ flexShrink: 0 }} />
          <span>{t.host.settingsTitle} · {t.host.autoCleanupPolicy}</span>
        </div>
      </div>
    </Modal>
  );
}

export function TroubleshootingModal({
  onClose,
  t,
}: {
  onClose: () => void;
  t: TranslationSchema;
}) {
  return (
    <Modal ariaLabel={t.host.troubleshootTitle} onClose={onClose} closeOnOverlayClick>
      <div className="modal-window" onClick={(event) => event.stopPropagation()} style={{ maxWidth: 480 }}>
        <div className="modal-title-bar">
          <h3>{t.host.troubleshootTitle}</h3>
          <button className={buttonVariants({ variant: "close" })} onClick={onClose} aria-label={t.host.troubleshootCloseAria}>
            <X size={15} />
          </button>
        </div>
        <div className="modal-scroll-area" style={{ padding: 16, display: "flex", flexDirection: "column", gap: 12 }}>
          {(
            [
              { icon: Wifi, title: t.host.troubleshootWifi, desc: t.host.troubleshootWifiDesc },
              { icon: AlertTriangle, title: t.host.troubleshootAp, desc: t.host.troubleshootApDesc },
              { icon: ShieldCheck, title: t.host.troubleshootFirewall, desc: t.host.troubleshootFirewallDesc },
              { icon: Monitor, title: t.host.troubleshootPerm, desc: t.host.troubleshootPermDesc },
              { icon: AppWindow, title: t.host.troubleshootHiddenWindow, desc: t.host.troubleshootHiddenWindowDesc },
            ] as const
          ).map(({ icon: Icon, title, desc }) => (
            <div className="troubleshoot-card" key={title}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <Icon size={16} />
                <span style={{ fontWeight: 700, fontSize: 13, color: "var(--text-primary)" }}>{title}</span>
              </div>
              <p style={{ margin: "4px 0 0", fontSize: 12, color: "var(--text-secondary)", lineHeight: 1.5 }}>
                {desc}
              </p>
            </div>
          ))}
        </div>
      </div>
    </Modal>
  );
}

export function PairingModal({
  onClose,
  language,
  t,
}: {
  onClose: () => void;
  language: SupportedLanguage;
  t: TranslationSchema;
}) {
  return (
    <Modal ariaLabel={t.host.pairingModalTitle} onClose={onClose} closeOnOverlayClick>
      <div
        className="modal-window modal-wide"
        onClick={(event) => event.stopPropagation()}
        style={{ maxWidth: 620 }}
      >
        <div className="modal-title-bar">
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <div
              style={{
                width: 28,
                height: 28,
                borderRadius: 7,
                background: "var(--bg-surface-subtle)",
                border: "1px solid var(--border-subtle)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                color: "var(--text-primary)",
              }}
            >
              <QrCode size={15} />
            </div>
            <h3 style={{ margin: 0, fontSize: 14, fontWeight: 700 }}>
              {t.host.pairingModalTitle}
            </h3>
          </div>
          <button
            className={buttonVariants({ variant: "close" })}
            onClick={onClose}
            aria-label={t.host.pairingModalCloseAria}
          >
            <X size={15} />
          </button>
        </div>
        <div className="modal-scroll-area">
          <PairingPanel language={language} />
        </div>
      </div>
    </Modal>
  );
}

export function StopStreamModal({
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
    <Modal
      labelledBy="stop-stream-title"
      onClose={() => {
        if (!busy) onCancel();
      }}
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
    </Modal>
  );
}

export interface DashboardModalsProps {
  showPairingModal: boolean;
  showHelpModal: boolean;
  showSettingsModal: boolean;
  pendingStopSession: SessionRow | null;
  inputBusy: number | "permission" | null;
  language: SupportedLanguage;
  t: TranslationSchema;
  clipboardShare: boolean;
  lockOnDisconnect: boolean;
  privacyCurtain: boolean;
  streamingBadge: boolean;
  wanAccess: boolean;
  clipboardPending: boolean;
  lockPending: boolean;
  curtainPending: boolean;
  badgePending: boolean;
  wanPending: boolean;
  clipboardError: string | null;
  lockError: string | null;
  curtainError: string | null;
  badgeError: string | null;
  wanError: string | null;
  onClosePairing: () => void;
  onCloseHelp: () => void;
  onCloseSettings: () => void;
  onCancelStopSession: () => void;
  onConfirmStopSession: (session: SessionRow) => void;
  onToggleClipboardShare: () => void;
  onToggleLockOnDisconnect: () => void;
  onTogglePrivacyCurtain: () => void;
  onToggleStreamingBadge: () => void;
  onToggleWanAccess: () => void;
  retryClipboard: () => void;
  retryLock: () => void;
  retryCurtain: () => void;
  retryBadge: () => void;
  retryWan: () => void;
}

export function DashboardModals(props: DashboardModalsProps) {
  return (
    <>
      {props.showPairingModal && (
        <PairingModal
          onClose={props.onClosePairing}
          language={props.language}
          t={props.t}
        />
      )}
      {props.showHelpModal && (
        <TroubleshootingModal
          onClose={props.onCloseHelp}
          t={props.t}
        />
      )}
      {props.showSettingsModal && (
        <HostSettingsModal
          onClose={props.onCloseSettings}
          t={props.t}
          clipboardShare={props.clipboardShare}
          lockOnDisconnect={props.lockOnDisconnect}
          privacyCurtain={props.privacyCurtain}
          streamingBadge={props.streamingBadge}
          wanAccess={props.wanAccess}
          clipboardPending={props.clipboardPending}
          lockPending={props.lockPending}
          curtainPending={props.curtainPending}
          badgePending={props.badgePending}
          wanPending={props.wanPending}
          clipboardError={props.clipboardError}
          lockError={props.lockError}
          curtainError={props.curtainError}
          badgeError={props.badgeError}
          wanError={props.wanError}
          onToggleClipboardShare={props.onToggleClipboardShare}
          onToggleLockOnDisconnect={props.onToggleLockOnDisconnect}
          onTogglePrivacyCurtain={props.onTogglePrivacyCurtain}
          onToggleStreamingBadge={props.onToggleStreamingBadge}
          onToggleWanAccess={props.onToggleWanAccess}
          retryClipboard={props.retryClipboard}
          retryLock={props.retryLock}
          retryCurtain={props.retryCurtain}
          retryBadge={props.retryBadge}
          retryWan={props.retryWan}
        />
      )}
      {props.pendingStopSession && (
        <StopStreamModal
          session={props.pendingStopSession}
          busy={props.inputBusy === props.pendingStopSession.session}
          t={props.t}
          onCancel={props.onCancelStopSession}
          onConfirm={() => {
            if (props.pendingStopSession) {
              props.onConfirmStopSession(props.pendingStopSession);
            }
          }}
        />
      )}
    </>
  );
}

export default DashboardModals;
