import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { trayStatus, type HostSnapshotView } from "./hostState";
import PairingPanel from "./PairingPanel";

interface SessionRow {
  session: number;
  sourceIndex: number;
  sourceName: string;
  viewerAddr: string;
  state: string;
  fps: number;
  kbps: number;
  fpsTarget?: number;
  inputEnabled: boolean;
  inputRateHz: number;
  dropped?: number;
  networkDropped?: number;
  captureQueueDropped?: number;
  captureToEncodeUs?: number;
  maxCaptureToEncodeUs?: number;
  captureQueueWaitUs?: number;
  maxCaptureQueueWaitUs?: number;
  encodeOutputUs?: number;
  maxEncodeOutputUs?: number;
  sendBlockUs?: number;
  maxSendBlockUs?: number;
  pendingFrame?: number;
  frames?: number;
  bytes?: number;
  captureBackend?: string;
  mediaTransport?: string;
  firstCaptureMs?: number;
  firstEncodeMs?: number;
  firstSendMs?: number;
  currentBitrate?: number;
  captureIntervalP95Us?: number;
  captureToEncodeP95Us?: number;
  captureQueueWaitP95Us?: number;
  encodeOutputP95Us?: number;
  sendBlockP95Us?: number;
  error?: string | null;
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
  const [error, setError] = useState<string | null>(null);
  const [inputPermission, setInputPermission] = useState(false);
  const [platform, setPlatform] = useState<HostSnapshotView["platform"]>("macos");
  const [controlPort, setControlPort] = useState(7777);
  const [lastUpdated, setLastUpdated] = useState<Date>(new Date());

