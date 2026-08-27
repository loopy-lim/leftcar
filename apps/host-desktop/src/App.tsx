import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronUp,
  Copy,
  Info,
  Laptop,
  Monitor,
  Moon,
  QrCode,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
  Square,
  Sun,
  Tv,
  X,
} from "lucide-react";
import { trayStatus, type HostSnapshotView } from "./hostState";
import SessionInspector from "./SessionInspector";
import type { SessionRow } from "./sessionTypes";
import PairingPanel from "./PairingPanel";
import {
  createTerminationNotice,
  isTerminalSession,
  type TerminationNotice,
} from "./streamTermination";

function hostErrorMessage(cause: unknown): string {
  const message = String(cause instanceof Error ? cause.message : cause).toLowerCase();
  if (message.includes("permission") || message.includes("not authorized")) {
    return "화면 공유 권한이 필요합니다. 시스템 설정에서 Leftcar를 허용해 주세요.";
  }
  if (message.includes("no lan interface")) {
    return "연결할 네트워크를 찾지 못했습니다. Wi-Fi 또는 Tailscale 연결을 확인해 주세요.";
  }
  if (message.includes("invoke") || message.includes("initialization")) {
    return "앱 서비스를 시작할 수 없습니다. Leftcar를 완전히 종료한 뒤 다시 실행해 주세요.";
  }
  return "연결 상태를 확인하지 못했습니다. 잠시 후 새로고침해 주세요.";
}

interface StatusView {
  sessions: SessionRow[];
}

type ThemeMode = "system" | "light" | "dark";

export default function App() {
  if (window.location.hash === "#/pairing") {
    return (
      <div className="pairing-standalone-view">
        <PairingPanel />
      </div>
    );
  }

  return <Dashboard />;
}

function useHostStatus() {
  const [banner, setBanner] = useState("Leftcar");
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [terminationNotice, setTerminationNotice] = useState<TerminationNotice | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [inputPermission, setInputPermission] = useState(false);
  const [platform, setPlatform] = useState<HostSnapshotView["platform"]>("macos");
  const [controlPort, setControlPort] = useState(7777);
  const [lastUpdated, setLastUpdated] = useState<Date>(new Date());
  const priorActiveSessions = useRef(new Map<number, SessionRow>());
  const seenTerminations = useRef(new Set<string>());
  const hasStatusSnapshot = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const [status, permission, hostPlatform, actualControlPort] = await Promise.all([
        invoke<StatusView>("get_status"),
        invoke<boolean>("get_input_permission"),
        invoke<HostSnapshotView["platform"]>("get_host_platform"),
        invoke<number>("get_control_port"),
      ]);
      const statusSessions = status.sessions || [];
      const activeSessions = statusSessions.filter((session) => !isTerminalSession(session));
      let nextTerminationNotice: TerminationNotice | null = null;

      for (const terminalSession of statusSessions.filter(isTerminalSession)) {
        const notice = createTerminationNotice(terminalSession);
        if (!seenTerminations.current.has(notice.key)) {
          seenTerminations.current.add(notice.key);
          nextTerminationNotice = notice;
        }
      }

      if (hasStatusSnapshot.current) {
        const reportedSessionIds = new Set(statusSessions.map((session) => session.session));
        for (const priorSession of priorActiveSessions.current.values()) {
          if (reportedSessionIds.has(priorSession.session)) continue;
          const notice = createTerminationNotice({
            ...priorSession,
            state: "stopped",
            error: null,
          });
          if (!seenTerminations.current.has(notice.key)) {
            seenTerminations.current.add(notice.key);
            nextTerminationNotice = notice;
          }
        }
      }

      priorActiveSessions.current = new Map(
        activeSessions.map((session) => [session.session, session]),
      );
      hasStatusSnapshot.current = true;
      setSessions(activeSessions);
      if (nextTerminationNotice) setTerminationNotice(nextTerminationNotice);
      setBanner(
        trayStatus({
          hostId: "local",
          platform: hostPlatform,
          pairingState: "connected",
          pairedDevices: [],
          approvedSources: [],
          activeStreamCount: activeSessions.length,
        } satisfies HostSnapshotView),
      );
      setError(null);
      setInputPermission(permission);
      setPlatform(hostPlatform);
      setControlPort(actualControlPort);
      setLastUpdated(new Date());
    } catch (cause) {
      setError(hostErrorMessage(cause));
    }
  }, []);

  useEffect(() => {
    const refreshWhenVisible = () => {
      if (!document.hidden) void refresh();
    };
    void refresh();
    const timer = setInterval(refreshWhenVisible, 2_000);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    window.addEventListener("focus", refreshWhenVisible);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
      window.removeEventListener("focus", refreshWhenVisible);
    };
  }, [refresh]);

  const dismissTerminationNotice = useCallback(() => setTerminationNotice(null), []);

  return {
    banner,
    sessions,
    terminationNotice,
    dismissTerminationNotice,
    error,
    inputPermission,
    platform,
    controlPort,
    lastUpdated,
    refresh,
  };
}

