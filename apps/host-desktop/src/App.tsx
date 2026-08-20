import { useEffect, useState } from "react";
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

function Dashboard() {
  const [banner, setBanner] = useState("Leftcar");
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [lastUpdated, setLastUpdated] = useState<Date>(new Date());

  const fetchStatus = async () => {
    try {
      const status = await invoke<StatusView>("get_status");
      const activeSessions = (status.sessions || []).filter((session) => session.state === "running");
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
      setLastUpdated(new Date());
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    fetchStatus();
    const poll = setInterval(fetchStatus, 1000);
    return () => clearInterval(poll);
  }, []);

  const totalKbps = sessions.reduce((acc, s) => acc + (s.kbps || 0), 0);
  const isStreaming = sessions.length > 0;

  const copyPortInfo = () => {
    navigator.clipboard.writeText("7777");
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <main className="app-container">
      {/* Top Header */}
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

        <div
          className={`header-status-badge ${
            isStreaming ? "status-active" : "status-standby"
          }`}
        >
          <span
            className={`status-dot ${
              isStreaming
                ? "status-dot-emerald animate-pulse-glow"
                : "status-dot-sky"
            }`}
          />
          {banner}
        </div>
      </header>

      {/* Error notification */}
      {error && (
        <div className="error-banner">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="12" r="10" />
            <line x1="12" y1="8" x2="12" y2="12" />
            <line x1="12" y1="16" x2="12.01" y2="16" />
          </svg>
          <span>{error}</span>
        </div>
      )}

      {/* Top Summary Stats */}
      <section className="stats-grid">
        <div className="stat-card">
          <span className="stat-label">Active Streams</span>
          <div className="stat-value-row">
            <span className="stat-value" style={{ color: isStreaming ? "#34d399" : "#f8fafc" }}>
              {sessions.length}
            </span>
            <span className="stat-chip">
              {isStreaming ? "STREAMING" : "STANDBY"}
            </span>
          </div>
        </div>

        <div className="stat-card">
          <span className="stat-label">Control Plane</span>
          <div className="stat-value-row">
            <span className="stat-value" style={{ fontSize: "18px", color: "#38bdf8" }}>
              TCP :7777
            </span>
            <button onClick={copyPortInfo} className="stat-chip" style={{ cursor: "pointer" }}>
              {copied ? "COPIED" : "mDNS AUTO"}
            </button>
          </div>
        </div>

        <div className="stat-card">
          <span className="stat-label">Total Bandwidth</span>
          <div className="stat-value-row">
            <span className="stat-value">
              {totalKbps > 1000 ? `${(totalKbps / 1000).toFixed(1)} Mbps` : `${totalKbps} kbps`}
            </span>
            <span className="stat-chip">H.264</span>
          </div>
        </div>

        <div className="stat-card">
          <span className="stat-label">Video Engine</span>
          <div className="stat-value-row">
            <span className="stat-value" style={{ fontSize: "15px", color: "#a5b4fc" }}>
              AMediaCodec
            </span>
            <span className="stat-chip">Direct Surface</span>
          </div>
        </div>
      </section>

      {/* Main Sessions Card */}
      <section className="dashboard-card">
        <div className="card-header">
          <div className="card-header-left">
            <h2 className="card-title">Live Capture Sessions</h2>
            <span className="badge-count">{sessions.length}</span>
          </div>
          <div className="card-header-right">
            <button onClick={fetchStatus} className="btn-ghost">
              새로고침
            </button>
          </div>
        </div>

        {sessions.length > 0 ? (
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
                  <th>Pipeline Latency</th>
                </tr>
              </thead>
              <tbody>
                {sessions.map((s) => (
                  <tr key={s.session}>
                    <td>
                      <span className="session-id-chip">#{s.session}</span>
                    </td>
                    <td>
                      <div className="source-cell">
                        <span className="source-icon">🖥️</span>
                        <span>{s.sourceName}</span>
                      </div>
                    </td>
                    <td className="mono-cell">{s.viewerAddr}</td>
                    <td>
                      <span className={`state-badge ${s.state === "running" ? "state-running" : "state-error"}`}>
                        <span className={`status-dot ${s.state === "running" ? "status-dot-emerald animate-pulse-glow" : "status-dot-red"}`} />
                        {s.state}
                      </span>
                    </td>
                    <td>
                      <span className="fps-metric">
                        {s.fps}
                        <span style={{ fontSize: "11px", color: "#94a3b8", fontWeight: 400 }}>
                          {s.fpsTarget ? ` / ${s.fpsTarget} fps` : " fps"}
                        </span>
                      </span>
                    </td>
                    <td>
                      <span className="kbps-metric">{s.kbps} kbps</span>
                    </td>
                    <td>
                      <div className="latency-metric">
                        {s.captureToEncodeUs !== undefined
                          ? `cap: ${(s.captureToEncodeUs / 1000).toFixed(1)}ms`
                          : "cap: <2ms"}
                        {s.captureQueueWaitUs !== undefined
                          ? ` · q: ${(s.captureQueueWaitUs / 1000).toFixed(1)}ms`
                          : ""}
                        {s.encodeOutputUs !== undefined
                          ? ` · enc: ${(s.encodeOutputUs / 1000).toFixed(1)}ms`
                          : ""}
                        {s.sendBlockUs !== undefined
                          ? ` · send: ${(s.sendBlockUs / 1000).toFixed(1)}ms`
                          : ""}
                        {s.dropped ? ` · drop: ${s.dropped}` : ""}
                        {s.error ? ` · ${s.error}` : ""}
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
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
              Galaxy XR 헤드셋 또는 모바일 뷰어 앱에서 <strong>호스트 연결</strong>을 누르고
              원하는 화면을 선택하면 서브 30ms 초저지연 스트리밍이 즉시 시작됩니다.
            </p>
            <div className="empty-tips-box">
              <span>⚡ 제어 포트: <span className="kbd-badge">7777</span></span>
              <span>•</span>
              <span>📡 mDNS 서비스: <span className="kbd-badge">_leftcar._tcp</span></span>
              <span>•</span>
              <span>🚀 AMediaCodec NDK 직결</span>
            </div>
          </div>
        )}
      </section>

      {/* System Footer */}
      <footer className="system-footer">
        <div className="footer-left">
          <span className="footer-badge">
            <span className="status-dot status-dot-emerald" style={{ width: "6px", height: "6px" }} />
            Tauri 2 + ScreenCaptureKit FFI
          </span>
          <span>•</span>
          <span>Direct Surface NDK Pipeline</span>
        </div>
        <div className="footer-right">
          Last polled: {lastUpdated.toLocaleTimeString()}
        </div>
      </footer>
    </main>
  );
}