  const refresh = useCallback(async () => {
    try {
      const [status, permission, hostPlatform, actualControlPort] = await Promise.all([
        invoke<StatusView>("get_status"),
        invoke<boolean>("get_input_permission"),
        invoke<HostSnapshotView["platform"]>("get_host_platform"),
        invoke<number>("get_control_port"),
      ]);
      const activeSessions = (status.sessions || []).filter(
        (session) => !["stopped", "unknown"].includes(session.state),
      );
      setSessions(activeSessions);
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
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    const refreshWhenVisible = () => {
      if (!document.hidden) void refresh();
    };
    void refresh();
    const timer = setInterval(refreshWhenVisible, 2_000);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  }, [refresh]);

  return { banner, sessions, error, inputPermission, platform, controlPort, lastUpdated, refresh };
}

function Dashboard() {
  const {
    sessions,
    error,
    inputPermission,
    platform,
    controlPort,
    lastUpdated,
    refresh,
  } = useHostStatus();
  const [inputActionError, setInputActionError] = useState<string | null>(null);
  const [inputBusy, setInputBusy] = useState<number | "permission" | null>(null);
  const [showInspector, setShowInspector] = useState(false);
  const [showPairingModal, setShowPairingModal] = useState(false);
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
  }, [refresh]);

  const requestInputPermission = async () => {
    setInputBusy("permission");
    try {
      const granted = await invoke<boolean>("request_input_permission");
      setInputActionError(
        granted
          ? null
          : "macOS 시스템 설정의 '개인정보 보호 및 보안 > 손쉬운 사용'에서 Leftcar Host를 허용해 주세요.",
      );
      await refresh();
    } catch (cause) {
      setInputActionError(String(cause));
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
      setInputActionError(String(cause));
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

  const themeIcon = theme === "light" ? "☀️" : theme === "dark" ? "🌙" : "💻";
  const themeLabel = theme === "light" ? "라이트 모드" : theme === "dark" ? "다크 모드" : "시스템 동기화";

  return (
    <div className="host-window">
      {/* Top Application Bar */}
      <header className="host-header">
        <div className="host-header-left">
          <div className="host-logo-box">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2">
              <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
              <line x1="8" y1="21" x2="16" y2="21" />
              <line x1="12" y1="17" x2="12" y2="21" />
            </svg>
          </div>
          <div className="host-title-group">
            <h1>Leftcar Host</h1>
            <span className="host-version">v0.1</span>
          </div>
        </div>

        <div className="host-header-right">
          <div className={`host-status-pill ${isStreaming ? "pill-active" : "pill-idle"}`}>
            <span className="status-dot" />
            <span>{isStreaming ? `${sessions.length}개 스트리밍 중` : "대기 중"}</span>
          </div>
          <button
            className="btn-primary"
            onClick={() => setShowPairingModal(true)}
            title="기기 페어링 (⌘P)"
          >
            기기 페어링
          </button>
          <button
            className="btn-icon"
            onClick={toggleTheme}
            title={`테마: ${themeLabel}`}
          >
            {themeIcon}
          </button>
          <button
            className="btn-icon"
            onClick={refresh}
            title="새로고침 (⌘R)"
          >
            🔄
          </button>
        </div>
      </header>

      {/* Main Scrollable Body */}
      <main className="host-body">
        {error && <div className="banner-alert banner-danger">⚠️ {error}</div>}
        {inputActionError && <div className="banner-alert banner-danger">⚠️ {inputActionError}</div>}

        {!inputPermission && platform === "macos" && (
          <div className="banner-alert banner-warning">
            <div className="banner-text">
              <strong>원격 조작 권한 필요</strong>
              <p>뷰어 기기에서 마우스/키보드로 컴퓨터를 조작하려면 손쉬운 사용 권한을 허용하세요.</p>
            </div>
            <button
              className="btn-primary btn-sm"
              disabled={inputBusy === "permission"}
              onClick={requestInputPermission}
            >
              {inputBusy === "permission" ? "확인 중…" : "권한 허용"}
            </button>
          </div>
        )}

        {isStreaming ? (
          <div className="streams-section">
            <div className="streams-section-header">
              <h2>활성 디스플레이 스트림 ({sessions.length})</h2>
              <button
                className="btn-link"
                onClick={() => setShowInspector((prev) => !prev)}
              >
                {showInspector ? "세부 수치 숨기기 ▴" : "세부 지표 보기 ▾"}
              </button>
            </div>

            <div className="stream-cards-container">
              {sessions.map((session) => (
                <div key={session.session} className="stream-card-item">
                  <div className="stream-card-top-row">
                    <div className="stream-card-identity">
                      <span className="stream-card-icon">🖥️</span>
                      <div className="stream-card-name-group">
                        <div className="stream-name-badge-row">
                          <h3>{session.sourceName}</h3>
                          <span className="session-tag">#{session.session}</span>
                        </div>
                        <span className="stream-card-target">대상: {session.viewerAddr}</span>
                      </div>
                    </div>

                    <div className="stream-card-action">
                      <button
                        className={`btn-control-toggle ${session.inputEnabled ? "toggle-active" : ""}`}
                        disabled={(!inputPermission && !session.inputEnabled) || session.state !== "running" || inputBusy === session.session}
                        onClick={() => void toggleSessionInput(session)}
                      >
                        {inputBusy === session.session
                          ? "처리 중…"
                          : session.inputEnabled
                          ? "원격 조작 허용됨"
                          : "원격 조작 끔"}
                      </button>
                    </div>
                  </div>

                  <div className="stream-card-metrics">
                    <div className="metric-pill">
                      <span className="signal-bars">
                        <span className="bar bar-1 active" />
                        <span className="bar bar-2 active" />
                        <span className="bar bar-3 active" />
                      </span>
                      <span className="metric-value font-emerald">{session.fps} FPS</span>
                    </div>
                    <div className="metric-pill">
                      <span className="metric-label">대역폭</span>
                      <span className="metric-value">{session.kbps} kbps</span>
                    </div>
                    <div className="metric-pill">
                      <span className="metric-label">지연시간</span>
                      <span className="metric-value font-blue">초저지연 (&lt;30ms)</span>
                    </div>
                  </div>

                  {showInspector && (
                    <div className="inspector-panel">
                      <span className="inspector-header">파이프라인 레이턴시 계측</span>
                      <code>{formatDetailedLatency(session)}</code>
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        ) : (
          <div className="empty-center-container">
            <div className="host-empty-card">
              <div className="empty-graphic-box">
                <span style={{ fontSize: "28px" }}>✨</span>
              </div>
              <h2>스트리밍 준비 완료</h2>
              <p>
                동일한 Wi-Fi 네트워크의 Galaxy XR 헤드셋이나 스마트폰 앱에서<br />
                이 컴퓨터를 선택하면 초저지연 화면 전송이 시작됩니다.
              </p>
              <div className="empty-action-group">
                <button className="btn-primary btn-lg" onClick={() => setShowPairingModal(true)}>
                  기기 페어링 QR 코드 열기
                </button>
              </div>
            </div>
          </div>
        )}
      </main>

      {/* Bottom Status Bar */}
      <footer className="host-footer">
        <div className="footer-status-info">
          <span
            className="clickable-chip"
            onClick={copyPortInfo}
            title="포트 복사하기"
          >
            제어 포트: <strong>:{controlPort}</strong> {copiedToast ? "✅ 복사됨!" : "📋"}
          </span>
          <span className="footer-divider">·</span>
          <span>원격 입력: <strong>{inputPermission ? "승인됨" : "권한 필요"}</strong></span>
        </div>
        <div className="footer-timestamp">최근 확인: {lastUpdated.toLocaleTimeString()}</div>
      </footer>

      {/* Pairing Modal */}
      {showPairingModal && (
        <div className="modal-overlay" onClick={() => setShowPairingModal(false)}>
          <div className="modal-window" onClick={(e) => e.stopPropagation()}>
            <div className="modal-title-bar">
              <h3>기기 페어링</h3>
              <button className="btn-close" onClick={() => setShowPairingModal(false)}>✕</button>
            </div>
            <div className="modal-scroll-area">
              <PairingPanel />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function formatDetailedLatency(session: SessionRow): string {
  return [
    session.captureToEncodeUs !== undefined
      ? `cap: ${(session.captureToEncodeUs / 1000).toFixed(1)}ms`
      : "cap: <2ms",
    session.captureQueueWaitUs !== undefined
      ? ` · queue: ${(session.captureQueueWaitUs / 1000).toFixed(1)}ms`
      : "",
    session.encodeOutputUs !== undefined
      ? ` · enc: ${(session.encodeOutputUs / 1000).toFixed(1)}ms`
      : "",
    session.sendBlockUs !== undefined
      ? ` · send: ${(session.sendBlockUs / 1000).toFixed(1)}ms`
      : "",
    session.captureToEncodeP95Us
      ? ` · cap-p95: ${(session.captureToEncodeP95Us / 1000).toFixed(1)}ms`
      : "",
    session.sendBlockP95Us
      ? ` · send-p95: ${(session.sendBlockP95Us / 1000).toFixed(1)}ms`
      : "",
    session.dropped ? ` · drops: ${session.dropped}` : "",
    session.captureBackend ? ` · backend: ${session.captureBackend}` : "",
  ].join("");
}
