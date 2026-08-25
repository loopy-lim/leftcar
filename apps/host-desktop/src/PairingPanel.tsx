import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import QRCode from "qrcode";
import {
  AlertTriangle,
  Check,
  Clock,
  Copy,
  KeyRound,
  QrCode,
  RefreshCw,
  ShieldCheck,
  Smartphone,
  Trash2,
} from "lucide-react";

interface PairingSessionView {
  qr_payload: string;
  code: string;
  expires_in_secs: number;
}

interface PairedDevice {
  device_id: string;
  name: string;
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
  return new Date(secs * 1000).toLocaleString("ko-KR", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatCountdown(remainingMs: number): string {
  const total = Math.max(0, Math.ceil(remainingMs / 1000));
  const mm = String(Math.floor(total / 60)).padStart(2, "0");
  const ss = String(total % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

function connectionErrorMessage(cause: unknown): string {
  const message = String(cause instanceof Error ? cause.message : cause).toLowerCase();
  if (message.includes("no lan interface")) {
    return "연결할 네트워크를 찾지 못했습니다. Wi-Fi 또는 Tailscale 연결을 확인해 주세요.";
  }
  if (message.includes("persistence")) {
    return "기기 연결 정보를 저장하지 못했습니다. 저장 공간과 권한을 확인해 주세요.";
  }
  return "연결을 준비하지 못했습니다. 잠시 후 다시 시도해 주세요.";
}

export default function PairingPanel() {
  const [session, setSession] = useState<ActiveSession | null>(null);
  const [starting, setStarting] = useState(false);
  const [devices, setDevices] = useState<PairedDevice[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [copiedCode, setCopiedCode] = useState(false);
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
        width: 220,
        margin: 1,
        color: {
          dark: "#0f172a",
          light: "#ffffff",
        },
      });
      setSession({
        qrDataUrl,
        code: view.code,
        expiresAt: Date.now() + view.expires_in_secs * 1000,
      });
      refreshDevices();
    } catch (e) {
      setError(connectionErrorMessage(e));
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

  const copyCode = useCallback(() => {
    if (!session) return;
    void navigator.clipboard.writeText(session.code);
    setCopiedCode(true);
    setTimeout(() => setCopiedCode(false), 2000);
  }, [session]);

  const revoke = useCallback(
    async (deviceId: string) => {
      setRevoking(deviceId);
      setError(null);
      try {
        await invoke("revoke_paired_device", { deviceId });
        await refreshDevices();
      } catch (e) {
        setError(connectionErrorMessage(e));
      } finally {
        setRevoking(null);
      }
    },
    [refreshDevices],
  );

  const revokeAll = useCallback(async () => {
    setRevoking("all");
    setError(null);
    try {
      await invoke("revoke_all_devices");
      await refreshDevices();
    } catch (e) {
      setError(connectionErrorMessage(e));
    } finally {
      setRevoking(null);
    }
  }, [refreshDevices]);

  useEffect(() => {
    refreshDevices();
    const interval = setInterval(refreshDevices, 2000);
    return () => clearInterval(interval);
  }, [refreshDevices]);

  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, []);

  useEffect(
    () => () => {
      // Closing this screen invalidates any QR that may still be visible in a
      // screenshot or camera preview.
      void invoke("cancel_pairing");
    },
    [],
  );

  const expired = session !== null && now >= session.expiresAt;

  useEffect(() => {
    if (session && expired) {
      invoke("cancel_pairing").catch(() => {});
    }
  }, [session, expired]);

  return (
    <div className="pairing-wrapper">
      <div className="pairing-guide">
        <p className="pairing-guide-title">휴대폰이나 태블릿 연결</p>
        <p className="pairing-guide-sub">
          Leftcar Viewer에서 QR 코드를 스캔한 뒤, 이 화면의 6자리 번호를 입력하세요.
        </p>
      </div>

      <div className="pairing-security-note">
        <ShieldCheck size={17} aria-hidden="true" />
        <div>
          <strong>안전한 연결을 위해 확인해 주세요</strong>
          <p>
            QR 코드와 인증 번호는 한 번만 사용할 수 있고 2분 뒤 만료됩니다. 신뢰하는 같은 Wi-Fi
            또는 Tailscale에 연결된 기기에서만 진행하세요.
          </p>
        </div>
      </div>

      {error && (
        <div className="banner-alert banner-danger">
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <AlertTriangle size={15} /> {error}
          </span>
        </div>
      )}

      <div className="pairing-qr-card">
        {!session ? (
          <div className="pairing-idle-state">
            <div className="idle-icon-box">
              <QrCode size={22} strokeWidth={2} />
            </div>
            <p className="idle-title">새 기기 연결하기</p>
            <p className="idle-sub">2분 동안 한 번만 사용할 수 있는 QR 코드를 만듭니다.</p>
            <button onClick={startPairing} className="btn-primary btn-lg" disabled={starting}>
              <QrCode size={15} />
              {starting ? "QR 코드 만드는 중…" : "연결 QR 코드 만들기"}
            </button>
          </div>
        ) : expired ? (
          <div className="pairing-idle-state">
            <div className="idle-icon-box">
              <Clock size={22} strokeWidth={2} />
            </div>
            <p className="idle-title">연결 코드가 만료되었어요</p>
            <p className="idle-sub">새 QR 코드를 만든 뒤 다시 시도해 주세요.</p>
            <button onClick={startPairing} className="btn-primary" disabled={starting}>
              <RefreshCw size={14} />
              {starting ? "생성 중…" : "새 QR 코드 생성"}
            </button>
          </div>
        ) : (
          <div className="pairing-active-state">
            <div className="qr-image-frame">
              <img
                src={session.qrDataUrl}
                alt="기기 연결 QR 코드"
                width={190}
                height={190}
                className="qr-img"
              />
            </div>
            <div className="code-display-box">
              <span className="code-label">인증 번호:</span>
              <span className="code-value">{session.code.replace(/(\d{3})(\d{3})/, "$1 $2")}</span>
              <button
                type="button"
                className="clickable-chip"
                onClick={copyCode}
                title="인증 번호 복사"
                style={{ marginLeft: 4 }}
              >
                {copiedCode ? (
                  <span style={{ display: "inline-flex", alignItems: "center", gap: 3, fontSize: 11, fontWeight: 600 }}>
                    <Check size={13} /> 복사됨
                  </span>
                ) : (
                  <Copy size={13} style={{ opacity: 0.8 }} />
                )}
              </button>
            </div>
            <div className="countdown-badge" style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
              <Clock size={12} /> 남은 시간: {formatCountdown(session.expiresAt - now)}
            </div>
            <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
              <button onClick={startPairing} className="btn-ghost btn-sm" title="새 코드로 갱신">
                <RefreshCw size={12} /> 새 코드
              </button>
              <button onClick={cancelPairing} className="btn-ghost btn-sm">
                연결 취소
              </button>
            </div>
          </div>
        )}
      </div>

      <div className="paired-devices-section">
        <div className="section-title-row">
          <div className="section-title-left">
            <ShieldCheck size={15} />
            <h4>연결을 허용한 기기</h4>
            <span className="count-pill">{devices.length}</span>
          </div>
          {devices.length > 0 && (
            <button
              onClick={revokeAll}
              className="btn-danger-outline btn-sm"
              disabled={revoking !== null}
            >
              {revoking === "all" ? "초기화 중…" : "모든 기기 연결 해제"}
            </button>
          )}
        </div>

        {devices.length > 0 ? (
          <div className="device-rows-container">
            {devices.map((device) => (
              <div key={device.device_id} className="device-row-item">
                <div className="device-row-main">
                  <span className="device-row-name" style={{ display: "inline-flex", alignItems: "center", gap: 7 }}>
                    <Smartphone size={15} strokeWidth={2} />
                    {device.name}
                  </span>
                  <span className="device-row-date">연결 허용: {formatPairedAt(device.paired_at)}</span>
                </div>
                <button
                  onClick={() => revoke(device.device_id)}
                  className="btn-danger-outline"
                  disabled={revoking === device.device_id}
                  title="이 기기 연결 해제"
                >
                  {revoking === device.device_id ? "제거 중…" : "연결 해제"}
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div className="empty-devices-box">
            <p>아직 연결을 허용한 기기가 없습니다.</p>
          </div>
        )}
      </div>
    </div>
  );
}
