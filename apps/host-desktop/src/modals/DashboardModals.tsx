import { Button, Notice, Surface, Text, Toggle } from "../ui/primitives";
import {
  AlertTriangle,
  AppWindow,
  Monitor,
  ShieldCheck,
  Square,
  Wifi,
} from "lucide-react";
import {
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import type { HostSnapshotView } from "../hostState";
import type { SessionRow } from "../sessionTypes";
import Modal from "../Modal";
import PairingPanel from "../PairingPanel";
import { ExperimentsSection } from "../ExperimentsSection";
import FileShareCard from "../components/FileShareCard";
import ExtendedDisplayCard from "../components/ExtendedDisplayCard";

export interface HostSettingsModalProps {
  onClose: () => void;
  platform?: HostSnapshotView["platform"];
  t: TranslationSchema;
  clipboardShare: boolean;
  privacyCurtain: boolean;
  streamingBadge: boolean;
  wanAccess: boolean;
  clipboardReady?: boolean;
  curtainReady?: boolean;
  badgeReady?: boolean;
  wanReady?: boolean;
  clipboardPending: boolean;
  curtainPending: boolean;
  badgePending: boolean;
  wanPending: boolean;
  clipboardError: string | null;
  curtainError: string | null;
  badgeError: string | null;
  wanError: string | null;
  onToggleClipboardShare: () => void;
  onTogglePrivacyCurtain: () => void;
  onToggleStreamingBadge: () => void;
  onToggleWanAccess: () => void;
  retryClipboard: () => void;
  retryCurtain: () => void;
  retryBadge: () => void;
  retryWan: () => void;
}

import { DialogPanel } from "./DialogPanel";
export function HostSettingsModal(props: HostSettingsModalProps) {
  const { t, onClose } = props;
  const settingsItems = [
    {
      title: t.host.clipboardShareLabel,
      desc: t.host.clipboardShareDesc,
      active: props.clipboardShare,
      ready: props.clipboardReady,
      pending: props.clipboardPending,
      error: props.clipboardError,
      onToggle: props.onToggleClipboardShare,
      onRetry: props.retryClipboard,
    },
    {
      title: t.host.privacyCurtainLabel,
      desc: t.host.privacyCurtainDesc,
      active: props.privacyCurtain,
      supported: props.platform !== "windows",
      ready: props.curtainReady,
      pending: props.curtainPending,
      error: props.curtainError,
      onToggle: props.onTogglePrivacyCurtain,
      onRetry: props.retryCurtain,
    },
    {
      title: t.host.streamingBadgeLabel,
      desc: t.host.streamingBadgeDesc,
      active: props.streamingBadge,
      ready: props.badgeReady,
      pending: props.badgePending,
      error: props.badgeError,
      onToggle: props.onToggleStreamingBadge,
      onRetry: props.retryBadge,
    },
    {
      title: t.host.wanAccessLabel,
      desc: t.host.wanAccessDesc,
      active: props.wanAccess,
      ready: props.wanReady,
      pending: props.wanPending,
      error: props.wanError,
      onToggle: props.onToggleWanAccess,
      onRetry: props.retryWan,
    },
  ];
  return (
    <Modal
      ariaLabel={t.host.settingsTitle}
      onClose={onClose}
      closeOnOverlayClick
    >
      <DialogPanel
        title={t.host.settingsTitle}
        closeLabel={t.host.settingsModalClose}
        onClose={onClose}
      >
        <div className="min-h-0 flex-1 space-y-5 overflow-y-auto p-4">
          <section className="space-y-2" aria-label={t.host.privacySection}>
            <h3 className="text-caption text-muted font-semibold">
              {t.host.privacySection}
            </h3>
            <Surface variant="card" className="divide-y divide-outline p-0">
              {settingsItems.map((item) => (
                <div key={item.title} className="flex items-start gap-3 p-4">
                  <div className="min-w-0 flex-1">
                    <Text className="block font-semibold">{item.title}</Text>
                    <p className="text-caption text-muted mt-1">{item.desc}</p>
                    {item.pending && (
                      <Text
                        variant="caption"
                        tone="muted"
                        role="status"
                        className="block mt-1"
                      >
                        {item.ready
                          ? t.host.savingSettings
                          : t.host.checkingSettings}
                      </Text>
                    )}
                    {item.error && (
                      <Notice tone="error" className="mt-2 space-y-2">
                        <Text variant="caption" className="block">
                          {item.ready
                            ? t.host.settingsSaveError
                            : t.host.settingsLoadError}
                        </Text>
                        <Text variant="caption" className="block break-words">
                          {item.error}
                        </Text>
                        <Button
                          variant="secondary"
                          size="compact"
                          onClick={item.onRetry}
                          busy={item.pending}
                        >
                          {t.common.retry}
                        </Button>
                      </Notice>
                    )}
                  </div>
                  <Toggle
                    checked={item.active}
                    busy={item.pending}
                    disabled={
                      !!item.error || ("supported" in item && !item.supported)
                    }
                    aria-label={item.title}
                    onClick={item.onToggle}
                  />
                </div>
              ))}
            </Surface>
          </section>
          <ExtendedDisplayCard t={t} />
          <FileShareCard t={t} />
          <ExperimentsSection t={t} />
        </div>
      </DialogPanel>
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
    <Modal
      ariaLabel={t.host.troubleshootTitle}
      onClose={onClose}
      closeOnOverlayClick
    >
      <DialogPanel
        title={t.host.troubleshootTitle}
        closeLabel={t.host.troubleshootCloseAria}
        onClose={onClose}
      >
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
          {(
            [
              {
                icon: Wifi,
                title: t.host.troubleshootWifi,
                desc: t.host.troubleshootWifiDesc,
              },
              {
                icon: AlertTriangle,
                title: t.host.troubleshootAp,
                desc: t.host.troubleshootApDesc,
              },
              {
                icon: ShieldCheck,
                title: t.host.troubleshootFirewall,
                desc: t.host.troubleshootFirewallDesc,
              },
              {
                icon: Monitor,
                title: t.host.troubleshootPerm,
                desc: t.host.troubleshootPermDesc,
              },
              {
                icon: AppWindow,
                title: t.host.troubleshootHiddenWindow,
                desc: t.host.troubleshootHiddenWindowDesc,
              },
            ] as const
          ).map(({ icon: Icon, title, desc }) => (
            <Surface variant="card" key={title} className="space-y-2">
              <h3 className="text-body text-ink flex items-center gap-2 font-semibold">
                <Icon size={16} className="shrink-0" />
                {title}
              </h3>
              <p className="text-caption text-muted">{desc}</p>
            </Surface>
          ))}
        </div>
      </DialogPanel>
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
    <Modal
      ariaLabel={t.host.pairingModalTitle}
      onClose={onClose}
      closeOnOverlayClick
    >
      <DialogPanel
        title={t.host.pairingModalTitle}
        closeLabel={t.host.pairingModalCloseAria}
        onClose={onClose}
        size="wide"
      >
        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          <PairingPanel language={language} />
        </div>
      </DialogPanel>
    </Modal>
  );
}

export function StopStreamModal({
  session,
  busy,
  error,
  t,
  onCancel,
  onConfirm,
}: {
  session: SessionRow;
  busy: boolean;
  error?: string | null;
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
      <DialogPanel
        title={t.host.stopModalTitle}
        titleId="stop-stream-title"
        closeLabel={t.common.close}
        onClose={onCancel}
        busy={busy}
        size="compact"
      >
        <div className="min-h-0 space-y-4 overflow-y-auto p-4">
          <Surface variant="inset" className="space-y-1">
            <Text className="block break-words font-semibold">
              {session.sourceName}
            </Text>
            <Text variant="code" tone="muted" className="block break-all">
              {interpolate(t.host.stopModalConnectedDevice, {
                addr: session.viewerAddr,
              })}
            </Text>
          </Surface>
          <p className="text-body text-muted">{t.host.stopModalSummary}</p>
          {error && <Notice tone="error">{error}</Notice>}
          <div className="flex flex-wrap justify-end gap-2">
            <Button variant="ghost" disabled={busy} onClick={onCancel}>
              {t.host.btnKeepStreaming}
            </Button>
            <Button variant="danger" busy={busy} onClick={onConfirm}>
              <Square size={16} fill="currentColor" />
              {busy
                ? t.host.stopping
                : error
                  ? t.common.retry
                  : t.host.btnConfirmStop}
            </Button>
          </div>
        </div>
      </DialogPanel>
    </Modal>
  );
}

export interface DashboardModalsProps {
  showPairingModal: boolean;
  showHelpModal: boolean;
  showSettingsModal: boolean;
  pendingStopSession: SessionRow | null;
  stopError: string | null;
  stopBusy: boolean;
  inputBusy: number | "permission" | null;
  language: SupportedLanguage;
  platform?: HostSnapshotView["platform"];
  t: TranslationSchema;
  clipboardShare: boolean;
  privacyCurtain: boolean;
  streamingBadge: boolean;
  wanAccess: boolean;
  clipboardReady?: boolean;
  curtainReady?: boolean;
  badgeReady?: boolean;
  wanReady?: boolean;
  clipboardPending: boolean;
  curtainPending: boolean;
  badgePending: boolean;
  wanPending: boolean;
  clipboardError: string | null;
  curtainError: string | null;
  badgeError: string | null;
  wanError: string | null;
  onClosePairing: () => void;
  onCloseHelp: () => void;
  onCloseSettings: () => void;
  onCancelStopSession: () => void;
  onConfirmStopSession: (session: SessionRow) => void;
  onToggleClipboardShare: () => void;
  onTogglePrivacyCurtain: () => void;
  onToggleStreamingBadge: () => void;
  onToggleWanAccess: () => void;
  retryClipboard: () => void;
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
        <TroubleshootingModal onClose={props.onCloseHelp} t={props.t} />
      )}
      {props.showSettingsModal && (
        <HostSettingsModal
          onClose={props.onCloseSettings}
          platform={props.platform}
          t={props.t}
          clipboardShare={props.clipboardShare}
          privacyCurtain={props.privacyCurtain}
          streamingBadge={props.streamingBadge}
          wanAccess={props.wanAccess}
          clipboardReady={props.clipboardReady}
          curtainReady={props.curtainReady}
          badgeReady={props.badgeReady}
          wanReady={props.wanReady}
          clipboardPending={props.clipboardPending}
          curtainPending={props.curtainPending}
          badgePending={props.badgePending}
          wanPending={props.wanPending}
          clipboardError={props.clipboardError}
          curtainError={props.curtainError}
          badgeError={props.badgeError}
          wanError={props.wanError}
          onToggleClipboardShare={props.onToggleClipboardShare}
          onTogglePrivacyCurtain={props.onTogglePrivacyCurtain}
          onToggleStreamingBadge={props.onToggleStreamingBadge}
          onToggleWanAccess={props.onToggleWanAccess}
          retryClipboard={props.retryClipboard}
          retryCurtain={props.retryCurtain}
          retryBadge={props.retryBadge}
          retryWan={props.retryWan}
        />
      )}
      {props.pendingStopSession && (
        <StopStreamModal
          session={props.pendingStopSession}
          error={props.stopError}
          busy={props.stopBusy}
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