interface DashboardHeaderProps {
  isStreaming: boolean;
  sessionCount: number;
  themeMode: ThemeMode;
  themeLabel: string;
  onPair: () => void;
  onTheme: () => void;
  onRefresh: () => void;
}

function DashboardHeader({
  isStreaming,
  sessionCount,
  themeMode,
  themeLabel,
  onPair,
  onTheme,
  onRefresh,
}: DashboardHeaderProps) {
  return (
    <header className="host-header">
      <div className="host-header-left">
        <div className="host-logo-box">
          <Monitor size={17} strokeWidth={2.4} aria-hidden="true" />
        </div>
        <div className="host-title-group">
          <h1>Leftcar</h1>
          <span className="host-version-badge">내 컴퓨터</span>
        </div>
      </div>
      <div className="host-header-right">
        <div className={`host-status-pill ${isStreaming ? "pill-active" : "pill-idle"}`}>
          <span className="status-dot" />
          <span>{isStreaming ? `${sessionCount}개 화면 공유 중` : "연결 준비됨"}</span>
        </div>
        <button className="btn-primary" onClick={onPair} title="새 기기 연결 (⌘P)">
          <QrCode size={14} />
          <span>새 기기 연결</span>
          <span className="kbd-shortcut" style={{ marginLeft: 2, opacity: 0.85, background: "rgba(255,255,255,0.2)", color: "inherit", borderColor: "rgba(255,255,255,0.3)" }}>⌘P</span>
        </button>
        <button
          className="btn-icon"
          onClick={onTheme}
          title={`테마: ${themeLabel}`}
          aria-label={`테마 변경: 현재 ${themeLabel}`}
        >
          {themeMode === "light" ? <Sun size={15} /> : themeMode === "dark" ? <Moon size={15} /> : <Laptop size={15} />}
        </button>
        <button
          className="btn-icon"
          onClick={onRefresh}
          title="새로고침 (⌘R)"
          aria-label="연결 상태 새로고침"
        >
          <RefreshCw size={14} />
        </button>
      </div>
    </header>
  );
}

interface DashboardFooterProps {
  controlPort: number;
  copiedToast: boolean;
  inputPermission: boolean;
  platform: HostSnapshotView["platform"];
  lastUpdated: Date;
  onCopyPort: () => void;
  onRequestPermission: () => void;
}

