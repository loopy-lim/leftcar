import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, Square, X } from "lucide-react";
import {
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import type { HostSnapshotView } from "../hostState";
import type { TerminationNotice } from "../streamTermination";
import { bannerAlertVariants, buttonVariants, terminationNoticeVariants } from "../lib/variants";
import type { HostErrorView } from "../hooks/useHostStatus";

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
        <p>{notice.detail}</p>
      </div>
      <button className={buttonVariants({ variant: "close" })} onClick={onDismiss} aria-label={t.common.close}>
        <X size={15} />
      </button>
    </section>
  );
}

export interface SystemAlertBannersProps {
  error: HostErrorView | null;
  inputActionError: string | null;
  inputPermission: boolean;
  screenPermission: boolean;
  platform: HostSnapshotView["platform"];
  inputBusy: number | "permission" | null;
  t: TranslationSchema;
  onRequestPermission: () => void;
  onOpenAccessibility: () => void;
}

export function SystemAlertBanners({
  error,
  inputActionError,
  inputPermission,
  screenPermission,
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
              <AlertTriangle size={16} /> {error.message}
            </span>
          </div>
          {platform === "macos" && error.kind === "remote-desktop-permission" && (
            <button
              className={buttonVariants({ variant: "ghost", size: "sm" })}
              onClick={() => void invoke("open_system_settings", { pane: "remote_desktop" })}
            >
              {t.host.openRemoteDesktopSettings}
            </button>
          )}
          {platform === "macos" && error.kind === "screen-permission" && (
            <button
              className={buttonVariants({ variant: "ghost", size: "sm" })}
              onClick={() => void invoke("open_system_settings", { pane: "screencapture" })}
            >
              {t.host.openScreenCaptureSettings}
            </button>
          )}
        </div>
      )}

      {platform === "macos" && !screenPermission && error?.kind !== "screen-permission" && (
        <div className={bannerAlertVariants({ tone: "warning" })}>
          <div className="banner-text">
            <strong>{t.host.screenPermBannerTitle}</strong>
            <p>{t.host.screenPermBannerDesc}</p>
          </div>
          <div className="banner-actions">
            <button
              className={buttonVariants({ variant: "primary", size: "sm" })}
              onClick={() => void invoke("open_system_settings", { pane: "screencapture" })}
            >
              {t.host.openScreenCaptureSettings}
            </button>
          </div>
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

export default SystemAlertBanners;
