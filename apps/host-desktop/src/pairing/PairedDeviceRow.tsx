import { Laptop, Smartphone } from "lucide-react";
import {
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import type { PairedDevice } from "../paired-device-state";
import { Button, Text } from "../ui/primitives";
function formatPairedAt(pairedAt: string, language: SupportedLanguage): string {
  const secs = Number(pairedAt.replace(/^unix:/, ""));
  if (!Number.isFinite(secs) || secs <= 0) return pairedAt;
  return new Date(secs * 1000).toLocaleString(
    language === "ko" ? "ko-KR" : "en-US",
    { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" },
  );
}
function grantsSummary(
  grants: PairedDevice["source_grants"],
  t: TranslationSchema,
): string {
  const screens = grants.reviewRequired
    ? t.host.deviceGrantsPending
    : grants.sourceIds.length > 0
      ? interpolate(t.host.deviceGrantsScreens, {
          count: grants.sourceIds.length,
        })
      : t.host.deviceGrantsNone;
  return grants.input ? `${screens} · ${t.host.deviceGrantsInputOn}` : screens;
}
interface PairedDeviceRowProps {
  disabled?: boolean;
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
  disabled = false,
  onRevoke,
  onApproveScreens,
}: PairedDeviceRowProps) {
  const isComputer =
    device.name.toLowerCase().includes("pc") ||
    device.name.toLowerCase().includes("mac");
  const busy = disabled || revoking !== null || approvingScreens !== null;
  return (
    <li className="flex flex-wrap items-center gap-3 p-4">
      <div className="flex min-w-0 flex-1 items-start gap-3">
        {isComputer ? (
          <Laptop size={20} className="shrink-0 text-muted" />
        ) : (
          <Smartphone size={20} className="shrink-0 text-muted" />
        )}
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
            <Text className="break-words font-semibold">{device.name}</Text>
            {device.connected && (
              <Text variant="caption" tone="muted">
                {t.host.deviceLiveLabel}
              </Text>
            )}
          </div>
          <Text variant="caption" tone="muted" className="block tabular-nums">
            {formatPairedAt(device.paired_at, language)}
          </Text>
          <Text variant="caption" tone="muted" className="block">
            {grantsSummary(device.source_grants, t)}
          </Text>
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        {device.source_grants.reviewRequired && (
          <Button
            variant="secondary"
            size="compact"
            busy={approvingScreens === device.device_id}
            disabled={busy}
            onClick={() => onApproveScreens(device)}
            title={t.host.sourceReviewRequired}
            aria-label={`${device.name}: ${t.host.deviceGrantsApprove}`}
          >
            {approvingScreens === device.device_id
              ? t.host.remoteInputProcessing
              : t.host.deviceGrantsApprove}
          </Button>
        )}
        <Button
          variant="ghost"
          size="compact"
          busy={revoking === device.device_id || revoking === "all"}
          disabled={busy}
          onClick={() => onRevoke(device.device_id)}
          aria-label={`${device.name}: ${t.host.revoke}`}
        >
          {revoking === device.device_id || revoking === "all"
            ? t.host.revoking
            : t.host.revoke}
        </Button>
      </div>
    </li>
  );
}