function DashboardFooter(props: DashboardFooterProps) {
  return (
    <footer className="host-footer">
      <div className="footer-status-info">
        <button
          type="button"
          className="clickable-chip"
          onClick={props.onCopyPort}
          title="로컬 제어 포트 복사하기"
        >
          연결 포트: <strong>:{props.controlPort}</strong>{" "}
          {props.copiedToast ? (
            <span style={{ display: "inline-flex", alignItems: "center", gap: 3, fontWeight: 600 }}>
              <Check size={13} strokeWidth={2.5} /> 복사됨!
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
          title={props.inputPermission ? "원격 제어 활성화됨" : "클릭하여 권한 허용"}
        >
          원격 조작:{" "}
          {props.inputPermission ? (
            <strong style={{ display: "inline-flex", alignItems: "center", gap: 3 }}>
              <ShieldCheck size={13} /> 승인됨
            </strong>
          ) : (
            <strong style={{ display: "inline-flex", alignItems: "center", gap: 3 }}>
              <ShieldAlert size={13} /> 권한 필요
            </strong>
          )}
        </button>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span className="footer-timestamp">
          {props.platform === "macos" ? "Mac" : props.platform === "windows" ? "Windows PC" : "컴퓨터"} · 최근 확인: {props.lastUpdated.toLocaleTimeString("ko-KR")}
        </span>
      </div>
    </footer>
  );
}

function PairingModal({ onClose }: { onClose: () => void }) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-window" onClick={(event) => event.stopPropagation()}>
        <div className="modal-title-bar">
          <h3>새 기기 연결</h3>
          <button className="btn-close" onClick={onClose} aria-label="기기 연결 창 닫기">
            <X size={15} />
          </button>
        </div>
        <div className="modal-scroll-area">
          <PairingPanel />
        </div>
      </div>
    </div>
  );
}

function StopStreamModal({
  session,
  busy,
  onCancel,
  onConfirm,
}: {
  session: SessionRow;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <dialog
      open
      className="modal-overlay"
      aria-labelledby="stop-stream-title"
      onClose={onCancel}
    >
      <div className="modal-window stop-stream-modal">
        <div className="modal-title-bar">
          <h3 id="stop-stream-title">화면 공유를 종료할까요?</h3>
          <button className="btn-close" disabled={busy} onClick={onCancel} aria-label="종료 확인 창 닫기">
            <X size={15} />
          </button>
        </div>
        <div className="stop-stream-modal-body">
          <div className="stop-stream-target">
            <strong>{session.sourceName}</strong>
            <span>연결된 기기: {session.viewerAddr}</span>
          </div>
          <p className="stop-stream-summary">
            종료하면 다음 작업을 즉시 수행합니다.
          </p>
          <ul className="stop-stream-effects">
            <li>화면 공유와 영상 전송을 즉시 중지합니다.</li>
            <li>원격 조작을 끄고 눌려 있는 키와 마우스 버튼을 해제합니다.</li>
            <li>연결된 기기에 종료 사실을 알리고 연결 정보를 정리합니다.</li>
          </ul>
          <div className="stop-stream-actions">
            <button className="btn-ghost" disabled={busy} onClick={onCancel}>계속 공유</button>
            <button className="btn-danger" disabled={busy} onClick={onConfirm}>
              <Square size={13} fill="currentColor" />
              {busy ? "종료 중…" : "화면 공유 종료"}
            </button>
          </div>
        </div>
      </div>
    </dialog>
  );
}

function TerminationBanner({
  notice,
  onDismiss,
}: {
  notice: TerminationNotice;
  onDismiss: () => void;
}) {
  return (
    <section
      className={`termination-notice termination-${notice.tone}`}
      role={notice.tone === "danger" ? "alert" : "status"}
      aria-label="최근 화면 공유 종료 상태"
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
            {notice.observedAt.toLocaleTimeString("ko-KR")}
          </time>
        </div>
        <span className="termination-notice-target">
          {notice.sourceName} · 연결된 기기 {notice.viewerAddr}
        </span>
        <p><b>종료 이유:</b> {notice.detail}</p>
      </div>
      <button className="btn-close" onClick={onDismiss} aria-label="종료 상태 알림 닫기">
        <X size={15} />
      </button>
    </section>
  );
}

