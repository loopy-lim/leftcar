import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import QRCode from "qrcode";

interface PairingSessionView {
  qr_payload: string;
  code: string;
  expires_in_secs: number;
}

interface PairedDevice {
  device_id: string;
  name: string;
  token_hex: string;
  paired_at: string;
}

interface ActiveSession {
  qrDataUrl: string;
  code: string;
  expiresAt: number;
}

/** "unix:<secs>" (pairing.rs) → readable local date; raw string when unparsable. */
function formatPairedAt(pairedAt: string): string {
  const secs = Number(pairedAt.replace(/^unix:/, ""));
  if (!Number.isFinite(secs) || secs <= 0) return pairedAt;
  return new Date(secs * 1000).toLocaleString();
}

function formatCountdown(remainingMs: number): string {
  const total = Math.max(0, Math.ceil(remainingMs / 1000));
  const mm = String(Math.floor(total / 60)).padStart(2, "0");
  const ss = String(total % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

export default function PairingPanel() {
  const [session, setSession] = useState<ActiveSession | null>(null);
  const [starting, setStarting] = useState(false);
  const [devices, setDevices] = useState<PairedDevice[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  // latest device count, readable by the poll callback without re-subscribing
  const deviceCountRef = useRef(devices.length);
  deviceCountRef.current = devices.length;

  const refreshDevices = useCallback(async () => {
    try {
      const list = await invoke<PairedDevice[]>("list_paired_devices");
      // A new device appearing means a viewer just completed pairing with the
      // shown QR (single-use offer) — retire the session so a dead QR doesn't
      // sit on screen until expiry.
      if (list.length > deviceCountRef.current) {
        setSession(null);
      }
      setDevices(list);
    } catch {
      // best-effort poll; the list refreshes on the next tick
    }
  }, []);

  const startPairing = useCallback(async () => {
    setStarting(true);
    setError(null);
    try {
      const view = await invoke<PairingSessionView>("begin_pairing");
      const qrDataUrl = await QRCode.toDataURL(view.qr_payload, {
        width: 240,
        margin: 1,
      });
      setSession({
        qrDataUrl,
        code: view.code,
        expiresAt: Date.now() + view.expires_in_secs * 1000,
      });
      refreshDevices();
    } catch (e) {
      setError(String(e));
    } finally {
      setStarting(false);
    }
  }, [refreshDevices]);

  const cancelPairing = useCallback(async () => {
    try {
      await invoke("cancel_pairing");
    } catch {
      // offer expiry (120s) covers a failed cancel
    }
    setSession(null);
  }, []);

  const revoke = useCallback(
    async (deviceId: string) => {
      setRevoking(deviceId);
      setError(null);
      try {
        await invoke("revoke_device", { deviceId });
        await refreshDevices();
      } catch (e) {
        setError(String(e));
      } finally {
        setRevoking(null);
      }
    },
    [refreshDevices],
  );

  // device list: initial load + 5s poll while the panel is open
  useEffect(() => {
    refreshDevices();
    const poll = setInterval(refreshDevices, 5000);
    return () => clearInterval(poll);
  }, [refreshDevices]);

  // countdown tick
  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, []);

  const expired = session !== null && now >= session.expiresAt;

  // release the host offer once the countdown hits zero (fire-and-forget:
  // the offer is dead either way after its own TTL)
  useEffect(() => {
    if (session && expired) {
      invoke("cancel_pairing").catch(() => {});
    }
  }, [session, expired]);

  return (
    <main className="pairing-container">
      <header className="pairing-header">
        <h1>기기 페어링</h1>
        <p>뷰어 앱에서 QR 코드를 스캔한 뒤 화면에 표시된 코드를 입력하세요.</p>
      </header>

      {error && <div className="error-banner">{error}</div>}

      <section className="dashboard-card">
        {!session ? (
          <div className="pairing-idle">
            <p className="empty-title">페어링 대기 중</p>
            <p className="empty-sub">
              QR 코드를 생성하면 2분 동안 유효한 페어링 코드가 만들어집니다.
            </p>
            <button onClick={startPairing} className="btn-primary" disabled={starting}>
              {starting ? "생성 중…" : "페어링 시작"}
            </button>
          </div>
        ) : expired ? (
          <div className="pairing-idle">
            <p className="empty-title">코드 만료</p>
            <p className="empty-sub">유효 시간(2분)이 지나 코드가 만료되었습니다.</p>
            <button onClick={startPairing} className="btn-primary" disabled={starting}>
              다시 생성
            </button>
          </div>
        ) : (
          <div className="pairing-active">
            <img
              src={session.qrDataUrl}
              alt="페어링 QR 코드"
              width={240}
              height={240}
              className="pairing-qr"
            />
            <div className="pairing-code">{session.code.replace(/(\d{3})(\d{3})/, "$1 $2")}</div>
            <div className="pairing-countdown">
              남은 시간 {formatCountdown(session.expiresAt - now)}
            </div>
            <button onClick={cancelPairing} className="btn-ghost">
              취소
            </button>
          </div>
        )}
      </section>

      <section className="dashboard-card">
        <div className="card-header">
          <div className="card-header-left">
            <h2 className="card-title">페어링된 기기</h2>
            <span className="badge-count">{devices.length}</span>
          </div>
          <div className="card-header-right">
            <button onClick={refreshDevices} className="btn-ghost">
              새로고침
            </button>
          </div>
        </div>
        {devices.length > 0 ? (
          <ul className="pairing-device-list">
            {devices.map((device) => (
              <li key={device.device_id} className="pairing-device-row">
                <div className="pairing-device-info">
                  <span className="pairing-device-name">{device.name}</span>
                  <span className="pairing-device-date">{formatPairedAt(device.paired_at)}</span>
                </div>
                <button
                  onClick={() => revoke(device.device_id)}
                  className="btn-ghost pairing-revoke"
                  disabled={revoking === device.device_id}
                >
                  {revoking === device.device_id ? "제거 중…" : "제거"}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <div className="empty-state">
            <p className="empty-title">페어링된 기기 없음</p>
            <p className="empty-sub">QR 코드를 스캔한 뷰어가 여기에 표시됩니다.</p>
          </div>
        )}
      </section>
    </main>
  );
}
