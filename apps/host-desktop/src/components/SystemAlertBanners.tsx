import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, Square, X } from "lucide-react";
import {
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import type { HostSnapshotView } from "../hostState";
import type { TerminationNotice } from "../streamTermination";
import type { HostErrorView } from "../hooks/useHostStatus";
import { Button, Notice, Text } from "../ui/primitives";

export function TerminationBanner({
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
    <Notice
      tone={notice.tone === "danger" ? "error" : "info"}
      aria-label={notice.title}
      className="flex items-start gap-3"
    >
      <span className="shrink-0 pt-1" aria-hidden="true">
        {notice.tone === "danger" ? (
          <AlertTriangle size={16} />
        ) : (
          <Square size={14} fill="currentColor" />
        )}
      </span>
      <div className="min-w-0 flex-1 space-y-1">
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <Text className="font-semibold">{notice.title}</Text>
          <time
            className="text-caption text-muted tabular-nums"
            dateTime={notice.observedAt.toISOString()}
          >
            {notice.observedAt.toLocaleTimeString(
              language === "ko" ? "ko-KR" : "en-US",
            )}
          </time>
        </div>
        <Text variant="caption" tone="muted" className="block break-words">
          {notice.sourceName} ·{" "}
          {interpolate(t.host.connectedDevice, { addr: notice.viewerAddr })}
        </Text>
        <p className="text-caption text-muted">{notice.detail}</p>
      </div>
      <Button
        variant="ghost"
        size="icon"
        onClick={onDismiss}
        aria-label={t.common.close}
      >
        <X size={16} />
      </Button>
    </Notice>
  );
}

export interface SystemAlertBannersProps {
  ready: boolean;
  error: HostErrorView | null;
  inputActionError: string | null;
  inputPermission: boolean;
  screenPermission: boolean;
  platform: HostSnapshotView["platform"];
  inputBusy: number | "permission" | null;
  t: TranslationSchema;
  onRequestPermission: () => void;
  onOpenAccessibility: () => void;
  onRefresh: () => void;
}

interface SettingsLauncher {
  openingPane: string | null;
  onOpenPane: (pane: string) => void;
}
function HostStatusError({
  error,
  ready,
  platform,
  t,
  onRefresh,
  openingPane,
  onOpenPane,
}: Pick<
  SystemAlertBannersProps,
  "error" | "ready" | "platform" | "t" | "onRefresh"
> &
  SettingsLauncher) {
  if (!error) return null;
  const canOpenSettings = ready && platform === "macos";
  return (
    <Notice
      tone="error"
      className="flex flex-wrap items-center justify-between gap-3"
    >
      <Text className="flex min-w-0 items-start gap-2">
        <AlertTriangle size={16} className="shrink-0" />
        {error.message}
      </Text>
      <div className="flex flex-wrap gap-2">
        {canOpenSettings && error.kind === "remote-desktop-permission" && (
          <Button
            variant="secondary"
            busy={openingPane === "remote_desktop"}
            onClick={() => onOpenPane("remote_desktop")}
          >
            {t.host.openRemoteDesktopSettings}
          </Button>
        )}
        {canOpenSettings && error.kind === "screen-permission" && (
          <Button
            variant="secondary"
            busy={openingPane === "screencapture"}
            onClick={() => onOpenPane("screencapture")}
          >
            {t.host.openScreenCaptureSettings}
          </Button>
        )}
        <Button variant="secondary" onClick={onRefresh}>
          {t.common.retry}
        </Button>
      </div>
    </Notice>
  );
}
function ScreenPermissionNotice({
  ready,
  platform,
  screenPermission,
  error,
  t,
  openingPane,
  onOpenPane,
}: Pick<
  SystemAlertBannersProps,
  "ready" | "platform" | "screenPermission" | "error" | "t"
> &
  SettingsLauncher) {
  if (
    !ready ||
    platform !== "macos" ||
    screenPermission ||
    error?.kind === "screen-permission"
  )
    return null;
  return (
    <Notice className="flex flex-wrap items-center justify-between gap-3">
      <div className="min-w-0 flex-1">
        <Text className="block font-semibold">{t.host.setupScreenStep}</Text>
        <p className="text-caption text-muted mt-1">
          {t.host.screenPermBannerDesc}
        </p>
      </div>
      <Button
        busy={openingPane === "screencapture"}
        onClick={() => onOpenPane("screencapture")}
      >
        {t.host.openScreenCaptureSettings}
      </Button>
    </Notice>
  );
}
function InputPermissionNotice({
  ready,
  platform,
  inputPermission,
  inputBusy,
  t,
  onRequestPermission,
  onOpenAccessibility,
}: Pick<
  SystemAlertBannersProps,
  | "ready"
  | "platform"
  | "inputPermission"
  | "inputBusy"
  | "t"
  | "onRequestPermission"
  | "onOpenAccessibility"
>) {
  if (!ready || platform !== "macos" || inputPermission) return null;
  const busy = inputBusy === "permission";
  return (
    <Notice className="flex flex-wrap items-center justify-between gap-3">
      <div className="min-w-0 flex-1">
        <Text className="block font-semibold">
          {t.host.inputPermBannerTitle}
        </Text>
        <p className="text-caption text-muted mt-1">
          {t.host.inputPermBannerDesc}
        </p>
        <p className="text-caption text-muted mt-1">
          {t.host.inputOptionalHint}
        </p>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button variant="secondary" busy={busy} onClick={onRequestPermission}>
          {busy ? t.host.checkingPerm : t.host.btnGrantPerm}
        </Button>
        <Button variant="ghost" onClick={onOpenAccessibility}>
          {t.host.btnOpenSettings}
        </Button>
      </div>
    </Notice>
  );
}
export function SystemAlertBanners(props: SystemAlertBannersProps) {
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [openingPane, setOpeningPane] = useState<string | null>(null);
  const lastPane = useRef("screencapture");
  const openPane = async (pane: string) => {
    lastPane.current = pane;
    setOpeningPane(pane);
    setSettingsError(null);
    try {
      await invoke("open_system_settings", { pane });
    } catch (cause) {
      setSettingsError(
        `${props.t.host.connectionCheckError} ${String(cause instanceof Error ? cause.message : cause)}`,
      );
    } finally {
      setOpeningPane(null);
    }
  };
  if (!props.ready && !props.error)
    return <Notice>{props.t.host.checkingHost}</Notice>;
  return (
    <>
      <HostStatusError
        {...props}
        openingPane={openingPane}
        onOpenPane={(pane) => void openPane(pane)}
      />
      <ScreenPermissionNotice
        {...props}
        openingPane={openingPane}
        onOpenPane={(pane) => void openPane(pane)}
      />
      {props.inputActionError && (
        <Notice
          tone="error"
          className="flex flex-wrap items-center justify-between gap-3"
        >
          <Text>{props.inputActionError}</Text>
          <Button variant="secondary" onClick={props.onRefresh}>
            {props.t.common.retry}
          </Button>
        </Notice>
      )}
      {settingsError && (
        <Notice
          tone="error"
          className="flex flex-wrap items-center justify-between gap-3"
        >
          <Text>{settingsError}</Text>
          <Button
            variant="secondary"
            busy={openingPane !== null}
            onClick={() => void openPane(lastPane.current)}
          >
            {props.t.common.retry}
          </Button>
        </Notice>
      )}
      <InputPermissionNotice {...props} />
    </>
  );
}
export default SystemAlertBanners;