interface SystemAlertBannersProps {
  error: string | null;
  inputActionError: string | null;
  inputPermission: boolean;
  platform: HostSnapshotView["platform"];
  inputBusy: number | "permission" | null;
  onRequestPermission: () => void;
  onOpenAccessibility: () => void;
}

function SystemAlertBanners({
  error,
  inputActionError,
  inputPermission,
  platform,
  inputBusy,
  onRequestPermission,
  onOpenAccessibility,
}: SystemAlertBannersProps) {
  return (
    <>
      {error && (
        <div className="banner-alert banner-danger">
          <div className="banner-text">
            <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
              <AlertTriangle size={16} /> {error}
            </span>
          </div>
          {platform === "macos" && error.includes("Remote Desktop") && (
            <button
              className="btn-ghost btn-sm"
              onClick={() => void invoke("open_system_settings", { pane: "remote_desktop" })}
            >
              Remote Desktop 설정 열기
            </button>
          )}
          {platform === "macos" && error.includes("권한") && !error.includes("Remote Desktop") && (
            <button
              className="btn-ghost btn-sm"
              onClick={() => void invoke("open_system_settings", { pane: "screencapture" })}
            >
              화면 기록 설정 열기
            </button>
          )}
        </div>
      )}

      {inputActionError && (
        <div className="banner-alert banner-danger">
          <div className="banner-text">
            <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
              <AlertTriangle size={16} /> {inputActionError}
            </span>
          </div>
          {platform === "macos" && (
            <button className="btn-ghost btn-sm" onClick={onOpenAccessibility}>
              설정 열기
            </button>
          )}
        </div>
      )}

      {!inputPermission && platform === "macos" && (
        <div className="banner-alert banner-warning">
          <div className="banner-text">
            <strong>원격 조작 권한 필요</strong>
            <p>연결한 휴대폰이나 태블릿에서 마우스와 키보드를 사용하려면 손쉬운 사용 권한이 필요합니다.</p>
          </div>
          <div className="banner-actions">
            <button
              className="btn-primary btn-sm"
              disabled={inputBusy === "permission"}
              onClick={onRequestPermission}
            >
              {inputBusy === "permission" ? "확인 중…" : "권한 허용"}
            </button>
            <button
              className="btn-ghost btn-sm"
              onClick={onOpenAccessibility}
              title="macOS 손쉬운 사용 설정 열기"
            >
              설정 열기
            </button>
          </div>
        </div>
      )}
    </>
  );
}

interface IdleStudioViewProps {
  onOpenPairing: () => void;
}

function IdleStudioView({ onOpenPairing }: IdleStudioViewProps) {
  return (
    <div className="idle-center-container">
      <div className="idle-center-card">
        <div className="idle-center-icon-box">
          <Monitor size={26} strokeWidth={2} />
        </div>
        <div className="idle-center-text">
          <h2>기기 연결을 기다리는 중</h2>
          <p>
            휴대폰이나 태블릿에서 Leftcar Viewer 앱을 열고<br />
            이 컴퓨터를 선택하거나 주소를 입력한 뒤 연결 코드를 입력하세요.
          </p>
        </div>

        <button className="btn-primary btn-lg" onClick={onOpenPairing} title="새 기기 연결 (⌘P)">
          <QrCode size={15} />
          <span>연결 코드 만들기</span>
          <span className="kbd-shortcut" style={{ marginLeft: 4, background: "rgba(255,255,255,0.2)", color: "inherit", borderColor: "rgba(255,255,255,0.3)" }}>⌘P</span>
        </button>

        <span className="idle-center-hint">
          <Info size={12} />
          같은 Wi-Fi 또는 Tailscale 네트워크의 기기에서 연결할 수 있습니다
        </span>
      </div>
    </div>
  );
}

