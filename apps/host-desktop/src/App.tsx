import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getTranslation, type SupportedLanguage } from "@leftcar/ui-tokens";
import { formatHostAddress } from "./hostState";
import PairingPanel from "./PairingPanel";
import Indicator from "./Indicator";
import PendingApprovalSection, {
  type PendingPairingView,
} from "./pairing/PendingApprovalSection";
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

function useDashboardLanguage() {
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

  return { language, toggleLanguage };
}

function useDashboardTheme() {
  const [theme, setTheme] = useState<ThemeMode>(() => {
    return (localStorage.getItem("leftcar_theme") as ThemeMode) || "system";
  });

  useEffect(() => {
    const root = document.documentElement;
    if (theme === "system") {
      root.removeAttribute("data-theme");
    } else {
      root.setAttribute("data-theme", theme);
    }
    localStorage.setItem("leftcar_theme", theme);
  }, [theme]);

  const toggleTheme = useCallback(() => {
    setTheme((prev) => {
      if (prev === "system") return "light";
      if (prev === "light") return "dark";
      return "system";
    });
  }, []);

  return { theme, toggleTheme };
}

function useCopyToast() {
  const [copiedToast, setCopiedToast] = useState(false);
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
    };
  }, []);

  const copyText = useCallback((text: string, onError: () => void) => {
    navigator.clipboard.writeText(text).then(
      () => {
        if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
        setCopiedToast(true);
        copiedTimerRef.current = setTimeout(() => {
          setCopiedToast(false);
          copiedTimerRef.current = null;
        }, 2000);
      },
      onError,
    );
  }, []);

  return { copiedToast, copyText };
}

function useDashboardPendingPairings({
  ready,
  paused,
  onApproved,
  onError,
}: {
  ready: boolean;
  paused: boolean;
  onApproved: () => void;
  onError: () => void;
}) {
  const [pendingPairings, setPendingPairings] = useState<PendingPairingView[]>(
    [],
  );
  const [pendingDecidingId, setPendingDecidingId] = useState<string | null>(
    null,
  );

  useEffect(() => {
    if (!ready || paused) return;
    let active = true;
    let inFlight = false;
    const poll = () => {
      if (document.hidden || inFlight) return;
      inFlight = true;
      void invoke<PendingPairingView[]>("list_pending_pairings")
        .then((list) => {
          if (active) setPendingPairings(Array.isArray(list) ? list : []);
        })
        .catch(() => {})
        .finally(() => {
          inFlight = false;
        });
    };
    poll();
    const timer = setInterval(poll, 2000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [ready, paused]);

  const decidePendingPairing = useCallback(
    async (
      command: "approve_pending_pairing" | "reject_pending_pairing",
      offerId: string,
    ) => {
      if (pendingDecidingId !== null) return;
      setPendingDecidingId(offerId);
      try {
        await invoke(command, { offerId });
        setPendingPairings((current) =>
          current.filter((request) => request.offer_id !== offerId),
        );
        if (command === "approve_pending_pairing") onApproved();
      } catch {
        onError();
      } finally {
        setPendingDecidingId(null);
      }
    },
    [pendingDecidingId, onApproved, onError],
  );

  return { pendingPairings, pendingDecidingId, decidePendingPairing };
}

function useDashboardShortcuts({
  disabled,
  onTogglePairing,
  onToggleHelp,
  onToggleSettings,
  onRefresh,
}: {
  disabled: boolean;
  onTogglePairing: () => void;
  onToggleHelp: () => void;
  onToggleSettings: () => void;
  onRefresh: () => void;
}) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (disabled) return;
      if (
        e.target instanceof HTMLElement &&
        (e.target.tagName === "INPUT" ||
          e.target.tagName === "SELECT" ||
          e.target.tagName === "TEXTAREA" ||
          e.target.isContentEditable)
      ) {
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "p") {
        e.preventDefault();
        onTogglePairing();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "h") {
        e.preventDefault();
        onToggleHelp();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
        e.preventDefault();
        onRefresh();
      } else if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        e.preventDefault();
        onToggleSettings();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [disabled, onTogglePairing, onToggleHelp, onToggleSettings, onRefresh]);
}

function Dashboard() {
  const { language, toggleLanguage } = useDashboardLanguage();
  const { theme, toggleTheme } = useDashboardTheme();
  const { copiedToast, copyText } = useCopyToast();
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
  } = useHostStatus(t, language);

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

  const onPendingApproved = useCallback(() => {
    void refresh();
  }, [refresh]);
  const onPendingError = useCallback(() => {
    setInputActionError(t.host.connectionCheckError);
  }, [setInputActionError, t.host.connectionCheckError]);

  const { pendingPairings, pendingDecidingId, decidePendingPairing } =
    useDashboardPendingPairings({
      ready,
      paused: showPairingModal,
      onApproved: onPendingApproved,
      onError: onPendingError,
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

  const togglePairingModal = useCallback(() => {
    setShowHelpModal(false);
    setShowSettingsModal(false);
    setShowPairingModal((prev) => !prev);
  }, []);
  const toggleHelpModal = useCallback(() => {
    setShowPairingModal(false);
    setShowSettingsModal(false);
    setShowHelpModal((prev) => !prev);
  }, []);
  const toggleSettingsModal = useCallback(() => {
    setShowPairingModal(false);
    setShowHelpModal(false);
    setShowSettingsModal((prev) => !prev);
  }, []);
  const handleShortcutRefresh = useCallback(() => {
    void refresh();
  }, [refresh]);

  useDashboardShortcuts({
    disabled: pendingStopSession !== null,
    onTogglePairing: togglePairingModal,
    onToggleHelp: toggleHelpModal,
    onToggleSettings: toggleSettingsModal,
    onRefresh: handleShortcutRefresh,
  });

  const copyAddressInfo = () => {
    copyText(formatHostAddress(lanIp, controlPort), () =>
      setInputActionError(t.host.copyFailed),
    );
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
        onOpenPairing={() => setShowPairingModal(true)}
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
          onRefresh={() => {
            setInputActionError(null);
            void refresh();
          }}
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

        {!showPairingModal && pendingPairings.length > 0 && (
          <PendingApprovalSection
            requests={pendingPairings}
            busyId={pendingDecidingId}
            onApprove={(offerId) =>
              void decidePendingPairing("approve_pending_pairing", offerId)
            }
            onDeny={(offerId) =>
              void decidePendingPairing("reject_pending_pairing", offerId)
            }
            t={t}
          />
        )}

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
