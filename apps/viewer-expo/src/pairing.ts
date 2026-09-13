import Constants from "expo-constants";
import * as SecureStore from "expo-secure-store";
import { connect, ControlRequestError } from "./control";
import {
  getPinnedHostKey,
  rememberPinnedHostKey,
  restorePinnedHostKey,
} from "./pinned-host-keys";
import { DEFAULT_CONTROL_PORT } from "./defaults";
import { LocalizedError } from "./localized-error";
import { currentTranslation } from "./language-store";
import { RECENT_HOSTS_KEY } from "./recent-hosts";

/**
 * Host pairing: connect to the selected Host endpoint, submit the six-digit
 * code shown in the Host window, then keep the issued token in secure storage
 * for all later control-plane requests. QR pairing remains supported as an
 * optional path for older Host screens.
 */

export interface QrPayload {
  id: string;
  secret: string;
  /** 호스트 Ed25519 공개키(b64url 32B) — 핸드셰이크 핀의 원천. */
  hostKey: string;
  host: string;
  port: number;
}

export interface HostEndpoint {
  host: string;
  port: number;
}

const DEVICE_ID_KEY = "leftcar.deviceId";
/** 구버전 전역 토큰 키 — 모든 호스트가 하나의 토큰을 공유하던 v1. 폐기 대상. */
const LEGACY_TOKEN_KEY = "leftcar.token";

/**
 * 토큰은 발급한 호스트 엔드포인트별로 저장된다 — 한 호스트의 401·토큰 삭제가
 * 다른 페어링된 호스트를 잠그지 않는다. SecureStore 키는 영숫자와 `.`, `-`,
 * `_`만 허용하므로 그 외 문자(IPv6 등)는 치환한다. 포트는 항상 정수라 끝에
 * 붙이면 모호하지 않다.
 */
function tokenKey(host: string, port: number): string {
  const sanitizedHost = host.replace(/[^A-Za-z0-9._-]/g, "_");
  return `leftcar.token.v2.${sanitizedHost}.${port}`;
}

function identityTokenKey(hostKey: string): string {
  return `leftcar.token.v3.${hostKey}`;
}

export interface StoredCredential {
  token: string;
  target: HostEndpoint;
  /** Present only after the network handshake verified this identity. */
  hostKey: string | null;
}

export interface PairingAttempt {
  readonly signal: AbortSignal;
  throwIfCancelled(): void;
  retainRollback(rollback: () => Promise<void>): void;
  commit(): void;
  cancel(): Promise<void>;
}

/** One cancellation lifetime for pair persistence, connect, and navigation. */
export function createPairingAttempt(): PairingAttempt {
  const controller = new AbortController();
  const rollbacks: Array<() => Promise<void>> = [];
  let committed = false;
  return {
    signal: controller.signal,
    throwIfCancelled: () => throwIfAborted(controller.signal),
    retainRollback(rollback) {
      if (committed) return;
      rollbacks.push(rollback);
    },
    commit() {
      throwIfAborted(controller.signal);
      committed = true;
      rollbacks.length = 0;
    },
    async cancel() {
      controller.abort();
      if (committed) return;
      const pending = rollbacks.splice(0).reverse();
      await Promise.all(pending.map((rollback) => rollback()));
    },
  };
}

let credentialMutationQueue: Promise<void> = Promise.resolve();

function serializeCredentialMutation<T>(operation: () => Promise<T>): Promise<T> {
  const pending = credentialMutationQueue.then(operation, operation);
  credentialMutationQueue = pending.then(() => undefined, () => undefined);
  return pending;
}

async function restoreValue(key: string, value: string | null): Promise<void> {
  if (value === null) await SecureStore.deleteItemAsync(key);
  else await SecureStore.setItemAsync(key, value);
}

async function rollbackValues(entries: ReadonlyArray<readonly [string, string | null]>): Promise<void> {
  const results = await Promise.allSettled(entries.map(([key, value]) => restoreValue(key, value)));
  const failures = results
    .filter((result): result is PromiseRejectedResult => result.status === "rejected")
    .map((result) => result.reason);
  if (failures.length > 0) throw new AggregateError(failures, "credential rollback failed");
}