interface StreamsListViewProps {
  sessions: SessionRow[];
  inputPermission: boolean;
  inputBusy: number | "permission" | null;
  showInspector: boolean;
  onToggleInspector: () => void;
  onToggleInput: (session: SessionRow) => Promise<void>;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
  qualityBusy: number | null;
  onForceStop: (session: SessionRow) => void;
}

function StreamsListView({
  sessions,
  inputPermission,
  inputBusy,
  showInspector,
  onToggleInspector,
  onToggleInput,
  onSetQuality,
  qualityBusy,
  onForceStop,
}: StreamsListViewProps) {
  return (
    <div className="streams-section">
      <div className="streams-section-header">
        <div className="streams-header-left">
          <h2>공유 중인 화면 ({sessions.length})</h2>
          <span className="live-badge-pulse">
            <span className="status-dot" /> 공유 중
          </span>
        </div>
        <button
          className="btn-link"
          onClick={onToggleInspector}
          style={{ display: "inline-flex", alignItems: "center", gap: 4 }}
        >
          {showInspector ? (
            <>세부 지표 숨기기 <ChevronUp size={14} /></>
          ) : (
            <>세부 지표 보기 <ChevronDown size={14} /></>
          )}
        </button>
      </div>

      <div className="stream-cards-container">
        {sessions.map((session) => (
          <SessionCard
            key={session.session}
            session={session}
            inputPermission={inputPermission}
            inputBusy={inputBusy === session.session}
            showInspector={showInspector}
            onToggleInput={onToggleInput}
            onSetQuality={onSetQuality}
            qualityBusy={qualityBusy === session.session}
            onForceStop={onForceStop}
          />
        ))}
      </div>
    </div>
  );
}

