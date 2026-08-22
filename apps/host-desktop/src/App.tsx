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

export default function App() {
  // The pairing window loads the same bundle at #/pairing (see lib.rs).
  if (window.location.hash === "#/pairing") {
    return <PairingPanel />;
  }

  return <Dashboard />;
}

function useHostStatus() {
  const [banner, setBanner] = useState("Leftcar");
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [inputPermission, setInputPermission] = useState(false);
  const [lastUpdated, setLastUpdated] = useState<Date>(new Date());

  const refresh = useCallback(async () => {
    try {
      const [status, permission] = await Promise.all([
        invoke<StatusView>("get_status"),
        invoke<boolean>("get_input_permission"),
      ]);
      const activeSessions = (status.sessions || []).filter(
        (session) => !["stopped", "unknown"].includes(session.state),
      );
      setSessions(activeSessions);
      setBanner(
        trayStatus({
          hostId: "local",
          platform: "macos",
          pairingState: "connected",
          pairedDevices: [],
          approvedSources: [],
          activeStreamCount: activeSessions.length,
        } satisfies HostSnapshotView),
      );
      setError(null);
      setInputPermission(permission);
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

  return { banner, sessions, error, inputPermission, lastUpdated, refresh };
}

function Dashboard() {
  const { banner, sessions, error, inputPermission, lastUpdated, refresh } = useHostStatus();
  const [inputActionError, setInputActionError] = useState<string | null>(null);
  const [inputBusy, setInputBusy] = useState<number | "permission" | null>(null);
  const totalKbps = sessions.reduce((total, session) => total + (session.kbps || 0), 0);
  const isStreaming = sessions.length > 0;

  const requestInputPermission = async () => {
    setInputBusy("permission");
    try {
      const granted = await invoke<boolean>("request_input_permission");
      setInputActionError(
        granted
          ? null
          : "macOS 시스템 설정의 개인정보 보호 및 보안 > 손쉬운 사용에서 Leftcar Host를 허용해주세요.",
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

  return (
    <main className="app-container">
      <HostHeader banner={banner} isStreaming={isStreaming} />
      {error && <ErrorBanner message={error} />}
      {inputActionError && <ErrorBanner message={inputActionError} />}
      <InputPermissionCard
        granted={inputPermission}
        busy={inputBusy === "permission"}
        onRequest={() => void requestInputPermission()}
      />
      <SummaryStats
        activeStreamCount={sessions.length}
        isStreaming={isStreaming}
        totalKbps={totalKbps}
      />
      <SessionsCard
        sessions={sessions}
        inputPermission={inputPermission}
        inputBusy={inputBusy}
        onToggleInput={(session) => void toggleSessionInput(session)}
        onRefresh={refresh}
      />
      <SystemFooter lastUpdated={lastUpdated} />
    </main>
  );
}

function InputPermissionCard({
  granted,
  busy,
  onRequest,
}: {
  granted: boolean;
  busy: boolean;
  onRequest: () => void;
}) {
  return (
    <section className={`input-permission-card ${granted ? "input-permission-granted" : ""}`}>
      <div>
        <strong>Remote Input</strong>
        <p>
          {granted
            ? "macOS 입력 권한 승인됨 · 각 스트림에서 별도로 켜야 합니다."
            : "키보드와 마우스 제어에는 macOS 손쉬운 사용 권한이 필요합니다."}
        </p>
      </div>
      {granted ? (
        <span className="input-permission-status">권한 승인</span>
      ) : (
        <button className="btn-primary" disabled={busy} onClick={onRequest}>
          {busy ? "확인 중…" : "입력 권한 요청"}
        </button>
      )}
    </section>
  );
}

function HostHeader({ banner, isStreaming }: { banner: string; isStreaming: boolean }) {
  return (
    <header className="app-header">
      <div className="brand-section">
        <div className="brand-logo-badge">
          <svg
            width="24"
            height="24"
            viewBox="0 0 24 24"
            fill="none"
            stroke="#ffffff"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
            <line x1="8" y1="21" x2="16" y2="21" />
            <line x1="12" y1="17" x2="12" y2="21" />
          </svg>
        </div>
        <div className="brand-info">
          <h1>
            Leftcar Host Studio
            <span className="kbd-badge" style={{ fontSize: "10px" }}>v0.1</span>
          </h1>
          <p>Low-Latency Multi-Window Desktop Streamer · Galaxy XR & Mobile</p>
        </div>
      </div>
      <div className={`header-status-badge ${isStreaming ? "status-active" : "status-standby"}`}>
        <span
          className={`status-dot ${
            isStreaming ? "status-dot-emerald animate-pulse-glow" : "status-dot-sky"
          }`}
        />
        {banner}
      </div>
    </header>
  );
}

function ErrorBanner({ message }: { message: string }) {
  return (
    <div className="error-banner">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
        <circle cx="12" cy="12" r="10" />
        <line x1="12" y1="8" x2="12" y2="12" />
        <line x1="12" y1="16" x2="12.01" y2="16" />
      </svg>
      <span>{message}</span>
    </div>
  );
}

function SummaryStats({
  activeStreamCount,
  isStreaming,
  totalKbps,
}: {
  activeStreamCount: number;
  isStreaming: boolean;
  totalKbps: number;
}) {
  const [copied, setCopied] = useState(false);

  const copyPortInfo = () => {
    void navigator.clipboard.writeText("7777");
    setCopied(true);
    setTimeout(() => setCopied(false), 2_000);
  };

  return (
    <section className="stats-grid">
      <StatCard label="Active Streams">
        <span className="stat-value" style={{ color: isStreaming ? "#34d399" : "#f8fafc" }}>
          {activeStreamCount}
        </span>
        <span className="stat-chip">{isStreaming ? "STREAMING" : "STANDBY"}</span>
      </StatCard>
      <StatCard label="Control Plane">
        <span className="stat-value" style={{ fontSize: "18px", color: "#38bdf8" }}>
          TCP :7777
        </span>
        <button onClick={copyPortInfo} className="stat-chip" style={{ cursor: "pointer" }}>
          {copied ? "COPIED" : "mDNS AUTO"}
        </button>
      </StatCard>
      <StatCard label="Total Bandwidth">
        <span className="stat-value">
          {totalKbps > 1000 ? `${(totalKbps / 1000).toFixed(1)} Mbps` : `${totalKbps} kbps`}
        </span>
        <span className="stat-chip">H.264</span>
      </StatCard>
      <StatCard label="Video Engine">
        <span className="stat-value" style={{ fontSize: "15px", color: "#a5b4fc" }}>
          AMediaCodec
        </span>
        <span className="stat-chip">Direct Surface</span>
      </StatCard>
    </section>
  );
}

function StatCard({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="stat-card">
      <span className="stat-label">{label}</span>
      <div className="stat-value-row">{children}</div>
    </div>
  );
}

function SessionsCard({
  sessions,
  inputPermission,
  inputBusy,
  onToggleInput,
  onRefresh,
}: {
  sessions: SessionRow[];
  inputPermission: boolean;
  inputBusy: number | "permission" | null;
  onToggleInput: (session: SessionRow) => void;
  onRefresh: () => void;
}) {
  return (
    <section className="dashboard-card">
      <div className="card-header">
        <div className="card-header-left">
          <h2 className="card-title">Live Capture Sessions</h2>
          <span className="badge-count">{sessions.length}</span>
        </div>
        <div className="card-header-right">
          <button onClick={onRefresh} className="btn-ghost">새로고침</button>
        </div>
      </div>
      {sessions.length > 0 ? (
        <SessionsTable
          sessions={sessions}
          inputPermission={inputPermission}
          inputBusy={inputBusy}
          onToggleInput={onToggleInput}
        />
      ) : (
        <EmptySessions />
      )}
    </section>
  );
}

function SessionsTable({
  sessions,
  inputPermission,
  inputBusy,
  onToggleInput,
}: {
  sessions: SessionRow[];
  inputPermission: boolean;
  inputBusy: number | "permission" | null;
  onToggleInput: (session: SessionRow) => void;
}) {
  return (
    <div className="table-container">
      <table className="sessions-table">
        <thead>
          <tr>
            <th>Session</th>
            <th>Source Display</th>
            <th>Viewer Destination</th>
            <th>Status</th>
            <th>FPS</th>
            <th>Bitrate</th>
            <th>Remote Input</th>
            <th>Pipeline Latency</th>
          </tr>
        </thead>
        <tbody>
          {sessions.map((session) => (
            <SessionTableRow
              key={session.session}
              session={session}
              inputPermission={inputPermission}
              inputBusy={inputBusy === session.session}
              onToggleInput={onToggleInput}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function SessionTableRow({
  session,
  inputPermission,
  inputBusy,
  onToggleInput,
}: {
  session: SessionRow;
  inputPermission: boolean;
  inputBusy: boolean;
  onToggleInput: (session: SessionRow) => void;
}) {
  const isRunning = session.state === "running";

  return (
    <tr>
      <td><span className="session-id-chip">#{session.session}</span></td>
      <td>
        <div className="source-cell">
          <span className="source-icon">🖥️</span>
          <span>{session.sourceName}</span>
        </div>
      </td>
      <td className="mono-cell">{session.viewerAddr}</td>
      <td>
        <span className={`state-badge ${isRunning ? "state-running" : "state-error"}`}>
          <span
            className={`status-dot ${
              isRunning ? "status-dot-emerald animate-pulse-glow" : "status-dot-red"
            }`}
          />
          {session.state}
        </span>
      </td>
      <td>
        <span className="fps-metric">
          {session.fps}
          <span style={{ fontSize: "11px", color: "#94a3b8", fontWeight: 400 }}>
            {session.fpsTarget ? ` / ${session.fpsTarget} fps` : " fps"}
          </span>
        </span>
      </td>
      <td><span className="kbps-metric">{session.kbps} kbps</span></td>
      <td>
        <button
          className={`input-toggle ${session.inputEnabled ? "input-toggle-enabled" : ""}`}
          disabled={(!inputPermission && !session.inputEnabled) || !isRunning || inputBusy}
          onClick={() => onToggleInput(session)}
        >
          {inputBusy
            ? "변경 중…"
            : `${session.inputEnabled ? "CONTROL" : "OBSERVE"} · ${session.inputRateHz} Hz 목표`}
        </button>
      </td>
      <td><div className="latency-metric">{formatLatency(session)}</div></td>
    </tr>
  );
}

function formatLatency(session: SessionRow): string {
  return [
    session.captureToEncodeUs !== undefined
      ? `cap: ${(session.captureToEncodeUs / 1000).toFixed(1)}ms`
      : "cap: <2ms",
    session.captureQueueWaitUs !== undefined
      ? ` · q: ${(session.captureQueueWaitUs / 1000).toFixed(1)}ms`
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
    session.firstSendMs ? ` · first: ${session.firstSendMs}ms` : "",
    session.dropped ? ` · drop: ${session.dropped}` : "",
    session.captureBackend ? ` · ${session.captureBackend}` : "",
    session.mediaTransport ? `/${session.mediaTransport.toUpperCase()}` : "",
    session.error ? ` · ${session.error}` : "",
  ].join("");
}

function EmptySessions() {
  return (
    <div className="empty-state">
      <div className="radar-wrapper">
        <div className="radar-ring animate-ripple" />
        <div className="radar-ring-2 animate-ripple" style={{ animationDelay: "1s" }} />
        <div className="radar-center-icon">
          <svg
            width="22"
            height="22"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="12" cy="12" r="2" />
            <path d="M16.24 7.76a6 6 0 0 1 0 8.49m-8.48-.01a6 6 0 0 1 0-8.49m11.31-2.82a10 10 0 0 1 0 14.14m-14.14 0a10 10 0 0 1 0-14.14" />
          </svg>
        </div>
      </div>
      <p className="empty-title">활성 스트림 없음 — 뷰어 연결 대기 중</p>
      <p className="empty-sub">
        Galaxy XR 헤드셋 또는 모바일 뷰어 앱에서 <strong>호스트 연결</strong>을 누르고 원하는 화면을
        선택하면 서브 30ms 초저지연 스트리밍이 즉시 시작됩니다.
      </p>
      <div className="empty-tips-box">
        <span>⚡ 제어 포트: <span className="kbd-badge">7777</span></span>
        <span>•</span>
        <span>📡 mDNS 서비스: <span className="kbd-badge">_leftcar._tcp</span></span>
        <span>•</span>
        <span>🚀 AMediaCodec NDK 직결</span>
      </div>
    </div>
  );
}

function SystemFooter({ lastUpdated }: { lastUpdated: Date }) {
  return (
    <footer className="system-footer">
      <div className="footer-left">
        <span className="footer-badge">
          <span className="status-dot status-dot-emerald" style={{ width: "6px", height: "6px" }} />
          Tauri 2 + ScreenCaptureKit FFI
        </span>
        <span>•</span>
        <span>Direct Surface NDK Pipeline</span>
      </div>
      <div className="footer-right">Last polled: {lastUpdated.toLocaleTimeString()}</div>
    </footer>
  );
}