interface RawQrPayload {
  v?: unknown;
  id?: unknown;
  s?: unknown;
  k?: unknown;
  h?: unknown;
  p?: unknown;
}

const OFFER_ID_PATTERN =
  /^offer-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const OFFER_SECRET_PATTERN = /^[A-Za-z0-9_-]{43}$/;
/** 호스트 Ed25519 공개키(b64url 32B = 43문자). */
export const HOST_KEY_PATTERN = /^[A-Za-z0-9_-]{43}$/;

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

/**
 * `{"v":2,"id":..,"s":..,"k":..,"h":..,"p":..}` → QrPayload; null on any
 * mismatch. v2부터 `k`(호스트 공개키)가 필수다 — 핀 없는 연결은 중간자를
 * 감지할 수 없으므로 QR이 핀의 원천이다.
 */
export function parseQrPayload(text: string): QrPayload | null {
  if (!text || typeof text !== "string") return null;
  let raw: RawQrPayload;
  try {
    raw = JSON.parse(text.trim()) as RawQrPayload;
  } catch {
    return null;
  }
  if (
    raw.v !== 2 ||
    typeof raw.id !== "string" ||
    !OFFER_ID_PATTERN.test(raw.id) ||
    typeof raw.s !== "string" ||
    !OFFER_SECRET_PATTERN.test(raw.s) ||
    typeof raw.k !== "string" ||
    !HOST_KEY_PATTERN.test(raw.k) ||
    typeof raw.h !== "string" ||
    !isTrustedHost(raw.h) ||
    typeof raw.p !== "number" ||
    !Number.isInteger(raw.p) ||
    raw.p <= 0 ||
    raw.p >= 65536
  ) {
    return null;
  }
  return { id: raw.id, secret: raw.s, hostKey: raw.k, host: raw.h, port: raw.p };
}

/** Stable per-install device label shown in the host's paired-device list. */
export async function getDeviceId(): Promise<string> {
  const existing = await SecureStore.getItemAsync(DEVICE_ID_KEY);
  if (existing) return existing;
  const id = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  await SecureStore.setItemAsync(DEVICE_ID_KEY, id);
  return id;
}

/** 이 대상 호스트에 발급된 토큰을 읽는다(없으면 null). */
export async function getStoredToken(target: HostEndpoint): Promise<string | null> {
  return SecureStore.getItemAsync(tokenKey(target.host, target.port));
}

/**
 * Resolve the immutable credential incarnation for a newly verified socket.
 * The stable identity path is opt-in: callers may pass a host key only after
 * `clientFinish` has verified it. USB has no such proof and remains endpoint-bound.
 */
export async function getStoredCredential(
  target: HostEndpoint,
  verifiedHostKey: string | null = null,
  signal?: AbortSignal,
): Promise<StoredCredential | null> {
  return serializeCredentialMutation(async () => {
    throwIfAborted(signal);
    if (verifiedHostKey) {
      const identityToken = await SecureStore.getItemAsync(identityTokenKey(verifiedHostKey));
      throwIfAborted(signal);
      if (identityToken) {
        await migrateVerifiedCredential(target, verifiedHostKey, identityToken, signal);
        return { token: identityToken, target, hostKey: verifiedHostKey };
      }
    }
    const endpointToken = await getStoredToken(target);
    throwIfAborted(signal);
    if (!endpointToken) return null;
    if (verifiedHostKey) {
      await migrateVerifiedCredential(target, verifiedHostKey, endpointToken, signal);
    }
    return { token: endpointToken, target, hostKey: verifiedHostKey };
  });
}

/** 이 대상 호스트의 토큰만 치운다 — 다른 호스트의 토큰은 그대로 둔다. */
export async function clearToken(target: HostEndpoint): Promise<void> {
  await serializeCredentialMutation(async () => {
    await SecureStore.deleteItemAsync(tokenKey(target.host, target.port));
  });
}

