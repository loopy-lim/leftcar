import { useId, useState } from "react";
import type { PairedDevice } from "./paired-device-state";
import { usePairingModel } from "./pairing/usePairingModel";
import { Check, ChevronDown, Copy } from "lucide-react";
import {
  cn,
  getTranslation,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import { formatHostAddress } from "./hostState";
import RevokeConfirmDialog from "./RevokeConfirmDialog";
import PairingQrCard from "./pairing/PairingQrCard";
import PendingApprovalSection from "./pairing/PendingApprovalSection";
import PairedDeviceRow from "./pairing/PairedDeviceRow";
import { Button, Notice, Surface, Text } from "./ui/primitives";
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
  const panelId = useId();
  return (
    <Surface variant="inset" className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Text className="font-semibold">{t.host.remoteReady}</Text>
        <Button
          variant="ghost"
          size="compact"
          aria-expanded={showAdvanced}
          aria-controls={panelId}
          onClick={() => setShowAdvanced((previous) => !previous)}
        >
          {t.host.advancedNetworkToggle}
          <ChevronDown size={16} className={cn(showAdvanced && "rotate-180")} />
        </Button>
      </div>
      <Text variant="caption" tone="muted" className="block">
        {t.host.remoteReadyDesc}
      </Text>
      {showAdvanced && (
        <div
          id={panelId}
          className="flex flex-wrap items-center justify-between gap-2 border-t border-outline pt-2"
        >
          <Text variant="caption" tone="muted">
            {t.host.localLanAddress}:{" "}
            <Text variant="code" className="break-all">
              {formatHostAddress(lanIp, controlPort)}
            </Text>
          </Text>
          <Button
            variant="secondary"
            size="compact"
            onClick={onCopyIp}
            aria-label={t.host.copyAddress}
          >
            {copiedIp ? <Check size={16} /> : <Copy size={16} />}
            {copiedIp ? t.host.copied : t.host.copyAddress}
          </Button>
          {copiedIp && (
            <Text variant="caption" role="status">
              {t.host.copied}
            </Text>
          )}
        </div>
      )}
    </Surface>
  );
}
function PairedDevicesSection({
  devices,
  disabled,
  revoking,
  approvingScreens,
  language,
  t,
  onRevoke,
  onRevokeAll,
  onApproveScreens,
}: {
  devices: PairedDevice[];
  disabled: boolean;
  revoking: string | null;
  approvingScreens: string | null;
  language: SupportedLanguage;
  t: TranslationSchema;
  onRevoke: (id: string) => void;
  onRevokeAll: () => void;
  onApproveScreens: (device: PairedDevice) => void;
}) {
  return (
    <section className="space-y-2" aria-label={t.host.pairedDevicesSection}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-body text-ink flex items-baseline gap-2 font-semibold">
          {t.host.pairedDevicesSection}
          <Text variant="code" tone="muted">
            {devices.length}
          </Text>
        </h3>
        {devices.length > 0 && (
          <Button
            variant="ghost"
            size="compact"
            disabled={
              disabled || revoking !== null || approvingScreens !== null
            }
            onClick={onRevokeAll}
          >
            {t.host.revokeAll}
          </Button>
        )}
      </div>
      {devices.length > 0 ? (
        <Surface variant="card" className="p-0">
          <ul className="divide-y divide-outline">
            {devices.map((device) => (
              <PairedDeviceRow
                key={device.source_grants.credentialId}
                device={device}
                language={language}
                t={t}
                revoking={revoking}
                approvingScreens={approvingScreens}
                disabled={disabled}
                onRevoke={onRevoke}
                onApproveScreens={onApproveScreens}
              />
            ))}
          </ul>
        </Surface>
      ) : (
        <Text variant="caption" tone="muted">
          {t.host.noPairedDevices}
        </Text>
      )}
    </section>
  );
}

export default function PairingPanel({
  language: propLanguage,
}: {
  language?: SupportedLanguage;
}) {
  const language =
    propLanguage ??
    (localStorage.getItem("leftcar_lang") === "en" ? "en" : "ko");
  const t = getTranslation(language);
  const {
    session,
    starting,
    cancelling,
    devices,
    pendingRequests,
    decidingId,
    error,
    pairedState,
    revokeError,
    success,
    devicesReady,
    devicesError,
    pendingError,
    revoking,
    approvingScreens,
    revokeConfirm,
    copiedCode,
    copiedIp,
    lanIp,
    controlPort,
    expired,
    remainingMs,
    progressPercent,
    setRevokeConfirm,
    setRevokeError,
    revoke,
    revokeAll,
    refreshDevices,
    refreshPending,
    approveRequest,
    denyRequest,
    startPairing,
    cancelPairing,
    copyCode,
    copyIp,
    approveScreens,
  } = usePairingModel(language);
  return (
    <div className="space-y-4">
      <Text tone="muted" className="block">
        {t.host.pairingPanelGuide}
      </Text>
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
        <Notice tone="error">
          <Text>
            {[error, pairedState.administrativeError]
              .filter(Boolean)
              .join(" · ")}
          </Text>
        </Notice>
      )}
      {revokeConfirm && (
        <RevokeConfirmDialog
          confirm={revokeConfirm}
          revoking={revoking}
          error={revokeError}
          t={t}
          onCancel={() => {
            if (!revoking) setRevokeConfirm(null);
          }}
          onConfirm={() => {
            const deviceId = revokeConfirm.deviceId;
            if (deviceId) void revoke(deviceId);
            else void revokeAll();
          }}
        />
      )}
      {success && <Notice>{success}</Notice>}
      {devicesError && (
        <Notice
          tone="error"
          className="flex flex-wrap items-center justify-between gap-2"
        >
          <Text>{devicesError}</Text>
          <Button
            variant="secondary"
            size="compact"
            onClick={() => void refreshDevices()}
          >
            {t.common.retry}
          </Button>
        </Notice>
      )}
      {pendingError && (
        <Notice
          tone="error"
          className="flex flex-wrap items-center justify-between gap-2"
        >
          <Text>{pendingError}</Text>
          <Button
            variant="secondary"
            size="compact"
            onClick={() => void refreshPending()}
          >
            {t.common.retry}
          </Button>
        </Notice>
      )}
      {!devicesReady && !devicesError && (
        <Notice>{t.host.deviceListLoading}</Notice>
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
        cancelling={cancelling}
        remainingMs={remainingMs}
        progressPercent={progressPercent}
        copiedCode={copiedCode}
        t={t}
        onStartPairing={startPairing}
        onCancelPairing={cancelPairing}
        onCopyCode={copyCode}
      />
      {(devicesReady || pairedState.revision >= 0 || devices.length > 0) && (
        <PairedDevicesSection
          devices={devices}
          disabled={false}
          revoking={revoking}
          approvingScreens={approvingScreens}
          language={language}
          t={t}
          onRevoke={(deviceId) => {
            setRevokeError(null);
            setRevokeConfirm({
              deviceId,
              name:
                devices.find((device) => device.device_id === deviceId)?.name ??
                null,
            });
          }}
          onRevokeAll={() => {
            setRevokeError(null);
            setRevokeConfirm({ deviceId: null, name: null });
          }}
          onApproveScreens={(device) => void approveScreens(device)}
        />
      )}
    </div>
  );
}
