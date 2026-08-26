import Constants from "expo-constants";
import * as SecureStore from "expo-secure-store";
import { connect } from "./control";

/**
 * Host pairing: connect to the selected Host endpoint, submit the six-digit
 * code shown in the Host window, then keep the issued token in secure storage
 * for all later control-plane requests. QR pairing remains supported as an
 * optional path for older Host screens.
 */

export interface QrPayload {
  id: string;
  secret: string;
  host: string;
  port: number;
}

export interface HostEndpoint {
  host: string;
  port: number;
}

const DEVICE_ID_KEY = "leftcar.deviceId";
const TOKEN_KEY = "leftcar.token";

interface RawQrPayload {
  v?: unknown;
  id?: unknown;
  s?: unknown;
  h?: unknown;
  p?: unknown;
}

const OFFER_ID_PATTERN =
  /^offer-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const OFFER_SECRET_PATTERN = /^[A-Za-z0-9_-]{43}$/;

/**
 * The current transport is protected by pairing but not encrypted by TLS.
 * Restrict it to loopback, private LAN, link-local, and Tailscale addresses.
 */
export function isTrustedHost(host: string): boolean {
  const normalized = host.trim().toLowerCase().replace(/\.$/, "");
  if (!normalized || normalized.length > 253) return false;
  if (normalized === "localhost" || normalized.endsWith(".local")) return true;
  if (normalized.endsWith(".ts.net")) return true;

  const octets = normalized.split(".");
  if (octets.length !== 4 || octets.some((part) => !/^\d{1,3}$/.test(part))) {
    return false;
  }
  const values = octets.map(Number);
  if (values.some((value) => value < 0 || value > 255)) return false;
  const [a, b] = values;
  return (
    a === 10 ||
    a === 127 ||
    (a === 169 && b === 254) ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 100 && b >= 64 && b <= 127)
  );
}

/** Parse an explicitly selected host endpoint without inventing a localhost fallback. */
export function parseHostEndpoint(endpoint: string): HostEndpoint | null {
  const trimmed = endpoint.trim();
  if (!trimmed) return null;

  const separator = trimmed.lastIndexOf(":");
  if (separator < 0) {
    return isTrustedHost(trimmed) ? { host: trimmed, port: 7777 } : null;
  }

  const host = trimmed.slice(0, separator).trim();
  const rawPort = trimmed.slice(separator + 1).trim();
  const port = Number(rawPort);
  if (
    !isTrustedHost(host) ||
    !/^\d+$/.test(rawPort) ||
    !Number.isInteger(port) ||
    port < 1 ||
    port > 65535
  ) {
    return null;
  }
  return { host, port };
}

export function formatHostEndpoint(host: string, port: number): string {
  return `${host}:${port}`;
}

/** Prefer the endpoint explicitly selected for this pairing attempt. */
export function resolvePairingHost(routeEndpoint: string | undefined, currentHost: string): string {
  return routeEndpoint?.trim() || currentHost;
}

/**
 * Keep this predicate shared with the UI so the enabled state cannot drift
 * from the direct Host endpoint + six-digit pairing contract.
 */
export function canSubmitPairingCode(
  code: string,
  hasHostTarget: boolean,
  busy: boolean,
): boolean {
  const normalized = code.trim().replace(/\s+/g, "");
  return hasHostTarget && !busy && /^\d{6}$/.test(normalized);
}

/** `{"v":1,"id":..,"s":..,"h":..,"p":..}` → QrPayload; null on any mismatch. */
export function parseQrPayload(text: string): QrPayload | null {
  if (!text || typeof text !== "string") return null;
  let raw: RawQrPayload;
  try {
    raw = JSON.parse(text.trim()) as RawQrPayload;
  } catch {
    return null;
  }
  if (
    raw.v !== 1 ||
    typeof raw.id !== "string" ||
    !OFFER_ID_PATTERN.test(raw.id) ||
    typeof raw.s !== "string" ||
    !OFFER_SECRET_PATTERN.test(raw.s) ||
    typeof raw.h !== "string" ||
    !isTrustedHost(raw.h) ||
    typeof raw.p !== "number" ||
    !Number.isInteger(raw.p) ||
    raw.p <= 0 ||
    raw.p >= 65536
  ) {
    return null;
  }
  return { id: raw.id, secret: raw.s, host: raw.h, port: raw.p };
}

/** Stable per-install device label shown in the host's paired-device list. */
export async function getDeviceId(): Promise<string> {
  const existing = await SecureStore.getItemAsync(DEVICE_ID_KEY);
  if (existing) return existing;
  const id = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  await SecureStore.setItemAsync(DEVICE_ID_KEY, id);
  return id;
}

export async function getStoredToken(): Promise<string | null> {
  return SecureStore.getItemAsync(TOKEN_KEY);
}

export async function clearToken(): Promise<void> {
  await SecureStore.deleteItemAsync(TOKEN_KEY);
}

/** Human-readable label sent with the pair request (host UI display only). */
export function deviceName(): string {
  return Constants.deviceName || "Android 뷰어";
}

/**
 * Complete pairing against the QR offer host. On success the issued token is
 * persisted; on any failure nothing is kept (a stale token is dropped too).
 */
export async function pairWithHost(p: QrPayload, code: string): Promise<string> {
  if (!isTrustedHost(p.host)) {
    throw new Error("같은 Wi-Fi 또는 Tailscale에 있는 컴퓨터만 연결할 수 있습니다");
  }
  const pairingCode = code.trim().replace(/\s+/g, "");
  if (!/^\d{6}$/.test(pairingCode)) {
    throw new Error("6자리 인증 코드를 정확히 입력해 주세요");
  }
  const client = await connect(p.host, p.port);
  try {
    const { token } = await client.request<{ token: string }>("pair", {
      offerId: p.id,
      secret: p.secret,
      code: pairingCode,
      deviceId: await getDeviceId(),
      deviceName: deviceName(),
    });
    if (!/^[0-9a-f]{64}$/.test(token)) {
      throw new Error("컴퓨터의 연결 승인 응답을 확인할 수 없습니다");
    }
    await SecureStore.setItemAsync(TOKEN_KEY, token);
    return token;
  } catch (e) {
    await clearToken();
    throw e;
  } finally {
    // The pairing connection is single-purpose; the token travels via secure
    // storage into the main control session, so always release the socket.
    client.close();
  }
}

/** Complete pairing directly against the selected Host endpoint. */
export async function pairWithHostByCode(
  host: string,
  port = 7777,
  code: string,
): Promise<string> {
  if (!isTrustedHost(host)) {
    throw new Error("같은 Wi-Fi 또는 Tailscale에 있는 컴퓨터만 연결할 수 있습니다");
  }
  const pairingCode = code.trim().replace(/\s+/g, "");
  if (!/^\d{6}$/.test(pairingCode)) {
    throw new Error("6자리 인증 코드를 정확히 입력해 주세요");
  }
  const client = await connect(host, port);
  try {
    const { token } = await client.request<{ token: string }>("pair", {
      code: pairingCode,
      deviceId: await getDeviceId(),
      deviceName: deviceName(),
    });
    if (!/^[0-9a-f]{64}$/.test(token)) {
      throw new Error("컴퓨터의 연결 승인 응답을 확인할 수 없습니다");
    }
    await SecureStore.setItemAsync(TOKEN_KEY, token);
    return token;
  } catch (e) {
    await clearToken();
    throw e;
  } finally {
    client.close();
  }
}