/** Clear only keys that still contain the failed socket's token incarnation. */
export async function clearStoredCredential(credential: StoredCredential): Promise<void> {
  await serializeCredentialMutation(async () => {
    const endpointKey = tokenKey(credential.target.host, credential.target.port);
    const keys = credential.hostKey
      ? [endpointKey, identityTokenKey(credential.hostKey)]
      : [endpointKey];
    const before = await Promise.all(
      keys.map(async (key) => [key, await SecureStore.getItemAsync(key)] as const),
    );
    try {
      await Promise.all(
        before.map(([key, value]) =>
          value === credential.token ? SecureStore.deleteItemAsync(key) : Promise.resolve(),
        ),
      );
    } catch (error) {
      await rollbackValues(before);
      throw error;
    }
  });
}

async function migrateVerifiedCredential(
  target: HostEndpoint,
  hostKey: string,
  token: string,
  signal?: AbortSignal,
): Promise<void> {
  const identityKey = identityTokenKey(hostKey);
  const endpointKey = tokenKey(target.host, target.port);
  const [recentBefore, identityBefore, endpointBefore] = await Promise.all([
    SecureStore.getItemAsync(RECENT_HOSTS_KEY),
    SecureStore.getItemAsync(identityKey),
    SecureStore.getItemAsync(endpointKey),
  ]);
  const pinBefore = getPinnedHostKey(target.host, target.port);
  const aliasesAlreadyCurrent =
    identityBefore === token && endpointBefore === token && pinBefore === hostKey;
  throwIfAborted(signal);
  if (aliasesAlreadyCurrent) return;
  try {
    if (identityBefore !== token) await SecureStore.setItemAsync(identityKey, token);
    // Once this endpoint has cryptographically proved `hostKey`, its legacy
    // alias must follow the identity credential too. Leaving an older token
    // here would make a later USB connection reuse the endpoint's stale
    // incarnation because loopback has no identity handshake of its own.
    if (endpointBefore !== token) await SecureStore.setItemAsync(endpointKey, token);
    await rememberPinnedHostKey(target.host, target.port, hostKey, signal);
  } catch (error) {
    restorePinnedHostKey(target.host, target.port, pinBefore);
    await rollbackValues([
      [identityKey, identityBefore],
      [endpointKey, endpointBefore],
      [RECENT_HOSTS_KEY, recentBefore],
    ]);
    throw error;
  }
}

