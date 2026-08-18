import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { trayStatus, type HostSnapshotView } from "./hostState";

/**
 * Host status dashboard (docs/03 §3.1):
 * banner (trayStatus) + live session table. Sessions come from the control
 * server snapshot via the `get_status` Tauri command.
 */
export default function App() {
  const [banner, setBanner] = useState("Leftcar");
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const poll = setInterval(async () => {
      try {
        const status = await invoke<StatusView>("get_status");
        setSessions(status.sessions);
        setBanner(
          trayStatus({
            hostId: "local",
            platform: "macos",
            pairingState: "connected",
            pairedDevices: [],
            approvedSources: [],
            activeStreamCount: status.sessions.length,
          } satisfies HostSnapshotView),
        );
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    }, 1000);
    return () => clearInterval(poll);
  }, []);

  return (
    <main style={{ fontFamily: "system-ui", padding: 16 }}>
      <h2>{banner}</h2>
      {error && <p style={{ color: "#c62828" }}>{error}</p>}
      <table style={{ borderCollapse: "collapse", width: "100%" }}>
        <thead>
          <tr>
            {["session", "source", "viewer", "state", "fps", "kbps"].map((h) => (
              <th key={h} style={{ textAlign: "left", borderBottom: "1px solid #ccc", padding: 4 }}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {sessions.map((s) => (
            <tr key={s.session}>
              <td style={{ padding: 4 }}>{s.session}</td>
              <td style={{ padding: 4 }}>{s.sourceName}</td>
              <td style={{ padding: 4 }}>{s.viewerAddr}</td>
              <td style={{ padding: 4 }}>{s.state}</td>
              <td style={{ padding: 4 }}>{s.fps}</td>
              <td style={{ padding: 4 }}>{s.kbps}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {sessions.length === 0 && (
        <p style={{ color: "#666" }}>활성 스트림 없음 — 뷰어에서 연결을 기다리는 중</p>
      )}
    </main>
  );
}

interface SessionRow {
  session: number;
  sourceIndex: number;
  sourceName: string;
  viewerAddr: string;
  state: string;
  fps: number;
  kbps: number;
}

interface StatusView {
  sessions: SessionRow[];
}
