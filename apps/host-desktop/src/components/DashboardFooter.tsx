import { Check, ClipboardCheck, Copy, EyeOff, Lock, Settings, ShieldAlert, ShieldCheck } from "lucide-react";
import { interpolate, type TranslationSchema } from "@leftcar/ui-tokens";
import { formatHostAddress, type HostSnapshotView } from "../hostState";

export interface DashboardFooterProps {
  controlPort: number;
  lanIp: string | null;
  copiedToast: boolean;
  inputPermission: boolean;
  clipboardShare: boolean;
  lockOnDisconnect: boolean;
  privacyCurtain: boolean;
  platform: HostSnapshotView["platform"];
  t: TranslationSchema;
  onCopyAddress: () => void;
  onRequestPermission: () => void;
  onOpenSettings: () => void;
}

export function DashboardFooter(props: DashboardFooterProps) {
  const { t } = props;
  const platformLabels: Record<HostSnapshotView["platform"], string> = {
    macos: t.common.myMac,
    windows: t.common.windowsPc,
    linux: t.common.myComputer,
  };
  const platformLabel = platformLabels[props.platform];
  const addressText = formatHostAddress(props.lanIp, props.controlPort);
  const hasActivePrivacyFeature = props.clipboardShare || props.lockOnDisconnect || props.privacyCurtain;

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

      <div className="footer-status-right">
        {hasActivePrivacyFeature && (
          <div className="footer-active-tags" aria-label={t.host.privacySection}>
            {props.clipboardShare && (
              <span className="footer-tag-pill" title={t.host.clipboardShareDesc}>
                <ClipboardCheck size={11} /> {t.host.clipboardShareLabel}
              </span>
            )}
            {props.lockOnDisconnect && (
              <span className="footer-tag-pill" title={t.host.lockOnDisconnectDesc}>
                <Lock size={11} /> {t.host.lockOnDisconnectLabel}
              </span>
            )}
            {props.privacyCurtain && (
              <span className="footer-tag-pill" title={t.host.privacyCurtainDesc}>
                <EyeOff size={11} /> {t.host.privacyCurtainLabel}
              </span>
            )}
          </div>
        )}
        <button
          type="button"
          className="footer-settings-btn"
          onClick={props.onOpenSettings}
          title={`${t.host.settingsTitle} (${t.host.shortcutSettings})`}
        >
          <Settings size={12} />
          <span>{t.common.settings}</span>
        </button>
        <span className="footer-timestamp">{platformLabel}</span>
      </div>
    </footer>
  );
}

export default DashboardFooter;
