import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getTranslation, type SupportedLanguage } from "@leftcar/ui-tokens";
import { formatHostAddress } from "./hostState";
import PairingPanel from "./PairingPanel";
import Indicator from "./Indicator";
import {
  Curtain,
  useClipboardShare,
  usePrivacySettings,
  useStreamingBadge,
  useWanAccess,
} from "./Privacy";
import { useHostStatus } from "./hooks/useHostStatus";
import { useSessionActions } from "./hooks/useSessionActions";
import { DashboardHeader, type ThemeMode } from "./components/DashboardHeader";
import { DashboardFooter } from "./components/DashboardFooter";
import { IdleStudioView } from "./components/IdleStudioView";
import { StreamsListView } from "./components/StreamsListView";
import {
  SystemAlertBanners,
  TerminationBanner,
} from "./components/SystemAlertBanners";
import { DashboardModals } from "./modals/DashboardModals";

export default function App() {
  if (window.location.hash === "#/pairing") {
    return (
      <div className="h-dvh w-full overflow-y-auto bg-canvas p-4">
        <PairingPanel />
      </div>
    );
  }

  // "보고 있음" 배지 창(U4a) — 캡처되는 화면 위에 연결 중임을 드러낸다.
  if (window.location.hash.startsWith("#/indicator")) {
    return <Indicator />;
  }

  // 프라이버시 커튼 창 — 스트리밍 중 물리 화면을 검게 가린다.
  if (window.location.hash.startsWith("#/curtain")) {
    return <Curtain />;
  }

  return <Dashboard />;
}

function useIndicatorWindow(isStreaming: boolean) {
  useEffect(() => {
    let cancelled = false;
    void WebviewWindow.getByLabel("indicator").then((indicator) => {
      if (!indicator || cancelled) return;
      if (isStreaming) void indicator.show();
      else void indicator.hide();
    });
    return () => {
      cancelled = true;
    };
  }, [isStreaming]);
}

