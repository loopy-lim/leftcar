import Constants from "expo-constants";
import * as SecureStore from "expo-secure-store";
import { connect, ControlRequestError } from "./control";
import { DEFAULT_CONTROL_PORT } from "./defaults";
import { LocalizedError } from "./localized-error";
import { currentTranslation } from "./language-store";

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
    return isTrustedHost(trimmed) ? { host: trimmed, port: DEFAULT_CONTROL_PORT } : null;
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
  return Constants.deviceName || currentTranslation().viewer.deviceLabel;
}

/** Pairing requests carry a six-digit human verification code. */
const PAIRING_CODE_PATTERN = /^\d{6}$/;
/** Issued tokens are 64 hex characters. */
const TOKEN_PATTERN = /^[0-9a-f]{64}$/;

/**
 * Complete pairing against the QR offer host. On success the issued token is
 * persisted; on failure any previously stored token is left untouched (it
 * belongs to its issuing host and self-heals via 401 on connect).
 */
export async function pairWithHost(p: QrPayload, code: string): Promise<string> {
  return pairAndStore(p.host, p.port, code, (pairingCode, deviceId, deviceName) => ({
    offerId: p.id,
    secret: p.secret,
    code: pairingCode,
    deviceId,
    deviceName,
  }));
}

/** Complete pairing directly against the selected Host endpoint. */
export async function pairWithHostByCode(
  host: string,
  port = DEFAULT_CONTROL_PORT,
  code: string,
): Promise<string> {
  return pairAndStore(host, port, code, (pairingCode, deviceId, deviceName) => ({
    code: pairingCode,
    deviceId,
    deviceName,
  }));
}

/**
 * Shared pairing tail: connect, request, validate, persist, release. A
 * failure must NOT clear the stored token: the token belongs to whichever
 * host issued it, and a failed attempt against another host (typo'd code,
 * old host without approval support) must not lock the user out of an
 * already-paired machine. Stale tokens self-heal on connect via 401.
 */
async function pairAndStore(
  host: string,
  port: number,
  code: string,
  args: (
    pairingCode: string,
    deviceId: string,
    deviceName: string,
  ) => Record<string, unknown>,
): Promise<string> {
  if (!isTrustedHost(host)) {
    throw new LocalizedError("trustedHostError");
  }
  const pairingCode = code.trim().replace(/\s+/g, "");
  if (!PAIRING_CODE_PATTERN.test(pairingCode)) {
    throw new LocalizedError("errPairingCodeInvalid");
  }
  const client = await connect(host, port);
  try {
    const { token } = await client.request<{ token: string }>(
      "pair",
      args(pairingCode, await getDeviceId(), deviceName()),
    );
    if (!TOKEN_PATTERN.test(token)) {
      throw new LocalizedError("errPairingResponseInvalid");
    }
    await SecureStore.setItemAsync(TOKEN_KEY, token);
    return token;
  } finally {
    // The pairing connection is single-purpose; the token travels via secure
    // storage into the main control session, so always release the socket.
    client.close();
  }
}

// -- 승인 기반 QR 페어링 ------------------------------------------------------
//
// QR 시크릿만 제시하고 Mac 사용자가 [허용]을 누를 때까지 폴링한다. Host는
// 시크릿이 맞으면 {status:"pending"}으로 답하고, 승인 후 같은 시크릿으로
// 폴링하면 토큰을 돌려준다. 거절은 "pairing rejected" 오류로 온다.

export type PairingApprovalResult =
  | { kind: "approved"; token: string }
  | { kind: "rejected" };

export const PAIRING_APPROVAL_TIMEOUT_MS = 150_000;

/** 구버전 호스트는 코드 없는 pair을 "pairing failed"로 거절한다 — PIN 폴백 신호. */
export function isPairingUnsupportedError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return /pairing failed/i.test(message);
}

export function isPairingRejectedError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return /pairing rejected/i.test(message);
}

/**
 * QR 스캔 한 번으로 페어링을 완결하는 승인 폴링. pending 동안 2.5초 간격으로
 * 같은 요청을 재전송한다(멱등 — 호스트는 매번 시크릿을 상수 시간 비교로
 * 검증하고, 대기 폴링은 offer를 소각하지 않는다).
 */
export async function pairWithHostApproval(
  p: QrPayload,
  options: {
    pollMs?: number;
    timeoutMs?: number;
    onPending?: () => void;
    /** Cancels the poll loop (screen unmounted, scan superseded). */
    signal?: AbortSignal;
  } = {},
): Promise<PairingApprovalResult> {
  if (!isTrustedHost(p.host)) {
    throw new LocalizedError("trustedHostError");
  }
  throwIfAborted(options.signal);
  const pollMs = options.pollMs ?? 2_500;
  const deadline = Date.now() + (options.timeoutMs ?? PAIRING_APPROVAL_TIMEOUT_MS);
  const args = {
    offerId: p.id,
    secret: p.secret,
    code: "",
    deviceId: await getDeviceId(),
    deviceName: deviceName(),
  };

  while (Date.now() < deadline) {
    throwIfAborted(options.signal);
    const client = await connect(p.host, p.port);
    try {
      const response = await client.request<{ token?: string; status?: string }>(
        "pair",
        args,
      );
      if (typeof response.token === "string") {
        if (!TOKEN_PATTERN.test(response.token)) {
          throw new LocalizedError("errPairingResponseInvalid");
        }
        await SecureStore.setItemAsync(TOKEN_KEY, response.token);
        return { kind: "approved", token: response.token };
      }
      if (response.status === "pending") {
        options.onPending?.();
        await delay(pollMs, options.signal);
        continue;
      }
      throw new LocalizedError("errPairingResponseInvalid");
    } catch (e) {
      if (isPairingRejectedError(e)) {
        return { kind: "rejected" };
      }
      // 개별 요청 타임아웃은 흐름을 죽이지 않는다 — 호스트가 한 순간
      // 바빠도 다음 폴링이 상태를 회수한다.
      if (e instanceof ControlRequestError && e.kind === "timeout") {
        await delay(pollMs, options.signal);
        continue;
      }
      throw e;
    } finally {
      client.close();
    }
  }
  throw new LocalizedError("errPairingApprovalTimeout");
}

function abortError(): Error {
  const error = new Error("pairing approval polling aborted");
  error.name = "AbortError";
  return error;
}

function throwIfAborted(signal?: AbortSignal): void {
  if (signal?.aborted) throw abortError();
}

function delay(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(abortError());
      return;
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    const onAbort = () => {
      clearTimeout(timer);
      reject(abortError());
    };
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}
