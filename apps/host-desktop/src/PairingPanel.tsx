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
  const deviceCountRef = useRef(devices.length);

  useEffect(() => {
    deviceCountRef.current = devices.length;
  }, [devices.length]);

  const refreshDevices = useCallback(async () => {
    try {
      const list = await invoke<PairedDevice[]>("list_paired_devices");
      if (list.length > deviceCountRef.current) {
        setSession(null);
      }
      setDevices(list);
    } catch {
      // best effort
    }
  }, []);

  const startPairing = useCallback(async () => {
    setStarting(true);
    setError(null);
    try {
      const view = await invoke<PairingSessionView>("begin_pairing");
      const qrDataUrl = await QRCode.toDataURL(view.qr_payload, {
        width: 200,
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
      // ignore
    }
    setSession(null);
  }, []);

  const revoke = useCallback(
    async (deviceId: string) => {
      setRevoking(deviceId);
      setError(null);
      try {
        await invoke("revoke_paired_device", { deviceId });
        await refreshDevices();
      } catch (e) {
        setError(String(e));
      } finally {
        setRevoking(null);
      }
    },
    [refreshDevices],
  );

  useEffect(() => {
    refreshDevices();
    const interval = setInterval(refreshDevices, 2000);
    return () => clearInterval(interval);
  }, [refreshDevices]);

  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, []);

  const expired = session !== null && now >= session.expiresAt;

  useEffect(() => {
    if (session && expired) {
      invoke("cancel_pairing").catch(() => {});
    }
  }, [session, expired]);

  return (
    <div className="pairing-wrapper">
      <div className="pairing-guide">
        <p className="pairing-guide-title">뷰어 앱(XR / 모바일)에서 QR 코드를 스캔하세요</p>
        <p className="pairing-guide-sub">동일한 Wi-Fi 네트워크에서 한 번 페어링하면 이후 자동 연결됩니다.</p>
      </div>

      {error && <div className="banner-alert banner-danger">⚠️ {error}</div>}

      <div className="pairing-qr-card">
        {!session ? (
          <div className="pairing-idle-state">
            <div className="idle-icon-box">🔐</div>
            <p className="idle-title">페어링 세션 시작</p>
            <p className="idle-sub">버튼을 누르면 2분 동안 유효한 일회용 QR 코드가 생성됩니다.</p>
            <button onClick={startPairing} className="btn-primary" disabled={starting}>
              {starting ? "생성 중…" : "페어링 QR 생성"}
            </button>
          </div>
        ) : expired ? (
          <div className="pairing-idle-state">
            <p className="idle-title font-rose">페어링 코드 만료</p>
            <p className="idle-sub">유효 시간이 지나 코드가 만료되었습니다.</p>
            <button onClick={startPairing} className="btn-primary" disabled={starting}>
              새 QR 코드 생성
            </button>
          </div>
        ) : (
          <div className="pairing-active-state">
            <div className="qr-image-frame">
              <img
                src={session.qrDataUrl}
                alt="페어링 QR 코드"
                width={190}
                height={190}
                className="qr-img"
              />
            </div>
            <div className="code-display-box">
              <span className="code-label">인증 번호:</span>
              <span className="code-value">{session.code.replace(/(\d{3})(\d{3})/, "$1 $2")}</span>
            </div>
            <div className="countdown-badge">
              ⏳ 남은 시간: {formatCountdown(session.expiresAt - now)}
            </div>
            <button onClick={cancelPairing} className="btn-ghost btn-sm">
              페어링 취소
            </button>
          </div>
        )}
      </div>

      <div className="paired-devices-section">
        <div className="section-title-row">
          <h4>연결된 기기 목록</h4>
          <span className="count-pill">{devices.length}</span>
        </div>

        {devices.length > 0 ? (
          <div className="device-rows-container">
            {devices.map((device) => (
              <div key={device.device_id} className="device-row-item">
                <div className="device-row-main">
                  <span className="device-row-name">📱 {device.name}</span>
                  <span className="device-row-date">{formatPairedAt(device.paired_at)}</span>
                </div>
                <button
                  onClick={() => revoke(device.device_id)}
                  className="btn-danger-outline"
                  disabled={revoking === device.device_id}
                >
                  {revoking === device.device_id ? "제거 중…" : "제거"}
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div className="empty-devices-box">
            <p>아직 등록된 기기가 없습니다.</p>
          </div>
        )}
      </div>
    </div>
  );
}