function Dashboard() {
  const {
    sessions,
    terminationNotice,
    dismissTerminationNotice,
    error,
    inputPermission,
    platform,
    controlPort,
    lastUpdated,
    refresh,
  } = useHostStatus();
  const [inputActionError, setInputActionError] = useState<string | null>(null);
  const [inputBusy, setInputBusy] = useState<number | "permission" | null>(null);
  const [qualityBusy, setQualityBusy] = useState<number | null>(null);
  const [showInspector, setShowInspector] = useState(false);
  const [showPairingModal, setShowPairingModal] = useState(false);
  const [pendingStopSession, setPendingStopSession] = useState<SessionRow | null>(null);
  const [copiedToast, setCopiedToast] = useState(false);
  const [theme, setTheme] = useState<ThemeMode>(() => {
    return (localStorage.getItem("leftcar_theme") as ThemeMode) || "system";
  });

  const isStreaming = sessions.length > 0;

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
      if (e.key === "Escape") {
        setShowPairingModal(false);
        if (inputBusy === null) setPendingStopSession(null);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "p") {
        e.preventDefault();
        setShowPairingModal((prev) => !prev);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
        e.preventDefault();
        void refresh();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [inputBusy, refresh]);

  const openAccessibilitySettings = async () => {
    try {
      await invoke("open_system_settings", { pane: "accessibility" });
    } catch (cause) {
      setInputActionError(hostErrorMessage(cause));
    }
  };

  const requestInputPermission = async () => {
    setInputBusy("permission");
    try {
      const granted = await invoke<boolean>("request_input_permission");
      if (!granted) {
        await invoke("open_system_settings", { pane: "accessibility" }).catch(() => {});
        setInputActionError(
          "macOS 시스템 설정의 '개인정보 보호 및 보안 > 손쉬운 사용'에서 Leftcar Host를 허용해 주세요.",
        );
      } else {
        setInputActionError(null);
      }
      await refresh();
    } catch (cause) {
      setInputActionError(hostErrorMessage(cause));
    } finally {
      setInputBusy(null);
    }
  };

  const toggleSessionInput = async (session: SessionRow) => {
    setInputBusy(session.session);
    try {
      await invoke("set_session_input", {
        session: session.session,
        enabled: !session.inputEnabled,
      });
      setInputActionError(null);
      await refresh();
    } catch (cause) {
      setInputActionError(hostErrorMessage(cause));
    } finally {
      setInputBusy(null);
    }
  };

  const setSessionQuality = async (session: SessionRow, quality: number | null) => {
    setQualityBusy(session.session);
    try {
      await invoke("set_session_quality", {
        session: session.session,
        quality,
      });
      setInputActionError(null);
      await refresh();
    } catch (cause) {
      setInputActionError(hostErrorMessage(cause));
    } finally {
      setQualityBusy(null);
    }
  };

  const forceStopSession = async (session: SessionRow) => {
    setInputBusy(session.session);
    try {
      await invoke("force_stop_session", { session: session.session });
      setInputActionError(null);
      await refresh();
      setPendingStopSession(null);
    } catch (cause) {
      setInputActionError(hostErrorMessage(cause));
    } finally {
      setInputBusy(null);
    }
  };

  const copyPortInfo = () => {
    void navigator.clipboard.writeText(`:${controlPort}`);
    setCopiedToast(true);
    setTimeout(() => setCopiedToast(false), 2000);
  };

  const toggleTheme = () => {
    setTheme((prev) => {
      if (prev === "system") return "light";
      if (prev === "light") return "dark";
      return "system";
    });
  };

  const themeLabel = theme === "light" ? "라이트 모드" : theme === "dark" ? "다크 모드" : "시스템 동기화";

  return (
    <div className="host-window">
      <DashboardHeader
        isStreaming={isStreaming}
        sessionCount={sessions.length}
        themeMode={theme}
        themeLabel={themeLabel}
        onPair={() => setShowPairingModal(true)}
        onTheme={toggleTheme}
        onRefresh={() => void refresh()}
      />

      <main className="host-body">
        {terminationNotice && (
          <TerminationBanner notice={terminationNotice} onDismiss={dismissTerminationNotice} />
        )}

        <SystemAlertBanners
          error={error}
          inputActionError={inputActionError}
          inputPermission={inputPermission}
          platform={platform}
          inputBusy={inputBusy}
          onRequestPermission={requestInputPermission}
          onOpenAccessibility={openAccessibilitySettings}
        />

        {isStreaming ? (
          <StreamsListView
            sessions={sessions}
            inputPermission={inputPermission}
            inputBusy={inputBusy}
            showInspector={showInspector}
            onToggleInspector={() => setShowInspector((prev) => !prev)}
            onToggleInput={toggleSessionInput}
            onSetQuality={setSessionQuality}
            qualityBusy={qualityBusy}
            onForceStop={setPendingStopSession}
          />
        ) : (
          <IdleStudioView
            onOpenPairing={() => setShowPairingModal(true)}
          />
        )}
      </main>

      <DashboardFooter
        controlPort={controlPort}
        copiedToast={copiedToast}
        inputPermission={inputPermission}
        platform={platform}
        lastUpdated={lastUpdated}
        onCopyPort={copyPortInfo}
        onRequestPermission={requestInputPermission}
      />

      {showPairingModal && <PairingModal onClose={() => setShowPairingModal(false)} />}
      {pendingStopSession && (
        <StopStreamModal
          session={pendingStopSession}
          busy={inputBusy === pendingStopSession.session}
          onCancel={() => setPendingStopSession(null)}
          onConfirm={() => void forceStopSession(pendingStopSession)}
        />
      )}
    </div>
  );
}

interface SessionCardProps {
  session: SessionRow;
  inputPermission: boolean;
  inputBusy: boolean;
  showInspector: boolean;
  onToggleInput: (session: SessionRow) => Promise<void>;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
  qualityBusy: boolean;
  onForceStop: (session: SessionRow) => void;
}