/** 발급 토큰을 페어링 대상 엔드포인트 아래에 저장하고 구버전 전역 키를 치운다. */
async function storeToken(
  host: string,
  port: number,
  token: string,
  hostKey: string | null,
  signal?: AbortSignal,
): Promise<() => Promise<void>> {
  return serializeCredentialMutation(async () => {
    const endpointKey = tokenKey(host, port);
    const identityKey = hostKey ? identityTokenKey(hostKey) : null;
    const keys = [endpointKey, ...(identityKey ? [identityKey] : []), LEGACY_TOKEN_KEY];
    const before = await Promise.all(
      keys.map(async (key) => [key, await SecureStore.getItemAsync(key)] as const),
    );
    const recentBefore = hostKey ? await SecureStore.getItemAsync(RECENT_HOSTS_KEY) : null;
    const pinBefore = hostKey ? getPinnedHostKey(host, port) : null;
    try {
      throwIfAborted(signal);
      await SecureStore.setItemAsync(endpointKey, token);
      throwIfAborted(signal);
      if (identityKey) await SecureStore.setItemAsync(identityKey, token);
      throwIfAborted(signal);
      await SecureStore.deleteItemAsync(LEGACY_TOKEN_KEY);
      throwIfAborted(signal);
      if (hostKey) await rememberPinnedHostKey(host, port, hostKey, signal);
      throwIfAborted(signal);
      const recentAfter = hostKey ? await SecureStore.getItemAsync(RECENT_HOSTS_KEY) : null;
      return async () => {
        await serializeCredentialMutation(async () => {
          const currentValues = await Promise.all(
            before.map(async ([key, previous]) => ({
              key,
              previous,
              current: await SecureStore.getItemAsync(key),
            })),
          );
          const rollback = currentValues.reduce<Array<readonly [string, string | null]>>(
            (entries, { key, previous, current }) => {
              if (current === (key === LEGACY_TOKEN_KEY ? null : token)) {
                entries.push([key, previous]);
              }
              return entries;
            },
            [],
          );
          if (
            hostKey &&
            (await SecureStore.getItemAsync(RECENT_HOSTS_KEY)) === recentAfter
          ) {
            rollback.push([RECENT_HOSTS_KEY, recentBefore]);
            restorePinnedHostKey(host, port, pinBefore);
          }
          await rollbackValues(rollback);
        });
      };
    } catch (error) {
      if (hostKey) restorePinnedHostKey(host, port, pinBefore);
      await rollbackValues([
        ...before,
        ...(hostKey ? [[RECENT_HOSTS_KEY, recentBefore] as const] : []),
      ]);
      throw error;
    }
  });
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
export async function pairWithHost(
  p: QrPayload,
  code: string,
  options: { attempt?: PairingAttempt; signal?: AbortSignal } = {},
): Promise<string> {
  return pairAndStore(
    p.host,
    p.port,
    code,
    (pairingCode, deviceId, deviceName) => ({
      offerId: p.id,
      secret: p.secret,
      code: pairingCode,
      deviceId,
      deviceName,
    }),
    {
      pinnedHostKey: p.hostKey,
      attempt: options.attempt,
      signal: options.signal,
    },
  );
}

/** Complete pairing directly against the selected Host endpoint. */
export async function pairWithHostByCode(
  host: string,
  port = DEFAULT_CONTROL_PORT,
  code: string,
  options: { attempt?: PairingAttempt; signal?: AbortSignal } = {},
): Promise<string> {
  // 수동 입력 경로는 핀이 없다 — TOFU로 첫 핸드셰이크의 키를 핀한다.
  return pairAndStore(
    host,
    port,
    code,
    (pairingCode, deviceId, deviceName) => ({
      code: pairingCode,
      deviceId,
      deviceName,
    }),
    { attempt: options.attempt, signal: options.signal },
  );
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
  options?: {
    pinnedHostKey?: string | null;
    attempt?: PairingAttempt;
    signal?: AbortSignal;
  },
): Promise<string> {
  if (!isTrustedHost(host)) {
    throw new LocalizedError("trustedHostError");
  }
  const pairingCode = code.trim().replace(/\s+/g, "");
  if (!PAIRING_CODE_PATTERN.test(pairingCode)) {
    throw new LocalizedError("errPairingCodeInvalid");
  }
  const signal = options?.attempt?.signal ?? options?.signal;
  throwIfAborted(signal);
  const client = await connect(host, port, 5000, undefined, {
    pinnedHostKey: options?.pinnedHostKey ?? null,
  });
  try {
    throwIfAborted(signal);
    const deviceId = await getDeviceId();
    throwIfAborted(signal);
    const { token } = await client.request<{ token: string }>(
      "pair",
      args(pairingCode, deviceId, deviceName()),
    );
    throwIfAborted(signal);
    if (!TOKEN_PATTERN.test(token)) {
      throw new LocalizedError("errPairingResponseInvalid");
    }
    const rollback = await storeToken(host, port, token, client.hostKey ?? null, signal);
    options?.attempt?.retainRollback(rollback);
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
    attempt?: PairingAttempt;
  } = {},
): Promise<PairingApprovalResult> {
  if (!isTrustedHost(p.host)) {
    throw new LocalizedError("trustedHostError");
  }
  const signal = options.attempt?.signal ?? options.signal;
  throwIfAborted(signal);
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
    throwIfAborted(signal);
    const client = await connect(p.host, p.port, 5000, undefined, {
      pinnedHostKey: p.hostKey,
    });
    try {
      throwIfAborted(signal);
      const response = await client.request<{ token?: string; status?: string }>(
        "pair",
        args,
      );
      // 요청 대기 중 취소됐다면 토큰을 검증·저장하지 않는다 — 취소 후 저장은
      // 사용자가 거부한 페어링을 뒤늦게 되살린다.
      throwIfAborted(signal);
      if (typeof response.token === "string") {
        if (!TOKEN_PATTERN.test(response.token)) {
          throw new LocalizedError("errPairingResponseInvalid");
        }
        const rollback = await storeToken(
          p.host,
          p.port,
          response.token,
          client.hostKey ?? null,
          signal,
        );
        options.attempt?.retainRollback(rollback);
        return { kind: "approved", token: response.token };
      }
      if (response.status === "pending") {
        options.onPending?.();
        await delay(pollMs, signal);
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
        await delay(pollMs, signal);
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
