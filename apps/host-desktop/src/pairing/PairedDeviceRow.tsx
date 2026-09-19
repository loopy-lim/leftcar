import { Laptop, Smartphone } from "lucide-react";
import { interpolate, type SupportedLanguage, type TranslationSchema } from "@leftcar/ui-tokens";
import { buttonVariants } from "../lib/variants";
import type { PairedDevice } from "../paired-device-state";

function formatPairedAt(pairedAt: string, language: SupportedLanguage): string {
  const secs = Number(pairedAt.replace(/^unix:/, ""));
  if (!Number.isFinite(secs) || secs <= 0) return pairedAt;
  return new Date(secs * 1000).toLocaleString(
    language === "ko" ? "ko-KR" : "en-US",
    {
      month: "numeric",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    },
  );
}

function grantsSummary(grants: PairedDevice["source_grants"], t: TranslationSchema): string {
  const screens = grants.reviewRequired
    ? t.host.deviceGrantsPending
    : grants.sourceIds.length > 0
      ? interpolate(t.host.deviceGrantsScreens, { count: grants.sourceIds.length })
      : t.host.deviceGrantsNone;
  return grants.input ? `${screens} · ${t.host.deviceGrantsInputOn}` : screens;
}

interface PairedDeviceRowProps {
  device: PairedDevice;
  language: SupportedLanguage;
  t: TranslationSchema;
  revoking: string | null;
  approvingScreens: string | null;
  onRevoke: (deviceId: string) => void;
  onApproveScreens: (device: PairedDevice) => void;
}

export default function PairedDeviceRow({
  device,
  language,
  t,
  revoking,
  approvingScreens,
  onRevoke,
  onApproveScreens,
}: PairedDeviceRowProps) {
  const isComputer =
    device.name.toLowerCase().includes("pc") ||
    device.name.toLowerCase().includes("mac");

  return (
    <div className="device-row-item">
      <div className="device-row-left">
        <div className="device-icon-box">
          {isComputer ? (
            <Laptop size={18} strokeWidth={2} />
          ) : (
            <Smartphone size={18} strokeWidth={2} />
          )}
        </div>
        <div className="device-row-main">
          <div className="device-row-header">
            <span className="device-row-name">{device.name}</span>
            {device.connected && (
              <span className="device-live-badge">
                <span
                  style={{
                    width: 6,
                    height: 6,
                    borderRadius: "50%",
                    background: "currentColor",
                  }}
                />
                {t.host.deviceLiveLabel}
              </span>
            )}
          </div>
          <div className="device-row-meta">
            <span className="device-row-date">
              {formatPairedAt(device.paired_at, language)}
            </span>
            <span className="device-row-date">
              {grantsSummary(device.source_grants, t)}
            </span>
          </div>
        </div>
      </div>

      <div className="device-row-actions">
        {device.source_grants.reviewRequired && (
          <button
            type="button"
            onClick={() => onApproveScreens(device)}
            className={buttonVariants({ variant: "outline", size: "sm" })}
            disabled={approvingScreens === device.device_id}
            title={t.host.sourceReviewRequired}
          >
            {t.host.deviceGrantsApprove}
          </button>
        )}
        <button
          type="button"
          onClick={() => onRevoke(device.device_id)}
          className={buttonVariants({ variant: "outlineDanger", size: "sm" })}
          disabled={revoking === device.device_id}
          title={t.host.revoke}
        >
          {t.host.revoke}
        </button>
      </div>
    </div>
  );
}