function SessionCard({
  session,
  inputPermission,
  inputBusy,
  showInspector,
  onToggleInput,
  onSetQuality,
  qualityBusy,
  onForceStop,
}: SessionCardProps) {
  const bitrateMbps = session.kbps > 0 ? (session.kbps / 1000).toFixed(1) : "0.0";
  const encodeOutputFps = session.encodeOutputFps ?? session.fps;
  const transportLabel = session.mediaTransport === "usb"
    ? "USB (AOAP)"
    : session.mediaTransport === "udp"
      ? "Wi-Fi UDP"
      : session.mediaTransport || "확인 중";
  const qualitySupported = session.qualityHint != null;
  const qualityPercent = Math.round((session.qualityOverride ?? session.qualityHint ?? 0.5) * 100);

  return (
    <div className="stream-card-item">
      <div className="stream-card-top-row">
        <div className="stream-card-identity">
          <div className="stream-card-icon">
            <Tv size={20} strokeWidth={2} />
          </div>
          <div className="stream-card-name-group">
            <div className="stream-name-badge-row">
              <h3>{session.sourceName}</h3>
              <span className="session-tag">#{session.session}</span>
            </div>
            <span className="stream-card-target">연결된 기기: {session.viewerAddr}</span>
          </div>
        </div>

        <div className="stream-card-action">
          <button
            className={`btn-control-toggle ${session.inputEnabled ? "toggle-active" : ""}`}
            disabled={(!inputPermission && !session.inputEnabled) || session.state !== "running" || inputBusy}
            onClick={() => void onToggleInput(session)}
            title={session.inputEnabled ? "원격 마우스/키보드 입력 허용 중" : "원격 입력 켜기"}
          >
            {inputBusy
              ? "처리 중…"
              : session.inputEnabled
                ? "원격 조작 허용됨"
                : "원격 조작 끔"}
          </button>
          <button
            className="btn-stop-stream"
            disabled={inputBusy || qualityBusy}
            onClick={() => onForceStop(session)}
            title="이 화면 공유 종료"
            aria-label={`${session.sourceName} 화면 공유 종료`}
          >
            <Square size={12} fill="currentColor" />
            공유 종료
          </button>
        </div>
      </div>

      <div className="stream-card-metrics-grid">
        <div className="metric-card">
          <span className="metric-card-label">인코더 출력</span>
          <span className="metric-card-value font-emerald">
            <span className="signal-bars" aria-hidden="true">
              <span className="bar bar-1 active" />
              <span className="bar bar-2 active" />
              <span className="bar bar-3 active" />
            </span>
            {encodeOutputFps} FPS
          </span>
        </div>

        <div className="metric-card">
          <span className="metric-card-label">전송량</span>
          <span className="metric-card-value">{bitrateMbps} Mbps</span>
        </div>

        <div className="metric-card">
          <span className="metric-card-label">연결 상태</span>
          <span className="metric-card-value font-blue">
            {session.state === "running" ? "정상 연결" : "상태 확인 중"}
          </span>
        </div>

        <div className="metric-card">
          <span className="metric-card-label">전송 안정성</span>
          <span className="metric-card-value">
            {session.dropped ? (
              <span className="font-rose">놓친 화면 {session.dropped}개</span>
            ) : (
              <span className="font-emerald">안정적</span>
            )}
          </span>
        </div>
      </div>

      {showInspector && (
        <SessionInspector
          session={session}
          transportLabel={transportLabel}
          qualitySupported={qualitySupported}
          qualityPercent={qualityPercent}
          qualityBusy={qualityBusy}
          onSetQuality={onSetQuality}
        />
      )}

      <div className="stream-card-footer">
        <div className="stream-termination-policy">
          <Info size={12} />
          <span>직접 종료하거나 연결된 기기가 6초 동안 응답하지 않으면 안전하게 연결을 정리합니다.</span>
        </div>
        <span style={{ fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--text-dim)" }}>
          {session.state === "running" ? "화면 공유 중" : "상태 확인 중"}
        </span>
      </div>
    </div>
  );
}
