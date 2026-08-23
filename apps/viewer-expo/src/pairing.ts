import Constants from "expo-constants";
import * as SecureStore from "expo-secure-store";
import { connect } from "./control";

/**
 * Host pairing (design §페어링): scan the host's QR offer, confirm the
 * 6-digit human verification code, then keep the issued token in the
 * device secure storage for all later control-plane requests.
 */

export interface QrPayload {
  id: string;
  secret: string;
  host: string;
  port: number;
  code?: string;
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
  c?: unknown;
}

/** Parse an explicitly selected host endpoint without inventing a localhost fallback. */
export function parseHostEndpoint(endpoint: string): HostEndpoint | null {
  const trimmed = endpoint.trim();
  if (!trimmed) return null;

  const separator = trimmed.lastIndexOf(":");
  if (separator < 0) return { host: trimmed, port: 7777 };

  const host = trimmed.slice(0, separator).trim();
  const rawPort = trimmed.slice(separator + 1).trim();
  const port = Number(rawPort);
  if (
    !host ||
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

/** `{"v":1,"id":..,"s":..,"h":..,"p":..,"c":..}` → QrPayload; null on any mismatch. */
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
    !raw.id ||
    typeof raw.s !== "string" ||
    !raw.s ||
    typeof raw.h !== "string" ||
    !raw.h ||
    typeof raw.p !== "number" ||
    !Number.isInteger(raw.p) ||
    raw.p <= 0 ||
    raw.p >= 65536
  ) {
    return null;
  }
  const code =
    typeof raw.c === "string" && /^\d{6}$/.test(raw.c.trim())
      ? raw.c.trim()
      : undefined;
  return { id: raw.id, secret: raw.s, host: raw.h, port: raw.p, ...(code ? { code } : {}) };
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
export async function pairWithHost(p: QrPayload, code?: string): Promise<string> {
  const pairingCode = code || p.code;
  if (!pairingCode) {
    throw new Error("6자리 인증 코드가 필요합니다");
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
    if (!token) throw new Error("페어링 응답에 토큰이 없습니다");
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

/**
 * Complete pairing by connecting directly to the host IP and entering the 6-digit code.
 */
export async function pairWithHostByCode(
  host: string,
  port = 7777,
  code: string,
): Promise<string> {
  const trimmed = code.trim().replace(/\s+/g, "");
  if (trimmed.length !== 6) {
    throw new Error("6자리 인증 코드를 정확히 입력해 주세요");
  }
  const client = await connect(host, port);
  try {
    const { token } = await client.request<{ token: string }>("pair", {
      code: trimmed,
      deviceId: await getDeviceId(),
      deviceName: deviceName(),
    });
    if (!token) throw new Error("페어링 응답에 토큰이 없습니다");
    await SecureStore.setItemAsync(TOKEN_KEY, token);
    return token;
  } catch (e) {
    await clearToken();
    throw e;
  } finally {
    client.close();
  }
}