function Dashboard() {
  const [language, setLanguage] = useState<SupportedLanguage>(() => {
    const saved = localStorage.getItem(
      "leftcar_lang",
    ) as SupportedLanguage | null;
    if (saved === "ko" || saved === "en") return saved;
    const navLang = navigator.language?.toLowerCase() || "ko";
    return navLang.startsWith("en") ? "en" : "ko";
  });

  useEffect(() => {
    localStorage.setItem("leftcar_lang", language);
    document.documentElement.lang = language;
    void invoke("set_language", { language }).catch(() => {});
  }, [language]);

  const toggleLanguage = useCallback(() => {
    setLanguage((prev) => (prev === "ko" ? "en" : "ko"));
  }, []);

  const t = getTranslation(language);

  const {
    sessions,
    inputRequests,
    terminationNotice,
    dismissTerminationNotice,
    error,
    ready,
    inputPermission,
    screenPermission,
    platform,
    controlPort,
    lanIp,
    refresh,
  } = useHostStatus(t);

  const {
    inputActionError,
    stopError,
    setInputActionError,
    inputBusy,
    qualityBusy,
    actionsBusy,
    actionErrors,
    retrySessionAction,
    pendingStopSession,
    setPendingStopSession,
    openAccessibilitySettings,
    requestInputPermission,
    toggleSessionInput,
    setSessionQuality,
    forceStopSession,
  } = useSessionActions(t, refresh);

  const [showInspector, setShowInspector] = useState(false);
  const [showPairingModal, setShowPairingModal] = useState(false);
  const [showHelpModal, setShowHelpModal] = useState(false);
  const [showSettingsModal, setShowSettingsModal] = useState(false);
  const [copiedToast, setCopiedToast] = useState(false);
  const [theme, setTheme] = useState<ThemeMode>(() => {
    return (localStorage.getItem("leftcar_theme") as ThemeMode) || "system";
  });

  const isStreaming = sessions.length > 0;
  useIndicatorWindow(isStreaming);

  const {
    clipboardShare,
    toggleClipboardShare,
    pending: clipboardPending,
    ready: clipboardReady,
    error: clipboardError,
    retryClipboard,
  } = useClipboardShare();
  const {
    privacyCurtain,
    togglePrivacyCurtain,
    curtainPending,
    curtainReady,
    curtainError,
    retryCurtain,
  } = usePrivacySettings();
  const {
    streamingBadge,
    toggleStreamingBadge,
    pending: badgePending,
    ready: badgeReady,
    error: badgeError,
    retryBadge,
  } = useStreamingBadge();
  const {
    wanAccess,
    toggleWanAccess,
    pending: wanPending,
    ready: wanReady,
    error: wanError,
    retryWan,
  } = useWanAccess();

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
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "p") {
        e.preventDefault();
        setShowPairingModal((prev) => !prev);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "h") {
        e.preventDefault();
        setShowHelpModal((prev) => !prev);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
        e.preventDefault();
        void refresh();
      } else if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        e.preventDefault();
        setShowSettingsModal((prev) => !prev);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [refresh]);

  const copyAddressInfo = () => {
    navigator.clipboard.writeText(formatHostAddress(lanIp, controlPort)).then(
      () => {
        setCopiedToast(true);
        setTimeout(() => setCopiedToast(false), 2000);
      },
      () => setInputActionError(t.host.copyFailed),
    );
  };

  const toggleTheme = () => {
    setTheme((prev) => {
      if (prev === "system") return "light";
      if (prev === "light") return "dark";
      return "system";
    });
  };

  const themeLabel = {
    light: t.common.themeLight,
    dark: t.common.themeDark,
    system: t.common.themeSystem,
  }[theme];

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-canvas">
      <DashboardHeader
        isStreaming={isStreaming}
        sessionCount={sessions.length}
        themeMode={theme}
        themeLabel={themeLabel}
        language={language}
        t={t}
        onHelp={() => setShowHelpModal(true)}
        onTheme={toggleTheme}
        onToggleLanguage={toggleLanguage}
        onOpenSettings={() => setShowSettingsModal(true)}
      />

      <main className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto overflow-x-hidden p-4">
        {terminationNotice && (
          <TerminationBanner
            notice={terminationNotice}
            language={language}
            t={t}
            onDismiss={dismissTerminationNotice}
          />
        )}

        <SystemAlertBanners
          ready={ready}
          onRefresh={() => void refresh()}
          error={error}
          inputActionError={inputActionError}
          inputPermission={inputPermission}
          screenPermission={screenPermission}
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
            inputRequestSessions={inputRequests.map(
              (request) => request.session,
            )}
            t={t}
            onToggleInspector={() => setShowInspector((prev) => !prev)}
            onToggleInput={toggleSessionInput}
            onSetQuality={setSessionQuality}
            qualityBusy={qualityBusy}
            actionsBusy={actionsBusy}
            actionErrors={actionErrors}
            onRetryAction={retrySessionAction}
            onForceStop={setPendingStopSession}
          />
        ) : (
          <IdleStudioView
            screenReady={ready && (platform !== "macos" || screenPermission)}
            t={t}
            onOpenPairing={() => setShowPairingModal(true)}
          />
        )}
      </main>

      <DashboardFooter
        controlPort={controlPort}
        lanIp={lanIp}
        copiedToast={copiedToast}
        clipboardShare={clipboardShare}
        privacyCurtain={privacyCurtain}
        t={t}
        onCopyAddress={copyAddressInfo}
      />

      <DashboardModals
        showPairingModal={showPairingModal}
        showHelpModal={showHelpModal}
        showSettingsModal={showSettingsModal}
        pendingStopSession={pendingStopSession}
        stopError={stopError}
        stopBusy={
          pendingStopSession
            ? actionsBusy[pendingStopSession.session] === "stop"
            : false
        }
        inputBusy={inputBusy}
        language={language}
        platform={platform}
        t={t}
        clipboardShare={clipboardShare}
        privacyCurtain={privacyCurtain}
        streamingBadge={streamingBadge}
        clipboardReady={clipboardReady}
        curtainReady={curtainReady}
        badgeReady={badgeReady}
        wanReady={wanReady}
        clipboardPending={clipboardPending}
        curtainPending={curtainPending}
        badgePending={badgePending}
        wanAccess={wanAccess}
        wanPending={wanPending}
        wanError={wanError}
        clipboardError={clipboardError}
        curtainError={curtainError}
        badgeError={badgeError}
        onClosePairing={() => setShowPairingModal(false)}
        onCloseHelp={() => setShowHelpModal(false)}
        onCloseSettings={() => setShowSettingsModal(false)}
        onCancelStopSession={() => setPendingStopSession(null)}
        onConfirmStopSession={(session) => void forceStopSession(session)}
        onToggleClipboardShare={toggleClipboardShare}
        onTogglePrivacyCurtain={togglePrivacyCurtain}
        onToggleStreamingBadge={toggleStreamingBadge}
        onToggleWanAccess={toggleWanAccess}
        retryClipboard={retryClipboard}
        retryCurtain={retryCurtain}
        retryBadge={retryBadge}
        retryWan={retryWan}
      />
    </div>
  );
}
